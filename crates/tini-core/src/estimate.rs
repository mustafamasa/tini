//! Duration estimates for long jobs: each finished job records a measurement and the next job is estimated from this
//! machine's measurements.
//!
//! Model (same as the reference project): duration = startup + chars × s/char; both values are updated per job kind
//! with an exponential moving average (new measurement weighs 30%). For readings, a per-voice factor is learned too.
//! After three measurements, one that deviates from the estimate by more than 2× is not averaged in; after three
//! outliers in a row the model is rebuilt from the latest measurement.

use serde::Serialize;
use serde_json::{json, Value};

use crate::engine::Progress;
use crate::store::Store;
use crate::Error;

const ALPHA: f64 = 0.3;
const KV_KEY: &str = "duration_stats";

#[derive(Debug, Clone, Copy)]
struct Model {
    startup_s: f64,
    s_per_char: f64,
    n: i64,
    outliers: i64,
}

/// Starting values before any measurement (close to Apple M4 measurements).
fn default_model(kind: &str) -> Model {
    let s_per_char = if kind.starts_with("design") {
        0.16
    } else {
        0.14
    };
    Model {
        startup_s: 25.0,
        s_per_char,
        n: 0,
        outliers: 0,
    }
}

fn model(stats: &Value, kind: &str) -> Model {
    let m = &stats[kind];
    match (m["startup_s"].as_f64(), m["s_per_char"].as_f64()) {
        (Some(a), Some(s)) => Model {
            startup_s: a.max(0.0),
            s_per_char: s.max(0.0),
            n: m["n"].as_i64().unwrap_or(0),
            outliers: m["outliers"].as_i64().unwrap_or(0),
        },
        _ => default_model(kind),
    }
}

fn voice_factor(stats: &Value, voice: Option<&str>) -> f64 {
    voice
        .and_then(|s| stats["voices"][s].as_f64())
        .map(|c| c.clamp(0.3, 3.0))
        .unwrap_or(1.0)
}

/// Characters to read (excluding whitespace) × how many times they are read.
pub fn chars(text: &str, times: usize) -> usize {
    text.chars().filter(|c| !c.is_whitespace()).count() * times.max(1)
}

#[derive(Debug, Clone, Serialize)]
pub struct Estimate {
    pub seconds: f64,
    pub samples: i64,
}

pub fn estimate(store: &Store, kind: &str, chars: usize, voice: Option<&str>) -> Estimate {
    let stats = store.kv(KV_KEY).ok().flatten().unwrap_or(json!({}));
    let m = model(&stats, kind);
    let seconds = (m.startup_s + chars as f64 * m.s_per_char) * voice_factor(&stats, voice);
    Estimate {
        seconds: (seconds * 10.0).round() / 10.0,
        samples: m.n,
    }
}

/// Records a finished job. `steps`: progress reports; `actual_s`: actual duration.
pub fn record(
    store: &Store,
    kind: &str,
    chars: usize,
    steps: &[Progress],
    actual_s: f64,
    voice: Option<&str>,
) -> Result<bool, Error> {
    if chars == 0 || actual_s <= 0.0 {
        return Ok(false);
    }
    let mut stats = store.kv(KV_KEY)?.unwrap_or(json!({}));
    if !stats.is_object() {
        stats = json!({});
    }
    let m = model(&stats, kind);
    let base = m.startup_s + chars as f64 * m.s_per_char;
    let expected = base * voice_factor(&stats, voice);
    let mut rebuild = false;
    if m.n >= 3 && !(expected / 2.0 <= actual_s && actual_s <= expected * 2.0) {
        if m.outliers + 1 < 3 {
            stats[kind] = json!({"startup_s": m.startup_s, "s_per_char": m.s_per_char, "n": m.n, "outliers": m.outliers + 1});
            store.set_kv(KV_KEY, &stats)?;
            return Ok(false);
        }
        rebuild = true;
    }
    let first = steps.iter().find(|a| a.completed >= 1);
    let (startup_s, rate) = match first {
        Some(a) if a.total > a.completed && actual_s > a.elapsed_s => {
            let (b, t, t1) = (a.completed as f64, a.total as f64, a.elapsed_s);
            let rate = (actual_s - t1) / (chars as f64 * (t - b) / t);
            ((t1 - rate * chars as f64 * b / t).max(0.0), rate)
        }
        _ => (
            m.startup_s,
            (actual_s - m.startup_s).max(0.0) / chars as f64,
        ),
    };
    let a = if m.n > 0 && !rebuild { ALPHA } else { 1.0 };
    let round = |x: f64, d: i32| (x * 10f64.powi(d)).round() / 10f64.powi(d);
    stats[kind] = json!({
        "startup_s": round((1.0 - a) * m.startup_s + a * startup_s, 2),
        "s_per_char": round((1.0 - a) * m.s_per_char + a * rate, 4),
        "n": if rebuild { 1 } else { m.n + 1 },
        "outliers": 0,
    });
    if let (Some(voice), true) = (voice, m.n > 0) {
        let ratio = (actual_s / base).clamp(0.3, 3.0);
        let prev = stats["voices"][voice].as_f64();
        if !stats["voices"].is_object() {
            stats["voices"] = json!({});
        }
        let factor = match prev {
            Some(_) => (1.0 - ALPHA) * voice_factor(&stats, Some(voice)) + ALPHA * ratio,
            None => ratio,
        };
        stats["voices"][voice] = json!(round(factor, 3));
    }
    store.set_kv(KV_KEY, &stats)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learns() {
        let s = Store::in_memory().unwrap();
        let first = estimate(&s, "read:metal", 100, None);
        assert_eq!(first.samples, 0);
        let steps = [Progress {
            completed: 1,
            total: 4,
            elapsed_s: 20.0,
        }];
        assert!(record(&s, "read:metal", 400, &steps, 50.0, Some("voices/x")).unwrap());
        let e = estimate(&s, "read:metal", 400, None);
        assert_eq!(e.samples, 1);
        assert!((e.seconds - 50.0).abs() < 1.0, "{}", e.seconds);
    }
}
