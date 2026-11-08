//! Transcription (speech to text) with whisper.cpp: audio/video → text, segments and word timings.
//! Used by the Transcribe page (SRT/VTT), to auto-fill the recording transcript when cloning, and for reading review
//! (every line is transcribed back and compared with its target text).
//!
//! Model: ggml-large-v3-turbo-q5_0 (whisper.cpp, MIT; OpenAI Whisper weights MIT). Downloaded on first use.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::audio::{self, Audio};
use crate::Error;

pub const MODEL_FILE: &str = "ggml-large-v3-turbo-q5_0.bin";
pub const MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-large-v3-turbo-q5_0.bin";
pub const MODEL_SHA256: &str = "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2";
pub const MODEL_SIZE: u64 = 574_041_195;

pub fn model_path(data_dir: &Path) -> PathBuf {
    data_dir.join("whisper").join(MODEL_FILE)
}

pub fn model_ready(data_dir: &Path) -> bool {
    fs::metadata(model_path(data_dir))
        .map(|m| m.len() == MODEL_SIZE)
        .unwrap_or(false)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub line: usize,
    pub text: String,
    pub t0: f64,
    pub t1: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeardWord {
    pub word: String,
    pub t0: f64,
    pub t1: f64,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Transcript {
    pub language: String,
    pub text: String,
    pub lines: Vec<Segment>,
    pub words: Vec<HeardWord>,
    pub audio_s: f64,
}

/// Windowed-sinc resampling (16 kHz for ASR input).
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let cutoff = (1.0f64).min(to as f64 / from as f64) * 0.95; // normalized cutoff frequency
    let half = 24i64;
    let n = (x.len() as f64 / ratio) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 * ratio;
        let center = t.floor() as i64;
        let mut sum = 0.0f64;
        let mut weight = 0.0f64;
        for k in (center - half + 1)..=(center + half) {
            if k < 0 || k as usize >= x.len() {
                continue;
            }
            let d = t - k as f64;
            let sinc = if d.abs() < 1e-9 {
                1.0
            } else {
                (std::f64::consts::PI * d * cutoff).sin() / (std::f64::consts::PI * d * cutoff)
            };
            let window = 0.5 + 0.5 * (std::f64::consts::PI * d / half as f64).cos();
            let w = sinc * window * cutoff;
            sum += x[k as usize] as f64 * w;
            weight += w;
        }
        out.push(if weight.abs() > 1e-9 {
            (sum / weight * cutoff.max(1e-9) / cutoff) as f32
        } else {
            0.0
        });
    }
    out
}

fn ms(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0) / 1000.0
}

/// Turns whisper-cli's full JSON output into segments and words.
pub fn parse_json(json: &Value) -> (String, Vec<Segment>, Vec<HeardWord>) {
    let language = json["result"]["language"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let mut segments = Vec::new();
    let mut words: Vec<HeardWord> = Vec::new();
    for (i, seg) in json["transcription"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let text = seg["text"].as_str().unwrap_or("").trim().to_string();
        if text.is_empty() {
            continue;
        }
        let line = i + 1;
        segments.push(Segment {
            line,
            text,
            t0: ms(&seg["offsets"]["from"]),
            t1: ms(&seg["offsets"]["to"]),
        });
        let mut current: Option<HeardWord> = None;
        for tok in seg["tokens"].as_array().cloned().unwrap_or_default() {
            let t = tok["text"].as_str().unwrap_or("");
            if t.starts_with("[_") || t.starts_with("<|") || t.is_empty() {
                continue;
            }
            let (t0, t1) = (ms(&tok["offsets"]["from"]), ms(&tok["offsets"]["to"]));
            if t.starts_with(' ') || current.is_none() {
                if let Some(w) = current.take() {
                    words.push(w);
                }
                current = Some(HeardWord {
                    word: t.trim().to_string(),
                    t0,
                    t1,
                    line,
                });
            } else if let Some(w) = current.as_mut() {
                w.word.push_str(t);
                w.t1 = t1;
            }
        }
        if let Some(w) = current.take() {
            words.push(w);
        }
    }
    words.retain(|w| !w.word.trim().is_empty());
    (language, segments, words)
}

/// Runs whisper-cli. `wav16`: 16 kHz mono WAV. Progress is reported as 0-100.
pub fn run(
    whisper: &Path,
    model: &Path,
    wav16: &Path,
    language: Option<&str>,
    threads: u32,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u32),
) -> Result<Value, Error> {
    let output = wav16.with_extension("");
    let mut cmd = Command::new(whisper);
    cmd.args([
        "-m",
        &model.to_string_lossy(),
        "-f",
        &wav16.to_string_lossy(),
        "-l",
        language.unwrap_or("auto"),
    ])
    .args([
        "-t",
        &threads.to_string(),
        "-oj",
        "-ojf",
        "-of",
        &output.to_string_lossy(),
        "-pp",
    ])
    .stdout(Stdio::null())
    .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| {
        Error::Engine(format!(
            "whisper başlatılamadı ({}): {e}",
            whisper.display()
        ))
    })?;
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    let mut tail: Vec<String> = Vec::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Cancelled);
        }
        while let Ok(l) = rx.try_recv() {
            if let Some(p) = l.split("progress =").nth(1) {
                if let Ok(v) = p.trim().trim_end_matches('%').trim().parse::<u32>() {
                    progress(v.min(100));
                }
            }
            tail.push(l);
            if tail.len() > 40 {
                tail.remove(0);
            }
        }
        if let Some(status) = child.try_wait()? {
            while let Ok(l) = rx.recv_timeout(Duration::from_millis(50)) {
                tail.push(l);
            }
            if !status.success() {
                return Err(Error::Engine(format!(
                    "whisper hata ile bitti ({status}):\n{}",
                    tail.join("\n")
                )));
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    let json_path = output.with_extension("json");
    let text = fs::read_to_string(&json_path).map_err(|e| {
        Error::Engine(format!(
            "whisper çıktısı yok ({}): {e}",
            json_path.display()
        ))
    })?;
    Ok(serde_json::from_str(&text)?)
}

/// Converts an audio/video file to 16 kHz mono WAV; returns its duration.
pub fn prepare_input(input: &Path, target: &Path) -> Result<f64, Error> {
    let a = audio::load(input)?;
    let x = resample(&a.samples, a.sample_rate, 16_000);
    let seconds = x.len() as f64 / 16_000.0;
    audio::write_wav(
        target,
        &Audio {
            samples: x,
            sample_rate: 16_000,
        },
        None,
    )?;
    Ok(seconds)
}

/// End to end: file → transcript. `work_dir`: folder for intermediate files (the 16 kHz WAV stays there for playback).
#[allow(clippy::too_many_arguments)]
pub fn transcribe(
    whisper: &Path,
    model: &Path,
    input: &Path,
    work_dir: &Path,
    language: Option<&str>,
    threads: u32,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u32),
) -> Result<Transcript, Error> {
    fs::create_dir_all(work_dir)?;
    let wav = work_dir.join("audio_16k.wav");
    let audio_s = prepare_input(input, &wav)?;
    let json = run(whisper, model, &wav, language, threads, cancel, progress)?;
    let (detected, lines, words) = parse_json(&json);
    let text = lines
        .iter()
        .map(|p| p.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Transcript {
        language: if detected.is_empty() {
            language.unwrap_or("").to_string()
        } else {
            detected
        },
        text,
        lines,
        words,
        audio_s,
    })
}

// --- review: word error rate -----------------------------------------------------------------------------------------

/// Normalization for comparison: Turkish lowercase, circumflex folding, no punctuation, digits spelled (Turkish).
pub fn normalize(text: &str, language: &str) -> Vec<String> {
    let mut t = text.to_string();
    if language == "tr" {
        t = crate::text_fix::fix_text(
            &t,
            "tr",
            &crate::text_fix::FixOptions {
                sentences: false,
                ..Default::default()
            },
        )
        .text;
        t = crate::text::turkish_lower(&t);
    } else {
        t = t.to_lowercase();
    }
    let fold = |c: char| match c {
        'â' => 'a',
        'î' => 'i',
        'û' => 'u',
        _ => c,
    };
    t.chars()
        .map(fold)
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(String::from)
        .collect()
}

/// Word-level Levenshtein distance.
pub fn word_distance(a: &[String], b: &[String]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, y) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(x != y))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Word error rate between the target text and the heard text (0 = identical).
pub fn wer(target: &str, heard: &str, language: &str) -> f64 {
    let a = normalize(target, language);
    let b = normalize(heard, language);
    if a.is_empty() {
        return if b.is_empty() { 0.0 } else { 1.0 };
    }
    word_distance(&a, &b) as f64 / a.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wer_normalization() {
        assert_eq!(wer("Merhaba, dünya!", "merhaba dünya", "tr"), 0.0);
        assert_eq!(
            wer("Yıl bin dokuz yüz doksan dört.", "Yıl 1994.", "tr"),
            0.0
        );
        assert_eq!(wer("hâlâ burada", "hala burada", "tr"), 0.0);
        assert!((wer("bir iki üç dört", "bir iki dört", "tr") - 0.25).abs() < 1e-9);
    }

    #[test]
    fn json_segments() {
        let j: Value = serde_json::from_str(
            r#"{"result":{"language":"tr"},"transcription":[{"offsets":{"from":0,"to":2000},"text":" Merhaba dünya.",
            "tokens":[{"text":"[_BEG_]","offsets":{"from":0,"to":0}},{"text":" Mer","offsets":{"from":0,"to":300}},
            {"text":"haba","offsets":{"from":300,"to":700}},{"text":" dünya.","offsets":{"from":800,"to":1500}}]}]}"#,
        )
        .unwrap();
        let (language, s, w) = parse_json(&j);
        assert_eq!(language, "tr");
        assert_eq!(s[0].text, "Merhaba dünya.");
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].word, "Merhaba");
        assert_eq!((w[0].t0, w[0].t1), (0.0, 0.7));
    }

    #[test]
    fn resampling() {
        let x: Vec<f32> = (0..48000)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
            .collect();
        let y = resample(&x, 48000, 16000);
        assert_eq!(y.len(), 16000);
        let rms = (y.iter().map(|v| v * v).sum::<f32>() / y.len() as f32).sqrt();
        assert!((rms - 0.707).abs() < 0.05, "{rms}");
    }
}
