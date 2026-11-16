//! Executes the coordinator's independent frozen-Python result-policy oracle.
//! Manual results here intentionally do NOT constitute successful session EOF.
use rambler_engine::*;
use rambler_protocol::decode_response;
use serde_json::Value;
use sha2::{Digest, Sha256};

fn context(value: &Value) -> Option<EditingContext> {
    if value.is_null() {
        return None;
    }
    Some(EditingContext {
        before: value["before"].as_str().unwrap_or("").into(),
        selected: value["selected"].as_str().unwrap_or("").into(),
        after: value["after"].as_str().unwrap_or("").into(),
        cursor_aware: value["cursor_aware"].as_bool().unwrap_or(false),
        ..Default::default()
    })
}
fn revision(value: &Value) -> Option<Revision> {
    match value["kind"].as_str() {
        None if value.is_null() => None,
        Some("string") => Some(Revision::String(value["value"].as_str().unwrap().into())),
        Some("integer") => Some(Revision::Integer(
            DecimalRevision::new(value["decimal"].as_str().unwrap()).unwrap(),
        )),
        _ => panic!("unknown oracle revision"),
    }
}
fn error_name(error: &EngineError) -> &'static str {
    match error {
        EngineError::AmbiguousResult => "AmbiguousResultError",
        EngineError::UnsupportedEdit => "UnsupportedEditError",
        EngineError::ContextConflict => "ContextConflictError",
        _ => panic!("unexpected oracle error {error:?}"),
    }
}
#[test]
fn frozen_python_policy_oracle_nineteen_cases() {
    let oracle: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/result_policy_conformance.json"
    ))
    .unwrap();
    assert_eq!(oracle["reference_commit"], "ec39b5d");
    let cases = oracle["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 19);
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let updates = case["response_payloads"]
            .as_array()
            .unwrap()
            .iter()
            .map(|payload| {
                let bytes = hex::decode(payload["hex"].as_str().unwrap()).unwrap();
                assert_eq!(
                    hex::encode(Sha256::digest(&bytes)),
                    payload["sha256"].as_str().unwrap(),
                    "{id}: fixture integrity"
                );
                decode_response(&bytes).unwrap()
            })
            .collect();
        let result = DictationResult {
            final_updates: updates,
            context: context(&case["context"]),
            metrics: None,
        };
        let expected = &case["expected"];
        match result.text() {
            Ok(text) => assert_eq!(
                text,
                expected["text"]["value"].as_str().unwrap(),
                "{id}: text"
            ),
            Err(error) => assert_eq!(
                error_name(&error),
                expected["text"]["error"].as_str().unwrap(),
                "{id}: text error"
            ),
        }
        match replacement_proposal(&result, revision(&case["expected_revision"])) {
            Err(error) => assert_eq!(
                error_name(&error),
                expected["proposal"]["error"].as_str().unwrap(),
                "{id}: proposal error"
            ),
            Ok(proposal) => {
                let expected = &expected["proposal"];
                assert_eq!(
                    proposal.replacement_text,
                    expected["replacement_text"].as_str().unwrap(),
                    "{id}"
                );
                let expected_context = context(&expected["replacement_context"]);
                let view = |value: Option<EditingContext>| {
                    value.map(|value| {
                        (
                            value.before,
                            value.selected,
                            value.after,
                            value.cursor_aware,
                        )
                    })
                };
                assert_eq!(
                    view(proposal.replacement_context.clone()),
                    view(expected_context),
                    "{id}: context"
                );
                let range = expected["selection_utf16_range"].as_array().map(|array| {
                    (
                        array[0].as_u64().unwrap() as usize,
                        array[1].as_u64().unwrap() as usize,
                    )
                });
                assert_eq!(proposal.selection_utf16_range(), range, "{id}: UTF-16");
                for (input, expected) in case["validations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(expected["validations"].as_array().unwrap())
                {
                    let current = context(&input["context"]).unwrap();
                    let revision = revision(&input["revision"]);
                    match proposal.validate_current(&current, revision.as_ref()) {
                        Ok(()) => assert_eq!(expected["valid"], true, "{id}: validation"),
                        Err(error) => assert_eq!(
                            error_name(&error),
                            expected["error"].as_str().unwrap(),
                            "{id}: conflict"
                        ),
                    }
                }
            }
        }
    }
}

#[test]
fn revision_integer_constructor_rejects_noncanonical_spellings() {
    for invalid in ["", "-", "+1", "01", "-01", "-0", "1.0", "true", " 1", "１"] {
        assert!(
            matches!(
                DecimalRevision::new(invalid),
                Err(EngineError::InvalidRevision)
            ),
            "{invalid}"
        );
    }
    for valid in [
        "0",
        "1",
        "-1",
        "1844674407370955161600000000000000000000000000",
    ] {
        assert_eq!(DecimalRevision::new(valid).unwrap().as_str(), valid);
    }
    assert_ne!(Revision::from(1i64), Revision::from("1"));
}
