//! A read-only app: WAV -> SDK collection -> strict display and optional edit review.
//! See docs/research/sdk-integration-guide.md for the offline fixture and live usage.

#[cfg(test)]
#[path = "../../rambler-engine/tests/common/mod.rs"]
mod common;
#[cfg(test)]
#[path = "minimal_app/tests.rs"]
mod tests;
#[path = "minimal_app/wav.rs"]
mod wav;

use bytes::Bytes;
use clap::Parser;
use rambler_audio::{Pcm16Opus, Pcm48kTo16k, pcm_channel};
use rambler_engine::{
    AudioSource, CollectorLimits, ContextInput, DictationObservation, DictationResult,
    EditingContext, Engine, EngineError, MetadataProvider, PartialTranscript, PresentationContent,
    RecognitionCompletion, RenderPolicy, RetryAudio, RetryAudioLimits, Revision, SessionOptions,
    collect_dictation_with_observer, replacement_proposal_with_policy,
};
use rambler_transport::{EnvironmentCredentials, TransportConfig};
use std::{
    error::Error,
    io::{self, IsTerminal, Write},
    ops::Range,
    path::PathBuf,
    sync::Arc,
};
use wav::{Rate, Wav};

type AppResult<T> = Result<T, Box<dyn Error>>;

#[derive(Parser)]
#[command(about = "Collect a PCM WAV and show a read-only SDK result")]
struct Args {
    /// Authorized TLS Host[:port]; no service endpoint is built in.
    #[arg(long)]
    authority: String,
    /// Keep bounded encoded audio in memory and ask before one retry on failure.
    #[arg(long)]
    offer_retry: bool,
    /// RIFF/WAVE PCM16LE, mono, 16 or 48 kHz.
    wav: PathBuf,
}

fn demo_window() -> (EditingContext, Revision) {
    // A synthetic editor window; no file, clipboard or real editor is involved.
    (
        EditingContext {
            before: "🪁 The kite is ".into(),
            selected: "blue".into(),
            after: ". Marker stays.".into(),
            cursor_aware: true,
            ..Default::default()
        },
        Revision::from(1_i64),
    )
}

async fn collect_audio(
    engine: &Engine,
    audio: Box<dyn AudioSource>,
    options: SessionOptions,
    replace_preview: &mut (impl FnMut(Option<String>) + Send),
) -> Result<DictationResult, EngineError> {
    let mut observe = |observation: DictationObservation<'_>| {
        if let PresentationContent::Recognition {
            completion: RecognitionCompletion::Other,
            result,
            ..
        } = observation.presentation.content
            && let Some(partial) = &result.partial_result
        {
            // Not safety-checked: provisional UI state only. Replace the whole slot.
            replace_preview(Some(PartialTranscript::from_result(partial).text()));
        }
    };
    let completed = collect_dictation_with_observer(
        engine,
        audio,
        options,
        CollectorLimits::default(),
        &mut observe,
    )
    .await;
    replace_preview(None);
    completed
}

async fn collect_wav(
    engine: &Engine,
    credentials: Option<Arc<dyn MetadataProvider>>,
    wav: Wav<'_>,
    captured_window: &EditingContext,
    retry_approval: Option<fn() -> bool>,
    mut replace_preview: impl FnMut(Option<String>) + Send,
) -> AppResult<DictationResult> {
    let (mut writer, pcm) = pcm_channel();
    let audio: Box<dyn AudioSource> = match wav.rate {
        Rate::Khz16 => Box::new(Pcm16Opus::new(pcm, Default::default())),
        Rate::Khz48 => Box::new(Pcm16Opus::new(Pcm48kTo16k::new(pcm), Default::default())),
    };
    let (retained, audio) = if retry_approval.is_some() {
        let (retained, audio) = RetryAudio::capture(
            audio,
            RetryAudioLimits {
                max_bytes: 1024 * 1024,
                max_chunks: 4096,
            },
        );
        (Some(retained), audio)
    } else {
        (None, audio)
    };
    let options = || {
        SessionOptions {
            credentials: credentials.clone(),
            // This demo window is immutable; a real app must check or recapture it.
            context: ContextInput::Static(captured_window.clone()),
            ..Default::default() // Both inputs become 16 kHz mono Ogg/Opus on the wire.
        }
        .with_recommended_finalize_timeout()
    };
    let completed = collect_audio(engine, audio, options(), &mut replace_preview);
    let produced = async move {
        for chunk in wav.pcm.chunks(4096) {
            let permit = writer.reserve().await?;
            permit
                .send(Bytes::copy_from_slice(chunk))
                .map_err(|error| error.kind())?;
        }
        writer.finish() // Normal WAV EOF, not an acknowledgement from the service.
    };
    // The in-memory producer never waits outside the channel. A live capture
    // producer must also be cancelled when collection ends (see the guide).
    let (completed, produced) = tokio::join!(completed, produced);
    let completed = match (completed, retained, retry_approval) {
        (Err(error), Some(retained), Some(approve)) => match retained.into_source() {
            Ok(audio) => {
                eprintln!("First collection failed: {error}");
                if tokio::task::spawn_blocking(approve).await.unwrap_or(false) {
                    // Only this approval opens a new RPC. Never restart the old one.
                    collect_audio(engine, audio, options(), &mut replace_preview).await
                } else {
                    Err(error)
                }
            }
            Err(_) => Err(error), // Incomplete input or retention cap: no retry.
        },
        (result, _, _) => result,
    };
    let result = completed?; // Prefer the session failure to a consequent Closed writer.
    produced?;
    Ok(result)
}

struct Review {
    splice: Option<(Range<usize>, String)>,
    resulting_selection: Option<(usize, usize)>,
}

fn propose_for_review(
    completed: &DictationResult,
    captured_revision: Revision,
    current_window: &EditingContext,
    current_revision: &Revision,
) -> Result<Review, EngineError> {
    let proposal = replacement_proposal_with_policy(
        completed,
        Some(captured_revision),
        RenderPolicy::S3ClientCompatible,
    )?;
    proposal.validate_current(current_window, Some(current_revision))?;
    let splice = proposal
        .derive_scalar_splice(current_window, current_revision)?
        .map(|splice| (splice.original_utf16_range(), splice.inserted_text().into()));
    Ok(Review {
        splice,
        resulting_selection: proposal.selection_utf16_range(),
    })
}

fn confirm_retry() -> bool {
    let stdin = io::stdin();
    let mut stderr = io::stderr();
    if !stdin.is_terminal() || !stderr.is_terminal() {
        return false;
    }
    let mut answer = String::new();
    write!(stderr, "Retry the same audio in a new session? [y/N] ").is_ok()
        && stderr.flush().is_ok()
        && stdin.read_line(&mut answer).is_ok()
        && answer.trim().eq_ignore_ascii_case("y")
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> AppResult<()> {
    let args = Args::parse();
    let wav_bytes = wav::read(&args.wav).await?;
    let wav = Wav::parse(&wav_bytes)?;
    let transport = TransportConfig::tls(&args.authority)?;
    let credentials = Arc::new(EnvironmentCredentials.required_snapshot()?);
    // Warm up once, outside the collection flow. A UI can do this at startup
    // and keep this engine alive across many utterances.
    let engine = Engine::connect_ready(transport).await?;
    let (window, revision) = demo_window(); // Capture window and revision together.
    let mut preview = None;
    let completed = collect_wav(
        &engine,
        Some(credentials),
        wav,
        &window,
        args.offer_retry.then_some(confirm_retry),
        |next| preview = next, // Publish to one replace-latest UI slot, not a log.
    )
    .await;
    let closed = engine.close().await;
    if completed.is_err()
        && let Err(error) = &closed
    {
        eprintln!("Engine shutdown also failed: {error}");
    }
    let completed = completed?;
    closed?;
    debug_assert!(preview.is_none());

    // Strict display and compatible edit review are independent decisions. A
    // compatible rewrite may have no strict text; do not fall back to ASR.
    match completed.text() {
        Ok(text) => println!("Final (strict): {text:?}"),
        Err(error) => eprintln!("No strict final text: {error}"),
    }
    // The synthetic editor is immutable. A real host would verify document
    // identity and window anchor and re-read the live window/revision here.
    match propose_for_review(&completed, revision.clone(), &window, &revision) {
        Ok(review) => {
            if let Some((range, text)) = review.splice {
                println!("Proposed window-local UTF-16 splice {range:?}: {text:?} (not applied)");
            } else {
                println!("Proposal has no text change (not applied)");
            }
            if let Some(selection) = review.resulting_selection {
                println!("Proposed selection in resulting window: {selection:?} (not applied)");
            }
        }
        Err(EngineError::UnsupportedEdit) => println!("No selection rewrite proposal"),
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
