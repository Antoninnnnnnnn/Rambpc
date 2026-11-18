use crate::{EditingContext, EngineError, ReplacementProposal, Revision};
use std::{fmt, iter, ops::Range};
use unicode_segmentation::UnicodeSegmentation;

/// One scalar-aligned textual replacement within the original captured window.
/// This is neither a document range, a selection update, nor authorization to
/// apply an edit. [`ReplacementProposal::derive_grapheme_splice`] additionally
/// aligns the splice to extended grapheme clusters within both windows.
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

struct ScalarBounds {
    original_utf16: Range<usize>,
    replacement_bytes: Range<usize>,
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
        Ok(self
            .scalar_bounds(current, current_revision)?
            .map(|bounds| ScalarSplice {
                original_utf16_range: bounds.original_utf16,
                inserted_text: &self.replacement_text[bounds.replacement_bytes],
            }))
    }

    /// Like [`Self::derive_scalar_splice`], but widens both ends to extended
    /// grapheme cluster boundaries (UAX #29) shared by the original and
    /// replacement windows. Segments each full window, not the individual
    /// `before`/`selected`/`after` parts. Text is not normalized.
    ///
    /// This is a window-relative guarantee only. Text beyond a captured window
    /// can affect segmentation inside it (e.g. a run of regional indicators).
    /// Before applying, verify both sets of boundaries in document context with
    /// the host's segmentation rules, as well as the document, anchor and
    /// revision in the same editor transaction. Do not round the offsets.
    pub fn derive_grapheme_splice(
        &self,
        current: &EditingContext,
        current_revision: &Revision,
    ) -> Result<Option<ScalarSplice<'_>>, EngineError> {
        let Some(bounds) = self.scalar_bounds(current, current_revision)? else {
            return Ok(None);
        };
        let original = [&current.before[..], &current.selected, &current.after].concat();
        let replacement = self.replacement_text.as_str();
        let scalar = bounds.replacement_bytes;

        let start = shared_offsets(
            grapheme_boundaries(&original),
            grapheme_boundaries(replacement),
        )
        .take_while(|&offset| offset <= scalar.start)
        .last()
        .unwrap_or(0);

        // The retained suffix is byte-identical, but may segment differently
        // due to the preceding edit. Find its first boundary in both windows.
        let suffix_len = replacement.len() - scalar.end;
        let original_suffix = original.len() - suffix_len;
        let suffix_skip = shared_offsets(
            grapheme_boundaries(&original).filter_map(|offset| offset.checked_sub(original_suffix)),
            grapheme_boundaries(replacement).filter_map(|offset| offset.checked_sub(scalar.end)),
        )
        .next()
        .unwrap_or(suffix_len);
        let original_end = original_suffix + suffix_skip;
        let start_utf16 = original[..start].encode_utf16().count();
        let end_utf16 = start_utf16 + original[start..original_end].encode_utf16().count();

        Ok(Some(ScalarSplice {
            original_utf16_range: start_utf16..end_utf16,
            inserted_text: &replacement[start..scalar.end + suffix_skip],
        }))
    }

    fn scalar_bounds(
        &self,
        current: &EditingContext,
        current_revision: &Revision,
    ) -> Result<Option<ScalarBounds>, EngineError> {
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

        Ok(Some(ScalarBounds {
            original_utf16: start_utf16..end_utf16,
            replacement_bytes: prefix_bytes..replacement_end,
        }))
    }
}

fn grapheme_boundaries(text: &str) -> impl Iterator<Item = usize> + '_ {
    text.grapheme_indices(true)
        .map(|(offset, _)| offset)
        .chain(iter::once(text.len()))
}

fn shared_offsets(
    left: impl Iterator<Item = usize>,
    right: impl Iterator<Item = usize>,
) -> impl Iterator<Item = usize> {
    let mut right = right.peekable();
    left.filter(move |left| {
        while right.peek().is_some_and(|right| right < left) {
            right.next();
        }
        right.peek() == Some(left)
    })
}
