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
mod relay;
mod tray;

use crate::controller::{Controller, ControllerDeps, ControllerHandle, ControllerInit};
use crate::ipc::AccessSource;
use crate::ports::{MicStream, Microphone, SystemEvents};
use crate::rambler::RamblerAccess;
use crate::settings::Settings;
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
    pub consent: Mutex<bool>,
    pub access: Mutex<AccessState>,
    pub microphone: Arc<dyn Microphone>,
    pub mic_test: Mutex<Option<Box<dyn MicStream>>>,
    /// Keeps the lock/suspend subscription alive.
    pub _system_events: Mutex<Option<crate::ports::Subscription>>,
}

pub fn run() {
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
            commands::copy_draft,
            commands::clear_draft,
            commands::open_mic_privacy_settings,
            commands::preview_render,
        ])
        .setup(move |app| {
            setup(app, root.clone())?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Sambler")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                if let Some(state) = app.try_state::<AppState>() {
                    let controller = state.controller.clone();
                    tauri::async_runtime::block_on(controller.shutdown());
                    state.store.cleanup_ephemeral();
                }
            }
        });
}

fn setup(app: &mut tauri::App, root: DataRoot) -> Result<(), Box<dyn std::error::Error>> {
    let platform = Platform::new();
    let store = Store::new(root.clone(), platform.protector.clone());
    let settings = store.load_settings();
    let consent = store.load_consent();
    let (access, source, _env_error) = storage::initial_access(&store);

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
    let controller = tauri::async_runtime::block_on(async move { Controller::spawn(deps, init) });

    let system_events = {
        let controller = controller.clone();
        platform
            .system_events
            .subscribe(Box::new(move |event| controller.system(event)))
    };

    app.manage(AppState {
        controller: controller.clone(),
        store,
        settings: Mutex::new(settings.clone()),
        consent: Mutex::new(consent),
        access: Mutex::new(AccessState { access, source }),
        microphone: platform.microphone.clone(),
        mic_test: Mutex::new(None),
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
    let window = WebviewWindowBuilder::from_config(app.handle(), &config)?
        .data_directory(root.webview_dir.clone())
        .build()?;
    {
        let window_for_close = window.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Closing the window keeps Sambler available in the tray.
                api.prevent_close();
                let _ = window_for_close.hide();
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
        Self {
            microphone: Arc::new(w::WasapiMicrophone::new()),
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
fn message_box(text: &str, flags: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE) -> i32 {
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
