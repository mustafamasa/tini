//! Voice engine: the official VoxCPM (OpenBMB, Apache-2.0) runs in a persistent Python sidecar
//! (`engine/tini_engine.py`).
//!
//! The model is loaded once (bfloat16 on MPS takes ~20 s) and jobs are generated in the same process. After an idle
//! period the process exits and frees memory; the next job restarts it. The protocol is one JSON object per line.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use crate::audio::{self, Audio, SAMPLE_RATE};
use crate::Error;

/// VoxCPM defaults (the documented recommended values).
pub const DEFAULT_STEPS: u32 = 10;
pub const DEFAULT_CFG: f64 = 2.0;
pub const MIN_REFERENCE_SECONDS: f64 = 8.0;
pub const ENGINE_DIR: &str = "_engine";
pub const MODEL_NAME: &str = "VoxCPM2";

/// How and where the engine runs.
#[derive(Debug, Clone, Serialize)]
pub struct EngineConfig {
    /// Python interpreter of the sidecar (the env the app installs)
    pub python: PathBuf,
    /// tini_engine.py
    pub script: PathBuf,
    /// VoxCPM2 weights folder
    pub model: PathBuf,
    /// auto | mps | cpu
    pub device: String,
    /// bfloat16 | float32
    pub precision: String,
    pub steps: u32,
    pub cfg: f64,
    /// on-disk cache of reference encodings (prompt caches)
    pub cache_dir: PathBuf,
    /// the process exits after this many idle seconds (0: never)
    pub idle_unload_s: u64,
    pub word_timing: bool,
    pub tone_match: bool,
    /// debugging: always keep intermediate files
    pub keep_intermediate: bool,
}

impl EngineConfig {
    fn process_key(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            self.python.display(),
            self.script.display(),
            self.model.display(),
            self.precision,
            self.device
        )
    }
}

/// A single request sent to the engine.
#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub id: String,
    pub text: String,
    pub seed: u32,
    /// design | ultimate | reference
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_text: Option<String>,
}

/// Design request: the description goes in parentheses before the text (VoxCPM Voice Design format).
pub fn design_request(id: &str, description: &str, text: &str, seed: u32) -> Request {
    Request {
        id: id.into(),
        text: format!("({}){}", description.trim(), text.trim()),
        seed,
        mode: "design".into(),
        reference: None,
        reference_text: None,
    }
}

/// Read request: the voice's recording serves as both continuation prompt and reference, with its exact transcript
/// (VoxCPM "Ultimate cloning"); timbre and rhythm are preserved most faithfully in this mode.
pub fn read_request(
    id: &str,
    text: &str,
    seed: u32,
    reference: &Path,
    reference_text: &str,
) -> Request {
    let reference_text = reference_text.trim();
    Request {
        id: id.into(),
        text: text.into(),
        seed,
        mode: if reference_text.is_empty() {
            "reference".into()
        } else {
            "ultimate".into()
        },
        reference: Some(reference.to_string_lossy().to_string()),
        reference_text: Some(reference_text.into()),
    }
}

/// Progress report: completed requests, total requests, elapsed seconds.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Progress {
    pub completed: usize,
    pub total: usize,
    pub elapsed_s: f64,
}

pub struct BatchResult {
    pub clips: HashMap<String, Vec<f32>>,
    /// requests that could not be completed (reasons in `reasons`)
    pub failed: Vec<String>,
    pub reasons: HashMap<String, String>,
    pub seconds: f64,
    pub gpu_peak_mb: Option<u64>,
    /// model load time (when this batch loaded it)
    pub load_s: Option<f64>,
}

// --- sidecar process -------------------------------------------------------------------------------------------------

struct Process {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    stderr: Arc<Mutex<VecDeque<String>>>,
    key: String,
    loaded: bool,
    last_used: Instant,
    idle_unload_s: u64,
}

impl Process {
    fn send(&mut self, v: &Value) -> Result<(), Error> {
        let mut line = serde_json::to_string(v)?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|_| self.stdin.flush())
            .map_err(|e| {
                Error::Engine(format!(
                    "motora yazılamadı (süreç kapanmış olabilir): {e}\n{}",
                    self.stderr_tail()
                ))
            })
    }

    fn stderr_tail(&self) -> String {
        self.stderr
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn shutdown(mut self) {
        let _ = self.send(&json!({"cmd": "shutdown"}));
        for _ in 0..30 {
            if !self.is_running() {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn process_slot() -> &'static Mutex<Option<Process>> {
    static SLOT: OnceLock<Mutex<Option<Process>>> = OnceLock::new();
    SLOT.get_or_init(|| {
        // idle-unload watcher
        thread::Builder::new()
            .name("tini-engine-idle".into())
            .spawn(|| loop {
                thread::sleep(Duration::from_secs(15));
                let slot = process_slot();
                let Ok(mut g) = slot.try_lock() else { continue };
                let idle = g
                    .as_ref()
                    .map(|s| {
                        s.idle_unload_s > 0 && s.last_used.elapsed().as_secs() >= s.idle_unload_s
                    })
                    .unwrap_or(false);
                if idle {
                    if let Some(s) = g.take() {
                        s.shutdown();
                    }
                }
            })
            .ok();
        Mutex::new(None)
    })
}

/// Whether the engine process is in memory with the model loaded.
pub fn is_loaded() -> bool {
    process_slot()
        .try_lock()
        .map(|g| g.as_ref().map(|s| s.loaded).unwrap_or(false))
        .unwrap_or(true)
}

/// Stops the engine (app exit, settings change, "free memory").
pub fn shutdown() {
    if let Some(s) = process_slot().lock().unwrap().take() {
        s.shutdown();
    }
}

fn spawn(config: &EngineConfig) -> Result<Process, Error> {
    if !config.python.is_file() {
        return Err(Error::Config(
            "ses motoru ortamı kurulmamış; Kurulum ekranından kurun".into(),
        ));
    }
    if !config.script.is_file() {
        return Err(Error::Config(format!(
            "motor betiği yok: {}",
            config.script.display()
        )));
    }
    let mut cmd = Command::new(&config.python);
    cmd.arg("-I")
        .arg(&config.script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PYTHONUNBUFFERED", "1")
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .env("TOKENIZERS_PARALLELISM", "false")
        .env("PYTORCH_ENABLE_MPS_FALLBACK", "1")
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME");
    let spawn_guard = crate::SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let spawned = cmd.spawn();
    drop(spawn_guard);
    let mut child = spawned.map_err(|e| {
        Error::Engine(format!(
            "motor başlatılamadı ({}): {e}",
            config.python.display()
        ))
    })?;
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr_pipe = child.stderr.take().unwrap();
    let (tx, rx) = mpsc::channel::<Value>();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if tx.send(v).is_err() {
                    break;
                }
            }
        }
    });
    let stderr = Arc::new(Mutex::new(VecDeque::new()));
    let tail = stderr.clone();
    thread::spawn(move || {
        for line in BufReader::new(stderr_pipe).lines().map_while(Result::ok) {
            let mut q = tail.lock().unwrap();
            q.push_back(line);
            while q.len() > 60 {
                q.pop_front();
            }
        }
    });
    let mut s = Process {
        child,
        stdin,
        rx,
        stderr,
        key: config.process_key(),
        loaded: false,
        last_used: Instant::now(),
        idle_unload_s: config.idle_unload_s,
    };
    match s.rx.recv_timeout(Duration::from_secs(60)) {
        Ok(v) if v["event"] == "started" => Ok(s),
        Ok(v) => Err(Error::Engine(format!(
            "motordan beklenmeyen ilk yanıt: {v}"
        ))),
        Err(_) => {
            let tail = s.stderr_tail();
            let _ = s.child.kill();
            Err(Error::Engine(format!(
                "motor açılmadı (Python ortamı bozuk olabilir; Kurulum'dan onarın)\n{tail}"
            )))
        }
    }
}

/// Waits for an event while watching for cancellation and process death.
fn wait_for(
    s: &mut Process,
    cancel: &AtomicBool,
    timeout: Duration,
    on_tick: &mut dyn FnMut(),
    on_event: &mut dyn FnMut(&Value) -> Option<Result<(), Error>>,
) -> Result<(), Error> {
    let start = Instant::now();
    let mut last_tick = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        if start.elapsed() > timeout {
            return Err(Error::Engine(format!(
                "motor {} sn içinde yanıt vermedi\n{}",
                timeout.as_secs(),
                s.stderr_tail()
            )));
        }
        match s.rx.recv_timeout(Duration::from_millis(200)) {
            Ok(v) => {
                if let Some(r) = on_event(&v) {
                    return r;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                thread::sleep(Duration::from_millis(200));
                return Err(Error::Engine(format!(
                    "motor süreci beklenmedik biçimde kapandı\n{}",
                    s.stderr_tail()
                )));
            }
        }
        if last_tick.elapsed() >= Duration::from_millis(1000) {
            on_tick();
            last_tick = Instant::now();
        }
    }
}

/// Makes the process ready (spawning it and loading the model if needed). Returns the load time if a load happened.
fn prepare(
    slot: &mut Option<Process>,
    config: &EngineConfig,
    cancel: &AtomicBool,
    tick: &mut dyn FnMut(),
) -> Result<Option<f64>, Error> {
    let reusable = match slot.as_mut() {
        Some(s) => s.key == config.process_key() && s.is_running(),
        None => false,
    };
    if !reusable {
        if let Some(s) = slot.take() {
            s.shutdown();
        }
        *slot = Some(spawn(config)?);
    }
    let s = slot.as_mut().unwrap();
    s.idle_unload_s = config.idle_unload_s;
    if s.loaded {
        return Ok(None);
    }
    s.send(&json!({"cmd": "load", "model": config.model, "precision": config.precision, "device": config.device}))?;
    let mut seconds = None;
    let r = wait_for(
        s,
        cancel,
        Duration::from_secs(900),
        tick,
        &mut |v| match v["event"].as_str() {
            Some("ready") => {
                seconds = v["seconds"].as_f64();
                Some(Ok(()))
            }
            Some("error") => Some(Err(Error::Config(format!(
                "model yüklenemedi: {}",
                v["message"].as_str().unwrap_or("?")
            )))),
            _ => None,
        },
    );
    match r {
        Ok(()) => {
            s.loaded = true;
            Ok(seconds)
        }
        Err(e) => {
            if let Some(s) = slot.take() {
                s.shutdown();
            }
            Err(e)
        }
    }
}

/// Generates the requests in order. Intermediate files go to `work_dir/_engine` and are removed once the batch
/// succeeds.
pub fn run_sequence(
    config: &EngineConfig,
    requests: &[Request],
    work_dir: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
) -> Result<BatchResult, Error> {
    if requests.is_empty() {
        return Ok(BatchResult {
            clips: HashMap::new(),
            failed: vec![],
            reasons: HashMap::new(),
            seconds: 0.0,
            gpu_peak_mb: None,
            load_s: None,
        });
    }
    let work_dir = work_dir.join(ENGINE_DIR);
    let out_dir = work_dir.join("out");
    fs::create_dir_all(&out_dir)?;
    fs::create_dir_all(&config.cache_dir)?;
    let started = Instant::now();
    let total = requests.len();
    let mut slot = process_slot().lock().unwrap();
    // Peak GPU memory comes from the sidecar's reports (torch.mps.driver_allocated_memory). No child process (e.g.
    // ioreg) is spawned here: on macOS its pipe ends can leak into a concurrently spawned sidecar and block the reader.
    let gpu_peak = std::cell::Cell::new(None::<u64>);
    let completed = std::cell::Cell::new(0usize);
    let load_s = {
        let mut tick = || {
            progress(Progress {
                completed: 0,
                total,
                elapsed_s: started.elapsed().as_secs_f64(),
            })
        };
        prepare(&mut slot, config, cancel, &mut tick)?
    };
    let s = slot.as_mut().unwrap();
    let command = json!({
        "cmd": "generate", "requests": requests, "out_dir": out_dir, "cache_dir": config.cache_dir,
        "steps": config.steps, "cfg": config.cfg, "max_attempts": 3,
    });
    if let Err(e) = s.send(&command) {
        if let Some(s) = slot.take() {
            s.shutdown();
        }
        return Err(e);
    }
    let mut clips = HashMap::new();
    let mut reasons: HashMap<String, String> = HashMap::new();
    let mut pending_error: Option<Error> = None;
    let r = {
        let progress_rc = std::cell::RefCell::new(&mut *progress);
        let report = || {
            (progress_rc.borrow_mut())(Progress {
                completed: completed.get(),
                total,
                elapsed_s: started.elapsed().as_secs_f64(),
            })
        };
        let mut tick = || report();
        wait_for(
            s,
            cancel,
            Duration::from_secs(1800 + 120 * total as u64),
            &mut tick,
            &mut |v| match v["event"].as_str() {
                Some("request_done") => {
                    if let Some(mb) = v["gpu_mb"].as_u64() {
                        gpu_peak.set(Some(gpu_peak.get().unwrap_or(0).max(mb)));
                    }
                    let id = v["id"].as_str().unwrap_or("").to_string();
                    completed.set(completed.get() + 1);
                    report();
                    if v["complete"].as_bool() == Some(false) {
                        reasons.insert(
                            id,
                            format!(
                                "{} denemede de ses metne göre çok uzun çıktı (bozuk üretim)",
                                v["attempts"]
                            ),
                        );
                    } else {
                        match audio::read_wav(Path::new(v["wav"].as_str().unwrap_or(""))) {
                            Ok(a) if a.sample_rate == SAMPLE_RATE => {
                                clips.insert(id, limit_peak(a.samples));
                            }
                            Ok(a) => {
                                pending_error = Some(Error::Engine(format!(
                                    "beklenmeyen örnekleme {}",
                                    a.sample_rate
                                )))
                            }
                            Err(e) => pending_error = Some(e),
                        }
                    }
                    None
                }
                Some("request_failed") => {
                    completed.set(completed.get() + 1);
                    report();
                    reasons.insert(
                        v["id"].as_str().unwrap_or("").to_string(),
                        v["message"].as_str().unwrap_or("?").to_string(),
                    );
                    None
                }
                Some("batch_done") => Some(Ok(())),
                Some("cancelled") => Some(Err(Error::Cancelled)),
                Some("error") => Some(Err(Error::Engine(
                    v["message"].as_str().unwrap_or("?").to_string(),
                ))),
                _ => None,
            },
        )
    };
    let gpu_peak_mb = gpu_peak.get();
    if let Err(e) = r {
        // cancelled: stop the engine mid-batch; if it does not answer, kill the process (the next job reloads it)
        let s = slot.as_mut().unwrap();
        let mut clean = false;
        if matches!(e, Error::Cancelled) && s.send(&json!({"cmd": "cancel"})).is_ok() {
            let never = AtomicBool::new(false);
            clean = wait_for(
                s,
                &never,
                Duration::from_secs(45),
                &mut || {},
                &mut |v| match v["event"].as_str() {
                    Some("cancelled") | Some("batch_done") => Some(Ok(())),
                    _ => None,
                },
            )
            .is_ok();
        }
        if !clean {
            if let Some(s) = slot.take() {
                s.shutdown();
            }
        } else {
            slot.as_mut().unwrap().last_used = Instant::now();
        }
        return Err(e);
    }
    if let Some(e) = pending_error {
        return Err(e);
    }
    slot.as_mut().unwrap().last_used = Instant::now();
    drop(slot);
    progress(Progress {
        completed: completed.get(),
        total,
        elapsed_s: started.elapsed().as_secs_f64(),
    });
    let failed: Vec<String> = requests
        .iter()
        .filter(|r| !clips.contains_key(&r.id))
        .map(|r| r.id.clone())
        .collect();
    for id in &failed {
        reasons
            .entry(id.clone())
            .or_insert_with(|| "motor bu isteğe yanıt vermedi".into());
    }
    if failed.is_empty() && !config.keep_intermediate {
        let _ = fs::remove_dir_all(&work_dir);
    }
    Ok(BatchResult {
        clips,
        failed,
        reasons,
        seconds: (started.elapsed().as_secs_f64() * 10.0).round() / 10.0,
        gpu_peak_mb,
        load_s,
    })
}

/// Peak limiter: without a reference, design output can come very close to full scale; output above 0.97 is scaled to
/// 0.94 (so 16-bit writing does not clip). Quieter output is left alone.
pub fn limit_peak(mut x: Vec<f32>) -> Vec<f32> {
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak > 0.97 {
        let k = 0.94 / peak;
        x.iter_mut().for_each(|v| *v *= k);
    }
    x
}

/// Converts a FLAC or other-format reference to WAV for the engine (lossless, same rate).
pub fn reference_wav(reference: &Path, work_dir: &Path) -> Result<PathBuf, Error> {
    if reference
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("wav"))
        .unwrap_or(false)
    {
        return Ok(reference.to_path_buf());
    }
    let decoded = audio::load(reference)?;
    fs::create_dir_all(work_dir)?;
    let stem = reference
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("reference");
    let target = work_dir.join(format!("{stem}.wav"));
    audio::write_wav(&target, &decoded, None)?;
    Ok(target)
}

/// Wraps engine-rate samples as `Audio`.
pub fn to_audio(samples: Vec<f32>) -> Audio {
    Audio {
        samples,
        sample_rate: SAMPLE_RATE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_limiter() {
        assert_eq!(limit_peak(vec![0.5, -0.9]), vec![0.5, -0.9]);
        let y = limit_peak(vec![0.2, -1.2]);
        assert!((y[1] + 0.94).abs() < 1e-6 && (y[0] - 0.2 * 0.94 / 1.2).abs() < 1e-6);
    }

    #[test]
    fn request_format() {
        let r = design_request("t_1", " A calm voice ", " Merhaba. ", 7);
        assert_eq!(r.text, "(A calm voice)Merhaba.");
        assert_eq!(r.mode, "design");
        let v = serde_json::to_value(&r).unwrap();
        assert!(v.get("reference").is_none());
        let o = read_request(
            "s1",
            "Merhaba.",
            42,
            Path::new("/x/r.wav"),
            " Kayıt metni. ",
        );
        assert_eq!(
            (o.mode.as_str(), o.reference_text.as_deref()),
            ("ultimate", Some("Kayıt metni."))
        );
        assert_eq!(
            read_request("s1", "a", 1, Path::new("/r.wav"), "  ").mode,
            "reference"
        );
    }
}
