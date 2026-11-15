mod common;

use bytes::Bytes;
use common::*;
use futures_util::poll;
use rambler_audio::{
    AudioError, EncoderConfig, FRAME_SAMPLES, Pcm16Opus, PcmInputError, PcmSender, pcm_channel,
};
use rambler_engine::{AudioSource, SourceError};
use tokio::{
    sync::oneshot,
    time::{Duration, timeout},
};

const SCENARIO_TIMEOUT: Duration = Duration::from_secs(5);

async fn collect_failure(source: &mut impl AudioSource) -> (Vec<u8>, SourceError) {
    let mut prefix = vec![];
    loop {
        match source.next().await {
            Ok(Some(chunk)) => prefix.extend(chunk),
            Err(error) => return (prefix, error),
            Ok(None) => panic!("input failure became healthy EOF"),
        }
    }
}

// Strengthened failure semantics: later pulls retain the primary error until
// explicit close; only then may they report healthy EOF.
async fn assert_latched_then_close(audio: &mut impl AudioSource, expected: AudioError) {
    for _ in 0..2 {
        assert_eq!(
            audio
                .next()
                .await
                .unwrap_err()
                .original()
                .downcast_ref::<AudioError>(),
            Some(&expected)
        );
    }
    audio.close().await.unwrap();
    assert!(audio.next().await.unwrap().is_none());
}

// The caller fills the only queue slot before spawning this future. Signal only
// after reserve has registered a real pending wait, not after an elapsed delay.
async fn blocked_reservation(
    mut sender: PcmSender,
    entered: oneshot::Sender<()>,
) -> Result<(), PcmInputError> {
    let mut reserve = Box::pin(sender.reserve());
    assert!(poll!(reserve.as_mut()).is_pending());
    entered.send(()).unwrap();
    reserve.await.map(drop)
}

#[tokio::test]
async fn finished_fragmented_channel_matches_reference_packets_and_duration() {
    timeout(SCENARIO_TIMEOUT, async {
        let samples = 16007;
        let pcm = pcm(samples);
        for page_duration_ms in [20, 1000] {
            let config = EncoderConfig {
                page_duration_ms,
                ..Default::default()
            };
            let (mut sender, input) = pcm_channel();
            let mut audio = Pcm16Opus::new(input, config);
            let producer = async {
                for chunk in fragment(&pcm, &[1, 3, 641, 1279, 2, 4097]) {
                    sender
                        .reserve()
                        .await
                        .unwrap()
                        .send(Bytes::from(chunk))
                        .unwrap();
                }
                sender.finish().unwrap();
            };
            // Queue acceptance and finish are not delivery acknowledgements: the
            // consumer must independently reach healthy EOF and decode correctly.
            let ((), encoded) = tokio::join!(producer, collect(&mut audio));
            let encoded = encoded.unwrap();
            let mut reference = Pcm16Opus::new(Source::new([pcm.clone()]), config);
            let reference = collect(&mut reference).await.unwrap();
            assert_eq!(packets(&encoded)[2..], packets(&reference)[2..]);
            assert_duration(&encoded, samples);
            assert_duration(&reference, samples);
            assert_eq!(
                pages(&encoded)
                    .iter()
                    .filter(|page| page.flags & 4 != 0)
                    .count(),
                1
            );
            assert!(audio.next().await.unwrap().is_none());
            assert!(audio.next().await.unwrap().is_none());
            audio.close().await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn abandoned_producer_drains_complete_queued_pcm_without_eos() {
    timeout(SCENARIO_TIMEOUT, async {
        let frames = 4;
        let pcm = pcm(FRAME_SAMPLES * frames);
        let (mut sender, input) = pcm_channel();
        sender
            .reserve()
            .await
            .unwrap()
            .send(Bytes::from(pcm.clone()))
            .unwrap();
        drop(sender);

        let mut audio = Pcm16Opus::new(input, EncoderConfig::default());
        let (prefix, error) = collect_failure(&mut audio).await;
        assert_eq!(
            error.original().downcast_ref::<AudioError>(),
            Some(&AudioError::InputAbandoned)
        );
        let actual = packets(&prefix);
        assert_eq!(actual.len(), 2 + frames);
        assert!(pages(&prefix).iter().all(|page| page.flags & 4 == 0));

        let mut reference = Pcm16Opus::new(Source::new([pcm]), EncoderConfig::default());
        let reference = packets(&collect(&mut reference).await.unwrap());
        assert_eq!(actual[2..], reference[2..2 + frames]);
        assert_latched_then_close(&mut audio, AudioError::InputAbandoned).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn explicit_finish_with_isolated_final_byte_is_truncated_without_eos() {
    timeout(SCENARIO_TIMEOUT, async {
        let (mut sender, input) = pcm_channel();
        let mut audio = Pcm16Opus::new(input, EncoderConfig::default());
        let producer = async {
            for chunk in [pcm(FRAME_SAMPLES * 4), vec![1]] {
                sender
                    .reserve()
                    .await
                    .unwrap()
                    .send(Bytes::from(chunk))
                    .unwrap();
            }
            sender.finish().unwrap();
        };
        let ((), (prefix, error)) = tokio::join!(producer, collect_failure(&mut audio));
        assert_eq!(
            error.original().downcast_ref::<AudioError>(),
            Some(&AudioError::TruncatedPcm)
        );
        assert_eq!(packets(&prefix).len(), 6);
        assert!(pages(&prefix).iter().all(|page| page.flags & 4 == 0));
        assert_latched_then_close(&mut audio, AudioError::TruncatedPcm).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn finishing_empty_channel_emits_nothing_cleanly() {
    timeout(SCENARIO_TIMEOUT, async {
        for empty_chunks in [0, 2] {
            let (mut sender, input) = pcm_channel();
            let mut audio = Pcm16Opus::new(input, EncoderConfig::default());
            let producer = async {
                for _ in 0..empty_chunks {
                    sender.reserve().await.unwrap().send(Bytes::new()).unwrap();
                }
                sender.finish().unwrap();
            };
            let ((), encoded) = tokio::join!(producer, collect(&mut audio));
            assert!(encoded.unwrap().is_empty());
            assert!(audio.next().await.unwrap().is_none());
            audio.close().await.unwrap();
            audio.close().await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn encoder_explicit_close_discards_input_and_wakes_blocked_producer() {
    timeout(SCENARIO_TIMEOUT, async {
        let (mut sender, input) = pcm_channel();
        sender
            .reserve()
            .await
            .unwrap()
            .send(Bytes::from(pcm(FRAME_SAMPLES)))
            .unwrap();
        let mut audio = Pcm16Opus::new(input, EncoderConfig::default());
        let (entered, ready) = oneshot::channel();
        let producer = tokio::spawn(blocked_reservation(sender, entered));
        ready.await.unwrap();

        audio.close().await.unwrap();
        assert_eq!(producer.await.unwrap(), Err(PcmInputError::Closed));
        assert!(audio.next().await.unwrap().is_none());
        audio.close().await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn encoder_invalid_config_closes_input_and_wakes_blocked_producer() {
    timeout(SCENARIO_TIMEOUT, async {
        let (mut sender, input) = pcm_channel();
        sender
            .reserve()
            .await
            .unwrap()
            .send(Bytes::from(pcm(FRAME_SAMPLES)))
            .unwrap();
        let mut audio = Pcm16Opus::new(
            input,
            EncoderConfig {
                page_duration_ms: 19,
                ..Default::default()
            },
        );
        let (entered, ready) = oneshot::channel();
        let producer = tokio::spawn(blocked_reservation(sender, entered));
        ready.await.unwrap();

        let error = audio.next().await.unwrap_err();
        assert_eq!(
            error.original().downcast_ref::<AudioError>(),
            Some(&AudioError::InvalidConfig)
        );
        assert_eq!(producer.await.unwrap(), Err(PcmInputError::Closed));
        assert_latched_then_close(&mut audio, AudioError::InvalidConfig).await;
    })
    .await
    .unwrap();
}
