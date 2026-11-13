//! Raw HTTP/2 peers prove per-stream reset and strict gRPC framing independently
//! of tonic's serializer and status handling. All listeners are numeric loopback.
use bytes::Bytes;
use futures_util::{future::poll_fn, stream};
use rambler_transport::*;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::Notify,
    task::{JoinHandle, JoinSet},
};
use tokio_util::sync::CancellationToken;
use tonic::Code;

type Request = http::Request<h2::RecvStream>;
type Reply = h2::server::SendResponse<Bytes>;
type Handler =
    Arc<dyn Fn(Request, Reply) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;
struct Peer {
    address: std::net::SocketAddr,
    stop: CancellationToken,
    task: Option<JoinHandle<()>>,
    connections: Arc<AtomicUsize>,
}
impl Peer {
    async fn new<F, Fut>(handler: F) -> Self
    where
        F: Fn(Request, Reply) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        let connections = Arc::new(AtomicUsize::new(0));
        let counter = connections.clone();
        let handler: Handler = Arc::new(move |r, s| Box::pin(handler(r, s)));
        let task = tokio::spawn(async move {
            let mut connections_tasks = JoinSet::new();
            loop {
                tokio::select! {
                    biased;
                    _ = stopped.cancelled() => break,
                    result = connections_tasks.join_next(), if !connections_tasks.is_empty() => { result.unwrap().unwrap(); },
                    accepted = listener.accept() => {
                        let (tcp, _) = accepted.unwrap();
                        counter.fetch_add(1, Ordering::SeqCst);
                        let handler = handler.clone();
                        let stopped = stopped.clone();
                        connections_tasks.spawn(async move {
                            let mut connection = h2::server::Builder::new().initial_window_size(1024).handshake(tcp).await.unwrap();
                            let mut calls = JoinSet::new();
                            loop {
                                tokio::select! {
                                    biased;
                                    _ = stopped.cancelled() => break,
                                    result = calls.join_next(), if !calls.is_empty() => { result.unwrap().unwrap(); },
                                    request = connection.accept() => match request {
                                        Some(Ok((request, reply))) => { calls.spawn(handler(request, reply)); },
                                        Some(Err(_)) | None => break,
                                    },
                                }
                            }
                            calls.abort_all();
                            while let Some(result) = calls.join_next().await { if !result.as_ref().is_err_and(|e| e.is_cancelled()) { result.unwrap(); } }
                        });
                    }
                }
            }
            connections_tasks.abort_all();
            while let Some(result) = connections_tasks.join_next().await {
                if !result.as_ref().is_err_and(|e| e.is_cancelled()) {
                    result.unwrap();
                }
            }
        });
        Self {
            address,
            stop,
            task: Some(task),
            connections,
        }
    }
    async fn transport(&self) -> GrpcTransport {
        GrpcTransport::connect(TransportConfig::loopback(self.address).unwrap())
            .await
            .unwrap()
    }
    async fn close(mut self) {
        self.stop.cancel();
        self.task.as_mut().unwrap().await.unwrap();
        self.task.take();
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
fn headers() -> http::Response<()> {
    http::Response::builder()
        .header("content-type", "application/grpc")
        .body(())
        .unwrap()
}
fn trailers(value: &str) -> http::HeaderMap {
    let mut headers = http::HeaderMap::new();
    headers.insert("grpc-status", value.parse().unwrap());
    headers
}
fn frame(bytes: &[u8]) -> Bytes {
    let mut out = vec![0];
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
    Bytes::from(out)
}
fn source(bytes: &'static [u8]) -> impl RequestSource {
    StreamSource::new(stream::iter([Ok(Bytes::from_static(bytes))]))
}
async fn bounded<F: Future>(f: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(3), f)
        .await
        .expect("bounded test")
}
async fn collect(session: &mut Session) -> Result<Vec<Bytes>, TransportError> {
    let mut result = Vec::new();
    while let Some(bytes) = session.next().await? {
        result.push(bytes);
    }
    Ok(result)
}
async fn drain(mut request: Request) -> Bytes {
    let mut bytes = Vec::new();
    while let Some(chunk) = request.body_mut().data().await {
        let chunk = chunk.unwrap();
        bytes.extend_from_slice(&chunk);
        request
            .body_mut()
            .flow_control()
            .release_capacity(chunk.len())
            .unwrap();
    }
    Bytes::from(bytes)
}

struct Endless {
    closes: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
}
impl RequestSource for Endless {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async { Ok(Some(Bytes::from(vec![0; 256 * 1024]))) })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.closes.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}
impl Drop for Endless {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

async fn prove_reset(action: u8) {
    let a_started = Arc::new(Notify::new());
    let a_reset = Arc::new(Notify::new());
    let no_resume = Arc::new(Notify::new());
    let started = a_started.clone();
    let reset = a_reset.clone();
    let resumed = no_resume.clone();
    let peer = Peer::new(move |mut request, mut reply| {
        let started = started.clone();
        let reset = reset.clone();
        let resumed = resumed.clone();
        async move {
            if request.headers().get("x-session").is_some_and(|v| v == "a") {
                let mut response = reply.send_response(headers(), false).unwrap();
                started.notify_one();
                // Never advance upload's receive window until cancellation.
                assert_eq!(
                    poll_fn(|cx| response.poll_reset(cx)).await.unwrap(),
                    h2::Reason::CANCEL
                );
                reset.notify_one();
                // Already received DATA can remain buffered after RST. Reopen
                // flow control while draining it: upload must never resume.
                let mut buffered = 0;
                let mut saw_cancel = false;
                while let Some(data) = request.body_mut().data().await {
                    match data {
                        Ok(bytes) => {
                            buffered += bytes.len();
                            let _ = request
                                .body_mut()
                                .flow_control()
                                .release_capacity(bytes.len());
                        }
                        Err(error) => {
                            assert_eq!(error.reason(), Some(h2::Reason::CANCEL));
                            saw_cancel = true;
                            break;
                        }
                    }
                }
                assert!(saw_cancel);
                // Before peer SETTINGS is processed, HTTP/2 permits the default
                // 65535-byte initial window, even though we advertise 1024.
                assert!(
                    buffered <= 65535,
                    "only pre-reset receive-window bytes may remain: {buffered}"
                );
                resumed.notify_one();
            } else {
                let bytes = drain(request).await;
                let mut response = reply.send_response(headers(), false).unwrap();
                response.send_data(bytes, false).unwrap();
                response.send_trailers(trailers("0")).unwrap();
            }
        }
    })
    .await;
    let transport = peer.transport().await;
    let closes = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let metadata =
        Metadata::new([("x-session".into(), CredentialValue::Text("a".into()))]).unwrap();
    let timeout = if action == 2 {
        Duration::from_millis(100)
    } else {
        Duration::from_secs(2)
    };
    let mut a = transport
        .start(
            Endless {
                closes: closes.clone(),
                dropped: dropped.clone(),
            },
            metadata,
            timeout,
        )
        .unwrap();
    bounded(a_started.notified()).await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(!a.completion().source_exhausted);
    let mut b = transport
        .start(
            source(b"sister-before-cancel"),
            Metadata::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    match action {
        0 => assert!(matches!(
            bounded(a.cancel()).await,
            Err(TransportError::Cancelled)
        )),
        1 => drop(a),
        2 => assert!(matches!(
            bounded(a.finish()).await,
            Err(TransportError::Timeout)
        )),
        _ => unreachable!(),
    }
    bounded(a_reset.notified()).await;
    bounded(no_resume.notified()).await;
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(
        bounded(collect(&mut b)).await.unwrap(),
        vec![Bytes::from_static(b"sister-before-cancel")]
    );
    let mut c = transport
        .start(
            source(b"sister-after-cancel"),
            Metadata::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(
        bounded(collect(&mut c)).await.unwrap(),
        vec![Bytes::from_static(b"sister-after-cancel")]
    );
    assert_eq!(peer.connections.load(Ordering::SeqCst), 1);
    bounded(transport.close()).await.unwrap();
    bounded(peer.close()).await;
}

async fn native_tonic_code(address: std::net::SocketAddr) -> Code {
    let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut client = tonic::client::Grpc::new(channel);
    client.ready().await.unwrap();
    let result = client
        .streaming(
            tonic::Request::new(stream::iter([Bytes::from_static(b"native")])),
            http::uri::PathAndQuery::from_static(DEFAULT_METHOD),
            RawBytesCodec,
        )
        .await;
    match result {
        Err(status) => status.code(),
        Ok(response) => {
            let mut stream = response.into_inner();
            loop {
                match stream.message().await {
                    Ok(Some(_)) => {}
                    Ok(None) => return Code::Ok,
                    Err(status) => return status.code(),
                }
            }
        }
    }
}

#[tokio::test]
async fn http_fallback_codes_match_native_tonic_without_private_error_details() {
    for (status, expected) in [
        (200, Code::Unknown),
        (201, Code::Unknown),
        (204, Code::Unknown),
        (301, Code::Unknown),
        (400, Code::Internal),
        (401, Code::Unauthenticated),
        (403, Code::PermissionDenied),
        (404, Code::Unimplemented),
        (418, Code::Unknown),
        (429, Code::Unavailable),
        (500, Code::Unknown),
        (502, Code::Unavailable),
        (503, Code::Unavailable),
        (504, Code::Unavailable),
    ] {
        let peer = Peer::new(move |request, mut reply| async move {
            drain(request).await;
            let mut head = headers();
            *head.status_mut() = http::StatusCode::from_u16(status).unwrap();
            head.headers_mut()
                .insert("private-header", "secret-server-data".parse().unwrap());
            reply.send_response(head, true).unwrap();
        })
        .await;
        let transport = peer.transport().await;
        let mut session = transport
            .start(
                source(b"request"),
                Metadata::default(),
                Duration::from_secs(2),
            )
            .unwrap();
        let error = bounded(session.finish()).await.unwrap_err();
        assert!(
            matches!(error, TransportError::Grpc(code) if code == expected),
            "HTTP {status}: {error:?}"
        );
        assert!(!format!("{error:?} {error}").contains("secret"));
        assert_eq!(
            bounded(native_tonic_code(peer.address)).await,
            expected,
            "native HTTP {status}"
        );
        transport.close().await.unwrap();
        peer.close().await;
    }
}

#[tokio::test]
async fn explicit_grpc_header_or_trailer_status_overrides_http_fallback_like_native() {
    for http_status in [403, 503] {
        for grpc_code in [Code::Ok, Code::PermissionDenied, Code::DataLoss] {
            for in_trailers in [false, true] {
                let peer = Peer::new(move |request, mut reply| async move {
                    drain(request).await;
                    let mut head = headers();
                    *head.status_mut() = http::StatusCode::from_u16(http_status).unwrap();
                    if in_trailers {
                        let mut response = reply.send_response(head, false).unwrap();
                        response
                            .send_trailers(trailers(&(grpc_code as i32).to_string()))
                            .unwrap();
                    } else {
                        head.headers_mut().insert(
                            "grpc-status",
                            (grpc_code as i32).to_string().parse().unwrap(),
                        );
                        head.headers_mut()
                            .insert("grpc-message", "secret-server-details".parse().unwrap());
                        reply.send_response(head, true).unwrap();
                    }
                })
                .await;
                let transport = peer.transport().await;
                let mut session = transport
                    .start(
                        source(b"request"),
                        Metadata::default(),
                        Duration::from_secs(2),
                    )
                    .unwrap();
                match bounded(session.finish()).await {
                    Ok(done) => assert_eq!(done.terminal_code, Some(Code::Ok)),
                    Err(error) => {
                        assert!(matches!(error, TransportError::Grpc(code) if code == grpc_code));
                        assert_ne!(grpc_code, Code::Ok);
                        assert!(!format!("{error:?} {error}").contains("secret"));
                    }
                }
                assert_eq!(bounded(native_tonic_code(peer.address)).await, grpc_code);
                transport.close().await.unwrap();
                peer.close().await;
            }
        }
    }
}

#[tokio::test]
async fn remote_reset_reasons_match_native_codes_and_refused_stream_is_not_replayed() {
    use h2::Reason;
    for (reason, expected) in [
        (Reason::NO_ERROR, Code::Internal),
        (Reason::PROTOCOL_ERROR, Code::Internal),
        (Reason::INTERNAL_ERROR, Code::Internal),
        (Reason::FLOW_CONTROL_ERROR, Code::Internal),
        (Reason::SETTINGS_TIMEOUT, Code::Internal),
        (Reason::FRAME_SIZE_ERROR, Code::Internal),
        (Reason::COMPRESSION_ERROR, Code::Internal),
        (Reason::CONNECT_ERROR, Code::Internal),
        (Reason::REFUSED_STREAM, Code::Unavailable),
        (Reason::CANCEL, Code::Cancelled),
        (Reason::ENHANCE_YOUR_CALM, Code::ResourceExhausted),
        (Reason::INADEQUATE_SECURITY, Code::PermissionDenied),
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let peer = Peer::new(move |request, mut reply| {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                drain(request).await;
                reply.send_reset(reason);
            }
        })
        .await;
        let transport = peer.transport().await;
        let mut session = transport
            .start(
                source(b"never-replay"),
                Metadata::default(),
                Duration::from_secs(2),
            )
            .unwrap();
        let error = bounded(session.finish()).await.unwrap_err();
        assert!(
            matches!(error, TransportError::Grpc(code) if code == expected),
            "{reason:?}: {error:?}"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "no automatic request replay"
        );
        assert_eq!(
            bounded(native_tonic_code(peer.address)).await,
            expected,
            "native {reason:?}"
        );
        transport.close().await.unwrap();
        peer.close().await;
    }
}

#[tokio::test]
async fn cancelling_blocked_upload_resets_only_that_http2_stream_without_channel_close() {
    prove_reset(0).await;
}
#[tokio::test]
async fn dropping_blocked_session_resets_only_that_http2_stream_without_channel_close() {
    prove_reset(1).await;
}
#[tokio::test]
async fn deadline_resets_blocked_upload_without_closing_sibling_streams() {
    prove_reset(2).await;
}

#[tokio::test]
async fn fragmented_and_coalesced_envelopes_preserve_all_payloads_and_empty_messages() {
    let peer = Peer::new(|request, mut reply| async move {
        let input = drain(request).await;
        assert_eq!(input, frame(b"\x00request\xff"));
        let mut response = reply.send_response(headers(), false).unwrap();
        let all = [frame(b"\x00response\xff"), frame(b""), frame(b"last")].concat();
        // Split the five-byte header and payload, then coalesce two messages.
        for bytes in [&all[..1], &all[1..3], &all[3..8], &all[8..]] {
            response
                .send_data(Bytes::copy_from_slice(bytes), false)
                .unwrap();
        }
        response.send_trailers(trailers("0")).unwrap();
    })
    .await;
    let transport = peer.transport().await;
    let mut session = transport
        .start(
            source(b"\x00request\xff"),
            Metadata::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(
        bounded(collect(&mut session)).await.unwrap(),
        vec![
            Bytes::from_static(b"\x00response\xff"),
            Bytes::new(),
            Bytes::from_static(b"last")
        ]
    );
    assert!(session.finish().await.unwrap().upload_eof_polled);
    transport.close().await.unwrap();
    peer.close().await;
}

#[tokio::test]
async fn strict_status_http_compression_and_truncated_frames_never_succeed() {
    for case in 1..18 {
        let peer = Peer::new(move |request, mut reply| async move {
            drain(request).await;
            let mut head = headers();
            if case == 1 {
                head.headers_mut().remove("content-type");
            }
            if case == 2 {
                head.headers_mut()
                    .insert("content-type", "text/plain".parse().unwrap());
            }
            if case == 3 {
                head.headers_mut()
                    .insert("grpc-encoding", "gzip".parse().unwrap());
            }
            if case == 13 || case == 15 {
                head.headers_mut()
                    .insert("grpc-status", "0".parse().unwrap());
            }
            if case == 16 {
                head.headers_mut()
                    .insert("content-encoding", "gzip".parse().unwrap());
            }
            if case == 17 {
                head.headers_mut()
                    .insert("grpc-status", "secret-invalid-status".parse().unwrap());
            }
            let mut response = reply.send_response(head, false).unwrap();
            if case == 14 {
                response.send_data(Bytes::new(), true).unwrap();
                return;
            }
            match case {
                4 => response
                    .send_data(Bytes::from_static(b"\x01\0\0\0\0"), false)
                    .unwrap(),
                5 => response
                    .send_data(Bytes::from_static(b"\x02\0\0\0\0"), false)
                    .unwrap(),
                6 => response
                    .send_data(Bytes::from_static(b"\0\0\0"), false)
                    .unwrap(),
                7 => response
                    .send_data(Bytes::from_static(b"\0\0\0\0\x05xx"), false)
                    .unwrap(),
                8 => response
                    .send_data(Bytes::from_static(b"\0\x7f\xff\xff\xff"), false)
                    .unwrap(),
                13 => response
                    .send_data(frame(b"data-after-trailers-only"), false)
                    .unwrap(),
                _ => {}
            }
            let mut tail = match case {
                9 => http::HeaderMap::new(),
                10 => trailers("17"),
                11 => trailers("no"),
                _ => trailers("0"),
            };
            if case == 12 {
                tail.append("grpc-status", "1".parse().unwrap());
            }
            // Client may already have reset on invalid headers/data.
            let _ = response.send_trailers(tail);
        })
        .await;
        let transport = peer.transport().await;
        let mut session = transport
            .start(
                source(b"request"),
                Metadata::default(),
                Duration::from_secs(2),
            )
            .unwrap();
        let error = bounded(session.finish()).await.unwrap_err();
        assert!(
            matches!(
                error,
                TransportError::Protocol
                    | TransportError::CompressionUnsupported
                    | TransportError::Grpc(Code::ResourceExhausted | Code::Unknown)
            ),
            "case {case}: {error:?}"
        );
        assert!(!format!("{error:?} {error}").contains("data-after"));
        let _ = transport.close().await;
        peer.close().await;
    }
}

#[tokio::test]
async fn trailers_only_and_late_non_ok_status_are_authoritative() {
    for case in 0..3 {
        let peer = Peer::new(move |request, mut reply| async move {
            drain(request).await;
            let mut head = headers();
            if case < 2 {
                head.headers_mut().insert(
                    "grpc-status",
                    if case == 0 { "0" } else { "7" }.parse().unwrap(),
                );
                head.headers_mut()
                    .insert("grpc-message", "secret-details".parse().unwrap());
                reply.send_response(head, true).unwrap();
            } else {
                let mut response = reply.send_response(head, false).unwrap();
                response.send_data(frame(b"partial"), false).unwrap();
                response.send_trailers(trailers("15")).unwrap();
            }
        })
        .await;
        let transport = peer.transport().await;
        let mut session = transport
            .start(
                source(b"request"),
                Metadata::default(),
                Duration::from_secs(2),
            )
            .unwrap();
        if case == 0 {
            assert!(bounded(session.finish()).await.is_ok());
        } else {
            if case == 2 {
                assert_eq!(
                    bounded(session.next()).await.unwrap(),
                    Some(Bytes::from_static(b"partial"))
                );
            }
            let error = bounded(session.finish()).await.unwrap_err();
            assert!(
                matches!(error, TransportError::Grpc(code) if code == if case == 1 { Code::PermissionDenied } else { Code::DataLoss })
            );
            assert!(!format!("{error:?} {error}").contains("secret"));
        }
        let _ = transport.close().await;
        peer.close().await;
    }
}

// The oracle is the original message vector, not another gRPC decoder.
fn generated_messages(mut seed: u64) -> Vec<Bytes> {
    (0..24)
        .map(|index| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let len = if index % 5 == 0 {
                0
            } else if index == 1 {
                32
            } else {
                (seed >> 32) as usize % 33
            };
            Bytes::from(
                (0..len)
                    .map(|i| (seed >> (i % 8 * 8)) as u8)
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

#[tokio::test]
async fn generated_envelopes_are_partition_invariant_at_exact_configured_limit() {
    let peer = Peer::new(|request, mut reply| async move {
        let seed: u64 = request.headers()["x-seed"]
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        drain(request).await;
        let all: Vec<u8> = generated_messages(seed)
            .iter()
            .flat_map(|bytes| frame(bytes))
            .collect();
        let mut response = reply.send_response(headers(), false).unwrap();
        let mut offset = 0;
        let mut state = seed;
        while offset < all.len() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let end = (offset + 1 + (state >> 32) as usize % 29).min(all.len());
            response
                .send_data(Bytes::copy_from_slice(&all[offset..end]), false)
                .unwrap();
            offset = end;
        }
        response.send_trailers(trailers("0")).unwrap();
    })
    .await;
    let transport = GrpcTransport::connect(
        TransportConfig::loopback(peer.address)
            .unwrap()
            .with_max_message_bytes(32)
            .unwrap(),
    )
    .await
    .unwrap();
    for seed in [1_u64, 2, 3, 5, 8, 13, 0x5eed, 0xcafe] {
        let metadata =
            Metadata::new([("x-seed".into(), CredentialValue::Text(seed.to_string()))]).unwrap();
        let mut session = transport
            .start(source(b"request"), metadata, Duration::from_secs(2))
            .unwrap();
        let expected = generated_messages(seed);
        assert_eq!(
            bounded(collect(&mut session)).await.unwrap(),
            expected,
            "seed={seed:x}"
        );
        let done = session.finish().await.unwrap();
        assert_eq!(done.responses_received, 24);
        assert_eq!(done.terminal_code, Some(Code::Ok));
    }
    assert_eq!(peer.connections.load(Ordering::SeqCst), 1);
    transport.close().await.unwrap();
    peer.close().await;
}

#[tokio::test]
async fn malformed_suffix_never_erases_valid_prefix_or_becomes_ok_after_draining() {
    for case in 0..6 {
        let suffix: &'static [u8] = match case {
            0 => b"\x01\0\0\0\0",
            1 => b"\x02\0\0\0\0",
            2 => b"\0\0\0\0\x21", // 33 > configured 32, without body allocation.
            3 => b"\0",
            4 => b"\0\0\0\0",
            _ => b"\0\0\0\0\x03ab",
        };
        let peer = Peer::new(move |request, mut reply| async move {
            drain(request).await;
            let mut response = reply.send_response(headers(), false).unwrap();
            let all = [
                frame(b""),
                frame(b"\xff\0prefix"),
                Bytes::from_static(suffix),
            ]
            .concat();
            // Split headers and cross the valid payload / malformed header boundary.
            for bytes in all.chunks(3) {
                response
                    .send_data(Bytes::copy_from_slice(bytes), false)
                    .unwrap();
            }
            let _ = response.send_trailers(trailers(if case % 2 == 0 { "0" } else { "15" }));
        })
        .await;
        let transport = GrpcTransport::connect(
            TransportConfig::loopback(peer.address)
                .unwrap()
                .with_max_message_bytes(32)
                .unwrap(),
        )
        .await
        .unwrap();
        let mut session = transport
            .start(
                source(b"request"),
                Metadata::default(),
                Duration::from_secs(2),
            )
            .unwrap();
        assert_eq!(
            bounded(session.next()).await.unwrap(),
            Some(Bytes::new()),
            "case={case}"
        );
        assert_eq!(
            bounded(session.next()).await.unwrap(),
            Some(Bytes::from_static(b"\xff\0prefix")),
            "case={case}"
        );
        let classify = |error: TransportError| match error {
            TransportError::CompressionUnsupported => 0,
            TransportError::Protocol => 1,
            TransportError::Grpc(Code::ResourceExhausted) => 2,
            other => panic!("case={case}, unexpected {other:?}"),
        };
        let expected = match case {
            0 => 0,
            2 => 2,
            _ => 1,
        };
        assert_eq!(
            classify(bounded(session.next()).await.unwrap_err()),
            expected
        );
        assert_eq!(
            session.next().await.unwrap(),
            None,
            "terminal delivered once"
        );
        assert_eq!(
            classify(session.finish().await.unwrap_err()),
            expected,
            "finish retains outcome"
        );
        assert_eq!(session.completion().responses_received, 2);
        assert!(session.completion().source_closed);
        transport.close().await.unwrap(); // Observed framing errors are not redelivered.
        peer.close().await;
    }
}
