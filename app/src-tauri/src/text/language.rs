//! Local language guess for `Language::Auto`. Rambler recognizes French and
//! English in the same take but does not report which one it heard (the
//! response language fields are empty in practice), so the typography —
//! French spaces and guillemets, or English punctuation — follows the text
//! itself. Deterministic, no model: common function words and French letters.

use crate::model::Language;
use unicode_segmentation::UnicodeSegmentation;

const FRENCH: &[&str] = &[
    "le", "la", "les", "un", "une", "des", "du", "de", "et", "est", "je", "tu", "il", "elle",
    "nous", "vous", "ils", "elles", "pour", "que", "qui", "dans", "pas", "ne", "avec", "sur",
    "au", "aux", "ce", "cette", "ces", "mais", "ou", "donc", "car", "mon", "ma", "mes", "ton",
    "ta", "tes", "son", "sa", "ses", "leur", "leurs", "notre", "votre", "très", "bien", "merci",
    "bonjour", "oui", "non", "à", "été", "être", "avoir", "fait", "faire", "on", "y", "en",
    "c'est", "j'ai", "qu'il", "n'est", "d'un", "d'une", "l'", "aujourd'hui", "demain",
];

const ENGLISH: &[&str] = &[
    "the", "and", "is", "are", "to", "of", "a", "an", "in", "you", "i", "it", "for", "with",
    "on", "that", "this", "we", "be", "was", "were", "have", "has", "not", "do", "does", "can",
    "will", "would", "should", "my", "your", "our", "their", "they", "he", "she", "at", "from",
    "by", "but", "or", "if", "so", "please", "thanks", "thank", "hello", "yes", "no", "what",
    "when", "where", "how", "why", "there", "here", "let's", "i'm", "it's", "don't", "can't",
    "today", "tomorrow", "meeting",
];

/// French or English for the typography of `text`; ties and empty text go to
/// French, the product's first language.
pub(super) fn detect(text: &str) -> Language {
    let mut french = 0_i32;
    let mut english = 0_i32;
    for word in text.unicode_words() {
        let lower = word.to_lowercase().replace('’', "'");
        if FRENCH.contains(&lower.as_str()) {
            french += 2;
        }
        if ENGLISH.contains(&lower.as_str()) {
            english += 2;
        }
        if lower.chars().any(|c| "éèêëàâîïôûùüçœæ".contains(c)) {
            french += 1;
        }
    }
    if english > french {
        Language::English
    } else {
        Language::French
    }
}

#[cfg(test)]
mod tests {
    use super::detect;
    use crate::model::Language::{English, French};

    #[test]
    fn detects_the_language_of_typical_dictations() {
        assert_eq!(detect("Rendez-vous jeudi à 14 heures."), French);
        assert_eq!(detect("Let us meet on Thursday at 2:00 p.m."), English);
        assert_eq!(detect("Je ne pourrai pas venir demain."), French);
        assert_eq!(detect("Could you send me the invoice by Friday?"), English);
        assert_eq!(detect("Write a function that sorts the list by date."), English);
        assert_eq!(detect("Écris une fonction qui trie la liste."), French);
        // Mixed: the majority decides.
        assert_eq!(detect("On fait le point sur le pull request demain"), French);
        assert_eq!(detect("Please review the PR, merci"), English);
        assert_eq!(detect(""), French);
    }
}
