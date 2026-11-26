//! Ricerca senza nomi: PhotoRec ricostruisce i file dal contenuto grezzo.
//! Serve quando il disco è stato formattato o il filesystem è rovinato.
//! PhotoRec legge la sorgente e scrive solo nella cartella indicata, che
//! deve stare su un altro disco (controllato prima con `recover::check_destination`).

use crate::diag::LagottoError;
use std::path::Path;
use std::process::{Command, Stdio};

/// Argomenti di PhotoRec in modalità non interattiva.
/// `freespace_only`: solo lo spazio libero (i file cancellati), non i file esistenti.
pub fn photorec_args(source: &Path, out_dir: &Path, freespace_only: bool) -> Vec<String> {
    let space = if freespace_only {
        "freespace"
    } else {
        "wholespace"
    };
    vec![
        "/log".into(),
        "/d".into(),
        out_dir.join("recup").display().to_string(),
        "/cmd".into(),
        source.display().to_string(),
        format!("fileopt,everything,enable,{space},search"),
    ]
}

/// Esegue PhotoRec direttamente (immagini o se si è già amministratori).
pub fn run_direct(
    source: &Path,
    out_dir: &Path,
    freespace_only: bool,
) -> Result<usize, LagottoError> {
    crate::tools::require("photorec")?;
    std::fs::create_dir_all(out_dir)
        .map_err(|_| LagottoError::BadDestination(out_dir.display().to_string()))?;
    let st = Command::new("photorec")
        .args(photorec_args(source, out_dir, freespace_only))
        .current_dir(out_dir) // photorec.log va qui, non sulla sorgente
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| LagottoError::ToolFailed {
            tool: "photorec".into(),
            detail: e.to_string(),
        })?;
    if !st.success() {
        return Err(LagottoError::ToolFailed {
            tool: "photorec".into(),
            detail: format!("exit {st}"),
        });
    }
    Ok(count_files(out_dir))
}

pub fn count_files(dir: &Path) -> usize {
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                n += count_files(&p);
            } else if p
                .file_name()
                .map(|f| f != "report.xml" && f != "photorec.log")
                .unwrap_or(false)
            {
                n += 1;
            }
        }
    }
    n
}
