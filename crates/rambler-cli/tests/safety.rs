mod common;
#[path = "support/ogg_fixture.rs"]
mod ogg_fixture;
use bytes::Bytes;
use common::{TestServer, drain, replies};
use rambler_protocol::{
    RECOGNITION_RESPONSE, VOICE_EDITING_RESULT,
    wire::{bytes_field as b, string_field as s, varint_field as v},
};
use serde_json::Value;
use std::{process::Command, time::Duration};

fn final_edit(safety: &[u8]) -> Bytes {
    let edit = [v(4, 2).unwrap(), s(3, "CONTROLLED CANDIDATE").unwrap()].concat();
    let hypothesis = [
        s(1, "CONTROLLED ORIGINAL").unwrap(),
        b(VOICE_EDITING_RESULT, &edit).unwrap(),
        safety.to_vec(),
    ]
    .concat();
    let recognition = [v(1, 1).unwrap(), b(3, &b(3, &hypothesis).unwrap()).unwrap()].concat();
    Bytes::from(b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_safety_refusal_emits_no_candidate_context_or_result_and_still_drains_upload() {
    for (safety, category) in [
        (vec![], None),
        (v(30, 0).unwrap(), None),
        (v(30, 1).unwrap(), Some("safety_blocked")),
        (v(30, 2).unwrap(), Some("unrecognized_safety")),
        (
            b(30, b"OPAQUE SAFETY").unwrap(),
            Some("unrecognized_safety"),
        ),
    ] {
        let payload = final_edit(&safety);
        let server = TestServer::new(move |request| {
            let payload = payload.clone();
            async move {
                let requests = drain(request.into_inner()).await;
                assert_eq!(requests.last().unwrap().as_ref(), b"\x10\x01\x18\x01");
                Ok(replies(futures_util::stream::iter([
                    Ok(payload),
                    Ok(Bytes::from_static(b"\x08\x01")),
                ])))
            }
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio.ogg");
        let audio = ogg_fixture::fixture(3);
        let audio_bytes = audio.len();
        std::fs::write(&path, audio).unwrap();
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
                    .args([
                        "--loopback",
                        &address,
                        "--timeout",
                        "3",
                        "--before",
                        "CONTROLLED SNAPSHOT",
                    ])
                    .output()
                    .unwrap()
            }),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(output.stderr.is_empty());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        if let Some(category) = category {
            assert!(!output.status.success());
            assert_eq!(value["type"], "error");
            assert_eq!(value["category"], category);
            let text = String::from_utf8(output.stdout).unwrap();
            for forbidden in [
                "CONTROLLED",
                "OPAQUE",
                "final_updates",
                "context",
                "transcript",
                "text",
            ] {
                assert!(
                    !text.contains(forbidden),
                    "safety refusal exposed candidate data"
                );
            }
        } else {
            assert!(output.status.success(), "{value}");
            assert_eq!(value["text"], "CONTROLLED CANDIDATE");
            assert_eq!(value["metrics"]["audio"]["bytes"], audio_bytes);
            assert_eq!(value["metrics"]["audio"]["eof"], true);
            assert_eq!(value["metrics"]["audio"]["cleanup_succeeded"], true);
        }
        tokio::time::timeout(Duration::from_secs(5), server.close())
            .await
            .unwrap();
    }
}
