//! French guillemets for straight double quotes that Rambler writes itself
//! (« ouvrez les guillemets … fermez les guillemets » often comes back as
//! `"…"`). Conservative: pairs left to right, and leaves the whole text
//! untouched when the quotes do not pair cleanly. Spaces inside « » are added
//! afterwards by the prose formatter (narrow no-break spaces), like for any
//! existing guillemets.

/// Returns the converted text, or `None` to keep the input unchanged.
pub(super) fn french_guillemets(text: &str) -> Option<String> {
    let quotes: Vec<usize> = text.match_indices('"').map(|(i, _)| i).collect();
    if quotes.is_empty() || quotes.len() % 2 != 0 || in_backticks(text) {
        return None;
    }
    let mut out = String::with_capacity(text.len() + quotes.len() * 2);
    let mut last = 0;
    let mut changed = false;
    for pair in quotes.chunks_exact(2) {
        let (open, close) = (pair[0], pair[1]);
        let inner = &text[open + 1..close];
        if !prose_quote(text, open, close, inner) {
            continue;
        }
        out.push_str(&text[last..open]);
        out.push('«');
        // Drop ASCII spaces hugging the quotes; the formatter adds the
        // French narrow no-break spaces.
        out.push_str(inner.trim_matches(' '));
        out.push('»');
        last = close + 1;
        changed = true;
    }
    if !changed {
        return None;
    }
    out.push_str(&text[last..]);
    Some(out)
}

fn in_backticks(text: &str) -> bool {
    text.contains('`')
}

/// A pair is ordinary quoted prose: not code (`key="v"`, `f("x")`, `{"a"}`),
/// not a quoted path or URL, not a measurement (`5"`), and on one line.
fn prose_quote(text: &str, open: usize, close: usize, inner: &str) -> bool {
    let before = text[..open].chars().next_back();
    let after = text[close + 1..].chars().next();
    if before.is_some_and(|c| c.is_alphanumeric() || matches!(c, '=' | '(' | '[' | '{' | ':' | '\\'))
    {
        return false;
    }
    if after.is_some_and(|c| c.is_alphanumeric() || matches!(c, '=' | ')' | ']' | '}' | ':')) {
        return false;
    }
    let trimmed = inner.trim();
    if trimmed.is_empty() || inner.contains(['\r', '\n']) {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    let path_or_url = lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("www.")
        || lower.starts_with('/')
        || lower.starts_with("\\\\")
        || lower.starts_with("~/")
        || lower.starts_with("./")
        || lower.starts_with("../")
        || (lower.len() > 2 && lower.as_bytes()[1] == b':' && matches!(lower.as_bytes()[2], b'\\' | b'/'));
    let code = trimmed.contains(['{', '}', ';', '=', '<', '>']) && !trimmed.contains(' ');
    !path_or_url && !code
}

#[cfg(test)]
mod tests {
    use super::french_guillemets as g;

    #[test]
    fn pairs_left_to_right() {
        assert_eq!(g("Il a dit \"Parfait\".").as_deref(), Some("Il a dit «Parfait»."));
        assert_eq!(
            g("\"Un\" et \"deux\"").as_deref(),
            Some("«Un» et «deux»")
        );
        assert_eq!(g("Il a dit \" oui \" hier").as_deref(), Some("Il a dit «oui» hier"));
    }

    #[test]
    fn leaves_unpaired_code_paths_and_urls() {
        assert_eq!(g("un seul \" guillemet"), None);
        assert_eq!(g("trois \"a\" \"b"), None);
        assert_eq!(g("le fichier \"C:\\Users\\x.txt\""), None);
        assert_eq!(g("le lien \"https://exemple.fr\""), None);
        assert_eq!(g("mets name=\"valeur\""), None);
        assert_eq!(g("appelle f(\"x\")"), None);
        assert_eq!(g("un écran de 27\" et 32\""), None);
        assert_eq!(g("voir `say \"hi\"`"), None);
        assert_eq!(g("déjà « bien »"), None);
    }
}
