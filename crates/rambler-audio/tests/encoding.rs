mod common;
use common::*;
use rambler_audio::*;
use rambler_engine::AudioSource;
use std::sync::{Arc, atomic::Ordering};
use tokio::{
    sync::Notify,
    time::{Duration, timeout},
};

#[tokio::test]
async fn native_codec_runs_and_tails_granules_packets_crc_decode_for_all_boundaries() {
    assert_eq!(opus_version(), "libopus 1.6.1");
    for n in [
        1, 2, 15, 16, 17, 31, 32, 80, 215, 216, 217, 319, 320, 321, 639, 640, 641, 16007, 32123,
    ] {
        for page_duration_ms in [20, 37, 1000] {
            let source = Source::new(fragment(&pcm(n), &[1, 17, 641, 3, 1280]));
            let stats = source.stats.clone();
            let mut audio = Pcm16Opus::new(
                source,
                EncoderConfig {
                    page_duration_ms,
                    ..Default::default()
                },
            );
            let data = collect(&mut audio).await.unwrap();
            assert_duration(&data, n);
            assert!(stats.eof.load(Ordering::SeqCst));
            assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
            audio.close().await.unwrap();
            audio.close().await.unwrap();
            assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
        }
    }
}

#[tokio::test]
async fn fragmentation_empty_chunks_page_batching_preserve_identical_opus_packets() {
    let pcm = pcm(32771);
    let mut expected = None;
    for page_duration_ms in [20, 37, 100, 1000] {
        for sizes in [&[pcm.len()][..], &[640][..], &[1, 3, 17, 1279, 2, 4097][..]] {
            let mut chunks = vec![vec![]];
            for chunk in fragment(&pcm, sizes) {
                chunks.push(chunk);
                chunks.push(vec![]);
            }
            let mut audio = Pcm16Opus::new(
                Source::new(chunks),
                EncoderConfig {
                    page_duration_ms,
                    ..Default::default()
                },
            );
            let encoded = collect(&mut audio).await.unwrap();
            let actual = packets(&encoded)[2..].to_vec();
            if let Some(expected) = &expected {
                assert_eq!(&actual, expected);
            } else {
                expected = Some(actual);
            }
            assert_duration(&encoded, 32771);
        }
    }
}

#[tokio::test]
async fn page_duration_controls_actual_mux_cadence_not_codec_frame_size() {
    let mut fast = Pcm16Opus::new(Source::new([pcm(48017)]), EncoderConfig::default());
    let mut slow = Pcm16Opus::new(
        Source::new([pcm(48017)]),
        EncoderConfig {
            page_duration_ms: 1000,
            ..Default::default()
        },
    );
    let fast = collect(&mut fast).await.unwrap();
    let slow = collect(&mut slow).await.unwrap();
    assert!(pages(&fast).len() >= 152);
    assert!((5..=6).contains(&pages(&slow).len()));
    assert_eq!(packets(&fast)[2..], packets(&slow)[2..]);
}

#[tokio::test]
async fn first_audio_page_precedes_source_eof_and_large_input_is_consumed_progressively() {
    let mut source = Source::new([pcm(320), pcm(320), pcm(320)]);
    source.block_after = Some(3);
    let stats = source.stats.clone();
    let gate = source.read_gate.clone();
    let mut audio = Pcm16Opus::new(source, EncoderConfig::default());
    let mut prefix = vec![];
    while pages(&prefix).len() < 3 {
        prefix.extend(
            timeout(Duration::from_secs(2), audio.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
        );
    }
    assert!(!stats.eof.load(Ordering::SeqCst));
    assert!(stats.reads.load(Ordering::SeqCst) <= 3);
    assert!(pages(&prefix).iter().all(|p| p.flags & 4 == 0));
    gate.notify_one();
    prefix.extend(collect(&mut audio).await.unwrap());
    assert_duration(&prefix, 960);
    let source = Source::new([vec![0; MAX_INPUT_BYTES]]);
    let stats = source.stats.clone();
    let mut audio = Pcm16Opus::new(source, EncoderConfig::default());
    let first = timeout(Duration::from_secs(2), audio.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(first.windows(8).any(|b| b == b"OpusHead"));
    assert_eq!(stats.reads.load(Ordering::SeqCst), 1);
    assert!(!stats.eof.load(Ordering::SeqCst));
    audio.close().await.unwrap();
    assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn empty_source_produces_no_headers_and_closes_once() {
    for chunks in [vec![], vec![vec![]], vec![vec![], vec![], vec![]]] {
        let source = Source::new(chunks);
        let stats = source.stats.clone();
        let mut audio = Pcm16Opus::new(source, EncoderConfig::default());
        assert!(collect(&mut audio).await.unwrap().is_empty());
        assert!(audio.next().await.unwrap().is_none());
        audio.close().await.unwrap();
        assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn invalid_profile_page_and_input_limits_fail_before_upstream_reads_when_possible() {
    for config in [
        EncoderConfig {
            sample_rate: 48000,
            ..Default::default()
        },
        EncoderConfig {
            channels: 2,
            ..Default::default()
        },
        EncoderConfig {
            bitrate: 32000,
            ..Default::default()
        },
        EncoderConfig {
            page_duration_ms: 19,
            ..Default::default()
        },
        EncoderConfig {
            page_duration_ms: 1001,
            ..Default::default()
        },
    ] {
        assert_eq!(config.validate(), Err(AudioError::InvalidConfig));
        let source = Source::new([pcm(320)]);
        let stats = source.stats.clone();
        let mut audio = Pcm16Opus::new(source, config);
        let error = audio.next().await.unwrap_err();
        assert_eq!(
            error.original().to_string(),
            AudioError::InvalidConfig.to_string()
        );
        assert_eq!(stats.reads.load(Ordering::SeqCst), 0);
        assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
    }
    let source = Source::new([vec![0; MAX_INPUT_BYTES + 1]]);
    let stats = source.stats.clone();
    let mut audio = Pcm16Opus::new(source, EncoderConfig::default());
    assert_eq!(
        audio.next().await.unwrap_err().original().to_string(),
        AudioError::InputLimit.to_string()
    );
    assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn isolated_final_byte_and_source_errors_do_not_emit_eos() {
    for chunks in [
        vec![vec![1]],
        vec![vec![0, 0], vec![1]],
        vec![pcm(1280), vec![255]],
    ] {
        let source = Source::new(chunks);
        let stats = source.stats.clone();
        let mut audio = Pcm16Opus::new(source, EncoderConfig::default());
        let mut prefix = vec![];
        let error = loop {
            match audio.next().await {
                Ok(Some(chunk)) => prefix.extend(chunk),
                Err(error) => break error,
                Ok(None) => panic!("unexpected EOS"),
            }
        };
        assert_eq!(
            error.original().to_string(),
            AudioError::TruncatedPcm.to_string()
        );
        assert!(pages(&prefix).iter().all(|p| p.flags & 4 == 0));
        assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
        assert!(stats.eof.load(Ordering::SeqCst));
        audio.close().await.unwrap();
    }
    let mut source = Source::new([pcm(1280)]);
    source.fail_after = Some(1);
    let stats = source.stats.clone();
    let mut audio = Pcm16Opus::new(source, EncoderConfig::default());
    let mut prefix = vec![];
    loop {
        match audio.next().await {
            Ok(Some(c)) => prefix.extend(c),
            Err(_) => break,
            Ok(None) => panic!("hidden source error"),
        }
    }
    assert!(pages(&prefix).len() > 2);
    assert!(pages(&prefix).iter().all(|p| p.flags & 4 == 0));
    assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cleanup_failure_prevents_eos_and_remains_observable_idempotently() {
    let mut source = Source::new([pcm(320)]);
    source.close_fails = true;
    let stats = source.stats.clone();
    let mut audio = Pcm16Opus::new(source, EncoderConfig::default());
    let mut prefix = vec![];
    loop {
        match audio.next().await {
            Ok(Some(c)) => prefix.extend(c),
            Err(_) => break,
            Ok(None) => panic!("cleanup failure became successful EOF"),
        }
    }
    assert!(pages(&prefix).iter().all(|p| p.flags & 4 == 0));
    assert!(audio.close().await.is_err());
    assert!(audio.close().await.is_err());
    assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancellation_waiting_for_input_then_failing_close_has_one_cleanup_and_no_eos() {
    let mut source = Source::new([pcm(320)]);
    source.block_after = Some(1);
    source.close_fails = true;
    let stats = source.stats.clone();
    let entered = source.read_entered.clone();
    let mut audio = Pcm16Opus::new(source, EncoderConfig::default());
    let mut prefix = vec![];
    while pages(&prefix).len() < 3 {
        prefix.extend(audio.next().await.unwrap().unwrap());
    }
    {
        let mut next = audio.next();
        tokio::select! { _ = entered.notified() => (), result = &mut next => panic!("unexpected {result:?}") }
    }
    assert_eq!(stats.closes.load(Ordering::SeqCst), 0);
    assert!(audio.close().await.is_err());
    assert!(audio.close().await.is_err());
    assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
    assert!(pages(&prefix).iter().all(|p| p.flags & 4 == 0));
}

#[tokio::test]
async fn cancelled_next_and_interrupted_close_resume_the_same_close_once() {
    let gate = Arc::new(Notify::new());
    let mut source = Source::new([pcm(320)]);
    source.block_after = Some(1);
    source.close_fails = true;
    source.close_gate = Some(gate.clone());
    let entered = source.read_entered.clone();
    let close_entered = source.close_entered.clone();
    let stats = source.stats.clone();
    let mut audio = Pcm16Opus::new(source, EncoderConfig::default());
    for _ in 0..3 {
        audio.next().await.unwrap().unwrap();
    }
    {
        let mut next = audio.next();
        tokio::select! { _ = entered.notified() => (), result = &mut next => panic!("unexpected {result:?}") }
    }
    {
        let mut close = audio.close();
        tokio::select! { _ = close_entered.notified() => (), result = &mut close => panic!("unexpected {result:?}") }
    }
    assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
    gate.notify_one();
    assert!(audio.close().await.is_err());
    assert!(audio.close().await.is_err());
    assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
    assert!(audio.next().await.unwrap().is_none());
}

#[tokio::test]
async fn cancelled_cleanup_keeps_the_original_error_until_it_can_be_reported() {
    let gate = Arc::new(Notify::new());
    let mut source = Source::new([vec![1]]);
    source.close_gate = Some(gate.clone());
    let entered = source.close_entered.clone();
    let stats = source.stats.clone();
    let mut audio = Pcm16Opus::new(source, EncoderConfig::default());
    {
        let mut next = audio.next();
        tokio::select! { _ = entered.notified() => (), result = &mut next => panic!("unexpected {result:?}") }
    }
    gate.notify_one();
    assert_eq!(
        audio.next().await.unwrap_err().original().to_string(),
        AudioError::TruncatedPcm.to_string()
    );
    audio.close().await.unwrap();
    assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn dropping_an_unpolled_or_cancelled_encoder_schedules_one_upstream_close() {
    let source = Source::new([pcm(320)]);
    let stats = source.stats.clone();
    let entered = source.close_entered.clone();
    drop(Pcm16Opus::new(source, EncoderConfig::default()));
    timeout(Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
    assert_eq!(stats.reads.load(Ordering::SeqCst), 0);
    let source = Source::new([vec![0; MAX_INPUT_BYTES]]);
    let stats = source.stats.clone();
    let entered = source.close_entered.clone();
    let task = tokio::spawn(async move {
        let mut audio = Pcm16Opus::new(source, EncoderConfig::default());
        while audio.next().await.unwrap().is_some() {}
    });
    tokio::task::yield_now().await;
    task.abort();
    let _ = task.await;
    timeout(Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
    assert!(!stats.eof.load(Ordering::SeqCst));
}
