//! Domain vocabulary shared by every module. No platform code, no I/O.
//!
//! Values that reach the WebView derive `Serialize`; their JSON shape is
//! mirrored by hand in `app/src/ipc/contract.ts`. Keep both in sync.

use serde::{Deserialize, Serialize};

/// Identity of one dictation attempt ("prise"). Monotonic for the process
/// lifetime and never reused: an event carrying an older id is stale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TakeId(pub u64);

/// Dictation language, frozen at the start of each take.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    /// French and English in the same take: Rambler recognizes both, and the
    /// local typography follows the language detected in each result.
    #[default]
    Auto,
    French,
    English,
}

impl Language {
    /// Primary BCP-47 code sent to Rambler in `ProtocolConfig::language`.
    pub fn code(self) -> &'static str {
        match self {
            Language::Auto | Language::French => "fr-FR",
            Language::English => "en-US",
        }
    }

    /// Secondary codes sent in `ProtocolConfig::secondary_languages`.
    pub fn secondary_codes(self) -> Vec<String> {
        match self {
            Language::Auto => vec!["en-US".into()],
            Language::French | Language::English => Vec::new(),
        }
    }
}

/// Text rendering profile, frozen at the start of each take.
/// Both profiles use `RenderPolicy::AsrOnly` on the SDK side; `Natural`
/// additionally applies the deterministic local rules of `crate::text`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderProfile {
    #[default]
    Natural,
    Faithful,
}

/// How the global shortcut drives a take.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Gesture {
    /// Press starts, release stops ("maintenir pour parler").
    #[default]
    Hold,
    /// First press starts, second press stops ("bascule").
    Toggle,
}

/// Which capture device to open for the next take.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MicChoice {
    /// The Windows default capture endpoint, resolved at the start of each take.
    #[default]
    SystemDefault,
    /// A specific endpoint. `id` is the stable local endpoint id; `name` is for display.
    /// If it is missing, report it: never silently open another device.
    Named { id: String, name: String },
}

/// Edge of the configured global shortcut, as observed by the shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeyEdge {
    Pressed,
    Released,
}

/// Where the result of a take may go. Decided once, at the trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DestinationKind {
    /// The external field identified at the trigger (single insertion attempt).
    Field,
    /// Sambler's own draft (button "Dicter dans Sambler" or user choice).
    Sambler,
}

/// Session-level Windows events that interrupt dictation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemEvent {
    /// Workstation locked or session disconnected: stop the mic, disarm, erase the draft.
    SessionLocked,
    SessionUnlocked,
    /// System suspend: stop the mic and disarm.
    Suspending,
    Resumed,
}
