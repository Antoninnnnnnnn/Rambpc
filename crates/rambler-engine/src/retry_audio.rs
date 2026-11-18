use crate::{AudioSource, SourceError, SourceFuture, StreamAudio};
use bytes::Bytes;
use futures_util::stream;
use std::sync::{Arc, Mutex, Weak};

/// Per-utterance limits on retained encoded payload bytes and nonempty chunks.
/// Counts exclude allocator/bookkeeping overhead and buffers outside retention.
/// A zero limit disables retention. Exceeding either limit discards the entire
/// capture, but does not interrupt the original upload.
#[derive(Clone, Copy, Debug)]
pub struct RetryAudioLimits {
    pub max_bytes: usize,
    pub max_chunks: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RetryAudioUnavailable {
    #[error("audio did not reach normal EOF and successful cleanup")]
    Incomplete,
    #[error("encoded audio retention limit exceeded or disabled")]
    LimitExceeded,
}

/// Opt-in, one-attempt retention of encoded audio for an explicit user retry.
/// Copies only audio, not options, credentials, context, or responses. Audio is
/// kept in process memory only; the SDK does not persist it or securely erase it.
/// Dropping this handle discards retention, even while the first source is live.
/// If consumed for retry, the new source owns the bytes until consumed/dropped.
/// Do not keep the handle beyond the current utterance's retry decision.
///
/// The SDK never restarts an RPC. Wait for the original collection to fail,
/// then only on user request, pass [`Self::into_source`] to a new session with
/// fresh options. A new session has fresh protobuf timing, deadlines and context;
/// only the encoded audio chunks and their boundaries are identical.
pub struct RetryAudio(Arc<Mutex<Capture>>);

impl RetryAudio {
    /// Wrap an encoded source (for example `Pcm16Opus` after the PCM channel)
    /// and return the separate retention handle. The wrapped source forwards
    /// input bytes and awaits the original cleanup unchanged.
    pub fn capture(
        source: Box<dyn AudioSource>,
        limits: RetryAudioLimits,
    ) -> (Self, Box<dyn AudioSource>) {
        let capture = if limits.max_bytes == 0 || limits.max_chunks == 0 {
            Capture::Unavailable(RetryAudioUnavailable::LimitExceeded)
        } else {
            Capture::Collecting {
                chunks: Vec::new(),
                bytes: 0,
            }
        };
        let owner = Arc::new(Mutex::new(capture));
        let wrapped = CapturingAudio {
            source,
            retained: Arc::downgrade(&owner),
            limits,
            eof: false,
        };
        (Self(owner), Box::new(wrapped))
    }

    /// Take the complete, one-shot audio source for a new session. Never returns
    /// a partial recording: normal source EOF and successful `close` are required.
    /// This does not open an RPC. Drop the handle instead if no retry is wanted.
    pub fn into_source(self) -> Result<Box<dyn AudioSource>, RetryAudioUnavailable> {
        let capture = std::mem::replace(
            &mut *self.0.lock().unwrap(),
            Capture::Unavailable(RetryAudioUnavailable::Incomplete),
        );
        match capture {
            Capture::Ready(chunks) => Ok(Box::new(StreamAudio::new(stream::iter(
                chunks.into_iter().map(Ok),
            )))),
            Capture::Unavailable(reason) => Err(reason),
            Capture::Collecting { .. } => Err(RetryAudioUnavailable::Incomplete),
        }
    }
}

enum Capture {
    Collecting { chunks: Vec<Bytes>, bytes: usize },
    Ready(Vec<Bytes>),
    Unavailable(RetryAudioUnavailable),
}

impl Capture {
    fn push(&mut self, chunk: &Bytes, limits: RetryAudioLimits) {
        if chunk.is_empty() {
            return;
        }
        if let Self::Collecting { chunks, bytes } = self {
            if chunk.len() > limits.max_bytes - *bytes || chunks.len() >= limits.max_chunks {
                *self = Self::Unavailable(RetryAudioUnavailable::LimitExceeded);
            } else {
                // A Bytes view can otherwise pin a much larger backing allocation.
                chunks.push(Bytes::copy_from_slice(chunk));
                *bytes += chunk.len();
            }
        }
    }

    fn finish(&mut self, complete: bool) {
        if let Self::Collecting { chunks, .. } = self {
            *self = if complete {
                Self::Ready(std::mem::take(chunks))
            } else {
                Self::Unavailable(RetryAudioUnavailable::Incomplete)
            };
        }
    }
}

struct CapturingAudio {
    source: Box<dyn AudioSource>,
    retained: Weak<Mutex<Capture>>,
    limits: RetryAudioLimits,
    eof: bool,
}

impl AudioSource for CapturingAudio {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async {
            let result = self.source.next().await;
            self.eof = matches!(result, Ok(None));
            if let Some(retained) = self.retained.upgrade() {
                let mut capture = retained.lock().unwrap();
                match &result {
                    Ok(Some(chunk)) => capture.push(chunk, self.limits),
                    Ok(None) => {}
                    Err(_) => capture.finish(false),
                }
            }
            result
        })
    }

    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async {
            if !self.eof
                && let Some(retained) = self.retained.upgrade()
            {
                retained.lock().unwrap().finish(false);
            }
            let result = self.source.close().await;
            if self.eof
                && let Some(retained) = self.retained.upgrade()
            {
                retained.lock().unwrap().finish(result.is_ok());
            }
            result
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: RetryAudioLimits = RetryAudioLimits {
        max_bytes: 1,
        max_chunks: 1,
    };

    struct OneChunk {
        chunk: Option<Bytes>,
        fails_close: bool,
    }

    impl AudioSource for OneChunk {
        fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
            Box::pin(async { Ok(self.chunk.take()) })
        }
        fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
            Box::pin(async {
                if self.fails_close {
                    Err(SourceError::new(std::io::Error::other(
                        "synthetic cleanup error",
                    )))
                } else {
                    Ok(())
                }
            })
        }
    }

    fn one_chunk(fails_close: bool) -> Box<dyn AudioSource> {
        Box::new(OneChunk {
            chunk: Some(Bytes::from_static(b"a")),
            fails_close,
        })
    }

    #[tokio::test]
    async fn incomplete_input_or_failed_source_cleanup_cannot_be_retried() {
        for eof in [false, true] {
            let (retained, mut audio) = RetryAudio::capture(one_chunk(eof), LIMITS);
            assert!(audio.next().await.unwrap().is_some());
            if eof {
                assert!(audio.next().await.unwrap().is_none());
            }
            assert_eq!(audio.close().await.is_err(), eof);
            assert!(matches!(
                retained.into_source(),
                Err(RetryAudioUnavailable::Incomplete)
            ));
        }
    }

    #[tokio::test]
    async fn dropping_handle_releases_retention_while_source_can_still_finish() {
        let (retained, mut audio) = RetryAudio::capture(one_chunk(false), LIMITS);
        let state = Arc::downgrade(&retained.0);
        assert!(audio.next().await.unwrap().is_some());
        drop(retained);
        assert!(state.upgrade().is_none());
        assert!(audio.next().await.unwrap().is_none());
        audio.close().await.unwrap();
    }
}
