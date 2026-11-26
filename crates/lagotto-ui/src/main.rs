//! Lagotto: l'interfaccia (Slint). Collega le schermate al motore.

mod i18n;

use i18n::{t, tf};
use lagotto_core::disks::{human_size, list_disks};
use lagotto_core::found::{FileKind, FileState, FoundFile};
use lagotto_core::helper::HelperReader;
use lagotto_core::moments::{group, ymd, Moment};
use lagotto_core::recover::{check_destination, recover, Options};
use lagotto_core::scan::{scan, DirectReader, Event, Reader, Target};
use lagotto_core::{notice, Disk, DiskKind, LagottoError, Probability};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

slint::include_modules!();

const CURLS: usize = 200;
const MAX_TILES: usize = 600;

#[derive(Default)]
struct St {
    disks: Vec<Disk>,
    selected: Option<usize>,
    what: usize,
    when: usize,
    files: Vec<FoundFile>,
    targets: Vec<Target>,
    reader: Option<Arc<dyn Reader>>,
    chip_kinds: Vec<Option<FileKind>>,
    chip: usize,
    moments: Vec<Moment>,
    moment: Option<usize>,
    shown: Vec<usize>,
    selection: HashSet<usize>,
    dest: Option<PathBuf>,
}

type Shared = Arc<Mutex<St>>;

fn sv(s: impl Into<SharedString>) -> SharedString {
    s.into()
}

fn err_text(e: &LagottoError) -> String {
    format!("{} — {}", e.code(), t(e.hint()))
}

pub(crate) const WHAT: [&str; 4] = ["photos and videos", "documents", "music", "everything"];
pub(crate) const WHEN: [&str; 3] = ["from any time", "from this year", "from last year"];

fn what_matches(what: usize, k: FileKind) -> bool {
    match what {
        0 => matches!(k, FileKind::Photo | FileKind::Video),
        1 => matches!(k, FileKind::Document),
        2 => matches!(k, FileKind::Music),
        _ => true,
    }
}

fn year_now() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    ymd(secs).0
}

fn when_matches(when: usize, mtime: i64) -> bool {
    match when {
        1 => ymd(mtime).0 == year_now(),
        2 => ymd(mtime).0 == year_now() - 1,
        _ => true,
    }
}

pub(crate) const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn date_label(secs: i64) -> String {
    let (y, m, d) = ymd(secs);
    tf(
        "{d} {month} {y}",
        &[
            ("d", &d.to_string()),
            ("month", &t(MONTHS[m as usize - 1])),
            ("y", &y.to_string()),
        ],
    )
}

fn moment_title(m: &Moment) -> String {
    let (y1, m1, d1) = ymd(m.start);
    let (y2, m2, d2) = ymd(m.end);
    if (y1, m1, d1) == (y2, m2, d2) {
        date_label(m.start)
    } else if (y1, m1) == (y2, m2) {
        tf(
            "{d1}–{d2} {month} {y}",
            &[
                ("d1", &d1.to_string()),
                ("d2", &d2.to_string()),
                ("month", &t(MONTHS[m1 as usize - 1])),
                ("y", &y1.to_string()),
            ],
        )
    } else {
        format!("{} – {}", date_label(m.start), date_label(m.end))
    }
}

fn moment_subtitle(m: &Moment) -> String {
    let mut parts = vec![];
    if m.photos > 0 {
        parts.push(tf("{n} photos", &[("n", &m.photos.to_string())]));
    }
    if m.videos > 0 {
        parts.push(tf("{n} videos", &[("n", &m.videos.to_string())]));
    }
    parts.join(" · ")
}

fn disk_cards(disks: &[Disk]) -> Vec<DiskCard> {
    disks
        .iter()
        .map(|d| {
            let (p, why) = d.probability();
            let mut subtitle = format!("{} · {}", if d.model.is_empty() { t(d.kind_label()) } else { d.model.clone() }, human_size(d.size));
            if d.system {
                subtitle.push_str(" · ");
                subtitle.push_str(&t("in use by the system"));
            }
            DiskCard {
                title: sv(t(d.kind_label())),
                subtitle: sv(subtitle),
                kind: match d.kind {
                    DiskKind::UsbStick => 0,
                    DiskKind::SdCard => 1,
                    DiskKind::Hdd => 2,
                    DiskKind::Ssd => 3,
                    DiskKind::Image => 4,
                    DiskKind::Other => 5,
                },
                prob: match p {
                    Probability::High => 2,
                    Probability::Medium => 1,
                    Probability::Low => 0,
                },
                reason: sv(if d.system {
                    t("This disk holds the running system: anything you do may overwrite files. Better to use the Lagotto live USB.")
                } else {
                    t(why)
                }),
                system: d.system,
            }
        })
        .collect()
}

fn refresh_disks(ui: &AppWindow, st: &Shared) {
    let images: Vec<Disk> = st
        .lock()
        .unwrap()
        .disks
        .iter()
        .filter(|d| d.kind == DiskKind::Image)
        .cloned()
        .collect();
    let mut disks = match list_disks() {
        Ok(d) => d,
        Err(e) => {
            ui.set_status_text(sv(format!("lsblk: {e}")));
            vec![]
        }
    };
    // Prima i dischi rimovibili (chiavette, schede), poi gli altri.
    disks.sort_by_key(|d| (d.system, !d.removable));
    let mut all = images;
    all.extend(disks);
    let disks = all;
    let mut s = st.lock().unwrap();
    s.selected = if disks.is_empty() {
        None
    } else {
        Some(disks.iter().position(|d| !d.system).unwrap_or(0))
    };
    ui.set_disks(ModelRc::new(VecModel::from(disk_cards(&disks))));
    ui.set_selected_disk(s.selected.map(|i| i as i32).unwrap_or(-1));
    s.disks = disks;
    update_sentence(ui, &s);
}

fn update_sentence(ui: &AppWindow, s: &St) {
    ui.set_what_label(sv(t(WHAT[s.what])));
    ui.set_when_label(sv(t(WHEN[s.when])));
    let wl = s
        .selected
        .and_then(|i| s.disks.get(i))
        .map(|d| {
            let l = t(d.kind_label());
            let mut c = l.chars();
            c.next()
                .map(|f| f.to_lowercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .unwrap_or_else(|| "…".into());
    ui.set_where_label(sv(wl));
}

fn filtered(s: &St) -> Vec<usize> {
    s.files
        .iter()
        .filter(|f| what_matches(s.what, f.kind) && when_matches(s.when, f.mtime))
        .map(|f| f.id)
        .collect()
}

fn state_code(st: FileState) -> i32 {
    match st {
        FileState::Intact => 0,
        FileState::Partial => 1,
        FileState::Damaged => 2,
        FileState::Unknown => 3,
    }
}

fn kind_code(k: FileKind) -> i32 {
    match k {
        FileKind::Photo => 0,
        FileKind::Video => 1,
        FileKind::Document => 2,
        FileKind::Music => 3,
        FileKind::Archive => 4,
        FileKind::Other => 5,
    }
}

/// Ricostruisce gruppi, momenti e riquadri dei risultati.
fn refresh_results(ui: &AppWindow, s: &mut St) {
    let base = filtered(s);
    // Gruppi per tipo, solo quelli presenti.
    let mut kinds: Vec<(FileKind, usize)> = vec![];
    for id in &base {
        let k = s.files[*id].kind;
        match kinds.iter_mut().find(|(x, _)| *x == k) {
            Some(e) => e.1 += 1,
            None => kinds.push((k, 1)),
        }
    }
    kinds.sort_by_key(|(k, _)| kind_code(*k));
    let mut chips = vec![KindChip {
        label: sv(tf("All · {n}", &[("n", &base.len().to_string())])),
        kind: -1,
    }];
    s.chip_kinds = vec![None];
    for (k, n) in &kinds {
        chips.push(KindChip {
            label: sv(format!("{} · {n}", t(k.label()))),
            kind: kind_code(*k),
        });
        s.chip_kinds.push(Some(*k));
    }
    if s.chip >= s.chip_kinds.len() {
        s.chip = 0;
    }
    let chip_kind = s.chip_kinds[s.chip];
    let in_chip: Vec<usize> = base
        .iter()
        .copied()
        .filter(|id| chip_kind.map(|k| s.files[*id].kind == k).unwrap_or(true))
        .collect();
    let subset: Vec<FoundFile> = in_chip.iter().map(|id| s.files[*id].clone()).collect();
    let (moments, _) = group(&subset);
    s.moments = moments.into_iter().take(3).collect();
    if s.moment.map(|m| m >= s.moments.len()).unwrap_or(false) {
        s.moment = None;
    }
    let (title, shown): (String, Vec<usize>) = match s.moment.and_then(|m| s.moments.get(m)) {
        Some(m) => (
            format!(
                "{} · {}",
                moment_title(m),
                tf("{n} files", &[("n", &m.files.len().to_string())])
            ),
            m.files.clone(),
        ),
        None => {
            let label = chip_kind
                .map(|k| t(k.label()))
                .unwrap_or_else(|| t("All the files"));
            (
                format!(
                    "{label} · {}",
                    tf("{n} files", &[("n", &in_chip.len().to_string())])
                ),
                in_chip.clone(),
            )
        }
    };
    s.shown = shown;
    let tiles: Vec<Tile> = s
        .shown
        .iter()
        .take(MAX_TILES)
        .map(|id| {
            let f = &s.files[*id];
            let mut name = f.name().to_string();
            if f.name_is_partial() {
                name = format!("?{}", &name[1..]);
            }
            Tile {
                id: *id as i32,
                name: sv(name),
                kind: kind_code(f.kind),
                state: state_code(f.state),
                selected: s.selection.contains(id),
            }
        })
        .collect();
    ui.set_chips(ModelRc::new(VecModel::from(chips)));
    ui.set_chip(s.chip as i32);
    ui.set_moments(ModelRc::new(VecModel::from(
        s.moments
            .iter()
            .map(|m| MomentCard {
                title: sv(moment_title(m)),
                subtitle: sv(moment_subtitle(m)),
            })
            .collect::<Vec<_>>(),
    )));
    ui.set_moment(s.moment.map(|m| m as i32).unwrap_or(-1));
    ui.set_group_title(sv(title));
    ui.set_tiles(ModelRc::new(VecModel::from(tiles)));
    let mut hint = String::new();
    if s.shown.len() > MAX_TILES {
        hint = tf(
            "Showing the first {n}; \"Select all\" takes them all.",
            &[("n", &MAX_TILES.to_string())],
        );
    }
    if s.files.iter().any(|f| f.name_is_partial()) {
        if !hint.is_empty() {
            hint.push(' ');
        }
        hint.push_str(&t("A \"?\" at the start of a name: on this kind of disk the first letter of deleted names is lost."));
    }
    ui.set_results_hint(sv(hint));
    refresh_selection(ui, s);
}

fn refresh_selection(ui: &AppWindow, s: &St) {
    let n = s.selection.len();
    let bytes: u64 = s.selection.iter().map(|id| s.files[*id].size).sum();
    ui.set_selection_text(sv(if n == 0 {
        t("Tap the files to choose them")
    } else {
        tf(
            "{n} files selected · {size}",
            &[("n", &n.to_string()), ("size", &human_size(bytes))],
        )
    }));
    ui.set_destination_text(sv(match &s.dest {
        Some(d) => tf("Save to: {folder}", &[("folder", &d.display().to_string())]),
        None => t("Save to: choose a folder on another disk"),
    }));
    ui.set_can_recover(n > 0 && s.dest.is_some());
}

fn elapsed(start: Instant) -> String {
    let s = start.elapsed().as_secs();
    format!("{:02}:{:02}", s / 60, s % 60)
}

fn start_scan(ui: &AppWindow, st: &Shared, stop: Arc<AtomicBool>) {
    stop.store(false, Ordering::SeqCst);
    let (disk, what) = {
        let s = st.lock().unwrap();
        match s.selected.and_then(|i| s.disks.get(i)) {
            Some(d) => (d.clone(), s.what),
            None => return,
        }
    };
    let _ = what;
    ui.set_status_text(sv(""));
    ui.set_screen(3);
    ui.set_scanning(true);
    ui.set_found_count(0);
    ui.set_scan_subtitle(sv(format!(
        "· {} · {}",
        t(disk.kind_label()),
        human_size(disk.size)
    )));
    ui.set_scan_progress(sv(t("Starting…")));
    ui.set_curls(ModelRc::new(VecModel::from(vec![Curl { state: 0 }; CURLS])));
    ui.set_diary(ModelRc::new(VecModel::from(Vec::<DiaryLine>::new())));
    let weak = ui.as_weak();
    let st = st.clone();
    std::thread::spawn(move || {
        let start = Instant::now();
        let diary: Arc<Mutex<Vec<DiaryLine>>> = Arc::new(Mutex::new(vec![]));
        let curls: Arc<Mutex<Vec<i32>>> = Arc::new(Mutex::new(vec![0; CURLS]));
        let push = |w: &slint::Weak<AppWindow>,
                    line: DiaryLine,
                    curls_now: Vec<i32>,
                    found: Option<usize>,
                    progress: Option<String>| {
            {
                let mut d = diary.lock().unwrap();
                d.insert(0, line);
                d.truncate(6);
            }
            let lines = diary.lock().unwrap().clone();
            let _ = w.upgrade_in_event_loop(move |ui| {
                ui.set_diary(ModelRc::new(VecModel::from(lines)));
                ui.set_curls(ModelRc::new(VecModel::from(
                    curls_now
                        .into_iter()
                        .map(|state| Curl { state })
                        .collect::<Vec<_>>(),
                )));
                if let Some(n) = found {
                    ui.set_found_count(n as i32);
                }
                if let Some(p) = progress {
                    ui.set_scan_progress(sv(p));
                }
            });
        };
        // Chi legge il disco: diretto per le immagini, altrimenti l'helper con la password.
        let reader: Arc<dyn Reader> = if disk.kind == DiskKind::Image || notice::is_root() {
            Arc::new(DirectReader)
        } else {
            push(
                &weak,
                DiaryLine {
                    time: sv(elapsed(start)),
                    text: sv(t("Asking for your password to read the disk (read only).")),
                    strong: false,
                },
                vec![0; CURLS],
                None,
                None,
            );
            match HelperReader::default_path()
                .ok_or(LagottoError::MissingTool("lagotto-helper".into()))
                .and_then(|p| HelperReader::start(&p))
            {
                Ok(r) => Arc::new(r),
                Err(e) => {
                    let msg = err_text(&e);
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.set_scanning(false);
                        ui.set_screen(2);
                        ui.set_status_text(sv(msg));
                    });
                    return;
                }
            }
        };
        let mut zone_total = 1usize;
        let mut zone_found_before = 0usize;
        let (files, targets) = scan(
            &disk,
            reader.as_ref(),
            &|| stop.load(Ordering::SeqCst),
            |ev| match ev {
                Event::Zone {
                    index,
                    total,
                    label,
                } => {
                    zone_total = total.max(1);
                    let mut c = curls.lock().unwrap();
                    let from = index * CURLS / zone_total;
                    if from < CURLS {
                        c[from] = 4;
                    }
                    let snapshot = c.clone();
                    drop(c);
                    let progress = tf(
                        "Part {i} of {n}",
                        &[("i", &(index + 1).to_string()), ("n", &total.to_string())],
                    );
                    push(
                        &weak,
                        DiaryLine {
                            time: sv(elapsed(start)),
                            text: sv(tf("Reading {part}…", &[("part", &label)])),
                            strong: false,
                        },
                        snapshot,
                        None,
                        Some(progress),
                    );
                }
                Event::Folder { path, count, kind } => {
                    let text = tf(
                        "Found the folder {folder}: {n} files ({kind}).",
                        &[
                            ("folder", &path),
                            ("n", &count.to_string()),
                            ("kind", &t(kind.label()).to_lowercase()),
                        ],
                    );
                    let snapshot = curls.lock().unwrap().clone();
                    push(
                        &weak,
                        DiaryLine {
                            time: sv(elapsed(start)),
                            text: sv(text),
                            strong: true,
                        },
                        snapshot,
                        None,
                        None,
                    );
                }
                Event::Found { total } => {
                    // Striscia: la parte appena letta, con una quota di riccioli
                    // verdi proporzionale ai file trovati (tappa 0: per zone, non
                    // la posizione esatta dei file).
                    let mut c = curls.lock().unwrap();
                    let zone = c.iter().position(|x| *x == 4).unwrap_or(0);
                    let end = ((zone * zone_total / CURLS) + 1) * CURLS / zone_total;
                    let found_here = total - zone_found_before;
                    zone_found_before = total;
                    let span = end.saturating_sub(zone).max(1);
                    let green = ((found_here as f64 / (found_here as f64 + 20.0))
                        * span as f64
                        * 0.8) as usize;
                    for (k, cell) in c[zone..end.min(CURLS)].iter_mut().enumerate() {
                        *cell = if ((zone + k) * 37 + 11) % 97 * span < green * 97 {
                            2
                        } else {
                            1
                        };
                    }
                    let snapshot = c.clone();
                    drop(c);
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.set_found_count(total as i32);
                        ui.set_curls(ModelRc::new(VecModel::from(
                            snapshot
                                .into_iter()
                                .map(|state| Curl { state })
                                .collect::<Vec<_>>(),
                        )));
                    });
                }
                Event::Unreadable { label, error } => {
                    let text = tf(
                        "{part}: I can't read the names here. {why}",
                        &[("part", &label), ("why", &err_text(&error))],
                    );
                    let mut c = curls.lock().unwrap();
                    for cell in c.iter_mut().filter(|x| **x == 4) {
                        *cell = 1;
                    }
                    let snapshot = c.clone();
                    drop(c);
                    push(
                        &weak,
                        DiaryLine {
                            time: sv(elapsed(start)),
                            text: sv(text),
                            strong: false,
                        },
                        snapshot,
                        None,
                        None,
                    );
                }
                Event::Done { total } => {
                    let text = tf("Done: {n} files found.", &[("n", &total.to_string())]);
                    let mut c = curls.lock().unwrap();
                    for cell in c.iter_mut().filter(|x| **x == 0 || **x == 4) {
                        *cell = 1;
                    }
                    let snapshot = c.clone();
                    drop(c);
                    push(
                        &weak,
                        DiaryLine {
                            time: sv(elapsed(start)),
                            text: sv(text),
                            strong: true,
                        },
                        snapshot,
                        Some(total),
                        Some(t("Finished")),
                    );
                }
            },
        );
        {
            let mut s = st.lock().unwrap();
            s.files = files;
            s.targets = targets;
            s.reader = Some(reader);
            s.selection.clear();
            s.chip = 0;
            s.moment = None;
        }
        let _ = weak.upgrade_in_event_loop(move |ui| ui.set_scanning(false));
    });
}

fn choose_destination(ui: &AppWindow, st: &Shared) {
    let weak = ui.as_weak();
    let st = st.clone();
    std::thread::spawn(move || {
        let Some(dir) = rfd::FileDialog::new()
            .set_title(t("Where should I save the files? (another disk)"))
            .pick_folder()
        else {
            return;
        };
        let mut s = st.lock().unwrap();
        let disk = s.selected.and_then(|i| s.disks.get(i)).cloned();
        let needed: u64 = s.selection.iter().map(|id| s.files[*id].size).sum();
        let res = disk
            .map(|d| check_destination(&dir, &d, needed))
            .unwrap_or(Ok(()));
        let msg = match &res {
            Ok(()) => {
                s.dest = Some(dir);
                String::new()
            }
            Err(e) => err_text(e),
        };
        drop(s);
        let st2 = st.clone();
        let _ = weak.upgrade_in_event_loop(move |ui| {
            ui.set_status_text(sv(msg));
            refresh_selection(&ui, &st2.lock().unwrap());
        });
    });
}

fn do_recover(ui: &AppWindow, st: &Shared) {
    let (files, targets, reader, dest, disk) = {
        let s = st.lock().unwrap();
        let Some(reader) = s.reader.clone() else {
            return;
        };
        let Some(dest) = s.dest.clone() else { return };
        let Some(disk) = s.selected.and_then(|i| s.disks.get(i)).cloned() else {
            return;
        };
        let mut ids: Vec<usize> = s.selection.iter().copied().collect();
        ids.sort();
        (
            ids.iter().map(|i| s.files[*i].clone()).collect::<Vec<_>>(),
            s.targets.clone(),
            reader,
            dest,
            disk,
        )
    };
    let needed: u64 = files.iter().map(|f| f.size).sum();
    if let Err(e) = check_destination(&dest, &disk, needed) {
        ui.set_status_text(sv(err_text(&e)));
        return;
    }
    ui.set_screen(5);
    ui.set_recover_progress(0.0);
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        let (y, m, d) = ymd(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0));
        let opts = Options {
            folder_name: format!("Lagotto {y}-{m:02}-{d:02}"),
            kind_folder: Box::new(|k| t(k.label())),
            damaged_folder: t("Damaged"),
        };
        let refs: Vec<&FoundFile> = files.iter().collect();
        let w2 = weak.clone();
        let res = recover(
            &refs,
            &targets,
            reader.as_ref(),
            &dest,
            &opts,
            move |done, total| {
                let p = if total == 0 {
                    1.0
                } else {
                    done as f32 / total as f32
                };
                let _ = w2.upgrade_in_event_loop(move |ui| ui.set_recover_progress(p));
            },
        );
        let _ = weak.upgrade_in_event_loop(move |ui| match res {
            Ok(sum) => {
                ui.set_done_count(sv(sum.recovered.to_string()));
                ui.set_done_size(sv(human_size(sum.bytes)));
                ui.set_done_damaged(sv(sum.damaged.to_string()));
                ui.set_done_folder(sv(tf(
                    "Saved in {folder}, divided by type.",
                    &[("folder", &sum.folder.display().to_string())],
                )));
                ui.set_screen(6);
                FOLDER.lock().unwrap().replace(sum.folder);
            }
            Err(e) => {
                ui.set_status_text(sv(err_text(&e)));
                ui.set_screen(4);
            }
        });
    });
}

static FOLDER: Mutex<Option<PathBuf>> = Mutex::new(None);

fn open_path(p: &str) {
    let _ = std::process::Command::new("xdg-open").arg(p).spawn();
}

fn main() -> Result<(), slint::PlatformError> {
    i18n::init();
    let ui = AppWindow::new()?;
    ui.global::<Tr>().on_t(|s| sv(t(&s)));
    ui.global::<Tr>().set_rtl(i18n::RTL.contains(&i18n::lang()));
    ui.set_first_hint(sv(t("Don't save anything on the disk you want to recover from: you could overwrite the lost files.")));
    let st: Shared = Arc::new(Mutex::new(St::default()));
    let stop = Arc::new(AtomicBool::new(false));

    if notice::accepted() {
        ui.set_screen(2);
        refresh_disks(&ui, &st);
    }
    // `lagotto-gui immagine.img`: apre subito un'immagine del disco.
    if let Some(arg) = std::env::args().nth(1) {
        if let Ok(d) = Disk::from_image(std::path::Path::new(&arg)) {
            let mut s = st.lock().unwrap();
            s.disks.insert(0, d);
            s.selected = Some(0);
            ui.set_disks(ModelRc::new(VecModel::from(disk_cards(&s.disks))));
            ui.set_selected_disk(s.selected.unwrap() as i32);
            update_sentence(&ui, &s);
        }
    }

    ui.on_quit(|| {
        let _ = slint::quit_event_loop();
    });
    {
        let (w, st) = (ui.as_weak(), st.clone());
        ui.on_accept_notice(move || {
            let ui = w.unwrap();
            if let Err(e) = notice::accept() {
                ui.set_status_text(sv(e.to_string()));
            }
            ui.set_screen(2);
            refresh_disks(&ui, &st);
        });
    }
    {
        let (w, st) = (ui.as_weak(), st.clone());
        ui.on_refresh_disks(move || refresh_disks(&w.unwrap(), &st));
    }
    {
        let (w, st) = (ui.as_weak(), st.clone());
        ui.on_pick_disk(move |i| {
            let ui = w.unwrap();
            let mut s = st.lock().unwrap();
            s.selected = Some(i as usize);
            ui.set_selected_disk(i);
            update_sentence(&ui, &s);
        });
    }
    {
        let (w, st) = (ui.as_weak(), st.clone());
        ui.on_cycle_what(move || {
            let ui = w.unwrap();
            let mut s = st.lock().unwrap();
            s.what = (s.what + 1) % WHAT.len();
            update_sentence(&ui, &s);
        });
    }
    {
        let (w, st) = (ui.as_weak(), st.clone());
        ui.on_cycle_when(move || {
            let ui = w.unwrap();
            let mut s = st.lock().unwrap();
            s.when = (s.when + 1) % WHEN.len();
            update_sentence(&ui, &s);
        });
    }
    {
        let (w, st) = (ui.as_weak(), st.clone());
        ui.on_open_image(move || {
            let w = w.clone();
            let st = st.clone();
            std::thread::spawn(move || {
                let Some(file) = rfd::FileDialog::new()
                    .set_title(t("Open a disk image"))
                    .add_filter(
                        t("Disk images"),
                        &["img", "dd", "raw", "iso", "bin", "image"],
                    )
                    .pick_file()
                else {
                    return;
                };
                let _ = w.upgrade_in_event_loop(move |ui| {
                    match Disk::from_image(&file) {
                        Ok(d) => {
                            let mut s = st.lock().unwrap();
                            s.disks.insert(0, d);
                            s.selected = Some(0);
                            ui.set_disks(ModelRc::new(VecModel::from(disk_cards(&s.disks))));
                            ui.set_selected_disk(s.selected.unwrap() as i32);
                            update_sentence(&ui, &s);
                        }
                        Err(e) => ui.set_status_text(sv(e.to_string())),
                    }
                    ui.set_screen(2);
                });
            });
        });
    }
    {
        let (w, st, stop) = (ui.as_weak(), st.clone(), stop.clone());
        ui.on_start_scan(move || start_scan(&w.unwrap(), &st, stop.clone()));
    }
    {
        let stop = stop.clone();
        let w = ui.as_weak();
        ui.on_pause_scan(move || {
            // Tappa 0: Sleuth Kit non si interrompe a metà; si ferma alla fine della parte in corso.
            stop.store(true, Ordering::SeqCst);
            w.unwrap()
                .set_scan_progress(sv(t("Stopping after this part…")));
        });
    }
    {
        let (w, st) = (ui.as_weak(), st.clone());
        ui.on_show_results(move || {
            let ui = w.unwrap();
            let mut s = st.lock().unwrap();
            refresh_results(&ui, &mut s);
            ui.set_screen(4);
        });
    }
    {
        let (w, st) = (ui.as_weak(), st.clone());
        ui.on_pick_chip(move |i| {
            let ui = w.unwrap();
            let mut s = st.lock().unwrap();
            s.chip = i as usize;
            s.moment = None;
            refresh_results(&ui, &mut s);
        });
    }
    {
        let (w, st) = (ui.as_weak(), st.clone());
        ui.on_pick_moment(move |i| {
            let ui = w.unwrap();
            let mut s = st.lock().unwrap();
            s.moment = if s.moment == Some(i as usize) {
                None
            } else {
                Some(i as usize)
            };
            refresh_results(&ui, &mut s);
        });
    }
    {
        let (w, st) = (ui.as_weak(), st.clone());
        ui.on_toggle_tile(move |id| {
            let ui = w.unwrap();
            let mut s = st.lock().unwrap();
            let id = id as usize;
            if !s.selection.remove(&id) {
                s.selection.insert(id);
            }
            refresh_results(&ui, &mut s);
        });
    }
    {
        let (w, st) = (ui.as_weak(), st.clone());
        ui.on_select_group(move || {
            let ui = w.unwrap();
            let mut s = st.lock().unwrap();
            let shown = s.shown.clone();
            if shown.iter().all(|id| s.selection.contains(id)) {
                for id in shown {
                    s.selection.remove(&id);
                }
            } else {
                s.selection.extend(shown);
            }
            refresh_results(&ui, &mut s);
        });
    }
    {
        let (w, st) = (ui.as_weak(), st.clone());
        ui.on_choose_destination(move || choose_destination(&w.unwrap(), &st));
    }
    {
        let (w, st) = (ui.as_weak(), st.clone());
        ui.on_recover(move || do_recover(&w.unwrap(), &st));
    }
    ui.on_open_folder(|| {
        if let Some(f) = FOLDER.lock().unwrap().clone() {
            open_path(&f.display().to_string());
        }
    });
    {
        let (w, st) = (ui.as_weak(), st.clone());
        ui.on_new_search(move || {
            let ui = w.unwrap();
            if ui.get_screen() <= 1 {
                return;
            }
            {
                let mut s = st.lock().unwrap();
                s.files.clear();
                s.selection.clear();
                s.reader = None;
            }
            ui.set_screen(2);
            refresh_disks(&ui, &st);
        });
    }
    ui.on_open_guide(|| open_path("https://github.com/outlook791/Lagotto#readme"));
    ui.on_open_about(|| open_path("https://github.com/outlook791/Lagotto"));
    ui.run()
}
