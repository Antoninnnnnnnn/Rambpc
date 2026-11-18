#![cfg(feature = "audio")]

#[path = "../../rambler-audio/tests/common/mod.rs"]
mod audio_common;
#[path = "../../rambler-engine/tests/common/mod.rs"]
mod common;

use audio_common::{Source, assert_duration, collect, packets};
use bytes::Bytes;
use common::{TestServer, bounded, replies};
use futures_util::stream;
use rambler_audio::{
    CaptureConfig, CaptureError, CaptureSender, EncoderConfig, FRAME_SAMPLES, LongDictationOptions,
    Pcm16Opus, Pcm48kTo16k, capture_channel, collect_long_pcm_dictation,
};
use rambler_engine::{
    AudioSource, CollectorLimits, Engine, EngineError, SessionOptions, SourceError, SourceFuture,
};
use rambler_protocol::{
    AUDIO_DATA, AUDIO_INFO, INITIAL_PARAMETER, RECOGNITION_RESPONSE, end_request,
    wire::{Value, bytes_field, parse_fields, string_field},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};
use tonic::Status;

const SEGMENT_BLOCKS: usize = 50; // Fifty synthetic 20 ms blocks at either sample rate.
const CAPTURE_SLOTS: usize = 54;

fn session() -> SessionOptions {
    SessionOptions {
        timeout: Duration::from_secs(4),
        finalize_timeout: Some(Duration::from_secs(3)),
        ..SessionOptions::default()
    }
}

fn options() -> LongDictationOptions {
    LongDictationOptions {
        segment_budget: Duration::from_secs(1),
        pause_search: Duration::ZERO,
    }
}

fn pcm(count: usize) -> Vec<u8> {
    (0..count)
        .flat_map(|sample_index| {
            let x = (sample_index as u32).wrapping_mul(0x9e37_79b9);
            let mixed = (x ^ (x >> 13)).wrapping_mul(0x5bd1_e995);
            ((mixed >> 17) as i16 - 16_384).to_le_bytes()
        })
        .collect()
}

fn offer(sender: &mut CaptureSender, all: &[u8], block_bytes: usize) {
    for chunk in all.chunks(block_bytes) {
        sender.offer(chunk).unwrap();
    }
}

fn final_text(index: usize) -> Bytes {
    let text = string_field(1, &format!("segment {index}")).unwrap();
    let hypothesis = bytes_field(3, &text).unwrap();
    let recognition = [&b"\x08\x01"[..], &bytes_field(3, &hypothesis).unwrap()].concat();
    let extension = bytes_field(1, &recognition).unwrap();
    Bytes::from(bytes_field(RECOGNITION_RESPONSE, &extension).unwrap())
}

fn append_audio(ogg: &mut Vec<u8>, message: &[u8]) {
    let fields = parse_fields(message).unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].number, AUDIO_INFO);
    assert_eq!(fields[1].number, AUDIO_DATA);
    let Value::Bytes(data) = &fields[1].value else {
        panic!("expected audio envelope")
    };
    let fields = parse_fields(data).unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].number, 1);
    let Value::Bytes(data) = &fields[0].value else {
        panic!("expected audio bytes")
    };
    ogg.extend_from_slice(data);
}

struct HeldFinal {
    index: usize,
    ogg: Vec<u8>,
    release: oneshot::Sender<()>,
}

async fn peer() -> (TestServer, Arc<AtomicUsize>, mpsc::Receiver<HeldFinal>) {
    let (ended, finals) = mpsc::channel(4);
    let calls = Arc::new(AtomicUsize::new(0));
    let server = TestServer::new({
        let calls = Arc::clone(&calls);
        move |request| {
            let index = calls.fetch_add(1, Ordering::SeqCst);
            let ended = ended.clone();
            async move {
                let mut input = request.into_inner();
                let init = input
                    .message()
                    .await?
                    .ok_or_else(|| Status::cancelled("init not received"))?;
                assert!(
                    parse_fields(&init)
                        .unwrap()
                        .iter()
                        .any(|f| f.number == INITIAL_PARAMETER)
                );
                let mut ogg = Vec::new();
                while let Some(message) = input.message().await? {
                    if message.as_ref() == end_request() {
                        assert!(input.message().await?.is_none());
                        let (release, wait) = oneshot::channel();
                        ended
                            .send(HeldFinal {
                                index,
                                ogg,
                                release,
                            })
                            .await
                            .unwrap();
                        wait.await.unwrap();
                        return Ok(replies(stream::iter([
                            Ok(final_text(index)),
                            Ok(Bytes::from_static(b"\x08\x01")),
                        ])));
                    }
                    append_audio(&mut ogg, &message);
                }
                Err(Status::cancelled("no upload END"))
            }
        }
    })
    .await;
    (server, calls, finals)
}

async fn assert_pcm(ogg: &[u8], expected: &[u8]) {
    assert_duration(ogg, expected.len() / 2);
    let reference = collect(&mut Pcm16Opus::new(
        Source::new([expected.to_vec()]),
        EncoderConfig::default(),
    ))
    .await
    .unwrap();
    // Ogg serials differ; Opus packet equality checks content beyond duration.
    assert_eq!(packets(ogg), packets(&reference));
}

struct Watched<S> {
    inner: S,
    closes: Arc<AtomicUsize>,
}
impl<S: AudioSource> AudioSource for Watched<S> {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        self.inner.next()
    }

    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.closes.fetch_add(1, Ordering::SeqCst);
        self.inner.close()
    }
}

async fn successful_rollover(rate_factor: usize, delayed_blocks: usize) {
    bounded(async {
        let (server, calls, mut finals) = peer().await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let block_samples = FRAME_SAMPLES * rate_factor;
        let block_bytes = 2 * block_samples;
        let (mut sender, input) = capture_channel(CaptureConfig {
            max_chunks: CAPTURE_SLOTS,
            max_chunk_bytes: block_bytes,
        })
        .unwrap();
        let closes = Arc::new(AtomicUsize::new(0));
        let watched = Watched {
            inner: input,
            closes: Arc::clone(&closes),
        };
        let input: Box<dyn AudioSource> = if rate_factor == 3 {
            Box::new(Pcm48kTo16k::new(watched))
        } else {
            Box::new(watched)
        };
        // One prepended block after the first segment gives the 48 kHz FIR
        // its right context before finalization stalls. Tail is not a 20 ms block.
        let initial_blocks = SEGMENT_BLOCKS + 1;
        let raw = pcm((initial_blocks + delayed_blocks) * block_samples + 37);
        let expected = if rate_factor == 3 {
            // A single, independent source partition; the FIR must not reset at rollover.
            collect(&mut Pcm48kTo16k::new(Source::new([raw.clone()])))
                .await
                .unwrap()
        } else {
            raw.clone()
        };
        assert_eq!(expected.len(), 2 * (raw.len() / 2).div_ceil(rate_factor));
        let first = SEGMENT_BLOCKS * FRAME_SAMPLES * 2;
        let feeding = async {
            let boundary = initial_blocks * block_bytes;
            offer(&mut sender, &raw[..boundary], block_bytes);
            let gate = finals.recv().await.unwrap();
            assert_eq!(gate.index, 0);
            // The peer has the first END but withholds its final. No downstream
            // polls free a slot during these synchronous synthetic callbacks.
            offer(&mut sender, &raw[boundary..], block_bytes);
            sender.finish().unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            gate.release.send(()).unwrap();
            let second = finals.recv().await.unwrap();
            assert_eq!(second.index, 1);
            second.release.send(()).unwrap();
            (gate.ogg, second.ogg)
        };
        let ((first_ogg, second_ogg), result) = tokio::join!(
            feeding,
            collect_long_pcm_dictation(
                &engine,
                input,
                session(),
                CollectorLimits::default(),
                options()
            ),
        );
        assert_eq!(result.unwrap().text().unwrap(), "segment 0 segment 1");
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_pcm(&first_ogg, &expected[..first]).await;
        assert_pcm(&second_ogg, &expected[first..]).await;
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn callback_capture_preserves_pcm_during_different_rollover_stalls() {
    for delayed_blocks in [3, 30, 45] {
        // 60, 600, and 900 ms of synthetic PCM.
        successful_rollover(1, delayed_blocks).await;
    }
}

#[tokio::test]
async fn capture_and_continuous_48k_decimation_preserve_phase_and_tail_across_rollover() {
    successful_rollover(3, 30).await;
}

#[tokio::test]
async fn abort_during_rollover_discards_unread_capture_and_keeps_only_the_first_segment() {
    bounded(async {
        let (server, calls, mut finals) = peer().await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let block_bytes = FRAME_SAMPLES * 2;
        let segment_bytes = SEGMENT_BLOCKS * block_bytes;
        let (mut sender, input) = capture_channel(CaptureConfig {
            max_chunks: CAPTURE_SLOTS,
            max_chunk_bytes: block_bytes,
        })
        .unwrap();
        let closes = Arc::new(AtomicUsize::new(0));
        let input = Watched {
            inner: input,
            closes: Arc::clone(&closes),
        };
        let raw = pcm((SEGMENT_BLOCKS + 30) * FRAME_SAMPLES);
        let feeding = async {
            offer(&mut sender, &raw[..segment_bytes], block_bytes);
            let first = finals.recv().await.unwrap();
            assert_eq!(first.index, 0);
            offer(&mut sender, &raw[segment_bytes..], block_bytes);
            sender.abort().unwrap();
            first.release.send(()).unwrap();
            first.ogg
        };
        let (first, result) = tokio::join!(
            feeding,
            collect_long_pcm_dictation(
                &engine,
                input,
                session(),
                CollectorLimits::default(),
                options()
            ),
        );
        let failure = result.unwrap_err();
        let EngineError::Audio(source) = failure.cause else {
            panic!("expected capture abort")
        };
        assert_eq!(
            source.original().downcast_ref(),
            Some(&CaptureError::Aborted)
        );
        assert_eq!(failure.completed.text().unwrap(), "segment 0");
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert_pcm(&first, &raw[..segment_bytes]).await;
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(finals.try_recv().is_err());
    })
    .await;
}

#[tokio::test]
async fn rollover_beyond_capacity_keeps_clean_segments_and_surfaces_overflow() {
    bounded(async {
        let (server, calls, mut finals) = peer().await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let block_bytes = FRAME_SAMPLES * 2;
        let segment_bytes = SEGMENT_BLOCKS * block_bytes;
        let (mut sender, input) = capture_channel(CaptureConfig {
            max_chunks: CAPTURE_SLOTS,
            max_chunk_bytes: block_bytes,
        })
        .unwrap();
        let closes = Arc::new(AtomicUsize::new(0));
        let input = Watched {
            inner: input,
            closes: Arc::clone(&closes),
        };
        let raw = pcm((SEGMENT_BLOCKS + CAPTURE_SLOTS + 1) * FRAME_SAMPLES);
        let feeding = async {
            offer(&mut sender, &raw[..segment_bytes], block_bytes);
            let first = finals.recv().await.unwrap();
            assert_eq!(first.index, 0);
            let reject_at = segment_bytes + CAPTURE_SLOTS * block_bytes;
            offer(&mut sender, &raw[segment_bytes..reject_at], block_bytes);
            // The one-second first segment is still held. This additional block
            // is beyond 1.08 s of configured headroom and is never enqueued.
            assert_eq!(sender.offer(&raw[reject_at..]), Err(CaptureError::Overflow));
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            first.release.send(()).unwrap();
            let second = finals.recv().await.unwrap();
            assert_eq!(second.index, 1);
            second.release.send(()).unwrap();
            (first.ogg, second.ogg)
        };
        let ((first, second), result) = tokio::join!(
            feeding,
            collect_long_pcm_dictation(
                &engine,
                input,
                session(),
                CollectorLimits::default(),
                options()
            ),
        );
        let failure = result.unwrap_err();
        let EngineError::Audio(source) = failure.cause else {
            panic!("expected capture overflow")
        };
        assert_eq!(
            source.original().downcast_ref(),
            Some(&CaptureError::Overflow)
        );
        assert_eq!(failure.completed.text().unwrap(), "segment 0 segment 1");
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert_eq!(sender.finish(), Err(CaptureError::Overflow));
        assert_pcm(&first, &raw[..segment_bytes]).await;
        assert_pcm(&second, &raw[segment_bytes..2 * segment_bytes]).await;
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
        // At most a third RPC starts, but it never reaches clean upload END.
        assert!((2..=3).contains(&calls.load(Ordering::SeqCst)));
        assert!(finals.try_recv().is_err());
    })
    .await;
}
