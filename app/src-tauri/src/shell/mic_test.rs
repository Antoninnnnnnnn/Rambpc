//! Voluntary microphone level test from the settings. Local only: no voice is
//! sent anywhere, and it works before consent or while Rambler is unavailable.

use crate::ports::PcmSink;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Latest level in 0..=1 (dBFS −60..0 mapped linearly), stored as f32 bits.
#[derive(Clone, Default)]
pub struct Level(Arc<AtomicU32>);

impl Level {
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    fn set(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }
}

pub fn level_of(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = (sum / samples.len() as f64).sqrt() / 32768.0;
    if rms <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * rms.log10();
    ((db + 60.0) / 60.0).clamp(0.0, 1.0) as f32
}

pub struct LevelOnlySink {
    level: Level,
}

impl LevelOnlySink {
    pub fn new(level: Level) -> Self {
        Self { level }
    }
}

impl PcmSink for LevelOnlySink {
    fn push(&mut self, samples: &[i16]) {
        self.level.set(level_of(samples));
    }
    fn finish(self: Box<Self>) {
        self.level.set(0.0);
    }
    fn abort(self: Box<Self>) {
        self.level.set(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::level_of;

    #[test]
    fn silence_is_zero_and_full_scale_is_one() {
        assert_eq!(level_of(&[0; 160]), 0.0);
        assert!((level_of(&[i16::MAX; 160]) - 1.0).abs() < 1e-3);
        let quiet = level_of(&[33; 160]); // about -60 dBFS
        assert!(quiet < 0.02);
    }
}
