mod ogg;
#[cfg(test)]
mod tests;

use clap::{Args, Parser, Subcommand};
use rambler_engine::{
    AudioSource, CollectorLimits, ContextInput, DictationResult, EditingContext,
    EncodedAudioProgress, Engine, EngineError, ProtocolConfig, SessionOptions, collect_dictation,
};
use rambler_transport::{EnvironmentCredentials, TransportConfig, TransportError};
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
        #[arg(long)]
        realtime: bool,
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
    #[arg(long, default_value_t = 30)]
    timeout: u64,
    #[arg(long, default_value_t = 2)]
    context_timeout: u64,
    #[arg(long)]
    bias_term: Vec<String>,
    #[arg(long)]
    preference: Vec<String>,
    #[arg(long)]
    secondary_language: Vec<String>,
    /// Test-only plaintext endpoint; only loopback addresses are accepted.
    #[arg(long)]
    loopback: Option<SocketAddr>,
}
enum InputKind {
    Ogg,
    #[cfg(feature = "audio")]
    Wav {
        realtime: bool,
        page_duration_ms: u32,
    },
}

async fn open_audio(path: PathBuf, kind: InputKind) -> Result<Box<dyn AudioSource>, &'static str> {
    match kind {
        InputKind::Ogg => Ok(Box::new(
            ogg::OggAudio::open(path).await.map_err(|_| "audio")?,
        )),
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
            Ok(Box::new(Pcm16Opus::new(wav, encoder)))
        }
    }
}

fn error_category(e: &EngineError) -> (&'static str, Option<String>) {
    match e {
        EngineError::Transport(TransportError::Grpc(code)) => ("grpc", Some(format!("{code:?}"))),
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
fn output(result: &DictationResult) -> Result<Value, EngineError> {
    result.check_safety()?;
    let finals: Vec<_> = result.final_updates.iter().map(|r| {
        let hypotheses: Vec<_> = r.hypotheses().iter().map(|h| {
            let edit = h.voice_edit.as_ref().map(|e| json!({"transcript": e.transcript, "alternative_text": e.alternative_text, "edited_text": e.edited_text, "edited_context": e.edited_context.as_ref().map(context), "intent": e.intent, "active_field": e.active_field.filter(|i| [2,3,7].contains(i))}));
            json!({"text":h.text, "confidence":h.confidence.filter(|v| v.is_finite()), "voice_edit":edit, "safety_status":h.safety_status})
        }).collect();
        json!({"hypotheses":hypotheses})
    }).collect();
    let metrics = result.metrics.as_ref().map(|m| json!({"audio":audio_progress(&m.audio), "requests_produced":m.requests_produced, "request_protobuf_bytes":m.request_protobuf_bytes, "responses_received":m.responses_received, "response_protobuf_bytes":m.response_protobuf_bytes, "elapsed_seconds":m.elapsed_seconds, "first_response_seconds":m.first_response_seconds, "eof_seconds":m.eof_seconds}));
    Ok(
        json!({"type":"result", "text":result.text().ok(), "final_updates":finals, "context":result.context.as_ref().map(context), "metrics":metrics}),
    )
}
fn print_error(category: &str, code: Option<String>) {
    println!(
        "{}",
        json!({"type":"error", "category":category, "grpc_code":code})
    );
}
fn failure_output(error: &EngineError, cleanup: Option<&EngineError>) -> Value {
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
    }
    value
}

async fn supervise<T, E, C, Close, Signal>(
    collection: C,
    close: impl FnOnce() -> Close,
    signal: Signal,
) -> (Option<Result<T, E>>, Result<(), E>, Option<bool>)
where
    C: std::future::Future<Output = Result<T, E>>,
    Close: std::future::Future<Output = Result<(), E>>,
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
            page_duration_ms,
        } => (
            common,
            InputKind::Wav {
                realtime,
                page_duration_ms,
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
        timeout,
        context_timeout,
        bias_term,
        preference,
        secondary_language,
        loopback,
    } = common;
    let transport =
        match loopback.map_or_else(|| Ok(TransportConfig::default()), TransportConfig::loopback) {
            Ok(v) => v,
            Err(_) => {
                print_error("configuration", None);
                std::process::exit(2);
            }
        };
    let mut audio = match open_audio(input, input_kind).await {
        Ok(audio) => audio,
        Err(category) => {
            print_error(category, None);
            std::process::exit(1);
        }
    };
    let engine = match Engine::connect(transport).await {
        Ok(e) => e,
        Err(e) => {
            let cleanup_failed = audio.close().await.is_err();
            let (c, g) = error_category(&e);
            if cleanup_failed {
                println!(
                    "{}",
                    json!({"type":"error", "category":c, "grpc_code":g, "close_error_category":"cleanup"})
                );
            } else {
                print_error(c, g);
            }
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
            ..Default::default()
        },
        context,
        timeout: Duration::from_secs(timeout),
        context_timeout: Duration::from_secs(context_timeout),
        credentials: if loopback.is_some() {
            None
        } else {
            Some(Arc::new(EnvironmentCredentials))
        },
    };
    let collection = collect_dictation(&engine, audio, options, CollectorLimits::default());
    let (result, cleanup, interrupted) = supervise(collection, || engine.close(), async {
        tokio::signal::ctrl_c().await.is_ok()
    })
    .await;
    if let Some(signal_ok) = interrupted {
        let (close_category, close_code) = cleanup
            .as_ref()
            .err()
            .map(error_category)
            .map_or((None, None), |(c, g)| (Some(c), g));
        println!(
            "{}",
            json!({"type":"error", "category": if signal_ok { "cancelled" } else { "signal" }, "grpc_code":null, "close_error_category":close_category, "close_grpc_code":close_code})
        );
        std::process::exit(if signal_ok { 130 } else { 1 });
    }
    let result = result.expect("collection or interrupt");
    match (result, cleanup) {
        (Err(e), cleanup) => {
            println!("{}", failure_output(&e, cleanup.as_ref().err()));
            std::process::exit(1);
        }
        (Ok(_), Err(e)) => {
            let (c, g) = error_category(&e);
            println!(
                "{}",
                json!({"type":"error", "category":"cleanup", "grpc_code":null, "close_error_category":c, "close_grpc_code":g})
            );
            std::process::exit(1);
        }
        (Ok(r), Ok(())) => {
            let ambiguous = r.text().is_err();
            let mut value = match output(&r) {
                Ok(value) => value,
                Err(error) => {
                    println!("{}", failure_output(&error, None));
                    std::process::exit(1);
                }
            };
            if ambiguous {
                value["type"] = json!("ambiguous");
            }
            println!("{value}");
            if ambiguous {
                std::process::exit(1);
            }
        }
    }
}
