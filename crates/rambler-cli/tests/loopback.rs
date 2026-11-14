mod common;
#[path = "support/ogg_fixture.rs"]
mod ogg_fixture;
use bytes::Bytes;
use common::{TestServer, drain, replies};
use serde_json::Value;
use std::{
    process::Command,
    sync::{Arc, Mutex},
};
use tonic::Status;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_loopback_cases() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/live_synthetic_responses.json"
    ))
    .unwrap();
    let mut raw = hex::decode(fixture["cases"][2]["payload_hex"].as_str().unwrap()).unwrap();
    raw.extend_from_slice(b"\xa2\x06\x10opaque-secret-id");
    let payload = Bytes::from(raw);
    for mode in 0..8 {
        let received = Arc::new(Mutex::new(Vec::new()));
        let captured = received.clone();
        let response = if mode < 4 {
            payload.clone()
        } else {
            final_edit([0, 4, 6, 999][mode - 4], "candidate")
        };
        let server = TestServer::new(move |request| {
            let captured = captured.clone();
            let response = response.clone();
            async move {
                for header in ["x-goog-api-key", "x-android-package", "x-android-cert"] {
                    assert!(request.metadata().get(header).is_none());
                }
                let requests = drain(request.into_inner()).await;
                *captured.lock().unwrap() = requests;
                let mut items = vec![Ok(response.clone())];
                match mode {
                    0 => items.push(Ok(Bytes::from_static(b"\x08\x01"))),
                    1 => items.push(Err(Status::internal("PRIVATE_SERVER_SECRET"))),
                    2 => items.push(Ok(Bytes::from_static(b"\x08\x02"))),
                    3 => {
                        items.push(Ok(response));
                        items.push(Ok(Bytes::from_static(b"\x08\x01")));
                    }
                    _ => items.push(Ok(Bytes::from_static(b"\x08\x01"))),
                }
                Ok(replies(futures_util::stream::iter(items)))
            }
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio.ogg");
        let audio = ogg_fixture::fixture(128);
        std::fs::write(&path, &audio).unwrap();
        let address = server.address.to_string();
        let output = tokio::task::spawn_blocking(move || {
            let mut command = Command::new(env!("CARGO_BIN_EXE_rambler"));
            command
                .env_clear()
                .env("RAMBLER_API_KEY", "fake-secret-key")
                .env("RAMBLER_ANDROID_PACKAGE", "fake-package")
                .env("RAMBLER_ANDROID_CERT", "fake-cert");
            if let Some(root) = std::env::var_os("SystemRoot") {
                command.env("SystemRoot", root);
            }
            command.arg("dictate").arg(path).args([
                "--loopback",
                &address,
                "--before",
                "été🙂",
                "--selected",
                "choix",
                "--after",
                "fin",
                "--cursor-aware",
                "--bias-term",
                "suggestion",
            ]);
            command.output().unwrap()
        })
        .await
        .unwrap();
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            !text.contains("PRIVATE_SERVER_SECRET")
                && !text.contains("fake-secret-key")
                && !text.contains("opaque-secret-id")
        );
        let value: Value = serde_json::from_str(&text).unwrap();
        if mode == 0 {
            assert!(output.status.success(), "{text}");
            assert_eq!(value["text"], fixture["cases"][2]["expected"]["text"]);
        } else {
            assert!(!output.status.success(), "{text}");
            if mode >= 3 {
                assert_eq!(value["type"], "ambiguous");
                assert_eq!(
                    value["final_updates"].as_array().unwrap().len(),
                    if mode == 3 { 2 } else { 1 }
                );
                if mode >= 4 {
                    assert_eq!(
                        value["final_updates"][0]["hypotheses"][0]["voice_edit"]["intent"],
                        [0, 4, 6, 999][mode - 4]
                    );
                }
            } else {
                assert_eq!(value["type"], "error");
            }
        }
        let requests = received.lock().unwrap().clone();
        assert_eq!(requests.last().unwrap().as_ref(), b"\x10\x01\x18\x01");
        assert_eq!(uploaded_audio(&requests), audio);
        let all = requests.concat();
        assert!(all.windows("été🙂".len()).any(|x| x == "été🙂".as_bytes()));
        assert!(all.windows(10).any(|x| x == b"suggestion"));
        drop(requests);
        server.close().await;
    }
}

fn uploaded_audio(requests: &[Bytes]) -> Vec<u8> {
    use rambler_protocol::{
        AUDIO_DATA,
        wire::{Value, parse_fields},
    };
    let mut audio = Vec::new();
    for request in requests {
        for field in parse_fields(request).unwrap() {
            if field.number == AUDIO_DATA
                && let Value::Bytes(data) = field.value
            {
                for inner in parse_fields(&data).unwrap() {
                    if let (1, Value::Bytes(bytes)) = (inner.number, inner.value) {
                        audio.extend(bytes);
                    }
                }
            }
        }
    }
    audio
}

fn final_edit(intent: u64, text: &str) -> Bytes {
    use rambler_protocol::{
        RECOGNITION_RESPONSE, VOICE_EDITING_RESULT,
        wire::{bytes_field as b, string_field as s, varint_field as v},
    };
    let edit = [v(4, intent).unwrap(), s(3, text).unwrap()].concat();
    let hypothesis = [
        s(1, "Existing text.").unwrap(),
        b(VOICE_EDITING_RESULT, &edit).unwrap(),
    ]
    .concat();
    let recognition = [v(1, 1).unwrap(), b(3, &b(3, &hypothesis).unwrap()).unwrap()].concat();
    Bytes::from(b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_edited_text_is_successful_deletion() {
    let payload = final_edit(2, "");
    let received = Arc::new(Mutex::new(Vec::new()));
    let captured = received.clone();
    let server = TestServer::new(move |request| {
        let payload = payload.clone();
        let captured = captured.clone();
        async move {
            let requests = drain(request.into_inner()).await;
            *captured.lock().unwrap() = requests;
            Ok(replies(futures_util::stream::iter([
                Ok(payload),
                Ok(Bytes::from_static(b"\x08\x01")),
            ])))
        }
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audio.ogg");
    std::fs::write(&path, ogg_fixture::fixture(3)).unwrap();
    let child_path = path.clone();
    let address = server.address.to_string();
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::task::spawn_blocking(move || {
            let mut command = Command::new(env!("CARGO_BIN_EXE_rambler"));
            command
                .env_clear()
                .env("RAMBLER_API_KEY", "fake-secret-key")
                .env("RAMBLER_ANDROID_PACKAGE", "fake-package")
                .env("RAMBLER_ANDROID_CERT", "fake-cert");
            if let Some(root) = std::env::var_os("SystemRoot") {
                command.env("SystemRoot", root);
            }
            command
                .arg("dictate")
                .arg(child_path)
                .args(["--loopback", &address, "--before", "Existing text."])
                .output()
                .unwrap()
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["type"], "result");
    assert_eq!(result["text"], "");
    assert_eq!(
        result["final_updates"][0]["hypotheses"][0]["voice_edit"]["edited_text"],
        ""
    );
    assert_eq!(result["context"]["before"], "Existing text.");
    assert!(result["metrics"]["eof_seconds"].is_number());
    let requests = received.lock().unwrap().clone();
    assert_eq!(requests.last().unwrap().as_ref(), b"\x10\x01\x18\x01");
    std::fs::remove_file(path).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), server.close())
        .await
        .unwrap();
}
