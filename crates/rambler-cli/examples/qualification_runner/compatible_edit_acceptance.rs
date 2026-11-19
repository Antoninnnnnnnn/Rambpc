//! Fixed S3 replies and a test-only editor transaction; no real editor or service.

use super::{
    Check, Failed, Peer, SESSION_DEADLINE, check, collect_result, fixtures, prepare_cases,
    verify_checker, with_peer, write_fixture,
};
use bytes::Bytes;
use rambler_engine::{
    DictationResult, EditingContext, Engine, EngineError, RenderPolicy, ReplacementProposal,
    Revision, TextProvenance, replacement_proposal_with_policy,
};
use rambler_protocol::{
    HYPOTHESIS_SAFETY_STATUS, RECOGNITION_RESPONSE, VOICE_EDITING_RESULT, decode_response,
    wire::{bytes_field as b, string_field as s, varint_field as v},
};
use serde_json::{Value, json};
use tokio::time::timeout;

const POLICY: RenderPolicy = RenderPolicy::S3ClientCompatible;
const POLICY_LABEL: &str = "s3-client-compatible";
const COMMAND: &str = "Rends ça plus poli.";
const BEFORE: &str = "L 🧭 | ";
const ORIGINAL: &str = "e\u{301}🇫🇷👩‍💻";
const RETURNED: &str = "e\u{300}🇫🇮👨‍💻";
const AFTER: &str = " | R";

fn provenance() -> Value {
    let mut labels = super::provenance();
    labels["corpus_id"] = json!("native-compatible-edit-fixtures-v1");
    labels["policy"] = json!(POLICY_LABEL);
    labels
}

fn declaration(negative: bool) -> Value {
    // Independently declared full window: 7 UTF-16 units before the selection,
    // then 2 (accent) + 4 (flag) + 5 (joined emoji). The negative oracle
    // wrongly starts one scalar late, between the base letter and its accent.
    json!({
        "schema": "rambpc-edit-expectations-v1", "artifact_id": "predeclared-compatible-oracle",
        "provenance": provenance(),
        "cases": [
            {"case_id": "selection", "input_id": "selection-input-v1", "intent": 3,
             "outcome": {"kind": "edit", "window": "L 🧭 | e\u{300}🇫🇮👨‍💻 | R",
                         "selection_utf16": [if negative { 8 } else { 7 }, 18],
                         "provenance": "voice_edit_context"}},
            {"case_id": "literal", "input_id": "literal-input-v1", "intent": null,
             "outcome": {"kind": "no_edit"}},
            {"case_id": "rewrite", "input_id": "rewrite-input-v1", "intent": 3,
             "outcome": {"kind": "no_edit"}},
        ],
    })
}

fn final_wire(hypotheses: &[&[u8]]) -> Bytes {
    let primary: Vec<u8> = hypotheses.iter().flat_map(|h| b(3, h).unwrap()).collect();
    let recognition = [v(1, 1).unwrap(), b(3, &primary).unwrap()].concat();
    b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap())
        .unwrap()
        .into()
}

fn structured_hypothesis() -> Vec<u8> {
    let context = [
        s(1, BEFORE).unwrap(),
        s(2, RETURNED).unwrap(),
        s(3, AFTER).unwrap(),
    ]
    .concat();
    let edit = [v(4, 3).unwrap(), b(7, &context).unwrap()].concat();
    [
        s(1, COMMAND).unwrap(),
        b(VOICE_EDITING_RESULT, &edit).unwrap(),
    ]
    .concat()
}

fn cases() -> Vec<fixtures::Case> {
    let mut cases = fixtures::cases();
    cases.retain(|case| matches!(case.id, "literal" | "rewrite" | "selection"));
    let literal = &mut cases[0];
    literal.replies[0] = final_wire(&[&s(1, COMMAND).unwrap()]);
    literal.expected.final_text = COMMAND;
    let selection = &mut cases[2];
    selection.context.before = BEFORE.into();
    selection.context.selected = ORIGINAL.into();
    selection.context.after = AFTER.into();
    selection.replies[0] = final_wire(&[&structured_hypothesis()]);
    selection.expected.intent = Some(3);
    selection.expected.final_text = "L 🧭 | e\u{300}🇫🇮👨‍💻 | R";
    selection.expected.selection_utf16 = Some((7, 18));
    cases
}

fn observe(
    case_id: &str,
    result: &DictationResult,
    revision: Revision,
) -> Check<(Value, Option<ReplacementProposal>)> {
    let unqualified = || {
        (
            json!({"case_id": case_id, "input_id": format!("{case_id}-input-v1"),
                "status": "unqualified"}),
            None,
        )
    };
    let Ok(hypothesis) = result.single_hypothesis() else {
        return Ok(unqualified());
    };
    if result.context.is_none() || result.final_updates[0].recognition_state() != Some(1) {
        return Ok(unqualified());
    }
    let intent = match hypothesis.voice_edit.as_ref() {
        Some(edit) => match edit.intent {
            Some(intent) => Some(intent),
            None => return Ok(unqualified()),
        },
        None => None,
    };
    let (outcome, proposal) = match replacement_proposal_with_policy(result, Some(revision), POLICY)
    {
        Ok(proposal) => {
            let rendered = result.render(POLICY).map_err(|_| Failed)?;
            check(
                result.context.as_ref() == Some(&proposal.snapshot)
                    && rendered.intent == intent
                    && rendered.text == proposal.replacement_text,
            )?;
            let source = match rendered.provenance {
                TextProvenance::HypothesisText => "hypothesis_text",
                TextProvenance::VoiceEditText => "voice_edit_text",
                TextProvenance::VoiceEditContext => "voice_edit_context",
            };
            (
                json!({"kind": "edit", "window": proposal.replacement_text,
                    "selection_utf16": proposal.selection_utf16_range(), "provenance": source}),
                Some(proposal),
            )
        }
        Err(EngineError::UnsupportedEdit) => (json!({"kind": "no_edit"}), None),
        Err(_) => return Ok(unqualified()),
    };
    Ok((
        json!({"case_id": case_id, "input_id": format!("{case_id}-input-v1"),
               "status": "observed", "intent": intent, "outcome": outcome}),
        proposal,
    ))
}

async fn collect_rows(engine: &Engine, peer: &Peer) -> Check<Vec<Value>> {
    let mut rows = Vec::new();
    for (ordinal, step) in peer.invocations.iter().enumerate() {
        let fixture = &peer.cases[step.case_index].fixture;
        let result = timeout(SESSION_DEADLINE, collect_result(engine, peer, ordinal))
            .await
            .map_err(|_| Failed)??;
        let revision = Revision::from(41_i64);
        let (row, proposal) = observe(fixture.id, &result, revision.clone())?;
        // Even the literal command with identical ASR wording never acquires
        // rewrite provenance or authorizes an ASR-only proposal.
        let literal = result.render(RenderPolicy::AsrOnly).map_err(|_| Failed)?;
        check(
            literal.provenance == TextProvenance::HypothesisText
                && (fixture.id == "rewrite" || literal.text == COMMAND),
        )?;
        for policy in [RenderPolicy::AsrOnly, RenderPolicy::Strict] {
            check(matches!(
                replacement_proposal_with_policy(&result, None, policy),
                Err(EngineError::UnsupportedEdit)
            ))?;
        }
        if fixture.id == "selection" {
            verify_host_transaction(
                proposal.as_ref().ok_or(Failed)?,
                &fixture.context,
                &revision,
            )?;
        }
        rows.push(row);
    }
    Ok(rows)
}

// The SDK only knows the text split and revision. This test-only host owns the
// document and anchor checks and deliberately contains an identical second window.
// The fixture compares general scalar and grapheme diffs; for an actual
// selection rewrite, use the selection-anchored review path in minimal_app.
fn verify_host_transaction(
    proposal: &ReplacementProposal,
    context: &EditingContext,
    revision: &Revision,
) -> Check<()> {
    let scalar = proposal
        .derive_scalar_splice(context, revision)
        .map_err(|_| Failed)?
        .ok_or(Failed)?;
    check(scalar.original_utf16_range() == (8..15))?;
    let original = "L 🧭 | e\u{301}🇫🇷👩‍💻 | R";
    let text: Vec<_> = format!("OUT 🧱 {original} MID {original} TAIL")
        .encode_utf16()
        .collect();
    let captured_anchor = 7;
    let apply =
        |buffer: &mut Vec<u16>, document, anchor, current: &EditingContext, rev: &Revision| {
            if document != "captured-doc" || anchor != captured_anchor {
                return Err(EngineError::ContextConflict);
            }
            let splice = proposal
                .derive_grapheme_splice(current, rev)?
                .ok_or(EngineError::ContextConflict)?;
            let range = splice.original_utf16_range();
            if range != (7..18) || splice.inserted_text() != "e\u{300}🇫🇮👨‍💻" {
                return Err(EngineError::ContextConflict);
            }
            let (start, end) = proposal
                .selection_utf16_range()
                .ok_or(EngineError::ContextConflict)?;
            let before: Vec<_> = [&current.before[..], &current.selected, &current.after]
                .concat()
                .encode_utf16()
                .collect();
            if buffer.get(anchor..anchor + before.len()) != Some(before.as_slice()) {
                return Err(EngineError::ContextConflict);
            }
            buffer.splice(
                anchor + range.start..anchor + range.end,
                splice.inserted_text().encode_utf16(),
            );
            Ok((anchor + start, anchor + end))
        };
    let mut changed_split = context.clone();
    changed_split.before.push('e');
    changed_split.selected.remove(0); // same full window, cursor/selection moved
    for (document, anchor, current, rev) in [
        ("other-doc", captured_anchor, context, revision.clone()),
        (
            "captured-doc",
            captured_anchor + 22 + 5,
            context,
            revision.clone(),
        ),
        (
            "captured-doc",
            captured_anchor,
            context,
            Revision::from(42_i64),
        ),
        (
            "captured-doc",
            captured_anchor,
            &changed_split,
            revision.clone(),
        ),
    ] {
        let mut buffer = text.clone();
        check(
            matches!(
                apply(&mut buffer, document, anchor, current, &rev),
                Err(EngineError::ContextConflict)
            ) && buffer == text,
        )?;
    }
    let mut buffer = text;
    let selection = apply(
        &mut buffer,
        "captured-doc",
        captured_anchor,
        context,
        revision,
    )
    .map_err(|_| Failed)?;
    check(
        selection == (14, 25)
            && String::from_utf16(&buffer).map_err(|_| Failed)?
                == format!("OUT 🧱 L 🧭 | e\u{300}🇫🇮👨‍💻 | R MID {original} TAIL"),
    )
}

#[test]
fn ambiguous_or_unsafe_native_results_are_unqualified_not_no_edits() {
    let selection = cases().pop().unwrap();
    let final_update = decode_response(&selection.replies[0]).unwrap();
    let primary = structured_hypothesis();
    let unsafe_hypothesis = [&primary[..], &v(HYPOTHESIS_SAFETY_STATUS, 2).unwrap()].concat();
    for updates in [
        vec![final_update.clone(), final_update], // identical duplicate finals still ambiguous
        vec![decode_response(&final_wire(&[&primary, &primary])).unwrap()],
        vec![decode_response(&final_wire(&[&unsafe_hypothesis])).unwrap()],
    ] {
        let result = DictationResult {
            context: Some(selection.context.clone()),
            final_updates: updates,
            ..Default::default()
        };
        for policy in [RenderPolicy::Strict, RenderPolicy::AsrOnly, POLICY] {
            assert!(matches!(
                replacement_proposal_with_policy(&result, None, policy),
                Err(EngineError::AmbiguousResult | EngineError::UnrecognizedSafety)
            ));
        }
        let (row, proposal) = observe("selection", &result, Revision::from(41_i64))
            .ok()
            .unwrap();
        assert!(proposal.is_none());
        assert_eq!(
            row,
            json!({"case_id": "selection", "input_id": "selection-input-v1",
                              "status": "unqualified"})
        );
    }
}

#[tokio::test]
#[ignore = "requires Python on PATH and owned IPv4 loopback; explicit offline E2E"]
async fn compatible_edit_acceptance_uses_native_proposals() {
    let temp = tempfile::tempdir()
        .ok()
        .expect("private synthetic directory");
    let expected = temp.path().join("compatible-expected.json");
    let negative = temp.path().join("compatible-negative.json");
    let observed = temp.path().join("compatible-observed.json");
    write_fixture(&expected, &declaration(false));
    write_fixture(&negative, &declaration(true));
    let peer = timeout(SESSION_DEADLINE, prepare_cases(cases(), 1))
        .await
        .ok()
        .and_then(Result::ok)
        .expect("prepare the three fixed fixtures");
    let rows = with_peer(peer, collect_rows)
        .await
        .ok()
        .expect("collect linked native results and close the owned peer");
    write_fixture(
        &observed,
        &json!({
            "schema": "rambpc-edit-observations-v1", "artifact_id": "native-compatible-sdk-observation",
            "provenance": provenance(), "cases": rows,
        }),
    );
    for (path, negative) in [(&expected, false), (&negative, true)] {
        let wanted = json!({
            "schema": "rambpc-edit-acceptance-result-v1",
            "scope": "offline-s3-client-compatible-proposal-only",
            "verdict": if negative { "mismatch" } else { "conforming" },
            "counts": {"expected": 3, "conforming_no_edit": 2, "conforming_edit": 0,
                       "conforming_rewrite": usize::from(!negative), "mismatch": usize::from(negative),
                       "missing": 0, "unqualified": 0, "unexpected": 0},
        });
        verify_checker(
            path,
            &observed,
            &["--policy", POLICY_LABEL],
            i32::from(negative),
            wanted,
        )
        .await;
    }
    assert!(temp.close().is_ok(), "remove synthetic artifacts");
}
