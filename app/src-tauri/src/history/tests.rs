use super::*;
use crate::ports::ProtectError;
use std::{fs, sync::Barrier};
use zeroize::Zeroizing;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sambler-history-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

/// Test only. Prefix simulates a profile-bound selector; XOR makes it easy
/// to assert that the SQLite pages do not contain the original UTF-8 bytes.
struct ProtectorStub(u8);
impl Protector for ProtectorStub {
    fn protect(&self, plain: &[u8]) -> std::result::Result<Vec<u8>, ProtectError> {
        Ok(std::iter::once(self.0)
            .chain(plain.iter().map(|b| b ^ self.0))
            .collect())
    }
    fn unprotect(&self, sealed: &[u8]) -> std::result::Result<Zeroizing<Vec<u8>>, ProtectError> {
        match sealed.split_first() {
            Some((&tag, data)) if tag == self.0 => {
                Ok(Zeroizing::new(data.iter().map(|b| b ^ tag).collect()))
            }
            _ => Err(ProtectError::Unreadable),
        }
    }
}

#[derive(Default)]
struct ClipboardStub(Mutex<Option<String>>);
impl Clipboard for ClipboardStub {
    fn copy_private(&self, text: &str) -> std::result::Result<(), ClipboardError> {
        *self.0.lock().unwrap() = Some(text.into());
        Ok(())
    }
}

fn options(enabled: bool, retention_days: u16) -> HistoryOptions {
    HistoryOptions {
        enabled,
        retention_days,
    }
}
fn database(path: PathBuf) -> Database {
    Database {
        path,
        conn: None,
        session: Uuid::new_v4(),
        protector: Arc::new(ProtectorStub(0x57)),
        clipboard: Arc::new(ClipboardStub::default()),
        policy: Arc::new(Policy::new(&options(true, 7), true)),
    }
}
fn take(id: u64, created: u64, text: &str) -> CompletedTake {
    CompletedTake {
        id: TakeId(id),
        created,
        language: Language::English,
        profile: RenderProfile::Faithful,
        text: Arc::from(text),
    }
}

#[test]
fn shortening_applies_to_existing_rows_and_longer_setting_cannot_restore_them() {
    let folder = Temp::new();
    let mut db = database(folder.0.join("test.sqlite3"));
    let t = db.policy.ticket().unwrap();
    db.append(t, take(1, DAY, "Too old"), DAY).unwrap();
    db.append(t, take(2, 2 * DAY, "Retained"), 2 * DAY).unwrap();
    let now = 2 * DAY + 1;
    assert_eq!(db.status(now).unwrap().entry_count, 2);
    db.policy.configure(&options(true, 1), false);
    assert_eq!(db.status(now).unwrap().entry_count, 1); // Even before physical purge.
    db.retain(1, now).unwrap();
    db.policy.configure(&options(true, 30), false);
    let page = db.page(HistoryQuery::default(), now).unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].expires_at_ms, 3 * DAY);
    assert_eq!(
        db.page(HistoryQuery::default(), 3 * DAY)
            .unwrap()
            .entries
            .len(),
        0
    );
    db.maintain(3 * DAY).unwrap();
    let conn = db.conn.as_ref().unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM history", [], |row| row
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("PRAGMA secure_delete", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        1
    );
}

#[test]
fn reopening_purges_expired_rows_and_never_reuses_run_ids() {
    let folder = Temp::new();
    let path = folder.0.join("test.sqlite3");
    let mut first = database(path.clone());
    let t = first.policy.ticket().unwrap();
    first.append(t, take(1, DAY, "Old"), DAY).unwrap();
    first
        .append(t, take(2, 7 * DAY, "Recent"), 7 * DAY)
        .unwrap();
    drop(first);
    let mut reopened = database(path);
    reopened.maintain(8 * DAY).unwrap();
    let t = reopened.policy.ticket().unwrap();
    reopened
        .append(t, take(2, 8 * DAY, "New run"), 8 * DAY)
        .unwrap();
    assert_eq!(reopened.status(8 * DAY).unwrap().entry_count, 2);
}

#[test]
fn skip_is_one_shot_and_disabling_during_finalization_invalidates_old_ticket() {
    let folder = Temp::new();
    let mut db = database(folder.0.join("test.sqlite3"));
    db.policy.flags.fetch_or(SKIP_NEXT, Ordering::Release);
    assert!(db.policy.ticket().is_none());
    let finalizing = db.policy.ticket().unwrap();
    db.policy.configure(&options(false, 7), false);
    // Re-enabling before the collector responds must not resurrect that take.
    db.policy.configure(&options(true, 7), false);
    db.append(finalizing, take(1, DAY, "Do not retain"), DAY + 2000)
        .unwrap();
    let accepted = db.policy.ticket().unwrap();
    db.append(accepted, take(2, DAY + 2001, "New take"), DAY + 3000)
        .unwrap();
    assert_eq!(db.status(DAY + 3000).unwrap().entry_count, 1);
    let clearing = db.policy.ticket().unwrap();
    db.policy.configure(&options(true, 7), true);
    db.delete(None).unwrap();
    db.append(clearing, take(3, DAY + 2002, "Do not restore"), DAY + 3001)
        .unwrap();
    assert_eq!(db.status(DAY + 3001).unwrap().entry_count, 0);
}

#[test]
fn writer_rechecks_revocation_after_encryption_but_before_sqlite() {
    struct Pause(Arc<Barrier>);
    impl Protector for Pause {
        fn protect(&self, plain: &[u8]) -> std::result::Result<Vec<u8>, ProtectError> {
            self.0.wait();
            self.0.wait();
            ProtectorStub(0x57).protect(plain)
        }
        fn unprotect(&self, blob: &[u8]) -> std::result::Result<Zeroizing<Vec<u8>>, ProtectError> {
            ProtectorStub(0x57).unprotect(blob)
        }
    }
    let folder = Temp::new();
    let gate = Arc::new(Barrier::new(2));
    let mut db = database(folder.0.join("test.sqlite3"));
    db.protector = Arc::new(Pause(gate.clone()));
    let policy = db.policy.clone();
    let t = policy.ticket().unwrap();
    let worker = thread::spawn(move || {
        db.append(t, take(1, DAY, "Late"), DAY + 2000).unwrap();
        db.status(DAY + 2000).unwrap().entry_count
    });
    gate.wait();
    policy.configure(&options(false, 7), false);
    gate.wait();
    assert_eq!(worker.join().unwrap(), 0);
}

#[test]
fn idempotent_ciphertext_and_unreadable_entries_are_preserved() {
    let folder = Temp::new();
    let path = folder.0.join("test.sqlite3");
    let mut db = database(path.clone());
    let t = db.policy.ticket().unwrap();
    let original = "Texte synthétique et privé.";
    db.append(t, take(8, DAY, original), DAY).unwrap();
    db.append(t, take(8, DAY, "Not a replacement"), DAY)
        .unwrap();
    assert!(
        !fs::read(path)
            .unwrap()
            .windows(original.len())
            .any(|window| window == original.as_bytes())
    );
    let page = db
        .page(
            HistoryQuery {
                search: Some("SYNTHÉTIQUE".into()),
                limit: Some(1),
                cursor: None,
            },
            DAY,
        )
        .unwrap();
    let entry = &page.entries[0];
    assert_eq!(entry.language, Language::English);
    assert_eq!(entry.profile, RenderProfile::Faithful);
    assert_eq!(entry.text.as_deref(), Some(original));
    let id = entry.id.clone();
    db.protector = Arc::new(ProtectorStub(0x34));
    let page = db.page(HistoryQuery::default(), DAY).unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].id, id);
    assert_eq!(page.entries[0].text, None);
    assert_eq!(db.copy(&id, DAY), Err(HistoryError::NoText));
    assert_eq!(db.status(DAY).unwrap().entry_count, 1);
}

#[test]
fn worker_can_page_copy_and_delete_and_ephemeral_does_not_create_files() {
    let folder = Temp::new();
    let path = folder.0.join("test.sqlite3");
    let protector = Arc::new(ProtectorStub(0x57));
    let clipboard = Arc::new(ClipboardStub::default());
    let disabled = History::spawn(
        None,
        &options(true, 7),
        protector.clone(),
        clipboard.clone(),
    );
    assert!(!disabled.status().available);
    assert!(disabled.ticket().is_none());
    assert!(!path.exists());
    let history = History::spawn(Some(path), &options(true, 7), protector, clipboard.clone());
    assert!(history.skip_next(true).skip_next);
    assert!(history.ticket().is_none());
    let t = history.ticket().unwrap();
    let now = now_ms();
    for id in 1..=3 {
        history.archive(t, take(id, now, &format!("Note {id}")));
    }
    let first = history
        .page(HistoryQuery {
            search: None,
            limit: Some(1),
            cursor: None,
        })
        .unwrap();
    assert_eq!(first.entries[0].text.as_deref(), Some("Note 3"));
    let second = history
        .page(HistoryQuery {
            search: None,
            limit: Some(2),
            cursor: first.next_cursor,
        })
        .unwrap();
    assert_eq!(second.entries.len(), 2);
    assert!(second.next_cursor.is_none());
    history.copy(second.entries[0].id.clone()).unwrap();
    assert_eq!(clipboard.0.lock().unwrap().as_deref(), Some("Note 2"));
    assert_eq!(
        history
            .delete(second.entries[0].id.clone())
            .unwrap()
            .entry_count,
        2
    );
    history.invalidate_for_clear(&options(false, 7));
    assert!(!history.clear().unwrap().enabled);
    assert_eq!(history.status().entry_count, 0);
}
