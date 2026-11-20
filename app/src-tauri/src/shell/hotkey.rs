//! Global shortcuts: the configurable dictation shortcut (both edges forwarded
//! to the controller) and Escape, registered only while a take is active.

use super::AppState;
use crate::model::KeyEdge;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const ESCAPE: &str = "Escape";
static ESCAPE_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Accelerators refused by policy (docs/windows/05): Windows-key combos, F12,
/// bare modifiers and Ctrl+Alt (AltGr on AZERTY).
pub fn check_policy(accelerator: &str) -> Result<Shortcut, String> {
    let shortcut: Shortcut = accelerator
        .parse()
        .map_err(|_| "raccourci illisible".to_string())?;
    let upper = accelerator.to_ascii_uppercase();
    let parts: Vec<&str> = upper.split('+').map(str::trim).collect();
    let has = |names: &[&str]| parts.iter().any(|p| names.contains(p));
    if has(&["SUPER", "WIN", "META", "CMD", "COMMAND"]) {
        return Err("les combinaisons avec la touche Windows sont réservées".into());
    }
    if has(&["CTRL", "CONTROL"]) && has(&["ALT", "OPTION"]) {
        return Err("Ctrl+Alt sert à AltGr sur certains claviers".into());
    }
    if parts.last().is_some_and(|k| *k == "F12") {
        return Err("F12 est réservée au débogage par Windows".into());
    }
    Ok(shortcut)
}

pub fn bind_dictation<R: Runtime>(app: &AppHandle<R>, accelerator: &str) -> Result<(), String> {
    let shortcut = check_policy(accelerator)?;
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _shortcut, event| {
            let edge = match event.state {
                ShortcutState::Pressed => KeyEdge::Pressed,
                ShortcutState::Released => KeyEdge::Released,
            };
            if let Some(state) = app.try_state::<AppState>() {
                state.controller.hotkey(edge);
            }
        })
        .map_err(|e| e.to_string())
}

/// Replace the dictation shortcut. On failure the previous one is restored.
pub fn rebind_dictation<R: Runtime>(app: &AppHandle<R>, old: &str, new: &str) -> Result<(), String> {
    if old.eq_ignore_ascii_case(new) {
        return Ok(());
    }
    check_policy(new)?;
    if let Ok(old_shortcut) = old.parse::<Shortcut>() {
        let _ = app.global_shortcut().unregister(old_shortcut);
    }
    match bind_dictation(app, new) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = bind_dictation(app, old);
            Err(error)
        }
    }
}

/// Register Escape as "cancel the current take" only while a take is active.
pub fn set_escape_active<R: Runtime>(app: &AppHandle<R>, active: bool) {
    let shortcuts = app.global_shortcut();
    if active && !ESCAPE_REGISTERED.load(Ordering::SeqCst) {
        let ok = shortcuts
            .on_shortcut(ESCAPE, |app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    if let Some(state) = app.try_state::<AppState>() {
                        state.controller.cancel(None);
                    }
                }
            })
            .is_ok();
        ESCAPE_REGISTERED.store(ok, Ordering::SeqCst);
    } else if !active && ESCAPE_REGISTERED.swap(false, Ordering::SeqCst) {
        let _ = shortcuts.unregister(ESCAPE);
    }
}

#[cfg(test)]
mod tests {
    use super::check_policy;

    #[test]
    fn default_shortcut_is_accepted() {
        assert!(check_policy("Ctrl+Shift+F2").is_ok());
    }

    #[test]
    fn reserved_combinations_are_refused() {
        assert!(check_policy("Super+H").is_err());
        assert!(check_policy("Ctrl+Alt+D").is_err());
        assert!(check_policy("Shift+F12").is_err());
        assert!(check_policy("Ctrl+").is_err());
    }
}
