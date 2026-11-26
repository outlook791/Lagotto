//! Dischi collegati e diagnosi: che tipo di disco è e quante probabilità ci sono.

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DiskKind {
    UsbStick,
    SdCard,
    Hdd,
    Ssd,
    /// Un file immagine di un disco (.img, .dd, .iso…).
    Image,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Probability {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize)]
pub struct Partition {
    pub path: PathBuf,
    pub fstype: Option<String>,
    pub label: Option<String>,
    pub size: u64,
    pub mountpoints: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Disk {
    /// Percorso del dispositivo (/dev/sdb) o del file immagine.
    pub path: PathBuf,
    pub model: String,
    pub size: u64,
    pub kind: DiskKind,
    pub removable: bool,
    /// Il disco accetta TRIM/discard: i blocchi cancellati possono essere svuotati.
    pub trim: bool,
    pub encrypted: bool,
    /// Contiene il sistema in uso (montato su "/").
    pub system: bool,
    pub partitions: Vec<Partition>,
}

impl Disk {
    /// Probabilità onesta di recupero e il motivo (testo sorgente inglese).
    pub fn probability(&self) -> (Probability, &'static str) {
        if self.encrypted {
            return (
                Probability::Low,
                "Encrypted disk: without the password nothing can be read.",
            );
        }
        match self.kind {
            DiskKind::Ssd if self.trim => (
                Probability::Low,
                "SSDs quickly empty deleted files (TRIM is on).",
            ),
            DiskKind::Ssd => (
                Probability::Medium,
                "SSD without TRIM: deleted files may still be there.",
            ),
            DiskKind::UsbStick if self.trim => (
                Probability::Medium,
                "This USB drive may empty deleted files (TRIM).",
            ),
            DiskKind::UsbStick => (
                Probability::High,
                "USB sticks keep deleted files until you write something else.",
            ),
            DiskKind::SdCard => (
                Probability::High,
                "Even after formatting, photos are often still there.",
            ),
            DiskKind::Hdd => (
                Probability::High,
                "Hard disks keep deleted files until they are overwritten.",
            ),
            DiskKind::Image => (
                Probability::High,
                "A disk image is read safely, without touching the disk.",
            ),
            DiskKind::Other => (Probability::Medium, "Unknown kind of disk."),
        }
    }

    /// Nome comprensibile ("USB stick", "Camera card"…), testo sorgente inglese.
    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            DiskKind::UsbStick => "USB stick",
            DiskKind::SdCard => "Camera card",
            DiskKind::Hdd => "Hard disk",
            DiskKind::Ssd => "Computer disk",
            DiskKind::Image => "Disk image",
            DiskKind::Other => "Disk",
        }
    }

    /// Crea un "disco" a partire da un file immagine.
    pub fn from_image(path: &Path) -> Result<Disk> {
        let meta =
            std::fs::metadata(path).with_context(|| format!("cannot read {}", path.display()))?;
        Ok(Disk {
            path: path.to_path_buf(),
            model: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            size: meta.len(),
            kind: DiskKind::Image,
            removable: false,
            trim: false,
            encrypted: false,
            system: false,
            partitions: vec![],
        })
    }
}

/// Testi mostrati all'utente (per il controllo delle traduzioni).
pub const USER_TEXTS: &[&str] = &[
    "Encrypted disk: without the password nothing can be read.",
    "SSDs quickly empty deleted files (TRIM is on).",
    "SSD without TRIM: deleted files may still be there.",
    "This USB drive may empty deleted files (TRIM).",
    "USB sticks keep deleted files until you write something else.",
    "Even after formatting, photos are often still there.",
    "Hard disks keep deleted files until they are overwritten.",
    "A disk image is read safely, without touching the disk.",
    "Unknown kind of disk.",
    "USB stick",
    "Camera card",
    "Hard disk",
    "Computer disk",
    "Disk image",
    "Disk",
];

/// Elenca i dischi del computer (solo lettura, con `lsblk`).
pub fn list_disks() -> Result<Vec<Disk>> {
    let out = Command::new("lsblk")
        .args([
            "-J",
            "-b",
            "-o",
            "NAME,PATH,SIZE,TYPE,TRAN,RM,ROTA,MODEL,VENDOR,FSTYPE,LABEL,MOUNTPOINTS,DISC-MAX,HOTPLUG",
        ])
        .output()
        .context("lsblk")?;
    if !out.status.success() {
        anyhow::bail!("lsblk failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    parse_lsblk(&String::from_utf8_lossy(&out.stdout))
}

fn as_bool(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::String(s) => s == "1" || s == "true",
        Value::Number(n) => n.as_u64().unwrap_or(0) != 0,
        _ => false,
    }
}

fn as_u64(v: &Value) -> u64 {
    match v {
        Value::Number(n) => n.as_u64().unwrap_or(0),
        Value::String(s) => s.trim_end_matches('B').parse().unwrap_or(0),
        _ => 0,
    }
}

fn as_str(v: &Value) -> Option<String> {
    v.as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn mountpoints(v: &Value) -> Vec<String> {
    match v.get("mountpoints") {
        Some(Value::Array(a)) => a.iter().filter_map(as_str).collect(),
        _ => vec![],
    }
}

fn collect_parts(node: &Value, parts: &mut Vec<Partition>, encrypted: &mut bool) {
    if let Some(Value::Array(children)) = node.get("children") {
        for c in children {
            let fstype = c.get("fstype").and_then(as_str);
            if matches!(fstype.as_deref(), Some("crypto_LUKS") | Some("BitLocker")) {
                *encrypted = true;
            }
            if c.get("type").and_then(|t| t.as_str()) == Some("part") {
                parts.push(Partition {
                    path: PathBuf::from(c.get("path").and_then(as_str).unwrap_or_default()),
                    fstype,
                    label: c.get("label").and_then(as_str),
                    size: c.get("size").map(as_u64).unwrap_or(0),
                    mountpoints: mountpoints(c),
                });
            }
            collect_parts(c, parts, encrypted);
        }
    }
}

/// Interpreta l'uscita JSON di `lsblk` (separato per poterlo provare).
pub fn parse_lsblk(json: &str) -> Result<Vec<Disk>> {
    let v: Value = serde_json::from_str(json).context("lsblk json")?;
    let mut disks = vec![];
    for d in v["blockdevices"].as_array().cloned().unwrap_or_default() {
        if d.get("type").and_then(|t| t.as_str()) != Some("disk") {
            continue;
        }
        let name = d.get("name").and_then(as_str).unwrap_or_default();
        if name.starts_with("zram") || name.starts_with("loop") || name.starts_with("ram") {
            continue;
        }
        let tran = d.get("tran").and_then(as_str).unwrap_or_default();
        let removable = d.get("rm").map(as_bool).unwrap_or(false)
            || d.get("hotplug").map(as_bool).unwrap_or(false);
        let rota = d.get("rota").map(as_bool).unwrap_or(false);
        let trim = d.get("disc-max").map(as_u64).unwrap_or(0) > 0;
        let kind = if name.starts_with("mmcblk") || tran == "mmc" {
            DiskKind::SdCard
        } else if tran == "usb" && !rota {
            DiskKind::UsbStick
        } else if rota {
            DiskKind::Hdd
        } else if tran == "nvme" || tran == "sata" || name.starts_with("nvme") {
            DiskKind::Ssd
        } else if tran == "usb" {
            DiskKind::Hdd
        } else {
            DiskKind::Other
        };
        let mut parts = vec![];
        let mut encrypted = matches!(
            d.get("fstype").and_then(as_str).as_deref(),
            Some("crypto_LUKS") | Some("BitLocker")
        );
        collect_parts(&d, &mut parts, &mut encrypted);
        let mut system = mountpoints(&d).iter().any(|m| m == "/");
        system |= all_mountpoints(&d).iter().any(|m| m == "/" || m == "/boot");
        let model = [
            d.get("vendor").and_then(as_str),
            d.get("model").and_then(as_str),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
        disks.push(Disk {
            path: PathBuf::from(
                d.get("path")
                    .and_then(as_str)
                    .unwrap_or_else(|| format!("/dev/{name}")),
            ),
            model,
            size: d.get("size").map(as_u64).unwrap_or(0),
            kind,
            removable,
            trim,
            encrypted,
            system,
            partitions: parts,
        });
    }
    Ok(disks)
}

fn all_mountpoints(node: &Value) -> Vec<String> {
    let mut out = mountpoints(node);
    if let Some(Value::Array(ch)) = node.get("children") {
        for c in ch {
            out.extend(all_mountpoints(c));
        }
    }
    out
}

/// Formato leggibile di una dimensione: "32 GB", "1,5 MB".
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 || v >= 10.0 {
        format!("{:.0} {}", v, UNITS[i])
    } else {
        format!("{:.1} {}", v, UNITS[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"blockdevices":[
      {"name":"nvme0n1","path":"/dev/nvme0n1","size":1000204886016,"type":"disk","tran":"nvme","rm":false,"rota":false,"model":"WD Black","vendor":null,"fstype":null,"label":null,"mountpoints":[null],"disc-max":2199023255040,"hotplug":false,
        "children":[{"name":"nvme0n1p1","path":"/dev/nvme0n1p1","size":600000000,"type":"part","tran":null,"rm":false,"rota":false,"model":null,"vendor":null,"fstype":"vfat","label":null,"mountpoints":["/boot/efi"],"disc-max":0,"hotplug":false},
                    {"name":"nvme0n1p2","path":"/dev/nvme0n1p2","size":999000000000,"type":"part","tran":null,"rm":false,"rota":false,"model":null,"vendor":null,"fstype":"btrfs","label":"fedora","mountpoints":["/","/home"],"disc-max":0,"hotplug":false}]},
      {"name":"sdb","path":"/dev/sdb","size":"32010928128","type":"disk","tran":"usb","rm":"1","rota":"0","model":"Cruzer","vendor":"SanDisk","fstype":null,"label":null,"mountpoints":[null],"disc-max":"0","hotplug":"1",
        "children":[{"name":"sdb1","path":"/dev/sdb1","size":32000000000,"type":"part","fstype":"vfat","label":"CHIAVETTA","mountpoints":["/run/media/u/CHIAVETTA"]}]},
      {"name":"mmcblk0","path":"/dev/mmcblk0","size":64000000000,"type":"disk","tran":null,"rm":false,"rota":false,"model":null,"vendor":null,"mountpoints":[null],"disc-max":0},
      {"name":"zram0","path":"/dev/zram0","size":8000000000,"type":"disk","rota":false,"mountpoints":["[SWAP]"]}
    ]}"#;

    #[test]
    fn parses_and_classifies() {
        let d = parse_lsblk(SAMPLE).unwrap();
        assert_eq!(d.len(), 3, "zram must be skipped");
        assert_eq!(d[0].kind, DiskKind::Ssd);
        assert!(d[0].trim && d[0].system);
        assert_eq!(d[0].probability().0, Probability::Low);
        assert_eq!(d[0].partitions.len(), 2);
        assert_eq!(d[1].kind, DiskKind::UsbStick);
        assert!(d[1].removable && !d[1].system);
        assert_eq!(d[1].model, "SanDisk Cruzer");
        assert_eq!(d[1].probability().0, Probability::High);
        assert_eq!(d[2].kind, DiskKind::SdCard);
    }

    #[test]
    fn sizes() {
        assert_eq!(human_size(32_010_928_128), "32 GB");
        assert_eq!(human_size(1_500_000), "1.5 MB");
        assert_eq!(human_size(512), "512 B");
    }
}
