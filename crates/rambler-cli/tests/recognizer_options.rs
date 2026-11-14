mod common;
#[path = "support/ogg_fixture.rs"]
mod ogg_fixture;
use bytes::Bytes;
use common::{TestServer, replies};
use rambler_protocol::{RECOGNITION_RESPONSE, RECOGNIZER_INFO, wire};
use std::{
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn optional_cli_preferences_reach_init_and_defaults_remain_absent() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let received = captured.clone();
    let hypothesis = wire::string_field(1, "ok").unwrap();
    let recognition = [
        wire::varint_field(1, 1).unwrap(),
        wire::bytes_field(3, &wire::bytes_field(3, &hypothesis).unwrap()).unwrap(),
    ]
    .concat();
    let final_response = wire::bytes_field(
        RECOGNITION_RESPONSE,
        &wire::bytes_field(1, &recognition).unwrap(),
    )
    .unwrap();
    let server = TestServer::new(move |request| {
        let received = received.clone();
        let final_response = final_response.clone();
        async move {
            let mut requests = request.into_inner();
            let first = requests.message().await.unwrap().unwrap();
            received.lock().unwrap().push(first);
            while requests.message().await.unwrap().is_some() {}
            Ok(replies(futures_util::stream::iter([
                Ok(Bytes::from(final_response)),
                Ok(Bytes::from_static(b"\x08\x01")),
            ])))
        }
    })
    .await;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.ogg");
    std::fs::write(&path, ogg_fixture::fixture(1)).unwrap();
    for args in [
        Vec::<&str>::new(),
        vec![
            "--block-offensive-words",
            "false",
            "--auto-punctuation",
            "true",
            "--secondary-routing",
            "offline-secondary",
        ],
        vec![
            "--block-offensive-words",
            "true",
            "--auto-punctuation",
            "false",
        ],
    ] {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_rambler"));
        command.env_clear().kill_on_drop(true).stdin(Stdio::null());
        if let Some(root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", root);
        }
        let output = tokio::time::timeout(
            Duration::from_secs(10),
            command
                .arg("dictate")
                .arg(&path)
                .args(["--loopback", &server.address.to_string(), "--timeout", "5"])
                .args(args)
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["text"], "ok");
    }
    server.close().await;
    let captured = captured.lock().unwrap();
    assert_eq!(captured.len(), 3);
    for (index, request) in captured.iter().enumerate() {
        let root = wire::parse_fields(request).unwrap();
        let recognizer = root.iter().find(|f| f.number == RECOGNIZER_INFO).unwrap();
        let wire::Value::Bytes(body) = &recognizer.value else {
            panic!("not a message")
        };
        let fields = wire::parse_fields(body).unwrap();
        for (number, expected) in [
            (7, [None, Some(0), Some(2)][index]),
            (40, [None, Some(1), Some(0)][index]),
        ] {
            let field = fields.iter().find(|f| f.number == number);
            assert_eq!(
                field.map(|f| &f.value),
                expected.as_ref().map(|v| wire::Value::Varint(*v)).as_ref()
            );
        }
        let secondary = fields.iter().find(|f| f.number == 64);
        if index == 1 {
            assert_eq!(
                secondary.unwrap().value,
                wire::Value::Bytes(b"offline-secondary".to_vec())
            );
        } else {
            assert!(secondary.is_none());
        }
    }
}
