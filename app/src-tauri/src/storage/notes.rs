//! The Carnet is independent of dictation history and only changes via its
//! commands. A persistent notebook uses one versioned, user-scope protected
//! snapshot. A session that cannot persist starts with a separate empty book.

use super::{DataRoot, write_atomic};
use crate::{
    ipc::{Note, NoteInput, NoteMeta},
    ports::Protector,
};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    fs, io,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use unicode_segmentation::UnicodeSegmentation;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

const NOTES_FILE: &str = "notes.bin";
const VERSION: u32 = 1;
const MAX_NOTES: usize = 1_000;
const MAX_CHARS: usize = 200_000;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum NotesError {
    #[error("the notebook could not be read")]
    Unreadable,
    #[error("the notebook could not be saved")]
    SaveFailed,
    #[error("note not found")]
    NotFound,
    #[error("note is too long")]
    TooLong,
    #[error("notebook is full")]
    Full,
    #[error("notebook is unavailable")]
    Unavailable,
}

#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    /// Most recent save first, including saves in the same millisecond.
    notes: Vec<StoredNote>,
}

impl Default for Record {
    fn default() -> Self {
        Self {
            version: VERSION,
            notes: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct StoredNote {
    id: String,
    created_at: u64,
    updated_at: u64,
    text: String,
}

impl Drop for StoredNote {
    fn drop(&mut self) {
        self.text.zeroize();
    }
}

impl StoredNote {
    fn meta(&self) -> NoteMeta {
        NoteMeta {
            id: self.id.clone(),
            title: self
                .text
                .split(['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}'])
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or_default()
                .chars()
                .take(80)
                .collect(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            words: self.text.unicode_words().count() as u64,
        }
    }
}

struct ProtectedFile {
    path: PathBuf,
    protector: Arc<dyn Protector>,
}

impl ProtectedFile {
    fn read(&self) -> Result<Record, NotesError> {
        let blob = match fs::read(&self.path) {
            Ok(blob) => blob,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Record::default()),
            Err(_) => return Err(NotesError::Unreadable),
        };
        let plain = self
            .protector
            .unprotect(&blob)
            .map_err(|_| NotesError::Unreadable)?;
        let mut record: Record =
            serde_json::from_slice(&plain).map_err(|_| NotesError::Unreadable)?;
        if record.version != VERSION || record.notes.len() > MAX_NOTES {
            return Err(NotesError::Unreadable);
        }
        record.notes.sort_by_key(|note| Reverse(note.updated_at));
        Ok(record)
    }

    fn write(&self, record: &Record) -> Result<(), NotesError> {
        let plain = Zeroizing::new(serde_json::to_vec(record).map_err(|_| NotesError::SaveFailed)?);
        let blob = self
            .protector
            .protect(&plain)
            .map_err(|_| NotesError::SaveFailed)?;
        write_atomic(&self.path, &blob).map_err(|_| NotesError::SaveFailed)
    }
}

pub(crate) struct Notes {
    file: Option<ProtectedFile>,
    /// Holds the notebook in ephemeral mode; in persistent mode only used to
    /// serialize read/modify/replace across concurrent commands.
    session: Mutex<Record>,
}

impl Notes {
    pub(crate) fn new(root: &DataRoot, protector: Arc<dyn Protector>) -> Self {
        Self {
            file: (!root.ephemeral).then(|| ProtectedFile {
                path: root.dir.join(NOTES_FILE),
                protector,
            }),
            session: Mutex::new(Record::default()),
        }
    }

    /// Load before *every* operation so an unreadable or foreign-account file
    /// is not mistaken for an empty book and can never be overwritten by us.
    fn operate<T>(
        &self,
        write: bool,
        work: impl FnOnce(&mut Record) -> Result<T, NotesError>,
    ) -> Result<T, NotesError> {
        let mut session = self.session.lock().map_err(|_| NotesError::Unavailable)?;
        let Some(file) = &self.file else {
            return work(&mut session);
        };
        let mut record = file.read()?;
        let value = work(&mut record)?;
        if write {
            file.write(&record)?;
        }
        Ok(value)
    }

    pub(crate) fn list(&self) -> Result<Vec<NoteMeta>, NotesError> {
        self.operate(false, |record| {
            Ok(record.notes.iter().map(StoredNote::meta).collect())
        })
    }

    pub(crate) fn get(&self, id: &str) -> Result<Note, NotesError> {
        self.operate(false, |record| {
            let note = record
                .notes
                .iter()
                .find(|note| note.id == id)
                .ok_or(NotesError::NotFound)?;
            Ok(Note {
                meta: note.meta(),
                text: note.text.clone(),
            })
        })
    }

    pub(crate) fn save(&self, input: NoteInput) -> Result<NoteMeta, NotesError> {
        self.operate(true, |record| {
            if input.text.chars().nth(MAX_CHARS).is_some() {
                return Err(NotesError::TooLong);
            }
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            // A backward clock change must not reorder recent saves.
            let now = record
                .notes
                .first()
                .map_or(now, |latest| now.max(latest.updated_at));
            let (id, created_at) = if let Some(id) = input.id {
                let index = record
                    .notes
                    .iter()
                    .position(|note| note.id == id)
                    .ok_or(NotesError::NotFound)?;
                (id, record.notes.remove(index).created_at)
            } else {
                if record.notes.len() >= MAX_NOTES {
                    return Err(NotesError::Full);
                }
                let id = loop {
                    let id = Uuid::new_v4().to_string();
                    if !record.notes.iter().any(|note| note.id == id) {
                        break id;
                    }
                };
                (id, now)
            };
            let note = StoredNote {
                id,
                created_at,
                updated_at: now,
                text: input.text,
            };
            let meta = note.meta();
            record.notes.insert(0, note);
            Ok(meta)
        })
    }

    pub(crate) fn delete(&self, id: &str) -> Result<(), NotesError> {
        self.operate(true, |record| {
            let index = record
                .notes
                .iter()
                .position(|note| note.id == id)
                .ok_or(NotesError::NotFound)?;
            record.notes.remove(index);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ports::ProtectError, storage::Edition};

    // Prefix identifies the simulated account and XOR lets us verify the
    // stored payload goes through the protection port, not directly to disk.
    struct Account(u8);
    impl Protector for Account {
        fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, ProtectError> {
            Ok(std::iter::once(self.0)
                .chain(plain.iter().map(|byte| byte ^ self.0))
                .collect())
        }
        fn unprotect(&self, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectError> {
            match blob.split_first() {
                Some((&account, ciphertext)) if account == self.0 => Ok(Zeroizing::new(
                    ciphertext.iter().map(|byte| byte ^ self.0).collect(),
                )),
                _ => Err(ProtectError::Unreadable),
            }
        }
    }

    struct Root(DataRoot);
    impl Root {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("sambler-notes-test-{}", Uuid::new_v4()));
            Self(DataRoot {
                edition: Edition::Portable,
                webview_dir: dir.join("WebView2"),
                dir,
                ephemeral: false,
            })
        }
        fn path(&self) -> PathBuf {
            self.0.dir.join(NOTES_FILE)
        }
        fn book(&self, account: u8) -> Notes {
            Notes::new(&self.0, Arc::new(Account(account)))
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0.dir);
        }
    }
    fn input(id: Option<&str>, text: &str) -> NoteInput {
        NoteInput {
            id: id.map(str::to_owned),
            text: text.into(),
        }
    }

    #[test]
    fn protected_round_trip_update_order_and_delete() {
        let root = Root::new();
        let book = root.book(0x5a);
        assert!(book.list().unwrap().is_empty());
        let first = book
            .save(input(None, "\r\n \t\r\n  titre privé  \r\nSuite."))
            .unwrap();
        assert_eq!(first.title, "titre privé");
        assert_eq!(first.words, 3);
        assert_eq!(first.created_at, first.updated_at);
        assert!(Uuid::parse_str(&first.id).is_ok_and(|uuid| uuid.get_version_num() == 4));
        let raw = fs::read(root.path()).unwrap();
        assert!(
            !raw.windows("titre privé".len())
                .any(|w| w == "titre privé".as_bytes())
        );
        let second = book.save(input(None, "Autre note.")).unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(book.list().unwrap(), [second.clone(), first.clone()]);
        let reopened = root.book(0x5a);
        assert_eq!(
            reopened.get(&first.id).unwrap().text,
            "\r\n \t\r\n  titre privé  \r\nSuite."
        );
        let updated = reopened
            .save(input(Some(&first.id), "nouveau titre\n2 mots"))
            .unwrap();
        assert_eq!(updated.created_at, first.created_at);
        assert!(updated.updated_at >= second.updated_at);
        assert_eq!(updated.title, "nouveau titre");
        assert_eq!(updated.words, 4);
        assert_eq!(book.list().unwrap(), [updated, second.clone()]);
        book.delete(&first.id).unwrap();
        assert_eq!(reopened.list().unwrap(), [second]);
        assert_eq!(reopened.get(&first.id), Err(NotesError::NotFound));
        assert_eq!(
            book.save(input(Some(&first.id), "Non")),
            Err(NotesError::NotFound)
        );
        assert_eq!(book.delete(&first.id), Err(NotesError::NotFound));
    }

    #[test]
    fn unicode_character_limits_and_title() {
        let root = Root::new();
        let book = root.book(0x5a);
        let text = "é".repeat(MAX_CHARS);
        let allowed = book.save(input(None, &text)).unwrap();
        assert_eq!(allowed.title, "é".repeat(80));
        assert_eq!(
            book.save(input(Some(&allowed.id), &(text.clone() + "é"))),
            Err(NotesError::TooLong)
        );
        assert_eq!(book.get(&allowed.id).unwrap().text, text);
        let blank = book.save(input(None, " \r\n\t")).unwrap();
        assert!(blank.title.is_empty());
        assert_eq!(blank.words, 0);
    }

    #[test]
    fn reaching_capacity_only_blocks_new_notes() {
        let mut root = Root::new();
        root.0.ephemeral = true;
        let book = root.book(0x5a);
        let first = book.save(input(None, "première")).unwrap();
        for _ in 1..MAX_NOTES {
            book.save(input(None, "")).unwrap();
        }
        assert_eq!(book.list().unwrap().len(), MAX_NOTES);
        assert_eq!(book.save(input(None, "en trop")), Err(NotesError::Full));
        assert!(book.save(input(Some(&first.id), "mise à jour")).is_ok());
        book.delete(&first.id).unwrap();
        assert!(book.save(input(None, "maintenant oui")).is_ok());
    }

    #[test]
    fn foreign_and_corrupt_files_are_preserved() {
        let root = Root::new();
        let original = root.book(0x5a);
        let note = original.save(input(None, "à conserver")).unwrap();
        let path = root.path();
        let raw = fs::read(&path).unwrap();
        let foreign = root.book(0x31);
        assert_eq!(foreign.list(), Err(NotesError::Unreadable));
        assert_eq!(foreign.get(&note.id), Err(NotesError::Unreadable));
        assert_eq!(
            foreign.save(input(None, "ne pas écraser")),
            Err(NotesError::Unreadable)
        );
        assert_eq!(foreign.delete(&note.id), Err(NotesError::Unreadable));
        assert_eq!(fs::read(&path).unwrap(), raw);
        let corrupt = Account(0x5a).protect(b"bad json").unwrap();
        fs::write(&path, &corrupt).unwrap();
        assert_eq!(
            original.save(input(None, "ne pas écraser")),
            Err(NotesError::Unreadable)
        );
        assert_eq!(fs::read(&path).unwrap(), corrupt);
    }

    #[test]
    fn ephemeral_mode_ignores_existing_storage_and_does_not_write() {
        let mut root = Root::new();
        let disk = root.book(0x5a);
        let persistent = disk.save(input(None, "sur disque")).unwrap();
        let raw = fs::read(root.path()).unwrap();
        root.0.ephemeral = true;
        let book = root.book(0x31);
        assert!(book.list().unwrap().is_empty());
        let note = book.save(input(None, "en mémoire")).unwrap();
        assert_eq!(book.get(&note.id).unwrap().text, "en mémoire");
        assert!(root.book(0x31).list().unwrap().is_empty());
        book.delete(&note.id).unwrap();
        assert!(book.list().unwrap().is_empty());
        assert_eq!(fs::read(root.path()).unwrap(), raw);
        assert_eq!(disk.list().unwrap(), [persistent]);
    }

    #[test]
    fn debug_does_not_disclose_title_or_note_text() {
        let root = Root::new();
        let book = root.book(0x5a);
        let payload = input(None, "titre secret\ncontenu privé");
        let input_debug = format!("{payload:?}");
        let meta = book.save(payload).unwrap();
        let note = book.get(&meta.id).unwrap();
        for rendered in [input_debug, format!("{meta:?}"), format!("{note:?}")] {
            assert!(!rendered.contains("titre secret"));
            assert!(!rendered.contains("contenu privé"));
        }
    }
}
