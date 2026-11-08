//! Tone match (optional, off by default): nudges a reading's high-frequency balance towards the voice's own reference
//! recording, for voices whose readings come out brighter than their reference.
//!
//! The correction adapts to the voice: the speech spectrum of the voice's own reference recording is compared with the
//! reading's spectrum (relative to 1-4 kHz); bands above 4 kHz that exceed the reference are cut by at most 15 dB. The
//! filter is zero-phase (FFT) and leaves the fundamental speech band (below 4 kHz) alone. The reference recording is
//! never modified.

use realfft::RealFftPlanner;
use serde::{Deserialize, Serialize};

const N: usize = 2048;
const TOLERANCE_DB: f64 = 2.0;
const MAX_CUT_DB: f64 = 15.0;
const LOWER_LIMIT_HZ: f64 = 4000.0;

/// Band centers (Hz), roughly third-octave spacing.
const CENTERS: [f64; 13] = [
    1500.0, 2500.0, 4000.0, 5000.0, 6300.0, 7100.0, 8000.0, 9000.0, 10000.0, 11200.0, 12500.0,
    14000.0, 16000.0,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToneProfile {
    pub center_hz: Vec<f64>,
    /// Gain applied to each band (dB, ≤ 0).
    pub gain_db: Vec<f64>,
}

impl ToneProfile {
    pub fn is_neutral(&self) -> bool {
        self.gain_db.iter().all(|g| g.abs() < 0.5)
    }
    pub fn max_cut(&self) -> f64 {
        self.gain_db.iter().cloned().fold(0.0, f64::min)
    }
}

fn band_edges() -> Vec<(f64, f64)> {
    CENTERS
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let lo = if i == 0 {
                1000.0
            } else {
                (CENTERS[i - 1] * m).sqrt()
            };
            let hi = if i + 1 == CENTERS.len() {
                m * 1.12
            } else {
                (CENTERS[i + 1] * m).sqrt()
            };
            (lo, hi)
        })
        .collect()
}

/// Mean band energy (dB) over speech frames, relative to the 1-4 kHz band. None when there is no speech.
pub fn band_profile(samples: &[f32], sample_rate: u32) -> Option<Vec<f64>> {
    if samples.len() < N * 4 {
        return None;
    }
    let mut planner = RealFftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(N);
    let window: Vec<f64> = (0..N)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (N - 1) as f64).cos())
        .collect();
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();
    let edges = band_edges();
    let mut sums = vec![0.0f64; edges.len()];
    let mut base = 0.0f64;
    let threshold = 10f64.powf(-40.0 / 10.0); // frames below -40 dBFS mean power are not speech
    let mut frames = 0;
    let mut i = 0;
    while i + N <= samples.len() {
        let frame = &samples[i..i + N];
        let power = frame.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / N as f64;
        if power > threshold {
            for k in 0..N {
                input[k] = frame[k] as f64 * window[k];
            }
            fft.process(&mut input, &mut output).unwrap();
            let bin = |hz: f64| ((hz * N as f64 / sample_rate as f64) as usize).min(N / 2);
            for (b, (lo, hi)) in edges.iter().enumerate() {
                sums[b] += output[bin(*lo)..bin(*hi).max(bin(*lo) + 1)]
                    .iter()
                    .map(|c| c.norm_sqr())
                    .sum::<f64>();
            }
            base += output[bin(1000.0)..bin(4000.0)]
                .iter()
                .map(|c| c.norm_sqr())
                .sum::<f64>();
            frames += 1;
        }
        i += N / 2;
    }
    if frames < 4 || base <= 0.0 {
        return None;
    }
    Some(
        sums.iter()
            .map(|t| 10.0 * ((t + 1e-20) / base).log10())
            .collect(),
    )
}

/// Correction from reference and output profiles. If the reference's high end is cut off (e.g. a clone from an MP3),
/// those bands look very low in the reference; the cut is still capped at 15 dB.
pub fn derive_profile(reference: &[f64], output: &[f64]) -> ToneProfile {
    let mut gain: Vec<f64> = reference
        .iter()
        .zip(output)
        .zip(CENTERS.iter())
        .map(|((r, c), m)| {
            if *m < LOWER_LIMIT_HZ {
                0.0
            } else {
                -((c - r - TOLERANCE_DB).clamp(0.0, MAX_CUT_DB))
            }
        })
        .collect();
    // smooth with neighbouring bands so sharp notches do not color the voice
    let raw = gain.clone();
    for i in 0..gain.len() {
        if CENTERS[i] < LOWER_LIMIT_HZ {
            continue;
        }
        // the peak is cut fully and half spreads to neighbours (so a narrow hiss band is fixed fully too)
        let a = raw[i.saturating_sub(1)];
        let b = raw[(i + 1).min(raw.len() - 1)];
        gain[i] = raw[i].min(0.5 * a).min(0.5 * b).min(0.0);
    }
    ToneProfile {
        center_hz: CENTERS.to_vec(),
        gain_db: gain.iter().map(|g| (g * 10.0).round() / 10.0).collect(),
    }
}

fn gain_at(p: &ToneProfile, hz: f64) -> f64 {
    if hz <= p.center_hz[0] {
        return p.gain_db[0];
    }
    if hz >= *p.center_hz.last().unwrap() {
        return *p.gain_db.last().unwrap();
    }
    let lg = hz.ln();
    for i in 0..p.center_hz.len() - 1 {
        let (a, b) = (p.center_hz[i], p.center_hz[i + 1]);
        if hz >= a && hz <= b {
            let t = (lg - a.ln()) / (b.ln() - a.ln());
            return p.gain_db[i] * (1.0 - t) + p.gain_db[i + 1] * t;
        }
    }
    0.0
}

/// Zero-phase application (whole-signal FFT); fast enough because line clips are short.
pub fn apply(samples: &[f32], sample_rate: u32, p: &ToneProfile) -> Vec<f32> {
    let n = samples.len();
    if n < 64 || p.is_neutral() {
        return samples.to_vec();
    }
    let mut planner = RealFftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(n);
    let inverse = planner.plan_fft_inverse(n);
    let mut input: Vec<f64> = samples.iter().map(|v| *v as f64).collect();
    let mut spectrum = forward.make_output_vec();
    forward.process(&mut input, &mut spectrum).unwrap();
    for (k, c) in spectrum.iter_mut().enumerate() {
        let hz = k as f64 * sample_rate as f64 / n as f64;
        *c *= 10f64.powf(gain_at(p, hz) / 20.0);
    }
    spectrum[0].im = 0.0;
    if n.is_multiple_of(2) {
        let last = spectrum.len() - 1;
        spectrum[last].im = 0.0;
    }
    let mut output = inverse.make_output_vec();
    inverse.process(&mut spectrum, &mut output).unwrap();
    output.iter().map(|v| (*v / n as f64) as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(hz: u32, components: &[(f64, f32)]) -> Vec<f32> {
        (0..hz * 2)
            .map(|i| {
                let t = i as f64 / hz as f64;
                components
                    .iter()
                    .map(|(f, a)| a * (2.0 * std::f64::consts::PI * f * t).sin() as f32)
                    .sum()
            })
            .collect()
    }

    #[test]
    fn cuts_excess_band_keeps_fundamental() {
        let hz = 48000;
        let reference = signal(hz, &[(2000.0, 0.3), (8000.0, 0.02)]);
        let output = signal(hz, &[(2000.0, 0.3), (8000.0, 0.2)]);
        let rp = band_profile(&reference, hz).unwrap();
        let op = band_profile(&output, hz).unwrap();
        let p = derive_profile(&rp, &op);
        assert!(p.max_cut() < -6.0, "{:?}", p.gain_db);
        let fixed = apply(&output, hz, &p);
        let after = band_profile(&fixed, hz).unwrap();
        let i8 = CENTERS.iter().position(|m| *m == 8000.0).unwrap();
        assert!(after[i8] < op[i8] - 5.0);
        let i2 = CENTERS.iter().position(|m| *m == 2500.0).unwrap();
        assert!((after[i2] - op[i2]).abs() < 1.0);
    }

    #[test]
    fn neutral_on_similar_spectrum() {
        let hz = 48000;
        let s = signal(hz, &[(2000.0, 0.3), (8000.0, 0.05)]);
        let p = derive_profile(
            &band_profile(&s, hz).unwrap(),
            &band_profile(&s, hz).unwrap(),
        );
        assert!(p.is_neutral());
    }
}
