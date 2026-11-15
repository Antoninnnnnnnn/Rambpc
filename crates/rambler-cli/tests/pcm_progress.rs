#![cfg(feature = "audio")]

#[path = "../../rambler-audio/tests/common/mod.rs"]
mod audio_common;
mod common;

use bytes::Bytes;
use common::{TestServer, drain, replies};
use rambler_engine::{EditingContext, ProtocolConfig};
use rambler_protocol::{
    RECOGNITION_RESPONSE, audio_request, context_request, end_request, initial_request,
    wire::{self, Value, parse_fields},
};
use serde_json::Value as Json;
use std::{
    path::Path,
    process::{Output, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::process::Command;
use tonic::Status;

fn wav(samples: usize) -> Vec<u8> {
    let pcm = audio_common::pcm(samples);
    let length = pcm.len() as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + length).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&16000u32.to_le_bytes());
    bytes.extend_from_slice(&32000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(&pcm);
    bytes
}

fn command(path: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rambler"));
    command.env_clear().kill_on_drop(true).stdin(Stdio::null());
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    command
        .arg("dictate-wav")
        .arg(path)
        .args(["--timeout", "3"]);
    command
}

async fn output(mut command: Command) -> (Output, Json) {
    let output = tokio::time::timeout(Duration::from_secs(8), command.output())
        .await
        .expect("child deadline")
        .unwrap();
    assert!(output.stderr.is_empty());
    assert!(output.stdout.len() <= 16384);
    let value = serde_json::from_slice(&output.stdout).unwrap();
    (output, value)
}

fn assert_shape(value: &Json) {
    let pcm = value["pcm"].as_object().expect("PCM progress object");
    assert_eq!(pcm.len(), 5);
    for name in ["input_bytes", "encoded_input_samples", "encoded_frames"] {
        assert!(pcm[name].as_u64().is_some(), "{name}");
    }
    for name in ["input_eof", "output_eof"] {
        assert!(pcm[name].as_bool().is_some(), "{name}");
    }
}

fn nested(payload: &[u8], number: u32) -> Vec<u8> {
    let fields = parse_fields(payload).unwrap();
    let matches: Vec<_> = fields.iter().filter(|f| f.number == number).collect();
    assert_eq!(matches.len(), 1);
    match &matches[0].value {
        Value::Bytes(bytes) => bytes.clone(),
        _ => panic!("expected bytes"),
    }
}

fn elapsed(payload: &[u8]) -> u64 {
    let fields = parse_fields(&nested(payload, 293100)).unwrap();
    match &fields.iter().find(|field| field.number == 9).unwrap().value {
        Value::Varint(value) => *value,
        _ => panic!("expected elapsed"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn partial_frame_reports_real_samples_and_normal_encoder_eof() {
    let seen = Arc::new(Mutex::new(Vec::<Bytes>::new()));
    let server = TestServer::new({
        let seen = seen.clone();
        move |request| {
            let seen = seen.clone();
            async move {
                let requests = drain(request.into_inner()).await;
                *seen.lock().unwrap() = requests;
                let hypothesis = wire::string_field(1, "invented final").unwrap();
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
                Ok(replies(futures_util::stream::iter([
                    Ok(Bytes::from(final_response)),
                    Ok(Bytes::from_static(b"\x08\x01")),
                ])))
            }
        }
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invented.wav");
    std::fs::write(&path, wav(1017)).unwrap();
    let mut cmd = command(&path);
    cmd.args([
        "--loopback",
        &server.address.to_string(),
        "--before",
        "invented context",
    ]);
    let (output, value) = output(cmd).await;
    assert!(output.status.success(), "{value}");
    assert_shape(&value);
    assert_eq!(value["pcm"]["input_bytes"], 2034);
    assert_eq!(value["pcm"]["encoded_input_samples"], 1017);
    assert!(value["pcm"]["encoded_frames"].as_u64().unwrap() > 0);
    assert_eq!(value["pcm"]["input_eof"], true);
    assert_eq!(value["pcm"]["output_eof"], true);
    let requests = seen.lock().unwrap().clone();
    assert!(requests.len() >= 4);
    assert_eq!(
        requests[0].as_ref(),
        initial_request(&ProtocolConfig::default(), elapsed(&requests[0])).unwrap()
    );
    let context = EditingContext {
        before: "invented context".into(),
        ..Default::default()
    };
    assert_eq!(
        requests[requests.len() - 2].as_ref(),
        context_request(&context).unwrap()
    );
    assert_eq!(requests.last().unwrap().as_ref(), end_request());
    let mut ogg = Vec::new();
    for request in &requests[1..requests.len() - 2] {
        let chunk = nested(&nested(request, 293101), 1);
        assert!(!chunk.is_empty());
        assert_eq!(
            request.as_ref(),
            audio_request(&chunk, elapsed(request)).unwrap()
        );
        ogg.extend(chunk);
    }
    audio_common::assert_duration(&ogg, 1017);
    tokio::time::timeout(Duration::from_secs(5), server.close())
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn early_server_failure_does_not_claim_encoder_eof() {
    let server = TestServer::new(|request| async move {
        let mut stream = request.into_inner();
        assert!(stream.message().await.unwrap().is_some());
        Err(Status::permission_denied("invented-private-detail"))
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invented-long.wav");
    std::fs::write(&path, wav(160000)).unwrap();
    let mut cmd = command(&path);
    cmd.args(["--loopback", &server.address.to_string(), "--realtime"]);
    let (output, value) = output(cmd).await;
    assert!(!output.status.success());
    assert_eq!(value["type"], "error");
    assert_shape(&value);
    assert_eq!(value["pcm"]["output_eof"], false);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("invented-private-detail"));
    tokio::time::timeout(Duration::from_secs(5), server.close())
        .await
        .unwrap();
}
