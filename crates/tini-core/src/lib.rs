//! Tını core.
//!
//! Runs the VoxCPM2 model (OpenBMB, Apache-2.0) with the official VoxCPM library in a persistent Python sidecar for
//! voice design, reference-based reading and consented cloning; runs jobs on a single GPU queue and keeps the job
//! history in SQLite.
//! Behavior follows the reference project voxcpm2-turkce-ses-tasarimi (Apache-2.0, Ali Haydar Öztürk).

pub mod api;
pub mod audio;
pub mod engine;
pub mod engine_env;
pub mod estimate;
pub mod export;
pub mod jobs;
pub mod model;
pub mod queue;
pub mod settings;
pub mod store;
pub mod studio;
pub mod subtitles;
pub mod text;
pub mod text_fix;
pub mod tone;
pub mod transcribe;
pub mod voice_prompts;
pub mod voices;
pub mod word_timing;

pub use studio::Studio;

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Input(String),
    #[error("metin sorunlu")]
    Issues(Vec<text::Issue>),
    #[error("motor: {0}")]
    Engine(String),
    #[error("ses: {0}")]
    Audio(String),
    #[error("ayar: {0}")]
    Config(String),
    #[error("veritabanı: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("dosya: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("ağ: {0}")]
    Network(String),
    #[error("iptal edildi")]
    Cancelled,
    #[error("bulunamadı: {0}")]
    NotFound(String),
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Error::Input(_) => "input",
            Error::Issues(_) => "issues",
            Error::Engine(_) => "engine",
            Error::Audio(_) => "audio",
            Error::Config(_) => "config",
            Error::Db(_) => "db",
            Error::Io(_) => "io",
            Error::Json(_) => "json",
            Error::Network(_) => "network",
            Error::Cancelled => "cancelled",
            Error::NotFound(_) => "not_found",
        }
    }
}

/// Error shape sent to the UI.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub code: String,
    pub message: String,
    pub issues: Vec<text::Issue>,
}

impl From<&Error> for ErrorResponse {
    fn from(e: &Error) -> Self {
        let issues = match e {
            Error::Issues(s) => s.clone(),
            _ => vec![],
        };
        let message = match e {
            Error::Issues(s) => s
                .iter()
                .map(|x| x.message.clone())
                .collect::<Vec<_>>()
                .join("\n"),
            other => other.to_string(),
        };
        ErrorResponse {
            code: e.code().into(),
            message,
            issues,
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Tests that spawn child processes and tests that bind sockets run one at a time: on macOS a socket created while
/// another thread forks can leak into the child (close-on-exec is set right after creation) and keep the port busy.
#[cfg(test)]
pub(crate) static PROCESS_AND_SOCKET_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Current local time, "YYYY-MM-DD HH:MM:SS".
/// Serializes creating long-lived file descriptors (the API's listening socket) with spawning the engine sidecar.
/// On macOS std sets close-on-exec in a separate step after `socket()`/`pipe()`, so a descriptor created in the same
/// instant as a spawn can leak into the child and, for a listening socket, keep the port busy until the child exits.
pub static SPAWN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn now_string() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Short sortable id: timestamp plus a random suffix.
pub fn new_id() -> String {
    use rand::Rng;
    let suffix: u32 = rand::thread_rng().gen_range(0..0xFFFFFF);
    format!(
        "{}_{suffix:06x}",
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    )
}
