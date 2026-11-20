//! Native Windows implementations of the dictation controller ports.

mod cancel_key;
mod clipboard;
mod dpapi;
mod events;
mod insertion;
mod microphone;
mod office;
mod pcm;
mod target;

pub use cancel_key::{CancelKeyHook, CancelKeyHookGuard};
pub use clipboard::WinClipboard;
pub use dpapi::Dpapi;
pub use events::WtsSystemEvents;
pub use insertion::SendInputInserter;
pub use microphone::{CaptureFormat, WasapiMicrophone};
pub use target::{UiaTargetProbe, is_late};
pub(crate) use target::{TriggerFocus, bind_protection, bind_write_permission, revalidate_after_install};

use windows::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
use windows::core::{PCWSTR, w};

pub fn open_mic_privacy_settings() {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            w!("ms-settings:privacy-microphone"),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

static INSERTION_METHOD: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Process-wide choice for fields without an atomic editor API; set by the
/// shell at startup and whenever the settings change.
pub fn set_insertion_method(method: crate::settings::InsertionMethod) {
    use crate::settings::InsertionMethod::*;
    let code = match method {
        Paste => 0,
        Typing => 1,
    };
    INSERTION_METHOD.store(code, std::sync::atomic::Ordering::Relaxed);
}

pub fn insertion_method() -> crate::settings::InsertionMethod {
    match INSERTION_METHOD.load(std::sync::atomic::Ordering::Relaxed) {
        1 => crate::settings::InsertionMethod::Typing,
        _ => crate::settings::InsertionMethod::Paste,
    }
}
