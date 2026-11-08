//! Python env of the voice engine: installed with the bundled `uv` under the data dir (`engine_env/`).
//!
//! Python (python-build-standalone) and the packages are pinned in `engine/requirements.txt`; VoxCPM is installed with
//! `--no-deps` (demo/training dependencies are not needed). `tini_env.json` records which requirements file the env
//! was built from; when that file changes (app update) the env has to be reinstalled.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::model::DownloadStatus;
use crate::Error;

pub const PYTHON_VERSION: &str = "3.12";
pub const VOXCPM_VERSION: &str = "voxcpm==2.0.3";
/// Approximate install size (for display).
pub const ESTIMATED_SIZE: u64 = 1_100_000_000;
pub const DIR_NAME: &str = "engine_env";
pub const MARKER_NAME: &str = "tini_env.json";

pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join(DIR_NAME)
}

pub fn python(data_dir: &Path) -> PathBuf {
    dir(data_dir).join("bin").join("python")
}

fn marker(data_dir: &Path) -> PathBuf {
    dir(data_dir).join(MARKER_NAME)
}

fn requirements_fingerprint(requirements: &Path) -> Option<String> {
    let content = fs::read(requirements).ok()?;
    let mut h = Sha256::new();
    h.update(&content);
    h.update(VOXCPM_VERSION.as_bytes());
    h.update(PYTHON_VERSION.as_bytes());
    Some(hex::encode(h.finalize())[..16].to_string())
}

fn read_marker(data_dir: &Path) -> Option<serde_json::Value> {
    let text = fs::read_to_string(marker(data_dir)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Env state: missing | outdated (requirements changed) | ready.
pub fn status(data_dir: &Path, requirements: &Path) -> &'static str {
    if !python(data_dir).is_file() {
        return "missing";
    }
    match (
        read_marker(data_dir),
        requirements_fingerprint(requirements),
    ) {
        (Some(m), Some(fp))
            if m["fingerprint"].as_str().or(m["iz"].as_str()) == Some(fp.as_str()) =>
        {
            "ready"
        }
        (Some(_), None) => "ready",
        _ => "outdated",
    }
}

/// Runs a command, passing each output line to `on_line` and watching for cancellation.
fn run(mut cmd: Command, cancel: &AtomicBool, on_line: &mut dyn FnMut(&str)) -> Result<(), Error> {
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    let name = format!("{:?}", cmd.get_program());
    let mut child = cmd
        .spawn()
        .map_err(|e| Error::Engine(format!("{name} başlatılamadı: {e}")))?;
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let streams = [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    ];
    for stream in streams.into_iter().flatten() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for l in BufReader::new(stream).lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        });
    }
    drop(tx);
    let mut tail: Vec<String> = Vec::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Cancelled);
        }
        while let Ok(l) = rx.try_recv() {
            on_line(&l);
            tail.push(l);
            if tail.len() > 30 {
                tail.remove(0);
            }
        }
        if let Some(st) = child.try_wait()? {
            while let Ok(l) = rx.recv_timeout(Duration::from_millis(50)) {
                tail.push(l);
            }
            if !st.success() {
                return Err(Error::Engine(format!(
                    "{name} hata ile bitti ({st}):\n{}",
                    tail.join("\n")
                )));
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(120));
    }
}

fn uv_command(uv: &Path, data_dir: &Path) -> Command {
    let mut c = Command::new(uv);
    c.env("UV_PYTHON_INSTALL_DIR", data_dir.join("python"))
        .env("UV_CACHE_DIR", data_dir.join("work").join("uv_cache"))
        .env("UV_PYTHON_PREFERENCE", "only-managed")
        .env("UV_NO_PROGRESS", "1")
        .env_remove("VIRTUAL_ENV")
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME");
    c
}

/// Installs (or repairs) the env. Steps are reported as 0-4.
pub fn install(
    uv: &Path,
    data_dir: &Path,
    requirements: &Path,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(DownloadStatus),
) -> Result<(), Error> {
    if !uv.is_file() {
        return Err(Error::Config(format!(
            "uv bulunamadı ({}); uygulama paketi eksik",
            uv.display()
        )));
    }
    if !requirements.is_file() {
        return Err(Error::Config(format!(
            "gereksinim listesi yok: {}",
            requirements.display()
        )));
    }
    let target = dir(data_dir);
    let py = python(data_dir);
    let total = 4u64;
    let step = |n: u64, message: &str, report: &mut dyn FnMut(DownloadStatus)| {
        report(DownloadStatus {
            stage: "installing".into(),
            current: n,
            total,
            speed_mb_s: 0.0,
            message: Some(message.into()),
        });
    };
    let _ = fs::remove_file(marker(data_dir));

    step(0, "Python 3.12 hazırlanıyor", report);
    let mut c = uv_command(uv, data_dir);
    c.args(["venv", "--clear", "--python", PYTHON_VERSION, "--seed"])
        .arg(&target);
    run(c, cancel, &mut |_| {})?;

    step(1, "PyTorch ve bağımlılıklar indiriliyor (~1 GB)", report);
    let mut c = uv_command(uv, data_dir);
    c.args(["pip", "install", "--python"])
        .arg(&py)
        .arg("-r")
        .arg(requirements);
    run(c, cancel, &mut |l| {
        let l = l.trim();
        if !l.is_empty() {
            let last: String = l.chars().take(120).collect();
            report(DownloadStatus {
                stage: "installing".into(),
                current: 1,
                total,
                speed_mb_s: 0.0,
                message: Some(last),
            });
        }
    })?;

    step(2, "VoxCPM kuruluyor", report);
    let mut c = uv_command(uv, data_dir);
    c.args(["pip", "install", "--no-deps", "--python"])
        .arg(&py)
        .arg(VOXCPM_VERSION);
    run(c, cancel, &mut |_| {})?;

    step(3, "Ortam sınanıyor", report);
    let mut c = Command::new(&py);
    c.args([
        "-I",
        "-c",
        "import voxcpm, torch, soundfile, librosa; print('mps', torch.backends.mps.is_available())",
    ]);
    let mut mps = String::new();
    run(c, cancel, &mut |l| {
        if l.starts_with("mps ") {
            mps = l.to_string();
        }
    })?;
    fs::write(
        marker(data_dir),
        serde_json::to_string_pretty(&json!({
            "fingerprint": requirements_fingerprint(requirements), "voxcpm": VOXCPM_VERSION, "python": PYTHON_VERSION,
            "mps": mps.ends_with("True"), "date": crate::now_string(),
        }))?,
    )?;
    let _ = fs::remove_dir_all(data_dir.join("work").join("uv_cache"));
    report(DownloadStatus {
        stage: "done".into(),
        current: total,
        total,
        speed_mb_s: 0.0,
        message: Some(mps),
    });
    Ok(())
}
