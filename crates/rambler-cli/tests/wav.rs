#![cfg(feature = "audio")]
mod common;
use bytes::Bytes;
use common::{TestServer, drain, replies};
use futures_util::stream;
use rambler_audio::{EncoderConfig, Pcm16Opus, Pcm48kTo16k};
use rambler_engine::{AudioSource, StreamAudio};
use rambler_protocol::{
    AUDIO_DATA, end_request,
    wire::{Value, parse_fields},
};
use std::{
    io::Cursor,
    path::Path,
    process::Output,
    sync::{Arc, Mutex, atomic::Ordering},
    time::{Duration, Instant},
};
use tokio::process::Command;
use tonic::Status;

fn wav(samples: &[i16], rate: u32, channels: u16, format: u16, width: u16) -> Vec<u8> {
    let length = (samples.len() * 2) as u32;
    let align = channels * (width / 8);
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((36 + length).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(format.to_le_bytes());
    bytes.extend(channels.to_le_bytes());
    bytes.extend(rate.to_le_bytes());
    bytes.extend((rate * u32::from(align)).to_le_bytes());
    bytes.extend(align.to_le_bytes());
    bytes.extend(width.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(length.to_le_bytes());
    bytes.extend(pcm(samples));
    bytes
}

fn pcm(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

async fn run(path: &Path, server: &TestServer, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rambler"));
    cmd.kill_on_drop(true)
        .current_dir(path.parent().unwrap())
        .env_clear()
        .env("RAMBLER_API_KEY", "fake-key")
        .env("RAMBLER_ANDROID_PACKAGE", "fake-package")
        .env("RAMBLER_ANDROID_CERT", "fake-cert");
    if let Some(root) = std::env::var_os("SystemRoot") {
        cmd.env("SystemRoot", root);
    }
    cmd.arg("dictate-wav")
        .arg(path)
        .args(["--loopback", &server.address.to_string()])
        .args(args);
    tokio::time::timeout(Duration::from_secs(10), cmd.output())
        .await
        .expect("CLI test deadline")
        .unwrap()
}

fn ogg(requests: &[Bytes]) -> Vec<u8> {
    assert_eq!(requests.last().unwrap().as_ref(), end_request());
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
    ogg
}

fn packets(ogg: &[u8]) -> Vec<(Vec<u8>, u64, bool)> {
    let mut reader = ogg::reading::PacketReader::new(Cursor::new(ogg));
    let mut packets = Vec::new();
    while let Some(packet) = reader.read_packet().unwrap() {
        let eos = packet.last_in_stream();
        let granule = packet.absgp_page();
        packets.push((packet.data, granule, eos));
    }
    packets
}

fn assert_duration(packets: &[(Vec<u8>, u64, bool)], sample_count: usize) -> (usize, usize) {
    let head = &packets[0].0;
    assert!(head.starts_with(b"OpusHead"));
    assert_eq!(head[9], 1);
    assert_eq!(u32::from_le_bytes(head[12..16].try_into().unwrap()), 16_000);
    assert!(packets[1].0.starts_with(b"OpusTags"));
    let skip = u16::from_le_bytes(head[10..12].try_into().unwrap()) as usize;
    let (_, granule, eos) = packets.last().unwrap();
    assert!(*eos);
    assert!(!packets[..packets.len() - 1].iter().any(|packet| packet.2));
    assert_eq!(*granule as usize, skip + sample_count * 3);
    (skip, *granule as usize)
}

async fn collect(mut source: impl AudioSource) -> Vec<u8> {
    let mut output = Vec::new();
    while let Some(bytes) = source.next().await.unwrap() {
        output.extend(bytes);
    }
    source.close().await.unwrap();
    output
}

fn samples_source(data: Vec<u8>) -> impl AudioSource {
    StreamAudio::new(stream::iter([Ok(Bytes::from(data))]))
}

async fn reference_packets(data: &[u8], page_duration_ms: u16) -> Vec<(Vec<u8>, u64, bool)> {
    packets(
        &collect(Pcm16Opus::new(
            samples_source(data.to_vec()),
            EncoderConfig {
                page_duration_ms,
                ..Default::default()
            },
        ))
        .await,
    )
}

fn fixture() -> (serde_json::Value, Bytes) {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/live_synthetic_responses.json"
    ))
    .unwrap();
    let case = fixture["cases"][2].clone();
    let payload = Bytes::from(hex::decode(case["payload_hex"].as_str().unwrap()).unwrap());
    (case, payload)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wav_process_encodes_exact_duration_and_rejects_late_failure() {
    let (case, payload) = fixture();
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
                Ok(replies(stream::iter(replies_list)))
            }
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio.wav");
        std::fs::write(&path, wav(&samples, 16_000, 1, 1, 16)).unwrap();
        let start = Instant::now();
        let mut args = vec!["--page-duration-ms", "40", "--before", "été🙂"];
        if mode == 0 {
            args.extend(["--realtime", "--result-policy", "strict"]);
        }
        let output = run(&path, &server, &args).await;
        assert!(output.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        if mode != 0 {
            assert!(!output.status.success());
            assert_eq!(value["type"], "error");
            assert_eq!(value["category"], if mode == 1 { "grpc" } else { "s3" });
        } else {
            assert!(start.elapsed() >= Duration::from_millis(150));
            assert!(output.status.success());
            assert_eq!(value["text"], case["expected"]["text"]);
            assert_eq!(value["provenance"], "voice_edit_text");
        }
        assert!(!String::from_utf8_lossy(&output.stdout).contains("private-server-detail"));
        let requests = captured.lock().unwrap().clone();
        let packets = packets(&ogg(&requests));
        let (skip, granule) = assert_duration(&packets, samples.len());
        let mut decoder = opus::Decoder::new(48_000, opus::Channels::Mono).unwrap();
        let mut decoded = Vec::new();
        for (packet, _, _) in &packets[2..] {
            let mut frame = [0i16; 5760];
            let count = decoder.decode(packet, &mut frame, false).unwrap();
            decoded.extend_from_slice(&frame[..count]);
        }
        assert!(decoded[skip..granule].iter().any(|s| *s != 0));
        std::fs::remove_file(path).unwrap();
        server.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_wave_and_bad_pages_fail_before_admission() {
    let server = TestServer::new(|_| async { Ok(replies(stream::empty())) }).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.wav");
    let valid = wav(&[1; 320], 48_000, 1, 1, 16);
    for data in [
        b"not wave".to_vec(),
        valid[..valid.len() - 2].to_vec(),
        wav(&[1; 320], 44_100, 1, 1, 16),
        wav(&[1; 320], 48_000, 2, 1, 16),
        wav(&[1; 320], 48_000, 1, 3, 32),
    ] {
        std::fs::write(&path, data).unwrap();
        for args in [&[][..], &["--interactive"][..]] {
            let output = run(&path, &server, args).await;
            assert!(!output.status.success());
            let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(result["type"], "error");
            assert_eq!(result["category"], "audio");
        }
    }
    std::fs::write(&path, valid).unwrap();
    let output = run(&path, &server, &["--page-duration-ms", "21"]).await;
    assert!(!output.status.success());
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["category"], "configuration");
    assert_eq!(server.accepted_connections.load(Ordering::SeqCst), 0);
    server.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wav_48k_normalizes_before_encoding_and_interactive_rollover() {
    for args in [
        &["--page-duration-ms", "40"][..],
        &["--realtime", "--page-duration-ms", "40"][..],
        &["--interactive", "--segment-seconds", "6"][..],
    ] {
        let interactive = args.contains(&"--interactive");
        let sample_count = if interactive { 76_802 } else { 9_602 };
        let samples: Vec<i16> = (0..sample_count)
            .map(|i| {
                // A quiet but nonzero first 1.25 s permits a pause cut near
                // 1.1 s; the cut still crosses a live FIR history.
                let amplitude = if interactive && i < 60_000 {
                    220
                } else {
                    24_000
                };
                (i % 171 - 85) as i16 * (amplitude / 85)
            })
            .collect();
        let normalized = collect(Pcm48kTo16k::new(samples_source(pcm(&samples)))).await;
        let expected_samples = samples.len().div_ceil(3);
        assert_eq!(normalized.len(), expected_samples * 2);

        let (_, payload) = fixture();
        let uploads = Arc::new(Mutex::new(Vec::new()));
        let captured = uploads.clone();
        let server = TestServer::new(move |request| {
            let payload = payload.clone();
            let captured = captured.clone();
            async move {
                assert!(request.metadata().get("x-goog-api-key").is_none());
                let stream = ogg(&drain(request.into_inner()).await);
                captured.lock().unwrap().push(stream);
                Ok(replies(futures_util::stream::iter([
                    Ok(payload),
                    Ok(Bytes::from_static(b"\x08\x01")),
                ])))
            }
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("48k.wav");
        std::fs::write(&path, wav(&samples, 48_000, 1, 1, 16)).unwrap();
        let output = run(&path, &server, args).await;
        assert!(output.stderr.is_empty());
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(output.status.success(), "{result}");
        assert_eq!(result["type"], "result");
        let streams = std::mem::take(&mut *uploads.lock().unwrap());
        assert_eq!(server.accepted_connections.load(Ordering::SeqCst), 1);
        if interactive {
            let segments = result["completed_segments"].as_array().unwrap();
            assert_eq!(streams.len(), 2);
            assert_eq!(segments.len(), streams.len());
            let mut cursor = 0;
            for (segment, stream) in segments.iter().zip(streams) {
                assert_eq!(segment["sample_range"][0], cursor);
                let end = segment["sample_range"][1].as_u64().unwrap() as usize;
                assert!(cursor < end && end <= expected_samples);
                let actual = packets(&stream);
                assert_duration(&actual, end - cursor);
                let expected = reference_packets(&normalized[cursor * 2..end * 2], 20).await;
                if cursor != 0 {
                    // Verify this fixture detects restarting the FIR at rollover.
                    let restarted = collect(Pcm48kTo16k::new(samples_source(pcm(
                        &samples[cursor * 3..]
                    ))))
                    .await;
                    assert_ne!(expected, reference_packets(&restarted, 20).await);
                }
                assert_eq!(actual, expected);
                cursor = end;
            }
            assert_eq!(cursor, expected_samples);
        } else {
            assert_eq!(streams.len(), 1);
            assert_eq!(result["pcm"]["input_bytes"], expected_samples * 2);
            assert_eq!(result["pcm"]["encoded_input_samples"], expected_samples);
            assert_eq!(result["pcm"]["input_eof"], true);
            assert_eq!(result["pcm"]["output_eof"], true);
            let actual = packets(&streams[0]);
            assert_duration(&actual, expected_samples);
            assert_eq!(actual, reference_packets(&normalized, 40).await);
        }
        server.close().await;
    }
}
