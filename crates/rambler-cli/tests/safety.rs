mod common;
#[path = "support/ogg_fixture.rs"]
mod ogg_fixture;
use bytes::Bytes;
use common::{TestServer, bounded, drain, replies};
use rambler_protocol::{
    HYPOTHESIS_SAFETY_STATUS, RECOGNITION_RESPONSE, VOICE_EDITING_RESULT,
    wire::{bytes_field as b, string_field as s, varint_field as v},
};
use serde_json::{Value, json};
use std::{process::Command, time::Duration};

const SELECTION: &str = "Send me the report now.";
const HYPOTHESIS: &str = "<cursor>Send me the report now.</cursor> Make this more polite.";
const REWRITTEN_WINDOW: &str = "Could you please send me the report at your earliest convenience?";
const REWRITE_ARGS: &[&str] = &["--selected", SELECTION, "--cursor-aware"];

fn final_edit(text: &str, edit: &[u8], safety: &[u8]) -> Bytes {
    let hypothesis = [
        s(1, text).unwrap(),
        b(VOICE_EDITING_RESULT, edit).unwrap(),
        safety.to_vec(),
    ]
    .concat();
    final_hypothesis(&hypothesis)
}

fn final_hypothesis(hypothesis: &[u8]) -> Bytes {
    let recognition = [v(1, 1).unwrap(), b(3, &b(3, hypothesis).unwrap()).unwrap()].concat();
    Bytes::from(b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap())
}

fn rewrite_response(safety: &[u8]) -> Bytes {
    let context = [
        s(1, REWRITTEN_WINDOW).unwrap(),
        s(2, "").unwrap(),
        s(3, "").unwrap(),
    ]
    .concat();
    let edit = [v(4, 3).unwrap(), b(7, &context).unwrap()].concat();
    final_edit(HYPOTHESIS, &edit, safety)
}

async fn run_dictate(
    payload: Bytes,
    input_args: &'static [&'static str],
    policy: Option<&'static str>,
) -> (i32, Value) {
    run_dictate_replies(vec![payload], input_args, policy).await
}

async fn run_dictate_replies(
    payloads: Vec<Bytes>,
    input_args: &'static [&'static str],
    policy: Option<&'static str>,
) -> (i32, Value) {
    let server = TestServer::new(move |request| {
        let mut payloads = payloads.clone();
        async move {
            let requests = drain(request.into_inner()).await;
            assert_eq!(requests.last().unwrap().as_ref(), b"\x10\x01\x18\x01");
            payloads.push(Bytes::from_static(b"\x08\x01"));
            Ok(replies(futures_util::stream::iter(
                payloads.into_iter().map(Ok),
            )))
        }
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audio.ogg");
    std::fs::write(&path, ogg_fixture::fixture(3)).unwrap();
    let address = server.address.to_string();
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::task::spawn_blocking(move || {
            let mut command = Command::new(env!("CARGO_BIN_EXE_rambler"));
            command.env_clear();
            if let Some(root) = std::env::var_os("SystemRoot") {
                command.env("SystemRoot", root);
            }
            command
                .arg("dictate")
                .arg(path)
                .args(["--loopback", &address, "--timeout", "3"])
                .args(input_args);
            if let Some(policy) = policy {
                command.args(["--result-policy", policy]);
            }
            command.output().unwrap()
        }),
    )
    .await
    .unwrap()
    .unwrap();
    bounded(server.close()).await;
    assert!(output.stderr.is_empty());
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}

fn safety_status(status: u64) -> Vec<u8> {
    v(HYPOTHESIS_SAFETY_STATUS, status).unwrap()
}

fn assert_safety_error(exit: i32, mut value: Value, category: &str) {
    assert_eq!(exit, 1);
    let timeline = value.as_object_mut().unwrap().remove("timeline").unwrap();
    assert!(timeline["half_close_sent_seconds"].is_number());
    assert_eq!(timeline["saw_s3_end"], true);
    assert!(!timeline.to_string().contains("CONTROLLED"));
    // No text (recognition or edit), provenance, context, or raw candidates.
    assert_eq!(
        value,
        json!({"type":"error", "category":category, "outcome":category, "grpc_code":null,
            "close_error_category":null, "close_grpc_code":null}),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_safety_refusal_emits_no_candidate_context_or_result_and_still_drains_upload() {
    for (safety, category) in [
        (vec![], None),
        (safety_status(0), None),
        (safety_status(1), Some("safety_blocked")),
        (safety_status(2), Some("unrecognized_safety")),
        (
            b(HYPOTHESIS_SAFETY_STATUS, b"OPAQUE SAFETY").unwrap(),
            Some("unrecognized_safety"),
        ),
    ] {
        let edit = [v(4, 2).unwrap(), s(3, "CONTROLLED CANDIDATE").unwrap()].concat();
        let payload = final_edit("CONTROLLED ORIGINAL", &edit, &safety);
        let (exit, value) = run_dictate(payload, &["--before", "CONTROLLED SNAPSHOT"], None).await;
        if let Some(category) = category {
            assert_safety_error(exit, value, category);
        } else {
            assert_eq!(exit, 0, "{value}");
            assert_eq!(value["text"], "CONTROLLED CANDIDATE");
            assert!(value.get("provenance").is_none());
            assert_eq!(
                value["metrics"]["audio"]["bytes"],
                ogg_fixture::fixture(3).len()
            );
            assert_eq!(value["metrics"]["audio"]["eof"], true);
            assert_eq!(value["metrics"]["audio"]["cleanup_succeeded"], true);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_distinguishes_no_finals_empty_asr_unsupported_fallback_and_multiple_finals() {
    let unsupported = [v(4, 2).unwrap(), s(2, "unsupported candidate").unwrap()].concat();
    let missing = final_hypothesis(&b(VOICE_EDITING_RESULT, &unsupported).unwrap());
    for (payloads, policy, outcome, reason) in [
        (vec![], None, "no_result", Some("no_finals")),
        (
            vec![final_hypothesis(&s(1, "").unwrap())],
            None,
            "no_result",
            Some("empty_text"),
        ),
        (
            vec![missing.clone()],
            Some("s3-client-compatible"),
            "no_result",
            Some("missing_text"),
        ),
        (vec![missing], None, "needs_review", None),
        (
            vec![
                final_hypothesis(&s(1, "One.").unwrap()),
                final_hypothesis(&s(1, "Two.").unwrap()),
            ],
            None,
            "needs_review",
            None,
        ),
    ] {
        let count = payloads.len();
        let (exit, value) = run_dictate_replies(payloads, &[], policy).await;
        assert_eq!(exit, 1, "{value}");
        assert_eq!(value["type"], "ambiguous");
        assert_eq!(value["outcome"], outcome);
        assert_eq!(
            value.get("no_result_reason").and_then(Value::as_str),
            reason
        );
        assert!(value["text"].is_null());
        assert!(value.get("provenance").is_none());
        assert_eq!(value["final_updates"].as_array().unwrap().len(), count);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_accepts_explicit_structured_empty_replacement_under_both_edit_policies() {
    let empty_context = [s(1, "").unwrap(), s(2, "").unwrap(), s(3, "").unwrap()].concat();
    let edit = [v(4, 2).unwrap(), b(7, &empty_context).unwrap()].concat();
    for policy in [None, Some("s3-client-compatible")] {
        let payload = final_edit("supprime lundi", &edit, &[]);
        let (exit, value) =
            run_dictate(payload, &["--selected", "lundi", "--cursor-aware"], policy).await;
        assert_eq!(exit, 0);
        assert_eq!(value["type"], "result");
        assert_eq!(value["outcome"], "verified_deletion");
        assert_eq!(value["text"], "");
        assert_eq!(value["context"]["selected"], "lundi");
        assert_eq!(
            value.get("provenance").and_then(Value::as_str),
            policy.map(|_| "voice_edit_context")
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_result_policy_projects_a_field_7_rewrite_only_when_opted_in() {
    for (policy, text, provenance) in [
        (None, None, None),
        (Some("strict"), None, None),
        (Some("asr-only"), Some(HYPOTHESIS), Some("hypothesis_text")),
        (
            Some("s3-client-compatible"),
            Some(REWRITTEN_WINDOW),
            Some("voice_edit_context"),
        ),
    ] {
        let (exit, value) = run_dictate(rewrite_response(&[]), REWRITE_ARGS, policy).await;
        assert_eq!(exit, i32::from(text.is_none()), "{policy:?}: {value}");
        assert_eq!(
            value["type"],
            if text.is_some() {
                "result"
            } else {
                "ambiguous"
            }
        );
        assert_eq!(value["text"], json!(text));
        assert_eq!(value.get("provenance").and_then(Value::as_str), provenance);
        assert_eq!(value.get("provenance").is_some(), provenance.is_some());
        assert_eq!(
            value["context"],
            json!({
                "before": "", "selected": SELECTION, "after": "", "cursor_aware": true,
            })
        );
        assert_eq!(
            value["final_updates"],
            json!([{"hypotheses": [{
                "text": HYPOTHESIS, "confidence": null, "safety_status": null,
                "voice_edit": {
                    "transcript": null, "alternative_text": null, "edited_text": null,
                    "intent": 3, "active_field": 7,
                    "edited_context": {
                        "before": REWRITTEN_WINDOW, "selected": "", "after": "", "cursor_aware": true,
                    },
                },
            }]}])
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_result_policies_all_refuse_blocked_and_unknown_safety_on_field_7_rewrite() {
    for policy in [
        Some("strict"),
        Some("asr-only"),
        Some("s3-client-compatible"),
    ] {
        for (status, category) in [(1, "safety_blocked"), (2, "unrecognized_safety")] {
            let payload = rewrite_response(&safety_status(status));
            let (exit, value) = run_dictate(payload, REWRITE_ARGS, policy).await;
            assert_safety_error(exit, value, category);
        }
    }
}
