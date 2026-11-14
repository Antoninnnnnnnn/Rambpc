use super::*;
#[path = "../../tests/common/mod.rs"]
mod common;
use common::{TestServer, bounded, drain, replies};
use std::sync::atomic::{AtomicUsize, Ordering};

fn wave(pcm: &[u8]) -> Vec<u8> {
    let mut out = b"RIFF".to_vec();
    out.extend((36_u32 + pcm.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16_u32.to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(16000_u32.to_le_bytes());
    out.extend(32000_u32.to_le_bytes());
    out.extend(2_u16.to_le_bytes());
    out.extend(16_u16.to_le_bytes());
    out.extend(b"data");
    out.extend((pcm.len() as u32).to_le_bytes());
    out.extend(pcm);
    out
}
fn response() -> Bytes {
    use rambler_protocol::{
        RECOGNITION_RESPONSE,
        wire::{bytes_field as b, string_field as s, varint_field as v},
    };
    let hyp = s(1, "Hello, controlled source.").unwrap();
    let recognition = [v(1, 1).unwrap(), b(3, &b(3, &hyp).unwrap()).unwrap()].concat();
    Bytes::from(b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap())
}
fn args(manifest: PathBuf, loopback: Option<SocketAddr>) -> Args {
    Args {
        manifest,
        loopback,
        allow_live: false,
        timeout: 3,
        realtime: false,
    }
}

#[tokio::test]
async fn live_gate_and_nonloopback_reject_before_files_or_connection() {
    let (report, ok) = run(args(PathBuf::from("missing-private-name"), None)).await;
    assert!(!ok);
    assert_eq!(report["status"], "rejected");
    let (report, ok) = run(args(
        PathBuf::from("missing-private-name"),
        Some("192.0.2.1:9999".parse().unwrap()),
    ))
    .await;
    assert!(!ok);
    assert_eq!(report["category"], "configuration");
    assert!(!report.to_string().contains("private-name"));
}

#[test]
fn markers_match_whole_words_without_silent_prefix_acceptance() {
    assert!(marker_found("Hello, WORLD!", "hello world"));
    assert!(!marker_found("hello worlds", "hello world"));
    assert!(!marker_found("shelloworld", "hello world"));
}

#[tokio::test]
async fn one_physical_tcp_for_three_sessions_and_stop_without_replay_on_inconclusive() {
    // Healthy, marker mismatch, PCM hash mismatch, late gRPC failure.
    for mode in 0..6 {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let server = TestServer::new(move |request| {
            let observed = observed.clone();
            async move {
                for name in ["x-goog-api-key", "x-android-package", "x-android-cert"] {
                    assert!(request.metadata().get(name).is_none());
                }
                let ordinal = observed.fetch_add(1, Ordering::SeqCst);
                let requests = drain(request.into_inner()).await;
                assert_eq!(requests.last().unwrap().as_ref(), b"\x10\x01\x18\x01");
                let terminal = if mode == 3 && ordinal == 1 {
                    Err(tonic::Status::unavailable("PRIVATE SERVER DETAILS"))
                } else {
                    Ok(Bytes::from_static(b"\x08\x01"))
                };
                Ok(replies(futures_util::stream::iter([
                    Ok(response()),
                    terminal,
                ])))
            }
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let pcm = vec![0; 321 * 2];
        std::fs::write(dir.path().join("private-audio.wav"), wave(&pcm)).unwrap();
        let mut cases = vec![
            json!({"input":"private-audio.wav", "language":"en-US",
            "pcm_sha256":hash(&pcm), "markers":["hello", "controlled source"], "marker_alternatives":[["missing alternative", "hello"]]});
            3
        ];
        if mode == 1 {
            cases[1]["markers"] = json!(["hellos"]);
        }
        if mode == 2 {
            cases[1]["pcm_sha256"] = json!("f".repeat(64));
        }
        if mode == 4 {
            cases[1]["expected_text_sha256"] = json!("0".repeat(64));
        }
        if mode == 5 {
            cases[1]["marker_alternatives"] = json!([["missing one", "missing two"]]);
        }
        let manifest = dir.path().join("private-manifest.json");
        std::fs::write(&manifest, serde_json::to_vec(&cases).unwrap()).unwrap();
        let (report, ok) = bounded(run(args(manifest, Some(server.address)))).await;
        assert_eq!(ok, mode == 0, "{report}");
        assert_eq!(report["connect_calls"], 1);
        assert_eq!(report["retry_count"], 0);
        assert_eq!(calls.load(Ordering::SeqCst), if mode == 0 { 3 } else { 2 });
        assert_eq!(server.accepted_connections.load(Ordering::SeqCst), 1);
        assert_eq!(report["sessions"][0]["pcm"]["bytes"], 642);
        assert_eq!(report["sessions"][0]["pcm"]["matches"], true);
        let serialized = report.to_string();
        for private in [
            "private-audio",
            "private-manifest",
            "Hello, controlled",
            "PRIVATE SERVER",
            "x-goog-api-key",
            "en-US",
        ] {
            assert!(!serialized.contains(private));
        }
        bounded(server.close()).await;
    }
}

#[tokio::test]
async fn manifest_bounds_unknown_fields_and_empty_markers_fail_before_rpc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("manifest.json");
    let valid = json!({"input":"never-opened.wav", "language":"en-US",
        "pcm_sha256":"a".repeat(64), "markers":["hello"]});
    let mut unknown = valid.clone();
    unknown["unknown"] = json!("PRIVATE");
    let mut empty = valid.clone();
    empty["markers"] = json!(["... "]);
    for payload in [
        vec![b'x'; 65537],
        serde_json::to_vec(&vec![unknown; 2]).unwrap(),
        serde_json::to_vec(&vec![empty; 2]).unwrap(),
        serde_json::to_vec(&vec![valid; 4]).unwrap(),
    ] {
        std::fs::write(&path, payload).unwrap();
        // Closed local port would fail connect if the manifest gate did not run first.
        let (report, ok) = run(args(path.clone(), Some("127.0.0.1:1".parse().unwrap()))).await;
        assert!(!ok);
        assert_eq!(report["status"], "rejected");
        assert_eq!(report["category"], "manifest");
    }
}
