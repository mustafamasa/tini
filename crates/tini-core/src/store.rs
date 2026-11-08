//! SQLite store: jobs (queue and history), drafts, voice overrides (alias, favorite), key-value.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{now_string, Error};

/// Schema version written to `PRAGMA user_version` (1 was the pre-English schema).
pub(crate) const SCHEMA_VERSION: i64 = 2;

pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS jobs (
        id TEXT PRIMARY KEY,
        kind TEXT NOT NULL,
        status TEXT NOT NULL,
        created_at TEXT NOT NULL,
        started_at TEXT,
        finished_at TEXT,
        title TEXT NOT NULL DEFAULT '',
        voice_id TEXT,
        voice_name TEXT,
        language TEXT,
        text TEXT NOT NULL DEFAULT '',
        request TEXT NOT NULL,
        result TEXT,
        folder TEXT,
        audio_s REAL,
        duration_s REAL,
        gpu_peak_mb INTEGER,
        error TEXT,
        starred INTEGER NOT NULL DEFAULT 0,
        notes TEXT NOT NULL DEFAULT '',
        completed INTEGER NOT NULL DEFAULT 0,
        total INTEGER NOT NULL DEFAULT 0,
        estimated_s REAL,
        parent_id TEXT
    );
    CREATE INDEX IF NOT EXISTS jobs_created_at ON jobs(created_at);
    CREATE INDEX IF NOT EXISTS jobs_status ON jobs(status);
    CREATE TABLE IF NOT EXISTS drafts (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL,
        text TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS voice_overrides (
        voice_id TEXT PRIMARY KEY,
        alias TEXT,
        favorite INTEGER NOT NULL DEFAULT 0,
        updated_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);";

/// A job record (queue entry and history).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    /// design | read | clone | line | transcribe | review | timed
    pub kind: String,
    /// queued | running | done | failed | cancelled | interrupted
    pub status: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub title: String,
    pub voice_id: Option<String>,
    pub voice_name: Option<String>,
    pub language: Option<String>,
    pub text: String,
    pub request: Value,
    pub result: Option<Value>,
    pub folder: Option<String>,
    pub audio_s: Option<f64>,
    pub duration_s: Option<f64>,
    pub gpu_peak_mb: Option<i64>,
    pub error: Option<String>,
    pub starred: bool,
    pub notes: String,
    pub completed: i64,
    pub total: i64,
    pub estimated_s: Option<f64>,
    pub parent_id: Option<String>,
}

fn row_job(r: &Row) -> rusqlite::Result<Job> {
    let request: String = r.get("request")?;
    let result: Option<String> = r.get("result")?;
    Ok(Job {
        id: r.get("id")?,
        kind: r.get("kind")?,
        status: r.get("status")?,
        created_at: r.get("created_at")?,
        started_at: r.get("started_at")?,
        finished_at: r.get("finished_at")?,
        title: r.get("title")?,
        voice_id: r.get("voice_id")?,
        voice_name: r.get("voice_name")?,
        language: r.get("language")?,
        text: r.get("text")?,
        request: serde_json::from_str(&request).unwrap_or(Value::Null),
        result: result.and_then(|s| serde_json::from_str(&s).ok()),
        folder: r.get("folder")?,
        audio_s: r.get("audio_s")?,
        duration_s: r.get("duration_s")?,
        gpu_peak_mb: r.get("gpu_peak_mb")?,
        error: r.get("error")?,
        starred: r.get::<_, i64>("starred")? != 0,
        notes: r.get("notes")?,
        completed: r.get("completed")?,
        total: r.get("total")?,
        estimated_s: r.get("estimated_s")?,
        parent_id: r.get("parent_id")?,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    pub id: i64,
    pub name: String,
    pub text: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoiceOverride {
    pub alias: Option<String>,
    pub favorite: bool,
}

/// Job list filter.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct JobFilter {
    pub search: Option<String>,
    pub kind: Option<String>,
    pub status: Option<String>,
    pub voice: Option<String>,
    pub starred: bool,
    pub limit: Option<i64>,
}

pub struct Store {
    db: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store, Error> {
        let db = Connection::open(path)?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        db.execute_batch(SCHEMA)?;
        db.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Store { db: Mutex::new(db) })
    }

    pub fn in_memory() -> Result<Store, Error> {
        let db = Connection::open_in_memory()?;
        db.execute_batch(SCHEMA)?;
        Ok(Store { db: Mutex::new(db) })
    }

    // --- jobs --------------------------------------------------------------------------------------------------------

    pub fn insert_job(&self, j: &Job) -> Result<(), Error> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO jobs (id, kind, status, created_at, title, voice_id, voice_name, language, text, request, total,
                estimated_s, parent_id, folder)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                j.id,
                j.kind,
                j.status,
                j.created_at,
                j.title,
                j.voice_id,
                j.voice_name,
                j.language,
                j.text,
                j.request.to_string(),
                j.total,
                j.estimated_s,
                j.parent_id,
                j.folder
            ],
        )?;
        Ok(())
    }

    pub fn job(&self, id: &str) -> Result<Option<Job>, Error> {
        let db = self.db.lock().unwrap();
        Ok(db
            .query_row("SELECT * FROM jobs WHERE id = ?1", [id], row_job)
            .optional()?)
    }

    pub fn jobs(&self, f: &JobFilter) -> Result<Vec<Job>, Error> {
        let mut sql = String::from("SELECT * FROM jobs WHERE 1=1");
        let mut args: Vec<String> = Vec::new();
        if let Some(a) = f.search.as_ref().filter(|a| !a.trim().is_empty()) {
            sql.push_str(" AND (text LIKE ? OR title LIKE ? OR voice_name LIKE ? OR notes LIKE ?)");
            let like = format!("%{}%", a.trim());
            args.extend([like.clone(), like.clone(), like.clone(), like]);
        }
        if let Some(k) = &f.kind {
            sql.push_str(" AND kind = ?");
            args.push(k.clone());
        }
        if let Some(s) = &f.status {
            sql.push_str(" AND status = ?");
            args.push(s.clone());
        }
        if let Some(v) = &f.voice {
            sql.push_str(" AND voice_id = ?");
            args.push(v.clone());
        }
        if f.starred {
            sql.push_str(" AND starred = 1");
        }
        sql.push_str(" ORDER BY created_at DESC, id DESC");
        sql.push_str(&format!(" LIMIT {}", f.limit.unwrap_or(500).clamp(1, 5000)));
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), row_job)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The oldest queued job.
    pub fn next_queued(&self) -> Result<Option<Job>, Error> {
        let db = self.db.lock().unwrap();
        Ok(db
            .query_row(
                "SELECT * FROM jobs WHERE status = 'queued' ORDER BY created_at, id LIMIT 1",
                [],
                row_job,
            )
            .optional()?)
    }

    pub fn queue(&self) -> Result<Vec<Job>, Error> {
        let db = self.db.lock().unwrap();
        let mut stmt = db
            .prepare("SELECT * FROM jobs WHERE status IN ('queued','running') ORDER BY status = 'queued', created_at, id")?;
        let rows = stmt.query_map([], row_job)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn set_status(&self, id: &str, status: &str) -> Result<(), Error> {
        let db = self.db.lock().unwrap();
        let (started_at, finished_at) = match status {
            "running" => (Some(now_string()), None),
            "done" | "failed" | "cancelled" | "interrupted" => (None, Some(now_string())),
            _ => (None, None),
        };
        db.execute(
            "UPDATE jobs SET status = ?2, started_at = COALESCE(?3, started_at), finished_at = COALESCE(?4, finished_at)
             WHERE id = ?1",
            params![id, status, started_at, finished_at],
        )?;
        Ok(())
    }

    /// Atomically claims a job: switches it to 'running' only if it is still 'queued', so a second process using the
    /// same database (e.g. the CLI) cannot take the same job.
    pub fn claim(&self, id: &str) -> Result<bool, Error> {
        let db = self.db.lock().unwrap();
        let n = db.execute(
            "UPDATE jobs SET status = 'running', started_at = ?2 WHERE id = ?1 AND status = 'queued'",
            params![id, now_string()],
        )?;
        Ok(n == 1)
    }

    pub fn set_progress(&self, id: &str, completed: i64, total: i64) -> Result<(), Error> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE jobs SET completed = ?2, total = ?3 WHERE id = ?1",
            params![id, completed, total],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn finish(
        &self,
        id: &str,
        result: &Value,
        folder: Option<&str>,
        audio_s: Option<f64>,
        duration_s: Option<f64>,
        gpu_mb: Option<u64>,
        voice_id: Option<&str>,
    ) -> Result<(), Error> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE jobs SET status = 'done', finished_at = ?2, result = ?3, folder = COALESCE(?4, folder), audio_s = ?5,
                duration_s = ?6, gpu_peak_mb = ?7, error = NULL, completed = total, voice_id = COALESCE(?8, voice_id)
             WHERE id = ?1",
            params![id, now_string(), result.to_string(), folder, audio_s, duration_s, gpu_mb.map(|g| g as i64), voice_id],
        )?;
        Ok(())
    }

    pub fn update_result(
        &self,
        id: &str,
        result: &Value,
        audio_s: Option<f64>,
    ) -> Result<(), Error> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE jobs SET result = ?2, audio_s = COALESCE(?3, audio_s) WHERE id = ?1",
            params![id, result.to_string(), audio_s],
        )?;
        Ok(())
    }

    pub fn fail(&self, id: &str, status: &str, message: &str) -> Result<(), Error> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE jobs SET status = ?2, error = ?3, finished_at = ?4 WHERE id = ?1",
            params![id, status, message, now_string()],
        )?;
        Ok(())
    }

    pub fn update_job(
        &self,
        id: &str,
        title: Option<&str>,
        starred: Option<bool>,
        notes: Option<&str>,
    ) -> Result<(), Error> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE jobs SET title = COALESCE(?2, title), starred = COALESCE(?3, starred), notes = COALESCE(?4, notes)
             WHERE id = ?1",
            params![id, title, starred.map(|y| y as i64), notes],
        )?;
        Ok(())
    }

    pub fn delete_job(&self, id: &str) -> Result<(), Error> {
        let db = self.db.lock().unwrap();
        db.execute("DELETE FROM jobs WHERE id = ?1", [id])?;
        Ok(())
    }

    /// At startup: jobs left running (app closed mid-job) become 'interrupted'.
    pub fn recover_interrupted(&self) -> Result<usize, Error> {
        let db = self.db.lock().unwrap();
        Ok(db.execute(
            "UPDATE jobs SET status = 'interrupted', error = 'uygulama iş sürerken kapandı', finished_at = ?1
             WHERE status = 'running'",
            [now_string()],
        )?)
    }

    pub fn stats(&self) -> Result<Value, Error> {
        let db = self.db.lock().unwrap();
        let (count, audio_s): (i64, Option<f64>) = db.query_row(
            "SELECT COUNT(*), SUM(audio_s) FROM jobs WHERE status = 'done'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(serde_json::json!({"completed": count, "total_audio_s": audio_s.unwrap_or(0.0)}))
    }

    // --- drafts ------------------------------------------------------------------------------------------------------

    pub fn drafts(&self) -> Result<Vec<Draft>, Error> {
        let db = self.db.lock().unwrap();
        let mut stmt =
            db.prepare("SELECT id, name, text, updated_at FROM drafts ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok(Draft {
                id: r.get(0)?,
                name: r.get(1)?,
                text: r.get(2)?,
                updated_at: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn save_draft(&self, id: Option<i64>, name: &str, text: &str) -> Result<i64, Error> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Input("taslak adı boş".into()));
        }
        let db = self.db.lock().unwrap();
        match id {
            Some(id) => {
                db.execute(
                    "UPDATE drafts SET name = ?2, text = ?3, updated_at = ?4 WHERE id = ?1",
                    params![id, name, text, now_string()],
                )?;
                Ok(id)
            }
            None => {
                db.execute(
                    "INSERT INTO drafts (name, text, updated_at) VALUES (?1, ?2, ?3)",
                    params![name, text, now_string()],
                )?;
                Ok(db.last_insert_rowid())
            }
        }
    }

    pub fn delete_draft(&self, id: i64) -> Result<(), Error> {
        let db = self.db.lock().unwrap();
        db.execute("DELETE FROM drafts WHERE id = ?1", [id])?;
        Ok(())
    }

    // --- voice overrides ---------------------------------------------------------------------------------------------

    pub fn voice_overrides(
        &self,
    ) -> Result<std::collections::HashMap<String, VoiceOverride>, Error> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT voice_id, alias, favorite FROM voice_overrides")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                VoiceOverride {
                    alias: r.get(1)?,
                    favorite: r.get::<_, i64>(2)? != 0,
                },
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn set_voice_override(&self, voice_id: &str, o: &VoiceOverride) -> Result<(), Error> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO voice_overrides (voice_id, alias, favorite, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(voice_id) DO UPDATE SET alias = ?2, favorite = ?3, updated_at = ?4",
            params![voice_id, o.alias, o.favorite as i64, now_string()],
        )?;
        Ok(())
    }

    pub fn move_voice_override(&self, old_id: &str, new_id: &str) -> Result<(), Error> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE OR REPLACE voice_overrides SET voice_id = ?2 WHERE voice_id = ?1",
            params![old_id, new_id],
        )?;
        Ok(())
    }

    // --- key-value ---------------------------------------------------------------------------------------------------

    pub fn kv(&self, key: &str) -> Result<Option<Value>, Error> {
        let db = self.db.lock().unwrap();
        let v: Option<String> = db
            .query_row("SELECT value FROM kv WHERE key = ?1", [key], |r| r.get(0))
            .optional()?;
        Ok(v.and_then(|s| serde_json::from_str(&s).ok()))
    }

    pub fn set_kv(&self, key: &str, value: &Value) -> Result<(), Error> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO kv (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value.to_string()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str, status: &str) -> Job {
        Job {
            id: id.into(),
            kind: "read".into(),
            status: status.into(),
            created_at: now_string(),
            started_at: None,
            finished_at: None,
            title: "Deneme".into(),
            voice_id: Some("voices/zeynep".into()),
            voice_name: Some("Zeynep".into()),
            language: Some("tr".into()),
            text: "Merhaba dünya.".into(),
            request: serde_json::json!({"kind": "read"}),
            result: None,
            folder: None,
            audio_s: None,
            duration_s: None,
            gpu_peak_mb: None,
            error: None,
            starred: false,
            notes: String::new(),
            completed: 0,
            total: 1,
            estimated_s: None,
            parent_id: None,
        }
    }

    #[test]
    fn job_lifecycle() {
        let s = Store::in_memory().unwrap();
        s.insert_job(&sample("a", "queued")).unwrap();
        s.insert_job(&sample("b", "queued")).unwrap();
        assert_eq!(s.next_queued().unwrap().unwrap().id, "a");
        s.set_status("a", "running").unwrap();
        assert_eq!(s.next_queued().unwrap().unwrap().id, "b");
        assert_eq!(s.recover_interrupted().unwrap(), 1);
        assert_eq!(s.job("a").unwrap().unwrap().status, "interrupted");
        s.finish(
            "b",
            &serde_json::json!({"wav": "x"}),
            Some("/k"),
            Some(2.0),
            Some(4.0),
            Some(100),
            None,
        )
        .unwrap();
        let b = s.job("b").unwrap().unwrap();
        assert_eq!(b.status, "done");
        assert_eq!(b.audio_s, Some(2.0));
        let found = s
            .jobs(&JobFilter {
                search: Some("dünya".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(found.len(), 2);
        s.update_job("b", None, Some(true), Some("iyi")).unwrap();
        assert_eq!(
            s.jobs(&JobFilter {
                starred: true,
                ..Default::default()
            })
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    fn drafts_and_overrides() {
        let s = Store::in_memory().unwrap();
        let id = s.save_draft(None, "Giriş", "Merhaba.").unwrap();
        s.save_draft(Some(id), "Giriş 2", "Selam.").unwrap();
        assert_eq!(s.drafts().unwrap()[0].name, "Giriş 2");
        assert!(s.save_draft(None, " ", "x").is_err());
        s.set_voice_override(
            "voices/x",
            &VoiceOverride {
                alias: Some("X".into()),
                favorite: true,
            },
        )
        .unwrap();
        assert!(s.voice_overrides().unwrap()["voices/x"].favorite);
        s.set_kv("a", &serde_json::json!({"b": 1})).unwrap();
        assert_eq!(s.kv("a").unwrap().unwrap()["b"], 1);
    }
}
