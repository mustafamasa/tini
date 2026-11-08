//! Export: WAV (with the synthetic tag), MP3, SRT/VTT (per line or per word), words.json.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::audio::{self, Tag};
use crate::Error;

fn timestamp(t: f64, separator: char) -> String {
    let ms = (t.max(0.0) * 1000.0).round() as u64;
    let (h, m, s, ms) = (ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000);
    format!("{h:02}:{m:02}:{s:02}{separator}{ms:03}")
}

/// SRT or VTT from (start, end, text) cues.
pub fn subtitles(cues: &[(f64, f64, String)], vtt: bool) -> String {
    let mut out = String::new();
    if vtt {
        out.push_str("WEBVTT\n\n");
    }
    for (i, (a, b, text)) in cues.iter().enumerate() {
        let sep = if vtt { '.' } else { ',' };
        if !vtt {
            out.push_str(&format!("{}\n", i + 1));
        }
        out.push_str(&format!(
            "{} --> {}\n{}\n\n",
            timestamp(*a, sep),
            timestamp(*b, sep),
            text
        ));
    }
    out
}

/// Subtitle cues from a reading summary: one per word if `per_word`, otherwise one per line.
pub fn cues(result: &Value, per_word: bool) -> Vec<(f64, f64, String)> {
    let (key, text_key) = if per_word {
        ("words", "word")
    } else {
        ("lines", "text")
    };
    result[key]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|w| {
                    (
                        w["t0"].as_f64().unwrap_or(0.0),
                        w["t1"].as_f64().unwrap_or(0.0),
                        w[text_key].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// File name template: {ses} {tarih} {saat} {baslik} (user-facing tokens); sanitized for the file system.
pub fn file_name(template: &str, voice: &str, title: &str) -> String {
    let now = chrono::Local::now();
    let raw = template
        .replace("{ses}", voice)
        .replace("{baslik}", title)
        .replace("{tarih}", &now.format("%Y-%m-%d").to_string())
        .replace("{saat}", &now.format("%H%M").to_string());
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_. ".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_");
    let cleaned: String = cleaned.chars().take(120).collect();
    if cleaned.is_empty() {
        "tini".into()
    } else {
        cleaned
    }
}

/// Formats: wav, mp3, srt, vtt, word_srt, word_vtt, json, txt. `target` is the full file path.
pub fn export(result: &Value, format: &str, target: &Path, title: &str) -> Result<PathBuf, Error> {
    if let Some(p) = target.parent() {
        fs::create_dir_all(p)?;
    }
    let wav = || {
        result["wav"]
            .as_str()
            .map(PathBuf::from)
            .ok_or_else(|| Error::Input("bu üretimde ses yok".into()))
    };
    match format {
        "txt" => {
            let text = result["text"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| {
                    cues(result, false)
                        .into_iter()
                        .map(|(_, _, m)| m)
                        .collect::<Vec<_>>()
                        .join("\n")
                });
            fs::write(target, text + "\n")?;
        }
        "wav" => {
            let decoded = audio::load(&wav()?)?;
            audio::write_wav(target, &decoded, Some(&Tag::synthetic(title)))?;
        }
        "mp3" => {
            let decoded = audio::load(&wav()?)?;
            audio::write_mp3(target, &decoded, title)?;
        }
        "srt" | "vtt" | "word_srt" | "word_vtt" => {
            let per_word = format.starts_with("word");
            let c = cues(result, per_word);
            if c.is_empty() {
                return Err(Error::Input(if per_word {
                    "bu okumada kelime zamanı yok".into()
                } else {
                    "satır bilgisi yok".into()
                }));
            }
            fs::write(target, subtitles(&c, format.ends_with("vtt")))?;
        }
        "json" => {
            fs::write(target, serde_json::to_string_pretty(&result["words"])?)?;
        }
        other => return Err(Error::Input(format!("bilinmeyen biçim: {other}"))),
    }
    Ok(target.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_vtt() {
        let cues = vec![
            (0.0, 1.5, "Merhaba.".to_string()),
            (3661.25, 3662.0, "İkinci.".to_string()),
        ];
        let srt = subtitles(&cues, false);
        assert!(srt.starts_with("1\n00:00:00,000 --> 00:00:01,500\nMerhaba."));
        assert!(srt.contains("01:01:01,250"));
        assert!(subtitles(&cues, true).starts_with("WEBVTT\n\n00:00:00.000"));
    }

    #[test]
    fn name_template() {
        let n = file_name("{ses}_{baslik}", "Zeynep Belgesel", "Giriş: bölüm/1");
        assert_eq!(n, "Zeynep_Belgesel_Giriş__bölüm_1");
    }
}
