use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

/// Assemble ordered transcript segments for display without changing the inputs.
///
/// Each segment is copied verbatim, including leading/trailing whitespace and
/// paragraph breaks. Between nonempty segments, use one ASCII space unless the
/// seam already has whitespace, splits an extended grapheme cluster, joins
/// Han/Kana text, or matches an attaching punctuation rule (e.g. `L’` + `équipe`
/// or `word` + `,`). A recognizable opening quote keeps the separator;
/// an ambiguous leading apostrophe only attaches next to a short letter run.
///
/// This is a small display heuristic, not language detection or tokenization.
/// Other word seams retain the default space, including mixed Han/Latin and
/// ordinary Cyrillic, Arabic and Hangul word seams. Ambiguous fragments like
/// `note` + `book` and `12` + `34` do not infer compounds or numbers. There is
/// no trimming, normalization or transliteration; the exact segments remain
/// the source of truth.
pub fn join_segment_texts<'a>(segments: impl IntoIterator<Item = &'a str>) -> String {
    let mut text = SegmentText::default();
    for segment in segments {
        text.push(segment);
    }
    text.finish()
}

/// Incremental projection for callers that do not hold all segments at once.
#[derive(Default)]
pub(crate) struct SegmentText {
    text: String,
    odd_trailing_ri: bool,
    #[cfg(test)]
    work: JoinWork,
}

#[cfg(test)]
#[derive(Default)]
struct JoinWork {
    suffix_scalars: usize,
    cursor_calls: usize,
}

impl SegmentText {
    pub(crate) fn push(&mut self, segment: &str) {
        if segment.is_empty() {
            return;
        }
        let seam = self.text.len();
        self.text.push_str(segment);
        let separate = needs_space(&self.text, seam, |last, first| {
            if regional_indicator(last) && regional_indicator(first) {
                // GB12/GB13: break only after an even number of contiguous RIs.
                // A fresh cursor would walk the entire RI run at every seam.
                return !self.odd_trailing_ri;
            }
            #[cfg(test)]
            {
                self.work.cursor_calls += 1;
            }
            cursor_boundary(&self.text, seam)
        });
        if separate {
            self.text.insert(seam, ' ');
        }
        self.update_trailing_ri(segment, separate);
    }

    pub(crate) fn finish(self) -> String {
        self.text
    }

    fn update_trailing_ri(&mut self, segment: &str, separated: bool) {
        let mut odd = false;
        // Examine only newly appended text. A non-RI inside it ends the run;
        // otherwise combine its parity with the prior suffix unless we spaced.
        for c in segment.chars().rev() {
            #[cfg(test)]
            {
                self.work.suffix_scalars += 1;
            }
            if !regional_indicator(c) {
                self.odd_trailing_ri = odd;
                return;
            }
            odd = !odd;
        }
        self.odd_trailing_ri = odd ^ (!separated && self.odd_trailing_ri);
    }
}

fn regional_indicator(c: char) -> bool {
    matches!(c, '\u{1f1e6}'..='\u{1f1ff}')
}

fn cursor_boundary(text: &str, seam: usize) -> bool {
    matches!(
        GraphemeCursor::new(seam, text.len(), true).is_boundary(text, 0),
        Ok(true)
    )
}

fn needs_space(text: &str, seam: usize, is_boundary: impl FnOnce(char, char) -> bool) -> bool {
    let (left, right) = text.split_at(seam);
    let mut left_chars = left.chars();
    let (Some(last), Some(first)) = (left_chars.next_back(), right.chars().next()) else {
        return false;
    };
    if explicit_boundary(last)
        || explicit_boundary(first)
        || attaches_left(first)
        || opens(last)
        || apostrophe(first) && leading_apostrophe_attaches(left, right, first)
    {
        return false;
    }
    if (apostrophe(last) && first.is_alphabetic()
        || matches!(last, '-' | '‐' | '‑') && first.is_alphanumeric())
        && last_base(left_chars.as_str()).is_some_and(char::is_alphanumeric)
    {
        return false;
    }
    // Besides RI parity, UAX #29 looks back for GB9c and GB11. A GB9c
    // probe crosses only trailing Linkers/Extends; the newly appended
    // consonant stops the next probe (at most one shared endpoint).
    // GB11 already short-circuits above on a literal ZWJ. All other
    // UAX #29 rules used here inspect only adjacent scalars.
    if !is_boundary(last, first) {
        return false;
    }
    if han_or_kana(first) {
        // Inspect the preceding cluster only when the new segment starts
        // with a Han/Kana base, which prevents later probes from revisiting
        // a long cluster or RI run before that base.
        return !last_base(left).is_some_and(|c| han_or_kana(c) || east_asian_closing(c));
    }
    true
}

fn last_base(text: &str) -> Option<char> {
    text.graphemes(true)
        .next_back()
        .and_then(|g| g.chars().next())
}

fn apostrophe(c: char) -> bool {
    matches!(c, '\'' | '’' | 'ʼ')
}

fn leading_apostrophe_attaches(left: &str, right: &str, mark: char) -> bool {
    // A lone mark or one followed by a separator can be a closing mark.
    let tail = &right[mark.len_utf8()..];
    let Some(first) = tail.chars().next() else {
        return true;
    };
    if first.is_whitespace() || attaches_left(first) {
        return true;
    }
    if !first.is_alphabetic() || !last_base(left).is_some_and(char::is_alphabetic) {
        return false;
    }
    // A matching terminal quote is stronger evidence than an elision. An
    // apostrophe inside a quoted word (e.g. 'don't') is not the terminator.
    if mark != 'ʼ'
        && tail.match_indices(mark).any(|(i, _)| {
            tail[i + mark.len_utf8()..]
                .chars()
                .next()
                .is_none_or(|c| c.is_whitespace() || attaches_left(c))
        })
    {
        return false;
    }
    // Otherwise attach only next to a short letter run (e.g. l + ’équipe
    // or John + ’s). This deliberately does not try to recognize words.
    short_letters(left.graphemes(true).rev()) || short_letters(tail.graphemes(true))
}

fn short_letters<'a>(graphemes: impl Iterator<Item = &'a str>) -> bool {
    let letters = graphemes
        .take(3)
        .take_while(|g| g.chars().next().is_some_and(char::is_alphabetic))
        .count();
    (1..=2).contains(&letters)
}

fn explicit_boundary(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{200b}'..='\u{200d}' | '\u{2060}' | '\u{feff}')
}

fn opens(c: char) -> bool {
    matches!(
        c,
        '(' | '['
            | '{'
            | '“'
            | '‘'
            | '«'
            | '‹'
            | '（'
            | '［'
            | '｛'
            | '「'
            | '『'
            | '《'
            | '〈'
            | '【'
            | '〔'
            | '｢'
    )
}

fn attaches_left(c: char) -> bool {
    east_asian_closing(c)
        || matches!(
            c,
            '.' | ','
                | ':'
                | ';'
                | '!'
                | '?'
                | '%'
                | ')'
                | ']'
                | '}'
                | '”'
                | '»'
                | '›'
                | '…'
                | '،'
                | '؛'
                | '؟'
                | '۔'
                | '।'
                | '॥'
                | '\u{0387}'
        )
}

fn east_asian_closing(c: char) -> bool {
    matches!(
        c,
        '。' | '、'
            | '，'
            | '．'
            | '：'
            | '；'
            | '！'
            | '？'
            | '％'
            | '）'
            | '］'
            | '｝'
            | '」'
            | '』'
            | '》'
            | '〉'
            | '】'
            | '〕'
            | '｡'
            | '､'
            | '｣'
    )
}

fn han_or_kana(c: char) -> bool {
    matches!(c,
        '々' | '〆' | '〇'
        | '\u{3041}'..='\u{3096}' | '\u{309d}'..='\u{309f}' // Hiragana
        | '\u{30a1}'..='\u{30fa}' | '\u{30fc}'..='\u{30ff}'  // Katakana
        | '\u{31f0}'..='\u{31ff}' | '\u{ff66}'..='\u{ff9d}'
        | '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}'  // Han ideographs
        | '\u{f900}'..='\u{faff}'
        | '\u{20000}'..='\u{2fa1f}' | '\u{30000}'..='\u{323af}'
    )
}

#[cfg(test)]
mod tests {
    use super::{SegmentText, cursor_boundary, join_segment_texts, needs_space};

    #[test]
    fn joins_elisions_and_han_kana_seams() {
        assert_eq!(join_segment_texts(["L’", "équipe"]), "L’équipe");
        assert_eq!(join_segment_texts(["l'", "équipe"]), "l'équipe");
        assert_eq!(join_segment_texts(["明天", "开会。"]), "明天开会。");
        assert_eq!(join_segment_texts(["明天。", "开会。"]), "明天。开会。");
        assert_eq!(join_segment_texts(["東京", "です", "。"]), "東京です。");
        assert_eq!(join_segment_texts(["Plan", "明天", "OK"]), "Plan 明天 OK");
        assert_eq!(join_segment_texts(["Plan ", "明天", " OK"]), "Plan 明天 OK");
    }

    #[test]
    fn adds_only_the_missing_word_separator_and_attaches_punctuation() {
        assert_eq!(join_segment_texts(["Hello", "world"]), "Hello world");
        assert_eq!(join_segment_texts(["déjà", "vu"]), "déjà vu");
        assert_eq!(join_segment_texts(["Привет", "мир."]), "Привет мир.");
        assert_eq!(
            join_segment_texts(["مرحبا", "بالعالم", "؟"]),
            "مرحبا بالعالم؟"
        );
        assert_eq!(join_segment_texts(["안녕", "세계."]), "안녕 세계.");
        assert_eq!(join_segment_texts(["Γεια", "κόσμε"]), "Γεια κόσμε");
        assert_eq!(join_segment_texts(["Room", "9", "open"]), "Room 9 open");
        assert_eq!(
            join_segment_texts(["Hello", ",", "world", "!"]),
            "Hello, world!"
        );
        assert_eq!(join_segment_texts(["What?", "Nothing."]), "What? Nothing.");
        assert_eq!(join_segment_texts(["Call", "…"]), "Call…");
        assert_eq!(join_segment_texts(["(", "Hello", ")"]), "(Hello)");
    }

    #[test]
    fn does_not_split_clusters_or_remove_explicit_boundary_markers() {
        assert_eq!(
            join_segment_texts(["cafe", "\u{301}", "noir"]),
            "cafe\u{301} noir"
        );
        assert_eq!(join_segment_texts(["👩\u{200d}", "💻"]), "👩\u{200d}💻");
        assert_eq!(join_segment_texts(["🇫", "🇷"]), "🇫🇷");
        assert_eq!(join_segment_texts(["ᄀ", "ᅡ", "ᆫ"]), "간");
        assert_eq!(
            join_segment_texts(["word\u{200b}", "word"]),
            "word\u{200b}word"
        );
    }

    #[test]
    fn preserves_explicit_whitespace_and_empty_segments() {
        assert_eq!(join_segment_texts([]), "");
        assert_eq!(join_segment_texts(["", "", ""]), "");
        assert_eq!(join_segment_texts(["hello", "", "world"]), "hello world");
        assert_eq!(join_segment_texts(["hello", " world"]), "hello world");
        assert_eq!(join_segment_texts(["hello ", "world"]), "hello world");
        assert_eq!(
            join_segment_texts([" hello ", "  world", "\n", "\nNext", "\t", "line\r", "\n"]),
            " hello   world\n\nNext\tline\r\n"
        );
        assert_eq!(join_segment_texts(["L’", " équipe"]), "L’ équipe");
        assert_eq!(
            join_segment_texts(["Bonjour", "\u{202f}!"]),
            "Bonjour\u{202f}!"
        );
    }

    #[test]
    fn retains_the_default_for_ambiguous_seams_and_keeps_segments() {
        assert_eq!(
            join_segment_texts(["state-", "of-the-art"]),
            "state-of-the-art"
        );
        assert_eq!(join_segment_texts(["3.", "14"]), "3. 14");
        assert_eq!(join_segment_texts(["12", "34"]), "12 34");
        let segments = [String::from("note"), String::from("book")];
        let joined = join_segment_texts(segments.iter().map(String::as_str));
        assert_eq!(joined, "note book");
        let mut appended = SegmentText::default();
        for segment in &segments {
            appended.push(segment);
        }
        assert_eq!(appended.finish(), joined);
        assert_eq!(segments, ["note", "book"]);
    }

    #[test]
    fn distinguishes_recognizable_quotes_from_attached_elisions() {
        for (segments, expected) in [
            (["She said", "'hello'"], "She said 'hello'"),
            (["She said", "'Hello!' Then"], "She said 'Hello!' Then"),
            (["She said", "‘hello’"], "She said ‘hello’"),
            (["He said", "’hello’"], "He said ’hello’"),
            (["John", "’s"], "John’s"),
            (["l", "’équipe"], "l’équipe"),
            (["hello", "’"], "hello’"),
            (["in", "'90"], "in '90"),
        ] {
            assert_eq!(join_segment_texts(segments), expected);
        }
    }

    #[test]
    fn halfwidth_brackets_and_decomposed_hyphen_seams_preserve_bytes() {
        assert_eq!(join_segment_texts(["｢", "東京", "｣"]), "｢東京｣");
        assert_eq!(join_segment_texts(["café-", "concert"]), "café-concert");
        assert_eq!(
            join_segment_texts(["cafe\u{301}-", "concert"]),
            "cafe\u{301}-concert"
        );
    }

    #[test]
    fn ri_run_spanning_many_seams_never_queries_the_accumulated_prefix() {
        let repeats = 4096;
        let mut joined = SegmentText::default();
        joined.push("🇦");
        for _ in 0..repeats {
            joined.push("🇧🇨");
        }
        assert_eq!(joined.work.suffix_scalars, 1 + repeats * 2);
        assert_eq!(joined.work.cursor_calls, 0);
        assert_eq!(joined.finish(), format!("🇦{}", "🇧🇨".repeat(repeats)));
    }

    #[test]
    fn parity_matches_the_full_cursor_across_empty_spaced_and_non_ri_segments() {
        let segments = [
            "",
            "🇦",
            "🇧🇨",
            "🇩🇪🇫",
            " 🇦",
            "🇦 ",
            "あ",
            "a🇦",
            "🇦a",
            "\u{301}",
        ];
        for left in segments {
            for middle in segments {
                for right in segments {
                    let input = [left, middle, right];
                    let mut full = String::new();
                    for segment in input {
                        let seam = full.len();
                        full.push_str(segment);
                        if needs_space(&full, seam, |_, _| cursor_boundary(&full, seam)) {
                            full.insert(seam, ' ');
                        }
                    }
                    assert_eq!(join_segment_texts(input), full, "{input:?}");
                }
            }
        }
    }

    #[test]
    fn indic_linker_splits_preserve_clusters_across_long_extends() {
        let link = format!("क्{}", "\u{093C}".repeat(128));
        let repeats = 256;
        let mut joined = SegmentText::default();
        for _ in 0..repeats {
            joined.push(&link);
            joined.push("");
        }
        joined.push("त");
        // Every nonempty seam consults UAX #29. The appended consonant is
        // the previous-context stop for the next such probe.
        assert_eq!(joined.work.cursor_calls, repeats);
        assert_eq!(joined.finish(), format!("{}त", link.repeat(repeats)));
    }
}
