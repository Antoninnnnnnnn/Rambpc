//! Installed-edition autostart. The Run value is a quoted absolute executable
//! path, never a shell command or a value supplied by the WebView.

use super::{DataRoot, Edition};

#[derive(Debug)]
pub(crate) enum StartupError {
    Portable,
    Ephemeral,
    Registry,
}

pub(crate) fn set_startup(root: &DataRoot, enabled: bool) -> Result<(), StartupError> {
    if root.edition == Edition::Portable {
        return Err(StartupError::Portable);
    }
    if root.ephemeral {
        return Err(StartupError::Ephemeral);
    }
    native::set(enabled).map_err(|()| StartupError::Registry)
}

pub(crate) fn startup_enabled(root: &DataRoot) -> bool {
    root.edition == Edition::Installed && !root.ephemeral && native::enabled()
}

#[cfg(windows)]
mod native {
    use std::{os::windows::ffi::OsStrExt, slice};
    use windows::{
        Win32::{
            Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
            System::Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
                REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW,
                RegOpenKeyExW, RegSetValueExW,
            },
        },
        core::w,
    };

    const RUN: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    const NAME: windows::core::PCWSTR = w!("Sambler");

    fn command() -> Result<Vec<u16>, ()> {
        let exe = std::env::current_exe().map_err(|_| ())?;
        if !exe.is_absolute() {
            return Err(());
        }
        let path = exe.as_os_str().encode_wide();
        let cmd: Vec<_> = std::iter::once(b'"' as u16)
            .chain(path)
            .chain([b'"' as u16, 0])
            .collect();
        // Windows documents a 260-character Run command line limit.
        (cmd.len() <= 260).then_some(cmd).ok_or(())
    }

    fn as_bytes(value: &[u16]) -> &[u8] {
        // Windows is little-endian. The slice borrows initialized UTF-16 data.
        unsafe { slice::from_raw_parts(value.as_ptr().cast(), std::mem::size_of_val(value)) }
    }

    pub(super) fn set(enabled: bool) -> Result<(), ()> {
        let value = if enabled { Some(command()?) } else { None };
        let mut key = HKEY::default();
        let opened = unsafe {
            if enabled {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    RUN,
                    None,
                    None,
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE,
                    None,
                    &mut key,
                    None,
                )
            } else {
                RegOpenKeyExW(HKEY_CURRENT_USER, RUN, None, KEY_SET_VALUE, &mut key)
            }
        };
        if opened == ERROR_FILE_NOT_FOUND && !enabled {
            return Ok(());
        }
        if opened != ERROR_SUCCESS {
            return Err(());
        }
        let result = unsafe {
            let result = if let Some(value) = value {
                RegSetValueExW(key, NAME, None, REG_SZ, Some(as_bytes(&value)))
            } else {
                RegDeleteValueW(key, NAME)
            };
            let _ = RegCloseKey(key);
            result
        };
        if result == ERROR_SUCCESS || (!enabled && result == ERROR_FILE_NOT_FOUND) {
            Ok(())
        } else {
            Err(())
        }
    }

    pub(super) fn enabled() -> bool {
        let Ok(expected) = command() else {
            return false;
        };
        let expected = as_bytes(&expected);
        let mut stored = vec![0_u8; expected.len()];
        let mut len = stored.len() as u32;
        let mut key = HKEY::default();
        let opened =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN, None, KEY_QUERY_VALUE, &mut key) };
        if opened != ERROR_SUCCESS {
            return false;
        }
        let read = unsafe {
            let result = RegGetValueW(
                key,
                None,
                NAME,
                RRF_RT_REG_SZ,
                None,
                Some(stored.as_mut_ptr().cast()),
                Some(&mut len),
            );
            let _ = RegCloseKey(key);
            result
        };
        read == ERROR_SUCCESS && len as usize == expected.len() && stored == expected
    }
}

#[cfg(not(windows))]
mod native {
    pub(super) fn set(_: bool) -> Result<(), ()> {
        Err(())
    }
    pub(super) fn enabled() -> bool {
        false
    }
}
