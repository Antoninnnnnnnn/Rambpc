//! Native S3 session assembly and conservative, bounded completed results.
//!
//! No audio replay, Python process, editor mutation, microphone, or encoder.
//! Sources/providers must cooperate with cancellation. `next` may be dropped;
//! asynchronous audio cleanup is moved into a stable task and awaited exactly once.
//! Call `Engine::close` / `EngineSession::close` explicitly and keep the Tokio
//! runtime alive: Drop only signals cancellation, it cannot await cleanup.
//! Payload metrics count produced protobufs, never network acknowledgements.
//!
//! # Collect caller-provided audio
//!
//! Supply one continuous Ogg/Opus source, not raw PCM labelled as Opus.
//! [`OggAudio::open`] streams a supported Ogg/Opus file without a codec. For
//! caller-owned PCM, `rambler-audio` provides a bounded `pcm_channel` and
//! `Pcm16Opus` adapter. Drive the PCM writer and collection concurrently and
//! explicitly finish the writer at normal input EOF. The application supplies
//! the already-configured engine and per-session options.
//!
//! Use [`collect_dictation`] if you only need the final result, or
//! [`collect_dictation_with_observer`] for response-driven progress and optional
//! preview. Each partial replaces the whole preview; never append across responses.
//! If your application requires safety-checked text only, omit the preview.
//!
//! ```no_run
//! use rambler_engine::{
//!     AudioSource, CollectorLimits, DictationObservation, DictationResult, Engine,
//!     EngineError, PartialTranscript, PresentationContent, RecognitionCompletion,
//!     SessionOptions, collect_dictation_with_observer,
//! };
//!
//! async fn completed(
//!     engine: &Engine,
//!     audio: Box<dyn AudioSource>,
//!     options: SessionOptions,
//!     mut replace_preview: impl FnMut(Option<(&str, &str)>) + Send,
//!     mut progress: impl FnMut(u64, u64) + Send,
//! ) -> Result<DictationResult, EngineError> {
//!     let mut observe = |observation: DictationObservation<'_>| {
//!         let metrics = observation.metrics;
//!         progress(metrics.audio.bytes, metrics.responses_received);
//!         if let PresentationContent::Recognition {
//!             completion: RecognitionCompletion::Other, result, ..
//!         } = observation.presentation.content
//!             && let Some(partial) = &result.partial_result
//!         {
//!             let preview = PartialTranscript::from_result(partial);
//!             replace_preview(Some((&preview.stable_prefix, &preview.tentative_suffix)));
//!         }
//!     };
//!     let result = collect_dictation_with_observer(
//!         engine, audio, options, CollectorLimits::default(), &mut observe,
//!     ).await;
//!     replace_preview(None);
//!     result // Only Ok here commits the collection. The text policy is separate.
//! }
//! ```
//!
//! The preview and progress callbacks above must return quickly and must not log
//! text or act on it. On external cancellation the function may never resume;
//! the outer owner should clear preview state and keep the runtime alive for
//! cleanup. At application shutdown, explicitly await [`Engine::close`] even
//! after a collection error. If constructed with [`Engine::new`], the shared
//! transport remains the caller's responsibility.
//!
//! # Validate an edit proposal against the editor
//!
//! After a successful collection, use [`DictationResult::text`] for strict text
//! display, or [`replacement_proposal`] for an explicit, safety-checked edit
//! proposal. A proposal covers only the captured window. Capture its revision
//! together with the window, not separately at session start. This provider is
//! per session and is called only after normal audio EOF and successful cleanup.
//!
//! ```no_run
//! use std::sync::{Arc, Mutex};
//! use rambler_engine::{
//!     ContextInput, ContextProvider, DictationResult, EditingContext, EngineError,
//!     ReplacementProposal, Revision, SessionOptions, SourceError, SourceFuture,
//!     replacement_proposal,
//! };
//!
//! struct LateWindow {
//!     // Updated by the application as one window+revision pair.
//!     editor: Arc<Mutex<(EditingContext, Revision)>>,
//!     captured_revision: Mutex<Option<Revision>>,
//! }
//!
//! impl ContextProvider for LateWindow {
//!     fn context(&self) -> SourceFuture<'_, Result<Option<EditingContext>, SourceError>> {
//!         Box::pin(async {
//!             let (context, revision) = self.editor.lock().unwrap().clone();
//!             *self.captured_revision.lock().unwrap() = Some(revision);
//!             Ok(Some(context))
//!         })
//!     }
//! }
//!
//! fn with_late_window(options: &mut SessionOptions, provider: Arc<LateWindow>) {
//!     options.context = ContextInput::Provider(provider);
//! }
//!
//! fn propose_for_review(
//!     completed: &DictationResult, provider: &LateWindow,
//! ) -> Result<ReplacementProposal, EngineError> {
//!     let revision = provider.captured_revision.lock().unwrap().clone()
//!         .ok_or(EngineError::UnsupportedEdit)?;
//!     let proposal = replacement_proposal(completed, Some(revision))?;
//!     let current = provider.editor.lock().unwrap();
//!     proposal.validate_current(&current.0, Some(&current.1))?;
//!     Ok(proposal) // No mutation. Revalidate in the same transaction as apply.
//! }
//! ```
//!
//! The caller must track the identity and range of the original window, and
//! revalidate it with the current revision atomically with any explicit apply.
//! This SDK does not perform edits, SEND, or CLEAR.
mod engine;
mod ogg;
mod presentation;
mod results;
mod splice;
pub use engine::*;
pub use ogg::OggAudio;
pub use presentation::*;
pub use rambler_protocol::{EditingContext, ProtocolConfig, S3Response};
pub use rambler_transport::{MetadataProvider, SourceError, SourceFuture};
pub use results::*;
pub use splice::ScalarSplice;

use std::{fmt, sync::Arc};

/// Sensitive response/provider details are available explicitly, not through
/// Display, Debug, or an automatically traversed error chain.
#[derive(Clone, thiserror::Error)]
pub enum EngineError {
    #[error("invalid engine configuration")]
    InvalidConfig,
    #[error("engine is closed")]
    Closed,
    #[error("session cancelled")]
    Cancelled,
    /// The optional post-audio budget expired after EOF and successful cleanup,
    /// before late context, upload END, and final gRPC status all completed.
    #[error("finalization deadline exceeded")]
    FinalizeTimeout,
    #[error("context deadline exceeded")]
    ContextTimeout,
    #[error("context provider failed")]
    Context(SourceError),
    #[error("audio source failed")]
    Audio(SourceError),
    #[error("audio source cleanup failed")]
    AudioCleanup(SourceError),
    #[error("invalid S3 protobuf")]
    Protocol(rambler_protocol::Error),
    #[error("transport failed")]
    Transport(rambler_transport::TransportError),
    #[error("S3 reported an application error")]
    S3(Arc<S3Response>),
    #[error("session did not complete S3 END and a fully exhausted upload with gRPC OK")]
    IncompleteSession(Arc<IncompleteSessionDiagnostic>),
    #[error("session supervisor failed")]
    Supervisor,
    #[error("result blocked by service safety status")]
    SafetyBlocked,
    #[error("result has an unrecognized safety status")]
    UnrecognizedSafety,
    #[error("expected one final update, one hypothesis, and a verified text policy")]
    AmbiguousResult,
    #[error("final result retention limit exceeded")]
    ResultLimit,
    #[error("no verified history replacement")]
    UnsupportedEdit,
    #[error("editing context or revision changed")]
    ContextConflict,
    #[error("invalid canonical integer revision")]
    InvalidRevision,
}
impl fmt::Debug for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl From<rambler_protocol::Error> for EngineError {
    fn from(error: rambler_protocol::Error) -> Self {
        Self::Protocol(error)
    }
}
impl From<rambler_transport::TransportError> for EngineError {
    fn from(error: rambler_transport::TransportError) -> Self {
        Self::Transport(error)
    }
}
