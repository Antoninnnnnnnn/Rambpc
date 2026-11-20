//! Tauri commands of the main window. See `crate::ipc` for the contract.

use super::mic_test::{Level, LevelOnlySink};
use super::{AppState, hotkey};
use crate::controller::CopyError;
use crate::ipc::{
    AccessInput, AccessSource, AccessStatus, CommandError, DraftView, EVENT_MIC_TEST_LEVEL,
    UiState,
};
use crate::model::TakeId;
use crate::ports::{ClipboardError, MicError, MicInfo};
use crate::rambler::{AccessError, RamblerAccess};
use crate::settings::Settings;
use crate::storage;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use zeroize::Zeroizing;

type CmdResult<T> = Result<T, CommandError>;

fn current_state(state: &AppState) -> UiState {
    state.controller.state().borrow().clone()
}

#[tauri::command]
pub fn get_state(state: State<'_, AppState>) -> UiState {
    current_state(&state)
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.lock().expect("settings lock").clone()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> CmdResult<Settings> {
    settings
        .validate()
        .map_err(|e| CommandError::new("invalid-settings", format!("Réglage invalide : {e}")))?;
    let previous = state.settings.lock().expect("settings lock").clone();
    if !previous.shortcut.eq_ignore_ascii_case(&settings.shortcut) {
        hotkey::rebind_dictation(&app, &previous.shortcut, &settings.shortcut).map_err(|reason| {
            CommandError::new(
                "shortcut",
                format!("Le raccourci n'a pas pu être enregistré ({reason}). L'ancien est conservé."),
            )
        })?;
    }
    let mut settings = settings;
    settings.version = crate::settings::SETTINGS_VERSION;
    let persisted = state.store.save_settings(&settings);
    *state.settings.lock().expect("settings lock") = settings.clone();
    state.controller.apply_settings(settings.clone());
    match persisted {
        Ok(()) | Err(storage::StoreError::Ephemeral) => Ok(settings),
        Err(_) => Err(CommandError::new(
            "save-failed",
            "Les réglages s'appliquent mais n'ont pas pu être enregistrés sur le disque.",
        )),
    }
}

fn mic_error(error: MicError) -> CommandError {
    let message = match &error {
        MicError::NoInput => "Aucun micro n'est disponible.".to_string(),
        MicError::NotFound => "Le micro choisi est introuvable.".to_string(),
        MicError::AccessDenied => {
            "Windows refuse l'accès au micro. Vérifiez « Confidentialité et sécurité → Microphone ».".to_string()
        }
        MicError::UnsupportedFormat { format } => {
            format!("Format du micro non pris en charge ({format}). Choisissez un autre micro.")
        }
        MicError::OpenTimeout => "Le micro n'a pas démarré à temps.".to_string(),
        MicError::Disconnected => "Le micro a été débranché.".to_string(),
        MicError::Other => "Erreur du micro.".to_string(),
    };
    let code = serde_json::to_value(&error)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "other".into());
    CommandError::new(&format!("mic-{code}"), message)
}

#[tauri::command]
pub async fn list_microphones(state: State<'_, AppState>) -> CmdResult<Vec<MicInfo>> {
    let microphone = state.microphone.clone();
    tauri::async_runtime::spawn_blocking(move || microphone.inputs())
        .await
        .map_err(|_| CommandError::new("internal", "Erreur interne."))?
        .map_err(mic_error)
}

static MIC_TEST_RUNNING: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub async fn start_mic_test(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    stop_mic_test_inner(&state);
    let choice = state.settings.lock().expect("settings lock").microphone.clone();
    let microphone = state.microphone.clone();
    let level = Level::default();
    let sink_level = level.clone();
    let stream = tauri::async_runtime::spawn_blocking(move || {
        microphone.open(
            &choice,
            Box::new(move |_rate| Box::new(LevelOnlySink::new(sink_level))),
            Box::new(|_fault| {}),
        )
    })
    .await
    .map_err(|_| CommandError::new("internal", "Erreur interne."))?
    .map_err(mic_error)?;
    *state.mic_test.lock().expect("mic test lock") = Some(stream);
    MIC_TEST_RUNNING.store(true, Ordering::SeqCst);
    tauri::async_runtime::spawn(async move {
        // Bounded to 60 s so a forgotten test never keeps the microphone open.
        for _ in 0..1200 {
            if !MIC_TEST_RUNNING.load(Ordering::SeqCst) {
                break;
            }
            let _ = app.emit(EVENT_MIC_TEST_LEVEL, level.get());
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if MIC_TEST_RUNNING.swap(false, Ordering::SeqCst) {
            if let Some(state) = tauri::Manager::try_state::<AppState>(&app) {
                stop_mic_test_inner(&state);
            }
        }
        let _ = app.emit(EVENT_MIC_TEST_LEVEL, 0.0_f32);
    });
    Ok(())
}

fn stop_mic_test_inner(state: &AppState) {
    MIC_TEST_RUNNING.store(false, Ordering::SeqCst);
    if let Some(stream) = state.mic_test.lock().expect("mic test lock").take() {
        stream.stop();
    }
}

#[tauri::command]
pub fn stop_mic_test(state: State<'_, AppState>) {
    stop_mic_test_inner(&state);
}

#[tauri::command]
pub fn set_consent(state: State<'_, AppState>, enabled: bool) -> CmdResult<UiState> {
    let saved = state.store.save_consent(enabled);
    *state.consent.lock().expect("consent lock") = enabled;
    state.controller.set_consent(enabled);
    match saved {
        Ok(()) | Err(storage::StoreError::Ephemeral) => Ok(current_state(&state)),
        Err(_) => Err(CommandError::new(
            "save-failed",
            "Le choix s'applique pour cette session mais n'a pas pu être enregistré.",
        )),
    }
}

fn access_status(state: &AppState) -> AccessStatus {
    let access = state.access.lock().expect("access lock");
    storage::access_status(access.access.as_ref(), access.source, &state.store)
}

#[tauri::command]
pub fn get_access(state: State<'_, AppState>) -> AccessStatus {
    access_status(&state)
}

#[tauri::command]
pub fn set_access(state: State<'_, AppState>, access: AccessInput) -> CmdResult<AccessStatus> {
    let parsed = RamblerAccess::new(
        &access.authority,
        Zeroizing::new(access.api_key.clone()),
        Zeroizing::new(access.android_package.clone()),
        Zeroizing::new(access.android_cert.clone()),
    )
    .map_err(|error| match error {
        AccessError::InvalidAuthority => CommandError::new(
            "invalid-authority",
            "Adresse du service invalide (attendu : hôte:port).",
        ),
        AccessError::InvalidCredentials => CommandError::new(
            "invalid-access",
            "Les trois valeurs d'accès sont requises et doivent être valides.",
        ),
    })?;
    let source = if access.remember && state.store.can_remember_access() {
        state.store.save_access(&access).map_err(|_| {
            CommandError::new("save-failed", "L'accès n'a pas pu être enregistré de façon protégée.")
        })?;
        AccessSource::Saved
    } else {
        AccessSource::Session
    };
    zeroize_input(access);
    {
        let mut current = state.access.lock().expect("access lock");
        current.access = Some(parsed.clone());
        current.source = source;
    }
    state.controller.set_access(Some(parsed));
    Ok(access_status(&state))
}

fn zeroize_input(mut access: AccessInput) {
    use zeroize::Zeroize;
    access.api_key.zeroize();
    access.android_package.zeroize();
    access.android_cert.zeroize();
}

#[tauri::command]
pub fn clear_access(state: State<'_, AppState>) -> CmdResult<AccessStatus> {
    let _ = state.store.forget_access();
    {
        let mut current = state.access.lock().expect("access lock");
        current.access = None;
        current.source = AccessSource::None;
    }
    state.controller.set_access(None);
    Ok(access_status(&state))
}

#[tauri::command]
pub fn retry_connection(state: State<'_, AppState>) -> UiState {
    state.controller.retry_connection();
    current_state(&state)
}

#[tauri::command]
pub fn dictate_in_sambler(state: State<'_, AppState>) -> UiState {
    state.controller.dictate_in_sambler();
    current_state(&state)
}

#[tauri::command]
pub fn finish_take(state: State<'_, AppState>, take: u64) -> UiState {
    state.controller.finish(TakeId(take));
    current_state(&state)
}

#[tauri::command]
pub fn cancel_take(state: State<'_, AppState>, take: u64) -> UiState {
    state.controller.cancel(Some(TakeId(take)));
    current_state(&state)
}

#[tauri::command]
pub async fn get_draft(state: State<'_, AppState>) -> CmdResult<Option<DraftView>> {
    Ok(state.controller.draft().await)
}

#[tauri::command]
pub async fn copy_draft(state: State<'_, AppState>) -> CmdResult<()> {
    state.controller.copy_draft().await.map_err(|error| match error {
        CopyError::NoDraft => CommandError::new("no-draft", "Aucun texte à copier."),
        CopyError::Clipboard(ClipboardError::Busy) => CommandError::new(
            "clipboard-busy",
            "Le presse-papiers est occupé par une autre application. Réessayez.",
        ),
        CopyError::Clipboard(ClipboardError::ExclusionFailed) => CommandError::new(
            "clipboard-exclusion",
            "Copie annulée : impossible d'exclure le texte de l'historique du presse-papiers.",
        ),
        CopyError::Clipboard(ClipboardError::Other) => {
            CommandError::new("clipboard", "La copie a échoué. Le texte reste dans Sambler.")
        }
    })
}

#[tauri::command]
pub fn clear_draft(state: State<'_, AppState>) -> UiState {
    state.controller.clear_draft();
    current_state(&state)
}

#[tauri::command]
pub fn open_mic_privacy_settings() {
    crate::platform::open_mic_privacy_settings();
}

#[allow(dead_code)]
fn _assert_send_sync() {
    fn check<T: Send + Sync>() {}
    check::<Arc<AppState>>();
}
