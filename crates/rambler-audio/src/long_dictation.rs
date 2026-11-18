use crate::{
    AudioError, EncoderConfig, FRAME_SAMPLES, MAX_INPUT_BYTES, Pcm16Opus, PcmSender,
    lifecycle::SourceOwner, pcm_channel,
};
use bytes::Bytes;
use rambler_engine::{
    AudioSource, CollectorLimits, ContextInput, DictationObservation, DictationResult, Engine,
    EngineError, RECOMMENDED_FINALIZE_TIMEOUT, SessionOptions, SourceError, collect_dictation,
    collect_dictation_with_observer,
};
use std::{fmt, future::Future, ops::Range, time::Duration};
use tokio::time::{Instant, sleep_until};

const SAMPLE_RATE: u128 = 16_000;
const PAUSE_SAMPLES: usize = FRAME_SAMPLES * 5; // 100 ms
const QUIET_RMS: u64 = 256;

/// Largest permitted audio budget for a single long-dictation segment.
pub const MAX_LONG_SEGMENT_BUDGET: Duration = Duration::from_secs(280);

/// Rollover policy for caller-provided 16 kHz mono PCM16LE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LongDictationOptions {
    /// Cut no later than this amount of admitted audio or elapsed segment time,
    /// whichever arrives first. Defaults to 240 s, at most 280 s, at least one
    /// sample. The PCM count rounds down to a whole sample.
    pub segment_budget: Duration,
    /// Look for a pause during the last part of the audio or wall budget.
    /// Defaults to 5 s; zero disables pause detection. Must be less than the
    /// budget (also after rounding both down to whole samples).
    pub pause_search: Duration,
}
impl Default for LongDictationOptions {
    fn default() -> Self {
        Self {
            segment_budget: Duration::from_secs(240),
            pause_search: Duration::from_secs(5),
        }
    }
}

/// Opt-in budgets for interactive, context-free PCM dictation via
/// [`collect_long_pcm_dictation`] or [`stream_long_pcm_dictation`].
/// The current defaults are a 120 s segment, 5 s pause search, 5 s separate
/// finalization budget, and a 135 s overall deadline **per RPC**. There is no
/// fixed duration limit for a whole capture: the host owns stopping its source.
///
/// Edit `session` to configure the protocol, credentials, or explicit deadlines.
/// The collector does not reinterpret those deadlines or extend them on rollover.
#[derive(Clone)]
pub struct InteractivePcmProfile {
    pub segmentation: LongDictationOptions,
    pub session: SessionOptions,
}
impl Default for InteractivePcmProfile {
    fn default() -> Self {
        Self::new(
            LongDictationOptions {
                segment_budget: Duration::from_secs(120),
                ..LongDictationOptions::default()
            },
            RECOMMENDED_FINALIZE_TIMEOUT,
        )
        .expect("valid built-in interactive profile")
    }
}
impl InteractivePcmProfile {
    /// Derive the per-RPC overall deadline as segment budget + 10 s for
    /// setup/drain + the separate finalization budget. The latter starts at
    /// clean encoded-source EOF and cleanup, not at the last microphone sample.
    /// The overall deadline may expire first. Explicit overrides on `session`
    /// keep their usual semantics; change segmentation via this constructor to
    /// derive a matching deadline rather than mutating the segment config alone.
    pub fn new(
        segmentation: LongDictationOptions,
        finalize_timeout: Duration,
    ) -> Result<Self, EngineError> {
        Policy::new(segmentation)?;
        if finalize_timeout.is_zero() {
            return Err(EngineError::InvalidConfig);
        }
        let timeout = segmentation
            .segment_budget
            .checked_add(Duration::from_secs(10))
            .and_then(|duration| duration.checked_add(finalize_timeout))
            .ok_or(EngineError::InvalidConfig)?;
        Ok(Self {
            segmentation,
            session: SessionOptions {
                timeout,
                finalize_timeout: Some(finalize_timeout),
                ..SessionOptions::default()
            },
        })
    }
}

/// Cleanly completed sessions in capture order. No editor context is sent.
#[derive(Default)]
pub struct LongDictationResult {
    pub segments: Vec<DictationResult>,
}
impl fmt::Debug for LongDictationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LongDictationResult")
            .field("segment_count", &self.segments.len())
            .finish_non_exhaustive()
    }
}
impl LongDictationResult {
    /// Strict text for every segment, joined using [`crate::join_segment_texts`].
    /// An ambiguous or unsafe segment makes the entire projection fail. No
    /// segments yields an empty string. The individual results are unchanged.
    pub fn text(&self) -> Result<String, EngineError> {
        let mut joined = crate::segment_text::SegmentText::default();
        for segment in &self.segments {
            joined.push(&segment.text()?);
        }
        Ok(joined.finish())
    }
}

/// The first failure, with only the preceding cleanly completed sessions.
/// Neither this helper nor its error path replays or resumes audio.
#[derive(Debug, thiserror::Error)]
#[error("long dictation failed after {} completed segment(s): {cause}", .completed.segments.len())]
pub struct LongDictationError {
    pub completed: LongDictationResult,
    #[source]
    pub cause: EngineError,
}

/// Response previews remain provisional, including final-looking responses and END.
#[non_exhaustive]
pub enum LongDictationObservation<'a> {
    Response {
        /// Zero-based index in capture order.
        segment_index: usize,
        observation: DictationObservation<'a>,
    },
    /// Signals that the segment completed successfully; clear its preview here.
    /// This signal does not imply that strict text can be projected.
    SegmentCompleted {
        /// Zero-based index in capture order.
        segment_index: usize,
        /// Absolute half-open sample range in the 16 kHz input to this helper
        /// (after any caller-supplied resampling). Excludes Opus padding.
        sample_range: Range<u64>,
    },
}

/// Ownership of one terminally successful RPC's result and its input location.
/// Success does not imply that the result contains safe or unambiguous text.
pub struct CompletedDictationSegment {
    /// Zero-based index in capture order.
    pub segment_index: usize,
    /// Absolute half-open range in the collector's 16 kHz PCM input, excluding
    /// Opus padding. If resampling upstream, these are post-resampling indices.
    pub sample_range: Range<u64>,
    /// Structured finals; use `result.render(...)` for safety-checked text.
    pub result: DictationResult,
}
impl fmt::Debug for CompletedDictationSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompletedDictationSegment")
            .field("segment_index", &self.segment_index)
            .field("sample_range", &self.sample_range)
            .finish_non_exhaustive()
    }
}

type Observer<'a> = &'a mut (dyn for<'event> FnMut(LongDictationObservation<'event>) + Send + 'a);
type SegmentSink<'a> = &'a mut (dyn FnMut(CompletedDictationSegment) + Send + 'a);

/// Collect successive live-PCM segments on an existing shared engine.
///
/// Pass a [`crate::PcmInput`] (drive its producer concurrently) or compose with
/// [`crate::Pcm48kTo16k`] for caller-owned 48 kHz PCM. Source chunks may split
/// samples at arbitrary byte boundaries (maximum logical length: 8 MiB per
/// chunk); clean EOF requires a whole number of samples. No RPC is opened until
/// a complete sample is available, including after rollover; empty input opens
/// no sessions. Input may backpressure while each segment finalizes. Use a
/// bounded handoff outside capture callbacks.
///
/// The search starts when either the sample count or the wall clock enters the
/// configured search window. Cut at the end of the first full non-overlapping
/// 100 ms block in that window whose RMS is at most 256 PCM16 units, otherwise
/// at the sample or wall budget. This is a low-energy heuristic, not VAD. Wall
/// time can force an earlier cut if capture or upload is slow. No samples are
/// discarded or repeated at a cut, and each segment has a fresh Opus/Ogg stream.
///
/// Each writer finishes normally at a cut, and its collector must complete before
/// the next session starts. Pass `SessionOptions` unchanged per segment, except
/// that a non-`None` context is rejected. Use [`InteractivePcmProfile`] for
/// ready-to-use capture and finalization budgets, or raise the ordinary 30 s
/// [`SessionOptions::timeout`] yourself to leave room for audio and finalization
/// (for example, 270 s with a 240 s segment budget). The segment budget ends
/// audio intake, not finalization; a slow peer can still fail. Collector retention
/// limits apply per segment, not to the accumulated result. No session deadline
/// runs while waiting for the first sample of a segment; bound overall capture
/// and between-segment idle waits at the application level.
///
/// On failure, remaining capture is closed and the error owns earlier committed
/// segments. As with other [`AudioSource`] consumers, the source's `next` must
/// tolerate cancellation and its `close` must cooperate: close is awaited even
/// beyond session deadlines. Dropping the future returns no result, cancels any
/// active session, and schedules best-effort source closure on the live runtime.
/// Keep Tokio alive and use [`Engine::close`] at application shutdown to await
/// engine cleanup.
pub fn collect_long_pcm_dictation(
    engine: &Engine,
    pcm: impl AudioSource,
    session: SessionOptions,
    limits: CollectorLimits,
    options: LongDictationOptions,
) -> impl Future<Output = Result<LongDictationResult, LongDictationError>> + Send + '_ {
    collect_accumulated(
        engine,
        SourceOwner::new(pcm),
        session,
        limits,
        options,
        None,
    )
}

/// Like [`collect_long_pcm_dictation`], with a synchronous, nonblocking observer.
/// Preview is per segment and must be cleared on `SegmentCompleted`, failure or
/// cancellation. This callback is not an authorization for editor/system effects.
pub fn collect_long_pcm_dictation_with_observer<'a>(
    engine: &'a Engine,
    pcm: impl AudioSource,
    session: SessionOptions,
    limits: CollectorLimits,
    options: LongDictationOptions,
    observer: Observer<'a>,
) -> impl Future<Output = Result<LongDictationResult, LongDictationError>> + Send + 'a {
    collect_accumulated(
        engine,
        SourceOwner::new(pcm),
        session,
        limits,
        options,
        Some(observer),
    )
}

/// Run the same segmentation and per-RPC collection as [`collect_long_pcm_dictation`],
/// moving each successful result to `sink` exactly once, in order, before opening
/// the next RPC. The collector does not retain delivered results or copy the
/// whole capture. Limits and backpressure still apply per segment.
///
/// The sink runs synchronously in the collecting task; keep it nonblocking. The
/// SDK does not log or queue its results. The structured result can contain
/// private or safety-blocked candidates. Do not log its text or apply editor
/// effects; use [`DictationResult::check_safety`] or [`DictationResult::render`]
/// when deciding whether a result may be used.
///
/// For a normal stop, finish the source (e.g. [`PcmSender::finish`]) and keep
/// polling: the collector drains admitted PCM and finalizes the last nonempty
/// segment. `Ok(())` means the whole source completed and closed successfully.
/// On a later error the collector closes the source and returns the cause, with
/// no replay and no result for the failed segment. For an explicit abort, drop
/// the future: this cancels the active RPC and schedules best-effort source
/// closure on the live Tokio runtime. Ownership of earlier deliveries is never
/// revoked. Keep the runtime alive and use [`Engine::close`] at shutdown to await
/// outstanding engine cleanup.
pub fn stream_long_pcm_dictation<'a>(
    engine: &'a Engine,
    pcm: impl AudioSource,
    session: SessionOptions,
    limits: CollectorLimits,
    options: LongDictationOptions,
    sink: SegmentSink<'a>,
) -> impl Future<Output = Result<(), EngineError>> + Send + 'a {
    collect(
        engine,
        SourceOwner::new(pcm),
        session,
        limits,
        options,
        sink,
        None,
    )
}

/// Like [`stream_long_pcm_dictation`], with a synchronous, nonblocking preview
/// observer. Its unchanged `SegmentCompleted` event follows owned delivery of
/// that segment. Clear previews on that event, failure or cancellation.
pub fn stream_long_pcm_dictation_with_observer<'a>(
    engine: &'a Engine,
    pcm: impl AudioSource,
    session: SessionOptions,
    limits: CollectorLimits,
    options: LongDictationOptions,
    sink: SegmentSink<'a>,
    observer: Observer<'a>,
) -> impl Future<Output = Result<(), EngineError>> + Send + 'a {
    collect(
        engine,
        SourceOwner::new(pcm),
        session,
        limits,
        options,
        sink,
        Some(observer),
    )
}

#[derive(Clone, Copy)]
struct Policy {
    max_samples: usize,
    search_start: usize,
    budget: Duration,
    pause_search: Duration,
}
impl Policy {
    fn new(options: LongDictationOptions) -> Result<Self, EngineError> {
        let budget = options.segment_budget;
        let pause_search = options.pause_search;
        if budget > MAX_LONG_SEGMENT_BUDGET || pause_search >= budget {
            return Err(EngineError::InvalidConfig);
        }
        // Both durations are bounded above by 280 s before multiplication.
        let samples =
            |duration: Duration| (duration.as_nanos() * SAMPLE_RATE / 1_000_000_000) as usize;
        let max_samples = samples(budget);
        let search_samples = samples(pause_search);
        if search_samples >= max_samples {
            return Err(EngineError::InvalidConfig);
        }
        Ok(Self {
            max_samples,
            search_start: max_samples - search_samples,
            budget,
            pause_search,
        })
    }
}

// Retains at most one source chunk and an orphan low byte across rollovers.
struct PcmRead<S: AudioSource> {
    source: SourceOwner<S>,
    pending: Bytes,
    low_byte: Option<u8>,
}
impl<S: AudioSource> PcmRead<S> {
    fn available(&self) -> usize {
        if self.low_byte.is_some() {
            usize::from(!self.pending.is_empty())
        } else {
            self.pending.len() / 2
        }
    }

    async fn fill(&mut self) -> Result<bool, EngineError> {
        while self.available() == 0 {
            if let [low] = self.pending.as_ref() {
                self.low_byte = Some(*low);
                self.pending = Bytes::new();
            }
            let Some(source) = self.source.source.as_mut() else {
                return Ok(false);
            };
            tokio::task::consume_budget().await;
            match source.next().await.map_err(EngineError::Audio)? {
                None if self.low_byte.is_some() => {
                    return Err(EngineError::Audio(SourceError::new(
                        AudioError::TruncatedPcm,
                    )));
                }
                None => return Ok(false),
                Some(chunk) if chunk.len() > MAX_INPUT_BYTES => {
                    return Err(EngineError::Audio(SourceError::new(AudioError::InputLimit)));
                }
                Some(chunk) => self.pending = chunk,
            }
        }
        Ok(true)
    }

    fn take(&mut self, samples: usize) -> Bytes {
        if let Some(low) = self.low_byte.take() {
            debug_assert_eq!(samples, 1);
            Bytes::copy_from_slice(&[low, self.pending.split_to(1)[0]])
        } else {
            self.pending.split_to(samples * 2)
        }
    }

    async fn close(&mut self) -> Result<(), EngineError> {
        self.source.close().await.map_err(EngineError::AudioCleanup)
    }
}

async fn collect_accumulated<S: AudioSource>(
    engine: &Engine,
    source: SourceOwner<S>,
    session: SessionOptions,
    limits: CollectorLimits,
    options: LongDictationOptions,
    observer: Option<Observer<'_>>,
) -> Result<LongDictationResult, LongDictationError> {
    let mut completed = LongDictationResult::default();
    let outcome = collect(
        engine,
        source,
        session,
        limits,
        options,
        |segment| completed.segments.push(segment.result),
        observer,
    )
    .await;
    match outcome {
        Ok(()) => Ok(completed),
        Err(cause) => Err(LongDictationError { completed, cause }),
    }
}

async fn collect<S: AudioSource>(
    engine: &Engine,
    source: SourceOwner<S>,
    session: SessionOptions,
    limits: CollectorLimits,
    options: LongDictationOptions,
    mut sink: impl FnMut(CompletedDictationSegment) + Send,
    mut observer: Option<Observer<'_>>,
) -> Result<(), EngineError> {
    let mut pcm = PcmRead {
        source,
        pending: Bytes::new(),
        low_byte: None,
    };
    let outcome = async {
        let policy = Policy::new(options)?;
        if !matches!(session.context, ContextInput::None) {
            return Err(EngineError::InvalidConfig);
        }
        let mut sample_start = 0u64;
        let mut segment_index = 0usize;
        let overflow = || EngineError::Audio(SourceError::new(AudioError::CounterOverflow));
        while pcm.fill().await? {
            let next_index = segment_index.checked_add(1).ok_or_else(overflow)?;
            // Queue one complete sample before starting the wall clock so even
            // very small budgets cannot create an empty rollover loop.
            let (mut writer, input) = pcm_channel();
            writer
                .reserve()
                .await
                .map_err(|_| EngineError::Supervisor)?
                .send(pcm.take(1))
                .map_err(|_| EngineError::Supervisor)?;
            let audio = Box::new(Pcm16Opus::new(input, EncoderConfig::default()));
            let cutoff = Instant::now() + policy.budget;
            let (result, samples) = if let Some(observer) = observer.as_deref_mut() {
                let mut observe = |observation: DictationObservation<'_>| {
                    observer(LongDictationObservation::Response {
                        segment_index,
                        observation,
                    });
                };
                let completion = collect_dictation_with_observer(
                    engine,
                    audio,
                    session.clone(),
                    limits,
                    &mut observe,
                );
                drive_segment(&mut pcm, policy, cutoff, writer, completion).await?
            } else {
                let completion = collect_dictation(engine, audio, session.clone(), limits);
                drive_segment(&mut pcm, policy, cutoff, writer, completion).await?
            };
            let sample_end = sample_start
                .checked_add(samples as u64)
                .ok_or_else(overflow)?;
            sink(CompletedDictationSegment {
                segment_index,
                sample_range: sample_start..sample_end,
                result,
            });
            if let Some(observer) = observer.as_deref_mut() {
                observer(LongDictationObservation::SegmentCompleted {
                    segment_index,
                    sample_range: sample_start..sample_end,
                });
            }
            sample_start = sample_end;
            segment_index = next_index;
        }
        Ok(())
    }
    .await;
    let cleanup = pcm.close().await;
    outcome.and(cleanup)
}

async fn drive_segment<S: AudioSource>(
    pcm: &mut PcmRead<S>,
    policy: Policy,
    cutoff: Instant,
    writer: PcmSender,
    completion: impl Future<Output = Result<DictationResult, EngineError>>,
) -> Result<(DictationResult, usize), EngineError> {
    let mut samples = 1; // First sample was queued before opening the segment.
    tokio::pin!(completion);
    let result = tokio::select! {
        biased;
        result = &mut completion => result,
        fed = feed_segment(pcm, policy, cutoff, writer, &mut samples) => {
            // Await collector/engine cleanup even on a source failure; a failed
            // producer drops its writer without finish rather than faking EOF.
            fed.and(completion.await)
        }
    }?;
    Ok((result, samples))
}

enum FeedEnd {
    Cut,
    Eof,
    Closed,
}

async fn feed_segment<S: AudioSource>(
    pcm: &mut PcmRead<S>,
    policy: Policy,
    cutoff: Instant,
    mut writer: PcmSender,
    samples: &mut usize,
) -> Result<(), EngineError> {
    let end = tokio::select! {
        biased;
        _ = sleep_until(cutoff) => FeedEnd::Cut,
        result = pump(pcm, policy, cutoff, &mut writer, samples) => result?,
    };
    match end {
        FeedEnd::Eof => pcm.close().await?,
        FeedEnd::Closed => return Ok(()), // Only the collector reports engine failure.
        FeedEnd::Cut => (),
    }
    let _ = writer.finish(); // Closure racing finish also belongs to the collector.
    Ok(())
}

async fn pump<S: AudioSource>(
    pcm: &mut PcmRead<S>,
    policy: Policy,
    cutoff: Instant,
    writer: &mut PcmSender,
    samples: &mut usize,
) -> Result<FeedEnd, EngineError> {
    let mut block_len = 0;
    let mut sum_squares = 0;
    while *samples < policy.max_samples {
        if !pcm.fill().await? {
            return Ok(FeedEnd::Eof);
        }
        let Ok(permit) = writer.reserve().await else {
            return Ok(FeedEnd::Closed);
        };
        let now = Instant::now();
        if now >= cutoff {
            return Ok(FeedEnd::Cut);
        }
        let searching = !policy.pause_search.is_zero()
            && (*samples >= policy.search_start || now >= cutoff - policy.pause_search);
        let boundary = if searching {
            PAUSE_SAMPLES - block_len
        } else {
            // Don't skip over the beginning of the sample-based search window.
            policy.search_start - *samples
        };
        let n = pcm
            .available()
            .min(PAUSE_SAMPLES)
            .min(policy.max_samples - *samples)
            .min(boundary);
        let chunk = pcm.take(n);
        if searching {
            sum_squares += chunk
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&bytes| {
                    let sample = i64::from(i16::from_le_bytes(bytes));
                    (sample * sample) as u64
                })
                .sum::<u64>();
            block_len += n;
        }
        if permit.send(chunk).is_err() {
            return Ok(FeedEnd::Closed);
        }
        *samples += n;
        if block_len == PAUSE_SAMPLES {
            if sum_squares <= QUIET_RMS * QUIET_RMS * PAUSE_SAMPLES as u64 {
                return Ok(FeedEnd::Cut);
            }
            block_len = 0;
            sum_squares = 0;
        }
    }
    Ok(FeedEnd::Cut)
}
