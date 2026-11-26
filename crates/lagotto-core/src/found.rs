//! I file ritrovati.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum FileKind {
    Photo,
    Video,
    Document,
    Music,
    Archive,
    Other,
}

impl FileKind {
    pub fn from_name(name: &str) -> FileKind {
        let ext = name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "jpg" | "jpeg" | "png" | "gif" | "heic" | "heif" | "webp" | "tif" | "tiff" | "bmp"
            | "cr2" | "cr3" | "nef" | "arw" | "dng" | "raf" | "orf" | "rw2" | "avif" => {
                FileKind::Photo
            }
            "mp4" | "mov" | "m4v" | "avi" | "mkv" | "mts" | "m2ts" | "3gp" | "webm" | "wmv"
            | "mpg" | "mpeg" => FileKind::Video,
            "pdf" | "doc" | "docx" | "odt" | "rtf" | "txt" | "md" | "xls" | "xlsx" | "ods"
            | "csv" | "ppt" | "pptx" | "odp" | "epub" | "pages" | "numbers" | "key" => {
                FileKind::Document
            }
            "mp3" | "flac" | "ogg" | "opus" | "m4a" | "aac" | "wav" | "wma" | "aiff" => {
                FileKind::Music
            }
            "zip" | "7z" | "rar" | "tar" | "gz" | "xz" | "bz2" | "zst" => FileKind::Archive,
            _ => FileKind::Other,
        }
    }

    /// Nome del gruppo (testo sorgente inglese).
    pub fn label(&self) -> &'static str {
        match self {
            FileKind::Photo => "Photos",
            FileKind::Video => "Videos",
            FileKind::Document => "Documents",
            FileKind::Music => "Music",
            FileKind::Archive => "Archives",
            FileKind::Other => "Other",
        }
    }
}

/// Testi mostrati all'utente (per il controllo delle traduzioni).
pub const USER_TEXTS: &[&str] = &[
    "Photos",
    "Videos",
    "Documents",
    "Music",
    "Archives",
    "Other",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FileState {
    /// Contenuto controllato e integro.
    Intact,
    /// Si apre solo in parte.
    Partial,
    /// Danneggiato.
    Damaged,
    /// Non ancora controllato.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Origin {
    /// Trovato nel filesystem, con nome e cartella (Sleuth Kit).
    Names,
    /// Ricostruito dal contenuto grezzo (PhotoRec), senza nome.
    Carved,
}

#[derive(Debug, Clone, Serialize)]
pub struct FoundFile {
    pub id: usize,
    /// Partizione o immagine da cui viene.
    pub source: String,
    /// Indirizzo interno per Sleuth Kit (inode, o "inode-tipo-id" su NTFS).
    pub inode: String,
    /// Percorso originale, quando si conosce ("/DCIM/IMG_0001.JPG").
    pub path: String,
    pub size: u64,
    /// Data di modifica (secondi dal 1970), 0 se sconosciuta.
    pub mtime: i64,
    pub kind: FileKind,
    pub state: FileState,
    pub origin: Origin,
    /// Il file era cancellato (non solo nascosto o in un filesystem rovinato).
    pub deleted: bool,
}

impl FoundFile {
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// Il primo carattere dei nomi cancellati su FAT si perde (diventa "_"):
    /// lo diciamo all'utente invece di inventarlo.
    pub fn name_is_partial(&self) -> bool {
        self.deleted && self.name().starts_with('_')
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kinds() {
        assert_eq!(FileKind::from_name("IMG_0001.JPG"), FileKind::Photo);
        assert_eq!(FileKind::from_name("clip.MOV"), FileKind::Video);
        assert_eq!(FileKind::from_name("contratto.pdf"), FileKind::Document);
        assert_eq!(FileKind::from_name("senza_estensione"), FileKind::Other);
    }
}
