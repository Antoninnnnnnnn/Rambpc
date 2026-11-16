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
//! The caller owns the shared engine and must explicitly close it after all
//! sessions, including when this function returns an error. Supply a continuous
//! Ogg/Opus source, not raw PCM labelled as Opus.
//!
//! ```no_run
//! use std::sync::Arc;
//! use rambler_engine::{AudioSource, CollectorLimits, ContextInput, DictationResult,
//!     EditingContext, Engine, EngineError, ProtocolConfig, SessionOptions,
//!     collect_dictation};
//! use rambler_transport::EnvironmentCredentials;
//!
//! async fn completed(engine: &Engine, audio: Box<dyn AudioSource>)
//!     -> Result<DictationResult, EngineError>
//! {
//!     collect_dictation(engine, audio, SessionOptions {
//!         config: ProtocolConfig { language: "fr-FR".into(), ..Default::default() },
//!         context: ContextInput::Static(EditingContext {
//!             before: "Texte existant.".into(), ..Default::default()
//!         }),
//!         credentials: Some(Arc::new(EnvironmentCredentials)),
//!         ..Default::default()
//!     }, CollectorLimits::default()).await
//! }
//! ```
mod engine;
mod presentation;
mod results;
pub use engine::*;
pub use presentation::*;
pub use rambler_protocol::{EditingContext, ProtocolConfig, S3Response};
pub use rambler_transport::{MetadataProvider, SourceError, SourceFuture};
pub use results::*;

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
