mod common;
use common::*;
use rambler_audio::*;
use rambler_engine::AudioSource;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn invalid_config_error_is_terminal_and_explicit_close_never_validates_or_reads() {
    for close_before_next in [false, true] {
        let source = Source::new([pcm(320)]);
        let stats = source.stats.clone();
        let mut audio = Pcm16Opus::new(
            source,
            EncoderConfig {
                page_duration_ms: 0,
                ..Default::default()
            },
        );
        if close_before_next {
            audio.close().await.unwrap();
        } else {
            assert!(audio.next().await.is_err());
        }
        assert!(audio.next().await.unwrap().is_none());
        audio.close().await.unwrap();
        audio.close().await.unwrap();
        assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
        assert_eq!(stats.reads.load(Ordering::SeqCst), 0);
    }
}
