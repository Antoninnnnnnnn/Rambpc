use crate::{EditingContext, EngineError, ProtocolConfig, S3Response, SourceError, SourceFuture};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use rambler_protocol::{
    audio_request, context_request, decode_response, end_request, initial_request,
};
use rambler_transport::{
    Completion, GrpcTransport, Metadata, MetadataProvider, RequestSource, TransportConfig,
    UploadSubmission,
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
/// Suggested opt-in post-audio budget for interactive apps. Healthy native
/// sessions in the measured campaign finished within about 0.5 s of audio end;
/// 5 s leaves room for variation and late context without waiting for the full
/// RPC deadline on stalls. This is not a service latency guarantee.
pub const RECOMMENDED_FINALIZE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct SessionOptions {
    pub config: ProtocolConfig,
    pub context: ContextInput,
    /// Hard overall session budget, including speech, cleanup, and final status.
    /// Defaults to 30 seconds; asynchronous cleanup is still awaited beyond it.
    pub timeout: Duration,
    /// Optional budget from audio EOF and successful cleanup through late context,
    /// upload END, final gRPC status, and successful EOF publication. Never extends
    /// `timeout` or ends audio.
    /// Defaults to None; opt in with [`Self::with_recommended_finalize_timeout`]
    /// or an app-specific budget. Zero is invalid; overflow uses the hard cap.
    pub finalize_timeout: Option<Duration>,
    pub context_timeout: Duration,
    pub credentials: Option<Arc<dyn MetadataProvider>>,
}
impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            config: ProtocolConfig::default(),
            context: ContextInput::None,
            timeout: Duration::from_secs(30),
            finalize_timeout: None,
            context_timeout: Duration::from_secs(2),
            credentials: None,
        }
    }
}

impl SessionOptions {
    /// Explicitly set the post-audio budget to [`RECOMMENDED_FINALIZE_TIMEOUT`].
    #[must_use]
    pub fn with_recommended_finalize_timeout(mut self) -> Self {
        self.finalize_timeout = Some(RECOMMENDED_FINALIZE_TIMEOUT);
        self
    }
}

/// Observations of the encoded source at the engine boundary, not PCM sample
/// counts or network acknowledgements. Only nonempty chunks returned by `next`
/// are counted, even if later protobuf validation fails. EOF is independent of
/// cleanup: an early cancellation may finish cleanup without ever seeing EOF.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EncodedAudioProgress {
    pub chunks: u64,
    pub bytes: u64,
    pub eof: bool,
    pub cleanup_finished: bool,
    pub cleanup_succeeded: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionMetrics {
    pub session_id: u64,
    pub audio: EncodedAudioProgress,
    pub requests_produced: u64,
    pub request_protobuf_bytes: u64,
    pub responses_received: u64,
    pub response_protobuf_bytes: u64,
    pub elapsed_seconds: f64,
    pub first_response_seconds: Option<f64>,
    /// Successful EOF commitment, after cleanup and event-queue reservation.
    pub eof_seconds: Option<f64>,
}
/// Local observations, relative to engine session start. Missing observations
/// remain `None`; h2 acceptance is not socket delivery or peer acknowledgement.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SessionTimeline {
    pub last_audio_sent_seconds: Option<f64>,
    pub context_sent_seconds: Option<f64>,
    pub end_sent_seconds: Option<f64>,
    pub half_close_sent_seconds: Option<f64>,
    /// Transport deframe time for the last response delivered to the engine.
    pub last_response_seconds: Option<f64>,
    /// S3 envelope status, not the gRPC trailer or recognition state. Absent
    /// for an omitted status, an undecodable payload, or no response at all.
    pub last_response_s3_status: Option<i32>,
    /// Last response with at least one decoded partial hypothesis (not heartbeat).
    pub last_partial_response_seconds: Option<f64>,
    pub recognition_finals_received: u64,
    pub requests_submitted: u64,
    pub request_protobuf_bytes_submitted: u64,
}
impl SessionTimeline {
    fn record_upload(&mut self, upload: &UploadSubmission, counts: &Counts, start: Instant) {
        let relative = |at: Instant| at.duration_since(start).as_secs_f64();
        let context = counts.context_ordinal.load(Ordering::SeqCst);
        let end = counts.end_ordinal.load(Ordering::SeqCst);
        // The first request is init. Only optional context and required END
        // can follow the final audio, so three completed requests suffice.
        let last_audio = upload.requests().min(
            [context, end]
                .into_iter()
                .find(|ordinal| *ordinal > 0)
                .map_or(u64::MAX, |ordinal| ordinal - 1),
        );
        self.last_audio_sent_seconds = (last_audio > 1)
            .then(|| upload.request_at(last_audio))
            .flatten()
            .map(relative);
        self.context_sent_seconds = upload.request_at(context).map(relative);
        self.end_sent_seconds = upload.request_at(end).map(relative);
        self.half_close_sent_seconds = upload.half_close_at().map(relative);
        self.requests_submitted = upload.requests();
        self.request_protobuf_bytes_submitted = upload.protobuf_bytes();
    }
}

/// Safe final counters and timings, deliberately excluding session identifiers,
/// audio, context, hypotheses, response bytes, and caller/server error details.
/// Request byte counts without a `submitted` suffix describe produced payloads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiagnosticMetrics {
    pub audio: EncodedAudioProgress,
    pub requests_produced: u64,
    pub request_protobuf_bytes: u64,
    pub responses_received: u64,
    pub response_protobuf_bytes: u64,
    pub elapsed_seconds: f64,
    pub first_response_seconds: Option<f64>,
    pub eof_seconds: Option<f64>,
    pub timeline: SessionTimeline,
}
impl From<&SessionMetrics> for DiagnosticMetrics {
    fn from(metrics: &SessionMetrics) -> Self {
        Self {
            audio: metrics.audio.clone(),
            requests_produced: metrics.requests_produced,
            request_protobuf_bytes: metrics.request_protobuf_bytes,
            responses_received: metrics.responses_received,
            response_protobuf_bytes: metrics.response_protobuf_bytes,
            elapsed_seconds: metrics.elapsed_seconds,
            first_response_seconds: metrics.first_response_seconds,
            eof_seconds: metrics.eof_seconds,
            timeline: SessionTimeline::default(),
        }
    }
}
/// Observations after transport supervision AND source cleanup have completed.
/// Missing criteria remain failures: this is diagnosis, not permission to retry
/// or replay audio. `upload_eof_polled` is a local transport observation, not a
/// server acknowledgement. Only safe structural data is retained here.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionDiagnostic {
    pub saw_s3_end: bool,
    pub completion: Completion,
    pub metrics: DiagnosticMetrics,
}
/// Diagnostic attached to [`EngineError::IncompleteSession`].
pub type IncompleteSessionDiagnostic = SessionDiagnostic;

#[derive(Clone)]
pub struct SessionDiagnostics(Arc<Mutex<Option<Arc<SessionDiagnostic>>>>);
impl SessionDiagnostics {
    /// The joined terminal observation, if published. Await terminal `next`,
    /// session `close`, or engine `close` before reading after cancellation.
    /// The handle does not keep the session alive or acknowledge its errors.
    pub fn final_snapshot(&self) -> Option<Arc<SessionDiagnostic>> {
        self.0.lock().unwrap().clone()
    }
}

#[derive(Clone, Debug)]
pub struct EngineEvent {
    /// None is the unique successful EOF event. Once published, cancellation or
    /// deadline expiration cannot revoke success. Response candidates alone never
    /// imply completed dictation, even if their recognition state is final.
    pub response: Option<S3Response>,
    pub metrics: SessionMetrics,
}
#[derive(Default)]
struct Counts {
    requests: AtomicU64,
    bytes: AtomicU64,
    context_ordinal: AtomicU64,
    end_ordinal: AtomicU64,
}
impl Counts {
    fn count(&self, payload: Vec<u8>, milestone: Option<&AtomicU64>) -> Bytes {
        let ordinal = self.requests.fetch_add(1, Ordering::SeqCst) + 1;
        self.bytes.fetch_add(payload.len() as u64, Ordering::SeqCst);
        if let Some(milestone) = milestone {
            milestone.store(ordinal, Ordering::SeqCst);
        }
        payload.into()
    }
}

type CleanupOutcome = Result<(), EngineError>;
struct AudioState {
    source: AsyncMutex<Option<Box<dyn AudioSource>>>,
    progress: Arc<Mutex<EncodedAudioProgress>>,
    cleanup: Mutex<Option<watch::Receiver<Option<CleanupOutcome>>>>,
}
impl AudioState {
    fn new(source: Box<dyn AudioSource>) -> Arc<Self> {
        Arc::new(Self {
            source: AsyncMutex::new(Some(source)),
            progress: Arc::new(Mutex::new(EncodedAudioProgress::default())),
            cleanup: Mutex::new(None),
        })
    }
    async fn next(&self) -> Result<Option<Bytes>, EngineError> {
        let mut slot = self.source.lock().await;
        let chunk = match slot.as_mut() {
            Some(source) => source.next().await.map_err(EngineError::Audio)?,
            None => return Err(EngineError::Supervisor),
        };
        let mut progress = self.progress.lock().unwrap();
        match &chunk {
            Some(chunk) if !chunk.is_empty() => {
                progress.chunks = progress.chunks.saturating_add(1);
                progress.bytes = progress.bytes.saturating_add(chunk.len() as u64);
            }
            None => progress.eof = true,
            _ => {}
        }
        Ok(chunk)
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
                let progress = self.progress.clone();
                tokio::spawn(async move {
                    let result = match audio {
                        Some(mut audio) => audio.close().await.map_err(EngineError::AudioCleanup),
                        None => Err(EngineError::Supervisor),
                    };
                    {
                        let mut progress = progress.lock().unwrap();
                        progress.cleanup_finished = true;
                        progress.cleanup_succeeded = result.is_ok();
                    }
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

fn finalize_deadline(start: Instant, budget: Duration, hard: Instant) -> Instant {
    start.checked_add(budget).unwrap_or(hard).min(hard)
}

// Failures resolve deadline ties before joining cleanup, so time spent cleaning
// up cannot replace the primary failure. Prospective success rechecks at commit.
fn deadline_error(
    cancelled: bool,
    now: Instant,
    hard: Instant,
    finalize: Option<Instant>,
) -> Option<EngineError> {
    if cancelled {
        Some(EngineError::Cancelled)
    } else if now >= hard {
        Some(EngineError::Transport(
            rambler_transport::TransportError::Timeout,
        ))
    } else if finalize.is_some_and(|deadline| now >= deadline) {
        Some(EngineError::FinalizeTimeout)
    } else {
        None
    }
}

// Requests.close only closes the retained audio owner. Its cleanup is already
// memoized in AudioState and delivered by the engine's secondary ledger, so
// unwrap transport composites locally, never in the public From impl.
fn restore_source_error(error: EngineError, fault: Option<&EngineError>) -> EngineError {
    match error {
        EngineError::Transport(error) => {
            let primary = match error {
                rambler_transport::TransportError::PrimaryAndCleanup(failure) => {
                    failure.into_parts().0
                }
                error => error,
            };
            match primary {
                rambler_transport::TransportError::Source(_)
                | rambler_transport::TransportError::Cleanup(_) => {
                    fault.cloned().unwrap_or(EngineError::Supervisor)
                }
                error => EngineError::from(error),
            }
        }
        error => error,
    }
}

fn selected_outcome(
    result: Result<bool, EngineError>,
    fault: Option<&EngineError>,
    cancelled: bool,
    now: Instant,
    hard: Instant,
    finalize: Option<Instant>,
) -> Result<bool, EngineError> {
    // Recover the selected primary before deciding whether it is a local timer.
    // Genuine failures (including remote gRPC DeadlineExceeded) are immutable.
    let result = result.map_err(|error| restore_source_error(error, fault));
    if matches!(
        result,
        Ok(_)
            | Err(EngineError::ContextTimeout | EngineError::FinalizeTimeout)
            | Err(EngineError::Transport(
                rambler_transport::TransportError::Timeout
            ))
    ) && let Some(error) = deadline_error(cancelled, now, hard, finalize)
    {
        return Err(error);
    }
    result
}

// Only produced after the successful EOF has been published. Callers must map
// this marker directly to success, without any further asynchronous cleanup.
struct Committed;

fn commit_success(
    cancelled: bool,
    now: Instant,
    hard: Instant,
    finalize: Option<Instant>,
    publish: impl FnOnce(),
) -> Result<Committed, EngineError> {
    if let Some(error) = deadline_error(cancelled, now, hard, finalize) {
        return Err(error);
    }
    publish();
    Ok(Committed)
}

async fn reserve_success<'a>(
    events: &'a mpsc::Sender<EngineEvent>,
    cancel: &CancellationToken,
    hard: Instant,
    finalize: &mut Option<watch::Receiver<Option<Instant>>>,
) -> Result<mpsc::Permit<'a, EngineEvent>, EngineError> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(EngineError::Cancelled),
        _ = tokio::time::sleep_until(hard) => Err(EngineError::Transport(rambler_transport::TransportError::Timeout)),
        _ = wait_finalization(finalize) => Err(EngineError::FinalizeTimeout),
        permit = events.reserve() => permit.map_err(|_| EngineError::Cancelled),
    }
}

async fn wait_finalization(done: &mut Option<watch::Receiver<Option<Instant>>>) {
    if let Some(done) = done {
        loop {
            let deadline = *done.borrow_and_update();
            if let Some(deadline) = deadline {
                tokio::time::sleep_until(deadline).await;
                return;
            }
            if done.changed().await.is_err() {
                break;
            }
        }
    }
    std::future::pending::<()>().await;
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
    finalization: Option<(Duration, watch::Sender<Option<Instant>>)>,
    phase: u8,
    fault: Arc<Mutex<Option<EngineError>>>,
}
impl Requests {
    async fn step(&mut self) -> Result<Option<Bytes>, EngineError> {
        loop {
            match self.phase {
                0 => {
                    self.phase = 1;
                    return Ok(Some(self.counts.count(self.initial.take().unwrap(), None)));
                }
                1 => match self.audio.next().await? {
                    Some(chunk) if chunk.is_empty() => {
                        // Repeated immediately-ready empty chunks must not starve
                        // cancellation or a deadline on a single-thread runtime.
                        tokio::task::yield_now().await;
                    }
                    Some(chunk) => {
                        return Ok(Some(
                            self.counts
                                .count(audio_request(&chunk, self.elapsed())?, None),
                        ));
                    }
                    None => {
                        self.audio.close().await?;
                        self.phase = 2;
                        if let Some((budget, sender)) = &self.finalization {
                            sender.send_replace(Some(finalize_deadline(
                                Instant::now(),
                                *budget,
                                self.deadline,
                            )));
                        }
                    }
                },
                2 => {
                    if let ContextInput::Provider(provider) = &self.context {
                        let deadline = self
                            .finalization
                            .as_ref()
                            .and_then(|(_, sender)| *sender.borrow())
                            .unwrap_or(self.deadline);
                        let remaining = deadline.saturating_duration_since(Instant::now());
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
                        return Ok(Some(
                            self.counts
                                .count(payload, Some(&self.counts.context_ordinal)),
                        ));
                    }
                }
                4 => {
                    self.phase = 5;
                    return Ok(Some(
                        self.counts
                            .count(end_request().to_vec(), Some(&self.counts.end_ordinal)),
                    ));
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
    // Published before done. Observing the unchanged primary session result
    // never acknowledges a distinct source-cleanup error that it did not carry.
    secondary: Mutex<Option<EngineError>>,
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
        // Read done first: a pending supervisor may still publish a secondary.
        // Once done is present, both outcomes are stable, even if another task
        // observes the primary while start/close is reaping this control.
        let primary_pending = match self.done.borrow().as_ref() {
            None => return true,
            Some(Ok(()) | Err(EngineError::Cancelled)) => false,
            Some(Err(_)) => !self.observed.load(Ordering::SeqCst),
        };
        primary_pending || self.secondary.lock().unwrap().is_some()
    }
    // Only called after all shutdown awaits, so claiming is delivery-atomic
    // with respect to cancellation. Concurrent closes cannot claim one failure.
    fn claim_failure(&self) -> Option<EngineError> {
        if let Some(Err(error)) = self.done.borrow().as_ref()
            && !matches!(error, EngineError::Cancelled)
            && self
                .observed
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            return Some(error.clone());
        }
        self.secondary.lock().unwrap().take()
    }
}
// AudioState memoizes close, and some sources return the same original failure
// from next and close. Only shared object identity proves duplication; fixed
// redacted formatting (or equal codes/details) cannot identify an error instance.
fn same_audio_failure(primary: &EngineError, cleanup: &EngineError) -> bool {
    match (primary, cleanup) {
        (
            EngineError::Audio(primary) | EngineError::AudioCleanup(primary),
            EngineError::AudioCleanup(cleanup),
        ) => std::ptr::eq(primary.original(), cleanup.original()),
        _ => false,
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
    /// Eagerly connect an owned transport before starting any dictation RPC.
    /// TCP and verified TLS (unless explicit loopback) complete here; use
    /// [`Self::connect_ready`] to also wait for confirmation from the HTTP/2 peer.
    pub async fn connect(config: TransportConfig) -> Result<Self, EngineError> {
        Ok(Self::with_owned_transport(
            GrpcTransport::connect(config).await?,
        ))
    }

    /// Warm up an owned connection before the first dictation. Waits for the
    /// HTTP/2 peer to acknowledge a PING within the configured connection
    /// timeout. Does not start an RPC or query credentials. Reuse the returned
    /// engine for sessions; the PING is not a guarantee of future availability.
    pub async fn connect_ready(config: TransportConfig) -> Result<Self, EngineError> {
        Ok(Self::with_owned_transport(
            GrpcTransport::connect_ready(config).await?,
        ))
    }

    fn with_owned_transport(transport: GrpcTransport) -> Self {
        let mut engine = Self::new(transport);
        engine.owns_transport = true;
        engine
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
            if options.timeout.is_zero()
                || options.context_timeout.is_zero()
                || options
                    .finalize_timeout
                    .is_some_and(|budget| budget.is_zero())
                || deadline.is_none()
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
            secondary: Mutex::new(None),
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
        let audio_progress = audio.progress.clone();
        let captured = Arc::new(Mutex::new(match &options.context {
            ContextInput::Static(context) => Some(context.clone()),
            _ => None,
        }));
        let counts = Arc::new(Counts::default());
        let fault = Arc::new(Mutex::new(None));
        let (finalization, finalize_done) = match options.finalize_timeout {
            Some(budget) => {
                let (sender, receiver) = watch::channel(None);
                (Some((budget, sender)), Some(receiver))
            }
            None => (None, None),
        };
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
            finalization,
            phase: 0,
            fault: fault.clone(),
        };
        let transport = self.transport.clone();
        let cancel = control.cancel.clone();
        let (events_tx, events) = mpsc::channel(1);
        let diagnostic = SessionDiagnostics(Arc::new(Mutex::new(None)));
        let final_diagnostic = diagnostic.clone();
        let final_control = control.clone();
        let id = SESSION_ID.fetch_add(1, Ordering::Relaxed);
        tokio::spawn(async move {
            let provider = options
                .credentials
                .unwrap_or_else(|| Arc::new(Metadata::default()));
            let result = match supervise(
                transport,
                requests,
                provider,
                options.timeout,
                finalize_done,
                cancel,
                events_tx,
                counts,
                start,
                id,
                fault,
                final_diagnostic.clone(),
            )
            .await
            {
                Ok(Committed) => Ok(()),
                Err(primary) => {
                    // Includes failed transport start (which takes and drops Requests).
                    // Success already awaited this memoized cleanup before publishing EOF.
                    match audio.close().await {
                        Err(cleanup) if matches!(primary, EngineError::Cancelled) => Err(cleanup),
                        Err(cleanup) => {
                            if !same_audio_failure(&primary, &cleanup) {
                                *final_control.secondary.lock().unwrap() = Some(cleanup);
                            }
                            Err(primary)
                        }
                        Ok(()) => Err(primary),
                    }
                }
            };
            // Cancellation or rejection before transport start still has a
            // session and final source-cleanup observations, but no wire events.
            let mut diagnostic = final_diagnostic.0.lock().unwrap();
            if diagnostic.is_none() {
                *diagnostic = Some(Arc::new(SessionDiagnostic {
                    saw_s3_end: false,
                    completion: Completion::default(),
                    metrics: DiagnosticMetrics {
                        audio: audio.progress.lock().unwrap().clone(),
                        elapsed_seconds: start.elapsed().as_secs_f64(),
                        ..Default::default()
                    },
                }));
            }
            let _ = sender.send(Some(result));
        });
        drop(lifecycle);
        Ok(EngineSession {
            control,
            events,
            captured,
            audio_progress,
            diagnostic,
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
            secondary: Mutex::new(None),
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
    /// A distinct audio-cleanup failure remains reportable even after observing the
    /// primary session failure. Owned transport errors likewise remain independent.
    pub async fn close(&self) -> Result<(), EngineError> {
        let sessions = {
            let mut lifecycle = self.lifecycle.lock().unwrap();
            lifecycle.closed = true;
            lifecycle.sessions.clone()
        };
        for session in &sessions {
            session.cancel.cancel();
        }
        for session in &sessions {
            let _ = session.wait().await;
        }
        let error = if self.owns_transport {
            self.transport.close().await.err().map(EngineError::from)
        } else {
            None
        };
        // Transport close acknowledges the error it returns. Deliver it without
        // overwriting it or claiming any session failure. No await follows this
        // handoff, so cancellation cannot lose the consumed transport outcome.
        // Claim only a failure actually returned; all others remain tracked.
        let error = error.or_else(|| sessions.iter().find_map(|session| session.claim_failure()));
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
    audio_progress: Arc<Mutex<EncodedAudioProgress>>,
    diagnostic: SessionDiagnostics,
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
    /// A safe live snapshot, also available after cancellation or another error.
    /// Await `close` or terminal `next` before treating cleanup flags as final.
    /// These are encoded-source observations, not PCM or server consumption.
    pub fn audio_progress(&self) -> EncodedAudioProgress {
        self.audio_progress.lock().unwrap().clone()
    }
    /// Retain safe final measurements across all terminal outcomes, including
    /// when the session is moved into [`crate::collect_session`].
    pub fn diagnostics(&self) -> SessionDiagnostics {
        self.diagnostic.clone()
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
    // Defensive collector invariants use the real joined transport snapshot,
    // never a fabricated default Completion. Late failures still take precedence.
    pub(crate) async fn incomplete_error(&mut self) -> EngineError {
        if let Err(error) = self.close_unobserved().await {
            return error;
        }
        self.diagnostic
            .final_snapshot()
            .map(EngineError::IncompleteSession)
            .unwrap_or(EngineError::Supervisor)
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
    mut finalize_done: Option<watch::Receiver<Option<Instant>>>,
    cancel: CancellationToken,
    events: mpsc::Sender<EngineEvent>,
    counts: Arc<Counts>,
    start: Instant,
    id: u64,
    fault: Arc<Mutex<Option<EngineError>>>,
    final_diagnostic: SessionDiagnostics,
) -> Result<Committed, EngineError> {
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
    let audio = requests.audio.clone();
    let audio_progress = audio.progress.clone();
    let mut rpc = transport.start_with_provider(requests, provider, timeout)?;
    let mut received = 0;
    let mut bytes = 0;
    let mut first = None;
    let mut timeline = SessionTimeline::default();
    let mut s3_end = false;
    let mut s3_error = None;
    let snapshot = |received, bytes, first, eof| SessionMetrics {
        session_id: id,
        audio: audio_progress.lock().unwrap().clone(),
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
            let Some((payload, deframed_at)) = rpc.next_timed().await? else {
                break;
            };
            received += 1;
            bytes += payload.len() as u64;
            let at = deframed_at.duration_since(start).as_secs_f64();
            first.get_or_insert(at);
            timeline.last_response_seconds = Some(at);
            timeline.last_response_s3_status = None;
            let response = decode_response(&payload)?;
            timeline.last_response_s3_status = response.status;
            if !response.partials().is_empty() {
                timeline.last_partial_response_seconds = Some(at);
            }
            timeline.recognition_finals_received +=
                u64::from(response.recognition_state() == Some(1));
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
            // Defer the error payload until the final cleanup/join below.
            return Ok(false);
        }
        // Success is only a candidate until cleanup and queue reservation finish.
        // Publishing EOF here would allow cancellation/deadlines to revoke it.
        Ok(true)
    };
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(EngineError::Cancelled),
        _ = tokio::time::sleep_until(deadline) => Err(EngineError::Transport(rambler_transport::TransportError::Timeout)),
        _ = wait_finalization(&mut finalize_done) => Err(EngineError::FinalizeTimeout),
        result = outcome => result,
    };
    // Local context/transport timers may wake before the watch waiter is polled.
    // Resolve their ties now, not after cleanup; selected real failures survive.
    let finalize = finalize_done.as_ref().and_then(|done| *done.borrow());
    let result = selected_outcome(
        result,
        fault.lock().unwrap().as_ref(),
        cancel.is_cancelled(),
        Instant::now(),
        deadline,
        finalize,
    );
    // Always join source cleanup. Cancel can interrupt any event send/response
    // wait; it cannot interrupt this stable cleanup section.
    if result.is_err() {
        rpc.request_cancel();
    }
    let cleanup = rpc.finish().await;
    timeline.record_upload(&rpc.upload_submission(), &counts, start);
    let completion = rpc.completion();
    let diagnostic = |metrics: &SessionMetrics| {
        Arc::new(SessionDiagnostic {
            saw_s3_end: s3_end,
            completion: completion.clone(),
            metrics: DiagnosticMetrics {
                timeline,
                ..DiagnosticMetrics::from(metrics)
            },
        })
    };
    let cleanup = cleanup
        .map_err(EngineError::from)
        .map_err(|error| restore_source_error(error, fault.lock().unwrap().as_ref()));
    let result = async {
        match result {
            Err(error) => Err(error),
            Ok(false) => {
                cleanup?;
                Err(EngineError::IncompleteSession(diagnostic(&snapshot(
                    received, bytes, first, None,
                ))))
            }
            Ok(true) => {
                cleanup?;
                audio.close().await?;
                let permit =
                    reserve_success(&events, &cancel, deadline, &mut finalize_done).await?;
                let finalize = finalize_done.as_ref().and_then(|done| *done.borrow());
                commit_success(
                    cancel.is_cancelled(),
                    Instant::now(),
                    deadline,
                    finalize,
                    || {
                        let metrics =
                            snapshot(received, bytes, first, Some(start.elapsed().as_secs_f64()));
                        *final_diagnostic.0.lock().unwrap() = Some(diagnostic(&metrics));
                        // No await or fallible result after this commitment. In
                        // particular a consumer cancelling upon EOF cannot revoke it.
                        permit.send(EngineEvent {
                            response: None,
                            metrics,
                        });
                    },
                )
            }
        }
    }
    .await;
    if let Err(error) = &result {
        let diagnostic = match error {
            EngineError::IncompleteSession(diagnostic) => diagnostic.clone(),
            _ => diagnostic(&snapshot(received, bytes, first, None)),
        };
        *final_diagnostic.0.lock().unwrap() = Some(diagnostic);
    }
    result
}

#[cfg(test)]
#[path = "../tests/common/mod.rs"]
mod test_common;

#[cfg(test)]
mod finalization_tests {
    use super::*;
    use std::cell::Cell;

    fn event(response: Option<S3Response>) -> EngineEvent {
        EngineEvent {
            response,
            metrics: SessionMetrics {
                session_id: 0,
                audio: EncodedAudioProgress::default(),
                requests_produced: 0,
                request_protobuf_bytes: 0,
                responses_received: 0,
                response_protobuf_bytes: 0,
                elapsed_seconds: 0.0,
                first_response_seconds: None,
                eof_seconds: Some(0.0),
            },
        }
    }

    #[test]
    fn selected_audio_failure_survives_late_cancel_and_expired_deadlines() {
        // Inject the post-select snapshot deterministically, not a socket race.
        let now = Instant::now();
        let original = SourceError::new(std::io::Error::other("private source"));
        let result = selected_outcome(
            Err(EngineError::Audio(original.clone())),
            None,
            true,
            now,
            now,
            Some(now),
        );
        match result {
            Err(EngineError::Audio(actual)) => {
                assert!(std::ptr::eq(actual.original(), original.original()));
            }
            _ => panic!("selected audio failure was replaced: {result:?}"),
        }
    }

    #[test]
    fn selected_source_and_context_failures_keep_identity_before_deadline_arbitration() {
        let start = Instant::now();
        let finalize = start + Duration::from_secs(1);
        let hard = start + Duration::from_secs(2);
        let original = SourceError::new(std::io::Error::other("private source"));
        for primary in [
            EngineError::Audio(original.clone()),
            EngineError::AudioCleanup(original.clone()),
            EngineError::Context(original.clone()),
        ] {
            for cancelled in [false, true] {
                for now in [start, finalize, hard] {
                    // Direct failures and both transport source wrappers must
                    // recover the same variant and shared original object.
                    for selected in [
                        primary.clone(),
                        EngineError::Transport(rambler_transport::TransportError::Source(
                            SourceError::new(primary.clone()),
                        )),
                        EngineError::Transport(rambler_transport::TransportError::Cleanup(
                            SourceError::new(primary.clone()),
                        )),
                    ] {
                        let error = selected_outcome(
                            Err(selected),
                            Some(&primary),
                            cancelled,
                            now,
                            hard,
                            Some(finalize),
                        )
                        .unwrap_err();
                        assert_eq!(
                            std::mem::discriminant(&error),
                            std::mem::discriminant(&primary)
                        );
                        match error {
                            EngineError::Audio(actual)
                            | EngineError::AudioCleanup(actual)
                            | EngineError::Context(actual) => {
                                assert!(std::ptr::eq(actual.original(), original.original()));
                            }
                            _ => unreachable!(),
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn selected_protocol_s3_and_remote_transport_failures_are_not_local_timeouts() {
        use rambler_transport::TransportError;
        let now = Instant::now();
        let s3 = Arc::new(decode_response(b"\x08\x02").unwrap());
        for cancelled in [false, true] {
            assert!(matches!(
                selected_outcome(
                    Err(EngineError::Protocol(rambler_protocol::Error::Truncated)),
                    None,
                    cancelled,
                    now,
                    now,
                    Some(now),
                ),
                Err(EngineError::Protocol(rambler_protocol::Error::Truncated))
            ));
            match selected_outcome(
                Err(EngineError::S3(s3.clone())),
                None,
                cancelled,
                now,
                now,
                Some(now),
            ) {
                Err(EngineError::S3(actual)) => assert!(Arc::ptr_eq(&actual, &s3)),
                other => panic!("selected S3 error was replaced: {other:?}"),
            }
            for primary in [
                TransportError::Grpc(tonic::Code::DeadlineExceeded),
                TransportError::Grpc(tonic::Code::Cancelled),
                TransportError::Protocol,
                TransportError::Credentials,
                TransportError::Connect,
                TransportError::Closed,
                TransportError::MessageTooLarge,
            ] {
                let error = selected_outcome(
                    Err(EngineError::Transport(primary.clone())),
                    None,
                    cancelled,
                    now,
                    now,
                    Some(now),
                )
                .unwrap_err();
                match error {
                    EngineError::Transport(actual) => {
                        assert_eq!(
                            std::mem::discriminant(&actual),
                            std::mem::discriminant(&primary)
                        );
                        if let TransportError::Grpc(code) = primary {
                            assert!(
                                matches!(actual, TransportError::Grpc(actual) if actual == code)
                            );
                        }
                    }
                    _ => panic!("selected transport error was replaced: {error:?}"),
                }
            }
        }
    }

    #[test]
    fn selected_candidates_and_local_timers_keep_cancel_hard_finalize_context_priority() {
        let start = Instant::now();
        let finalize = start + Duration::from_secs(1);
        let hard = start + Duration::from_secs(2);
        let candidates = [
            Ok(false),
            Ok(true),
            Err(EngineError::ContextTimeout),
            Err(EngineError::FinalizeTimeout),
            Err(EngineError::Transport(
                rambler_transport::TransportError::Timeout,
            )),
        ];
        for candidate in candidates {
            // With no expired global timer, retain the exact local outcome.
            let unchanged = selected_outcome(candidate.clone(), None, false, start, hard, None);
            match (&candidate, unchanged) {
                (Ok(expected), Ok(actual)) => assert_eq!(*expected, actual),
                (Err(expected), Err(actual)) => {
                    assert_eq!(
                        std::mem::discriminant(expected),
                        std::mem::discriminant(&actual)
                    );
                }
                (_, actual) => panic!("local outcome changed: {actual:?}"),
            }
            for (cancelled, at, expected) in [
                (false, finalize, "finalize"),
                (false, hard, "hard"),
                (true, start, "cancel"),
                (true, hard, "cancel"),
            ] {
                let result =
                    selected_outcome(candidate.clone(), None, cancelled, at, hard, Some(finalize));
                match expected {
                    "finalize" => assert!(matches!(result, Err(EngineError::FinalizeTimeout))),
                    "hard" => assert!(matches!(
                        result,
                        Err(EngineError::Transport(
                            rambler_transport::TransportError::Timeout
                        ))
                    )),
                    _ => assert!(matches!(result, Err(EngineError::Cancelled))),
                }
            }
        }
        // A restored source timer participates in the same tie rules.
        for (at, expected_hard) in [(finalize, false), (hard, true)] {
            let fault = EngineError::ContextTimeout;
            let result = selected_outcome(
                Err(EngineError::Transport(
                    rambler_transport::TransportError::Source(SourceError::new(fault.clone())),
                )),
                Some(&fault),
                false,
                at,
                hard,
                Some(finalize),
            );
            if expected_hard {
                assert!(matches!(
                    result,
                    Err(EngineError::Transport(
                        rambler_transport::TransportError::Timeout
                    ))
                ));
            } else {
                assert!(matches!(result, Err(EngineError::FinalizeTimeout)));
            }
        }
        assert!(matches!(
            selected_outcome(
                Err(EngineError::Cancelled),
                None,
                false,
                hard,
                hard,
                Some(finalize)
            ),
            Err(EngineError::Cancelled)
        ));
    }

    #[tokio::test]
    async fn selected_composite_restores_source_primary_before_late_cancel() {
        use super::test_common::{TestServer, bounded};
        struct FailedSource(SourceError);
        impl RequestSource for FailedSource {
            fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
                Box::pin(async { Err(self.0.clone()) })
            }
            fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
                Box::pin(async { Err(SourceError::new(std::io::Error::other("private cleanup"))) })
            }
        }
        bounded(async {
            let server = TestServer::new(|mut request| async move {
                while let Ok(Some(_)) = request.get_mut().message().await {}
                std::future::pending().await
            })
            .await;
            let transport = server.transport().await;
            let original = SourceError::new(std::io::Error::other("private source"));
            let primary = EngineError::Audio(original.clone());
            let mut rpc = transport
                .start(
                    FailedSource(SourceError::new(primary.clone())),
                    Metadata::default(),
                    Duration::from_secs(2),
                )
                .unwrap();
            let composite = rpc.next().await.unwrap_err();
            assert!(matches!(
                composite,
                rambler_transport::TransportError::PrimaryAndCleanup(_)
            ));
            // A real composite is obtained offline; cancellation/clock state at
            // the engine decision boundary is injected, not a claimed RPC race.
            let now = Instant::now();
            for cancelled in [false, true] {
                let result = selected_outcome(
                    Err(EngineError::Transport(composite.clone())),
                    Some(&primary),
                    cancelled,
                    now,
                    now,
                    Some(now),
                );
                match result {
                    Err(EngineError::Audio(actual)) => {
                        assert!(std::ptr::eq(actual.original(), original.original()));
                    }
                    other => panic!("composite source primary was replaced: {other:?}"),
                }
            }
            transport.close().await.unwrap();
            server.close().await;
        })
        .await;
    }

    #[tokio::test]
    async fn selected_real_failure_is_unobserved_and_claimable_by_control_exactly_once() {
        let original = SourceError::new(std::io::Error::other("private source"));
        let now = Instant::now();
        let outcome = selected_outcome(
            Err(EngineError::Audio(original.clone())),
            None,
            true,
            now,
            now,
            Some(now),
        )
        .map(|_| ());
        let (_sender, done) = watch::channel(Some(outcome));
        let control = Arc::new(Control {
            cancel: CancellationToken::new(),
            done,
            observed: AtomicBool::new(false),
            secondary: Mutex::new(None),
        });
        let (_events, events) = mpsc::channel(1);
        let mut session = EngineSession {
            control: control.clone(),
            events,
            captured: Arc::new(Mutex::new(None)),
            audio_progress: Arc::new(Mutex::new(EncodedAudioProgress::default())),
            diagnostic: SessionDiagnostics(Arc::new(Mutex::new(None))),
            terminal_delivered: false,
        };
        // This is a deterministic helper/control injection, not an end-to-end
        // reproduction of cancellation landing between select and arbitration.
        let error = tokio::time::timeout(Duration::from_secs(1), session.close_unobserved())
            .await
            .unwrap()
            .unwrap_err();
        match error {
            EngineError::Audio(actual) => {
                assert!(std::ptr::eq(actual.original(), original.original()))
            }
            _ => panic!("close_unobserved discarded a real failure: {error:?}"),
        }
        assert!(!control.observed.load(Ordering::SeqCst));
        assert!(control.needs_tracking());
        match control.claim_failure() {
            Some(EngineError::Audio(actual)) => {
                assert!(std::ptr::eq(actual.original(), original.original()))
            }
            other => panic!("unobserved failure was not claimable: {other:?}"),
        }
        assert!(control.claim_failure().is_none());
        assert!(!control.needs_tracking());
    }

    #[test]
    fn published_eof_cannot_be_revoked_by_cancel_or_expiration() {
        let now = Instant::now();
        let hard = now + Duration::from_secs(30);
        let finalize = now + Duration::from_secs(1);
        let logical_now = Cell::new(now);
        let cancel = CancellationToken::new();
        let (events, mut receiver) = mpsc::channel(1);
        let permit = events.try_reserve().unwrap();
        let result = commit_success(false, logical_now.get(), hard, Some(finalize), || {
            permit.send(event(None));
            // Force both changes in the exact publication window. They must not
            // reclassify the committed result, regardless of executor scheduling.
            cancel.cancel();
            logical_now.set(hard + Duration::from_secs(1));
        });
        assert!(matches!(result, Ok(Committed)));
        assert!(matches!(
            deadline_error(
                cancel.is_cancelled(),
                logical_now.get(),
                hard,
                Some(finalize)
            ),
            Some(EngineError::Cancelled)
        ));
        assert!(receiver.try_recv().unwrap().response.is_none());
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        drop(events);
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn rejected_commit_never_calls_publisher_and_preserves_priority() {
        let now = Instant::now();
        let hard = now + Duration::from_secs(30);
        let finalize = now + Duration::from_secs(1);
        for (cancelled, at, expected) in [
            (false, finalize, "finalize"),
            (false, hard, "hard"),
            (true, hard, "cancel"),
        ] {
            let published = Cell::new(false);
            let result =
                commit_success(cancelled, at, hard, Some(finalize), || published.set(true));
            assert!(!published.get());
            match expected {
                "finalize" => assert!(matches!(result, Err(EngineError::FinalizeTimeout))),
                "hard" => assert!(matches!(
                    result,
                    Err(EngineError::Transport(
                        rambler_transport::TransportError::Timeout
                    ))
                )),
                _ => assert!(matches!(result, Err(EngineError::Cancelled))),
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn full_event_queue_expires_without_publishing_eof() {
        let now = Instant::now();
        let hard = now + Duration::from_secs(30);
        let finalize = now + Duration::from_secs(1);
        let (_sender, done) = watch::channel(Some(finalize));
        let mut done = Some(done);
        let cancel = CancellationToken::new();
        let (events, mut receiver) = mpsc::channel(1);
        events
            .send(event(Some(decode_response(b"\x08\x01").unwrap())))
            .await
            .unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            reserve_success(&events, &cancel, hard, &mut done),
        )
        .await
        .expect("queue reservation exceeded its deadline");
        assert!(matches!(result, Err(EngineError::FinalizeTimeout)));
        drop(result);
        assert_eq!(Instant::now(), finalize);
        assert!(receiver.try_recv().unwrap().response.is_some());
        drop(events);
        assert!(
            receiver.recv().await.is_none(),
            "timed out queue contains an EOF"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn ready_permit_does_not_bypass_cancel_hard_or_finalize() {
        let now = Instant::now();
        let (events, _receiver) = mpsc::channel(1);
        let cancel = CancellationToken::new();
        cancel.cancel();
        let (_sender, done) = watch::channel(Some(now));
        let mut done = Some(done);
        assert!(matches!(
            reserve_success(&events, &cancel, now, &mut done).await,
            Err(EngineError::Cancelled)
        ));
        let cancel = CancellationToken::new();
        assert!(matches!(
            reserve_success(&events, &cancel, now, &mut done).await,
            Err(EngineError::Transport(
                rambler_transport::TransportError::Timeout
            ))
        ));
        assert!(matches!(
            reserve_success(&events, &cancel, now + Duration::from_secs(30), &mut done).await,
            Err(EngineError::FinalizeTimeout)
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn expiration_after_reservation_still_prevents_eof() {
        let now = Instant::now();
        let finalize = now + Duration::from_secs(1);
        let hard = now + Duration::from_secs(30);
        let cancel = CancellationToken::new();
        let (events, mut receiver) = mpsc::channel(1);
        let permit = reserve_success(&events, &cancel, hard, &mut None)
            .await
            .unwrap();
        tokio::time::advance(Duration::from_secs(1)).await;
        let result = commit_success(false, Instant::now(), hard, Some(finalize), || {
            permit.send(event(None));
        });
        assert!(matches!(result, Err(EngineError::FinalizeTimeout)));
        drop(events);
        assert!(receiver.recv().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn abandoned_consumer_prevents_commit() {
        let (events, receiver) = mpsc::channel(1);
        events
            .send(event(Some(decode_response(b"\x08\x01").unwrap())))
            .await
            .unwrap();
        let cancel = CancellationToken::new();
        let mut finalize = None;
        let reservation = reserve_success(
            &events,
            &cancel,
            Instant::now() + Duration::from_secs(30),
            &mut finalize,
        );
        tokio::pin!(reservation);
        tokio::select! {
            biased;
            _ = &mut reservation => panic!("reservation did not wait for the full queue"),
            _ = tokio::task::yield_now() => {},
        }
        drop(receiver);
        assert!(matches!(reservation.await, Err(EngineError::Cancelled)));
    }

    // Pure deadline arithmetic: a single immutable clock snapshot is advanced by
    // explicit values. No timer, socket, scheduler, or wall-clock wait is used.
    #[test]
    fn finalize_deadline_min_and_overflow() {
        let start = Instant::now();
        let hard = start + Duration::from_secs(30);
        let eof = start + Duration::from_secs(10);
        assert_eq!(
            finalize_deadline(eof, Duration::from_secs(2), hard),
            start + Duration::from_secs(12)
        );
        assert_eq!(finalize_deadline(eof, Duration::from_secs(60), hard), hard);
        assert!(eof.checked_add(Duration::MAX).is_none());
        assert_eq!(finalize_deadline(eof, Duration::MAX, hard), hard);
    }

    #[test]
    fn finalize_deadline_priority_and_none() {
        let start = Instant::now();
        let hard = start + Duration::from_secs(30);
        let finalize = start + Duration::from_secs(12);
        assert!(deadline_error(false, start, hard, None).is_none());
        assert!(deadline_error(false, finalize, hard, None).is_none());
        assert!(
            deadline_error(
                false,
                finalize - Duration::from_nanos(1),
                hard,
                Some(finalize)
            )
            .is_none()
        );
        assert!(matches!(
            deadline_error(false, finalize, hard, Some(finalize)),
            Some(EngineError::FinalizeTimeout)
        ));
        assert!(matches!(
            deadline_error(false, hard, hard, Some(finalize)),
            Some(EngineError::Transport(
                rambler_transport::TransportError::Timeout
            ))
        ));
        assert!(matches!(
            deadline_error(false, hard, hard, Some(hard)),
            Some(EngineError::Transport(
                rambler_transport::TransportError::Timeout
            ))
        ));
        assert!(matches!(
            deadline_error(true, hard, hard, Some(finalize)),
            Some(EngineError::Cancelled)
        ));
    }
}
