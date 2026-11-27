//! `lagotto`: la riga di comando.
//!
//!   lagotto disks [--json]
//!   lagotto scan <disco|immagine> [--json]
//!   lagotto recover <disco|immagine> <cartella> [--kind photo|video|document|music|archive|other] [--carve]
//!   lagotto --accept-risk     (accetta l'avviso, una volta)

use lagotto_core::carve::{self, start_direct, CarveJob};
use lagotto_core::disks::{human_size, list_disks};
use lagotto_core::found::FileKind;
use lagotto_core::helper::HelperReader;
use lagotto_core::recover::{check_destination, recover, Options};
use lagotto_core::scan::{scan, DirectReader, Event, Reader};
use lagotto_core::{notice, Disk, DiskKind};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::sync::Arc;

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("lagotto: {msg}");
    exit(1)
}

fn resolve(arg: &str) -> Disk {
    let p = Path::new(arg);
    if arg.starts_with("/dev/") {
        let disks = list_disks().unwrap_or_else(|e| die(e));
        if let Some(d) = disks.iter().find(|d| d.path == p) {
            return d.clone();
        }
        // Una partizione sola: trattala come un disco senza partizioni.
        if let Some(d) = disks
            .iter()
            .find(|d| d.partitions.iter().any(|x| x.path == p))
        {
            let mut d = d.clone();
            d.partitions.retain(|x| x.path == p);
            return d;
        }
        die(format!("{arg}: disk not found"));
    }
    Disk::from_image(p).unwrap_or_else(|e| die(e))
}

/// Chi legge il disco: diretto (immagini, amministratore) o l'helper con la password.
fn reader_for(disk: &Disk) -> (Arc<dyn Reader>, Option<Arc<HelperReader>>) {
    if disk.kind == DiskKind::Image || notice::is_root() {
        return (Arc::new(DirectReader), None);
    }
    let helper = HelperReader::default_path()
        .unwrap_or_else(|| die("lagotto-helper not found next to lagotto"));
    let h = Arc::new(HelperReader::start(&helper).unwrap_or_else(|e| die(e)));
    (h.clone(), Some(h))
}

fn kind_arg(s: &str) -> FileKind {
    match s {
        "photo" => FileKind::Photo,
        "video" => FileKind::Video,
        "document" => FileKind::Document,
        "music" => FileKind::Music,
        "archive" => FileKind::Archive,
        "other" => FileKind::Other,
        _ => die(format!("unknown kind: {s}")),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    if args.iter().any(|a| a == "--accept-risk") {
        notice::accept().unwrap_or_else(|e| die(e));
        println!("Accepted.");
        return;
    }
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    if matches!(cmd, "help" | "-h" | "--help") {
        println!("Lagotto — recover deleted files\n\n  lagotto disks [--json]\n  lagotto scan <disk|image> [--json]\n  lagotto recover <disk|image> <folder> [--kind photo|video|document|music|archive|other] [--carve]\n  lagotto --accept-risk\n");
        return;
    }
    if !notice::accepted() {
        eprintln!("{}", notice::full_text());
        eprintln!("To accept, run once: lagotto --accept-risk");
        exit(3);
    }
    match cmd {
        "disks" => {
            let disks = list_disks().unwrap_or_else(|e| die(e));
            if json {
                println!("{}", serde_json::to_string_pretty(&disks).unwrap());
                return;
            }
            for d in disks {
                let (p, why) = d.probability();
                println!(
                    "{}  {}  {}  {}{}\n    {:?}: {}",
                    d.path.display(),
                    d.kind_label(),
                    if d.model.is_empty() { "-" } else { &d.model },
                    human_size(d.size),
                    if d.system { "  (system disk)" } else { "" },
                    p,
                    why
                );
            }
        }
        "scan" => {
            let disk = resolve(
                args.get(1)
                    .map(String::as_str)
                    .unwrap_or_else(|| die("which disk or image?")),
            );
            let (reader, _helper) = reader_for(&disk);
            let (files, _) = scan(&disk, reader.as_ref(), &|| false, |e| {
                if !json {
                    match e {
                        Event::Zone {
                            index,
                            total,
                            label,
                        } => eprintln!("[{}/{}] {}", index + 1, total, label),
                        Event::Folder { path, count, .. } => eprintln!("  {count} files in {path}"),
                        Event::Unreadable { label, error } => eprintln!("  {label}: {error}"),
                        _ => {}
                    }
                }
            });
            if json {
                println!("{}", serde_json::to_string_pretty(&files).unwrap());
            } else {
                for f in &files {
                    println!("{:>10}  {}", human_size(f.size), f.path);
                }
                println!("{} files", files.len());
            }
        }
        "recover" => {
            let disk = resolve(
                args.get(1)
                    .map(String::as_str)
                    .unwrap_or_else(|| die("which disk or image?")),
            );
            let dest = PathBuf::from(args.get(2).unwrap_or_else(|| die("which folder?")));
            let kind = args
                .iter()
                .position(|a| a == "--kind")
                .and_then(|i| args.get(i + 1))
                .map(|s| kind_arg(s));
            let (reader, helper) = reader_for(&disk);
            let (files, targets) = scan(&disk, reader.as_ref(), &|| false, |_| {});
            let chosen: Vec<_> = files
                .iter()
                .filter(|f| kind.map(|k| f.kind == k).unwrap_or(true))
                .collect();
            let needed: u64 = chosen.iter().map(|f| f.size).sum();
            check_destination(&dest, &disk, needed).unwrap_or_else(|e| die(e));
            let root = dest.join("Lagotto");
            let nameless = root.join("Without names");
            if args.iter().any(|a| a == "--carve") {
                // Fase 2, senza nomi: PhotoRec legge la sorgente e scrive solo qui.
                std::fs::create_dir_all(&nameless).unwrap_or_else(|e| die(e));
                let job: Box<dyn CarveJob> = match &helper {
                    None => Box::new(
                        start_direct(&disk.path, &nameless, true).unwrap_or_else(|e| die(e)),
                    ),
                    Some(h) => Box::new(
                        h.carve_start(&disk.path, &nameless, true)
                            .unwrap_or_else(|e| die(e)),
                    ),
                };
                let st = carve::wait(job.as_ref(), |st| {
                    if !json {
                        eprint!("\r{} files rebuilt without names", st.files);
                    }
                })
                .unwrap_or_else(|e| die(e));
                if !json {
                    eprintln!();
                }
                if !st.ok {
                    eprintln!("PhotoRec stopped with an error");
                }
            }
            let opts = Options {
                folder_name: "Lagotto".into(),
                kind_folder: Box::new(|k| k.label().to_string()),
                damaged_folder: "Damaged".into(),
                nameless_dir: nameless.is_dir().then(|| nameless.clone()),
            };
            let sum = recover(&chosen, &targets, reader.as_ref(), &dest, &opts, |_, _| {})
                .unwrap_or_else(|e| die(e));
            if json {
                println!("{}", serde_json::to_string_pretty(&sum).unwrap());
            } else {
                println!(
                    "{} files recovered ({}), {} damaged, {} duplicates skipped, {} failed\n{}",
                    sum.recovered,
                    human_size(sum.bytes),
                    sum.damaged,
                    sum.duplicates,
                    sum.failed,
                    sum.folder.display()
                );
            }
        }
        other => die(format!("unknown command: {other}")),
    }
}
