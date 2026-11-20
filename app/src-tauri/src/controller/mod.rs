//! The single owner of take transitions, from the trigger to the one
//! insertion attempt. Owned by the "core" work package.
//!
//! Public surface used by the shell (`crate::shell`) is `Controller::spawn`
//! and `ControllerHandle`. Keep these signatures stable; everything else in
//! this module tree is internal.
//!
//! PLACEHOLDER: this skeleton only fixes the API. The core work package
//! replaces the bodies (pure state machine + single Tokio task + effects).

use crate::ipc::{DraftView, UiState};
use crate::model::{KeyEdge, SystemEvent, TakeId};
use crate::ports::{Clipboard, ClipboardError, Inserter, Microphone, TargetProbe};
use crate::rambler::RamblerAccess;
use crate::settings::Settings;
use std::sync::Arc;
use tokio::sync::watch;

/// Platform implementations handed to the controller.
#[derive(Clone)]
pub struct ControllerDeps {
    pub microphone: Arc<dyn Microphone>,
    pub target: Arc<dyn TargetProbe>,
    pub inserter: Arc<dyn Inserter>,
    pub clipboard: Arc<dyn Clipboard>,
}

/// Initial state read from storage before the controller starts.
pub struct ControllerInit {
    pub settings: Settings,
    pub consent: bool,
    pub access: Option<RamblerAccess>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CopyError {
    #[error("no draft")]
    NoDraft,
    #[error(transparent)]
    Clipboard(#[from] ClipboardError),
}

pub struct Controller;

impl Controller {
    /// Spawn the controller task on the current Tokio runtime.
    pub fn spawn(deps: ControllerDeps, init: ControllerInit) -> ControllerHandle {
        let _ = deps;
        let state = UiState {
            phase: Default::default(),
            take: None,
            destination: None,
            availability: Default::default(),
            last_outcome: None,
            draft: None,
            listening_ms: None,
            gesture: init.settings.gesture,
            shortcut: init.settings.shortcut.clone(),
        };
        let (state_tx, state_rx) = watch::channel(state);
        let (level_tx, level_rx) = watch::channel(0.0);
        ControllerHandle {
            _state_tx: Arc::new(state_tx),
            state_rx,
            _level_tx: Arc::new(level_tx),
            level_rx,
        }
    }
}

/// Cheap to clone. Every method is non-blocking: intents are queued to the
/// controller task (bounded; Finish/Cancel/Shutdown are never dropped).
#[derive(Clone)]
pub struct ControllerHandle {
    _state_tx: Arc<watch::Sender<UiState>>,
    state_rx: watch::Receiver<UiState>,
    _level_tx: Arc<watch::Sender<f32>>,
    level_rx: watch::Receiver<f32>,
}

impl ControllerHandle {
    /// Edge of the configured global shortcut (hold or toggle per settings).
    pub fn hotkey(&self, edge: KeyEdge) {
        let _ = edge;
    }
    /// "Dicter dans Sambler": start a take whose only destination is the draft.
    pub fn dictate_in_sambler(&self) {}
    /// Finish the identified take (stale ids are ignored).
    pub fn finish(&self, take: TakeId) {
        let _ = take;
    }
    /// Cancel the identified take, or the current one when `None`.
    pub fn cancel(&self, take: Option<TakeId>) {
        let _ = take;
    }
    pub fn system(&self, event: SystemEvent) {
        let _ = event;
    }
    /// New settings; apply to the next take.
    pub fn apply_settings(&self, settings: Settings) {
        let _ = settings;
    }
    /// Enabling may warm the engine; withdrawing stops any take immediately.
    pub fn set_consent(&self, enabled: bool) {
        let _ = enabled;
    }
    /// Replace (or remove) the Rambler access; closes and re-warms the engine.
    pub fn set_access(&self, access: Option<RamblerAccess>) {
        let _ = access;
    }
    /// Explicit retry after "Dictée indisponible".
    pub fn retry_connection(&self) {}
    /// Latest UI state (text-free).
    pub fn state(&self) -> watch::Receiver<UiState> {
        self.state_rx.clone()
    }
    /// Latest microphone level in 0..=1 while listening.
    pub fn level(&self) -> watch::Receiver<f32> {
        self.level_rx.clone()
    }
    /// Draft text, for the view that displays it.
    pub async fn draft(&self) -> Option<DraftView> {
        None
    }
    /// Explicit "Copier" of the draft's available text.
    pub async fn copy_draft(&self) -> Result<(), CopyError> {
        Err(CopyError::NoDraft)
    }
    pub fn clear_draft(&self) {}
    /// Stop any take, release the microphone, close the engine. Idempotent.
    pub async fn shutdown(&self) {}
}
