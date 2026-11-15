//! Reusable direct, verified-TLS, raw-byte bidirectional gRPC transport.
//!
//! No S3 parsing or success verdict, retries, or audio replay. Exhaust `Session::next`
//! and call `finish` to observe terminal status **and** producer cleanup. An early
//! remote OK is transport OK even when upload was incomplete: inspect `Completion`.
//!
//! One supervised task owns each source, writer and reader; application queues have
//! capacity one. The source must not block a runtime thread, and `next` must be
//! cancellation-safe: its future can be dropped. `close` is called exactly once,
//! even before the first `next` poll, and is never cancelled by ordinary session
//! cancellation, timeout or remote completion. No `next` is called until metadata
//! succeeds, h2 is ready, and local `send_request` succeeds. This admission is not
//! server/auth acceptance or proof of delivery; no replay/retry is performed.
//! `close` must eventually finish. Deadline
//! bounds RPC work, not cooperative asynchronous cleanup. A paused consumer applies
//! bounded response backpressure: terminal trailers behind multiple queued responses
//! can only be observed after draining; the independent deadline still stops upload.
//! Sources/providers must not panic. Internal HTTP/2 flow buffers are separate
//! from the two capacity-one application queues (plus one in-flight item each).
//!
//! The client uses h2 + ring/rustls with OS trust roots and hostname verification.
//! Each RPC owns an explicit HTTP/2 CANCEL reset guard: cancelling a blocked upload
//! releases its stream without closing the shared connection or sibling sessions.
//! A tonic client was tested first, but dropping its response left a native upload
//! pipe alive under peer flow-control starvation; direct stream ownership avoids
//! that leak. The public tonic Bytes codec remains available for generic peers.
//!
//! Dropping a Session signals cancellation; its task stays tracked by the transport.
//! `Session::close/cancel` and `GrpcTransport::close` wait for cleanup. Async shutdown
//! futures may themselves be cancelled and retried without losing task ownership.
//! Dropping the final transport signals cancellation and aborts remaining tasks:
//! Rust Drop cannot await asynchronous cleanup. Call `close` before that point for
//! resource cleanup guarantees, and keep the Tokio runtime alive until it returns.
//! A transport is caller-owned; future engines must not close an injected transport.
//!
//! A connection that dies or receives GOAWAY is never reconnected automatically.
//! `start` can still accept a source on that channel: the session reports the h2
//! failure and closes its source, without retry or replay. Recover only for new
//! sessions by explicitly calling `GrpcTransport::connect(config.clone())` and
//! retaining the returned independent transport. Existing sessions and clones are
//! not moved; a permanently closed old transport stays closed. Keep the config if
//! replacement is needed, and observe each old session before closing the old
//! transport. No idle keepalive/health guarantee is provided; an RPC deadline also
//! bounds detection of an unresponsive peer, not its asynchronous source cleanup.

pub mod codec;
mod connection;
pub mod credentials;
mod wire;

use bytes::Bytes;
pub use codec::RawBytesCodec;
pub use credentials::{
    CredentialValue, EnvironmentCredentials, Metadata, MetadataFuture, MetadataProvider,
};
use futures_util::{Stream, StreamExt};
use std::{
    fmt,
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    sync::{Mutex as AsyncMutex, mpsc, oneshot, watch},
    task::JoinHandle,
    time::Instant,
};
use tokio_util::sync::CancellationToken;
use tonic::Code;

pub const DEFAULT_TARGET: &str = "speechs3proto2-pa.googleapis.com:443";
pub const DEFAULT_METHOD: &str = "/speech.s3.S3StubbyClientConnectorService/S3Bidi";
pub const DEFAULT_MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

/// Caller error retained only behind an explicit accessor, never an error chain.
#[derive(Clone)]
pub struct SourceError(Arc<dyn std::error::Error + Send + Sync>);
impl SourceError {
    pub fn new(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self(Arc::new(error))
    }
    /// Explicit opt-in to the concrete caller error. The object is `'static`,
    /// but this reference still borrows `self`; this permits safe downcasting.
    pub fn original(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
        self.0.as_ref()
    }
}
impl fmt::Debug for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SourceError([redacted])")
    }
}
impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("request source failed")
    }
}
impl std::error::Error for SourceError {}

/// All formatting is fixed text / status code only: never tonic status details,
/// metadata, credentials, endpoint text, or caller error formatting.
#[derive(Clone, Debug, thiserror::Error)]
pub enum TransportError {
    #[error("invalid transport configuration")]
    InvalidConfig,
    #[error("invalid credential metadata")]
    InvalidMetadata,
    #[error("all three environment credentials must be supplied together")]
    IncompleteCredentials,
    #[error("credential provider failed")]
    Credentials,
    #[error("transport connection failed")]
    Connect,
    #[error("transport is closed")]
    Closed,
    #[error("session cancelled")]
    Cancelled,
    #[error("session deadline exceeded")]
    Timeout,
    #[error("gRPC failed ({0:?})")]
    Grpc(Code),
    #[error("request message exceeds configured limit")]
    MessageTooLarge,
    #[error("request source failed")]
    Source(SourceError),
    #[error("request source cleanup failed")]
    Cleanup(SourceError),
    /// A selected primary failure and a distinct failed source-close attempt.
    #[error("session failed and request source cleanup failed")]
    PrimaryAndCleanup(Box<PrimaryAndCleanup>),
    #[error("invalid gRPC response framing or terminal status")]
    Protocol,
    #[error("gRPC response compression is unsupported")]
    CompressionUnsupported,
    #[error("session supervisor failed")]
    Supervisor,
}

/// Double failure with explicit, opt-in access to both causes. Construction is
/// private: the primary is never cancellation, cleanup, or another composite.
/// Formatting and the automatic error chain never expose either caller error.
#[derive(Clone)]
pub struct PrimaryAndCleanup {
    primary: TransportError,
    cleanup: SourceError,
}
impl PrimaryAndCleanup {
    fn new(primary: TransportError, cleanup: SourceError) -> Self {
        debug_assert!(!matches!(
            primary,
            TransportError::Cancelled
                | TransportError::Cleanup(_)
                | TransportError::PrimaryAndCleanup(_)
        ));
        Self { primary, cleanup }
    }
    pub fn primary(&self) -> &TransportError {
        &self.primary
    }
    pub fn cleanup(&self) -> &SourceError {
        &self.cleanup
    }
    pub fn into_parts(self) -> (TransportError, SourceError) {
        (self.primary, self.cleanup)
    }
}
impl fmt::Debug for PrimaryAndCleanup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PrimaryAndCleanup([redacted])")
    }
}
impl fmt::Display for PrimaryAndCleanup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("session failed and request source cleanup failed")
    }
}
impl std::error::Error for PrimaryAndCleanup {}

pub type SourceFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
/// Owned per session. See crate-level cancellation and async cleanup contract.
pub trait RequestSource: Send + 'static {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>>;
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async { Ok(()) })
    }
}
/// Adapter for streams whose synchronous Drop is sufficient for cleanup.
/// For asynchronous resource release, implement RequestSource instead.
pub struct StreamSource<S>(Pin<Box<S>>);
impl<S> StreamSource<S> {
    pub fn new(stream: S) -> Self {
        Self(Box::pin(stream))
    }
}
impl<S: Stream<Item = Result<Bytes, SourceError>> + Send + 'static> RequestSource
    for StreamSource<S>
{
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async { self.0.next().await.transpose() })
    }
}

#[derive(Clone)]
pub struct TransportConfig {
    target: String,
    port: u16,
    method: http::uri::PathAndQuery,
    max_message_bytes: usize,
    connect_timeout: Duration,
    plaintext_loopback: bool,
}
impl fmt::Debug for TransportConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TransportConfig([endpoint redacted])")
    }
}
impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            target: DEFAULT_TARGET.into(),
            port: 443,
            method: http::uri::PathAndQuery::from_static(DEFAULT_METHOD),
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            connect_timeout: Duration::from_secs(10),
            plaintext_loopback: false,
        }
    }
}
impl TransportConfig {
    /// `Host[:port]`, not a URL. An absent port means 443; explicit destination
    /// ports must be decimal 1..=65535 (including bracketed IPv6 authorities).
    /// TLS roots come from the OS, with hostname verification.
    pub fn tls(target: impl Into<String>) -> Result<Self, TransportError> {
        let target = target.into();
        let authority = target
            .parse::<http::uri::Authority>()
            .map_err(|_| TransportError::InvalidConfig)?;
        if authority.host().is_empty() || target.contains('@') {
            return Err(TransportError::InvalidConfig);
        }
        let suffix = &target[authority.host().len()..];
        let port = if suffix.is_empty() {
            443
        } else {
            let value = suffix
                .strip_prefix(':')
                .ok_or(TransportError::InvalidConfig)?;
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(TransportError::InvalidConfig);
            }
            let port = value
                .parse::<u16>()
                .map_err(|_| TransportError::InvalidConfig)?;
            if port == 0 {
                return Err(TransportError::InvalidConfig);
            }
            port
        };
        Ok(Self {
            target,
            port,
            ..Self::default()
        })
    }
    /// Explicit plaintext escape hatch restricted to numeric loopback addresses.
    pub fn loopback(address: SocketAddr) -> Result<Self, TransportError> {
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(TransportError::InvalidConfig);
        }
        Ok(Self {
            target: address.to_string(),
            port: address.port(),
            plaintext_loopback: true,
            ..Self::default()
        })
    }
    pub fn with_method(mut self, method: &str) -> Result<Self, TransportError> {
        let parts: Vec<_> = method.split('/').collect();
        if parts.len() != 3
            || !parts[0].is_empty()
            || parts[1..].iter().any(|v| v.is_empty())
            || method.contains(['?', '#', '%'])
        {
            return Err(TransportError::InvalidConfig);
        }
        self.method = method.parse().map_err(|_| TransportError::InvalidConfig)?;
        Ok(self)
    }
    pub fn with_max_message_bytes(mut self, limit: usize) -> Result<Self, TransportError> {
        if limit == 0 || limit > i32::MAX as usize {
            return Err(TransportError::InvalidConfig);
        }
        self.max_message_bytes = limit;
        Ok(self)
    }
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Result<Self, TransportError> {
        if timeout.is_zero() || Instant::now().checked_add(timeout).is_none() {
            return Err(TransportError::InvalidConfig);
        }
        self.connect_timeout = timeout;
        Ok(self)
    }
    pub fn target(&self) -> &str {
        &self.target
    }
    pub fn method(&self) -> &str {
        self.method.as_str()
    }
    pub fn max_message_bytes(&self) -> usize {
        self.max_message_bytes
    }
    pub fn is_tls(&self) -> bool {
        !self.plaintext_loopback
    }
}

/// Observation of local gRPC writer polling, not acknowledgement by the peer.
/// `requests_polled` counts frames dequeued by the h2 writer, before complete DATA
/// submission. `upload_eof_polled` means it drained every frame, observed source
/// queue EOF and successfully submitted END_STREAM to h2. It is stronger than
/// enqueue/exhaustion, but is not proof of network delivery or any S3 meaning.
/// `source_closed` means
/// the single close attempt finished, even if it returned a cleanup error.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Completion {
    pub requests_produced: u64,
    pub requests_polled: u64,
    pub responses_received: u64,
    pub source_exhausted: bool,
    pub upload_eof_polled: bool,
    pub source_closed: bool,
    pub terminal_code: Option<Code>,
}
#[derive(Default)]
struct Progress {
    produced: AtomicU64,
    polled: AtomicU64,
    responses: AtomicU64,
    exhausted: AtomicBool,
    eof: AtomicBool,
    closed: AtomicBool,
    terminal: Mutex<Option<Code>>,
}
impl Progress {
    fn snapshot(&self) -> Completion {
        Completion {
            requests_produced: self.produced.load(Ordering::SeqCst),
            requests_polled: self.polled.load(Ordering::SeqCst),
            responses_received: self.responses.load(Ordering::SeqCst),
            source_exhausted: self.exhausted.load(Ordering::SeqCst),
            upload_eof_polled: self.eof.load(Ordering::SeqCst),
            source_closed: self.closed.load(Ordering::SeqCst),
            terminal_code: *self.terminal.lock().unwrap(),
        }
    }
}
struct Upload {
    receiver: mpsc::Receiver<Bytes>,
    progress: Arc<Progress>,
}
impl Stream for Upload {
    type Item = Bytes;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Bytes>> {
        let result = self.receiver.poll_recv(cx);
        match &result {
            Poll::Ready(Some(_)) => {
                self.progress.polled.fetch_add(1, Ordering::SeqCst);
            }
            Poll::Ready(None) | Poll::Pending => {}
        }
        result
    }
}
type Outcome = Result<Completion, TransportError>;
struct Control {
    cancel: CancellationToken,
    progress: Arc<Progress>,
    done: watch::Receiver<Option<Outcome>>,
    observed: Arc<AtomicBool>,
    task: AsyncMutex<Option<JoinHandle<()>>>,
}
impl Control {
    async fn wait(&self) -> Outcome {
        let mut done = self.done.clone();
        loop {
            let value = done.borrow_and_update().clone();
            if let Some(value) = value {
                return value;
            }
            if done.changed().await.is_err() {
                return Err(TransportError::Supervisor);
            }
        }
    }
    async fn join(&self) -> Outcome {
        let mut slot = self.task.lock().await;
        if let Some(task) = slot.as_mut() {
            if task.await.is_err() {
                slot.take();
                return Err(TransportError::Supervisor);
            }
            slot.take();
        }
        self.wait().await
    }
}
impl Drop for Control {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(task) = self.task.get_mut().take() {
            task.abort();
        }
    }
}
#[derive(Clone)]
struct RetiredFailure {
    error: TransportError,
    observed: Arc<AtomicBool>,
}
struct Lifecycle {
    closed: bool,
    channel: Option<h2::client::SendRequest<Bytes>>,
    sessions: Vec<Arc<Control>>,
    retired_failures: Vec<RetiredFailure>,
}
struct Inner {
    config: TransportConfig,
    network: connection::Driver,
    lifecycle: Mutex<Lifecycle>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        for session in &self.lifecycle.get_mut().unwrap().sessions {
            session.cancel.cancel();
            // No detached tasks when the final transport owner disappears.
            if let Ok(slot) = session.task.try_lock()
                && let Some(task) = slot.as_ref()
            {
                task.abort();
            }
        }
    }
}
/// Clones share one channel/lifecycle. Closing one closes the shared transport;
/// closing/cancelling a Session never closes any other session or the channel.
#[derive(Clone)]
pub struct GrpcTransport {
    inner: Arc<Inner>,
}
impl GrpcTransport {
    pub async fn connect(config: TransportConfig) -> Result<Self, TransportError> {
        let connected = tokio::time::timeout(config.connect_timeout, connection::connect(&config))
            .await
            .map_err(|_| TransportError::Timeout)??;
        Ok(Self {
            inner: Arc::new(Inner {
                config,
                network: connected.driver,
                lifecycle: Mutex::new(Lifecycle {
                    closed: false,
                    channel: Some(connected.sender),
                    sessions: Vec::new(),
                    retired_failures: Vec::new(),
                }),
            }),
        })
    }
    /// Start immediately on the current Tokio runtime. Ownership/async-close
    /// guarantees apply once Ok(Session) is returned. Invalid timeout or a closed
    /// transport rejects the source synchronously (ordinary Rust Drop only).
    pub fn start<S: RequestSource>(
        &self,
        source: S,
        metadata: Metadata,
        timeout: Duration,
    ) -> Result<Session, TransportError> {
        self.start_with_provider(source, Arc::new(metadata), timeout)
    }
    pub fn start_with_provider<S: RequestSource>(
        &self,
        source: S,
        provider: Arc<dyn MetadataProvider>,
        timeout: Duration,
    ) -> Result<Session, TransportError> {
        if timeout.is_zero() {
            return Err(TransportError::InvalidConfig);
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(TransportError::InvalidConfig)?;
        let mut lifecycle = self.inner.lifecycle.lock().unwrap();
        if lifecycle.closed {
            return Err(TransportError::Closed);
        }
        // Reap only finished tasks (never detach a live task). Keep only
        // unobserved final failures, with a marker shared with surviving Sessions
        // so that observation after reaping also prevents duplicate close errors.
        lifecycle
            .retired_failures
            .retain(|failure| !failure.observed.load(Ordering::SeqCst));
        let mut retired_failures = Vec::new();
        lifecycle.sessions.retain(|session| {
            let Ok(mut slot) = session.task.try_lock() else {
                return true;
            };
            if slot.as_ref().is_some_and(|task| !task.is_finished()) {
                return true;
            }
            slot.take();
            let result = session
                .done
                .borrow()
                .clone()
                .unwrap_or(Err(TransportError::Supervisor));
            if let Err(error) = result
                && !matches!(error, TransportError::Cancelled)
                && !session.observed.load(Ordering::SeqCst)
            {
                retired_failures.push(RetiredFailure {
                    error,
                    observed: session.observed.clone(),
                });
            }
            false
        });
        lifecycle.retired_failures.extend(retired_failures);
        let progress = Arc::new(Progress::default());
        let cancel = CancellationToken::new();
        let (done_tx, done) = watch::channel(None);
        let (responses_tx, responses) = mpsc::channel(1);
        let control = Arc::new(Control {
            cancel: cancel.clone(),
            progress: progress.clone(),
            done,
            observed: Arc::new(AtomicBool::new(false)),
            task: AsyncMutex::new(None),
        });
        let channel = lifecycle
            .channel
            .as_ref()
            .ok_or(TransportError::Closed)?
            .clone();
        let config = self.inner.config.clone();
        let task = tokio::spawn(async move {
            let result = supervise(
                source,
                provider,
                channel,
                config,
                deadline,
                cancel,
                progress.clone(),
                responses_tx,
            )
            .await;
            let _ = done_tx.send(Some(result.map(|()| progress.snapshot())));
        });
        *control
            .task
            .try_lock()
            .expect("new task slot is uncontended") = Some(task);
        lifecycle.sessions.push(control.clone());
        Ok(Session {
            control,
            responses,
            terminal_delivered: false,
        })
    }
    /// Permanent shutdown, idempotent. Cancels every session and waits for every
    /// cleanup even if one fails. Returns one unobserved final failure per call;
    /// subsequent closes expose remaining failures before returning Ok. Only the
    /// error actually returned is acknowledged, after every cleanup await.
    /// Includes failures of dropped sessions, but never repeats a final failure
    /// already observed through Session::next/finish/cancel/close. The independent
    /// driver error remains reportable, even if a session error may share its
    /// cause: h2 provides no identity for safe cross-layer deduplication. Partial reads
    /// and cancelled consumer futures do not mark a final outcome observed.
    /// Does not close independently owned transports.
    pub async fn close(&self) -> Result<(), TransportError> {
        let (sessions, mut failures) = {
            let mut lifecycle = self.inner.lifecycle.lock().unwrap();
            lifecycle.closed = true;
            lifecycle.channel.take();
            (
                lifecycle.sessions.clone(),
                lifecycle.retired_failures.clone(),
            )
        };
        for session in &sessions {
            session.cancel.cancel();
        }
        // Persist a driver failure before later awaits, too: an interrupted
        // close must not consume its JoinHandle and silently lose that error.
        if let Err(error) = self.inner.network.shutdown().await {
            // Public h2 errors expose kind/reason but no shared identity or
            // generation. Never let observing a stream error acknowledge this
            // independent driver result: successive GOAWAYs may differ, or even
            // have identical reasons. A possible duplicate is the safe fallback.
            let failure = RetiredFailure {
                error,
                observed: Arc::new(AtomicBool::new(false)),
            };
            self.inner
                .lifecycle
                .lock()
                .unwrap()
                .retired_failures
                .push(failure.clone());
            failures.push(failure);
        }
        for session in sessions {
            if let Err(error) = session.join().await
                && !matches!(error, TransportError::Cancelled)
            {
                failures.push(RetiredFailure {
                    error,
                    observed: session.observed.clone(),
                });
            }
        }
        let mut lifecycle = self.inner.lifecycle.lock().unwrap();
        lifecycle.sessions.clear();
        for failure in failures {
            if !failure.observed.load(Ordering::SeqCst)
                && !lifecycle
                    .retired_failures
                    .iter()
                    .any(|retired| Arc::ptr_eq(&retired.observed, &failure.observed))
            {
                lifecycle.retired_failures.push(failure);
            }
        }
        // No await below this point: acknowledge exactly the error delivered by
        // this poll, never errors merely encountered while joining other calls.
        // CAS also prevents concurrent closes from returning the same failure.
        let error = lifecycle.retired_failures.iter().find_map(|failure| {
            failure
                .observed
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .ok()
                .map(|_| failure.error.clone())
        });
        lifecycle
            .retired_failures
            .retain(|failure| !failure.observed.load(Ordering::SeqCst));
        error.map_or(Ok(()), Err)
    }
}

pub struct Session {
    control: Arc<Control>,
    responses: mpsc::Receiver<Bytes>,
    terminal_delivered: bool,
}
impl Session {
    /// Each response is opaque. EOF is returned only after gRPC terminal status
    /// and source cleanup; failures are returned once here, and again by finish.
    pub async fn next(&mut self) -> Result<Option<Bytes>, TransportError> {
        if self.terminal_delivered {
            return Ok(None);
        }
        if let Some(response) = self.responses.recv().await {
            return Ok(Some(response));
        }
        let result = self.control.join().await;
        self.control.observed.store(true, Ordering::SeqCst);
        self.terminal_delivered = true;
        result.map(|_| None)
    }
    pub fn completion(&self) -> Completion {
        self.control.progress.snapshot()
    }
    /// Drain remaining responses (discarding their bytes), then join supervision.
    pub async fn finish(&mut self) -> Outcome {
        while self.responses.recv().await.is_some() {}
        let result = self.control.join().await;
        self.control.observed.store(true, Ordering::SeqCst);
        self.terminal_delivered = true;
        result
    }
    /// Signal only. Useful before dropping a session; close/cancel await cleanup.
    pub fn request_cancel(&self) {
        self.control.cancel.cancel();
    }
    pub async fn cancel(&mut self) -> Outcome {
        self.request_cancel();
        self.finish().await
    }
    pub async fn close(&mut self) -> Outcome {
        self.cancel().await
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.control.cancel.cancel();
    }
}

#[derive(Default)]
struct ProducerOutcome {
    primary: Option<TransportError>,
    cleanup: Option<SourceError>,
}
impl ProducerOutcome {
    fn failed(&self) -> bool {
        self.primary.is_some() || self.cleanup.is_some()
    }
    fn compose(self, rpc: Result<(), TransportError>) -> Result<(), TransportError> {
        // Only errors actually selected by supervision are candidates. Never poll
        // a hypothetical RPC outcome after source failure or wait for one here.
        let primary = self.primary.or_else(|| rpc.err());
        match (primary, self.cleanup) {
            (Some(TransportError::Cancelled) | None, Some(cleanup)) => {
                Err(TransportError::Cleanup(cleanup))
            }
            (Some(primary), Some(cleanup)) => Err(TransportError::PrimaryAndCleanup(Box::new(
                PrimaryAndCleanup::new(primary, cleanup),
            ))),
            (Some(primary), None) => Err(primary),
            (None, None) => Ok(()),
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn produce<S: RequestSource>(
    mut source: S,
    sender: mpsc::Sender<Bytes>,
    admitted: oneshot::Receiver<()>,
    stop: CancellationToken,
    failed: CancellationToken,
    progress: Arc<Progress>,
    limit: usize,
) -> ProducerOutcome {
    let mut error = None;
    // A closed/failed admission gate still falls through to exactly one close.
    let admitted = tokio::select! {
        biased;
        _ = stop.cancelled() => false,
        result = admitted => result.is_ok(),
    };
    if admitted {
        loop {
            if stop.is_cancelled() {
                break;
            }
            let next = tokio::select! { biased; _ = stop.cancelled() => break, item = source.next() => item };
            let bytes = match next {
                Ok(Some(bytes)) if bytes.len() <= limit => bytes,
                Ok(Some(_)) => {
                    error = Some(TransportError::MessageTooLarge);
                    break;
                }
                Ok(None) => {
                    progress.exhausted.store(true, Ordering::SeqCst);
                    break;
                }
                Err(e) => {
                    error = Some(TransportError::Source(e));
                    break;
                }
            };
            progress.produced.fetch_add(1, Ordering::SeqCst);
            let sent = tokio::select! { biased; _ = stop.cancelled() => break, sent = sender.send(bytes) => sent };
            if sent.is_err() {
                break;
            }
        }
    }
    drop(sender); // The h2 writer observes EOF independently of source cleanup.
    if error.is_some() {
        failed.cancel();
    }
    let cleanup = source.close().await;
    progress.closed.store(true, Ordering::SeqCst);
    ProducerOutcome {
        primary: error,
        cleanup: cleanup.err(),
    }
}

#[allow(clippy::too_many_arguments)]
async fn supervise<S: RequestSource>(
    source: S,
    provider: Arc<dyn MetadataProvider>,
    channel: h2::client::SendRequest<Bytes>,
    config: TransportConfig,
    deadline: Instant,
    cancel: CancellationToken,
    progress: Arc<Progress>,
    responses: mpsc::Sender<Bytes>,
) -> Result<(), TransportError> {
    let stop = CancellationToken::new();
    let failed = CancellationToken::new();
    let (sender, receiver) = mpsc::channel(1);
    let (admit, admitted) = oneshot::channel();
    let writer = produce(
        source,
        sender,
        admitted,
        stop.clone(),
        failed.clone(),
        progress.clone(),
        config.max_message_bytes,
    );
    let reader = wire::rpc(
        channel,
        config,
        provider,
        Upload {
            receiver,
            progress: progress.clone(),
        },
        deadline,
        responses,
        progress,
        admit,
    );
    tokio::pin!(writer);
    let mut writer_result = None;
    let result = {
        tokio::pin!(reader);
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => break Err(TransportError::Cancelled.into()),
                _ = tokio::time::sleep_until(deadline) => break Err(TransportError::Timeout.into()),
                _ = failed.cancelled() => break Err(TransportError::Cancelled.into()),
                result = &mut writer, if writer_result.is_none() => {
                    if result.failed() { writer_result = Some(result); break Err(TransportError::Cancelled.into()); }
                    writer_result = Some(result);
                }
                result = &mut reader => break result,
            }
        }
    }; // Drop the reader/body before waiting for potentially slow source cleanup.
    stop.cancel();
    let writer_result = match writer_result {
        Some(result) => result,
        None => writer.await,
    };
    writer_result.compose(result.map_err(wire::RpcError::into_transport))
}

#[cfg(test)]
mod admission_tests;

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn tls_ports_are_validated_without_network_and_ipv6_is_unambiguous() {
        for target in [
            "example.invalid:0",
            "example.invalid:65536",
            "example.invalid:abc",
            "example.invalid:",
            "example.invalid:+443",
            "[::1]:0",
            "[::1]:65536",
            "[::1]:abc",
            "[::1]:",
        ] {
            assert!(
                matches!(
                    TransportConfig::tls(target),
                    Err(TransportError::InvalidConfig)
                ),
                "{target}"
            );
        }
        for (target, port) in [
            ("example.invalid", 443),
            ("example.invalid:1", 1),
            ("example.invalid:65535", 65535),
            ("[::1]", 443),
            ("[::1]:443", 443),
            ("[::1]:65535", 65535),
        ] {
            let config = TransportConfig::tls(target).unwrap();
            assert_eq!(config.port, port);
            assert!(config.is_tls());
        }
        assert!(TransportConfig::loopback("127.0.0.1:0".parse().unwrap()).is_err());
    }
}
