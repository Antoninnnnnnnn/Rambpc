use crate::{EditingContext, EngineError, ProtocolConfig, S3Response, SourceError, SourceFuture};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use rambler_protocol::{
    audio_request, context_request, decode_response, end_request, initial_request,
};
use rambler_transport::{
    GrpcTransport, Metadata, MetadataProvider, RequestSource, TransportConfig,
};
use std::{
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex as AsyncMutex, mpsc, watch},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

/// A continuous encoded audio stream, not a sequence of independently encoded
/// files. Empty chunks are ignored. `next` must tolerate its future being dropped.
/// `close` must eventually complete, and is awaited even beyond RPC deadlines.
pub trait AudioSource: Send + 'static {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>>;
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>>;
}
/// Adapter only for sources with synchronous Drop cleanup. Resource-bearing
/// asynchronous sources must implement AudioSource::close themselves.
pub struct StreamAudio<S>(Pin<Box<S>>);
impl<S> StreamAudio<S> {
    pub fn new(stream: S) -> Self {
        Self(Box::pin(stream))
    }
}
impl<S: Stream<Item = Result<Bytes, SourceError>> + Send + 'static> AudioSource for StreamAudio<S> {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async { self.0.next().await.transpose() })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async { Ok(()) })
    }
}

/// Invoked once, only after normal audio EOF AND completed audio cleanup.
/// Explicit None omits context; errors/timeouts never become None.
pub trait ContextProvider: Send + Sync + 'static {
    fn context(&self) -> SourceFuture<'_, Result<Option<EditingContext>, SourceError>>;
}
#[derive(Clone, Default)]
pub enum ContextInput {
    #[default]
    None,
    Static(EditingContext),
    Provider(Arc<dyn ContextProvider>),
}
#[derive(Clone)]
pub struct SessionOptions {
    pub config: ProtocolConfig,
    pub context: ContextInput,
    pub timeout: Duration,
    pub context_timeout: Duration,
    pub credentials: Option<Arc<dyn MetadataProvider>>,
}
impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            config: ProtocolConfig::default(),
            context: ContextInput::None,
            timeout: Duration::from_secs(30),
            context_timeout: Duration::from_secs(2),
            credentials: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionMetrics {
    pub session_id: u64,
    pub requests_produced: u64,
    pub request_protobuf_bytes: u64,
    pub responses_received: u64,
    pub response_protobuf_bytes: u64,
    pub elapsed_seconds: f64,
    pub first_response_seconds: Option<f64>,
    pub eof_seconds: Option<f64>,
}
#[derive(Clone, Debug)]
pub struct EngineEvent {
    /// None is the unique successful EOF event. Response candidates alone never
    /// imply completed dictation, even if their recognition state is final.
    pub response: Option<S3Response>,
    pub metrics: SessionMetrics,
}
#[derive(Default)]
struct Counts {
    requests: AtomicU64,
    bytes: AtomicU64,
}
impl Counts {
    fn count(&self, payload: Vec<u8>) -> Bytes {
        self.requests.fetch_add(1, Ordering::SeqCst);
        self.bytes.fetch_add(payload.len() as u64, Ordering::SeqCst);
        payload.into()
    }
}

type CleanupOutcome = Result<(), EngineError>;
struct AudioState {
    source: AsyncMutex<Option<Box<dyn AudioSource>>>,
    cleanup: Mutex<Option<watch::Receiver<Option<CleanupOutcome>>>>,
}
impl AudioState {
    fn new(source: Box<dyn AudioSource>) -> Arc<Self> {
        Arc::new(Self {
            source: AsyncMutex::new(Some(source)),
            cleanup: Mutex::new(None),
        })
    }
    async fn next(&self) -> Result<Option<Bytes>, EngineError> {
        let mut slot = self.source.lock().await;
        match slot.as_mut() {
            Some(source) => source.next().await.map_err(EngineError::Audio),
            None => Err(EngineError::Supervisor),
        }
    }
    /// Move the actual Box into an independent task BEFORE awaiting close. A
    /// dropped caller future cannot discard a borrowed, half-executed close.
    async fn close(self: &Arc<Self>) -> CleanupOutcome {
        let mut source = self.source.lock().await;
        let done = {
            let mut cleanup = self.cleanup.lock().unwrap();
            if cleanup.is_none() {
                let audio = source.take();
                let (sender, done) = watch::channel(None);
                *cleanup = Some(done);
                tokio::spawn(async move {
                    let result = match audio {
                        Some(mut audio) => audio.close().await.map_err(EngineError::AudioCleanup),
                        None => Err(EngineError::Supervisor),
                    };
                    let _ = sender.send(Some(result));
                });
            }
            cleanup.as_ref().unwrap().clone()
        };
        drop(source);
        wait(done).await
    }
}
async fn wait(mut done: watch::Receiver<Option<CleanupOutcome>>) -> CleanupOutcome {
    loop {
        if let Some(result) = done.borrow_and_update().clone() {
            return result;
        }
        if done.changed().await.is_err() {
            return Err(EngineError::Supervisor);
        }
    }
}

struct Requests {
    audio: Arc<AudioState>,
    initial: Option<Vec<u8>>,
    context: ContextInput,
    context_payload: Option<Vec<u8>>,
    context_timeout: Duration,
    captured: Arc<Mutex<Option<EditingContext>>>,
    counts: Arc<Counts>,
    start: Instant,
    deadline: Instant,
    phase: u8,
    fault: Arc<Mutex<Option<EngineError>>>,
}
impl Requests {
    async fn step(&mut self) -> Result<Option<Bytes>, EngineError> {
        loop {
            match self.phase {
                0 => {
                    self.phase = 1;
                    return Ok(Some(self.counts.count(self.initial.take().unwrap())));
                }
                1 => match self.audio.next().await? {
                    Some(chunk) if chunk.is_empty() => {
                        // Repeated immediately-ready empty chunks must not starve
                        // cancellation or a deadline on a single-thread runtime.
                        tokio::task::yield_now().await;
                    }
                    Some(chunk) => {
                        return Ok(Some(
                            self.counts.count(audio_request(&chunk, self.elapsed())?),
                        ));
                    }
                    None => {
                        self.phase = 2;
                    }
                },
                2 => {
                    self.audio.close().await?;
                    if let ContextInput::Provider(provider) = &self.context {
                        let remaining = self.deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            return Err(EngineError::ContextTimeout);
                        }
                        let current = tokio::time::timeout(
                            self.context_timeout.min(remaining),
                            provider.context(),
                        )
                        .await
                        .map_err(|_| EngineError::ContextTimeout)?
                        .map_err(EngineError::Context)?;
                        self.context_payload = current.as_ref().map(context_request).transpose()?;
                        *self.captured.lock().unwrap() = current;
                    }
                    self.phase = 3;
                }
                3 => {
                    self.phase = 4;
                    if let Some(payload) = self.context_payload.take() {
                        return Ok(Some(self.counts.count(payload)));
                    }
                }
                4 => {
                    self.phase = 5;
                    return Ok(Some(self.counts.count(end_request().to_vec())));
                }
                _ => return Ok(None),
            }
        }
    }
    fn elapsed(&self) -> u64 {
        self.start.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
}
impl RequestSource for Requests {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            self.step().await.map_err(|error| {
                *self.fault.lock().unwrap() = Some(error.clone());
                SourceError::new(error)
            })
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.audio.close().await.map_err(|error| {
                let mut fault = self.fault.lock().unwrap();
                if fault.is_none() {
                    *fault = Some(error.clone());
                }
                SourceError::new(error)
            })
        })
    }
}

struct Control {
    cancel: CancellationToken,
    done: watch::Receiver<Option<CleanupOutcome>>,
    observed: AtomicBool,
}
impl Control {
    async fn wait(&self) -> CleanupOutcome {
        wait(self.done.clone()).await
    }
    async fn observe(&self) -> CleanupOutcome {
        let result = self.wait().await;
        self.observed.store(true, Ordering::SeqCst);
        result
    }
    fn needs_tracking(&self) -> bool {
        match self.done.borrow().as_ref() {
            None => true,
            Some(Ok(()) | Err(EngineError::Cancelled)) => false,
            Some(Err(_)) => !self.observed.load(Ordering::SeqCst),
        }
    }
}
struct Lifecycle {
    closed: bool,
    sessions: Vec<Arc<Control>>,
}
/// Engines isolate all per-session state. Closing an injected transport is always
/// the caller's job. Engine close is permanent, idempotent, and cancellation-safe.
pub struct Engine {
    transport: GrpcTransport,
    owns_transport: bool,
    lifecycle: Mutex<Lifecycle>,
}
static SESSION_ID: AtomicU64 = AtomicU64::new(1);
impl Engine {
    pub fn new(transport: GrpcTransport) -> Self {
        Self {
            transport,
            owns_transport: false,
            lifecycle: Mutex::new(Lifecycle {
                closed: false,
                sessions: Vec::new(),
            }),
        }
    }
    pub async fn connect(config: TransportConfig) -> Result<Self, EngineError> {
        let mut engine = Self::new(GrpcTransport::connect(config).await?);
        engine.owns_transport = true;
        Ok(engine)
    }
    /// Requires an active Tokio runtime. Ownership of audio transfers immediately,
    /// even before the session's first poll. Failed preflight also cleans up audio;
    /// Engine::close waits for that cleanup. No RPC starts on invalid static data.
    pub fn start(
        &self,
        audio: Box<dyn AudioSource>,
        options: SessionOptions,
    ) -> Result<EngineSession, EngineError> {
        let start = Instant::now();
        let deadline = start.checked_add(options.timeout);
        let preflight = (|| {
            if options.timeout.is_zero() || options.context_timeout.is_zero() || deadline.is_none()
            {
                return Err(EngineError::InvalidConfig);
            }
            if options.config.encoding != 10
                || options.config.sample_rate != 16000
                || options.config.channels != 1
            {
                return Err(EngineError::InvalidConfig);
            }
            let initial = initial_request(&options.config, 0)?;
            let context_payload = match &options.context {
                ContextInput::Static(context) => Some(context_request(context)?),
                _ => None,
            };
            Ok((initial, context_payload))
        })();
        let audio = AudioState::new(audio);
        let (sender, done) = watch::channel(None);
        let control = Arc::new(Control {
            cancel: CancellationToken::new(),
            done,
            observed: AtomicBool::new(false),
        });
        let mut lifecycle = self.lifecycle.lock().unwrap();
        lifecycle
            .sessions
            .retain(|control| control.needs_tracking());
        lifecycle.sessions.push(control.clone());
        let preflight = if lifecycle.closed {
            Err(EngineError::Closed)
        } else {
            preflight
        };
        let (initial, context_payload) = match preflight {
            Ok(values) => values,
            Err(error) => {
                tokio::spawn(async move {
                    let _ = sender.send(Some(audio.close().await));
                });
                return Err(error);
            }
        };
        let captured = Arc::new(Mutex::new(match &options.context {
            ContextInput::Static(context) => Some(context.clone()),
            _ => None,
        }));
        let counts = Arc::new(Counts::default());
        let fault = Arc::new(Mutex::new(None));
        let requests = Requests {
            audio: audio.clone(),
            initial: Some(initial),
            context: options.context,
            context_payload,
            context_timeout: options.context_timeout,
            captured: captured.clone(),
            counts: counts.clone(),
            start,
            deadline: deadline.unwrap(),
            phase: 0,
            fault: fault.clone(),
        };
        let transport = self.transport.clone();
        let cancel = control.cancel.clone();
        let (events_tx, events) = mpsc::channel(1);
        let id = SESSION_ID.fetch_add(1, Ordering::Relaxed);
        tokio::spawn(async move {
            let provider = options
                .credentials
                .unwrap_or_else(|| Arc::new(Metadata::default()));
            let result = supervise(
                transport,
                requests,
                provider,
                options.timeout,
                cancel,
                events_tx,
                counts,
                start,
                id,
                fault,
            )
            .await;
            // Includes failed transport start (which takes and then drops Requests).
            // Idempotent and stable if normal EOF cleanup was already in progress.
            let cleanup = audio.close().await;
            let result = match (result, cleanup) {
                (Ok(()) | Err(EngineError::Cancelled), Err(cleanup)) => Err(cleanup),
                (result, _) => result,
            };
            let _ = sender.send(Some(result));
        });
        drop(lifecycle);
        Ok(EngineSession {
            control,
            events,
            captured,
            terminal_delivered: false,
        })
    }
    pub(crate) fn cleanup_unstarted(
        &self,
        audio: Box<dyn AudioSource>,
    ) -> SourceFuture<'static, Result<(), EngineError>> {
        let audio = AudioState::new(audio);
        let (sender, done) = watch::channel(None);
        let control = Arc::new(Control {
            cancel: CancellationToken::new(),
            done,
            observed: AtomicBool::new(false),
        });
        {
            let mut lifecycle = self.lifecycle.lock().unwrap();
            lifecycle
                .sessions
                .retain(|control| control.needs_tracking());
            lifecycle.sessions.push(control.clone());
        }
        tokio::spawn(async move {
            let _ = sender.send(Some(audio.close().await));
        });
        Box::pin(async move { control.observe().await })
    }
    /// Permanently cancels and awaits all sessions. Already-delivered session
    /// failures are not replayed. At most one unobserved background/cleanup failure
    /// is delivered per call; additional failures remain available to later closes.
    pub async fn close(&self) -> Result<(), EngineError> {
        let sessions = {
            let mut lifecycle = self.lifecycle.lock().unwrap();
            lifecycle.closed = true;
            lifecycle.sessions.clone()
        };
        for session in &sessions {
            session.cancel.cancel();
        }
        let mut failures = Vec::new();
        for session in &sessions {
            if let Err(error) = session.wait().await
                && !matches!(error, EngineError::Cancelled)
            {
                failures.push((session, error));
            }
        }
        let mut error = if self.owns_transport {
            self.transport.close().await.err().map(EngineError::from)
        } else {
            None
        };
        // No acknowledgement before the final await: cancelling close must not
        // discard a background failure that has not reached its caller yet.
        // Claim only the error actually returned. Other failures remain tracked
        // for a subsequent close; concurrent closes cannot claim the same error.
        for (session, failure) in failures {
            if session
                .observed
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                error = Some(failure);
                break;
            }
        }
        self.lifecycle
            .lock()
            .unwrap()
            .sessions
            .retain(|control| control.needs_tracking());
        error.map_or(Ok(()), Err)
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        for session in &self.lifecycle.get_mut().unwrap().sessions {
            session.cancel.cancel();
        }
    }
}

pub struct EngineSession {
    control: Arc<Control>,
    events: mpsc::Receiver<EngineEvent>,
    captured: Arc<Mutex<Option<EditingContext>>>,
    terminal_delivered: bool,
}
impl EngineSession {
    pub async fn next(&mut self) -> Result<Option<EngineEvent>, EngineError> {
        if self.terminal_delivered {
            return Ok(None);
        }
        if let Some(event) = self.events.recv().await {
            return Ok(Some(event));
        }
        let result = self.control.observe().await;
        self.terminal_delivered = true;
        result.map(|()| None)
    }
    pub fn captured_context(&self) -> Option<EditingContext> {
        self.captured.lock().unwrap().clone()
    }
    pub fn request_cancel(&self) {
        self.control.cancel.cancel();
    }
    /// Cancels this session, not other sessions or its shared transport. A cancelled
    /// close future can be retried: cleanup is owned by the stable supervisor.
    pub async fn close(&mut self) -> Result<(), EngineError> {
        let result = self.close_unobserved().await;
        self.control.observed.store(true, Ordering::SeqCst);
        result
    }
    // Collectors that return a different primary error must not acknowledge a
    // cleanup failure they discard. Engine::close will surface that background error.
    pub(crate) async fn close_unobserved(&mut self) -> Result<(), EngineError> {
        self.request_cancel();
        self.events.close();
        match self.control.wait().await {
            Err(EngineError::Cancelled) => Ok(()),
            result => result,
        }
    }
    pub async fn cancel(&mut self) -> Result<(), EngineError> {
        self.close().await
    }
}
impl Drop for EngineSession {
    fn drop(&mut self) {
        self.request_cancel();
    }
}

#[allow(clippy::too_many_arguments)]
async fn supervise(
    transport: GrpcTransport,
    requests: Requests,
    provider: Arc<dyn MetadataProvider>,
    timeout: Duration,
    cancel: CancellationToken,
    events: mpsc::Sender<EngineEvent>,
    counts: Arc<Counts>,
    start: Instant,
    id: u64,
    fault: Arc<Mutex<Option<EngineError>>>,
) -> Result<(), EngineError> {
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    let deadline = requests.deadline;
    let timeout = timeout.min(deadline.saturating_duration_since(Instant::now()));
    if timeout.is_zero() {
        return Err(EngineError::Transport(
            rambler_transport::TransportError::Timeout,
        ));
    }
    let mut rpc = transport.start_with_provider(requests, provider, timeout)?;
    let mut received = 0;
    let mut bytes = 0;
    let mut first = None;
    let mut s3_end = false;
    let mut s3_error = None;
    let snapshot = |received, bytes, first, eof| SessionMetrics {
        session_id: id,
        requests_produced: counts.requests.load(Ordering::SeqCst),
        request_protobuf_bytes: counts.bytes.load(Ordering::SeqCst),
        responses_received: received,
        response_protobuf_bytes: bytes,
        elapsed_seconds: start.elapsed().as_secs_f64(),
        first_response_seconds: first,
        eof_seconds: eof,
    };
    let outcome = async {
        loop {
            let payload = rpc.next().await?;
            let Some(payload) = payload else {
                break;
            };
            received += 1;
            bytes += payload.len() as u64;
            if first.is_none() {
                first = Some(start.elapsed().as_secs_f64());
            }
            let response = decode_response(&payload)?;
            if response.status == Some(1) {
                s3_end = true;
            }
            if response.status == Some(2) || response.error_code.is_some_and(|code| code != 0) {
                s3_error = Some(Arc::new(response.clone()));
            }
            events
                .send(EngineEvent {
                    response: Some(response),
                    metrics: snapshot(received, bytes, first, None),
                })
                .await
                .map_err(|_| EngineError::Cancelled)?;
        }
        let completion = rpc.finish().await?;
        if let Some(error) = s3_error {
            return Err(EngineError::S3(error));
        }
        if !s3_end
            || !completion.source_exhausted
            || !completion.upload_eof_polled
            || !completion.source_closed
            || completion.terminal_code != Some(tonic::Code::Ok)
        {
            return Err(EngineError::IncompleteSession);
        }
        events
            .send(EngineEvent {
                response: None,
                metrics: snapshot(received, bytes, first, Some(start.elapsed().as_secs_f64())),
            })
            .await
            .map_err(|_| EngineError::Cancelled)?;
        Ok(())
    };
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(EngineError::Cancelled),
        _ = tokio::time::sleep_until(deadline) => Err(EngineError::Transport(rambler_transport::TransportError::Timeout)),
        result = outcome => result,
    };
    // Always join source cleanup. Cancel can interrupt any event send/response
    // wait; it cannot interrupt this stable cleanup section.
    if result.is_err() {
        rpc.request_cancel();
    }
    let cleanup = rpc.finish().await;
    match result {
        // Restore an explicitly retained engine-source error without publishing
        // caller error text. A real late gRPC failure is never hidden by S3 END.
        Err(EngineError::Transport(rambler_transport::TransportError::Source(_)))
        | Err(EngineError::Transport(rambler_transport::TransportError::Cleanup(_))) => Err(fault
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(EngineError::Supervisor)),
        Err(error) => Err(error),
        Ok(()) => cleanup.map(|_| ()).map_err(Into::into),
    }
}
