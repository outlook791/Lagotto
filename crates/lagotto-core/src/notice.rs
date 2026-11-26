//! L'avviso di responsabilità da accettare prima di usare Lagotto
//! (interfaccia, riga di comando, MCP). Se il testo cambia, si alza la
//! versione e l'avviso si ripresenta.

use std::path::PathBuf;

pub const VERSION: u32 = 1;

/// Testo sorgente in inglese; le traduzioni stanno nei file della lingua.
pub const TITLE: &str = "Before you start";
pub const INTRO: &str =
    "Lagotto is free and open source, and it is provided as is, without any warranty.";
pub const POINTS: [&str; 4] = [
    "Recovery is not guaranteed: some files may be impossible to recover.",
    "The authors are not liable for data loss, damage to disks or any other damage resulting from the use of Lagotto.",
    "By using it you take on every risk and every responsibility.",
    "For irreplaceable data, go to a professional recovery lab first.",
];
pub const CHECKBOX: &str = "I have read and accept: I use Lagotto at my own risk.";
pub const REFUSED_TITLE: &str = "Lagotto cannot continue";
pub const REFUSED_TEXT: &str =
    "Without your acceptance Lagotto cannot be used. It stays free and open source: you can read its code and license whenever you like.";

fn file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("lagotto").join("accepted"))
}

/// L'utente ha già accettato questa versione dell'avviso?
pub fn accepted() -> bool {
    file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|s| s.trim().parse::<u32>().ok())
        .map(|v| v >= VERSION)
        .unwrap_or(false)
}

pub fn accept() -> std::io::Result<()> {
    let f = file().ok_or_else(|| std::io::Error::other("no HOME"))?;
    std::fs::create_dir_all(f.parent().unwrap())?;
    std::fs::write(f, VERSION.to_string())
}

/// Testo completo in inglese (riga di comando, README).
pub fn full_text() -> String {
    let mut s = format!("{TITLE}\n\n{INTRO}\n");
    for p in POINTS {
        s.push_str(&format!("  - {p}\n"));
    }
    s
}

/// Siamo già amministratori?
pub fn is_root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .map(|l| l.split_whitespace().nth(2) == Some("0"))
        })
        .unwrap_or(false)
}
