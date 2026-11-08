//! Tını command line (for development and end-to-end testing).
//!
//!   tini-cli status
//!   tini-cli voices
//!   tini-cli jobs
//!   tini-cli read <voice id> <text|@file>
//!   tini-cli design <name> <description> <text|@file> [variants] [preview text|@file]
//!   tini-cli clone <recording> <recording text> <text> --consent
//!   tini-cli line <job id> <line no> [seed]
//!   tini-cli job <request JSON|@file>
//!   tini-cli serve                (local HTTP API in the foreground; settings port and token, see /docs)
//!   tini-cli install-env          (installs the voice engine's Python env with uv)
//!   tini-cli use-model <dir>      (verifies and uses an existing VoxCPM2 folder)
//!   tini-cli raw <voice dir> <text> <out.wav> [ultimate|reference]
//!   tini-cli tone <reference> <input> <out.wav>
//!   tini-cli json <out.json> [sample text file]
//!
//! Environment: TINI_DATA_DIR (data dir), TINI_RESOURCES (resources folder), TINI_UV (uv binary), TINI_WHISPER,
//! TINI_NO_WORKER (enqueue only).

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;

use tini_core::jobs::JobRequest;
use tini_core::queue::Event;
use tini_core::Studio;

fn arg_text(v: &str) -> String {
    match v.strip_prefix('@') {
        Some(p) => std::fs::read_to_string(p).expect("cannot read file"),
        None => v.to_string(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let resources = std::env::var("TINI_RESOURCES")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo.join("resources"));
    let uv = std::env::var("TINI_UV")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            Some(repo.join("app/src-tauri/binaries/uv-aarch64-apple-darwin"))
                .filter(|p| p.is_file())
        });
    let data_dir = std::env::var("TINI_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| tini_core::settings::data_dir());
    let (tx, rx) = mpsc::channel::<Event>();
    let tx = std::sync::Mutex::new(tx);
    let binaries = tini_core::settings::Binaries {
        uv,
        engine: Some(repo.join("engine/voxcpm")),
        whisper: std::env::var("TINI_WHISPER")
            .map(PathBuf::from)
            .ok()
            .or_else(|| {
                Some(repo.join("app/src-tauri/binaries/whisper_cli-aarch64-apple-darwin"))
                    .filter(|p| p.is_file())
            }),
    };
    let studio = Studio::open_at(
        data_dir,
        resources,
        binaries,
        Arc::new(move |e| {
            let _ = tx.lock().unwrap().send(e);
        }),
    )
    .expect("cannot open studio");
    let command = args.first().map(String::as_str).unwrap_or("status");
    let request = match command {
        "status" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&studio.status()).unwrap()
            );
            return;
        }
        "voices" => {
            for v in studio.voices().unwrap() {
                println!(
                    "{:<28} {:<10} {:<4} {}",
                    v.id, v.source, v.language, v.display_name
                );
            }
            return;
        }
        "json" => {
            // tini-cli json <file> [sample text]: dumps real data as a sample for the UI preview
            let mut job_map = serde_json::Map::new();
            for j in studio.jobs(&Default::default()).unwrap() {
                job_map.insert(
                    j.id.clone(),
                    serde_json::to_value(studio.job(&j.id).unwrap()).unwrap(),
                );
            }
            let sample = args.get(2).map(|f| std::fs::read_to_string(f).unwrap());
            let data = serde_json::json!({
                "status": studio.status(),
                "settings": studio.settings(),
                "voices": studio.voices().unwrap(),
                "voice_prompts": studio.voice_prompts().unwrap(),
                "jobs": studio.jobs(&Default::default()).unwrap(),
                "job": job_map,
                "text_check": studio.check_text(
                    "Merhaba, GPU ile ilk denememizi yapıyoruz.\nSaat 5'te başlıyoruz. || paragraf\nHer şey yolunda.",
                    None,
                    None,
                ),
                "dictionary": studio.read_dictionary(),
                "fix": sample.as_ref().map(|t| studio.fix_text(t, None, None, &Default::default())),
                "fix_text": sample,
            });
            let out = Path::new(&args[1]);
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir).unwrap();
            }
            std::fs::write(out, serde_json::to_string(&data).unwrap()).unwrap();
            return;
        }
        "raw" => {
            // tini-cli raw <voice dir> <text> <out.wav> [ultimate|reference]
            // Produces raw engine output without touching the product (no tone match, no trimming).
            let mut config = studio.shared.engine_config().expect("engine config");
            config.word_timing = false;
            config.keep_intermediate = true;
            let folder = PathBuf::from(&args[1]);
            let card = tini_core::voices::read_card(&folder).unwrap();
            let temp = std::env::temp_dir().join("tini_raw");
            let reference = tini_core::engine::reference_wav(
                &folder.join(card["reference"].as_str().unwrap()),
                &temp,
            )
            .unwrap();
            let mut request = tini_core::engine::read_request(
                "s1",
                &args[2],
                card["read_seed"].as_u64().unwrap_or(42) as u32,
                &reference,
                card["design_text"].as_str().unwrap_or(""),
            );
            if let Some(mode) = args.get(4) {
                request.mode = mode.clone();
            }
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let r =
                tini_core::engine::run_sequence(&config, &[request], &temp, &cancel, &mut |p| {
                    eprintln!("  {}/{} {:.0} s", p.completed, p.total, p.elapsed_s)
                })
                .unwrap();
            println!(
                "{} s (load {:?}), failed {:?} {:?}",
                r.seconds, r.load_s, r.failed, r.reasons
            );
            tini_core::audio::write_wav(
                Path::new(&args[3]),
                &tini_core::engine::to_audio(r.clips["s1"].clone()),
                None,
            )
            .unwrap();
            tini_core::engine::shutdown();
            return;
        }
        "install-env" | "use-model" => {
            if command == "install-env" {
                studio
                    .install_engine_env()
                    .expect("cannot install engine env");
            } else {
                studio
                    .use_local_model(PathBuf::from(&args[1]))
                    .expect("cannot use model");
            }
            for event in rx {
                let d = match event {
                    Event::EngineEnv(d) | Event::Model(d) => d,
                    _ => continue,
                };
                println!(
                    "  {} {}/{} {}",
                    d.stage,
                    d.current,
                    d.total,
                    d.message.clone().unwrap_or_default()
                );
                if d.stage == "done" || d.stage == "error" || d.stage == "cancelled" {
                    std::process::exit(if d.stage == "done" { 0 } else { 1 });
                }
            }
            return;
        }
        "tone" => {
            // tini-cli tone <reference> <input> <out.wav>: applies tone match to a file
            let r = tini_core::audio::load(Path::new(&args[1])).unwrap();
            let g = tini_core::audio::load(Path::new(&args[2])).unwrap();
            let p = tini_core::tone::derive_profile(
                &tini_core::tone::band_profile(&r.samples, r.sample_rate).unwrap(),
                &tini_core::tone::band_profile(&g.samples, g.sample_rate).unwrap(),
            );
            println!("{}", serde_json::to_string(&p).unwrap());
            let out = tini_core::tone::apply(&g.samples, g.sample_rate, &p);
            tini_core::audio::write_wav(
                Path::new(&args[3]),
                &tini_core::audio::Audio {
                    samples: out,
                    sample_rate: g.sample_rate,
                },
                None,
            )
            .unwrap();
            return;
        }
        "serve" => {
            // the local HTTP API in the foreground (development); uses the settings' port and token
            let s = studio.settings();
            let server = match tini_core::api::bind(s.api_port) {
                Ok(server) => server,
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            };
            let url = tini_core::api::url(s.api_port);
            println!("Tını API listening on {url}");
            println!("  docs:    {url}/docs");
            println!("  openapi: {url}/openapi.json");
            println!("  token:   {}", s.api_token);
            println!("  header:  Authorization: Bearer <token>");
            std::thread::spawn(move || for _ in rx {});
            tini_core::api::serve(&studio, &server, s.api_port);
            return;
        }
        "jobs" => {
            for j in studio.jobs(&Default::default()).unwrap() {
                println!(
                    "{} {:<10} {:<11} {:>6.1} s audio  {}",
                    j.id,
                    j.kind,
                    j.status,
                    j.audio_s.unwrap_or(0.0),
                    j.title
                );
            }
            return;
        }
        "read" => JobRequest::Read {
            voice: args[1].clone(),
            text: arg_text(&args[2]),
            language: None,
            title: None,
        },
        "design" => JobRequest::Design {
            name: args[1].clone(),
            description: arg_text(&args[2]),
            text: arg_text(&args[3]),
            language: None,
            variants: args.get(4).and_then(|c| c.parse().ok()).unwrap_or(2),
            seeds: None,
            preview_text: args.get(5).map(|d| arg_text(d)),
        },
        "clone" => JobRequest::Clone {
            recording: args[1].clone(),
            recording_text: arg_text(&args[2]),
            text: arg_text(&args[3]),
            seed: None,
            consent: args.iter().any(|a| a == "--consent"),
            library_name: None,
        },
        "job" => serde_json::from_str(&arg_text(&args[1])).expect("invalid job request JSON"),
        "line" => JobRequest::Line {
            job_id: args[1].clone(),
            line: args[2].parse().unwrap(),
            text: None,
            seed: args.get(3).and_then(|t| t.parse().ok()),
        },
        other => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
    };
    let job = match studio.enqueue_job(request) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("error: {}", tini_core::ErrorResponse::from(&e).message);
            std::process::exit(1);
        }
    };
    println!(
        "job {} queued ({} steps, estimated {:.0} s)",
        job.id,
        job.total,
        job.estimated_s.unwrap_or(0.0)
    );
    if std::env::var_os(tini_core::studio::NO_WORKER_ENV).is_some() {
        return;
    }
    for event in rx {
        if let Event::Job {
            id,
            status,
            completed,
            total,
            elapsed_s,
            message,
            ..
        } = event
        {
            if id != job.id {
                continue;
            }
            println!(
                "  {status} {completed}/{total} {elapsed_s:.0} s {}",
                message.unwrap_or_default()
            );
            if status != "running" {
                let last = studio.job(&job.id).unwrap().unwrap();
                let mut result = last.result.clone().unwrap_or_default();
                if let Some(o) = result.as_object_mut() {
                    o.remove("words");
                }
                println!("{}", serde_json::to_string_pretty(&result).unwrap());
                if let Some(e) = last.error {
                    eprintln!("error: {e}");
                }
                std::process::exit(if status == "done" { 0 } else { 1 });
            }
        }
    }
}
