//! Offline client contract only: invented metadata and synthetic message bytes.
//! A peer denial does not refresh credentials, replay an RPC, or reconnect. Only
//! a later explicit start snapshots caller-replaced metadata on the same channel.
//! This does not qualify real credential revocation or durable provisioning.
use bytes::Bytes;
use futures_util::stream;
use rambler_transport::{
    Completion, CredentialValue, DEFAULT_METHOD, GrpcTransport, Metadata, MetadataFuture,
    MetadataProvider, RequestSource, SourceError, SourceFuture, StreamSource, TransportConfig,
    TransportError,
};
use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::Semaphore,
    task::{JoinHandle, JoinSet},
};
use tonic::Code;

const STEP: Duration = Duration::from_secs(5);
const WHOLE: Duration = Duration::from_secs(20);

struct ReplacedProvider {
    current: Mutex<Metadata>,
    calls: AtomicUsize,
}
impl ReplacedProvider {
    fn new(generation: &str) -> Arc<Self> {
        Arc::new(Self {
            current: Mutex::new(invented_metadata(generation)),
            calls: AtomicUsize::new(0),
        })
    }
}
impl MetadataProvider for ReplacedProvider {
    fn metadata(&self) -> MetadataFuture<'_> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let snapshot = self.current.lock().unwrap().clone();
        Box::pin(async move { Ok(snapshot) })
    }
}

fn invented_metadata(generation: &str) -> Metadata {
    Metadata::new([
        (
            "authorization".into(),
            CredentialValue::Text(format!("invented-{generation}-authorization")),
        ),
        (
            "x-goog-api-key".into(),
            CredentialValue::Text(format!("invented-{generation}-key")),
        ),
        (
            "proof-bin".into(),
            CredentialValue::Binary(Bytes::from(format!("\0invented-{generation}\u{7f}"))),
        ),
    ])
    .unwrap()
}

fn assert_metadata(headers: &http::HeaderMap, generation: &str) {
    for key in ["authorization", "x-goog-api-key", "proof-bin"] {
        assert_eq!(headers.get_all(key).iter().count(), 1);
    }
    // Exact equality and cardinality exclude old/duplicate fake values on the
    // replacement RPC. Decoded h2 headers do not retain HPACK sensitivity flags;
    // the production Metadata constructor marks these values sensitive.
    assert_eq!(
        headers["authorization"],
        format!("invented-{generation}-authorization")
    );
    assert_eq!(
        headers["x-goog-api-key"],
        format!("invented-{generation}-key")
    );
    let metadata = tonic::metadata::MetadataMap::from_headers(headers.clone());
    assert_eq!(
        metadata.get_bin("proof-bin").unwrap().to_bytes().unwrap(),
        Bytes::from(format!("\0invented-{generation}\u{7f}")),
    );
}

#[derive(Clone, Default)]
struct SourceCounts {
    closes: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
impl SourceCounts {
    fn source(&self, bytes: &'static [u8]) -> TrackedSource {
        TrackedSource {
            counts: self.clone(),
            message: Some(Bytes::from_static(bytes)),
        }
    }
    fn assert_cleaned(&self) {
        assert_eq!(self.closes.load(Ordering::SeqCst), 1);
        assert_eq!(self.drops.load(Ordering::SeqCst), 1);
    }
}
struct TrackedSource {
    counts: SourceCounts,
    message: Option<Bytes>,
}
impl RequestSource for TrackedSource {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        let message = self.message.take();
        Box::pin(async move { Ok(message) })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.counts.closes.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}
impl Drop for TrackedSource {
    fn drop(&mut self) {
        self.counts.drops.fetch_add(1, Ordering::SeqCst);
    }
}

// A timed-out/panicking scenario must not leave the peer detached. Its JoinSet
// similarly aborts any still-owned stream handlers when the peer is dropped.
struct PeerTask(JoinHandle<usize>);
impl PeerTask {
    async fn join(mut self) -> usize {
        bounded(&mut self.0).await.unwrap()
    }
}
impl Drop for PeerTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn bounded<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(STEP, future)
        .await
        .expect("credential denial scenario step exceeded deadline")
}
fn frame(bytes: &[u8]) -> Bytes {
    let mut framed = vec![0];
    framed.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    framed.extend_from_slice(bytes);
    Bytes::from(framed)
}
fn response_headers() -> http::Response<()> {
    http::Response::builder()
        .header("content-type", "application/grpc")
        .body(())
        .unwrap()
}
fn trailers(code: Code) -> http::HeaderMap {
    let mut trailers = http::HeaderMap::new();
    trailers.insert("grpc-status", (code as i32).to_string().parse().unwrap());
    if code != Code::Ok {
        trailers.insert(
            "grpc-message",
            "invented-private-denial-details".parse().unwrap(),
        );
    }
    trailers
}
async fn assert_upload(mut request: http::Request<h2::RecvStream>, expected: &'static [u8]) {
    let mut uploaded = Vec::new();
    while let Some(data) = request.body_mut().data().await {
        let data = data.unwrap();
        uploaded.extend_from_slice(&data);
        request
            .body_mut()
            .flow_control()
            .release_capacity(data.len())
            .unwrap();
    }
    assert!(request.body_mut().trailers().await.unwrap().is_none());
    assert_eq!(uploaded.as_slice(), frame(expected).as_ref());
}
fn assert_completion(completion: &Completion, code: Code, responses: u64) {
    assert_eq!(completion.terminal_code, Some(code));
    assert!(completion.source_exhausted && completion.upload_eof_polled);
    assert!(completion.source_closed);
    assert_eq!(completion.requests_produced, 1);
    assert_eq!(completion.requests_polled, 1);
    assert_eq!(completion.responses_received, responses);
}

async fn replacement_after_denial(code: Code) {
    tokio::time::timeout(WHOLE, async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let neighbour_progress = Arc::new(Semaphore::new(0));
        let neighbour_finish = Arc::new(Semaphore::new(0));
        let progress = neighbour_progress.clone();
        let finish = neighbour_finish.clone();
        let peer = PeerTask(tokio::spawn(async move {
            let (tcp, remote) = listener.accept().await.unwrap();
            assert!(remote.ip().is_loopback());
            // Keep the listener observable until the original connection closes:
            // a second TCP connection must fail the scenario, not go unnoticed.
            tcp.set_nodelay(true).unwrap();
            let mut connection = h2::server::handshake(tcp).await.unwrap();
            let mut handlers = JoinSet::new();
            let mut requests = 0;
            loop {
                tokio::select! {
                    biased;
                    _ = listener.accept() => panic!("unexpected second TCP connection"),
                    result = handlers.join_next(), if !handlers.is_empty() => {
                        result.unwrap().unwrap();
                    }
                    request = connection.accept() => {
                        let Some(request) = request else { break };
                        let (request, mut reply) = request.unwrap();
                        requests += 1;
                        assert!(requests <= 3, "automatic retry/replay or extra RPC");
                        assert_eq!(request.method(), http::Method::POST);
                        assert_eq!(request.uri().path(), DEFAULT_METHOD);
                        assert_eq!(
                            u32::from(request.body().stream_id()),
                            u32::try_from(2 * requests - 1).unwrap(),
                        );
                        let generation = match requests {
                            1 => "neighbour",
                            2 => "prior",
                            3 => "replacement",
                            _ => unreachable!(),
                        };
                        assert_metadata(request.headers(), generation);
                        let progress = progress.clone();
                        let finish = finish.clone();
                        handlers.spawn(async move {
                            let expected: &'static [u8] = match generation {
                                "neighbour" => b"synthetic-neighbour-request",
                                "prior" => b"synthetic-denied-request",
                                _ => b"synthetic-explicit-replacement-request",
                            };
                            // Complete upload before denial: this contract is
                            // credential replacement, not an early-upload race.
                            assert_upload(request, expected).await;
                            let mut response = reply.send_response(response_headers(), false).unwrap();
                            match generation {
                                "neighbour" => {
                                    response.send_data(frame(b"neighbour-before-denial"), false).unwrap();
                                    progress.acquire().await.unwrap().forget();
                                    response.send_data(frame(b"neighbour-after-denial"), false).unwrap();
                                    finish.acquire().await.unwrap().forget();
                                    response.send_data(frame(b"neighbour-after-replacement"), false).unwrap();
                                    response.send_trailers(trailers(Code::Ok)).unwrap();
                                }
                                "prior" => response.send_trailers(trailers(code)).unwrap(),
                                "replacement" => {
                                    response.send_data(frame(b"replacement-ok"), false).unwrap();
                                    response.send_trailers(trailers(Code::Ok)).unwrap();
                                }
                                _ => unreachable!(),
                            }
                        });
                    }
                }
            }
            while let Some(result) = handlers.join_next().await {
                result.unwrap();
            }
            requests
        }));
        let transport = bounded(GrpcTransport::connect(
            TransportConfig::loopback(address).unwrap(),
        ))
        .await
        .unwrap();
        let provider = ReplacedProvider::new("prior");
        let neighbour_provider = ReplacedProvider::new("neighbour");
        let neighbour_counts = SourceCounts::default();
        let denied_counts = SourceCounts::default();
        let replacement_counts = SourceCounts::default();
        let mut neighbour = transport
            .start_with_provider(
                neighbour_counts.source(b"synthetic-neighbour-request"),
                neighbour_provider.clone(),
                WHOLE,
            )
            .unwrap();
        assert_eq!(
            bounded(neighbour.next()).await.unwrap(),
            Some(Bytes::from_static(b"neighbour-before-denial")),
        );
        let mut denied = transport
            .start_with_provider(
                denied_counts.source(b"synthetic-denied-request"),
                provider.clone(),
                WHOLE,
            )
            .unwrap();
        let error = bounded(denied.next()).await.unwrap_err();
        assert!(matches!(error, TransportError::Grpc(actual) if actual == code));
        assert!(!format!("{error:?} {error}").contains("invented-"));
        assert!(matches!(
            bounded(denied.finish()).await,
            Err(TransportError::Grpc(actual)) if actual == code
        ));
        assert_completion(&denied.completion(), code, 0);
        denied_counts.assert_cleaned();
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(neighbour.completion().terminal_code, None);

        // Swap caller-owned state, then advance the existing neighbour before
        // explicitly starting. The snapshot count is a consistency check; the
        // peer's exact stream/body counts detect any replay in this scenario.
        *provider.current.lock().unwrap() = invented_metadata("replacement");
        neighbour_progress.add_permits(1);
        assert_eq!(
            bounded(neighbour.next()).await.unwrap(),
            Some(Bytes::from_static(b"neighbour-after-denial")),
        );
        assert_eq!(neighbour.completion().terminal_code, None);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(neighbour_provider.calls.load(Ordering::SeqCst), 1);

        let mut replacement = transport
            .start_with_provider(
                replacement_counts.source(b"synthetic-explicit-replacement-request"),
                provider.clone(),
                WHOLE,
            )
            .unwrap();
        assert_eq!(
            bounded(replacement.next()).await.unwrap(),
            Some(Bytes::from_static(b"replacement-ok")),
        );
        assert!(bounded(replacement.next()).await.unwrap().is_none());
        let completion = bounded(replacement.finish()).await.unwrap();
        assert_completion(&completion, Code::Ok, 1);
        replacement_counts.assert_cleaned();
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
        assert_eq!(neighbour.completion().terminal_code, None);

        neighbour_finish.add_permits(1);
        assert_eq!(
            bounded(neighbour.next()).await.unwrap(),
            Some(Bytes::from_static(b"neighbour-after-replacement")),
        );
        assert!(bounded(neighbour.next()).await.unwrap().is_none());
        let completion = bounded(neighbour.finish()).await.unwrap();
        assert_completion(&completion, Code::Ok, 3);
        neighbour_counts.assert_cleaned();
        // The old Session retains its denial after provider replacement/reaping.
        assert!(matches!(
            bounded(denied.finish()).await,
            Err(TransportError::Grpc(actual)) if actual == code
        ));
        bounded(transport.close()).await.unwrap();
        assert_eq!(peer.join().await, 3, "one RPC per explicit session");
        bounded(transport.close()).await.unwrap();
        assert!(matches!(
            transport.start_with_provider(
                StreamSource::new(stream::empty()),
                provider.clone(),
                STEP,
            ),
            Err(TransportError::Closed)
        ));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
        assert_eq!(neighbour_provider.calls.load(Ordering::SeqCst), 1);
        for counts in [&denied_counts, &replacement_counts, &neighbour_counts] {
            counts.assert_cleaned();
        }
    })
    .await
    .expect("whole credential denial scenario exceeded deadline");
}

#[tokio::test]
async fn unauthenticated_requires_explicit_replacement_and_preserves_live_neighbour() {
    replacement_after_denial(Code::Unauthenticated).await;
}

#[tokio::test]
async fn permission_denied_requires_explicit_replacement_and_preserves_live_neighbour() {
    replacement_after_denial(Code::PermissionDenied).await;
}
