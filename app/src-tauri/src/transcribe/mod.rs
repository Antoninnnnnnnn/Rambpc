//! Native file transcription. Independent of live takes, editor insertion,
//! clipboard and history. The only text destination is the injected Carnet sink.

mod decode;
#[cfg(windows)]
mod native;
mod opus_file;
mod recognize;
mod resample;
mod wav;

use crate::ipc::{CommandError, FileTranscription, FileTranscriptionStatus as Status};
use crate::{rambler::RamblerAccess, text::TextRules};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

pub(super) type NoteSink = Arc<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>;
pub(super) type Emit = Arc<dyn Fn(FileTranscription) + Send + Sync>;

pub(super) struct Transcriptions {
    registry: Mutex<Registry>,
    note_sink: NoteSink,
}

#[derive(Default)]
struct Registry {
    allowed: HashSet<String>,
    jobs: Vec<Arc<Job>>,
    active: Option<Arc<Job>>,
    closed: bool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

impl Transcriptions {
    pub fn new(note_sink: NoteSink) -> Self {
        Self {
            registry: Mutex::default(),
            note_sink,
        }
    }

    pub fn allow(&self, path: &Path) -> Option<String> {
        if !path.is_absolute() || !decode::supported(path) {
            return None;
        }
        // Do not use a lossy spelling: it could name a different file.
        let path = path.to_str()?.to_owned();
        lock(&self.registry).allowed.insert(path.clone());
        Some(path)
    }

    pub fn start(
        self: &Arc<Self>,
        path: String,
        access: RamblerAccess,
        rules: TextRules,
        emit: Emit,
    ) -> Result<FileTranscription, CommandError> {
        let (path, job) = {
            let mut registry = lock(&self.registry);
            if !registry.allowed.contains(&path) {
                return Err(CommandError::new(
                    "file-not-authorized",
                    "Choisissez ou déposez ce fichier dans Sambler avant de le transcrire.",
                ));
            }
            if registry.closed || registry.active.is_some() {
                return Err(CommandError::new(
                    "transcription-busy",
                    "Une transcription est déjà en cours ou Sambler est en train de se fermer.",
                ));
            }
            let path = PathBuf::from(path);
            let file_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| {
                    CommandError::new("file-not-authorized", "Le nom du fichier est illisible.")
                })?
                .to_owned();
            let job = Arc::new(Job::new(file_name, emit.clone()));
            registry.jobs.push(job.clone());
            registry.active = Some(job.clone());
            (path, job)
        };
        let initial = job.view();
        emit(initial.clone());
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            let outcome =
                recognize::run(path, &job, access, rules, manager.note_sink.clone()).await;
            let final_view = job.finish(outcome);
            lock(&manager.registry).active = None;
            emit(final_view);
            job.done.send_replace(true);
        });
        Ok(initial)
    }

    pub fn list(&self) -> Vec<FileTranscription> {
        lock(&self.registry)
            .jobs
            .iter()
            .rev()
            .map(|job| job.view())
            .collect()
    }

    pub async fn cancel(&self, id: &str) -> Result<FileTranscription, CommandError> {
        let job = lock(&self.registry)
            .jobs
            .iter()
            .find(|job| lock(&job.state).view.id == id)
            .cloned()
            .ok_or_else(|| {
                CommandError::new(
                    "transcription-not-found",
                    "Cette transcription n’existe pas dans cette session.",
                )
            })?;
        job.stop();
        job.wait().await;
        Ok(job.view())
    }

    pub fn cancel_active(&self) {
        if let Some(job) = &lock(&self.registry).active {
            job.stop();
        }
    }

    pub async fn shutdown(&self) {
        let job = {
            let mut registry = lock(&self.registry);
            registry.closed = true;
            registry.active.clone()
        };
        if let Some(job) = job {
            job.stop();
            job.wait().await;
        }
    }
}

struct JobState {
    view: FileTranscription,
    last_emit: Instant,
    saving: bool,
    // Retained only while running: a Tauri emitter can own an AppHandle.
    emit: Option<Emit>,
}

struct Job {
    state: Mutex<JobState>,
    cancelled: CancellationToken,
    done: watch::Sender<bool>,
}

impl Job {
    fn new(file_name: String, emit: Emit) -> Self {
        let view = FileTranscription {
            id: uuid::Uuid::new_v4().to_string(),
            file_name,
            duration_ms: None,
            status: Status::Running,
            progress: 0.0,
            words: 0,
            note_id: None,
            error: None,
            started_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };
        Self {
            state: Mutex::new(JobState {
                view,
                last_emit: Instant::now(),
                saving: false,
                emit: Some(emit),
            }),
            cancelled: CancellationToken::new(),
            done: watch::channel(false).0,
        }
    }

    fn view(&self) -> FileTranscription {
        lock(&self.state).view.clone()
    }

    fn update(&self, update: impl FnOnce(&mut FileTranscription)) {
        let (emit, view) = {
            let mut state = lock(&self.state);
            if self.cancelled.is_cancelled() || state.view.status != Status::Running {
                return;
            }
            update(&mut state.view);
            if state.last_emit.elapsed() < Duration::from_millis(250) {
                return;
            }
            state.last_emit = Instant::now();
            (state.emit.clone(), state.view.clone())
        };
        if let Some(emit) = emit {
            emit(view);
        }
    }

    fn stop(&self) {
        // After starting the note commit, let the save resolve rather than
        // reporting cancellation while still writing a note behind the UI.
        let state = lock(&self.state);
        if state.view.status == Status::Running && !state.saving {
            self.cancelled.cancel();
        }
    }

    fn begin_save(&self) -> bool {
        let mut state = lock(&self.state);
        if self.cancelled.is_cancelled() {
            return false;
        }
        state.saving = true;
        true
    }

    async fn wait(&self) {
        let _ = self.done.subscribe().wait_for(|&done| done).await;
    }

    fn finish(&self, outcome: recognize::Outcome) -> FileTranscription {
        let mut state = lock(&self.state);
        state.emit = None;
        let view = &mut state.view;
        if self.cancelled.is_cancelled() {
            view.status = Status::Cancelled;
            view.words = 0;
        } else {
            view.duration_ms = outcome.duration_ms.or(view.duration_ms);
            view.status = if outcome.error.is_some() {
                Status::Failed
            } else {
                Status::Done
            };
            if outcome.audio_complete {
                view.progress = 1.0;
            }
            view.words = outcome.words;
            view.note_id = outcome.note_id;
            view.error = outcome.error;
        }
        view.clone()
    }
}

#[cfg(windows)]
pub(super) fn pick(owner: isize) -> Result<Option<PathBuf>, CommandError> {
    native::pick(owner).map_err(|_| {
        CommandError::new(
            "audio-picker",
            "Impossible d’ouvrir la boîte de dialogue de sélection audio.",
        )
    })
}

pub(super) fn supported(path: &Path) -> bool {
    decode::supported(path)
}

#[cfg(test)]
mod tests;
