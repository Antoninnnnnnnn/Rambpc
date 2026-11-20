//! Matching and validation share exactly the same keys. Offsets always refer
//! to the original admitted UTF-8, not to a normalized copy of the dictation.

use super::{Expression, LexiconEntry, Piece, RenderProfile, TextRules, lf};
use icu_normalizer::ComposingNormalizerBorrowed;
use regex::Regex;
use serde::Serialize;
use std::{borrow::Cow, collections::BTreeMap, ops::Range, sync::LazyLock};
use unicase::UniCase;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleKind {
    Lexicon,
    Expression,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleRef {
    pub kind: RuleKind,
    pub id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleConflictReason {
    EmptyMatch,
    EmptyReplacement,
    NoMatchableWords,
    MultilineMatch,
    DuplicateHeard,
    DuplicateTrigger,
    TriggerShadowsHeard,
    /// `rule` is the shorter trigger; `related` contains it at word boundaries.
    TriggerOverlap,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleConflict {
    pub reason: RuleConflictReason,
    pub rule: RuleRef,
    pub related: Option<RuleRef>,
}

fn line_break(c: char) -> bool {
    matches!(c, '\r' | '\n' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

// Word-internal apostrophes remain part of a whole word; straight, curly
// and modifier-letter forms are interchangeable. Other punctuation separates
// expression words, including punctuation UAX #29 might otherwise join (a.b).
static EXPRESSION_PUNCTUATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\p{P}&&[^'’ʼ]]+").expect("static punctuation regex"));

struct Token {
    span: Range<usize>,
    key: String,
    word: bool,
}

fn tokenize(text: &str, kind: RuleKind) -> Vec<Token> {
    let mut tokens: Vec<Token> = Vec::new();
    let mut scan = |base, slice: &str| {
        for (i, part) in slice.split_word_bound_indices() {
            let start = base + i;
            let end = start + part.len();
            let word = part.chars().any(|c| c.is_alphanumeric() && c != 'ʼ');
            let key = if part.chars().all(char::is_whitespace) && !part.contains(line_break) {
                if kind == RuleKind::Expression {
                    continue;
                }
                if let Some(last) = tokens
                    .last_mut()
                    .filter(|t| t.key == " " && t.span.end == start)
                {
                    last.span.end = end;
                    continue;
                }
                " ".into()
            } else if !word && kind == RuleKind::Expression && part.chars().all(is_apostrophe) {
                // Treat standalone quotation marks like other punctuation.
                continue;
            } else if part.is_ascii() {
                part.to_ascii_lowercase()
            } else {
                let nfc = ComposingNormalizerBorrowed::new_nfc();
                let part = part.replace(['’', 'ʼ'], "'");
                let folded = UniCase::new(nfc.normalize(&part)).to_folded_case();
                nfc.normalize(&folded).into_owned()
            };
            tokens.push(Token {
                span: start..end,
                key,
                word,
            });
        }
    };
    if kind == RuleKind::Expression {
        let mut start = 0;
        for m in EXPRESSION_PUNCTUATION.find_iter(text) {
            scan(start, &text[start..m.start()]);
            start = m.end();
        }
        scan(start, &text[start..]);
    } else {
        scan(0, text);
    }
    tokens
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '’' | 'ʼ')
}

fn match_key(text: &str, kind: RuleKind) -> Result<Vec<String>, RuleConflictReason> {
    if text.trim().is_empty() {
        return Err(RuleConflictReason::EmptyMatch);
    }
    if text.contains(line_break) {
        return Err(RuleConflictReason::MultilineMatch);
    }
    let tokens = tokenize(text.trim(), kind);
    if !tokens.iter().any(|t| t.word) {
        return Err(RuleConflictReason::NoMatchableWords);
    }
    Ok(tokens.into_iter().map(|t| t.key).collect())
}

#[derive(Default)]
struct Matcher<'a>(BTreeMap<String, Vec<Pattern<'a>>>);

struct Pattern<'a> {
    rest: Vec<String>,
    literal: &'a str,
}

impl<'a> Matcher<'a> {
    fn add(&mut self, name: &str, literal: &'a str, kind: RuleKind) {
        if literal.trim().is_empty() {
            return;
        }
        if let Ok(key) = match_key(name, kind) {
            let mut key = key.into_iter();
            if let Some(first) = key.next() {
                self.0.entry(first).or_default().push(Pattern {
                    rest: key.collect(),
                    literal,
                });
            }
        }
    }

    fn apply(&self, text: &'a str, kind: RuleKind, out: &mut Vec<Piece<'a>>) {
        if text.is_empty() {
            return;
        }
        if self.0.is_empty() {
            out.push(Piece::Prose(text));
            return;
        }
        let tokens = tokenize(text, kind);
        let mut copied = 0;
        let mut i = 0;
        while let Some(token) = tokens.get(i) {
            let mut best = None;
            if let Some(patterns) = self.0.get(&token.key) {
                for pattern in patterns {
                    let n = pattern.rest.len();
                    // Keep configuration order for equal-length conflicts.
                    if best.is_none_or(|(len, _)| n > len)
                        && tokens.get(i + 1..i + 1 + n).is_some_and(|next| {
                            next.iter().zip(&pattern.rest).all(|(t, k)| t.key == *k)
                        })
                    {
                        best = Some((n, pattern.literal));
                    }
                }
            }
            if let Some((n, literal)) = best {
                if kind == RuleKind::Expression && i == 0 && n + 1 == tokens.len() {
                    // A standalone trigger is a command to insert precisely
                    // the payload, not also its ASR-generated terminal stop.
                    // Explicit line breaks and non-punctuation symbols remain
                    // tokens, so this cannot swallow them.
                    out.push(Piece::Literal(lf(literal)));
                    return;
                }
                if copied < token.span.start {
                    out.push(Piece::Prose(&text[copied..token.span.start]));
                }
                copied = tokens[i + n].span.end;
                i += n + 1;
                out.push(Piece::Literal(if kind == RuleKind::Expression {
                    lf(literal)
                } else {
                    Cow::Borrowed(literal)
                }));
            } else {
                i += 1;
            }
        }
        if copied < text.len() {
            out.push(Piece::Prose(&text[copied..]));
        }
    }
}

pub(super) fn substitute<'a>(admitted: &'a str, rules: &'a TextRules) -> Vec<Piece<'a>> {
    let natural = rules.profile == RenderProfile::Natural;
    let mut lexicon = Matcher::default();
    for entry in &rules.lexicon {
        if entry.enabled && (natural || entry.apply_in_faithful) {
            lexicon.add(&entry.heard, &entry.written, RuleKind::Lexicon);
        }
    }
    let mut output = Vec::new();
    if !natural || !rules.expressions.iter().any(|e| e.enabled) {
        lexicon.apply(admitted, RuleKind::Lexicon, &mut output);
        return output;
    }
    let mut expressions = Matcher::default();
    for expression in rules.expressions.iter().filter(|e| e.enabled) {
        expressions.add(&expression.trigger, &expression.text, RuleKind::Expression);
    }
    let mut expanded = Vec::new();
    expressions.apply(admitted, RuleKind::Expression, &mut expanded);
    for piece in expanded {
        match piece {
            Piece::Prose(text) => lexicon.apply(text, RuleKind::Lexicon, &mut output),
            literal => output.push(literal),
        }
    }
    output
}

struct Checked<'a> {
    rule: RuleRef,
    source: &'a str,
    key: Vec<String>,
    usable: bool,
}

fn check<'a>(
    kind: RuleKind,
    id: &str,
    source: &'a str,
    literal: &str,
    conflicts: &mut Vec<RuleConflict>,
) -> Option<Checked<'a>> {
    let rule = RuleRef {
        kind,
        id: id.into(),
    };
    let key = match_key(source, kind);
    let usable = !literal.trim().is_empty();
    for reason in key
        .as_ref()
        .err()
        .copied()
        .into_iter()
        .chain((!usable).then_some(RuleConflictReason::EmptyReplacement))
    {
        conflicts.push(RuleConflict {
            reason,
            rule: rule.clone(),
            related: None,
        });
    }
    key.ok().map(|key| Checked {
        rule,
        source,
        key,
        usable,
    })
}

fn pair(
    conflicts: &mut Vec<RuleConflict>,
    reason: RuleConflictReason,
    rule: &Checked<'_>,
    related: &Checked<'_>,
) {
    conflicts.push(RuleConflict {
        reason,
        rule: rule.rule.clone(),
        related: Some(related.rule.clone()),
    });
}

/// Diagnose enabled personal rules using the same keys as rendering. Disabled
/// entries are ignored. For invalid sets, rendering skips unusable rules and
/// resolves competing matches left-to-right, longest first, then settings order.
pub fn validate_rules(lexicon: &[LexiconEntry], expressions: &[Expression]) -> Vec<RuleConflict> {
    use RuleConflictReason as Reason;

    let mut conflicts = Vec::new();
    let heard: Vec<_> = lexicon
        .iter()
        .filter(|e| e.enabled)
        .filter_map(|e| {
            check(
                RuleKind::Lexicon,
                &e.id,
                &e.heard,
                &e.written,
                &mut conflicts,
            )
        })
        .collect();
    let triggers: Vec<_> = expressions
        .iter()
        .filter(|e| e.enabled)
        .filter_map(|e| {
            check(
                RuleKind::Expression,
                &e.id,
                &e.trigger,
                &e.text,
                &mut conflicts,
            )
        })
        .collect();
    for (i, a) in heard.iter().enumerate() {
        for b in &heard[i + 1..] {
            if a.key == b.key {
                pair(&mut conflicts, Reason::DuplicateHeard, a, b);
            }
        }
        if let Ok(key) = match_key(a.source, RuleKind::Expression) {
            for trigger in triggers
                .iter()
                .filter(|t| t.usable && key.windows(t.key.len()).any(|w| w == t.key))
            {
                pair(&mut conflicts, Reason::TriggerShadowsHeard, trigger, a);
            }
        }
    }
    for (i, a) in triggers.iter().enumerate() {
        for b in &triggers[i + 1..] {
            if a.key == b.key {
                pair(&mut conflicts, Reason::DuplicateTrigger, a, b);
            } else {
                let (short, long) = if a.key.len() < b.key.len() {
                    (a, b)
                } else {
                    (b, a)
                };
                if long.key.windows(short.key.len()).any(|w| w == short.key) {
                    pair(&mut conflicts, Reason::TriggerOverlap, short, long);
                }
            }
        }
    }
    conflicts
}
