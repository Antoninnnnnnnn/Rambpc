//! Exercise both new public CLI behaviors together at the binary boundary.
#![cfg(feature = "audio")]

#[path = "../../rambler-engine/tests/common/mod.rs"]
mod fixture;

use bytes::Bytes;
use fixture::{TestServer, drain, replies};
use futures_util::stream;
use rambler_protocol::{
    RECOGNITION_RESPONSE, end_request,
    wire::{bytes_field as b, string_field as s, varint_field as v},
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::process::Command;

const INPUT_SAMPLES: u32 = 67_200; // 1.4 s at 48 kHz, including a second segment.
const OUTPUT_SAMPLES: u32 = INPUT_SAMPLES / 3;
const TEXT: &str = "Synthetic user-visible first segment.";

fn silent_wav() -> Vec<u8> {
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + INPUT_SAMPLES * 2).to_le_bytes());
    wav.extend(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
    wav.extend(48_000u32.to_le_bytes());
    wav.extend(96_000u32.to_le_bytes());
    wav.extend(b"\x02\0\x10\0data");
    wav.extend((INPUT_SAMPLES * 2).to_le_bytes());
    wav.resize(44 + INPUT_SAMPLES as usize * 2, 0);
    wav
}

async fn check_cli(diagnostics: bool) {
    let count = Arc::new(AtomicUsize::new(0));
    let requests = count.clone();
    let peer = TestServer::new(move |request| {
        let index = requests.fetch_add(1, Ordering::SeqCst);
        async move {
            assert!(index < 2, "no unexpected replay or rollover");
            assert!(!request.metadata().contains_key("x-goog-api-key"));
            let upload = drain(request.into_inner()).await;
            assert!(upload.len() > 3);
            assert_eq!(upload.last().unwrap().as_ref(), end_request());
            let mut response = Vec::new();
            if index == 0 {
                let hypothesis = b(3, &s(1, TEXT).unwrap()).unwrap();
                let recognition = [v(1, 1).unwrap(), b(3, &hypothesis).unwrap()].concat();
                response.push(Ok(Bytes::from(
                    b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap(),
                )));
            }
            response.push(Ok(Bytes::from_static(b"\x08\x01")));
            Ok(replies(stream::iter(response)))
        }
    })
    .await;
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("synthetic-48k.wav");
    std::fs::write(&path, silent_wav()).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_rambler"));
    command.kill_on_drop(true).env_clear();
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    command.arg("dictate-wav").arg(&path).args([
        "--interactive",
        "--segment-seconds",
        "6",
        "--loopback",
        &peer.address.to_string(),
    ]);
    if diagnostics {
        command.arg("--diagnostics-only");
    }
    let output = tokio::time::timeout(Duration::from_secs(15), command.output())
        .await
        .expect("owned CLI deadline")
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let text = std::str::from_utf8(&output.stdout).unwrap();
    let result: Value = serde_json::from_str(text).unwrap();
    assert_eq!(result["type"], "ambiguous");
    assert_eq!(result["outcome"], "partial");
    assert!(result["text"].is_null());
    let segments = result["completed_segments"].as_array().unwrap();
    assert_eq!(segments.len(), 2);
    let cut = segments[0]["sample_range"][1].as_u64().unwrap();
    assert!(cut > 0 && cut < u64::from(OUTPUT_SAMPLES));
    assert_eq!(segments[0]["sample_range"], json!([0, cut]));
    assert_eq!(segments[1]["sample_range"], json!([cut, OUTPUT_SAMPLES]));
    assert_eq!(segments[0]["outcome"], "usable_text");
    assert_eq!(segments[1]["outcome"], "no_result");
    assert_eq!(segments[1]["no_result_reason"], "no_finals");
    if diagnostics {
        assert_eq!(result["diagnostics_only"], true);
        assert!(!text.contains(TEXT));
        assert!(result.get("text").is_none());
        assert!(segments.iter().all(|part| part.get("text").is_none()));
    } else {
        assert_eq!(segments[0]["text"], TEXT);
        assert!(segments[1]["text"].is_null());
    }
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert_eq!(peer.accepted_connections(), 1);
    peer.close().await;
}

#[tokio::test]
async fn partial_48k_wav_reports_exact_16k_ranges_and_respects_output_privacy() {
    tokio::join!(check_cli(false), check_cli(true));
}
