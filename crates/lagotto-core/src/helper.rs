//! Il piccolo programma con i permessi di amministratore (`lagotto-helper`)
//! e il modo di parlarci. L'interfaccia gira come utente normale; solo
//! l'helper legge i dischi, e solo in lettura.
//!
//! Protocollo: una richiesta JSON per riga sullo standard input.
//! Risposta: una riga JSON; per `cat`, dopo la riga arrivano blocchi
//! "4 byte di lunghezza (big endian) + dati", chiusi da un blocco di
//! lunghezza 0 e da una riga JSON finale.

use crate::diag::LagottoError;
use crate::scan::{Reader, Target};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Request {
    List {
        path: PathBuf,
        offset: Option<u64>,
    },
    Mmls {
        path: PathBuf,
    },
    Cat {
        path: PathBuf,
        offset: Option<u64>,
        inode: String,
    },
    Carve {
        path: PathBuf,
        out: PathBuf,
        freespace: bool,
    },
    Quit,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(default)]
    pub data: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub count: usize,
}

pub fn error_from_code(code: &str, data: &str) -> LagottoError {
    match code {
        "L101" => LagottoError::MissingTool(data.into()),
        "L104" => LagottoError::SourceUnreadable(data.into()),
        "L105" => LagottoError::UnknownFilesystem,
        "L107" => LagottoError::NotAuthorized,
        "L108" => LagottoError::BadDestination(data.into()),
        _ => LagottoError::ToolFailed {
            tool: "lagotto-helper".into(),
            detail: data.into(),
        },
    }
}

/// Client: avvia l'helper con `pkexec` (la finestra della password del sistema)
/// e gli passa le richieste. Una sola password per tutta la sessione.
pub struct HelperReader {
    inner: Mutex<(Child, ChildStdin, BufReader<ChildStdout>)>,
}

impl HelperReader {
    /// `helper`: percorso assoluto di `lagotto-helper`.
    pub fn start(helper: &Path) -> Result<HelperReader, LagottoError> {
        let mut child = Command::new("pkexec")
            .arg(helper)
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| LagottoError::MissingTool("pkexec".into()))?;
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        let me = HelperReader {
            inner: Mutex::new((child, stdin, stdout)),
        };
        // La prima risposta arriva dopo la password: se manca, l'utente ha annullato.
        let mut g = me.inner.lock().unwrap();
        let mut line = String::new();
        if g.2.read_line(&mut line).unwrap_or(0) == 0 || !line.contains("\"ok\":true") {
            return Err(LagottoError::NotAuthorized);
        }
        drop(g);
        Ok(me)
    }

    /// Percorso di `lagotto-helper` accanto al programma in esecuzione.
    /// Prima accanto al programma (cartella di sviluppo), poi `/usr/libexec`
    /// (pacchetti installati, con la regola polkit dedicata).
    pub fn default_path() -> Option<PathBuf> {
        let mut candidates = vec![
            PathBuf::from("/usr/libexec/lagotto-helper"),
            PathBuf::from("/usr/lib/lagotto/lagotto-helper"),
        ];
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|p| p.to_path_buf()))
        {
            candidates.insert(0, dir.join("lagotto-helper"));
        }
        candidates.into_iter().find(|p| p.is_file())
    }

    fn request(&self, req: &Request) -> Result<Reply, LagottoError> {
        let mut g = self.inner.lock().unwrap();
        let line = serde_json::to_string(req).unwrap();
        writeln!(g.1, "{line}").map_err(|_| LagottoError::NotAuthorized)?;
        g.1.flush().ok();
        let mut resp = String::new();
        if g.2.read_line(&mut resp).unwrap_or(0) == 0 {
            return Err(LagottoError::NotAuthorized);
        }
        let r: Reply = serde_json::from_str(&resp).map_err(|e| LagottoError::ToolFailed {
            tool: "lagotto-helper".into(),
            detail: e.to_string(),
        })?;
        if !r.ok {
            return Err(error_from_code(&r.code, &r.data));
        }
        Ok(r)
    }

    pub fn carve(&self, path: &Path, out: &Path, freespace: bool) -> Result<usize, LagottoError> {
        Ok(self
            .request(&Request::Carve {
                path: path.into(),
                out: out.into(),
                freespace,
            })?
            .count)
    }
}

impl Drop for HelperReader {
    fn drop(&mut self) {
        if let Ok(mut g) = self.inner.lock() {
            let _ = writeln!(g.1, "{}", serde_json::to_string(&Request::Quit).unwrap());
            let _ = g.0.wait();
        }
    }
}

impl Reader for HelperReader {
    fn list_deleted(&self, t: &Target) -> Result<String, LagottoError> {
        Ok(self
            .request(&Request::List {
                path: t.path.clone(),
                offset: t.offset,
            })?
            .data)
    }
    fn partitions(&self, path: &Path) -> Result<String, LagottoError> {
        Ok(self.request(&Request::Mmls { path: path.into() })?.data)
    }
    fn cat(&self, t: &Target, inode: &str, out: &mut dyn Write) -> Result<u64, LagottoError> {
        let mut g = self.inner.lock().unwrap();
        let req = Request::Cat {
            path: t.path.clone(),
            offset: t.offset,
            inode: inode.into(),
        };
        writeln!(g.1, "{}", serde_json::to_string(&req).unwrap())
            .map_err(|_| LagottoError::NotAuthorized)?;
        g.1.flush().ok();
        let mut head = String::new();
        g.2.read_line(&mut head)
            .map_err(|_| LagottoError::NotAuthorized)?;
        let r: Reply = serde_json::from_str(&head).unwrap_or_default();
        if !r.ok {
            return Err(error_from_code(&r.code, &r.data));
        }
        let mut total = 0u64;
        loop {
            let mut len = [0u8; 4];
            g.2.read_exact(&mut len)
                .map_err(|_| LagottoError::NotAuthorized)?;
            let n = u32::from_be_bytes(len) as usize;
            if n == 0 {
                break;
            }
            let mut buf = vec![0u8; n];
            g.2.read_exact(&mut buf)
                .map_err(|_| LagottoError::NotAuthorized)?;
            out.write_all(&buf)
                .map_err(|e| LagottoError::BadDestination(e.to_string()))?;
            total += n as u64;
        }
        let mut tail = String::new();
        g.2.read_line(&mut tail).ok();
        let r: Reply = serde_json::from_str(&tail).unwrap_or_default();
        if !r.ok {
            return Err(error_from_code(&r.code, &r.data));
        }
        Ok(total)
    }
}
