//! Tauri shell: windows, commands, global shortcuts, tray and native indicator.
//!
//! Startup order matters: the data root (installed or portable) is resolved
//! before any WebView exists, then the main window is created from Rust with
//! its WebView2 data folder inside that root.

mod commands;
mod hotkey;
#[cfg(windows)]
mod indicator;
mod mic_test;
mod overlay;
mod relay;
#[path = "../transcribe/mod.rs"]
mod transcribe;
mod tray;
mod webview2;

use crate::controller::{Controller, ControllerDeps, ControllerHandle, ControllerInit};
use crate::ipc::{AccessSource, PersonalRules};
use crate::model::SystemEvent;
use crate::ports::{Microphone, SystemEvents};
use crate::rambler::RamblerAccess;
use crate::settings::Settings;
use crate::storage::history::History;
use crate::storage::{self, DataRoot, RootError, Store};
use std::sync::{Arc, Mutex};
use tauri::{Manager, WebviewWindowBuilder, WindowEvent};

pub const MAIN_WINDOW: &str = "main";

/// Access currently in effect and where it came from.
pub struct AccessState {
    pub access: Option<RamblerAccess>,
    pub source: AccessSource,
}

/// Shared by every command and background task.
pub struct AppState {
    pub controller: ControllerHandle,
    pub store: Store,
    pub settings: Mutex<Settings>,
    pub(crate) personal_rules: Mutex<Arc<PersonalRules>>,
    pub(crate) history: History,
    notes: Arc<storage::notes::Notes>,
    pub consent: Mutex<bool>,
    consent_write: Mutex<()>,
    settings_write: Mutex<()>,
    access_write: Mutex<()>,
    rules_write: Mutex<()>,
    exit_ready: std::sync::atomic::AtomicBool,
    pub access: Mutex<AccessState>,
    pub microphone: Arc<dyn Microphone>,
    mic_test: Arc<mic_test::MicTest>,
    transcriptions: Arc<transcribe::Transcriptions>,
    #[cfg(windows)]
    wasapi: Arc<crate::platform::windows::WasapiMicrophone>,
    /// Keeps the lock/suspend subscription alive.
    pub _system_events: Mutex<Option<crate::ports::Subscription>>,
}

pub fn run() {
    if !webview2::runtime_present() {
        show_fatal(webview2::MISSING_MESSAGE);
        return;
    }
    let root = match storage::resolve_root() {
        Ok(root) => root,
        Err(RootError::NotWritable { root }) => {
            if ask_ephemeral_session(&root) {
                storage::ephemeral_root(root)
            } else {
                return;
            }
        }
        Err(_) => {
            show_fatal("Sambler ne trouve pas son dossier de données.");
            return;
        }
    };

    let (cleanup, temporary_dirs) = std::sync::mpsc::channel();
    let mut shutting_down = false;
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::get_settings,
            commands::save_settings,
            commands::list_microphones,
            commands::start_mic_test,
            commands::stop_mic_test,
            commands::set_consent,
            commands::get_access,
            commands::set_access,
            commands::clear_access,
            commands::retry_connection,
            commands::dictate_in_sambler,
            commands::finish_take,
            commands::cancel_take,
            commands::get_draft,
            commands::get_recent_drafts,
            commands::copy_draft,
            commands::clear_draft,
            commands::get_stats,
            commands::get_activity,
            commands::get_known_apps,
            commands::open_mic_privacy_settings,
            commands::preview_render,
            commands::get_personal_rules,
            commands::save_personal_rules,
            commands::get_voice_commands,
            commands::list_notes,
            commands::get_note,
            commands::save_note,
            commands::delete_note,
            commands::get_history,
            commands::copy_history_entry,
            commands::delete_history_entry,
            commands::clear_history,
            commands::get_history_status,
            commands::skip_history_next_take,
            commands::set_launch_at_startup,
            commands::pick_audio_file,
            commands::transcribe_file,
            commands::cancel_transcription,
            commands::list_transcriptions,
        ])
        .setup(move |app| {
            setup(app, root.clone())?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Sambler")
        .run_return(move |app, event| match event {
            tauri::RunEvent::ExitRequested { api, code, .. } => {
                if let Some(state) = app.try_state::<AppState>() {
                    if state.exit_ready.load(std::sync::atomic::Ordering::Acquire) {
                        return;
                    }
                    api.prevent_exit();
                    if !std::mem::replace(&mut shutting_down, true) {
                        state.mic_test.close();
                        state.controller.request_shutdown();
                        let app = app.clone();
                        // Keep the event loop pumping while capture, the engine,
                        // and storage finish their blocking work.
                        tauri::async_runtime::spawn(async move {
                            let state = app.state::<AppState>();
                            tokio::join!(
                                state.controller.shutdown(),
                                state.mic_test.drain(),
                                state.transcriptions.shutdown(),
                            );
                            #[cfg(windows)]
                            {
                                let wasapi = state.wasapi.clone();
                                let _ = tauri::async_runtime::spawn_blocking(move || {
                                    wasapi.join_late_open()
                                })
                                .await;
                            }
                            state
                                .exit_ready
                                .store(true, std::sync::atomic::Ordering::Release);
                            app.exit(code.unwrap_or(0));
                        });
                    }
                }
            }
            tauri::RunEvent::Exit => {
                if let Some(dir) = app
                    .try_state::<AppState>()
                    .and_then(|state| state.store.ephemeral_webview_dir())
                {
                    let _ = cleanup.send(dir);
                }
            }
            _ => (),
        });
    // WebView2 may release its user-data locks shortly after its controller
    // closes. Never attempt this while the live WebView still owns the files.
    if let Ok(dir) = temporary_dirs.try_recv() {
        for _ in 0..20 {
            match std::fs::remove_dir_all(&dir) {
                Ok(()) => return,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
            }
        }
        eprintln!("Sambler: temporary WebView profile cleanup failed");
    }
}

fn setup(app: &mut tauri::App, root: DataRoot) -> Result<(), Box<dyn std::error::Error>> {
    let platform = Platform::new();
    let store = Store::new(root.clone(), platform.protector.clone());
    let foreign = store.protected_profile_unreadable();
    let root = if foreign && !root.ephemeral {
        #[cfg(windows)]
        {
            use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONWARNING, MB_YESNO};
            if message_box(
                "Les choix protégés de ce profil sont illisibles. Ouvrir pour cette session sans modifier les données existantes ?",
                MB_ICONWARNING | MB_YESNO,
            ) != IDYES.0
            {
                app.handle().exit(0);
                return Ok(());
            }
        }
        storage::ephemeral_root(root)
    } else {
        root
    };
    let store = Store::new(root.clone(), platform.protector.clone());
    let mut settings = store.load_settings();
    settings.history = if root.ephemeral {
        crate::settings::HistoryOptions::default()
    } else {
        store
            .load_history_options()
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    settings.launch_at_startup = storage::startup_enabled(&root);
    #[cfg(windows)]
    crate::platform::windows::set_insertion_method(settings.insertion);
    let consent = !foreign && store.load_consent();
    let personal_rules = Arc::new(if foreign {
        store.withheld_personal_rules()
    } else {
        store.load_personal_rules()
    });
    let history = History::spawn(
        (!root.ephemeral).then(|| root.dir.join("history.sqlite3")),
        &settings.history,
        platform.protector.clone(),
        platform.clipboard.clone(),
    );
    let (access, source, _env_error) = if foreign {
        (None, AccessSource::None, None)
    } else {
        storage::initial_access(&store)
    };

    let deps = ControllerDeps {
        microphone: platform.microphone.clone(),
        target: platform.target.clone(),
        inserter: platform.inserter.clone(),
        clipboard: platform.clipboard.clone(),
    };
    let init = ControllerInit {
        settings: settings.clone(),
        consent,
        access: access.clone(),
    };
    let controller_rules = personal_rules.clone();
    let controller_history = history.clone();
    let stats = storage::stats::Stats::load(&root);
    let controller = tauri::async_runtime::block_on(async move {
        Controller::spawn_with_profile(deps, init, controller_rules, controller_history, stats)
    });

    let mic_test = Arc::new(mic_test::MicTest::default());
    let system_events = {
        let controller = controller.clone();
        let mic_test = mic_test.clone();
        platform.system_events.subscribe(Box::new(move |event| {
            controller.system(event);
            if matches!(event, SystemEvent::SessionLocked | SystemEvent::Suspending) {
                mic_test.stop();
            }
        }))
    };

    // Integration: replace this temporary sink with the Carnet store's note creation.
    let transcriptions = Arc::new(transcribe::Transcriptions::new(Arc::new(|_, _| {
        Err("Carnet non raccordé".into())
    })));
    app.manage(AppState {
        controller: controller.clone(),
        store,
        settings: Mutex::new(settings.clone()),
        personal_rules: Mutex::new(personal_rules),
        history,
        notes: Arc::new(storage::notes::Notes::new(&root, platform.protector.clone())),
        consent: Mutex::new(consent),
        consent_write: Mutex::new(()),
        settings_write: Mutex::new(()),
        access_write: Mutex::new(()),
        rules_write: Mutex::new(()),
        exit_ready: std::sync::atomic::AtomicBool::new(false),
        access: Mutex::new(AccessState { access, source }),
        microphone: platform.microphone.clone(),
        #[cfg(windows)]
        wasapi: platform.wasapi.clone(),
        mic_test: mic_test.clone(),
        transcriptions: transcriptions.clone(),
        _system_events: Mutex::new(Some(system_events)),
    });

    // Main window, created from Rust with the profile's WebView2 folder.
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == MAIN_WINDOW)
        .cloned()
        .ok_or("missing main window config")?;
    let window = WebviewWindowBuilder::from_config(app.handle(), &config)
        .map_err(|_| "cannot configure the Sambler window")?
        .data_directory(root.webview_dir.clone())
        .build()
        .map_err(|_| "cannot create the Sambler window")?;
    {
        let window_for_close = window.clone();
        window.on_window_event(move |event| {
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    // Closing the window keeps Sambler available in the tray.
                    api.prevent_close();
                    mic_test.stop();
                    let _ = window_for_close.hide();
                }
                WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) => {
                    use tauri::Emitter;
                    for path in paths {
                        if let Some(path) = transcriptions.allow(path) {
                            let _ = window_for_close.emit_to(
                                MAIN_WINDOW,
                                crate::ipc::EVENT_FILE_DROPPED,
                                path,
                            );
                        }
                    }
                }
                _ => (),
            }
        });
    }
    let _ = window.show();
    let _ = window.set_focus();

    tray::create(app.handle())?;
    if let Err(error) = hotkey::bind_dictation(app.handle(), &settings.shortcut) {
        // Keep the app usable: the in-app button and tray still work.
        eprintln!("Sambler: dictation shortcut not registered ({error})");
    }
    relay::start(app.handle().clone(), controller);
    Ok(())
}

pub fn show_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Real Windows implementations of the ports.
struct Platform {
    microphone: Arc<dyn Microphone>,
    #[cfg(windows)]
    wasapi: Arc<crate::platform::windows::WasapiMicrophone>,
    target: Arc<dyn crate::ports::TargetProbe>,
    inserter: Arc<dyn crate::ports::Inserter>,
    clipboard: Arc<dyn crate::ports::Clipboard>,
    protector: Arc<dyn crate::ports::Protector>,
    system_events: Arc<dyn SystemEvents>,
}

impl Platform {
    #[cfg(windows)]
    fn new() -> Self {
        use crate::platform::windows as w;
        let wasapi = Arc::new(w::WasapiMicrophone::new());
        Self {
            microphone: wasapi.clone(),
            wasapi,
            target: Arc::new(w::UiaTargetProbe::new()),
            inserter: Arc::new(w::SendInputInserter::new()),
            clipboard: Arc::new(w::WinClipboard::new()),
            protector: Arc::new(w::Dpapi::new()),
            system_events: Arc::new(w::WtsSystemEvents::new()),
        }
    }

    #[cfg(not(windows))]
    fn new() -> Self {
        unimplemented!("Sambler targets Windows 11 x64")
    }
}

#[cfg(windows)]
fn message_box(
    text: &str,
    flags: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE,
) -> i32 {
    use windows::Win32::UI::WindowsAndMessaging::MessageBoxW;
    use windows::core::HSTRING;
    unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from("Sambler"), flags).0 }
}

fn ask_ephemeral_session(root: &DataRoot) -> bool {
    let _ = root;
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONWARNING, MB_YESNO};
        let text = "Le dossier de données de Sambler n'est pas accessible en écriture.\n\n\
                    Oui : ouvrir pour cette session (réglages en mémoire, historique suspendu, \
                    rien n'est enregistré).\nNon : quitter.";
        message_box(text, MB_YESNO | MB_ICONWARNING) == IDYES.0
    }
    #[cfg(not(windows))]
    false
}

fn show_fatal(text: &str) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR;
        message_box(text, MB_ICONERROR);
    }
    #[cfg(not(windows))]
    eprintln!("{text}");
}
