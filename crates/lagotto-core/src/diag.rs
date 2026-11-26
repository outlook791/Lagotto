//! Codici di errore di Lagotto (serie L101–L999): codice, significato, cosa fare.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LagottoError {
    /// L101: manca un programma necessario.
    MissingTool(String),
    /// L102: la destinazione sta sullo stesso disco da cui si recupera.
    SameDisk,
    /// L103: spazio insufficiente nella destinazione.
    NoSpace { needed: u64, available: u64 },
    /// L104: la sorgente non esiste o non si legge.
    SourceUnreadable(String),
    /// L105: il filesystem non è riconosciuto (si può ancora cercare senza nomi).
    UnknownFilesystem,
    /// L106: un programma esterno è fallito.
    ToolFailed { tool: String, detail: String },
    /// L107: permessi di amministratore negati.
    NotAuthorized,
    /// L108: la destinazione non è una cartella scrivibile.
    BadDestination(String),
}

impl LagottoError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingTool(_) => "L101",
            Self::SameDisk => "L102",
            Self::NoSpace { .. } => "L103",
            Self::SourceUnreadable(_) => "L104",
            Self::UnknownFilesystem => "L105",
            Self::ToolFailed { .. } => "L106",
            Self::NotAuthorized => "L107",
            Self::BadDestination(_) => "L108",
        }
    }

    /// Cosa fare, in inglese (testo sorgente per le traduzioni).
    pub fn hint(&self) -> &'static str {
        match self {
            Self::MissingTool(_) => "Install the missing program and try again.",
            Self::SameDisk => "Choose a folder on a different disk, so the lost files are not overwritten.",
            Self::NoSpace { .. } => "Free some space or choose a bigger disk.",
            Self::SourceUnreadable(_) => "Check that the disk is connected; if it makes noises, stop and make an image first.",
            Self::UnknownFilesystem => "Lagotto can still search for files without their names.",
            Self::ToolFailed { .. } => "Try again; if it happens again, send a report.",
            Self::NotAuthorized => "Lagotto needs your password to read disks.",
            Self::BadDestination(_) => "Choose a folder you can write to.",
        }
    }
}

/// Testi mostrati all'utente (per il controllo delle traduzioni).
pub const USER_TEXTS: &[&str] = &[
    "Install the missing program and try again.",
    "Choose a folder on a different disk, so the lost files are not overwritten.",
    "Free some space or choose a bigger disk.",
    "Check that the disk is connected; if it makes noises, stop and make an image first.",
    "Lagotto can still search for files without their names.",
    "Try again; if it happens again, send a report.",
    "Lagotto needs your password to read disks.",
    "Choose a folder you can write to.",
];

impl fmt::Display for LagottoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self {
            Self::MissingTool(t) => format!("missing program: {t}"),
            Self::SameDisk => "the destination is on the same disk you are recovering from".into(),
            Self::NoSpace { needed, available } => {
                format!("not enough space: {needed} bytes needed, {available} available")
            }
            Self::SourceUnreadable(s) => format!("cannot read {s}"),
            Self::UnknownFilesystem => "file system not recognised".into(),
            Self::ToolFailed { tool, detail } => format!("{tool} failed: {detail}"),
            Self::NotAuthorized => "administrator permission denied".into(),
            Self::BadDestination(d) => format!("cannot write to {d}"),
        };
        write!(f, "{} {}. {}", self.code(), what, self.hint())
    }
}

impl std::error::Error for LagottoError {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codes_are_unique() {
        let all = [
            LagottoError::MissingTool("x".into()),
            LagottoError::SameDisk,
            LagottoError::NoSpace {
                needed: 1,
                available: 0,
            },
            LagottoError::SourceUnreadable("x".into()),
            LagottoError::UnknownFilesystem,
            LagottoError::ToolFailed {
                tool: "x".into(),
                detail: "y".into(),
            },
            LagottoError::NotAuthorized,
            LagottoError::BadDestination("x".into()),
        ];
        let mut codes: Vec<_> = all.iter().map(|e| e.code()).collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), all.len());
    }
}
