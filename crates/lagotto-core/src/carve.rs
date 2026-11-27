//! Fase 2, ricerca senza nomi: PhotoRec ricostruisce i file dal contenuto
//! grezzo. Serve quando il disco è stato formattato o i nomi sono persi.
//! PhotoRec legge la sorgente e scrive solo nella cartella indicata, che
//! deve stare su un altro disco (controllato prima con `recover::check_destination`).
//!
//! La ricerca gira in sottofondo e si può sospendere, riprendere e fermare.

use crate::diag::LagottoError;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

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

/// File ricostruiti finora (senza contare i file di servizio di PhotoRec).
pub fn count_files(dir: &Path) -> usize {
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                n += count_files(&p);
            } else if p
                .file_name()
                .map(|f| f != "report.xml" && f != "photorec.log" && f != "photorec.ses")
                .unwrap_or(false)
            {
                n += 1;
            }
        }
    }
    n
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CarveStatus {
    pub running: bool,
    pub paused: bool,
    pub files: usize,
    /// Finita senza errori (vale solo quando `running` è falso).
    pub ok: bool,
}

/// Una ricerca senza nomi in corso.
pub trait CarveJob: Send + Sync {
    fn status(&self) -> Result<CarveStatus, LagottoError>;
    fn pause(&self) -> Result<(), LagottoError>;
    fn resume(&self) -> Result<(), LagottoError>;
    fn stop(&self) -> Result<(), LagottoError>;
}

/// Manda un segnale a un processo con il programma `kill` del sistema.
pub fn signal(pid: u32, sig: &str) -> Result<(), LagottoError> {
    let st = Command::new("kill")
        .arg(format!("-{sig}"))
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| LagottoError::ToolFailed {
            tool: "kill".into(),
            detail: e.to_string(),
        })?;
    if st.success() {
        Ok(())
    } else {
        Err(LagottoError::ToolFailed {
            tool: "kill".into(),
            detail: format!("{sig} {pid}"),
        })
    }
}

/// Avvia PhotoRec come processo figlio (usato direttamente e dall'helper).
pub fn spawn(
    photorec: &Path,
    source: &Path,
    out_dir: &Path,
    freespace_only: bool,
) -> Result<Child, LagottoError> {
    std::fs::create_dir_all(out_dir)
        .map_err(|_| LagottoError::BadDestination(out_dir.display().to_string()))?;
    Command::new(photorec)
        .args(photorec_args(source, out_dir, freespace_only))
        .current_dir(out_dir) // photorec.log va qui, mai sulla sorgente
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| LagottoError::ToolFailed {
            tool: "photorec".into(),
            detail: e.to_string(),
        })
}

/// Un processo PhotoRec figlio: il cuore comune al modo diretto e all'helper.
pub struct ChildCarve {
    child: Mutex<Child>,
    out: PathBuf,
    paused: Mutex<bool>,
    done: Mutex<Option<bool>>,
}

impl ChildCarve {
    pub fn new(child: Child, out: PathBuf) -> ChildCarve {
        ChildCarve {
            child: Mutex::new(child),
            out,
            paused: Mutex::new(false),
            done: Mutex::new(None),
        }
    }

    pub fn out(&self) -> &Path {
        &self.out
    }

    pub fn poll(&self) -> CarveStatus {
        let mut done = self.done.lock().unwrap();
        if done.is_none() {
            if let Ok(Some(st)) = self.child.lock().unwrap().try_wait() {
                *done = Some(st.success());
            }
        }
        CarveStatus {
            running: done.is_none(),
            paused: *self.paused.lock().unwrap() && done.is_none(),
            files: count_files(&self.out),
            ok: done.unwrap_or(false),
        }
    }
}

impl CarveJob for ChildCarve {
    fn status(&self) -> Result<CarveStatus, LagottoError> {
        Ok(self.poll())
    }
    fn pause(&self) -> Result<(), LagottoError> {
        signal(self.child.lock().unwrap().id(), "STOP")?;
        *self.paused.lock().unwrap() = true;
        Ok(())
    }
    fn resume(&self) -> Result<(), LagottoError> {
        signal(self.child.lock().unwrap().id(), "CONT")?;
        *self.paused.lock().unwrap() = false;
        Ok(())
    }
    fn stop(&self) -> Result<(), LagottoError> {
        let mut c = self.child.lock().unwrap();
        let _ = signal(c.id(), "CONT");
        let _ = c.kill();
        let _ = c.wait();
        *self.done.lock().unwrap() = Some(true);
        *self.paused.lock().unwrap() = false;
        Ok(())
    }
}

/// Avvia la ricerca senza nomi direttamente (immagini, o se si è già amministratori).
pub fn start_direct(
    source: &Path,
    out_dir: &Path,
    freespace_only: bool,
) -> Result<ChildCarve, LagottoError> {
    let photorec = crate::tools::require("photorec")?;
    let child = spawn(&photorec, source, out_dir, freespace_only)?;
    Ok(ChildCarve::new(child, out_dir.to_path_buf()))
}

/// Attende la fine di una ricerca, chiamando `tick` ogni secondo.
pub fn wait(
    job: &dyn CarveJob,
    mut tick: impl FnMut(CarveStatus),
) -> Result<CarveStatus, LagottoError> {
    loop {
        let st = job.status()?;
        tick(st);
        if !st.running {
            return Ok(st);
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_never_touch_source_dir() {
        let a = photorec_args(
            Path::new("/dev/sdb"),
            Path::new("/media/altro/Lagotto"),
            true,
        );
        assert_eq!(a[2], "/media/altro/Lagotto/recup");
        assert!(a[5].ends_with("freespace,search"));
    }
}
