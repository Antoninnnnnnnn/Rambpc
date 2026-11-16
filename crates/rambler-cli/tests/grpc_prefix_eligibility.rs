//! Offline eligibility observation, not a benchmark or a transport/cache test.
//! Only numeric summaries are emitted; no PCM, Ogg, identifiers or hashes.
#![cfg(feature = "audio")]

use bytes::Bytes;
use rambler_audio::{EncoderConfig, FRAME_BYTES, FRAME_SAMPLES, Pcm16Opus, PcmProgress};
use rambler_engine::{AudioSource, SourceError, SourceFuture};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

const SAMPLES: usize = 80017;
const ELAPSED_MS: [u64; 3] = [0, 128, i64::MAX as u64];

#[derive(Default)]
struct Lifecycle {
    bytes: AtomicUsize,
    chunks: AtomicUsize,
    eof_reads: AtomicUsize,
    closes: AtomicUsize,
    drops: AtomicUsize,
}

struct MemorySource {
    pcm: Bytes,
    offset: usize,
    closed: bool,
    lifecycle: Arc<Lifecycle>,
}

impl AudioSource for MemorySource {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            assert!(!self.closed, "upstream read after close");
            if self.offset == self.pcm.len() {
                self.lifecycle.eof_reads.fetch_add(1, Ordering::Relaxed);
                return Ok(None);
            }
            let end = (self.offset + FRAME_BYTES).min(self.pcm.len());
            let out = self.pcm.slice(self.offset..end);
            self.offset = end;
            self.lifecycle.bytes.fetch_add(out.len(), Ordering::Relaxed);
            self.lifecycle.chunks.fetch_add(1, Ordering::Relaxed);
            Ok(Some(out))
        })
    }

    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.closed = true;
        self.lifecycle.closes.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { Ok(()) })
    }
}

impl Drop for MemorySource {
    fn drop(&mut self) {
        self.lifecycle.drops.fetch_add(1, Ordering::Relaxed);
    }
}

// Integer-only recipes copied from rambler-audio/examples/profile_pipeline.rs.
// Harmonic triangles are synthetic, not speech or semantic voice data.
fn synthetic(kind: &str, samples: usize) -> Bytes {
    let mut seed = 0x1234_5678_u32;
    let mut out = Vec::with_capacity(samples * 2);
    for i in 0..samples {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let sample = match kind {
            "silence" => 0,
            "harmonic" => {
                let triangle = |period: usize| {
                    let phase = (i % period) as i32;
                    (phase.min(period as i32 - phase) * 4 - period as i32) * 6000 / period as i32
                };
                let envelope = if (i / 3200) % 5 == 4 { 0 } else { 1 };
                ((triangle(80) + triangle(37) / 3) * envelope) as i16
            }
            "noise" => (seed >> 16) as i16 / 3,
            _ => unreachable!(),
        };
        out.extend(sample.to_le_bytes());
    }
    out.into()
}

#[derive(Clone, Copy, Default)]
struct Summary {
    messages: usize,
    eligible: usize,
    fallback: usize,
    min_bytes: usize,
    max_bytes: usize,
    total_bytes: usize,
}

impl Summary {
    fn observe(&mut self, len: usize) {
        assert!(len > 0);
        self.min_bytes = if self.messages == 0 {
            len
        } else {
            self.min_bytes.min(len)
        };
        self.max_bytes = self.max_bytes.max(len);
        self.messages += 1;
        self.eligible += usize::from(len <= 255);
        self.fallback += usize::from(len > 255);
        self.total_bytes += len;
    }

    fn assert_consistent(&self, chunks: usize) {
        assert!(chunks > 0);
        assert_eq!(self.messages, chunks);
        assert_eq!(self.eligible + self.fallback, self.messages);
        assert!(self.min_bytes > 0 && self.min_bytes <= self.max_bytes);
        assert!(self.total_bytes >= self.messages * self.min_bytes);
        assert!(self.total_bytes <= self.messages * self.max_bytes);
    }
}

fn assert_progress(previous: &PcmProgress, current: &PcmProgress) {
    assert!(current.input_bytes >= previous.input_bytes);
    assert!(current.encoded_input_samples >= previous.encoded_input_samples);
    assert!(current.encoded_frames >= previous.encoded_frames);
    assert!(!previous.input_eof || current.input_eof);
    assert!(!previous.output_eof || current.output_eof);
    assert!(current.input_bytes <= (SAMPLES * 2) as u64);
    assert!(current.encoded_input_samples <= SAMPLES as u64);
    assert!(current.encoded_input_samples * 2 <= current.input_bytes);
    assert!(current.encoded_input_samples <= current.encoded_frames * FRAME_SAMPLES as u64);
    assert!(!current.output_eof || current.input_eof);
}

async fn observe(pcm: Bytes, page_ms: u16) -> [Summary; 3] {
    assert_eq!(pcm.len(), SAMPLES * 2);
    let lifecycle = Arc::new(Lifecycle::default());
    let source = MemorySource {
        pcm,
        offset: 0,
        closed: false,
        lifecycle: lifecycle.clone(),
    };
    let (mut audio, progress) = Pcm16Opus::new_with_progress(
        source,
        EncoderConfig {
            page_duration_ms: page_ms,
            ..Default::default()
        },
    );
    let mut previous = progress.snapshot();
    assert_eq!(previous, PcmProgress::default());
    let mut summaries = [Summary::default(); 3];
    let mut chunks = 0;
    loop {
        let output = audio.next().await.expect("native pipeline failed");
        let current = progress.snapshot();
        assert_progress(&previous, &current);
        assert_eq!(
            current.input_bytes,
            lifecycle.bytes.load(Ordering::Relaxed) as u64
        );
        assert_eq!(
            current.input_eof,
            lifecycle.eof_reads.load(Ordering::Relaxed) > 0
        );
        assert_eq!(current.output_eof, output.is_none());
        previous = current;
        let Some(chunk) = output else {
            break;
        };
        assert!(!chunk.is_empty());
        assert!(chunk.len() <= rambler_audio::MAX_PENDING_BYTES);
        chunks += 1;
        for (summary, elapsed_ms) in summaries.iter_mut().zip(ELAPSED_MS) {
            let message = rambler_protocol::audio_request(&chunk, elapsed_ms)
                .expect("synthetic audio request failed");
            // Eligibility is the complete protobuf message length, not the Ogg
            // chunk length, Opus packet length or gRPC-framed message length.
            summary.observe(message.len());
        }
    }
    assert_eq!(previous.input_bytes, (SAMPLES * 2) as u64);
    assert_eq!(previous.encoded_input_samples, SAMPLES as u64);
    assert!(previous.encoded_frames >= SAMPLES.div_ceil(FRAME_SAMPLES) as u64);
    assert!(previous.input_eof && previous.output_eof);
    assert_eq!(lifecycle.bytes.load(Ordering::Relaxed), SAMPLES * 2);
    assert_eq!(
        lifecycle.chunks.load(Ordering::Relaxed),
        (SAMPLES * 2).div_ceil(FRAME_BYTES)
    );
    assert_eq!(lifecycle.eof_reads.load(Ordering::Relaxed), 1);
    // Natural EOF must already have closed upstream, before explicit cleanup.
    assert_eq!(lifecycle.closes.load(Ordering::Relaxed), 1);
    assert!(audio.next().await.expect("repeated EOF failed").is_none());
    audio.close().await.expect("explicit close failed");
    audio.close().await.expect("repeated close failed");
    drop(audio);
    assert_eq!(progress.snapshot(), previous);
    assert_eq!(lifecycle.eof_reads.load(Ordering::Relaxed), 1);
    assert_eq!(lifecycle.closes.load(Ordering::Relaxed), 1);
    assert_eq!(lifecycle.drops.load(Ordering::Relaxed), 1);
    for summary in &summaries {
        summary.assert_consistent(chunks);
    }
    summaries
}

#[tokio::test(flavor = "current_thread")]
async fn native_grpc_prefix_eligibility() {
    // One cooperative timeout covers all six encodings, not 30s per case.
    tokio::time::timeout(Duration::from_secs(30), async {
        assert_eq!(FRAME_BYTES, 640);
        let defaults = EncoderConfig::default();
        assert_eq!(defaults.sample_rate, 16000);
        assert_eq!(defaults.channels, 1);
        assert_eq!(defaults.bitrate, 24000);
        assert_eq!(defaults.page_duration_ms, 20);
        let mut rows = Vec::with_capacity(18);
        for (signal, kind) in ["silence", "harmonic", "noise"].into_iter().enumerate() {
            let pcm = synthetic(kind, SAMPLES);
            for page_ms in [defaults.page_duration_ms, 1000] {
                let summaries = observe(pcm.clone(), page_ms).await;
                for (elapsed_ms, summary) in ELAPSED_MS.into_iter().zip(summaries) {
                    rows.push((signal, page_ms, elapsed_ms, summary));
                }
            }
        }
        assert_eq!(rows.len(), 18);
        // No header: exactly 18 rows, numeric fields after the fixed prefix.
        // signal: 0=silence, 1=harmonic triangles, 2=seeded noise.
        // prefix,signal,page_ms,elapsed_ms,messages,eligible,fallback,
        // min_message_bytes,max_message_bytes,total_message_bytes
        for (signal, page_ms, elapsed_ms, summary) in rows {
            println!(
                "grpc_prefix_eligibility,{signal},{page_ms},{elapsed_ms},{},{},{},{},{},{}",
                summary.messages,
                summary.eligible,
                summary.fallback,
                summary.min_bytes,
                summary.max_bytes,
                summary.total_bytes
            );
        }
    })
    .await
    .expect("eligibility observation exceeded the overall 30s timeout");
}
