//! Local-only meter for the microphone test. Pass a `SinkFactory` returning
//! `Box::new(LevelSink::new(level.clone()))` to `Microphone::open`. Poll
//! `level.get()` off the audio thread (e.g. publish it over a watch at 20 Hz).
//! The sink never stores audio, opens a connection or calls the UI.

use crate::ports::PcmSink;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

/// The latest RMS level, mapped linearly from −60 to 0 dBFS into 0..=1.
#[derive(Clone, Default)]
pub struct Level(Arc<AtomicU32>);

impl Level {
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    pub(crate) fn measure(&self, samples: &[i16]) {
        if samples.is_empty() {
            return;
        }
        let sum: f64 = samples.iter().map(|&s| f64::from(s).powi(2)).sum();
        let mean_square = sum / (samples.len() as f64 * 32768.0_f64.powi(2));
        let dbfs = 10.0 * mean_square.log10();
        self.0.store(
            (((dbfs + 60.0) / 60.0).clamp(0.0, 1.0) as f32).to_bits(),
            Ordering::Relaxed,
        );
    }

    fn reset(&self) {
        self.0.store(0, Ordering::Relaxed);
    }
}

pub struct LevelSink(Level);

impl LevelSink {
    pub fn new(level: Level) -> Self {
        level.reset();
        Self(level)
    }
}

impl PcmSink for LevelSink {
    fn push(&mut self, samples: &[i16]) {
        self.0.measure(samples);
    }

    fn finish(self: Box<Self>) {
        self.0.reset();
    }

    fn abort(self: Box<Self>) {
        self.0.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_and_full_scale_samples_and_reset() {
        let level = Level::default();
        let mut sink = Box::new(LevelSink::new(level.clone()));
        sink.push(&[0; 10]);
        assert_eq!(level.get(), 0.0);
        sink.push(&[i16::MIN; 10]);
        assert_eq!(level.get(), 1.0);
        sink.push(&[3277; 10]);
        assert!((level.get() - 2.0 / 3.0).abs() < 0.001);
        sink.finish();
        assert_eq!(level.get(), 0.0);
    }
}
