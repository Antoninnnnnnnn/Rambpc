//! Bounded native PCM16LE/16kHz/mono -> continuous Ogg/Opus engine sources.
//!
//! [`Pcm16Opus`] implements the engine's `AudioSource` and accepts arbitrarily
//! fragmented PCM bytes, including odd chunk boundaries. Only the observed VoIP,
//! 24kbps VBR, 20ms codec-frame profile is exposed. [`WavAudio`] reads strict PCM
//! WAVE progressively, with optional first-chunk-at-t0 file pacing. Compose them
//! as `Pcm16Opus::new(WavAudio::open(path, WavConfig::default()).await?,
//! EncoderConfig::default())`, then give ownership to the engine. For caller-fed
//! PCM dictation, [`start_pcm_dictation`] and [`start_pcm_dictation_with_observer`]
//! pair a bounded producer with an engine completion future.
//!
//! Normal EOF closes upstream BEFORE emitting a successful Ogg EOS. All tail
//! samples and encoder lookahead are flushed; EOS granules trim padding exactly.
//! Errors and cancellation do not synthesize EOS. Both sources latch failures:
//! repeated `next().await` returns the same typed primary error until explicit
//! `close`, rather than turning a failure into healthy EOF. `Pcm16Opus` also
//! preserves the original upstream error identity. Cleanup failures remain
//! separately observable through `close().await`. `next` tolerates its future
//! being dropped; call `close().await` to await cleanup. `close` is idempotent and
//! resumes an interrupted upstream close rather than invoking it twice. Drop
//! releases the native codec synchronously and schedules upstream async cleanup
//! when a Tokio runtime exists; outside a runtime only explicit close can await it.
//!
//! The sources use no Python, FFmpeg process/library, microphone, credentials,
//! network or logging. The dictation helpers use only your supplied engine.
//! See `BUILDING.md` for native toolchain, linkage audit and third-party licences.

mod dictation;
mod encode;
mod input;
mod lifecycle;
mod ogg;
mod progress;
mod wav;
pub use dictation::{start_pcm_dictation, start_pcm_dictation_with_observer};
pub use encode::{EncoderConfig, Pcm16Opus};
pub use input::{PcmInput, PcmInputError, PcmPermit, PcmSendError, PcmSender, pcm_channel};
pub use progress::{PcmProgress, PcmProgressHandle};
pub use wav::{PcmFormat, WavAudio, WavConfig};

pub const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PENDING_BYTES: usize = 64 * 1024;
pub const FRAME_SAMPLES: usize = 320;
pub const FRAME_BYTES: usize = FRAME_SAMPLES * 2;

/// Errors never include PCM, encoded bytes, filesystem paths or upstream messages.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AudioError {
    #[error("unsupported audio profile or invalid chunk/page duration")]
    InvalidConfig,
    #[error("PCM input chunk exceeds 8 MiB limit")]
    InputLimit,
    #[error("PCM input producer abandoned without finishing")]
    InputAbandoned,
    #[error("truncated PCM16LE: isolated final byte (incomplete sample)")]
    TruncatedPcm,
    #[error("native Opus operation failed")]
    Opus,
    #[error("bounded Ogg output buffer exceeded")]
    OutputLimit,
    #[error("audio stream counter overflow")]
    CounterOverflow,
    #[error("invalid or unsupported PCM16LE mono 16kHz WAVE")]
    InvalidWav,
    #[error("truncated WAVE audio or header")]
    TruncatedWav,
    #[error("WAVE I/O failed")]
    Io,
}
/// Version supplied by the statically linked codec (not a loaded system DLL).
pub fn opus_version() -> &'static str {
    opus::version()
}
