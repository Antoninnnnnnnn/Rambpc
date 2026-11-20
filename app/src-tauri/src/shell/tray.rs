//! Notification-area icon and menu: open, dictate into Sambler, cancel, quit.

use super::{AppState, show_main_window};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

pub fn create<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Ouvrir Sambler", true, None::<&str>)?;
    let dictate = MenuItem::with_id(app, "dictate", "Dicter dans Sambler", true, None::<&str>)?;
    let cancel = MenuItem::with_id(app, "cancel", "Annuler la dictée", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quitter Sambler", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &dictate, &cancel, &separator, &quit])?;

    let mut builder = TrayIconBuilder::with_id("sambler")
        .tooltip("Sambler")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "dictate" => {
                show_main_window(app);
                if let Some(state) = app.try_state::<AppState>() {
                    state.controller.dictate_in_sambler();
                }
            }
            "cancel" => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.controller.cancel(None);
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
