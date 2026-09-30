//! Terminal real dentro de nexo que ejecuta Claude Code (`claude`) en la carpeta del vault.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::vault::vault_root;

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PtyEvent {
    Data { text: String },
    Exit { code: Option<u32> },
}

struct Session {
    id: u64,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

#[derive(Default)]
pub struct ClaudeCodeState(Mutex<Option<Session>>);

static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Busca el ejecutable `claude` en el PATH y en las rutas donde lo instala su instalador.
pub fn find_claude(app: &AppHandle) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    };
    let from_path = std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .map(|p| p.join(name))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let home = app.path().home_dir().ok();
    let extra = home.iter().flat_map(|h| {
        [
            h.join(".local/bin").join(name),
            h.join(".claude/local").join(name),
        ]
    });
    from_path.into_iter().chain(extra).find(|p| p.is_file())
}

/// Nombre de la sesión de Claude Code a la que entra nexo.
pub const SESSION_NAME: &str = "Nexo";

pub struct NamedSession {
    pub id: String,
    /// Carpeta donde se creó: `claude --resume` solo la encuentra desde ahí.
    pub cwd: PathBuf,
}

/// Busca la sesión de Claude Code cuyo nombre (`/rename` o `--name`) es `name`.
/// Las sesiones son archivos JSONL en `~/.claude/projects/<carpeta>/<id>.jsonl`; el nombre
/// vigente es la última entrada `custom-title`. Si hay varias, gana la usada más recientemente.
pub fn find_named_session(app: &AppHandle, name: &str) -> Option<NamedSession> {
    let projects = app.path().home_dir().ok()?.join(".claude/projects");
    find_named_session_in(&projects, name)
}

fn find_named_session_in(projects: &std::path::Path, name: &str) -> Option<NamedSession> {
    use std::io::BufRead;
    let mut best: Option<(std::time::SystemTime, NamedSession)> = None;
    for project in std::fs::read_dir(projects).ok()?.flatten() {
        let Ok(files) = std::fs::read_dir(project.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            let Ok(handle) = std::fs::File::open(&path) else {
                continue;
            };
            let mut title: Option<String> = None;
            let mut cwd: Option<PathBuf> = None;
            for line in std::io::BufReader::new(handle)
                .lines()
                .map_while(Result::ok)
            {
                let is_title = line.contains("\"custom-title\"");
                if !is_title && cwd.is_some() {
                    continue;
                }
                let Ok(entry) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                if is_title {
                    title = entry["customTitle"].as_str().map(str::to_string);
                }
                if cwd.is_none() {
                    cwd = entry["cwd"].as_str().map(PathBuf::from);
                }
            }
            let (Some(title), Some(cwd)) = (title, cwd) else {
                continue;
            };
            if !title.eq_ignore_ascii_case(name) || !cwd.is_dir() {
                continue;
            }
            let modified = file
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            if best.as_ref().is_none_or(|(t, _)| modified > *t) {
                let id = path.file_stem()?.to_string_lossy().into_owned();
                best = Some((modified, NamedSession { id, cwd }));
            }
        }
    }
    best.map(|(_, s)| s)
}

fn stop(session: Option<Session>) {
    if let Some(mut s) = session {
        let _ = s.child.kill();
        let _ = s.child.wait();
    }
}

/// Arranca (o reinicia) Claude Code en el vault y envía su salida por `channel`.
#[tauri::command]
pub fn claude_code_start(
    app: AppHandle,
    state: State<'_, ClaudeCodeState>,
    cols: u16,
    rows: u16,
    channel: Channel<PtyEvent>,
) -> Result<String, String> {
    let root = vault_root(&app)?;
    let claude = find_claude(&app)
        .ok_or("No encontré Claude Code. Instálalo desde https://claude.com/claude-code y vuelve a intentarlo.")?;

    stop(state.0.lock().map_err(|e| e.to_string())?.take());

    let pty = native_pty_system()
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;
    // Entra en la sesión "Nexo" si existe; si no, crea una nueva con ese nombre en el vault.
    let mut cmd = CommandBuilder::new(&claude);
    let (cwd, description) = match find_named_session(&app, SESSION_NAME) {
        Some(session) => {
            cmd.args(["--resume", &session.id]);
            let label = format!("sesión {SESSION_NAME} · {}", session.cwd.display());
            (session.cwd, label)
        }
        None => {
            cmd.args(["--name", SESSION_NAME]);
            (
                root.clone(),
                format!("nueva sesión {SESSION_NAME} · {}", root.display()),
            )
        }
    };
    cmd.cwd(&cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    let child = pty
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("No se pudo iniciar Claude Code: {e}"))?;
    drop(pty.slave);

    let mut reader = pty.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pty.master.take_writer().map_err(|e| e.to_string())?;

    // Lector en su propio hilo: reenvía la salida respetando los caracteres UTF-8 partidos entre lecturas.
    let events = channel.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut pending: Vec<u8> = Vec::new();
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    pending.extend_from_slice(&buf[..n]);
                    let valid = match std::str::from_utf8(&pending) {
                        Ok(_) => pending.len(),
                        Err(e) if e.error_len().is_none() => e.valid_up_to(),
                        Err(_) => pending.len(), // bytes inválidos: se envían con reemplazo
                    };
                    let text = String::from_utf8_lossy(&pending[..valid]).into_owned();
                    pending.drain(..valid);
                    if !text.is_empty() && events.send(PtyEvent::Data { text }).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let session = Session {
        id,
        master: pty.master,
        writer,
        child,
    };
    *state.0.lock().map_err(|e| e.to_string())? = Some(session);

    // Aviso de salida: se comprueba el proceso en segundo plano.
    let app_handle = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(400));
            let state = app_handle.state::<ClaudeCodeState>();
            let Ok(mut guard) = state.0.lock() else { break };
            // Si se reinició o se detuvo, esta vigilancia ya no corresponde a la sesión actual.
            let Some(session) = guard.as_mut().filter(|s| s.id == id) else {
                break;
            };
            if let Ok(Some(status)) = session.child.try_wait() {
                *guard = None;
                let _ = channel.send(PtyEvent::Exit {
                    code: Some(status.exit_code()),
                });
                break;
            }
        }
    });

    Ok(description)
}

#[tauri::command]
pub fn claude_code_write(state: State<'_, ClaudeCodeState>, data: String) -> Result<(), String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    let session = guard.as_mut().ok_or("Claude Code no está en marcha.")?;
    session
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| e.to_string())?;
    session.writer.flush().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn claude_code_resize(
    state: State<'_, ClaudeCodeState>,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(session) = guard.as_ref() {
        session
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn claude_code_stop(state: State<'_, ClaudeCodeState>) -> Result<(), String> {
    stop(state.0.lock().map_err(|e| e.to_string())?.take());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Usa las sesiones reales del equipo: `cargo test -- --ignored sesion`.
    #[test]
    #[ignore]
    fn encuentra_la_sesion_nexo() {
        let home = std::env::var("HOME").unwrap();
        let found =
            find_named_session_in(&PathBuf::from(home).join(".claude/projects"), SESSION_NAME)
                .expect("hay una sesión Nexo");
        println!("{} · {}", found.id, found.cwd.display());
    }
}
