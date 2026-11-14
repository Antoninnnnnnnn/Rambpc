//! Real, offline tonic HTTP/2 loopback tests; no protoc, generated proto, real
//! credentials, Google access, or process environment reads.
use bytes::Bytes;
use futures_util::{Stream, StreamExt, stream};
use rambler_transport::*;
use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{Notify, Semaphore},
    task::JoinHandle,
};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::sync::CancellationToken;
use tonic::{
    Code, Request, Response, Status, Streaming,
    body::Body,
    codegen::{Service, StdError},
    server::{Grpc, NamedService, StreamingService},
    transport::Server,
};

type Replies = Pin<Box<dyn Stream<Item = Result<Bytes, Status>> + Send>>;
type HandlerFuture = Pin<Box<dyn Future<Output = Result<Response<Replies>, Status>> + Send>>;
type Handler = Arc<dyn Fn(Request<Streaming<Bytes>>) -> HandlerFuture + Send + Sync>;
#[derive(Clone)]
struct RawService {
    handler: Handler,
    paths: Arc<Mutex<Vec<String>>>,
}
impl NamedService for RawService {
    const NAME: &'static str = "speech.s3.S3StubbyClientConnectorService";
}
impl<B> Service<http::Request<B>> for RawService
where
    B: tonic::codegen::Body + Send + 'static,
    B::Error: Into<StdError> + Send + 'static,
{
    type Response = http::Response<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: http::Request<B>) -> Self::Future {
        self.paths
            .lock()
            .unwrap()
            .push(request.uri().path().to_owned());
        let handler = self.handler.clone();
        Box::pin(async move {
            struct Method(Handler);
            impl StreamingService<Bytes> for Method {
                type Response = Bytes;
                type ResponseStream = Replies;
                type Future = HandlerFuture;
                fn call(&mut self, request: Request<Streaming<Bytes>>) -> Self::Future {
                    (self.0)(request)
                }
            }
            Ok(Grpc::new(RawBytesCodec)
                .max_decoding_message_size(16 * 1024 * 1024)
                .max_encoding_message_size(16 * 1024 * 1024)
                .streaming(Method(handler), request)
                .await)
        })
    }
}
struct TestServer {
    address: std::net::SocketAddr,
    paths: Arc<Mutex<Vec<String>>>,
    stop: CancellationToken,
    task: Option<JoinHandle<()>>,
}
impl TestServer {
    async fn new<F, Fut>(handler: F) -> Self
    where
        F: Fn(Request<Streaming<Bytes>>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Response<Replies>, Status>> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let paths = Arc::new(Mutex::new(Vec::new()));
        let service = RawService {
            handler: Arc::new(move |request| Box::pin(handler(request))),
            paths: paths.clone(),
        };
        let stop = CancellationToken::new();
        let done = stop.clone();
        // Custom incoming streams bypass tonic's TCP acceptor configuration.
        // Disable Nagle explicitly, as the production client already does, so
        // Linux delayed ACKs do not dominate the flow-window boundary test.
        let incoming = TcpListenerStream::new(listener).map(|accepted| {
            accepted.and_then(|stream| {
                stream.set_nodelay(true)?;
                Ok(stream)
            })
        });
        let task = tokio::spawn(async move {
            Server::builder()
                .add_service(service)
                .serve_with_incoming_shutdown(incoming, done.cancelled_owned())
                .await
                .unwrap();
        });
        Self {
            address,
            paths,
            stop,
            task: Some(task),
        }
    }
    async fn transport(&self) -> GrpcTransport {
        GrpcTransport::connect(TransportConfig::loopback(self.address).unwrap())
            .await
            .unwrap()
    }
    async fn close(mut self) {
        self.stop.cancel();
        self.task.take().unwrap().await.unwrap();
    }
}
impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
fn replies(
    stream: impl Stream<Item = Result<Bytes, Status>> + Send + 'static,
) -> Response<Replies> {
    Response::new(Box::pin(stream))
}
async fn echo(request: Request<Streaming<Bytes>>) -> Result<Response<Replies>, Status> {
    Ok(replies(stream::unfold(
        request.into_inner(),
        |mut requests| async move {
            match requests.message().await {
                Ok(Some(bytes)) => Some((Ok(bytes), requests)),
                Ok(None) => None,
                Err(e) => Some((Err(e), requests)),
            }
        },
    )))
}
fn source(values: impl IntoIterator<Item = Bytes>) -> impl RequestSource {
    StreamSource::new(stream::iter(
        values.into_iter().collect::<Vec<_>>().into_iter().map(Ok),
    ))
}
async fn collect(session: &mut Session) -> Result<Vec<Bytes>, TransportError> {
    let mut result = Vec::new();
    while let Some(bytes) = session.next().await? {
        result.push(bytes);
    }
    Ok(result)
}
async fn bounded<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("test deadline")
}
fn fake_metadata(value: &str) -> Metadata {
    Metadata::new([
        ("x-test".into(), CredentialValue::Text(value.into())),
        (
            "opaque-bin".into(),
            CredentialValue::Binary(Bytes::from_static(b"\x00\xff")),
        ),
    ])
    .unwrap()
}

#[tokio::test]
async fn exact_eight_mib_message_roundtrips_across_http2_flow_windows() {
    let server = TestServer::new(echo).await;
    let transport = server.transport().await;
    let payload = Bytes::from(vec![0xa5; DEFAULT_MAX_MESSAGE_BYTES]);
    let mut session = transport
        .start(
            source([payload.clone()]),
            Metadata::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(bounded(collect(&mut session)).await.unwrap(), vec![payload]);
    assert!(session.finish().await.unwrap().upload_eof_polled);
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn observed_final_failure_does_not_poison_reused_transport_close() {
    for observer in 0..3 {
        for observe_after_reaping in [false, true] {
            let server = TestServer::new(|request| async move {
                let mut input = request.into_inner();
                let bytes = input.message().await?.unwrap();
                if bytes == "fail" {
                    Err(Status::data_loss("private terminal failure"))
                } else {
                    Ok(replies(stream::iter([Ok(bytes)])))
                }
            })
            .await;
            let transport = server.transport().await;
            let mut failed = transport
                .start(
                    source_bytes(b"fail"),
                    Metadata::default(),
                    Duration::from_secs(2),
                )
                .unwrap();
            bounded(async {
                while failed.completion().terminal_code != Some(Code::DataLoss)
                    || !failed.completion().source_closed
                {
                    tokio::task::yield_now().await;
                }
            })
            .await;
            // Ensure the final supervisor is finished, so the next start reaps it.
            tokio::task::yield_now().await;
            if !observe_after_reaping {
                let result = match observer {
                    0 => failed.next().await.map(|_| ()),
                    1 => failed.finish().await.map(|_| ()),
                    _ => failed.cancel().await.map(|_| ()),
                };
                assert!(matches!(result, Err(TransportError::Grpc(Code::DataLoss))));
            }
            let mut reused = transport
                .start(
                    source_bytes(b"healthy"),
                    Metadata::default(),
                    Duration::from_secs(2),
                )
                .unwrap();
            assert_eq!(
                bounded(collect(&mut reused)).await.unwrap(),
                vec![Bytes::from_static(b"healthy")]
            );
            if observe_after_reaping {
                let result = match observer {
                    0 => failed.next().await.map(|_| ()),
                    1 => failed.finish().await.map(|_| ()),
                    _ => failed.cancel().await.map(|_| ()),
                };
                assert!(matches!(result, Err(TransportError::Grpc(Code::DataLoss))));
            }
            transport.close().await.unwrap();
            server.close().await;
        }
    }
}

#[tokio::test]
async fn unobserved_late_cleanup_failure_survives_partial_reads_reaping_and_reuse() {
    let server = TestServer::new(|request| async move {
        let mut input = request.into_inner();
        let bytes = input.message().await?.unwrap();
        if bytes == "request" {
            Ok(replies(stream::iter([
                Ok(Bytes::from_static(b"partial")),
                Err(Status::data_loss("private details")),
            ])))
        } else {
            Ok(replies(stream::iter([Ok(bytes)])))
        }
    })
    .await;
    let transport = server.transport().await;
    let mut source = OwnedSource::new();
    source.fail_close = true;
    let started = source.close_started.clone();
    let release = source.release.clone();
    let dropped = source.dropped.clone();
    let closes = source.close_calls.clone();
    let mut failed = transport
        .start(source, Metadata::default(), Duration::from_secs(2))
        .unwrap();
    assert_eq!(
        bounded(failed.next()).await.unwrap(),
        Some(Bytes::from_static(b"partial"))
    );
    bounded(started.notified()).await;
    assert_eq!(failed.completion().terminal_code, Some(Code::DataLoss));
    // Merely polling a terminal read before cleanup completes must not mark the
    // final outcome observed; otherwise the later cleanup error would disappear.
    assert!(
        tokio::time::timeout(Duration::from_millis(20), failed.next())
            .await
            .is_err()
    );
    drop(failed);
    release.add_permits(1);
    bounded(async {
        while dropped.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    tokio::task::yield_now().await;
    let mut reused = transport
        .start(
            source_bytes(b"healthy"),
            Metadata::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(
        bounded(collect(&mut reused)).await.unwrap(),
        vec![Bytes::from_static(b"healthy")]
    );
    assert!(matches!(
        transport.close().await,
        Err(TransportError::Cleanup(_))
    ));
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    transport.close().await.unwrap();
    server.close().await;
}

#[derive(Debug)]
struct CleanupTag(i32);
impl std::fmt::Display for CleanupTag {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}
impl std::error::Error for CleanupTag {}
struct DistinctCleanupSource {
    source: OwnedSource,
    error_id: i32,
}
impl RequestSource for DistinctCleanupSource {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        self.source.next()
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.source.close().await?;
            Err(SourceError::new(CleanupTag(self.error_id)))
        })
    }
}
fn cleanup_error_id(error: TransportError) -> i32 {
    let TransportError::Cleanup(source) = error else {
        panic!("expected a cleanup error, got {error:?}");
    };
    source.original().to_string().parse().unwrap()
}

#[tokio::test]
async fn close_reports_each_unobserved_cleanup_failure_after_reaping_or_interrupted_shutdown() {
    for interrupt_close in [false, true] {
        let server = TestServer::new(echo).await;
        let transport = server.transport().await;
        let mut a = OwnedSource::new();
        a.block_after_first = false;
        let mut b = OwnedSource::new();
        b.block_after_first = false;
        let started = [a.close_started.clone(), b.close_started.clone()];
        let releases = [a.release.clone(), b.release.clone()];
        let dropped = [a.dropped.clone(), b.dropped.clone()];
        let closes = [a.close_calls.clone(), b.close_calls.clone()];
        let a = transport
            .start(
                DistinctCleanupSource {
                    source: a,
                    error_id: 101,
                },
                Metadata::default(),
                Duration::from_secs(2),
            )
            .unwrap();
        let b = transport
            .start(
                DistinctCleanupSource {
                    source: b,
                    error_id: 102,
                },
                Metadata::default(),
                Duration::from_secs(2),
            )
            .unwrap();
        for start in started {
            bounded(start.notified()).await;
        }
        drop((a, b)); // Neither final error is observed through its Session.
        releases[0].add_permits(1);
        if !interrupt_close {
            releases[1].add_permits(1);
        }
        bounded(async {
            while dropped[0].load(Ordering::SeqCst) == 0
                || (!interrupt_close && dropped[1].load(Ordering::SeqCst) == 0)
            {
                tokio::task::yield_now().await;
            }
        })
        .await;
        tokio::task::yield_now().await;
        // Reap completed failures while retaining a healthy shared channel.
        let mut reused = transport
            .start(
                source_bytes(b"healthy"),
                Metadata::default(),
                Duration::from_secs(2),
            )
            .unwrap();
        assert_eq!(
            bounded(collect(&mut reused)).await.unwrap(),
            vec![Bytes::from_static(b"healthy")]
        );
        if interrupt_close {
            // A is already a known failure, B is still in asynchronous cleanup.
            // Cancelling this close must not acknowledge A before the last await.
            assert!(
                tokio::time::timeout(Duration::from_millis(20), transport.close())
                    .await
                    .is_err()
            );
            assert_eq!(dropped[1].load(Ordering::SeqCst), 0);
            releases[1].add_permits(1);
        }
        let mut errors = [
            cleanup_error_id(bounded(transport.close()).await.unwrap_err()),
            cleanup_error_id(bounded(transport.close()).await.unwrap_err()),
        ];
        errors.sort();
        assert_eq!(errors, [101, 102]);
        transport.close().await.unwrap();
        assert!(closes.iter().all(|count| count.load(Ordering::SeqCst) == 1));
        assert!(
            dropped
                .iter()
                .all(|count| count.load(Ordering::SeqCst) == 1)
        );
        server.close().await;
    }
}

// Concurrent close callers share an error ledger, not one error per caller.
// Poll-and-drop injects interruption at an exact cleanup barrier without sleeps.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_interrupted_closes_deliver_each_unobserved_failure_exactly_once() {
    for release_order in [[4, 5], [5, 4]] {
        let server = TestServer::new(echo).await;
        let transport = server.transport().await;
        let mut sessions = Vec::new();
        let mut releases = Vec::new();
        let mut drops = Vec::new();
        let mut closes = Vec::new();
        for id in 0..6 {
            let mut source = OwnedSource::new();
            source.block_after_first = false;
            let started = source.close_started.clone();
            releases.push(source.release.clone());
            drops.push(source.dropped.clone());
            closes.push(source.close_calls.clone());
            let session = transport
                .start(
                    DistinctCleanupSource {
                        source,
                        error_id: id,
                    },
                    Metadata::default(),
                    Duration::from_secs(5),
                )
                .unwrap();
            bounded(started.notified()).await;
            sessions.push(session);
        }
        for release in &releases[..4] {
            release.add_permits(1);
        }
        bounded(async {
            while drops[..4]
                .iter()
                .any(|count| count.load(Ordering::SeqCst) == 0)
            {
                tokio::task::yield_now().await;
            }
        })
        .await;
        // First observe three errors, deliberately leaving error 0 unobserved.
        for (id, session) in sessions.iter_mut().enumerate().take(4).skip(1) {
            assert_eq!(
                cleanup_error_id(bounded(session.finish()).await.unwrap_err()),
                id as i32
            );
        }
        // Starting another session retires completed supervisors. Their error
        // markers must still be shared with Sessions and concurrent closers.
        let mut healthy = transport
            .start(
                source_bytes(b"healthy"),
                Metadata::default(),
                Duration::from_secs(5),
            )
            .unwrap();
        bounded(healthy.finish()).await.unwrap();
        {
            let interrupted = futures_util::future::join_all((0..8).map(|_| transport.close()));
            tokio::pin!(interrupted);
            assert!(futures_util::poll!(interrupted.as_mut()).is_pending());
        }
        assert_eq!(drops[4].load(Ordering::SeqCst), 0);
        assert_eq!(drops[5].load(Ordering::SeqCst), 0);
        let retries = futures_util::future::join_all((0..8).map(|_| transport.close()));
        tokio::pin!(retries);
        assert!(futures_util::poll!(retries.as_mut()).is_pending());
        releases[release_order[0]].add_permits(1);
        bounded(async {
            while drops[release_order[0]].load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await;
        // No close may acknowledge error 0 while the last cleanup is pending.
        assert!(futures_util::poll!(retries.as_mut()).is_pending());
        releases[release_order[1]].add_permits(1);
        let results = bounded(retries).await;
        let mut errors: Vec<_> = results
            .into_iter()
            .filter_map(Result::err)
            .map(cleanup_error_id)
            .collect();
        errors.sort();
        assert_eq!(errors, [0, 4, 5], "release order {release_order:?}");
        // Sessions retain their own outcomes even after transport delivery.
        for (id, session) in sessions.iter_mut().enumerate() {
            assert_eq!(
                cleanup_error_id(bounded(session.cancel()).await.unwrap_err()),
                id as i32
            );
        }
        assert!(closes.iter().all(|count| count.load(Ordering::SeqCst) == 1));
        assert!(drops.iter().all(|count| count.load(Ordering::SeqCst) == 1));
        transport.close().await.unwrap();
        server.close().await;
    }
}

#[tokio::test]
async fn deadline_outcome_survives_cancelled_observation_and_late_cleanup() {
    for fail_close in [false, true] {
        let server = TestServer::new(echo).await;
        let transport = server.transport().await;
        let mut source = OwnedSource::new();
        source.fail_close = fail_close;
        let started = source.close_started.clone();
        let release = source.release.clone();
        let closes = source.close_calls.clone();
        let dropped = source.dropped.clone();
        let provider = Arc::new(GateProvider::new());
        let mut session = transport
            .start_with_provider(source, provider.clone(), Duration::from_millis(40))
            .unwrap();
        // No gRPC request can reach the server, so only our local deadline can
        // cause cleanup (not remote grpc-timeout handling or an early terminal).
        bounded(started.notified()).await;
        assert!(server.paths.lock().unwrap().is_empty());
        assert_eq!(session.completion().terminal_code, None);
        assert!(!session.completion().source_closed);
        {
            let observation = session.cancel();
            tokio::pin!(observation);
            assert!(futures_util::poll!(observation.as_mut()).is_pending());
        }
        drop(session); // Neither polling nor dropping observed a final error.
        {
            let shutdown = transport.close();
            tokio::pin!(shutdown);
            assert!(futures_util::poll!(shutdown.as_mut()).is_pending());
        }
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        release.add_permits(1);
        let error = bounded(transport.close()).await.unwrap_err();
        if fail_close {
            assert!(matches!(error, TransportError::Cleanup(_)));
        } else {
            assert!(matches!(error, TransportError::Timeout));
        }
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        transport.close().await.unwrap();
        server.close().await;
    }
}

#[test]
fn defaults_and_config_restrictions() {
    let config = TransportConfig::default();
    assert_eq!(config.target(), "speechs3proto2-pa.googleapis.com:443");
    assert_eq!(config.method(), DEFAULT_METHOD);
    assert_eq!(config.max_message_bytes(), 8 * 1024 * 1024);
    assert!(config.is_tls());
    assert!(TransportConfig::loopback("192.0.2.1:443".parse().unwrap()).is_err());
    for target in [
        "",
        "https://example.com",
        "secret@example.com:443",
        "host/path",
    ] {
        assert!(TransportConfig::tls(target).is_err(), "{target}");
    }
    for method in [
        "",
        "missing/slash",
        "/service/",
        "//method",
        "/a/b/c",
        "/a/b?secret",
    ] {
        assert!(TransportConfig::default().with_method(method).is_err());
    }
    for limit in [0, i32::MAX as usize + 1] {
        assert!(
            TransportConfig::default()
                .with_max_message_bytes(limit)
                .is_err()
        );
    }
}

#[tokio::test]
async fn exact_path_metadata_bytes_bidirectional_half_close_late_response() {
    let seen = Arc::new(Mutex::new(None));
    let eof = Arc::new(Notify::new());
    let seen_copy = seen.clone();
    let eof_copy = eof.clone();
    let server = TestServer::new(move |request| {
        *seen_copy.lock().unwrap() = Some(request.metadata().clone());
        let eof = eof_copy.clone();
        async move {
            Ok(replies(stream::unfold(
                (request.into_inner(), false),
                move |(mut input, final_sent)| {
                    let eof = eof.clone();
                    async move {
                        if final_sent {
                            return None;
                        }
                        match input.message().await.unwrap() {
                            Some(bytes) => Some((Ok(bytes), (input, false))),
                            None => {
                                eof.notify_one();
                                Some((Ok(Bytes::from_static(b"late-final")), (input, true)))
                            }
                        }
                    }
                },
            )))
        }
    })
    .await;
    let transport = server.transport().await;
    let release = Arc::new(Semaphore::new(0));
    let producer_release = release.clone();
    let input = stream::unfold(0, move |state| {
        let release = producer_release.clone();
        async move {
            match state {
                0 => Some((Ok(Bytes::from_static(b"\x00first\xff")), 1)),
                1 => {
                    release.acquire().await.unwrap().forget();
                    Some((Ok(Bytes::new()), 2))
                }
                _ => None,
            }
        }
    });
    let mut session = transport
        .start(
            StreamSource::new(input),
            fake_metadata("invented"),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(
        bounded(session.next()).await.unwrap(),
        Some(Bytes::from_static(b"\x00first\xff"))
    );
    release.add_permits(1); // Requires reading before completing the upload.
    assert_eq!(
        bounded(collect(&mut session)).await.unwrap(),
        vec![Bytes::new(), Bytes::from_static(b"late-final")]
    );
    bounded(eof.notified()).await;
    let completion = session.finish().await.unwrap();
    assert!(
        completion.source_exhausted && completion.upload_eof_polled && completion.source_closed
    );
    assert_eq!(completion.requests_produced, 2);
    assert_eq!(completion.requests_polled, 2);
    assert_eq!(completion.responses_received, 3);
    assert_eq!(completion.terminal_code, Some(Code::Ok));
    assert_eq!(*server.paths.lock().unwrap(), vec![DEFAULT_METHOD]);
    {
        let metadata = seen.lock().unwrap();
        assert_eq!(
            metadata.as_ref().unwrap().get("x-test").unwrap(),
            "invented"
        );
        assert_eq!(
            metadata
                .as_ref()
                .unwrap()
                .get_bin("opaque-bin")
                .unwrap()
                .to_bytes()
                .unwrap(),
            Bytes::from_static(b"\x00\xff")
        );
    }
    transport.close().await.unwrap();
    server.close().await;
}

struct OwnedSource {
    next_calls: Arc<AtomicUsize>,
    close_calls: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
    close_started: Arc<Notify>,
    close_done: Arc<Notify>,
    release: Arc<Semaphore>,
    step: usize,
    block_after_first: bool,
    fail_next: bool,
    fail_close: bool,
}
impl OwnedSource {
    fn new() -> Self {
        Self {
            next_calls: Arc::new(AtomicUsize::new(0)),
            close_calls: Arc::new(AtomicUsize::new(0)),
            dropped: Arc::new(AtomicUsize::new(0)),
            close_started: Arc::new(Notify::new()),
            close_done: Arc::new(Notify::new()),
            release: Arc::new(Semaphore::new(0)),
            step: 0,
            block_after_first: true,
            fail_next: false,
            fail_close: false,
        }
    }
}
impl RequestSource for OwnedSource {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        self.next_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if self.step == 0 {
                self.step = 1;
                return Ok(Some(Bytes::from_static(b"request")));
            }
            if self.fail_next {
                return Err(SourceError::new(std::io::Error::other(
                    "secret source details",
                )));
            }
            if self.block_after_first {
                std::future::pending::<()>().await;
            }
            Ok(None)
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.close_calls.fetch_add(1, Ordering::SeqCst);
        self.close_started.notify_one();
        Box::pin(async move {
            self.release.acquire().await.unwrap().forget();
            self.close_done.notify_one();
            if self.fail_close {
                Err(SourceError::new(std::io::Error::other(
                    "secret cleanup details",
                )))
            } else {
                Ok(())
            }
        })
    }
}
impl Drop for OwnedSource {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn cancel_before_first_poll_still_closes_source_exactly_once() {
    let server = TestServer::new(echo).await;
    let transport = server.transport().await;
    let source = OwnedSource::new();
    let next = source.next_calls.clone();
    let closes = source.close_calls.clone();
    let dropped = source.dropped.clone();
    source.release.add_permits(1);
    let mut session = transport
        .start(source, Metadata::default(), Duration::from_secs(2))
        .unwrap();
    session.request_cancel(); // No yield: the spawned supervisor has never run.
    assert!(matches!(
        bounded(session.close()).await,
        Err(TransportError::Cancelled)
    ));
    assert_eq!(next.load(Ordering::SeqCst), 0);
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert!(session.completion().source_closed);
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn early_ok_and_late_status_stop_source_while_consumer_paused() {
    for code in [Code::Ok, Code::DataLoss] {
        let server = TestServer::new(move |request| async move {
            let mut input = request.into_inner();
            input.message().await?.unwrap();
            let items = if code == Code::Ok {
                vec![Ok(Bytes::from_static(b"partial"))]
            } else {
                vec![
                    Ok(Bytes::from_static(b"partial")),
                    Err(Status::new(code, "secret remote details")),
                ]
            };
            Ok(replies(stream::iter(items)))
        })
        .await;
        let transport = server.transport().await;
        let source = OwnedSource::new();
        let started = source.close_started.clone();
        let release = source.release.clone();
        let closes = source.close_calls.clone();
        let dropped = source.dropped.clone();
        let mut session = transport
            .start(source, Metadata::default(), Duration::from_secs(2))
            .unwrap();
        assert_eq!(
            bounded(session.next()).await.unwrap(),
            Some(Bytes::from_static(b"partial"))
        );
        bounded(started.notified()).await; // No further consumer poll.
        assert!(!session.completion().source_exhausted);
        assert!(!session.completion().upload_eof_polled);
        assert!(!session.completion().source_closed);
        release.add_permits(1);
        let result = bounded(session.finish()).await;
        if code == Code::Ok {
            assert_eq!(result.unwrap().terminal_code, Some(Code::Ok));
        } else {
            let error = result.unwrap_err();
            assert!(matches!(error, TransportError::Grpc(Code::DataLoss)));
            assert!(!format!("{error:?} {error}").contains("secret"));
        }
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        let _ = transport.close().await;
        server.close().await;
    }
}

#[tokio::test]
async fn normal_eof_waits_for_cleanup_and_never_hides_cleanup_failure() {
    for fail_close in [false, true] {
        let server = TestServer::new(echo).await;
        let transport = server.transport().await;
        let mut source = OwnedSource::new();
        source.block_after_first = false;
        source.fail_close = fail_close;
        let started = source.close_started.clone();
        let release = source.release.clone();
        let closes = source.close_calls.clone();
        let mut session = transport
            .start(source, Metadata::default(), Duration::from_secs(2))
            .unwrap();
        assert!(bounded(session.next()).await.unwrap().is_some());
        bounded(started.notified()).await;
        // The network can finish while source.close is still pending.
        bounded(async {
            while session.completion().terminal_code.is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await;
        assert_eq!(session.completion().terminal_code, Some(Code::Ok));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), session.next())
                .await
                .is_err()
        );
        assert!(!session.completion().source_closed);
        release.add_permits(1);
        let result = bounded(session.finish()).await;
        if fail_close {
            let error = result.unwrap_err();
            assert!(matches!(error, TransportError::Cleanup(_)));
            assert!(!format!("{error:?} {error}").contains("secret"));
        } else {
            assert!(result.unwrap().upload_eof_polled);
        }
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        let _ = transport.close().await;
        server.close().await;
    }
}

#[tokio::test]
async fn cancellation_during_natural_cleanup_can_be_retried_without_interrupting_it() {
    let server = TestServer::new(echo).await;
    let transport = server.transport().await;
    let mut source = OwnedSource::new();
    source.block_after_first = false;
    let started = source.close_started.clone();
    let release = source.release.clone();
    let closes = source.close_calls.clone();
    let dropped = source.dropped.clone();
    let mut session = transport
        .start(source, Metadata::default(), Duration::from_secs(2))
        .unwrap();
    bounded(started.notified()).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(20), session.close())
            .await
            .is_err()
    );
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    release.add_permits(1);
    let _ = bounded(session.close()).await;
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn source_error_is_not_success_and_survives_slow_cleanup() {
    let server = TestServer::new(echo).await;
    let transport = server.transport().await;
    let mut source = OwnedSource::new();
    source.fail_next = true;
    let started = source.close_started.clone();
    let release = source.release.clone();
    let mut session = transport
        .start(source, Metadata::default(), Duration::from_secs(2))
        .unwrap();
    bounded(started.notified()).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(20), session.close())
            .await
            .is_err()
    );
    release.add_permits(1);
    let error = bounded(session.finish()).await.unwrap_err();
    assert!(matches!(error, TransportError::Source(_)));
    assert!(!format!("{error:?} {error}").contains("secret"));
    let mut reused = transport
        .start(
            source_bytes(b"reused"),
            Metadata::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(
        bounded(collect(&mut reused)).await.unwrap(),
        vec![Bytes::from_static(b"reused")]
    );
    let _ = transport.close().await;
    server.close().await;
}
fn source_bytes(bytes: &'static [u8]) -> impl RequestSource {
    source([Bytes::from_static(bytes)])
}

#[tokio::test]
async fn deadline_works_with_paused_consumer_and_blocked_source() {
    let server = TestServer::new(echo).await;
    let transport = server.transport().await;
    let source = OwnedSource::new();
    let started = source.close_started.clone();
    let release = source.release.clone();
    let mut session = transport
        .start(source, Metadata::default(), Duration::from_millis(150))
        .unwrap();
    assert!(bounded(session.next()).await.unwrap().is_some());
    bounded(started.notified()).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(20), session.finish())
            .await
            .is_err()
    );
    release.add_permits(1);
    assert!(matches!(
        bounded(session.finish()).await,
        Err(TransportError::Timeout
            | TransportError::Grpc(Code::Cancelled | Code::DeadlineExceeded))
    ));
    let _ = transport.close().await;
    server.close().await;
}

#[tokio::test]
async fn session_drop_releases_source_but_does_not_close_shared_transport() {
    let server = TestServer::new(echo).await;
    let transport = server.transport().await;
    let source = OwnedSource::new();
    let started = source.close_started.clone();
    let done = source.close_done.clone();
    let release = source.release.clone();
    let dropped = source.dropped.clone();
    let mut session = transport
        .start(source, Metadata::default(), Duration::from_secs(2))
        .unwrap();
    assert!(bounded(session.next()).await.unwrap().is_some());
    drop(session);
    bounded(started.notified()).await;
    release.add_permits(1);
    bounded(done.notified()).await;
    tokio::task::yield_now().await;
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    let mut reused = transport
        .start(
            source_bytes(b"independent"),
            Metadata::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(
        bounded(collect(&mut reused)).await.unwrap(),
        vec![Bytes::from_static(b"independent")]
    );
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn transport_close_waits_for_all_cleanup_even_if_one_fails_and_is_retry_safe() {
    let server = TestServer::new(echo).await;
    let transport = server.transport().await;
    let mut first = OwnedSource::new();
    first.fail_close = true;
    let second = OwnedSource::new();
    let starts = [first.close_started.clone(), second.close_started.clone()];
    let releases = [first.release.clone(), second.release.clone()];
    let drops = [first.dropped.clone(), second.dropped.clone()];
    let _first = transport
        .start(first, Metadata::default(), Duration::from_secs(2))
        .unwrap();
    let _second = transport
        .start(second, Metadata::default(), Duration::from_secs(2))
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(20), transport.close())
            .await
            .is_err()
    );
    for start in starts {
        bounded(start.notified()).await;
    }
    releases[0].add_permits(1);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), transport.close())
            .await
            .is_err()
    );
    releases[1].add_permits(1);
    assert!(matches!(
        bounded(transport.close()).await,
        Err(TransportError::Cleanup(_))
    ));
    assert!(drops.iter().all(|d| d.load(Ordering::SeqCst) == 1));
    assert!(matches!(
        transport.start(
            source_bytes(b"closed"),
            Metadata::default(),
            Duration::from_secs(2)
        ),
        Err(TransportError::Closed)
    ));
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn concurrent_sessions_and_credentials_are_isolated() {
    let server = TestServer::new(|request| async move {
        let tag = request
            .metadata()
            .get("x-test")
            .unwrap()
            .as_bytes()
            .to_vec();
        let mut request = request.into_inner();
        let bytes = request.message().await?.unwrap();
        Ok(replies(stream::iter([Ok(Bytes::from(
            [tag, bytes.to_vec()].concat(),
        ))])))
    })
    .await;
    let transport = server.transport().await;
    let mut sessions = (0..6)
        .map(|index| {
            transport
                .start(
                    source_bytes(b"payload"),
                    fake_metadata(&index.to_string()),
                    Duration::from_secs(2),
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    for (index, session) in sessions.iter_mut().enumerate() {
        assert_eq!(
            bounded(collect(session)).await.unwrap(),
            vec![Bytes::from(format!("{index}payload"))]
        );
    }
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn configured_send_and_receive_limits_are_enforced() {
    for receive in [false, true] {
        let server = TestServer::new(move |request| async move {
            let mut input = request.into_inner();
            if let Some(bytes) = input.message().await? {
                Ok(replies(stream::iter([Ok(if receive {
                    Bytes::from(vec![0; 65])
                } else {
                    bytes
                })])))
            } else {
                Ok(replies(stream::empty()))
            }
        })
        .await;
        let transport = GrpcTransport::connect(
            TransportConfig::loopback(server.address)
                .unwrap()
                .with_max_message_bytes(64)
                .unwrap(),
        )
        .await
        .unwrap();
        let mut session = transport
            .start(
                source([Bytes::from(vec![0; if receive { 1 } else { 65 }])]),
                Metadata::default(),
                Duration::from_secs(2),
            )
            .unwrap();
        let error = bounded(session.finish()).await.unwrap_err();
        if receive {
            assert!(matches!(
                error,
                TransportError::Grpc(Code::OutOfRange | Code::ResourceExhausted)
            ));
        } else {
            assert!(matches!(error, TransportError::MessageTooLarge));
        }
        let _ = transport.close().await;
        server.close().await;
    }
}

#[tokio::test]
async fn unavailable_is_never_retried() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let server = TestServer::new(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        async { Err(Status::unavailable("secret details")) }
    })
    .await;
    let transport = server.transport().await;
    let mut session = transport
        .start(
            source_bytes(b"unreplayable"),
            Metadata::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert!(matches!(
        bounded(session.finish()).await,
        Err(TransportError::Grpc(Code::Unavailable))
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let _ = transport.close().await;
    server.close().await;
}

#[tokio::test]
async fn tls_never_downgrades_to_loopback_plaintext() {
    let server = TestServer::new(echo).await;
    let config = TransportConfig::tls(server.address.to_string())
        .unwrap()
        .with_connect_timeout(Duration::from_millis(150))
        .unwrap();
    assert!(matches!(
        bounded(GrpcTransport::connect(config)).await,
        Err(TransportError::Connect | TransportError::Timeout)
    ));
    assert!(server.paths.lock().unwrap().is_empty());
    server.close().await;
}

struct GateProvider {
    entered: Arc<Notify>,
    release: Arc<Semaphore>,
    calls: Arc<AtomicUsize>,
    fail: bool,
}
impl MetadataProvider for GateProvider {
    fn metadata(&self) -> MetadataFuture<'_> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        Box::pin(async move {
            self.release.acquire().await.unwrap().forget();
            if self.fail {
                Err(TransportError::InvalidMetadata)
            } else {
                Ok(fake_metadata("fake-provider"))
            }
        })
    }
}
impl GateProvider {
    fn new() -> Self {
        Self {
            entered: Arc::new(Notify::new()),
            release: Arc::new(Semaphore::new(0)),
            calls: Arc::new(AtomicUsize::new(0)),
            fail: false,
        }
    }
}

#[tokio::test]
async fn exhausted_source_and_enqueued_final_are_not_upload_eof() {
    let server = TestServer::new(echo).await;
    let transport = server.transport().await;
    let provider = Arc::new(GateProvider::new());
    let mut session = transport
        .start_with_provider(
            source_bytes(b"last-frame"),
            provider.clone(),
            Duration::from_secs(2),
        )
        .unwrap();
    bounded(provider.entered.notified()).await;
    let state = session.completion();
    assert!(state.source_exhausted && state.source_closed);
    assert_eq!(state.requests_produced, 1);
    assert_eq!(state.requests_polled, 0);
    assert!(!state.upload_eof_polled);
    assert!(server.paths.lock().unwrap().is_empty());
    provider.release.add_permits(1);
    assert_eq!(
        bounded(collect(&mut session)).await.unwrap(),
        vec![Bytes::from_static(b"last-frame")]
    );
    assert!(session.finish().await.unwrap().upload_eof_polled);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn request_application_queue_is_capacity_one_without_prefill_requirement() {
    let server = TestServer::new(echo).await;
    let transport = server.transport().await;
    let provider = Arc::new(GateProvider::new());
    let input = source((0..256).map(|_| Bytes::from_static(b"bounded")));
    let mut session = transport
        .start_with_provider(input, provider.clone(), Duration::from_secs(2))
        .unwrap();
    bounded(provider.entered.notified()).await;
    // One item queued, one pending send: no unbounded source prefetch.
    assert_eq!(session.completion().requests_produced, 2);
    assert_eq!(session.completion().requests_polled, 0);
    assert!(!session.completion().source_exhausted);
    assert!(matches!(
        bounded(session.cancel()).await,
        Err(TransportError::Cancelled)
    ));
    assert!(session.completion().source_closed);
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn blocked_provider_deadline_and_provider_failure_both_close_owned_source() {
    for failure in [false, true] {
        let server = TestServer::new(echo).await;
        let transport = server.transport().await;
        let source = OwnedSource::new();
        let started = source.close_started.clone();
        let release = source.release.clone();
        let mut provider = GateProvider::new();
        provider.fail = failure;
        let provider = Arc::new(provider);
        let mut session = transport
            .start_with_provider(source, provider.clone(), Duration::from_millis(80))
            .unwrap();
        bounded(provider.entered.notified()).await;
        if failure {
            provider.release.add_permits(1);
        }
        bounded(started.notified()).await;
        assert!(server.paths.lock().unwrap().is_empty()); // Before any gRPC call.
        release.add_permits(1);
        let error = bounded(session.finish()).await.unwrap_err();
        if failure {
            assert!(matches!(error, TransportError::Credentials));
        } else {
            assert!(matches!(error, TransportError::Timeout));
        }
        assert!(session.completion().source_closed);
        let _ = transport.close().await;
        server.close().await;
    }
}

#[tokio::test]
async fn response_backpressure_is_bounded_and_deadline_still_stops_source() {
    let server = TestServer::new(|mut request| async move {
        request.get_mut().message().await?.unwrap();
        // Retain requests to avoid resetting upload prematurely.
        Ok(replies(stream::unfold(request, |request| async move {
            Some((Ok(Bytes::from_static(b"response")), request))
        })))
    })
    .await;
    let transport = server.transport().await;
    let source = OwnedSource::new();
    let started = source.close_started.clone();
    let release = source.release.clone();
    let mut session = transport
        .start(source, Metadata::default(), Duration::from_millis(100))
        .unwrap();
    bounded(started.notified()).await; // Never read any responses.
    assert_eq!(session.completion().responses_received, 2); // queued + pending send
    release.add_permits(1);
    assert!(matches!(
        bounded(session.finish()).await,
        Err(TransportError::Timeout)
    ));
    let _ = transport.close().await;
    server.close().await;
}

#[tokio::test]
async fn cancelling_one_active_session_preserves_other_active_session() {
    let server = TestServer::new(echo).await;
    let transport = server.transport().await;
    let source = OwnedSource::new();
    source.release.add_permits(1);
    let mut cancelled = transport
        .start(source, Metadata::default(), Duration::from_secs(2))
        .unwrap();
    assert!(bounded(cancelled.next()).await.unwrap().is_some());
    let mut independent = transport
        .start(
            source_bytes(b"independent"),
            Metadata::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert!(matches!(
        bounded(cancelled.cancel()).await,
        Err(TransportError::Cancelled)
    ));
    assert_eq!(
        bounded(collect(&mut independent)).await.unwrap(),
        vec![Bytes::from_static(b"independent")]
    );
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn immediate_source_failure_and_empty_stream_are_not_confused() {
    let server = TestServer::new(echo).await;
    let transport = server.transport().await;
    let input = StreamSource::new(stream::iter([Err(SourceError::new(
        std::io::Error::other("private immediate failure"),
    ))]));
    let mut failed = transport
        .start(input, Metadata::default(), Duration::from_secs(2))
        .unwrap();
    assert!(matches!(
        bounded(failed.next()).await,
        Err(TransportError::Source(_))
    ));
    assert!(matches!(
        failed.finish().await,
        Err(TransportError::Source(_))
    ));
    let mut empty = transport
        .start(source([]), Metadata::default(), Duration::from_secs(2))
        .unwrap();
    assert_eq!(bounded(empty.next()).await.unwrap(), None);
    let done = empty.finish().await.unwrap();
    assert!(done.upload_eof_polled && done.source_exhausted && done.source_closed);
    assert_eq!(done.requests_polled, 0);
    let _ = transport.close().await;
    server.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn early_terminal_racing_continuous_upload_is_supervised() {
    let server = TestServer::new(|mut request| async move {
        request.get_mut().message().await?.unwrap();
        Ok(replies(stream::iter([Ok(Bytes::from_static(b"early"))])))
    })
    .await;
    let transport = server.transport().await;
    for _ in 0..20 {
        let input = StreamSource::new(stream::repeat_with(|| Ok(Bytes::from(vec![0; 64 * 1024]))));
        let mut session = transport
            .start(input, Metadata::default(), Duration::from_secs(2))
            .unwrap();
        let result = bounded(session.finish()).await;
        match result {
            Ok(done) => {
                assert_eq!(done.terminal_code, Some(Code::Ok));
                assert!(!done.source_exhausted);
            }
            // An actual local HTTP/2 reset is never synthesized into OK.
            Err(TransportError::Grpc(_)) => {}
            other => panic!("unexpected outcome {other:?}"),
        }
        assert!(session.completion().source_closed);
    }
    let _ = transport.close().await;
    server.close().await;
}

#[tokio::test]
async fn custom_method_and_final_transport_drop_are_explicit() {
    let server = TestServer::new(echo).await;
    let config = TransportConfig::loopback(server.address)
        .unwrap()
        .with_method("/speech.s3.S3StubbyClientConnectorService/Custom")
        .unwrap();
    let transport = GrpcTransport::connect(config).await.unwrap();
    let mut session = transport
        .start(
            source_bytes(b"custom"),
            Metadata::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(
        bounded(collect(&mut session)).await.unwrap(),
        vec![Bytes::from_static(b"custom")]
    );
    assert_eq!(
        *server.paths.lock().unwrap(),
        vec!["/speech.s3.S3StubbyClientConnectorService/Custom"]
    );
    let source = OwnedSource::new();
    let dropped = source.dropped.clone();
    let closes = source.close_calls.clone();
    let mut active = transport
        .start(source, Metadata::default(), Duration::from_secs(2))
        .unwrap();
    assert!(bounded(active.next()).await.unwrap().is_some());
    drop(transport); // Async cleanup cannot be promised by Rust Drop.
    assert!(matches!(
        bounded(active.finish()).await,
        Err(TransportError::Supervisor)
    ));
    assert_eq!(dropped.load(Ordering::SeqCst), 1); // No detached producer leak.
    assert_eq!(closes.load(Ordering::SeqCst), 0);
    server.close().await;
}

#[tokio::test]
async fn native_http2_backpressure_does_not_exhaust_source() {
    let started = Arc::new(Notify::new());
    let server_started = started.clone();
    let server = TestServer::new(move |request| {
        server_started.notify_one();
        async move {
            Ok(replies(stream::unfold(request, |request| async move {
                std::future::pending::<()>().await;
                Some((Ok(Bytes::new()), request))
            })))
        }
    })
    .await;
    let transport = server.transport().await;
    let chunk = Bytes::from(vec![0; 256 * 1024]);
    let input = StreamSource::new(stream::iter((0..256).map(move |_| Ok(chunk.clone()))));
    let mut session = transport
        .start(input, Metadata::default(), Duration::from_secs(2))
        .unwrap();
    bounded(started.notified()).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let state = session.completion();
    assert!(state.requests_produced > 0 && state.requests_produced < 256);
    assert!(!state.source_exhausted);
    assert!(matches!(
        bounded(session.cancel()).await,
        Err(TransportError::Cancelled)
    ));
    assert!(session.completion().source_closed);
    transport.close().await.unwrap();
    bounded(server.close()).await;
}
