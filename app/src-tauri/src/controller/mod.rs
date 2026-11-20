//! The single owner of take transitions, from the trigger to the one
//! insertion attempt. The pure reducer never touches ports or reads a clock.

pub mod level;
mod runtime;
mod state;

use crate::ipc::{DraftView, PersonalRules, UiState};
use crate::model::{KeyEdge, SystemEvent, TakeId};
use crate::ports::{Clipboard, ClipboardError, Inserter, Microphone, TargetProbe};
use crate::rambler::RamblerAccess;
use crate::settings::Settings;
use crate::storage::history::History;
use runtime::{Command, Inbox};
use state::Input;
use std::sync::Arc;
use tokio::sync::{oneshot, watch};

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
    #[error("Aucun texte disponible dans le brouillon.")]
    NoDraft,
    #[error("Impossible de copier dans le presse-papiers de façon privée.")]
    Clipboard(#[from] ClipboardError),
}

pub struct Controller;

impl Controller {
    /// Spawn the controller task on the current Tokio runtime.
    pub fn spawn(deps: ControllerDeps, init: ControllerInit) -> ControllerHandle {
        runtime::spawn(deps, init, None)
    }

    pub(crate) fn spawn_with_profile(
        deps: ControllerDeps,
        init: ControllerInit,
        rules: Arc<PersonalRules>,
        history: History,
    ) -> ControllerHandle {
        runtime::spawn(deps, init, Some((rules, history)))
    }
}

/// Cheap to clone. Synchronous methods enqueue without waiting for capacity;
/// `Finish`, `Cancel` and `Shutdown` are never dropped while the actor is alive.
#[derive(Clone)]
pub struct ControllerHandle {
    inbox: Inbox,
    state_rx: watch::Receiver<UiState>,
    level_rx: watch::Receiver<f32>,
    stopped: watch::Receiver<bool>,
}

impl ControllerHandle {
    /// Edge of the configured global shortcut (hold or toggle per settings).
    pub fn hotkey(&self, edge: KeyEdge) {
        self.inbox.send(Command::Input(Input::Hotkey(edge)));
    }

    /// "Dicter dans Sambler": start a take whose only destination is the draft.
    pub fn dictate_in_sambler(&self) {
        self.inbox.send(Command::Input(Input::StartInternal));
    }

    /// Finish the identified take (stale ids are ignored).
    pub fn finish(&self, take: TakeId) {
        self.inbox.send(Command::Input(Input::Finish(take)));
    }

    /// Cancel the identified take, or the current one when `None`.
    pub fn cancel(&self, take: Option<TakeId>) {
        self.inbox.send(Command::Input(Input::Cancel(take)));
    }

    pub fn system(&self, event: SystemEvent) {
        self.inbox.send(Command::Input(Input::System(event)));
    }

    /// New settings; apply to the next take.
    pub fn apply_settings(&self, settings: Settings) {
        self.inbox.send(Command::Input(Input::Settings(settings)));
    }

    pub(crate) fn apply_personal_rules(&self, rules: Arc<PersonalRules>) {
        self.inbox.send(Command::Input(Input::PersonalRules(rules)));
    }

    /// Enabling may warm the engine; withdrawing stops any take immediately.
    pub fn set_consent(&self, enabled: bool) {
        self.inbox.send(Command::Input(Input::Consent(enabled)));
    }

    /// Replace (or remove) the Rambler access; closes and re-warms the engine.
    pub fn set_access(&self, access: Option<RamblerAccess>) {
        self.inbox.send(Command::Access(access));
    }

    /// Explicit retry after "Dictée indisponible".
    pub fn retry_connection(&self) {
        self.inbox.send(Command::Input(Input::Retry));
    }

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
        let (reply, result) = oneshot::channel();
        self.inbox.send(Command::Draft(reply));
        result.await.ok().flatten()
    }

    /// Explicit "Copier" of the draft's available text.
    pub async fn copy_draft(&self) -> Result<(), CopyError> {
        let (reply, result) = oneshot::channel();
        self.inbox.send(Command::Copy(reply));
        result.await.unwrap_or(Err(CopyError::NoDraft))
    }

    pub fn clear_draft(&self) {
        self.inbox.send(Command::Input(Input::ClearDraft));
    }

    /// Stop any take, release the microphone, close the engine. Idempotent.
    pub async fn shutdown(&self) {
        self.inbox.send(Command::Input(Input::Shutdown));
        let _ = self.stopped.clone().wait_for(|stopped| *stopped).await;
    }
}

#[cfg(test)]
mod tests;
