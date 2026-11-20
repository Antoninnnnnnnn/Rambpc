//! Data crossing the WebView boundary. Mirrored by hand in
//! `app/src/ipc/contract.ts`; keep both files in sync.
//!
//! Rule: dictated text crosses only through `DraftView`, on explicit request of
//! the view that displays it (command `get_draft`). State events never carry text.
//!
//! Commands (Rust `#[tauri::command]` in `crate::shell::commands`, camelCase
//! arguments on the JS side):
//!
//! | command               | args                                   | returns            |
//! |-----------------------|----------------------------------------|--------------------|
//! | `get_state`           | —                                      | `UiState`          |
//! | `get_settings`        | —                                      | `Settings`         |
//! | `save_settings`       | `settings: Settings`                   | `Settings` or `CommandError` |
//! | `list_microphones`    | —                                      | `MicInfo[]`        |
//! | `start_mic_test`      | —                                      | `()` (+ `sambler://mic-test-level` events) |
//! | `stop_mic_test`       | —                                      | `()`               |
//! | `set_consent`         | `enabled: boolean`                     | `UiState`          |
//! | `get_access`          | —                                      | `AccessStatus`     |
//! | `set_access`          | `access: AccessInput`                  | `AccessStatus`     |
//! | `clear_access`        | —                                      | `AccessStatus`     |
//! | `retry_connection`    | —                                      | `UiState`          |
//! | `dictate_in_sambler`  | —                                      | `UiState`          |
//! | `finish_take`         | `take: number`                         | `UiState`          |
//! | `cancel_take`         | `take: number`                         | `UiState`          |
//! | `get_draft`           | —                                      | `DraftView \| null` |
//! | `copy_draft`          | —                                      | `()` or `CommandError` |
//! | `clear_draft`         | —                                      | `UiState`          |
//! | `open_mic_privacy_settings` | —                                | `()` (opens `ms-settings:privacy-microphone`) |
//!
//! Events (`listen` on the main window):
//! - `sambler://state`: `UiState`, whenever it changes (coalesced, latest wins).
//! - `sambler://level`: `number` in 0..=1, ~20 Hz while listening (latest wins).
//! - `sambler://mic-test-level`: `number` in 0..=1 during a microphone test.

use crate::model::{DestinationKind, Gesture, TakeId};
use crate::ports::{MicError, NotWrittenReason, TargetChange, UncertainReason};
use serde::{Deserialize, Serialize};

pub const EVENT_STATE: &str = "sambler://state";
pub const EVENT_LEVEL: &str = "sambler://level";
pub const EVENT_MIC_TEST_LEVEL: &str = "sambler://mic-test-level";

/// Lifecycle of the current take.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Phase {
    /// "Disponible"
    #[default]
    Idle,
    /// Microphone opening, engine readiness and target checks.
    Preparing,
    /// "Écoute": the person can speak.
    Listening,
    /// "Traitement": capture closed, waiting for Rambler's final results.
    Finalizing,
}

/// Can a take be started right now?
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Availability {
    /// The person has not yet enabled sending their voice to Google.
    #[default]
    NeedsConsent,
    /// No Rambler access configured on this PC.
    NeedsAccess,
    Connecting,
    Ready,
    /// "Dictée indisponible". Settings, draft and history stay usable.
    Unavailable {
        reason: UnavailableReason,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnavailableReason {
    /// No network / DNS / TCP / TLS failure.
    Network,
    /// The service refused the access values (authentication / permission).
    AccessRefused,
    /// Rate limiting or quota.
    Throttled,
    /// Access values rejected locally (incomplete or malformed).
    InvalidAccess,
    /// Any other service-side failure.
    Service,
}

/// Why a take ended without (complete) insertion. Text-free.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum KeptReason {
    /// The person chose "Dicter dans Sambler" or "Toujours garder le texte dans Sambler".
    SamblerDestination,
    /// The field changed during the take.
    TargetChanged { change: TargetChange },
    /// The insertion attempt wrote nothing.
    NotWritten { reason: NotWrittenReason },
    /// Some ranges are missing, need review or are blocked: no automatic insertion.
    Incomplete,
    /// The field could not be qualified at the trigger (terminal, elevated
    /// window, unknown control…): the take went to Sambler instead of refusing.
    UncertainTarget { reason: UncertainReason },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum InterruptReason {
    /// No microphone frames within the watchdog delay.
    MicrophoneStalled,
    Microphone {
        error: MicError,
    },
    /// PCM handoff overflow: capture incomplete.
    CaptureOverflow,
    Network,
    AccessRefused,
    Throttled,
    Timeout,
    Service,
    /// Workstation locked, session change or suspend.
    SystemInterrupted,
    /// Consent withdrawn during the take.
    ConsentWithdrawn,
    /// The take reached the maximum duration and was finished (not an error).
    DurationLimit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RefusalReason {
    NeedsConsent,
    NeedsAccess,
    Unavailable {
        reason: UnavailableReason,
    },
    /// Password field or secure desktop: microphone not opened.
    ProtectedField,
    /// Target uncertain: offer "Dicter dans Sambler" instead.
    UncertainTarget {
        reason: UncertainReason,
    },
    /// Legacy (no longer produced since 0.10): a pending draft never blocks a
    /// new take; recent results are kept side by side instead.
    PendingDraft,
    Microphone {
        error: MicError,
    },
}

/// Result of the latest take. Text-free.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TakeOutcome {
    /// "Texte transmis au champ" (all events accepted by Windows).
    Inserted,
    /// "Vérifiez le champ, une partie du texte a pu être saisie."
    InsertedPartially,
    /// "Texte disponible" in Sambler with Copier / Effacer.
    KeptInSambler {
        reason: KeptReason,
    },
    /// "Aucun texte obtenu".
    NoText,
    Cancelled,
    /// The take stopped early; `recovered` tells whether admitted text is in the draft.
    Interrupted {
        reason: InterruptReason,
        recovered: bool,
    },
    /// The take could not start.
    Refused {
        reason: RefusalReason,
    },
}

/// Draft metadata (no text). Times are Unix epoch milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftMeta {
    pub take: TakeId,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    /// All ranges admitted and joined: the full text is available.
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    pub phase: Phase,
    pub take: Option<TakeId>,
    pub destination: Option<DestinationKind>,
    pub availability: Availability,
    pub last_outcome: Option<TakeOutcome>,
    pub draft: Option<DraftMeta>,
    /// Milliseconds since listening started (for the 5-minute limit warning).
    pub listening_ms: Option<u64>,
    pub gesture: Gesture,
    pub shortcut: String,
    /// Number of recent results kept in RAM (see `get_recent_drafts`).
    pub recent_count: u32,
    /// Current appearance, so the floating bar (events only) follows it live.
    pub appearance: crate::settings::AppearanceSettings,
}

/// Status of one recognized range in the draft.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PartStatus {
    /// Admitted text.
    Admitted,
    /// "Aucun texte obtenu" for this range.
    NoText,
    /// Ambiguous: to review, excluded from automatic insertion.
    NeedsReview,
    /// Safety-blocked: text hidden.
    Blocked,
    /// Lost to an interruption (gap).
    Missing,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftPart {
    pub status: PartStatus,
    /// Present only for `Admitted` (and for `NeedsReview` when displayable).
    pub text: Option<String>,
}

impl std::fmt::Debug for DraftPart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DraftPart")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

/// The draft text, returned only by `get_draft`.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftView {
    pub meta: DraftMeta,
    pub parts: Vec<DraftPart>,
    /// The rendered full text when the take is complete; `None` otherwise.
    pub full_text: Option<String>,
}

impl std::fmt::Debug for DraftView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DraftView")
            .field("meta", &self.meta)
            .field("parts", &self.parts)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccessSource {
    None,
    /// `RAMBLER_AUTHORITY` + the three `RAMBLER_*` variables of the process.
    Environment,
    /// Saved for this Windows account with DPAPI (installed edition only).
    Saved,
    /// Kept in memory for this session only (portable edition, or not remembered).
    Session,
}

/// Never contains the secret values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessStatus {
    pub configured: bool,
    pub source: AccessSource,
    /// Service address (host:port) — not a secret.
    pub authority: Option<String>,
    /// False in the portable edition: access never written to disk there.
    pub can_remember: bool,
}

/// Entered once by the person. Values are moved to native memory and zeroized.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessInput {
    pub authority: String,
    pub api_key: String,
    pub android_package: String,
    pub android_cert: String,
    /// Save with DPAPI for this account (installed edition only).
    pub remember: bool,
}

impl std::fmt::Debug for AccessInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AccessInput(..)")
    }
}

/// Error returned by commands. `code` is stable; `message` is French, text-free.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl CommandError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

// ---------------------------------------------------------------- milestone 2
//
// Personal rules (lexicon + expressions) and the optional text history.
// Commands (implemented in `crate::shell::commands`):
//
// | command                   | args                                  | returns              |
// |---------------------------|---------------------------------------|----------------------|
// | `get_personal_rules`      | —                                     | `PersonalRules`      |
// | `save_personal_rules`     | `rules: PersonalRulesInput`           | `SaveRulesResult`    |
// | `get_history`             | `query: HistoryQuery`                 | `HistoryPage`        |
// | `copy_history_entry`      | `id: string`                          | `()` or `CommandError` |
// | `delete_history_entry`    | `id: string`                          | `HistoryStatus`      |
// | `clear_history`           | `disable: boolean` (also turn it off) | `HistoryStatus`      |
// | `get_history_status`      | —                                     | `HistoryStatus`      |
// | `skip_history_next_take`  | `skip: boolean`                       | `HistoryStatus`      |
// | `set_launch_at_startup`   | `enabled: boolean`                    | `()` or `CommandError` (installed only) |
//
// Rules: lexicon and expressions are stored protected with DPAPI; they reach
// the WebView only through `get_personal_rules` (settings view). History text
// reaches the WebView only through `get_history` on the history view.

use crate::model::{Language, RenderProfile};
use crate::text::{Expression, LexiconEntry, RuleConflict};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalRules {
    pub lexicon: Vec<LexiconEntry>,
    pub expressions: Vec<Expression>,
    /// The stored rules exist but cannot be decrypted under this Windows
    /// account (moved portable profile). They are preserved, not shown.
    pub unreadable: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalRulesInput {
    pub lexicon: Vec<LexiconEntry>,
    pub expressions: Vec<Expression>,
}

/// `saved` is false when enabled rules conflict; nothing is written then.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveRulesResult {
    pub saved: bool,
    pub rules: PersonalRules,
    pub conflicts: Vec<RuleConflict>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HistoryQuery {
    /// Case-insensitive substring search over decrypted text, done in memory
    /// in small batches (no plaintext index).
    pub search: Option<String>,
    /// Page size (default 50, max 200).
    pub limit: Option<u32>,
    /// Opaque cursor from a previous page.
    pub cursor: Option<String>,
}

impl std::fmt::Debug for HistoryQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HistoryQuery")
            .field("limit", &self.limit)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub language: Language,
    pub profile: RenderProfile,
    /// `None` when this entry cannot be decrypted under this account.
    pub text: Option<String>,
}

impl std::fmt::Debug for HistoryEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HistoryEntry")
            .field("id", &self.id)
            .field("created_at_ms", &self.created_at_ms)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub entries: Vec<HistoryEntry>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryStatus {
    pub enabled: bool,
    pub retention_days: u16,
    pub entry_count: u32,
    /// "Sans historique pour la prochaine dictée" is armed.
    pub skip_next: bool,
    /// False in ephemeral sessions (read-only data folder).
    pub available: bool,
}

#[cfg(test)]
#[test]
fn transcript_debug_is_redacted_without_changing_serialization() {
    let text = "fixture private text";
    let draft = DraftView {
        meta: DraftMeta {
            take: TakeId(1),
            created_at_ms: 1,
            expires_at_ms: 2,
            complete: true,
        },
        parts: vec![DraftPart {
            status: PartStatus::Admitted,
            text: Some(text.into()),
        }],
        full_text: Some(text.into()),
    };
    let entry = HistoryEntry {
        id: "synthetic-id".into(),
        created_at_ms: 1,
        expires_at_ms: 2,
        language: Language::English,
        profile: RenderProfile::Natural,
        text: Some(text.into()),
    };
    let query = HistoryQuery {
        search: Some(text.into()),
        ..HistoryQuery::default()
    };
    assert!(!format!("{draft:?} {entry:?} {query:?}").contains(text));
    assert_eq!(serde_json::to_value(&draft).unwrap()["fullText"], text);
    assert_eq!(serde_json::to_value(&entry).unwrap()["text"], text);
}

// ---------------------------------------------------------------- milestone 3
//
// Instant start, recent results and usage statistics.
//
// | command              | args                         | returns              |
// |----------------------|------------------------------|----------------------|
// | `get_recent_drafts`  | —                            | `DraftView[]` (newest first, at most 5, RAM only, same lifetime as the draft) |
// | `copy_draft`         | `take?: number` (default: latest) | `()` or `CommandError` |
// | `clear_draft`        | `take?: number` (default: all recent) | `UiState` |
// | `get_stats`          | —                            | `UsageStats`         |
//
// Behaviour changes: the take enters `Listening` immediately on the shortcut
// edge (audio buffered locally until the field is qualified; discarded if the
// field is protected); an uncertain field sends the take to Sambler with
// `KeptReason::UncertainTarget` instead of refusing; a pending draft never
// blocks a new take.

/// Counters only (no text), persisted as plain JSON in the data root.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageStats {
    pub words_today: u64,
    pub words_this_week: u64,
    pub words_total: u64,
    pub takes_total: u64,
    /// Words per minute over listening time of completed takes, when known.
    pub average_wpm: Option<u32>,
}

// ---------------------------------------------------------------- milestone 4
//
// | command        | args | returns          |
// |----------------|------|------------------|
// | `get_activity` | —    | `ActivityStats`  |
//
// Counters only (never text), persisted with the usage statistics.

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityDay {
    /// Local calendar date, `YYYY-MM-DD`.
    pub date: String,
    pub words: u64,
    pub takes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppWords {
    /// Display name.
    pub app: String,
    pub words: u64,
    /// Lowercased executable name without extension.
    pub id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityStats {
    /// The last 112 local days, oldest first, ending today, zero days included.
    pub days: Vec<ActivityDay>,
    /// Consecutive days with a completed take, ending today (or yesterday when
    /// there is none today yet).
    pub current_streak: u32,
    pub longest_streak: u32,
    pub longest_streak_end: Option<String>,
    /// First day with a completed take.
    pub first_day: Option<String>,
    /// Captured audio time of completed takes.
    pub listening_ms_total: u64,
    /// Words per target application; `None` until the host records it.
    pub apps: Option<Vec<AppWords>>,
}

// ---------------------------------------------------------------- milestone 5
//
// | command              | args                                   | returns            |
// |----------------------|----------------------------------------|--------------------|
// | `get_known_apps`     | —                                      | `AppIdentity[]` (most used first) |
// | `get_voice_commands` | —                                      | `VoiceCommandInfo[]` |
// | `list_notes`         | —                                      | `NoteMeta[]` (most recently updated first) |
// | `get_note`           | `id: string`                           | `Note`             |
// | `save_note`          | `note: { id: string \| null, text }`   | `NoteMeta` (null id creates; ≤ 200 000 chars, ≤ 1 000 notes) |
// | `delete_note`        | `id: string`                           | `()`               |
// | `pick_audio_file`  | —                                      | `string \| null` (native open dialog; the path is remembered as allowed) |
// | `transcribe_file`    | `path: string` (from `pick_audio_file` or `sambler://file-dropped` only) | `FileTranscription` (one at a time) |
// | `cancel_transcription` | `id: string`                         | `FileTranscription` |
// | `list_transcriptions` | —                                     | `FileTranscription[]` (this session, newest first) |
//
// Events: `sambler://transcription` → `FileTranscription` (~4 Hz while running
// and at the end; never any text); `sambler://file-dropped` → full path of an
// audio file dropped on the main window (now allowed for `transcribe_file`).
// A finished transcription becomes a Carnet note « Transcription · <fileName> ».
// `ActivityStats.apps` is now filled (`AppWords` gains `id`).

pub const EVENT_TRANSCRIPTION: &str = "sambler://transcription";
pub const EVENT_FILE_DROPPED: &str = "sambler://file-dropped";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceCommandInfo {
    /// What to say, e.g. « à la ligne ».
    pub spoken: String,
    /// What it produces, shown literally (e.g. "↵" for a line break).
    pub result: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteMeta {
    pub id: String,
    /// First non-empty line, at most 80 characters.
    pub title: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub words: u64,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    #[serde(flatten)]
    pub meta: NoteMeta,
    pub text: String,
}

impl std::fmt::Debug for Note {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Note").field("meta", &self.meta).finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteInput {
    pub id: Option<String>,
    pub text: String,
}

impl std::fmt::Debug for NoteInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoteInput").field("id", &self.id).finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileTranscriptionStatus {
    Running,
    Done,
    Failed,
    Cancelled,
}

/// One audio file transcription of this session. Never carries text.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTranscription {
    pub id: String,
    /// Base name only, no folder.
    pub file_name: String,
    pub duration_ms: Option<u64>,
    pub status: FileTranscriptionStatus,
    /// 0..=1.
    pub progress: f32,
    pub words: u64,
    /// The Carnet note holding the text (also set for a partial result).
    pub note_id: Option<String>,
    /// French, text-free reason when failed.
    pub error: Option<String>,
    /// Unix epoch milliseconds.
    pub started_at: u64,
}
