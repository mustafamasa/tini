//! Tını desktop shell: exposes the core to the UI through Tauri commands and events.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use tauri::{Emitter, Manager, State};
use tini_core::jobs::JobRequest;
use tini_core::settings::Settings;
use tini_core::store::{Draft, Job, JobFilter};
use tini_core::voices::{TrashEntry, VoiceCard};
use tini_core::{Error, ErrorResponse, Studio};

/// Event channel the UI listens on.
const EVENT_CHANNEL: &str = "tini-event";

type Reply<T> = Result<T, ErrorResponse>;

fn err(e: Error) -> ErrorResponse {
    ErrorResponse::from(&e)
}

struct AppState(Arc<Studio>);

/// A bundled helper binary (Tauri `externalBin` places it next to the app without the target triple):
/// "uv" (engine env installer), "whisper_cli" (transcription).
fn binary_path(name: &str, env_var: &str) -> Option<PathBuf> {
    if let Ok(p) = std::env::var(env_var) {
        return Some(PathBuf::from(p));
    }
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let candidate = exe_dir.join(name);
    if candidate.is_file() {
        return Some(candidate);
    }
    // development: the binary under src-tauri/binaries
    Some(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join(format!("{name}-aarch64-apple-darwin")),
    )
}

/// Bundled engine folder (tini_engine.py, requirements.txt).
fn engine_dir(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(dir) = app.path().resource_dir() {
        let d = dir.join("engine");
        if d.join("tini_engine.py").is_file() {
            return d;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../engine/voxcpm")
}

fn resources_dir(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(dir) = app.path().resource_dir() {
        let d = dir.join("resources");
        if d.join("voice_prompts").is_dir() {
            return d;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources")
}

// --- status and settings ---------------------------------------------------------------------------------------------

#[tauri::command]
async fn get_status(s: State<'_, AppState>) -> Reply<Value> {
    Ok(s.0.status())
}

#[tauri::command]
async fn get_settings(s: State<'_, AppState>) -> Reply<Settings> {
    Ok(s.0.settings())
}

#[tauri::command]
async fn save_settings(s: State<'_, AppState>, settings: Settings) -> Reply<Settings> {
    s.0.save_settings(settings).map_err(err)
}

#[tauri::command]
async fn get_api_info(s: State<'_, AppState>) -> Reply<tini_core::api::ApiInfo> {
    Ok(s.0.api_info())
}

#[tauri::command]
async fn call_local_api(
    s: State<'_, AppState>,
    method: String,
    path: String,
    body: Option<serde_json::Value>,
) -> Reply<tini_core::api::LocalCall> {
    s.0.call_local_api(&method, &path, body.as_ref())
        .map_err(err)
}

#[tauri::command]
async fn regenerate_api_token(s: State<'_, AppState>) -> Reply<tini_core::api::ApiInfo> {
    s.0.regenerate_api_token().map_err(err)
}

#[tauri::command]
async fn set_data_dir(s: State<'_, AppState>, dir: String) -> Reply<()> {
    s.0.set_data_dir(&PathBuf::from(dir)).map_err(err)
}

#[tauri::command]
async fn unload_engine(s: State<'_, AppState>) -> Reply<()> {
    s.0.unload_engine().map_err(err)
}

#[tauri::command]
async fn clear_cache(s: State<'_, AppState>) -> Reply<u64> {
    s.0.clear_cache().map_err(err)
}

// --- voices ----------------------------------------------------------------------------------------------------------

#[tauri::command]
async fn list_voices(s: State<'_, AppState>) -> Reply<Vec<VoiceCard>> {
    s.0.voices().map_err(err)
}

#[tauri::command]
async fn manage_voice(
    s: State<'_, AppState>,
    id: String,
    action: String,
    name: Option<String>,
) -> Reply<()> {
    s.0.manage_voice(&id, &action, name.as_deref()).map_err(err)
}

#[tauri::command]
async fn list_trash(s: State<'_, AppState>) -> Reply<Vec<TrashEntry>> {
    s.0.trash().map_err(err)
}

#[tauri::command]
async fn export_voice(s: State<'_, AppState>, id: String, target: String) -> Reply<String> {
    s.0.export_voice(&id, &PathBuf::from(target))
        .map(|p| p.to_string_lossy().into())
        .map_err(err)
}

#[tauri::command]
async fn import_voice(s: State<'_, AppState>, source: String) -> Reply<String> {
    s.0.import_voice(&PathBuf::from(source)).map_err(err)
}

#[tauri::command]
async fn list_voice_prompts(
    s: State<'_, AppState>,
) -> Reply<Vec<tini_core::voice_prompts::VoicePrompt>> {
    s.0.voice_prompts().map_err(err)
}

// --- text ------------------------------------------------------------------------------------------------------------

#[tauri::command]
async fn check_text(
    s: State<'_, AppState>,
    text: String,
    language: Option<String>,
    voice: Option<String>,
) -> Reply<Value> {
    Ok(s.0.check_text(&text, language.as_deref(), voice.as_deref()))
}

#[tauri::command]
async fn check_design(
    s: State<'_, AppState>,
    description: String,
    text: String,
    language: Option<String>,
    variants: Option<usize>,
    preview_text: Option<String>,
) -> Reply<Value> {
    Ok(s.0.check_design(
        &description,
        &text,
        language.as_deref(),
        variants.unwrap_or(3),
        preview_text.as_deref(),
    ))
}

#[tauri::command]
async fn fix_text(
    s: State<'_, AppState>,
    text: String,
    language: Option<String>,
    voice: Option<String>,
    options: Option<tini_core::text_fix::FixOptions>,
) -> Reply<Value> {
    Ok(s.0.fix_text(
        &text,
        language.as_deref(),
        voice.as_deref(),
        &options.unwrap_or_default(),
    ))
}

#[tauri::command]
async fn read_dictionary(s: State<'_, AppState>) -> Reply<String> {
    Ok(s.0.read_dictionary())
}

#[tauri::command]
async fn save_dictionary(s: State<'_, AppState>, content: String) -> Reply<usize> {
    s.0.save_dictionary(&content).map_err(err)
}

#[tauri::command]
async fn preview_pronunciation(
    s: State<'_, AppState>,
    text: String,
    language: String,
) -> Reply<String> {
    Ok(s.0.preview_pronunciation(&text, &language))
}

// --- jobs ------------------------------------------------------------------------------------------------------------

#[tauri::command]
async fn enqueue_job(s: State<'_, AppState>, request: JobRequest) -> Reply<Job> {
    s.0.enqueue_job(request).map_err(err)
}

#[tauri::command]
async fn cancel_job(s: State<'_, AppState>, id: String) -> Reply<()> {
    s.0.cancel_job(&id).map_err(err)
}

#[tauri::command]
async fn rerun_job(s: State<'_, AppState>, id: String) -> Reply<Job> {
    s.0.rerun_job(&id).map_err(err)
}

#[tauri::command]
async fn list_queue(s: State<'_, AppState>) -> Reply<Vec<Job>> {
    s.0.queue().map_err(err)
}

#[tauri::command]
async fn list_jobs(s: State<'_, AppState>, filter: Option<JobFilter>) -> Reply<Vec<Job>> {
    s.0.jobs(&filter.unwrap_or_default()).map_err(err)
}

#[tauri::command]
async fn get_job(s: State<'_, AppState>, id: String) -> Reply<Option<Job>> {
    s.0.job(&id).map_err(err)
}

#[tauri::command]
async fn update_job(
    s: State<'_, AppState>,
    id: String,
    title: Option<String>,
    starred: Option<bool>,
    notes: Option<String>,
) -> Reply<()> {
    s.0.update_job(&id, title.as_deref(), starred, notes.as_deref())
        .map_err(err)
}

#[tauri::command]
async fn delete_job(s: State<'_, AppState>, id: String, delete_files: bool) -> Reply<()> {
    s.0.delete_job(&id, delete_files).map_err(err)
}

#[tauri::command]
async fn add_design_variant(
    s: State<'_, AppState>,
    job_id: String,
    variant: u32,
    name: Option<String>,
) -> Reply<VoiceCard> {
    s.0.add_design_variant(&job_id, variant, name.as_deref())
        .map_err(err)
}

#[tauri::command]
async fn delete_design(s: State<'_, AppState>, job_id: String) -> Reply<()> {
    s.0.delete_design(&job_id).map_err(err)
}

#[tauri::command]
async fn export_job(
    s: State<'_, AppState>,
    id: String,
    format: String,
    target: String,
) -> Reply<String> {
    s.0.export_job(&id, &format, &PathBuf::from(target))
        .map(|p| p.to_string_lossy().into())
        .map_err(err)
}

#[tauri::command]
async fn suggested_file_name(s: State<'_, AppState>, id: String) -> Reply<String> {
    s.0.suggested_file_name(&id).map_err(err)
}

// --- drafts ----------------------------------------------------------------------------------------------------------

#[tauri::command]
async fn list_drafts(s: State<'_, AppState>) -> Reply<Vec<Draft>> {
    s.0.drafts().map_err(err)
}

#[tauri::command]
async fn save_draft(
    s: State<'_, AppState>,
    id: Option<i64>,
    name: String,
    text: String,
) -> Reply<i64> {
    s.0.save_draft(id, &name, &text).map_err(err)
}

#[tauri::command]
async fn delete_draft(s: State<'_, AppState>, id: i64) -> Reply<()> {
    s.0.delete_draft(id).map_err(err)
}

// --- model, engine env, whisper --------------------------------------------------------------------------------------

#[tauri::command]
async fn download_model(s: State<'_, AppState>) -> Reply<()> {
    s.0.download_model().map_err(err)
}

#[tauri::command]
async fn cancel_model_download(s: State<'_, AppState>) -> Reply<()> {
    s.0.cancel_model_download();
    Ok(())
}

#[tauri::command]
async fn use_local_model(s: State<'_, AppState>, path: String) -> Reply<()> {
    s.0.use_local_model(PathBuf::from(path)).map_err(err)
}

#[tauri::command]
async fn download_whisper_model(s: State<'_, AppState>) -> Reply<()> {
    s.0.download_whisper_model().map_err(err)
}

#[tauri::command]
async fn cancel_whisper_download(s: State<'_, AppState>) -> Reply<()> {
    s.0.cancel_whisper_download();
    Ok(())
}

#[tauri::command]
async fn parse_subtitles(
    s: State<'_, AppState>,
    path: String,
) -> Reply<Vec<tini_core::subtitles::Cue>> {
    s.0.parse_subtitles(&PathBuf::from(path)).map_err(err)
}

#[tauri::command]
async fn install_engine_env(s: State<'_, AppState>) -> Reply<()> {
    s.0.install_engine_env().map_err(err)
}

#[tauri::command]
async fn cancel_engine_env_install(s: State<'_, AppState>) -> Reply<()> {
    s.0.cancel_engine_env_install();
    Ok(())
}

/// In-app microphone recording (encoded as WAV by the UI) is written to the work folder; the clone job deletes it.
#[tauri::command]
async fn save_recording(s: State<'_, AppState>, data: Vec<u8>) -> Reply<String> {
    if data.len() as u64 > tini_core::jobs::CLONE_MAX_BYTES * 2 {
        return Err(err(Error::Input("kayıt çok büyük".into())));
    }
    let dir = s.0.shared.paths.work();
    std::fs::create_dir_all(&dir).map_err(|e| err(e.into()))?;
    let path = dir.join(format!("mic_{}.wav", tini_core::new_id()));
    std::fs::write(&path, data).map_err(|e| err(e.into()))?;
    Ok(path.to_string_lossy().into())
}

#[tauri::command]
async fn audio_duration(path: String) -> Reply<f64> {
    tini_core::audio::load(&PathBuf::from(path))
        .map(|a| (a.seconds() * 100.0).round() / 100.0)
        .map_err(err)
}

/// Grants read access to one user-selected file so the UI can play it.
#[tauri::command]
fn allow_file(app: tauri::AppHandle, path: String) -> Reply<()> {
    app.asset_protocol_scope()
        .allow_file(PathBuf::from(path))
        .map_err(|e| err(Error::Input(e.to_string())))
}

#[tauri::command]
fn restart_app(app: tauri::AppHandle) {
    app.restart();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let emitter = handle.clone();
            let studio = Studio::open(
                resources_dir(&handle),
                tini_core::settings::Binaries {
                    uv: binary_path("uv", "TINI_UV"),
                    engine: Some(engine_dir(&handle)),
                    whisper: binary_path("whisper_cli", "TINI_WHISPER"),
                },
                Arc::new(move |event| {
                    let _ = emitter.emit(EVENT_CHANNEL, event);
                }),
            )
            .map_err(|e| {
                Box::new(std::io::Error::other(e.to_string())) as Box<dyn std::error::Error>
            })?;
            // local HTTP API (when enabled): binding is immediate; a busy port is reported by get_api_info
            studio.start_api();
            let scope = app.asset_protocol_scope();
            let _ = scope.allow_directory(&studio.shared.paths.data_dir, true);
            let _ = scope.allow_directory(&studio.shared.paths.resources, true);
            app.manage(AppState(Arc::new(studio)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_settings,
            save_settings,
            get_api_info,
            call_local_api,
            regenerate_api_token,
            set_data_dir,
            unload_engine,
            clear_cache,
            list_voices,
            manage_voice,
            list_trash,
            export_voice,
            import_voice,
            list_voice_prompts,
            check_text,
            check_design,
            fix_text,
            read_dictionary,
            save_dictionary,
            preview_pronunciation,
            enqueue_job,
            cancel_job,
            rerun_job,
            list_queue,
            list_jobs,
            get_job,
            update_job,
            delete_job,
            add_design_variant,
            delete_design,
            export_job,
            suggested_file_name,
            list_drafts,
            save_draft,
            delete_draft,
            download_model,
            cancel_model_download,
            use_local_model,
            install_engine_env,
            cancel_engine_env_install,
            download_whisper_model,
            cancel_whisper_download,
            parse_subtitles,
            save_recording,
            audio_duration,
            restart_app,
            allow_file,
        ])
        .build(tauri::generate_context!())
        .expect("Tını başlatılamadı")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(s) = app.try_state::<AppState>() {
                    s.0.stop_api();
                }
                // stop the engine sidecar (frees the model memory)
                tini_core::engine::shutdown();
            }
        });
}
