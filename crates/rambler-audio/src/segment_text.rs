use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

/// Assemble ordered transcript segments for display without changing the inputs.
///
/// Each segment is copied verbatim, including leading/trailing whitespace and
/// paragraph breaks. Between nonempty segments, use one ASCII space unless the
/// seam already has whitespace, splits an extended grapheme cluster, joins
/// Han/Kana text, or matches an attaching punctuation rule (e.g. `L’` + `équipe`
/// or `word` + `,`).
///
/// This is a small display heuristic, not language detection or tokenization.
/// Other word seams retain the default space, including mixed Han/Latin and
/// ordinary Cyrillic, Arabic and Hangul word seams. Ambiguous fragments like
/// `note` + `book` and `12` + `34` do not infer compounds or numbers. There is
/// no trimming, normalization or transliteration; the exact segments remain
/// the source of truth.
pub fn join_segment_texts<'a>(segments: impl IntoIterator<Item = &'a str>) -> String {
    let mut text = String::new();
    for segment in segments {
        append_segment_text(&mut text, segment);
    }
    text
}

pub(crate) fn append_segment_text(text: &mut String, segment: &str) {
    let seam = text.len();
    text.push_str(segment);
    if needs_space(text, seam) {
        text.insert(seam, ' ');
    }
}

fn needs_space(text: &str, seam: usize) -> bool {
    let (left, right) = text.split_at(seam);
    let mut left_chars = left.chars();
    let (Some(last), Some(first)) = (left_chars.next_back(), right.chars().next()) else {
        return false;
    };
    if explicit_boundary(last) || explicit_boundary(first) || attaches_left(first) || opens(last) {
        return false;
    }
    if (matches!(last, '\'' | '’' | 'ʼ') && first.is_alphabetic()
        || matches!(last, '-' | '‐' | '‑') && first.is_alphanumeric())
        && left_chars.next_back().is_some_and(char::is_alphanumeric)
    {
        return false;
    }
    // A whitespace insertion must not split an existing cluster (e.g. a
    // decomposed accent or ZWJ emoji). The cursor has the whole joined string.
    if !matches!(
        GraphemeCursor::new(seam, text.len(), true).is_boundary(text, 0),
        Ok(true)
    ) {
        return false;
    }
    if han_or_kana(first) {
        let last_base = left
            .graphemes(true)
            .next_back()
            .and_then(|g| g.chars().next());
        if last_base.is_some_and(|c| han_or_kana(c) || east_asian_closing(c)) {
            return false;
        }
    }
    true
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
                | '\''
                | '’'
                | 'ʼ'
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
    use super::{append_segment_text, join_segment_texts};

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
        let mut appended = String::new();
        for segment in &segments {
            append_segment_text(&mut appended, segment);
        }
        assert_eq!(appended, joined);
        assert_eq!(segments, ["note", "book"]);
    }
}
