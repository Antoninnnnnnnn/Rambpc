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
    /// Theme of the main window and the floating bar. Theme ids are defined by
    /// the web UI; the native side only stores and forwards them.
    pub appearance: AppearanceSettings,
    /// Per-application overrides of the rendering profile and light rules.
    /// At most 50, unique by `app.id` (the last one wins when normalizing).
    pub app_styles: Vec<AppStyle>,
}

pub const MAX_APP_STYLES: usize = 50;

/// A target application as recorded locally (never sent to Rambler).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppIdentity {
    /// Lowercased executable name without extension, e.g. "winword".
    pub id: String,
    /// Display name, e.g. "Word".
    pub name: String,
}

/// Fields left `None` keep the global value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStyle {
    pub app: AppIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<RenderProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextOptionsPatch>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TextOptionsPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_punctuation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typographic_spaces: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sentence_case: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_commands: Option<bool>,
}

pub const DEFAULT_THEME: &str = "verre-aurore";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppearanceSettings {
    /// `^[a-z0-9-]{1,32}$`; anything else is replaced by [`DEFAULT_THEME`].
    pub theme: String,
    pub color_scheme: ColorSchemeChoice,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: DEFAULT_THEME.into(),
            color_scheme: ColorSchemeChoice::System,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ColorSchemeChoice {
    /// Follow the operating system's light/dark preference.
    #[default]
    System,
    Light,
    Dark,
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
            appearance: AppearanceSettings::default(),
            app_styles: Vec::new(),
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
    /// Spoken layout commands (« à la ligne », « ouvrez les guillemets »…),
    /// Naturel only. Field-level default `false`: settings files written before
    /// 0.11 keep today's behaviour; new settings get `true` from `Default`.
    #[serde(default)]
    pub voice_commands: bool,
}

impl Default for TextOptions {
    fn default() -> Self {
        Self {
            light_punctuation: true,
            typographic_spaces: true,
            sentence_case: true,
            voice_commands: true,
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
    /// Repair values that must never reject a whole settings file: an invalid
    /// theme id falls back to the default theme.
    pub fn normalize(&mut self) {
        let theme = &self.appearance.theme;
        let valid = (1..=32).contains(&theme.len())
            && theme
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !valid {
            self.appearance.theme = DEFAULT_THEME.into();
        }
        // Unique by app id (last wins), at most MAX_APP_STYLES, order kept.
        let mut seen = std::collections::HashSet::new();
        let mut unique: Vec<AppStyle> = self
            .app_styles
            .drain(..)
            .rev()
            .filter(|style| !style.app.id.is_empty() && seen.insert(style.app.id.clone()))
            .collect();
        unique.reverse();
        unique.truncate(MAX_APP_STYLES);
        self.app_styles = unique;
    }

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
    fn invalid_theme_falls_back_to_the_default() {
        let mut s: Settings =
            serde_json::from_str(r#"{"appearance":{"theme":"Bad Theme!","colorScheme":"dark"}}"#)
                .unwrap();
        s.normalize();
        assert_eq!(s.appearance.theme, DEFAULT_THEME);
        assert_eq!(s.appearance.color_scheme, ColorSchemeChoice::Dark);
        let mut ok: Settings =
            serde_json::from_str(r#"{"appearance":{"theme":"papier-chaud"}}"#).unwrap();
        ok.normalize();
        assert_eq!(ok.appearance.theme, "papier-chaud");
        assert_eq!(Settings::default().appearance.theme, "verre-aurore");
    }

    #[test]
    fn missing_fields_take_defaults() {
        let s: Settings = serde_json::from_str(r#"{"version":1,"gesture":"toggle"}"#).unwrap();
        assert_eq!(s.gesture, Gesture::Toggle);
        assert_eq!(s.shortcut, DEFAULT_SHORTCUT);
    }
}
