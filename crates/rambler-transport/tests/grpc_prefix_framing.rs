//! Independent upload-envelope oracle: synthetic bytes, one RPC, numeric loopback.
//! No production framing helper or protobuf decoder participates in verification.
use bytes::Bytes;
use futures_util::{StreamExt, stream};
use rambler_transport::{CredentialValue, GrpcTransport, Metadata, StreamSource, TransportConfig};
use std::{sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::oneshot};
use tonic::Code;

#[tokio::test]
async fn every_small_upload_length_and_large_boundaries_preserve_raw_grpc_envelopes() {
    tokio::time::timeout(Duration::from_secs(30), async {
        const MESSAGE_COUNT: u64 = 260;
        const PAYLOAD_BYTES: usize = 115_840;
        const WIRE_BYTES: usize = PAYLOAD_BYTES + 5 * MESSAGE_COUNT as usize;
        const METHOD: &str = "/synthetic.PrefixOracle/Upload";

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let source_lifetime = Arc::new(());
        let source_guard = source_lifetime.clone();
        // Generate lazily; do not retain a second complete payload fixture.
        // This captured marker is released when the source stream is dropped.
        let source = StreamSource::new(
            stream::iter((0usize..=256).chain([1024, 16_384, 65_536]).enumerate()).map(
                move |(message_index, length)| {
                    let _keep_source_alive = &source_guard;
                    Ok(Bytes::from(
                        (0..length)
                            .map(|offset| ((message_index * 37 + offset * 13) % 256) as u8)
                            .collect::<Vec<_>>(),
                    ))
                },
            ),
        );
        assert_eq!(Arc::strong_count(&source_lifetime), 2);

        let client = async move {
            let config = TransportConfig::loopback(address)
                .unwrap()
                .with_method(METHOD)
                .unwrap();
            let transport = GrpcTransport::connect(config).await.unwrap();
            let metadata = Metadata::new([(
                "x-prefix-oracle".into(),
                CredentialValue::Text("invented-loopback-only".into()),
            )])
            .unwrap();
            let mut session = transport
                .start(source, metadata, Duration::from_secs(25))
                .unwrap();

            // The peer sends OK only after fully draining and checking the upload.
            // There are deliberately no response messages to discard implicitly.
            assert_eq!(session.next().await.unwrap(), None);
            let done = session.finish().await.unwrap();
            assert_eq!(done.terminal_code, Some(Code::Ok));
            assert_eq!(done.requests_produced, MESSAGE_COUNT);
            assert_eq!(done.requests_polled, MESSAGE_COUNT);
            assert_eq!(done.responses_received, 0);
            assert!(done.source_exhausted);
            assert!(done.upload_eof_polled);
            assert!(done.source_closed);
            assert_eq!(Arc::strong_count(&source_lifetime), 1, "source dropped");
            assert_eq!(session.next().await.unwrap(), None);
            drop(session);
            transport.close().await.unwrap();
            drop(transport);
        };

        let peer = async move {
            let (tcp, remote) = listener.accept().await.unwrap();
            assert!(remote.ip().is_loopback());
            let single_connection = async move {
                let mut connection = h2::server::handshake(tcp).await.unwrap();
                let (mut request, mut reply) =
                    connection.accept().await.expect("one upload RPC").unwrap();
                assert_eq!(u32::from(request.body().stream_id()), 1);
                assert_eq!(request.method(), http::Method::POST);
                assert_eq!(request.uri().path(), METHOD);
                assert_eq!(request.headers()["content-type"], "application/grpc");
                assert_eq!(request.headers()["te"], "trailers");
                assert_eq!(
                    request.headers()["x-prefix-oracle"],
                    "invented-loopback-only"
                );

                let exchange = async move {
                    // Preserve every DATA byte, independent of HTTP/2 partitioning.
                    let mut raw = Vec::with_capacity(WIRE_BYTES);
                    while let Some(chunk) = request.body_mut().data().await {
                        let chunk = chunk.unwrap();
                        assert!(
                            chunk.len() <= WIRE_BYTES - raw.len(),
                            "upload exceeds the exact bounded fixture"
                        );
                        raw.extend_from_slice(&chunk);
                        request
                            .body_mut()
                            .flow_control()
                            .release_capacity(chunk.len())
                            .unwrap();
                    }
                    assert!(request.body().is_end_stream());
                    assert!(request.body_mut().trailers().await.unwrap().is_none());
                    assert!(
                        request.body_mut().data().await.is_none(),
                        "stable upload EOF"
                    );
                    assert_eq!(raw.len(), WIRE_BYTES);

                    // Decode the complete raw upload here, without a transport codec,
                    // prefix helper, or assertions about HTTP/2 DATA frame boundaries.
                    let mut cursor = 0;
                    let mut decoded = 0;
                    for (message_index, expected_length) in
                        (0usize..=256).chain([1024, 16_384, 65_536]).enumerate()
                    {
                        let header = raw
                            .get(cursor..cursor + 5)
                            .expect("every message, including empty, has five prefix bytes");
                        assert_eq!(header[0], 0, "compression flag, message {message_index}");
                        let declared_length =
                            u32::from_be_bytes([header[1], header[2], header[3], header[4]])
                                as usize;
                        assert_eq!(
                            declared_length, expected_length,
                            "big-endian length, message {message_index}"
                        );
                        cursor += 5;
                        let payload = raw
                            .get(cursor..cursor + declared_length)
                            .expect("complete declared payload");
                        assert!(
                            payload.iter().enumerate().all(|(offset, &byte)| {
                                byte == ((message_index * 37 + offset * 13) % 256) as u8
                            }),
                            "exact payload and order, message {message_index}"
                        );
                        cursor += declared_length;
                        decoded += 1;
                    }
                    assert_eq!(decoded, MESSAGE_COUNT);
                    assert_eq!(cursor, raw.len(), "no omitted messages or trailing bytes");

                    let headers = http::Response::builder()
                        .header("content-type", "application/grpc")
                        .body(())
                        .unwrap();
                    let mut response = reply.send_response(headers, false).unwrap();
                    let mut trailers = http::HeaderMap::new();
                    trailers.insert("grpc-status", "0".parse().unwrap());
                    response.send_trailers(trailers).unwrap();
                    raw
                };
                let drive_to_close = async {
                    // Polling accept also drives flow-control updates and trailers.
                    // No second RPC or arbitrary connection error counts as success.
                    match connection.accept().await {
                        None => {}
                        Some(Ok(_)) => panic!("unexpected second RPC on the connection"),
                        Some(Err(error)) => panic!("peer connection failed: {error:?}"),
                    }
                };
                let (raw, ()) = tokio::join!(exchange, drive_to_close);
                raw
            };
            tokio::pin!(single_connection);
            // Keep the listener until the original connection closes. A queued or
            // concurrent extra TCP connection fails rather than escaping observation.
            tokio::select! {
                biased;
                extra = listener.accept() => {
                    extra.unwrap();
                    panic!("unexpected extra TCP connection");
                }
                raw = &mut single_connection => raw,
            }
        };

        // Both futures are owned here: timeout/panic drops them, not detached tasks.
        let ((), raw) = tokio::join!(client, peer);
        assert_eq!(
            raw.len(),
            WIRE_BYTES,
            "complete upload retained through cleanup"
        );
    })
    .await
    .expect("bounded synthetic upload-envelope oracle");
}

#[tokio::test]
async fn three_byte_remote_window_forces_a_split_cached_prefix() {
    tokio::time::timeout(Duration::from_secs(30), async {
        const METHOD: &str = "/synthetic.PrefixSplit/Upload";
        const PAYLOAD: &[u8] = b"\x00\xff\x42\xa5";
        const WIRE_BYTES: usize = 9;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (release_source, source_ready) = oneshot::channel::<()>();
        // The one owned gate prevents any payload (and hence its prefix) from
        // entering the upload before the peer's SETTINGS/PING barrier completes.
        let source = StreamSource::new(stream::once(async move {
            source_ready.await.expect("peer releases source after PONG");
            Ok(Bytes::from_static(PAYLOAD))
        }));

        let client = async move {
            let config = TransportConfig::loopback(address)
                .unwrap()
                .with_method(METHOD)
                .unwrap();
            let transport = GrpcTransport::connect(config).await.unwrap();
            let metadata = Metadata::new([(
                "x-prefix-split".into(),
                CredentialValue::Text("invented-loopback-only".into()),
            )])
            .unwrap();
            let mut session = transport
                .start(source, metadata, Duration::from_secs(25))
                .unwrap();

            assert_eq!(session.next().await.unwrap(), None);
            let done = session.finish().await.unwrap();
            assert_eq!(done.terminal_code, Some(Code::Ok));
            assert_eq!(done.requests_produced, 1);
            assert_eq!(done.requests_polled, 1);
            assert_eq!(done.responses_received, 0);
            assert!(done.source_exhausted);
            assert!(done.upload_eof_polled);
            assert!(done.source_closed);
            assert_eq!(session.next().await.unwrap(), None);
            drop(session);
            transport.close().await.unwrap();
            drop(transport);
        };

        let peer = async move {
            let (tcp, remote) = listener.accept().await.unwrap();
            assert!(remote.ip().is_loopback());
            let single_connection = async move {
                let mut connection = h2::server::Builder::new()
                    .initial_window_size(3)
                    .handshake(tcp)
                    .await
                    .unwrap();
                let mut pings = connection.ping_pong().unwrap();
                let settings_barrier = async move {
                    // Server SETTINGS precedes this PING on the connection.
                    // Its PONG proves the client processed the three-byte window;
                    // merely completing handshake or accepting headers would not.
                    pings.ping(h2::Ping::opaque()).await.unwrap();
                    release_source.send(()).expect("source gate is alive");
                };
                tokio::pin!(settings_barrier);
                let mut settings_applied = false;
                let mut accepted = None;
                // Keep driving h2 while awaiting PONG, even if request headers
                // arrive first. No sleeps, detached drivers, or timing guesses.
                while !settings_applied || accepted.is_none() {
                    tokio::select! {
                        () = &mut settings_barrier, if !settings_applied => {
                            settings_applied = true;
                        }
                        incoming = connection.accept() => {
                            let incoming = incoming.expect("one upload RPC").unwrap();
                            assert!(accepted.is_none(), "unexpected second RPC");
                            accepted = Some(incoming);
                        }
                    }
                }
                let (mut request, mut reply): (
                    http::Request<h2::RecvStream>,
                    h2::server::SendResponse<Bytes>,
                ) = accepted.unwrap();
                assert_eq!(u32::from(request.body().stream_id()), 1);
                assert_eq!(request.method(), http::Method::POST);
                assert_eq!(request.uri().path(), METHOD);
                assert_eq!(request.headers()["content-type"], "application/grpc");
                assert_eq!(request.headers()["te"], "trailers");
                assert_eq!(
                    request.headers()["x-prefix-split"],
                    "invented-loopback-only"
                );

                let exchange = async move {
                    let mut raw = Vec::with_capacity(WIRE_BYTES);
                    while let Some(chunk) = request.body_mut().data().await {
                        let chunk = chunk.unwrap();
                        if raw.is_empty() {
                            assert_eq!(chunk.len(), 3, "first DATA splits the five-byte prefix");
                            assert_eq!(chunk.as_ref(), &[0, 0, 0]);
                        }
                        assert!(
                            chunk.len() <= WIRE_BYTES - raw.len(),
                            "upload exceeds the exact nine-byte fixture"
                        );
                        raw.extend_from_slice(&chunk);
                        // Reopen the tiny window for the remaining prefix and
                        // payload; only this nine-byte fixture uses window three.
                        request
                            .body_mut()
                            .flow_control()
                            .release_capacity(chunk.len())
                            .unwrap();
                    }
                    assert!(request.body().is_end_stream());
                    assert!(request.body_mut().trailers().await.unwrap().is_none());
                    assert!(
                        request.body_mut().data().await.is_none(),
                        "stable upload EOF"
                    );
                    assert_eq!(raw.len(), WIRE_BYTES);
                    // Independent literal framing oracle, not the cached helper.
                    assert_eq!(raw[0], 0, "compression flag");
                    assert_eq!(
                        u32::from_be_bytes([raw[1], raw[2], raw[3], raw[4]]),
                        4,
                        "big-endian payload length"
                    );
                    assert_eq!(&raw[5..], PAYLOAD, "exact payload, no trailing bytes");

                    // Only a verified complete upload and stable EOF earn OK.
                    let headers = http::Response::builder()
                        .header("content-type", "application/grpc")
                        .body(())
                        .unwrap();
                    let mut response = reply.send_response(headers, false).unwrap();
                    let mut trailers = http::HeaderMap::new();
                    trailers.insert("grpc-status", "0".parse().unwrap());
                    response.send_trailers(trailers).unwrap();
                    raw
                };
                let drive_to_close = async {
                    match connection.accept().await {
                        None => {}
                        Some(Ok(_)) => panic!("unexpected second RPC on the connection"),
                        Some(Err(error)) => panic!("peer connection failed: {error:?}"),
                    }
                };
                let (raw, ()) = tokio::join!(exchange, drive_to_close);
                raw
            };
            tokio::pin!(single_connection);
            // Retain the listener tripwire until the original connection closes.
            tokio::select! {
                biased;
                extra = listener.accept() => {
                    extra.unwrap();
                    panic!("unexpected extra TCP connection");
                }
                raw = &mut single_connection => raw,
            }
        };

        // Both top-level futures are owned; the timeout drops them together.
        let ((), raw) = tokio::join!(client, peer);
        assert_eq!(
            raw.len(),
            WIRE_BYTES,
            "complete upload retained through cleanup"
        );
    })
    .await
    .expect("bounded forced upload-prefix split oracle");
}
