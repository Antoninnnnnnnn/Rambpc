#![cfg(feature = "audio")]

#[path = "../../rambler-audio/tests/common/mod.rs"]
mod audio;
mod common;

use bytes::Bytes;
use common::{TestServer, drain, replies};
use futures_util::stream;
use rambler_protocol::{
    AUDIO_DATA, HYPOTHESIS_SAFETY_STATUS, RECOGNITION_RESPONSE, VOICE_EDITING_RESULT, end_request,
    wire::{
        Value as WireValue, bytes_field as b, parse_fields, string_field as s, varint_field as v,
    },
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::process::Command;

const FIRST: &str = "Synthetic first phrase.";
const LATE: &str = "Synthetic late phrase.";
const UNSUPPORTED: &str = "PRIVATE_UNSUPPORTED_CANDIDATE";
const UNCOMMITTED: &str = "PRIVATE_UNCOMMITTED_CANDIDATE";
const REJECTED: &str = "PRIVATE_SAFETY_CANDIDATE";
const SAMPLES: usize = 56_000;

fn oga(requests: &[Bytes]) -> Vec<u8> {
    let (end, body) = requests[1..].split_last().unwrap();
    assert_eq!(end.as_ref(), end_request());
    let mut result = Vec::new();
    for req in body {
        let fields = parse_fields(req).unwrap();
        let WireValue::Bytes(ref wrapper) = fields
            .iter()
            .find(|f| f.number == AUDIO_DATA)
            .unwrap()
            .value
        else {
            panic!("expected audio data");
        };
        let inner = parse_fields(wrapper).unwrap();
        let WireValue::Bytes(data) = &inner[0].value else {
            panic!("expected Ogg bytes");
        };
        result.extend_from_slice(data);
    }
    result
}

fn final_response(index: usize, safety: u64, late_failure: bool) -> Bytes {
    let hypothesis = match index {
        0 => s(1, FIRST).unwrap(),
        1 => s(1, "").unwrap(),
        2 if safety != 0 => [
            s(1, REJECTED).unwrap(),
            v(HYPOTHESIS_SAFETY_STATUS, safety).unwrap(),
        ]
        .concat(),
        2 => b(
            VOICE_EDITING_RESULT,
            &[v(4, 2).unwrap(), s(2, UNSUPPORTED).unwrap()].concat(),
        )
        .unwrap(),
        3 if late_failure => s(1, UNCOMMITTED).unwrap(),
        3 => s(1, LATE).unwrap(),
        _ => panic!("unexpected extra RPC or replay"),
    };
    let recognition = [v(1, 1).unwrap(), b(3, &b(3, &hypothesis).unwrap()).unwrap()].concat();
    Bytes::from(b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap())
}

async fn scenario(
    policy: &str,
    gap_outcome: &str,
    safety: u64,
    diagnostics: bool,
    late_failure: bool,
) {
    let uploads = Arc::new(Mutex::new(Vec::new()));
    let peer = TestServer::new({
        let uploads = Arc::clone(&uploads);
        move |request| {
            let uploads = Arc::clone(&uploads);
            async move {
                assert!(!request.metadata().contains_key("x-goog-api-key"));
                let audio = oga(&drain(request.into_inner()).await);
                let index = {
                    let mut uploads = uploads.lock().unwrap();
                    let index = uploads.len();
                    uploads.push(audio);
                    index
                };
                let mut responses = vec![
                    Ok(final_response(index, safety, late_failure)),
                    Ok(Bytes::from_static(b"\x08\x01")),
                ];
                if late_failure && index == 3 {
                    responses.push(Err(tonic::Status::internal("owned synthetic failure")));
                }
                Ok(replies(stream::iter(responses)))
            }
        }
    })
    .await;

    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("synthetic-mixed.wav");
    // Six-second cap with a five-second quiet search window yields three short
    // rollovers. The mock responses are fixture data, not claims about PCM.
    std::fs::write(&path, audio::wav(&vec![0; SAMPLES * 2])).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_rambler"));
    command.env_clear().env("TOKIO_WORKER_THREADS", "2");
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    command
        .current_dir(folder.path())
        .kill_on_drop(true)
        .arg("dictate-wav")
        .arg(path)
        .args([
            "--interactive",
            "--segment-seconds",
            "6",
            "--loopback",
            &peer.address.to_string(),
            "--result-policy",
            policy,
        ]);
    if diagnostics {
        command.arg("--diagnostics-only");
    }
    let output = command.output().await.unwrap();
    assert!(output.stderr.is_empty());
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let segments = report["completed_segments"].as_array().unwrap();
    assert_eq!(segments.len(), if late_failure { 3 } else { 4 });
    {
        let uploads = uploads.lock().unwrap();
        assert_eq!(uploads.len(), 4);
        let mut start = 0;
        for (segment, upload) in segments.iter().zip(uploads.iter()) {
            let end = segment["sample_range"][1].as_u64().unwrap() as usize;
            assert!(end > start);
            assert_eq!(segment["sample_range"], json!([start, end]));
            // Verify the extent against the actual RPC's Ogg granules, not
            // against the variable wall-clock position of a quiet cut.
            audio::assert_duration(upload, end - start);
            start = end;
        }
        if late_failure {
            assert!(start < SAMPLES);
            audio::assert_duration(&uploads[3], SAMPLES - start);
        } else {
            assert_eq!(start, SAMPLES);
        }
    }
    assert_eq!(
        peer.accepted_connections
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    common::bounded(peer.close()).await;

    assert_eq!(segments[0]["outcome"], "usable_text");
    assert_eq!(segments[1]["outcome"], "no_result");
    assert_eq!(segments[1]["no_result_reason"], "empty_text");
    assert_eq!(segments[2]["outcome"], gap_outcome);
    if late_failure {
        assert_eq!(report["type"], "error");
        assert_eq!(report["category"], "grpc");
        assert_eq!(report["grpc_code"], "Internal");
        assert!(report.get("outcome").is_none());
        assert!(report.get("text").is_none());
    } else {
        assert_eq!(segments[3]["outcome"], "usable_text");
    }
    if safety == 0 && !late_failure {
        assert_eq!(report["type"], "ambiguous");
        assert_eq!(report["outcome"], "partial");
        if gap_outcome == "no_result" {
            assert_eq!(segments[2]["no_result_reason"], "missing_text");
        }
        if !diagnostics {
            assert_eq!(report.get("text"), Some(&Value::Null));
            assert_eq!(segments[1].get("text"), Some(&Value::Null));
            assert_eq!(segments[2].get("text"), Some(&Value::Null));
        }
    } else if safety != 0 {
        assert_eq!(report["type"], "error");
        assert_eq!(report["outcome"], gap_outcome);
        assert_eq!(report["category"], gap_outcome);
        assert!(report.get("text").is_none());
        assert_eq!(segments[2]["category"], gap_outcome);
        for key in ["text", "final_updates", "context", "provenance"] {
            assert!(segments[2].get(key).is_none());
        }
    }
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains(REJECTED));
    assert!(!stdout.contains(UNCOMMITTED));
    if diagnostics {
        assert_eq!(report["diagnostics_only"], true);
        for marker in [
            FIRST,
            LATE,
            UNSUPPORTED,
            "\"text\"",
            "\"final_updates\"",
            "\"context\"",
        ] {
            assert!(!stdout.contains(marker));
        }
    } else {
        assert_eq!(segments[0]["text"], FIRST);
        if !late_failure {
            assert_eq!(segments[3]["text"], LATE);
        }
    }
}

#[tokio::test]
async fn interactive_cli_reports_non_contiguous_text_without_success_or_safety_leaks() {
    tokio::time::timeout(Duration::from_secs(25), async {
        tokio::join!(
            scenario("strict", "needs_review", 0, false, false),
            scenario("strict", "needs_review", 0, false, true),
            scenario("asr-only", "no_result", 0, true, false),
            scenario("s3-client-compatible", "no_result", 0, false, false),
            scenario("asr-only", "safety_blocked", 1, false, false),
            scenario(
                "s3-client-compatible",
                "unrecognized_safety",
                2,
                false,
                false
            ),
        );
    })
    .await
    .unwrap();
}
