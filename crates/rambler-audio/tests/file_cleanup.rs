mod common;
use common::*;
use rambler_audio::*;
use rambler_engine::AudioSource;

#[cfg(windows)]
fn assert_released(path: &std::path::Path) {
    use std::os::windows::fs::OpenOptionsExt;
    // No sharing is permitted: success proves the earlier read handle is closed,
    // not merely scheduled for release after a Tokio blocking operation.
    drop(
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(path)
            .unwrap(),
    );
}
#[cfg(not(windows))]
fn assert_released(path: &std::path::Path) {
    assert!(path.exists()); // The exclusive Windows-handle audit is Windows-only.
}

#[tokio::test]
async fn wav_close_awaits_inflight_reads_even_when_next_was_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pending.wav");
    tokio::fs::write(&path, wav(&vec![0; MAX_INPUT_BYTES]))
        .await
        .unwrap();
    let mut audio = WavAudio::open(
        &path,
        WavConfig {
            chunk_ms: MAX_INPUT_BYTES as u32 / 32,
            realtime: false,
        },
    )
    .await
    .unwrap();
    {
        let mut next = audio.next();
        // Start a real Tokio blocking read, then drop its caller's future.
        let _ = futures_util::poll!(&mut next);
    }
    audio.close().await.unwrap();
    audio.close().await.unwrap();
    assert!(audio.next().await.unwrap().is_none());
    assert_released(&path);
}

#[tokio::test]
async fn eof_header_error_and_data_error_release_wav_descriptor_before_return() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.wav");
    tokio::fs::write(&path, wav(&pcm(321))).await.unwrap();
    let mut audio = WavAudio::open(&path, WavConfig::default()).await.unwrap();
    collect(&mut audio).await.unwrap();
    assert_released(&path);
    tokio::fs::write(&path, b"bad header").await.unwrap();
    assert!(WavAudio::open(&path, WavConfig::default()).await.is_err());
    assert_released(&path);
    let data = wav(&pcm(321));
    tokio::fs::write(&path, &data).await.unwrap();
    let mut audio = WavAudio::open(&path, WavConfig::default()).await.unwrap();
    {
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .await
            .unwrap();
        file.set_len(44).await.unwrap();
    }
    assert!(audio.next().await.is_err());
    assert_released(&path);
    audio.close().await.unwrap();
    assert_released(&path);
}

#[tokio::test]
async fn an_unpolled_close_still_stops_further_wav_reads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stop.wav");
    tokio::fs::write(&path, wav(&pcm(321))).await.unwrap();
    let mut audio = WavAudio::open(&path, WavConfig::default()).await.unwrap();
    drop(audio.close());
    assert!(audio.next().await.unwrap().is_none());
    assert_released(&path);
}
