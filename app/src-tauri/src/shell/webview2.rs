//! WebView2 Evergreen presence check, before any WebView is created
//! (docs/windows/09: the portable edition installs nothing and must explain
//! the prerequisite natively). Detection follows Microsoft's documented
//! registry keys for the Evergreen Runtime.

#[cfg(windows)]
pub fn runtime_present() -> bool {
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW,
    };
    use windows::core::{HSTRING, PCWSTR};

    const CLIENT: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    let keys: [(HKEY, String); 3] = [
        (
            HKEY_LOCAL_MACHINE,
            format!(r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{CLIENT}"),
        ),
        (
            HKEY_LOCAL_MACHINE,
            format!(r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{CLIENT}"),
        ),
        (
            HKEY_CURRENT_USER,
            format!(r"Software\Microsoft\EdgeUpdate\Clients\{CLIENT}"),
        ),
    ];
    keys.iter().any(|(root, path)| {
        let mut buffer = [0u16; 64];
        let mut size = (buffer.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(
                *root,
                &HSTRING::from(path.as_str()),
                &HSTRING::from("pv"),
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if status.is_err() {
            return false;
        }
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        let version = String::from_utf16_lossy(&buffer[..len]);
        let _ = PCWSTR::null();
        !version.is_empty() && version != "0.0.0.0"
    })
}

#[cfg(not(windows))]
pub fn runtime_present() -> bool {
    true
}

pub const MISSING_MESSAGE: &str = "Sambler a besoin de Microsoft Edge WebView2 Runtime, \
normalement inclus dans Windows 11, pour afficher sa fenêtre.\n\n\
Installez « WebView2 Runtime (Evergreen) » depuis le site officiel de Microsoft \
(developer.microsoft.com/microsoft-edge/webview2), puis relancez Sambler.\n\n\
L'édition portable n'installe rien sur ce PC.";
