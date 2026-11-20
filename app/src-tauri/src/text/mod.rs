//! Deterministic local text rendering (profiles Naturel / Fidèle, personal
//! lexicon and expressions). Owned by the "text" work package. Pure Rust, no I/O.
//!
//! Contract: `render` is applied ONCE to the complete admitted text of a take
//! (after `rambler_audio::join_segment_texts`), never to provisional text and
//! never across a gap. It must preserve meaning: names, negations, numbers,
//! amounts, URLs, e-mail addresses, file names and intended repetitions.

use crate::model::{Language, RenderProfile};
use crate::settings::{Settings, TextOptions};
use serde::{Deserialize, Serialize};

/// Rules frozen at the start of a take. Later settings changes apply to the next take.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextRules {
    pub language: Language,
    pub profile: RenderProfile,
    pub options: TextOptions,
    pub lexicon: Vec<LexiconEntry>,
    pub expressions: Vec<Expression>,
}

impl TextRules {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            language: settings.language,
            profile: settings.profile,
            options: settings.text.clone(),
            lexicon: Vec::new(),
            expressions: Vec::new(),
        }
    }
}

/// Whole-word replacement: what Rambler writes -> what the person wants.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LexiconEntry {
    pub id: String,
    pub heard: String,
    pub written: String,
    pub enabled: bool,
    /// Substitutions in Faithful need a separate opt-in.
    pub apply_in_faithful: bool,
}

/// Explicit trigger phrase -> literal text (may contain line breaks).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Expression {
    pub id: String,
    pub trigger: String,
    pub text: String,
    pub enabled: bool,
}

mod personal;
mod prose;
#[cfg(test)]
mod tests;

pub use personal::{RuleConflict, RuleConflictReason, RuleKind, RuleRef, validate_rules};

use std::borrow::Cow;

/// User-provided replacement text is opaque to all subsequent passes, even
/// when it resembles another trigger or contains nonstandard typography.
enum Piece<'a> {
    Prose(&'a str),
    Literal(Cow<'a, str>),
}

fn lf(text: &str) -> Cow<'_, str> {
    if text.contains('\r') {
        Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(text)
    }
}

/// Render the complete admitted text of a take with frozen rules.
pub fn render(admitted: &str, rules: &TextRules) -> String {
    let natural = rules.profile == RenderProfile::Natural;
    // In Faithful the admitted bytes (including line endings) are authoritative.
    let admitted = if natural {
        lf(admitted)
    } else {
        admitted.into()
    };
    let pieces = personal::substitute(&admitted, rules);
    if natural
        && (rules.options.light_punctuation
            || rules.options.typographic_spaces
            || rules.options.sentence_case)
    {
        prose::format(&pieces, rules, admitted.len())
    } else {
        let mut output = String::with_capacity(admitted.len());
        for piece in pieces {
            output.push_str(match &piece {
                Piece::Prose(text) => text,
                Piece::Literal(text) => text,
            });
        }
        output
    }
}

/// Use exactly the same local rules as a take, without opening the microphone.
pub fn preview(sample: &str, rules: &TextRules) -> String {
    render(sample, rules)
}
