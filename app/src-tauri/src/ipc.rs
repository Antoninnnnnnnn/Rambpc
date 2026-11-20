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
    Unavailable { reason: UnavailableReason },
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum InterruptReason {
    /// No microphone frames within the watchdog delay.
    MicrophoneStalled,
    Microphone { error: MicError },
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
    Unavailable { reason: UnavailableReason },
    /// Password field or secure desktop: microphone not opened.
    ProtectedField,
    /// Target uncertain: offer "Dicter dans Sambler" instead.
    UncertainTarget { reason: UncertainReason },
    /// A previous failed take left text pending recovery; offer copy or replace.
    PendingDraft,
    Microphone { error: MicError },
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
    KeptInSambler { reason: KeptReason },
    /// "Aucun texte obtenu".
    NoText,
    Cancelled,
    /// The take stopped early; `recovered` tells whether admitted text is in the draft.
    Interrupted { reason: InterruptReason, recovered: bool },
    /// The take could not start.
    Refused { reason: RefusalReason },
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftPart {
    pub status: PartStatus,
    /// Present only for `Admitted` (and for `NeedsReview` when displayable).
    pub text: Option<String>,
}

/// The draft text, returned only by `get_draft`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftView {
    pub meta: DraftMeta,
    pub parts: Vec<DraftPart>,
    /// The rendered full text when the take is complete; `None` otherwise.
    pub full_text: Option<String>,
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
