//! Explicit opt-in development oracle. No Python is used by the Rust library.
mod common;
use common::*;
use rambler_audio::{EncoderConfig, Pcm16Opus};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[tokio::test]
#[ignore = "requires RAMBLER_AUDIO_TEST_PYTHON pointing to a PyAV-equipped dev interpreter"]
async fn independent_pyav_decodes_complete_ogg_with_exact_preskip_tail_and_eos_trim() {
    let python = std::env::var_os("RAMBLER_AUDIO_TEST_PYTHON")
        .expect("set RAMBLER_AUDIO_TEST_PYTHON explicitly for this dev-only test");
    for n in [
        1, 2, 15, 16, 17, 31, 32, 80, 215, 216, 217, 319, 320, 321, 639, 640, 641, 16007, 32123,
    ] {
        for page_duration_ms in [20, 37, 1000] {
            let mut audio = Pcm16Opus::new(
                Source::new(fragment(&pcm(n), &[1, 17, 641, 3, 1280])),
                EncoderConfig {
                    page_duration_ms,
                    ..Default::default()
                },
            );
            let encoded = collect(&mut audio).await.unwrap();
            let mut child = Command::new(&python)
                .arg("-I")
                .arg("-c")
                .arg(include_str!("support/independent_decode.py"))
                .arg(if n < 80 { "short" } else { "normal" })
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(&encoded).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "dev decoder failed for N={n}, pages={page_duration_ms}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8(output.stdout).unwrap().trim(),
                format!("{} {n}", n * 3),
                "N={n}, pages={page_duration_ms}"
            );
        }
    }
}
