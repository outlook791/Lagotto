//! Recupero: copia i file ritrovati in una cartella su un ALTRO disco,
//! senza mai sovrascrivere, togliendo i doppioni e controllando che i file
//! si aprano.

use crate::diag::LagottoError;
use crate::found::{FileKind, FileState, FoundFile};
use crate::scan::{Reader, Target};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Il disco fisico (es. /dev/sdb) su cui sta un dispositivo o una partizione.
pub fn physical_disk(dev: &str) -> Option<String> {
    let out = Command::new("lsblk")
        .args(["-s", "-n", "-o", "PATH,TYPE", dev])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .filter_map(|l| {
            let mut c = l.split_whitespace();
            Some((
                c.next()?
                    .trim_start_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '/')
                    .to_string(),
                c.next()?.to_string(),
            ))
        })
        .find(|(_, t)| t == "disk")
        .map(|(p, _)| p)
}

/// Il dispositivo su cui è montata una cartella.
pub fn device_of(dir: &Path) -> Option<String> {
    let out = Command::new("findmnt")
        .args(["-n", "-o", "SOURCE", "--target"])
        .arg(dir)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // btrfs mostra "/dev/nvme0n1p2[/home]".
    let s = s.split('[').next().unwrap_or("").to_string();
    (!s.is_empty()).then_some(s)
}

pub fn free_space(dir: &Path) -> Option<u64> {
    let out = Command::new("df")
        .args(["-B1", "--output=avail"])
        .arg(dir)
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .nth(1)?
        .trim()
        .parse()
        .ok()
}

/// La destinazione va bene? Mai sullo stesso disco della sorgente.
pub fn check_destination(
    dest: &Path,
    source: &crate::Disk,
    needed: u64,
) -> Result<(), LagottoError> {
    let meta =
        fs::metadata(dest).map_err(|_| LagottoError::BadDestination(dest.display().to_string()))?;
    if !meta.is_dir() || meta.permissions().readonly() {
        return Err(LagottoError::BadDestination(dest.display().to_string()));
    }
    if source.kind == crate::DiskKind::Image {
        // Mai scrivere dentro l'immagine stessa; accanto va bene.
        if dest.starts_with(&source.path) {
            return Err(LagottoError::SameDisk);
        }
    } else {
        let src_disk = physical_disk(&source.path.display().to_string())
            .unwrap_or_else(|| source.path.display().to_string());
        match device_of(dest).and_then(|d| physical_disk(&d).or(Some(d))) {
            Some(d) if d == src_disk => return Err(LagottoError::SameDisk),
            Some(_) => {}
            // Non sappiamo dove sta: meglio fermarsi che rischiare.
            None => return Err(LagottoError::BadDestination(dest.display().to_string())),
        }
    }
    if let Some(avail) = free_space(dest) {
        if avail < needed {
            return Err(LagottoError::NoSpace {
                needed,
                available: avail,
            });
        }
    }
    Ok(())
}

/// "nome.ext", poi "nome (1).ext", "nome (2).ext"… Mai sovrascrivere.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    (1..)
        .map(|i| dir.join(format!("{stem} ({i}){ext}")))
        .find(|p| !p.exists())
        .expect("infinite")
}

/// Controllo rapido: il file è integro? (Inizio e fine dei formati più comuni.)
pub fn validate(path: &Path, kind: FileKind) -> FileState {
    let Ok(mut f) = File::open(path) else {
        return FileState::Unknown;
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let mut head = [0u8; 8];
    if f.read(&mut head).unwrap_or(0) < 4 {
        return FileState::Damaged;
    }
    let tail = {
        use std::io::{Seek, SeekFrom};
        let n = len.min(1024);
        let mut buf = vec![0u8; n as usize];
        let _ = f.seek(SeekFrom::Start(len - n));
        let _ = f.read_exact(&mut buf);
        buf
    };
    let contains = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).any(|w| w == needle);
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" => {
            if head[..2] != [0xFF, 0xD8] {
                FileState::Damaged
            } else if contains(&tail, &[0xFF, 0xD9]) {
                FileState::Intact
            } else {
                FileState::Partial
            }
        }
        "png" => {
            if head != [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
                FileState::Damaged
            } else if contains(&tail, b"IEND") {
                FileState::Intact
            } else {
                FileState::Partial
            }
        }
        "pdf" => {
            if &head[..4] != b"%PDF" {
                FileState::Damaged
            } else if contains(&tail, b"%%EOF") {
                FileState::Intact
            } else {
                FileState::Partial
            }
        }
        "zip" | "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp" | "epub" => {
            if &head[..2] != b"PK" {
                FileState::Damaged
            } else if contains(&tail, b"PK\x05\x06") {
                FileState::Intact
            } else {
                FileState::Partial
            }
        }
        _ => {
            // Un file tutto a zeri è vuoto: lo spazio è stato svuotato (TRIM).
            if head.iter().all(|b| *b == 0) && tail.iter().all(|b| *b == 0) {
                FileState::Damaged
            } else {
                let _ = kind;
                FileState::Unknown
            }
        }
    }
}

/// Data di scatto dalla foto ("2026-08-12 10.30.15"), se `exiftool` c'è.
pub fn photo_date(path: &Path) -> Option<String> {
    crate::tools::which("exiftool")?;
    let out = Command::new("exiftool")
        .args(["-s3", "-d", "%Y-%m-%d %H.%M.%S", "-DateTimeOriginal"])
        .arg(path)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (s.len() >= 10).then_some(s)
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct Summary {
    pub recovered: usize,
    pub bytes: u64,
    pub damaged: usize,
    pub duplicates: usize,
    pub failed: usize,
    pub folder: PathBuf,
}

pub struct Options {
    /// Nome della cartella principale ("Lagotto 2026-11-27").
    pub folder_name: String,
    /// Nome tradotto della sottocartella di ogni tipo.
    pub kind_folder: Box<dyn Fn(FileKind) -> String + Send + Sync>,
    /// Nome tradotto della cartella dei file danneggiati.
    pub damaged_folder: String,
}

/// Recupera i file scelti. `progress(fatti, totale)` a ogni file.
pub fn recover(
    files: &[&FoundFile],
    targets: &[Target],
    reader: &dyn Reader,
    dest: &Path,
    opts: &Options,
    mut progress: impl FnMut(usize, usize),
) -> Result<Summary, LagottoError> {
    let root = unique_path(dest, &opts.folder_name);
    fs::create_dir_all(&root)
        .map_err(|_| LagottoError::BadDestination(root.display().to_string()))?;
    let mut sum = Summary {
        folder: root.clone(),
        ..Default::default()
    };
    let mut seen: HashSet<[u8; 32]> = HashSet::new();
    for (i, f) in files.iter().enumerate() {
        progress(i, files.len());
        let Some(t) = f.source.parse::<usize>().ok().and_then(|i| targets.get(i)) else {
            sum.failed += 1;
            continue;
        };
        let dir = root.join((opts.kind_folder)(f.kind));
        let sub = f
            .path
            .trim_start_matches('/')
            .rsplit_once('/')
            .map(|(d, _)| d.to_string());
        let dir = match sub {
            Some(s) if !s.starts_with('$') => dir.join(s),
            _ => dir,
        };
        if fs::create_dir_all(&dir).is_err() {
            sum.failed += 1;
            continue;
        }
        let tmp = dir.join(format!(".lagotto-{}.part", f.id));
        let res = (|| -> Result<[u8; 32], LagottoError> {
            let mut out = HashWriter {
                file: File::create(&tmp)
                    .map_err(|_| LagottoError::BadDestination(dir.display().to_string()))?,
                hash: Sha256::new(),
            };
            reader.cat(t, &f.inode, &mut out)?;
            out.file.sync_all().ok();
            Ok(out.hash.finalize().into())
        })();
        let hash = match res {
            Ok(h) => h,
            Err(_) => {
                let _ = fs::remove_file(&tmp);
                sum.failed += 1;
                continue;
            }
        };
        if !seen.insert(hash) {
            let _ = fs::remove_file(&tmp);
            sum.duplicates += 1;
            continue;
        }
        let mut name = f.name().to_string();
        if f.kind == FileKind::Photo && f.name_is_partial() {
            if let Some(d) = photo_date(&tmp) {
                let ext = name
                    .rsplit_once('.')
                    .map(|(_, e)| e.to_string())
                    .unwrap_or_else(|| "jpg".into());
                name = format!("{d}.{ext}");
            }
        }
        let tmp_named = tmp.with_file_name(format!(
            ".lagotto-{}.{}",
            f.id,
            name.rsplit_once('.').map(|(_, e)| e).unwrap_or("bin")
        ));
        let _ = fs::rename(&tmp, &tmp_named);
        let state = validate(&tmp_named, f.kind);
        let final_dir = if state == FileState::Damaged {
            let d = root.join(&opts.damaged_folder);
            let _ = fs::create_dir_all(&d);
            sum.damaged += 1;
            d
        } else {
            dir
        };
        let target = unique_path(&final_dir, &name);
        if fs::rename(&tmp_named, &target).is_err() {
            let _ = fs::remove_file(&tmp_named);
            sum.failed += 1;
            continue;
        }
        sum.recovered += 1;
        sum.bytes += f.size;
    }
    progress(files.len(), files.len());
    Ok(sum)
}

struct HashWriter {
    file: File,
    hash: Sha256,
}

impl Write for HashWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.file.write(buf)?;
        self.hash.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_overwrites() {
        let d = std::env::temp_dir().join(format!("lagotto-test-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("foto.jpg"), b"x").unwrap();
        fs::write(d.join("foto (1).jpg"), b"x").unwrap();
        assert_eq!(unique_path(&d, "foto.jpg"), d.join("foto (2).jpg"));
        assert_eq!(unique_path(&d, "nuovo.jpg"), d.join("nuovo.jpg"));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn validation() {
        let d = std::env::temp_dir().join(format!("lagotto-val-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let ok = d.join("a.jpg");
        fs::write(&ok, [0xFF, 0xD8, 1, 2, 3, 4, 0xFF, 0xD9]).unwrap();
        assert_eq!(validate(&ok, FileKind::Photo), FileState::Intact);
        let cut = d.join("b.jpg");
        fs::write(&cut, [0xFF, 0xD8, 1, 2, 3, 4, 5, 6]).unwrap();
        assert_eq!(validate(&cut, FileKind::Photo), FileState::Partial);
        let zero = d.join("c.dat");
        fs::write(&zero, [0u8; 64]).unwrap();
        assert_eq!(validate(&zero, FileKind::Other), FileState::Damaged);
        fs::remove_dir_all(&d).unwrap();
    }
}
