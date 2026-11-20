//! Real implementations of `crate::ports` for Windows 11 x64.
//! Owned by the "platform" work package.

#[cfg(windows)]
pub mod windows;

/// Open "Confidentialité et sécurité → Microphone" (`ms-settings:privacy-microphone`).
pub fn open_mic_privacy_settings() {
    #[cfg(windows)]
    windows::open_mic_privacy_settings();
}
