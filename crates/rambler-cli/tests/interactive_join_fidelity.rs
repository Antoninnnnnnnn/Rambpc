//! Same synthetic finals through the public SDK and actual interactive CLI.
#![cfg(feature = "audio")]

#[path = "../../rambler-engine/tests/common/mod.rs"]
mod fixture;

use bytes::Bytes;
use fixture::{TestServer, drain, replies};
use futures_util::{future::join_all, stream};
use rambler_audio::{LongDictationOptions, collect_long_pcm_dictation};
use rambler_engine::{CollectorLimits, Engine, SessionOptions, StreamAudio};
use rambler_protocol::{
    RECOGNITION_RESPONSE, end_request,
    wire::{bytes_field as b, string_field as s, varint_field as v},
};
use rambler_transport::TransportConfig;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::process::Command;

#[derive(Clone, Copy)]
struct Case {
    pieces: [&'static str; 2],
    expected: &'static str,
}

async fn peer_for(case: Case) -> (TestServer, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let peer = TestServer::new(move |request| {
        let index = counter.fetch_add(1, Ordering::SeqCst);
        async move {
            assert!(!request.metadata().contains_key("x-goog-api-key"));
            let uploaded = drain(request.into_inner()).await;
            assert!(
                uploaded.len() >= 4,
                "fixture must see a nonempty audio upload"
            );
            assert_eq!(uploaded.last().unwrap().as_ref(), end_request());
            let hypothesis = b(3, &s(1, case.pieces[index]).unwrap()).unwrap();
            let recognition = [v(1, 1).unwrap(), b(3, &hypothesis).unwrap()].concat();
            let final_response = b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap();
            Ok(replies(stream::iter([
                Ok(Bytes::from(final_response)),
                Ok(Bytes::from_static(b"\x08\x01")),
            ])))
        }
    })
    .await;
    (peer, calls)
}

async fn sdk(case: Case) {
    let (peer, calls) = peer_for(case).await;
    let engine = Engine::connect(TransportConfig::loopback(peer.address).unwrap())
        .await
        .unwrap();
    // Unpaced PCM; a one-second sample budget cuts after exactly one second.
    let input = StreamAudio::new(stream::iter([Ok(Bytes::from(vec![0; 40_000]))]));
    let result = collect_long_pcm_dictation(
        &engine,
        input,
        SessionOptions::default(),
        CollectorLimits::default(),
        LongDictationOptions {
            segment_budget: Duration::from_secs(1),
            pause_search: Duration::ZERO,
        },
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(result.segments[0].text().unwrap(), case.pieces[0]);
    assert_eq!(result.segments[1].text().unwrap(), case.pieces[1]);
    assert_eq!(result.text().unwrap(), case.expected);
    engine.close().await.unwrap();
    peer.close().await;
}

fn silent_wav(samples: u32) -> Vec<u8> {
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + samples * 2).to_le_bytes());
    wav.extend(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
    wav.extend(16_000u32.to_le_bytes());
    wav.extend(32_000u32.to_le_bytes());
    wav.extend(b"\x02\0\x10\0data");
    wav.extend((samples * 2).to_le_bytes());
    wav.resize(44 + samples as usize * 2, 0);
    wav
}

async fn cli(case: Case) {
    let (peer, calls) = peer_for(case).await;
    // Six seconds minus the five-second pause-search horizon is one second.
    // A quiet 1.4 s WAV yields two segments without a six-second process wait.
    let samples = 22_400;
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("public-synthetic.wav");
    std::fs::write(&path, silent_wav(samples)).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rambler"))
        .kill_on_drop(true)
        .arg("dictate-wav")
        .arg(path)
        .args([
            "--interactive",
            "--segment-seconds",
            "6",
            "--loopback",
            &peer.address.to_string(),
        ])
        .output()
        .await
        .unwrap();
    assert!(
        out.status.success(),
        "synthetic CLI failed: {:?}",
        out.status
    );
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["type"], "result");
    assert_eq!(result["text"], case.expected);
    let segments = result["completed_segments"].as_array().unwrap();
    assert_eq!(segments.len(), 2);
    for (index, segment) in segments.iter().enumerate() {
        assert_eq!(segment["text"], case.pieces[index]);
    }
    let end_first = segments[0]["sample_range"][1].as_u64().unwrap();
    assert!(end_first > 0 && end_first < u64::from(samples));
    assert_eq!(segments[0]["sample_range"], json!([0, end_first]));
    assert_eq!(segments[1]["sample_range"], json!([end_first, samples]));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(peer.accepted_connections(), 1);
    peer.close().await;
}

#[tokio::test]
async fn cli_and_sdk_apply_the_same_join_without_mutating_segment_text() {
    let cases = [
        Case {
            pieces: ["L’", "équipe arrive."],
            expected: "L’équipe arrive.",
        },
        Case {
            pieces: ["明天", "开会。"],
            expected: "明天开会。",
        },
        Case {
            pieces: ["First.", "\n\nSecond."],
            expected: "First.\n\nSecond.",
        },
        Case {
            pieces: ["one", " two"],
            expected: "one two",
        },
    ];
    tokio::time::timeout(
        Duration::from_secs(18),
        join_all(cases.into_iter().map(|case| async move {
            tokio::join!(sdk(case), cli(case));
        })),
    )
    .await
    .expect("owned SDK/CLI join-qualification deadline");
}
