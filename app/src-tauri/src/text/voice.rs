//! French spoken layout commands, on the complete admitted take. Explicit
//! personal rules take precedence; technical spans and replacement text are
//! never candidates. Commands are interpreted once, before prose formatting.

use super::{Piece, prose};
use crate::ipc::VoiceCommandInfo;
use regex::Regex;
use std::{ops::Range, sync::LazyLock};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Layout {
    Line,
    Paragraph,
    OpenQuote,
    CloseQuote,
    OpenParen,
    CloseParen,
}

impl Layout {
    pub(super) fn text(self) -> &'static str {
        match self {
            Self::Line => "\n",
            Self::Paragraph => "\n\n",
            Self::OpenQuote => "«",
            Self::CloseQuote => "»",
            Self::OpenParen => "(",
            Self::CloseParen => ")",
        }
    }

    pub(super) fn is_break(self) -> bool {
        matches!(self, Self::Line | Self::Paragraph)
    }
}

// One catalog for both rendering and Settings help. Break phrases can be
// ordinary prose: say them after a pause (comma/sentence mark in the ASR),
// at the start of a line/take, or followed by a pause (comma). A number
// immediately after the phrase, without a pause, suggests e.g. a table row.
const COMMANDS: &[(&str, Layout, &str)] = &[
    (
        "à la ligne",
        Layout::Line,
        "Saut de ligne, avec une pause ou en début de phrase.",
    ),
    (
        "nouvelle ligne",
        Layout::Line,
        "Saut de ligne, avec une pause ou en début de phrase.",
    ),
    (
        "nouveau paragraphe",
        Layout::Paragraph,
        "Deux sauts de ligne, avec une pause ou en début de phrase.",
    ),
    (
        "ouvrez les guillemets",
        Layout::OpenQuote,
        "Ouvre les guillemets français.",
    ),
    (
        "fermez les guillemets",
        Layout::CloseQuote,
        "Ferme les guillemets français.",
    ),
    (
        "ouvrez la parenthèse",
        Layout::OpenParen,
        "Ouvre une parenthèse.",
    ),
    (
        "fermez la parenthèse",
        Layout::CloseParen,
        "Ferme une parenthèse.",
    ),
];

pub(super) fn catalog() -> Vec<VoiceCommandInfo> {
    COMMANDS
        .iter()
        .map(|&(spoken, layout, description)| VoiceCommandInfo {
            spoken: spoken.into(),
            result: match layout {
                Layout::Line => "↵",
                Layout::Paragraph => "↵↵",
                _ => layout.text(),
            }
            .into(),
            description: description.into(),
        })
        .collect()
}

static SPOKEN: LazyLock<Regex> = LazyLock::new(|| {
    let alternatives: Vec<_> = COMMANDS
        .iter()
        .map(|(spoken, _, _)| {
            let phrase = regex::escape(spoken)
                .replace(' ', r"[\t\p{Zs}]+")
                // Recognize decomposed French accents too, without touching
                // the original text or computing offsets on a normalized copy.
                .replace('à', r"(?:à|a\u{300})")
                .replace('è', r"(?:è|e\u{300})");
            format!("({phrase})")
        })
        .collect();
    Regex::new(&format!(r"(?i)\b(?:{})\b", alternatives.join("|")))
        .expect("static spoken layout commands")
});

fn line_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

fn inline_space(c: char) -> bool {
    c.is_whitespace() && !line_break(c)
}

fn word_joiner(c: char) -> bool {
    matches!(c, '\'' | '’' | 'ʼ' | '-' | '‐' | '‑')
}

// Remove ASR commas and single stops adjacent to the command, not a period
// in a protected abbreviation/URL or punctuation clusters such as ellipses.
fn removable(mark: char, other: Option<char>, single_stop: bool) -> bool {
    (mark == ',' || (single_stop && mark == '.'))
        && !other.is_some_and(|c| matches!(c, '.' | ',' | '!' | '?' | '…'))
}

pub(super) fn apply<'a>(admitted: &'a str, pieces: Vec<Piece<'a>>) -> Vec<Piece<'a>> {
    let mut matches = SPOKEN.captures_iter(admitted).peekable();
    if matches.peek().is_none() {
        return pieces;
    }
    // Use ranges in the admitted text, not per-piece protection scans: a
    // personal substitution might have split a URL or another opaque span.
    let protected: Vec<_> = prose::special_units(admitted)
        .map(|(range, _)| range)
        .collect();
    let intersects = |range: Range<usize>| {
        protected
            .get(protected.partition_point(|p| p.end <= range.start))
            .is_some_and(|p| p.start < range.end)
    };
    let mut out = Vec::new();
    for piece in pieces {
        let Piece::Prose(text) = piece else {
            out.push(piece);
            continue;
        };
        // Every Prose slice is borrowed directly from admitted by substitute.
        let start = text.as_ptr().addr() - admitted.as_ptr().addr();
        let end = start + text.len();
        let mut copied = start;
        while let Some(captures) = matches.next_if(|cap| cap.get(0).unwrap().start() < end) {
            let spoken = captures.get(0).unwrap().range();
            if spoken.start < copied || spoken.end > end || intersects(spoken.clone()) {
                continue;
            }
            let index = captures
                .iter()
                .skip(1)
                .position(|cap| cap.is_some())
                .unwrap();
            let layout = COMMANDS[index].1;
            let before = &admitted[..spoken.start];
            let after = &admitted[spoken.end..];
            if before.ends_with(word_joiner) || after.starts_with(word_joiner) {
                continue;
            }
            let context = before.trim_end_matches(inline_space);
            let following = after.trim_start_matches(inline_space);
            if layout.is_break() {
                let left_boundary = context.is_empty()
                    || (copied != start && context.len() <= copied)
                    || context.ends_with(line_break)
                    || (context.ends_with([',', '.', ';', ':', '!', '?', '…'])
                        && !intersects(context.len() - 1..context.len()));
                if (!left_boundary && !following.starts_with(','))
                    || following.starts_with(char::is_numeric)
                {
                    continue;
                }
            }

            let mut left = admitted[copied..spoken.start].trim_end_matches(inline_space);
            if let Some(mark) = left.chars().next_back() {
                let mark_start = copied + left.len() - mark.len_utf8();
                let preceding = admitted[..mark_start].chars().next_back();
                if removable(mark, preceding, layout.is_break())
                    && !intersects(mark_start..mark_start + mark.len_utf8())
                {
                    left = admitted[copied..mark_start].trim_end_matches(inline_space);
                }
            }
            let mut right = admitted[spoken.end..end].trim_start_matches(inline_space);
            let mut chars = right.chars();
            if let Some(mark) = chars.next() {
                let mark_start = end - right.len();
                if removable(mark, chars.next(), true)
                    && !intersects(mark_start..mark_start + mark.len_utf8())
                {
                    right = right[mark.len_utf8()..].trim_start_matches(inline_space);
                }
            }
            if !left.is_empty() {
                out.push(Piece::Prose(left));
            }
            out.push(Piece::Layout(layout));
            copied = end - right.len();
        }
        if copied < end {
            out.push(Piece::Prose(&admitted[copied..end]));
        }
    }
    out
}
