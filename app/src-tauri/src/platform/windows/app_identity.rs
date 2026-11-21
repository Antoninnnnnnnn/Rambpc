//! Local executable metadata. Full paths stay in a bounded, in-memory cache.

use crate::settings::AppIdentity;
use std::{
    collections::HashMap,
    ffi::c_void,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex, OnceLock, mpsc},
    time::Duration,
};

pub(super) fn identify(path: &Path) -> Option<AppIdentity> {
    identity(path, description(path))
}

fn identity(path: &Path, description: Option<String>) -> Option<AppIdentity> {
    let id = path.file_stem()?.to_string_lossy().to_lowercase();
    let name = description
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            let known = match id.as_str() {
                "winword" => "Word",
                "outlook" | "olk" => "Outlook",
                "brave" => "Brave",
                "chrome" => "Chrome",
                "msedge" => "Edge",
                "firefox" => "Firefox",
                "discord" => "Discord",
                "slack" => "Slack",
                "code" => "Visual Studio Code",
                "notepad" => "Bloc-notes",
                "windowsterminal" => "Windows Terminal",
                "teams" | "ms-teams" => "Teams",
                "whatsapp" => "WhatsApp",
                "notion" => "Notion",
                "obsidian" => "Obsidian",
                _ => {
                    return path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                }
            };
            known.to_owned()
        });
    Some(AppIdentity { id, name })
}

type Lookup = (PathBuf, mpsc::SyncSender<Option<String>>);
type Descriptions = HashMap<PathBuf, Option<String>>;
static CACHE: LazyLock<Mutex<Descriptions>> = LazyLock::new(Mutex::default);
static LOOKUPS: OnceLock<Option<mpsc::SyncSender<Lookup>>> = OnceLock::new();

fn cached(path: &Path) -> Option<Option<String>> {
    CACHE.lock().ok()?.get(path).cloned()
}

fn description(path: &Path) -> Option<String> {
    if let Some(value) = cached(path) {
        return value;
    }
    // Reading version resources can hit a slow filesystem. Keep that work off
    // the UIA apartment and bound both the wait and the number of queued reads.
    let requests = LOOKUPS.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Lookup>(1);
        std::thread::Builder::new()
            .name("sambler-app-descriptions".into())
            .spawn(move || {
                while let Ok((path, reply)) = rx.recv() {
                    let value = cached(&path).unwrap_or_else(|| {
                        let value = file_description(&path);
                        if let Ok(mut cache) = CACHE.lock() {
                            if cache.len() >= 256 {
                                cache.clear();
                            }
                            cache.insert(path, value.clone());
                        }
                        value
                    });
                    let _ = reply.try_send(value);
                }
            })
            .ok()?;
        Some(tx)
    });
    let (reply, value) = mpsc::sync_channel(1);
    requests.as_ref()?.try_send((path.to_owned(), reply)).ok()?;
    value.recv_timeout(Duration::from_millis(20)).ok().flatten()
}

// Import the three version.dll functions without widening the Windows crate's
// enabled feature set. BOOL is i32 and DWORD/UINT are u32 on Windows.
#[link(name = "version")]
unsafe extern "system" {
    fn GetFileVersionInfoSizeW(path: *const u16, handle: *mut u32) -> u32;
    fn GetFileVersionInfoW(path: *const u16, handle: u32, len: u32, data: *mut c_void) -> i32;
    fn VerQueryValueW(
        data: *const c_void,
        key: *const u16,
        value: *mut *mut c_void,
        len: *mut u32,
    ) -> i32;
}

fn wide_z(text: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain(Some(0)).collect()
}

fn file_description(path: &Path) -> Option<String> {
    let path = wide_z(path);
    let mut ignored = 0;
    // SAFETY: NUL-terminated path and writable DWORD.
    let len = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &mut ignored) };
    if !(1..=1024 * 1024).contains(&len) {
        return None;
    }
    let mut data = vec![0_u32; (len as usize).div_ceil(4)];
    // SAFETY: the aligned writable buffer covers at least len bytes.
    if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, len, data.as_mut_ptr().cast()) } == 0 {
        return None;
    }
    let (pairs, _) = query(&data, r"\VarFileInfo\Translation", true)
        .unwrap_or_default()
        .as_chunks::<2>();
    let pairs = pairs.iter().map(|&[lang, cp]| (lang, cp));
    // Some files omit the translation table but have a conventional English entry.
    pairs.chain([(0x0409, 1200), (0x0409, 1252)]).find_map(|(lang, cp)| {
        let key = format!(r"\StringFileInfo\{lang:04x}{cp:04x}\FileDescription");
        let value = query(&data, &key, false)?;
        let end = value
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(value.len());
        let name = String::from_utf16_lossy(&value[..end]);
        let name = name.trim();
        (!name.is_empty()).then(|| name.to_owned())
    })
}

// Translation lengths are in bytes; string lengths are in UTF-16 code units.
fn query<'a>(data: &'a [u32], key: &str, length_in_bytes: bool) -> Option<&'a [u16]> {
    let key = wide_z(key);
    let start = data.as_ptr().cast::<u8>();
    let mut value = std::ptr::null_mut();
    let mut len = 0;
    // SAFETY: data came from GetFileVersionInfoW, key is NUL-terminated, and
    // the outputs are writable; Windows returns a pointer into data.
    if unsafe { VerQueryValueW(start.cast(), key.as_ptr(), &mut value, &mut len) } == 0 {
        return None;
    }
    let offset = value.addr().checked_sub(start.addr())?;
    let mut len = len as usize;
    if length_in_bytes {
        len /= 2;
    }
    let size = len.checked_mul(2)?;
    if len == 0
        || offset.checked_add(size)? > std::mem::size_of_val(data)
        || !value.cast::<u16>().is_aligned()
    {
        return None;
    }
    // SAFETY: the range and alignment were checked; the slice borrows the buffer.
    Some(unsafe { std::slice::from_raw_parts(start.add(offset).cast(), len) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_windows_file_description_without_launching_it() {
        let root = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
        assert!(file_description(&root.join(r"System32\kernel32.dll")).is_some());
    }

    #[test]
    fn exe_stem_is_the_id_and_description_has_priority_over_fallbacks() {
        for (exe, id, name) in [
            ("WINWORD.EXE", "winword", "Word"),
            ("Outlook.exe", "outlook", "Outlook"),
            ("olk.exe", "olk", "Outlook"),
            ("brave.exe", "brave", "Brave"),
            ("chrome.exe", "chrome", "Chrome"),
            ("msedge.exe", "msedge", "Edge"),
            ("firefox.exe", "firefox", "Firefox"),
            ("Discord.exe", "discord", "Discord"),
            ("Slack.exe", "slack", "Slack"),
            ("Code.exe", "code", "Visual Studio Code"),
            ("notepad.exe", "notepad", "Bloc-notes"),
            ("WindowsTerminal.exe", "windowsterminal", "Windows Terminal"),
            ("Teams.exe", "teams", "Teams"),
            ("ms-teams.exe", "ms-teams", "Teams"),
            ("WhatsApp.exe", "whatsapp", "WhatsApp"),
            ("Notion.exe", "notion", "Notion"),
            ("Obsidian.exe", "obsidian", "Obsidian"),
            ("Éditeur.Pro.exe", "éditeur.pro", "Éditeur.Pro.exe"),
        ] {
            let path = Path::new(r"C:\Private\Folder").join(exe);
            assert_eq!(identity(&path, Some("  ".into())), Some(AppIdentity {
                id: id.into(),
                name: name.into(),
            }));
        }
        let word = identity(Path::new("WINWORD.EXE"), Some("Microsoft Word".into())).unwrap();
        assert_eq!(word.id, "winword");
        assert_eq!(word.name, "Microsoft Word");
    }
}
