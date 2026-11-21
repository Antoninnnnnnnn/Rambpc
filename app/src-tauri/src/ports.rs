//! Boundaries between the dictation controller and Windows.
//!
//! `crate::platform` provides the real implementations. Test doubles live only
//! under `#[cfg(test)]` (or in `tests/support`): the shipped binary contains no
//! simulated microphone, recognizer, canned transcript or demo mode.
//!
//! Privacy rule for every implementation: errors, logs and `Debug` output never
//! contain audio, dictated text, document text, window titles, user paths or
//! access values.

use crate::{
    model::{MicChoice, SystemEvent},
    settings::AppIdentity,
};
use serde::Serialize;
use std::any::Any;
use std::sync::Arc;

// ---------------------------------------------------------------- microphone

/// Sample rate of the normalized PCM delivered to a [`PcmSink`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum PcmRate {
    Hz16000,
    Hz48000,
}

impl PcmRate {
    pub fn hz(self) -> u32 {
        match self {
            PcmRate::Hz16000 => 16_000,
            PcmRate::Hz48000 => 48_000,
        }
    }
}

/// Receives normalized **mono PCM16** samples at the negotiated [`PcmRate`].
///
/// `push` runs on the audio driver thread: it must be bounded, never block,
/// never wait for capacity, never touch disk, network or UI. Exactly one of
/// `finish` / `abort` is called, once, after the last `push` has returned.
pub trait PcmSink: Send + 'static {
    fn push(&mut self, samples: &[i16]);
    /// Normal end of capture: flush any buffered tail and declare EOF.
    fn finish(self: Box<Self>);
    /// Abandon capture (cancel, fault, watchdog).
    fn abort(self: Box<Self>);
}

/// Builds the sink once the device format is known (before streaming starts).
pub type SinkFactory = Box<dyn FnOnce(PcmRate) -> Box<dyn PcmSink> + Send>;
/// Reports an asynchronous device fault after a successful open. Called at most once.
pub type FaultCallback = Box<dyn FnOnce(MicError) + Send>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MicInfo {
    /// Stable local endpoint id (opaque).
    pub id: String,
    /// Friendly name, for display only.
    pub name: String,
    pub is_default: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MicError {
    #[error("no capture device")]
    NoInput,
    #[error("chosen microphone not found")]
    NotFound,
    #[error("microphone access denied by Windows privacy settings")]
    AccessDenied,
    /// `format` describes the device format only (e.g. "44100 Hz, 6 ch, f32").
    #[error("unsupported microphone format: {format}")]
    UnsupportedFormat { format: String },
    #[error("microphone did not start in time")]
    OpenTimeout,
    #[error("microphone disconnected or invalidated")]
    Disconnected,
    #[error("microphone error")]
    Other,
}

/// A running capture. Dropping it without `stop` behaves like `abort`.
pub trait MicStream: Send {
    /// Stop the device stream, join the driver callback, then call `sink.finish()`.
    fn stop(self: Box<Self>);
    /// Stop the device stream, join the driver callback, then call `sink.abort()`.
    fn abort(self: Box<Self>);
}

pub trait Microphone: Send + Sync + 'static {
    /// Capture endpoints only (never render/loopback endpoints).
    fn inputs(&self) -> Result<Vec<MicInfo>, MicError>;

    /// Resolve `choice`, negotiate a shared-mode format (prefer 48 or 16 kHz,
    /// mono if available), build the sink with the resulting rate, and start.
    /// Converts any supported device format (f32/i16/i32/u16, 1..n channels,
    /// 44.1 kHz) to mono PCM16 at 16 or 48 kHz with a stateful converter.
    /// Returns once the device is actually running, bounded by an internal
    /// open timeout (a device that opens late is closed, not leaked).
    fn open(
        &self,
        choice: &MicChoice,
        sink_for: SinkFactory,
        on_fault: FaultCallback,
    ) -> Result<Box<dyn MicStream>, MicError>;
}

// ---------------------------------------------------------------- target field

/// Opaque native identity of the field focused at the trigger (window, owning
/// thread/process, UIA element, caret). Kept in memory for one take only.
#[derive(Clone)]
pub struct TargetToken(pub Arc<dyn Any + Send + Sync>);

impl std::fmt::Debug for TargetToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TargetToken(..)")
    }
}

/// Coarse family of the target editor, for compatibility notes and insertion
/// strategy. Never sent to Rambler.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppKind {
    Notepad,
    Word,
    Outlook,
    Edge,
    Chrome,
    VsCode,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TargetInfo {
    pub app: AppKind,
    /// Executable identity for local counters and rendering, when Windows supplies it.
    pub app_identity: Option<AppIdentity>,
    /// Whether the field accepts several lines, when known.
    pub multiline: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UncertainReason {
    NoFocusedField,
    ReadOnly,
    Selection,
    Elevated,
    Terminal,
    RemoteDesktop,
    Unresponsive,
    Unsupported,
}

#[derive(Clone, Debug)]
pub enum TargetVerdict {
    /// Editable, enabled, not a password, single empty selection (caret),
    /// compatible integrity level: one insertion may be armed.
    Eligible { token: TargetToken, info: TargetInfo },
    /// Recognized password field or secure desktop: the microphone must not open.
    Protected,
    /// The user may choose to dictate into Sambler's draft instead.
    Uncertain { reason: UncertainReason },
    /// Sambler's own window has the focus.
    Sambler,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetChange {
    FocusMoved,
    SelectionChanged,
    WindowGone,
    /// Focus moved to a password field or the secure desktop: also stop the mic.
    Protected,
    Unknown,
}

/// Cancels a [`TargetProbe::watch`] when dropped.
pub type WatchGuard = Box<dyn Send>;

pub trait TargetProbe: Send + Sync + 'static {
    /// Snapshot the focused field. UIA runs on a dedicated COM MTA thread with a
    /// bounded wait (an unresponsive editor yields `Uncertain{Unresponsive}`).
    /// Reads no document text and no clipboard content.
    fn capture(&self) -> TargetVerdict;

    /// Watch the target during the take. `on_change` is called at most once,
    /// from any thread, on the first observed focus move, selection/caret
    /// change or window change (even if focus later returns).
    fn watch(&self, token: &TargetToken, on_change: Box<dyn FnOnce(TargetChange) + Send>)
    -> WatchGuard;

    /// Final check of window, element, protection, caret and selection.
    fn revalidate(&self, token: &TargetToken) -> Result<(), TargetChange>;
}

// ---------------------------------------------------------------- insertion

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotWrittenReason {
    TargetChanged,
    /// Modifiers still held after the bounded wait.
    ModifiersHeld,
    /// Text shape not qualified for this field (e.g. line breaks, tab, too long).
    Unsupported,
    /// Windows accepted zero events (UIPI, foreground lock, ...).
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum InsertOutcome {
    /// Every event was accepted by Windows (queue acceptance, not display proof).
    Delivered,
    /// Only part of the events were accepted: "Vérifiez le champ".
    Partial,
    /// Nothing was written; the text stays in Sambler.
    NotWritten { reason: NotWrittenReason },
}

pub trait Inserter: Send + Sync + 'static {
    /// Exactly one attempt, never retried. Implementations wait (bounded) for
    /// modifier keys to be released, revalidate the target, then write the whole
    /// text in one bounded batch. Never sends Enter/Tab keystrokes to create
    /// lines: multi-line text needs a qualified literal path or is refused as
    /// `Unsupported`.
    fn insert_once(&self, token: &TargetToken, text: &str) -> InsertOutcome;
}

// ---------------------------------------------------------------- clipboard

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "kebab-case")]
pub enum ClipboardError {
    #[error("clipboard busy")]
    Busy,
    #[error("clipboard history exclusion unavailable")]
    ExclusionFailed,
    #[error("clipboard error")]
    Other,
}

pub trait Clipboard: Send + Sync + 'static {
    /// Only on an explicit "Copier": in one clipboard opening, register
    /// `ExcludeClipboardContentFromMonitorProcessing` then set CF_UNICODETEXT.
    /// If the exclusion cannot be set, write nothing and return an error.
    fn copy_private(&self, text: &str) -> Result<(), ClipboardError>;
}

// ---------------------------------------------------------------- DPAPI

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "kebab-case")]
pub enum ProtectError {
    #[error("data protection failed")]
    Protect,
    /// Typically a profile moved to another Windows account or PC.
    #[error("protected data unreadable under this account")]
    Unreadable,
}

/// User-scope data protection (Windows DPAPI `CryptProtectData`).
pub trait Protector: Send + Sync + 'static {
    fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, ProtectError>;
    fn unprotect(&self, blob: &[u8]) -> Result<zeroize::Zeroizing<Vec<u8>>, ProtectError>;
}

// ---------------------------------------------------------------- session events

/// Keeps a subscription alive; dropping it unsubscribes.
pub type Subscription = Box<dyn Send>;

pub trait SystemEvents: Send + Sync + 'static {
    /// Lock/unlock (WTS session notifications) and suspend/resume
    /// (power broadcasts), delivered from a top-level hidden window thread.
    fn subscribe(&self, on_event: Box<dyn Fn(SystemEvent) + Send + Sync>) -> Subscription;
}
