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
pub use target::UiaTargetProbe;

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
