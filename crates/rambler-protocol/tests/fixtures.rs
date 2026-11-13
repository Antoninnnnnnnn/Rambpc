//! Three original reviewed payloads and ONE DERIVED/SANITIZED rewrite payload.
//! No network, credentials, APK code or Python runtime is used by these tests.
use rambler_protocol::{wire::Value, *};
use serde_json::Value as Json;
use sha2::{Digest, Sha256};

const ORIGINALS: &str = include_str!("../../../tests/fixtures/live_synthetic_responses.json");
const DERIVED: &str = include_str!("../../../tests/fixtures/live_rewrite_response.json");

#[test]
fn bounded_fixture_mutations_and_all_truncations_never_panic() {
    // Small deterministic corpus, not an unbounded fuzz/stress job. Mutations
    // need not remain valid; on success the exact mutated root must survive.
    for document in [ORIGINALS, DERIVED] {
        let fixture: Json = serde_json::from_str(document).unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let payload = hex::decode(case["payload_hex"].as_str().unwrap()).unwrap();
            for end in 0..=payload.len() {
                if let Ok(decoded) = decode_response(&payload[..end]) {
                    assert_eq!(decoded.raw, payload[..end]);
                }
            }
            for position in 0..payload.len() {
                for replacement in [0, 0x80, 0xff] {
                    let mut changed = payload.clone();
                    changed[position] = replacement;
                    if let Ok(decoded) = decode_response(&changed) {
                        assert_eq!(decoded.raw, changed);
                    }
                }
            }
        }
    }
}

#[test]
fn four_payloads_pinned_by_sha256_and_decoded_against_reviewed_expectations() {
    let original: Json = serde_json::from_str(ORIGINALS).unwrap();
    let derived: Json = serde_json::from_str(DERIVED).unwrap();
    assert_eq!(
        original["fixture_kind"],
        "original_unmodified_server_response"
    );
    assert_eq!(original["privacy_review"]["payload_modified"], false);
    assert_eq!(derived["fixture_kind"], "sanitized_derived_server_response");
    assert_eq!(derived["payload_modified"], true);
    let mut cases = original["cases"].as_array().unwrap().clone();
    assert_eq!(cases.len(), 3);
    cases.extend(derived["cases"].as_array().unwrap().iter().cloned());
    let pinned = [
        (
            "correction-en",
            "43fd638823e88612faa6ee8a50614c0ba30ac38bef51f1623fee1178013b9c1e",
        ),
        (
            "context-edit-en",
            "788d7342768e01bc529ef1799182641f10ff954b72b2e21099884672d22f922c",
        ),
        (
            "plain-fr",
            "b8c4e7ce9a84b34f27fd1bebf1201cd1ecd538940962a4b8b2cb243380b70d15",
        ),
        (
            "rewrite-en",
            "0874d1f2021cd27f03bf1fd3c494d9133dedac8a7c97ea061acd84239ea000bb",
        ),
    ];
    assert_eq!(cases.len(), pinned.len());
    for (case, (id, sha)) in cases.iter().zip(pinned) {
        assert_eq!(case["id"], id);
        let payload = hex::decode(case["payload_hex"].as_str().unwrap()).unwrap();
        assert_eq!(hex::encode(Sha256::digest(&payload)), sha);
        assert_eq!(case["payload_sha256"], sha);
        assert_eq!(
            case["payload_bytes"].as_u64().unwrap(),
            payload.len() as u64
        );
        let decoded = decode_response(&payload).unwrap();
        assert_eq!(decoded.raw, payload);
        assert_eq!(decoded.status, None);
        assert_eq!(decoded.recognition_state(), Some(1));
        assert_eq!(decoded.hypotheses().len(), 1);
        assert!(decoded.partials().is_empty());
        let hypothesis = &decoded.hypotheses()[0];
        let voice = hypothesis.voice_edit.as_ref().unwrap();
        let expected = &case["expected"];
        let text_key = if id == "rewrite-en" {
            "hypothesis_text"
        } else {
            "text"
        };
        let transcript_key = if id == "rewrite-en" {
            "voice_transcript"
        } else {
            "transcript"
        };
        assert_eq!(hypothesis.text.as_deref(), expected[text_key].as_str());
        assert_eq!(
            voice.transcript.as_deref(),
            expected[transcript_key].as_str()
        );
        assert_eq!(
            voice.edited_text.as_deref(),
            expected["edited_text"].as_str()
        );
        assert_eq!(
            voice.intent,
            Some(expected["intent"].as_i64().unwrap() as i32)
        );
        assert_eq!(voice.active_field, Some(3));
        assert_eq!(voice.alternative_text, None);
        assert_eq!(voice.edited_context, None);
        let result = decoded.result.as_ref().unwrap();
        assert_eq!(result.active_field, Some(1));
        assert_eq!(
            result.active_raw.as_deref(),
            Some(result.recognition.as_ref().unwrap().raw.as_slice())
        );
        if id == "rewrite-en" {
            assert!(decoded.unknown_fields.is_empty());
            assert!(
                result
                    .recognition
                    .as_ref()
                    .unwrap()
                    .unknown_fields
                    .is_empty()
            );
            // Exact reviewed extension is retained, not regenerated from text.
            assert_eq!(
                hex::encode(Sha256::digest(&voice.raw)),
                derived["source_known_component_sha256"]["voice_editing_result_payload"]
            );
        } else {
            assert_eq!(decoded.unknown_fields.len(), 1);
            assert_eq!(decoded.unknown_fields[0].number, 5);
            assert_eq!(
                decoded.unknown_fields[0].value,
                Value::Varint(
                    expected["top_level_unknown_varint_field5"]
                        .as_u64()
                        .unwrap()
                )
            );
            let recognition = result.recognition.as_ref().unwrap();
            assert_eq!(recognition.unknown_fields[0].number, 5); // opaque duplicate branch
            let list = recognition.hypothesis_result.as_ref().unwrap();
            assert_eq!(
                list.unknown_fields
                    .iter()
                    .map(|f| f.number)
                    .collect::<Vec<_>>(),
                [1, 2]
            );
        }
    }
}
