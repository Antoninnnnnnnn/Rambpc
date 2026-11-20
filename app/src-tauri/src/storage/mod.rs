//! Data directory (installed vs portable), settings file and protected values.
//!
//! Resolved in Rust before the first WebView (docs/windows/09, "Décider du
//! profil avant la première WebView"):
//! - `portable.flag` next to the executable → `<exe dir>\Data\` (absolute path,
//!   independent of the working directory);
//! - otherwise the installed edition → `%LOCALAPPDATA%\Sambler\`.
//!
//! The directory is checked by creating, writing and renaming a small file.
//! If that fails the app may run in ephemeral mode: readable settings are
//! loaded, nothing is written, and WebView2 uses a private temp folder.
//!
//! Ordinary settings are plain versioned JSON written atomically. Consent and
//! remembered Rambler access are protected with DPAPI (user scope). In the
//! portable edition the access is never written to disk.

#[path = "../history/mod.rs"]
pub(crate) mod history;
mod personal;
mod startup;

pub(crate) use startup::{StartupError, set_startup, startup_enabled};

use crate::ipc::{AccessInput, AccessSource, AccessStatus};
use crate::ports::Protector;
use crate::rambler::{AccessError, RamblerAccess};
use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zeroize::Zeroizing;

pub const PORTABLE_FLAG: &str = "portable.flag";
const SETTINGS_FILE: &str = "settings.json";
const CONSENT_FILE: &str = "consent.bin";
const ACCESS_FILE: &str = "access.bin";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Edition {
    Installed,
    Portable,
}

#[derive(Clone, Debug)]
pub struct DataRoot {
    pub edition: Edition,
    /// Where settings and protected files live (may be unwritable if ephemeral).
    pub dir: PathBuf,
    /// Folder handed to WebView2 for its user data.
    pub webview_dir: PathBuf,
    /// Nothing is persisted this session.
    pub ephemeral: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum RootError {
    #[error("cannot locate the executable")]
    Executable,
    #[error("cannot locate the local application data folder")]
    LocalAppData,
    #[error("data folder is not writable")]
    NotWritable { root: DataRoot },
}

/// Pick the edition and data directory. Call before creating any WebView.
pub fn resolve_root() -> Result<DataRoot, RootError> {
    let exe = std::env::current_exe().map_err(|_| RootError::Executable)?;
    let exe_dir = exe.parent().ok_or(RootError::Executable)?.to_path_buf();
    let (edition, dir) = if exe_dir.join(PORTABLE_FLAG).is_file() {
        (Edition::Portable, exe_dir.join("Data"))
    } else {
        let local = std::env::var_os("LOCALAPPDATA").ok_or(RootError::LocalAppData)?;
        (Edition::Installed, PathBuf::from(local).join("Sambler"))
    };
    let root = DataRoot {
        edition,
        webview_dir: dir.join("WebView2"),
        dir,
        ephemeral: false,
    };
    match probe_writable(&root.dir) {
        Ok(()) => Ok(root),
        Err(_) => Err(RootError::NotWritable { root }),
    }
}

/// Ephemeral variant of a root whose folder is not writable: WebView2 data goes
/// to a private, unique temp subfolder removed at exit.
pub fn ephemeral_root(mut root: DataRoot) -> DataRoot {
    let unique = format!(
        "Sambler-session-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    );
    root.webview_dir = std::env::temp_dir().join(unique);
    root.ephemeral = true;
    root
}

fn probe_writable(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let probe = dir.join(".write-probe.tmp");
    let renamed = dir.join(".write-probe.ok");
    {
        let mut file = fs::File::create(&probe)?;
        file.write_all(b"ok")?;
        file.sync_all()?;
    }
    fs::rename(&probe, &renamed)?;
    fs::remove_file(&renamed)?;
    Ok(())
}

/// Write `bytes` to `path` atomically (temp file in the same folder + rename).
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsentRecord {
    version: u32,
    rambler_dictation: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccessRecord {
    authority: String,
    api_key: String,
    android_package: String,
    android_cert: String,
}

impl Drop for AccessRecord {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.api_key.zeroize();
        self.android_package.zeroize();
        self.android_cert.zeroize();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("this session does not save changes")]
    Ephemeral,
    #[error("could not write the data folder")]
    Io,
    #[error("data protection failed")]
    Protect,
    #[error("the portable edition never saves the Rambler access")]
    PortableAccess,
}

/// Settings and protected values for one data root.
pub struct Store {
    root: DataRoot,
    protector: Arc<dyn Protector>,
}

impl Store {
    pub fn new(root: DataRoot, protector: Arc<dyn Protector>) -> Self {
        Self { root, protector }
    }

    pub fn root(&self) -> &DataRoot {
        &self.root
    }

    /// Missing or unreadable settings fall back to defaults. A corrupt file is
    /// kept aside (`settings.json.bad`) rather than overwritten silently.
    pub fn load_settings(&self) -> Settings {
        let path = self.root.dir.join(SETTINGS_FILE);
        let Ok(bytes) = fs::read(&path) else {
            return Settings::default();
        };
        match serde_json::from_slice::<Settings>(&bytes) {
            Ok(settings) if settings.validate().is_ok() => settings,
            _ => {
                if !self.root.ephemeral {
                    let _ = fs::rename(&path, path.with_extension("json.bad"));
                }
                Settings::default()
            }
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), StoreError> {
        if self.root.ephemeral {
            return Err(StoreError::Ephemeral);
        }
        let mut ordinary = settings.clone();
        // Consent to archival is profile-bound, never trusted from plain JSON.
        ordinary.history.enabled = false;
        let bytes = serde_json::to_vec_pretty(&ordinary).map_err(|_| StoreError::Io)?;
        write_atomic(&self.root.dir.join(SETTINGS_FILE), &bytes).map_err(|_| StoreError::Io)
    }

    /// `false` when absent or unreadable under this Windows account.
    pub fn load_consent(&self) -> bool {
        let Ok(blob) = fs::read(self.root.dir.join(CONSENT_FILE)) else {
            return false;
        };
        let Ok(plain) = self.protector.unprotect(&blob) else {
            return false;
        };
        serde_json::from_slice::<ConsentRecord>(&plain)
            .map(|r| r.version == 1 && r.rambler_dictation)
            .unwrap_or(false)
    }

    pub fn save_consent(&self, enabled: bool) -> Result<(), StoreError> {
        if self.root.ephemeral {
            return Err(StoreError::Ephemeral);
        }
        let record = ConsentRecord {
            version: 1,
            rambler_dictation: enabled,
        };
        let plain = Zeroizing::new(serde_json::to_vec(&record).map_err(|_| StoreError::Io)?);
        let blob = self
            .protector
            .protect(&plain)
            .map_err(|_| StoreError::Protect)?;
        write_atomic(&self.root.dir.join(CONSENT_FILE), &blob).map_err(|_| StoreError::Io)
    }

    pub fn can_remember_access(&self) -> bool {
        self.root.edition == Edition::Installed && !self.root.ephemeral
    }

    /// Remembered access (installed edition only).
    pub fn load_access(&self) -> Option<RamblerAccess> {
        if self.root.edition != Edition::Installed {
            return None;
        }
        let blob = fs::read(self.root.dir.join(ACCESS_FILE)).ok()?;
        let plain = self.protector.unprotect(&blob).ok()?;
        let record: AccessRecord = serde_json::from_slice(&plain).ok()?;
        RamblerAccess::new(
            &record.authority,
            Zeroizing::new(record.api_key.clone()),
            Zeroizing::new(record.android_package.clone()),
            Zeroizing::new(record.android_cert.clone()),
        )
        .ok()
    }

    pub fn save_access(&self, input: &AccessInput) -> Result<(), StoreError> {
        if !self.can_remember_access() {
            return Err(StoreError::PortableAccess);
        }
        let record = AccessRecord {
            authority: input.authority.trim().to_owned(),
            api_key: input.api_key.trim().to_owned(),
            android_package: input.android_package.trim().to_owned(),
            android_cert: input.android_cert.trim().to_owned(),
        };
        let plain = Zeroizing::new(serde_json::to_vec(&record).map_err(|_| StoreError::Io)?);
        let blob = self
            .protector
            .protect(&plain)
            .map_err(|_| StoreError::Protect)?;
        write_atomic(&self.root.dir.join(ACCESS_FILE), &blob).map_err(|_| StoreError::Io)
    }

    pub fn forget_access(&self) -> Result<(), StoreError> {
        match fs::remove_file(self.root.dir.join(ACCESS_FILE)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(StoreError::Io),
        }
    }

    /// Remove the private WebView2 folder of an ephemeral session.
    pub fn cleanup_ephemeral(&self) {
        if self.root.ephemeral {
            let _ = fs::remove_dir_all(&self.root.webview_dir);
        }
    }
}

/// Access chosen at startup: environment first, then remembered access.
pub fn initial_access(store: &Store) -> (Option<RamblerAccess>, AccessSource, Option<AccessError>) {
    match RamblerAccess::from_environment() {
        Ok(Some(access)) => return (Some(access), AccessSource::Environment, None),
        Ok(None) => {}
        Err(error) => return (None, AccessSource::None, Some(error)),
    }
    match store.load_access() {
        Some(access) => (Some(access), AccessSource::Saved, None),
        None => (None, AccessSource::None, None),
    }
}

pub fn access_status(
    access: Option<&RamblerAccess>,
    source: AccessSource,
    store: &Store,
) -> AccessStatus {
    AccessStatus {
        configured: access.is_some(),
        source: if access.is_some() {
            source
        } else {
            AccessSource::None
        },
        authority: access.map(|a| a.authority().to_owned()),
        can_remember: store.can_remember_access(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::ProtectError;

    /// Test-only reversible protector (XOR), never used by the app.
    struct XorProtector;
    impl Protector for XorProtector {
        fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, ProtectError> {
            Ok(plain.iter().map(|b| b ^ 0x5a).collect())
        }
        fn unprotect(&self, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectError> {
            Ok(Zeroizing::new(blob.iter().map(|b| b ^ 0x5a).collect()))
        }
    }

    fn temp_root(edition: Edition) -> DataRoot {
        let dir = std::env::temp_dir().join(format!(
            "sambler-store-test-{}-{:?}-{}",
            std::process::id(),
            edition,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        DataRoot {
            edition,
            webview_dir: dir.join("WebView2"),
            dir,
            ephemeral: false,
        }
    }

    #[test]
    fn settings_round_trip_and_corrupt_file_is_set_aside() {
        let root = temp_root(Edition::Installed);
        let store = Store::new(root.clone(), Arc::new(XorProtector));
        assert_eq!(store.load_settings(), Settings::default());
        let s = Settings {
            draft_minutes: 5,
            ..Settings::default()
        };
        store.save_settings(&s).unwrap();
        assert_eq!(store.load_settings().draft_minutes, 5);
        fs::write(root.dir.join(SETTINGS_FILE), b"{not json").unwrap();
        assert_eq!(store.load_settings(), Settings::default());
        assert!(root.dir.join("settings.json.bad").exists());
        let _ = fs::remove_dir_all(&root.dir);
    }

    #[test]
    fn consent_is_protected_and_defaults_to_false() {
        let root = temp_root(Edition::Portable);
        let store = Store::new(root.clone(), Arc::new(XorProtector));
        assert!(!store.load_consent());
        store.save_consent(true).unwrap();
        let raw = fs::read(root.dir.join(CONSENT_FILE)).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("ramblerDictation"));
        assert!(store.load_consent());
        let _ = fs::remove_dir_all(&root.dir);
    }

    #[test]
    fn portable_edition_never_writes_access() {
        let root = temp_root(Edition::Portable);
        let store = Store::new(root.clone(), Arc::new(XorProtector));
        let input = AccessInput {
            authority: "example.invalid:443".into(),
            api_key: "k".into(),
            android_package: "p".into(),
            android_cert: "c".into(),
            remember: true,
        };
        assert!(matches!(
            store.save_access(&input),
            Err(StoreError::PortableAccess)
        ));
        assert!(!root.dir.join(ACCESS_FILE).exists());
        let _ = fs::remove_dir_all(&root.dir);
    }

    #[test]
    fn rules_are_protected_conflicts_never_write_and_explicit_save_replaces_unreadable() {
        use crate::{ipc::PersonalRulesInput, text::LexiconEntry};
        let root = temp_root(Edition::Portable);
        let store = Store::new(root.clone(), Arc::new(XorProtector));
        let path = root.dir.join("personal-rules.bin");
        let mut rules = PersonalRulesInput {
            lexicon: vec![LexiconEntry {
                id: "one".into(),
                heard: "melusine".into(),
                written: "Mélusine privée".into(),
                enabled: true,
                apply_in_faithful: true,
            }],
            expressions: vec![],
        };
        assert!(store.save_personal_rules(&rules).unwrap().is_empty());
        let sealed = fs::read(&path).unwrap();
        assert!(
            !sealed
                .windows("Mélusine privée".len())
                .any(|w| w == "Mélusine privée".as_bytes())
        );
        assert_eq!(store.load_personal_rules().lexicon, rules.lexicon);
        let withheld = store.withheld_personal_rules();
        assert!(withheld.unreadable && withheld.lexicon.is_empty());
        let duplicate = LexiconEntry {
            id: "two".into(),
            written: "Other".into(),
            ..rules.lexicon[0].clone()
        };
        rules.lexicon.push(duplicate);
        assert!(!store.save_personal_rules(&rules).unwrap().is_empty());
        assert_eq!(fs::read(&path).unwrap(), sealed);
        fs::write(&path, b"unreadable profile").unwrap();
        assert!(store.load_personal_rules().unreadable);
        assert_eq!(fs::read(&path).unwrap(), b"unreadable profile");
        rules.lexicon.pop();
        let mut ephemeral = root.clone();
        ephemeral.ephemeral = true;
        assert!(
            Store::new(ephemeral, Arc::new(XorProtector))
                .save_personal_rules(&rules)
                .unwrap()
                .is_empty()
        );
        assert_eq!(fs::read(&path).unwrap(), b"unreadable profile");
        assert!(store.save_personal_rules(&rules).unwrap().is_empty());
        assert!(!store.load_personal_rules().unreadable);
        assert!(matches!(
            set_startup(&root, true),
            Err(StartupError::Portable)
        ));
        let _ = fs::remove_dir_all(&root.dir);
    }

    #[test]
    fn history_opt_in_is_profile_bound_and_invalid_policy_is_preserved() {
        let root = temp_root(Edition::Portable);
        let store = Store::new(root.clone(), Arc::new(XorProtector));
        let settings = Settings {
            history: crate::settings::HistoryOptions {
                enabled: true,
                retention_days: 30,
            },
            ..Settings::default()
        };
        store.save_settings(&settings).unwrap();
        assert!(!store.load_settings().history.enabled);
        assert!(store.load_history_options().unwrap().is_none());
        store.save_history_options(&settings.history).unwrap();
        assert_eq!(
            store.load_history_options().unwrap(),
            Some(settings.history)
        );
        assert!(!store.protected_profile_unreadable());
        let path = root.dir.join("history-policy.bin");
        fs::write(&path, b"foreign profile").unwrap();
        assert!(store.protected_profile_unreadable());
        assert_eq!(fs::read(&path).unwrap(), b"foreign profile");
        let _ = fs::remove_dir_all(&root.dir);
    }

    #[test]
    fn installed_access_round_trip() {
        let root = temp_root(Edition::Installed);
        let store = Store::new(root.clone(), Arc::new(XorProtector));
        let input = AccessInput {
            authority: "example.invalid:443".into(),
            api_key: "invented-key".into(),
            android_package: "invented.package".into(),
            android_cert: "invented-cert".into(),
            remember: true,
        };
        store.save_access(&input).unwrap();
        let access = store.load_access().expect("access");
        assert_eq!(access.authority(), "example.invalid:443");
        store.forget_access().unwrap();
        assert!(store.load_access().is_none());
        let _ = fs::remove_dir_all(&root.dir);
    }
}
