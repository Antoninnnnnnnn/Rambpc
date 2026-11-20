//! One native thread owns SQLite, DPAPI work, reads and writes. The controller
//! only issues a text-free authorization ticket and submits completed text.

use crate::{
    ipc::{HistoryEntry, HistoryPage, HistoryQuery, HistoryStatus},
    model::{Language, RenderProfile, TakeId},
    ports::{Clipboard, ClipboardError, Protector},
    settings::HistoryOptions,
};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const ENABLED: u64 = 1;
const SKIP_NEXT: u64 = 2;
const NEXT_GENERATION: u64 = 4;
const DAY: u64 = 86_400_000;
const BATCH: u32 = 32;
const PURGE_INTERVAL: Duration = Duration::from_secs(3600);

#[derive(Clone, Copy)]
pub(crate) struct Ticket(u64);

struct Policy {
    flags: AtomicU64,
    retention: AtomicU16,
    available: AtomicBool,
    // Serialize policy revocation with the final INSERT, never with DPAPI,
    // decryption or reads. Issuing a ticket is lock-free on the actor.
    commit: Mutex<()>,
}

impl Policy {
    fn new(options: &HistoryOptions, available: bool) -> Self {
        Self {
            flags: AtomicU64::new(u64::from(options.enabled)),
            retention: AtomicU16::new(options.retention_days),
            available: AtomicBool::new(available),
            commit: Mutex::new(()),
        }
    }

    fn ticket(&self) -> Option<Ticket> {
        let flags = self.flags.fetch_and(!SKIP_NEXT, Ordering::AcqRel);
        (flags & (ENABLED | SKIP_NEXT) == ENABLED && self.available.load(Ordering::Acquire))
            .then_some(Ticket(flags & !SKIP_NEXT))
    }

    fn admits(&self, ticket: Ticket) -> bool {
        self.available.load(Ordering::Acquire)
            && self.flags.load(Ordering::Acquire) & !SKIP_NEXT == ticket.0
    }

    fn days(&self) -> u16 {
        self.retention.load(Ordering::Acquire)
    }

    fn configure(&self, options: &HistoryOptions, erase: bool) {
        let _commit = self.commit.lock().unwrap();
        self.retention
            .store(options.retention_days, Ordering::Release);
        let mut flags = self.flags.load(Ordering::Acquire);
        loop {
            let revoke = erase || (flags & ENABLED != 0 && !options.enabled);
            let next = if revoke {
                flags + NEXT_GENERATION
            } else {
                flags
            };
            let next = (next & !ENABLED) | u64::from(options.enabled);
            match self
                .flags
                .compare_exchange_weak(flags, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => break,
                Err(current) => flags = current,
            }
        }
    }

    fn status(&self, entry_count: u32) -> HistoryStatus {
        let flags = self.flags.load(Ordering::Acquire);
        let available = self.available.load(Ordering::Acquire);
        HistoryStatus {
            enabled: available && flags & ENABLED != 0,
            retention_days: self.days(),
            entry_count,
            skip_next: available && flags & SKIP_NEXT != 0,
            available,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HistoryError {
    Unavailable,
    Database,
    Protection,
    NoText,
    Clipboard(ClipboardError),
}

impl From<rusqlite::Error> for HistoryError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Database
    }
}

type Result<T> = std::result::Result<T, HistoryError>;
type Reply<T> = mpsc::Sender<Result<T>>;

pub(crate) struct CompletedTake {
    pub id: TakeId,
    pub created: u64,
    pub language: Language,
    pub profile: RenderProfile,
    pub text: Arc<str>,
}

enum Request {
    Append(Ticket, CompletedTake),
    List(HistoryQuery, Reply<HistoryPage>),
    Copy(String, Reply<()>),
    Delete(String, Reply<HistoryStatus>),
    Clear(Reply<HistoryStatus>),
    Retain(u16, Reply<()>),
    Status(Reply<HistoryStatus>),
}

#[derive(Clone)]
pub(crate) struct History(Arc<Handle>);

struct Handle {
    // Sender is dropped before joining, draining queued jobs at normal exit.
    sender: Option<SyncSender<Request>>,
    ephemeral: bool,
    worker: Mutex<Option<JoinHandle<()>>>,
    policy: Arc<Policy>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.get_mut().unwrap().take() {
            let _ = worker.join();
        }
    }
}

impl History {
    pub(crate) fn spawn(
        path: Option<PathBuf>,
        options: &HistoryOptions,
        protector: Arc<dyn Protector>,
        clipboard: Arc<dyn Clipboard>,
    ) -> Self {
        let ephemeral = path.is_none();
        let policy = Arc::new(Policy::new(options, !ephemeral));
        let (sender, worker) = if let Some(path) = path {
            let (tx, rx) = mpsc::sync_channel(32);
            let policy = policy.clone();
            match thread::Builder::new()
                .name("sambler-history".into())
                .spawn(move || {
                    let mut db = Database {
                        path,
                        conn: None,
                        session: Uuid::new_v4(),
                        protector,
                        clipboard,
                        policy,
                    };
                    db.run(rx);
                }) {
                Ok(worker) => (Some(tx), Some(worker)),
                Err(_) => (None, None),
            }
        } else {
            (None, None)
        };
        if sender.is_none() {
            policy.available.store(false, Ordering::Release);
        }
        Self(Arc::new(Handle {
            sender,
            ephemeral,
            worker: Mutex::new(worker),
            policy,
        }))
    }

    pub(crate) fn ticket(&self) -> Option<Ticket> {
        self.0.policy.ticket()
    }

    pub(crate) fn archive(&self, ticket: Ticket, take: CompletedTake) {
        if let Some(sender) = &self.0.sender
            && sender.try_send(Request::Append(ticket, take)).is_err()
        {
            // An overloaded/disconnected writer cannot silently claim to be
            // archiving. Dictation and insertion remain independent.
            self.0.policy.available.store(false, Ordering::Release);
        }
    }

    fn request<T>(&self, request: impl FnOnce(Reply<T>) -> Request) -> Result<T> {
        let sender = self.0.sender.as_ref().ok_or(HistoryError::Unavailable)?;
        let (reply, result) = mpsc::channel();
        sender
            .send(request(reply))
            .map_err(|_| HistoryError::Unavailable)?;
        result.recv().map_err(|_| HistoryError::Unavailable)?
    }

    pub(crate) fn configure(&self, options: &HistoryOptions) -> Result<()> {
        let old_days = self.0.policy.days();
        self.0.policy.configure(options, false);
        if options.retention_days < old_days && self.0.sender.is_some() {
            self.request(|reply| Request::Retain(options.retention_days, reply))?;
        }
        Ok(())
    }

    pub(crate) fn invalidate_for_clear(&self, options: &HistoryOptions) {
        self.0.policy.configure(options, true);
    }

    pub(crate) fn clear(&self) -> Result<HistoryStatus> {
        match self.request(Request::Clear) {
            Err(HistoryError::Unavailable) if self.0.ephemeral => Ok(self.0.policy.status(0)),
            result => result,
        }
    }

    pub(crate) fn status(&self) -> HistoryStatus {
        self.request(Request::Status)
            .unwrap_or_else(|_| self.0.policy.status(0))
    }

    pub(crate) fn skip_next(&self, skip: bool) -> HistoryStatus {
        if self.0.policy.available.load(Ordering::Acquire) {
            if skip {
                self.0.policy.flags.fetch_or(SKIP_NEXT, Ordering::AcqRel);
            } else {
                self.0.policy.flags.fetch_and(!SKIP_NEXT, Ordering::AcqRel);
            }
        }
        self.status()
    }

    pub(crate) fn page(&self, query: HistoryQuery) -> Result<HistoryPage> {
        if self.0.ephemeral {
            return Ok(HistoryPage {
                entries: Vec::new(),
                next_cursor: None,
            });
        }
        self.request(|reply| Request::List(query, reply))
    }

    pub(crate) fn delete(&self, id: String) -> Result<HistoryStatus> {
        self.request(|reply| Request::Delete(id, reply))
    }

    pub(crate) fn copy(&self, id: String) -> Result<()> {
        self.request(|reply| Request::Copy(id, reply))
    }
}

struct Database {
    path: PathBuf,
    conn: Option<Connection>,
    session: Uuid,
    protector: Arc<dyn Protector>,
    clipboard: Arc<dyn Clipboard>,
    policy: Arc<Policy>,
}

impl Database {
    fn run(&mut self, requests: Receiver<Request>) {
        let result = self.maintain(now_ms());
        let _ = self.record_failure(result);
        let mut next_purge = Instant::now() + PURGE_INTERVAL;
        loop {
            if Instant::now() >= next_purge {
                let result = self.maintain(now_ms());
                let _ = self.record_failure(result);
                next_purge = Instant::now() + PURGE_INTERVAL;
            }
            match requests.recv_timeout(next_purge.saturating_duration_since(Instant::now())) {
                Ok(request) => self.process(request, now_ms()),
                Err(RecvTimeoutError::Timeout) => (),
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    fn record_failure<T>(&self, result: Result<T>) -> Result<T> {
        if matches!(
            result,
            Err(HistoryError::Database | HistoryError::Protection)
        ) {
            self.policy.available.store(false, Ordering::Release);
        }
        result
    }

    fn process(&mut self, request: Request, now: u64) {
        match request {
            Request::Append(ticket, value) => {
                let result = self.append(ticket, value, now);
                let _ = self.record_failure(result);
            }
            Request::List(query, reply) => {
                let result = self.page(query, now);
                let _ = reply.send(self.record_failure(result));
            }
            Request::Copy(id, reply) => {
                let result = self.copy(&id, now);
                let _ = reply.send(self.record_failure(result));
            }
            Request::Delete(id, reply) => {
                let result = self.delete(Some(&id)).and_then(|()| self.status(now));
                let _ = reply.send(self.record_failure(result));
            }
            Request::Clear(reply) => {
                let result = self.delete(None).and_then(|()| self.status(now));
                let _ = reply.send(self.record_failure(result));
            }
            Request::Status(reply) => {
                let result = self.status(now);
                let _ = reply.send(self.record_failure(result));
            }
            Request::Retain(days, reply) => {
                let result = self.retain(days, now);
                let _ = reply.send(self.record_failure(result));
            }
        }
    }

    fn open(&mut self, create: bool) -> Result<Option<&Connection>> {
        if self.conn.is_none()
            && (create || self.path.try_exists().map_err(|_| HistoryError::Database)?)
        {
            let conn = Connection::open(&self.path)?;
            conn.busy_timeout(Duration::from_millis(250))?;
            conn.execute_batch(
                "PRAGMA secure_delete = ON;
                CREATE TABLE IF NOT EXISTS history (
                    id TEXT PRIMARY KEY NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    expires_at_ms INTEGER NOT NULL,
                    language INTEGER NOT NULL,
                    profile INTEGER NOT NULL,
                    protected_text BLOB NOT NULL
                );
                CREATE INDEX IF NOT EXISTS history_newest ON history(created_at_ms DESC, id DESC);
                CREATE INDEX IF NOT EXISTS history_expiration ON history(expires_at_ms);",
            )?;
            self.conn = Some(conn);
        }
        Ok(self.conn.as_ref())
    }

    fn maintain(&mut self, now: u64) -> Result<()> {
        self.retain(self.policy.days(), now)
    }

    fn retain(&mut self, days: u16, now: u64) -> Result<()> {
        if let Some(conn) = self.open(false)? {
            conn.execute(
                "DELETE FROM history WHERE expires_at_ms <= ?1 OR created_at_ms + ?2 <= ?1",
                params![now, days_ms(days)],
            )?;
            conn.execute("UPDATE history SET expires_at_ms = created_at_ms + ?1 WHERE expires_at_ms > created_at_ms + ?1", [days_ms(days)])?;
        }
        Ok(())
    }

    fn append(&mut self, ticket: Ticket, take: CompletedTake, now: u64) -> Result<()> {
        if !self.policy.admits(ticket) {
            return Ok(());
        }
        let sealed = self
            .protector
            .protect(take.text.as_bytes())
            .map_err(|_| HistoryError::Protection)?;
        let id = format!("{}-{:016x}", self.session.simple(), take.id.0);
        let policy = self.policy.clone();
        let _commit = policy.commit.lock().unwrap();
        if !policy.admits(ticket) {
            return Ok(());
        }
        let expires = take.created.saturating_add(days_ms(policy.days()));
        if expires <= now {
            return Ok(());
        }
        self.open(true)?.unwrap().execute(
            "INSERT INTO history (id, created_at_ms, expires_at_ms, language, profile, protected_text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(id) DO NOTHING",
            params![id, take.created, expires, encode_language(take.language), encode_profile(take.profile), sealed],
        )?;
        Ok(())
    }

    fn page(&mut self, query: HistoryQuery, now: u64) -> Result<HistoryPage> {
        let days = days_ms(self.policy.days());
        let protector = self.protector.clone();
        let mut entries = Vec::new();
        let mut next_cursor = None;
        let page_len = query.limit.unwrap_or(50).clamp(1, 200) as usize;
        let search = query
            .search
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase);
        let Some(conn) = self.open(false)? else {
            return Ok(HistoryPage {
                entries,
                next_cursor,
            });
        };
        let mut cursor = query.cursor.as_deref().and_then(parse_cursor);
        loop {
            let (before, before_id) = cursor.take().unwrap_or((i64::MAX as u64, String::new()));
            let mut statement = conn.prepare_cached("SELECT id, created_at_ms, MIN(expires_at_ms, created_at_ms + ?1), language, profile, protected_text
                FROM history WHERE expires_at_ms > ?2 AND created_at_ms + ?1 > ?2
                    AND (created_at_ms, id) < (?3, ?4)
                ORDER BY created_at_ms DESC, id DESC LIMIT ?5")?;
            // Only this batch of ciphertext is read before decryption.
            let rows = statement
                .query_map(params![days, now, before, before_id, BATCH], read_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let count = rows.len();
            for (mut entry, sealed) in rows {
                entry.text = decrypt(&*protector, &sealed);
                cursor = Some((entry.created_at_ms, entry.id.clone()));
                let matches = search.as_ref().is_none_or(|pattern| {
                    entry
                        .text
                        .as_ref()
                        .is_some_and(|text| text.to_lowercase().contains(pattern))
                });
                if matches {
                    if entries.len() == page_len {
                        next_cursor = entries.last().map(|last: &HistoryEntry| {
                            format!("{}:{}", last.created_at_ms, last.id)
                        });
                        return Ok(HistoryPage {
                            entries,
                            next_cursor,
                        });
                    }
                    entries.push(entry);
                }
            }
            if count < BATCH as usize {
                return Ok(HistoryPage {
                    entries,
                    next_cursor,
                });
            }
        }
    }

    fn copy(&mut self, id: &str, now: u64) -> Result<()> {
        let days = days_ms(self.policy.days());
        let sealed: Vec<u8> = self.open(false)?.ok_or(HistoryError::NoText)?.query_row(
            "SELECT protected_text FROM history WHERE id = ?1 AND expires_at_ms > ?2 AND created_at_ms + ?3 > ?2",
            params![id, now, days], |row| row.get(0),
        ).optional()?.ok_or(HistoryError::NoText)?;
        let plain = self
            .protector
            .unprotect(&sealed)
            .map_err(|_| HistoryError::NoText)?;
        let text = std::str::from_utf8(&plain).map_err(|_| HistoryError::NoText)?;
        self.clipboard
            .copy_private(text)
            .map_err(HistoryError::Clipboard)
    }

    fn delete(&mut self, id: Option<&str>) -> Result<()> {
        if let Some(conn) = self.open(false)? {
            match id {
                Some(id) => conn.execute("DELETE FROM history WHERE id = ?1", [id])?,
                None => conn.execute("DELETE FROM history", [])?,
            };
        }
        Ok(())
    }

    fn status(&mut self, now: u64) -> Result<HistoryStatus> {
        let days = days_ms(self.policy.days());
        let count = match self.open(false)? {
            Some(conn) => conn.query_row(
                "SELECT COUNT(*) FROM history WHERE expires_at_ms > ?1 AND created_at_ms + ?2 > ?1",
                params![now, days],
                |row| row.get::<_, u64>(0),
            )?,
            None => 0,
        };
        Ok(self.policy.status(u32::try_from(count).unwrap_or(u32::MAX)))
    }
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(HistoryEntry, Vec<u8>)> {
    Ok((
        HistoryEntry {
            id: row.get(0)?,
            created_at_ms: row.get(1)?,
            expires_at_ms: row.get(2)?,
            language: if row.get::<_, u8>(3)? == 0 {
                Language::French
            } else {
                Language::English
            },
            profile: if row.get::<_, u8>(4)? == 0 {
                RenderProfile::Natural
            } else {
                RenderProfile::Faithful
            },
            text: None,
        },
        row.get(5)?,
    ))
}

fn decrypt(protector: &dyn Protector, sealed: &[u8]) -> Option<String> {
    let plain = protector.unprotect(sealed).ok()?;
    std::str::from_utf8(&plain).ok().map(str::to_owned)
}

fn encode_language(value: Language) -> u8 {
    match value {
        Language::French => 0,
        Language::English => 1,
    }
}
fn encode_profile(value: RenderProfile) -> u8 {
    match value {
        RenderProfile::Natural => 0,
        RenderProfile::Faithful => 1,
    }
}
fn days_ms(days: u16) -> u64 {
    u64::from(days) * DAY
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn parse_cursor(cursor: &str) -> Option<(u64, String)> {
    let (time, id) = cursor.split_once(':')?;
    let time: i64 = time.parse().ok()?;
    if id.len() != 49 || !id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return None;
    }
    Some((u64::try_from(time).ok()?, id.to_owned()))
}

#[cfg(test)]
mod tests;
