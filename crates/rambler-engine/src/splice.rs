use crate::{EditingContext, EngineError, ReplacementProposal, Revision};
use std::{fmt, ops::Range};

/// One textual replacement within the original captured window. This is neither
/// a document range, a selection update, nor authorization to apply an edit.
pub struct ScalarSplice<'a> {
    original_utf16_range: Range<usize>,
    inserted_text: &'a str,
}

impl ScalarSplice<'_> {
    /// Half-open UTF-16 code-unit range measured from the original window start.
    /// An empty range means insertion, not a no-op.
    pub fn original_utf16_range(&self) -> Range<usize> {
        self.original_utf16_range.clone()
    }

    /// Exact text replacing the range, borrowed from the full replacement.
    /// Empty text with a nonempty range means deletion within the window.
    pub fn inserted_text(&self) -> &str {
        self.inserted_text
    }
}

impl fmt::Debug for ScalarSplice<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScalarSplice")
            .field("original_utf16_range", &self.original_utf16_range)
            .finish_non_exhaustive()
    }
}

impl ReplacementProposal {
    /// Revalidate the exact text split and captured revision, then describe at
    /// most one scalar-aligned text splice. Unversioned proposals return
    /// [`EngineError::ContextConflict`] as do text or revision mismatches.
    /// `Ok(None)` means unchanged **text**, not unchanged selection.
    ///
    /// The longest equal prefix wins ties on repeated text, then the longest
    /// equal suffix that does not overlap it in either string. No normalization,
    /// interpretation of commands, or policy fallback is performed.
    ///
    /// Use an unmodified proposal from [`crate::replacement_proposal`] after
    /// successful collection. Before any explicit host apply, verify document
    /// identity and the window anchor, then derive again within the same editor
    /// transaction. The revision must distinguish changes even if text is
    /// later restored. Scalar boundaries never split a surrogate pair, but may
    /// bisect grapheme clusters in the original or proposed document. Use only
    /// with a host that supports exact scalar edits, or separately verifies both
    /// sets of boundaries in document context; do not round the offsets. For
    /// replacement selection, see [`Self::selection_utf16_range`] separately.
    pub fn derive_scalar_splice(
        &self,
        current: &EditingContext,
        current_revision: &Revision,
    ) -> Result<Option<ScalarSplice<'_>>, EngineError> {
        self.validate_current(current, Some(current_revision))?;
        if self.expected_revision.is_none() {
            return Err(EngineError::ContextConflict);
        }

        let original = || {
            current
                .before
                .chars()
                .chain(current.selected.chars())
                .chain(current.after.chars())
        };
        let replacement = self.replacement_text.as_str();
        let mut prefix_bytes = 0;
        let mut start_utf16 = 0;
        for (old, new) in original().zip(replacement.chars()) {
            if old != new {
                break;
            }
            prefix_bytes += old.len_utf8();
            start_utf16 += old.len_utf16();
        }

        let mut end_utf16 = original().map(char::len_utf16).sum::<usize>();
        let mut replacement_end = replacement.len();
        if start_utf16 == end_utf16 && prefix_bytes == replacement_end {
            return Ok(None);
        }
        for (old, new) in original()
            .rev()
            .zip(replacement[prefix_bytes..].chars().rev())
        {
            if old != new || old.len_utf16() > end_utf16 - start_utf16 {
                break;
            }
            end_utf16 -= old.len_utf16();
            replacement_end -= old.len_utf8();
        }

        Ok(Some(ScalarSplice {
            original_utf16_range: start_utf16..end_utf16,
            inserted_text: &replacement[prefix_bytes..replacement_end],
        }))
    }
}
