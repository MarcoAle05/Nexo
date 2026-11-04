//! Terminales de los workspaces (`docs/spec-terminales-workspaces.md`): cada una es un PTY con
//! el shell del usuario en la carpeta del workspace, como en Orca. nexo no interpreta lo que
//! corre dentro: guarda su salida (para repetírsela a una interfaz recién conectada y
//! recuperarla al reiniciar) y una pantalla sin ventana (`vt100`) para leerla desde la orden de
//! control (`control.rs`).
//!
//! - El shell arranca con un archivo de inicio de nexo que carga la configuración del usuario
//!   y después pone delante en el `PATH` el `claude` de nexo (`install_shell`): así un Claude
//!   Code lanzado ahí avisa a nexo con hooks de solo esa sesión (`sesiones.rs`), sin tocar
//!   `~/.claude`. Va después del `.bashrc` porque ese archivo suele anteponer `~/.local/bin`,
//!   donde está el `claude` de verdad: puesto antes, quedaría tapado.
//! - Viven mientras nexo esté abierto, también al cambiar de workspace. La lista de cada
//!   workspace y lo último de su salida se guardan en la carpeta de datos de nexo (fuera de la
//!   del proyecto, archivos 0600: una terminal puede haber enseñado una clave) cada pocos
//!   segundos y al cerrar; al volver se recuperan: su salida de antes, una línea tenue y un
//!   shell nuevo, y la de un Claude con sesión conocida la retoma con `claude --resume`.

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::claude_code::INHERITED_ENV;
use crate::workspace::{write_private, ws_root};

/// Terminales vivas como mucho por workspace y en total.
pub(crate) const MAX_PER_WS: usize = 12;
pub(crate) const MAX_TOTAL: usize = 32;
/// Lo que se guarda de la salida de cada terminal (para repetir y recuperar).
const RING_MAX: usize = 1024 * 1024;
/// Líneas de historial de la pantalla sin ventana (las mismas que la de la interfaz).
pub(crate) const SCROLLBACK: usize = 10_000;
const START_ROWS: u16 = 30;
const START_COLS: u16 = 100;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Shell,
    Claude,
}

/// Lo que se guarda de cada terminal para recuperarla al reiniciar nexo.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub(crate) struct Saved {
    pub slot: String,
    pub name: String,
    pub kind: Kind,
    /// La sesión de Claude Code que corría en ella (se retoma al volver).
    #[serde(default)]
    pub session: Option<String>,
    /// El agente del vault con el que se lanzó (su definición va en `<slot>.agents.json`).
    #[serde(default)]
    pub agent: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct SavedList {
    terminales: Vec<Saved>,
}

/// Lo que la interfaz y la orden de control saben de una terminal.
#[derive(Serialize, Clone, Debug)]
pub struct TermInfo {
    pub id: u64,
    /// Id estable: sobrevive al reinicio (las pestañas guardadas lo usan).
    pub slot: String,
    pub path: String,
    pub name: String,
    pub kind: Kind,
    /// Terminó su programa (el shell).
    pub exited: bool,
    /// Con qué código terminó, si se sabe.
    pub code: Option<i32>,
    /// El Claude Code que corre en ella, con su estado (de los hooks).
    pub claude: Option<crate::sesiones::SesionView>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TermEvent {
    Data { data: String },
    Exit { code: Option<i32> },
}

/// Cómo arrancar una terminal.
#[derive(Clone, Debug, Default)]
pub(crate) struct Launch {
    pub kind: Option<Kind>,
    pub name: Option<String>,
    /// Orden que el shell ejecuta al mostrar su primer prompt (queda en su historial).
    pub start: Option<String>,
    /// Sesión de Claude Code que se sabe desde el principio (`--session-id`, `--resume`).
    pub session: Option<String>,
    pub agent: Option<String>,
    /// Lo que se ve antes de que hable el shell (la salida recuperada).
    pub prefill: Vec<u8>,
    /// Id estable si se recupera una guardada.
    pub slot: Option<String>,
}

/// Dónde están los archivos de arranque del shell y el socket de control (ver `install_shell`).
#[derive(Clone, Debug)]
pub(crate) struct ShellFiles {
    /// Carpeta con el `claude` de nexo.
    pub bin: PathBuf,
    pub bashrc: PathBuf,
    pub zdotdir: PathBuf,
    pub fish: PathBuf,
    /// Carpeta de ejecución de nexo (socket, marcas de «esperando»).
    pub run: PathBuf,
    /// Socket de control de esta apertura de nexo.
    pub ctl: PathBuf,
}

// ---------------------------------------------------------------- salida

struct Out {
    ring: VecDeque<u8>,
    screen: vt100::Parser,
    /// La interfaz conectada (con su número: una conexión vieja no recibe nada).
    sink: Option<(u64, Channel<TermEvent>)>,
    /// Bytes de un carácter UTF-8 a medias, para el siguiente trozo.
    pending: Vec<u8>,
    exit: Option<Option<i32>>,
    /// Cambió desde que se guardó.
    dirty: bool,
}

impl Out {
    fn new(prefill: &[u8]) -> Self {
        let mut out = Out {
            ring: VecDeque::new(),
            screen: vt100::Parser::new(START_ROWS, START_COLS, SCROLLBACK),
            sink: None,
            pending: Vec::new(),
            exit: None,
            dirty: false,
        };
        out.push(prefill);
        out
    }

    fn push(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.screen.process(bytes);
        self.ring.extend(bytes);
        if self.ring.len() > RING_MAX {
            // Se recorta hasta el principio de una línea: así lo repetido no empieza en medio
            // de una secuencia de escape.
            let mut cut = self.ring.len() - RING_MAX;
            while cut < self.ring.len() && self.ring[cut - 1] != b'\n' {
                cut += 1;
            }
            self.ring.drain(..cut);
        }
        self.dirty = true;
    }

    fn bytes(&self) -> Vec<u8> {
        self.ring.iter().copied().collect()
    }
}

/// Texto de un trozo de salida sin partir un carácter UTF-8: lo que queda a medias espera al
/// siguiente trozo.
pub(crate) fn decode(pending: &mut Vec<u8>, chunk: &[u8]) -> String {
    pending.extend_from_slice(chunk);
    let upto = match std::str::from_utf8(pending) {
        Ok(_) => pending.len(),
        Err(e) if e.error_len().is_none() => e.valid_up_to(),
        Err(_) => pending.len(),
    };
    let text = String::from_utf8_lossy(&pending[..upto]).into_owned();
    pending.drain(..upto);
    text
}

/// Lo guardado sin el carácter que el recorte dejó a medias al principio.
fn replay_text(bytes: &[u8]) -> String {
    let start = bytes
        .iter()
        .position(|b| b & 0b1100_0000 != 0b1000_0000)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

/// Las últimas `n` líneas de la pantalla (con su historial), sin códigos de escape.
pub(crate) fn tail_lines(parser: &mut vt100::Parser, n: usize) -> String {
    let (rows, cols) = parser.screen().size();
    let rows = rows as usize;
    let current: Vec<String> = parser.screen().rows(0, cols).collect();
    let mut above: Vec<String> = Vec::new();
    let mut prev = 0;
    while above.len() + current.len() < n {
        parser.screen_mut().set_scrollback(prev + rows);
        let now = parser.screen().scrollback();
        if now <= prev {
            break;
        }
        let page: Vec<String> = parser.screen().rows(0, cols).collect();
        let fresh = (now - prev).min(page.len());
        let mut chunk: Vec<String> = page[..fresh].to_vec();
        chunk.append(&mut above);
        above = chunk;
        prev = now;
    }
    parser.screen_mut().set_scrollback(0);
    let mut lines: Vec<String> = above.into_iter().chain(current).collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let skip = lines.len().saturating_sub(n);
    lines[skip..]
        .iter()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------- una terminal

struct Proc {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    pid: Option<u32>,
}

pub(crate) struct Term {
    pub id: u64,
    pub root: PathBuf,
    pub slot: String,
    pub kind: Kind,
    name: Mutex<String>,
    proc: Mutex<Option<Proc>>,
    out: Mutex<Out>,
    session: Mutex<Option<String>>,
    agent: Option<String>,
    /// La cerró nexo (su «×», la orden de control o al cerrarse la app): no es una caída.
    closing: AtomicBool,
}

impl Term {
    pub fn name(&self) -> String {
        self.name.lock().map(|n| n.clone()).unwrap_or_default()
    }

    pub fn session(&self) -> Option<String> {
        self.session.lock().ok().and_then(|s| s.clone())
    }

    pub fn set_session(&self, session: Option<String>) -> bool {
        let Ok(mut s) = self.session.lock() else {
            return false;
        };
        if *s == session {
            return false;
        }
        *s = session;
        true
    }

    pub fn closing(&self) -> bool {
        self.closing.load(Ordering::Relaxed)
    }

    pub fn exited(&self) -> Option<Option<i32>> {
        self.out.lock().ok().and_then(|o| o.exit)
    }

    fn saved(&self) -> Saved {
        Saved {
            slot: self.slot.clone(),
            name: self.name(),
            kind: self.kind,
            session: self.session(),
            agent: self.agent.clone(),
        }
    }

    /// Escribe en el shell (lo que teclea el usuario o la orden de control).
    pub fn write(&self, data: &[u8]) -> Result<(), String> {
        let mut proc = self.proc.lock().map_err(|e| e.to_string())?;
        let p = proc.as_mut().ok_or("Esa terminal ya terminó.")?;
        p.writer.write_all(data).map_err(|e| e.to_string())?;
        p.writer.flush().map_err(|e| e.to_string())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        let (cols, rows) = (cols.clamp(2, 1000), rows.clamp(2, 500));
        if let Ok(mut out) = self.out.lock() {
            out.screen.screen_mut().set_size(rows, cols);
        }
        let proc = self.proc.lock().map_err(|e| e.to_string())?;
        if let Some(p) = proc.as_ref() {
            p.master
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

    /// Hay un programa en primer plano que no es el shell (un `npm run dev`, Claude Code…).
    pub fn busy(&self) -> bool {
        let Ok(proc) = self.proc.lock() else {
            return false;
        };
        let Some(p) = proc.as_ref() else {
            return false;
        };
        match (p.master.process_group_leader(), p.pid) {
            (Some(leader), Some(pid)) => leader != pid as i32,
            _ => false,
        }
    }

    /// Las últimas `n` líneas de su pantalla.
    pub fn screen_tail(&self, n: usize) -> String {
        self.out
            .lock()
            .map(|mut o| tail_lines(&mut o.screen, n))
            .unwrap_or_default()
    }

    /// Conecta la interfaz: le repite lo guardado y le manda lo que llegue.
    pub fn attach(&self, channel: Channel<TermEvent>) -> Result<(), String> {
        static ATTACH: AtomicU64 = AtomicU64::new(1);
        let mut out = self.out.lock().map_err(|e| e.to_string())?;
        let text = replay_text(&out.bytes());
        if !text.is_empty() {
            channel
                .send(TermEvent::Data { data: text })
                .map_err(|e| e.to_string())?;
        }
        if let Some(code) = out.exit {
            let _ = channel.send(TermEvent::Exit { code });
        }
        out.pending.clear();
        out.sink = Some((ATTACH.fetch_add(1, Ordering::Relaxed), channel));
        Ok(())
    }

    /// La cierra: cuelga el shell y lo que corre en primer plano (SIGHUP, como al cerrar una
    /// ventana de terminal) y, si siguen vivos al rato, los mata.
    pub fn close(&self) {
        self.closing.store(true, Ordering::Relaxed);
        let proc = self.proc.lock().ok().and_then(|mut p| p.take());
        if let Some(p) = proc {
            hang_up(p);
        }
    }
}

fn hang_up(p: Proc) {
    let Proc {
        master,
        writer,
        mut child,
        pid,
    } = p;
    let fg = master.process_group_leader();
    #[cfg(unix)]
    unsafe {
        if let Some(fg) = fg {
            libc::kill(-fg, libc::SIGHUP);
        }
        if let Some(pid) = pid {
            libc::kill(-(pid as i32), libc::SIGHUP);
            libc::kill(pid as i32, libc::SIGHUP);
        }
    }
    drop(writer);
    drop(master);
    std::thread::spawn(move || {
        for _ in 0..30 {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        #[cfg(unix)]
        unsafe {
            if let Some(fg) = fg {
                libc::kill(-fg, libc::SIGKILL);
            }
            if let Some(pid) = pid {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
        let _ = child.kill();
        let _ = child.wait();
    });
}

// ---------------------------------------------------------------- arrancar

/// Comilla simple de POSIX (vale en bash, zsh y fish): nada de lo de dentro se interpreta.
pub(crate) fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// El shell del usuario: `$SHELL`, si existe; si no, `/bin/bash`.
pub(crate) fn user_shell() -> PathBuf {
    std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_file())
        .unwrap_or_else(|| PathBuf::from("/bin/bash"))
}

/// La orden del shell con su arranque de nexo, y si sabe ejecutar la orden de inicio
/// (`NEXO_START`) al mostrar su primer prompt.
pub(crate) fn shell_command(shell: &Path, files: &ShellFiles) -> (CommandBuilder, bool) {
    let mut cmd = CommandBuilder::new(shell);
    let name = shell
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let knows_start = match name.as_str() {
        "bash" => {
            cmd.args([
                "--rcfile".into(),
                files.bashrc.as_os_str().to_owned(),
                "-i".into(),
            ]);
            true
        }
        "zsh" => {
            cmd.env("ZDOTDIR", &files.zdotdir);
            cmd.arg("-i");
            true
        }
        "fish" => {
            cmd.args(["-i", "--init-command"]);
            cmd.arg(format!("source {}", quote(&files.fish.to_string_lossy())));
            true
        }
        _ => {
            cmd.arg("-i");
            false
        }
    };
    (cmd, knows_start)
}

fn spawn(
    root: &Path,
    id: u64,
    ctx: &Ctx,
    start: Option<&str>,
) -> Result<(Proc, Box<dyn Read + Send>, bool), String> {
    let files = ctx.files;
    let pty = native_pty_system()
        .openpty(PtySize {
            rows: START_ROWS,
            cols: START_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;
    let (mut cmd, knows_start) = shell_command(ctx.shell, files);
    cmd.cwd(root);
    for (key, value) in &ctx.env {
        cmd.env(key, value);
    }
    for var in INHERITED_ENV {
        cmd.env_remove(var);
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![files.bin.clone()];
    paths.extend(std::env::split_paths(&path).filter(|p| *p != files.bin));
    cmd.env(
        "PATH",
        std::env::join_paths(paths).map_err(|e| e.to_string())?,
    );
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "nexo");
    cmd.env("NEXO_TERMINAL", id.to_string());
    cmd.env("NEXO_WORKSPACE", root);
    cmd.env("NEXO_RUN", &files.run);
    cmd.env("NEXO_CTL", &files.ctl);
    cmd.env_remove("NEXO_START");
    if let (Some(start), true) = (start, knows_start) {
        cmd.env("NEXO_START", start);
    }
    let child = pty
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("No se pudo abrir el shell: {e}"))?;
    drop(pty.slave);
    let reader = pty.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pty.master.take_writer().map_err(|e| e.to_string())?;
    let pid = child.process_id();
    Ok((
        Proc {
            master: pty.master,
            writer,
            child,
            pid,
        },
        reader,
        knows_start,
    ))
}

/// Separador entre lo recuperado y el shell nuevo: deja la terminal en un estado normal (sin
/// pantalla alternativa, con cursor, sin ratón) y una línea tenue con la fecha.
pub(crate) fn restored_mark(when: &str) -> Vec<u8> {
    format!(
        "\x1b[?1049l\x1b[?25h\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l\x1b[0m\r\n\x1b[2m— nexo se cerró · {when} —\x1b[0m\r\n"
    )
    .into_bytes()
}

// ---------------------------------------------------------------- todas

/// Avisos de lo que pasa en las terminales (los pone `lib.rs`; las pruebas, otros).
pub(crate) trait Oyente: Send + Sync {
    /// Terminó el programa de una terminal.
    fn exited(&self, term: &Arc<Term>, code: Option<i32>);
    /// Cambió la lista de un workspace (para la interfaz).
    fn changed(&self, root: &Path);
}

#[derive(Default)]
pub struct Terminales {
    terms: Mutex<Vec<Arc<Term>>>,
    /// Workspaces que ya recuperaron sus terminales en esta apertura de nexo.
    restored: Mutex<HashSet<PathBuf>>,
}

/// Lo que hace falta para abrir terminales: dónde guardar, los archivos del shell y a quién
/// avisar.
pub(crate) struct Ctx<'a> {
    /// `<datos de nexo>/terminales`.
    pub store: &'a Path,
    pub files: &'a ShellFiles,
    pub shell: &'a Path,
    pub oyente: Arc<dyn Oyente>,
    /// Variables que se añaden al entorno del shell (las pruebas: su `HOME`).
    pub env: Vec<(String, String)>,
}

fn store_dir(store: &Path, root: &Path) -> PathBuf {
    store.join(crate::registro::project_dir_name(root))
}

impl Terminales {
    pub fn get(&self, id: u64) -> Option<Arc<Term>> {
        self.terms.lock().ok()?.iter().find(|t| t.id == id).cloned()
    }

    pub fn of(&self, root: Option<&Path>) -> Vec<Arc<Term>> {
        self.terms
            .lock()
            .map(|t| {
                t.iter()
                    .filter(|t| root.is_none_or(|r| t.root == r))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn all(&self) -> Vec<Arc<Term>> {
        self.of(None)
    }

    /// Nombre libre: «Terminal», «Terminal 2»… o «Claude Code», «Claude Code 2»…
    fn free_name(&self, root: &Path, kind: Kind) -> String {
        let base = match kind {
            Kind::Shell => "Terminal",
            Kind::Claude => "Claude Code",
        };
        let taken: HashSet<String> = self.of(Some(root)).iter().map(|t| t.name()).collect();
        (1..)
            .map(|n| {
                if n == 1 {
                    base.to_string()
                } else {
                    format!("{base} {n}")
                }
            })
            .find(|n| !taken.contains(n))
            .unwrap_or_else(|| base.to_string())
    }

    /// Abre una terminal en `root`.
    pub(crate) fn open(&self, ctx: &Ctx, root: &Path, launch: Launch) -> Result<Arc<Term>, String> {
        {
            let terms = self.terms.lock().map_err(|e| e.to_string())?;
            if terms.len() >= MAX_TOTAL {
                return Err(format!(
                    "Ya hay {MAX_TOTAL} terminales abiertas en nexo: cierra alguna."
                ));
            }
            if terms.iter().filter(|t| t.root == root).count() >= MAX_PER_WS {
                return Err(format!(
                    "Este workspace ya tiene {MAX_PER_WS} terminales: cierra alguna."
                ));
            }
        }
        let kind = launch.kind.unwrap_or(Kind::Shell);
        let name = launch
            .name
            .map(|n| clean_name(&n))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| self.free_name(root, kind));
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let slot = launch
            .slot
            .filter(|s| crate::workspace::safe_id(s))
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string()[..12].to_string());
        let (proc, mut reader, knows_start) = spawn(root, id, ctx, launch.start.as_deref())?;
        let term = Arc::new(Term {
            id,
            root: root.to_path_buf(),
            slot,
            kind,
            name: Mutex::new(name),
            proc: Mutex::new(Some(proc)),
            out: Mutex::new(Out::new(&launch.prefill)),
            session: Mutex::new(launch.session),
            agent: launch.agent,
            closing: AtomicBool::new(false),
        });
        self.terms
            .lock()
            .map_err(|e| e.to_string())?
            .push(term.clone());
        // Un shell que no sabe ejecutar la orden de inicio la recibe tecleada en cuanto habla.
        let typed = (!knows_start).then_some(launch.start).flatten();
        let reading = term.clone();
        let oyente = ctx.oyente.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            let mut typed = typed;
            loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                if let Ok(mut out) = reading.out.lock() {
                    out.push(&buf[..n]);
                    if let Some((_, sink)) = out.sink.clone() {
                        let text = decode(&mut out.pending, &buf[..n]);
                        if !text.is_empty() && sink.send(TermEvent::Data { data: text }).is_err() {
                            out.sink = None;
                        }
                    }
                }
                if let Some(cmd) = typed.take() {
                    std::thread::sleep(Duration::from_millis(150));
                    let _ = reading.write(format!("{cmd}\r").as_bytes());
                }
            }
            let code = reading
                .proc
                .lock()
                .ok()
                .and_then(|mut p| p.as_mut().and_then(|p| p.child.try_wait().ok().flatten()))
                .map(|s| s.exit_code() as i32);
            if let Ok(mut out) = reading.out.lock() {
                out.exit = Some(code);
                out.dirty = true;
                if let Some((_, sink)) = out.sink.as_ref() {
                    let _ = sink.send(TermEvent::Exit { code });
                }
            }
            oyente.exited(&reading, code);
            oyente.changed(&reading.root);
        });
        self.save_list(ctx.store, root);
        ctx.oyente.changed(root);
        Ok(term)
    }

    /// Cierra una terminal y la quita de lo guardado.
    pub(crate) fn close(&self, ctx: &Ctx, id: u64) -> Result<(), String> {
        let term = {
            let mut terms = self.terms.lock().map_err(|e| e.to_string())?;
            let at = terms
                .iter()
                .position(|t| t.id == id)
                .ok_or("Esa terminal ya no existe.")?;
            terms.remove(at)
        };
        term.close();
        let _ =
            fs::remove_file(store_dir(ctx.store, &term.root).join(format!("{}.log", term.slot)));
        let _ = fs::remove_file(
            store_dir(ctx.store, &term.root).join(format!("{}.agents.json", term.slot)),
        );
        self.save_list(ctx.store, &term.root);
        ctx.oyente.changed(&term.root);
        Ok(())
    }

    /// Recupera las terminales guardadas de `root` la primera vez que se piden en esta
    /// apertura de nexo (si no tiene ninguna viva).
    pub(crate) fn restore(&self, ctx: &Ctx, root: &Path, claude: Option<&Path>) -> Vec<String> {
        let mut errors = Vec::new();
        {
            let Ok(mut done) = self.restored.lock() else {
                return errors;
            };
            if !done.insert(root.to_path_buf()) || !self.of(Some(root)).is_empty() {
                return errors;
            }
        }
        let dir = store_dir(ctx.store, root);
        let list: SavedList = fs::read_to_string(dir.join("terminales.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let when = chrono::Local::now().format("%d-%m-%Y %H:%M").to_string();
        for saved in list.terminales {
            if !crate::workspace::safe_id(&saved.slot) {
                continue;
            }
            let mut prefill = fs::read(dir.join(format!("{}.log", saved.slot))).unwrap_or_default();
            if !prefill.is_empty() {
                prefill.extend(restored_mark(&when));
            }
            let agents = dir.join(format!("{}.agents.json", saved.slot));
            let start = saved
                .session
                .as_deref()
                .filter(|s| uuid::Uuid::parse_str(s).is_ok())
                .and_then(|s| {
                    claude?;
                    let mut line = format!("claude --resume {s}");
                    if let Some(agent) = &saved.agent
                        && agents.is_file()
                    {
                        line.push_str(&format!(
                            " --agents {} --agent {}",
                            quote(&agents.to_string_lossy()),
                            quote(agent)
                        ));
                    }
                    Some(line)
                });
            let launch = Launch {
                kind: Some(saved.kind),
                name: Some(saved.name.clone()),
                session: saved.session.clone().filter(|_| start.is_some()),
                agent: saved.agent.clone(),
                start,
                prefill,
                slot: Some(saved.slot.clone()),
            };
            if let Err(e) = self.open(ctx, root, launch) {
                errors.push(format!("{}: {e}", saved.name));
            }
        }
        errors
    }

    /// Guarda la lista de terminales de `root`, en su orden.
    pub(crate) fn save_list(&self, store: &Path, root: &Path) {
        let list = SavedList {
            terminales: self.of(Some(root)).iter().map(|t| t.saved()).collect(),
        };
        let dir = store_dir(store, root);
        let text = serde_json::to_string_pretty(&list).unwrap_or_default();
        if let Err(e) = write_private(&dir.join("terminales.json"), text.as_bytes()) {
            log::warn!("No se pudo guardar la lista de terminales: {e}");
        }
    }

    /// Guarda la salida de las terminales que cambiaron (`all`: todas).
    pub(crate) fn save_output(&self, store: &Path, all: bool) {
        self.save_output_of(store, None, all);
    }

    fn save_output_of(&self, store: &Path, root: Option<&Path>, all: bool) {
        for term in self.of(root) {
            let bytes = {
                let Ok(mut out) = term.out.lock() else {
                    continue;
                };
                if !out.dirty && !all {
                    continue;
                }
                out.dirty = false;
                out.bytes()
            };
            let path = store_dir(store, &term.root).join(format!("{}.log", term.slot));
            if let Err(e) = write_private(&path, &bytes) {
                log::warn!("No se pudo guardar la salida de una terminal: {e}");
            }
        }
    }

    /// Un workspace sale del panel: sus terminales se cuelgan. Quitado del panel conserva lo
    /// guardado (al volver a abrirlo se recuperan); olvidado (`forget`), se borra.
    pub(crate) fn leave(&self, store: &Path, root: &Path, forget: bool) {
        if !forget {
            self.save_list(store, root);
            self.save_output_of(store, Some(root), true);
        }
        let gone: Vec<Arc<Term>> = {
            let Ok(mut terms) = self.terms.lock() else {
                return;
            };
            let (gone, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut *terms)
                .into_iter()
                .partition(|t| t.root == root);
            *terms = keep;
            gone
        };
        for term in gone {
            term.close();
        }
        if let Ok(mut done) = self.restored.lock() {
            done.remove(root);
        }
        if forget {
            let _ = fs::remove_dir_all(store_dir(store, root));
        }
    }

    /// Al cerrar nexo: guarda todo y cuelga todas.
    pub(crate) fn shutdown(&self, store: &Path) {
        let roots: HashSet<PathBuf> = self.all().iter().map(|t| t.root.clone()).collect();
        for root in &roots {
            self.save_list(store, root);
        }
        self.save_output(store, true);
        let terms: Vec<Arc<Term>> = self
            .terms
            .lock()
            .map(|mut t| std::mem::take(&mut *t))
            .unwrap_or_default();
        for term in terms {
            term.close();
        }
    }
}

/// Un nombre visible: una línea, sin controles, como mucho 60 caracteres.
pub(crate) fn clean_name(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_control())
        .take(60)
        .collect::<String>()
        .trim()
        .to_string()
}

// ---------------------------------------------------------------- archivos del shell

const BASHRC: &str = r#"# Lo carga nexo en sus terminales (bash --rcfile): primero tu configuración de siempre y
# después, delante en el PATH, el claude de nexo (avisa a nexo de si trabaja o espera).
if [ -f "$HOME/.bashrc" ]; then . "$HOME/.bashrc"; fi
export PATH="{bin}:$PATH"
if [ -n "${NEXO_START-}" ]; then
  __nexo_start=$NEXO_START
  unset NEXO_START
  __nexo_prompt_command=${PROMPT_COMMAND-}
  __nexo_run() {
    PROMPT_COMMAND=$__nexo_prompt_command
    unset __nexo_prompt_command
    local c=$__nexo_start
    unset __nexo_start
    unset -f __nexo_run
    history -s "$c"
    eval "$c"
    if [ -n "$PROMPT_COMMAND" ]; then eval "$PROMPT_COMMAND"; fi
  }
  PROMPT_COMMAND=__nexo_run
fi
"#;

const ZSHENV: &str = r#"# Lo carga nexo en sus terminales (ZDOTDIR): tu .zshenv de siempre.
if [ -f "$HOME/.zshenv" ]; then ZDOTDIR=$HOME . "$HOME/.zshenv"; fi
"#;

const ZSHRC: &str = r#"# Lo carga nexo en sus terminales: tu .zshrc de siempre y después, delante en el PATH, el
# claude de nexo (avisa a nexo de si trabaja o espera).
ZDOTDIR=$HOME
if [ -f "$HOME/.zshrc" ]; then . "$HOME/.zshrc"; fi
export PATH="{bin}:$PATH"
if [[ -n ${NEXO_START-} ]]; then
  __nexo_start=$NEXO_START
  unset NEXO_START
  __nexo_run() {
    precmd_functions=(${precmd_functions:#__nexo_run})
    local c=$__nexo_start
    unset __nexo_start
    print -s -- "$c"
    eval "$c"
  }
  precmd_functions+=(__nexo_run)
fi
"#;

const FISH: &str = r#"# Lo carga nexo en sus terminales (fish --init-command, después de tu configuración): delante
# en el PATH, el claude de nexo (avisa a nexo de si trabaja o espera).
set -gx PATH {bin} $PATH
if set -q NEXO_START
    set -g __nexo_start $NEXO_START
    set -e NEXO_START
    function __nexo_run --on-event fish_prompt
        functions -e __nexo_run
        eval $__nexo_start
        set -e __nexo_start
    end
end
"#;

const LAUNCHER: &str = r#"#!/bin/sh
# El claude de las terminales de nexo (nexo lo pone delante en su PATH): ejecuta el de verdad y
# le añade, solo a esta sesión, los hooks con los que nexo sabe si trabaja, espera una
# respuesta o terminó. Tu ~/.claude no se toca.
dir=$(dirname -- "$0")
real=
old_ifs=$IFS
IFS=:
for d in $PATH; do
  [ "$d" = "$dir" ] && continue
  if [ -x "$d/claude" ] && [ ! -d "$d/claude" ]; then real=$d/claude; break; fi
done
IFS=$old_ifs
if [ -z "$real" ]; then echo "nexo: no encuentro claude en el PATH" >&2; exit 127; fi
# Un claude que lanza otro Claude Code, fuera de una terminal de nexo, o una orden de gestión:
# tal cual.
if [ -n "${CLAUDECODE-}" ] || [ -z "${NEXO_TERMINAL-}" ]; then exec "$real" "$@"; fi
case "${1-}" in
  agents|attach|auth|auto-mode|doctor|gateway|import|install|logs|mcp|plugin|plugins|purge|respawn|rm|setup-token|stop|kill|ultrareview|update|upgrade|-v|--version|-h|--help) exec "$real" "$@" ;;
esac
exec "$real" --settings {hooks} "$@"
"#;

/// Los hooks de una sesión de Claude Code de una terminal de nexo: cada uno ejecuta
/// `nexo ctl hook`, que manda a nexo el evento sin la entrada de la herramienta ni el texto del
/// prompt. Los que llegan con cada herramienta (`PostToolUse`…) solo llaman a nexo si esa
/// terminal esperaba una respuesta (la marca de `<run>/esperando/`), para no lanzar un proceso
/// por cada herramienta.
pub(crate) fn hooks_settings(exe: &Path) -> Value {
    let call = format!("{} ctl hook", quote(&exe.to_string_lossy()));
    let only_waiting =
        format!("[ -e \"$NEXO_RUN/esperando/$NEXO_TERMINAL\" ] || exit 0; exec {call}");
    let hook = |command: &str| json!([{ "hooks": [{ "type": "command", "command": command, "timeout": 5 }] }]);
    json!({
        "hooks": {
            "SessionStart": hook(&call),
            "SessionEnd": hook(&call),
            "UserPromptSubmit": hook(&call),
            "Stop": hook(&call),
            "StopFailure": hook(&call),
            "PermissionRequest": hook(&call),
            "PreToolUse": [{ "matcher": "AskUserQuestion|ExitPlanMode", "hooks": [{ "type": "command", "command": call, "timeout": 5 }] }],
            "PostToolUse": hook(&only_waiting),
            "PostToolUseFailure": hook(&only_waiting),
            "PermissionDenied": hook(&only_waiting),
            "SubagentStart": hook(&call),
            "SubagentStop": hook(&call),
        }
    })
}

/// Deja en `<datos>/terminal/` los archivos de arranque del shell, el `claude` de nexo y sus
/// hooks (con las rutas de verdad), y devuelve dónde están. Se rehacen en cada apertura de
/// nexo: el ejecutable puede haber cambiado de sitio.
pub(crate) fn install_shell(
    data: &Path,
    exe: &Path,
    run: &Path,
    ctl: &Path,
) -> Result<ShellFiles, String> {
    let dir = data.join("terminal");
    let bin = dir.join("bin");
    let zdotdir = dir.join("zsh");
    fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    fs::create_dir_all(&zdotdir).map_err(|e| e.to_string())?;
    let hooks = dir.join("hooks.json");
    let bin_text = bin.to_string_lossy();
    let write = |path: &Path, text: &str| -> Result<(), String> {
        if fs::read_to_string(path).ok().as_deref() != Some(text) {
            fs::write(path, text).map_err(|e| e.to_string())?;
        }
        Ok(())
    };
    let settings = serde_json::to_string_pretty(&hooks_settings(exe)).map_err(|e| e.to_string())?;
    write(&hooks, &settings)?;
    let launcher = LAUNCHER.replace("{hooks}", &quote(&hooks.to_string_lossy()));
    write(&bin.join("claude"), &launcher)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(bin.join("claude"), fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    // Dentro de comillas dobles en bash y zsh: la ruta no lleva comillas ni `$` (es la carpeta
    // de datos de nexo), pero por si acaso se escapan.
    let dq = bin_text
        .replace('\\', r"\\")
        .replace('"', "\\\"")
        .replace('$', "\\$");
    let bashrc = dir.join("bashrc");
    write(&bashrc, &BASHRC.replace("{bin}", &dq))?;
    write(&zdotdir.join(".zshenv"), ZSHENV)?;
    write(&zdotdir.join(".zshrc"), &ZSHRC.replace("{bin}", &dq))?;
    let fish = dir.join("nexo.fish");
    write(&fish, &FISH.replace("{bin}", &quote(&bin_text)))?;
    Ok(ShellFiles {
        bin,
        bashrc,
        zdotdir,
        fish,
        run: run.to_path_buf(),
        ctl: ctl.to_path_buf(),
    })
}

/// Carpeta de ejecución de nexo: la del usuario (`$XDG_RUNTIME_DIR/nexo`) o, si no hay,
/// `/tmp/nexo-<uid>`. Sin crearla (lo que usa el cliente de `nexo ctl`).
pub(crate) fn run_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_dir());
    match base {
        Some(b) => b.join("nexo"),
        #[cfg(unix)]
        None => PathBuf::from(format!("/tmp/nexo-{}", unsafe { libc::getuid() })),
        #[cfg(not(unix))]
        None => std::env::temp_dir().join("nexo"),
    }
}

/// La carpeta de ejecución de nexo, creada solo para el usuario (0700).
pub(crate) fn run_dir() -> PathBuf {
    let dir = run_path();
    if fs::create_dir_all(&dir).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
        }
    }
    dir
}

// ---------------------------------------------------------------- comandos de Tauri

/// El estado de la app que necesitan las terminales.
pub(crate) struct AppTerms {
    pub store: PathBuf,
    pub files: ShellFiles,
    pub shell: PathBuf,
    pub oyente: Arc<dyn Oyente>,
}

impl AppTerms {
    pub fn ctx(&self) -> Ctx<'_> {
        Ctx {
            store: &self.store,
            files: &self.files,
            shell: &self.shell,
            oyente: self.oyente.clone(),
            env: Vec::new(),
        }
    }
}

/// Lo que se ha preparado al arrancar (`lib.rs`); `None` si falló (las terminales no abren).
#[derive(Default)]
pub struct TermSetup(pub Mutex<Option<Arc<AppTerms>>>);

pub(crate) fn app_terms(app: &AppHandle) -> Result<Arc<AppTerms>, String> {
    app.state::<TermSetup>()
        .0
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "Las terminales no se pudieron preparar al abrir nexo.".to_string())
}

pub(crate) fn info(app: &AppHandle, term: &Term) -> TermInfo {
    TermInfo {
        id: term.id,
        slot: term.slot.clone(),
        path: term.root.display().to_string(),
        name: term.name(),
        kind: term.kind,
        exited: term.exited().is_some(),
        code: term.exited().flatten(),
        claude: crate::sesiones::view_of(app, term.id),
    }
}

/// Las terminales de un workspace, en su orden. La primera vez en cada apertura de nexo
/// recupera las que tenía.
#[tauri::command]
pub fn terminal_list(
    app: AppHandle,
    state: State<'_, Terminales>,
    path: String,
) -> Result<Vec<TermInfo>, String> {
    let root = ws_root(&app, &path)?;
    let terms = app_terms(&app)?;
    let claude = crate::claude_code::find_claude(&app);
    for e in state.restore(&terms.ctx(), &root, claude.as_deref()) {
        log::warn!("No se pudo recuperar una terminal: {e}");
    }
    Ok(state
        .of(Some(&root))
        .iter()
        .map(|t| info(&app, t))
        .collect())
}

/// Abre una terminal: un shell vacío o, con `kind: claude`, uno que ejecuta `claude`.
#[tauri::command]
pub fn terminal_open(
    app: AppHandle,
    state: State<'_, Terminales>,
    path: String,
    kind: Kind,
) -> Result<TermInfo, String> {
    let root = ws_root(&app, &path)?;
    let terms = app_terms(&app)?;
    let launch = Launch {
        kind: Some(kind),
        start: (kind == Kind::Claude).then(|| "claude".to_string()),
        ..Launch::default()
    };
    let term = state.open(&terms.ctx(), &root, launch)?;
    Ok(info(&app, &term))
}

#[tauri::command]
pub fn terminal_attach(
    state: State<'_, Terminales>,
    id: u64,
    channel: Channel<TermEvent>,
) -> Result<(), String> {
    state
        .get(id)
        .ok_or("Esa terminal ya no existe.")?
        .attach(channel)
}

#[tauri::command]
pub fn terminal_input(state: State<'_, Terminales>, id: u64, data: String) -> Result<(), String> {
    state
        .get(id)
        .ok_or("Esa terminal ya no existe.")?
        .write(data.as_bytes())
}

#[tauri::command]
pub fn terminal_resize(
    state: State<'_, Terminales>,
    id: u64,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state
        .get(id)
        .ok_or("Esa terminal ya no existe.")?
        .resize(cols, rows)
}

/// Si cerrarla cortaría algo: un programa en primer plano o su Claude trabajando o esperando.
#[tauri::command]
pub fn terminal_busy(
    app: AppHandle,
    state: State<'_, Terminales>,
    id: u64,
) -> Result<bool, String> {
    let term = state.get(id).ok_or("Esa terminal ya no existe.")?;
    let claude = crate::sesiones::view_of(&app, id).is_some_and(|v| v.state != "idle");
    Ok(term.busy() || claude)
}

/// Si cerrar las terminales de un workspace cortaría algo (antes de quitarlo del panel). No
/// recupera las guardadas.
#[tauri::command]
pub fn terminal_busy_in(
    app: AppHandle,
    state: State<'_, Terminales>,
    path: String,
) -> Result<bool, String> {
    let root = ws_root(&app, &path)?;
    Ok(state.of(Some(&root)).iter().any(|t| {
        t.busy() || crate::sesiones::view_of(&app, t.id).is_some_and(|v| v.state != "idle")
    }))
}

#[tauri::command]
pub fn terminal_close(app: AppHandle, state: State<'_, Terminales>, id: u64) -> Result<(), String> {
    let terms = app_terms(&app)?;
    state.close(&terms.ctx(), id)
}

/// Un workspace sale del panel (`forget`: se olvida del todo): ver `Terminales::leave`.
pub(crate) fn leave_workspace(app: &AppHandle, root: &Path, forget: bool) {
    let Ok(terms) = app_terms(app) else {
        return;
    };
    app.state::<Terminales>().leave(&terms.store, root, forget);
    terms.oyente.changed(root);
}

/// El oyente de la app: la caída de un programa la cuenta `sesiones.rs` y los cambios de lista
/// salen en el evento global `workspace-terminals` (`{ path }`).
pub(crate) struct AppOyente(pub AppHandle);

impl Oyente for AppOyente {
    fn exited(&self, term: &Arc<Term>, _code: Option<i32>) {
        crate::sesiones::terminal_exited(&self.0, term);
    }
    fn changed(&self, root: &Path) {
        let _ = self.0.emit(
            "workspace-terminals",
            json!({ "path": root.display().to_string() }),
        );
    }
}

/// Guarda la salida cada pocos segundos (si cambió) y mira si un Claude se cayó sin avisar.
pub(crate) fn keep_saving(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(5));
            let Ok(terms) = app_terms(&app) else {
                continue;
            };
            app.state::<Terminales>().save_output(&terms.store, false);
            crate::sesiones::sweep(&app);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nexo-terminales-{name}-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[derive(Default)]
    struct Contador {
        exits: AtomicUsize,
        changes: AtomicUsize,
    }

    impl Oyente for Contador {
        fn exited(&self, _: &Arc<Term>, _: Option<i32>) {
            self.exits.fetch_add(1, Ordering::Relaxed);
        }
        fn changed(&self, _: &Path) {
            self.changes.fetch_add(1, Ordering::Relaxed);
        }
    }

    struct Banco {
        dir: PathBuf,
        root: PathBuf,
        files: ShellFiles,
        store: PathBuf,
        oyente: Arc<Contador>,
    }

    impl Banco {
        fn new(name: &str) -> Self {
            let dir = temp(name);
            let root = dir.join("proyecto");
            fs::create_dir_all(&root).unwrap();
            let run = dir.join("run");
            fs::create_dir_all(&run).unwrap();
            let files = install_shell(
                &dir.join("datos"),
                Path::new("/bin/true"),
                &run,
                &run.join("ctl.sock"),
            )
            .unwrap();
            Banco {
                store: dir.join("datos/terminales"),
                dir,
                root,
                files,
                oyente: Arc::new(Contador::default()),
            }
        }
        fn ctx<'a>(&'a self, shell: &'a Path) -> Ctx<'a> {
            Ctx {
                store: &self.store,
                files: &self.files,
                shell,
                oyente: self.oyente.clone(),
                env: Vec::new(),
            }
        }
    }

    fn screen_of(term: &Term) -> String {
        term.screen_tail(200)
    }

    fn wait_for(term: &Term, text: &str) -> String {
        for _ in 0..200 {
            let s = screen_of(term);
            if s.contains(text) {
                return s;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("no apareció «{text}» en la terminal:\n{}", screen_of(term));
    }

    #[test]
    fn un_shell_escribe_lee_cambia_de_tamano_y_se_cierra() {
        let banco = Banco::new("basico");
        let sh = PathBuf::from("/bin/sh");
        let terms = Terminales::default();
        let ctx = banco.ctx(&sh);
        let term = terms.open(&ctx, &banco.root, Launch::default()).unwrap();
        assert_eq!(term.name(), "Terminal");
        term.write(b"echo hola-$((40+2))\n").unwrap();
        wait_for(&term, "hola-42");
        term.resize(77, 21).unwrap();
        term.write(b"stty size\n").unwrap();
        wait_for(&term, "21 77");
        // Un programa en primer plano: la terminal está ocupada.
        term.write(b"sleep 30\n").unwrap();
        let mut busy = false;
        for _ in 0..100 {
            if term.busy() {
                busy = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(busy, "con sleep en marcha la terminal está ocupada");
        let pid = term.proc.lock().unwrap().as_ref().unwrap().pid.unwrap();
        terms.close(&ctx, term.id).unwrap();
        assert!(terms.get(term.id).is_none());
        // El shell y su sleep se van.
        let mut gone = false;
        for _ in 0..100 {
            if unsafe { libc::kill(pid as i32, 0) } != 0 {
                gone = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        assert!(gone, "el shell sigue vivo tras cerrarlo");
        let _ = fs::remove_dir_all(&banco.dir);
    }

    #[test]
    fn el_entorno_no_lleva_lo_de_claude_code_y_si_lo_de_nexo() {
        let banco = Banco::new("entorno");
        let sh = PathBuf::from("/bin/sh");
        let terms = Terminales::default();
        // Como si nexo se hubiera abierto desde una terminal de Claude Code.
        let mut ctx = banco.ctx(&sh);
        ctx.env = vec![("CLAUDE_CODE_CHILD_SESSION".into(), "1".into())];
        let term = terms.open(&ctx, &banco.root, Launch::default()).unwrap();
        term.write(b"echo hijo=${CLAUDE_CODE_CHILD_SESSION:-no} term=$NEXO_TERMINAL; pwd\n")
            .unwrap();
        let screen = wait_for(&term, &format!("term={}", term.id));
        assert!(screen.contains("hijo=no"), "{screen}");
        assert!(
            screen.contains("proyecto"),
            "arranca en la carpeta del workspace: {screen}"
        );
        term.write(b"echo ${PATH%%:*}\n").unwrap();
        wait_for(&term, &banco.files.bin.to_string_lossy());
        terms.close(&ctx, term.id).unwrap();
        let _ = fs::remove_dir_all(&banco.dir);
    }

    #[test]
    fn bash_pone_el_claude_de_nexo_aunque_el_bashrc_anteponga_otra_carpeta() {
        let bash = PathBuf::from("/bin/bash");
        if !bash.is_file() {
            return;
        }
        let banco = Banco::new("bashrc");
        let home = banco.dir.join("home");
        let otra = banco.dir.join("otra-bin");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&otra).unwrap();
        fs::write(
            home.join(".bashrc"),
            format!("export PATH=\"{}:$PATH\"\nPS1='$ '\n", otra.display()),
        )
        .unwrap();
        let terms = Terminales::default();
        let mut ctx = banco.ctx(&bash);
        ctx.env = vec![("HOME".into(), home.to_string_lossy().into_owned())];
        let launch = Launch {
            start: Some("echo arrancado-$((1+1))".into()),
            ..Launch::default()
        };
        let term = terms.open(&ctx, &banco.root, launch).unwrap();
        wait_for(&term, "arrancado-2");
        term.write(b"echo primero=${PATH%%:*}\n").unwrap();
        wait_for(&term, &format!("primero={}", banco.files.bin.display()));
        terms.close(&ctx, term.id).unwrap();
        let _ = fs::remove_dir_all(&banco.dir);
    }

    #[test]
    fn una_interfaz_que_se_conecta_recibe_lo_anterior_y_lo_guardado_se_recupera() {
        let banco = Banco::new("recuperar");
        let sh = PathBuf::from("/bin/sh");
        let terms = Terminales::default();
        let ctx = banco.ctx(&sh);
        let launch = Launch {
            name: Some("Servidor".into()),
            ..Launch::default()
        };
        let term = terms.open(&ctx, &banco.root, launch).unwrap();
        term.write(b"echo antes-del-cierre\n").unwrap();
        wait_for(&term, "antes-del-cierre");
        assert!(replay_text(&term.out.lock().unwrap().bytes()).contains("antes-del-cierre"));
        terms.shutdown(&banco.store);
        assert!(terms.all().is_empty());
        // Otra apertura de nexo.
        let again = Terminales::default();
        let errors = again.restore(&ctx, &banco.root, None);
        assert!(errors.is_empty(), "{errors:?}");
        let back = again.of(Some(&banco.root));
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].name(), "Servidor");
        assert_eq!(back[0].slot, term.slot, "conserva su id estable");
        let screen = wait_for(&back[0], "nexo se cerró");
        assert!(screen.contains("antes-del-cierre"), "{screen}");
        // Solo la primera vez.
        assert!(again.restore(&ctx, &banco.root, None).is_empty());
        assert_eq!(again.of(Some(&banco.root)).len(), 1);
        // Cerrarla la quita de lo guardado.
        again.close(&ctx, back[0].id).unwrap();
        let list: SavedList = serde_json::from_str(
            &fs::read_to_string(store_dir(&banco.store, &banco.root).join("terminales.json"))
                .unwrap(),
        )
        .unwrap();
        assert!(list.terminales.is_empty());
        let _ = fs::remove_dir_all(&banco.dir);
    }

    #[test]
    fn lo_guardado_es_solo_del_usuario() {
        let banco = Banco::new("permisos");
        let sh = PathBuf::from("/bin/sh");
        let terms = Terminales::default();
        let ctx = banco.ctx(&sh);
        let term = terms.open(&ctx, &banco.root, Launch::default()).unwrap();
        term.write(b"echo x\n").unwrap();
        wait_for(&term, "x");
        terms.save_output(&banco.store, true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir = store_dir(&banco.store, &banco.root);
            for name in ["terminales.json".to_string(), format!("{}.log", term.slot)] {
                let mode = fs::metadata(dir.join(&name)).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "{name}");
            }
        }
        terms.close(&ctx, term.id).unwrap();
        let _ = fs::remove_dir_all(&banco.dir);
    }

    #[test]
    fn los_topes_de_terminales() {
        let banco = Banco::new("topes");
        let sh = PathBuf::from("/bin/sh");
        let terms = Terminales::default();
        let ctx = banco.ctx(&sh);
        let mut open = Vec::new();
        for _ in 0..MAX_PER_WS {
            open.push(terms.open(&ctx, &banco.root, Launch::default()).unwrap());
        }
        let names: HashSet<String> = open.iter().map(|t| t.name()).collect();
        assert_eq!(names.len(), MAX_PER_WS, "cada una con su nombre");
        assert!(names.contains("Terminal 2"));
        let err = terms
            .open(&ctx, &banco.root, Launch::default())
            .err()
            .unwrap();
        assert!(err.contains("12 terminales"), "{err}");
        for t in open {
            terms.close(&ctx, t.id).unwrap();
        }
        let _ = fs::remove_dir_all(&banco.dir);
    }

    #[test]
    fn el_texto_no_parte_caracteres_y_la_pantalla_da_su_historial() {
        let mut pending = Vec::new();
        let bytes = "año ñ".as_bytes();
        let a = decode(&mut pending, &bytes[..2]);
        let b = decode(&mut pending, &bytes[2..]);
        assert_eq!(format!("{a}{b}"), "año ñ");
        assert_eq!(
            replay_text(&bytes[2..]),
            "o ñ",
            "un carácter cortado al principio no sale"
        );
        let mut parser = vt100::Parser::new(5, 20, 100);
        for n in 1..=12 {
            parser.process(format!("linea {n}\r\n").as_bytes());
        }
        let tail = tail_lines(&mut parser, 8);
        assert_eq!(
            tail.lines().collect::<Vec<_>>(),
            (5..=12).map(|n| format!("linea {n}")).collect::<Vec<_>>()
        );
        assert_eq!(
            parser.screen().scrollback(),
            0,
            "la vista vuelve a su sitio"
        );
    }

    #[test]
    fn el_claude_de_nexo_solo_anade_los_hooks_en_sus_terminales() {
        let banco = Banco::new("lanzador");
        let fake = banco.dir.join("real");
        fs::create_dir_all(&fake).unwrap();
        fs::write(fake.join("claude"), "#!/bin/sh\necho \"real $*\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(fake.join("claude"), fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = format!(
            "{}:{}:/usr/bin:/bin",
            banco.files.bin.display(),
            fake.display()
        );
        let run = |args: &[&str], env: &[(&str, &str)]| {
            let out = std::process::Command::new(banco.files.bin.join("claude"))
                .args(args)
                .env_clear()
                .env("PATH", &path)
                .envs(env.iter().copied())
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let hooks = banco.dir.join("datos/terminal/hooks.json");
        assert_eq!(
            run(&["hola"], &[("NEXO_TERMINAL", "3")]),
            format!("real --settings {} hola", hooks.display())
        );
        assert_eq!(
            run(&["mcp", "list"], &[("NEXO_TERMINAL", "3")]),
            "real mcp list"
        );
        assert_eq!(
            run(&["hola"], &[]),
            "real hola",
            "fuera de una terminal de nexo"
        );
        assert_eq!(
            run(&["hola"], &[("NEXO_TERMINAL", "3"), ("CLAUDECODE", "1")]),
            "real hola",
            "un claude que lanza Claude Code"
        );
        let settings: Value = serde_json::from_str(&fs::read_to_string(&hooks).unwrap()).unwrap();
        for event in ["SessionStart", "Stop", "PermissionRequest", "SubagentStop"] {
            let cmd = settings["hooks"][event][0]["hooks"][0]["command"]
                .as_str()
                .unwrap();
            assert!(cmd.ends_with("ctl hook"), "{event}: {cmd}");
        }
        let post = settings["hooks"]["PostToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(post.starts_with("[ -e \"$NEXO_RUN/esperando/$NEXO_TERMINAL\" ] || exit 0"));
        let _ = fs::remove_dir_all(&banco.dir);
    }
}
