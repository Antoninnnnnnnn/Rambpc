mod common;

use bytes::Bytes;
use common::*;
use rambler_audio::*;
use rambler_engine::{AudioSource, SourceError, SourceFuture};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{
    sync::Notify,
    time::{Duration, timeout},
};

const DEADLINE: Duration = Duration::from_secs(10);

async fn failure(source: &mut impl AudioSource) -> (Vec<u8>, SourceError) {
    let mut prefix = vec![];
    // All fixtures are finite; also bound pulls independently of the deadline.
    for _ in 0..1024 {
        match source.next().await {
            Ok(Some(chunk)) => prefix.extend(chunk),
            Err(error) => return (prefix, error),
            Ok(None) => panic!("failure became healthy EOF"),
        }
    }
    panic!("fixture exceeded pull bound");
}

fn typed(error: &SourceError, expected: AudioError) {
    assert_eq!(
        error.original().downcast_ref::<AudioError>(),
        Some(&expected)
    );
}

async fn repeated(source: &mut impl AudioSource, first: &SourceError, expected: AudioError) {
    for _ in 0..3 {
        let again = source
            .next()
            .await
            .expect_err("failure must stay latched until close");
        typed(&again, expected.clone());
        // Pcm16Opus must retain the original SourceError, not reconstruct it.
        assert!(std::ptr::eq(first.original(), again.original()));
    }
}

struct Counted<S> {
    inner: S,
    closes: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
impl<S> Drop for Counted<S> {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl<S: AudioSource> AudioSource for Counted<S> {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        self.inner.next()
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.closes.fetch_add(1, Ordering::SeqCst);
        self.inner.close()
    }
}
fn counted<S>(inner: S) -> (Counted<S>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let closes = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    (
        Counted {
            inner,
            closes: closes.clone(),
            drops: drops.clone(),
        },
        closes,
        drops,
    )
}

#[tokio::test]
async fn invalid_config_stays_typed_without_reading_and_closes_once() {
    timeout(DEADLINE, async {
        let source = Source::new([pcm(320)]);
        let stats = source.stats.clone();
        let mut audio = Pcm16Opus::new(
            source,
            EncoderConfig {
                channels: 2,
                ..Default::default()
            },
        );
        let first = audio.next().await.unwrap_err();
        typed(&first, AudioError::InvalidConfig);
        repeated(&mut audio, &first, AudioError::InvalidConfig).await;
        assert_eq!(stats.reads.load(Ordering::SeqCst), 0);
        assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
        audio.close().await.unwrap();
        audio.close().await.unwrap();
        assert!(audio.next().await.unwrap().is_none());
        assert_eq!(stats.reads.load(Ordering::SeqCst), 0);
        assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn producer_abandon_latches_without_eos_and_releases_once() {
    channel_failure(false).await;
}

#[tokio::test]
async fn truncated_pcm_latches_without_eos_and_releases_once() {
    channel_failure(true).await;
}

async fn channel_failure(finish: bool) {
    timeout(DEADLINE, async {
        let (mut sender, input) = pcm_channel();
        let mut data = pcm(FRAME_SAMPLES * 4);
        if finish {
            data.push(1);
        }
        sender
            .reserve()
            .await
            .unwrap()
            .send(Bytes::from(data))
            .unwrap();
        if finish {
            sender.finish().unwrap();
        } else {
            drop(sender);
        }
        let (input, closes, drops) = counted(input);
        let mut audio = Pcm16Opus::new(input, EncoderConfig::default());
        let expected = if finish {
            AudioError::TruncatedPcm
        } else {
            AudioError::InputAbandoned
        };
        let (prefix, first) = failure(&mut audio).await;
        typed(&first, expected.clone());
        assert!(pages(&prefix).len() > 2);
        assert!(pages(&prefix).iter().all(|page| page.flags & 4 == 0));
        repeated(&mut audio, &first, expected).await;
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        audio.close().await.unwrap();
        audio.close().await.unwrap();
        assert!(audio.next().await.unwrap().is_none());
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    })
    .await
    .unwrap();
}

// Synthetic messages deliberately never belong in SourceError formatting/logs.
#[derive(Debug, thiserror::Error)]
#[error("synthetic-private-marker-{0}")]
struct Identity(u8);
struct GatedFailure {
    primary: SourceError,
    cleanup: SourceError,
    reads: Arc<AtomicUsize>,
    entered: Arc<Notify>,
    gate: Arc<Notify>,
}
impl AudioSource for GatedFailure {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let error = self.primary.clone();
        Box::pin(async move { Err(error) })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.entered.notify_one();
            self.gate.notified().await;
            Err(self.cleanup.clone())
        })
    }
}
fn same_identity(actual: &SourceError, expected: &SourceError) {
    let actual_identity = actual.original().downcast_ref::<Identity>().unwrap();
    let expected_identity = expected.original().downcast_ref::<Identity>().unwrap();
    assert!(std::ptr::eq(actual_identity, expected_identity));
    assert!(!format!("{actual:?} {actual}").contains("synthetic-private-marker"));
}

#[tokio::test]
async fn cancelled_cleanup_retains_primary_identity_and_separate_close_failure() {
    timeout(DEADLINE, async {
        for explicit_close_resumes in [false, true] {
            let primary = SourceError::new(Identity(1));
            let cleanup = SourceError::new(Identity(2));
            let reads = Arc::new(AtomicUsize::new(0));
            let entered = Arc::new(Notify::new());
            let gate = Arc::new(Notify::new());
            let (source, closes, drops) = counted(GatedFailure {
                primary: primary.clone(),
                cleanup: cleanup.clone(),
                reads: reads.clone(),
                entered: entered.clone(),
                gate: gate.clone(),
            });
            let mut audio = Pcm16Opus::new(source, EncoderConfig::default());
            {
                let mut next = audio.next();
                tokio::select! {
                    biased;
                    _ = entered.notified() => (),
                    _ = &mut next => panic!("cleanup gate was bypassed"),
                }
            }
            assert_eq!(reads.load(Ordering::SeqCst), 1);
            assert_eq!(closes.load(Ordering::SeqCst), 1);
            assert_eq!(drops.load(Ordering::SeqCst), 0);
            gate.notify_one();
            if !explicit_close_resumes {
                for _ in 0..3 {
                    same_identity(&audio.next().await.unwrap_err(), &primary);
                }
            }
            // Explicit close also resumes an unreported failure's pending cleanup.
            same_identity(&audio.close().await.unwrap_err(), &cleanup);
            same_identity(&audio.close().await.unwrap_err(), &cleanup);
            assert!(audio.next().await.unwrap().is_none());
            assert_eq!(reads.load(Ordering::SeqCst), 1);
            assert_eq!(closes.load(Ordering::SeqCst), 1);
            assert_eq!(drops.load(Ordering::SeqCst), 1);
        }
    })
    .await
    .unwrap();
}

async fn opened_wav() -> (tempfile::TempDir, WavAudio, usize) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic.wav");
    let data = pcm(128 * 1024);
    tokio::fs::write(&path, wav(&data)).await.unwrap();
    let mut audio = WavAudio::open(
        &path,
        WavConfig {
            chunk_ms: 20,
            realtime: false,
        },
    )
    .await
    .unwrap();
    // Establish a successful header parse AND actual data read before mutation.
    assert_eq!(audio.next().await.unwrap().unwrap().as_ref(), &data[..640]);
    (dir, audio, data.len() - 640)
}

async fn truncate(dir: &tempfile::TempDir) {
    let writer = tokio::fs::OpenOptions::new()
        .write(true)
        .open(dir.path().join("synthetic.wav"))
        .await
        .unwrap();
    writer.set_len(44).await.unwrap();
    assert_eq!(writer.metadata().await.unwrap().len(), 44);
}

#[cfg(windows)]
fn assert_file_released(dir: &tempfile::TempDir) {
    use std::os::windows::fs::OpenOptionsExt;
    // An exclusive open fails while WavAudio still retains its read handle.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(dir.path().join("synthetic.wav"))
        .unwrap();
    drop(file);
}
#[cfg(not(windows))]
fn assert_file_released(_: &tempfile::TempDir) {}

#[tokio::test]
async fn wav_read_failure_after_valid_open_stays_typed_until_close() {
    timeout(DEADLINE, async {
        let (dir, mut audio, remaining) = opened_wav().await;
        truncate(&dir).await;
        let (prefix, first) = failure(&mut audio).await;
        typed(&first, AudioError::TruncatedWav);
        // Prove truncation reached the reader; do not assume exact read-ahead.
        assert!(prefix.len() < remaining);
        for _ in 0..3 {
            typed(
                &audio
                    .next()
                    .await
                    .expect_err("WAV failure must stay latched until close"),
                AudioError::TruncatedWav,
            );
        }
        assert_file_released(&dir);
        audio.close().await.unwrap();
        audio.close().await.unwrap();
        assert!(audio.next().await.unwrap().is_none());
        assert_file_released(&dir);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn wav_read_failure_through_encoder_stays_typed_without_fabricated_eos() {
    timeout(DEADLINE, async {
        let (dir, wav, _) = opened_wav().await;
        let (wav, closes, drops) = counted(wav);
        let mut audio = Pcm16Opus::new(wav, EncoderConfig::default());
        let mut prefix = vec![];
        // Prove real Ogg audio was emitted before inducing a read failure.
        for _ in 0..3 {
            prefix.extend(audio.next().await.unwrap().unwrap());
        }
        assert!(pages(&prefix).len() > 2);
        truncate(&dir).await;
        let (tail, first) = failure(&mut audio).await;
        prefix.extend(tail);
        typed(&first, AudioError::TruncatedWav);
        assert!(pages(&prefix).iter().all(|page| page.flags & 4 == 0));
        repeated(&mut audio, &first, AudioError::TruncatedWav).await;
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_file_released(&dir);
        audio.close().await.unwrap();
        assert!(audio.next().await.unwrap().is_none());
        assert_eq!(closes.load(Ordering::SeqCst), 1);
    })
    .await
    .unwrap();
}
