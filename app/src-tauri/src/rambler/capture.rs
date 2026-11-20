use crate::{
    controller::level::Level,
    ipc::InterruptReason,
    model::MicChoice,
    ports::{MicError, MicStream, Microphone, PcmRate, PcmSink},
};
use rambler_audio::{CaptureConfig, CaptureError, CaptureInput, CaptureSender, capture_channel};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::time::Instant;

pub(crate) const FRAME_WAIT_MS: u64 = 1_000;
const MAX_CHUNK_BYTES: usize = 1_920; // 20 ms at 48 kHz; 640 bytes at 16 kHz.
// 320 full 20 ms slots give 6.4 s (614,400 bytes reserved) at 48 kHz.
// Coalescing short callbacks prevents them from reducing effective capacity.
// Covers a 1.45 s Chromium probe + 500 ms revalidation, even behind one
// retiring probe, and retains the existing margin for SDK segment rollover.
const CHUNKS: usize = 320;
const RELEASED_BEFORE_FRAME: u64 = u64::MAX;

pub(crate) struct CaptureHealth {
    origin: Instant,
    pub level: Level,
    last_frame: AtomicU64, // millisecond offset plus one, zero or RELEASED_BEFORE_FRAME
    opened: AtomicU64,     // same encoding; armed as soon as the port returns
    stalled: AtomicBool,
    overflow: AtomicBool,
    pub(crate) fault: OnceLock<MicError>,
}

impl CaptureHealth {
    pub(crate) fn new(origin: Instant) -> Self {
        Self {
            origin,
            level: Level::default(),
            last_frame: AtomicU64::new(0),
            opened: AtomicU64::new(0),
            overflow: AtomicBool::new(false),
            stalled: AtomicBool::new(false),
            fault: OnceLock::new(),
        }
    }

    pub(crate) fn last_frame(&self) -> Option<u64> {
        let last = self.last_frame.load(Ordering::Acquire);
        match last {
            RELEASED_BEFORE_FRAME | 0 => None,
            offset => Some(offset - 1),
        }
    }

    pub(crate) fn stop_before_first_frame(&self) {
        // The first callback and an early release arbitrate on the same slot.
        // Once there is audio, let the stream stop and flush its normal tail.
        let _ = self.last_frame.compare_exchange(
            0,
            RELEASED_BEFORE_FRAME,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    pub(crate) fn failure(&self) -> Option<InterruptReason> {
        if self.overflow.load(Ordering::Acquire) {
            Some(InterruptReason::CaptureOverflow)
        } else if self.stalled.load(Ordering::Acquire) {
            Some(InterruptReason::MicrophoneStalled)
        } else {
            self.fault
                .get()
                .cloned()
                .map(|error| InterruptReason::Microphone { error })
        }
    }

    fn frame(&self) -> bool {
        let now = self.origin.elapsed().as_millis() as u64;
        // A driver resuming between controller polls cannot hide a long gap.
        let previous = self.last_frame.load(Ordering::Acquire);
        if previous == RELEASED_BEFORE_FRAME {
            return false;
        }
        if previous
            .checked_sub(1)
            .or_else(|| self.opened.load(Ordering::Acquire).checked_sub(1))
            .is_some_and(|previous| now.saturating_sub(previous) >= FRAME_WAIT_MS)
        {
            self.stalled.store(true, Ordering::Release);
            return false;
        }
        // Callbacks are serialized. The only competing update can mark an
        // early release before any frames were accepted.
        self.last_frame
            .compare_exchange(previous, now + 1, Ordering::Release, Ordering::Relaxed)
            .is_ok()
    }
}

pub(crate) struct OpenedMic {
    pub stream: Box<dyn MicStream>,
    pub input: CaptureInput,
    pub rate: PcmRate,
}

/// Run on a blocking thread. The port creates the sink before callbacks begin.
pub(crate) fn open(
    microphone: &dyn Microphone,
    choice: &MicChoice,
    health: Arc<CaptureHealth>,
    live: Arc<AtomicBool>,
) -> Result<OpenedMic, MicError> {
    if !live.load(Ordering::Acquire) {
        return Err(MicError::OpenTimeout);
    }
    let (sender, input) = capture_channel(CaptureConfig {
        max_chunks: CHUNKS,
        max_chunk_bytes: MAX_CHUNK_BYTES,
    })
    .map_err(|_| MicError::Other)?;
    let rate = Arc::new(OnceLock::new());
    let sink_rate = rate.clone();
    let sink_live = live.clone();
    let meter = health.clone();
    let fault = health.clone();
    let stream = microphone.open(
        choice,
        Box::new(move |rate| {
            let _ = sink_rate.set(rate);
            Box::new(CaptureSink {
                sender,
                health: meter,
                live: sink_live,
                data: [0; MAX_CHUNK_BYTES],
                used: 0,
                block: rate.hz() as usize / 50 * 2,
                failed: false,
            })
        }),
        Box::new(move |error| {
            let _ = fault.fault.set(error);
        }),
    )?;
    if !live.load(Ordering::Acquire) {
        stream.abort();
        return Err(MicError::OpenTimeout);
    }
    health.opened.store(
        health.origin.elapsed().as_millis() as u64 + 1,
        Ordering::Release,
    );
    let Some(&rate) = rate.get() else {
        stream.abort();
        return Err(MicError::Other);
    };
    Ok(OpenedMic {
        stream,
        input,
        rate,
    })
}

struct CaptureSink {
    sender: CaptureSender,
    health: Arc<CaptureHealth>,
    live: Arc<AtomicBool>,
    data: [u8; MAX_CHUNK_BYTES],
    used: usize,
    block: usize,
    failed: bool,
}

impl CaptureSink {
    fn offer(&mut self) {
        if let Err(error) = self.sender.offer(&self.data[..self.used]) {
            self.failed = true;
            if error == CaptureError::Overflow {
                self.health.overflow.store(true, Ordering::Release);
            }
        }
        self.used = 0;
    }
}

impl PcmSink for CaptureSink {
    fn push(&mut self, mut samples: &[i16]) {
        if samples.is_empty() || self.failed || !self.live.load(Ordering::Acquire) {
            return;
        }
        if !self.health.frame() {
            self.failed = self.health.stalled.load(Ordering::Acquire);
            return;
        }
        self.health.level.measure(samples);
        while !samples.is_empty() && !self.failed && self.live.load(Ordering::Acquire) {
            let count = samples.len().min((self.block - self.used) / 2);
            let end = self.used + 2 * count;
            let (pairs, _) = self.data[self.used..end].as_chunks_mut::<2>();
            for (dst, sample) in pairs.iter_mut().zip(samples) {
                *dst = sample.to_le_bytes();
            }
            samples = &samples[count..];
            self.used = end;
            if self.used == self.block {
                self.offer();
            }
        }
    }

    fn finish(mut self: Box<Self>) {
        if !self.live.load(Ordering::Acquire) {
            let _ = self.sender.abort();
            return;
        }
        if self.used != 0 && !self.failed {
            self.offer();
        }
        if self.failed {
            // A latched overflow remains observable; no EOF can mask a fault.
            let _ = self.sender.abort();
        } else {
            let _ = self.sender.finish();
        }
    }

    fn abort(self: Box<Self>) {
        let _ = self.sender.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rambler_engine::AudioSource;

    fn sink(rate: PcmRate) -> (Box<CaptureSink>, CaptureInput) {
        let (sender, input) = capture_channel(CaptureConfig {
            max_chunks: CHUNKS,
            max_chunk_bytes: MAX_CHUNK_BYTES,
        })
        .unwrap();
        (
            Box::new(CaptureSink {
                sender,
                health: Arc::new(CaptureHealth::new(Instant::now())),
                live: Arc::new(AtomicBool::new(true)),
                data: [0; MAX_CHUNK_BYTES],
                used: 0,
                block: rate.hz() as usize / 50 * 2,
                failed: false,
            }),
            input,
        )
    }

    #[tokio::test]
    async fn regularizes_small_callbacks_and_flushes_even_little_endian_tail() {
        for rate in [PcmRate::Hz16000, PcmRate::Hz48000] {
            let (mut sink, mut input) = sink(rate);
            let block = sink.block;
            let pcm: Vec<i16> = (0..block + 15).map(|i| -2000 + i as i16).collect();
            for fragment in pcm.chunks(73) {
                sink.push(fragment);
            }
            sink.finish();
            let mut bytes = Vec::new();
            let mut lengths = Vec::new();
            while let Some(chunk) = input.next().await.unwrap() {
                lengths.push(chunk.len());
                bytes.extend(chunk);
            }
            assert_eq!(lengths, [block, block, 30]);
            assert_eq!(
                bytes,
                pcm.into_iter()
                    .flat_map(i16::to_le_bytes)
                    .collect::<Vec<_>>()
            );
        }
    }

    #[tokio::test]
    async fn stalled_consumer_has_6400_ms_before_sticky_overflow() {
        let (mut sink, mut input) = sink(PcmRate::Hz48000);
        let block = [0; 960];
        for _ in 0..CHUNKS {
            sink.push(&block);
        }
        assert!(sink.health.failure().is_none());
        sink.push(&block);
        assert_eq!(
            sink.health.failure(),
            Some(InterruptReason::CaptureOverflow)
        );
        sink.finish();
        for _ in 0..CHUNKS {
            assert_eq!(input.next().await.unwrap().unwrap().len(), MAX_CHUNK_BYTES);
        }
        assert_eq!(
            input
                .next()
                .await
                .unwrap_err()
                .original()
                .downcast_ref::<CaptureError>(),
            Some(&CaptureError::Overflow)
        );
    }
}
