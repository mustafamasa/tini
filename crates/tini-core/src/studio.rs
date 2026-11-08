//! The single entry point for the UI.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use serde_json::{json, Value};

use crate::engine::EngineConfig;
use crate::jobs::{self, JobRequest};
use crate::queue::{self, Event, EventSink, Queue};
use crate::settings::{self, Binaries, Paths, Settings};
use crate::store::{Draft, Job, JobFilter, Store};
use crate::text::{self, Dictionary, Issue};
use crate::voices::{self, TrashEntry, VoiceCard};
use crate::{estimate, model, voice_prompts, Error};

/// Env var: when set, the studio only enqueues (no worker thread, no crash recovery); the open app runs the jobs.
pub const NO_WORKER_ENV: &str = "TINI_NO_WORKER";

const MODEL_FINGERPRINT_KEY: &str = "model_fingerprint";

/// Shared state (also used by the worker thread).
pub struct Shared {
    pub paths: Paths,
    pub store: Store,
    pub settings: RwLock<Settings>,
    pub queue: Queue,
    pub events: EventSink,
    pub model_cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub env_cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub whisper_cancel: Mutex<Option<Arc<AtomicBool>>>,
    /// local HTTP API server (see `api`)
    pub api: Mutex<crate::api::ApiState>,
}

impl Shared {
    /// Engine label used in duration-estimate keys.
    pub fn engine_label(&self) -> String {
        self.settings.read().unwrap().engine_label()
    }

    pub fn model_path(&self) -> PathBuf {
        self.settings
            .read()
            .unwrap()
            .model_path
            .clone()
            .filter(|p| model::is_ready(p))
            .unwrap_or_else(|| self.paths.downloaded_model())
    }

    pub fn model_ready(&self) -> bool {
        model::is_ready(&self.model_path())
    }

    pub fn engine_config(&self) -> Result<EngineConfig, Error> {
        let s = self.settings.read().unwrap().clone();
        match crate::engine_env::status(&self.paths.data_dir, &self.paths.requirements()) {
            "ready" => {}
            "outdated" => {
                return Err(Error::Config(
                    "ses motoru ortamı bu sürüm için güncellenmeli; Kurulum ekranından onarın"
                        .into(),
                ))
            }
            _ => {
                return Err(Error::Config(
                    "ses motoru ortamı kurulmamış; Kurulum ekranından kurun".into(),
                ))
            }
        }
        if !self.model_ready() {
            return Err(Error::Config(
                "VoxCPM2 modeli yok ya da eksik; Kurulum ekranından indirin".into(),
            ));
        }
        Ok(EngineConfig {
            python: crate::engine_env::python(&self.paths.data_dir),
            script: self.paths.engine_script(),
            model: self.model_path(),
            device: s.device.clone(),
            precision: s.precision.clone(),
            steps: s.steps,
            cfg: s.cfg,
            cache_dir: self.paths.cache(),
            idle_unload_s: s.idle_unload_min as u64 * 60,
            word_timing: s.word_timing,
            tone_match: s.tone_match,
            keep_intermediate: s.keep_intermediate,
        })
    }

    pub fn dictionary(&self) -> Result<Dictionary, Error> {
        let text = std::fs::read_to_string(self.paths.dictionary()).unwrap_or_default();
        Dictionary::parse(&text).map_err(|s| Error::Issues(vec![s]))
    }
}

/// Cheap to clone: every clone shares the same state (the API server threads hold one).
#[derive(Clone)]
pub struct Studio {
    pub shared: Arc<Shared>,
}

fn is_auto(language: &str) -> bool {
    language.is_empty() || language == "auto"
}

impl Studio {
    /// Opens the studio at the user's data dir and starts the queue worker.
    pub fn open(
        resources: PathBuf,
        binaries: Binaries,
        events: EventSink,
    ) -> Result<Studio, Error> {
        Self::open_at(settings::data_dir(), resources, binaries, events)
    }

    pub fn open_at(
        data_dir: PathBuf,
        resources: PathBuf,
        binaries: Binaries,
        events: EventSink,
    ) -> Result<Studio, Error> {
        let engine = binaries.engine.unwrap_or_else(|| resources.join("engine"));
        let paths = Paths {
            data_dir,
            resources,
            uv: binaries.uv,
            engine,
            whisper: binaries.whisper,
        };
        paths.prepare()?;
        let store = Store::open(&paths.database())?;
        let worker = std::env::var_os(NO_WORKER_ENV).is_none();
        // only the process that runs jobs recovers interrupted ones; enqueue-only mode leaves the open app's jobs alone
        if worker {
            store.recover_interrupted()?;
        }
        let mut settings = Settings::load(&paths.settings_file());
        if settings.api_token.is_empty() {
            settings.api_token = settings::new_api_token();
            let _ = settings.save(&paths.settings_file());
        }
        let shared = Arc::new(Shared {
            paths,
            store,
            settings: RwLock::new(settings),
            queue: Queue::new(),
            events,
            model_cancel: Mutex::new(None),
            env_cancel: Mutex::new(None),
            whisper_cancel: Mutex::new(None),
            api: Mutex::new(Default::default()),
        });
        if worker {
            let s = shared.clone();
            std::thread::Builder::new()
                .name("tini-queue".into())
                .spawn(move || queue::run_worker(s))?;
        }
        Ok(Studio { shared })
    }

    fn emit(&self, e: Event) {
        (self.shared.events)(e)
    }

    // --- status and settings ------------------------------------------------------------------------------------------

    pub fn status(&self) -> Value {
        let sh = &self.shared;
        let model_path = sh.model_path();
        let size = model::present_size(&model_path);
        let verified = sh
            .store
            .kv(MODEL_FINGERPRINT_KEY)
            .ok()
            .flatten()
            .and_then(|v| v.as_str().map(String::from))
            == model::fingerprint(&model_path);
        let env = crate::engine_env::status(&sh.paths.data_dir, &sh.paths.requirements());
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "data_dir": sh.paths.data_dir,
            "resources": sh.paths.resources,
            "engine": {"name": "VoxCPM", "script": sh.paths.engine_script(), "present": sh.paths.engine_script().is_file(),
                       "uv": sh.paths.uv.as_ref().map(|u| u.is_file()).unwrap_or(false),
                       "env": env, "env_ready": env == "ready",
                       "env_size": crate::engine_env::ESTIMATED_SIZE, "loaded": crate::engine::is_loaded()},
            "model": {"path": model_path, "present": size > 0, "ready": sh.model_ready(), "verified": verified,
                      "size": size, "expected": model::total_size(), "commit": model::COMMIT, "repo": model::REPO,
                      "partial": if sh.model_ready() { 0 } else { size }},
            "engine_label": sh.engine_label(),
            "whisper": {"binary_present": sh.paths.whisper.as_ref().map(|w| w.is_file()).unwrap_or(false),
                        "model_ready": crate::transcribe::model_ready(&sh.paths.data_dir),
                        "size": crate::transcribe::MODEL_SIZE},
            "ffmpeg": crate::subtitles::find_ffmpeg(),
            "system": settings::system_info(),
            "stats": sh.store.stats().unwrap_or(Value::Null),
        })
    }

    pub fn settings(&self) -> Settings {
        self.shared.settings.read().unwrap().clone()
    }

    pub fn save_settings(&self, mut s: Settings) -> Result<Settings, Error> {
        s.validate()?;
        let old = self.shared.settings.read().unwrap().clone();
        if s.api_token.trim().is_empty() {
            // the token is changed only with `regenerate_api_token`
            s.api_token = old.api_token.clone();
        }
        if (old.device != s.device || old.precision != s.precision) && !self.shared.queue.is_busy()
        {
            // load settings changed: release the engine in memory (the next job loads with the new settings)
            std::thread::spawn(crate::engine::shutdown);
        }
        s.save(&self.shared.paths.settings_file())?;
        *self.shared.settings.write().unwrap() = s.clone();
        crate::api::apply(self);
        Ok(s)
    }

    // --- local HTTP API -----------------------------------------------------------------------------------------------

    /// Starts the local HTTP API when enabled in settings; from then on it follows `save_settings`. A busy port does
    /// not fail: it is reported in `api_info().error`.
    pub fn start_api(&self) {
        crate::api::start(self)
    }

    /// Stops the API server (app exit).
    pub fn stop_api(&self) {
        crate::api::stop(self)
    }

    pub fn api_info(&self) -> crate::api::ApiInfo {
        crate::api::info(self)
    }

    /// Sends a request to the running local API over HTTP (the settings screen's example request).
    pub fn call_local_api(
        &self,
        method: &str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<crate::api::LocalCall, Error> {
        crate::api::call_local(self, method, path, body)
    }

    /// Replaces the API token (old token stops working immediately).
    pub fn regenerate_api_token(&self) -> Result<crate::api::ApiInfo, Error> {
        {
            let mut s = self.shared.settings.write().unwrap();
            let mut next = s.clone();
            next.api_token = settings::new_api_token();
            next.save(&self.shared.paths.settings_file())?;
            *s = next;
        }
        Ok(self.api_info())
    }

    pub fn set_data_dir(&self, dir: &Path) -> Result<(), Error> {
        std::fs::create_dir_all(dir)?;
        settings::set_data_dir(dir)
    }

    /// Unloads the engine from memory (when no job is running).
    pub fn unload_engine(&self) -> Result<(), Error> {
        if self.shared.queue.is_busy() {
            return Err(Error::Input("bir iş çalışıyor; bitince boşaltın".into()));
        }
        crate::engine::shutdown();
        Ok(())
    }

    pub fn clear_cache(&self) -> Result<u64, Error> {
        let mut freed = 0;
        for dir in [self.shared.paths.cache(), self.shared.paths.work()] {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten() {
                    freed += disk_size(&e.path());
                    let _ = if e.path().is_dir() {
                        std::fs::remove_dir_all(e.path())
                    } else {
                        std::fs::remove_file(e.path())
                    };
                }
            }
        }
        Ok(freed)
    }

    // --- voice library ------------------------------------------------------------------------------------------------

    pub fn voices(&self) -> Result<Vec<VoiceCard>, Error> {
        voices::list(&self.shared.paths, &self.shared.store)
    }

    pub fn manage_voice(&self, id: &str, action: &str, name: Option<&str>) -> Result<(), Error> {
        voices::manage(&self.shared.paths, &self.shared.store, id, action, name)?;
        self.emit(Event::Library);
        Ok(())
    }

    pub fn trash(&self) -> Result<Vec<TrashEntry>, Error> {
        voices::list_trash(&self.shared.paths)
    }

    pub fn export_voice(&self, id: &str, target: &Path) -> Result<PathBuf, Error> {
        voices::export(&self.shared.paths, id, target)
    }

    pub fn import_voice(&self, source: &Path) -> Result<String, Error> {
        let id = voices::import(&self.shared.paths, source)?;
        self.emit(Event::Library);
        Ok(id)
    }

    pub fn voice_prompts(&self) -> Result<Vec<voice_prompts::VoicePrompt>, Error> {
        voice_prompts::list(&self.shared.paths.resources.join("voice_prompts"))
    }

    // --- text ---------------------------------------------------------------------------------------------------------

    fn voice_language(&self, voice: Option<&str>) -> Option<String> {
        voice
            .and_then(|s| voices::find(&self.shared.paths, s).ok())
            .and_then(|f| voices::read_card(&f).ok())
            .and_then(|c| c["language"].as_str().map(String::from))
    }

    fn resolve_language(
        &self,
        language: Option<&str>,
        body: &str,
        fallback: Option<&str>,
    ) -> String {
        match language.filter(|l| !is_auto(l)) {
            Some(l) => text::language(Some(l)).unwrap_or_else(|_| "tr".into()),
            None => text::detect(body, fallback),
        }
    }

    /// Live check of a reading text: language, issues, inline findings, duration estimate, dictionary matches.
    pub fn check_text(&self, body: &str, language: Option<&str>, voice: Option<&str>) -> Value {
        let card_language = self.voice_language(voice);
        let language = self.resolve_language(language, body, card_language.as_deref());
        let lines = text::lines(body).unwrap_or_default();
        let dictionary = self.shared.dictionary().unwrap_or_default();
        let matches: Vec<Value> = if language == "tr" {
            body.lines()
                .enumerate()
                .flat_map(|(i, l)| {
                    dictionary.matches(l).into_iter().map(move |(c, n, s)| {
                        json!({"line": i + 1, "column": c, "length": n, "pronunciation": s})
                    })
                })
                .collect()
        } else {
            vec![]
        };
        let audio_s: f64 = lines
            .iter()
            .map(|(l, _)| text::estimate_seconds(l, Some(&language)))
            .sum::<f64>()
            + lines
                .iter()
                .take(lines.len().saturating_sub(1))
                .map(|(_, p)| *p)
                .sum::<f64>();
        let chars = estimate::chars(body, 1);
        let est = estimate::estimate(
            &self.shared.store,
            &format!("read:{}", self.shared.engine_label()),
            chars,
            voice,
        );
        json!({
            "language": language,
            "issues": text::problems(body, "text", "metin", &language),
            "findings": text::line_diagnostics(body, &language),
            "line_count": lines.len(),
            "audio_s": (audio_s * 10.0).round() / 10.0,
            "job_s": est.seconds,
            "estimate_samples": est.samples,
            "dictionary": matches,
        })
    }

    /// Live check of the design form: language, design-text duration and limits, issues, job estimate.
    pub fn check_design(
        &self,
        description: &str,
        design_text: &str,
        language: Option<&str>,
        variants: usize,
        preview_text: Option<&str>,
    ) -> Value {
        let language = self.resolve_language(language, design_text, None);
        let mut chars = estimate::chars(design_text, variants);
        if let Some(p) = preview_text {
            chars += estimate::chars(p, variants);
        }
        let est = estimate::estimate(
            &self.shared.store,
            &format!("design:{}", self.shared.engine_label()),
            chars,
            None,
        );
        let issues: Vec<Issue> = if description.is_empty() && design_text.is_empty() {
            vec![]
        } else {
            jobs::check_design(description, design_text, &language)
        };
        json!({
            "language": language,
            "seconds": text::estimate_seconds(design_text, Some(&language)),
            "limit": jobs::DESIGN_LIMIT_SECONDS,
            "hard_limit": jobs::DESIGN_HARD_LIMIT_SECONDS,
            "issues": issues,
            "job_s": est.seconds,
        })
    }

    /// Makes the text readable (numbers, abbreviations, citations, parentheses, quotes, sentences).
    pub fn fix_text(
        &self,
        body: &str,
        language: Option<&str>,
        voice: Option<&str>,
        options: &crate::text_fix::FixOptions,
    ) -> Value {
        let card_language = self.voice_language(voice);
        let language = self.resolve_language(language, body, card_language.as_deref());
        let r = crate::text_fix::fix_text(body, &language, options);
        json!({
            "language": language,
            "remaining": text::line_diagnostics(&r.text, &language),
            "text": r.text,
            "changes": r.changes,
        })
    }

    pub fn read_dictionary(&self) -> String {
        std::fs::read_to_string(self.shared.paths.dictionary()).unwrap_or_default()
    }

    pub fn save_dictionary(&self, content: &str) -> Result<usize, Error> {
        let d = Dictionary::parse(content).map_err(|s| Error::Issues(vec![s]))?;
        std::fs::write(self.shared.paths.dictionary(), content)?;
        Ok(d.len())
    }

    pub fn preview_pronunciation(&self, body: &str, language: &str) -> String {
        text::spoken(
            body,
            language,
            &self.shared.dictionary().unwrap_or_default(),
        )
    }

    // --- jobs ---------------------------------------------------------------------------------------------------------

    pub fn enqueue_job(&self, request: JobRequest) -> Result<Job, Error> {
        let request = match request {
            JobRequest::Clone {
                recording,
                recording_text,
                text,
                seed,
                consent,
                library_name,
            } => {
                // a copy of the recording goes to the app's work folder (so moving the file does not break the job);
                // the copy is deleted when the job ends
                if !consent {
                    return Err(Error::Issues(vec![Issue::new(
                        "consent_missing",
                        jobs::CONSENT_MISSING,
                    )
                    .with("field", "consent")]));
                }
                let src = PathBuf::from(&recording);
                let ext = src
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("wav")
                    .to_string();
                let copy = self
                    .shared
                    .paths
                    .work()
                    .join(format!("clone_input_{}.{ext}", crate::new_id()));
                if src.is_file() {
                    std::fs::copy(&src, &copy)?;
                }
                JobRequest::Clone {
                    recording: copy.to_string_lossy().into(),
                    recording_text,
                    text,
                    seed,
                    consent,
                    library_name,
                }
            }
            other => other,
        };
        let copied = match &request {
            JobRequest::Clone { recording, .. } => Some(PathBuf::from(recording)),
            _ => None,
        };
        let result = queue::enqueue(&self.shared, request);
        if result.is_err() {
            // a request rejected by validation never runs, so its recording copy would otherwise stay behind
            if let Some(copy) = copied {
                let _ = std::fs::remove_file(copy);
            }
        }
        result
    }

    pub fn cancel_job(&self, id: &str) -> Result<(), Error> {
        if self.shared.queue.cancel(id) {
            return Ok(());
        }
        if let Some(j) = self.shared.store.job(id)? {
            if j.status == "queued" {
                self.shared
                    .store
                    .fail(id, "cancelled", "kuyruktayken iptal edildi")?;
                if let Some(rec) = j.request["recording"]
                    .as_str()
                    .filter(|r| Path::new(r).starts_with(self.shared.paths.work()))
                {
                    let _ = std::fs::remove_file(rec);
                }
                self.emit(Event::Queue);
            }
        }
        Ok(())
    }

    /// Re-queues a finished or failed job with the same request.
    pub fn rerun_job(&self, id: &str) -> Result<Job, Error> {
        let j = self
            .shared
            .store
            .job(id)?
            .ok_or_else(|| Error::NotFound(id.into()))?;
        if j.kind == "clone" {
            return Err(Error::Input(
                "klon kaydı saklanmadığı için yeniden çalıştırılamaz; kaydı yeniden seçin".into(),
            ));
        }
        let mut request = j.request.clone();
        if let Some(o) = request.as_object_mut() {
            o.remove(queue::CHARS_KEY);
        }
        self.enqueue_job(serde_json::from_value(request)?)
    }

    pub fn queue(&self) -> Result<Vec<Job>, Error> {
        self.shared.store.queue()
    }

    pub fn jobs(&self, filter: &JobFilter) -> Result<Vec<Job>, Error> {
        self.shared.store.jobs(filter)
    }

    pub fn job(&self, id: &str) -> Result<Option<Job>, Error> {
        let mut j = self.shared.store.job(id)?;
        // words are not kept in summary.json but are in the result; fill them from words.json for older records
        if let Some(x) = j.as_mut() {
            if let (Some(r), Some(folder)) = (x.result.as_mut(), x.folder.clone()) {
                if r.get("words").map(|v| v.is_null()).unwrap_or(true) {
                    if let Ok(t) = std::fs::read_to_string(Path::new(&folder).join("words.json")) {
                        r["words"] = serde_json::from_str(&t).unwrap_or(Value::Null);
                    }
                }
            }
        }
        Ok(j)
    }

    pub fn update_job(
        &self,
        id: &str,
        title: Option<&str>,
        starred: Option<bool>,
        notes: Option<&str>,
    ) -> Result<(), Error> {
        self.shared.store.update_job(id, title, starred, notes)
    }

    /// Deletes the record; with `delete_files` also the job folder (a design study's folder; voices already added to
    /// the library stay).
    pub fn delete_job(&self, id: &str, delete_files: bool) -> Result<(), Error> {
        let j = self
            .shared
            .store
            .job(id)?
            .ok_or_else(|| Error::NotFound(id.into()))?;
        if j.status == "running" || j.status == "queued" {
            return Err(Error::Input("önce işi iptal edin".into()));
        }
        if delete_files && j.kind == "design" {
            self.remove_design_folder(id);
        }
        if delete_files && j.kind != "design" && j.kind != "line" {
            if let Some(folder) = &j.folder {
                let p = PathBuf::from(folder);
                if p.starts_with(self.shared.paths.jobs()) {
                    let _ = std::fs::remove_dir_all(p);
                }
            }
        }
        self.shared.store.delete_job(id)
    }

    // --- design studies -----------------------------------------------------------------------------------------------

    fn remove_design_folder(&self, job_id: &str) {
        if job_id.is_empty() || job_id.contains(['/', '\\']) || job_id.contains("..") {
            return;
        }
        let dir = self.shared.paths.designs().join(job_id);
        if dir.is_dir() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// Removes a design study: its folder under `designs/` and the job record. Voices already added to the library
    /// stay.
    pub fn delete_design(&self, job_id: &str) -> Result<(), Error> {
        let j = self
            .shared
            .store
            .job(job_id)?
            .ok_or_else(|| Error::NotFound(job_id.into()))?;
        if j.kind != "design" {
            return Err(Error::Input("bu bir ses tasarımı değil".into()));
        }
        self.delete_job(job_id, true)
    }

    /// Adds one variant of a design study to the library as `voices/<slug(name)>/variant_N` (name defaults to the
    /// study's name). Adding the same variant again returns the voice already added.
    pub fn add_design_variant(
        &self,
        job_id: &str,
        variant: u32,
        name: Option<&str>,
    ) -> Result<VoiceCard, Error> {
        static ADDING: Mutex<()> = Mutex::new(());
        let _guard = ADDING.lock().unwrap_or_else(|e| e.into_inner());
        let paths = &self.shared.paths;
        let store = &self.shared.store;
        let job = store
            .job(job_id)?
            .ok_or_else(|| Error::NotFound(format!("tasarım yok: {job_id}")))?;
        if job.kind != "design" {
            return Err(Error::Input("bu bir ses tasarımı değil".into()));
        }
        let mut result = job
            .result
            .clone()
            .filter(|_| job.status == "done")
            .ok_or_else(|| Error::Input("tasarım henüz bitmedi".into()))?;
        let index = result["variants"]
            .as_array()
            .and_then(|vs| {
                vs.iter()
                    .position(|v| v["variant"].as_u64() == Some(variant as u64))
            })
            .ok_or_else(|| Error::NotFound(format!("çeşit yok: {variant}")))?;
        let entry = result["variants"][index].clone();
        // already added (or a study from before design studies, whose variants were written to the library directly)
        for key in ["library_voice_id", "id"] {
            if let Some(existing) = entry[key].as_str() {
                if let Ok(card) = voices::get_card(paths, store, existing) {
                    if key == "id" {
                        result["variants"][index]["library_voice_id"] = json!(existing);
                        store.update_result(job_id, &result, None)?;
                    }
                    return Ok(card);
                }
            }
        }
        let source = paths
            .designs()
            .join(job_id)
            .join(voices::variant_dir(variant));
        let card = voices::read_card(&source)
            .map_err(|_| Error::NotFound(format!("çeşidin dosyaları yok: {variant}")))?;
        let display = name
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(String::from)
            .or_else(|| result["name"].as_str().map(String::from))
            .or_else(|| result["display_name"].as_str().map(String::from))
            .unwrap_or_else(|| job.title.clone());
        let display: String = display.trim().chars().take(80).collect();
        let slug = text::slug(&display);
        let voice_dir = paths.voices().join(&slug);
        std::fs::create_dir_all(&voice_dir)?;
        let mut number = 1;
        while voice_dir.join(voices::variant_dir(number)).exists() {
            number += 1;
        }
        let target = voice_dir.join(voices::variant_dir(number));
        std::fs::create_dir_all(&target)?;
        let copied = (|| -> Result<(), Error> {
            for e in std::fs::read_dir(&source)?.flatten() {
                if e.path().is_file() {
                    std::fs::copy(e.path(), target.join(e.file_name()))?;
                }
            }
            let mut card = card;
            for (k, v) in [
                ("name", json!(slug)),
                ("display_name", json!(display)),
                ("variant", json!(number)),
                ("source", json!("design")),
                ("job_id", json!(job_id)),
            ] {
                card[k] = v;
            }
            voices::write_card(&target, &card)
        })();
        if let Err(e) = copied {
            let _ = std::fs::remove_dir_all(&target);
            let _ = std::fs::remove_dir(&voice_dir);
            return Err(e);
        }
        let id = format!("{slug}/{number}");
        result["variants"][index]["library_voice_id"] = json!(id);
        store.update_result(job_id, &result, None)?;
        self.emit(Event::Library);
        voices::get_card(paths, store, &id)
    }

    pub fn export_job(&self, id: &str, format: &str, target: &Path) -> Result<PathBuf, Error> {
        let j = self.job(id)?.ok_or_else(|| Error::NotFound(id.into()))?;
        let result = j
            .result
            .ok_or_else(|| Error::Input("üretim bitmemiş".into()))?;
        crate::export::export(&result, format, target, &j.title)
    }

    pub fn suggested_file_name(&self, id: &str) -> Result<String, Error> {
        let j = self
            .shared
            .store
            .job(id)?
            .ok_or_else(|| Error::NotFound(id.into()))?;
        let template = self.settings().file_name_template;
        Ok(crate::export::file_name(
            &template,
            j.voice_name.as_deref().unwrap_or("tini"),
            &j.title,
        ))
    }

    // --- drafts -------------------------------------------------------------------------------------------------------

    pub fn drafts(&self) -> Result<Vec<Draft>, Error> {
        self.shared.store.drafts()
    }

    pub fn save_draft(&self, id: Option<i64>, name: &str, body: &str) -> Result<i64, Error> {
        self.shared.store.save_draft(id, name, body)
    }

    pub fn delete_draft(&self, id: i64) -> Result<(), Error> {
        self.shared.store.delete_draft(id)
    }

    // --- model and engine env -----------------------------------------------------------------------------------------

    pub fn download_model(&self) -> Result<(), Error> {
        let mut slot = self.shared.model_cancel.lock().unwrap();
        if slot.is_some() {
            return Err(Error::Input("indirme zaten sürüyor".into()));
        }
        let flag = Arc::new(AtomicBool::new(false));
        *slot = Some(flag.clone());
        let sh = self.shared.clone();
        std::thread::spawn(move || {
            let target = sh.paths.downloaded_model();
            let events = sh.events.clone();
            match model::download(&target, &flag, &mut |d| events(Event::Model(d))) {
                Ok(()) => {
                    if let Some(fp) = model::fingerprint(&target) {
                        let _ = sh.store.set_kv(MODEL_FINGERPRINT_KEY, &json!(fp));
                    }
                    {
                        let mut s = sh.settings.write().unwrap();
                        s.model_path = None;
                        let _ = s.save(&sh.paths.settings_file());
                    }
                    let total = model::total_size();
                    events(Event::Model(model::DownloadStatus::new(
                        "done", total, total,
                    )));
                }
                Err(Error::Cancelled) => {}
                Err(e) => events(Event::Model(model::DownloadStatus {
                    message: Some(e.to_string()),
                    ..model::DownloadStatus::new("error", 0, model::total_size())
                })),
            }
            *sh.model_cancel.lock().unwrap() = None;
        });
        Ok(())
    }

    /// Downloads the transcription model (whisper large-v3-turbo q5_0).
    pub fn download_whisper_model(&self) -> Result<(), Error> {
        let mut slot = self.shared.whisper_cancel.lock().unwrap();
        if slot.is_some() {
            return Err(Error::Input("indirme zaten sürüyor".into()));
        }
        let flag = Arc::new(AtomicBool::new(false));
        *slot = Some(flag.clone());
        let sh = self.shared.clone();
        std::thread::spawn(move || {
            let events = sh.events.clone();
            let target = crate::transcribe::model_path(&sh.paths.data_dir);
            let t = crate::transcribe::MODEL_SIZE;
            let r = model::download_file(
                crate::transcribe::MODEL_URL,
                &target,
                crate::transcribe::MODEL_SHA256,
                Some(t),
                0,
                t,
                &flag,
                &mut |d| events(Event::Whisper(d)),
            );
            match r {
                Ok(()) => events(Event::Whisper(model::DownloadStatus::new("done", t, t))),
                Err(Error::Cancelled) => {}
                Err(e) => events(Event::Whisper(model::DownloadStatus {
                    message: Some(e.to_string()),
                    ..model::DownloadStatus::new("error", 0, t)
                })),
            }
            *sh.whisper_cancel.lock().unwrap() = None;
        });
        Ok(())
    }

    pub fn cancel_whisper_download(&self) {
        if let Some(f) = self.shared.whisper_cancel.lock().unwrap().as_ref() {
            f.store(true, Ordering::Relaxed);
        }
    }

    /// Parses an SRT/VTT file into cues.
    pub fn parse_subtitles(&self, path: &Path) -> Result<Vec<crate::subtitles::Cue>, Error> {
        let content = std::fs::read_to_string(path)?;
        let cues = crate::subtitles::parse(&content);
        if cues.is_empty() {
            return Err(Error::Input(
                "dosyada altyazı ipucu bulunamadı (SRT ya da VTT olmalı)".into(),
            ));
        }
        Ok(cues)
    }

    /// Installs or repairs the voice engine's Python env (with uv).
    pub fn install_engine_env(&self) -> Result<(), Error> {
        let mut slot = self.shared.env_cancel.lock().unwrap();
        if slot.is_some() {
            return Err(Error::Input("kurulum zaten sürüyor".into()));
        }
        if self.shared.queue.is_busy() {
            return Err(Error::Input("bir iş çalışıyor; bitince kurun".into()));
        }
        let uv = self
            .shared
            .paths
            .uv
            .clone()
            .ok_or_else(|| Error::Config("uv uygulama paketinde yok".into()))?;
        let flag = Arc::new(AtomicBool::new(false));
        *slot = Some(flag.clone());
        let sh = self.shared.clone();
        std::thread::spawn(move || {
            let events = sh.events.clone();
            crate::engine::shutdown();
            let r = crate::engine_env::install(
                &uv,
                &sh.paths.data_dir,
                &sh.paths.requirements(),
                &flag,
                &mut |d| events(Event::EngineEnv(d)),
            );
            match r {
                Ok(()) => {}
                Err(Error::Cancelled) => events(Event::EngineEnv(model::DownloadStatus::new(
                    "cancelled",
                    0,
                    4,
                ))),
                Err(e) => events(Event::EngineEnv(model::DownloadStatus {
                    message: Some(e.to_string()),
                    ..model::DownloadStatus::new("error", 0, 4)
                })),
            }
            *sh.env_cancel.lock().unwrap() = None;
        });
        Ok(())
    }

    pub fn cancel_engine_env_install(&self) {
        if let Some(f) = self.shared.env_cancel.lock().unwrap().as_ref() {
            f.store(true, Ordering::Relaxed);
        }
    }

    pub fn cancel_model_download(&self) {
        if let Some(f) = self.shared.model_cancel.lock().unwrap().as_ref() {
            f.store(true, Ordering::Relaxed);
        }
    }

    /// Verifies a VoxCPM2 folder the user already has (sha256 per file) and uses it in place; nothing is copied.
    pub fn use_local_model(&self, path: PathBuf) -> Result<(), Error> {
        let mut slot = self.shared.model_cancel.lock().unwrap();
        if slot.is_some() {
            return Err(Error::Input("başka bir model işlemi sürüyor".into()));
        }
        let flag = Arc::new(AtomicBool::new(false));
        *slot = Some(flag.clone());
        let sh = self.shared.clone();
        std::thread::spawn(move || {
            let events = sh.events.clone();
            let total = model::total_size();
            let status = match model::verify_dir(&path, &flag, &mut |d| events(Event::Model(d))) {
                Ok(()) => {
                    if let Some(fp) = model::fingerprint(&path) {
                        let _ = sh.store.set_kv(MODEL_FINGERPRINT_KEY, &json!(fp));
                    }
                    let mut s = sh.settings.write().unwrap();
                    s.model_path = if path == sh.paths.downloaded_model() {
                        None
                    } else {
                        Some(path.clone())
                    };
                    let _ = s.save(&sh.paths.settings_file());
                    model::DownloadStatus::new("done", total, total)
                }
                Err(e) => model::DownloadStatus {
                    message: Some(e.to_string()),
                    ..model::DownloadStatus::new("error", 0, total)
                },
            };
            events(Event::Model(status));
            *sh.model_cancel.lock().unwrap() = None;
        });
        Ok(())
    }
}

fn disk_size(p: &Path) -> u64 {
    if p.is_file() {
        return std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    }
    std::fs::read_dir(p)
        .map(|r| r.flatten().map(|e| disk_size(&e.path())).sum())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal stand-in for the VoxCPM sidecar (same line protocol): every request yields a short tone.
    const FAKE_ENGINE: &str = r#"
import json, math, os, struct, sys, wave
def out(o):
    print(json.dumps(o), flush=True)
out({"event": "started"})
for line in sys.stdin:
    cmd = json.loads(line)
    c = cmd.get("cmd")
    if c == "load":
        out({"event": "ready", "seconds": 0.0})
    elif c == "generate":
        for r in cmd["requests"]:
            path = os.path.join(cmd["out_dir"], r["id"] + ".wav")
            w = wave.open(path, "wb")
            w.setnchannels(1); w.setsampwidth(2); w.setframerate(48000)
            n = 48000
            w.writeframes(b"".join(struct.pack("<h", int(6000 * math.sin(i / 20.0)) if (i // 4800) % 2 == 0 else 3) for i in range(n)))
            w.close()
            out({"event": "request_done", "id": r["id"], "wav": path, "complete": True})
        out({"event": "batch_done"})
    elif c == "cancel":
        out({"event": "cancelled"})
    elif c == "shutdown":
        break
"#;

    fn studio(dir: &Path) -> Studio {
        std::env::set_var(NO_WORKER_ENV, "1");
        let res = dir.join("res");
        std::fs::create_dir_all(&res).unwrap();
        Studio::open_at(dir.join("data"), res, Default::default(), Arc::new(|_| {})).unwrap()
    }

    #[test]
    fn design_study_lives_outside_the_library_until_added() {
        let _serial = crate::PROCESS_AND_SOCKET_TESTS
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let python = PathBuf::from("/usr/bin/python3");
        if !python.is_file() {
            eprintln!("skipped: no python3 for the fake engine");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let s = studio(dir.path());
        let script = dir.path().join("fake_engine.py");
        std::fs::write(&script, FAKE_ENGINE).unwrap();
        let sh = &s.shared;
        let job = s
            .enqueue_job(JobRequest::Design {
                name: "Deniz Feneri".into(),
                description: "A calm adult male voice".into(),
                text: "Merhaba, bugün hava çok güzel.".into(),
                language: Some("tr".into()),
                variants: 2,
                seeds: Some(vec![11, 22]),
                preview_text: Some("İyi günler.".into()),
            })
            .unwrap();
        assert!(sh.store.claim(&job.id).unwrap());
        let cancel = AtomicBool::new(false);
        let ctx = jobs::Context {
            paths: &sh.paths,
            engine: EngineConfig {
                python,
                script,
                model: dir.path().join("model"),
                device: "cpu".into(),
                precision: "float32".into(),
                steps: 4,
                cfg: 2.0,
                cache_dir: dir.path().join("cache"),
                idle_unload_s: 0,
                word_timing: false,
                tone_match: true,
                keep_intermediate: false,
            },
            dictionary: Default::default(),
            cancel: &cancel,
        };
        let out = jobs::design(
            &ctx,
            &job.id,
            "Deniz Feneri",
            "A calm adult male voice",
            "Merhaba, bugün hava çok güzel.",
            "tr",
            2,
            Some(&vec![11, 22]),
            Some("İyi günler."),
            &mut |_| {},
        );
        crate::engine::shutdown();
        let out = out.unwrap();
        let study = sh.paths.designs().join(&job.id);
        assert_eq!(out.folder.as_deref(), Some(study.as_path()));
        assert!(out.voice_id.is_none());
        for n in [1, 2] {
            let v = study.join(voices::variant_dir(n));
            assert!(v.join("reference.wav").is_file());
            assert!(v.join("preview.wav").is_file());
            assert!(v.join(voices::CARD_FILE).is_file());
        }
        // nothing in the library yet
        assert!(s.voices().unwrap().is_empty());
        assert_eq!(std::fs::read_dir(sh.paths.voices()).unwrap().count(), 0);
        let r = &out.result;
        assert_eq!(r["name"], "Deniz Feneri");
        assert_eq!(r["language"], "tr");
        let v0 = &r["variants"][0];
        assert!(v0.get("id").is_none());
        assert!(v0["library_voice_id"].is_null());
        assert_eq!(v0["variant"], 1);
        assert_eq!(v0["seed"], 11);
        assert!(v0["noisy"].is_boolean());
        assert!(v0["preview_path"]
            .as_str()
            .unwrap()
            .starts_with(study.to_str().unwrap()));
        sh.store
            .finish(
                &job.id,
                &out.result,
                Some(&study.to_string_lossy()),
                None,
                out.duration_s,
                None,
                None,
            )
            .unwrap();

        // adding a variant copies it into the library, once
        let card = s.add_design_variant(&job.id, 2, None).unwrap();
        assert_eq!(card.id, "deniz_feneri/1");
        assert_eq!(card.job_id.as_deref(), Some(job.id.as_str()));
        assert_eq!(card.design_seed, Some(22));
        assert!(card.preview.is_some());
        let card_json = voices::read_card(Path::new(&card.folder)).unwrap();
        assert_eq!(card_json["source"], "design");
        assert_eq!(card_json["variant"], 1);
        let again = s.add_design_variant(&job.id, 2, Some("Başka ad")).unwrap();
        assert_eq!(again.id, card.id);
        assert_eq!(s.voices().unwrap().len(), 1);
        let stored = s.job(&job.id).unwrap().unwrap().result.unwrap();
        assert_eq!(stored["variants"][1]["library_voice_id"], "deniz_feneri/1");
        assert!(stored["variants"][0]["library_voice_id"].is_null());
        let other = s.add_design_variant(&job.id, 1, Some("Fener")).unwrap();
        assert_eq!(other.id, "fener/1");
        assert!(s.add_design_variant(&job.id, 9, None).is_err());

        // deleting the study keeps the library voices
        s.delete_design(&job.id).unwrap();
        assert!(!study.exists());
        assert!(s.job(&job.id).unwrap().is_none());
        assert_eq!(s.voices().unwrap().len(), 2);
    }

    #[test]
    fn api_token_is_generated_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let s = studio(dir.path());
        let token = s.settings().api_token;
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        let mut next = s.settings();
        next.api_token = String::new();
        assert_eq!(s.save_settings(next).unwrap().api_token, token);
        let info = s.regenerate_api_token().unwrap();
        assert_ne!(info.token, token);
        assert!(!info.running && !info.enabled);
        assert_eq!(info.url, "http://127.0.0.1:7860");
        let reopened = studio(dir.path());
        assert_eq!(reopened.settings().api_token, info.token);
    }
}
