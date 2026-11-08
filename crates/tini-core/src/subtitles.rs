//! Timed narration from subtitles: parses SRT/VTT cues, reads each cue with the chosen voice and places it at the
//! cue's start time. Audio that does not fit its slot is sped up without changing pitch (WSOLA, at most
//! `max_speedup`×); if it still does not fit, later cues are shifted and reported. Optionally muxed onto a video
//! with ffmpeg.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Cue {
    pub t0: f64,
    pub t1: f64,
    pub text: String,
    #[serde(default)]
    pub speaker: Option<String>,
}

fn parse_time(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    let (h, m, sec) = match parts.len() {
        3 => (
            parts[0].parse::<f64>().ok()?,
            parts[1].parse::<f64>().ok()?,
            parts[2].parse::<f64>().ok()?,
        ),
        2 => (
            0.0,
            parts[0].parse::<f64>().ok()?,
            parts[1].parse::<f64>().ok()?,
        ),
        _ => return None,
    };
    Some(h * 3600.0 + m * 60.0 + sec)
}

/// Strips `<...>` and `{...}` tags; a WebVTT `<v Name>` tag yields the speaker.
fn strip_tags(text: &str) -> (String, Option<String>) {
    let mut speaker = None;
    let mut out = String::new();
    let mut i = 0;
    let c: Vec<char> = text.chars().collect();
    while i < c.len() {
        if c[i] == '<' {
            if let Some(j) = c[i..].iter().position(|x| *x == '>') {
                let tag: String = c[i + 1..i + j].iter().collect();
                if let Some(name) = tag.strip_prefix("v ").or_else(|| tag.strip_prefix("v.")) {
                    let name = name.split_once(' ').map(|(_, a)| a).unwrap_or(name);
                    speaker = Some(name.trim().to_string());
                }
                i += j + 1;
                continue;
            }
        }
        if c[i] == '{' {
            if let Some(j) = c[i..].iter().position(|x| *x == '}') {
                i += j + 1;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    (
        out.split_whitespace().collect::<Vec<_>>().join(" "),
        speaker,
    )
}

/// Parses SRT or WebVTT content into cues.
pub fn parse(content: &str) -> Vec<Cue> {
    let text = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = Vec::new();
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().filter(|l| !l.trim().is_empty()).collect();
        let Some(ti) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let (start, end) = lines[ti].split_once("-->").unwrap();
        let end = end.split_whitespace().next().unwrap_or("");
        let (Some(t0), Some(t1)) = (parse_time(start), parse_time(end)) else {
            continue;
        };
        let raw = lines[ti + 1..].join(" ");
        let (clean, speaker) = strip_tags(&raw);
        if clean.is_empty() {
            continue;
        }
        out.push(Cue {
            t0,
            t1: t1.max(t0),
            text: clean,
            speaker,
        });
    }
    out.sort_by(|a, b| a.t0.partial_cmp(&b.t0).unwrap());
    out
}

/// Pitch-preserving time stretch (WSOLA). `ratio` > 1 speeds up (shortens).
pub fn time_stretch(x: &[f32], sample_rate: u32, ratio: f64) -> Vec<f32> {
    if (ratio - 1.0).abs() < 0.005 || x.len() < 4096 {
        return x.to_vec();
    }
    let n = (sample_rate as f64 * 0.04) as usize; // 40 ms frame
    let hop_out = n / 2;
    let hop_in = hop_out as f64 * ratio;
    let tolerance = (sample_rate as f64 * 0.01) as i64; // ±10 ms search
    let window: Vec<f32> = (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos())
        .collect();
    let target_len = (x.len() as f64 / ratio) as usize;
    let mut out = vec![0f32; target_len + n];
    let mut norm = vec![0f32; target_len + n];
    let overlap = n - hop_out;
    let mut prev: usize = 0;
    let mut k = 0usize;
    loop {
        let out_pos = k * hop_out;
        if out_pos >= target_len {
            break;
        }
        let ideal = (k as f64 * hop_in).round() as i64;
        let pos = if k == 0 {
            0
        } else {
            let natural = prev + hop_out;
            let mut best = ideal;
            let mut best_score = f32::NEG_INFINITY;
            let mut d = -tolerance;
            while d <= tolerance {
                let cand = ideal + d;
                if cand >= 0 && (cand as usize) + n <= x.len() && natural + overlap <= x.len() {
                    let a = cand as usize;
                    let mut sum = 0f32;
                    let mut j = 0;
                    while j < overlap {
                        sum += x[natural + j] * x[a + j];
                        j += 4;
                    }
                    if sum > best_score {
                        best_score = sum;
                        best = cand;
                    }
                }
                d += 2;
            }
            best.max(0) as usize
        };
        if pos + n > x.len() {
            break;
        }
        for i in 0..n {
            out[out_pos + i] += x[pos + i] * window[i];
            norm[out_pos + i] += window[i];
        }
        prev = pos;
        k += 1;
    }
    for (o, w) in out.iter_mut().zip(&norm) {
        if *w > 1e-3 {
            *o /= w;
        }
    }
    out.truncate(target_len);
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct Placement {
    pub target_t0: f64,
    pub t0: f64,
    pub t1: f64,
    /// Applied speed-up (1 = none).
    pub speed: f64,
    /// How late the cue started relative to its target time (s).
    pub drift: f64,
}

/// Places clips at their cue times. Returns the mixed audio, per-cue placement and the stretched clips.
pub fn place(
    clips: &[Vec<f32>],
    cues: &[Cue],
    sample_rate: u32,
    max_speedup: f64,
) -> (Vec<f32>, Vec<Placement>, Vec<Vec<f32>>) {
    let sr = sample_rate as f64;
    let mut cursor = 0.0f64;
    let mut placements = Vec::new();
    let mut stretched = Vec::new();
    for (i, (s, cue)) in clips.iter().zip(cues).enumerate() {
        let start = cue.t0.max(cursor);
        let slot_end = cues.get(i + 1).map(|n| n.t0).unwrap_or(f64::INFINITY);
        let available = slot_end - start - 0.05;
        let len = s.len() as f64 / sr;
        let mut ratio = 1.0;
        let mut clip = s.clone();
        if len > available && available > 0.2 {
            ratio = (len / available).min(max_speedup);
            if ratio > 1.01 {
                clip = time_stretch(s, sample_rate, ratio);
                ratio = s.len() as f64 / clip.len().max(1) as f64;
            } else {
                ratio = 1.0;
            }
        }
        let end = start + clip.len() as f64 / sr;
        placements.push(Placement {
            target_t0: cue.t0,
            t0: (start * 1000.0).round() / 1000.0,
            t1: (end * 1000.0).round() / 1000.0,
            speed: (ratio * 100.0).round() / 100.0,
            drift: ((start - cue.t0) * 1000.0).round() / 1000.0,
        });
        cursor = end + 0.05;
        stretched.push(clip);
    }
    let total = placements
        .iter()
        .map(|y| y.t1)
        .chain(cues.iter().map(|i| i.t1))
        .fold(0.0, f64::max);
    let mut out = vec![0f32; (total * sr) as usize + 1];
    for (p, s) in placements.iter().zip(&stretched) {
        let a = (p.t0 * sr) as usize;
        for (k, v) in s.iter().enumerate() {
            if let Some(o) = out.get_mut(a + k) {
                *o += v;
            }
        }
    }
    (out, placements, stretched)
}

/// System ffmpeg (only needed for muxing onto video).
pub fn find_ffmpeg() -> Option<PathBuf> {
    let candidates = [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
    ];
    for c in candidates {
        if Path::new(c).is_file() {
            return Some(PathBuf::from(c));
        }
    }
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join("ffmpeg"))
            .find(|p| p.is_file())
    })
}

/// Muxes the narration onto a video. `original_audio`: "duck" (original audio kept at 15%) or "remove".
pub fn mux_video(
    ffmpeg: &Path,
    video: &Path,
    audio: &Path,
    output: &Path,
    original_audio: &str,
) -> Result<(), Error> {
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-y", "-v", "error", "-i"])
        .arg(video)
        .arg("-i")
        .arg(audio);
    if original_audio == "duck" {
        cmd.args([
            "-filter_complex",
            "[0:a]volume=0.15[a0];[a0][1:a]amix=inputs=2:duration=longest:normalize=0[a]",
            "-map",
            "0:v:0",
            "-map",
            "[a]",
        ]);
    } else {
        cmd.args(["-map", "0:v:0", "-map", "1:a:0"]);
    }
    cmd.args([
        "-c:v",
        "copy",
        "-c:a",
        "aac",
        "-b:a",
        "192k",
        "-metadata",
        "comment=AI-generated voice (Tını, VoxCPM2)",
    ])
    .arg(output);
    let out = cmd
        .output()
        .map_err(|e| Error::Engine(format!("ffmpeg çalıştırılamadı: {e}")))?;
    if !out.status.success() {
        // mixing fails when the video has no audio track: retry with the new audio only
        if original_audio == "duck" {
            return mux_video(ffmpeg, video, audio, output, "remove");
        }
        return Err(Error::Engine(format!(
            "ffmpeg hata: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_and_vtt() {
        let srt = "1\n00:00:01,000 --> 00:00:02,500\n<i>Merhaba</i> dünya.\n\n2\n00:00:03,000 --> 00:00:04,000\nİkinci\nsatır.\n";
        let a = parse(srt);
        assert_eq!(a.len(), 2);
        assert_eq!(
            a[0],
            Cue {
                t0: 1.0,
                t1: 2.5,
                text: "Merhaba dünya.".into(),
                speaker: None
            }
        );
        assert_eq!(a[1].text, "İkinci satır.");
        let vtt = "WEBVTT\n\ncue-1\n00:01.000 --> 00:02.000 align:start\n<v Ayşe>Selam.</v>\n";
        let b = parse(vtt);
        assert_eq!(b[0].speaker.as_deref(), Some("Ayşe"));
        assert_eq!(b[0].text, "Selam.");
        assert_eq!(b[0].t0, 1.0);
    }

    #[test]
    fn stretch_length_and_pitch() {
        let hz = 48000;
        let x: Vec<f32> = (0..hz * 2)
            .map(|i| (2.0 * std::f32::consts::PI * 300.0 * i as f32 / hz as f32).sin() * 0.5)
            .collect();
        let y = time_stretch(&x, hz, 1.25);
        assert!((y.len() as f64 - x.len() as f64 / 1.25).abs() < 10.0);
        // pitch preserved: zero-crossing rate per second unchanged
        let crossings = |s: &[f32]| {
            s.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f64
                / (s.len() as f64 / hz as f64)
        };
        assert!((crossings(&y[4800..y.len() - 4800]) - 300.0).abs() < 8.0);
    }

    #[test]
    fn placement_speeds_up_and_shifts() {
        let hz = 8000;
        let cues = vec![
            Cue {
                t0: 0.0,
                t1: 1.0,
                text: "a".into(),
                speaker: None,
            },
            Cue {
                t0: 1.0,
                t1: 2.0,
                text: "b".into(),
                speaker: None,
            },
        ];
        // first clip is 3 s: sped up 1.25× but still does not fit → second shifts
        let tone = |n: usize| {
            (0..n)
                .map(|i| (i as f32 * 0.2).sin() * 0.3)
                .collect::<Vec<f32>>()
        };
        let (out, p, _) = place(&[tone(24000), tone(4000)], &cues, hz, 1.25);
        assert!(p[0].speed > 1.2);
        assert!(p[1].drift > 1.0);
        assert!(out.len() as f64 / hz as f64 >= p[1].t1 - 0.01);
    }
}
