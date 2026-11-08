//! Persistent job queue: one generation on the GPU at a time. Jobs live in SQLite; queued jobs survive an app exit,
//! and a job that was running when the app closed becomes 'interrupted' at the next start.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::json;

use crate::engine::{EngineConfig, Progress};
use crate::jobs::{self, Context, JobRequest};
use crate::store::Job;
use crate::studio::Shared;
use crate::{estimate, Error, ErrorResponse};

/// Events sent to the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Job {
        id: String,
        kind: String,
        status: String,
        completed: usize,
        total: usize,
        elapsed_s: f64,
        estimated_s: Option<f64>,
        message: Option<String>,
        parent_id: Option<String>,
    },
    Model(crate::model::DownloadStatus),
    EngineEnv(crate::model::DownloadStatus),
    Whisper(crate::model::DownloadStatus),
    Library,
    Queue,
}

pub type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

/// Request key holding the character count used for duration estimates.
pub const CHARS_KEY: &str = "_chars";

#[derive(Default)]
pub struct QueueState {
    pub running: Option<String>,
    pub cancel_flags: HashMap<String, Arc<AtomicBool>>,
}

pub struct Queue {
    pub state: Mutex<QueueState>,
    pub wake: Condvar,
}

impl Queue {
    pub fn new() -> Self {
        Queue {
            state: Mutex::new(QueueState::default()),
            wake: Condvar::new(),
        }
    }

    pub fn nudge(&self) {
        self.wake.notify_all();
    }

    pub fn cancel(&self, id: &str) -> bool {
        let s = self.state.lock().unwrap();
        if let Some(flag) = s.cancel_flags.get(id) {
            flag.store(true, Ordering::Relaxed);
            return true;
        }
        false
    }

    pub fn is_busy(&self) -> bool {
        self.state.lock().unwrap().running.is_some()
    }
}

impl Default for Queue {
    fn default() -> Self {
        Self::new()
    }
}

/// Worker loop (on its own thread): polls every 3 s or when nudged.
pub fn run_worker(shared: Arc<Shared>) {
    loop {
        let next = shared.store.next_queued().unwrap_or_default();
        let Some(job) = next else {
            let guard = shared.queue.state.lock().unwrap();
            let _ = shared
                .queue
                .wake
                .wait_timeout(guard, Duration::from_secs(3))
                .unwrap();
            continue;
        };
        run_one(&shared, job);
    }
}

fn estimate_kind(job_kind: &str) -> &'static str {
    match job_kind {
        "design" => "design",
        "transcribe" | "review" => "transcribe",
        _ => "read",
    }
}

fn run_one(shared: &Arc<Shared>, job: Job) {
    let id = job.id.clone();
    if !matches!(shared.store.claim(&id), Ok(true)) {
        // another process took it; wait briefly and move on
        std::thread::sleep(Duration::from_millis(500));
        return;
    }
    let flag = Arc::new(AtomicBool::new(false));
    {
        let mut s = shared.queue.state.lock().unwrap();
        s.running = Some(id.clone());
        s.cancel_flags.insert(id.clone(), flag.clone());
    }
    let total = job.total.max(1) as usize;
    let emit =
        |status: &str, completed: usize, total: usize, elapsed: f64, message: Option<String>| {
            (shared.events)(Event::Job {
                id: id.clone(),
                kind: job.kind.clone(),
                status: status.into(),
                completed,
                total,
                elapsed_s: elapsed,
                estimated_s: job.estimated_s,
                message,
                parent_id: job.parent_id.clone(),
            })
        };
    emit(
        "running",
        0,
        total,
        0.0,
        Some("motor başlatılıyor, model yükleniyor".into()),
    );
    let started = Instant::now();
    let result = execute(shared, &job, &flag, &mut |p: Progress| {
        let _ = shared
            .store
            .set_progress(&id, p.completed as i64, p.total as i64);
        emit(
            "running",
            p.completed,
            p.total,
            started.elapsed().as_secs_f64(),
            None,
        );
    });
    let elapsed = started.elapsed().as_secs_f64();
    match result {
        Ok(r) => {
            let folder = r.folder.as_ref().map(|k| k.to_string_lossy().to_string());
            let _ = shared.store.finish(
                &id,
                &r.result,
                folder.as_deref(),
                r.audio_s,
                Some((elapsed * 10.0).round() / 10.0),
                r.gpu_peak_mb,
                r.voice_id.as_deref(),
            );
            if let Some(parent) = &job.parent_id {
                let _ = shared.store.update_result(
                    parent,
                    &r.result["parent_result"],
                    r.result["audio_s"].as_f64(),
                );
            }
            let label = shared.engine_label();
            let chars = job
                .request
                .get(CHARS_KEY)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            let _ = estimate::record(
                &shared.store,
                &format!("{}:{label}", estimate_kind(&job.kind)),
                chars,
                &r.steps,
                elapsed,
                job.voice_id.as_deref().filter(|_| job.kind == "read"),
            );
            emit("done", total, total, elapsed, None);
            // design studies stay outside the library (`add_design_variant` emits the event)
            if r.voice_id.is_some() && job.kind == "clone" {
                (shared.events)(Event::Library);
            }
        }
        Err(Error::Cancelled) => {
            let _ = shared.store.fail(&id, "cancelled", "kullanıcı iptal etti");
            emit("cancelled", 0, total, elapsed, Some("iptal edildi".into()));
        }
        Err(e) => {
            let response = ErrorResponse::from(&e);
            let _ = shared.store.fail(&id, "failed", &response.message);
            emit("failed", 0, total, elapsed, Some(response.message));
        }
    }
    {
        let mut s = shared.queue.state.lock().unwrap();
        s.running = None;
        s.cancel_flags.remove(&id);
    }
    (shared.events)(Event::Queue);
}

fn execute(
    shared: &Arc<Shared>,
    job: &Job,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
) -> Result<jobs::JobOutput, Error> {
    let mut request_json = job.request.clone();
    if let Some(o) = request_json.as_object_mut() {
        o.remove(CHARS_KEY);
    }
    let request: JobRequest = serde_json::from_value(request_json)?;
    // transcription and review do not need the voice engine
    let engine = match (&request, shared.engine_config()) {
        (_, Ok(m)) => m,
        (JobRequest::Transcribe { .. } | JobRequest::Review { .. }, Err(_)) => EngineConfig {
            python: Default::default(),
            script: Default::default(),
            model: Default::default(),
            device: String::new(),
            precision: String::new(),
            steps: crate::engine::DEFAULT_STEPS,
            cfg: crate::engine::DEFAULT_CFG,
            cache_dir: shared.paths.cache(),
            idle_unload_s: 0,
            word_timing: false,
            tone_match: false,
            keep_intermediate: false,
        },
        (_, Err(e)) => return Err(e),
    };
    let dictionary = shared.dictionary()?;
    let ctx = Context {
        paths: &shared.paths,
        engine,
        dictionary,
        cancel,
    };
    match request {
        JobRequest::Design {
            name,
            description,
            text,
            variants,
            seeds,
            preview_text,
            ..
        } => {
            let language = job.language.clone().unwrap_or_else(|| "tr".into());
            jobs::design(
                &ctx,
                &job.id,
                &name,
                &description,
                &text,
                &language,
                variants,
                seeds.as_ref(),
                preview_text.as_deref(),
                progress,
            )
        }
        JobRequest::Read {
            voice,
            text,
            language,
            ..
        } => jobs::read(
            &ctx,
            &job.id,
            &voice,
            &text,
            language.as_deref().or(job.language.as_deref()),
            &job.title,
            progress,
        ),
        JobRequest::Clone {
            recording,
            recording_text,
            text,
            seed,
            consent,
            library_name,
        } => {
            let r = jobs::clone(
                &ctx,
                &job.id,
                Path::new(&recording),
                &recording_text,
                &text,
                seed,
                consent,
                library_name.as_deref(),
                progress,
            );
            // the copy of the recording (when it is in the app's work folder) never stays
            if Path::new(&recording).starts_with(shared.paths.work()) {
                let _ = std::fs::remove_file(&recording);
            }
            r
        }
        JobRequest::Line {
            job_id,
            line,
            text,
            seed,
        } => {
            let parent = shared
                .store
                .job(&job_id)?
                .ok_or_else(|| Error::NotFound(job_id.clone()))?;
            jobs::regenerate_line(&ctx, &parent, line, text.as_deref(), seed, progress)
        }
        JobRequest::Transcribe { file, language } => {
            let language = language.filter(|l| !l.is_empty() && l != "auto");
            jobs::transcribe(
                &ctx,
                &job.id,
                Path::new(&file),
                language.as_deref(),
                progress,
            )
        }
        JobRequest::Review { job_id } => {
            let parent = shared
                .store
                .job(&job_id)?
                .ok_or_else(|| Error::NotFound(job_id.clone()))?;
            jobs::review(&ctx, &parent, progress)
        }
        JobRequest::Timed {
            voice,
            cues,
            speaker_voices,
            max_speedup,
            video,
            original_audio,
            title,
            ..
        } => {
            let language = job.language.clone().unwrap_or_else(|| "tr".into());
            jobs::timed(
                &ctx,
                &job.id,
                &voice,
                &cues,
                &speaker_voices,
                &language,
                max_speedup.unwrap_or(1.25).clamp(1.0, 1.6),
                video.as_deref().filter(|v| !v.is_empty()).map(Path::new),
                original_audio.as_deref().unwrap_or("duck"),
                title.as_deref().unwrap_or(&job.title),
                progress,
            )
        }
    }
}

/// Adds a request to the queue (validation + summary + duration estimate).
pub fn enqueue(shared: &Arc<Shared>, request: JobRequest) -> Result<Job, Error> {
    let summary = jobs::summarize_request(&shared.paths, &request, &shared.store)?;
    let kind = match &request {
        JobRequest::Design { .. } => "design",
        JobRequest::Read { .. } => "read",
        JobRequest::Clone { .. } => "clone",
        JobRequest::Line { .. } => "line",
        JobRequest::Transcribe { .. } => "transcribe",
        JobRequest::Review { .. } => "review",
        JobRequest::Timed { .. } => "timed",
    };
    let parent_id = match &request {
        JobRequest::Line { job_id, .. } | JobRequest::Review { job_id } => Some(job_id.clone()),
        _ => None,
    };
    let mut request_json = serde_json::to_value(&request)?;
    request_json[CHARS_KEY] = json!(summary.chars);
    let label = shared.engine_label();
    let est = estimate::estimate(
        &shared.store,
        &format!("{}:{label}", summary.estimate_kind),
        summary.chars,
        summary.voice_id.as_deref().filter(|_| kind == "read"),
    );
    let job = Job {
        id: crate::new_id(),
        kind: kind.into(),
        status: "queued".into(),
        created_at: crate::now_string(),
        started_at: None,
        finished_at: None,
        title: summary.title,
        voice_id: summary.voice_id,
        voice_name: summary.voice_name,
        language: summary.language,
        text: summary.text,
        request: request_json,
        result: None,
        folder: None,
        audio_s: None,
        duration_s: None,
        gpu_peak_mb: None,
        error: None,
        starred: false,
        notes: String::new(),
        completed: 0,
        total: summary.total as i64,
        estimated_s: Some(est.seconds),
        parent_id,
    };
    shared.store.insert_job(&job)?;
    shared.queue.nudge();
    (shared.events)(Event::Queue);
    Ok(job)
}
