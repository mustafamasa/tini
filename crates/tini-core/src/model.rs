//! Download and verification of the VoxCPM2 weights (openbmb/VoxCPM2, Apache-2.0): pinned commit, per-file sha256,
//! resume via HTTP Range, cancellation.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::Error;

pub const REPO: &str = "openbmb/VoxCPM2";
pub const COMMIT: &str = "32279effe8c19989596f05d353d1447f51d9e915";

/// (file, bytes, sha256): files the model needs to run.
pub const FILES: [(&str, u64, &str); 7] = [
    (
        "config.json",
        4_336,
        "405f0dcd92f7feba6011ed4eac5c8d4f74cba9712f07fd5cfa3063bbdd95402c",
    ),
    (
        "tokenizer.json",
        3_676_772,
        "f8984687e4a92a3503d521396d454b7d68e9fdaab2a0288eb3536c7c1aa4bc20",
    ),
    (
        "tokenizer_config.json",
        5_059,
        "e78a3ebb48a0b9437efd1823b6b726c823da89e49dd8bcc90c02419d9baa772b",
    ),
    (
        "special_tokens_map.json",
        1_632,
        "068594063e37662c02b21acf42ebb334ef6a74fb810e68a2368f88f08351de76",
    ),
    (
        "tokenization_voxcpm2.py",
        2_895,
        "84489ea32b6ee0cae22ed5480cacb6df85c46624c3119be9a2021c3649a12729",
    ),
    (
        "audiovae.pth",
        376_951_122,
        "94b5d51e107e0507d4acc976cfdadb64edd6fd06d1f751dadbf2fd1594274bf1",
    ),
    (
        "model.safetensors",
        4_580_080_592,
        "f7f964cfa9da23653baec6e6f7750719977ad944ed9f95fe52fe3a620506891d",
    ),
];

pub fn total_size() -> u64 {
    FILES.iter().map(|d| d.1).sum()
}

fn file_url(file: &str) -> String {
    format!("https://huggingface.co/{REPO}/resolve/{COMMIT}/{file}")
}

/// Whether every file in the folder has the expected size.
pub fn is_ready(dir: &Path) -> bool {
    FILES.iter().all(|(name, size, _)| {
        fs::metadata(dir.join(name))
            .map(|m| m.len() == *size)
            .unwrap_or(false)
    })
}

/// Total size of the model files present in the folder (including partial downloads).
pub fn present_size(dir: &Path) -> u64 {
    FILES
        .iter()
        .map(|(name, _, _)| {
            let full = fs::metadata(dir.join(name)).map(|m| m.len()).unwrap_or(0);
            if full > 0 {
                full
            } else {
                fs::metadata(dir.join(format!("{name}.part")))
                    .map(|m| m.len())
                    .unwrap_or(0)
            }
        })
        .sum()
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadStatus {
    /// downloading | verifying | extracting | installing | done | error | cancelled
    pub stage: String,
    pub current: u64,
    pub total: u64,
    pub speed_mb_s: f64,
    pub message: Option<String>,
}

impl DownloadStatus {
    pub fn new(stage: &str, current: u64, total: u64) -> Self {
        DownloadStatus {
            stage: stage.into(),
            current,
            total,
            speed_mb_s: 0.0,
            message: None,
        }
    }
}

/// sha256 of a file, reporting progress.
pub fn sha256_file(
    path: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64),
) -> Result<String, Error> {
    let mut f = fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 8 * 1024 * 1024];
    let mut done = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        done += n as u64;
        progress(done);
    }
    Ok(hex::encode(h.finalize()))
}

/// Downloads `url` to `target` (resuming a partial `.part`) and verifies its sha256. `before` and `grand_total` let a
/// multi-file download report progress as a single bar.
#[allow(clippy::too_many_arguments)]
pub fn download_file(
    url: &str,
    target: &Path,
    sha: &str,
    size: Option<u64>,
    before: u64,
    grand_total: u64,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(DownloadStatus),
) -> Result<(), Error> {
    if let Some(p) = target.parent() {
        fs::create_dir_all(p)?;
    }
    let part = target.with_file_name(format!(
        "{}.part",
        target.file_name().unwrap().to_string_lossy()
    ));
    let mut have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if size.map(|b| have < b).unwrap_or(true) {
        let agent = ureq::AgentBuilder::new().redirects(10).build();
        let mut req = agent.get(url);
        if have > 0 {
            req = req.set("Range", &format!("bytes={have}-"));
        }
        let resp = match req.call() {
            Ok(r) => r,
            Err(ureq::Error::Status(416, _)) if have > 0 => {
                have = 0;
                agent
                    .get(url)
                    .call()
                    .map_err(|e| Error::Network(format!("indirme başlatılamadı: {e}")))?
            }
            Err(e) => return Err(Error::Network(format!("indirme başlatılamadı: {e}"))),
        };
        if have > 0 && resp.status() != 206 {
            have = 0; // server does not support resume: start over
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(have > 0)
            .truncate(have == 0)
            .open(&part)?;
        let mut reader = resp.into_reader();
        let mut buf = vec![0u8; 1024 * 1024];
        let started = Instant::now();
        let start_have = have;
        let mut last = Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                report(DownloadStatus::new("cancelled", before + have, grand_total));
                return Err(Error::Cancelled);
            }
            let n = reader
                .read(&mut buf)
                .map_err(|e| Error::Network(format!("indirme kesildi: {e}")))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            have += n as u64;
            if last.elapsed().as_millis() > 300 {
                let secs = started.elapsed().as_secs_f64().max(0.001);
                report(DownloadStatus {
                    stage: "downloading".into(),
                    current: before + have,
                    total: grand_total,
                    speed_mb_s: ((have - start_have) as f64 / 1e6 / secs * 10.0).round() / 10.0,
                    message: None,
                });
                last = Instant::now();
            }
        }
        file.flush()?;
    }
    report(DownloadStatus::new("verifying", before, grand_total));
    let mut last = Instant::now();
    let got = sha256_file(&part, cancel, &mut |done| {
        if last.elapsed().as_millis() > 300 {
            report(DownloadStatus::new("verifying", before + done, grand_total));
            last = Instant::now();
        }
    })?;
    if got != sha {
        let _ = fs::remove_file(&part);
        return Err(Error::Network(format!(
            "sha256 tutmadı ({}); indirme silindi, yeniden deneyin",
            target.display()
        )));
    }
    fs::rename(&part, target)?;
    Ok(())
}

/// Downloads the model into `dir`; skips complete files and resumes from partial `.part` files.
pub fn download(
    dir: &Path,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(DownloadStatus),
) -> Result<(), Error> {
    fs::create_dir_all(dir)?;
    let total = total_size();
    let mut before = 0u64;
    for (name, size, sha) in FILES {
        let target = dir.join(name);
        if fs::metadata(&target)
            .map(|m| m.len() == size)
            .unwrap_or(false)
        {
            before += size;
            continue;
        }
        let _ = fs::remove_file(&target);
        download_file(
            &file_url(name),
            &target,
            sha,
            Some(size),
            before,
            total,
            cancel,
            report,
        )?;
        before += size;
    }
    // the caller reports "done" after it has recorded the result
    Ok(())
}

/// Verifies an existing model folder (sha256 of every file).
pub fn verify_dir(
    dir: &Path,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(DownloadStatus),
) -> Result<(), Error> {
    let total = total_size();
    let mut before = 0u64;
    for (name, size, sha) in FILES {
        let path = dir.join(name);
        let m = fs::metadata(&path).map_err(|_| {
            Error::Input(format!(
                "klasörde {name} yok; VoxCPM2 model klasörünü seçin"
            ))
        })?;
        if m.len() != size {
            return Err(Error::Input(format!(
                "{name} beklenen boyutta değil ({} bayt); dosya farklı sürüm ya da yarım",
                m.len()
            )));
        }
        let mut last = Instant::now();
        let got = sha256_file(&path, cancel, &mut |done| {
            if last.elapsed().as_millis() > 300 {
                report(DownloadStatus::new("verifying", before + done, total));
                last = Instant::now();
            }
        })?;
        if got != sha {
            return Err(Error::Input(format!(
                "{name} sha256 tutmadı; bu klasör beklenen VoxCPM2 sürümü değil"
            )));
        }
        before += size;
    }
    // the caller reports "done" after it has recorded the result
    Ok(())
}

/// Fingerprint of a model folder (path plus size|mtime of the large files), so a verified folder is not re-hashed on
/// every launch.
pub fn fingerprint(dir: &Path) -> Option<String> {
    let mut parts = vec![dir.display().to_string()];
    for name in ["model.safetensors", "audiovae.pth"] {
        let m = fs::metadata(dir.join(name)).ok()?;
        let mtime = m
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        parts.push(format!("{}|{mtime}", m.len()));
    }
    Some(parts.join("|"))
}
