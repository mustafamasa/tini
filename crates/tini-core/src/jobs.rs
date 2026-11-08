//! Jobs: voice design, reference reading, consented cloning, line regeneration, transcription, review and timed
//! narration.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::audio::{self, Audio, Tag, SAMPLE_RATE};
use crate::engine::{self, EngineConfig, Progress};
use crate::settings::Paths;
use crate::store::{Job, Store};
use crate::subtitles::Cue;
use crate::text::{self, Dictionary, Issue};
use crate::voices;
use crate::word_timing::{self, Word};
use crate::Error;

pub const DESIGN_LIMIT_SECONDS: f64 = 30.0;
pub const DESIGN_HARD_LIMIT_SECONDS: f64 = 33.0;
pub const DEFAULT_VARIANTS: usize = 3;
pub const MAX_VARIANTS: usize = 50;
pub const CLONE_MIN_S: f64 = 3.0;
pub const CLONE_MAX_S: f64 = 36.0;
pub const CLONE_RECOMMENDED_S: (f64, f64) = (5.0, 30.0);
pub const CLONE_MAX_BYTES: u64 = 25_000_000;
pub const CLONE_SEED: u32 = 42;
pub const CONSENT_TEXT: &str =
    "Bu ses bana ait ya da sahibinden açık izin aldım; taklit, dolandırıcılık ya da yanıltma için kullanmayacağım.";
/// Error shown when the consent checkbox was not ticked (short; the consent text is shown once next to the checkbox).
pub const CONSENT_MISSING: &str = "Kaydı kullanma izni onaylanmadı";

/// A job request entering the queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobRequest {
    Design {
        name: String,
        description: String,
        text: String,
        #[serde(default)]
        language: Option<String>,
        #[serde(default = "default_variants")]
        variants: usize,
        #[serde(default)]
        seeds: Option<Vec<u32>>,
        #[serde(default)]
        preview_text: Option<String>,
    },
    Read {
        voice: String,
        text: String,
        #[serde(default)]
        language: Option<String>,
        #[serde(default)]
        title: Option<String>,
    },
    Clone {
        recording: String,
        recording_text: String,
        text: String,
        #[serde(default)]
        seed: Option<u32>,
        #[serde(default)]
        consent: bool,
        /// name to keep the recording in the library as a consented clone voice (not kept when empty)
        #[serde(default)]
        library_name: Option<String>,
    },
    Line {
        job_id: String,
        line: usize,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        seed: Option<u32>,
    },
    /// Audio/video file to text (whisper.cpp).
    Transcribe {
        file: String,
        #[serde(default)]
        language: Option<String>,
    },
    /// Review of a reading: transcribed back and compared line by line with the target text.
    Review { job_id: String },
    /// Timed narration from subtitles.
    Timed {
        voice: String,
        cues: Vec<Cue>,
        /// speaker → voice id (VTT `<v Name>`)
        #[serde(default)]
        speaker_voices: HashMap<String, String>,
        #[serde(default)]
        language: Option<String>,
        #[serde(default)]
        max_speedup: Option<f64>,
        #[serde(default)]
        video: Option<String>,
        /// duck | remove
        #[serde(default)]
        original_audio: Option<String>,
        #[serde(default)]
        title: Option<String>,
    },
}

fn default_variants() -> usize {
    DEFAULT_VARIANTS
}

/// Summary extracted before a request enters the queue.
#[derive(Debug, Clone, Default)]
pub struct RequestSummary {
    pub title: String,
    pub voice_id: Option<String>,
    pub voice_name: Option<String>,
    pub language: Option<String>,
    pub text: String,
    pub total: usize,
    pub chars: usize,
    pub estimate_kind: String,
}

/// What a finished job records.
pub struct JobOutput {
    pub result: Value,
    pub folder: Option<PathBuf>,
    pub audio_s: Option<f64>,
    pub duration_s: Option<f64>,
    pub gpu_peak_mb: Option<u64>,
    pub voice_id: Option<String>,
    /// progress reports, for learning duration estimates
    pub steps: Vec<Progress>,
}

pub struct Context<'a> {
    pub paths: &'a Paths,
    pub engine: EngineConfig,
    pub dictionary: Dictionary,
    pub cancel: &'a AtomicBool,
}

fn issues<T>(found: Vec<Issue>) -> Result<T, Error> {
    Err(Error::Issues(found))
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_auto(language: &str) -> bool {
    language.is_empty() || language == "auto"
}

fn now_minutes() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

/// Checks the design form.
pub fn check_design(description: &str, design_text: &str, language: &str) -> Vec<Issue> {
    let mut found = Vec::new();
    if description.trim().is_empty() {
        found.push(Issue::new("description_empty", "tarif boş").with("field", "description"));
    }
    if description.contains('(') || description.contains(')') {
        found.push(
            Issue::new(
                "description_parenthesis",
                "tarifte parantez olmaz (motor parantezi tarif sınırı sayar)",
            )
            .with("field", "description"),
        );
    }
    found.extend(text::problems(
        design_text,
        "text",
        "tasarım metni",
        language,
    ));
    let seconds = text::estimate_seconds(design_text, Some(language));
    if seconds > DESIGN_HARD_LIMIT_SECONDS {
        found.push(
            Issue::new(
                "design_too_long",
                format!(
                    "tasarım metni örnek için çok uzun (~{seconds:.0} sn > {DESIGN_LIMIT_SECONDS:.0} sn); 1-3 cümle yeter, uzun metni okuma adımında okut"
                ),
            )
            .with("seconds", seconds.round())
            .with("limit", DESIGN_LIMIT_SECONDS),
        );
    }
    found
}

/// Reading language: explicit if given; otherwise detected from the text; when undecided the voice card's, else "tr".
pub fn reading_language(
    card: &Value,
    language: Option<&str>,
    text_in: &str,
) -> Result<String, Error> {
    if let Some(l) = language.filter(|l| !is_auto(l)) {
        return text::language(Some(l)).map_err(|s| Error::Issues(vec![s]));
    }
    Ok(text::detect(text_in, card["language"].as_str()))
}

/// Validates a request and extracts its summary (before any GPU work).
pub fn summarize_request(
    paths: &Paths,
    request: &JobRequest,
    store: &Store,
) -> Result<RequestSummary, Error> {
    match request {
        JobRequest::Design {
            name,
            description,
            text: design_text,
            language,
            variants,
            seeds,
            preview_text,
        } => {
            let language = match language.as_deref().filter(|l| !is_auto(l)) {
                Some(l) => text::language(Some(l)).map_err(|s| Error::Issues(vec![s]))?,
                None => text::detect(design_text, None),
            };
            let mut found = check_design(description, design_text, &language);
            if name.trim().is_empty() {
                found.push(Issue::new("name_empty", "üretim adı boş").with("field", "name"));
            }
            let count = seeds.as_ref().map(|t| t.len()).unwrap_or(*variants);
            if !(1..=MAX_VARIANTS).contains(&count) {
                found.push(
                    Issue::new("variants", "çeşit sayısı 1-50 olmalı").with("field", "variants"),
                );
            }
            let preview = preview_text.as_deref().unwrap_or("").trim().to_string();
            let mut preview_lines = 0;
            if !preview.is_empty() {
                let preview_language = text::detect(&preview, Some(&language));
                found.extend(text::problems(
                    &preview,
                    "preview_text",
                    "duymak istediğin metin",
                    &preview_language,
                ));
                preview_lines = text::lines(&preview).map(|l| l.len()).unwrap_or(0);
            }
            if !found.is_empty() {
                return issues(found);
            }
            Ok(RequestSummary {
                title: name.trim().into(),
                language: Some(language),
                text: description.trim().into(),
                total: count * (1 + preview_lines),
                chars: crate::estimate::chars(design_text, count)
                    + crate::estimate::chars(&preview, count),
                estimate_kind: "design".into(),
                ..Default::default()
            })
        }
        JobRequest::Read {
            voice,
            text: body,
            language,
            title,
        } => {
            let folder = voices::find(paths, voice)?;
            let card = voices::read_card(&folder)?;
            let language = reading_language(&card, language.as_deref(), body)?;
            let found = text::problems(body, "text", "metin", &language);
            if !found.is_empty() {
                return issues(found);
            }
            let vc = voices::get_card(paths, store, voice)?;
            let voice_name = vc.alias.clone().unwrap_or(vc.display_name.clone());
            let first = body
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim();
            let first = first.split("||").next().unwrap_or("").trim();
            let auto_title: String = first.chars().take(60).collect();
            Ok(RequestSummary {
                title: title
                    .clone()
                    .filter(|b| !b.trim().is_empty())
                    .unwrap_or(auto_title),
                voice_id: Some(voice.clone()),
                voice_name: Some(voice_name),
                language: Some(language),
                text: body.clone(),
                total: text::lines(body).map(|l| l.len()).unwrap_or(1),
                chars: crate::estimate::chars(body, 1),
                estimate_kind: "read".into(),
            })
        }
        JobRequest::Clone {
            recording,
            recording_text,
            text: body,
            consent,
            ..
        } => {
            let mut found = Vec::new();
            if !consent {
                found.push(Issue::new("consent_missing", CONSENT_MISSING).with("field", "consent"));
            }
            if recording_text.trim().chars().count() < 10 {
                found.push(
                    Issue::new(
                        "recording_text",
                        "kaydın birebir metni zorunlu: kayıtta ne söyleniyorsa aynısını yazın",
                    )
                    .with("field", "recording_text"),
                );
            }
            match fs::metadata(Path::new(recording)) {
                Ok(m) if m.len() > CLONE_MAX_BYTES => found.push(
                    Issue::new(
                        "recording_too_large",
                        "ses dosyası 25 MB'tan büyük; 36 sn'lik bir kayıt yeter",
                    )
                    .with("field", "recording"),
                ),
                Ok(_) => {}
                Err(_) => found.push(
                    Issue::new("recording_missing", "ses kaydı yok ya da okunamadı")
                        .with("field", "recording"),
                ),
            }
            let language = text::detect(body, None);
            found.extend(text::problems(body, "text", "okunacak metin", &language));
            if !found.is_empty() {
                return issues(found);
            }
            Ok(RequestSummary {
                title: "Klon okuma".into(),
                language: Some(language),
                text: body.clone(),
                total: text::lines(body).map(|l| l.len()).unwrap_or(1),
                chars: crate::estimate::chars(body, 1),
                estimate_kind: "read".into(),
                ..Default::default()
            })
        }
        JobRequest::Line {
            job_id,
            line,
            text: new_text,
            ..
        } => {
            let parent = store
                .job(job_id)?
                .ok_or_else(|| Error::NotFound(format!("üretim yok: {job_id}")))?;
            if parent.kind != "read" || parent.status != "done" {
                return Err(Error::Input(
                    "yalnız bitmiş okumanın satırı yeniden üretilir".into(),
                ));
            }
            let lines = parent
                .result
                .as_ref()
                .and_then(|s| s["lines"].as_array().cloned())
                .unwrap_or_default();
            let record = lines
                .get(line.wrapping_sub(1))
                .ok_or_else(|| Error::Input(format!("satır yok: {line}")))?;
            let body = new_text
                .clone()
                .unwrap_or_else(|| record["text"].as_str().unwrap_or("").to_string());
            let language = parent.language.clone().unwrap_or_else(|| "tr".into());
            let found = text::problems(&body, "text", "satır", &language);
            if !found.is_empty() {
                return issues(found);
            }
            if body.contains('\n') {
                return Err(Error::Input("satır tek cümle olmalı".into()));
            }
            Ok(RequestSummary {
                title: format!("{} · satır {line}", parent.title),
                voice_id: parent.voice_id.clone(),
                voice_name: parent.voice_name.clone(),
                language: Some(language),
                text: body.clone(),
                total: 1,
                chars: crate::estimate::chars(&body, 1),
                estimate_kind: "read".into(),
            })
        }
        JobRequest::Transcribe { file, language } => {
            let path = Path::new(file);
            if !path.is_file() {
                return issues(vec![
                    Issue::new("file_missing", "dosya bulunamadı").with("field", "file")
                ]);
            }
            whisper_ready(paths)?;
            Ok(RequestSummary {
                title: path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                language: language.clone().filter(|l| !is_auto(l)),
                text: file.clone(),
                total: 100,
                chars: (fs::metadata(path).map(|m| m.len()).unwrap_or(0) / 4000) as usize,
                estimate_kind: "transcribe".into(),
                ..Default::default()
            })
        }
        JobRequest::Review { job_id } => {
            whisper_ready(paths)?;
            let parent = store
                .job(job_id)?
                .ok_or_else(|| Error::NotFound(format!("üretim yok: {job_id}")))?;
            if parent.status != "done"
                || !matches!(parent.kind.as_str(), "read" | "clone" | "timed")
            {
                return Err(Error::Input("yalnız bitmiş okumalar denetlenir".into()));
            }
            Ok(RequestSummary {
                title: format!("{} · kalite kontrolü", parent.title),
                voice_id: parent.voice_id.clone(),
                voice_name: parent.voice_name.clone(),
                language: parent.language.clone(),
                text: String::new(),
                total: 100,
                chars: (parent.audio_s.unwrap_or(10.0) * 15.0) as usize,
                estimate_kind: "transcribe".into(),
            })
        }
        JobRequest::Timed {
            voice,
            cues,
            speaker_voices,
            language,
            title,
            video,
            ..
        } => {
            if cues.is_empty() {
                return issues(vec![
                    Issue::new("empty", "altyazıda ipucu yok").with("field", "cues")
                ]);
            }
            let vc = voices::get_card(paths, store, voice)?;
            for v in speaker_voices.values() {
                voices::find(paths, v)?;
            }
            let joined = cues
                .iter()
                .map(|c| c.text.clone())
                .collect::<Vec<_>>()
                .join("\n");
            let language = match language.as_deref().filter(|l| !is_auto(l)) {
                Some(l) => text::language(Some(l)).map_err(|s| Error::Issues(vec![s]))?,
                None => text::detect(&joined, Some(&vc.language)),
            };
            let mut found = Vec::new();
            for (i, cue) in cues.iter().enumerate() {
                for s in text::problems(
                    &cue.text.replace("||", " "),
                    "cues",
                    &format!("{}. ipucu", i + 1),
                    &language,
                ) {
                    found.push(s.with("line", i + 1));
                }
            }
            if let Some(v) = video.as_ref().filter(|v| !v.is_empty()) {
                if !Path::new(v).is_file() {
                    found.push(
                        Issue::new("video_missing", "video dosyası bulunamadı")
                            .with("field", "video"),
                    );
                }
            }
            if !found.is_empty() {
                return issues(found);
            }
            Ok(RequestSummary {
                title: title
                    .clone()
                    .filter(|b| !b.trim().is_empty())
                    .unwrap_or_else(|| "Zamanlı seslendirme".into()),
                voice_id: Some(voice.clone()),
                voice_name: Some(vc.alias.clone().unwrap_or(vc.display_name.clone())),
                language: Some(language),
                text: joined.clone(),
                total: cues.len(),
                chars: crate::estimate::chars(&joined, 1),
                estimate_kind: "read".into(),
            })
        }
    }
}

fn whisper_ready(paths: &Paths) -> Result<(), Error> {
    if !paths.whisper.as_ref().map(|w| w.is_file()).unwrap_or(false) {
        return Err(Error::Config(
            "transkript motoru (whisper) uygulama paketinde yok".into(),
        ));
    }
    if !crate::transcribe::model_ready(&paths.data_dir) {
        return Err(Error::Config(
            "transkript modeli indirilmedi; Transkript sayfasından indirin".into(),
        ));
    }
    Ok(())
}

fn resolve_seeds(count: usize, seeds: Option<&Vec<u32>>) -> Vec<u32> {
    if let Some(s) = seeds.filter(|s| !s.is_empty()) {
        return s.clone();
    }
    let mut rng = rand::thread_rng();
    let mut chosen = std::collections::BTreeSet::new();
    while chosen.len() < count.max(1) {
        chosen.insert(rng.gen::<u32>());
    }
    chosen.into_iter().collect()
}

fn remove_empty(folder: &Path) {
    let _ = fs::remove_dir(folder);
}

/// Voice design plus (optionally) a preview reading with each variant's final voice.
#[allow(clippy::too_many_arguments)]
pub fn design(
    ctx: &Context,
    job_id: &str,
    name: &str,
    description: &str,
    design_text: &str,
    language: &str,
    variant_count: usize,
    seeds: Option<&Vec<u32>>,
    preview_text: Option<&str>,
    progress: &mut dyn FnMut(Progress),
) -> Result<JobOutput, Error> {
    let spoken_text = text::lines(&text::spoken(design_text, language, &ctx.dictionary))
        .map_err(|s| Error::Issues(vec![s]))?
        .into_iter()
        .map(|(l, _)| l)
        .collect::<Vec<_>>()
        .join(" ");
    let slug = text::slug(name);
    // a design study lives outside the library; variants are added to the library one by one by the user
    let study_dir = ctx.paths.designs().join(job_id);
    fs::create_dir_all(&study_dir)?;
    let seeds = resolve_seeds(variant_count, seeds);
    let requests: Vec<engine::Request> = seeds
        .iter()
        .map(|s| engine::design_request(&format!("t_{s}"), description, &spoken_text, *s))
        .collect();
    let preview_text = preview_text.map(str::trim).filter(|d| !d.is_empty());
    let preview_lines = match preview_text {
        Some(d) => {
            let preview_language = text::detect(d, Some(language));
            text::lines(&text::spoken(d, &preview_language, &ctx.dictionary))
                .map_err(|s| Error::Issues(vec![s]))?
        }
        None => vec![],
    };
    let total = requests.len() * (1 + preview_lines.len());
    let mut steps = Vec::new();
    let work = ctx.paths.work().join(format!("design_{job_id}"));
    let result = engine::run_sequence(&ctx.engine, &requests, &work, ctx.cancel, &mut |p| {
        let p = Progress {
            completed: p.completed,
            total,
            elapsed_s: p.elapsed_s,
        };
        steps.push(p);
        progress(p);
    })?;
    remove_empty(&work);
    let created_at = now_minutes();
    let mut variants: Vec<Value> = Vec::new();
    let mut number = 0;
    let mut good: Vec<(PathBuf, Value)> = Vec::new();
    for request in &requests {
        if result.failed.contains(&request.id) {
            let reason = result.reasons.get(&request.id).cloned().unwrap_or_default();
            variants.push(json!({"seed": request.seed, "error": format!("üretilemedi: {reason}; başka tohumla yeniden dene")}));
            continue;
        }
        number += 1;
        let clip = &result.clips[&request.id];
        let variant_dir = study_dir.join(voices::variant_dir(number));
        fs::create_dir_all(&variant_dir)?;
        audio::write_wav(
            &variant_dir.join("reference.wav"),
            &engine::to_audio(clip.clone()),
            Some(&Tag::synthetic(name)),
        )?;
        let seconds = (clip.len() as f64 / SAMPLE_RATE as f64 * 100.0).round() / 100.0;
        let mut card = json!({
            "name": slug, "display_name": name.trim(), "variant": number, "language": language,
            "description": description.trim(),
            "design_text": spoken_text,
            "design_seed": request.seed, "read_seed": request.seed,
            "reference": "reference.wav", "reference_s": seconds,
            "model": engine::MODEL_NAME, "engine": "voxcpm", "precision": ctx.engine.precision,
            "steps": ctx.engine.steps, "cfg": ctx.engine.cfg, "synthetic": true, "source": "design",
            "created_at": created_at, "job_id": job_id,
        });
        if seconds < engine::MIN_REFERENCE_SECONDS {
            card["warning"] = json!(format!(
                "referans {seconds} sn < {} sn; tasarım metnini uzat",
                engine::MIN_REFERENCE_SECONDS
            ));
        }
        // seed luck: some variants come out with a hiss/hum bed that every reading would inherit
        let margin = audio::noise_margin_db(clip);
        card["noise_margin_db"] = json!(margin);
        voices::write_card(&variant_dir, &card)?;
        let mut v = card.clone();
        v["noisy"] = json!(margin.map(|m| m < audio::NOISY_MARGIN_DB).unwrap_or(false));
        v["seed"] = json!(request.seed);
        v["library_voice_id"] = Value::Null;
        v["folder"] = json!(variant_dir.to_string_lossy());
        v["reference_path"] = json!(variant_dir.join("reference.wav").to_string_lossy());
        variants.push(v);
        good.push((variant_dir, card));
    }
    let mut seconds = result.seconds;
    let mut gpu = result.gpu_peak_mb;
    if !preview_lines.is_empty() && !good.is_empty() {
        let base = result.seconds;
        let done_design = requests.len();
        let total = done_design + good.len() * preview_lines.len();
        let mut reqs = Vec::new();
        let reading_work = ctx.paths.work().join(format!("design_{job_id}_reading"));
        for (variant_dir, card) in &good {
            let reference = variant_dir.join("reference.wav");
            let n = card["variant"].as_i64().unwrap_or(0);
            for (i, (line, _)) in preview_lines.iter().enumerate() {
                reqs.push(engine::read_request(
                    &format!("v{n}_s{}", i + 1),
                    line,
                    card["read_seed"].as_u64().unwrap_or(42) as u32,
                    &reference,
                    card["design_text"].as_str().unwrap_or(""),
                ));
            }
        }
        let read = engine::run_sequence(&ctx.engine, &reqs, &reading_work, ctx.cancel, &mut |p| {
            let p = Progress {
                completed: done_design + p.completed,
                total,
                elapsed_s: base + p.elapsed_s,
            };
            steps.push(p);
            progress(p);
        });
        remove_empty(&reading_work);
        let preview_joined = preview_lines
            .iter()
            .map(|(l, _)| l.clone())
            .collect::<Vec<_>>()
            .join("\n");
        match read {
            Ok(read) => {
                seconds = ((seconds + read.seconds) * 10.0).round() / 10.0;
                gpu = gpu.max(read.gpu_peak_mb);
                for (variant_dir, card) in &good {
                    let n = card["variant"].as_i64().unwrap_or(0);
                    let ids: Vec<String> = (1..=preview_lines.len())
                        .map(|i| format!("v{n}_s{i}"))
                        .collect();
                    let fields = if ids.iter().any(|i| !read.clips.contains_key(i)) {
                        json!({"preview_error": "okuma yarım kaldı (bozuk üretim); Bu sesle oku ile yeniden dene"})
                    } else {
                        let mut parts: Vec<Vec<f32>> =
                            ids.iter().map(|i| read.clips[i].clone()).collect();
                        if ctx.engine.tone_match {
                            tone_match(&variant_dir.join("reference.wav"), &mut parts);
                        }
                        let pauses: Vec<f64> = preview_lines.iter().map(|(_, p)| *p).collect();
                        let joined = audio::join_lines(&parts, &pauses, SAMPLE_RATE);
                        audio::write_wav(
                            &variant_dir.join("preview.wav"),
                            &engine::to_audio(joined.clone()),
                            Some(&Tag::synthetic(name)),
                        )?;
                        json!({"preview": "preview.wav", "preview_text": preview_joined,
                               "preview_s": (joined.len() as f64 / SAMPLE_RATE as f64 * 100.0).round() / 100.0,
                               "preview_input": preview_text})
                    };
                    voices::update_card_fields(variant_dir, &fields)?;
                    if let Some(v) = variants
                        .iter_mut()
                        .find(|v| v["variant"].as_i64() == Some(n))
                    {
                        for (k, val) in fields.as_object().unwrap() {
                            v[k] = val.clone();
                        }
                        if fields.get("preview").is_some() {
                            v["preview_path"] =
                                json!(variant_dir.join("preview.wav").to_string_lossy());
                        }
                    }
                }
            }
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => {
                for (variant_dir, card) in &good {
                    let fields = json!({"preview_error": format!("okuma motoru durdu: {e}")});
                    voices::update_card_fields(variant_dir, &fields)?;
                    let n = card["variant"].as_i64();
                    if let Some(v) = variants.iter_mut().find(|v| v["variant"].as_i64() == n) {
                        v["preview_error"] = fields["preview_error"].clone();
                    }
                }
            }
        }
    }
    if good.is_empty() {
        let _ = fs::remove_dir(&study_dir);
    }
    Ok(JobOutput {
        result: json!({"name": name.trim(), "display_name": name.trim(), "slug": slug,
                       "folder": study_dir.to_string_lossy(),
                       "language": language, "description": description.trim(), "design_text": spoken_text,
                       "variants": variants, "duration_s": seconds, "gpu_peak_mb": gpu}),
        folder: Some(study_dir),
        audio_s: None,
        duration_s: Some(seconds),
        gpu_peak_mb: gpu,
        voice_id: None,
        steps,
    })
}

/// One line of a reading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineRecord {
    pub line: usize,
    /// what the user wrote (dictionary not applied)
    pub text: String,
    /// what the engine received (dictionary applied)
    pub spoken: String,
    pub pause: f64,
    pub seed: u32,
    pub wav: String,
    pub t0: f64,
    pub t1: f64,
    #[serde(default)]
    pub words: Vec<Word>,
    #[serde(default)]
    pub speed_warning: bool,
    #[serde(default)]
    pub version: u32,
    /// heard text and word error rate from a review
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heard: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wer: Option<f64>,
}

/// Per-sentence speed gate: lines whose letters/s rate is below 0.55× or above 1.6× the median.
pub fn speed_outliers(lines: &[(usize, String, f64)]) -> Vec<usize> {
    if lines.len() < 3 {
        return vec![];
    }
    let rates: Vec<f64> = lines
        .iter()
        .map(|(_, t, d)| t.chars().count() as f64 / d.max(0.01))
        .collect();
    let mut sorted = rates.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = sorted[sorted.len() / 2];
    lines
        .iter()
        .zip(rates)
        .filter(|(_, r)| *r < 0.55 * median || *r > 1.6 * median)
        .map(|((n, _, _), _)| *n)
        .collect()
}

struct LinesOutput {
    clips: Vec<Vec<f32>>,
    words: Option<Vec<Vec<Word>>>,
    pauses: Vec<f64>,
    word_timing_error: Option<String>,
    seconds: f64,
    gpu: Option<u64>,
}

/// Reads lines with a reference; with word timing on, lines are trimmed (silence between lines = pause).
#[allow(clippy::too_many_arguments)]
fn read_lines(
    ctx: &Context,
    config: &EngineConfig,
    reference: &Path,
    reference_text: &str,
    lines: &[(String, f64)],
    seeds: &[u32],
    language: &str,
    work: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<LinesOutput, Error> {
    let requests: Vec<engine::Request> = lines
        .iter()
        .enumerate()
        .map(|(i, (line, _))| {
            engine::read_request(
                &format!("s{}", i + 1),
                line,
                seeds[i],
                reference,
                reference_text,
            )
        })
        .collect();
    let result = engine::run_sequence(config, &requests, work, ctx.cancel, progress)?;
    if !result.failed.is_empty() {
        let details: Vec<String> = result
            .failed
            .iter()
            .map(|h| {
                format!(
                    "{} ({})",
                    h.trim_start_matches('s'),
                    result.reasons.get(h).map(String::as_str).unwrap_or("?")
                )
            })
            .collect();
        return Err(Error::Engine(format!(
            "şu satırlar üretilemedi: {}; cümleyi kısalt ya da yeniden dene",
            details.join(", ")
        )));
    }
    let mut clips: Vec<Vec<f32>> = requests
        .iter()
        .map(|r| result.clips[&r.id].clone())
        .collect();
    let mut pauses: Vec<f64> = lines.iter().map(|(_, p)| *p).collect();
    let mut words = None;
    let mut word_timing_error = None;
    if config.word_timing {
        let spokens: Vec<String> = lines.iter().map(|(l, _)| l.clone()).collect();
        let heard = match heard_words(ctx, &clips, language, work) {
            Ok(d) => d,
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => {
                word_timing_error = Some(format!(
                    "whisper hizalaması yapılamadı, kelime zamanı sesten tahmin edildi: {e}"
                ));
                None
            }
        };
        if heard.is_none() && word_timing_error.is_none() {
            word_timing_error = Some(
                "transkript modeli yok; kelime zamanı sesten tahmin edildi (daha kaba)".into(),
            );
        }
        let timed =
            word_timing::reading_words(&spokens, &clips, heard.as_deref(), SAMPLE_RATE, language);
        let mut ws = Vec::new();
        for (i, (line_words, (a, b))) in timed.into_iter().enumerate() {
            clips[i] = word_timing::trim(&clips[i], SAMPLE_RATE, a, b);
            let r3 = |x: f64| (x * 1000.0).round() / 1000.0;
            ws.push(
                line_words
                    .into_iter()
                    .map(|w| Word {
                        word: w.word,
                        t0: r3((w.t0 - a).max(0.0)),
                        t1: r3(w.t1 - a),
                    })
                    .collect(),
            );
        }
        pauses = pauses
            .iter()
            .map(|p| ((p - 2.0 * word_timing::TRIM_PAD).max(0.0) * 1000.0).round() / 1000.0)
            .collect();
        words = Some(ws);
    }
    Ok(LinesOutput {
        clips,
        words,
        pauses,
        word_timing_error,
        seconds: result.seconds,
        gpu: result.gpu_peak_mb,
    })
}

/// Transcribes all lines together with whisper; returns heard words per line (in the line's own time). `None` when
/// there is no transcription model.
fn heard_words(
    ctx: &Context,
    clips: &[Vec<f32>],
    language: &str,
    work: &Path,
) -> Result<Option<Vec<Vec<word_timing::HeardWord>>>, Error> {
    if whisper_ready(ctx.paths).is_err() {
        return Ok(None);
    }
    const GAP: f64 = 0.5;
    let mut joined: Vec<f32> = Vec::new();
    let mut spans = Vec::new();
    for a in clips {
        let start = joined.len() as f64 / SAMPLE_RATE as f64;
        joined.extend_from_slice(a);
        spans.push((start, joined.len() as f64 / SAMPLE_RATE as f64));
        joined.extend(std::iter::repeat_n(
            0.0f32,
            (GAP * SAMPLE_RATE as f64) as usize,
        ));
    }
    let dir = work.join("_word_timing");
    fs::create_dir_all(&dir)?;
    let wav = dir.join("reading_16k.wav");
    let x = crate::transcribe::resample(&joined, SAMPLE_RATE, 16_000);
    audio::write_wav(
        &wav,
        &Audio {
            samples: x,
            sample_rate: 16_000,
        },
        None,
    )?;
    let r = crate::transcribe::run(
        ctx.paths.whisper.as_ref().unwrap(),
        &crate::transcribe::model_path(&ctx.paths.data_dir),
        &wav,
        Some(language),
        whisper_threads(),
        ctx.cancel,
        &mut |_| {},
    );
    let _ = fs::remove_dir_all(&dir);
    let (_, _, words) = crate::transcribe::parse_json(&r?);
    let mut out: Vec<Vec<word_timing::HeardWord>> = vec![Vec::new(); clips.len()];
    for w in words {
        let mid = (w.t0 + w.t1) / 2.0;
        if let Some(i) = spans
            .iter()
            .position(|(a, z)| mid >= a - 0.05 && mid <= z + 0.05)
        {
            let (a, z) = spans[i];
            out[i].push(word_timing::HeardWord {
                word: w.word,
                t0: (w.t0 - a).clamp(0.0, z - a),
                t1: (w.t1 - a).clamp(0.0, z - a),
            });
        }
    }
    Ok(Some(out))
}

/// Applies tone match to a reading: compares the voice's reference spectrum with the reading's and cuts excess bands.
fn tone_match(reference: &Path, clips: &mut [Vec<f32>]) -> Option<crate::tone::ToneProfile> {
    let r = audio::load(reference).ok()?;
    let rp = crate::tone::band_profile(&r.samples, r.sample_rate)?;
    let joined: Vec<f32> = clips.iter().flatten().copied().collect();
    let op = crate::tone::band_profile(&joined, SAMPLE_RATE)?;
    let profile = crate::tone::derive_profile(&rp, &op);
    if profile.is_neutral() {
        return Some(profile);
    }
    for c in clips.iter_mut() {
        *c = crate::tone::apply(c, SAMPLE_RATE, &profile);
    }
    Some(profile)
}

/// Writes reading.wav, words.json and summary.json from the line records; returns the summary.
fn write_reading(
    out_dir: &Path,
    records: &mut [LineRecord],
    title: &str,
    extra: Value,
) -> Result<(Value, f64), Error> {
    let mut parts = Vec::new();
    for r in records.iter() {
        parts.push(audio::read_wav(&out_dir.join(&r.wav))?.samples);
    }
    let pauses: Vec<f64> = records.iter().map(|r| r.pause).collect();
    let joined = audio::join_lines(&parts, &pauses, SAMPLE_RATE);
    audio::write_wav(
        &out_dir.join("reading.wav"),
        &engine::to_audio(joined.clone()),
        Some(&Tag::synthetic(title)),
    )?;
    let mut cursor = 0usize;
    let mut all_words = Vec::new();
    for (i, r) in records.iter_mut().enumerate() {
        let start = cursor as f64 / SAMPLE_RATE as f64;
        cursor += parts[i].len();
        r.t0 = (start * 1000.0).round() / 1000.0;
        r.t1 = (cursor as f64 / SAMPLE_RATE as f64 * 1000.0).round() / 1000.0;
        for w in &r.words {
            all_words.push(json!({"line": r.line, "word": w.word,
                "t0": ((w.t0 + start) * 1000.0).round() / 1000.0, "t1": ((w.t1 + start) * 1000.0).round() / 1000.0}));
        }
        cursor += (r.pause * SAMPLE_RATE as f64) as usize;
    }
    let outliers = speed_outliers(
        &records
            .iter()
            .map(|r| (r.line, r.spoken.clone(), r.t1 - r.t0))
            .collect::<Vec<_>>(),
    );
    for r in records.iter_mut() {
        r.speed_warning = outliers.contains(&r.line);
    }
    let audio_s = (joined.len() as f64 / SAMPLE_RATE as f64 * 100.0).round() / 100.0;
    if !all_words.is_empty() {
        fs::write(
            out_dir.join("words.json"),
            serde_json::to_string(&all_words)?,
        )?;
    }
    let mut summary = json!({
        "wav": out_dir.join("reading.wav").to_string_lossy(),
        "folder": out_dir.to_string_lossy(),
        "audio_s": audio_s,
        "lines": records,
        "words": all_words,
        "suspect_lines": outliers,
        "synthetic": true,
        "date": now_minutes(),
    });
    if let (Some(o), Some(e)) = (summary.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            o.insert(k.clone(), v.clone());
        }
    }
    let mut on_disk = summary.clone();
    on_disk.as_object_mut().unwrap().remove("words");
    fs::write(
        out_dir.join("summary.json"),
        serde_json::to_string_pretty(&on_disk)?,
    )?;
    Ok((summary, audio_s))
}

fn reference_file(voice_dir: &Path, card: &Value) -> PathBuf {
    voice_dir.join(card["reference"].as_str().unwrap_or("reference.wav"))
}

/// Reference reading: a voice folder + text → a reading under `out_dir`.
#[allow(clippy::too_many_arguments)]
pub fn read_into(
    ctx: &Context,
    config: &EngineConfig,
    voice_dir: &Path,
    body: &str,
    language: Option<&str>,
    out_dir: &Path,
    title: &str,
    progress: &mut dyn FnMut(Progress),
) -> Result<JobOutput, Error> {
    let card = voices::read_card(voice_dir)?;
    let language = reading_language(&card, language, body)?;
    let found = text::problems(body, "text", "metin", &language);
    if !found.is_empty() {
        return issues(found);
    }
    let seed = card["read_seed"].as_u64().unwrap_or(42) as u32;
    let original = text::lines(body).map_err(|s| Error::Issues(vec![s]))?;
    let parsed = text::lines(&text::spoken(body, &language, &ctx.dictionary))
        .map_err(|s| Error::Issues(vec![s]))?;
    let out_dir = out_dir.to_path_buf();
    fs::create_dir_all(out_dir.join("lines"))?;
    let reference = engine::reference_wav(
        &reference_file(voice_dir, &card),
        &out_dir.join("_reference"),
    )?;
    let reference_text = card["design_text"].as_str().unwrap_or("").to_string();
    let seeds = vec![seed; parsed.len()];
    let mut steps = Vec::new();
    let mut output = read_lines(
        ctx,
        config,
        &reference,
        &reference_text,
        &parsed,
        &seeds,
        &language,
        &out_dir,
        &mut |p| {
            steps.push(p);
            progress(p);
        },
    )?;
    if !config.keep_intermediate {
        let _ = fs::remove_dir_all(out_dir.join("_reference"));
    }
    let tone = if config.tone_match {
        tone_match(&reference_file(voice_dir, &card), &mut output.clips)
    } else {
        None
    };
    let mut records = Vec::new();
    for (i, clip) in output.clips.iter().enumerate() {
        let wav = format!("lines/{:03}.wav", i + 1);
        audio::write_wav(&out_dir.join(&wav), &engine::to_audio(clip.clone()), None)?;
        records.push(LineRecord {
            line: i + 1,
            text: original.get(i).map(|(l, _)| l.clone()).unwrap_or_default(),
            spoken: parsed[i].0.clone(),
            pause: output.pauses[i],
            seed,
            wav,
            t0: 0.0,
            t1: 0.0,
            words: output
                .words
                .as_ref()
                .map(|w| w[i].clone())
                .unwrap_or_default(),
            speed_warning: false,
            version: 1,
            heard: None,
            wer: None,
        });
    }
    let extra = json!({
        "voice": card["name"], "display_name": card["display_name"], "language": language, "seed": seed,
        "voice_folder": voice_dir.to_string_lossy(), "duration_s": output.seconds, "gpu_peak_mb": output.gpu,
        "word_timing_error": output.word_timing_error, "title": title,
        "model": engine::MODEL_NAME, "engine": "voxcpm", "steps": config.steps, "cfg": config.cfg,
        "precision": config.precision,
        "raw_pauses": parsed.iter().map(|(_, p)| *p).collect::<Vec<_>>(),
        "trimmed": output.words.is_some(),
        "tone_match": tone,
    });
    let (summary, audio_s) = write_reading(&out_dir, &mut records, title, extra)?;
    Ok(JobOutput {
        result: summary,
        folder: Some(out_dir),
        audio_s: Some(audio_s),
        duration_s: Some(output.seconds),
        gpu_peak_mb: output.gpu,
        voice_id: None,
        steps,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn read(
    ctx: &Context,
    job_id: &str,
    voice_id: &str,
    body: &str,
    language: Option<&str>,
    title: &str,
    progress: &mut dyn FnMut(Progress),
) -> Result<JobOutput, Error> {
    let folder = voices::find(ctx.paths, voice_id)?;
    let out_dir = ctx.paths.jobs().join(job_id);
    let mut r = read_into(
        ctx,
        &ctx.engine,
        &folder,
        body,
        language,
        &out_dir,
        title,
        progress,
    )?;
    r.result["voice_id"] = json!(voice_id);
    let summary_path = out_dir.join("summary.json");
    if let Ok(t) = fs::read_to_string(&summary_path) {
        if let Ok(mut v) = serde_json::from_str::<Value>(&t) {
            v["voice_id"] = json!(voice_id);
            let _ = fs::write(summary_path, serde_json::to_string_pretty(&v)?);
        }
    }
    Ok(r)
}

/// Prepares a clone recording: mono, trimmed above 36 s, rejected below 3 s.
pub fn prepare_clone_reference(
    recording: &Path,
    target: &Path,
) -> Result<(f64, Vec<String>), Error> {
    let decoded = audio::load(recording)?;
    let seconds = decoded.seconds();
    let mut notes = Vec::new();
    if seconds < CLONE_MIN_S {
        return Err(Error::Issues(vec![Issue::new(
            "recording_too_short",
            format!(
                "kayıt çok kısa ({seconds:.1} sn); en az {CLONE_MIN_S:.0} sn, önerilen 5-30 sn"
            ),
        )
        .with("field", "recording")]));
    }
    let mut samples = decoded.samples;
    if seconds > CLONE_MAX_S {
        samples.truncate((CLONE_MAX_S * decoded.sample_rate as f64) as usize);
        notes.push(format!(
            "kayıt {seconds:.1} sn idi, ilk {CLONE_MAX_S:.0} sn kullanıldı; kayıt metni yalnız bu kısmı içermeli"
        ));
    } else if seconds < CLONE_RECOMMENDED_S.0 || seconds > CLONE_RECOMMENDED_S.1 {
        notes.push(format!(
            "kayıt {seconds:.1} sn; en iyi sonuç 5-30 sn temiz kayıtla"
        ));
    }
    let a = Audio {
        samples,
        sample_rate: decoded.sample_rate,
    };
    audio::write_wav(target, &a, None)?;
    Ok(((a.seconds() * 100.0).round() / 100.0, notes))
}

/// Consented cloning. The recording, its transcript and the encoding cache are processed in a temp folder that is
/// removed when the job ends (even on error); only the synthetic output stays. With `library_name` the recording is
/// kept in the library as a voice, together with the consent record.
#[allow(clippy::too_many_arguments)]
pub fn clone(
    ctx: &Context,
    job_id: &str,
    recording: &Path,
    recording_text: &str,
    body: &str,
    seed: Option<u32>,
    consent: bool,
    library_name: Option<&str>,
    progress: &mut dyn FnMut(Progress),
) -> Result<JobOutput, Error> {
    if !consent {
        return Err(Error::Issues(vec![Issue::new(
            "consent_missing",
            CONSENT_MISSING,
        )]));
    }
    let seed = seed.unwrap_or(CLONE_SEED);
    let temp = ctx.paths.work().join(format!("clone_{job_id}"));
    let outcome = (|| -> Result<JobOutput, Error> {
        let voice = temp.join("voice");
        fs::create_dir_all(&voice)?;
        let (reference_s, mut notes) =
            prepare_clone_reference(recording, &voice.join("reference.wav"))?;
        let margin = audio::read_wav(&voice.join("reference.wav"))
            .ok()
            .and_then(|a| audio::noise_margin_db(&a.samples));
        if let Some(m) = margin.filter(|m| *m < audio::NOISY_MARGIN_DB) {
            notes.push(format!(
                "kayıtta arka plan gürültüsü var (zemin konuşmanın yalnız {m:.0} dB altında); klon bu gürültüyü okumaya taşır, daha sessiz bir ortamda kaydedin"
            ));
        }
        let card = json!({"name": "clone", "reference": "reference.wav", "read_seed": seed,
            "design_text": collapse(recording_text), "synthetic": true});
        voices::write_card(&voice, &card)?;
        let mut local = ctx.engine.clone();
        local.cache_dir = temp.join("cache");
        fs::create_dir_all(&local.cache_dir)?;
        let out_dir = ctx.paths.jobs().join(job_id);
        let mut r = read_into(
            ctx,
            &local,
            &voice,
            body,
            None,
            &out_dir,
            "Klon okuma",
            progress,
        )?;
        let wav = out_dir.join("clone_synthetic.wav");
        fs::rename(out_dir.join("reading.wav"), &wav)?;
        r.result["wav"] = json!(wav.to_string_lossy());
        r.result["kind"] = json!("clone");
        r.result["reference_s"] = json!(reference_s);
        r.result["notes"] = json!(notes);
        r.result["consent"] = json!(CONSENT_TEXT);
        r.result["note"] = json!("AI-generated synthetic speech (VoxCPM2 voice cloning) / Yapay zekâ ile üretilmiş sestir");
        if let Some(name) = library_name.map(str::trim).filter(|a| !a.is_empty()) {
            let slug = text::slug(name);
            let dir = ctx.paths.voices().join(&slug);
            let mut n = 1;
            while dir.join(voices::variant_dir(n)).exists() {
                n += 1;
            }
            let target = dir.join(voices::variant_dir(n));
            fs::create_dir_all(&target)?;
            fs::copy(voice.join("reference.wav"), target.join("reference.wav"))?;
            let language = text::detect(recording_text, None);
            voices::write_card(
                &target,
                &json!({"name": slug, "display_name": name, "variant": n, "language": language, "source": "clone",
                    "design_text": collapse(recording_text), "read_seed": seed,
                    "reference": "reference.wav", "reference_s": reference_s, "synthetic": true,
                    "noise_margin_db": margin,
                    "created_at": now_minutes(), "job_id": job_id,
                    "consent": {"type": "confirmed", "text": CONSENT_TEXT, "date": crate::now_string(),
                                "version": 1}}),
            )?;
            r.result["library_voice_id"] = json!(format!("{slug}/{n}"));
            r.voice_id = Some(format!("{slug}/{n}"));
        }
        let mut on_disk: Value =
            serde_json::from_str(&fs::read_to_string(out_dir.join("summary.json"))?)?;
        for k in ["wav", "kind", "reference_s", "notes", "consent", "note"] {
            on_disk[k] = r.result[k].clone();
        }
        fs::write(
            out_dir.join("summary.json"),
            serde_json::to_string_pretty(&on_disk)?,
        )?;
        Ok(r)
    })();
    for _ in 0..5 {
        let _ = fs::remove_dir_all(&temp);
        if !temp.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
    outcome
}

/// Regenerates one line of a finished reading (new seed and/or corrected text) and re-joins the reading.
#[allow(clippy::too_many_arguments)]
pub fn regenerate_line(
    ctx: &Context,
    parent: &Job,
    line: usize,
    new_text: Option<&str>,
    seed: Option<u32>,
    progress: &mut dyn FnMut(Progress),
) -> Result<JobOutput, Error> {
    let result = parent
        .result
        .clone()
        .ok_or_else(|| Error::Input("üretimin sonucu yok".into()))?;
    let out_dir = PathBuf::from(
        result["folder"]
            .as_str()
            .ok_or_else(|| Error::Input("klasör yok".into()))?,
    );
    let mut records: Vec<LineRecord> = serde_json::from_value(result["lines"].clone())?;
    let index = line
        .checked_sub(1)
        .filter(|i| *i < records.len())
        .ok_or_else(|| Error::Input("satır yok".into()))?;
    // resolve by id first: built-in voices live inside the app bundle, whose path changes between installs
    let voice_dir = result["voice_id"]
        .as_str()
        .and_then(|id| voices::find(ctx.paths, id).ok())
        .unwrap_or_else(|| PathBuf::from(result["voice_folder"].as_str().unwrap_or("")));
    let card = voices::read_card(&voice_dir)
        .map_err(|_| Error::Input("okumanın sesi artık yok (silinmiş olabilir)".into()))?;
    let language = result["language"].as_str().unwrap_or("tr").to_string();
    let line_text = new_text
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(&records[index].text)
        .to_string();
    let (_, raw_pause) = text::split_pause(&line_text).map_err(|s| Error::Issues(vec![s]))?;
    let has_mark = line_text.contains("||");
    let text_only = text::split_pause(&line_text)
        .map(|(t, _)| t)
        .unwrap_or(line_text.clone());
    let spoken = text::spoken(&text_only, &language, &ctx.dictionary);
    let seed = seed.unwrap_or_else(|| rand::thread_rng().gen());
    let reference = engine::reference_wav(
        &reference_file(&voice_dir, &card),
        &out_dir.join("_reference"),
    )?;
    let reference_text = card["design_text"].as_str().unwrap_or("").to_string();
    let trimmed = result["trimmed"].as_bool().unwrap_or(false);
    let mut steps = Vec::new();
    let mut local = ctx.engine.clone();
    // keep the line's trimming consistent with the rest of the reading
    local.word_timing = trimmed;
    let output = read_lines(
        ctx,
        &local,
        &reference,
        &reference_text,
        &[(spoken.clone(), raw_pause)],
        &[seed],
        &language,
        &out_dir,
        &mut |p| {
            steps.push(p);
            progress(p);
        },
    )?;
    if !ctx.engine.keep_intermediate {
        let _ = fs::remove_dir_all(out_dir.join("_reference"));
    }
    let mut clip = output.clips[0].clone();
    if let Ok(profile) =
        serde_json::from_value::<crate::tone::ToneProfile>(result["tone_match"].clone())
    {
        clip = crate::tone::apply(&clip, SAMPLE_RATE, &profile);
    }
    let r = &mut records[index];
    let version = r.version + 1;
    let wav = format!("lines/{:03}_v{version}.wav", line);
    audio::write_wav(&out_dir.join(&wav), &engine::to_audio(clip), None)?;
    r.text = text_only;
    r.spoken = spoken;
    r.seed = seed;
    r.wav = wav;
    r.version = version;
    r.heard = None;
    r.wer = None;
    r.words = output.words.map(|w| w[0].clone()).unwrap_or_default();
    if has_mark {
        r.pause = if trimmed {
            (raw_pause - 2.0 * word_timing::TRIM_PAD).max(0.0)
        } else {
            raw_pause
        };
    }
    let title = result["title"]
        .as_str()
        .unwrap_or(&parent.title)
        .to_string();
    let mut extra = result.clone();
    for key in [
        "wav",
        "folder",
        "audio_s",
        "lines",
        "words",
        "suspect_lines",
        "date",
    ] {
        extra.as_object_mut().unwrap().remove(key);
    }
    let (summary, audio_s) = write_reading(&out_dir, &mut records, &title, extra)?;
    Ok(JobOutput {
        result: json!({"parent_id": parent.id, "line": line, "seed": seed, "parent_result": summary, "audio_s": audio_s}),
        folder: Some(out_dir),
        audio_s: Some(audio_s),
        duration_s: Some(output.seconds),
        gpu_peak_mb: output.gpu,
        voice_id: None,
        steps,
    })
}

fn whisper_threads() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4)
        .clamp(2, 8)
}

/// Audio/video file to text.
pub fn transcribe(
    ctx: &Context,
    job_id: &str,
    file: &Path,
    language: Option<&str>,
    progress: &mut dyn FnMut(Progress),
) -> Result<JobOutput, Error> {
    whisper_ready(ctx.paths)?;
    let out = ctx.paths.jobs().join(job_id);
    let start = std::time::Instant::now();
    let mut steps = Vec::new();
    let r = crate::transcribe::transcribe(
        ctx.paths.whisper.as_ref().unwrap(),
        &crate::transcribe::model_path(&ctx.paths.data_dir),
        file,
        &out,
        language,
        whisper_threads(),
        ctx.cancel,
        &mut |p| {
            let i = Progress {
                completed: p as usize,
                total: 100,
                elapsed_s: start.elapsed().as_secs_f64(),
            };
            steps.push(i);
            progress(i);
        },
    )?;
    let audio_s = (r.audio_s * 100.0).round() / 100.0;
    let result = json!({
        "kind": "transcribe",
        "wav": out.join("audio_16k.wav").to_string_lossy(),
        "folder": out.to_string_lossy(),
        "source_file": file.to_string_lossy(),
        "language": r.language,
        "text": r.text,
        "lines": r.lines,
        "words": r.words,
        "audio_s": audio_s,
    });
    fs::write(
        out.join("transcript.json"),
        serde_json::to_string_pretty(&result)?,
    )?;
    Ok(JobOutput {
        result,
        folder: Some(out),
        audio_s: Some(audio_s),
        duration_s: Some((start.elapsed().as_secs_f64() * 10.0).round() / 10.0),
        gpu_peak_mb: None,
        voice_id: None,
        steps,
    })
}

/// Review of a reading: the audio is transcribed back, the words are assigned to lines by their times and each line's
/// word error rate against its target text is computed.
pub fn review(
    ctx: &Context,
    parent: &Job,
    progress: &mut dyn FnMut(Progress),
) -> Result<JobOutput, Error> {
    whisper_ready(ctx.paths)?;
    let mut result = parent
        .result
        .clone()
        .ok_or_else(|| Error::Input("üretimin sonucu yok".into()))?;
    let wav = PathBuf::from(
        result["wav"]
            .as_str()
            .ok_or_else(|| Error::Input("ses yok".into()))?,
    );
    let language = result["language"]
        .as_str()
        .or(parent.language.as_deref())
        .unwrap_or("tr")
        .to_string();
    let work = ctx.paths.work().join(format!("review_{}", parent.id));
    let start = std::time::Instant::now();
    let mut steps = Vec::new();
    let r = crate::transcribe::transcribe(
        ctx.paths.whisper.as_ref().unwrap(),
        &crate::transcribe::model_path(&ctx.paths.data_dir),
        &wav,
        &work,
        Some(&language),
        whisper_threads(),
        ctx.cancel,
        &mut |p| {
            let i = Progress {
                completed: p as usize,
                total: 100,
                elapsed_s: start.elapsed().as_secs_f64(),
            };
            steps.push(i);
            progress(i);
        },
    );
    let _ = fs::remove_dir_all(&work);
    let r = r?;
    let lines = result["lines"]
        .as_array_mut()
        .ok_or_else(|| Error::Input("satır bilgisi yok".into()))?;
    let mut flagged = Vec::new();
    let mut wer_sum = 0.0;
    for l in lines.iter_mut() {
        let (t0, t1) = (
            l["t0"].as_f64().unwrap_or(0.0),
            l["t1"].as_f64().unwrap_or(0.0),
        );
        let heard = r
            .words
            .iter()
            .filter(|w| {
                let mid = (w.t0 + w.t1) / 2.0;
                mid >= t0 - 0.15 && mid <= t1 + 0.15
            })
            .map(|w| w.word.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let target = l["spoken"]
            .as_str()
            .or(l["text"].as_str())
            .unwrap_or("")
            .to_string();
        let w = crate::transcribe::wer(&target, &heard, &language);
        wer_sum += w;
        if w > 0.2 {
            flagged.push(l["line"].clone());
        }
        l["heard"] = json!(heard);
        l["wer"] = json!((w * 1000.0).round() / 1000.0);
    }
    let n = lines.len().max(1) as f64;
    result["review"] = json!({
        "date": now_minutes(),
        "mean_wer": ((wer_sum / n) * 1000.0).round() / 1000.0,
        "flagged": flagged,
        "model": crate::transcribe::MODEL_FILE,
    });
    if let Some(folder) = result["folder"].as_str() {
        let summary = Path::new(folder).join("summary.json");
        if let Ok(t) = fs::read_to_string(&summary) {
            if let Ok(mut v) = serde_json::from_str::<Value>(&t) {
                v["lines"] = result["lines"].clone();
                v["review"] = result["review"].clone();
                let _ = fs::write(&summary, serde_json::to_string_pretty(&v)?);
            }
        }
    }
    let audio_s = result["audio_s"].as_f64();
    Ok(JobOutput {
        result: json!({"parent_id": parent.id, "parent_result": result, "audio_s": audio_s, "review": result["review"]}),
        folder: None,
        audio_s: None,
        duration_s: Some((start.elapsed().as_secs_f64() * 10.0).round() / 10.0),
        gpu_peak_mb: None,
        voice_id: None,
        steps,
    })
}

/// Timed narration from subtitles.
#[allow(clippy::too_many_arguments)]
pub fn timed(
    ctx: &Context,
    job_id: &str,
    default_voice: &str,
    cues: &[Cue],
    speaker_voices: &HashMap<String, String>,
    language: &str,
    max_speedup: f64,
    video: Option<&Path>,
    original_audio: &str,
    title: &str,
    progress: &mut dyn FnMut(Progress),
) -> Result<JobOutput, Error> {
    let out = ctx.paths.jobs().join(job_id);
    fs::create_dir_all(&out)?;
    // group cues by voice; each voice is read in a single engine batch
    let voice_of = |cue: &Cue| -> String {
        cue.speaker
            .as_ref()
            .and_then(|k| speaker_voices.get(k))
            .cloned()
            .unwrap_or_else(|| default_voice.to_string())
    };
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, cue) in cues.iter().enumerate() {
        let v = voice_of(cue);
        match groups.iter_mut().find(|(s, _)| *s == v) {
            Some((_, idx)) => idx.push(i),
            None => groups.push((v, vec![i])),
        }
    }
    let total = cues.len();
    let mut completed = 0usize;
    let mut clips: Vec<Vec<f32>> = vec![Vec::new(); total];
    let mut words_per_cue: Vec<Vec<Word>> = vec![Vec::new(); total];
    let mut spoken_per_cue: Vec<String> = vec![String::new(); total];
    let mut seed_per_cue: Vec<u32> = vec![0; total];
    let mut seconds = 0.0;
    let mut gpu = None;
    let mut steps = Vec::new();
    for (voice_id, indices) in &groups {
        let folder = voices::find(ctx.paths, voice_id)?;
        let card = voices::read_card(&folder)?;
        let seed = card["read_seed"].as_u64().unwrap_or(42) as u32;
        let lines: Vec<(String, f64)> = indices
            .iter()
            .map(|i| {
                (
                    text::spoken(&cues[*i].text.replace("||", " "), language, &ctx.dictionary)
                        .replace('\n', " "),
                    0.0,
                )
            })
            .collect();
        let reference =
            engine::reference_wav(&reference_file(&folder, &card), &out.join("_reference"))?;
        let reference_text = card["design_text"].as_str().unwrap_or("").to_string();
        let seeds = vec![seed; lines.len()];
        let before = completed;
        let output = read_lines(
            ctx,
            &ctx.engine,
            &reference,
            &reference_text,
            &lines,
            &seeds,
            language,
            &out,
            &mut |p| {
                let i = Progress {
                    completed: before + p.completed,
                    total,
                    elapsed_s: seconds + p.elapsed_s,
                };
                steps.push(i);
                progress(i);
            },
        )?;
        let mut group_clips = output.clips;
        if ctx.engine.tone_match {
            tone_match(&reference_file(&folder, &card), &mut group_clips);
        }
        for (k, i) in indices.iter().enumerate() {
            clips[*i] = group_clips[k].clone();
            words_per_cue[*i] = output
                .words
                .as_ref()
                .map(|w| w[k].clone())
                .unwrap_or_default();
            spoken_per_cue[*i] = lines[k].0.clone();
            seed_per_cue[*i] = seed;
        }
        completed += indices.len();
        seconds += output.seconds;
        gpu = gpu.max(output.gpu);
    }
    if !ctx.engine.keep_intermediate {
        let _ = fs::remove_dir_all(out.join("_reference"));
    }
    let (mixed, placements, _) = crate::subtitles::place(&clips, cues, SAMPLE_RATE, max_speedup);
    let wav = out.join("timed.wav");
    audio::write_wav(
        &wav,
        &engine::to_audio(mixed.clone()),
        Some(&Tag::synthetic(title)),
    )?;
    let mut line_json = Vec::new();
    let mut all_words = Vec::new();
    for (i, (cue, p)) in cues.iter().zip(&placements).enumerate() {
        let words: Vec<Word> = words_per_cue[i]
            .iter()
            .map(|w| Word {
                word: w.word.clone(),
                t0: (w.t0 / p.speed * 1000.0).round() / 1000.0,
                t1: (w.t1 / p.speed * 1000.0).round() / 1000.0,
            })
            .collect();
        for w in &words {
            all_words.push(json!({"line": i + 1, "word": w.word, "t0": ((w.t0 + p.t0) * 1000.0).round() / 1000.0,
                "t1": ((w.t1 + p.t0) * 1000.0).round() / 1000.0}));
        }
        line_json.push(json!({
            "line": i + 1, "text": cue.text, "spoken": spoken_per_cue[i], "t0": p.t0, "t1": p.t1, "pause": 0.0,
            "seed": seed_per_cue[i], "wav": "", "words": words, "speed_warning": p.drift > 0.3, "version": 1,
            "target_t0": p.target_t0, "speed": p.speed, "drift": p.drift, "voice": voice_of(cue), "speaker": cue.speaker,
        }));
    }
    let srt: Vec<(f64, f64, String)> = placements
        .iter()
        .zip(cues)
        .map(|(p, c)| (p.t0, p.t1, c.text.clone()))
        .collect();
    fs::write(out.join("timed.srt"), crate::export::subtitles(&srt, false))?;
    let audio_s = (mixed.len() as f64 / SAMPLE_RATE as f64 * 100.0).round() / 100.0;
    let mut result = json!({
        "kind": "timed", "wav": wav.to_string_lossy(), "folder": out.to_string_lossy(), "audio_s": audio_s,
        "lines": line_json, "words": all_words, "language": language, "title": title,
        "sped_up_count": placements.iter().filter(|p| p.speed > 1.01).count(),
        "shifted_count": placements.iter().filter(|p| p.drift > 0.3).count(),
        "max_speedup": max_speedup, "duration_s": seconds, "gpu_peak_mb": gpu, "synthetic": true,
    });
    if let Some(v) = video {
        match crate::subtitles::find_ffmpeg() {
            Some(ff) => {
                let stem = v
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "video".into());
                let target = out.join(format!("{stem}_tini.mp4"));
                crate::subtitles::mux_video(&ff, v, &wav, &target, original_audio)?;
                result["video"] = json!(target.to_string_lossy());
            }
            None => {
                result["video_error"] =
                    json!("ffmpeg bulunamadı; ses ve altyazı üretildi, videoya bindirilmedi")
            }
        }
    }
    fs::write(
        out.join("summary.json"),
        serde_json::to_string_pretty(&result)?,
    )?;
    Ok(JobOutput {
        result,
        folder: Some(out),
        audio_s: Some(audio_s),
        duration_s: Some(seconds),
        gpu_peak_mb: gpu,
        voice_id: None,
        steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_gate() {
        let lines = vec![
            (1, "a".repeat(30), 2.0),
            (2, "a".repeat(30), 2.1),
            (3, "a".repeat(30), 1.9),
            (4, "a".repeat(30), 6.0),
        ];
        assert_eq!(speed_outliers(&lines), vec![4]);
    }

    #[test]
    fn request_json() {
        let r: JobRequest =
            serde_json::from_value(json!({"kind": "read", "voice": "x/1", "text": "Merhaba."}))
                .unwrap();
        assert!(matches!(r, JobRequest::Read { .. }));
        let r: JobRequest = serde_json::from_value(
            json!({"kind": "design", "name": "a", "description": "b", "text": "c"}),
        )
        .unwrap();
        assert!(matches!(r, JobRequest::Design { variants: 3, .. }));
    }

    #[test]
    fn clone_request_needs_only_consent() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            data_dir: dir.path().join("data"),
            resources: dir.path().join("res"),
            uv: None,
            engine: dir.path().join("engine"),
            whisper: None,
        };
        paths.prepare().unwrap();
        let store = Store::in_memory().unwrap();
        let recording = dir.path().join("rec.wav");
        fs::write(&recording, b"RIFF").unwrap();
        let request = |consent: bool| -> JobRequest {
            serde_json::from_value(json!({
                "kind": "clone", "recording": recording, "recording_text": "Merhaba, bu benim sesim.",
                "text": "Bugün hava çok güzel.", "consent": consent, "library_name": "Benim sesim",
            }))
            .unwrap()
        };
        summarize_request(&paths, &request(true), &store).unwrap();
        // an old client still sending consent_type is accepted (the field is ignored)
        let old: JobRequest = serde_json::from_value(json!({
            "kind": "clone", "recording": recording, "recording_text": "Merhaba, bu benim sesim.",
            "text": "Bugün hava çok güzel.", "consent": true, "consent_type": "self",
        }))
        .unwrap();
        summarize_request(&paths, &old, &store).unwrap();
        match summarize_request(&paths, &request(false), &store) {
            Err(Error::Issues(found)) => {
                let c = found.iter().find(|i| i.code == "consent_missing").unwrap();
                assert_eq!(c.message, CONSENT_MISSING);
                assert!(!c.message.contains(CONSENT_TEXT));
            }
            other => panic!("expected consent issue, got {other:?}"),
        }
    }

    #[test]
    fn design_check() {
        assert!(check_design("A (calm) voice", "Merhaba.", "tr")
            .iter()
            .any(|s| s.code == "description_parenthesis"));
        let long = "Bu çok uzun bir cümledir ve tekrar eder. ".repeat(20);
        assert!(check_design("A calm voice", &long, "tr")
            .iter()
            .any(|s| s.code == "design_too_long"));
    }
}
