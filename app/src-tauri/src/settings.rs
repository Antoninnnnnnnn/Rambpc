//! Ordinary (non-sensitive) preferences, with the dossier's default values
//! (docs/windows/03-produit-et-priorites.md, "Réglages et valeurs de départ").
//!
//! Stored as versioned JSON by `crate::storage`, written atomically. Consent,
//! Rambler access values, lexicon and expressions are NOT here: they are
//! protected separately with DPAPI.
//!
//! A change applies to the next take; withdrawing consent applies immediately.
//! Mirrored in `app/src/ipc/contract.ts` (`Settings`).

use crate::model::{Gesture, Language, MicChoice, RenderProfile};
use serde::{Deserialize, Serialize};

pub const SETTINGS_VERSION: u32 = 1;
pub const DEFAULT_SHORTCUT: &str = "Ctrl+Shift+F2";
/// Allowed draft retention values, in minutes.
pub const DRAFT_MINUTES_CHOICES: [u8; 3] = [1, 5, 10];
/// Allowed history retention values, in days.
pub const HISTORY_DAYS_CHOICES: [u16; 3] = [1, 7, 30];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub version: u32,
    pub language: Language,
    pub microphone: MicChoice,
    pub gesture: Gesture,
    /// Tauri accelerator syntax, e.g. "Ctrl+Shift+F2".
    pub shortcut: String,
    /// While a take is active, Escape cancels it (global, only during the take).
    pub cancel_with_escape: bool,
    pub profile: RenderProfile,
    pub text: TextOptions,
    /// "Toujours garder le texte dans Sambler": never insert, always draft.
    pub always_keep_in_sambler: bool,
    pub indicator: IndicatorOptions,
    /// Draft kept in RAM for this many minutes (1, 5 or 10).
    pub draft_minutes: u8,
    pub history: HistoryOptions,
    /// Installed edition only; ignored by the portable edition.
    pub launch_at_startup: bool,
    /// How text reaches fields without an atomic editor API (browsers,
    /// Electron apps, Outlook…). Native Edit/RichEdit and Word keep their
    /// atomic paths whatever this says.
    pub insertion: InsertionMethod,
    /// Which field receives the text at the end of a take.
    pub insertion_target: InsertionTarget,
}

/// Where the text goes when the take ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InsertionTarget {
    /// The field where the take started, provided it has the focus again at
    /// the end (leaving it and coming back during the take is fine).
    /// Otherwise the text stays in Sambler.
    #[default]
    OriginalField,
    /// Whatever eligible field has the focus when the take ends (Wispr-like).
    /// A password field or an unrecognized field keeps the text in Sambler.
    CursorAtEnd,
}

/// Insertion for fields without an atomic editor API.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InsertionMethod {
    /// One paste: the clipboard is saved, the text pasted with Ctrl+V, then the
    /// previous clipboard restored (excluded from Win+V history and cloud sync).
    /// Instant, multi-line capable.
    #[default]
    Paste,
    /// Paced Unicode typing, never touches the clipboard; single-line only.
    Typing,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            language: Language::French,
            microphone: MicChoice::SystemDefault,
            gesture: Gesture::Hold,
            shortcut: DEFAULT_SHORTCUT.into(),
            cancel_with_escape: true,
            profile: RenderProfile::Natural,
            text: TextOptions::default(),
            always_keep_in_sambler: false,
            indicator: IndicatorOptions::default(),
            draft_minutes: 10,
            history: HistoryOptions::default(),
            launch_at_startup: false,
            insertion: InsertionMethod::Paste,
            insertion_target: InsertionTarget::OriginalField,
        }
    }
}

/// Light rules of the Natural profile. All on in Natural, all ignored in Faithful.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TextOptions {
    pub light_punctuation: bool,
    pub typographic_spaces: bool,
    pub sentence_case: bool,
}

impl Default for TextOptions {
    fn default() -> Self {
        Self {
            light_punctuation: true,
            typographic_spaces: true,
            sentence_case: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IndicatorPosition {
    #[default]
    BottomCenter,
    TopCenter,
    NearCursor,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct IndicatorOptions {
    pub sound: bool,
    pub position: IndicatorPosition,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HistoryOptions {
    /// Off at installation, including in the portable edition.
    pub enabled: bool,
    /// Applied when enabled: 1, 7 or 30 days.
    pub retention_days: u16,
}

impl Default for HistoryOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            retention_days: 7,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    #[error("invalid draft retention")]
    DraftMinutes,
    #[error("invalid history retention")]
    HistoryDays,
    #[error("invalid shortcut")]
    Shortcut,
}

impl Settings {
    /// Reject values outside the documented choices. The shortcut is only
    /// checked for emptiness here; the shell validates registration.
    pub fn validate(&self) -> Result<(), SettingsError> {
        if !DRAFT_MINUTES_CHOICES.contains(&self.draft_minutes) {
            return Err(SettingsError::DraftMinutes);
        }
        if !HISTORY_DAYS_CHOICES.contains(&self.history.retention_days) {
            return Err(SettingsError::HistoryDays);
        }
        if self.shortcut.trim().is_empty() {
            return Err(SettingsError::Shortcut);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_dossier() {
        let s = Settings::default();
        assert_eq!(s.language, Language::French);
        assert_eq!(s.gesture, Gesture::Hold);
        assert_eq!(s.shortcut, "Ctrl+Shift+F2");
        assert_eq!(s.profile, RenderProfile::Natural);
        assert_eq!(s.draft_minutes, 10);
        assert!(!s.history.enabled);
        assert_eq!(s.history.retention_days, 7);
        assert!(!s.indicator.sound);
        assert!(!s.launch_at_startup);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn missing_fields_take_defaults() {
        let s: Settings = serde_json::from_str(r#"{"version":1,"gesture":"toggle"}"#).unwrap();
        assert_eq!(s.gesture, Gesture::Toggle);
        assert_eq!(s.shortcut, DEFAULT_SHORTCUT);
    }
}
