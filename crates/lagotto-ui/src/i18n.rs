//! Traduzioni: file `i18n/<lingua>.tsv`, una riga per testo:
//! `testo inglese<TAB>traduzione`. Le righe con `#` sono commenti.
//! Stesso metodo di Joey. Il nome "Lagotto" non si traduce mai.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Lingue incluse nel programma.
pub const CATALOGS: &[(&str, &str)] = &[("it", include_str!("../i18n/it.tsv"))];

/// Lingue che si leggono da destra a sinistra.
pub const RTL: &[&str] = &["ar", "ur"];

static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
static LANG: OnceLock<String> = OnceLock::new();

pub fn parse(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| l.split_once('\t'))
        .map(|(a, b)| (a.replace("\\n", "\n"), b.replace("\\n", "\n")))
        .collect()
}

/// La lingua del sistema (LANGUAGE, LC_ALL, LC_MESSAGES, LANG), con l'inglese come ripiego.
pub fn system_language() -> String {
    for var in ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            for part in v.split(':') {
                let code = part
                    .split(['_', '.', '@'])
                    .next()
                    .unwrap_or("")
                    .to_lowercase();
                if code.is_empty() || code == "c" || code == "posix" {
                    continue;
                }
                if code == "en" || CATALOGS.iter().any(|(c, _)| *c == code) {
                    return code;
                }
            }
        }
    }
    "en".into()
}

pub fn init() {
    let lang = system_language();
    let table = CATALOGS
        .iter()
        .find(|(c, _)| *c == lang)
        .map(|(_, t)| parse(t))
        .unwrap_or_default();
    let _ = TABLE.set(table);
    let _ = LANG.set(lang);
}

pub fn lang() -> &'static str {
    LANG.get().map(String::as_str).unwrap_or("en")
}

pub fn t(s: &str) -> String {
    TABLE
        .get()
        .and_then(|m| m.get(s))
        .cloned()
        .unwrap_or_else(|| s.to_string())
}

/// Testo con segnaposto: `tf("{n} files", &[("n", "3")])`.
pub fn tf(s: &str, args: &[(&str, &str)]) -> String {
    let mut out = t(s);
    for (k, v) in args {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ogni testo usato nel programma deve esistere in ogni lingua, con gli
    /// stessi segnaposto: altrimenti la verifica fallisce.
    #[test]
    fn catalogs_complete() {
        let used = used_strings();
        for (code, text) in CATALOGS {
            let m = parse(text);
            let mut missing = vec![];
            for s in &used {
                match m.get(s) {
                    None => missing.push(s.clone()),
                    Some(tr) => {
                        for ph in placeholders(s) {
                            assert!(
                                tr.contains(&ph),
                                "{code}: placeholder {ph} missing in: {tr}"
                            );
                        }
                        assert!(!tr.contains("Lagott0"), "typo");
                    }
                }
            }
            assert!(
                missing.is_empty(),
                "{code}: {} missing:\n{}",
                missing.len(),
                missing.join("\n")
            );
        }
    }

    fn placeholders(s: &str) -> Vec<String> {
        let mut v = vec![];
        let mut rest = s;
        while let Some(a) = rest.find('{') {
            if let Some(b) = rest[a..].find('}') {
                v.push(rest[a..a + b + 1].to_string());
                rest = &rest[a + b + 1..];
            } else {
                break;
            }
        }
        v
    }

    /// Raccoglie i testi da `Tr.t("…")` nello .slint e da `t("…")`/`tf("…"` nel Rust.
    fn used_strings() -> Vec<String> {
        let mut out = vec![];
        let sources = [
            (include_str!("../ui/app.slint"), "Tr.t(\""),
            (include_str!("main.rs"), "t(\""),
            (include_str!("main.rs"), "tf(\""),
            (include_str!("../../lagotto-core/src/disks.rs"), "\""),
        ];
        for (text, marker) in sources.iter().take(3) {
            let mut rest = *text;
            while let Some(i) = rest.find(marker) {
                let after = &rest[i + marker.len()..];
                let end = find_end(after);
                let s = after[..end].replace("\\\"", "\"");
                if !s.is_empty() && !out.contains(&s) {
                    out.push(s);
                }
                rest = &after[end..];
            }
        }
        // Testi del motore mostrati all'utente.
        for s in lagotto_core::disks::USER_TEXTS
            .iter()
            .chain(lagotto_core::found::USER_TEXTS)
            .chain(lagotto_core::diag::USER_TEXTS)
            .chain(crate::WHAT.iter())
            .chain(crate::WHEN.iter())
            .chain(crate::MONTHS.iter())
        {
            if !out.contains(&s.to_string()) {
                out.push(s.to_string());
            }
        }
        let _ = sources[3];
        out
    }

    fn find_end(s: &str) -> usize {
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' {
                i += 2;
                continue;
            }
            if b[i] == b'"' {
                return i;
            }
            i += 1;
        }
        b.len()
    }
}
