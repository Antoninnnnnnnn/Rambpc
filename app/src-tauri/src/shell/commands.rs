//! Tauri commands of the main window. See `crate::ipc` for the contract.

use super::{AppState, hotkey};
use crate::controller::CopyError;
use crate::ipc::{
    AccessInput, AccessSource, AccessStatus, CommandError, DraftView, EVENT_MIC_TEST_LEVEL,
    HistoryPage, HistoryQuery, HistoryStatus, PersonalRules, PersonalRulesInput, SaveRulesResult,
    UiState, UsageStats,
};
use crate::model::TakeId;
use crate::ports::{ClipboardError, MicError, MicInfo};
use crate::rambler::{AccessError, RamblerAccess};
use crate::settings::Settings;
use crate::storage::{self, history::HistoryError};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use zeroize::Zeroizing;

type CmdResult<T> = Result<T, CommandError>;

fn internal_error() -> CommandError {
    CommandError::new("internal", "Erreur interne.")
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| internal_error())
}

fn current_state(state: &AppState) -> UiState {
    state.controller.state().borrow().clone()
}

#[tauri::command]
pub fn get_state(state: State<'_, AppState>) -> UiState {
    current_state(&state)
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, settings: Settings) -> CmdResult<Settings> {
    settings
        .validate()
        .map_err(|e| CommandError::new("invalid-settings", format!("Réglage invalide : {e}")))?;
    blocking(move || save_settings_inner(&app, settings)).await?
}

fn save_settings_inner(app: &AppHandle, mut settings: Settings) -> CmdResult<Settings> {
    settings.normalize();
    let state = tauri::Manager::state::<AppState>(app);
    let _write = state.settings_write.lock().map_err(|_| internal_error())?;
    let previous = state.settings.lock().map_err(|_| internal_error())?.clone();
    if !previous.shortcut.eq_ignore_ascii_case(&settings.shortcut) {
        hotkey::rebind_dictation(app, &previous.shortcut, &settings.shortcut).map_err(
            |reason| {
                CommandError::new(
                    "shortcut",
                    format!(
                        "Le raccourci n'a pas pu être enregistré ({reason}). L'ancien est conservé."
                    ),
                )
            },
        )?;
    }
    let mut settings = settings;
    settings.version = crate::settings::SETTINGS_VERSION;
    settings.launch_at_startup = previous.launch_at_startup; // Only the dedicated command can change HKCU Run.
    let mut history_result = Ok(());
    if state.store.root().ephemeral {
        settings.history.enabled = false;
    } else if settings.history != previous.history {
        // Withdraw permission before storage work. Enabling requires a successfully
        // saved, account-bound opt-in before the native writer can admit a take.
        if !settings.history.enabled {
            history_result = state
                .history
                .configure(&settings.history)
                .map_err(history_error);
        }
        match state.store.save_history_options(&settings.history) {
            Ok(()) if settings.history.enabled => {
                history_result = state
                    .history
                    .configure(&settings.history)
                    .map_err(history_error);
            }
            Ok(()) => (),
            Err(_) => {
                if settings.history.enabled {
                    settings.history = previous.history.clone();
                }
                history_result = Err(CommandError::new(
                    "save-failed",
                    "Le choix d’historique n’a pas pu être enregistré de façon protégée.",
                ));
            }
        }
    }
    let persisted = state.store.save_settings(&settings);
    *state.settings.lock().map_err(|_| internal_error())? = settings.clone();
    #[cfg(windows)]
    crate::platform::windows::set_insertion_method(settings.insertion);
    state.controller.apply_settings(settings.clone());
    history_result?;
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

#[tauri::command]
pub async fn start_mic_test(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let choice = state
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .microphone
        .clone();
    let microphone = state.microphone.clone();
    let test = state.mic_test.clone();
    let Some(stopped) = test.request() else {
        return Ok(());
    };
    let (opened, reply) = tokio::sync::oneshot::channel();
    // The worker, not the invoke future, owns teardown even if the view closes.
    tauri::async_runtime::spawn(async move {
        test.run(microphone, choice, stopped, opened, |level| {
            let _ = app.emit(EVENT_MIC_TEST_LEVEL, level);
        })
        .await;
    });
    reply
        .await
        .map_err(|_| CommandError::new("internal", "Erreur interne."))?
        .map_err(mic_error)
}

#[tauri::command]
pub fn stop_mic_test(state: State<'_, AppState>) {
    state.mic_test.stop();
}

#[tauri::command]
pub async fn set_consent(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> CmdResult<UiState> {
    {
        let mut current = state.consent.lock().unwrap_or_else(|e| e.into_inner());
        *current = enabled;
        if !enabled {
            state.transcriptions.cancel_active();
        }
        state.controller.set_consent(enabled);
    }
    let saved = blocking(move || {
        let state = tauri::Manager::state::<AppState>(&app);
        let _write = state
            .consent_write
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let current = *state.consent.lock().unwrap_or_else(|e| e.into_inner());
        state.store.save_consent(current)
    })
    .await?;
    match saved {
        Ok(()) | Err(storage::StoreError::Ephemeral) => Ok(current_state(&state)),
        Err(_) => Err(CommandError::new(
            "save-failed",
            "Le choix s'applique pour cette session mais n'a pas pu être enregistré.",
        )),
    }
}

fn access_status(state: &AppState) -> AccessStatus {
    let access = state.access.lock().unwrap_or_else(|e| e.into_inner());
    storage::access_status(access.access.as_ref(), access.source, &state.store)
}

#[tauri::command]
pub fn get_access(state: State<'_, AppState>) -> AccessStatus {
    access_status(&state)
}

#[tauri::command]
pub async fn set_access(app: AppHandle, access: AccessInput) -> CmdResult<AccessStatus> {
    let access = AccessOnDrop(access);
    blocking(move || set_access_inner(&app, &access.0)).await?
}

fn set_access_inner(app: &AppHandle, access: &AccessInput) -> CmdResult<AccessStatus> {
    let state = tauri::Manager::state::<AppState>(app);
    let _write = state.access_write.lock().map_err(|_| internal_error())?;
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
        state.store.save_access(access).map_err(|_| {
            CommandError::new(
                "save-failed",
                "L'accès n'a pas pu être enregistré de façon protégée.",
            )
        })?;
        AccessSource::Saved
    } else {
        AccessSource::Session
    };
    {
        let mut current = state.access.lock().unwrap_or_else(|e| e.into_inner());
        state.transcriptions.cancel_active();
        current.access = Some(parsed.clone());
        current.source = source;
    }
    state.controller.set_access(Some(parsed));
    Ok(access_status(&state))
}

struct AccessOnDrop(AccessInput);
impl Drop for AccessOnDrop {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.0.api_key.zeroize();
        self.0.android_package.zeroize();
        self.0.android_cert.zeroize();
    }
}

#[tauri::command]
pub async fn clear_access(app: AppHandle) -> CmdResult<AccessStatus> {
    blocking(move || {
        let state = tauri::Manager::state::<AppState>(&app);
        let _write = state.access_write.lock().map_err(|_| internal_error())?;
        {
            let mut current = state.access.lock().unwrap_or_else(|e| e.into_inner());
            state.transcriptions.cancel_active();
            current.access = None;
            current.source = AccessSource::None;
        }
        state.controller.set_access(None);
        state.store.forget_access().map_err(|_| CommandError::new(
            "save-failed",
            "L’accès a été retiré pour cette session, mais la copie protégée n’a pas pu être supprimée.",
        ))?;
        Ok(access_status(&state))
    }).await?
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
pub async fn get_recent_drafts(state: State<'_, AppState>) -> CmdResult<Vec<DraftView>> {
    Ok(state.controller.recent_drafts().await)
}

#[tauri::command]
pub async fn get_stats(state: State<'_, AppState>) -> CmdResult<UsageStats> {
    state.controller.stats().await.ok_or_else(internal_error)
}

#[tauri::command]
pub async fn get_activity(state: State<'_, AppState>) -> CmdResult<crate::ipc::ActivityStats> {
    state.controller.activity().await.ok_or_else(internal_error)
}

#[tauri::command]
pub async fn copy_draft(state: State<'_, AppState>, take: Option<u64>) -> CmdResult<()> {
    state
        .controller
        .copy_draft(take.map(TakeId))
        .await
        .map_err(|error| match error {
            CopyError::NoDraft => CommandError::new("no-draft", "Aucun texte à copier."),
            CopyError::Clipboard(ClipboardError::Busy) => CommandError::new(
                "clipboard-busy",
                "Le presse-papiers est occupé par une autre application. Réessayez.",
            ),
            CopyError::Clipboard(ClipboardError::ExclusionFailed) => CommandError::new(
                "clipboard-exclusion-failed",
                "Copie annulée : impossible d'exclure le texte de l'historique du presse-papiers.",
            ),
            CopyError::Clipboard(ClipboardError::Other) => CommandError::new(
                "clipboard-other",
                "La copie a échoué. Le texte reste dans Sambler.",
            ),
        })
}

#[tauri::command]
pub async fn clear_draft(state: State<'_, AppState>, take: Option<u64>) -> CmdResult<UiState> {
    Ok(state.controller.clear_draft(take.map(TakeId)).await)
}

/// Settings preview of the text rules on a sample sentence (no microphone, no network).
#[tauri::command]
pub fn preview_render(
    state: State<'_, AppState>,
    sample: String,
    settings: Settings,
) -> CmdResult<String> {
    if sample.chars().count() > 500 {
        return Err(CommandError::new(
            "too-long",
            "Phrase d'exemple trop longue.",
        ));
    }
    let mut rules = crate::text::TextRules::from_settings(&settings);
    let personal = state
        .personal_rules
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    rules.lexicon.clone_from(&personal.lexicon);
    rules.expressions.clone_from(&personal.expressions);
    Ok(crate::text::preview(&sample, &rules))
}

#[tauri::command]
pub fn open_mic_privacy_settings() {
    crate::platform::open_mic_privacy_settings();
}

#[tauri::command]
pub fn get_personal_rules(state: State<'_, AppState>) -> PersonalRules {
    get_rules(&state)
}

fn get_rules(state: &AppState) -> PersonalRules {
    let rules = state
        .personal_rules
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    (*rules).clone()
}

#[tauri::command]
pub async fn save_personal_rules(
    app: AppHandle,
    rules: PersonalRulesInput,
) -> CmdResult<SaveRulesResult> {
    blocking(move || save_personal_rules_inner(&app, rules)).await?
}

fn save_personal_rules_inner(
    app: &AppHandle,
    rules: PersonalRulesInput,
) -> CmdResult<SaveRulesResult> {
    let state = tauri::Manager::state::<AppState>(app);
    let _write = state.rules_write.lock().map_err(|_| internal_error())?;
    let conflicts = state.store.save_personal_rules(&rules).map_err(|_| {
        CommandError::new(
            "save-failed",
            "Les règles personnelles n’ont pas pu être enregistrées de façon protégée.",
        )
    })?;
    if !conflicts.is_empty() {
        return Ok(SaveRulesResult {
            saved: false,
            rules: get_rules(&state),
            conflicts,
        });
    }
    let updated = Arc::new(PersonalRules {
        lexicon: rules.lexicon,
        expressions: rules.expressions,
        unreadable: false,
    });
    *state.personal_rules.lock().map_err(|_| internal_error())? = updated.clone();
    state.controller.apply_personal_rules(updated.clone());
    Ok(SaveRulesResult {
        saved: true,
        rules: (*updated).clone(),
        conflicts,
    })
}

fn history_error(error: HistoryError) -> CommandError {
    match error {
        HistoryError::Unavailable => CommandError::new(
            "history-unavailable",
            "L’historique est indisponible pour cette session.",
        ),
        HistoryError::Database | HistoryError::Protection => CommandError::new(
            "history-storage",
            "L’opération sur l’historique n’a pas abouti. Vérifiez le dossier de données.",
        ),
        HistoryError::NoText => CommandError::new(
            "no-text",
            "Cette dictée a expiré ou son texte est illisible.",
        ),
        HistoryError::Clipboard(ClipboardError::Busy) => {
            CommandError::new("clipboard-busy", "Le presse-papiers est occupé. Réessayez.")
        }
        HistoryError::Clipboard(ClipboardError::ExclusionFailed) => CommandError::new(
            "clipboard-exclusion-failed",
            "Copie annulée : impossible d’exclure le texte de l’historique du presse-papiers.",
        ),
        HistoryError::Clipboard(ClipboardError::Other) => CommandError::new(
            "clipboard-other",
            "La copie a échoué. La dictée reste dans l’historique.",
        ),
    }
}

async fn history_io<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, HistoryError> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| history_error(HistoryError::Unavailable))?
        .map_err(history_error)
}

#[tauri::command]
pub async fn get_history(
    state: State<'_, AppState>,
    query: HistoryQuery,
) -> CmdResult<HistoryPage> {
    let history = state.history.clone();
    history_io(move || history.page(query)).await
}

#[tauri::command]
pub async fn copy_history_entry(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let history = state.history.clone();
    history_io(move || history.copy(id)).await
}

#[tauri::command]
pub async fn delete_history_entry(
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<HistoryStatus> {
    let history = state.history.clone();
    history_io(move || history.delete(id)).await
}

#[tauri::command]
pub async fn clear_history(app: AppHandle, disable: bool) -> CmdResult<HistoryStatus> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = tauri::Manager::state::<AppState>(&app);
        let _write = state.settings_write.lock().map_err(|_| internal_error())?;
        let mut settings = state.settings.lock().map_err(|_| internal_error())?.clone();
        if disable {
            settings.history.enabled = false;
        }
        state.history.invalidate_for_clear(&settings.history);
        let saved = if disable {
            *state.settings.lock().map_err(|_| internal_error())? = settings.clone();
            state.controller.apply_settings(settings.clone());
            state.store.save_history_options(&settings.history)
        } else {
            Ok(())
        };
        let cleared = state.history.clear();
        match saved {
            Ok(()) | Err(storage::StoreError::Ephemeral) => cleared.map_err(history_error),
            Err(_) => Err(CommandError::new(
                "save-failed",
                "L’activation de l’historique n’a pas pu être modifiée sur le disque.",
            )),
        }
    })
    .await
    .map_err(|_| history_error(HistoryError::Unavailable))?
}

#[tauri::command]
pub async fn get_history_status(state: State<'_, AppState>) -> CmdResult<HistoryStatus> {
    let history = state.history.clone();
    history_io(move || Ok(history.status())).await
}

#[tauri::command]
pub async fn skip_history_next_take(
    state: State<'_, AppState>,
    skip: bool,
) -> CmdResult<HistoryStatus> {
    let history = state.history.clone();
    history_io(move || Ok(history.skip_next(skip))).await
}

#[tauri::command]
pub async fn set_launch_at_startup(app: AppHandle, enabled: bool) -> CmdResult<()> {
    blocking(move || set_launch_at_startup_inner(&app, enabled)).await?
}

fn set_launch_at_startup_inner(app: &AppHandle, enabled: bool) -> CmdResult<()> {
    let state = tauri::Manager::state::<AppState>(app);
    let _write = state.settings_write.lock().map_err(|_| internal_error())?;
    let mut settings = state.settings.lock().map_err(|_| internal_error())?.clone();
    storage::set_startup(state.store.root(), enabled).map_err(|error| match error {
        storage::StartupError::Portable => CommandError::new(
            "portable",
            "Le démarrage avec Windows est réservé à l’édition installée.",
        ),
        storage::StartupError::Ephemeral => CommandError::new(
            "ephemeral",
            "Le démarrage avec Windows ne peut pas être modifié pendant une session éphémère.",
        ),
        storage::StartupError::Registry => CommandError::new(
            "startup",
            "Windows n’a pas pu modifier le démarrage automatique de Sambler.",
        ),
    })?;
    settings.launch_at_startup = enabled;
    *state.settings.lock().map_err(|_| internal_error())? = settings.clone();
    state.controller.apply_settings(settings.clone());
    state.store.save_settings(&settings).map_err(|_| {
        CommandError::new(
            "save-failed",
            "Le démarrage Windows a été modifié, mais les réglages n’ont pas pu être enregistrés.",
        )
    })
}

#[tauri::command]
pub async fn pick_audio_file(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<Option<String>> {
    #[cfg(windows)]
    let path = {
        use tauri::Manager;
        let owner = app
            .get_webview_window(super::MAIN_WINDOW)
            .ok_or_else(internal_error)?
            .hwnd()
            .map_err(|_| internal_error())?
            .0 as isize;
        blocking(move || super::transcribe::pick(owner)).await??
    };
    #[cfg(not(windows))]
    let path: Option<std::path::PathBuf> = {
        let _ = app;
        None
    };
    path.map(|path| {
        state.transcriptions.allow(&path).ok_or_else(|| {
            if super::transcribe::supported(&path) {
                CommandError::new("audio-picker", "Le chemin du fichier audio est illisible.")
            } else {
                unsupported_audio()
            }
        })
    })
    .transpose()
}

fn unsupported_audio() -> CommandError {
    CommandError::new(
        "unsupported-audio",
        "Format non pris en charge : convertissez en WAV ou Ogg",
    )
}

#[tauri::command]
pub fn transcribe_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CmdResult<crate::ipc::FileTranscription> {
    if !super::transcribe::supported(std::path::Path::new(&path)) {
        return Err(unsupported_audio());
    }
    // Keep both locks until registration so revocation/credential changes
    // either precede admission or cancel that exact job afterwards.
    let consent = state.consent.lock().map_err(|_| internal_error())?;
    if !*consent {
        return Err(CommandError::new(
            "consent-required",
            "Autorisez l’envoi audio à Google dans Sambler avant de transcrire un fichier.",
        ));
    }
    let access = state.access.lock().map_err(|_| internal_error())?;
    let credentials = access.access.clone().ok_or_else(|| {
        CommandError::new(
            "access-required",
            "Configurez l’accès à Rambler avant de transcrire un fichier.",
        )
    })?;
    let mut rules = crate::text::TextRules::from_settings(&get_settings(state.clone()));
    {
        let personal = state.personal_rules.lock().map_err(|_| internal_error())?;
        rules.lexicon.clone_from(&personal.lexicon);
        rules.expressions.clone_from(&personal.expressions);
    }
    state.transcriptions.start(
        path,
        credentials,
        rules,
        Arc::new(move |update| {
            let _ = app.emit_to(super::MAIN_WINDOW, crate::ipc::EVENT_TRANSCRIPTION, update);
        }),
    )
}

#[tauri::command]
pub async fn cancel_transcription(
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<crate::ipc::FileTranscription> {
    state.transcriptions.cancel(&id).await
}

#[tauri::command]
pub fn list_transcriptions(state: State<'_, AppState>) -> Vec<crate::ipc::FileTranscription> {
    state.transcriptions.list()
}

#[allow(dead_code)]
fn _assert_send_sync() {
    fn check<T: Send + Sync>() {}
    check::<Arc<AppState>>();
}
