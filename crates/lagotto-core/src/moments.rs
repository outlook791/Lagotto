//! "Momenti ritrovati": foto e video vicini nel tempo diventano un gruppo.
//! Nessun nome inventato: solo le date che i file contengono.

use crate::found::{FileKind, FoundFile};
use serde::Serialize;

/// Due file a più di 36 ore di distanza stanno in momenti diversi.
pub const GAP_SECONDS: i64 = 36 * 3600;
/// Meno di 5 foto o video non fanno un momento.
pub const MIN_FILES: usize = 5;

#[derive(Debug, Clone, Serialize)]
pub struct Moment {
    pub start: i64,
    pub end: i64,
    pub photos: usize,
    pub videos: usize,
    pub files: Vec<usize>,
}

/// Data civile (anno, mese 1-12, giorno) da secondi dal 1970, UTC.
pub fn ymd(secs: i64) -> (i64, u32, u32) {
    let days = secs.div_euclid(86_400);
    // Algoritmo di Howard Hinnant ("civil_from_days").
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Raggruppa i file in momenti. Restituisce i momenti (dal più recente) e
/// gli id dei file che non stanno in nessun momento.
pub fn group(files: &[FoundFile]) -> (Vec<Moment>, Vec<usize>) {
    let mut media: Vec<&FoundFile> = files
        .iter()
        .filter(|f| matches!(f.kind, FileKind::Photo | FileKind::Video) && f.mtime > 86_400 * 365)
        .collect();
    media.sort_by_key(|f| f.mtime);
    let mut moments: Vec<Moment> = vec![];
    let mut cur: Vec<&FoundFile> = vec![];
    let flush = |cur: &mut Vec<&FoundFile>, moments: &mut Vec<Moment>| {
        if cur.len() >= MIN_FILES {
            moments.push(Moment {
                start: cur.first().unwrap().mtime,
                end: cur.last().unwrap().mtime,
                photos: cur.iter().filter(|f| f.kind == FileKind::Photo).count(),
                videos: cur.iter().filter(|f| f.kind == FileKind::Video).count(),
                files: cur.iter().map(|f| f.id).collect(),
            });
        }
        cur.clear();
    };
    for f in media {
        if let Some(last) = cur.last() {
            if f.mtime - last.mtime > GAP_SECONDS {
                flush(&mut cur, &mut moments);
            }
        }
        cur.push(f);
    }
    flush(&mut cur, &mut moments);
    moments.reverse();
    let inside: std::collections::HashSet<usize> = moments
        .iter()
        .flat_map(|m| m.files.iter().copied())
        .collect();
    let rest = files
        .iter()
        .map(|f| f.id)
        .filter(|id| !inside.contains(id))
        .collect();
    (moments, rest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::found::{FileState, Origin};

    fn file(id: usize, name: &str, mtime: i64) -> FoundFile {
        FoundFile {
            id,
            source: "0".into(),
            inode: id.to_string(),
            path: format!("/{name}"),
            size: 10,
            mtime,
            kind: FileKind::from_name(name),
            state: FileState::Unknown,
            origin: Origin::Names,
            deleted: true,
        }
    }

    #[test]
    fn dates() {
        assert_eq!(ymd(0), (1970, 1, 1));
        assert_eq!(ymd(1_786_492_800), (2026, 8, 12));
        assert_eq!(ymd(951_782_400), (2000, 2, 29));
    }

    #[test]
    fn groups_by_gap() {
        let aug = 1_786_492_800; // 12 agosto 2026
        let sep = aug + 22 * 86_400;
        let mut f = vec![];
        for i in 0..6 {
            f.push(file(i, &format!("a{i}.jpg"), aug + i as i64 * 3600));
        }
        f.push(file(6, "v.mp4", aug + 30 * 3600));
        for i in 7..10 {
            f.push(file(i, &format!("b{i}.jpg"), sep + i as i64));
        }
        f.push(file(10, "doc.pdf", aug));
        let (m, rest) = group(&f);
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].photos, m[0].videos), (6, 1));
        assert_eq!(rest, vec![7, 8, 9, 10]);
    }
}
