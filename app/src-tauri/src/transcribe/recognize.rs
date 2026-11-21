use super::{
    Job, NoteSink,
    decode::{self, AudioError},
};
use crate::{
    ipc::UnavailableReason,
    rambler::{self, RamblerAccess},
    text::{self, TextRules},
};
use bytes::Bytes;
use rambler_audio::{
    CompletedDictationSegment, InteractivePcmProfile, LongDictationOptions, PcmInput,
    join_segment_texts, pcm_channel, stream_long_pcm_dictation,
};
use rambler_engine::{
    AudioSource, CollectorLimits, ContextInput, DictationOutcome, Engine, EngineError,
    RenderPolicy, SourceError, SourceFuture,
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use unicode_segmentation::UnicodeSegmentation;

const GAP: &str = "[Passage non transcrit]";
const PARTIAL: &str =
    "La transcription est incomplète. Les passages manquants sont indiqués dans la note.";
const TOO_MUCH_TEXT: &str = "La transcription dépasse la taille maximale d’une note.";
const NOTE_FAILED: &str = "Impossible d’enregistrer la transcription dans le Carnet.";
const MAX_NOTE_CHARS: usize = 200_000;
/// Audio per RPC segment for files (same as live dictation).
const FILE_SEGMENT: std::time::Duration = std::time::Duration::from_secs(120);
/// A file is uploaded much faster than real time, but Rambler still works at
/// roughly real time: after the end of a segment's upload, finalization can
/// take about as long as the segment itself (measured: the live profile's 5 s
/// finalization budget fails on a 15 s file). Leave the segment length plus a margin.
const FILE_FINALIZE: std::time::Duration = std::time::Duration::from_secs(150);

#[derive(Default)]
pub(super) struct Outcome {
    pub duration_ms: Option<u64>,
    pub words: u64,
    pub note_id: Option<String>,
    pub error: Option<String>,
    pub audio_complete: bool,
}

impl Outcome {
    fn failed(message: impl Into<String>) -> Self {
        Self {
            error: Some(message.into()),
            ..Self::default()
        }
    }
}

// AudioSource requires 'static, so the collector owns a reference-counted job.
struct FileInput {
    pcm: PcmInput,
    job: Arc<Job>,
    samples: u64,
}

impl AudioSource for FileInput {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            let chunk = self.pcm.next().await?;
            if let Some(chunk) = &chunk {
                self.samples += (chunk.len() / 2) as u64;
                let read_ms = decode::samples_ms(self.samples);
                self.job.update(|view| {
                    if let Some(duration) = view.duration_ms.filter(|&ms| ms != 0) {
                        view.progress = (read_ms as f32 / duration as f32).clamp(0.0, 0.99);
                    }
                });
            }
            Ok(chunk)
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.pcm.close()
    }
}

#[derive(Default)]
struct Parts {
    segments: Vec<Option<String>>,
    bytes: usize,
    words: u64,
}

impl Parts {
    fn push(&mut self, segment: CompletedDictationSegment) -> bool {
        if let DictationOutcome::UsableText(rendered) =
            segment.result.interpret(RenderPolicy::AsrOnly)
        {
            if self.bytes + rendered.text.len() > rambler::MAX_DRAFT_BYTES {
                return false;
            }
            self.bytes += rendered.text.len();
            self.words += rendered.text.unicode_words().count() as u64;
            self.segments.push(Some(rendered.text));
        } else {
            // NoText, safety blocks, adjudication/deletion and unknown outcomes
            // are all gaps; no candidate from them crosses into the note.
            self.segments.push(None);
        }
        true
    }

    /// Render each contiguous admitted stretch once, never across a gap.
    fn note(&self, rules: &TextRules, failed: bool) -> (String, u64, bool) {
        let mut lines = Vec::new();
        let mut words = 0;
        let mut gap = failed;
        for run in self.segments.chunk_by(|a, b| a.is_some() == b.is_some()) {
            if run[0].is_some() {
                let joined = join_segment_texts(run.iter().filter_map(Option::as_deref));
                let rendered = text::render(&joined, rules);
                words += rendered.unicode_words().count() as u64;
                lines.push(rendered);
            } else {
                gap = true;
                lines.push(GAP.into());
            }
        }
        if failed && lines.last().is_some_and(|line| line != GAP) {
            lines.push(GAP.into());
        }
        (lines.join("\n\n"), words, gap)
    }
}

fn service_message(error: &EngineError) -> &'static str {
    match rambler::connection_failure(error) {
        UnavailableReason::AccessRefused => {
            "Rambler refuse l’accès configuré. Vérifiez les réglages."
        }
        UnavailableReason::Network => {
            "La connexion à Rambler a été interrompue ou n’a pas répondu."
        }
        UnavailableReason::Throttled => "Rambler reçoit trop de demandes. Réessayez plus tard.",
        _ => "Rambler n’a pas pu terminer la reconnaissance.",
    }
}

pub(super) async fn run(
    path: PathBuf,
    job: &Arc<Job>,
    access: RamblerAccess,
    rules: TextRules,
    sink: NoteSink,
) -> Outcome {
    let mut profile = InteractivePcmProfile::new(
        LongDictationOptions {
            segment_budget: FILE_SEGMENT,
            ..LongDictationOptions::default()
        },
        FILE_FINALIZE,
    )
    .expect("valid file transcription profile");
    profile.session.config.language = rules.language.code().into();
    profile.session.context = ContextInput::None;
    profile.session.credentials = Some(Arc::new(access.metadata().clone()));
    run_with_profile(path, job, access.transport_config(), profile, rules, sink).await
}

pub(super) async fn run_with_profile(
    path: PathBuf,
    job: &Arc<Job>,
    config: rambler_transport::TransportConfig,
    profile: InteractivePcmProfile,
    rules: TextRules,
    sink: NoteSink,
) -> Outcome {
    let (mut writer, pcm) = pcm_channel();
    let (ready, opened) = oneshot::channel();
    let stop = job.cancelled.child_token();
    let decoder_stop = stop.clone();
    let runtime = tokio::runtime::Handle::current();
    let worker = tokio::task::spawn_blocking(move || {
        let duration = decode::decode(
            &path,
            &decoder_stop,
            |duration| ready.send(duration).map_err(|_| AudioError::Stopped),
            |chunk| {
                runtime.block_on(async {
                tokio::select! {
                    biased;
                    _ = decoder_stop.cancelled() => Err(AudioError::Stopped),
                    permit = writer.reserve() => permit.map_err(|_| AudioError::Stopped)?.send(chunk).map_err(|_| AudioError::Stopped),
                }
            })
            },
        )?;
        writer.finish().map_err(|_| AudioError::Stopped)?;
        Ok::<_, AudioError>(duration)
    });
    let opened = tokio::select! {
        biased;
        _ = job.cancelled.cancelled() => None,
        result = opened => result.ok(),
    };
    let Some(duration) = opened else {
        drop(pcm);
        stop.cancel();
        return match worker.await {
            Ok(Err(error)) => Outcome::failed(error.to_string()),
            _ => Outcome::failed(AudioError::Read.to_string()),
        };
    };
    job.update(|view| view.duration_ms = duration);
    let engine = tokio::select! {
        biased;
        _ = job.cancelled.cancelled() => None,
        engine = Engine::connect_ready(config) => Some(engine),
    };
    let mut parts = Parts::default();
    let mut error = match engine {
        Some(Ok(engine)) => collect(&engine, pcm, job, profile, &mut parts).await,
        other => {
            drop(pcm);
            other
                .and_then(Result::err)
                .map(|error| service_message(&error))
        }
    };
    stop.cancel();
    let duration_ms = match worker.await {
        Ok(Ok(duration)) => Some(duration),
        Ok(Err(AudioError::Stopped)) => None,
        Ok(Err(audio)) => {
            // Failures in the blocking decoder are more precise than the SDK's
            // InputAbandoned, and do not leak its original I/O diagnostics.
            return save(job, sink, rules, parts, Outcome::failed(audio.to_string())).await;
        }
        Err(_) => {
            return save(
                job,
                sink,
                rules,
                parts,
                Outcome::failed(AudioError::Read.to_string()),
            )
            .await;
        }
    };
    if duration_ms.is_none() && !job.cancelled.is_cancelled() && error.is_none() {
        error = Some("La reconnaissance n’a pas traité la totalité du fichier.");
    }
    save(
        job,
        sink,
        rules,
        parts,
        Outcome {
            duration_ms,
            error: error.map(str::to_owned),
            audio_complete: duration_ms.is_some() && error.is_none(),
            ..Outcome::default()
        },
    )
    .await
}

async fn collect(
    engine: &Engine,
    pcm: PcmInput,
    job: &Arc<Job>,
    profile: InteractivePcmProfile,
    parts: &mut Parts,
) -> Option<&'static str> {
    let too_large = CancellationToken::new();
    let result = {
        let mut sink = |segment| {
            if !parts.push(segment) {
                too_large.cancel();
            }
            job.update(|view| view.words = parts.words);
        };
        let input = FileInput {
            pcm,
            job: job.clone(),
            samples: 0,
        };
        let collector = stream_long_pcm_dictation(
            engine,
            input,
            profile.session,
            CollectorLimits::default(),
            profile.segmentation,
            &mut sink,
        );
        tokio::select! {
            biased;
            _ = job.cancelled.cancelled() => None,
            _ = too_large.cancelled() => None,
            result = collector => Some(result),
        }
    }; // Drop the collector and request cleanup before closing the owned engine.
    let close = engine.close().await;
    if too_large.is_cancelled() {
        return Some(TOO_MUCH_TEXT);
    }
    result
        .and_then(Result::err)
        .or_else(|| close.err())
        .as_ref()
        .map(service_message)
}

async fn save(
    job: &Arc<Job>,
    sink: NoteSink,
    rules: TextRules,
    parts: Parts,
    mut outcome: Outcome,
) -> Outcome {
    if job.cancelled.is_cancelled() || parts.segments.is_empty() {
        return outcome;
    }
    let job = job.clone();
    // Neither local rendering nor a synchronous Carnet write belongs on the
    // runtime used by the live controller. Cancel can still win during render.
    tokio::task::spawn_blocking(move || {
        let (mut note, words, gap) = parts.note(&rules, outcome.error.is_some());
        outcome.words = words;
        if gap && outcome.error.is_none() {
            outcome.error = Some(PARTIAL.into());
        }
        if let Some((pos, _)) = note.char_indices().nth(MAX_NOTE_CHARS) {
            let marker = format!("\n\n{GAP}");
            let cut = note
                .char_indices()
                .nth(MAX_NOTE_CHARS - marker.chars().count())
                .map_or(pos, |(i, _)| i);
            note.truncate(cut);
            outcome.words = note.unicode_words().count() as u64;
            note.push_str(&marker);
            outcome.error = Some(TOO_MUCH_TEXT.into());
        }
        if job.begin_save() {
            let title = format!("Transcription · {}", job.view().file_name);
            match sink(&title, &note) {
                Ok(id) => outcome.note_id = Some(id),
                Err(_) => outcome.error = Some(NOTE_FAILED.into()), // Never forward sink internals/paths.
            }
        }
        outcome
    })
    .await
    .unwrap_or_else(|_| Outcome::failed(NOTE_FAILED))
}
