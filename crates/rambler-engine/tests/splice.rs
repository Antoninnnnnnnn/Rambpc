use rambler_engine::{
    DictationResult, EditingContext, EngineError, Revision, replacement_proposal,
};
use rambler_protocol::{
    HYPOTHESIS_SAFETY_STATUS, RECOGNITION_RESPONSE, VOICE_EDITING_RESULT, decode_response,
    wire::{bytes_field as b, string_field as s, varint_field as v},
};
use std::ops::Range;

fn window(before: &str, selected: &str, after: &str) -> EditingContext {
    EditingContext {
        before: before.into(),
        selected: selected.into(),
        after: after.into(),
        cursor_aware: true,
        ..Default::default()
    }
}

fn field3(text: &str) -> Vec<u8> {
    [v(4, 2).unwrap(), s(3, text).unwrap()].concat()
}

fn field7(before: &str, selected: &str, after: &str) -> Vec<u8> {
    let context = [
        s(1, before).unwrap(),
        s(2, selected).unwrap(),
        s(3, after).unwrap(),
    ]
    .concat();
    [v(4, 2).unwrap(), b(7, &context).unwrap()].concat()
}

fn fixture(context: &EditingContext, edit: Option<&[u8]>, safety: Option<u64>) -> DictationResult {
    let mut hypothesis = s(1, "SEND").unwrap(); // Never an edit fallback.
    if let Some(edit) = edit {
        hypothesis.extend(b(VOICE_EDITING_RESULT, edit).unwrap());
    }
    if let Some(status) = safety {
        hypothesis.extend(v(HYPOTHESIS_SAFETY_STATUS, status).unwrap());
    }
    let hypotheses = b(3, &hypothesis).unwrap();
    let recognition = [v(1, 1).unwrap(), b(3, &hypotheses).unwrap()].concat();
    let wire = b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap();
    DictationResult {
        final_updates: vec![decode_response(&wire).unwrap()],
        context: Some(context.clone()),
        ..Default::default()
    }
}

fn assert_change(
    current: &EditingContext,
    new: &str,
    expected_range: Range<usize>,
    expected_inserted: &str,
) {
    let revision = Revision::from(1i64);
    let result = fixture(current, Some(&field3(new)), None);
    let proposal = replacement_proposal(&result, Some(revision.clone())).unwrap();
    let splice = proposal
        .derive_scalar_splice(current, &revision)
        .unwrap()
        .unwrap();
    assert_eq!(splice.original_utf16_range(), expected_range);
    assert_eq!(splice.inserted_text(), expected_inserted);
    let mut original: Vec<u16> = [&current.before, &current.selected, &current.after]
        .into_iter()
        .flat_map(|part| part.encode_utf16())
        .collect();
    original.splice(
        splice.original_utf16_range(),
        splice.inserted_text().encode_utf16(),
    );
    assert_eq!(String::from_utf16(&original).unwrap(), new);
}

#[test]
fn canonical_edit_preserves_surroundings_without_inserting_the_full_window() {
    let current = window("The kite is ", "blue", ". Marker stays.");
    let revision = Revision::from("captured");
    let edit = field7("The kite is ", "green", ". Marker stays.");
    let result = fixture(&current, Some(&edit), None);
    let proposal = replacement_proposal(&result, Some(revision.clone())).unwrap();
    let splice = proposal
        .derive_scalar_splice(&current, &revision)
        .unwrap()
        .unwrap();
    let original: Vec<u16> = "The kite is blue. Marker stays.".encode_utf16().collect();
    let range = splice.original_utf16_range();
    assert_eq!(range, 12..16);
    let prefix = String::from_utf16(&original[..range.start]).unwrap();
    let suffix = String::from_utf16(&original[range.end..]).unwrap();
    assert_eq!(prefix, "The kite is ");
    assert_eq!(suffix, ". Marker stays.");
    assert_eq!(splice.inserted_text(), "green");
    assert_eq!(
        format!("{prefix}{}{suffix}", splice.inserted_text()),
        "The kite is green. Marker stays."
    );
    assert_eq!(proposal.selection_utf16_range(), Some((12, 17)));
    assert!(!format!("{splice:?}").contains("green"));
}

#[test]
fn non_bmp_offsets_count_two_units_and_never_split_a_scalar() {
    // The changing emoji share a high surrogate; matching code units would split it.
    assert_change(&window("🚲A", "😀", "💡"), "🚲A😁💡", 3..5, "😁");
}

#[test]
fn scalar_opt_in_can_split_graphemes_and_does_not_normalize() {
    let current = window("Caf", "e\u{301}", ".");
    assert_change(&current, "Cafe\u{300}.", 4..5, "\u{300}");
    assert_change(&current, "Café.", 3..5, "é");
    assert_change(&window("e", "", ""), "e\u{301}", 1..1, "\u{301}");
}

#[test]
fn prefix_wins_repetition_ties_and_suffix_cannot_overlap_it() {
    assert_change(&window("aa", "a", "a"), "aaaaa", 4..4, "a");
    assert_change(&window("aa", "aa", "a"), "aaaa", 4..5, "");
    assert_change(&window("abab", "X", "abab"), "ababab", 4..7, "");
    assert_change(&window("b", "ab", "ab"), "ab", 0..3, "");
}

#[test]
fn distinguishes_unchanged_text_deletion_and_insertion() {
    assert_change(&window("", "one", ""), "", 0..3, "");
    assert_change(&window("", "", ""), "one", 0..0, "one");
    let revision = Revision::from(1i64);
    let current = window("a", "🦀", "c");
    let proposal = replacement_proposal(
        &fixture(&current, Some(&field7("a🦀", "", "c")), None),
        Some(revision.clone()),
    )
    .unwrap();
    assert!(
        proposal
            .derive_scalar_splice(&current, &revision)
            .unwrap()
            .is_none()
    );
    assert_eq!(proposal.selection_utf16_range(), Some((3, 3)));
    let empty = window("", "", "");
    let proposal = replacement_proposal(
        &fixture(&empty, Some(&field3("")), None),
        Some(revision.clone()),
    )
    .unwrap();
    assert!(
        proposal
            .derive_scalar_splice(&empty, &revision)
            .unwrap()
            .is_none()
    );
}

#[test]
fn mismatched_text_split_or_revision_conflicts_before_even_a_no_op() {
    let current = window("a", "b", "c");
    let revision = Revision::from(1i64);
    let result = fixture(&current, Some(&field3("abc")), None);
    let proposal = replacement_proposal(&result, Some(revision.clone())).unwrap();
    for (live, rev) in [
        (window("a", "B", "c"), revision.clone()),
        (window("ab", "", "c"), revision.clone()), // Same joined text, different split.
        (current.clone(), Revision::from(3i64)),   // Changed and then restored.
        (current.clone(), Revision::from("1")),    // Same spelling, different type.
    ] {
        assert!(matches!(
            proposal.derive_scalar_splice(&live, &rev),
            Err(EngineError::ContextConflict)
        ));
    }
    let unversioned = replacement_proposal(&result, None).unwrap();
    unversioned
        .validate_current(&current, Some(&revision))
        .unwrap();
    assert!(matches!(
        unversioned.derive_scalar_splice(&current, &revision),
        Err(EngineError::ContextConflict)
    ));
}

#[test]
fn strict_proposal_exits_before_splice_and_never_falls_back() {
    let current = window("captured", "", "");
    let revision = Revision::from(1i64);
    let attempt = |result: &DictationResult| -> Result<bool, EngineError> {
        let proposal = replacement_proposal(result, Some(revision.clone()))?;
        Ok(proposal
            .derive_scalar_splice(&current, &revision)?
            .is_some())
    };
    let edit = field3("captured");
    let mut blocked = fixture(&current, Some(&edit), Some(1));
    blocked.final_updates.push(blocked.final_updates[0].clone());
    assert!(matches!(attempt(&blocked), Err(EngineError::SafetyBlocked)));
    assert!(matches!(
        attempt(&fixture(&current, Some(&edit), Some(2))),
        Err(EngineError::UnrecognizedSafety)
    ));
    let mut ambiguous = fixture(&current, Some(&edit), None);
    ambiguous
        .final_updates
        .push(ambiguous.final_updates[0].clone());
    assert!(matches!(
        attempt(&ambiguous),
        Err(EngineError::AmbiguousResult)
    ));
    let mut no_context = fixture(&current, Some(&edit), None);
    no_context.context = None;
    assert!(matches!(
        attempt(&no_context),
        Err(EngineError::UnsupportedEdit)
    ));
    let wrong_intent = [v(4, 3).unwrap(), s(3, "CLEAR").unwrap()].concat();
    let wrong_member = [v(4, 2).unwrap(), s(2, "CLEAR ALL").unwrap()].concat();
    for rejected in [
        None,
        Some(wrong_intent.as_slice()),
        Some(wrong_member.as_slice()),
    ] {
        assert!(matches!(
            attempt(&fixture(&current, rejected, None)),
            Err(EngineError::UnsupportedEdit)
        ));
    }
}

#[test]
fn accepted_command_like_text_remains_literal_data() {
    for text in ["SEND", "CLEAR", "CLEAR ALL"] {
        assert_change(&window("captured", "", ""), text, 0..8, text);
    }
}
