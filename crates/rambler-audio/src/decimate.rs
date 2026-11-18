use crate::{AudioError, FRAME_BYTES, FRAME_SAMPLES, MAX_INPUT_BYTES, lifecycle::SourceOwner};
use bytes::{Buf, Bytes};
use rambler_engine::{AudioSource, SourceError, SourceFuture};

const RADIUS: usize = 90;
const TAPS: usize = 2 * RADIUS + 1;
const SHIFT: u32 = 30;
const UNITY: i64 = 1 << SHIFT;
const DECIMATION: usize = 3;

// Center outward: 7 kHz ideal low-pass at 48 kHz, windowed with a symmetric
// Blackman window, normalized to unit DC gain, quantized to Q30, with the center
// corrected for exact integer unity. Verified independently in decimate_tests.
const HALF_Q30: [i32; RADIUS + 1] = [
    313174244, 271018570, 164738534, 43402485, -42382479, -66929857, -39560723, 6218742, 35832597,
    33690414, 8413669, -17801542, -26496751, -14702906, 5725930, 18798841, 16262795, 2268594,
    -11402160, -14862659, -6980166, 4982236, 11741772, 9013362, 0, -7890603, -8995919, -3338508,
    4086908, 7593810, 5075206, -877840, -5456730, -5452580, -1430332, 3149054, 4839893, 2755298,
    -1094864, -3650510, -3194114, -449037, 2267227, 2960334, 1389052, -988224, -2316301, -1764811,
    0, 1513551, 1703506, 623095, -750617, -1370377, -898545, 152252, 925765, 903539, 231157,
    -495575, -740528, -409206, 157565, 508162, 429260, 58142, -282221, -353418, -158632, 107649,
    239913, 173189, 0, -131676, -138504, -47064, 52305, 87381, 51922, -7881, -42317, -35838, -7781,
    13757, 16315, 6784, -1817, -3596, -1478, -55, 0,
];
const Q30: [i32; TAPS] = {
    let mut taps = [0; TAPS];
    let mut n = 0;
    while n < TAPS {
        taps[n] = HALF_Q30[n.abs_diff(RADIUS)];
        n += 1;
    }
    taps
};

fn pcm_sample(acc: i64) -> i16 {
    // Signed arithmetic shift floors; subtract one for negative half-ties so
    // rounding is nearest, ties away from zero. Clamp before narrowing.
    let rounded = (acc + UNITY / 2 - i64::from(acc < 0)) >> SHIFT;
    rounded.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16
}

struct Fir {
    history: [i16; TAPS],
    oldest: usize,
    until_output: u8,
}

impl Fir {
    fn new() -> Self {
        Self {
            history: [0; TAPS],
            oldest: 0,
            // Center first output on input sample 0; wait for its right wing.
            until_output: RADIUS as u8,
        }
    }

    fn push(&mut self, sample: i16) -> Option<i16> {
        self.history[self.oldest] = sample;
        self.oldest = (self.oldest + 1) % TAPS;
        if self.until_output != 0 {
            self.until_output -= 1;
            return None;
        }
        self.until_output = DECIMATION as u8 - 1;
        let (left, right) = self.history.split_at(self.oldest);
        // Sum of absolute Q30 weights is < 2.1 * UNITY; even full-scale PCM
        // cannot overflow i64 (including the rounding bias).
        let acc = right
            .iter()
            .chain(left)
            .zip(Q30.iter())
            .map(|(&x, &weight)| i64::from(x) * i64::from(weight))
            .sum();
        Some(pcm_sample(acc))
    }
}

/// Opt-in decimator from raw mono PCM16LE at 48 kHz to mono PCM16LE at 16 kHz.
///
/// Use before [`crate::Pcm16Opus`], not in place of the encoder. The caller
/// supplies already-acquired bytes and asserts their format; raw PCM has no
/// header to verify. No device access, format autodetection or clock recovery.
///
/// A fixed symmetric low-pass FIR centers outputs on input samples 0, 3, 6, …
/// and needs 90 input samples (1.875 ms) of right context. Missing context at
/// either end is zero. Clean EOF yields exactly `ceil(N / 3)` samples for `N`
/// complete input samples, with no extra group delay or ringing tail appended.
/// Samples round to nearest (ties away from zero) and saturate to signed 16-bit.
/// The linear filter stays within ±0.02 dB over 0–6 kHz and rejects 8–24 kHz
/// by at least 70 dB; 6–8 kHz is the transition. This does not describe stream
/// edge transients, PCM rounding/clipping or codec/recognition quality.
///
/// Empty chunks and odd fragmentation are allowed; one unmatched final byte is
/// [`AudioError::TruncatedPcm`]. Uses one current input chunk of at most
/// [`MAX_INPUT_BYTES`] logical bytes, fixed filter state, and output blocks of
/// at most [`FRAME_BYTES`] bytes.
/// As with the encoder, a sliced `Bytes` may retain a larger backing allocation.
///
/// Clean end padding is produced only after successful upstream close. On
/// failure, `next` repeats the original error until explicit close. Cancelling
/// a `next` or `close` wait does not re-read consumed input or invoke upstream
/// close again. Explicitly await `close` for cleanup; Drop uses the same best-
/// effort Tokio cleanup as the encoder.
///
/// ```
/// use rambler_audio::{EncoderConfig, Pcm16Opus, Pcm48kTo16k, pcm_channel};
/// use rambler_engine::AudioSource;
///
/// let (sender, input) = pcm_channel(); // Producer sends 48 kHz mono PCM16LE.
/// let audio: Box<dyn AudioSource> = Box::new(Pcm16Opus::new(
///     Pcm48kTo16k::new(input), EncoderConfig::default(),
/// ));
/// // Hand audio to the engine/collector; drive sender concurrently.
/// ```
pub struct Pcm48kTo16k<S: AudioSource> {
    upstream: SourceOwner<S>,
    input: Bytes,
    low_byte: Option<u8>,
    fir: Fir,
    real_samples: u64,
    emitted: u64,
    eof: bool,
    done: bool,
    failure: Option<SourceError>,
}

impl<S: AudioSource> Pcm48kTo16k<S> {
    /// Owns `source` without reading it. Its input must be 48 kHz mono PCM16LE.
    pub fn new(source: S) -> Self {
        Self {
            upstream: SourceOwner::new(source),
            input: Bytes::new(),
            low_byte: None,
            fir: Fir::new(),
            real_samples: 0,
            emitted: 0,
            eof: false,
            done: false,
            failure: None,
        }
    }

    fn stop(&mut self) {
        self.done = true;
        self.input = Bytes::new();
        self.low_byte = None;
        self.fir = Fir::new();
    }

    fn input_sample(&mut self) -> Option<i16> {
        if self.input.is_empty() {
            return None;
        }
        if let Some(low) = self.low_byte.take() {
            return Some(i16::from_le_bytes([low, self.input.get_u8()]));
        }
        if self.input.len() == 1 {
            self.low_byte = Some(self.input.get_u8());
            return None;
        }
        Some(self.input.get_i16_le())
    }

    fn convert_chunk(&mut self) -> Result<Option<Bytes>, AudioError> {
        let mut output = [0; FRAME_BYTES];
        let mut len = 0;
        while len < FRAME_BYTES {
            let Some(sample) = self.input_sample() else {
                break;
            };
            self.real_samples = self
                .real_samples
                .checked_add(1)
                .ok_or(AudioError::CounterOverflow)?;
            if let Some(filtered) = self.fir.push(sample) {
                output[len..len + 2].copy_from_slice(&filtered.to_le_bytes());
                len += 2;
                self.emitted += 1; // Never more than ceil(real_samples / 3).
            }
        }
        if self.input.is_empty() {
            self.input = Bytes::new();
        }
        Ok((len != 0).then(|| Bytes::copy_from_slice(&output[..len])))
    }

    fn tail(&mut self) -> Option<Bytes> {
        // At most ceil(RADIUS / 3) centers lacked their right context.
        let mut output = [0; 2 * RADIUS.div_ceil(DECIMATION)];
        let mut len = 0;
        let target = self.real_samples.div_ceil(DECIMATION as u64);
        while self.emitted < target {
            if let Some(filtered) = self.fir.push(0) {
                output[len..len + 2].copy_from_slice(&filtered.to_le_bytes());
                len += 2;
                self.emitted += 1;
            }
        }
        self.stop();
        (len != 0).then(|| Bytes::copy_from_slice(&output[..len]))
    }

    async fn step(&mut self) -> Result<Option<Bytes>, SourceError> {
        if self.done {
            return Ok(None);
        }
        loop {
            // Yield before each bounded conversion block or upstream read,
            // including empty/instantly ready sources. No converted block is
            // held across an await, so cancellation cannot lose it.
            tokio::task::yield_now().await;
            if self.eof {
                if self.low_byte.is_some() {
                    return Err(SourceError::new(AudioError::TruncatedPcm));
                }
                self.upstream.close().await?;
                return Ok(self.tail());
            }
            if !self.input.is_empty() {
                if let Some(output) = self.convert_chunk().map_err(SourceError::new)? {
                    return Ok(Some(output));
                }
                continue;
            }
            let Some(source) = self.upstream.source.as_mut() else {
                unreachable!("live decimator must own its source");
            };
            match source.next().await? {
                None => self.eof = true,
                Some(chunk) => {
                    if chunk.len() > MAX_INPUT_BYTES {
                        return Err(SourceError::new(AudioError::InputLimit));
                    }
                    self.input = chunk;
                }
            }
        }
    }
}

impl<S: AudioSource> AudioSource for Pcm48kTo16k<S> {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            let error = match self.failure.clone() {
                Some(error) => error,
                None => match self.step().await {
                    Ok(output) => return Ok(output),
                    Err(error) => {
                        self.stop();
                        self.failure = Some(error.clone());
                        error
                    }
                },
            };
            // Primary failure survives even cancellation while closing. Any
            // separate close failure can still be retrieved via explicit close.
            let _ = self.upstream.close().await;
            Err(error)
        })
    }

    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.stop();
        self.failure = None;
        Box::pin(async move { self.upstream.close().await })
    }
}

const _: () = assert!(RADIUS.div_ceil(DECIMATION) <= FRAME_SAMPLES);

#[cfg(test)]
#[path = "decimate_tests.rs"]
mod tests;
