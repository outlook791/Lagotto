//! Programmi esterni collaudati usati da Lagotto.

use crate::diag::LagottoError;
use std::path::PathBuf;

/// Programmi richiesti e il pacchetto che li contiene (nomi Fedora / Debian).
pub const REQUIRED: &[(&str, &str)] = &[
    ("fls", "sleuthkit"),
    ("icat", "sleuthkit"),
    ("photorec", "testdisk"),
    ("lsblk", "util-linux"),
];

/// Programmi facoltativi: migliorano il risultato ma non sono indispensabili.
pub const OPTIONAL: &[(&str, &str)] = &[
    ("exiftool", "perl-Image-ExifTool / libimage-exiftool-perl"),
    ("smartctl", "smartmontools"),
];

pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

pub fn require(name: &str) -> Result<PathBuf, LagottoError> {
    which(name).ok_or_else(|| LagottoError::MissingTool(name.to_string()))
}

/// Elenco dei programmi mancanti tra quelli richiesti.
pub fn missing_required() -> Vec<(&'static str, &'static str)> {
    REQUIRED
        .iter()
        .copied()
        .filter(|(n, _)| which(n).is_none())
        .collect()
}
