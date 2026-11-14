use crate::{AudioError, MAX_INPUT_BYTES};
use bytes::Bytes;
use rambler_engine::{AudioSource, SourceError, SourceFuture};
use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};
use tokio::sync::mpsc;

const OPEN: u8 = 0;
const FINISHED: u8 = 1;
const ABANDONED: u8 = 2;
const CLOSED: u8 = 3;

/// Creates a native PCM input with exactly one queued [`Bytes`] slot.
///
/// Compose the input with [`crate::Pcm16Opus`] for PCM16LE/16kHz/mono encoding.
/// This channel opens no capture hardware and uses no credentials. Each chunk's
/// logical length is limited to [`MAX_INPUT_BYTES`]; empty chunks and odd byte
/// boundaries are allowed (the encoder checks final sample completeness). These
/// bounds do not limit backing allocations retained by sliced `Bytes`.
///
/// Reserve capacity before moving a caller-owned chunk into a permit. Acceptance
/// means queue acceptance, not consumption, delivery, or successful dictation.
/// Explicit source close/drop or engine cancellation may discard accepted PCM.
/// Call [`PcmSender::finish`] for intentional producer EOF, then still await the
/// engine's healthy terminal completion before treating dictation as successful.
/// Dropping the sender without finishing instead drains queued PCM and reports
/// [`AudioError::InputAbandoned`].
pub fn pcm_channel() -> (PcmSender, PcmInput) {
    let (sender, receiver) = mpsc::channel(1);
    let state = Arc::new(AtomicU8::new(OPEN));
    (
        PcmSender {
            sender,
            state: Arc::clone(&state),
        },
        PcmInput {
            receiver: Some(receiver),
            state,
        },
    )
}

/// A fixed, payload-free input failure. Formatting never includes PCM.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PcmInputError {
    #[error("PCM input is closed")]
    Closed,
    #[error("PCM input chunk exceeds 8 MiB limit")]
    ChunkTooLarge,
}

/// A rejected chunk, available only through explicit ownership recovery.
///
/// `Debug`, `Display`, and the error chain never expose the chunk's bytes.
pub struct PcmSendError {
    kind: PcmInputError,
    chunk: Bytes,
}
impl PcmSendError {
    /// Returns the payload-free rejection reason.
    pub fn kind(&self) -> PcmInputError {
        self.kind
    }

    /// Recovers the original rejected chunk without copying it.
    pub fn into_chunk(self) -> Bytes {
        self.chunk
    }
}
impl fmt::Debug for PcmSendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PcmSendError { chunk: [REDACTED], .. }")
    }
}
impl fmt::Display for PcmSendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PCM input chunk rejected")
    }
}
impl std::error::Error for PcmSendError {}

/// The unique, non-cloneable producer. Drop without [`Self::finish`] is an error,
/// not healthy EOF, even when the queue is empty.
pub struct PcmSender {
    sender: mpsc::Sender<Bytes>,
    state: Arc<AtomicU8>,
}
impl PcmSender {
    /// Waits for the single queue slot without taking ownership of any chunk.
    ///
    /// Cancelling this wait leaves caller-owned PCM untouched. A returned permit
    /// exclusively borrows this producer, preventing finish or another reserve
    /// until the permit is sent or dropped. Dropping a permit restores capacity.
    /// Source close/drop wakes blocked reservations with [`PcmInputError::Closed`].
    pub async fn reserve(&mut self) -> Result<PcmPermit<'_>, PcmInputError> {
        if self.state.load(Ordering::Acquire) != OPEN {
            return Err(PcmInputError::Closed);
        }
        let permit = self
            .sender
            .reserve()
            .await
            .map_err(|_| PcmInputError::Closed)?;
        if self.state.load(Ordering::Acquire) != OPEN {
            return Err(PcmInputError::Closed);
        }
        Ok(PcmPermit {
            permit,
            state: &self.state,
        })
    }

    /// Queues intentional successful producer EOF by publishing it before dropping
    /// the sender. The source drains every accepted chunk before returning `None`.
    ///
    /// This is not an engine completion acknowledgement: still await a healthy
    /// engine terminal result. Concurrent close/cancellation can discard queued
    /// PCM. Returns [`PcmInputError::Closed`] if source closure won the transition.
    pub fn finish(self) -> Result<(), PcmInputError> {
        self.state
            .compare_exchange(OPEN, FINISHED, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| PcmInputError::Closed)
    }
}
impl Drop for PcmSender {
    fn drop(&mut self) {
        // Publish abandonment before the sender field drops and wakes recv().
        let _ = self
            .state
            .compare_exchange(OPEN, ABANDONED, Ordering::AcqRel, Ordering::Acquire);
    }
}

/// One reserved queue slot. Dropping it without sending releases the slot.
pub struct PcmPermit<'a> {
    permit: mpsc::Permit<'a, Bytes>,
    state: &'a AtomicU8,
}
impl PcmPermit<'_> {
    /// Synchronously accepts a chunk into the queue, or returns it on rejection.
    ///
    /// Oversize chunks are rejected; empty and odd fragments are accepted. A
    /// source already observed closed rejects the chunk. Closure racing acceptance
    /// may discard it: `Ok(())` is not consumption or delivery acknowledgement.
    pub fn send(self, chunk: Bytes) -> Result<(), PcmSendError> {
        let kind = if chunk.len() > MAX_INPUT_BYTES {
            Some(PcmInputError::ChunkTooLarge)
        } else if self.state.load(Ordering::Acquire) != OPEN {
            Some(PcmInputError::Closed)
        } else {
            None
        };
        if let Some(kind) = kind {
            return Err(PcmSendError { kind, chunk });
        }
        self.permit.send(chunk);
        Ok(())
    }
}
impl fmt::Debug for PcmPermit<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PcmPermit { .. }")
    }
}

/// The receiving [`AudioSource`] for caller-supplied PCM, without capture hardware.
///
/// `next` is cancellation safe. Producer finish drains the queue to healthy EOF;
/// producer abandonment drains it to a latched [`AudioError::InputAbandoned`]
/// error, repeated until explicit close. Close/drop immediately mark this source
/// closed, discard queued PCM, release the receiver, and wake waiting producers.
/// Even an unpolled `close` future has already performed that transition. After
/// explicit close, `next` returns `None`.
pub struct PcmInput {
    receiver: Option<mpsc::Receiver<Bytes>>,
    state: Arc<AtomicU8>,
}
impl PcmInput {
    fn close_now(&mut self) {
        self.state.store(CLOSED, Ordering::Release);
        self.receiver = None;
    }
}
impl AudioSource for PcmInput {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            if let Some(receiver) = self.receiver.as_mut() {
                if let Some(chunk) = receiver.recv().await {
                    return Ok(Some(chunk));
                }
                self.receiver = None;
            }
            if self.state.load(Ordering::Acquire) == ABANDONED {
                Err(SourceError::new(AudioError::InputAbandoned))
            } else {
                Ok(None)
            }
        })
    }

    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.close_now();
        Box::pin(async { Ok(()) })
    }
}
impl Drop for PcmInput {
    fn drop(&mut self) {
        self.close_now();
    }
}
