//! Automatic fixer that makes text readable by the engine.
//!
//! The engine cannot read digits, quotes or apostrophes, wants one sentence per line, and may take a parenthesis at
//! the start of a line for a voice description. Instead of asking the user to fix these by hand, these rules rewrite
//! the text: numbers become Turkish words (with their suffixes), abbreviations are expanded, citations and parentheses
//! are removed, quotes and apostrophes are dropped, and paragraphs are split into one sentence per line. Every change
//! is recorded so the UI can show a preview.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FixOptions {
    pub numbers: bool,
    pub abbreviations: bool,
    pub citations: bool,
    pub parentheses: bool,
    pub quotes: bool,
    pub sentences: bool,
}

impl Default for FixOptions {
    fn default() -> Self {
        FixOptions {
            numbers: true,
            abbreviations: true,
            citations: true,
            parentheses: true,
            quotes: true,
            sentences: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Change {
    /// number | abbreviation | citation | parenthesis | quote | sentence
    pub kind: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Fixed {
    pub text: String,
    pub changes: Vec<Change>,
}

// --- numbers ---------------------------------------------------------------------------------------------------------

const ONES: [&str; 10] = [
    "", "bir", "iki", "üç", "dört", "beş", "altı", "yedi", "sekiz", "dokuz",
];
const TENS: [&str; 10] = [
    "", "on", "yirmi", "otuz", "kırk", "elli", "altmış", "yetmiş", "seksen", "doksan",
];
const SCALES: [&str; 6] = ["", "bin", "milyon", "milyar", "trilyon", "katrilyon"];

fn three_digits(n: u64) -> Vec<&'static str> {
    let mut w = Vec::new();
    let (y, o, b) = (n / 100, (n / 10) % 10, n % 10);
    if y > 0 {
        if y > 1 {
            w.push(ONES[y as usize]);
        }
        w.push("yüz");
    }
    if o > 0 {
        w.push(TENS[o as usize]);
    }
    if b > 0 {
        w.push(ONES[b as usize]);
    }
    w
}

/// Turkish reading of a number: 1994 → "bin dokuz yüz doksan dört".
pub fn number_words(n: u64) -> String {
    if n == 0 {
        return "sıfır".into();
    }
    let mut groups = Vec::new();
    let mut x = n;
    while x > 0 {
        groups.push(x % 1000);
        x /= 1000;
    }
    let mut words: Vec<&str> = Vec::new();
    for (i, g) in groups.iter().enumerate().rev() {
        if *g == 0 {
            continue;
        }
        if i == 1 && *g == 1 {
            words.push("bin"); // never "bir bin"
            continue;
        }
        words.extend(three_digits(*g));
        if i > 0 {
            words.push(SCALES[i]);
        }
    }
    words.join(" ")
}

fn ordinal_suffix(last: &str) -> String {
    match last {
        "bir" => "birinci",
        "iki" => "ikinci",
        "üç" => "üçüncü",
        "dört" => "dördüncü",
        "beş" => "beşinci",
        "altı" => "altıncı",
        "yedi" => "yedinci",
        "sekiz" => "sekizinci",
        "dokuz" => "dokuzuncu",
        "on" => "onuncu",
        "yirmi" => "yirminci",
        "otuz" => "otuzuncu",
        "kırk" => "kırkıncı",
        "elli" => "ellinci",
        "altmış" => "altmışıncı",
        "yetmiş" => "yetmişinci",
        "seksen" => "sekseninci",
        "doksan" => "doksanıncı",
        "yüz" => "yüzüncü",
        "bin" => "bininci",
        "milyon" => "milyonuncu",
        "milyar" => "milyarıncı",
        "trilyon" => "trilyonuncu",
        "sıfır" => "sıfırıncı",
        other => return format!("{other}inci"),
    }
    .to_string()
}

/// Ordinal: 19 → "on dokuzuncu".
pub fn ordinal_words(n: u64) -> String {
    let s = number_words(n);
    match s.rsplit_once(' ') {
        Some((head, last)) => format!("{head} {}", ordinal_suffix(last)),
        None => ordinal_suffix(&s),
    }
}

fn digits_one_by_one(d: &str) -> String {
    d.chars()
        .map(|c| number_words(c.to_digit(10).unwrap_or(0) as u64))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Reads an integer string; leading-zero or very long strings are read digit by digit.
fn read_integer(d: &str) -> String {
    if (d.len() > 1 && d.starts_with('0')) || d.len() > 18 {
        return digits_one_by_one(d);
    }
    number_words(d.parse().unwrap_or(0))
}

fn attach_suffix(spelled: &str, suffix: &str) -> String {
    if suffix.is_empty() {
        return spelled.to_string();
    }
    let vowel = suffix
        .chars()
        .next()
        .map(|c| "aeıioöuüâîûAEIİOÖUÜ".contains(c))
        .unwrap_or(false);
    if vowel && spelled.ends_with("dört") {
        return format!("{}d{suffix}", &spelled[..spelled.len() - 1]); // dört + ü → dördü
    }
    format!("{spelled}{suffix}")
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '’' | '‘' | '`' | '´')
}

/// Spells out the numbers in the text (Turkish only).
fn spell_numbers(text: &str, log: &mut Vec<Change>) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 32);
    let mut i = 0;
    let is_digit = |k: usize| k < c.len() && c[k].is_ascii_digit();
    while i < c.len() {
        // percent: "%50" or "% 50"
        let mut percent = false;
        let head = i;
        if c[i] == '%' {
            let mut j = i + 1;
            while j < c.len() && c[j] == ' ' {
                j += 1;
            }
            if is_digit(j) {
                percent = true;
                i = j;
            } else {
                out.push(c[i]);
                i += 1;
                continue;
            }
        }
        if !is_digit(i) {
            out.push(c[i]);
            i += 1;
            continue;
        }
        // integer (may have thousands dots: 40.000)
        let mut j = i;
        while is_digit(j) {
            j += 1;
        }
        let mut int_part: String = c[i..j].iter().collect();
        let mut thousands = false;
        if int_part.len() <= 3 {
            let mut k = j;
            while k + 3 < c.len() + 1
                && k < c.len()
                && c[k] == '.'
                && is_digit(k + 1)
                && is_digit(k + 2)
                && is_digit(k + 3)
                && !is_digit(k + 4)
            {
                int_part.extend(&c[k + 1..k + 4]);
                k += 4;
                thousands = true;
            }
            j = k;
        }
        let mut spelled;
        // time: 14:30
        if !thousands
            && j + 2 < c.len() + 1
            && j < c.len()
            && c[j] == ':'
            && is_digit(j + 1)
            && is_digit(j + 2)
            && !is_digit(j + 3)
            && int_part.len() <= 2
        {
            let minutes: String = c[j + 1..j + 3].iter().collect();
            spelled = format!(
                "{} {}",
                read_integer(&int_part),
                if minutes == "00" {
                    "sıfır sıfır".into()
                } else {
                    read_integer(minutes.trim_start_matches('0'))
                }
            );
            if minutes.starts_with('0') && minutes != "00" {
                spelled = format!(
                    "{} sıfır {}",
                    read_integer(&int_part),
                    read_integer(&minutes[1..])
                );
            }
            j += 3;
        } else if j < c.len() && c[j] == ',' && is_digit(j + 1) {
            // decimal: 3,5 ; 0,05
            let mut k = j + 1;
            while is_digit(k) {
                k += 1;
            }
            let fraction: String = c[j + 1..k].iter().collect();
            let zeros = fraction.chars().take_while(|x| *x == '0').count();
            let mut parts = vec![read_integer(&int_part), "virgül".to_string()];
            parts.extend(std::iter::repeat_n(
                "sıfır".to_string(),
                zeros.min(fraction.len()),
            ));
            if zeros < fraction.len() {
                parts.push(read_integer(&fraction[zeros..]));
            }
            spelled = parts.join(" ");
            j = k;
        } else if !thousands
            && int_part.len() <= 3
            && j < c.len()
            && c[j] == '.'
            && j + 1 < c.len()
            && c[j + 1] == ' '
            && c[j + 2..]
                .iter()
                .find(|x| **x != ' ')
                .map(|x| x.is_alphabetic())
                .unwrap_or(false)
        {
            // ordinal: "19. yüzyıl", "2. Dünya"
            spelled = ordinal_words(int_part.parse().unwrap_or(0));
            j += 1;
        } else {
            spelled = read_integer(&int_part);
        }
        // trailing % (50%)
        if j < c.len() && c[j] == '%' {
            percent = true;
            j += 1;
        }
        // suffix: after an apostrophe or directly attached letters
        let mut suffix = String::new();
        if j + 1 < c.len() && is_apostrophe(c[j]) && c[j + 1].is_alphabetic() {
            j += 1;
        }
        while j < c.len() && c[j].is_alphabetic() {
            suffix.push(c[j]);
            j += 1;
        }
        let mut replacement = attach_suffix(&spelled, &suffix);
        if percent {
            replacement = format!("yüzde {replacement}");
        }
        if let Some(prev) = out.chars().last() {
            if prev.is_alphanumeric() {
                out.push(' ');
            }
        }
        let before: String = c[head..j].iter().collect();
        log.push(Change {
            kind: "number".into(),
            before,
            after: replacement.clone(),
        });
        out.push_str(&replacement);
        i = j;
    }
    out
}

// --- abbreviations ---------------------------------------------------------------------------------------------------

/// (abbreviation, expansion, may end a sentence)
const ABBREVIATIONS: &[(&str, &str, bool)] = &[
    ("M.Ö.", "milattan önce", true),
    ("M.S.", "milattan sonra", true),
    ("Hz.", "Hazreti", false),
    ("Prof.", "Profesör", false),
    ("Doç.", "Doçent", false),
    ("Dr.", "Doktor", false),
    ("Av.", "Avukat", false),
    ("Bkz.", "Bakınız", false),
    ("bkz.", "bakınız", false),
    ("Bk.", "Bakınız", false),
    ("bk.", "bakınız", false),
    ("vb.", "ve benzeri", true),
    ("vs.", "vesaire", true),
    ("yy.", "yüzyıl", true),
    ("örn.", "örneğin", false),
    ("Örn.", "Örneğin", false),
    ("çev.", "çeviren", false),
    ("haz.", "hazırlayan", false),
    ("ss.", "sayfalar", false),
    ("s.", "sayfa", false),
    ("No.", "numara", false),
    ("no.", "numara", false),
    ("St.", "Sokak", false),
    ("Cad.", "Caddesi", false),
    ("Mah.", "Mahallesi", false),
    ("vd.", "ve diğerleri", true),
];

fn expand_abbreviations(text: &str, log: &mut Vec<Change>) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    'outer: while i < c.len() {
        let word_start = i == 0 || !c[i - 1].is_alphanumeric();
        if word_start {
            for (k, expansion, may_end) in ABBREVIATIONS {
                let abbr: Vec<char> = k.chars().collect();
                let j = i + abbr.len();
                if j > c.len() || c[i..j] != abbr[..] {
                    continue;
                }
                // "s." and "No." only when followed by a number (so a sentence-internal "s." is left alone)
                let next = c[j..].iter().find(|x| **x != ' ').copied();
                if matches!(*k, "s." | "ss." | "No." | "no.")
                    && !next.map(|x| x.is_ascii_digit()).unwrap_or(false)
                {
                    continue;
                }
                let ends_sentence =
                    *may_end && next.map(|x| x.is_uppercase() || x == ')').unwrap_or(true);
                out.push_str(expansion);
                if ends_sentence {
                    out.push('.');
                }
                log.push(Change {
                    kind: "abbreviation".into(),
                    before: k.to_string(),
                    after: expansion.to_string(),
                });
                i = j;
                continue 'outer;
            }
        }
        if c[i] == '&' {
            out.push_str("ve");
            log.push(Change {
                kind: "abbreviation".into(),
                before: "&".into(),
                after: "ve".into(),
            });
            i += 1;
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

// --- citations and parentheses -------------------------------------------------------------------------------------

/// A parenthesis containing a 4-digit year or "s. 12" is an academic citation: "(Korkmaz, 1994, s. 240)".
fn is_citation(inner: &str) -> bool {
    let c: Vec<char> = inner.chars().collect();
    let has_year = c.windows(4).enumerate().any(|(i, w)| {
        w.iter().all(|x| x.is_ascii_digit())
            && (i == 0 || !c[i - 1].is_ascii_digit())
            && c.get(i + 4).map(|x| !x.is_ascii_digit()).unwrap_or(true)
    });
    has_year || inner.contains("s. ") && inner.chars().any(|x| x.is_ascii_digit())
}

fn process_parentheses(
    text: &str,
    citations: bool,
    parentheses: bool,
    log: &mut Vec<Change>,
) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < c.len() {
        if c[i] == '(' || c[i] == '[' {
            let closing = if c[i] == '(' { ')' } else { ']' };
            if let Some(off) = c[i + 1..]
                .iter()
                .position(|x| *x == closing || *x == '\n' || *x == '(' || *x == '[')
            {
                let j = i + 1 + off;
                if c[j] == closing {
                    let inner: String = c[i + 1..j].iter().collect();
                    let int_part: String = c[i..=j].iter().collect();
                    if citations && is_citation(&inner) {
                        while out.ends_with(' ') {
                            out.pop();
                        }
                        log.push(Change {
                            kind: "citation".into(),
                            before: int_part,
                            after: String::new(),
                        });
                        i = j + 1;
                        continue;
                    }
                    if parentheses {
                        let trimmed = inner.trim();
                        // parenthesis (also at line/sentence start): keep only its content
                        out.push_str(trimmed);
                        log.push(Change {
                            kind: "parenthesis".into(),
                            before: int_part,
                            after: trimmed.to_string(),
                        });
                        i = j + 1;
                        continue;
                    }
                }
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

// --- quotes ----------------------------------------------------------------------------------------------------------

fn strip_quotes(text: &str, turkish: bool, log: &mut Vec<Change>) -> String {
    let marks: &str = if turkish {
        "\"“”„«»‘’‚‹›`´'"
    } else {
        "\"“”„«»‹›`´"
    };
    let count = text.chars().filter(|c| marks.contains(*c)).count();
    if count == 0 {
        return text.to_string();
    }
    log.push(Change {
        kind: "quote".into(),
        before: format!("{count} işaret"),
        after: String::new(),
    });
    text.chars().filter(|c| !marks.contains(*c)).collect()
}

// --- sentence splitting ----------------------------------------------------------------------------------------------

const LONG_SENTENCE: usize = 320;

fn split_sentences(line: &str) -> Vec<String> {
    let c: Vec<char> = line.chars().collect();
    let mut pieces = Vec::new();
    let mut head = 0;
    let mut i = 0;
    while i < c.len() {
        if matches!(c[i], '.' | '!' | '?' | '…') {
            let mut j = i + 1;
            while j < c.len() && matches!(c[j], '.' | '!' | '?' | '…' | '"' | '”' | '’' | '»' | ')')
            {
                j += 1;
            }
            if j < c.len() && c[j].is_whitespace() {
                let mut k = j;
                while k < c.len() && c[k].is_whitespace() {
                    k += 1;
                }
                while k < c.len() && matches!(c[k], '"' | '“' | '«' | '(' | '‘') {
                    k += 1;
                }
                // a single capital + dot (an initial: "A. Yılmaz") does not end a sentence
                let initial =
                    i >= 1 && c[i - 1].is_uppercase() && (i == 1 || !c[i - 2].is_alphanumeric());
                if k < c.len() && c[k].is_uppercase() && !initial {
                    pieces.push(c[head..j].iter().collect::<String>().trim().to_string());
                    head = j;
                    i = j;
                    continue;
                }
            }
            i = j;
            continue;
        }
        i += 1;
    }
    pieces.push(c[head..].iter().collect::<String>().trim().to_string());
    // very long sentence: split at semicolons
    let mut replacement = Vec::new();
    for p in pieces.into_iter().filter(|p| !p.is_empty()) {
        if p.chars().count() > LONG_SENTENCE && p.contains("; ") {
            replacement.extend(
                p.split("; ")
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty()),
            );
        } else {
            replacement.push(p);
        }
    }
    replacement
}

fn clean_spaces(line: &str) -> String {
    let mut s = line.split_whitespace().collect::<Vec<_>>().join(" ");
    for (a, b) in [
        (" ,", ","),
        (" .", "."),
        (" ;", ";"),
        (" :", ":"),
        (",,", ","),
        (", .", "."),
        (",.", "."),
    ] {
        while s.contains(a) {
            s = s.replace(a, b);
        }
    }
    s.trim().to_string()
}

/// Makes the text readable. Trailing pause marks (`|| ...`) and blank lines are kept.
pub fn fix_text(text: &str, language: &str, s: &FixOptions) -> Fixed {
    let turkish = language == "tr";
    let mut log = Vec::new();
    let mut lines_out: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            lines_out.push(String::new());
            continue;
        }
        let (body, marker) = match line.rfind("||") {
            Some(i) => (&line[..i], Some(line[i..].trim().to_string())),
            None => (line, None),
        };
        let mut t = body.to_string();
        if s.citations || s.parentheses {
            t = process_parentheses(&t, s.citations, s.parentheses, &mut log);
        }
        if turkish && s.abbreviations {
            t = expand_abbreviations(&t, &mut log);
        }
        if turkish && s.numbers {
            t = spell_numbers(&t, &mut log);
        }
        if s.quotes {
            t = strip_quotes(&t, turkish, &mut log);
        }
        let t = clean_spaces(&t);
        let mut sentences = if s.sentences {
            split_sentences(&t)
        } else {
            vec![t]
        };
        if sentences.len() > 1 {
            log.push(Change {
                kind: "sentence".into(),
                before: sentences[0].chars().take(40).collect::<String>() + "…",
                after: format!("{} satıra bölündü", sentences.len()),
            });
        }
        if let (Some(marker), Some(last)) = (marker, sentences.last_mut()) {
            last.push(' ');
            last.push_str(&marker);
        }
        lines_out.extend(sentences);
    }
    // merge quote records into a single entry
    let quote_count: usize = log
        .iter()
        .filter(|d| d.kind == "quote")
        .filter_map(|d| {
            d.before
                .split(' ')
                .next()
                .and_then(|n| n.parse::<usize>().ok())
        })
        .sum();
    log.retain(|d| d.kind != "quote");
    if quote_count > 0 {
        log.push(Change {
            kind: "quote".into(),
            before: format!("{quote_count} tırnak / kesme işareti"),
            after: "kaldırıldı".into(),
        });
    }
    let mut text = lines_out.join("\n");
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    Fixed {
        text: text.trim_matches('\n').to_string(),
        changes: log,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_readings() {
        assert_eq!(number_words(0), "sıfır");
        assert_eq!(number_words(1994), "bin dokuz yüz doksan dört");
        assert_eq!(number_words(2009), "iki bin dokuz");
        assert_eq!(number_words(100), "yüz");
        assert_eq!(number_words(240), "iki yüz kırk");
        assert_eq!(number_words(1_000_000), "bir milyon");
        assert_eq!(number_words(40_000), "kırk bin");
        assert_eq!(number_words(101_001), "yüz bir bin bir");
        assert_eq!(ordinal_words(19), "on dokuzuncu");
        assert_eq!(ordinal_words(4), "dördüncü");
        assert_eq!(ordinal_words(100), "yüzüncü");
    }

    fn d(m: &str) -> String {
        fix_text(m, "tr", &FixOptions::default()).text
    }

    #[test]
    fn numbers_suffixes_and_formats() {
        assert_eq!(d("1994'te geldi."), "bin dokuz yüz doksan dörtte geldi.");
        assert_eq!(d("4'ü de geldi."), "dördü de geldi.");
        assert_eq!(
            d("1990larda yaşandı."),
            "bin dokuz yüz doksanlarda yaşandı."
        );
        assert_eq!(d("Nüfus 40.000'in altında."), "Nüfus kırk binin altında.");
        assert_eq!(d("Oran %50 oldu."), "Oran yüzde elli oldu.");
        assert_eq!(d("Değer 3,05 çıktı."), "Değer üç virgül sıfır beş çıktı.");
        assert_eq!(d("Saat 14:30 oldu."), "Saat on dört otuz oldu.");
        assert_eq!(d("19. yüzyılda oldu."), "on dokuzuncu yüzyılda oldu.");
        assert_eq!(
            d("Yıl 1994. Sonra geldi."),
            "Yıl bin dokuz yüz doksan dört.\nSonra geldi."
        );
    }

    #[test]
    fn user_text() {
        let m = "Mevâlî, mevla kelimesinin çoğuludur ve Alevi-Bektaşi yolunda Hz. Ali’ye bağlananları karşılar. Kelimenin tekili \
                 veli anlamlarıyla verilir (Korkmaz,1994, s. 240). Sözcüğün Arapçadaki anlam genişliği, terimin ayırt edici yanını oluşturur.\n\n\
                 Gadîr Hum\n\n\
                 “Ben kimin mevlâsı isem Ali de onun mevlâsıdır” hadisine dayanılarak Hz. Ali’nin bendeleri. Mevâlî böylece bir ad hâline gelir. \
                 (Bk. Men küntü, Mevlahu, Velayet)";
        let r = fix_text(m, "tr", &FixOptions::default());
        let expected = "Mevâlî, mevla kelimesinin çoğuludur ve Alevi-Bektaşi yolunda Hazreti Aliye bağlananları karşılar.\n\
                        Kelimenin tekili veli anlamlarıyla verilir.\n\
                        Sözcüğün Arapçadaki anlam genişliği, terimin ayırt edici yanını oluşturur.\n\n\
                        Gadîr Hum\n\n\
                        Ben kimin mevlâsı isem Ali de onun mevlâsıdır hadisine dayanılarak Hazreti Alinin bendeleri.\n\
                        Mevâlî böylece bir ad hâline gelir.\n\
                        Bakınız Men küntü, Mevlahu, Velayet";
        assert_eq!(r.text, expected);
        assert!(crate::text::problems(&r.text, "text", "metin", "tr").is_empty());
        assert!(r.changes.iter().any(|x| x.kind == "citation"));
    }

    #[test]
    fn numbers_read_when_citations_off() {
        let s = FixOptions {
            citations: false,
            ..Default::default()
        };
        assert_eq!(
            fix_text("Bkz. (Korkmaz, 1994, s. 240).", "tr", &s).text,
            "Bakınız Korkmaz, bin dokuz yüz doksan dört, sayfa iki yüz kırk."
        );
    }

    #[test]
    fn pauses_and_initials_kept() {
        assert_eq!(
            d("A. Yılmaz geldi. Sonra gitti. || paragraf"),
            "A. Yılmaz geldi.\nSonra gitti. || paragraf"
        );
    }

    #[test]
    fn foreign_language_only_double_quotes() {
        let r = fix_text(
            "He said \"hello\". It's 5 pm.",
            "en",
            &FixOptions::default(),
        );
        assert_eq!(r.text, "He said hello.\nIt's 5 pm.");
    }
}
