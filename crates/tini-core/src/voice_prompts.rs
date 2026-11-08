//! Voice prompt presets: 18 roles × 8 languages (from the reference project, Apache-2.0). Descriptions are English and
//! mention nationality and native accent; each role's category has a ready design text in that language.

use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::Error;

#[derive(Debug, Clone, Serialize)]
pub struct VoicePrompt {
    pub id: String,
    pub language: String,
    pub role: String,
    pub category: String,
    pub gender: String,
    pub age: String,
    pub name_tr: String,
    pub name_en: String,
    pub description: String,
    pub text: String,
}

pub fn list(dir: &Path) -> Result<Vec<VoicePrompt>, Error> {
    let roles: Value = serde_json::from_str(&fs::read_to_string(dir.join("roles.json"))?)?;
    let roles = roles["roles"].as_array().cloned().unwrap_or_default();
    let mut languages: Vec<Value> = Vec::new();
    let mut paths: Vec<_> = fs::read_dir(dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().map(|e| e == "json").unwrap_or(false) && !p.ends_with("roles.json")
        })
        .collect();
    paths.sort();
    for p in paths {
        languages.push(serde_json::from_str(&fs::read_to_string(p)?)?);
    }
    // Turkish first
    languages.sort_by_key(|d| if d["language"] == "tr" { 0 } else { 1 });
    let str_of = |v: &Value, k: &str| v[k].as_str().unwrap_or("").to_string();
    let mut out = Vec::new();
    for lang in &languages {
        let code = str_of(lang, "language");
        for role in &roles {
            let description = str_of(role, "template")
                .replace("{man}", &str_of(lang, "man"))
                .replace("{woman}", &str_of(lang, "woman"))
                .replace("{native}", &str_of(lang, "native"));
            let category = str_of(role, "category");
            out.push(VoicePrompt {
                id: format!("{code}_{}", str_of(role, "role")),
                language: code.clone(),
                role: str_of(role, "role"),
                category: category.clone(),
                gender: str_of(role, "gender"),
                age: str_of(role, "age"),
                name_tr: str_of(role, "name_tr"),
                name_en: str_of(role, "name_en"),
                description,
                text: lang["texts"][&category].as_str().unwrap_or("").to_string(),
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundled_voice_prompts() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/voice_prompts");
        let all = super::list(&dir).unwrap();
        assert_eq!(all.len(), 144);
        assert_eq!(all[0].language, "tr");
        assert!(all
            .iter()
            .all(|o| !o.description.contains('{') && !o.text.is_empty()));
        assert!(all
            .iter()
            .all(|o| o.gender == "male" || o.gender == "female"));
    }
}
