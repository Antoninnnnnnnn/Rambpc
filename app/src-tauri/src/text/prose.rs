//! Conservative formatting of ordinary prose, not a parser or a rewriter.

use super::{Language, Piece, TextOptions, TextRules};
use regex::Regex;
use std::sync::LazyLock;
use unicode_segmentation::UnicodeSegmentation;

// First recognize literal-looking spans. A dotted run with no separator
// could be a filename or a domain, so do not guess that it is two sentences.
// The numeric alternative accepts ASCII/NBSP/NNBSP grouping, French and
// English clocks, and ambiguous SDK joins such as `3. 14` or `3... 14`.
static SPECIAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?x)
        (?:
            (?s:```.*?```) | `[^`\r\n]+`
          | \x22(?:(?i:[A-Z]:)[\\/]|\\\\|\.{1,2}[\\/]|~/|/)[^\x22\r\n]*\x22
          | (?i:\b[A-Z][A-Z0-9+.-]*://|\bwww\.|\bmailto:)[^\s<>«»“”\x22]+
          | (?i:\blocalhost):[0-9]+(?:[/?][^\s<>«»“”\x22]*)?
          | (?:(?i:\b[A-Z]:)[\\/]|\\\\|\.{1,2}[\\/]|~/)[^\s<>«»“”\x22]*
          | /[^\s<>«»“”\x22]+
          | \b[\p{L}\p{M}\p{N}_.+%'-]+@[\p{L}\p{M}\p{N}-]+(?:\.[\p{L}\p{M}\p{N}-]+)+
          | \b[\p{L}\p{M}\p{N}_.-]+[\\/][^\s<>«»“”\x22]+
          | \b[A-Za-z_]\w*(?:::\w+)+
          | \b[A-Za-z_]\w*\([^()\r\n]*\)
          | \b[A-Za-z_]\w*(?:[-+*/]=|[=<>]=?|\+\+|--)\w*
          | \b[A-Za-z_]\w*[+*/]\w+\b
          | (?:--[A-Za-z]|\#[\p{L}_]|@[\p{L}_])[\w-]*
        )
        | (?P<dotted>
            (?:\b[\p{L}_][\w-]*(?:\.[\w-]+)+
              |\b\p{Nd}[\w-]*(?:\.[\w-]+)*\.[\p{L}_][\w-]*(?:\.[\w-]+)*
              |\.[A-Za-z_-][\w-]*(?:\.[\w-]+)*)
            (?::[0-9]+)? (?:\?[\p{L}\p{N}_%][^\s<>«»“”\x22]*|\.?)
        )
        | (?:
            \b(?:[A-Za-z\d]+_(?:\w+)?|_[A-Za-z\d_]\w*|[a-z][a-z0-9]*[A-Z][A-Za-z0-9]*)\b
        )
        | (?:
            (?:[+−-][\x20\u00A0\u202F]*)?
            (?:(?:\p{Sc}|(?i:\b(?:EUR|USD|GBP|CAD|CHF))) [\x20\u00A0\u202F]*)?
            \b\p{Nd}+
            (?:[\x20\u00A0\u202F]+\p{Nd}+
              |[\x20\u00A0\u202F]*(?:[.,:…/−-][\x20\x{A0}\x{202F}]*)+[\x20\u00A0\u202F]*\p{Nd}+)*
            (?:[\x20\u00A0\u202F]*
               (?:[hH]\b(?:[\x20\u00A0\u202F]*\p{Nd}+)?
                 |(?i:[ap] \.? [\x20\u00A0\u202F]* m\b\.?)
                 |\p{Sc}|%|(?i:(?:EUR|USD|GBP|CAD|CHF)\b)))?
            (?:[\p{L}\p{M}_][\p{L}\p{M}\p{N}_]*)?
        )
        | (?P<abbreviation>
            \b(?i:(?:mr|mrs|ms|dr|prof|st|mt|mme|mlle|m|sr|jr|vs|etc|cf|env|réf|no|e\.g|i\.e|p\.[\x20\u00A0\u202F]*ex)\.)
          | \b[A-Z]\.
        )"#,
    )
    .expect("static prose regex")
});

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Plain,
    Space,
    Opaque,
    Abbreviation,
    Literal,
}

#[derive(Clone, Copy)]
struct Unit<'a> {
    text: &'a str,
    kind: Kind,
}

impl Unit<'_> {
    fn mark(self) -> Option<char> {
        (self.kind == Kind::Plain).then_some(())?;
        match self.text {
            "." | "," | ";" | ":" | "!" | "?" | "«" | "»" | "…" => self.first(),
            _ => None,
        }
    }

    fn first(self) -> Option<char> {
        self.text.chars().next()
    }

    fn last(self) -> Option<char> {
        self.text.chars().next_back()
    }
}

fn inline_space(text: &str) -> bool {
    text.chars()
        .all(|c| matches!(c, ' ' | '\u{a0}' | '\u{202f}'))
}

fn push_plain<'a>(units: &mut Vec<Unit<'a>>, text: &'a str, light: bool) {
    let mut graphemes = text.grapheme_indices(true).peekable();
    while let Some((start, grapheme)) = graphemes.next() {
        if inline_space(grapheme) {
            let mut end = start + grapheme.len();
            while let Some(&(i, g)) = graphemes.peek().filter(|(_, g)| inline_space(g)) {
                end = i + g.len();
                graphemes.next();
            }
            units.push(Unit {
                kind: Kind::Space,
                text: &text[start..end],
            });
        } else if !(light && grapheme == "," && units.last().is_some_and(|u| u.mark() == Some(',')))
        {
            units.push(Unit {
                kind: Kind::Plain,
                text: grapheme,
            });
        }
    }
}

fn units<'a>(pieces: &'a [Piece<'_>], light: bool) -> Vec<Unit<'a>> {
    let mut units = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Prose(text) => {
                let mut start = 0;
                for spans in SPECIAL.captures_iter(text) {
                    let span = spans.get(0).expect("whole match");
                    push_plain(&mut units, &text[start..span.start()], light);
                    let kind = if spans.name("abbreviation").is_some()
                        || spans.name("dotted").is_some() && dotted_abbreviation(span.as_str())
                    {
                        Kind::Abbreviation
                    } else {
                        Kind::Opaque
                    };
                    start = span.end();
                    if kind == Kind::Opaque
                        && spans.name("dotted").is_some()
                        && span.as_str().ends_with('.')
                    {
                        // Ordinary filenames and bare domains do not include
                        // the adjacent sentence stop. Abbreviations do.
                        start -= 1;
                    }
                    units.push(Unit {
                        kind,
                        text: &text[span.start()..start],
                    });
                }
                push_plain(&mut units, &text[start..], light);
            }
            Piece::Literal(text) => units.push(Unit {
                kind: Kind::Literal,
                text,
            }),
        }
    }
    units
}

fn dotted_abbreviation(text: &str) -> bool {
    ["e.g.", "i.e.", "p.ex.", "a.m.", "p.m."]
        .iter()
        .any(|abbr| text.eq_ignore_ascii_case(abbr))
        || text.strip_suffix('.').is_some_and(|stem| {
            stem.split('.')
                .all(|part| part.len() == 1 && part.as_bytes()[0].is_ascii_uppercase())
        })
}

fn boundary(left: Option<char>, right: Option<char>) -> bool {
    [left, right].into_iter().flatten().any(char::is_whitespace)
}

fn spacing<'a>(
    left: Option<Unit<'_>>,
    space: &'a str,
    right: Option<Unit<'_>>,
    language: Language,
    options: &TextOptions,
) -> &'a str {
    let (Some(left), Some(right)) = (left, right) else {
        return space;
    };
    if boundary(left.last(), right.first()) {
        return space;
    }
    let lmark = left.mark();
    let rmark = right.mark();
    // Leave expressive clusters and spaced ellipses alone. In particular,
    // inserting a French space inside `?!` would change the cluster.
    if lmark.is_some_and(|c| matches!(c, '.' | ',' | '!' | '?' | '…'))
        && rmark.is_some_and(|c| matches!(c, '.' | ',' | '!' | '?' | '…'))
    {
        return space;
    }
    if options.typographic_spaces {
        if language == Language::French {
            // France (Imprimerie nationale convention): a full NBSP before
            // the colon, a narrow NBSP before ;!? and inside guillemets.
            match rmark {
                Some(':') => return "\u{a0}",
                Some(';' | '!' | '?' | '»') => return "\u{202f}",
                _ if lmark == Some('«') => return "\u{202f}",
                _ => {}
            }
        }
        if rmark.is_some_and(is_punctuation) {
            return "";
        }
    }
    if options.light_punctuation {
        if rmark == Some('.') && !space.is_empty() {
            return "";
        }
        if space.is_empty()
            && lmark.is_some_and(is_punctuation)
            && right.first().is_some_and(char::is_alphabetic)
        {
            return " ";
        }
    }
    if options.typographic_spaces && space.bytes().all(|b| b == b' ') && !space.is_empty() {
        return " ";
    }
    space
}

fn is_punctuation(c: char) -> bool {
    matches!(c, '.' | ',' | ';' | ':' | '!' | '?')
}

fn ellipsis(units: &[Unit<'_>], index: usize) -> bool {
    let left = units[..index].last().and_then(|u| {
        if u.kind == Kind::Space {
            units.get(index.checked_sub(2)?)
        } else {
            Some(u)
        }
    });
    let right = units.get(index + 1).and_then(|u| {
        if u.kind == Kind::Space {
            units.get(index + 2)
        } else {
            Some(u)
        }
    });
    [left, right]
        .into_iter()
        .flatten()
        .any(|u| matches!(u.mark(), Some('.' | '…')))
}

fn write_unit(out: &mut String, unit: Unit<'_>, pending: &mut bool, sentence_case: bool) {
    if *pending && sentence_case && matches!(unit.kind, Kind::Plain | Kind::Abbreviation) {
        if let Some((i, c)) = unit.text.char_indices().find(|(_, c)| c.is_alphabetic()) {
            *pending = false;
            out.push_str(&unit.text[..i]);
            out.extend(c.to_uppercase());
            out.push_str(&unit.text[i + c.len_utf8()..]);
            return;
        }
    } else if matches!(unit.kind, Kind::Literal | Kind::Opaque) {
        // The unit already supplies sentence content; do not capitalize the
        // next word merely because the leading token was protected.
        if unit.text.chars().any(char::is_alphanumeric) {
            *pending = false;
        }
    }
    out.push_str(unit.text);
}

pub(super) fn format(pieces: &[Piece<'_>], rules: &TextRules, capacity: usize) -> String {
    let units = units(pieces, rules.options.light_punctuation);
    let mut output = String::with_capacity(capacity);
    let mut previous = None;
    let mut pending = true;
    for (i, &unit) in units.iter().enumerate() {
        if unit.kind == Kind::Space {
            output.push_str(spacing(
                previous,
                unit.text,
                units.get(i + 1).copied(),
                rules.language,
                &rules.options,
            ));
        } else {
            if previous.is_none_or(|u: Unit<'_>| u.kind != Kind::Space) {
                output.push_str(spacing(
                    previous,
                    "",
                    Some(unit),
                    rules.language,
                    &rules.options,
                ));
            }
            write_unit(&mut output, unit, &mut pending, rules.options.sentence_case);
            if matches!(unit.mark(), Some('!' | '?'))
                || (unit.mark() == Some('.') && !ellipsis(&units, i))
            {
                pending = true;
            }
        }
        previous = Some(unit);
    }
    output
}
