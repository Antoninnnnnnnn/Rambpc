//! The file variant of the 64-tap Blackman-windowed sinc in `platform/windows/pcm.rs`.
//! It retains phase across decoder blocks, and flushes only the filter delay.

use super::decode::{AudioError, RATE};
use std::f64::consts::PI;

const TAPS: usize = 64;
const DELAY: usize = TAPS / 2 - 1;
const PHASES: usize = 128;

pub(super) struct Resampler {
    kernels: Box<[[f32; TAPS]]>,
    history: [f32; TAPS],
    next: usize,
    warmup: usize,
    phase: u64,
    input_rate: u64,
}

impl Resampler {
    pub fn new(input_rate: u32) -> Self {
        let cutoff = 0.43 * (f64::from(RATE) / f64::from(input_rate)).min(1.0);
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
        }
    }

    pub fn push(
        &mut self,
        sample: f32,
        emit: &mut impl FnMut(f32) -> Result<(), AudioError>,
    ) -> Result<(), AudioError> {
        self.history[self.next] = sample;
        self.next = (self.next + 1) % TAPS;
        if self.warmup > 0 {
            self.warmup -= 1;
            return Ok(());
        }
        self.phase += u64::from(RATE);
        while self.phase >= self.input_rate {
            self.phase -= self.input_rate;
            let index = (self.phase * PHASES as u64 / u64::from(RATE)) as usize;
            let start = self.next + TAPS - 1;
            let sum = self.kernels[index]
                .iter()
                .enumerate()
                .map(|(tap, coefficient)| self.history[(start - tap) % TAPS] * coefficient)
                .sum();
            emit(sum)?;
        }
        Ok(())
    }

    pub fn finish(
        &mut self,
        mut emit: impl FnMut(f32) -> Result<(), AudioError>,
    ) -> Result<(), AudioError> {
        for _ in 0..DELAY {
            self.push(0.0, &mut emit)?;
        }
        Ok(())
    }
}
