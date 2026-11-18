#![cfg(feature = "audio")]
mod common;
use bytes::Bytes;
use common::{TestServer, drain, replies};
use rambler_protocol::{
    AUDIO_DATA,
    wire::{Value, parse_fields},
};
use std::{
    process::Command,
    sync::{Arc, Mutex},
};
use tonic::Status;

fn wav(samples: &[i16]) -> Vec<u8> {
    let length = (samples.len() * 2) as u32;
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
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}
fn command(path: &std::path::Path, addr: &str) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rambler"));
    cmd.env_clear()
        .env("RAMBLER_API_KEY", "fake-key")
        .env("RAMBLER_ANDROID_PACKAGE", "fake-package")
        .env("RAMBLER_ANDROID_CERT", "fake-cert");
    if let Some(root) = std::env::var_os("SystemRoot") {
        cmd.env("SystemRoot", root);
    }
    cmd.arg("dictate-wav").arg(path).args(["--loopback", addr]);
    cmd
}
fn assert_audio(requests: &[Bytes], sample_count: usize) {
    let mut ogg = Vec::new();
    for request in requests {
        for field in parse_fields(request).unwrap() {
            if field.number == AUDIO_DATA
                && let Value::Bytes(data) = field.value
            {
                for inner in parse_fields(&data).unwrap() {
                    if let (1, Value::Bytes(audio)) = (inner.number, inner.value) {
                        ogg.extend(audio);
                    }
                }
            }
        }
    }
    let mut cursor = 0;
    let mut end_granule = 0;
    let mut eos = false;
    while cursor < ogg.len() {
        assert_eq!(&ogg[cursor..cursor + 4], b"OggS");
        let segments = ogg[cursor + 26] as usize;
        let body: usize = ogg[cursor + 27..cursor + 27 + segments]
            .iter()
            .map(|&x| x as usize)
            .sum();
        end_granule = u64::from_le_bytes(ogg[cursor + 6..cursor + 14].try_into().unwrap());
        eos = ogg[cursor + 5] & 4 != 0;
        cursor += 27 + segments + body;
    }
    assert!(eos);
    let mut reader = ogg::reading::PacketReader::new(std::io::Cursor::new(&ogg));
    let head = reader.read_packet().unwrap().unwrap().data;
    assert!(head.starts_with(b"OpusHead"));
    let skip = u16::from_le_bytes(head[10..12].try_into().unwrap()) as usize;
    assert!(
        reader
            .read_packet()
            .unwrap()
            .unwrap()
            .data
            .starts_with(b"OpusTags")
    );
    let mut decoder = opus::Decoder::new(48000, opus::Channels::Mono).unwrap();
    let mut decoded = Vec::new();
    while let Some(packet) = reader.read_packet().unwrap() {
        let mut frame = [0i16; 5760];
        let count = decoder.decode(&packet.data, &mut frame, false).unwrap();
        decoded.extend_from_slice(&frame[..count]);
    }
    assert_eq!(end_granule as usize - skip, sample_count * 3);
    assert!(decoded.len() >= end_granule as usize);
    assert!(decoded[skip..end_granule as usize].iter().any(|s| *s != 0));
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wav_process_encodes_exact_duration_and_rejects_late_failure() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/live_synthetic_responses.json"
    ))
    .unwrap();
    let payload =
        Bytes::from(hex::decode(fixture["cases"][2]["payload_hex"].as_str().unwrap()).unwrap());
    let samples: Vec<i16> = (0..3223)
        .map(|i| ((i as f64 * 0.17).sin() * 10000.0) as i16)
        .collect();
    for mode in 0..3 {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let capture = captured.clone();
        let result = payload.clone();
        let server = TestServer::new(move |request| {
            let capture = capture.clone();
            let result = result.clone();
            async move {
                for header in ["x-goog-api-key", "x-android-package", "x-android-cert"] {
                    assert!(request.metadata().get(header).is_none());
                }
                let requests = drain(request.into_inner()).await;
                *capture.lock().unwrap() = requests;
                let mut replies_list = vec![Ok(result)];
                if mode == 2 {
                    replies_list.push(Ok(Bytes::from_static(b"\x08\x02")));
                } else {
                    replies_list.push(Ok(Bytes::from_static(b"\x08\x01")));
                    if mode == 1 {
                        replies_list.push(Err(Status::internal("private-server-detail")));
                    }
                }
                Ok(replies(futures_util::stream::iter(replies_list)))
            }
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio.wav");
        std::fs::write(&path, wav(&samples)).unwrap();
        let child_path = path.clone();
        let addr = server.address.to_string();
        let start = std::time::Instant::now();
        let output = tokio::task::spawn_blocking(move || {
            let mut cmd = command(&child_path, &addr);
            cmd.args(["--page-duration-ms", "40", "--before", "été🙂"]);
            if mode == 0 {
                cmd.args(["--realtime", "--result-policy", "strict"]);
            }
            cmd.output().unwrap()
        })
        .await
        .unwrap();
        assert!(output.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        if mode != 0 {
            assert!(!output.status.success());
            assert_eq!(value["type"], "error");
            assert_eq!(value["category"], if mode == 1 { "grpc" } else { "s3" });
        } else {
            assert!(start.elapsed() >= std::time::Duration::from_millis(150));
            assert!(output.status.success());
            assert_eq!(value["text"], fixture["cases"][2]["expected"]["text"]);
            assert_eq!(value["provenance"], "voice_edit_text");
        }
        assert!(!String::from_utf8_lossy(&output.stdout).contains("private-server-detail"));
        let requests = captured.lock().unwrap().clone();
        assert_eq!(requests.last().unwrap().as_ref(), b"\x10\x01\x18\x01");
        assert_audio(&requests, samples.len());
        std::fs::remove_file(path).unwrap();
        server.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_wave_and_bad_pages_fail_before_rpc() {
    let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = requests.clone();
    let server = TestServer::new(move |_request| {
        let counter = counter.clone();
        async move {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(replies(futures_util::stream::empty()))
        }
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.wav");
    std::fs::write(&path, b"not wave").unwrap();
    let good = dir.path().join("good.wav");
    let valid = wav(&[1; 320]);
    std::fs::write(&good, &valid).unwrap();
    let truncated = dir.path().join("truncated.wav");
    std::fs::write(&truncated, &valid[..valid.len() - 2]).unwrap();
    let stereo = dir.path().join("stereo.wav");
    let mut bad_format = valid.clone();
    bad_format[22..24].copy_from_slice(&2u16.to_le_bytes());
    std::fs::write(&stereo, bad_format).unwrap();
    for (file, duration) in [
        (path, "20"),
        (truncated, "20"),
        (stereo, "20"),
        (good, "21"),
    ] {
        let addr = server.address.to_string();
        let output = tokio::task::spawn_blocking(move || {
            command(&file, &addr)
                .args(["--page-duration-ms", duration])
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(!output.status.success());
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["type"], "error");
    }
    assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 0);
    server.close().await;
}
