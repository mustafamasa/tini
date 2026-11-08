//! Voice library: built-in voices that ship with the app, designed voices and consented clones.
//!
//! Each voice variant is a folder: `reference.wav|flac` + a `voice.json` card. Ids: `builtin/<name>` for the
//! read-only voices in `resources/voices/`, `<name>/<variant>` for the user's library.
//! Alias and favorite live in the SQLite overrides table; card files are not touched for them. A deleted voice is
//! moved under `voices/_trash/` and can be restored.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Value};

use crate::settings::Paths;
use crate::store::{Store, VoiceOverride};
use crate::{now_string, Error};

pub const CARD_FILE: &str = "voice.json";
pub const BUILTIN_PREFIX: &str = "builtin/";
pub const VARIANT_PREFIX: &str = "variant_";
const TRASH_META: &str = "trash.json";

#[derive(Debug, Clone, Serialize)]
pub struct VoiceCard {
    pub id: String,
    /// builtin | designed | clone
    pub source: String,
    pub folder: String,
    pub name: String,
    pub display_name: String,
    pub alias: Option<String>,
    pub variant: Option<i64>,
    pub language: String,
    pub description: String,
    pub design_text: String,
    pub design_seed: Option<u64>,
    pub read_seed: u64,
    pub reference: String,
    pub reference_s: Option<f64>,
    pub preview: Option<String>,
    pub preview_text: Option<String>,
    pub gender: Option<String>,
    pub created_at: Option<String>,
    pub job_id: Option<String>,
    pub favorite: bool,
    pub warning: Option<String>,
    pub preview_error: Option<String>,
    /// speech level minus background-noise floor of the reference (dB); see `audio::noise_margin_db`
    pub noise_margin_db: Option<f64>,
    /// the reference has audible background noise that readings will inherit
    pub noisy: bool,
}

pub fn read_card(folder: &Path) -> Result<Value, Error> {
    let text = fs::read_to_string(folder.join(CARD_FILE))
        .map_err(|e| Error::NotFound(format!("ses kartı okunamadı ({}): {e}", folder.display())))?;
    Ok(serde_json::from_str(&text)?)
}

pub fn write_card(folder: &Path, card: &Value) -> Result<(), Error> {
    let tmp = folder.join(format!("{CARD_FILE}.tmp"));
    fs::write(&tmp, serde_json::to_string_pretty(card)? + "\n")?;
    fs::rename(tmp, folder.join(CARD_FILE))?;
    Ok(())
}

pub fn update_card_fields(folder: &Path, fields: &Value) -> Result<Value, Error> {
    let mut card = read_card(folder)?;
    if let (Some(c), Some(f)) = (card.as_object_mut(), fields.as_object()) {
        for (k, v) in f {
            c.insert(k.clone(), v.clone());
        }
    }
    write_card(folder, &card)?;
    Ok(card)
}

fn s(card: &Value, key: &str) -> Option<String> {
    card.get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
        .filter(|x| !x.is_empty())
}

fn gender(card: &Value) -> Option<String> {
    if let Some(g) = s(card, "gender") {
        return Some(g);
    }
    let prompt = s(card, "description").unwrap_or_default().to_lowercase();
    if prompt.contains("woman") || prompt.contains("female") || prompt.contains(" lady") {
        Some("female".into())
    } else if prompt.contains(" man ") || prompt.contains("male") || prompt.contains("man,") {
        Some("male".into())
    } else {
        None
    }
}

fn source_of(card: &Value) -> &'static str {
    if s(card, "source").as_deref() == Some("clone") {
        "clone"
    } else {
        "designed"
    }
}

fn voice_card(id: String, folder: &Path, card: &Value, o: Option<&VoiceOverride>) -> VoiceCard {
    let source = if id.starts_with(BUILTIN_PREFIX) {
        "builtin"
    } else {
        source_of(card)
    };
    let reference = folder.join(s(card, "reference").unwrap_or_else(|| "reference.wav".into()));
    let preview = s(card, "preview")
        .map(|d| folder.join(d))
        .filter(|p| p.exists())
        .map(|p| p.to_string_lossy().to_string());
    let name = s(card, "name").unwrap_or_else(|| {
        folder
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    });
    let mut display_name = s(card, "display_name").unwrap_or_else(|| name.clone());
    if let Some(n) = card["variant"].as_i64() {
        display_name = format!("{display_name} · {n}");
    }
    let o = o.cloned().unwrap_or_default();
    VoiceCard {
        id,
        source: source.into(),
        folder: folder.to_string_lossy().into(),
        name,
        display_name,
        alias: o.alias,
        variant: card["variant"].as_i64(),
        language: s(card, "language").unwrap_or_else(|| "tr".into()),
        description: s(card, "description").unwrap_or_default(),
        design_text: s(card, "design_text").unwrap_or_default(),
        design_seed: card["design_seed"].as_u64(),
        read_seed: card["read_seed"].as_u64().unwrap_or(42),
        reference: reference.to_string_lossy().into(),
        reference_s: card["reference_s"].as_f64(),
        preview,
        preview_text: s(card, "preview_text"),
        gender: gender(card),
        created_at: s(card, "created_at"),
        job_id: s(card, "job_id"),
        favorite: o.favorite,
        warning: s(card, "warning"),
        preview_error: s(card, "preview_error"),
        noise_margin_db: card["noise_margin_db"].as_f64(),
        noisy: card["noise_margin_db"]
            .as_f64()
            .map(|m| m < crate::audio::NOISY_MARGIN_DB)
            .unwrap_or(false),
    }
}

/// Voices made before the noise check get their margin measured once and stored in the card.
fn with_noise_margin(folder: &Path, card: Value) -> Value {
    if card.get("noise_margin_db").is_some() {
        return card;
    }
    let reference = folder.join(s(&card, "reference").unwrap_or_else(|| "reference.wav".into()));
    let margin = crate::audio::load(&reference)
        .ok()
        .and_then(|a| crate::audio::noise_margin_db(&a.samples));
    update_card_fields(folder, &json!({"noise_margin_db": margin})).unwrap_or(card)
}

/// Variant folder name for a variant number.
pub fn variant_dir(number: u32) -> String {
    format!("{VARIANT_PREFIX}{number:02}")
}

/// All voices: built-in first, then designed and cloned.
pub fn list(paths: &Paths, store: &Store) -> Result<Vec<VoiceCard>, Error> {
    let overrides = store.voice_overrides()?;
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(paths.builtin_voices()) {
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join(CARD_FILE).is_file())
            .collect();
        dirs.sort();
        for dir in dirs {
            let id = format!(
                "{BUILTIN_PREFIX}{}",
                dir.file_name().unwrap().to_string_lossy()
            );
            let card = read_card(&dir)?;
            out.push(voice_card(id.clone(), &dir, &card, overrides.get(&id)));
        }
    }
    if let Ok(entries) = fs::read_dir(paths.voices()) {
        let mut voices: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && !p.file_name().unwrap().to_string_lossy().starts_with('_'))
            .collect();
        voices.sort();
        for voice in voices {
            let name = voice.file_name().unwrap().to_string_lossy().to_string();
            let mut variants: Vec<PathBuf> = fs::read_dir(&voice)?
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(VARIANT_PREFIX)
                        && p.join(CARD_FILE).is_file()
                })
                .collect();
            variants.sort();
            for variant in variants {
                let number: i64 = variant.file_name().unwrap().to_string_lossy()
                    [VARIANT_PREFIX.len()..]
                    .parse()
                    .unwrap_or(0);
                let id = format!("{name}/{number}");
                let card = with_noise_margin(&variant, read_card(&variant)?);
                out.push(voice_card(id.clone(), &variant, &card, overrides.get(&id)));
            }
        }
    }
    Ok(out)
}

/// Resolves an id to its folder: 'builtin/<name>', '<name>/<variant>' or a direct folder path.
pub fn find(paths: &Paths, id: &str) -> Result<PathBuf, Error> {
    let direct = PathBuf::from(id);
    if direct.is_absolute() && direct.join(CARD_FILE).is_file() {
        return Ok(direct);
    }
    let not_found = || Error::NotFound(format!("ses bulunamadı: {id}"));
    if let Some(name) = id.strip_prefix(BUILTIN_PREFIX) {
        let folder = paths.builtin_voices().join(name);
        if name.is_empty()
            || name.contains(['/', '\\'])
            || name.contains("..")
            || !folder.join(CARD_FILE).is_file()
        {
            return Err(not_found());
        }
        return Ok(folder);
    }
    let (name, number) = id.rsplit_once('/').ok_or_else(not_found)?;
    let n: u32 = number.parse().map_err(|_| not_found())?;
    let folder = paths.voices().join(name).join(variant_dir(n));
    if id.contains("..") || !folder.join(CARD_FILE).is_file() {
        return Err(not_found());
    }
    Ok(folder)
}

pub fn get_card(paths: &Paths, store: &Store, id: &str) -> Result<VoiceCard, Error> {
    let folder = find(paths, id)?;
    let card = read_card(&folder)?;
    let overrides = store.voice_overrides()?;
    Ok(voice_card(id.into(), &folder, &card, overrides.get(id)))
}

#[derive(Debug, Clone, Serialize)]
pub struct TrashEntry {
    pub trash_id: String,
    pub voice_id: String,
    pub display_name: String,
    pub deleted_at: String,
}

pub fn list_trash(paths: &Paths) -> Result<Vec<TrashEntry>, Error> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(paths.trash()) {
        for e in entries.flatten() {
            let p = e.path();
            let Ok(meta) = fs::read_to_string(p.join(TRASH_META)) else {
                continue;
            };
            let meta: Value = serde_json::from_str(&meta).unwrap_or(Value::Null);
            let card = read_card(&p).unwrap_or(Value::Null);
            out.push(TrashEntry {
                trash_id: p.file_name().unwrap().to_string_lossy().into(),
                voice_id: meta["voice_id"].as_str().unwrap_or("").into(),
                display_name: s(&card, "display_name")
                    .or_else(|| s(&card, "name"))
                    .unwrap_or_default(),
                deleted_at: meta["deleted_at"].as_str().unwrap_or("").into(),
            });
        }
    }
    out.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
    Ok(out)
}

/// Actions: 'rename' (empty name = back to the original), 'favorite', 'unfavorite', 'trash', 'restore' (by trash id).
pub fn manage(
    paths: &Paths,
    store: &Store,
    id: &str,
    action: &str,
    name: Option<&str>,
) -> Result<(), Error> {
    if action == "restore" {
        return restore(paths, store, id);
    }
    let overrides = store.voice_overrides()?;
    let mut o = overrides.get(id).cloned().unwrap_or_default();
    match action {
        "rename" => {
            find(paths, id)?;
            o.alias = name
                .map(|a| a.trim().chars().take(80).collect::<String>())
                .filter(|a| !a.is_empty());
        }
        "favorite" => {
            find(paths, id)?;
            o.favorite = true
        }
        "unfavorite" => o.favorite = false,
        "trash" => {
            if id.starts_with(BUILTIN_PREFIX) {
                return Err(Error::Input(
                    "uygulamayla gelen hazır sesler silinemez".into(),
                ));
            }
            let folder = find(paths, id)?;
            let trash = paths.trash();
            fs::create_dir_all(&trash)?;
            let target = trash.join(format!(
                "{}__{}",
                chrono::Local::now().format("%Y%m%d_%H%M%S"),
                id.replace('/', "__")
            ));
            fs::rename(&folder, &target)?;
            fs::write(
                target.join(TRASH_META),
                json!({"voice_id": id, "deleted_at": now_string()}).to_string(),
            )?;
            if let Some(parent) = folder.parent() {
                let _ = fs::remove_dir(parent); // if now empty
            }
            return Ok(());
        }
        other => return Err(Error::Input(format!("bilinmeyen işlem: {other}"))),
    }
    store.set_voice_override(id, &o)
}

fn restore(paths: &Paths, store: &Store, trash_id: &str) -> Result<(), Error> {
    if trash_id.contains('/') || trash_id.contains('\\') || trash_id.contains("..") {
        return Err(Error::Input("geçersiz çöp kaydı".into()));
    }
    let source = paths.trash().join(trash_id);
    let meta: Value = serde_json::from_str(&fs::read_to_string(source.join(TRASH_META))?)?;
    let id = meta["voice_id"]
        .as_str()
        .ok_or_else(|| Error::Input("çöp kaydında kimlik yok".into()))?;
    let (name, number) = id
        .rsplit_once('/')
        .ok_or_else(|| Error::Input("geçersiz kimlik".into()))?;
    let voice = paths.voices().join(name);
    fs::create_dir_all(&voice)?;
    let mut number = number.parse::<u32>().unwrap_or(1);
    while voice.join(variant_dir(number)).exists() {
        number += 1;
    }
    let target = voice.join(variant_dir(number));
    fs::rename(&source, &target)?;
    let _ = fs::remove_file(target.join(TRASH_META));
    let new_id = format!("{name}/{number}");
    if new_id != id {
        update_card_fields(&target, &json!({"variant": number}))?;
        store.move_voice_override(id, &new_id)?;
    }
    Ok(())
}

/// Exports a voice folder as a plain folder (reference + card + preview).
pub fn export(paths: &Paths, id: &str, target: &Path) -> Result<PathBuf, Error> {
    let folder = find(paths, id)?;
    let out = target.join(id.replace('/', "_"));
    fs::create_dir_all(&out)?;
    for e in fs::read_dir(&folder)?.flatten() {
        if e.path().is_file() {
            fs::copy(e.path(), out.join(e.file_name()))?;
        }
    }
    Ok(out)
}

/// Imports a voice folder exported from Tını: it needs a `voice.json` card and the reference recording.
pub fn import(paths: &Paths, source: &Path) -> Result<String, Error> {
    let card = read_card(source)?;
    let reference = s(&card, "reference").unwrap_or_else(|| "reference.wav".into());
    if !source.join(&reference).is_file() {
        return Err(Error::Input(format!("referans kaydı yok: {reference}")));
    }
    if card["design_text"].as_str().unwrap_or("").is_empty() {
        return Err(Error::Input(
            "kartta referans metni (design_text) yok".into(),
        ));
    }
    let name = crate::text::slug(
        &s(&card, "display_name")
            .or_else(|| s(&card, "name"))
            .unwrap_or_else(|| "voice".into()),
    );
    let voice = paths.voices().join(&name);
    fs::create_dir_all(&voice)?;
    let mut number = 1;
    while voice.join(variant_dir(number)).exists() {
        number += 1;
    }
    let target = voice.join(variant_dir(number));
    fs::create_dir_all(&target)?;
    for e in fs::read_dir(source)?.flatten() {
        if e.path().is_file() {
            fs::copy(e.path(), target.join(e.file_name()))?;
        }
    }
    write_card(&target, &card)?;
    update_card_fields(
        &target,
        &json!({"name": name, "variant": number, "reference": reference}),
    )?;
    Ok(format!("{name}/{number}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(dir: &Path) -> Paths {
        let p = Paths {
            data_dir: dir.join("data"),
            resources: dir.join("res"),
            uv: None,
            engine: dir.join("engine"),
            whisper: None,
        };
        p.prepare().unwrap();
        let b = p.builtin_voices().join("cansu");
        fs::create_dir_all(&b).unwrap();
        fs::write(
            b.join(CARD_FILE),
            json!({"name": "cansu", "display_name": "Cansu", "read_seed": 42, "source": "designed",
            "reference": "reference.wav", "design_text": "Merhaba."})
            .to_string(),
        )
        .unwrap();
        let k = p.voices().join("recording").join("variant_01");
        fs::create_dir_all(&k).unwrap();
        fs::write(
            k.join(CARD_FILE),
            json!({"name": "recording", "display_name": "Kayıt", "variant": 1, "read_seed": 7, "source": "clone",
            "reference": "reference.wav", "design_text": "Merhaba.", "gender": "female"})
            .to_string(),
        )
        .unwrap();
        let t = p.voices().join("deniz").join("variant_01");
        fs::create_dir_all(&t).unwrap();
        fs::write(
            t.join(CARD_FILE),
            json!({"name": "deniz", "display_name": "Deniz", "variant": 1, "read_seed": 9,
            "reference": "reference.wav", "design_text": "Selam."})
            .to_string(),
        )
        .unwrap();
        p
    }

    #[test]
    fn list_rename_trash_restore() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        let s = Store::in_memory().unwrap();
        let all = list(&p, &s).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(
            (
                all[0].id.as_str(),
                all[0].source.as_str(),
                all[0].display_name.as_str()
            ),
            ("builtin/cansu", "builtin", "Cansu")
        );
        assert_eq!(all[1].id, "deniz/1");
        assert_eq!(all[1].display_name, "Deniz · 1");
        assert_eq!(
            (all[2].id.as_str(), all[2].source.as_str()),
            ("recording/1", "clone")
        );
        assert_eq!(all[2].gender.as_deref(), Some("female"));
        assert!(find(&p, "missing/1").is_err());
        assert!(find(&p, "builtin/../x").is_err());
        assert!(manage(&p, &s, "builtin/cansu", "trash", None).is_err());
        manage(&p, &s, "builtin/cansu", "favorite", None).unwrap();
        assert!(list(&p, &s).unwrap()[0].favorite);
        manage(&p, &s, "deniz/1", "rename", Some("Fener")).unwrap();
        assert_eq!(list(&p, &s).unwrap()[1].alias.as_deref(), Some("Fener"));
        manage(&p, &s, "deniz/1", "trash", None).unwrap();
        assert_eq!(list(&p, &s).unwrap().len(), 2);
        let trash = list_trash(&p).unwrap();
        assert_eq!(trash.len(), 1);
        assert_eq!(trash[0].voice_id, "deniz/1");
        manage(&p, &s, &trash[0].trash_id, "restore", None).unwrap();
        assert_eq!(list(&p, &s).unwrap().len(), 3);
        assert!(find(&p, "../x/1").is_err());
    }

    #[test]
    fn exported_voice_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        let src = dir.path().join("exported");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("reference.wav"), b"RIFF").unwrap();
        write_card(
            &src,
            &json!({"name": "old", "display_name": "Eski Ses", "variant": 3, "reference": "reference.wav",
                "design_text": "Merhaba.", "source": "design"}),
        )
        .unwrap();
        let id = import(&p, &src).unwrap();
        assert_eq!(id, "eski_ses/1");
        let card = read_card(&find(&p, &id).unwrap()).unwrap();
        assert_eq!(card["reference"], "reference.wav");
        assert_eq!(card["design_text"], "Merhaba.");
        assert_eq!(card["variant"], 1);
    }
}
