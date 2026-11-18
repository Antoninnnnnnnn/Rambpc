use super::{Q30, pcm_sample};
use bytes::Bytes;
use rambler_engine::{AudioSource, SourceError, SourceFuture};
use std::{collections::VecDeque, f64::consts::PI};

use crate::{FRAME_BYTES, Pcm48kTo16k};

// Independent centered, full-length floating-point design, quantized once;
// neither generation nor direct convolution uses the production tap layout,
// ring indexing, streaming phase, sample conversion, or signed rounding.
fn coefficients() -> [i64; 181] {
    let weights: [f64; 181] = std::array::from_fn(|n| {
        let phase = 2.0 * PI * n as f64 / 180.0;
        let blackman = 0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos();
        let t = n as f64 - 90.0;
        let normalized_cutoff = 7_000.0 / 48_000.0;
        let x = 2.0 * PI * normalized_cutoff * t;
        let sinc = if x == 0.0 { 1.0 } else { x.sin() / x };
        blackman * (2.0 * normalized_cutoff) * sinc
    });
    let sum = weights.iter().sum::<f64>();
    let scale = 2_i64.pow(30);
    let mut fixed = weights.map(|value| (value / sum * scale as f64).round() as i64);
    fixed[90] += scale - fixed.iter().sum::<i64>();
    fixed
}

#[test]
fn fixed_coefficients_meet_passband_and_stopband_contract() {
    let coefficients = coefficients();
    assert_eq!(coefficients, Q30.map(i64::from));
    assert!(coefficients.iter().map(|c| c.abs()).sum::<i64>() < 21 * (1 << 30) / 10);
    assert_eq!(coefficients.iter().sum::<i64>(), 1 << 30);

    // Bound signed zero-phase H(f) between 1 Hz grid points, not merely at
    // isolated tones: linear interpolation error <= max(|H''|) * (1 Hz)^2 / 8.
    // Triangle inequality on the analytic second derivative gives that bound.
    let scale = (1_u64 << 30) as f64;
    let rad_per_hz = 2.0 * PI / 48_000.0;
    let interpolation_error = coefficients
        .iter()
        .enumerate()
        .map(|(n, c)| c.abs() as f64 * ((n as f64 - 90.0) * rad_per_hz).powi(2))
        .sum::<f64>()
        / (8.0 * scale);
    let gain = |f: u32| {
        coefficients
            .iter()
            .enumerate()
            .map(|(n, &c)| c as f64 * ((n as f64 - 90.0) * rad_per_hz * f64::from(f)).cos())
            .sum::<f64>()
            / scale
    };
    let db_to_gain = |db: f64| 10.0_f64.powf(db / 20.0);
    for hz in 0..=6_000 {
        assert!(
            gain(hz) - interpolation_error >= db_to_gain(-0.02),
            "{hz} Hz"
        );
        assert!(
            gain(hz) + interpolation_error <= db_to_gain(0.02),
            "{hz} Hz"
        );
    }
    for hz in 8_000..=24_000 {
        assert!(
            gain(hz).abs() + interpolation_error <= db_to_gain(-70.0),
            "{hz} Hz"
        );
    }
    assert_eq!(gain(0), 1.0);
    for hz in [1_000, 3_400, 6_000] {
        assert!(gain(hz) > db_to_gain(-0.002) && gain(hz) < db_to_gain(0.002));
    }
    for hz in [10_000, 14_000] {
        assert!(gain(hz).abs() <= db_to_gain(-95.0));
    }
}

fn oracle(samples: &[i16]) -> Vec<u8> {
    let coefficients = coefficients();
    (0..samples.len())
        .step_by(3)
        .flat_map(|center| {
            let acc: i64 = coefficients
                .iter()
                .enumerate()
                .filter_map(|(n, &c)| {
                    let index = (center + n).checked_sub(90)?;
                    Some(i64::from(*samples.get(index)?) * c)
                })
                .sum();
            // Accumulators here fit in f64 without loss. round() ties away
            // from zero; Rust's float-to-integer conversion saturates.
            let value = (acc as f64 / (2.0_f64.powi(30))).round() as i16;
            value.to_le_bytes()
        })
        .collect()
}

#[test]
fn signed_rounding_and_saturation_are_explicit() {
    let half = 1 << 29;
    let unit = 2 * half;
    for (x, y) in [(half - 1, 0), (half, 1), (half + 1, 1), (3 * half, 2)] {
        assert_eq!(pcm_sample(x), y);
        assert_eq!(pcm_sample(-x), -y);
    }
    assert_eq!(pcm_sample(33_000 * unit), i16::MAX);
    assert_eq!(pcm_sample(-33_000 * unit), i16::MIN);
}

struct Parts(VecDeque<Bytes>);

impl AudioSource for Parts {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async { Ok(self.0.pop_front()) })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async { Ok(()) })
    }
}

async fn check_partition(samples: &[i16], sizes: &[usize], expected: &[u8]) {
    let bytes: Vec<_> = samples.iter().flat_map(|x| x.to_le_bytes()).collect();
    let mut parts = VecDeque::new();
    let mut offset = 0;
    for size in sizes.iter().cycle() {
        let end = (offset + size).min(bytes.len());
        parts.push_back(Bytes::copy_from_slice(&bytes[offset..end]));
        if end == bytes.len() {
            break;
        }
        offset = end;
    }
    parts.push_back(Bytes::new());
    let mut adapter = Pcm48kTo16k::new(Parts(parts));
    let mut output = Vec::new();
    while let Some(chunk) = adapter.next().await.unwrap() {
        assert!(!chunk.is_empty() && chunk.len() <= FRAME_BYTES && chunk.len().is_multiple_of(2));
        output.extend_from_slice(&chunk);
    }
    assert!(adapter.next().await.unwrap().is_none());
    adapter.close().await.unwrap();
    assert_eq!(output.len(), 2 * samples.len().div_ceil(3));
    assert_eq!(
        output,
        expected,
        "length {}, sizes {sizes:?}",
        samples.len()
    );
}

#[tokio::test]
async fn arbitrary_byte_partitions_match_the_centered_oracle() {
    let data: Vec<i16> = (0_u32..5_000)
        .scan(0xa9e7_d3f0_u32, |state, _| {
            *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            Some((*state >> 16) as i16)
        })
        .collect();
    for length in [
        0, 1, 2, 3, 4, 88, 89, 90, 91, 92, 93, 94, 959, 960, 961, 5_000,
    ] {
        let samples = &data[..length];
        let expected = oracle(samples);
        for sizes in [&[15_000][..], &[1, 0], &[181, 1, 0, 26, 637, 1, 640, 2111]] {
            check_partition(samples, sizes, &expected).await;
        }
    }
}

#[tokio::test]
async fn endpoint_impulses_dc_and_overshoot_match_oracle() {
    let mut impulses = [0; 1_001];
    for n in [0, 202, 410, 677, 1_000] {
        impulses[n] = 30_000;
    }
    check_partition(&impulses, &[1, 2, 7, 9], &oracle(&impulses)).await;
    for polarity in [1, -1] {
        // This pattern realizes the sum of absolute tap magnitudes at center 90.
        let samples = Q30.map(|c| (c.signum() * polarity * 32_767) as i16);
        let expected = oracle(&samples);
        assert_eq!(
            i16::from_le_bytes(expected[60..62].try_into().unwrap()),
            if polarity == 1 { i16::MAX } else { i16::MIN }
        );
        check_partition(&samples, &[1, 3, 17], &expected).await;
    }
    for dc in [i16::MIN, 1, i16::MAX] {
        let samples = [dc; 777];
        let expected = oracle(&samples);
        assert!(
            expected[60..458]
                .as_chunks::<2>()
                .0
                .iter()
                .all(|&s| s == dc.to_le_bytes())
        );
        check_partition(&samples, &[127, 0, 4, 55], &expected).await;
    }
}
