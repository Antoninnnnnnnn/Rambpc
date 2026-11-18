use atomic_waker::AtomicWaker;
use bytes::Bytes;
use rambler_engine::{AudioSource, SourceError, SourceFuture};
use std::{
    future::poll_fn,
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicUsize, Ordering},
    },
    task::Poll,
};

/// Hard limit on reserved PCM payload per capture handoff (8 MiB).
pub const MAX_CAPTURE_BYTES: usize = crate::MAX_INPUT_BYTES;
/// Hard limit on the number of queued chunks per capture handoff.
pub const MAX_CAPTURE_CHUNKS: usize = 4096;

const OPEN: u8 = 0;
const FINISHED: u8 = 1;
const ABORTED: u8 = 2;
const OVERFLOW: u8 = 3;
const INVALID_CHUNK: u8 = 4;
// Independent of the producer outcome: source close never overwrites a fault.
const CLOSED: u8 = 128;

/// Capacity of a single-producer, single-consumer PCM16LE capture handoff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureConfig {
    /// Maximum number of unread chunks, including short chunks.
    pub max_chunks: usize,
    /// Maximum size of one chunk in bytes. Must be positive and even.
    /// The ring reserves `max_chunks * max_chunk_bytes` PCM bytes (half as many
    /// samples) up front. Both this product and the slot count have hard limits.
    pub max_chunk_bytes: usize,
}

/// Typed capture failure. Diagnostics don't contain PCM or other caller data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CaptureError {
    #[error("invalid PCM capture capacity")]
    InvalidConfig,
    #[error("PCM capture requires a nonempty chunk of complete 16-bit samples")]
    InvalidChunk,
    #[error("PCM capture handoff capacity exceeded")]
    Overflow,
    #[error("PCM capture aborted before normal finish")]
    Aborted,
    #[error("PCM capture consumer closed")]
    Closed,
}

/// Sets up an opt-in, preallocated handoff from a caller-owned PCM callback.
///
/// Construct it off the callback thread. Pass [`CaptureInput`] to
/// [`crate::collect_long_pcm_dictation`] for 16 kHz mono PCM16LE, or through one
/// continuous [`crate::Pcm48kTo16k`] for 48 kHz mono PCM16LE. This channel neither
/// acquires audio nor infers format or sample rate. It does not change
/// [`crate::pcm_channel`] and does not itself create a task or require Tokio.
///
/// The handoff retains no caller-supplied backing allocation. It reserves at most
/// [`MAX_CAPTURE_BYTES`] of PCM and one length word per slot; each `next` copies
/// one chunk out on the consumer side, allowing that slot to be reused regardless
/// of downstream `Bytes` lifetimes. Downstream memory is not part of this bound.
pub fn capture_channel(
    config: CaptureConfig,
) -> Result<(CaptureSender, CaptureInput), CaptureError> {
    let CaptureConfig {
        max_chunks,
        max_chunk_bytes,
    } = config;
    if !(1..=MAX_CAPTURE_CHUNKS).contains(&max_chunks)
        || max_chunk_bytes == 0
        || !max_chunk_bytes.is_multiple_of(2)
    {
        return Err(CaptureError::InvalidConfig);
    }
    let capacity = max_chunks
        .checked_mul(max_chunk_bytes)
        .filter(|&capacity| capacity <= MAX_CAPTURE_BYTES)
        .ok_or(CaptureError::InvalidConfig)?;
    let ring = Arc::new(Ring {
        state: AtomicU8::new(OPEN),
        read: AtomicUsize::new(0),
        write: AtomicUsize::new(0),
        lengths: (0..max_chunks).map(|_| AtomicUsize::new(0)).collect(),
        pcm: (0..capacity).map(|_| AtomicU8::new(0)).collect(),
        max_chunk_bytes,
        waiter: AtomicWaker::new(),
    });
    Ok((
        CaptureSender {
            ring: Arc::clone(&ring),
        },
        CaptureInput { ring: Some(ring) },
    ))
}

struct Ring {
    state: AtomicU8,
    // Each cursor has a single writer and ranges over twice the slot count:
    // equal cursors are empty; a distance of one ring is full. One release store
    // publishes each copy, and the opposite acquire load gates slot reuse/read.
    read: AtomicUsize,
    write: AtomicUsize,
    lengths: Box<[AtomicUsize]>,
    pcm: Box<[AtomicU8]>,
    max_chunk_bytes: usize,
    waiter: AtomicWaker,
}

impl Ring {
    fn advance(&self, cursor: usize) -> usize {
        (cursor + 1) % (2 * self.lengths.len())
    }

    fn chunk(&self, slot: usize, len: usize) -> &[AtomicU8] {
        let start = slot * self.max_chunk_bytes;
        &self.pcm[start..start + len]
    }

    fn end(&self, state: u8) -> Result<(), CaptureError> {
        match self
            .state
            .compare_exchange(OPEN, state, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {
                self.waiter.wake();
                Ok(())
            }
            Err(existing) => Err(outcome(existing)),
        }
    }

    fn fail(&self, state: u8) -> CaptureError {
        match self.end(state) {
            Ok(()) => outcome(state),
            Err(existing) => existing,
        }
    }

    fn read_ready(&self) -> Poll<Result<Option<Bytes>, SourceError>> {
        // Observe a terminal state before checking the cursor, so finished/fault
        // observations include all earlier publications by that same producer.
        let state = self.state.load(Ordering::Acquire);
        if state == ABORTED {
            return Poll::Ready(Err(SourceError::new(CaptureError::Aborted)));
        }
        let read = self.read.load(Ordering::Relaxed);
        if read != self.write.load(Ordering::Acquire) {
            let slot = read % self.lengths.len();
            let len = self.lengths[slot].load(Ordering::Relaxed);
            let data: Vec<_> = self
                .chunk(slot, len)
                .iter()
                .map(|byte| byte.load(Ordering::Relaxed))
                .collect();
            self.read.store(self.advance(read), Ordering::Release);
            return Poll::Ready(Ok(Some(Bytes::from(data))));
        }
        match state {
            OPEN => Poll::Pending,
            FINISHED => Poll::Ready(Ok(None)),
            _ => Poll::Ready(Err(SourceError::new(outcome(state)))),
        }
    }
}

fn outcome(state: u8) -> CaptureError {
    match state & !CLOSED {
        ABORTED => CaptureError::Aborted,
        OVERFLOW => CaptureError::Overflow,
        INVALID_CHUNK => CaptureError::InvalidChunk,
        _ => CaptureError::Closed,
    }
}

/// The unique non-cloneable writer for a single capture callback at a time.
/// Dropping without normal [`Self::finish`] has the same semantics as abort.
pub struct CaptureSender {
    ring: Arc<Ring>,
}

impl CaptureSender {
    /// Copies a borrowed PCM chunk in capture order, or accepts none of it.
    ///
    /// The ring does not wait for the reader, lock, allocate or create tasks.
    /// Its work is bounded by the configured chunk size. Waking the consumer
    /// may call its executor's waker synchronously; that code is outside the
    /// ring's guarantees. `Ok` acknowledges queue acceptance only. A concurrent
    /// consumer close can discard the chunk, and a concurrent read may free a
    /// slot just after a full queue has been observed.
    ///
    /// A full queue or oversized even chunk permanently latches `Overflow`;
    /// an empty or odd-length chunk permanently latches `InvalidChunk`. Stop
    /// capture after any error; no later offer can replace the missed audio.
    /// Previously accepted chunks remain in order for the reader to drain.
    pub fn offer(&mut self, pcm: &[u8]) -> Result<(), CaptureError> {
        let ring = &self.ring;
        let state = ring.state.load(Ordering::Acquire);
        if state != OPEN {
            return Err(outcome(state));
        }
        if pcm.is_empty() || !pcm.len().is_multiple_of(2) {
            return Err(ring.fail(INVALID_CHUNK));
        }
        let write = ring.write.load(Ordering::Relaxed);
        let read = ring.read.load(Ordering::Acquire);
        if pcm.len() > ring.max_chunk_bytes || write.abs_diff(read) == ring.lengths.len() {
            return Err(ring.fail(OVERFLOW));
        }
        let slot = write % ring.lengths.len();
        for (target, &byte) in ring.chunk(slot, pcm.len()).iter().zip(pcm) {
            target.store(byte, Ordering::Relaxed);
        }
        ring.lengths[slot].store(pcm.len(), Ordering::Relaxed);
        ring.write.store(ring.advance(write), Ordering::Release);
        ring.waiter.wake();
        Ok(())
    }

    /// Declare intentional EOF; the consumer drains every accepted chunk before
    /// seeing EOF. This cannot hide a previously latched capture failure, and is
    /// not a dictation completion acknowledgement.
    pub fn finish(self) -> Result<(), CaptureError> {
        self.ring.end(FINISHED)
    }

    /// Abandon capture. The consumer discards unread chunks and reports `Aborted`
    /// instead of EOF. A `next` racing abort may already be reading a chunk.
    /// An earlier fault wins and remains visible to both ends.
    pub fn abort(self) -> Result<(), CaptureError> {
        self.ring.end(ABORTED)
    }
}

impl Drop for CaptureSender {
    fn drop(&mut self) {
        let _ = self.ring.end(ABORTED);
    }
}

/// The receiving [`AudioSource`]. `next` is cancellation safe. Normal finish
/// drains to `None`; overflow and invalid input drain the accepted prefix to a
/// sticky typed error. Producer abort/drop yields a sticky error without draining.
/// Explicit source close/drop discards pending PCM, causes further offers to fail
/// and releases this endpoint's ownership. The reservation is freed when both
/// endpoints are gone. After explicit close, `next` returns `None`.
pub struct CaptureInput {
    ring: Option<Arc<Ring>>,
}

impl CaptureInput {
    fn close_now(&mut self) {
        if let Some(ring) = self.ring.take() {
            ring.state.fetch_or(CLOSED, Ordering::AcqRel);
            // Don't retain a cancelled consumer's task through this ring.
            drop(ring.waiter.take());
        }
    }
}

impl AudioSource for CaptureInput {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(poll_fn(|cx| {
            let Some(ring) = &self.ring else {
                return Poll::Ready(Ok(None));
            };
            let mut ready = ring.read_ready();
            if ready.is_pending() {
                ring.waiter.register(cx.waker());
                // A publication racing registration must not be missed.
                ready = ring.read_ready();
            }
            if ready.is_ready() {
                drop(ring.waiter.take());
            }
            ready
        }))
    }

    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.close_now();
        Box::pin(async { Ok(()) })
    }
}

impl Drop for CaptureInput {
    fn drop(&mut self) {
        self.close_now();
    }
}
