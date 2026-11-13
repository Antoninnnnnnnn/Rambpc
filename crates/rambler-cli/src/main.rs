use bytes::Bytes;
use clap::{Parser, Subcommand};
use rambler_engine::{
    AudioSource, CollectorLimits, ContextInput, DictationResult, EditingContext, Engine,
    EngineError, ProtocolConfig, SessionOptions, SourceError, SourceFuture, collect_dictation,
};
use rambler_transport::{EnvironmentCredentials, TransportConfig, TransportError};
use serde_json::{Value, json};
use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

#[derive(Parser)]
#[command(
    name = "rambler",
    about = "Dictate a continuous Ogg/Opus file. Only the Ogg magic is checked."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Dictate {
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
    },
}
struct FileAudio(Option<tokio::fs::File>);
impl AudioSource for FileAudio {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            let mut buf = vec![0; 8192];
            let n = self
                .0
                .as_mut()
                .expect("source open")
                .read(&mut buf)
                .await
                .map_err(SourceError::new)?;
            buf.truncate(n);
            Ok((n != 0).then(|| Bytes::from(buf)))
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            if let Some(file) = self.0.take() {
                drop(file.into_std().await);
            }
            Ok(())
        })
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
        EngineError::AmbiguousResult => ("ambiguous", None),
        EngineError::ResultLimit => ("result_limit", None),
        EngineError::IncompleteSession => ("incomplete", None),
        EngineError::InvalidConfig => ("configuration", None),
        _ => ("engine", None),
    }
}
fn context(c: &EditingContext) -> Value {
    json!({"before": c.before, "selected": c.selected, "after": c.after, "cursor_aware": c.cursor_aware})
}
fn output(result: &DictationResult) -> Value {
    let finals: Vec<_> = result.final_updates.iter().map(|r| {
        let hypotheses: Vec<_> = r.hypotheses().iter().map(|h| {
            let edit = h.voice_edit.as_ref().map(|e| json!({"transcript": e.transcript, "alternative_text": e.alternative_text, "edited_text": e.edited_text, "edited_context": e.edited_context.as_ref().map(context), "intent": e.intent, "active_field": e.active_field.filter(|i| [2,3,7].contains(i))}));
            json!({"text":h.text, "confidence":h.confidence.filter(|v| v.is_finite()), "voice_edit":edit})
        }).collect();
        json!({"hypotheses":hypotheses})
    }).collect();
    let metrics = result.metrics.as_ref().map(|m| json!({"requests_produced":m.requests_produced, "request_protobuf_bytes":m.request_protobuf_bytes, "responses_received":m.responses_received, "response_protobuf_bytes":m.response_protobuf_bytes, "elapsed_seconds":m.elapsed_seconds, "first_response_seconds":m.first_response_seconds, "eof_seconds":m.eof_seconds}));
    json!({"type":"result", "text":result.text().ok(), "final_updates":finals, "context":result.context.as_ref().map(context), "metrics":metrics})
}
fn print_error(category: &str, code: Option<String>) {
    println!(
        "{}",
        json!({"type":"error", "category":category, "grpc_code":code})
    );
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
    let Command::Dictate {
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
    } = cli.command;
    let transport =
        match loopback.map_or_else(|| Ok(TransportConfig::default()), TransportConfig::loopback) {
            Ok(v) => v,
            Err(_) => {
                print_error("configuration", None);
                std::process::exit(2);
            }
        };
    let mut file = match tokio::fs::File::open(input).await {
        Ok(f) => f,
        Err(_) => {
            print_error("audio", None);
            std::process::exit(1);
        }
    };
    let mut magic = [0; 4];
    if file.read_exact(&mut magic).await.is_err()
        || &magic != b"OggS"
        || file.seek(std::io::SeekFrom::Start(0)).await.is_err()
    {
        drop(file.into_std().await);
        print_error("audio", None);
        std::process::exit(1);
    }
    let engine = match Engine::connect(transport).await {
        Ok(e) => e,
        Err(e) => {
            drop(file.into_std().await);
            let (c, g) = error_category(&e);
            print_error(c, g);
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
    let result = collect_dictation(
        &engine,
        Box::new(FileAudio(Some(file))),
        options,
        CollectorLimits::default(),
    )
    .await;
    let cleanup = engine.close().await;
    if cleanup.is_err() {
        print_error("cleanup", None);
        std::process::exit(1);
    }
    match result {
        Err(e) => {
            let (c, g) = error_category(&e);
            print_error(c, g);
            std::process::exit(1);
        }
        Ok(r) => {
            let ambiguous = r.text().is_err();
            let mut value = output(&r);
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
