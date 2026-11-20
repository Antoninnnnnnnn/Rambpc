//! Allocation-free sample conversion after construction.

use crate::ports::PcmRate;
use std::f64::consts::PI;

const TAPS: usize = 64;
const DELAY: usize = TAPS / 2 - 1;
const PHASES: usize = 128;
const CHUNK: usize = 1024;

pub(super) trait Sample: Copy {
    fn normalized(self) -> f32;
}

impl Sample for f32 {
    fn normalized(self) -> f32 {
        if self.is_nan() {
            0.0
        } else {
            self.clamp(-1.0, 1.0)
        }
    }
}
impl Sample for i16 {
    fn normalized(self) -> f32 {
        f32::from(self) / 32768.0
    }
}
impl Sample for i32 {
    fn normalized(self) -> f32 {
        self as f32 / 2147483648.0
    }
}
impl Sample for u16 {
    fn normalized(self) -> f32 {
        (f32::from(self) - 32768.0) / 32768.0
    }
}

fn to_pcm16(x: f32) -> i16 {
    (x * 32768.0).round() as i16 // Float-to-int cast saturates, including +1.0.
}

struct Resampler {
    kernels: Box<[[f32; TAPS]]>,
    history: [f32; TAPS],
    next: usize,
    warmup: usize,
    phase: u64,
    input_rate: u64,
    output_rate: u64,
}

impl Resampler {
    fn new(input_rate: u32, output_rate: u32) -> Self {
        // Fractional-delay Blackman-windowed sinc. Leave room for the transition
        // band below the smaller Nyquist frequency. Normalize each phase for DC.
        let cutoff = 0.43 * (f64::from(output_rate) / f64::from(input_rate)).min(1.0);
        let kernels = (0..PHASES)
            .map(|phase| {
                let frac = phase as f64 / PHASES as f64;
                let mut kernel = std::array::from_fn(|tap| {
                    let x = tap as f64 - DELAY as f64 - frac;
                    let angle = 2.0 * PI * cutoff * x;
                    let sinc = if angle == 0.0 {
                        1.0
                    } else {
                        angle.sin() / angle
                    };
                    let w = 2.0 * PI * tap as f64 / (TAPS - 1) as f64;
                    ((0.42 - 0.5 * w.cos() + 0.08 * (2.0 * w).cos()) * sinc) as f32
                });
                let sum = kernel.iter().sum::<f32>();
                kernel
                    .iter_mut()
                    .for_each(|coefficient| *coefficient /= sum);
                kernel
            })
            .collect();
        Self {
            kernels,
            history: [0.0; TAPS],
            next: 0,
            warmup: DELAY,
            phase: 0,
            input_rate: input_rate.into(),
            output_rate: output_rate.into(),
        }
    }

    fn push(&mut self, sample: f32, mut emit: impl FnMut(f32)) {
        self.history[self.next] = sample;
        self.next = (self.next + 1) % TAPS;
        if self.warmup > 0 {
            self.warmup -= 1;
            return;
        }
        // Integer phase carries the exact rational frame count across callbacks.
        self.phase += self.output_rate;
        while self.phase >= self.input_rate {
            self.phase -= self.input_rate;
            let index = (self.phase * PHASES as u64 / self.output_rate) as usize;
            let mut sum = 0.0;
            for (tap, coefficient) in self.kernels[index].iter().enumerate() {
                sum += self.history[(self.next + TAPS - 1 - tap) % TAPS] * coefficient;
            }
            emit(sum);
        }
    }
}

pub(super) struct Converter {
    channels: usize,
    resampler: Option<Resampler>,
    buffer: [i16; CHUNK],
    used: usize,
}

impl Converter {
    pub(super) fn new(channels: u16, input_rate: u32, output_rate: PcmRate) -> Self {
        let output_rate = output_rate.hz();
        Self {
            channels: channels.into(),
            resampler: (input_rate != output_rate).then(|| Resampler::new(input_rate, output_rate)),
            buffer: [0; CHUNK],
            used: 0,
        }
    }

    fn emit(
        buffer: &mut [i16; CHUNK],
        used: &mut usize,
        sample: f32,
        push: &mut impl FnMut(&[i16]),
    ) {
        buffer[*used] = to_pcm16(sample);
        *used += 1;
        if *used == buffer.len() {
            push(buffer);
            *used = 0;
        }
    }

    pub(super) fn push<T: Sample>(&mut self, data: &[T], mut push: impl FnMut(&[i16])) {
        let mut emit = |sample| Self::emit(&mut self.buffer, &mut self.used, sample, &mut push);
        for frame in data.chunks_exact(self.channels) {
            let mono =
                frame.iter().map(|sample| sample.normalized()).sum::<f32>() / self.channels as f32;
            match &mut self.resampler {
                Some(resampler) => resampler.push(mono, &mut emit),
                None => emit(mono),
            }
        }
        self.flush_block(push);
    }

    pub(super) fn finish(&mut self, mut push: impl FnMut(&[i16])) {
        if let Some(resampler) = &mut self.resampler {
            // Pad just the lookahead. Over a complete stream, the output length
            // is floor(input frames * output rate / input rate), including the tail.
            for _ in 0..DELAY {
                resampler.push(0.0, |sample| {
                    Self::emit(&mut self.buffer, &mut self.used, sample, &mut push);
                });
            }
        }
        self.flush_block(push);
    }

    fn flush_block(&mut self, mut push: impl FnMut(&[i16])) {
        if self.used > 0 {
            push(&self.buffer[..self.used]);
            self.used = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct<T: Sample>(samples: &[T], channels: u16) -> Vec<i16> {
        let mut output = Vec::new();
        Converter::new(channels, 48_000, PcmRate::Hz48000)
            .push(samples, |part| output.extend_from_slice(part));
        output
    }

    #[test]
    fn sample_types_normalize_and_saturate() {
        assert_eq!(
            direct(&[-2.0, -1.0, 0.5, 1.0, 2.0, f32::NAN], 1),
            [-32768, -32768, 16384, 32767, 32767, 0]
        );
        assert_eq!(
            direct(&[i32::MIN, -(1 << 30), 0, i32::MAX], 1),
            [-32768, -16384, 0, 32767]
        );
        assert_eq!(direct(&[0u16, 32768, 65535], 1), [-32768, 0, 32767]);
        assert_eq!(
            direct(&[i16::MIN, -1, 0, 1, i16::MAX], 1),
            [i16::MIN, -1, 0, 1, i16::MAX]
        );
    }

    #[test]
    fn stereo_mix_keeps_either_channel_at_half_weight() {
        let speech_like = [4096i16, -8192, 12288, 16384, -4096];
        for right in [false, true] {
            let frames = speech_like.map(|s| if right { [0, s] } else { [s, 0] });
            assert_eq!(direct(frames.as_flattened(), 2), speech_like.map(|s| s / 2));
        }
    }

    fn resample(input: &[f32], chunks: impl Iterator<Item = usize>) -> Vec<i16> {
        let mut converter = Converter::new(1, 44_100, PcmRate::Hz16000);
        let mut output = Vec::new();
        let mut at = 0;
        for size in chunks {
            let end = (at + size).min(input.len());
            converter.push(&input[at..end], |part| output.extend_from_slice(part));
            at = end;
            if at == input.len() {
                break;
            }
        }
        assert_eq!(at, input.len());
        converter.finish(|part| output.extend_from_slice(part));
        output
    }

    #[test]
    fn resampling_is_continuous_and_keeps_exact_length_and_tail() {
        let input: Vec<f32> = (0..44_123).map(|i| (i as f32 * 0.12).sin() * 0.5).collect();
        let whole = resample(&input, [input.len()].into_iter());
        let split = resample(&input, [1, 67, 13, 1027, 256, 3, 4096].into_iter().cycle());
        assert_eq!(split.len(), input.len() * 16_000 / 44_100);
        assert!(split == whole);
        let mut impulse = vec![0.0; 99];
        impulse[98] = 1.0;
        let tail = resample(&impulse, [98, 1].into_iter());
        assert!(tail.last().unwrap().unsigned_abs() > 100);
        for len in 0..DELAY {
            assert_eq!(
                resample(&input[..len], [len].into_iter()).len(),
                len * 16_000 / 44_100
            );
        }
    }

    #[test]
    fn filter_preserves_voice_band_and_suppresses_ultrasonic_alias() {
        let rms = |hz: f64| {
            let input: Vec<_> = (0..4410)
                .map(|i| (2.0 * PI * hz * f64::from(i) / 44_100.0).sin() as f32)
                .collect();
            let output = resample(&input, [4410].into_iter());
            let middle = &output[100..output.len() - 100];
            (middle.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / middle.len() as f64).sqrt()
        };
        assert!(rms(3000.0) > 22_000.0);
        assert!(rms(12_000.0) < 50.0);
    }
}
