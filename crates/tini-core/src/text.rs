//! Text preparation: quote stripping, pronunciation dictionary, line and pause parsing, language detection, text
//! checks.
//!
//! Rules match the reference project (voxcpm2-turkce-ses-tasarimi, `ses_tasarim/okunus.py`): digits and quotes never
//! reach the engine, one line = one sentence, `|| bagli/normal/paragraf/agir` or `|| 0,8` at the end of a line is a
//! pause, and a blank line gives the previous sentence a paragraph pause. For non-Turkish languages the Turkish
//! dictionary is not applied and apostrophes are kept.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use serde_json::{json, Map, Value};

pub const MAX_LINE_CHARS: usize = 2000;
pub const GAP_SECONDS: f64 = 0.35;
pub const MAX_PAUSE_SECONDS: f64 = 5.0;
pub const DEFAULT_LANGUAGE: &str = "tr";

pub const PAUSES: [(&str, f64); 4] = [
    ("bagli", 0.25),
    ("normal", 0.5),
    ("paragraf", 1.0),
    ("agir", 1.5),
];

static QUOTE_MARKS: &str = "\"“”„«»‘’‚‹›`´'";
static DOUBLE_QUOTES: &str = "\"“”„«»‹›`´";
static UNSPOKEN_TR: LazyLock<Regex> = LazyLock::new(|| Regex::new("[\"“”„«»‘’‚‹›`´'\\d]").unwrap());
static UNSPOKEN_FOREIGN: LazyLock<Regex> = LazyLock::new(|| Regex::new("[\"“”„`´\\d]").unwrap());
static PAUSE_MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)\|\|[^\n]*$").unwrap());
static NEWLINE_SPACES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^\S\n]*\n[^\S\n]*").unwrap());
static MULTI_SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\S\n]{2,}").unwrap());
static CUSTOM_PAUSE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+(?:[.,]\d+)?)\s*s?$").unwrap());
static LANG_CODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z]{2,3}(-[a-z0-9]{2,8})?$").unwrap());
static WORDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\w']+").unwrap());

static STOPWORDS: LazyLock<Vec<(&'static str, HashSet<&'static str>)>> = LazyLock::new(|| {
    let raw: [(&str, &str); 5] = [
        (
            "tr",
            "ve bir bu da de için ile çok gibi daha ne ama var yok mı mi ben sen o biz siz her en kadar sonra olarak değil çünkü kapadokya merhaba",
        ),
        ("en", "the and of to is in it that for with as was on are this be at by an or from you not have but one"),
        ("fr", "le la les des est une un et en du de que qui dans pour pas sur au avec ce il elle nous vous sont l'un"),
        ("de", "der die das und ist ein eine nicht mit von zu den dem auf für auch sich es im wie ich sie so"),
        ("es", "el la los las es un una y en de que por con para no se su al lo como más pero uno esos esas"),
    ];
    raw.iter()
        .map(|(code, words)| (*code, words.split_whitespace().collect()))
        .collect()
});
const CHAR_HINTS: [(&str, &str); 3] = [("es", "ñ¿¡áíóú"), ("fr", "èêàâôîœëùû"), ("de", "äß")];

/// A user-facing issue message plus a code and values so the UI can phrase it itself.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Issue {
    pub code: String,
    pub message: String,
    pub data: Map<String, Value>,
}

impl Issue {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Issue {
            code: code.into(),
            message: message.into(),
            data: Map::new(),
        }
    }
    pub fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.data.insert(key.into(), value.into());
        self
    }
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Validates a language code; empty means "tr". Any well-formed code is accepted.
pub fn language(code: Option<&str>) -> Result<String, Issue> {
    let code = code.unwrap_or("").trim().to_lowercase();
    if code.is_empty() {
        return Ok(DEFAULT_LANGUAGE.into());
    }
    if !LANG_CODE.is_match(&code) {
        return Err(Issue::new(
            "language",
            format!("dil kodu tanınmadı: {code:?} (ör. tr, en, fr, de, es, ru, zh, ja)"),
        )
        .with("language", code));
    }
    Ok(code)
}

fn count_range(text: &str, lo: u32, hi: u32) -> usize {
    text.chars()
        .filter(|c| (lo..=hi).contains(&(*c as u32)))
        .count()
}

/// Detects the language: Turkish-specific letter → tr; kana → ja; Han → zh; Cyrillic → ru; otherwise stopword and
/// letter hints; with no hint `fallback`, else "tr".
pub fn detect(text: &str, fallback: Option<&str>) -> String {
    if count_range(text, 0x3040, 0x30FF) > 0 {
        return "ja".into();
    }
    if count_range(text, 0x4E00, 0x9FFF) > 0 {
        return "zh".into();
    }
    if count_range(text, 0x0400, 0x04FF) > 0 {
        return "ru".into();
    }
    if text.chars().any(|c| "ğşıİĞŞ".contains(c)) {
        return "tr".into();
    }
    let lowered = text.to_lowercase();
    let words: Vec<&str> = WORDS.find_iter(&lowered).map(|m| m.as_str()).collect();
    let mut scores: Vec<(&str, usize)> = STOPWORDS
        .iter()
        .map(|(code, stops)| (*code, words.iter().filter(|w| stops.contains(*w)).count()))
        .collect();
    for (code, chars) in CHAR_HINTS {
        let hits: usize = chars.chars().map(|ch| lowered.matches(ch).count()).sum();
        if let Some(entry) = scores.iter_mut().find(|(c, _)| *c == code) {
            entry.1 += 2 * hits;
        }
    }
    let best = scores.iter().map(|(_, s)| *s).max().unwrap_or(0);
    let leaders: Vec<&str> = scores
        .iter()
        .filter(|(_, s)| *s == best)
        .map(|(c, _)| *c)
        .collect();
    if best > 0 && leaders.len() == 1 {
        return leaders[0].into();
    }
    match fallback {
        Some(f) => language(Some(f)).unwrap_or_else(|_| DEFAULT_LANGUAGE.into()),
        None => DEFAULT_LANGUAGE.into(),
    }
}

fn lower_char(c: char) -> char {
    match c {
        'I' => 'ı',
        'İ' => 'i',
        _ => {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(l), None) => l,
                _ => c,
            }
        }
    }
}

/// Turkish lowercase (I → ı, İ → i); preserves the character count.
pub fn turkish_lower(text: &str) -> String {
    text.chars().map(lower_char).collect()
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Pronunciation dictionary: (written, spoken) rules, longest first.
#[derive(Debug, Clone, Default)]
pub struct Dictionary {
    rules: Vec<(Vec<char>, String)>,
}

impl Dictionary {
    /// TSV: `written<TAB>spoken`; blank lines and lines starting with `#` are skipped.
    pub fn parse(content: &str) -> Result<Dictionary, Issue> {
        let mut rules: Vec<(String, String)> = Vec::new();
        let mut seen: HashMap<String, usize> = HashMap::new();
        for (index, raw) in content.lines().enumerate() {
            let number = index + 1;
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((written, spoken)) = line.split_once('\t') else {
                return Err(Issue::new(
                    "dictionary",
                    format!("{number}. satır: beklenen biçim 'yazılış<TAB>okunuş'"),
                )
                .with("line", number));
            };
            let (written, spoken) = (written.trim().to_string(), spoken.trim().to_string());
            let key = turkish_lower(&written);
            if let Some(first) = seen.get(&key) {
                return Err(Issue::new(
                    "dictionary",
                    format!("{number}. satır: '{written}' zaten {first}. satırda"),
                )
                .with("line", number));
            }
            seen.insert(key, number);
            rules.push((written, spoken));
        }
        rules.sort_by_key(|(written, _)| std::cmp::Reverse(written.chars().count()));
        Ok(Dictionary {
            rules: rules
                .into_iter()
                .map(|(w, s)| (turkish_lower(&w).chars().collect(), s))
                .collect(),
        })
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Whole-word, case-insensitive matching, left to right, non-overlapping.
    pub fn apply(&self, text: &str) -> String {
        if self.rules.is_empty() {
            return text.to_string();
        }
        let original: Vec<char> = text.chars().collect();
        let lowered: Vec<char> = original.iter().map(|c| lower_char(*c)).collect();
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < original.len() {
            let edge_ok = i == 0 || !is_word_char(lowered[i - 1]);
            let mut matched = None;
            if edge_ok {
                for (key, spoken) in &self.rules {
                    let end = i + key.len();
                    if end <= lowered.len()
                        && lowered[i..end] == key[..]
                        && (end == lowered.len() || !is_word_char(lowered[end]))
                    {
                        matched = Some((end, spoken));
                        break;
                    }
                }
            }
            match matched {
                Some((end, spoken)) => {
                    out.push_str(spoken);
                    i = end;
                }
                None => {
                    out.push(original[i]);
                    i += 1;
                }
            }
        }
        out
    }

    /// (char start, char length, spoken) of every match in the text; for UI highlighting.
    pub fn matches(&self, text: &str) -> Vec<(usize, usize, String)> {
        let lowered: Vec<char> = text.chars().map(lower_char).collect();
        let mut found = Vec::new();
        let mut i = 0;
        while i < lowered.len() {
            let edge_ok = i == 0 || !is_word_char(lowered[i - 1]);
            let mut step = 1;
            if edge_ok {
                for (key, spoken) in &self.rules {
                    let end = i + key.len();
                    if end <= lowered.len()
                        && lowered[i..end] == key[..]
                        && (end == lowered.len() || !is_word_char(lowered[end]))
                    {
                        found.push((i, key.len(), spoken.clone()));
                        step = key.len();
                        break;
                    }
                }
            }
            i += step;
        }
        found
    }
}

fn clean_spaces(text: &str) -> String {
    let text = NEWLINE_SPACES.replace_all(text, "\n");
    MULTI_SPACES.replace_all(&text, " ").trim().to_string()
}

/// Removes quotes and collapses whitespace; newlines stay as sentence boundaries.
pub fn strip_quotes(text: &str) -> String {
    clean_spaces(
        &text
            .chars()
            .filter(|c| !QUOTE_MARKS.contains(*c))
            .collect::<String>(),
    )
}

/// Non-Turkish languages: only double quotes are removed, apostrophes stay.
pub fn strip_double_quotes(text: &str) -> String {
    clean_spaces(
        &text
            .chars()
            .filter(|c| !DOUBLE_QUOTES.contains(*c))
            .collect::<String>(),
    )
}

/// The form sent to the engine.
pub fn spoken(text: &str, language: &str, dictionary: &Dictionary) -> String {
    if language != "tr" {
        return strip_double_quotes(text);
    }
    strip_quotes(&dictionary.apply(text))
}

/// A named pause or custom seconds ('0,8', '1.2s'; 0-5 s).
pub fn pause_seconds(name: &str) -> Result<f64, Issue> {
    if let Some((_, s)) = PAUSES.iter().find(|(n, _)| *n == name) {
        return Ok(*s);
    }
    if let Some(cap) = CUSTOM_PAUSE.captures(name) {
        if let Ok(value) = cap[1].replace(',', ".").parse::<f64>() {
            if value <= MAX_PAUSE_SECONDS {
                return Ok(value);
            }
        }
    }
    let options = format!("bagli, normal, paragraf, agir, 0-{} sn", MAX_PAUSE_SECONDS);
    Err(Issue::new(
        "pause",
        format!("bilinmeyen durak '{name}'; seçenekler: {options}"),
    )
    .with("pause", name)
    .with("options", options))
}

/// 'text || paragraf' or 'text || 0,8' → (text, seconds).
pub fn split_pause(line: &str) -> Result<(String, f64), Issue> {
    if let Some(index) = line.rfind("||") {
        let (text, name) = (&line[..index], &line[index + 2..]);
        return Ok((text.trim().to_string(), pause_seconds(name.trim())?));
    }
    Ok((line.trim().to_string(), GAP_SECONDS))
}

/// One sentence per line, with its trailing pause. A blank line gives the previous sentence a paragraph pause.
pub fn lines(text: &str) -> Result<Vec<(String, f64)>, Issue> {
    let mut parsed: Vec<(String, f64)> = Vec::new();
    let mut marked = false;
    for line in text.lines() {
        if line.trim().is_empty() {
            if let Some(last) = parsed.last_mut() {
                if !marked {
                    last.1 = 1.0;
                }
            }
            continue;
        }
        marked = line.contains("||");
        parsed.push(split_pause(line)?);
    }
    Ok(parsed)
}

/// Issues to fix before generating. `field` is the request field the text came from; `label` names it (in Turkish)
/// in the message.
pub fn problems(text: &str, field: &str, label: &str, language: &str) -> Vec<Issue> {
    let mut found = Vec::new();
    let turkish = language == "tr";
    let without_marks = PAUSE_MARK.replace_all(text, "");
    let pattern: &Regex = if turkish {
        &UNSPOKEN_TR
    } else {
        &UNSPOKEN_FOREIGN
    };
    let mut marks: Vec<String> = pattern
        .find_iter(&without_marks)
        .map(|m| m.as_str().to_string())
        .collect();
    marks.sort();
    marks.dedup();
    if !marks.is_empty() {
        let joined = marks.join(" ");
        let (code, hint) = if turkish {
            (
                "marks_tr",
                "tırnak ve kesme işaretini kaldır, rakamı yazıyla yaz",
            )
        } else {
            ("marks", "rakamı o dilde yazıyla yaz")
        };
        found.push(
            Issue::new(
                code,
                format!("{label}: okunamayan işaret [{joined}]; {hint}"),
            )
            .with("marks", joined)
            .with("field", field),
        );
    }
    for (index, raw) in text.lines().enumerate() {
        if raw.trim().is_empty() {
            continue;
        }
        let number = index + 1;
        match split_pause(raw) {
            Ok((line, _)) => {
                let length = line.chars().count();
                if length > MAX_LINE_CHARS {
                    found.push(
                        Issue::new(
                            "long_line",
                            format!("{label}: {number}. satır {length} karakter > {MAX_LINE_CHARS}; satırı böl"),
                        )
                        .with("line", number)
                        .with("length", length)
                        .with("limit", MAX_LINE_CHARS),
                    );
                }
            }
            Err(issue) => found.push(issue.with("line", number)),
        }
    }
    if text.trim().is_empty() {
        found.push(Issue::new("empty", format!("{label}: boş")).with("field", field));
    }
    found
}

/// Inline findings for the script editor: each has line number, start column, length, code and message.
pub fn line_diagnostics(text: &str, language: &str) -> Vec<Value> {
    let turkish = language == "tr";
    let pattern: &Regex = if turkish {
        &UNSPOKEN_TR
    } else {
        &UNSPOKEN_FOREIGN
    };
    let mut out = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let body = match raw.rfind("||") {
            Some(i) => &raw[..i],
            None => raw,
        };
        for m in pattern.find_iter(body) {
            let col = body[..m.start()].chars().count();
            let is_digit = m.as_str().chars().all(|c| c.is_numeric());
            out.push(json!({
                "line": index + 1, "column": col, "length": m.as_str().chars().count(),
                "code": if is_digit { "digit" } else { "quote" },
                "message": if is_digit { "Rakam okunmaz; sayıyı yazıyla yaz" } else { "Tırnak ve kesme işareti okunmaz" },
            }));
        }
        if let Some(i) = raw.rfind("||") {
            if let Err(issue) = pause_seconds(raw[i + 2..].trim()) {
                out.push(
                    json!({"line": index + 1, "column": raw[..i].chars().count(),
                    "length": raw[i..].chars().count(), "code": "pause", "message": issue.message}),
                );
            }
        }
        let length = body.trim().chars().count();
        if length > MAX_LINE_CHARS {
            out.push(json!({"line": index + 1, "column": 0, "length": raw.chars().count(), "code": "long_line",
                "message": format!("Satır {length} karakter; en fazla {MAX_LINE_CHARS}")}));
        }
    }
    out
}

/// Speaking rate (chars/s, including spaces and punctuation); measured in the reference project.
pub fn chars_per_second(language: &str) -> f64 {
    match language {
        "tr" => 14.5,
        "en" => 17.0,
        "de" => 16.5,
        "fr" => 21.5,
        "es" => 13.5,
        "ru" => 14.0,
        "ja" => 7.5,
        "zh" => 5.0,
        "ko" => 7.0,
        _ => 14.0,
    }
}

/// Estimated spoken duration of the text (s).
pub fn estimate_seconds(text: &str, language: Option<&str>) -> f64 {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut language = language
        .map(|d| d.to_string())
        .unwrap_or_else(|| detect(&collapsed, None));
    let hangul = collapsed.chars().any(|c| {
        let u = c as u32;
        (0x1100..=0x11FF).contains(&u)
            || (0x3130..=0x318F).contains(&u)
            || (0xAC00..=0xD7AF).contains(&u)
    });
    if hangul && language != "ja" && language != "zh" {
        language = "ko".into();
    }
    let seconds = collapsed.chars().count() as f64 / chars_per_second(&language);
    (seconds * 10.0).round() / 10.0
}

/// Folder name: Turkish letters to ASCII, anything but letters/digits to underscores.
pub fn slug(name: &str) -> String {
    let map = |c: char| match c {
        'ç' => 'c',
        'ğ' => 'g',
        'ı' => 'i',
        'ö' => 'o',
        'ş' => 's',
        'ü' => 'u',
        'Ç' => 'C',
        'Ğ' => 'G',
        'İ' => 'I',
        'Ö' => 'O',
        'Ş' => 'S',
        'Ü' => 'U',
        other => other,
    };
    let ascii: String = name.chars().map(map).collect::<String>().to_lowercase();
    let kept: String = ascii
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let joined = kept.split_whitespace().collect::<Vec<_>>().join("_");
    if joined.is_empty() {
        "voice".into()
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_detection() {
        assert_eq!(detect("Merhaba dünya, bugün hava güzel.", None), "tr");
        assert_eq!(
            detect(
                "The quick brown fox jumps over the lazy dog and the cat.",
                None
            ),
            "en"
        );
        assert_eq!(detect("Привет, мир", None), "ru");
        assert_eq!(detect("こんにちは世界", None), "ja");
        assert_eq!(detect("你好世界", None), "zh");
        assert_eq!(
            detect("Der Hund und die Katze sind nicht im Haus.", None),
            "de"
        );
        assert_eq!(detect("¿Dónde está la estación?", None), "es");
        assert_eq!(detect("xyz", Some("fr")), "fr");
        assert_eq!(detect("xyz", None), "tr");
    }

    #[test]
    fn dictionary_whole_word() {
        let s =
            Dictionary::parse("GPU\tci pi yu\nwww\tdabılyu dabılyu dabılyu\n# yorum\n").unwrap();
        assert_eq!(
            s.apply("Bu GPU hızlı, gpu'lar da."),
            "Bu ci pi yu hızlı, ci pi yu'lar da."
        );
        assert_eq!(s.apply("GPUlar"), "GPUlar");
        assert!(Dictionary::parse("a\tb\nA\tc").is_err());
        assert!(Dictionary::parse("yanlis").is_err());
    }

    #[test]
    fn quotes_and_spaces() {
        assert_eq!(
            strip_quotes("  \"Merhaba\"   dünya  \n  ikinci 'satır'  "),
            "Merhaba dünya\nikinci satır"
        );
        assert_eq!(strip_double_quotes("l'eau \"bonne\""), "l'eau bonne");
    }

    #[test]
    fn pauses_and_lines() {
        assert_eq!(pause_seconds("paragraf").unwrap(), 1.0);
        assert_eq!(pause_seconds("0,8").unwrap(), 0.8);
        assert_eq!(pause_seconds("1.2s").unwrap(), 1.2);
        assert!(pause_seconds("6").is_err());
        assert!(pause_seconds("uzun").is_err());
        let parsed = lines("Bir.\nİki. || agir\n\nÜç.\n\nDört. || bagli\n\nBeş.").unwrap();
        assert_eq!(
            parsed,
            vec![
                ("Bir.".into(), 0.35),
                ("İki.".into(), 1.5),
                ("Üç.".into(), 1.0),
                ("Dört.".into(), 0.25),
                ("Beş.".into(), 0.35)
            ]
        );
    }

    #[test]
    fn text_problems() {
        let found = problems("Saat 5'te geldi.", "text", "metin", "tr");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code, "marks_tr");
        assert_eq!(found[0].data["field"], "text");
        assert!(problems("Bonjour l'ami.", "text", "metin", "fr").is_empty());
        assert_eq!(problems("  ", "text", "metin", "tr")[0].code, "empty");
        assert!(problems("Bir. || 0,8", "text", "metin", "tr").is_empty());
        let long = "a".repeat(2001);
        assert_eq!(problems(&long, "text", "metin", "tr")[0].code, "long_line");
    }

    #[test]
    fn estimate_and_slug() {
        assert_eq!(estimate_seconds("Merhaba dünya.", Some("tr")), 1.0);
        assert_eq!(slug("Deniz Feneri Çığ"), "deniz_feneri_cig");
        assert_eq!(slug("!!"), "voice");
    }
}
