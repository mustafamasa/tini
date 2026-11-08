//! Paths and user settings.
//!
//! Data dir: `~/Library/Application Support/Tini`. If the user picks another location, the choice is stored in
//! `data_dir.json` in the system config folder.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Error;

pub const MODEL_DIR_NAME: &str = "VoxCPM2";

/// Folders the app uses.
#[derive(Debug, Clone, Serialize)]
pub struct Paths {
    pub data_dir: PathBuf,
    /// bundled resources: voice_prompts, flags, pronunciation_dictionary.tsv
    pub resources: PathBuf,
    /// uv (Python env installer)
    pub uv: Option<PathBuf>,
    /// bundled engine folder: tini_engine.py, requirements.txt
    pub engine: PathBuf,
    /// whisper.cpp (transcription)
    pub whisper: Option<PathBuf>,
}

/// Binaries and engine folder shipped with the app.
#[derive(Debug, Clone, Default)]
pub struct Binaries {
    pub uv: Option<PathBuf>,
    pub engine: Option<PathBuf>,
    pub whisper: Option<PathBuf>,
}

impl Paths {
    pub fn model_root(&self) -> PathBuf {
        self.data_dir.join("model")
    }
    pub fn downloaded_model(&self) -> PathBuf {
        self.model_root().join(MODEL_DIR_NAME)
    }
    pub fn engine_script(&self) -> PathBuf {
        self.engine.join("tini_engine.py")
    }
    pub fn requirements(&self) -> PathBuf {
        self.engine.join("requirements.txt")
    }
    pub fn voices(&self) -> PathBuf {
        self.data_dir.join("voices")
    }
    /// Voices that ship with the app (read-only): `resources/voices/<name>/`.
    pub fn builtin_voices(&self) -> PathBuf {
        self.resources.join("voices")
    }
    /// Design studies: `designs/<job_id>/variant_NN/` (not part of the library until a variant is added).
    pub fn designs(&self) -> PathBuf {
        self.data_dir.join("designs")
    }
    pub fn trash(&self) -> PathBuf {
        self.voices().join("_trash")
    }
    pub fn jobs(&self) -> PathBuf {
        self.data_dir.join("jobs")
    }
    pub fn cache(&self) -> PathBuf {
        self.data_dir.join("cache")
    }
    pub fn work(&self) -> PathBuf {
        self.data_dir.join("work")
    }
    pub fn database(&self) -> PathBuf {
        self.data_dir.join("tini.sqlite")
    }
    pub fn settings_file(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }
    pub fn dictionary(&self) -> PathBuf {
        self.data_dir.join("pronunciation_dictionary.tsv")
    }

    pub fn prepare(&self) -> Result<(), Error> {
        for dir in [
            self.model_root(),
            self.voices(),
            self.designs(),
            self.jobs(),
            self.cache(),
            self.work(),
        ] {
            fs::create_dir_all(dir)?;
        }
        let dictionary = self.dictionary();
        let bundled = self.resources.join("pronunciation_dictionary.tsv");
        if !dictionary.exists() && bundled.exists() {
            fs::copy(bundled, dictionary)?;
        }
        Ok(())
    }
}

fn config_root() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("Tini"))
}

fn bootstrap_path() -> Option<PathBuf> {
    config_root().map(|d| d.join("data_dir.json"))
}

/// Default data dir (when the user has not picked one).
pub fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .map(|d| d.join("Tini"))
        .unwrap_or_else(|| PathBuf::from("Tini"))
}

/// The user-selected data dir, or the default.
pub fn data_dir() -> PathBuf {
    if let Some(path) = bootstrap_path() {
        if let Ok(text) = fs::read_to_string(path) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(dir) = v["data_dir"].as_str() {
                    return PathBuf::from(dir);
                }
            }
        }
    }
    default_data_dir()
}

/// Changes the data dir (files are not moved; the UI asks to move them first or a fresh dir is started).
pub fn set_data_dir(dir: &Path) -> Result<(), Error> {
    let path =
        bootstrap_path().ok_or_else(|| Error::Config("sistem ayar klasörü bulunamadı".into()))?;
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&serde_json::json!({"data_dir": dir}))?,
    )?;
    Ok(())
}

/// User settings (`<data_dir>/settings.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// auto | mps | cpu
    pub device: String,
    /// bfloat16 (fast, less memory) | float32 (slow, most stable)
    pub precision: String,
    /// solver steps (VoxCPM inference_timesteps): 4-30, recommended 10
    pub steps: u32,
    /// guidance strength (VoxCPM cfg_value): 1.0-3.0, recommended 2.0
    pub cfg: f64,
    /// unload the engine after this many idle minutes (0: never)
    pub idle_unload_min: u32,
    pub word_timing: bool,
    /// bring the reading's timbre closer to the voice's reference recording (spectral matching)
    pub tone_match: bool,
    /// a user-selected, sha256-verified model folder
    pub model_path: Option<PathBuf>,
    /// export defaults
    pub export_dir: Option<PathBuf>,
    pub export_format: String,
    /// tokens: {ses} {tarih} {saat} {baslik} (user-facing syntax, kept in Turkish)
    pub file_name_template: String,
    pub keep_intermediate: bool,
    /// system | light | dark
    pub theme: String,
    pub last_voice: Option<String>,
    /// local HTTP API (127.0.0.1 only)
    pub api_enabled: bool,
    pub api_port: u16,
    /// bearer token for the API (32 hex characters, generated on first start)
    pub api_token: String,
}

/// Default port of the local HTTP API.
pub const DEFAULT_API_PORT: u16 = 7860;

/// A new random API token (32 hex characters).
pub fn new_api_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            device: "auto".into(),
            precision: "bfloat16".into(),
            steps: crate::engine::DEFAULT_STEPS,
            cfg: crate::engine::DEFAULT_CFG,
            idle_unload_min: 10,
            word_timing: true,
            tone_match: false,
            model_path: None,
            export_dir: None,
            export_format: "wav".into(),
            file_name_template: "{ses}_{tarih}_{saat}".into(),
            keep_intermediate: false,
            theme: "system".into(),
            last_voice: None,
            api_enabled: false,
            api_port: DEFAULT_API_PORT,
            api_token: String::new(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), Error> {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p)?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), Error> {
        if !["auto", "mps", "cpu"].contains(&self.device.as_str()) {
            return Err(Error::Config(format!("bilinmeyen cihaz: {}", self.device)));
        }
        if !["bfloat16", "float32"].contains(&self.precision.as_str()) {
            return Err(Error::Config(format!(
                "bilinmeyen hassasiyet: {}",
                self.precision
            )));
        }
        if !(1..=50).contains(&self.steps) {
            return Err(Error::Config(
                "adım sayısı 1-50 olmalı (önerilen 10)".into(),
            ));
        }
        if !(1.0..=4.0).contains(&self.cfg) {
            return Err(Error::Config("CFG 1.0-4.0 olmalı (önerilen 2.0)".into()));
        }
        if self.idle_unload_min > 240 {
            return Err(Error::Config("boşta kapatma süresi en çok 240 dk".into()));
        }
        if !["wav", "mp3"].contains(&self.export_format.as_str()) {
            return Err(Error::Config(
                "dışa aktarma biçimi wav ya da mp3 olmalı".into(),
            ));
        }
        if self.api_port < 1024 {
            return Err(Error::Config("API portu 1024-65535 arasında olmalı".into()));
        }
        Ok(())
    }

    /// Key for duration estimates: the engine configuration (keeps measurements of different setups apart).
    pub fn engine_label(&self) -> String {
        format!("voxcpm-{}-{}-{}", self.device, self.precision, self.steps)
    }
}

/// System info (for the setup wizard).
#[derive(Debug, Clone, Serialize)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub ram_gb: Option<f64>,
    pub gpu: Option<String>,
    pub warnings: Vec<String>,
}

pub fn system_info() -> SystemInfo {
    let mut warnings = Vec::new();
    let ram_gb = ram_bytes().map(|b| (b as f64 / 1024f64.powi(3) * 10.0).round() / 10.0);
    if let Some(r) = ram_gb {
        if r < 12.0 {
            warnings.push(format!(
                "Bu Mac'te {r} GB bellek var. Motor (VoxCPM2, bfloat16) yaklaşık 5,5 GB kullanır; 16 GB önerilir. Üretim sırasında diğer uygulamaları kapatın."
            ));
        }
    }
    if std::env::consts::ARCH != "aarch64" {
        warnings.push("Intel Mac desteklenmiyor; Apple Silicon (M1 ve sonrası) gerekir.".into());
    }
    SystemInfo {
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        ram_gb,
        gpu: chip_name(),
        warnings,
    }
}

fn ram_bytes() -> Option<u64> {
    let out = std::process::Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn chip_name() -> Option<String> {
    let out = std::process::Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
