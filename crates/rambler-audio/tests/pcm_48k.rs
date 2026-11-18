mod common;

use bytes::Bytes;
use common::{Source, assert_duration, collect, fragment, pages, pcm};
use rambler_audio::{
    AudioError, EncoderConfig, FRAME_BYTES, MAX_INPUT_BYTES, Pcm16Opus, Pcm48kTo16k, PcmInputError,
    pcm_channel,
};
use rambler_engine::{AudioSource, SourceError};
use std::sync::{Arc, atomic::Ordering::SeqCst};
use tokio::{
    sync::Notify,
    time::{Duration, timeout},
};

const LIMIT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn cancelled_upstream_read_keeps_odd_byte_and_filters_once() {
    timeout(LIMIT, async {
        let data = pcm(1_600);
        let mut reference = Pcm48kTo16k::new(Source::new([data.clone()]));
        let mut source = Source::new([data[..5].to_vec(), data[5..].to_vec()]);
        source.block_after = Some(1);
        let entered = source.read_entered.clone();
        let gate = source.read_gate.clone();
        let stats = source.stats.clone();
        let mut adapter = Pcm48kTo16k::new(source);
        tokio::select! {
            _ = entered.notified() => {},
            output = adapter.next() => panic!("output before more input: {output:?}"),
        } // The pull is now cancelled with an odd byte already staged.
        assert_eq!(stats.reads.load(SeqCst), 1);
        gate.notify_one();
        assert_eq!(
            collect(&mut adapter).await.unwrap(),
            collect(&mut reference).await.unwrap()
        );
        assert_eq!(stats.reads.load(SeqCst), 2);
        assert_eq!(stats.closes.load(SeqCst), 1);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn one_chunk_at_a_time_and_explicit_close_discards_pending_pcm() {
    let source = Source::new([pcm(3_000), vec![0]]);
    let stats = source.stats.clone();
    let mut adapter = Pcm48kTo16k::new(source);
    assert_eq!(stats.reads.load(SeqCst), 0);
    for _ in 0..2 {
        assert_eq!(adapter.next().await.unwrap().unwrap().len(), FRAME_BYTES);
        assert_eq!(stats.reads.load(SeqCst), 1);
    }
    drop(adapter.close()); // Marks closed even without polling the cleanup.
    assert!(adapter.next().await.unwrap().is_none());
    adapter.close().await.unwrap();
    adapter.close().await.unwrap();
    assert_eq!(stats.reads.load(SeqCst), 1);
    assert_eq!(stats.closes.load(SeqCst), 1);
}

#[tokio::test]
async fn successful_eof_waits_for_resumable_close_before_returning_tail() {
    timeout(LIMIT, async {
        let mut source = Source::new([pcm(2)]);
        let gate = Arc::new(Notify::new());
        source.close_gate = Some(gate.clone());
        let entered = source.close_entered.clone();
        let stats = source.stats.clone();
        let mut adapter = Pcm48kTo16k::new(source);
        tokio::select! {
            _ = entered.notified() => {},
            output = adapter.next() => panic!("tail before cleanup: {output:?}"),
        }
        assert!(stats.eof.load(SeqCst));
        assert_eq!(stats.closes.load(SeqCst), 1);
        gate.notify_one();
        assert_eq!(adapter.next().await.unwrap().unwrap().len(), 2);
        assert!(adapter.next().await.unwrap().is_none());
        adapter.close().await.unwrap();
        assert_eq!(stats.closes.load(SeqCst), 1);
    })
    .await
    .unwrap();
}

fn assert_original(error: &SourceError, message: &str) {
    assert_eq!(
        error
            .original()
            .downcast_ref::<std::io::Error>()
            .unwrap()
            .to_string(),
        message
    );
}

#[tokio::test]
async fn cancelled_error_cleanup_keeps_primary_identity_and_close_failure() {
    timeout(LIMIT, async {
        for read_fails in [true, false] {
            let mut source = Source::new([pcm(200)]);
            if read_fails {
                source.fail_after = Some(1);
            }
            source.close_fails = true;
            let gate = Arc::new(Notify::new());
            source.close_gate = Some(gate.clone());
            let entered = source.close_entered.clone();
            let stats = source.stats.clone();
            let mut adapter = Pcm48kTo16k::new(source);
            // The available prefix needs no end padding. The pending tail must
            // never be returned for either of the two failure paths below.
            assert_eq!(adapter.next().await.unwrap().unwrap().len(), 2 * 37);
            tokio::select! {
                _ = entered.notified() => {},
                output = adapter.next() => panic!("returned before cleanup: {output:?}"),
            }
            gate.notify_one();
            let error = adapter.next().await.unwrap_err();
            let repeated = adapter.next().await.unwrap_err();
            assert!(std::ptr::eq(error.original(), repeated.original()));
            assert_original(
                &error,
                if read_fails {
                    "input failure"
                } else {
                    "close failure"
                },
            );
            let secondary = adapter.close().await.unwrap_err();
            assert_original(&secondary, "close failure");
            assert_eq!(
                std::ptr::eq(error.original(), secondary.original()),
                !read_fails
            );
            assert!(adapter.next().await.unwrap().is_none());
            assert_eq!(stats.reads.load(SeqCst), 1);
            assert_eq!(stats.closes.load(SeqCst), 1);
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn malformed_pcm_and_oversize_input_are_sticky() {
    for (chunks, expected) in [
        (vec![pcm(2), vec![255], vec![]], AudioError::TruncatedPcm),
        (vec![vec![0; MAX_INPUT_BYTES + 1]], AudioError::InputLimit),
    ] {
        let source = Source::new(chunks);
        let stats = source.stats.clone();
        let mut adapter = Pcm48kTo16k::new(source);
        let first = adapter.next().await.unwrap_err();
        assert_eq!(
            first.original().downcast_ref::<AudioError>(),
            Some(&expected)
        );
        let repeated = adapter.next().await.unwrap_err();
        assert!(std::ptr::eq(first.original(), repeated.original()));
        assert_eq!(stats.closes.load(SeqCst), 1);
        adapter.close().await.unwrap();
        assert!(adapter.next().await.unwrap().is_none());
    }
}

#[tokio::test]
async fn dropping_after_cancelled_explicit_close_wakes_channel_producer() {
    timeout(LIMIT, async {
        let (mut sender, input) = pcm_channel();
        sender
            .reserve()
            .await
            .unwrap()
            .send(Bytes::from(pcm(200)))
            .unwrap();
        let mut wait = Box::pin(sender.reserve());
        assert!(futures_util::poll!(wait.as_mut()).is_pending());
        let mut adapter = Pcm48kTo16k::new(input);
        drop(adapter.close());
        drop(adapter);
        assert!(matches!(wait.await, Err(PcmInputError::Closed)));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn channel_adapter_and_native_encoder_preserve_duration_and_eos_rules() {
    timeout(LIMIT, async {
        for (samples, finishes) in [
            (1_usize, true),
            (960, true),
            (4_801, true),
            (4_802, true),
            (4_802, false),
        ] {
            let (mut sender, input) = pcm_channel();
            let mut audio = Pcm16Opus::new(Pcm48kTo16k::new(input), EncoderConfig::default());
            let produce = async move {
                for chunk in fragment(&pcm(samples), &[1, 7, 0, 640, 123, 321]) {
                    sender
                        .reserve()
                        .await
                        .unwrap()
                        .send(Bytes::from(chunk))
                        .unwrap();
                }
                if finishes {
                    sender.finish().unwrap();
                }
            };
            let drain = async {
                let mut encoded = Vec::new();
                loop {
                    match audio.next().await {
                        Ok(Some(chunk)) => encoded.extend(chunk),
                        Ok(None) => {
                            assert!(finishes);
                            assert_duration(&encoded, samples.div_ceil(3));
                            break;
                        }
                        Err(error) => {
                            assert!(!finishes);
                            assert_eq!(
                                error.original().downcast_ref::<AudioError>(),
                                Some(&AudioError::InputAbandoned)
                            );
                            let prefix = pages(&encoded);
                            assert!(
                                prefix.len() > 2 && prefix.iter().all(|page| page.flags & 4 == 0)
                            );
                            break;
                        }
                    }
                }
                audio.close().await.unwrap();
            };
            tokio::join!(produce, drain);
        }
    })
    .await
    .unwrap();
}
