use crate::{lifecycle::SourceOwner, ogg::Ogg, *};
use bytes::Bytes;
use rambler_engine::{AudioSource, SourceError, SourceFuture};

/// Only the reference profile is supported; page batching may be 20..1000ms.
/// Profile validation is deferred to the first next so invalid configs still own
/// and close their upstream source without reading it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncoderConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub bitrate: u32,
    pub page_duration_ms: u16,
}
impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            channels: 1,
            bitrate: 24000,
            page_duration_ms: 20,
        }
    }
}
impl EncoderConfig {
    pub fn validate(&self) -> Result<(), AudioError> {
        if self.sample_rate != 16000
            || self.channels != 1
            || self.bitrate != 24000
            || !(20..=1000).contains(&self.page_duration_ms)
        {
            return Err(AudioError::InvalidConfig);
        }
        Ok(())
    }
}
struct Native {
    encoder: opus::Encoder,
    mux: Ogg,
    lookahead: u64,
}
impl Native {
    fn new(config: EncoderConfig) -> Result<Self, AudioError> {
        let mut encoder = opus::Encoder::new(16000, opus::Channels::Mono, opus::Application::Voip)
            .map_err(|_| AudioError::Opus)?;
        encoder
            .set_bitrate(opus::Bitrate::Bits(24000))
            .map_err(|_| AudioError::Opus)?;
        encoder.set_vbr(true).map_err(|_| AudioError::Opus)?;
        let lookahead = u64::try_from(encoder.get_lookahead().map_err(|_| AudioError::Opus)?)
            .map_err(|_| AudioError::Opus)?;
        // This profile has positive encoder delay. Reject an incompatible codec
        // rather than emit an EOS after an already-flushed final packet.
        if lookahead == 0 {
            return Err(AudioError::Opus);
        }
        let pre_skip = u16::try_from(
            lookahead
                .checked_mul(3)
                .ok_or(AudioError::CounterOverflow)?,
        )
        .map_err(|_| AudioError::Opus)?;
        Ok(Self {
            encoder,
            mux: Ogg::new(config.page_duration_ms, pre_skip)?,
            lookahead,
        })
    }
}

/// Pull-based, continuous encoder. At most one <=8MiB input chunk, one 640-byte
/// frame, one <=64KiB mux page and <=64KiB output are retained; never a whole file.
/// Every frame/input read cooperatively yields. Native calls encode one 20ms frame.
pub struct Pcm16Opus<S: AudioSource> {
    upstream: SourceOwner<S>,
    config: EncoderConfig,
    validated: bool,
    native: Option<Native>,
    input: Bytes,
    offset: usize,
    pending: [u8; FRAME_BYTES],
    pending_len: usize,
    input_bytes: u64,
    encoded_samples: u64,
    eof: bool,
    done: bool,
    failure: Option<SourceError>,
    progress: Option<PcmProgressHandle>,
}
impl<S: AudioSource> Pcm16Opus<S> {
    /// Create an encoder without allocating or locking an observer.
    pub fn new(source: S, config: EncoderConfig) -> Self {
        Self {
            upstream: SourceOwner::new(source),
            config,
            validated: false,
            native: None,
            input: Bytes::new(),
            offset: 0,
            pending: [0; FRAME_BYTES],
            pending_len: 0,
            input_bytes: 0,
            encoded_samples: 0,
            eof: false,
            done: false,
            failure: None,
            progress: None,
        }
    }

    /// Create an encoder and a bounded, thread-safe local progress observer.
    /// Encoding, framing, validation and cleanup are identical to [`Self::new`].
    /// Snapshots survive close, failure and drop, and contain no audio or source
    /// identity. Counters describe local work, not network acknowledgement.
    pub fn new_with_progress(source: S, config: EncoderConfig) -> (Self, PcmProgressHandle) {
        let mut encoder = Self::new(source, config);
        let progress = PcmProgressHandle::new();
        encoder.progress = Some(progress.clone());
        (encoder, progress)
    }

    fn stop(&mut self) {
        self.done = true;
        self.native = None;
        self.input = Bytes::new();
        self.offset = 0;
        self.pending.fill(0);
        self.pending_len = 0;
    }
    fn encode_frame(&mut self, final_packet: bool) -> Result<(), AudioError> {
        let mut samples = [0_i16; FRAME_SAMPLES];
        for (sample, pair) in samples.iter_mut().zip(self.pending.as_chunks::<2>().0) {
            *sample = i16::from_le_bytes([pair[0], pair[1]]);
        }
        let native = self.native.as_mut().ok_or(AudioError::Opus)?;
        let mut packet = [0_u8; 1275];
        let size = native
            .encoder
            .encode(&samples, &mut packet)
            .map_err(|_| AudioError::Opus)?;
        self.encoded_samples = self
            .encoded_samples
            .checked_add(FRAME_SAMPLES as u64)
            .ok_or(AudioError::CounterOverflow)?;
        if let Some(progress) = &self.progress {
            // Count successful codec work even if granule/mux accounting fails
            // below; pending_len includes real input only, never zero padding.
            progress.frame(self.pending_len / 2);
        }
        let granule = if final_packet {
            (self.input_bytes / 2)
                .checked_add(native.lookahead)
                .and_then(|n| n.checked_mul(3))
        } else {
            self.encoded_samples.checked_mul(3)
        }
        .ok_or(AudioError::CounterOverflow)?;
        native.mux.packet(&packet[..size], granule, final_packet)?;
        self.pending.fill(0);
        self.pending_len = 0;
        if final_packet {
            self.done = true;
        }
        Ok(())
    }
    async fn step(&mut self) -> Result<Option<Bytes>, SourceError> {
        if self.done && self.native.is_none() {
            return Ok(None);
        }
        if !self.validated {
            self.config.validate().map_err(SourceError::new)?;
            self.validated = true;
        }
        loop {
            if let Some(native) = self.native.as_mut()
                && let Some(output) = native.mux.pop()
            {
                return Ok(Some(output));
            }
            if self.done {
                self.native = None;
                if let Some(progress) = &self.progress {
                    progress.output_eof();
                }
                return Ok(None);
            }
            if self.eof {
                if !self.pending_len.is_multiple_of(2) {
                    return Err(SourceError::new(AudioError::TruncatedPcm));
                }
                // No EOS is observable until upstream cleanup has succeeded.
                self.upstream.close().await?;
                if self.input_bytes == 0 {
                    self.done = true;
                    if let Some(progress) = &self.progress {
                        progress.output_eof();
                    }
                    return Ok(None);
                }
                if self.native.is_none() {
                    self.native = Some(Native::new(self.config).map_err(SourceError::new)?);
                }
                let lookahead = self
                    .native
                    .as_ref()
                    .ok_or_else(|| SourceError::new(AudioError::Opus))?
                    .lookahead;
                let target = (self.input_bytes / 2)
                    .checked_add(lookahead)
                    .ok_or_else(|| SourceError::new(AudioError::CounterOverflow))?;
                let next_end = self
                    .encoded_samples
                    .checked_add(FRAME_SAMPLES as u64)
                    .ok_or_else(|| SourceError::new(AudioError::CounterOverflow))?;
                tokio::task::yield_now().await;
                self.encode_frame(next_end >= target)
                    .map_err(SourceError::new)?;
                continue;
            }
            if self.pending_len == FRAME_BYTES {
                tokio::task::yield_now().await;
                if self.native.is_none() {
                    self.native = Some(Native::new(self.config).map_err(SourceError::new)?);
                }
                self.encode_frame(false).map_err(SourceError::new)?;
                continue;
            }
            if self.offset < self.input.len() {
                let count = (FRAME_BYTES - self.pending_len).min(self.input.len() - self.offset);
                self.pending[self.pending_len..self.pending_len + count]
                    .copy_from_slice(&self.input[self.offset..self.offset + count]);
                self.pending_len += count;
                self.offset += count;
                continue;
            }
            self.input = Bytes::new();
            self.offset = 0;
            tokio::task::yield_now().await;
            let next = match self.upstream.source.as_mut() {
                Some(source) => source.next().await?,
                None => None,
            };
            match next {
                None => {
                    self.eof = true;
                    if let Some(progress) = &self.progress {
                        progress.input_eof();
                    }
                }
                Some(chunk) => {
                    if chunk.len() > MAX_INPUT_BYTES {
                        return Err(SourceError::new(AudioError::InputLimit));
                    }
                    self.input_bytes = self
                        .input_bytes
                        .checked_add(chunk.len() as u64)
                        .ok_or_else(|| SourceError::new(AudioError::CounterOverflow))?;
                    if let Some(progress) = &self.progress {
                        progress.input(self.input_bytes);
                    }
                    self.input = chunk;
                }
            }
        }
    }
}
impl<S: AudioSource> AudioSource for Pcm16Opus<S> {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            if self.failure.is_none() {
                match self.step().await {
                    Ok(output) => return Ok(output),
                    Err(error) => {
                        self.failure = Some(error);
                        self.stop();
                    }
                }
            }
            // Keep the primary error latched until explicit close, including
            // across cancellation while cleanup awaits. A failing close remains
            // observable separately via close().
            let _ = self.upstream.close().await;
            Err(self
                .failure
                .clone()
                .unwrap_or_else(|| SourceError::new(AudioError::Opus)))
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.stop();
        self.failure = None;
        Box::pin(async move { self.upstream.close().await })
    }
}
