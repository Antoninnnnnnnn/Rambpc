//! Bounded developer validator: 2..=3 sessions on ONE Engine/transport.
//! --observe-partials also allows one session for a no-replay observation probe.
//! No retries. Outputs only hashes, counters and classifications, never text,
//! paths, metadata or raw response fields. Live mode requires --allow-live.
use bytes::Bytes;
use clap::Parser;
use rambler_audio::{EncoderConfig, Pcm16Opus, WavAudio, WavConfig};
use rambler_engine::{
    AudioSource, CollectorLimits, ContextInput, DictationResult, EditingContext, Engine,
    EngineError, EngineSession, PartialTranscript, Presentation, PresentationContent,
    ProtocolConfig, SessionOptions, SourceError, SourceFuture, collect_dictation,
};
use rambler_transport::{EnvironmentCredentials, TransportConfig, TransportError};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Parser)]
struct Args {
    manifest: PathBuf,
    #[arg(long, conflicts_with = "loopback")]
    allow_live: bool,
    #[arg(long)]
    loopback: Option<SocketAddr>,
    #[arg(long, default_value_t = 30)]
    timeout: u64,
    #[arg(long)]
    realtime: bool,
    /// Add per-session native field4 preview counters and ordered output hashes.
    #[arg(long)]
    observe_partials: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    input: PathBuf,
    language: String,
    pcm_sha256: String,
    markers: Vec<String>,
    #[serde(default)]
    marker_alternatives: Vec<Vec<String>>,
    #[serde(default)]
    expected_text_sha256: Option<String>,
    #[serde(default)]
    before: Option<String>,
    #[serde(default)]
    selected: Option<String>,
    #[serde(default)]
    after: Option<String>,
    #[serde(default)]
    cursor_aware: bool,
    #[serde(default)]
    bias_terms: Vec<String>,
    #[serde(default)]
    preferences: Vec<String>,
    #[serde(default)]
    secondary_languages: Vec<String>,
    #[serde(default)]
    block_offensive_words: Option<bool>,
    #[serde(default)]
    auto_punctuation: Option<bool>,
    #[serde(default)]
    secondary_routing: Option<String>,
}
#[derive(Default)]
struct Receipt {
    hash: Sha256,
    bytes: u64,
    eof: bool,
    closed: bool,
}
struct MeasuredPcm {
    wav: WavAudio,
    receipt: Arc<Mutex<Receipt>>,
}
impl AudioSource for MeasuredPcm {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async {
            let chunk = self.wav.next().await?;
            let mut receipt = self.receipt.lock().unwrap();
            match &chunk {
                Some(chunk) => {
                    receipt.hash.update(chunk);
                    receipt.bytes += chunk.len() as u64;
                }
                None => receipt.eof = true,
            }
            Ok(chunk)
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async {
            self.wav.close().await?;
            self.receipt.lock().unwrap().closed = true;
            Ok(())
        })
    }
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
/// Constant-space observations of sequential previews, never final candidates.
/// Each output hash covers ordered (u64 little-endian UTF-8 byte length, bytes)
/// records, including empty previews. Previews are NOT concatenated as text.
#[derive(Default)]
struct PartialObservations {
    updates: u64,
    updates_before_first_state1: u64,
    state1_coincident_updates: u64,
    saw_state1: bool,
    parts: u64,
    missing_text_parts: u64,
    empty_text_parts: u64,
    stable_nonempty_updates: u64,
    tentative_nonempty_updates: u64,
    nonfinite_score_updates: u64,
    stable: Sha256,
    tentative: Sha256,
    rendered: Sha256,
}
impl PartialObservations {
    fn observe(&mut self, response: &rambler_engine::S3Response) {
        // Latch on EVERY response, including state1 without any field4. A
        // coincident or later preview is not evidence of pre-final progression.
        let state1 = response.recognition_state() == Some(1);
        self.saw_state1 |= state1;
        let PresentationContent::Recognition { result, .. } = Presentation::new(response).content
        else {
            return;
        };
        let Some(partial) = &result.partial_result else {
            return;
        };
        let preview = PartialTranscript::from_result(partial);
        self.updates += 1;
        self.updates_before_first_state1 += u64::from(!self.saw_state1);
        self.state1_coincident_updates += u64::from(state1);
        self.parts += preview.parts.len() as u64;
        self.missing_text_parts += preview.parts.iter().filter(|p| p.text.is_none()).count() as u64;
        self.empty_text_parts += preview
            .parts
            .iter()
            .filter(|p| p.text.as_deref() == Some(""))
            .count() as u64;
        self.stable_nonempty_updates += u64::from(!preview.stable_prefix.is_empty());
        self.tentative_nonempty_updates += u64::from(!preview.tentative_suffix.is_empty());
        self.nonfinite_score_updates += u64::from(preview.has_nonfinite_scores);
        let rendered = preview.text();
        for (digest, text) in [
            (&mut self.stable, preview.stable_prefix.as_str()),
            (&mut self.tentative, preview.tentative_suffix.as_str()),
            (&mut self.rendered, rendered.as_str()),
        ] {
            digest.update((text.len() as u64).to_le_bytes());
            digest.update(text.as_bytes());
        }
    }
    fn report(&self) -> Value {
        json!({"updates":self.updates, "parts":self.parts,
            "updates_before_first_state1":self.updates_before_first_state1,
            "state1_coincident_updates":self.state1_coincident_updates,
            "missing_text_parts":self.missing_text_parts, "empty_text_parts":self.empty_text_parts,
            "stable_nonempty_updates":self.stable_nonempty_updates,
            "tentative_nonempty_updates":self.tentative_nonempty_updates,
            "nonfinite_score_updates":self.nonfinite_score_updates,
            "hash_encoding":"ordered_u64le_length_prefixed_utf8_v1",
            "stable_outputs_sha256":format!("{:x}",self.stable.clone().finalize()),
            "tentative_outputs_sha256":format!("{:x}",self.tentative.clone().finalize()),
            "rendered_outputs_sha256":format!("{:x}",self.rendered.clone().finalize())})
    }
}
fn with_partials(mut row: Value, observations: Option<&PartialObservations>) -> Value {
    if let Some(observations) = observations {
        row["partial_observations"] = observations.report();
    }
    row
}
/// Example-only opt-in collector. The engine's terminal `next` enforces S3 END,
/// full upload, cleanup and gRPC OK. Still require its unique EOF and await the
/// terminal next: neither a partial nor a final response establishes success.
/// On an early limit/error/drop, session Drop cancels; run always awaits Engine::close.
async fn collect_observed(
    session: Result<EngineSession, EngineError>,
    observations: &mut PartialObservations,
) -> Result<DictationResult, EngineError> {
    let mut session = session?;
    let limits = CollectorLimits::default();
    let mut result = DictationResult::default();
    let mut size = 0usize;
    while let Some(event) = session.next().await? {
        if let Some(response) = event.response {
            if result.metrics.is_some() {
                return Err(EngineError::Supervisor);
            }
            observations.observe(&response);
            // Same strict state1 retention as collect_dictation. No secondary
            // container selection or partial promotion, even when no final exists.
            if response.recognition_state() == Some(1) {
                size = size
                    .checked_add(response.raw.len())
                    .ok_or(EngineError::ResultLimit)?;
                if result.final_updates.len() >= limits.max_final_updates
                    || size > limits.max_result_bytes
                {
                    return Err(EngineError::ResultLimit);
                }
                result.final_updates.push(response);
            }
        } else if result.metrics.replace(event.metrics).is_some() {
            return Err(EngineError::Supervisor);
        }
    }
    if result.metrics.is_none() {
        return Err(EngineError::Supervisor);
    }
    result.context = session.captured_context();
    Ok(result)
}
fn normalized(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
fn marker_found(text: &str, marker: &str) -> bool {
    format!(" {} ", normalized(text)).contains(&format!(" {} ", normalized(marker)))
}
fn finals_hash(result: &DictationResult) -> String {
    // Closed projection, ordered hypotheses and finals, no opaque extensions/IDs.
    let finals: Vec<_> = result
        .final_updates
        .iter()
        .map(|r| {
            r.hypotheses()
                .iter()
                .map(|h| {
                    let edit = h.voice_edit.as_ref().map(|e| json!({
                "intent":e.intent, "field":e.active_field,
                "transcript":e.transcript.as_ref().map(|s| hash(s.as_bytes())),
                "alternative":e.alternative_text.as_ref().map(|s| hash(s.as_bytes())),
                "text":e.edited_text.as_ref().map(|s| hash(s.as_bytes())),
                "context":e.edited_context.as_ref().map(|c| json!({
                    "before":hash(c.before.as_bytes()), "selected":hash(c.selected.as_bytes()),
                    "after":hash(c.after.as_bytes()), "cursor_aware":c.cursor_aware,
                })),
            }));
                    json!({"text":h.text.as_ref().map(|s| hash(s.as_bytes())),
                "confidence":h.confidence.filter(|v| v.is_finite()),
                "safety":h.safety_status, "edit":edit})
                })
                .collect::<Vec<_>>()
        })
        .collect();
    hash(&serde_json::to_vec(&finals).unwrap())
}
fn failure(index: usize, category: &str) -> Value {
    json!({"case":index, "status":"failed_or_inconclusive", "category":category})
}
fn engine_failure(index: usize, error: &EngineError) -> Value {
    let mut value = failure(index, error_category(error));
    if let EngineError::Transport(TransportError::Grpc(code)) = error {
        value["grpc_code"] = json!(format!("{code:?}"));
    }
    if let EngineError::IncompleteSession(d) = error {
        value["incomplete"] = json!({"saw_s3_end":d.saw_s3_end,
            "source_exhausted":d.completion.source_exhausted, "upload_eof_polled":d.completion.upload_eof_polled,
            "source_closed":d.completion.source_closed, "terminal_code":d.completion.terminal_code.map(|c|format!("{c:?}")),
            "requests_produced":d.completion.requests_produced, "requests_polled":d.completion.requests_polled,
            "responses_received":d.completion.responses_received,
            "audio":{"bytes":d.metrics.audio.bytes,"chunks":d.metrics.audio.chunks,"eof":d.metrics.audio.eof,
                "cleanup_finished":d.metrics.audio.cleanup_finished,"cleanup_succeeded":d.metrics.audio.cleanup_succeeded},
            "elapsed_seconds":d.metrics.elapsed_seconds});
    }
    value
}
fn error_category(error: &EngineError) -> &'static str {
    match error {
        EngineError::IncompleteSession(_) => "incomplete",
        EngineError::SafetyBlocked => "safety_blocked",
        EngineError::UnrecognizedSafety => "unrecognized_safety",
        EngineError::AmbiguousResult => "ambiguous",
        EngineError::Transport(TransportError::Timeout) => "timeout",
        EngineError::Transport(TransportError::Grpc(_)) => "grpc",
        EngineError::Audio(_) | EngineError::AudioCleanup(_) => "audio",
        _ => "engine",
    }
}
async fn run(args: Args) -> (Value, bool) {
    // Gate before reading files, environment or connecting to any endpoint.
    if (!args.allow_live && args.loopback.is_none()) || !(1..=360).contains(&args.timeout) {
        return (
            json!({"status":"rejected", "category":"configuration"}),
            false,
        );
    }
    let config = match args.loopback {
        Some(address) => match TransportConfig::loopback(address) {
            Ok(config) => config,
            Err(_) => {
                return (
                    json!({"status":"rejected", "category":"configuration"}),
                    false,
                );
            }
        },
        None => TransportConfig::default(),
    };
    let bytes = match std::fs::File::open(&args.manifest).and_then(|file| {
        use std::io::Read;
        let mut bytes = Vec::new();
        file.take(65537).read_to_end(&mut bytes)?;
        Ok(bytes)
    }) {
        Ok(bytes) if bytes.len() <= 65536 => bytes,
        _ => return (json!({"status":"rejected", "category":"manifest"}), false),
    };
    let cases: Vec<Case> = match serde_json::from_slice(&bytes) {
        Ok(cases) => cases,
        Err(_) => return (json!({"status":"rejected", "category":"manifest"}), false),
    };
    let minimum_cases = if args.observe_partials { 1 } else { 2 };
    if !(minimum_cases..=3).contains(&cases.len())
        || cases.iter().any(|case| {
            case.pcm_sha256.len() != 64
                || !case.pcm_sha256.bytes().all(|b| b.is_ascii_hexdigit())
                || !(1..=8).contains(&case.markers.len())
                || case
                    .markers
                    .iter()
                    .any(|s| s.len() > 256 || normalized(s).is_empty())
                || case.marker_alternatives.len() > 8
                || case.marker_alternatives.iter().any(|group| {
                    group.is_empty()
                        || group.len() > 4
                        || group
                            .iter()
                            .any(|s| s.len() > 256 || normalized(s).is_empty())
                })
                || case.expected_text_sha256.as_ref().is_some_and(|hash| {
                    hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit())
                })
                || case.language.is_empty()
                || case.language.len() > 128
        })
    {
        return (json!({"status":"rejected", "category":"manifest"}), false);
    }
    let engine = match Engine::connect(config).await {
        Ok(engine) => engine,
        Err(_) => {
            return (
                json!({"status":"failed_or_inconclusive", "category":"connect"}),
                false,
            );
        }
    };
    let signal = tokio::signal::ctrl_c();
    tokio::pin!(signal);
    let mut rows = Vec::new();
    let mut healthy = true;
    for (index, case) in cases.into_iter().enumerate() {
        let path = if case.input.is_absolute() {
            case.input
        } else {
            args.manifest
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(case.input)
        };
        if !std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= 16 * 1024 * 1024) {
            rows.push(failure(index, "audio"));
            healthy = false;
            break;
        }
        let mut wav = match WavAudio::open(
            path,
            WavConfig {
                chunk_ms: 20,
                realtime: args.realtime,
            },
        )
        .await
        {
            Ok(wav) => wav,
            Err(_) => {
                rows.push(failure(index, "audio"));
                healthy = false;
                break;
            }
        };
        let frames = wav.frame_count;
        if frames == 0 || frames > 300 * 16000 {
            let _ = wav.close().await;
            rows.push(failure(index, "audio"));
            healthy = false;
            break;
        }
        let receipt = Arc::new(Mutex::new(Receipt::default()));
        let source = Pcm16Opus::new(
            MeasuredPcm {
                wav,
                receipt: receipt.clone(),
            },
            EncoderConfig::default(),
        );
        let context = if case.before.is_some()
            || case.selected.is_some()
            || case.after.is_some()
            || case.cursor_aware
        {
            ContextInput::Static(EditingContext {
                before: case.before.unwrap_or_default(),
                selected: case.selected.unwrap_or_default(),
                after: case.after.unwrap_or_default(),
                cursor_aware: case.cursor_aware,
                ..Default::default()
            })
        } else {
            ContextInput::None
        };
        let options = SessionOptions {
            config: ProtocolConfig {
                language: case.language,
                bias_terms: case.bias_terms,
                preferences: case.preferences,
                secondary_languages: case.secondary_languages,
                block_offensive_words: case.block_offensive_words,
                auto_punctuation: case.auto_punctuation,
                secondary_routing: case.secondary_routing,
                ..Default::default()
            },
            context,
            timeout: Duration::from_secs(args.timeout),
            credentials: if args.loopback.is_some() {
                None
            } else {
                Some(Arc::new(EnvironmentCredentials))
            },
            ..Default::default()
        };
        let mut observations = args.observe_partials.then(PartialObservations::default);
        let result = {
            // Transfer ownership synchronously, even if the signal wins before
            // the collector's first poll: dropping it must still cancel cleanup.
            let mut collection: std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<DictationResult, EngineError>>
                        + Send
                        + '_,
                >,
            > = if let Some(observations) = observations.as_mut() {
                Box::pin(collect_observed(
                    engine.start(Box::new(source), options),
                    observations,
                ))
            } else {
                Box::pin(collect_dictation(
                    &engine,
                    Box::new(source),
                    options,
                    CollectorLimits::default(),
                ))
            };
            tokio::select! {
                biased;
                _ = &mut signal => None,
                result = &mut collection => Some(result),
            }
        };
        let Some(result) = result else {
            rows.push(with_partials(
                failure(index, "cancelled"),
                observations.as_ref(),
            ));
            healthy = false;
            break;
        };
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                rows.push(with_partials(
                    engine_failure(index, &error),
                    observations.as_ref(),
                ));
                healthy = false;
                break;
            }
        };
        let text = match result.text() {
            Ok(text) => text,
            Err(error) => {
                rows.push(with_partials(
                    engine_failure(index, &error),
                    observations.as_ref(),
                ));
                healthy = false;
                break;
            }
        };
        let pcm = receipt.lock().unwrap();
        let pcm_hash = format!("{:x}", pcm.hash.clone().finalize());
        let consumed = pcm.eof
            && pcm.closed
            && pcm.bytes == frames * 2
            && pcm_hash.eq_ignore_ascii_case(&case.pcm_sha256);
        let markers: Vec<_> = case
            .markers
            .iter()
            .map(|marker| marker_found(&text, marker))
            .collect();
        let alternatives: Vec<_> = case
            .marker_alternatives
            .iter()
            .map(|group| group.iter().any(|marker| marker_found(&text, marker)))
            .collect();
        let text_matches_expected = case
            .expected_text_sha256
            .as_ref()
            .map(|expected| hash(text.as_bytes()).eq_ignore_ascii_case(expected));
        let passed = consumed
            && alternatives.iter().all(|found| *found)
            && markers.iter().all(|found| *found)
            && text_matches_expected != Some(false);
        let metrics = result.metrics.as_ref().unwrap();
        rows.push(with_partials(json!({"case":index, "status":if passed {"healthy"} else {"failed_or_inconclusive"},
            "pcm":{"bytes":pcm.bytes, "sha256":pcm_hash, "eof":pcm.eof, "closed":pcm.closed,
                "expected_frames":frames, "matches":consumed},
            "markers_found":markers, "marker_alternatives_found":alternatives, "text_sha256":hash(text.as_bytes()), "text_matches_expected":text_matches_expected,
            "ordered_finals_sha256":finals_hash(&result), "final_updates":result.final_updates.len(),
            "metrics":{"requests_produced":metrics.requests_produced,
                "request_protobuf_bytes":metrics.request_protobuf_bytes,
                "responses_received":metrics.responses_received,
                "response_protobuf_bytes":metrics.response_protobuf_bytes,
                "elapsed_seconds":metrics.elapsed_seconds,
                "first_response_seconds":metrics.first_response_seconds,
                "encoded_audio_bytes":metrics.audio.bytes, "encoded_audio_eof":metrics.audio.eof},
        }), observations.as_ref()));
        if !passed {
            healthy = false;
            break;
        }
    }
    if engine.close().await.is_err() {
        healthy = false;
        rows.push(json!({"category":"close"}));
    }
    (
        json!({"report_kind":"native_rust_session_reuse_v1", "status":if healthy {"healthy"} else {"failed_or_inconclusive"},
        "engine_instances":1, "connect_calls":1, "retry_count":0,
        "physical_connections_observed":null, "sessions":rows}),
        healthy,
    )
}
#[tokio::main]
async fn main() {
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(error) if !error.use_stderr() => {
            print!("{error}");
            return;
        }
        Err(_) => {
            println!("{}", json!({"status":"rejected", "category":"arguments"}));
            std::process::exit(2);
        }
    };
    let (report, success) = run(args).await;
    println!("{report}");
    if !success {
        std::process::exit(1);
    }
}

#[cfg(test)]
#[path = "support/session_reuse_tests.rs"]
mod tests;
