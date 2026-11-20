//! Forwards controller state to the WebViews (main window and floating bar),
//! shows or hides the floating bar, and manages the Escape registration. State and level are "latest wins" watches, so a slow
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
    RefusedSelection,
    RefusedProtected,
    RefusedUncertain,
    RefusedPending,
    RefusedConsent,
    RefusedMicrophone,
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
            Capsule::RefusedSelection => "Texte sélectionné : non démarrée",
            Capsule::RefusedProtected => "Champ protégé : micro non ouvert",
            Capsule::RefusedUncertain => "Champ non reconnu : dictez dans Sambler",
            Capsule::RefusedPending => "Un texte attend : rappuyez pour remplacer",
            Capsule::RefusedConsent => "Dictée non activée dans Sambler",
            Capsule::RefusedMicrophone => "Micro indisponible",
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
            TakeOutcome::Refused { reason } => match reason {
                RefusalReason::Unavailable { .. } | RefusalReason::NeedsAccess => {
                    Capsule::Unavailable
                }
                RefusalReason::NeedsConsent => Capsule::RefusedConsent,
                RefusalReason::ProtectedField => Capsule::RefusedProtected,
                RefusalReason::UncertainTarget {
                    reason: crate::ports::UncertainReason::Selection,
                } => Capsule::RefusedSelection,
                RefusalReason::UncertainTarget { .. } => Capsule::RefusedUncertain,
                RefusalReason::PendingDraft => Capsule::RefusedPending,
                RefusalReason::Microphone { .. } => Capsule::RefusedMicrophone,
            },
        }
    }
}

/// How long an outcome stays visible in the bar.
const OUTCOME_VISIBLE: Duration = Duration::from_millis(2500);

/// The floating bar: the cross-platform WebView overlay, or the native Win32
/// capsule as a fallback when the overlay window cannot be created.
enum Bar<R: Runtime> {
    Overlay(super::overlay::Overlay<R>),
    #[cfg(windows)]
    Native(super::indicator::Indicator),
    #[cfg(not(windows))]
    None,
}

impl<R: Runtime> Clone for Bar<R> {
    fn clone(&self) -> Self {
        match self {
            Bar::Overlay(overlay) => Bar::Overlay(overlay.clone()),
            #[cfg(windows)]
            Bar::Native(indicator) => Bar::Native(indicator.clone()),
            #[cfg(not(windows))]
            Bar::None => Bar::None,
        }
    }
}

impl<R: Runtime> Bar<R> {
    fn new(app: &AppHandle<R>) -> Self {
        let webview_dir = app
            .try_state::<super::AppState>()
            .map(|state| state.store.root().webview_dir.clone());
        if let Some(dir) = webview_dir
            && let Some(overlay) = super::overlay::create(app, dir)
        {
            return Bar::Overlay(overlay);
        }
        #[cfg(windows)]
        return Bar::Native(super::indicator::Indicator::spawn());
        #[cfg(not(windows))]
        Bar::None
    }

    /// The overlay page renders the state itself from the events.
    fn show(&self, capsule: Capsule) {
        match self {
            Bar::Overlay(overlay) => {
                if capsule == Capsule::Hidden {
                    overlay.hide();
                } else {
                    overlay.show();
                }
            }
            #[cfg(windows)]
            Bar::Native(indicator) => indicator.show(capsule),
            #[cfg(not(windows))]
            Bar::None => {}
        }
    }

    fn level(&self, level: f32) {
        #[cfg(windows)]
        if let Bar::Native(indicator) = self {
            indicator.level(level);
        }
        let _ = level;
    }
}

pub fn start<R: Runtime>(app: AppHandle<R>, controller: ControllerHandle) {
    let capsule = Bar::new(&app);

    // State relay.
    {
        let app = app.clone();
        let mut rx = controller.state();
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

    // Level relay, ~30 Hz for both the main window and the floating bar.
    {
        let mut rx = controller.level();
        tauri::async_runtime::spawn(async move {
            let mut last_emit = tokio::time::Instant::now();
            while rx.changed().await.is_ok() {
                let level = *rx.borrow_and_update();
                capsule.level(level);
                if last_emit.elapsed() >= Duration::from_millis(33) {
                    let _ = app.emit(EVENT_LEVEL, level);
                    last_emit = tokio::time::Instant::now();
                }
                tokio::time::sleep(Duration::from_millis(33)).await;
            }
        });
    }
}
