//! Deterministic offline connection-lifetime tests. Control messages, not sleeps,
//! choose where a TCP connection dies or receives GOAWAY.
// Raw frames also exercise GOAWAY cutoffs that h2::server cannot generate.
use bytes::Bytes;
use futures_util::future::poll_fn;
use rambler_transport::*;
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{Semaphore, mpsc, oneshot},
    task::{JoinHandle, JoinSet},
};

async fn bounded<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("bounded lifecycle test")
}

#[derive(Clone, Default)]
struct Counts {
    next: Arc<AtomicUsize>,
    closes: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
struct Source {
    counts: Counts,
    bytes: Option<Bytes>,
}
impl Counts {
    fn source(&self, bytes: Bytes) -> Source {
        Source {
            counts: self.clone(),
            bytes: Some(bytes),
        }
    }
    fn assert_closed(&self) {
        assert_eq!(self.closes.load(Ordering::SeqCst), 1);
        assert_eq!(self.drops.load(Ordering::SeqCst), 1);
    }
}
impl RequestSource for Source {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        self.counts.next.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(self.bytes.take()) })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.counts.closes.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        self.counts.drops.fetch_add(1, Ordering::SeqCst);
    }
}

struct GatedCleanup {
    source: Source,
    release: Arc<Semaphore>,
    fail: bool,
}
impl RequestSource for GatedCleanup {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        if self.source.bytes.is_none() {
            Box::pin(std::future::pending())
        } else {
            self.source.next()
        }
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.source.close().await?;
            self.release.acquire().await.unwrap().forget();
            if self.fail {
                Err(SourceError::new(std::io::Error::other("private cleanup")))
            } else {
                Ok(())
            }
        })
    }
}

enum Command {
    Disconnect(oneshot::Sender<()>),
    GoAway(h2::Reason, oneshot::Sender<()>),
    Graceful(oneshot::Sender<()>),
}
struct Peer {
    address: std::net::SocketAddr,
    commands: mpsc::UnboundedSender<Command>,
    connections: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
    uploaded: mpsc::UnboundedReceiver<()>,
    release: Arc<Semaphore>,
    task: JoinHandle<()>,
}
impl Peer {
    async fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let connections = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let (commands, mut rx) = mpsc::unbounded_channel();
        let (uploaded_tx, uploaded) = mpsc::unbounded_channel();
        let connection_count = connections.clone();
        let call_count = calls.clone();
        let release = Arc::new(Semaphore::new(0));
        let held_release = release.clone();
        let task = tokio::spawn(async move {
            // Tests replace connections sequentially, keeping the same listener.
            loop {
                let (tcp, _) = listener.accept().await.unwrap();
                tcp.set_nodelay(true).unwrap();
                connection_count.fetch_add(1, Ordering::SeqCst);
                let mut connection = h2::server::handshake(tcp).await.unwrap();
                let mut streams = JoinSet::new();
                loop {
                    tokio::select! {
                        biased;
                        command = rx.recv() => match command {
                            Some(Command::Disconnect(done)) => {
                                drop(connection);
                                streams.abort_all();
                                while let Some(result) = streams.join_next().await { if let Err(error) = result { assert!(error.is_cancelled()); } }
                                let _ = done.send(());
                                break;
                            }
                            Some(Command::GoAway(reason, done)) => {
                                connection.abrupt_shutdown(reason);
                                // Acknowledge only after GOAWAY has been flushed.
                                let _ = poll_fn(|cx| connection.poll_closed(cx)).await;
                                streams.abort_all();
                                while let Some(result) = streams.join_next().await { if let Err(error) = result { assert!(error.is_cancelled()); } }
                                let _ = done.send(());
                                break;
                            }
                            Some(Command::Graceful(done)) => {
                                connection.graceful_shutdown();
                                let mut ping = connection.ping_pong().unwrap();
                                streams.spawn(async move {
                                    // The PONG proves the client processed the
                                    // preceding GOAWAY, with no timing sleep.
                                    ping.ping(h2::Ping::opaque()).await.unwrap();
                                    let _ = done.send(());
                                });
                            }
                            None => return,
                        },
                        result = streams.join_next(), if !streams.is_empty() => { result.unwrap().unwrap(); },
                        request = connection.accept() => match request {
                            Some(Ok((mut request, mut reply))) => {
                                call_count.fetch_add(1, Ordering::SeqCst);
                                let uploaded = uploaded_tx.clone();
                                let release = held_release.clone();
                                streams.spawn(async move {
                                    let mode = request.headers().get("x-mode").and_then(|v| v.to_str().ok()).unwrap_or("echo").to_owned();
                                    let mut input = Vec::new();
                                    while let Some(data) = request.body_mut().data().await {
                                        let Ok(data) = data else { return; };
                                        input.extend_from_slice(&data);
                                        request.body_mut().flow_control().release_capacity(data.len()).unwrap();
                                        if mode == "cancel-upload" {
                                            reply.send_reset(h2::Reason::CANCEL);
                                            return;
                                        }
                                        if mode == "during-upload" {
                                            let _ = uploaded.send(());
                                            std::future::pending::<()>().await;
                                        }
                                    }
                                    let _ = uploaded.send(());
                                    if mode == "before-headers" { std::future::pending::<()>().await; }
                                    let head = http::Response::builder().header("content-type", "application/grpc").body(()).unwrap();
                                    let mut response = reply.send_response(head, false).unwrap();
                                    if mode == "after-prefix" {
                                        response.send_data(Bytes::from_static(b"\0\0\0\0\x07partial"), false).unwrap();
                                        std::future::pending::<()>().await;
                                    }
                                    if mode == "reset" {
                                        response.send_reset(h2::Reason::REFUSED_STREAM);
                                    } else {
                                        if mode == "held" {
                                            response.send_data(Bytes::from_static(b"\0\0\0\0\x07partial"), false).unwrap();
                                            release.acquire().await.unwrap().forget();
                                        } else {
                                            response.send_data(Bytes::from(input), false).unwrap();
                                        }
                                        let mut trailers = http::HeaderMap::new();
                                        trailers.insert("grpc-status", "0".parse().unwrap());
                                        let _ = response.send_trailers(trailers);
                                    }
                                });
                            }
                            Some(Err(_)) | None => break,
                        }
                    }
                }
            }
        });
        Self {
            address,
            commands,
            connections,
            calls,
            uploaded,
            release,
            task,
        }
    }
    async fn transport(&self) -> GrpcTransport {
        bounded(GrpcTransport::connect(
            TransportConfig::loopback(self.address).unwrap(),
        ))
        .await
        .unwrap()
    }
    async fn disconnect(&self) {
        let (tx, rx) = oneshot::channel();
        self.commands.send(Command::Disconnect(tx)).unwrap();
        bounded(rx).await.unwrap();
    }
    async fn goaway(&self, reason: h2::Reason) {
        let (tx, rx) = oneshot::channel();
        self.commands.send(Command::GoAway(reason, tx)).unwrap();
        bounded(rx).await.unwrap();
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn metadata(mode: &str) -> Metadata {
    Metadata::new([("x-mode".into(), CredentialValue::Text(mode.into()))]).unwrap()
}
async fn echo(transport: &GrpcTransport, bytes: Bytes) {
    let counts = Counts::default();
    let mut session = transport
        .start(
            counts.source(bytes.clone()),
            Metadata::default(),
            Duration::from_secs(3),
        )
        .unwrap();
    assert_eq!(bounded(session.next()).await.unwrap(), Some(bytes));
    assert_eq!(bounded(session.next()).await.unwrap(), None);
    let done = session.finish().await.unwrap();
    assert!(done.source_closed && done.source_exhausted && done.upload_eof_polled);
    counts.assert_closed();
}

#[tokio::test]
async fn initial_http2_protocol_failure_closes_accepted_source_without_replay() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (sent, received) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        let mut preface = [0; 24];
        tcp.read_exact(&mut preface).await.unwrap();
        assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        // SETTINGS is forbidden on stream 1. The server never accepts an RPC.
        tcp.write_all(b"\0\0\0\x04\0\0\0\0\x01").await.unwrap();
        sent.send(()).unwrap();
        let mut bytes = [0; 1024];
        while tcp.read(&mut bytes).await.is_ok_and(|n| n != 0) {}
    });
    let transport = bounded(GrpcTransport::connect(
        TransportConfig::loopback(address).unwrap(),
    ))
    .await
    .unwrap();
    bounded(received).await.unwrap();
    let counts = Counts::default();
    let mut session = transport
        .start(
            counts.source(Bytes::from_static(b"not-replayed")),
            Metadata::default(),
            Duration::from_secs(3),
        )
        .unwrap();
    let error = bounded(session.finish()).await.unwrap_err();
    assert!(matches!(error, TransportError::Grpc(_)), "{error:?}");
    counts.assert_closed();
    assert!(matches!(
        bounded(transport.close()).await,
        Err(TransportError::Connect)
    ));
    bounded(transport.close()).await.unwrap();
    bounded(server).await.unwrap();
}

#[tokio::test]
async fn remote_cancel_during_flow_blocked_upload_preserves_reset_code_and_neighbors() {
    let peer = Peer::new().await;
    let transport = peer.transport().await;
    echo(&transport, Bytes::from_static(b"before")).await;
    let counts = Counts::default();
    let mut session = transport
        .start(
            counts.source(Bytes::from(vec![0; 1024 * 1024])),
            metadata("cancel-upload"),
            Duration::from_secs(3),
        )
        .unwrap();
    let error = bounded(session.finish()).await.unwrap_err();
    assert!(
        matches!(error, TransportError::Grpc(tonic::Code::Cancelled)),
        "remote CANCEL was replaced by {error:?}"
    );
    counts.assert_closed();
    echo(&transport, Bytes::from_static(b"after")).await;
    assert_eq!(peer.calls.load(Ordering::SeqCst), 3);
    assert_eq!(peer.connections.load(Ordering::SeqCst), 1);
    transport.close().await.unwrap();
}

#[tokio::test]
async fn observed_goaway_during_blocked_upload_keeps_independent_driver_result() {
    let mut peer = Peer::new().await;
    let transport = peer.transport().await;
    let counts = Counts::default();
    let mut session = transport
        .start(
            counts.source(Bytes::from(vec![0; 1024 * 1024])),
            metadata("during-upload"),
            Duration::from_secs(3),
        )
        .unwrap();
    bounded(peer.uploaded.recv()).await.unwrap();
    peer.goaway(h2::Reason::PROTOCOL_ERROR).await;
    assert!(matches!(
        bounded(session.finish()).await,
        Err(TransportError::Grpc(_))
    ));
    counts.assert_closed();
    assert!(matches!(
        bounded(transport.close()).await,
        Err(TransportError::Connect)
    ));
    bounded(transport.close()).await.unwrap();
    assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
    assert_eq!(peer.connections.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn observed_goaway_keeps_driver_result_without_cross_layer_identity() {
    let mut peer = Peer::new().await;
    let transport = peer.transport().await;
    let counts = Counts::default();
    let mut session = transport
        .start(
            counts.source(Bytes::from_static(b"never-replay")),
            metadata("before-headers"),
            Duration::from_secs(3),
        )
        .unwrap();
    bounded(peer.uploaded.recv()).await.unwrap();
    peer.goaway(h2::Reason::PROTOCOL_ERROR).await;
    let error = bounded(session.finish()).await.unwrap_err();
    assert!(
        matches!(
            error,
            TransportError::Grpc(tonic::Code::Internal | tonic::Code::Unavailable)
        ),
        "{error:?}"
    );
    counts.assert_closed();
    assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
    assert!(matches!(
        bounded(transport.close()).await,
        Err(TransportError::Connect)
    ));
    bounded(transport.close()).await.unwrap();
}

#[tokio::test]
async fn interrupted_shutdown_retains_driver_error_regardless_of_session_observation() {
    for fail_cleanup in [false, true] {
        let mut peer = Peer::new().await;
        let transport = peer.transport().await;
        let counts = Counts::default();
        let release = Arc::new(Semaphore::new(0));
        let mut session = transport
            .start(
                GatedCleanup {
                    source: counts.source(Bytes::from_static(b"request")),
                    release: release.clone(),
                    fail: fail_cleanup,
                },
                metadata("during-upload"),
                Duration::from_secs(3),
            )
            .unwrap();
        bounded(peer.uploaded.recv()).await.unwrap();
        peer.goaway(h2::Reason::PROTOCOL_ERROR).await;
        // Source.next is pending, so cleanup begins only after the supervisor
        // has committed the network outcome, not after natural source EOF.
        bounded(async {
            while counts.closes.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await;
        {
            let close = transport.close();
            tokio::pin!(close);
            assert!(futures_util::poll!(close.as_mut()).is_pending());
        }
        assert_eq!(counts.drops.load(Ordering::SeqCst), 0);
        release.add_permits(1);
        let error = bounded(session.finish()).await.unwrap_err();
        if fail_cleanup {
            assert!(matches!(error, TransportError::Cleanup(_)));
        } else {
            assert!(matches!(error, TransportError::Grpc(_)));
        }
        // Neither session result identifies this exact driver failure. It stays
        // owed after the interrupted close, until actually returned by close.
        assert!(matches!(
            bounded(transport.close()).await,
            Err(TransportError::Connect)
        ));
        counts.assert_closed();
        bounded(transport.close()).await.unwrap();
    }
}

#[tokio::test]
async fn graceful_goaway_rejects_new_sessions_without_interrupting_existing_stream() {
    let peer = Peer::new().await;
    let transport = peer.transport().await;
    let held_counts = Counts::default();
    let mut held = transport
        .start(
            held_counts.source(Bytes::from_static(b"in-flight")),
            metadata("held"),
            Duration::from_secs(3),
        )
        .unwrap();
    assert_eq!(
        bounded(held.next()).await.unwrap(),
        Some(Bytes::from_static(b"partial"))
    );
    let (tx, rx) = oneshot::channel();
    peer.commands.send(Command::Graceful(tx)).unwrap();
    bounded(rx).await.unwrap();
    let rejected_counts = Counts::default();
    let mut rejected = transport
        .start(
            rejected_counts.source(Bytes::from_static(b"never-replay")),
            Metadata::default(),
            Duration::from_secs(3),
        )
        .unwrap();
    assert!(matches!(
        bounded(rejected.finish()).await,
        Err(TransportError::Grpc(_))
    ));
    rejected_counts.assert_closed();
    assert_eq!(
        peer.calls.load(Ordering::SeqCst),
        1,
        "no stream accepted after GOAWAY barrier"
    );
    peer.release.add_permits(1);
    assert_eq!(
        bounded(held.finish()).await.unwrap().terminal_code,
        Some(tonic::Code::Ok)
    );
    held_counts.assert_closed();
    assert_eq!(peer.connections.load(Ordering::SeqCst), 1);
    transport.close().await.unwrap();
}

#[tokio::test]
async fn connection_dying_between_sessions_never_reopens_or_loses_accepted_sources() {
    for goaway in [false, true] {
        let peer = Peer::new().await;
        let config = TransportConfig::loopback(peer.address).unwrap();
        let transport = GrpcTransport::connect(config.clone()).await.unwrap();
        echo(&transport, Bytes::from_static(b"first")).await;
        if goaway {
            peer.goaway(h2::Reason::NO_ERROR).await;
        } else {
            peer.disconnect().await;
        }
        for _ in 0..4 {
            let counts = Counts::default();
            // start accepts ownership even if the existing h2 channel has died.
            let mut session = transport
                .start(
                    counts.source(Bytes::from_static(b"not-replayed")),
                    Metadata::default(),
                    Duration::from_secs(3),
                )
                .unwrap();
            let error = bounded(session.finish()).await.unwrap_err();
            assert!(matches!(error, TransportError::Grpc(_)), "{error:?}");
            counts.assert_closed();
        }
        assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
        assert_eq!(peer.connections.load(Ordering::SeqCst), 1);
        bounded(transport.close()).await.unwrap();
        let clone = transport.clone();
        assert!(matches!(
            clone.start(
                Counts::default().source(Bytes::new()),
                Metadata::default(),
                Duration::from_secs(3)
            ),
            Err(TransportError::Closed)
        ));
        // Recovery is an explicit new transport, never a mutation of old clones.
        let replacement = bounded(GrpcTransport::connect(config)).await.unwrap();
        echo(&replacement, Bytes::from_static(b"new-session-only")).await;
        assert_eq!(peer.connections.load(Ordering::SeqCst), 2);
        assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
        assert!(matches!(
            transport.start(
                Counts::default().source(Bytes::new()),
                Metadata::default(),
                Duration::from_secs(3)
            ),
            Err(TransportError::Closed)
        ));
        replacement.close().await.unwrap();
    }
}

#[tokio::test]
async fn unrelated_observed_rpc_failure_does_not_acknowledge_later_driver_failure() {
    let mut peer = Peer::new().await;
    let transport = peer.transport().await;
    let counts = Counts::default();
    let mut reset = transport
        .start(
            counts.source(Bytes::from_static(b"one")),
            metadata("reset"),
            Duration::from_secs(3),
        )
        .unwrap();
    assert!(matches!(
        bounded(reset.finish()).await,
        Err(TransportError::Grpc(tonic::Code::Unavailable))
    ));
    counts.assert_closed();
    bounded(peer.uploaded.recv()).await.unwrap();
    let pending_counts = Counts::default();
    let release = Arc::new(Semaphore::new(0));
    let mut pending = transport
        .start(
            GatedCleanup {
                source: pending_counts.source(Bytes::from_static(b"two")),
                release: release.clone(),
                fail: false,
            },
            metadata("during-upload"),
            Duration::from_secs(3),
        )
        .unwrap();
    bounded(peer.uploaded.recv()).await.unwrap();
    peer.goaway(h2::Reason::PROTOCOL_ERROR).await;
    bounded(async {
        while pending_counts.closes.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    release.add_permits(1);
    assert!(matches!(
        bounded(transport.close()).await,
        Err(TransportError::Connect)
    ));
    assert!(matches!(
        bounded(pending.finish()).await,
        Err(TransportError::Grpc(_))
    ));
    pending_counts.assert_closed();
    transport.close().await.unwrap();
    assert_eq!(peer.connections.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn observed_graceful_goaway_does_not_hide_later_connection_failure() {
    let mut peer = Peer::new().await;
    let transport = peer.transport().await;
    let pending_counts = Counts::default();
    let release = Arc::new(Semaphore::new(0));
    let mut pending = transport
        .start(
            GatedCleanup {
                source: pending_counts.source(Bytes::from_static(b"held")),
                release: release.clone(),
                fail: false,
            },
            metadata("during-upload"),
            Duration::from_secs(3),
        )
        .unwrap();
    bounded(peer.uploaded.recv()).await.unwrap();
    let (tx, rx) = oneshot::channel();
    peer.commands.send(Command::Graceful(tx)).unwrap();
    bounded(rx).await.unwrap();
    let rejected_counts = Counts::default();
    let mut rejected = transport
        .start(
            rejected_counts.source(Bytes::from_static(b"rejected")),
            Metadata::default(),
            Duration::from_secs(3),
        )
        .unwrap();
    assert!(matches!(
        bounded(rejected.finish()).await,
        Err(TransportError::Grpc(_))
    ));
    rejected_counts.assert_closed();
    peer.goaway(h2::Reason::PROTOCOL_ERROR).await;
    bounded(async {
        while pending_counts.closes.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    release.add_permits(1);
    assert!(
        matches!(
            bounded(transport.close()).await,
            Err(TransportError::Connect)
        ),
        "observing graceful GOAWAY must not acknowledge a later non-OK GOAWAY"
    );
    assert!(matches!(
        bounded(pending.finish()).await,
        Err(TransportError::Grpc(_))
    ));
    pending_counts.assert_closed();
    transport.close().await.unwrap();
    assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
    assert_eq!(peer.connections.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn endurance_512_short_sessions_keep_one_tcp_and_isolate_failures() {
    let peer = Peer::new().await;
    let transport = peer.transport().await;
    // First force sequential retirement, then exercise concurrent independent
    // streams. Every eighth stream is refused after upload; none may be replayed.
    for wave in 0..92 {
        let width = if wave < 64 { 1 } else { 16 };
        let mut sessions = Vec::new();
        for index in 0..width {
            let id = if wave < 64 {
                wave
            } else {
                64 + (wave - 64) * 16 + index
            };
            let bytes = Bytes::from(format!("session-{id:04}"));
            let counts = Counts::default();
            let fail = id % 8 == 0;
            let session = transport
                .start(
                    counts.source(bytes.clone()),
                    metadata(if fail { "reset" } else { "echo" }),
                    Duration::from_secs(3),
                )
                .unwrap();
            sessions.push((session, counts, bytes, fail));
        }
        bounded(futures_util::future::join_all(sessions.into_iter().map(
            |(mut session, counts, bytes, fail)| async move {
                if fail {
                    assert!(matches!(
                        session.finish().await,
                        Err(TransportError::Grpc(tonic::Code::Unavailable))
                    ));
                } else {
                    assert_eq!(session.next().await.unwrap(), Some(bytes));
                    assert_eq!(session.next().await.unwrap(), None);
                    assert!(session.finish().await.unwrap().upload_eof_polled);
                }
                assert!(session.completion().source_closed);
                assert_eq!(session.completion().requests_produced, 1);
                counts.assert_closed();
            },
        )))
        .await;
    }
    assert_eq!(
        peer.calls.load(Ordering::SeqCst),
        512,
        "one call per accepted source; no replay"
    );
    assert_eq!(
        peer.connections.load(Ordering::SeqCst),
        1,
        "same TCP for every wave"
    );
    transport.close().await.unwrap();
}

#[tokio::test]
async fn connection_loss_preserves_session_error_and_independent_driver_result() {
    for mode in ["before-headers", "after-prefix"] {
        let mut peer = Peer::new().await;
        let transport = peer.transport().await;
        let counts = Counts::default();
        let mut session = transport
            .start(
                counts.source(Bytes::from_static(b"never-replay")),
                metadata(mode),
                Duration::from_secs(3),
            )
            .unwrap();
        bounded(peer.uploaded.recv()).await.unwrap();
        if mode == "after-prefix" {
            assert_eq!(
                bounded(session.next()).await.unwrap(),
                Some(Bytes::from_static(b"partial"))
            );
        }
        peer.disconnect().await;
        let error = bounded(session.finish()).await.unwrap_err();
        assert!(
            matches!(error, TransportError::Grpc(tonic::Code::Unavailable)),
            "{mode}: {error:?}"
        );
        counts.assert_closed();
        assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
        assert_eq!(peer.connections.load(Ordering::SeqCst), 1);
        // Windows can finish the driver normally on this peer teardown while
        // Linux may retain a terminal I/O error. The driver has an independent
        // outcome: it cannot be acknowledged from a possibly related RPC error.
        let driver = bounded(transport.close()).await;
        assert!(
            matches!(driver, Ok(()) | Err(TransportError::Connect)),
            "unexpected independent driver outcome: {driver:?}"
        );
        assert!(bounded(transport.close()).await.is_ok());
    }
}

// Raw frames are needed here: h2::server::abrupt_shutdown keeps the last
// stream alive, so it cannot reproduce cutoff-zero rejection without TCP EOF.
async fn raw_frame(tcp: &mut tokio::net::TcpStream) -> (u8, u8, Vec<u8>) {
    let mut header = [0; 9];
    tcp.read_exact(&mut header).await.unwrap();
    let size =
        (usize::from(header[0]) << 16) | (usize::from(header[1]) << 8) | usize::from(header[2]);
    let mut payload = vec![0; size];
    tcp.read_exact(&mut payload).await.unwrap();
    (header[3], header[4], payload)
}
async fn raw_send(tcp: &mut tokio::net::TcpStream, kind: u8, flags: u8, stream: u32, data: &[u8]) {
    let size = u32::try_from(data.len()).unwrap().to_be_bytes();
    let id = stream.to_be_bytes();
    tcp.write_all(&[
        size[1], size[2], size[3], kind, flags, id[0], id[1], id[2], id[3],
    ])
    .await
    .unwrap();
    tcp.write_all(data).await.unwrap();
}
async fn raw_goaway(tcp: &mut tokio::net::TcpStream, last: u32, reason: h2::Reason) {
    let mut payload = last.to_be_bytes().to_vec();
    payload.extend_from_slice(&u32::from(reason).to_be_bytes());
    raw_send(tcp, 7, 0, 0, &payload).await;
}
async fn raw_ping_barrier(tcp: &mut tokio::net::TcpStream) {
    raw_send(tcp, 6, 0, 0, &[0; 8]).await;
    loop {
        let (kind, flags, payload) = raw_frame(tcp).await;
        if kind == 6 && flags == 1 {
            assert_eq!(payload, [0; 8]);
            break;
        }
    }
}
async fn raw_preface(tcp: &mut tokio::net::TcpStream, zero_window: bool) {
    tcp.set_nodelay(true).unwrap();
    let mut preface = [0; 24];
    tcp.read_exact(&mut preface).await.unwrap();
    assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
    let settings: &[u8] = if zero_window {
        &[0, 4, 0, 0, 0, 0]
    } else {
        &[]
    };
    raw_send(tcp, 4, 0, 0, settings).await;
    if zero_window {
        // Caller waits for this barrier before creating a request. Otherwise
        // draining for PONG could consume its HEADERS/DATA synchronization.
        raw_ping_barrier(tcp).await;
    }
}
async fn raw_eof(tcp: &mut tokio::net::TcpStream) {
    let mut buf = [0; 1024];
    while tcp.read(&mut buf).await.is_ok_and(|n| n != 0) {}
}

#[tokio::test]
async fn cutoff_zero_goaway_keeps_driver_reportable_with_or_without_response_backpressure() {
    for backpressure in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (ready_tx, ready_rx) = oneshot::channel();
        let (fatal_tx, fatal_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut tcp, _) = listener.accept().await.unwrap();
            raw_preface(&mut tcp, true).await;
            ready_tx.send(()).unwrap();
            while raw_frame(&mut tcp).await.0 != 1 {}
            if backpressure {
                // HPACK: indexed :status=200, literal content-type (index 31).
                let mut headers = vec![0x88, 0x0f, 0x10, 16];
                headers.extend_from_slice(b"application/grpc");
                raw_send(&mut tcp, 1, 4, 1, &headers).await;
                raw_send(
                    &mut tcp,
                    0,
                    0,
                    1,
                    b"\0\0\0\0\x01a\0\0\0\0\x01b\0\0\0\0\x01c",
                )
                .await;
            }
            fatal_rx.await.unwrap();
            raw_goaway(&mut tcp, 0, h2::Reason::PROTOCOL_ERROR).await;
            raw_eof(&mut tcp).await;
        });
        let transport = bounded(GrpcTransport::connect(
            TransportConfig::loopback(address).unwrap(),
        ))
        .await
        .unwrap();
        bounded(ready_rx).await.unwrap();
        let counts = Counts::default();
        let release = Arc::new(Semaphore::new(0));
        let mut session = transport
            .start(
                GatedCleanup {
                    source: counts.source(Bytes::from(vec![0; 1024 * 1024])),
                    release: release.clone(),
                    fail: false,
                },
                Metadata::default(),
                Duration::from_secs(3),
            )
            .unwrap();
        if backpressure {
            bounded(async {
                while session.completion().responses_received < 2 {
                    tokio::task::yield_now().await;
                }
            })
            .await;
            assert_eq!(
                session.completion().responses_received,
                2,
                "second send blocks on capacity-one queue"
            );
        }
        fatal_tx.send(()).unwrap();
        // Do not drain responses until the RPC has selected its failure. This
        // proves the recovery poll cannot await a queue-full reader indefinitely.
        bounded(async {
            while counts.closes.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await;
        if backpressure {
            assert_eq!(session.completion().responses_received, 2);
        }
        release.add_permits(1);
        assert!(matches!(
            bounded(session.finish()).await,
            Err(TransportError::Grpc(tonic::Code::Internal))
        ));
        counts.assert_closed();
        // h2 provides no identity linking stream and driver failures. A possible
        // duplicate is safer than silently acknowledging an independent error.
        assert!(matches!(
            bounded(transport.close()).await,
            Err(TransportError::Connect)
        ));
        bounded(transport.close()).await.unwrap();
        bounded(server).await.unwrap();
    }
}

#[tokio::test]
async fn observed_nonfatal_goaway_never_acknowledges_later_driver_failure() {
    for same_reason in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (first_tx, first_rx) = oneshot::channel();
        let (fatal_tx, fatal_rx) = oneshot::channel();
        let first_reason = h2::Reason::ENHANCE_YOUR_CALM;
        let server = tokio::spawn(async move {
            let (mut tcp, _) = listener.accept().await.unwrap();
            raw_preface(&mut tcp, false).await;
            while raw_frame(&mut tcp).await.0 != 0 {}
            raw_goaway(&mut tcp, 1, first_reason).await;
            raw_ping_barrier(&mut tcp).await;
            first_tx.send(()).unwrap();
            fatal_rx.await.unwrap();
            raw_goaway(
                &mut tcp,
                0,
                if same_reason {
                    first_reason
                } else {
                    h2::Reason::PROTOCOL_ERROR
                },
            )
            .await;
            raw_eof(&mut tcp).await;
        });
        let transport = bounded(GrpcTransport::connect(
            TransportConfig::loopback(address).unwrap(),
        ))
        .await
        .unwrap();
        let counts = Counts::default();
        let release = Arc::new(Semaphore::new(0));
        let mut held = transport
            .start(
                GatedCleanup {
                    source: counts.source(Bytes::from_static(b"held")),
                    release: release.clone(),
                    fail: true,
                },
                Metadata::default(),
                Duration::from_secs(3),
            )
            .unwrap();
        bounded(first_rx).await.unwrap();
        assert_eq!(
            counts.closes.load(Ordering::SeqCst),
            0,
            "cutoff 1 leaves stream 1 active, even for non-OK GOAWAY"
        );
        let rejected_counts = Counts::default();
        let mut rejected = transport
            .start(
                rejected_counts.source(Bytes::from_static(b"rejected")),
                Metadata::default(),
                Duration::from_secs(3),
            )
            .unwrap();
        assert!(matches!(
            bounded(rejected.finish()).await,
            Err(TransportError::Grpc(tonic::Code::ResourceExhausted))
        ));
        rejected_counts.assert_closed();
        fatal_tx.send(()).unwrap();
        bounded(async {
            while counts.closes.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await;
        // Cancel close after it saved the driver failure, while cleanup waits.
        {
            let close = transport.close();
            tokio::pin!(close);
            assert!(futures_util::poll!(close.as_mut()).is_pending());
        }
        release.add_permits(1);
        assert!(matches!(
            bounded(held.finish()).await,
            Err(TransportError::Cleanup(_))
        ));
        counts.assert_closed();
        bounded(server).await.unwrap();
        assert!(
            matches!(
                bounded(transport.close()).await,
                Err(TransportError::Connect)
            ),
            "first observed GOAWAY must not hide a later driver failure (same_reason={same_reason})"
        );
        bounded(transport.close()).await.unwrap();
    }
}
