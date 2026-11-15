use std::sync::{Arc, Mutex, MutexGuard};

/// Coherent, cumulative local PCM encoder progress, containing no audio or source
/// identifiers. These counters are not network acknowledgements or source hashes.
/// Values and EOF flags never decrease, including after failure or explicit close.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PcmProgress {
    /// Bytes in accepted upstream chunks, including bytes not yet encoded.
    /// Chunks exceeding [`crate::MAX_INPUT_BYTES`] are rejected without counting.
    pub input_bytes: u64,
    /// Real PCM samples in successfully accounted native encode calls, excluding
    /// zero padding and lookahead-only frames. One sample is two input bytes.
    pub encoded_input_samples: u64,
    /// Successfully accounted native encode calls, including padding and
    /// lookahead-only frames. Each call encodes one 20ms frame.
    pub encoded_frames: u64,
    /// Upstream naturally returned `None`; cleanup may still be pending or fail.
    pub input_eof: bool,
    /// The encoder naturally returned `None` after successful upstream cleanup
    /// and draining all mux output. Early close, failure and drop do not set this.
    pub output_eof: bool,
}

/// Cloneable, thread-safe observer independent of the encoder's lifetime.
/// Stores only one bounded snapshot; no audio, paths, text or identifiers.
#[derive(Clone, Debug)]
pub struct PcmProgressHandle {
    state: Arc<Mutex<PcmProgress>>,
}

impl PcmProgressHandle {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(PcmProgress::default())),
        }
    }

    /// Read one coherent snapshot from any thread, including after close or drop.
    /// The short synchronous lock never spans codec work, upstream I/O or await.
    pub fn snapshot(&self) -> PcmProgress {
        self.lock().clone()
    }

    fn lock(&self) -> MutexGuard<'_, PcmProgress> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub(crate) fn input(&self, total_bytes: u64) {
        self.lock().input_bytes = total_bytes;
    }

    pub(crate) fn frame(&self, input_samples: usize) {
        let mut state = self.lock();
        // Both totals are bounded by the encoder's checked encoded_samples
        // counter; input_samples excludes the zero-filled part of this frame.
        state.encoded_input_samples += input_samples as u64;
        state.encoded_frames += 1;
    }

    pub(crate) fn input_eof(&self) {
        self.lock().input_eof = true;
    }

    pub(crate) fn output_eof(&self) {
        self.lock().output_eof = true;
    }
}
