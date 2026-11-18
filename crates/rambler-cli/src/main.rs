#[cfg(test)]
mod tests;

use clap::{Args, Parser, Subcommand, ValueEnum};
use rambler_engine::{
    AudioSource, CollectorLimits, ContextInput, DictationResult, EditingContext,
    EncodedAudioProgress, Engine, EngineError, OggAudio, ProtocolConfig, RenderPolicy,
    SessionDiagnostic, SessionOptions, TextProvenance, collect_session,
};
use rambler_transport::{
    EnvironmentCredentials, Metadata, MetadataProvider, TransportConfig, TransportError,
};
use serde_json::{Value, json};
use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

#[derive(Parser)]
#[command(
    name = "rambler",
    about = "Dictate audio files without a microphone or external encoder."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Stream bounded, validated mono/16 kHz Ogg/Opus without decoding.
    Dictate {
        #[command(flatten)]
        common: CommonArgs,
    },
    #[cfg(feature = "audio")]
    /// Encode PCM16LE mono 16 kHz WAVE and stream Ogg/Opus.
    DictateWav {
        #[command(flatten)]
        common: CommonArgs,
        /// Pace the file at its audio duration rather than sending as fast as possible.
        #[arg(long, conflicts_with = "interactive")]
        realtime: bool,
        /// Pace context-free PCM and roll over into independent sessions (default 120 s each).
        #[arg(long, conflicts_with_all = ["before", "selected", "after", "cursor_aware", "page_duration_ms"])]
        interactive: bool,
        /// Maximum audio/elapsed seconds per interactive segment (6–280; default 120).
        #[arg(long, requires = "interactive", value_parser = clap::value_parser!(u64).range(6..=280))]
        segment_seconds: Option<u64>,
        /// Ogg page duration in milliseconds, a multiple of 20 from 20 to 1000.
        #[arg(long, default_value_t = 20)]
        page_duration_ms: u32,
    },
}
#[derive(Args)]
struct CommonArgs {
    input: PathBuf,
    #[arg(long, default_value = "fr-FR")]
    language: String,
    #[arg(long)]
    before: Option<String>,
    #[arg(long)]
    selected: Option<String>,
    #[arg(long)]
    after: Option<String>,
    #[arg(long)]
    cursor_aware: bool,
    /// Final text projection (default: strict). Explicit selection reports text provenance.
    #[arg(long, value_enum)]
    result_policy: Option<ResultPolicy>,
    /// Overall seconds per RPC (default 30; interactive: segment + 10 + finalization).
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    timeout: Option<u64>,
    /// Seconds after audio EOF and cleanup (default: off; interactive: 5).
    /// The overall --timeout still applies.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    finalize_timeout: Option<u64>,
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u64).range(1..))]
    context_timeout: u64,
    #[arg(long)]
    bias_term: Vec<String>,
    #[arg(long)]
    preference: Vec<String>,
    #[arg(long)]
    secondary_language: Vec<String>,
    /// Explicit RecognizerInfo preference; omission preserves the server default.
    #[arg(long, action = clap::ArgAction::Set)]
    block_offensive_words: Option<bool>,
    /// Explicit auto-punctuation preference (server effect unverified).
    #[arg(long, action = clap::ArgAction::Set)]
    auto_punctuation: Option<bool>,
    /// Optional secondary S3 routing key; no automatic route discovery.
    #[arg(long)]
    secondary_routing: Option<String>,
    /// Package name of the editor app being dictated into (ClientInfo field 16).
    #[arg(long)]
    trigger_application_id: Option<String>,
    /// Test-only plaintext endpoint; only loopback addresses are accepted.
    #[arg(long)]
    loopback: Option<SocketAddr>,
}

#[derive(Clone, Copy, ValueEnum)]
enum ResultPolicy {
    /// Use the conservative SDK text projection.
    Strict,
    /// Project the final recognition hypothesis text only.
    AsrOnly,
    /// Also accept VoiceEdit rewrites of the captured window.
    S3ClientCompatible,
}
impl From<ResultPolicy> for RenderPolicy {
    fn from(policy: ResultPolicy) -> Self {
        match policy {
            ResultPolicy::Strict => Self::Strict,
            ResultPolicy::AsrOnly => Self::AsrOnly,
            ResultPolicy::S3ClientCompatible => Self::S3ClientCompatible,
        }
    }
}

enum InputKind {
    Ogg,
    #[cfg(feature = "audio")]
    Wav {
        realtime: bool,
        page_duration_ms: u32,
    },
    #[cfg(feature = "audio")]
    InteractiveWav {
        segment_seconds: Option<u64>,
    },
}

#[derive(Default)]
struct PcmObservation {
    #[cfg(feature = "audio")]
    handle: Option<rambler_audio::PcmProgressHandle>,
}
impl PcmObservation {
    /// Read only after source/session cleanup; these are encoder, not network, counters.
    fn append_to(&self, value: Value) -> Value {
        #[cfg(feature = "audio")]
        {
            let mut value = value;
            if let Some(handle) = &self.handle {
                let progress = handle.snapshot();
                value["pcm"] = json!({
                    "input_bytes": progress.input_bytes,
                    "encoded_input_samples": progress.encoded_input_samples,
                    "encoded_frames": progress.encoded_frames,
                    "input_eof": progress.input_eof,
                    "output_eof": progress.output_eof,
                });
            }
            value
        }
        #[cfg(not(feature = "audio"))]
        {
            value
        }
    }
}

async fn open_audio(
    path: PathBuf,
    kind: InputKind,
) -> Result<(Box<dyn AudioSource>, PcmObservation), &'static str> {
    match kind {
        InputKind::Ogg => Ok((
            Box::new(OggAudio::open(path).await.map_err(|_| "audio")?),
            PcmObservation::default(),
        )),
        #[cfg(feature = "audio")]
        InputKind::InteractiveWav { .. } => {
            use rambler_audio::{WavAudio, WavConfig};
            let pcm = WavAudio::open(
                path,
                WavConfig {
                    chunk_ms: 20,
                    realtime: true,
                },
            )
            .await
            .map_err(|_| "audio")?;
            Ok((Box::new(pcm), PcmObservation::default()))
        }
        #[cfg(feature = "audio")]
        InputKind::Wav {
            realtime,
            page_duration_ms,
        } => {
            use rambler_audio::{EncoderConfig, Pcm16Opus, WavAudio, WavConfig};
            let page_duration_ms = u16::try_from(page_duration_ms).map_err(|_| "configuration")?;
            if page_duration_ms % 20 != 0 {
                return Err("configuration");
            }
            let encoder = EncoderConfig {
                page_duration_ms,
                ..Default::default()
            };
            encoder.validate().map_err(|_| "configuration")?;
            let wav = WavAudio::open(
                path,
                WavConfig {
                    chunk_ms: 20,
                    realtime,
                },
            )
            .await
            .map_err(|_| "audio")?;
            let (encoder, handle) = Pcm16Opus::new_with_progress(wav, encoder);
            Ok((
                Box::new(encoder),
                PcmObservation {
                    handle: Some(handle),
                },
            ))
        }
    }
}

fn error_category(e: &EngineError) -> (&'static str, Option<String>) {
    match e {
        EngineError::Transport(TransportError::Grpc(code)) => ("grpc", Some(format!("{code:?}"))),
        EngineError::FinalizeTimeout => ("finalize_timeout", None),
        EngineError::Transport(TransportError::Timeout) | EngineError::ContextTimeout => {
            ("timeout", None)
        }
        EngineError::Transport(
            TransportError::IncompleteCredentials
            | TransportError::InvalidMetadata
            | TransportError::Credentials,
        ) => ("credentials", None),
        EngineError::Transport(_) => ("transport", None),
        EngineError::S3(_) => ("s3", None),
        EngineError::Audio(_) => ("audio", None),
        EngineError::AudioCleanup(_) => ("cleanup", None),
        EngineError::Protocol(_) => ("protocol", None),
        EngineError::SafetyBlocked => ("safety_blocked", None),
        EngineError::UnrecognizedSafety => ("unrecognized_safety", None),
        EngineError::AmbiguousResult => ("ambiguous", None),
        EngineError::ResultLimit => ("result_limit", None),
        EngineError::IncompleteSession(_) => ("incomplete", None),
        EngineError::InvalidConfig => ("configuration", None),
        _ => ("engine", None),
    }
}
fn context(c: &EditingContext) -> Value {
    json!({"before": c.before, "selected": c.selected, "after": c.after, "cursor_aware": c.cursor_aware})
}
fn audio_progress(progress: &EncodedAudioProgress) -> Value {
    json!({"chunks":progress.chunks, "bytes":progress.bytes, "eof":progress.eof,
        "cleanup_finished":progress.cleanup_finished, "cleanup_succeeded":progress.cleanup_succeeded})
}
fn output(
    result: &DictationResult,
    policy: Option<ResultPolicy>,
) -> Result<(Value, bool), EngineError> {
    // Rendering checks all retained candidates for safety before it can be ambiguous.
    let rendered = match result.render(policy.unwrap_or(ResultPolicy::Strict).into()) {
        Ok(rendered) => Some(rendered),
        Err(EngineError::AmbiguousResult) => None,
        Err(error) => return Err(error),
    };
    let finals: Vec<_> = result.final_updates.iter().map(|r| {
        let hypotheses: Vec<_> = r.hypotheses().iter().map(|h| {
            let edit = h.voice_edit.as_ref().map(|e| json!({"transcript": e.transcript, "alternative_text": e.alternative_text, "edited_text": e.edited_text, "edited_context": e.edited_context.as_ref().map(context), "intent": e.intent, "active_field": e.active_field.filter(|i| [2,3,7].contains(i))}));
            json!({"text":h.text, "confidence":h.confidence.filter(|v| v.is_finite()), "voice_edit":edit, "safety_status":h.safety_status})
        }).collect();
        json!({"hypotheses":hypotheses})
    }).collect();
    let metrics = result.metrics.as_ref().map(|m| json!({"audio":audio_progress(&m.audio), "requests_produced":m.requests_produced, "request_protobuf_bytes":m.request_protobuf_bytes, "responses_received":m.responses_received, "response_protobuf_bytes":m.response_protobuf_bytes, "elapsed_seconds":m.elapsed_seconds, "first_response_seconds":m.first_response_seconds, "eof_seconds":m.eof_seconds}));
    let ambiguous = rendered.is_none();
    let mut value = json!({
        "type": if ambiguous { "ambiguous" } else { "result" },
        "text": rendered.as_ref().map(|r| &r.text),
        "final_updates": finals,
        "context": result.context.as_ref().map(context),
        "metrics": metrics,
    });
    // Leave the legacy JSON unchanged when the flag is omitted.
    if let (Some(_), Some(rendered)) = (policy, rendered) {
        value["provenance"] = json!(match rendered.provenance {
            TextProvenance::HypothesisText => "hypothesis_text",
            TextProvenance::VoiceEditText => "voice_edit_text",
            TextProvenance::VoiceEditContext => "voice_edit_context",
        });
    }
    Ok((value, ambiguous))
}
fn print_error(category: &str, code: Option<String>) {
    println!(
        "{}",
        json!({"type":"error", "category":category, "grpc_code":code, "timeline":null})
    );
}

fn with_timeline(mut value: Value, diagnostic: Option<&SessionDiagnostic>) -> Value {
    value["timeline"] = diagnostic.map_or(Value::Null, |diagnostic| {
        let metrics = &diagnostic.metrics;
        let timeline = &metrics.timeline;
        json!({
            "elapsed_seconds": metrics.elapsed_seconds,
            "first_response_seconds": metrics.first_response_seconds,
            "last_audio_sent_seconds": timeline.last_audio_sent_seconds,
            "context_sent_seconds": timeline.context_sent_seconds,
            "end_sent_seconds": timeline.end_sent_seconds,
            "half_close_sent_seconds": timeline.half_close_sent_seconds,
            "last_response_seconds": timeline.last_response_seconds,
            "last_response_s3_status": timeline.last_response_s3_status,
            "last_partial_response_seconds": timeline.last_partial_response_seconds,
            "recognition_finals_received": timeline.recognition_finals_received,
            "requests_produced": metrics.requests_produced,
            "requests_submitted": timeline.requests_submitted,
            "request_protobuf_bytes": metrics.request_protobuf_bytes,
            "request_protobuf_bytes_submitted": timeline.request_protobuf_bytes_submitted,
            "responses_received": metrics.responses_received,
            "responses_deframed": diagnostic.completion.responses_received,
            "response_protobuf_bytes": metrics.response_protobuf_bytes,
            "audio_source_eof": metrics.audio.eof,
            "audio_cleanup_succeeded": metrics.audio.cleanup_succeeded,
            "request_source_exhausted": diagnostic.completion.source_exhausted,
            "saw_s3_end": diagnostic.saw_s3_end,
            "terminal_grpc_code": diagnostic.completion.terminal_code.map(|code| format!("{code:?}")),
        })
    });
    value
}

fn failure_output(
    error: &EngineError,
    cleanup: Option<&EngineError>,
    diagnostic: Option<&SessionDiagnostic>,
) -> Value {
    let (category, code) = error_category(error);
    let (close_category, close_code) = cleanup
        .map(error_category)
        .map_or((None, None), |(category, code)| (Some(category), code));
    let mut value = json!({"type":"error", "category":category, "grpc_code":code,
        "close_error_category":close_category, "close_grpc_code":close_code});
    if let EngineError::IncompleteSession(diagnostic) = error {
        let completion = &diagnostic.completion;
        let metrics = &diagnostic.metrics;
        value["incomplete"] = json!({
            "saw_s3_end": diagnostic.saw_s3_end,
            "completion": {
                "requests_produced": completion.requests_produced,
                "requests_polled": completion.requests_polled,
                "responses_received": completion.responses_received,
                "source_exhausted": completion.source_exhausted,
                "upload_eof_polled": completion.upload_eof_polled,
                "source_closed": completion.source_closed,
                "terminal_code": completion.terminal_code.map(|code| format!("{code:?}")),
            },
            "metrics": {
                "audio": audio_progress(&metrics.audio),
                "requests_produced": metrics.requests_produced,
                "request_protobuf_bytes": metrics.request_protobuf_bytes,
                "responses_received": metrics.responses_received,
                "response_protobuf_bytes": metrics.response_protobuf_bytes,
                "elapsed_seconds": metrics.elapsed_seconds,
                "first_response_seconds": metrics.first_response_seconds,
                "eof_seconds": metrics.eof_seconds,
            }
        });
        return with_timeline(value, Some(diagnostic));
    }
    with_timeline(value, diagnostic)
}

/// Add no invented measurements when the session never started; still await cleanup.
async fn pre_session_failure_output(audio: &mut dyn AudioSource, error: &EngineError) -> Value {
    let cleanup_failed = audio.close().await.is_err();
    let (category, code) = error_category(error);
    let mut value = json!({"type":"error", "category":category, "grpc_code":code, "timeline":null});
    if cleanup_failed {
        value["close_error_category"] = json!("cleanup");
    }
    value
}

async fn admit_credentials(
    audio: &mut dyn AudioSource,
    loopback: bool,
    required_snapshot: impl FnOnce() -> Result<Metadata, TransportError>,
) -> Result<Option<Arc<dyn MetadataProvider>>, Value> {
    if loopback {
        // Explicit loopback must not even inspect real credential variables.
        return Ok(None);
    }
    match required_snapshot() {
        Ok(snapshot) => Ok(Some(Arc::new(snapshot))),
        Err(error) => Err(pre_session_failure_output(audio, &EngineError::Transport(error)).await),
    }
}

async fn supervise<T, E, CloseError, C, Close, Signal>(
    collection: C,
    close: impl FnOnce() -> Close,
    signal: Signal,
) -> (Option<Result<T, E>>, Result<(), CloseError>, Option<bool>)
where
    C: std::future::Future<Output = Result<T, E>>,
    Close: std::future::Future<Output = Result<(), CloseError>>,
    Signal: std::future::Future<Output = bool>,
{
    tokio::pin!(signal);
    // Drop the owning collector before requesting engine shutdown.
    let (result, mut interrupted) = {
        tokio::pin!(collection);
        tokio::select! {
            biased;
            ok = &mut signal => (None, Some(ok)),
            result = &mut collection => (Some(result), None),
        }
    };
    let cleanup = close();
    tokio::pin!(cleanup);
    let cleanup = if interrupted.is_some() {
        cleanup.await
    } else {
        tokio::select! {
            biased;
            ok = &mut signal => { interrupted = Some(ok); cleanup.await },
            result = &mut cleanup => result,
        }
    };
    (result, cleanup, interrupted)
}

#[cfg(feature = "audio")]
fn interactive_profile(
    segment_seconds: Option<u64>,
    timeout: Option<u64>,
    finalize_timeout: Option<u64>,
) -> Result<rambler_audio::InteractivePcmProfile, EngineError> {
    use rambler_audio::InteractivePcmProfile;
    let mut profile = InteractivePcmProfile::default();
    // Clap limits the override to 6..=280, compatible with the 5 s pause search.
    if let Some(seconds) = segment_seconds {
        profile.segmentation.segment_budget = Duration::from_secs(seconds);
    }
    let finalize = finalize_timeout.map_or(
        rambler_engine::RECOMMENDED_FINALIZE_TIMEOUT,
        Duration::from_secs,
    );
    if let Some(seconds) = timeout {
        // Preserve explicit deadlines without requiring a representable derived sum.
        profile.session.timeout = Duration::from_secs(seconds);
        profile.session.finalize_timeout = Some(finalize);
        return Ok(profile);
    }
    InteractivePcmProfile::new(profile.segmentation, finalize)
}

#[cfg(feature = "audio")]
async fn run_interactive(
    engine: &Engine,
    pcm: Box<dyn AudioSource>,
    session: SessionOptions,
    segmentation: rambler_audio::LongDictationOptions,
    policy: Option<ResultPolicy>,
) -> (Value, i32) {
    use rambler_audio::{LongDictationObservation, collect_long_pcm_dictation_with_observer};
    let mut ranges = Vec::new();
    let mut observe = |event: LongDictationObservation<'_>| {
        if let LongDictationObservation::SegmentCompleted { sample_range, .. } = event {
            ranges.push(sample_range);
        }
    };
    let collection = collect_long_pcm_dictation_with_observer(
        engine,
        pcm,
        session,
        CollectorLimits::default(),
        segmentation,
        &mut observe,
    );
    let (result, cleanup, interrupted) = supervise(collection, || engine.close(), async {
        tokio::signal::ctrl_c().await.is_ok()
    })
    .await;
    let (completed, failure) = match result {
        Some(Ok(completed)) => (completed, None),
        Some(Err(error)) => (error.completed, Some(error.cause)),
        None => (Default::default(), None),
    };
    let mut text = String::new();
    let mut ambiguous = false;
    let mut projection_failure = None;
    let segments: Vec<_> = completed
        .segments
        .iter()
        .zip(ranges)
        .enumerate()
        .map(|(index, (segment, range))| {
            let mut value = match output(segment, policy) {
                Ok((value, needs_review)) => {
                    ambiguous |= needs_review;
                    if let Some(part) = value["text"].as_str() {
                        if index != 0 {
                            text.push(' ');
                        }
                        text.push_str(part);
                    }
                    value
                }
                Err(error) => {
                    let value = failure_output(&error, None, None);
                    projection_failure.get_or_insert(error);
                    value
                }
            };
            value["sample_range"] = json!([range.start, range.end]);
            value
        })
        .collect();
    let (mut value, code) = match (interrupted, failure, cleanup, projection_failure) {
        (Some(signal_ok), _, cleanup, _) => {
            let (close_category, close_code) = cleanup
                .as_ref()
                .err()
                .map(error_category)
                .map_or((None, None), |(c, g)| (Some(c), g));
            (
                json!({"type":"error", "category":if signal_ok { "cancelled" } else { "signal" },
                "grpc_code":null, "close_error_category":close_category,
                "close_grpc_code":close_code, "timeline":null}),
                if signal_ok { 130 } else { 1 },
            )
        }
        (None, Some(error), cleanup, _) => {
            (failure_output(&error, cleanup.as_ref().err(), None), 1)
        }
        (None, None, Err(error), _) => {
            let (c, g) = error_category(&error);
            (
                json!({"type":"error", "category":"cleanup", "grpc_code":null,
                "close_error_category":c, "close_grpc_code":g, "timeline":null}),
                1,
            )
        }
        (None, None, Ok(()), Some(error)) => (failure_output(&error, None, None), 1),
        (None, None, Ok(()), None) if ambiguous => (json!({"type":"ambiguous", "text":null}), 1),
        (None, None, Ok(()), None) => (json!({"type":"result", "text":text}), 0),
    };
    value["completed_segments"] = json!(segments);
    (value, code)
}

#[tokio::main]
async fn main() {
    let cli = match Cli::try_parse() {
        Ok(v) => v,
        Err(e) => {
            if e.use_stderr() {
                print_error("arguments", None);
                std::process::exit(2);
            } else {
                print!("{e}");
                return;
            }
        }
    };
    let (common, input_kind) = match cli.command {
        Command::Dictate { common } => (common, InputKind::Ogg),
        #[cfg(feature = "audio")]
        Command::DictateWav {
            common,
            realtime,
            interactive,
            segment_seconds,
            page_duration_ms,
        } => (
            common,
            if interactive {
                InputKind::InteractiveWav { segment_seconds }
            } else {
                InputKind::Wav {
                    realtime,
                    page_duration_ms,
                }
            },
        ),
    };
    let CommonArgs {
        input,
        language,
        before,
        selected,
        after,
        cursor_aware,
        result_policy,
        timeout,
        finalize_timeout,
        context_timeout,
        bias_term,
        preference,
        secondary_language,
        block_offensive_words,
        auto_punctuation,
        secondary_routing,
        trigger_application_id,
        loopback,
    } = common;
    #[cfg(feature = "audio")]
    let profile = match input_kind {
        InputKind::InteractiveWav { segment_seconds } => {
            match interactive_profile(segment_seconds, timeout, finalize_timeout) {
                Ok(profile) => Some(profile),
                Err(_) => {
                    print_error("configuration", None);
                    std::process::exit(2);
                }
            }
        }
        _ => None,
    };
    let defaults = SessionOptions::default();
    #[cfg(feature = "audio")]
    let defaults = profile.as_ref().map_or(&defaults, |p| &p.session);
    let transport =
        match loopback.map_or_else(|| Ok(TransportConfig::default()), TransportConfig::loopback) {
            Ok(v) => v,
            Err(_) => {
                print_error("configuration", None);
                std::process::exit(2);
            }
        };
    let (mut audio, pcm) = match open_audio(input, input_kind).await {
        Ok(audio) => audio,
        Err(category) => {
            print_error(category, None);
            std::process::exit(1);
        }
    };
    let credentials = match admit_credentials(audio.as_mut(), loopback.is_some(), || {
        EnvironmentCredentials.required_snapshot()
    })
    .await
    {
        Ok(credentials) => credentials,
        Err(value) => {
            println!("{}", pcm.append_to(value));
            std::process::exit(1);
        }
    };
    let engine = match Engine::connect(transport).await {
        Ok(e) => e,
        Err(e) => {
            let value = pre_session_failure_output(audio.as_mut(), &e).await;
            println!("{}", pcm.append_to(value));
            std::process::exit(1);
        }
    };
    let context = if before.is_some() || selected.is_some() || after.is_some() || cursor_aware {
        ContextInput::Static(EditingContext {
            before: before.unwrap_or_default(),
            selected: selected.unwrap_or_default(),
            after: after.unwrap_or_default(),
            cursor_aware,
            ..Default::default()
        })
    } else {
        ContextInput::None
    };
    let options = SessionOptions {
        config: ProtocolConfig {
            language,
            bias_terms: bias_term,
            preferences: preference,
            secondary_languages: secondary_language,
            block_offensive_words,
            auto_punctuation,
            secondary_routing,
            trigger_application_id,
            ..Default::default()
        },
        context,
        timeout: timeout.map_or(defaults.timeout, Duration::from_secs),
        finalize_timeout: finalize_timeout
            .map(Duration::from_secs)
            .or(defaults.finalize_timeout),
        context_timeout: Duration::from_secs(context_timeout),
        credentials,
    };
    #[cfg(feature = "audio")]
    if let Some(profile) = profile {
        let (value, code) =
            run_interactive(&engine, audio, options, profile.segmentation, result_policy).await;
        println!("{value}");
        if code != 0 {
            std::process::exit(code);
        }
        return;
    }
    let session = engine.start(audio, options);
    let diagnostics = session.as_ref().ok().map(|session| session.diagnostics());
    let collection = async move { collect_session(session?, CollectorLimits::default()).await };
    let (result, cleanup, interrupted) = supervise(collection, || engine.close(), async {
        tokio::signal::ctrl_c().await.is_ok()
    })
    .await;
    let diagnostic = diagnostics.and_then(|handle| handle.final_snapshot());
    let diagnostic = diagnostic.as_deref();
    if let Some(signal_ok) = interrupted {
        let (close_category, close_code) = cleanup
            .as_ref()
            .err()
            .map(error_category)
            .map_or((None, None), |(c, g)| (Some(c), g));
        println!(
            "{}",
            pcm.append_to(with_timeline(json!({"type":"error", "category": if signal_ok { "cancelled" } else { "signal" }, "grpc_code":null, "close_error_category":close_category, "close_grpc_code":close_code}), diagnostic))
        );
        std::process::exit(if signal_ok { 130 } else { 1 });
    }
    let result = result.expect("collection or interrupt");
    match (result, cleanup) {
        (Err(e), cleanup) => {
            println!(
                "{}",
                pcm.append_to(failure_output(&e, cleanup.as_ref().err(), diagnostic))
            );
            std::process::exit(1);
        }
        (Ok(_), Err(e)) => {
            let (c, g) = error_category(&e);
            println!(
                "{}",
                pcm.append_to(with_timeline(json!({"type":"error", "category":"cleanup", "grpc_code":null, "close_error_category":c, "close_grpc_code":g}), diagnostic))
            );
            std::process::exit(1);
        }
        (Ok(r), Ok(())) => {
            let (value, ambiguous) = match output(&r, result_policy) {
                Ok(output) => output,
                Err(error) => {
                    println!(
                        "{}",
                        pcm.append_to(failure_output(&error, None, diagnostic))
                    );
                    std::process::exit(1);
                }
            };
            println!("{}", pcm.append_to(value));
            if ambiguous {
                std::process::exit(1);
            }
        }
    }
}
