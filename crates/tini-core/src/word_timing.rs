//! Word timing: start and end second of every word.
//!
//! 1. Coarse timing: the reading is transcribed back with whisper and the heard words are matched to the target words
//!    by sequence alignment (unmatched ones are spread between their neighbours by letter count). Without a
//!    transcription model the line's speech span is split by letter count.
//! 2. Refinement: coarse bounds are snapped to the audio energy (10 ms frames, audio high-passed at 100 Hz).
//! 3. Core edges (peak -15 dB) and a 9-feature combiner; line rule (no long silence inside a word).

use std::collections::HashMap;

use realfft::RealFftPlanner;
use serde::{Deserialize, Serialize};

/// Scale of the coarse-timing prior when snapping: a boundary this far from its coarse estimate costs one unit.
pub const COARSE_TOLERANCE_S: f64 = 0.16;
pub const FRAME_SECONDS: f64 = 0.01;
pub const HIGHPASS_HZ: f64 = 100.0;
pub const SNAP_COARSE_WEIGHT: f64 = 3.0;
pub const CORE_DB: f64 = 15.0;
pub const TRIM_PAD: f64 = 0.04;
pub const TRIM_FADE: f64 = 0.008;
const COMBINER_WINDOW: i64 = 16;
const COMBINER_GAP: f64 = 0.35;
const COMBINER_LETTER_SECONDS: f64 = 0.06;
const COMBINER_INTERNAL_GAP: f64 = 0.05;
const W_D2: f64 = -0.6247187773936547;
const W_ABS_D: f64 = -0.409397500489501;
const W_SLOPE: f64 = 0.3722002428409541;
const W_TIMBRE: f64 = 0.11928912848021733;
const W_DURATION: f64 = -0.7577336915093991;
const W_INTERNAL_LEVEL: f64 = -0.17517597182213396;
const W_BEGIN_INTERNAL: f64 = 1.353039975703895;
const W_BEGIN_PAUSE: f64 = 0.41866650608017886;
const MEL_BANDS: usize = 40;
const MEL_FFT: usize = 1024;
const LINE_SILENCE_DB: f64 = 30.0;
const LINE_SILENCE_SECONDS: f64 = 0.3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Word {
    pub word: String,
    pub t0: f64,
    pub t1: f64,
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

// --- coarse timing ---------------------------------------------------------------------------------------------------

/// Units to time: whitespace-separated words; characters for zh/ja (punctuation sticks to the previous one).
pub fn units(text: &str, language: &str) -> Vec<String> {
    let words: Vec<String> = text.split_whitespace().map(String::from).collect();
    if language != "zh" && language != "ja" {
        return words;
    }
    let mut out: Vec<String> = Vec::new();
    for word in words {
        for ch in word.chars() {
            if !out.is_empty() && !ch.is_alphanumeric() {
                out.last_mut().unwrap().push(ch);
            } else {
                out.push(ch.to_string());
            }
        }
    }
    out
}

// --- refinement: snapping to audio energy ----------------------------------------------------------------------------

/// Zero-phase 4th-order Butterworth high-pass (forward-backward magnitude 1/(1+(fc/f)^8)), via FFT.
pub fn highpass(audio: &[f32], sample_rate: u32, cutoff: f64) -> Vec<f32> {
    let n = audio.len();
    if n < 2 {
        return audio.to_vec();
    }
    let mut planner = RealFftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(n);
    let inverse = planner.plan_fft_inverse(n);
    let mut input: Vec<f64> = audio.iter().map(|x| *x as f64).collect();
    let mut spectrum = forward.make_output_vec();
    forward.process(&mut input, &mut spectrum).unwrap();
    for (k, c) in spectrum.iter_mut().enumerate() {
        let freq = k as f64 * sample_rate as f64 / n as f64;
        let gain = 1.0 / (1.0 + (cutoff / freq.max(1e-9)).powi(8));
        *c *= gain;
    }
    // irfft: the imaginary part of DC (and Nyquist when n is even) must be zero
    spectrum[0].im = 0.0;
    if n.is_multiple_of(2) {
        let last = spectrum.len() - 1;
        spectrum[last].im = 0.0;
    }
    let mut output = inverse.make_output_vec();
    inverse.process(&mut spectrum, &mut output).unwrap();
    output.iter().map(|v| (*v / n as f64) as f32).collect()
}

fn levels_db(audio: &[f32], sample_rate: u32) -> Vec<f64> {
    let size = (sample_rate as f64 * FRAME_SECONDS) as usize;
    let count = audio.len() / size;
    (0..count)
        .map(|i| {
            let frame = &audio[i * size..(i + 1) * size];
            let mean = frame.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / size as f64;
            10.0 * (mean + 1e-12).log10()
        })
        .collect()
}

/// numpy.percentile (linear interpolation).
fn percentile(values: &[f64], q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pos = q / 100.0 * (v.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    v[lo] + (v[hi] - v[lo]) * (pos - lo as f64)
}

fn letters(word: &str) -> usize {
    word.chars().filter(|c| c.is_alphanumeric()).count().max(1)
}

fn boundary_cost(levels: &[f64], speech_db: f64, range_db: f64) -> Vec<f64> {
    let n = levels.len();
    (0..n)
        .map(|i| {
            let a = levels[i.saturating_sub(1)];
            let b = levels[i];
            let c = levels[(i + 1).min(n - 1)];
            let a = if i == 0 { levels[0] } else { a };
            (((a + b + c) / 3.0 - (speech_db - range_db)) / range_db).clamp(0.0, 1.0)
        })
        .collect()
}

fn pause_bounds(
    words: &[Word],
    voiced: &[bool],
    min_pause: usize,
) -> HashMap<usize, (usize, usize)> {
    let index: Vec<usize> = voiced
        .iter()
        .enumerate()
        .filter(|(_, v)| **v)
        .map(|(i, _)| i)
        .collect();
    let mut forced = HashMap::new();
    for pair in index.windows(2) {
        let (before, after) = (pair[0], pair[1]);
        if after - before < min_pause {
            continue;
        }
        let pause_start = (before + 1) as f64 * FRAME_SECONDS;
        let Some(mut owner) = words
            .iter()
            .position(|w| w.t0 <= pause_start && pause_start < w.t1)
        else {
            continue;
        };
        let word = &words[owner];
        let last_alnum = word
            .word
            .chars()
            .last()
            .map(|c| c.is_alphanumeric())
            .unwrap_or(false);
        if !last_alnum || pause_start - word.t0 > word.t1 - pause_start {
            owner += 1;
        }
        if owner > 0 && owner < words.len() && !forced.contains_key(&(owner - 1)) {
            forced.insert(owner - 1, (before + 1, after));
        }
    }
    forced
}

/// Snaps a line's coarse timings (seconds from line start) to the audio energy.
pub fn snap_line(words: &[Word], audio: &[f32], sample_rate: u32) -> Vec<Word> {
    let (coarse_weight, duration_weight, energy_weight, below_speech_db, min_pause_seconds) =
        (SNAP_COARSE_WEIGHT, 3.0, 3.0, 35.0, 0.15);
    let levels = levels_db(audio, sample_rate);
    let levels_v = levels_db(&highpass(audio, sample_rate, HIGHPASS_HZ), sample_rate);
    if levels.is_empty() {
        return words.to_vec();
    }
    let speech_db = percentile(&levels, 95.0);
    let threshold = percentile(&levels_v, 95.0) - below_speech_db;
    let voiced: Vec<bool> = levels_v.iter().map(|l| *l > threshold).collect();
    let voiced_index: Vec<usize> = voiced
        .iter()
        .enumerate()
        .filter(|(_, v)| **v)
        .map(|(i, _)| i)
        .collect();
    if voiced_index.is_empty() {
        return words.to_vec();
    }
    let (start, end) = (voiced_index[0], *voiced_index.last().unwrap() + 1);
    let mut voiced_sum = vec![0usize; voiced.len() + 1];
    for (i, v) in voiced.iter().enumerate() {
        voiced_sum[i + 1] = voiced_sum[i] + *v as usize;
    }
    let energy = boundary_cost(&levels, speech_db, 40.0);
    let lets: Vec<f64> = words.iter().map(|w| letters(&w.word) as f64).collect();
    let total_letters: f64 = lets.iter().sum();
    let expected: Vec<f64> = lets
        .iter()
        .map(|l| l * voiced_sum[end] as f64 / total_letters)
        .collect();
    let coarse_bounds: Vec<f64> = words
        .iter()
        .take(words.len().saturating_sub(1))
        .map(|w| w.t1 / FRAME_SECONDS)
        .collect();
    let coarse_scale = COARSE_TOLERANCE_S / FRAME_SECONDS;
    let forced = pause_bounds(
        words,
        &voiced,
        (min_pause_seconds / FRAME_SECONDS).round() as usize,
    );
    let frames = end - start + 1;
    let positions: Vec<usize> = (start..=end).collect();
    if frames <= words.len() {
        return words.to_vec();
    }
    let mut cost = vec![f64::INFINITY; frames];
    cost[0] = 0.0;
    let mut back: Vec<Vec<usize>> = Vec::new();
    for index in 0..words.len() {
        let last = index == words.len() - 1;
        let mut new_cost = vec![f64::INFINITY; frames];
        let mut choice = vec![0usize; frames];
        let targets: Vec<usize> = if last {
            vec![frames - 1]
        } else {
            (1..frames).collect()
        };
        for target in targets {
            let mut best = 0usize;
            let mut best_total = f64::INFINITY;
            for previous in 0..target {
                let spoken =
                    (voiced_sum[positions[target]] - voiced_sum[positions[previous]]) as f64;
                let total = cost[previous]
                    + duration_weight * ((spoken + 1.0) / (expected[index] + 1.0)).ln().powi(2);
                if total < best_total {
                    best_total = total;
                    best = previous;
                }
            }
            let mut boundary = 0.0;
            if !last {
                let distance =
                    (positions[target] as f64 - coarse_bounds[index]).abs() / coarse_scale;
                boundary = energy_weight * energy[positions[target].min(energy.len() - 1)]
                    + coarse_weight * distance.min(3.0);
                if let Some((lo, hi)) = forced.get(&index) {
                    if !(*lo <= positions[target] && positions[target] <= *hi) {
                        boundary = f64::INFINITY;
                    }
                }
            }
            new_cost[target] = best_total + boundary;
            choice[target] = best;
        }
        back.push(choice);
        cost = new_cost;
    }
    if !cost[frames - 1].is_finite() {
        return words.to_vec();
    }
    let mut bounds = vec![frames - 1];
    for index in (0..words.len()).rev() {
        let last = *bounds.last().unwrap();
        bounds.push(back[index][last]);
    }
    bounds.reverse();
    let bounds: Vec<usize> = bounds.iter().map(|b| positions[*b]).collect();
    words
        .iter()
        .enumerate()
        .map(|(index, word)| {
            let (lo, hi) = (bounds[index], bounds[index + 1]);
            let seg: Vec<usize> = (lo..hi.min(voiced.len())).filter(|i| voiced[*i]).collect();
            let a = seg.first().copied().unwrap_or(lo);
            let b = seg.last().map(|x| x + 1).unwrap_or(lo + 1);
            Word {
                word: word.word.clone(),
                t0: round3(a as f64 * FRAME_SECONDS),
                t1: round3(b as f64 * FRAME_SECONDS),
            }
        })
        .collect()
}

/// Narrows word edges relative to the word's own peak (peak - 15 dB).
pub fn core_edges(words: &[Word], audio: &[f32], sample_rate: u32) -> Vec<Word> {
    let levels = levels_db(&highpass(audio, sample_rate, HIGHPASS_HZ), sample_rate);
    words
        .iter()
        .map(|word| {
            let a = (word.t0 / FRAME_SECONDS).round() as usize;
            let b = ((word.t1 / FRAME_SECONDS).round() as usize).min(levels.len());
            if a >= b {
                return word.clone();
            }
            let seg = &levels[a..b];
            let max = seg.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let keep: Vec<usize> = seg
                .iter()
                .enumerate()
                .filter(|(_, v)| **v > max - CORE_DB)
                .map(|(i, _)| i)
                .collect();
            match (keep.first(), keep.last()) {
                (Some(f), Some(l)) => Word {
                    word: word.word.clone(),
                    t0: round3((a + f) as f64 * FRAME_SECONDS),
                    t1: round3((a + l + 1) as f64 * FRAME_SECONDS),
                },
                _ => word.clone(),
            }
        })
        .collect()
}

// --- combiner --------------------------------------------------------------------------------------------------------

fn zscore(x: &[f64]) -> Vec<f64> {
    let n = x.len() as f64;
    let mean = x.iter().sum::<f64>() / n;
    let std = (x.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
    x.iter().map(|v| (v - mean) / (std + 1e-9)).collect()
}

fn mel_bank(sample_rate: u32) -> Vec<Vec<f64>> {
    let to_mel = |f: f64| 2595.0 * (1.0 + f / 700.0).log10();
    let to_hz = |m: f64| 700.0 * (10f64.powf(m / 2595.0) - 1.0);
    let (lo, hi) = (to_mel(80.0), to_mel(8000.0));
    let points: Vec<f64> = (0..MEL_BANDS + 2)
        .map(|i| to_hz(lo + (hi - lo) * i as f64 / (MEL_BANDS + 1) as f64))
        .collect();
    let bins: Vec<usize> = points
        .iter()
        .map(|p| ((MEL_FFT + 1) as f64 * p / sample_rate as f64).floor() as usize)
        .collect();
    let mut bank = vec![vec![0.0; MEL_FFT / 2 + 1]; MEL_BANDS];
    for (i, row) in bank.iter_mut().enumerate() {
        let (a, b, c) = (bins[i], bins[i + 1], bins[i + 2]);
        for (k, w) in row.iter_mut().enumerate().take(c).skip(a) {
            *w = if k < b {
                (k - a) as f64 / (b - a) as f64
            } else {
                (c - k) as f64 / (c - b) as f64
            };
        }
    }
    bank
}

struct Features {
    l: Vec<f64>,
    timbre: Vec<f64>,
    n: usize,
    reference: f64,
}

fn features(audio: &[f32], sample_rate: u32) -> Features {
    let hp = highpass(audio, sample_rate, HIGHPASS_HZ);
    let hop = (sample_rate as f64 * FRAME_SECONDS) as usize;
    let count = hp.len() / hop;
    let mut levels = levels_db(&hp, sample_rate);
    levels.truncate(count);
    let window: Vec<f64> = (0..MEL_FFT)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (MEL_FFT - 1) as f64).cos())
        .collect();
    let bank = mel_bank(sample_rate);
    let mut planner = RealFftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(MEL_FFT);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    let mut mel = vec![vec![0.0; MEL_BANDS]; count];
    for (f, row) in mel.iter_mut().enumerate() {
        for i in 0..MEL_FFT {
            let idx = f * hop + i;
            input[i] = if idx < hp.len() {
                hp[idx] as f64 * window[i]
            } else {
                0.0
            };
        }
        fft.process(&mut input, &mut spectrum).unwrap();
        for (band, weights) in bank.iter().enumerate() {
            let e: f64 = spectrum
                .iter()
                .zip(weights)
                .map(|(c, w)| c.norm_sqr() * w)
                .sum();
            row[band] = (e + 1e-10).ln();
        }
    }
    if count > 0 {
        for band in 0..MEL_BANDS {
            let mean = mel.iter().map(|r| r[band]).sum::<f64>() / count as f64;
            mel.iter_mut().for_each(|r| r[band] -= mean);
        }
    }
    let mean_rows = |a: usize, b: usize| -> Vec<f64> {
        let mut v = vec![0.0; MEL_BANDS];
        for r in &mel[a..b] {
            for (i, x) in r.iter().enumerate() {
                v[i] += x;
            }
        }
        v.iter_mut().for_each(|x| *x /= (b - a) as f64);
        v
    };
    let mut timbre = vec![0.0; count];
    for (f, out) in timbre.iter_mut().enumerate() {
        let a = if f > 0 {
            mean_rows(f.saturating_sub(4), f)
        } else {
            mel[0].clone()
        };
        let b = mean_rows(f, (f + 4).min(count));
        let dot: f64 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
        let na = a.iter().map(|x| x * x).sum::<f64>().sqrt();
        let nb = b.iter().map(|x| x * x).sum::<f64>().sqrt();
        *out = 1.0 - dot / (na * nb + 1e-9);
    }
    let reference = percentile(&levels, 95.0);
    Features {
        l: levels,
        timbre,
        n: count,
        reference,
    }
}

fn silences(quiet: &[bool], minimum: usize) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut f = 0;
    while f < quiet.len() {
        if quiet[f] {
            let mut g = f;
            while g < quiet.len() && quiet[g] {
                g += 1;
            }
            if g - f >= minimum {
                runs.push((f, g));
            }
            f = g;
        } else {
            f += 1;
        }
    }
    runs
}

/// Line rule: no long silence inside a word.
fn line_silence(
    line_of: &[usize],
    picked: &mut HashMap<(usize, char), f64>,
    levels: &[f64],
    reference: f64,
) {
    let minimum = (LINE_SILENCE_SECONDS / FRAME_SECONDS).round() as usize;
    for i in 0..line_of.len() {
        let (Some(&b), Some(&s)) = (picked.get(&(i, 'b')), picked.get(&(i, 'e'))) else {
            continue;
        };
        let last = i == line_of.len() - 1 || line_of[i + 1] != line_of[i];
        let first = i == 0 || line_of[i - 1] != line_of[i];
        if !(last || first) {
            continue;
        }
        let f0 = (b / FRAME_SECONDS) as usize;
        let f1 = ((s / FRAME_SECONDS) as usize).min(levels.len());
        if f1 <= f0 + minimum {
            continue;
        }
        let quiet: Vec<bool> = levels[f0..f1]
            .iter()
            .map(|l| *l < reference - LINE_SILENCE_DB)
            .collect();
        let runs = silences(&quiet, minimum);
        if runs.is_empty() {
            continue;
        }
        if last {
            picked.insert((i, 'e'), (f0 + runs[0].0) as f64 * FRAME_SECONDS);
        } else {
            picked.insert(
                (i, 'b'),
                (f0 + runs.last().unwrap().1) as f64 * FRAME_SECONDS,
            );
        }
    }
}

/// A line passed to the combiner.
pub struct LineInput<'a> {
    pub words: Vec<Word>,
    pub audio: &'a [f32],
}

/// Re-picks the core edges of the whole reading with the combiner; returns words per line.
pub fn combine_reading(lines: &[LineInput], sample_rate: u32) -> Vec<Vec<Word>> {
    let w = COMBINER_WINDOW;
    let fr = FRAME_SECONDS;
    let mut parts: Vec<f32> = Vec::new();
    let mut starts = Vec::new();
    let mut cursor = 0.0;
    let mut words: Vec<(Word, usize)> = Vec::new();
    for (number, line) in lines.iter().enumerate() {
        starts.push(cursor);
        for k in &line.words {
            words.push((
                Word {
                    word: k.word.clone(),
                    t0: k.t0 + cursor,
                    t1: k.t1 + cursor,
                },
                number,
            ));
        }
        parts.extend_from_slice(line.audio);
        cursor += line.audio.len() as f64 / sample_rate as f64;
        if number < lines.len() - 1 {
            parts.extend(std::iter::repeat_n(
                0.0f32,
                (COMBINER_GAP * sample_rate as f64) as usize,
            ));
            cursor += COMBINER_GAP;
        }
    }
    if words.is_empty() {
        return lines.iter().map(|l| l.words.clone()).collect();
    }
    let feat = features(&parts, sample_rate);
    let (l, n) = (&feat.l, feat.n as i64);
    let ks: Vec<i64> = (-w..=w).collect();
    let mut picked: HashMap<(usize, char), f64> = HashMap::new();
    for i in 0..words.len() {
        let (word, _) = &words[i];
        let prev_t1 = if i > 0 { words[i - 1].0.t1 } else { -9.0 };
        let next_t0 = if i + 1 < words.len() {
            words[i + 1].0.t0
        } else {
            9e3
        };
        let lets = letters(&word.word) as f64;
        for kind in ['b', 'e'] {
            let current = if kind == 'b' { word.t0 } else { word.t1 };
            let fc = (current / fr).round() as i64;
            if fc < 4 || fc > n - w - 6 {
                continue;
            }
            let internal = if kind == 'b' {
                word.t0 - prev_t1 < COMBINER_INTERNAL_GAP
            } else {
                next_t0 - word.t1 < COMBINER_INTERNAL_GAP
            };
            let internal = if internal { 1.0 } else { 0.0 };
            let fs: Vec<usize> = ks
                .iter()
                .map(|k| (fc + k).clamp(0, n - 1) as usize)
                .collect();
            let sign = if kind == 'b' { 1.0 } else { -1.0 };
            let level: Vec<f64> = fs.iter().map(|f| (l[*f] - feat.reference) / 20.0).collect();
            let slope: Vec<f64> = fs
                .iter()
                .map(|f| {
                    let up = l[((*f as i64) + 3).clamp(0, n - 1) as usize];
                    let down = l[((*f as i64) - 3).clamp(0, n - 1) as usize];
                    sign * (up - down)
                })
                .collect();
            let prior: Vec<f64> = fs
                .iter()
                .map(|f| {
                    let t = *f as f64 * fr;
                    let dur = (if kind == 'e' {
                        t - word.t0
                    } else {
                        word.t1 - t
                    })
                    .max(0.02);
                    (dur / (lets * COMBINER_LETTER_SECONDS)).ln().powi(2)
                })
                .collect();
            let begin = if kind == 'b' { 1.0 } else { 0.0 };
            let timbre: Vec<f64> = fs.iter().map(|f| feat.timbre[*f]).collect();
            let (z_slope, z_timbre, z_prior, z_level) = (
                zscore(&slope),
                zscore(&timbre),
                zscore(&prior),
                zscore(&level),
            );
            let mut best = 0usize;
            let mut best_score = f64::NEG_INFINITY;
            for (j, k) in ks.iter().enumerate() {
                let d = *k as f64 / w as f64;
                let score = W_D2 * d * d
                    + W_ABS_D * d.abs()
                    + W_SLOPE * z_slope[j]
                    + W_TIMBRE * z_timbre[j]
                    + W_DURATION * z_prior[j]
                    + W_INTERNAL_LEVEL * internal * z_level[j]
                    + W_BEGIN_INTERNAL * begin * internal * d
                    + W_BEGIN_PAUSE * begin * (1.0 - internal) * d;
                if score > best_score {
                    best_score = score;
                    best = j;
                }
            }
            picked.insert((i, kind), (fc + ks[best]) as f64 * fr);
        }
    }
    for i in 0..words.len().saturating_sub(1) {
        if let (Some(&a), Some(&b)) = (picked.get(&(i, 'e')), picked.get(&(i + 1, 'b'))) {
            if a > b {
                let mid = 0.5 * (a + b);
                picked.insert((i, 'e'), mid);
                picked.insert((i + 1, 'b'), mid);
            }
        }
    }
    let line_of: Vec<usize> = words.iter().map(|(_, s)| *s).collect();
    line_silence(&line_of, &mut picked, l, feat.reference);
    let mut out: Vec<Vec<Word>> = vec![Vec::new(); lines.len()];
    for (i, (word, line_no)) in words.iter().enumerate() {
        let start = starts[*line_no];
        let t0 = picked.get(&(i, 'b')).copied().unwrap_or(word.t0) - start;
        let t1 = picked.get(&(i, 'e')).copied().unwrap_or(word.t1) - start;
        out[*line_no].push(Word {
            word: word.word.clone(),
            t0: round3(t0),
            t1: round3(t1),
        });
    }
    out
}

/// Span of the line to keep (s): first word - pad, last word + pad; the line rule applies to trimming too.
pub fn trim_bounds(
    words: &[Word],
    line_seconds: f64,
    audio: &[f32],
    sample_rate: u32,
) -> (f64, f64) {
    if words.is_empty() {
        return (0.0, line_seconds);
    }
    let levels = levels_db(&highpass(audio, sample_rate, HIGHPASS_HZ), sample_rate);
    let last = words.len() - 1;
    let mut picked = HashMap::new();
    picked.insert((0, 'b'), words[0].t0);
    picked.insert((0, 'e'), words[0].t1);
    if last > 0 {
        picked.insert((last, 'b'), words[last].t0);
        picked.insert((last, 'e'), words[last].t1);
    }
    let line_of = vec![0usize; words.len()];
    line_silence(&line_of, &mut picked, &levels, percentile(&levels, 95.0));
    let (t0, t1) = (picked[&(0, 'b')], picked[&(last, 'e')]);
    ((t0 - TRIM_PAD).max(0.0), (t1 + TRIM_PAD).min(line_seconds))
}

/// Trims to [start, end) with an 8 ms fade at each cut.
pub fn trim(audio: &[f32], sample_rate: u32, start: f64, end: f64) -> Vec<f32> {
    let a = ((start * sample_rate as f64) as usize).min(audio.len());
    let b = ((end * sample_rate as f64) as usize).clamp(a, audio.len());
    let mut piece = audio[a..b].to_vec();
    let fade = ((TRIM_FADE * sample_rate as f64) as usize).min(piece.len() / 2);
    if fade > 1 {
        let len = piece.len();
        for i in 0..fade {
            let g = i as f32 / (fade - 1) as f32;
            piece[i] *= g;
            piece[len - 1 - i] *= g;
        }
    }
    piece
}

/// A heard (ASR) word, in the line's own time.
#[derive(Debug, Clone)]
pub struct HeardWord {
    pub word: String,
    pub t0: f64,
    pub t1: f64,
}

/// Comparison key: lowercase, letters/digits only, circumflex removed.
fn match_key(word: &str) -> String {
    crate::text::turkish_lower(word)
        .chars()
        .map(|c| match c {
            'â' => 'a',
            'î' => 'i',
            'û' => 'u',
            _ => c,
        })
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// Similarity of two keys (0-1) from the character edit distance; identical keys give 1.
fn similarity(a: &str, b: &str) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let (x, y): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    // character-level Levenshtein
    let mut prev: Vec<usize> = (0..=y.len()).collect();
    for (i, cx) in x.iter().enumerate() {
        let mut cur = vec![i + 1; y.len() + 1];
        for (j, cy) in y.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(cx != cy))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        prev = cur;
    }
    1.0 - prev[y.len()] as f64 / x.len().max(y.len()) as f64
}

/// Aligns target words with heard words (Needleman-Wunsch). A matched target takes the heard word's time; unmatched
/// ones are spread between neighbouring matches by letter count. `None` if nothing matches.
pub fn align(target: &[String], heard: &[HeardWord], line_s: f64) -> Option<Vec<Word>> {
    let (n, m) = (target.len(), heard.len());
    if n == 0 || m == 0 {
        return None;
    }
    let target_keys: Vec<String> = target.iter().map(|k| match_key(k)).collect();
    let heard_keys: Vec<String> = heard.iter().map(|k| match_key(&k.word)).collect();
    let gap = -0.4;
    let score = |i: usize, j: usize| {
        let b = similarity(&target_keys[i], &heard_keys[j]);
        if b >= 0.5 {
            b * 2.0
        } else {
            -1.0
        }
    };
    let mut t = vec![vec![0.0f64; m + 1]; n + 1];
    for (i, row) in t.iter_mut().enumerate() {
        row[0] = i as f64 * gap;
    }
    for (j, cell) in t[0].iter_mut().enumerate() {
        *cell = j as f64 * gap;
    }
    for i in 1..=n {
        for j in 1..=m {
            t[i][j] = (t[i - 1][j - 1] + score(i - 1, j - 1))
                .max(t[i - 1][j] + gap)
                .max(t[i][j - 1] + gap);
        }
    }
    let mut matches: Vec<Option<usize>> = vec![None; n];
    let (mut i, mut j) = (n, m);
    while i > 0 && j > 0 {
        if (t[i][j] - (t[i - 1][j - 1] + score(i - 1, j - 1))).abs() < 1e-9
            && score(i - 1, j - 1) > 0.0
        {
            matches[i - 1] = Some(j - 1);
            i -= 1;
            j -= 1;
        } else if (t[i][j] - (t[i - 1][j] + gap)).abs() < 1e-9 {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    if matches.iter().all(|e| e.is_none()) {
        return None;
    }
    let mut out: Vec<Option<(f64, f64)>> = matches
        .iter()
        .map(|e| e.map(|j| (heard[j].t0, heard[j].t1)))
        .collect();
    // spread unmatched runs between their neighbours
    let mut k = 0;
    while k < n {
        if out[k].is_some() {
            k += 1;
            continue;
        }
        let run_start = k;
        while k < n && out[k].is_none() {
            k += 1;
        }
        let left = if run_start == 0 {
            0.0
        } else {
            out[run_start - 1].unwrap().1
        };
        let right = if k >= n { line_s } else { out[k].unwrap().0 };
        let letter_counts: Vec<f64> = (run_start..k)
            .map(|x| letters(&target[x]).max(1) as f64)
            .collect();
        let total: f64 = letter_counts.iter().sum();
        let mut cursor = left;
        for (x, h) in (run_start..k).zip(letter_counts) {
            let share = (right - left).max(0.0) * h / total;
            out[x] = Some((cursor, cursor + share));
            cursor += share;
        }
    }
    Some(
        target
            .iter()
            .zip(out)
            .map(|(w, z)| {
                let (a, b) = z.unwrap();
                Word {
                    word: w.clone(),
                    t0: round3(a.clamp(0.0, line_s)),
                    t1: round3(b.clamp(0.0, line_s).max(a)),
                }
            })
            .collect(),
    )
}

/// Speech span of a line (s): first and last frame above peak level - 35 dB.
pub fn speech_span(audio: &[f32], sample_rate: u32) -> (f64, f64) {
    let levels = levels_db(&highpass(audio, sample_rate, HIGHPASS_HZ), sample_rate);
    let seconds = audio.len() as f64 / sample_rate as f64;
    if levels.is_empty() {
        return (0.0, seconds);
    }
    let threshold = percentile(&levels, 95.0) - 35.0;
    let first = levels.iter().position(|l| *l > threshold).unwrap_or(0);
    let last = levels
        .iter()
        .rposition(|l| *l > threshold)
        .unwrap_or(levels.len() - 1);
    (
        first as f64 * FRAME_SECONDS,
        ((last + 1) as f64 * FRAME_SECONDS).min(seconds),
    )
}

/// Coarse timing without ASR: the speech span is split by letter count.
pub fn proportional(target: &[String], audio: &[f32], sample_rate: u32) -> Vec<Word> {
    let (a, b) = speech_span(audio, sample_rate);
    let letter_counts: Vec<f64> = target
        .iter()
        .map(|w| letters(w).max(1) as f64 + 0.5)
        .collect();
    let total: f64 = letter_counts.iter().sum::<f64>().max(1.0);
    let mut cursor = a;
    target
        .iter()
        .zip(letter_counts)
        .map(|(w, h)| {
            let share = (b - a) * h / total;
            let k = Word {
                word: w.clone(),
                t0: round3(cursor),
                t1: round3(cursor + share),
            };
            cursor += share;
            k
        })
        .collect()
}

/// The whole reading: per line (words, trim bounds). `heard`: ASR words per line (proportional when absent).
pub fn reading_words(
    spokens: &[String],
    audios: &[Vec<f32>],
    heard: Option<&[Vec<HeardWord>]>,
    sample_rate: u32,
    language: &str,
) -> Vec<(Vec<Word>, (f64, f64))> {
    let mut lines = Vec::new();
    let mut bounds = Vec::new();
    for (index, (spoken, audio)) in spokens.iter().zip(audios).enumerate() {
        let words = units(spoken, language);
        let seconds = audio.len() as f64 / sample_rate as f64;
        if words.is_empty() {
            lines.push(LineInput {
                words: vec![],
                audio,
            });
            bounds.push((0.0, seconds));
            continue;
        }
        let coarse = heard
            .and_then(|d| d.get(index))
            .and_then(|d| align(&words, d, seconds))
            .unwrap_or_else(|| proportional(&words, audio, sample_rate));
        let audible = snap_line(&coarse, audio, sample_rate);
        lines.push(LineInput {
            words: core_edges(&audible, audio, sample_rate),
            audio,
        });
        bounds.push(trim_bounds(&audible, seconds, audio, sample_rate));
    }
    combine_reading(&lines, sample_rate)
        .into_iter()
        .zip(bounds)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_split() {
        assert_eq!(units("Merhaba dünya.", "tr"), vec!["Merhaba", "dünya."]);
        assert_eq!(
            units("你好，世界。", "zh"),
            vec!["你", "好，", "世", "界。"]
        );
    }

    #[test]
    fn alignment() {
        let target: Vec<String> = ["Bugün", "hava", "çok", "güzel."]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let d = |k: &str, a: f64, b: f64| HeardWord {
            word: k.into(),
            t0: a,
            t1: b,
        };
        let heard = vec![
            d("bugün", 0.1, 0.5),
            d("hava", 0.6, 0.9),
            d("güzel", 1.3, 1.8),
        ];
        let k = align(&target, &heard, 2.0).unwrap();
        assert_eq!((k[0].t0, k[1].t1, k[3].t0), (0.1, 0.9, 1.3));
        assert!(k[2].t0 >= 0.9 && k[2].t1 <= 1.3);
        assert!(align(&target, &[d("xyz", 0.0, 1.0)], 2.0).is_none());
    }

    #[test]
    fn highpass_removes_low_frequencies() {
        let sr = 48000;
        let low: Vec<f32> = (0..sr)
            .map(|i| (2.0 * std::f32::consts::PI * 30.0 * i as f32 / sr as f32).sin())
            .collect();
        let high: Vec<f32> = (0..sr)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr as f32).sin())
            .collect();
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        assert!(rms(&highpass(&low, sr, 100.0)) < 0.01);
        assert!((rms(&highpass(&high, sr, 100.0)) - rms(&high)).abs() < 0.01);
    }

    #[test]
    fn percentile_matches_numpy() {
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 50.0), 2.5);
        assert_eq!(percentile(&[5.0], 95.0), 5.0);
    }

    #[test]
    fn trimming() {
        let a = vec![1.0f32; 48000];
        let t = trim(&a, 48000, 0.1, 0.5);
        assert_eq!(t.len(), 19200);
        assert_eq!(t[0], 0.0);
    }
}
