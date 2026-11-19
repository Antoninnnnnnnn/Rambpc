mod common;
use common::*;
use rambler_audio::*;
use rambler_engine::AudioSource;
use sha2::{Digest, Sha256};
use tokio::time::{Duration, Instant};

async fn open_bytes(
    bytes: &[u8],
    config: WavConfig,
) -> Result<(tempfile::TempDir, WavAudio), AudioError> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.wav");
    tokio::fs::write(&path, bytes).await.unwrap();
    let audio = WavAudio::open(path, config).await?;
    Ok((dir, audio))
}
fn set16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn set32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[tokio::test]
async fn wav_chunks_preserve_pcm_little_endian_sha_duration_and_short_tail() {
    let original = pcm(16007);
    for chunk_ms in [1, 20, 40, 100] {
        let (_dir, mut source) = open_bytes(
            &wav(&original),
            WavConfig {
                chunk_ms,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            source.format,
            PcmFormat {
                sample_rate: 16000,
                channels: 1,
                sample_width: 2
            }
        );
        assert_eq!(source.frame_count, 16007);
        assert_eq!(source.duration, Duration::from_nanos(1_000_437_500));
        let mut chunks = vec![];
        while let Some(chunk) = source.next().await.unwrap() {
            assert_eq!(chunk.len() % 2, 0);
            assert!(chunk.len() <= chunk_ms as usize * 32);
            chunks.push(chunk);
        }
        assert_eq!(
            chunks.last().unwrap().len(),
            original.len() % (chunk_ms as usize * 32)
        );
        let collected = chunks.concat();
        assert_eq!(collected, original);
        assert_eq!(Sha256::digest(&collected), Sha256::digest(&original));
        source.close().await.unwrap();
        source.close().await.unwrap();
        assert!(source.next().await.unwrap().is_none());
    }
}

#[tokio::test]
async fn empty_wav_and_two_independent_reads() {
    let (_dir, mut audio) = open_bytes(&wav(b""), WavConfig::default()).await.unwrap();
    assert_eq!(audio.frame_count, 0);
    assert_eq!(audio.duration, Duration::ZERO);
    assert!(audio.next().await.unwrap().is_none());
    let original = pcm(37);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("twice.wav");
    tokio::fs::write(&path, wav(&original)).await.unwrap();
    for _ in 0..2 {
        let mut source = WavAudio::open(&path, WavConfig::default()).await.unwrap();
        assert_eq!(collect(&mut source).await.unwrap(), original);
    }
}

#[tokio::test]
async fn strict_profile_rejects_every_malformed_format_field() {
    let baseline = wav(&pcm(20));
    let mut bad = vec![vec![], b"not a wave".to_vec()];
    for (offset, value) in [
        (20, 3),
        (20, 6),
        (22, 2),
        (32, 1),
        (32, 4),
        (34, 8),
        (34, 12),
        (34, 24),
        (34, 32),
    ] {
        let mut bytes = baseline.clone();
        set16(&mut bytes, offset, value);
        bad.push(bytes);
    }
    for (offset, value) in [(24, 0), (24, 8000), (24, 48000), (28, 0), (28, 31999)] {
        let mut bytes = baseline.clone();
        set32(&mut bytes, offset, value);
        bad.push(bytes);
    }
    for signature in [b"RIFX", b"RF64", b"NOPE"] {
        let mut bytes = baseline.clone();
        bytes[..4].copy_from_slice(signature);
        bad.push(bytes);
    }
    let mut bytes = baseline.clone();
    bytes[8..12].copy_from_slice(b"NOPE");
    bad.push(bytes);
    let mut odd = wav(&[1, 2, 3]);
    odd.push(0);
    let size = odd.len() as u32 - 8;
    set32(&mut odd, 4, size);
    bad.push(odd);
    let mut data_first = baseline.clone();
    data_first[12..16].copy_from_slice(b"data");
    bad.push(data_first);
    let mut truncated_fmt = baseline.clone();
    set32(&mut truncated_fmt, 16, 15);
    bad.push(truncated_fmt);
    for bytes in bad {
        assert!(
            open_bytes(&bytes, WavConfig::default()).await.is_err(),
            "bad WAV accepted"
        );
    }
    for chunk_ms in [0, MAX_INPUT_BYTES as u32 / 32 + 1, u32::MAX] {
        assert!(
            open_bytes(
                &baseline,
                WavConfig {
                    chunk_ms,
                    ..Default::default()
                }
            )
            .await
            .is_err()
        );
    }
}

#[tokio::test]
async fn all_header_data_truncations_and_riff_extent_overflows_are_rejected() {
    let baseline = wav(&pcm(3));
    for n in 0..baseline.len() {
        assert!(
            open_bytes(&baseline[..n], WavConfig::default())
                .await
                .is_err(),
            "truncation {n} accepted"
        );
    }
    for (offset, value) in [(4, u32::MAX), (4, 3), (16, u32::MAX), (40, u32::MAX)] {
        let mut bytes = baseline.clone();
        set32(&mut bytes, offset, value);
        assert!(open_bytes(&bytes, WavConfig::default()).await.is_err());
    }
}

#[tokio::test]
async fn unknown_odd_metadata_chunks_skip_padding_without_changing_audio() {
    let pcm = pcm(33);
    let standard = wav(&pcm);
    let mut extended = standard[..12].to_vec();
    extended.extend(b"JUNK");
    extended.extend(3_u32.to_le_bytes());
    extended.extend([0xff, 0, 1, 0]);
    extended.extend(&standard[12..]);
    let size = extended.len() as u32 - 8;
    set32(&mut extended, 4, size);
    let (_dir, mut source) = open_bytes(&extended, WavConfig::default()).await.unwrap();
    assert_eq!(collect(&mut source).await.unwrap(), pcm);
    let mut missing_padding = extended.clone();
    missing_padding.remove(23);
    let size = missing_padding.len() as u32 - 8;
    set32(&mut missing_padding, 4, size);
    assert!(
        open_bytes(&missing_padding, WavConfig::default())
            .await
            .is_err()
    );
    let mut duplicate_fmt = standard[..36].to_vec();
    duplicate_fmt.extend(&standard[12..]);
    let size = duplicate_fmt.len() as u32 - 8;
    set32(&mut duplicate_fmt, 4, size);
    assert!(
        open_bytes(&duplicate_fmt, WavConfig::default())
            .await
            .is_err()
    );
}

fn extensible_wav(pcm: &[u8]) -> Vec<u8> {
    let standard = wav(pcm);
    let mut bytes = standard[..36].to_vec();
    set16(&mut bytes, 20, 0xfffe);
    set32(&mut bytes, 16, 40);
    bytes.extend(22_u16.to_le_bytes());
    bytes.extend(16_u16.to_le_bytes());
    bytes.extend(4_u32.to_le_bytes());
    bytes.extend([
        1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71,
    ]);
    bytes.extend(&standard[36..]);
    let size = bytes.len() as u32 - 8;
    set32(&mut bytes, 4, size);
    bytes
}

#[tokio::test]
async fn pcm_extensible_accepts_pcm_guid_16validbits_and_rejects_other_subformats() {
    let original = pcm(17);
    let bytes = extensible_wav(&original);
    let (_dir, mut source) = open_bytes(&bytes, WavConfig::default()).await.unwrap();
    assert_eq!(collect(&mut source).await.unwrap(), original);
    for (offset, value) in [(36, 21), (36, 23), (38, 12), (44, 3)] {
        let mut bad = bytes.clone();
        set16(&mut bad, offset, value);
        assert!(open_bytes(&bad, WavConfig::default()).await.is_err());
    }
}

#[tokio::test]
async fn data_truncation_after_open_is_detected_without_faking_encoder_eos() {
    let pcm = pcm(641);
    let baseline = wav(&pcm);
    let (dir, source) = open_bytes(
        &baseline,
        WavConfig {
            chunk_ms: 20,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(dir.path().join("input.wav"))
        .await
        .unwrap();
    file.set_len((baseline.len() - 4) as u64).await.unwrap();
    let mut audio = Pcm16Opus::new(source, EncoderConfig::default());
    let mut prefix = vec![];
    let error = loop {
        match audio.next().await {
            Ok(Some(c)) => prefix.extend(c),
            Err(e) => break e,
            Ok(None) => panic!("truncated WAV accepted"),
        }
    };
    assert_eq!(
        error.original().to_string(),
        AudioError::TruncatedWav.to_string()
    );
    assert!(pages(&prefix).iter().all(|p| p.flags & 4 == 0));
    audio.close().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn pacing_starts_on_first_next_not_open_and_has_no_final_sleep() {
    let (_dir, mut source) = open_bytes(
        &wav(&pcm(33)),
        WavConfig {
            chunk_ms: 1,
            realtime: true,
        },
    )
    .await
    .unwrap();
    tokio::time::advance(Duration::from_secs(15)).await;
    let start = Instant::now();
    assert_eq!(source.next().await.unwrap().unwrap().len(), 32);
    assert_eq!(start.elapsed(), Duration::ZERO);
    assert_eq!(source.next().await.unwrap().unwrap().len(), 32);
    assert_eq!(start.elapsed(), Duration::from_millis(1));
    assert_eq!(source.next().await.unwrap().unwrap().len(), 2);
    assert_eq!(start.elapsed(), Duration::from_millis(2));
    assert!(source.next().await.unwrap().is_none());
    assert_eq!(start.elapsed(), Duration::from_millis(2));
}

#[tokio::test(start_paused = true)]
async fn opt_in_48k_paces_extensible_pcm_at_the_original_rate_without_rewriting_bytes() {
    let original = pcm(97);
    let mut wave = extensible_wav(&original);
    set32(&mut wave, 24, 48_000);
    set32(&mut wave, 28, 96_000);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("48k.wav");
    tokio::fs::write(&path, wave).await.unwrap();
    assert!(matches!(
        WavAudio::open(&path, WavConfig::default()).await,
        Err(AudioError::InvalidWav)
    ));
    let mut config = WavConfig {
        chunk_ms: MAX_INPUT_BYTES as u32 / 96 + 1,
        realtime: true,
    };
    assert!(matches!(
        WavAudio::open_16_or_48k(&path, config).await,
        Err(AudioError::InvalidConfig)
    ));
    config.chunk_ms = 1;
    let mut source = WavAudio::open_16_or_48k(&path, config).await.unwrap();
    assert_eq!(source.format.sample_rate, 48_000);
    assert_eq!(source.frame_count, 97);
    assert_eq!(source.duration, Duration::from_nanos(2_020_833));

    // Public descriptive fields cannot change private read bounds or pacing.
    source.format.sample_rate = 16_000;
    tokio::time::advance(Duration::from_secs(15)).await;
    let start = Instant::now();
    for (millis, expected) in original.chunks(96).enumerate() {
        assert_eq!(source.next().await.unwrap().unwrap().as_ref(), expected);
        assert_eq!(start.elapsed(), Duration::from_millis(millis as u64));
    }
    assert!(source.next().await.unwrap().is_none());
    assert_eq!(start.elapsed(), Duration::from_millis(2));
}

#[tokio::test(start_paused = true)]
async fn cancelled_pacing_future_keeps_the_already_read_pcm_chunk() {
    let original = pcm(48);
    let (_dir, mut source) = open_bytes(
        &wav(&original),
        WavConfig {
            chunk_ms: 1,
            realtime: true,
        },
    )
    .await
    .unwrap();
    let first = source.next().await.unwrap().unwrap();
    {
        let next = source.next();
        tokio::pin!(next);
        // Poll once to start reading, then let the file worker become ready.
        tokio::select! { biased; _ = &mut next => panic!("second chunk must be paced"), _ = tokio::task::yield_now() => () }
    }
    tokio::time::advance(Duration::from_millis(1)).await;
    let second = source.next().await.unwrap().unwrap();
    let third = source.next().await.unwrap().unwrap();
    assert_eq!([first, second, third].concat(), original);
    assert!(source.next().await.unwrap().is_none());
}

#[tokio::test]
async fn wav_to_opus_composition_is_progressive_and_retains_tail() {
    let (_dir, source) = open_bytes(
        &wav(&pcm(16007)),
        WavConfig {
            chunk_ms: 1,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut source = Pcm16Opus::new(source, EncoderConfig::default());
    let out = collect(&mut source).await.unwrap();
    assert_duration(&out, 16007);
}
