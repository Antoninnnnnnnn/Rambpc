#![cfg(feature = "audio")]

#[path = "../../rambler-engine/tests/common/mod.rs"]
mod common;

use bytes::Bytes;
use common::{TestServer, replies};
use futures_util::{StreamExt, stream};
use rambler_protocol::{
    AUDIO_DATA, end_request,
    wire::{Value, parse_fields},
};
use std::{
    io,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::Poll,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};

const SAMPLES: usize = 640;
const OUTPUT_LIMIT: usize = 4096;
const UPLOAD_LIMIT: usize = 64 * 1024;

// Original fixture: 40 ms of synthetic mono PCM16LE/16 kHz silence.
fn synthetic_wav() -> Vec<u8> {
    let length = (SAMPLES * 2) as u32;
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + length).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&16000u32.to_le_bytes());
    wav.extend_from_slice(&32000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&length.to_le_bytes());
    wav.resize(44 + length as usize, 0);
    wav
}

fn assert_complete_audio(ogg: &[u8]) {
    let mut cursor = 0;
    let mut pre_skip = None;
    let mut final_granule = 0;
    let mut eos = false;
    while cursor < ogg.len() {
        assert!(!eos, "audio followed the Ogg EOS page");
        assert!(ogg.len() - cursor >= 27, "truncated Ogg header");
        assert_eq!(&ogg[cursor..cursor + 4], b"OggS");
        let segments = ogg[cursor + 26] as usize;
        let body_start = cursor + 27 + segments;
        assert!(body_start <= ogg.len(), "truncated Ogg lacing table");
        let body_length: usize = ogg[cursor + 27..body_start]
            .iter()
            .map(|&length| length as usize)
            .sum();
        let page_end = body_start + body_length;
        assert!(page_end <= ogg.len(), "truncated Ogg body");
        if cursor == 0 {
            let head = &ogg[body_start..page_end];
            assert!(head.len() >= 19 && head.starts_with(b"OpusHead"));
            pre_skip = Some(u16::from_le_bytes([head[10], head[11]]) as u64);
        }
        final_granule = u64::from_le_bytes(ogg[cursor + 6..cursor + 14].try_into().unwrap());
        eos = ogg[cursor + 5] & 4 != 0;
        cursor = page_end;
    }
    assert!(eos, "peer did not receive the encoder's EOS page");
    assert_eq!(final_granule, pre_skip.unwrap() + SAMPLES as u64 * 3);
}

async fn limited_output(reader: impl tokio::io::AsyncRead + Unpin) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((OUTPUT_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > OUTPUT_LIMIT {
        return Err(io::Error::other("synthetic CLI output exceeded its limit"));
    }
    Ok(bytes)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s3_end_without_grpc_trailers_exits_with_finalize_timeout() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let upload_complete = Arc::new(AtomicBool::new(false));
        let trailers_withheld = Arc::new(AtomicBool::new(false));
        let server = TestServer::new({
            let upload_complete = upload_complete.clone();
            let trailers_withheld = trailers_withheld.clone();
            move |request| {
                let upload_complete = upload_complete.clone();
                let trailers_withheld = trailers_withheld.clone();
                async move {
                    for header in ["x-goog-api-key", "x-android-package", "x-android-cert"] {
                        assert!(request.metadata().get(header).is_none());
                    }
                    let mut request = request.into_inner();
                    let mut requests = 0;
                    let mut bytes = 0;
                    let mut saw_end = false;
                    let mut ogg = Vec::new();
                    while let Some(payload) = request.message().await? {
                        requests += 1;
                        bytes += payload.len();
                        assert!(requests <= 32 && bytes <= UPLOAD_LIMIT);
                        assert!(!saw_end, "request followed S3 request END");
                        if payload.as_ref() == end_request() {
                            saw_end = true;
                            continue;
                        }
                        for field in parse_fields(&payload).unwrap() {
                            if field.number == AUDIO_DATA {
                                let Value::Bytes(data) = field.value else {
                                    panic!("audio extension was not length-delimited");
                                };
                                for inner in parse_fields(&data).unwrap() {
                                    if let (1, Value::Bytes(audio)) = (inner.number, inner.value) {
                                        ogg.extend_from_slice(&audio);
                                    }
                                }
                            }
                        }
                    }
                    // Only a clean request-stream EOF, complete encoded fixture,
                    // and final request END count as successful peer observation.
                    assert!(requests >= 3 && saw_end, "missing complete request upload");
                    assert_complete_audio(&ogg);
                    upload_complete.store(true, Ordering::SeqCst);
                    Ok(replies(
                        stream::once(async { Ok(Bytes::from_static(b"\x08\x01")) }).chain(
                            stream::poll_fn(move |_| {
                                // This tail is polled only after synthetic S3 END
                                // was yielded. It never yields EOF or gRPC status.
                                trailers_withheld.store(true, Ordering::SeqCst);
                                Poll::Pending
                            }),
                        ),
                    ))
                }
            }
        })
        .await;

        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("synthetic.wav");
        std::fs::write(&path, synthetic_wav()).unwrap();
        // DEV-ONLY release-validation override; no production credentials are read.
        let executable = std::env::var_os("RAMBLER_TEST_CLI_EXECUTABLE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_rambler")));
        assert!(executable.is_absolute(), "CLI executable must be absolute");
        let mut command = Command::new(executable);
        command.env_clear();
        if let Some(root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", &root);
            #[cfg(windows)]
            {
                let root = PathBuf::from(root);
                command.env(
                    "PATH",
                    std::env::join_paths([root.join("System32"), root]).unwrap(),
                );
            }
        }
        command
            .current_dir(fixture.path())
            .arg("dictate-wav")
            .arg(&path)
            .arg("--loopback")
            .arg(server.address.to_string())
            // The actual CLI accepts integer seconds, not fractional seconds.
            .args(["--timeout", "5", "--finalize-timeout", "1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().expect("spawn synthetic CLI process");
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        // No detached readers or unbounded wait_with_output allocation. Timeout,
        // output overflow, or assertion failure drops/kills the child and drops
        // the original TestServer guard, which cancels and aborts its task.
        let (status, stdout, stderr) =
            tokio::try_join!(child.wait(), limited_output(stdout), limited_output(stderr),)
                .expect("wait for bounded synthetic CLI output");
        assert!(
            upload_complete.load(Ordering::SeqCst),
            "peer upload was not complete"
        );
        assert!(
            trailers_withheld.load(Ordering::SeqCst),
            "peer did not withhold trailers after S3 END"
        );
        assert_eq!(
            status.code(),
            Some(1),
            "expected CLI session failure, not argument rejection"
        );
        assert!(stderr.is_empty(), "unexpected synthetic CLI stderr");
        let value: serde_json::Value = serde_json::from_slice(&stdout).expect("one CLI JSON value");
        assert_eq!(value["type"], "error");
        assert_eq!(value["category"], "finalize_timeout");
        assert!(value["grpc_code"].is_null());
        assert!(value["close_error_category"].is_null());
        assert!(value["close_grpc_code"].is_null());
        let timeline = &value["timeline"];
        let audio = timeline["last_audio_sent_seconds"].as_f64().unwrap();
        let end = timeline["end_sent_seconds"].as_f64().unwrap();
        let half_close = timeline["half_close_sent_seconds"].as_f64().unwrap();
        let first = timeline["first_response_seconds"].as_f64().unwrap();
        assert!(audio <= end && end <= half_close && half_close <= first);
        assert_eq!(timeline["last_response_seconds"], first);
        assert_eq!(timeline["last_response_s3_status"], 1);
        assert_eq!(timeline["responses_received"], 1);
        assert_eq!(timeline["response_protobuf_bytes"], 2);
        assert!(timeline["request_protobuf_bytes"].as_u64().unwrap() > 4);
        assert_eq!(
            timeline["request_protobuf_bytes"],
            timeline["request_protobuf_bytes_submitted"]
        );
        assert!(timeline["context_sent_seconds"].is_null());
        assert!(timeline["terminal_grpc_code"].is_null());
        server.close().await;
    })
    .await
    .expect("whole CLI finalization scenario exceeded 20 seconds");
}
