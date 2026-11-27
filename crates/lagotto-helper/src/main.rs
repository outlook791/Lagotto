//! lagotto-helper: avviato da Lagotto con `pkexec`, legge i dischi.
//!
//! Sicurezza:
//! - accetta solo dispositivi a blocchi (/dev/…), mai file qualsiasi;
//! - esegue solo programmi che leggono (fls, mmls, icat) e PhotoRec, che
//!   scrive solo nella cartella indicata, poi ridà quei file all'utente;
//! - non apre mai un disco in scrittura.

use lagotto_core::helper::{Reply, Request};
use lagotto_core::scan::{fls_args, icat_args, Target};
use lagotto_core::LagottoError;
use std::io::{BufRead, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;
use std::process::{Command, Stdio};

/// Programmi cercati solo nelle cartelle di sistema: niente PATH dell'utente.
fn tool(name: &str) -> Result<String, LagottoError> {
    for dir in ["/usr/bin", "/usr/sbin", "/bin", "/sbin", "/usr/local/bin"] {
        let p = format!("{dir}/{name}");
        if Path::new(&p).is_file() {
            return Ok(p);
        }
    }
    Err(LagottoError::MissingTool(name.into()))
}

fn check_device(p: &Path) -> Result<(), LagottoError> {
    let ok = p.starts_with("/dev/")
        && std::fs::metadata(p)
            .map(|m| m.file_type().is_block_device())
            .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(LagottoError::SourceUnreadable(p.display().to_string()))
    }
}

fn caller_ids() -> Option<(u32, u32)> {
    let uid: u32 = std::env::var("PKEXEC_UID").ok()?.parse().ok()?;
    // Il gruppo principale dell'utente, da /etc/passwd.
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    let gid = passwd.lines().find_map(|l| {
        let c: Vec<&str> = l.split(':').collect();
        (c.len() > 3 && c[2] == uid.to_string())
            .then(|| c[3].parse().ok())
            .flatten()
    })?;
    Some((uid, gid))
}

fn capture(cmd: &str, args: &[String]) -> Result<String, LagottoError> {
    let out = Command::new(tool(cmd)?)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| LagottoError::ToolFailed {
            tool: cmd.into(),
            detail: e.to_string(),
        })?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
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

fn reply(out: &mut impl Write, r: Result<Reply, LagottoError>) {
    let r = r.unwrap_or_else(|e| Reply {
        ok: false,
        code: e.code().into(),
        data: e.to_string(),
        ..Default::default()
    });
    let _ = writeln!(out, "{}", serde_json::to_string(&r).unwrap());
    let _ = out.flush();
}

fn chown_tree(dir: &Path, uid: u32, gid: u32) {
    let _ = std::os::unix::fs::chown(dir, Some(uid), Some(gid));
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                chown_tree(&p, uid, gid);
            } else {
                let _ = std::os::unix::fs::chown(&p, Some(uid), Some(gid));
            }
        }
    }
}

fn serve() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    reply(
        &mut out,
        Ok(Reply {
            ok: true,
            data: "lagotto-helper".into(),
            ..Default::default()
        }),
    );
    // La ricerca senza nomi in corso, con l'utente a cui restituire i file.
    let mut job: Option<(lagotto_core::carve::ChildCarve, u32, u32)> = None;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                reply(
                    &mut out,
                    Err(LagottoError::ToolFailed {
                        tool: "request".into(),
                        detail: e.to_string(),
                    }),
                );
                continue;
            }
        };
        match req {
            Request::Quit => {
                if let Some((j, uid, gid)) = &job {
                    use lagotto_core::carve::CarveJob;
                    let _ = j.stop();
                    chown_tree(j.out(), *uid, *gid);
                }
                break;
            }
            Request::List { path, offset } => {
                let r = check_device(&path).and_then(|_| {
                    let t = Target {
                        path,
                        offset,
                        label: String::new(),
                    };
                    capture("fls", &fls_args(&t))
                });
                reply(
                    &mut out,
                    r.map(|data| Reply {
                        ok: true,
                        data,
                        ..Default::default()
                    }),
                );
            }
            Request::Mmls { path } => {
                let r = check_device(&path)
                    .and_then(|_| capture("mmls", &[path.display().to_string()]));
                reply(
                    &mut out,
                    r.map(|data| Reply {
                        ok: true,
                        data,
                        ..Default::default()
                    }),
                );
            }
            Request::Cat {
                path,
                offset,
                inode,
            } => {
                if let Err(e) = check_device(&path).and_then(|_| {
                    if inode.chars().all(|c| c.is_ascii_digit() || c == '-') {
                        Ok(())
                    } else {
                        Err(LagottoError::SourceUnreadable(inode.clone()))
                    }
                }) {
                    reply(&mut out, Err(e));
                    continue;
                }
                let t = Target {
                    path,
                    offset,
                    label: String::new(),
                };
                let child = tool("icat").and_then(|icat| {
                    Command::new(icat)
                        .args(icat_args(&t, &inode))
                        .stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn()
                        .map_err(|e| LagottoError::ToolFailed {
                            tool: "icat".into(),
                            detail: e.to_string(),
                        })
                });
                let mut child = match child {
                    Ok(c) => c,
                    Err(e) => {
                        reply(&mut out, Err(e));
                        continue;
                    }
                };
                reply(
                    &mut out,
                    Ok(Reply {
                        ok: true,
                        ..Default::default()
                    }),
                );
                let mut so = child.stdout.take().unwrap();
                let mut buf = vec![0u8; 1 << 20];
                loop {
                    let n = so.read(&mut buf).unwrap_or(0);
                    let _ = out.write_all(&(n as u32).to_be_bytes());
                    if n == 0 {
                        break;
                    }
                    let _ = out.write_all(&buf[..n]);
                }
                let ok = child.wait().map(|s| s.success()).unwrap_or(false);
                reply(
                    &mut out,
                    if ok {
                        Ok(Reply {
                            ok: true,
                            ..Default::default()
                        })
                    } else {
                        Err(LagottoError::ToolFailed {
                            tool: "icat".into(),
                            detail: "read error".into(),
                        })
                    },
                );
            }
            Request::CarveStart {
                path,
                out: dir,
                freespace,
            } => {
                let r = (|| {
                    check_device(&path)?;
                    if job
                        .as_ref()
                        .map(|(j, ..)| j.poll().running)
                        .unwrap_or(false)
                    {
                        return Err(LagottoError::ToolFailed {
                            tool: "photorec".into(),
                            detail: "already running".into(),
                        });
                    }
                    let (uid, gid) = caller_ids().ok_or(LagottoError::NotAuthorized)?;
                    // La cartella deve esistere ed essere dell'utente che ha chiesto.
                    let m = std::fs::metadata(&dir)
                        .map_err(|_| LagottoError::BadDestination(dir.display().to_string()))?;
                    if !m.is_dir() || m.uid() != uid {
                        return Err(LagottoError::BadDestination(dir.display().to_string()));
                    }
                    let child = lagotto_core::carve::spawn(
                        Path::new(&tool("photorec")?),
                        &path,
                        &dir,
                        freespace,
                    )?;
                    job = Some((
                        lagotto_core::carve::ChildCarve::new(child, dir.clone()),
                        uid,
                        gid,
                    ));
                    Ok(())
                })();
                reply(
                    &mut out,
                    r.map(|_| Reply {
                        ok: true,
                        ..Default::default()
                    }),
                );
            }
            Request::CarveStatus => {
                let r = match &job {
                    Some((j, uid, gid)) => {
                        let st = j.poll();
                        // I file ricostruiti tornano all'utente man mano.
                        chown_tree(j.out(), *uid, *gid);
                        Ok(Reply {
                            ok: true,
                            carve: Some(st),
                            ..Default::default()
                        })
                    }
                    None => Err(LagottoError::ToolFailed {
                        tool: "photorec".into(),
                        detail: "not running".into(),
                    }),
                };
                reply(&mut out, r);
            }
            Request::CarvePause | Request::CarveResume | Request::CarveStop => {
                use lagotto_core::carve::CarveJob;
                let r = match &job {
                    Some((j, uid, gid)) => {
                        let res = match req {
                            Request::CarvePause => j.pause(),
                            Request::CarveResume => j.resume(),
                            _ => j.stop(),
                        };
                        chown_tree(j.out(), *uid, *gid);
                        res.map(|_| Reply {
                            ok: true,
                            ..Default::default()
                        })
                    }
                    None => Err(LagottoError::ToolFailed {
                        tool: "photorec".into(),
                        detail: "not running".into(),
                    }),
                };
                reply(&mut out, r);
            }
        }
    }
    // Lagotto chiuso all'improvviso: niente PhotoRec orfano come amministratore.
    if let Some((j, uid, gid)) = &job {
        use lagotto_core::carve::CarveJob;
        if j.poll().running {
            let _ = j.stop();
        }
        chown_tree(j.out(), *uid, *gid);
    }
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("serve") => serve(),
        _ => {
            eprintln!("lagotto-helper is started by Lagotto (pkexec lagotto-helper serve).");
            std::process::exit(2);
        }
    }
}
