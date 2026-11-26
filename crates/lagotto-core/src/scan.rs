//! Scansione: trova i file cancellati con il loro nome (Sleuth Kit) e,
//! a richiesta, quelli senza nome (PhotoRec, in `carve`).
//!
//! Tutto in sola lettura: `mmls`, `fls` e `icat` leggono soltanto.

use crate::diag::LagottoError;
use crate::found::{FileKind, FileState, FoundFile, Origin};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Una zona da esaminare: una partizione, o un'immagine con uno scostamento.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub path: PathBuf,
    /// Scostamento in settori (per le immagini di un disco intero).
    pub offset: Option<u64>,
    pub label: String,
}

/// Chi legge davvero il disco: direttamente (immagini, o se siamo già
/// amministratori) oppure attraverso `lagotto-helper` con la password.
pub trait Reader: Send + Sync {
    /// Uscita di `fls -r -d -m / [-o N] percorso` (formato "body").
    fn list_deleted(&self, t: &Target) -> Result<String, LagottoError>;
    /// Uscita di `mmls percorso` (tabella delle partizioni di un'immagine).
    fn partitions(&self, path: &Path) -> Result<String, LagottoError>;
    /// Contenuto del file `inode` scritto in `out` (`icat`).
    fn cat(&self, t: &Target, inode: &str, out: &mut dyn Write) -> Result<u64, LagottoError>;
}

/// Lettore diretto: esegue Sleuth Kit con i permessi di chi lo chiama.
pub struct DirectReader;

fn run_capture(cmd: &str, args: &[String]) -> Result<String, LagottoError> {
    crate::tools::require(cmd)?;
    let out = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| LagottoError::ToolFailed {
            tool: cmd.into(),
            detail: e.to_string(),
        })?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if err.contains("Permission denied") {
            return Err(LagottoError::NotAuthorized);
        }
        if err.contains("Cannot determine file system type") || err.contains("Possible encryption")
        {
            return Err(LagottoError::UnknownFilesystem);
        }
        return Err(LagottoError::ToolFailed {
            tool: cmd.into(),
            detail: err.lines().last().unwrap_or("").into(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn fls_args(t: &Target) -> Vec<String> {
    let mut a = vec!["-r".to_string(), "-d".into(), "-m".into(), "/".into()];
    if let Some(o) = t.offset {
        a.push("-o".into());
        a.push(o.to_string());
    }
    a.push(t.path.display().to_string());
    a
}

pub fn icat_args(t: &Target, inode: &str) -> Vec<String> {
    let mut a = vec![];
    if let Some(o) = t.offset {
        a.push("-o".to_string());
        a.push(o.to_string());
    }
    a.push(t.path.display().to_string());
    a.push(inode.to_string());
    a
}

impl Reader for DirectReader {
    fn list_deleted(&self, t: &Target) -> Result<String, LagottoError> {
        run_capture("fls", &fls_args(t))
    }
    fn partitions(&self, path: &Path) -> Result<String, LagottoError> {
        run_capture("mmls", &[path.display().to_string()])
    }
    fn cat(&self, t: &Target, inode: &str, out: &mut dyn Write) -> Result<u64, LagottoError> {
        crate::tools::require("icat")?;
        let mut child = Command::new("icat")
            .args(icat_args(t, inode))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| LagottoError::ToolFailed {
                tool: "icat".into(),
                detail: e.to_string(),
            })?;
        let mut so = child.stdout.take().expect("stdout");
        let n = std::io::copy(&mut so, out).map_err(|e| LagottoError::ToolFailed {
            tool: "icat".into(),
            detail: e.to_string(),
        })?;
        let st = child.wait().map_err(|e| LagottoError::ToolFailed {
            tool: "icat".into(),
            detail: e.to_string(),
        })?;
        if !st.success() {
            return Err(LagottoError::ToolFailed {
                tool: "icat".into(),
                detail: format!("exit {st}"),
            });
        }
        Ok(n)
    }
}

/// Interpreta `mmls`: restituisce gli scostamenti delle partizioni con dati.
pub fn parse_mmls(text: &str) -> Vec<(u64, u64, String)> {
    let mut out = vec![];
    for line in text.lines() {
        // "002:  000:000   0000002048   0000206847   0000204800   Linux (0x83)"
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 6 || !cols[0].ends_with(':') {
            continue;
        }
        if cols[1].starts_with("Meta") || cols[1] == "-------" {
            continue;
        }
        let (Ok(start), Ok(len)) = (cols[2].parse::<u64>(), cols[4].parse::<u64>()) else {
            continue;
        };
        let desc = cols[5..].join(" ");
        if desc.contains("Unallocated")
            || desc.contains("Primary Table")
            || desc.contains("Extended")
        {
            continue;
        }
        out.push((start, len, desc));
    }
    out
}

/// Le zone da esaminare per un disco o un'immagine.
pub fn targets(disk: &crate::Disk, reader: &dyn Reader) -> Vec<Target> {
    if !disk.partitions.is_empty() {
        return disk
            .partitions
            .iter()
            .map(|p| Target {
                path: p.path.clone(),
                offset: None,
                label: p
                    .label
                    .clone()
                    .unwrap_or_else(|| p.path.display().to_string()),
            })
            .collect();
    }
    // Immagine o disco senza partizioni note: proviamo la tabella delle partizioni.
    if let Ok(text) = reader.partitions(&disk.path) {
        let parts = parse_mmls(&text);
        if !parts.is_empty() {
            return parts
                .into_iter()
                .map(|(start, _, desc)| Target {
                    path: disk.path.clone(),
                    offset: Some(start),
                    label: desc,
                })
                .collect();
        }
    }
    vec![Target {
        path: disk.path.clone(),
        offset: None,
        label: disk.model.clone(),
    }]
}

/// Interpreta il formato "body" di `fls -m`.
pub fn parse_body(text: &str, source: &str, first_id: usize) -> Vec<FoundFile> {
    let mut out = vec![];
    for line in text.lines() {
        let f: Vec<&str> = line.split('|').collect();
        if f.len() < 11 {
            continue;
        }
        let n = f.len();
        let name = f[1..n - 9].join("|");
        let inode = f[n - 9];
        let mode = f[n - 8];
        if !mode.starts_with("r/") && !mode.starts_with("-/r") {
            continue; // solo file normali
        }
        let size: u64 = f[n - 5].parse().unwrap_or(0);
        if size == 0 {
            continue;
        }
        let mtime: i64 = f[n - 3].parse().unwrap_or(0);
        let deleted = name.ends_with(" (deleted)") || name.ends_with(" (deleted-realloc)");
        if name.ends_with(" (deleted-realloc)") {
            continue; // lo spazio è già stato riusato: il contenuto non è più quello
        }
        let clean = name.trim_end_matches(" (deleted)").to_string();
        if clean.contains("/$OrphanFiles/") && clean.ends_with("OrphanFile-") {
            continue;
        }
        out.push(FoundFile {
            id: first_id + out.len(),
            source: source.to_string(),
            inode: inode.to_string(),
            kind: FileKind::from_name(&clean),
            path: clean,
            size,
            mtime,
            state: FileState::Unknown,
            origin: Origin::Names,
            deleted,
        });
    }
    out
}

/// Cosa succede durante la scansione: il "diario del fiuto".
#[derive(Debug, Clone)]
pub enum Event {
    /// Inizio di una zona (indice, totale, nome).
    Zone {
        index: usize,
        total: usize,
        label: String,
    },
    /// Una cartella con molti file ritrovati.
    Folder {
        path: String,
        count: usize,
        kind: FileKind,
    },
    /// File trovati finora.
    Found {
        total: usize,
    },
    /// Una zona non si legge come filesystem.
    Unreadable {
        label: String,
        error: LagottoError,
    },
    Done {
        total: usize,
    },
}

/// Scansione con i nomi di tutte le zone di un disco.
pub fn scan(
    disk: &crate::Disk,
    reader: &dyn Reader,
    stop: &dyn Fn() -> bool,
    mut on: impl FnMut(Event),
) -> (Vec<FoundFile>, Vec<Target>) {
    let zones = targets(disk, reader);
    let mut all: Vec<FoundFile> = vec![];
    for (i, t) in zones.iter().enumerate() {
        if stop() {
            break;
        }
        on(Event::Zone {
            index: i,
            total: zones.len(),
            label: t.label.clone(),
        });
        match reader.list_deleted(t) {
            Ok(body) => {
                let src = i.to_string();
                let files = parse_body(&body, &src, all.len());
                // Le cartelle più ricche, per il diario.
                let mut by_dir: BTreeMap<String, (usize, BTreeMap<FileKind, usize>)> =
                    BTreeMap::new();
                for f in &files {
                    let dir = f
                        .path
                        .rsplit_once('/')
                        .map(|(d, _)| d)
                        .unwrap_or("")
                        .to_string();
                    let e = by_dir
                        .entry(if dir.is_empty() { "/".into() } else { dir })
                        .or_default();
                    e.0 += 1;
                    *e.1.entry(f.kind).or_default() += 1;
                }
                let mut dirs: Vec<_> = by_dir.into_iter().collect();
                dirs.sort_by_key(|d| std::cmp::Reverse(d.1 .0));
                for (path, (count, kinds)) in dirs.into_iter().take(3) {
                    let kind = kinds
                        .into_iter()
                        .max_by_key(|(_, n)| *n)
                        .map(|(k, _)| k)
                        .unwrap_or(FileKind::Other);
                    on(Event::Folder { path, count, kind });
                }
                all.extend(files);
                on(Event::Found { total: all.len() });
            }
            Err(e) => on(Event::Unreadable {
                label: t.label.clone(),
                error: e,
            }),
        }
    }
    on(Event::Done { total: all.len() });
    (all, zones)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_lines() {
        let body = "0|/DCIM/_.jpg (deleted)|517|r/rrwxrwxrwx|0|0|140563|1795708800|1795716764|0|1795716764\n\
                    0|/_oc.txt (deleted)|5|r/rrwxrwxrwx|0|0|3800|1795708800|1795716764|0|1795716764\n\
                    0|/old (deleted)|9|d/drwxrwxrwx|0|0|2048|0|0|0|0\n\
                    0|/re.jpg (deleted-realloc)|10|r/rrwxrwxrwx|0|0|99|0|0|0|0\n\
                    0|/a|b.pdf (deleted)|11|r/rrwxrwxrwx|0|0|99|0|1795716764|0|0\n";
        let f = parse_body(body, "0", 0);
        assert_eq!(f.len(), 3);
        assert_eq!(f[0].path, "/DCIM/_.jpg");
        assert_eq!(f[0].inode, "517");
        assert_eq!(f[0].kind, FileKind::Photo);
        assert!(f[0].deleted && f[0].name_is_partial());
        assert_eq!(f[2].path, "/a|b.pdf");
    }

    #[test]
    fn mmls_table() {
        let t = "DOS Partition Table\nOffset Sector: 0\nUnits are in 512-byte sectors\n\n      Slot      Start        End          Length       Description\n000:  Meta      0000000000   0000000000   0000000001   Primary Table (#0)\n001:  -------   0000000000   0000002047   0000002048   Unallocated\n002:  000:000   0000002048   0000067583   0000065536   Win95 FAT32 (0x0c)\n";
        let p = parse_mmls(t);
        assert_eq!(p, vec![(2048, 65536, "Win95 FAT32 (0x0c)".to_string())]);
    }
}
