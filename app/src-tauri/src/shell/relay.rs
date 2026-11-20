//! Forwards controller state to the WebView, the native indicator and the
//! Escape registration. State and level are "latest wins" watches, so a slow
//! WebView never delays the controller.

use super::hotkey;
use crate::controller::ControllerHandle;
use crate::ipc::{EVENT_LEVEL, EVENT_STATE, Phase, TakeOutcome, UiState};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// What the native capsule should show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capsule {
    Hidden,
    Preparing,
    Listening,
    Processing,
    Inserted,
    CheckField,
    KeptInSambler,
    NoText,
    Cancelled,
    Interrupted,
    Unavailable,
    Refused,
}

impl Capsule {
    pub fn label(self) -> &'static str {
        match self {
            Capsule::Hidden => "",
            Capsule::Preparing => "Préparation…",
            Capsule::Listening => "Écoute",
            Capsule::Processing => "Traitement…",
            Capsule::Inserted => "Texte transmis",
            Capsule::CheckField => "Vérifiez le champ",
            Capsule::KeptInSambler => "Texte disponible dans Sambler",
            Capsule::NoText => "Aucun texte obtenu",
            Capsule::Cancelled => "Dictée annulée",
            Capsule::Interrupted => "Dictée interrompue",
            Capsule::Unavailable => "Dictée indisponible",
            Capsule::Refused => "Dictée non démarrée",
        }
    }

    fn for_outcome(outcome: &TakeOutcome) -> Capsule {
        use crate::ipc::RefusalReason;
        match outcome {
            TakeOutcome::Inserted => Capsule::Inserted,
            TakeOutcome::InsertedPartially => Capsule::CheckField,
            TakeOutcome::KeptInSambler { .. } => Capsule::KeptInSambler,
            TakeOutcome::NoText => Capsule::NoText,
            TakeOutcome::Cancelled => Capsule::Cancelled,
            TakeOutcome::Interrupted { .. } => Capsule::Interrupted,
            TakeOutcome::Refused {
                reason: RefusalReason::Unavailable { .. } | RefusalReason::NeedsAccess,
            } => Capsule::Unavailable,
            TakeOutcome::Refused { .. } => Capsule::Refused,
        }
    }
}

/// How long an outcome stays visible in the capsule.
const OUTCOME_VISIBLE: Duration = Duration::from_millis(2500);

pub fn start<R: Runtime>(app: AppHandle<R>, controller: ControllerHandle) {
    #[cfg(windows)]
    let capsule = super::indicator::Indicator::spawn();

    // State relay.
    {
        let app = app.clone();
        let mut rx = controller.state();
        #[cfg(windows)]
        let capsule = capsule.clone();
        tauri::async_runtime::spawn(async move {
            let mut previous: Option<UiState> = None;
            let mut hide_at: Option<tokio::time::Instant> = None;
            loop {
                let state = rx.borrow_and_update().clone();
                let _ = app.emit(EVENT_STATE, &state);
                let active = state.phase != Phase::Idle;
                let escape_enabled = app
                    .try_state::<super::AppState>()
                    .map(|s| s.settings.lock().map(|s| s.cancel_with_escape).unwrap_or(true))
                    .unwrap_or(true);
                hotkey::set_escape_active(&app, active && escape_enabled);

                let shown = match state.phase {
                    Phase::Preparing => Some(Capsule::Preparing),
                    Phase::Listening => Some(Capsule::Listening),
                    Phase::Finalizing => Some(Capsule::Processing),
                    Phase::Idle => {
                        let new_outcome = state.last_outcome.is_some()
                            && previous.as_ref().is_none_or(|p| {
                                p.last_outcome != state.last_outcome
                                    || p.take != state.take
                                    || p.phase != Phase::Idle
                            });
                        if new_outcome && previous.is_some() {
                            state.last_outcome.as_ref().map(Capsule::for_outcome)
                        } else {
                            None
                        }
                    }
                };
                #[cfg(windows)]
                match shown {
                    Some(c) if active => {
                        hide_at = None;
                        capsule.show(c);
                    }
                    Some(c) => {
                        hide_at = Some(tokio::time::Instant::now() + OUTCOME_VISIBLE);
                        capsule.show(c);
                    }
                    None if active => {}
                    None => {
                        if hide_at.is_none() {
                            capsule.show(Capsule::Hidden);
                        }
                    }
                }
                #[cfg(not(windows))]
                let _ = (shown, &mut hide_at);
                previous = Some(state);

                // Wait for the next change, or the outcome timeout.
                let changed = match hide_at {
                    Some(deadline) => {
                        tokio::select! {
                            r = rx.changed() => Some(r),
                            _ = tokio::time::sleep_until(deadline) => None,
                        }
                    }
                    None => Some(rx.changed().await),
                };
                match changed {
                    Some(Ok(())) => {}
                    Some(Err(_)) => break,
                    None => {
                        hide_at = None;
                        #[cfg(windows)]
                        capsule.show(Capsule::Hidden);
                        // Re-run the loop without a change: re-emit is harmless.
                        if rx.changed().await.is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }

    // Level relay, throttled to ~20 Hz for the WebView, ~30 Hz for the capsule.
    {
        let mut rx = controller.level();
        tauri::async_runtime::spawn(async move {
            let mut last_emit = tokio::time::Instant::now();
            while rx.changed().await.is_ok() {
                let level = *rx.borrow_and_update();
                #[cfg(windows)]
                capsule.level(level);
                if last_emit.elapsed() >= Duration::from_millis(50) {
                    let _ = app.emit(EVENT_LEVEL, level);
                    last_emit = tokio::time::Instant::now();
                }
                tokio::time::sleep(Duration::from_millis(33)).await;
            }
        });
    }
}
