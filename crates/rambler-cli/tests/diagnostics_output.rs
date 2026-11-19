//! The shipped CLI must not expose transcript or editor context in diagnostics mode.
mod common;
#[path = "support/ogg_fixture.rs"]
mod ogg_fixture;

use bytes::Bytes;
use common::{TestServer, drain, replies};
use rambler_protocol::{
    RECOGNITION_RESPONSE,
    wire::{bytes_field as b, string_field as s, varint_field as v},
};
use serde_json::Value;
use std::time::Duration;
use tokio::process::Command;

const TRANSCRIPT: &str = "Synthetic transcript marker.";
const EDITOR_CONTEXT: &str = "Synthetic editor context marker.";

fn final_message(text: &str) -> Bytes {
    let hypothesis = b(3, &s(1, text).unwrap()).unwrap();
    let recognition = [v(1, 1).unwrap(), b(3, &hypothesis).unwrap()].concat();
    Bytes::from(b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap())
}

#[tokio::test]
async fn native_diagnostics_only_retains_status_without_dictation_or_context() {
    for ambiguous in [false, true] {
        let peer = TestServer::new(move |request| async move {
            assert!(!request.metadata().contains_key("x-goog-api-key"));
            let uploaded = drain(request.into_inner()).await;
            assert!(uploaded.len() >= 3);
            let mut frames = vec![Ok(final_message(TRANSCRIPT))];
            if ambiguous {
                frames.push(Ok(final_message("Second synthetic final.")));
            }
            frames.push(Ok(Bytes::from_static(b"\x08\x01")));
            Ok(replies(futures_util::stream::iter(frames)))
        })
        .await;
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("synthetic.ogg");
        std::fs::write(&path, ogg_fixture::fixture(128)).unwrap();
        for diagnostics_only in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_rambler"));
            command.kill_on_drop(true).env_clear();
            if let Some(root) = std::env::var_os("SystemRoot") {
                command.env("SystemRoot", root);
            }
            command.arg("dictate").arg(&path).args([
                "--loopback",
                &peer.address.to_string(),
                "--selected",
                EDITOR_CONTEXT,
                "--cursor-aware",
            ]);
            if diagnostics_only {
                command.arg("--diagnostics-only");
            }
            let output = tokio::time::timeout(Duration::from_secs(12), command.output())
                .await
                .expect("owned CLI test deadline")
                .unwrap();
            assert!(output.stderr.is_empty());
            let stdout = std::str::from_utf8(&output.stdout).unwrap();
            let value: Value = serde_json::from_str(stdout).unwrap();
            assert_eq!(
                output.status.success(),
                !ambiguous,
                "category={:?}; grpc={:?}; type={:?}",
                value["category"].as_str(),
                value["grpc_code"].as_str(),
                value["type"].as_str()
            );
            assert_eq!(
                value["type"],
                if ambiguous { "ambiguous" } else { "result" }
            );
            assert!(value["metrics"]["responses_received"].as_u64().is_some());
            assert_eq!(stdout.contains(TRANSCRIPT), !diagnostics_only);
            assert_eq!(stdout.contains(EDITOR_CONTEXT), !diagnostics_only);
            if diagnostics_only {
                assert_eq!(value["diagnostics_only"], true);
                for private in ["text", "context", "final_updates"] {
                    assert!(value.get(private).is_none());
                }
            }
        }
        peer.close().await;
    }
}
