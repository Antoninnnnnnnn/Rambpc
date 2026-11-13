use rambler_engine::*;
use rambler_protocol::{
    RECOGNITION_RESPONSE, VOICE_EDITING_RESULT, decode_response,
    wire::{bytes_field as b, string_field as s, varint_field as v},
};

fn final_update(safety: &[u8], edit: Option<(u64, u32)>) -> S3Response {
    let mut hypothesis = s(1, "CONTROLLED ORIGINAL").unwrap();
    if let Some((intent, field)) = edit {
        let candidate = if field == 7 {
            b(7, &s(1, "CONTROLLED EDIT").unwrap()).unwrap()
        } else {
            s(field, "CONTROLLED EDIT").unwrap()
        };
        let edit = [v(4, intent).unwrap(), candidate].concat();
        hypothesis.extend(b(VOICE_EDITING_RESULT, &edit).unwrap());
    }
    hypothesis.extend_from_slice(safety);
    let recognition = [v(1, 1).unwrap(), b(3, &b(3, &hypothesis).unwrap()).unwrap()].concat();
    decode_response(&b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap()).unwrap()
}
fn result(safety: &[u8], edit: Option<(u64, u32)>) -> DictationResult {
    DictationResult {
        final_updates: vec![final_update(safety, edit)],
        context: Some(EditingContext {
            before: "CONTROLLED SNAPSHOT".into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn safety_one_refuses_render_and_proposal_without_falling_back_to_transcript() {
    for edit in [None, Some((2, 3)), Some((2, 7)), Some((3, 3)), Some((3, 7))] {
        let result = result(&v(30, 1).unwrap(), edit);
        assert!(matches!(
            result.check_safety(),
            Err(EngineError::SafetyBlocked)
        ));
        assert!(matches!(result.text(), Err(EngineError::SafetyBlocked)));
        assert!(matches!(
            replacement_proposal(&result, None),
            Err(EngineError::SafetyBlocked)
        ));
        // Retention is explicit; only convenience rendering/proposals are blocked.
        assert_eq!(
            result.final_updates[0].hypotheses()[0].text.as_deref(),
            Some("CONTROLLED ORIGINAL")
        );
    }
}

#[test]
fn absent_and_zero_safety_leave_verified_policies_and_empty_deletion_unchanged() {
    for safety in [vec![], v(30, 0).unwrap()] {
        for edit in [None, Some((2, 3)), Some((2, 7)), Some((3, 3))] {
            let result = result(&safety, edit);
            result.check_safety().unwrap();
            assert_eq!(
                result.text().unwrap(),
                if edit.is_none() {
                    "CONTROLLED ORIGINAL"
                } else {
                    "CONTROLLED EDIT"
                }
            );
            if matches!(edit, Some((2, _))) {
                replacement_proposal(&result, None).unwrap();
            }
        }
    }
}

#[test]
fn unrecognized_safety_is_preserved_and_refuses_render_even_alongside_known_zero() {
    for unknown in [
        v(30, 2).unwrap(),
        v(30, u64::MAX).unwrap(),
        v(30, 1_u64 << 32).unwrap(),
        v(30, (1_u64 << 32) + 1).unwrap(),
        b(30, b"CONTROLLED UNKNOWN").unwrap(),
    ] {
        for safety in [
            unknown.clone(),
            [v(30, 0).unwrap(), unknown.clone()].concat(),
            [unknown.clone(), v(30, 0).unwrap()].concat(),
        ] {
            let result = result(&safety, Some((2, 3)));
            assert!(matches!(
                result.text(),
                Err(EngineError::UnrecognizedSafety)
            ));
            assert!(matches!(
                replacement_proposal(&result, None),
                Err(EngineError::UnrecognizedSafety)
            ));
            assert!(
                result.final_updates[0].hypotheses()[0]
                    .unknown_fields
                    .iter()
                    .any(|f| f.number == 30)
            );
        }
    }
}

#[test]
fn safety_guard_checks_all_retained_candidates_before_ambiguity_and_prioritizes_blockage() {
    let mut result = result(&v(30, 0).unwrap(), Some((2, 3)));
    result
        .final_updates
        .push(final_update(&v(30, 2).unwrap(), Some((3, 3))));
    result
        .final_updates
        .push(final_update(&v(30, 1).unwrap(), Some((2, 7))));
    assert!(matches!(result.text(), Err(EngineError::SafetyBlocked)));
    result.final_updates.reverse();
    assert!(matches!(
        replacement_proposal(&result, None),
        Err(EngineError::SafetyBlocked)
    ));
    result.final_updates.remove(0);
    assert!(matches!(
        result.text(),
        Err(EngineError::UnrecognizedSafety)
    ));
}
