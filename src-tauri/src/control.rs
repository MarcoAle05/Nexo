//! La orden de control de nexo (`nexo ctl …`), como `orca-ide` en Orca: con ella el agente de
//! Nexo (la skill `nexo-orquestacion`) ve los workspaces, abre terminales, lanza Claude Code
//! con un encargo, lee lo que hizo, espera a que acabe y mira cuánto gasta; y los hooks de las
//! terminales cuentan lo que hace cada Claude (`nexo ctl hook`, ver `sesiones.rs`).
//!
//! - nexo escucha en un socket Unix de su carpeta de ejecución (`terminales::run_dir`, 0700):
//!   `ctl-<pid>.sock` y, apuntando al de la última apertura, `ctl.sock`. Las terminales llevan
//!   el suyo en `NEXO_CTL`; el agente de Nexo usa `ctl.sock`.
//! - Una petición JSON por línea y una respuesta JSON por línea (`{ ok, data | error }`). Lo
//!   que hace cada orden está en `handle`, sobre el rasgo `Nexo`: las pruebas lo llaman con un
//!   nexo de mentira, sin socket ni interfaz.
//! - El cliente es el propio ejecutable de nexo: `main` mira los argumentos antes de crear la
//!   ventana. Si nexo no está abierto lo dice y sale con 2; nunca lo abre.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use crate::claude_code::{EFFORTS, PERMISSION_MODES, valid_model};
use crate::sesiones::{Hook, SesionView};
use crate::terminales::{Kind, Launch, TermInfo, Terminales, quote};

/// Lo que la orden necesita de nexo (las pruebas lo sustituyen).
pub(crate) trait Nexo: Send + Sync {
    /// Los workspaces del panel: nombre, ruta y si es el actual.
    fn workspaces(&self) -> Vec<(String, PathBuf, bool)>;
    fn terminals(&self, root: Option<&Path>) -> Vec<TermInfo>;
    fn open(&self, root: &Path, launch: Launch) -> Result<TermInfo, String>;
    fn write(&self, id: u64, data: &str) -> Result<(), String>;
    fn screen(&self, id: u64, lines: usize) -> Result<String, String>;
    fn close(&self, id: u64) -> Result<(), String>;
    /// El registro de agentes (`registro::agent_log`) como JSON.
    fn usage(&self) -> Result<Value, String>;
    /// La definición de un agente del vault para `--agents`: `(nombre, definición)`.
    fn vault_agent(&self, name: &str) -> Option<(String, Value)>;
    /// Dónde dejar el archivo de `--agents` de una terminal nueva.
    fn agents_file(&self, root: &Path, slot: &str) -> PathBuf;
    fn hook(&self, hook: Hook);
}

fn ok(data: Value) -> Value {
    json!({ "ok": true, "data": data })
}

fn err(text: impl Into<String>) -> Value {
    json!({ "ok": false, "error": text.into() })
}

fn str_of<'a>(req: &'a Value, key: &str) -> Option<&'a str> {
    req[key].as_str().map(str::trim).filter(|s| !s.is_empty())
}

/// El workspace por su nombre o su ruta, solo entre los del panel.
fn resolve(nexo: &dyn Nexo, which: &str) -> Result<PathBuf, String> {
    let list = nexo.workspaces();
    let found = list
        .iter()
        .find(|(_, path, _)| path.to_string_lossy() == which.trim_end_matches('/'))
        .or_else(|| {
            let by_name: Vec<_> = list.iter().filter(|(name, ..)| name == which).collect();
            (by_name.len() == 1).then(|| by_name[0])
        });
    found.map(|(_, path, _)| path.clone()).ok_or_else(|| {
        let names: Vec<&str> = list.iter().map(|(n, ..)| n.as_str()).collect();
        format!(
            "No hay ningún workspace «{which}» en el panel de nexo. Están: {}.",
            if names.is_empty() {
                "ninguno".to_string()
            } else {
                names.join(", ")
            }
        )
    })
}

fn terminal_id(req: &Value) -> Result<u64, String> {
    match &req["terminal"] {
        Value::Number(n) => n.as_u64().ok_or_else(|| "Terminal no válida.".to_string()),
        Value::String(s) => s
            .trim()
            .parse()
            .map_err(|_| "Terminal no válida.".to_string()),
        _ => Err("Falta la terminal (su número, de `nexo ctl terminales`).".into()),
    }
}

fn find(nexo: &dyn Nexo, id: u64) -> Result<TermInfo, String> {
    nexo.terminals(None)
        .into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| format!("No hay ninguna terminal {id}."))
}

/// La línea con la que el shell de una terminal lanza Claude Code para un encargo. Todo lo
/// que viene de fuera va entre comillas; el modelo, el esfuerzo y el modo de permisos se
/// validan, y nunca se salta los permisos.
pub(crate) fn claude_line(
    req: &Value,
    session: &str,
    agents: Option<&Path>,
) -> Result<String, String> {
    let mut line = format!("claude --session-id {session}");
    if let Some(name) = str_of(req, "nombre") {
        line.push_str(&format!(
            " --name {}",
            quote(&format!("{} · nexo", crate::terminales::clean_name(name)))
        ));
    }
    if let (Some(agent), Some(file)) = (str_of(req, "agente"), agents) {
        line.push_str(&format!(
            " --agents {} --agent {}",
            quote(&file.to_string_lossy()),
            quote(agent)
        ));
    }
    if let Some(model) = str_of(req, "modelo") {
        if !valid_model(model) {
            return Err(format!("Modelo no válido: {model}"));
        }
        line.push_str(&format!(" --model {}", quote(model)));
    }
    if let Some(effort) = str_of(req, "esfuerzo") {
        if !EFFORTS.contains(&effort) {
            return Err(format!(
                "Esfuerzo no válido: {effort} (vale {}).",
                EFFORTS.join(", ")
            ));
        }
        line.push_str(&format!(" --effort {effort}"));
    }
    if let Some(mode) = str_of(req, "permisos") {
        if !PERMISSION_MODES.contains(&mode) {
            return Err(format!(
                "Modo de permisos no válido: {mode} (vale {}; nunca uno que se salte los permisos).",
                PERMISSION_MODES.join(", ")
            ));
        }
        line.push_str(&format!(" --permission-mode {mode}"));
    }
    if let Some(task) = req["encargo"].as_str().filter(|t| !t.trim().is_empty()) {
        // Un encargo que empieza por «-» se leería como una opción.
        let task = if task.starts_with('-') {
            format!(" {task}")
        } else {
            task.to_string()
        };
        line.push(' ');
        line.push_str(&quote(&task));
    }
    Ok(line)
}

/// El estado de una terminal para `esperar`: `fin` si terminó su programa; si no, el de su
/// Claude (`inactivo`, `espera`, `activo`) o `inactivo` si no corre ninguno.
fn wait_state(t: &TermInfo) -> &'static str {
    if t.exited {
        return "fin";
    }
    match t.claude.as_ref() {
        Some(SesionView {
            state: "waiting", ..
        }) => "espera",
        Some(SesionView {
            state: "active", ..
        }) => "activo",
        Some(c) if c.subagents.iter().any(|s| s.state == "active") => "activo",
        _ => "inactivo",
    }
}

/// Los últimos mensajes de una sesión de Claude Code (solo texto, sin razonamiento ni
/// herramientas): `(quién, texto)`.
pub(crate) fn last_messages(transcript: &Path, n: usize) -> Vec<(String, String)> {
    let Ok(mut file) = std::fs::File::open(transcript) else {
        return Vec::new();
    };
    // Basta con el final: las transcripciones solo crecen.
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let from = len.saturating_sub(4 * 1024 * 1024);
    let mut text = String::new();
    {
        use std::io::Seek;
        let _ = file.seek(std::io::SeekFrom::Start(from));
        let mut bytes = Vec::new();
        let _ = file.read_to_end(&mut bytes);
        text.push_str(&String::from_utf8_lossy(&bytes));
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v["isSidechain"] == true || v["isMeta"] == true {
            continue;
        }
        let who = match v["type"].as_str() {
            Some("assistant") => "Claude",
            Some("user") => "Usuario",
            _ => continue,
        };
        let content = &v["message"]["content"];
        let said = match content {
            Value::String(s) => s.clone(),
            Value::Array(blocks) => blocks
                .iter()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };
        let said = said.trim();
        if said.is_empty() || said.starts_with('<') {
            continue;
        }
        // Una respuesta llega en varias líneas con el mismo mensaje: se juntan.
        match out.last_mut() {
            Some((w, t)) if w == who && who == "Claude" => {
                t.push('\n');
                t.push_str(said);
            }
            _ => out.push((who.to_string(), said.to_string())),
        }
    }
    let skip = out.len().saturating_sub(n);
    out.split_off(skip)
}

/// Lo que hace cada orden. `esperar` bloquea su conexión hasta que pasa lo que espera.
pub(crate) fn handle(nexo: &dyn Nexo, req: &Value) -> Value {
    let cmd = req["cmd"].as_str().unwrap_or_default();
    let result: Result<Value, String> = (|| match cmd {
        "workspaces" => Ok(Value::Array(
            nexo.workspaces()
                .into_iter()
                .map(|(name, path, current)| {
                    let terms = nexo.terminals(Some(&path)).len();
                    json!({ "nombre": name, "ruta": path, "actual": current, "terminales": terms })
                })
                .collect(),
        )),
        "terminales" | "estado" => {
            let root = str_of(req, "workspace")
                .map(|w| resolve(nexo, w))
                .transpose()?;
            Ok(serde_json::to_value(nexo.terminals(root.as_deref())).map_err(|e| e.to_string())?)
        }
        "abrir-terminal" => {
            let root = resolve(nexo, str_of(req, "workspace").ok_or("Falta el workspace.")?)?;
            let launch = Launch {
                kind: Some(Kind::Shell),
                name: str_of(req, "nombre").map(String::from),
                start: str_of(req, "orden").map(String::from),
                ..Launch::default()
            };
            Ok(serde_json::to_value(nexo.open(&root, launch)?).map_err(|e| e.to_string())?)
        }
        "claude" => {
            let root = resolve(nexo, str_of(req, "workspace").ok_or("Falta el workspace.")?)?;
            let session = uuid::Uuid::new_v4().to_string();
            let slot = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
            let mut agents = None;
            if let Some(agent) = str_of(req, "agente") {
                let (name, def) = nexo.vault_agent(agent).ok_or_else(|| {
                    format!("No hay ningún agente «{agent}» en el vault (.claude/agents/).")
                })?;
                let file = nexo.agents_file(&root, &slot);
                let text = serde_json::to_string_pretty(&json!({ name: def }))
                    .map_err(|e| e.to_string())?;
                crate::workspace::write_private(&file, text.as_bytes())?;
                agents = Some(file);
            }
            let line = claude_line(req, &session, agents.as_deref())?;
            let launch = Launch {
                kind: Some(Kind::Claude),
                name: str_of(req, "nombre").map(String::from),
                start: Some(line),
                session: Some(session.clone()),
                agent: str_of(req, "agente").map(String::from),
                slot: Some(slot),
                ..Launch::default()
            };
            let term = nexo.open(&root, launch)?;
            Ok(
                json!({ "terminal": term.id, "sesion": session, "nombre": term.name, "workspace": term.path }),
            )
        }
        "escribir" => {
            let id = terminal_id(req)?;
            let term = find(nexo, id)?;
            if term.claude.as_ref().is_some_and(|c| {
                c.state == "waiting" || c.subagents.iter().any(|s| s.state == "waiting")
            }) {
                return Err(
                    "Su Claude espera una respuesta del usuario (un permiso, una pregunta o un plan): esa respuesta solo la da el usuario."
                        .into(),
                );
            }
            let mut text = req["texto"].as_str().unwrap_or_default().to_string();
            if req["enter"] == true {
                text.push('\r');
            }
            nexo.write(id, &text)?;
            Ok(json!({ "terminal": id }))
        }
        "leer" => {
            let id = terminal_id(req)?;
            let term = find(nexo, id)?;
            let n = req["lineas"].as_u64().unwrap_or(40).clamp(1, 2000) as usize;
            let messages = term
                .claude
                .as_ref()
                .and_then(|c| c.transcript.as_deref())
                .map(|t| last_messages(t, n.min(20)))
                .unwrap_or_default();
            Ok(json!({
                "terminal": id,
                "nombre": term.name,
                "estado": wait_state(&term),
                "mensajes": messages.into_iter().map(|(who, text)| json!({ "de": who, "texto": text })).collect::<Vec<_>>(),
                "pantalla": nexo.screen(id, n)?,
            }))
        }
        "esperar" => {
            let id = terminal_id(req)?;
            let until = str_of(req, "hasta").unwrap_or("inactivo");
            if !["inactivo", "espera", "fin"].contains(&until) {
                return Err("`hasta` vale inactivo, espera o fin.".into());
            }
            let max = Duration::from_secs(req["max"].as_u64().unwrap_or(600).clamp(1, 3600));
            let start = Instant::now();
            // Primero que empiece: recién lanzado, su Claude aún no ha dicho nada.
            let grace = Duration::from_secs(20);
            loop {
                let term = find(nexo, id)?;
                let state = wait_state(&term);
                let started = term.claude.is_some() || term.exited;
                let reached = match until {
                    "fin" => state == "fin",
                    "espera" => state == "espera" || state == "fin",
                    _ => state != "activo" && (started || start.elapsed() > grace),
                };
                if reached {
                    return Ok(
                        json!({ "terminal": id, "estado": state, "segundos": start.elapsed().as_secs() }),
                    );
                }
                if start.elapsed() > max {
                    return Ok(
                        json!({ "terminal": id, "estado": state, "agotado": true, "segundos": start.elapsed().as_secs() }),
                    );
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
        "gasto" => {
            let mut log = nexo.usage()?;
            if let Some(w) = str_of(req, "workspace") {
                let root = resolve(nexo, w)?;
                let name = nexo
                    .workspaces()
                    .into_iter()
                    .find(|(_, p, _)| *p == root)
                    .map(|(n, ..)| n)
                    .unwrap_or_default();
                let places = log["places"].clone();
                if let Some(agents) = log["agents"].as_array_mut() {
                    agents.retain(|a| {
                        a["id"]
                            .as_str()
                            .is_some_and(|id| places[id].as_str() == Some(name.as_str()))
                    });
                }
            }
            Ok(log)
        }
        "cerrar" => {
            let id = terminal_id(req)?;
            find(nexo, id)?;
            nexo.close(id)?;
            Ok(json!({ "terminal": id, "cerrada": true }))
        }
        "hook" => {
            let h: Hook = serde_json::from_value(req["hook"].clone()).map_err(|e| e.to_string())?;
            nexo.hook(h);
            Ok(Value::Null)
        }
        "" => Err("Falta la orden.".into()),
        other => Err(format!(
            "Orden desconocida: {other}. Prueba `nexo ctl ayuda`."
        )),
    })();
    match result {
        Ok(data) => ok(data),
        Err(e) => err(e),
    }
}

/// Un evento de hook con solo lo que nexo necesita: nunca la entrada de la herramienta, el
/// texto del prompt ni lo último que dijo el modelo.
pub(crate) fn hook_from(raw: &Value, terminal: u64) -> Hook {
    let s = |key: &str| {
        raw[key]
            .as_str()
            .map(String::from)
            .filter(|v| !v.is_empty())
    };
    Hook {
        event: s("hook_event_name").unwrap_or_default(),
        terminal,
        session: s("session_id"),
        transcript: s("transcript_path"),
        tool: s("tool_name"),
        agent: s("agent_id"),
        agent_type: s("agent_type"),
        source: s("source"),
        reason: s("reason"),
    }
}

// ---------------------------------------------------------------- en la app

pub(crate) fn socket_paths(run: &Path) -> (PathBuf, PathBuf) {
    (
        run.join(format!("ctl-{}.sock", std::process::id())),
        run.join("ctl.sock"),
    )
}

/// El nexo de verdad.
struct AppNexo(AppHandle);

impl Nexo for AppNexo {
    fn workspaces(&self) -> Vec<(String, PathBuf, bool)> {
        crate::workspace::panel(&self.0)
    }
    fn terminals(&self, root: Option<&Path>) -> Vec<TermInfo> {
        self.0
            .state::<Terminales>()
            .of(root)
            .iter()
            .map(|t| crate::terminales::info(&self.0, t))
            .collect()
    }
    fn open(&self, root: &Path, launch: Launch) -> Result<TermInfo, String> {
        let terms = crate::terminales::app_terms(&self.0)?;
        let term = self
            .0
            .state::<Terminales>()
            .open(&terms.ctx(), root, launch)?;
        Ok(crate::terminales::info(&self.0, &term))
    }
    fn write(&self, id: u64, data: &str) -> Result<(), String> {
        self.0
            .state::<Terminales>()
            .get(id)
            .ok_or("Esa terminal ya no existe.")?
            .write(data.as_bytes())
    }
    fn screen(&self, id: u64, lines: usize) -> Result<String, String> {
        Ok(self
            .0
            .state::<Terminales>()
            .get(id)
            .ok_or("Esa terminal ya no existe.")?
            .screen_tail(lines))
    }
    fn close(&self, id: u64) -> Result<(), String> {
        let terms = crate::terminales::app_terms(&self.0)?;
        self.0.state::<Terminales>().close(&terms.ctx(), id)
    }
    fn usage(&self) -> Result<Value, String> {
        let log =
            tauri::async_runtime::block_on(crate::registro::agent_log(self.0.clone(), false))?;
        serde_json::to_value(log).map_err(|e| e.to_string())
    }
    fn vault_agent(&self, name: &str) -> Option<(String, Value)> {
        let root = crate::vault::vault_root(&self.0).ok()?;
        crate::agents::definition(&root, name)
    }
    fn agents_file(&self, root: &Path, slot: &str) -> PathBuf {
        crate::terminales::app_terms(&self.0)
            .map(|t| {
                t.store
                    .join(crate::registro::project_dir_name(root))
                    .join(format!("{slot}.agents.json"))
            })
            .unwrap_or_else(|_| std::env::temp_dir().join(format!("nexo-{slot}.agents.json")))
    }
    fn hook(&self, hook: Hook) {
        crate::sesiones::hook(&self.0, hook);
    }
}

fn serve(nexo: Arc<dyn Nexo>, stream: UnixStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut line = String::new();
    // Una petición no pasa de 4 MB (un encargo largo cabe de sobra).
    if reader
        .by_ref()
        .take(4 * 1024 * 1024)
        .read_line(&mut line)
        .is_err()
    {
        return;
    }
    let response = match serde_json::from_str::<Value>(&line) {
        Ok(req) => handle(nexo.as_ref(), &req),
        Err(e) => err(format!("Petición no válida: {e}")),
    };
    let mut stream = stream;
    let _ = stream.write_all(format!("{response}\n").as_bytes());
}

/// Arranca el servidor de control (en `lib.rs`, al abrir nexo).
pub(crate) fn listen(app: AppHandle, run: &Path) -> Result<PathBuf, String> {
    let (own, shared) = socket_paths(run);
    let _ = std::fs::remove_file(&own);
    let listener = UnixListener::bind(&own).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&own, std::fs::Permissions::from_mode(0o600));
    }
    // `ctl.sock` apunta a esta apertura (el agente de Nexo no sabe cuál es la suya).
    let tmp = run.join(format!("ctl-{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    #[cfg(unix)]
    if std::os::unix::fs::symlink(&own, &tmp).is_ok() {
        let _ = std::fs::rename(&tmp, &shared);
    }
    let nexo: Arc<dyn Nexo> = Arc::new(AppNexo(app));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let nexo = nexo.clone();
            std::thread::spawn(move || serve(nexo, stream));
        }
    });
    Ok(own)
}

/// Al cerrar nexo: quita su socket y `ctl.sock` si apuntaba a él.
pub(crate) fn unlisten(run: &Path) {
    let (own, shared) = socket_paths(run);
    if std::fs::read_link(&shared).ok().as_deref() == Some(own.as_path()) {
        let _ = std::fs::remove_file(&shared);
    }
    let _ = std::fs::remove_file(&own);
}

// ---------------------------------------------------------------- cliente (`nexo ctl`)

const HELP: &str = "nexo ctl: habla con el nexo abierto.

  nexo ctl workspaces
  nexo ctl terminales [workspace]
  nexo ctl estado [workspace]
  nexo ctl abrir-terminal <workspace> [--nombre N] [--orden \"…\"]
  nexo ctl claude <workspace> [--agente A] [--modelo M] [--esfuerzo E] [--permisos P] [--nombre N] [\"encargo\" | -]
  nexo ctl escribir <terminal> \"texto\" [--enter]
  nexo ctl leer <terminal> [--lineas N]
  nexo ctl esperar <terminal> [--hasta inactivo|espera|fin] [--max segundos]
  nexo ctl gasto [workspace]
  nexo ctl cerrar <terminal>

El workspace va por su nombre (o su ruta); la terminal, por su número. La respuesta es JSON.
Con «-» el encargo se lee de la entrada.";

/// Convierte los argumentos en una petición. `stdin` da el encargo con «-».
pub(crate) fn request_of(args: &[String], stdin: impl FnOnce() -> String) -> Result<Value, String> {
    let (cmd, rest) = args.split_first().ok_or(HELP)?;
    let mut positional: Vec<&str> = Vec::new();
    let mut flags = serde_json::Map::new();
    let mut i = 0;
    while i < rest.len() {
        let a = rest[i].as_str();
        if let Some(name) = a.strip_prefix("--") {
            if name == "enter" {
                flags.insert("enter".into(), true.into());
            } else {
                let value = rest
                    .get(i + 1)
                    .ok_or_else(|| format!("Falta el valor de --{name}."))?;
                let value = match name {
                    "lineas" | "max" => json!(
                        value
                            .parse::<u64>()
                            .map_err(|_| format!("--{name} es un número."))?
                    ),
                    _ => json!(value),
                };
                flags.insert(name.into(), value);
                i += 1;
            }
        } else {
            positional.push(a);
        }
        i += 1;
    }
    let mut req = json!({ "cmd": cmd });
    for (k, v) in flags {
        req[k] = v;
    }
    let need = |n: usize, what: &str| -> Result<(), String> {
        if positional.len() < n {
            Err(format!("Falta {what}. Prueba `nexo ctl ayuda`."))
        } else {
            Ok(())
        }
    };
    match cmd.as_str() {
        "terminales" | "estado" | "gasto" => {
            if let Some(w) = positional.first() {
                req["workspace"] = json!(w);
            }
        }
        "abrir-terminal" => {
            need(1, "el workspace")?;
            req["workspace"] = json!(positional[0]);
        }
        "claude" => {
            need(1, "el workspace")?;
            req["workspace"] = json!(positional[0]);
            if let Some(task) = positional.get(1) {
                req["encargo"] = json!(if *task == "-" {
                    stdin()
                } else {
                    task.to_string()
                });
            }
        }
        "escribir" => {
            need(2, "la terminal y el texto")?;
            req["terminal"] = json!(positional[0]);
            req["texto"] = json!(positional[1]);
        }
        "leer" | "esperar" | "cerrar" => {
            need(1, "la terminal")?;
            req["terminal"] = json!(positional[0]);
        }
        "workspaces" => {}
        "ayuda" | "help" | "-h" | "--help" => return Err(HELP.into()),
        other => return Err(format!("Orden desconocida: {other}.\n\n{HELP}")),
    }
    Ok(req)
}

fn send(socket: &Path, req: &Value, timeout: Option<Duration>) -> Result<Value, std::io::Error> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(timeout)?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(format!("{req}\n").as_bytes())?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    serde_json::from_str(&line).map_err(std::io::Error::other)
}

fn socket_for_client() -> PathBuf {
    std::env::var_os("NEXO_CTL")
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .unwrap_or_else(|| socket_paths(&crate::terminales::run_path()).1)
}

/// `nexo ctl hook`: lo que llega de un hook de Claude Code. Nunca falla ni escribe nada (un
/// hook que escribe o sale con error podría cambiar lo que hace Claude Code).
fn client_hook() -> i32 {
    let Some(terminal) = std::env::var("NEXO_TERMINAL")
        .ok()
        .and_then(|t| t.parse::<u64>().ok())
    else {
        return 0;
    };
    let mut input = String::new();
    let _ = std::io::stdin()
        .take(4 * 1024 * 1024)
        .read_to_string(&mut input);
    let Ok(raw) = serde_json::from_str::<Value>(&input) else {
        return 0;
    };
    let hook = hook_from(&raw, terminal);
    if hook.event.is_empty() {
        return 0;
    }
    let req = json!({ "cmd": "hook", "hook": hook });
    let _ = send(&socket_for_client(), &req, Some(Duration::from_secs(2)));
    0
}

/// Si nexo se ejecutó como `nexo ctl …`, hace la orden y devuelve con qué código salir.
pub fn cli() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("ctl") {
        return None;
    }
    let rest = &args[1..];
    if rest.first().map(String::as_str) == Some("hook") {
        return Some(client_hook());
    }
    let req = match request_of(rest, || {
        let mut s = String::new();
        let _ = std::io::stdin().read_to_string(&mut s);
        s
    }) {
        Ok(r) => r,
        Err(text) => {
            let help = text == HELP;
            if help {
                println!("{text}");
            } else {
                eprintln!("{text}");
            }
            return Some(if help { 0 } else { 1 });
        }
    };
    let timeout = if req["cmd"] == "esperar" {
        None
    } else {
        Some(Duration::from_secs(60))
    };
    match send(&socket_for_client(), &req, timeout) {
        Ok(response) if response["ok"] == true => {
            println!(
                "{}",
                serde_json::to_string_pretty(&response["data"]).unwrap_or_default()
            );
            Some(0)
        }
        Ok(response) => {
            eprintln!(
                "{}",
                response["error"].as_str().unwrap_or("Error desconocido.")
            );
            Some(1)
        }
        Err(_) => {
            eprintln!("nexo no está abierto (o no contesta). Ábrelo y vuelve a intentarlo.");
            Some(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Un nexo de mentira: dos workspaces y lo que se le pide, apuntado.
    #[derive(Default)]
    struct Falso {
        terms: Mutex<Vec<TermInfo>>,
        writes: Mutex<Vec<(u64, String)>>,
        opened: Mutex<Vec<Launch>>,
        hooks: Mutex<Vec<Hook>>,
        closed: Mutex<Vec<u64>>,
    }

    fn term(id: u64, path: &str, claude: Option<SesionView>) -> TermInfo {
        TermInfo {
            id,
            slot: format!("slot{id}"),
            path: path.into(),
            name: format!("Terminal {id}"),
            kind: Kind::Shell,
            exited: false,
            code: None,
            claude,
        }
    }

    fn session(state: &'static str) -> SesionView {
        SesionView {
            session: "s1".into(),
            name: "Claude Code".into(),
            state,
            tool: (state == "waiting").then(|| "Bash".into()),
            subagents: Vec::new(),
            transcript: None,
        }
    }

    impl Nexo for Falso {
        fn workspaces(&self) -> Vec<(String, PathBuf, bool)> {
            vec![
                ("api".into(), PathBuf::from("/p/api"), true),
                ("web".into(), PathBuf::from("/p/web"), false),
            ]
        }
        fn terminals(&self, root: Option<&Path>) -> Vec<TermInfo> {
            self.terms
                .lock()
                .unwrap()
                .iter()
                .filter(|t| root.is_none_or(|r| Path::new(&t.path) == r))
                .cloned()
                .collect()
        }
        fn open(&self, root: &Path, launch: Launch) -> Result<TermInfo, String> {
            self.opened.lock().unwrap().push(launch);
            let t = term(9, &root.to_string_lossy(), None);
            self.terms.lock().unwrap().push(t.clone());
            Ok(t)
        }
        fn write(&self, id: u64, data: &str) -> Result<(), String> {
            self.writes.lock().unwrap().push((id, data.into()));
            Ok(())
        }
        fn screen(&self, _: u64, _: usize) -> Result<String, String> {
            Ok("$ ls\nsrc".into())
        }
        fn close(&self, id: u64) -> Result<(), String> {
            self.closed.lock().unwrap().push(id);
            Ok(())
        }
        fn usage(&self) -> Result<Value, String> {
            Ok(
                json!({ "agents": [{ "id": "s:1" }, { "id": "s:2" }], "places": { "s:1": "api", "s:2": "web" } }),
            )
        }
        fn vault_agent(&self, name: &str) -> Option<(String, Value)> {
            (name == "revisor").then(|| {
                (
                    "revisor".into(),
                    json!({ "description": "Revisa", "prompt": "Eres un revisor." }),
                )
            })
        }
        fn agents_file(&self, _: &Path, slot: &str) -> PathBuf {
            std::env::temp_dir().join(format!("nexo-control-{}-{slot}.json", std::process::id()))
        }
        fn hook(&self, hook: Hook) {
            self.hooks.lock().unwrap().push(hook);
        }
    }

    fn call(nexo: &Falso, req: Value) -> Value {
        handle(nexo, &req)
    }

    #[test]
    fn un_workspace_que_no_esta_en_el_panel_se_rechaza() {
        let nexo = Falso::default();
        for which in ["otro", "/etc", "/p/api/../../etc"] {
            let r = call(
                &nexo,
                json!({ "cmd": "abrir-terminal", "workspace": which }),
            );
            assert_eq!(r["ok"], false, "{which}");
            assert!(r["error"].as_str().unwrap().contains("api, web"), "{r}");
        }
        assert!(nexo.opened.lock().unwrap().is_empty());
        let r = call(
            &nexo,
            json!({ "cmd": "abrir-terminal", "workspace": "/p/web/", "orden": "npm test" }),
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            nexo.opened.lock().unwrap()[0].start.as_deref(),
            Some("npm test")
        );
    }

    #[test]
    fn no_se_escribe_en_una_terminal_cuyo_claude_espera_al_usuario() {
        let nexo = Falso::default();
        nexo.terms
            .lock()
            .unwrap()
            .push(term(1, "/p/api", Some(session("waiting"))));
        let r = call(
            &nexo,
            json!({ "cmd": "escribir", "terminal": 1, "texto": "1", "enter": true }),
        );
        assert_eq!(r["ok"], false);
        assert!(
            r["error"]
                .as_str()
                .unwrap()
                .contains("solo la da el usuario")
        );
        assert!(nexo.writes.lock().unwrap().is_empty());
        nexo.terms.lock().unwrap()[0].claude = Some(session("idle"));
        let r = call(
            &nexo,
            json!({ "cmd": "escribir", "terminal": "1", "texto": "sigue", "enter": true }),
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(nexo.writes.lock().unwrap()[0], (1, "sigue\r".to_string()));
    }

    #[test]
    fn claude_se_lanza_con_su_encargo_y_nunca_saltandose_los_permisos() {
        let nexo = Falso::default();
        let r = call(
            &nexo,
            json!({ "cmd": "claude", "workspace": "api", "agente": "revisor", "modelo": "sonnet", "esfuerzo": "low",
                    "nombre": "Revisión", "encargo": "Revisa src/ y di qué falla; no toques 'main'" }),
        );
        assert_eq!(r["ok"], true, "{r}");
        let launch = nexo.opened.lock().unwrap()[0].clone();
        let line = launch.start.unwrap();
        let session = r["data"]["sesion"].as_str().unwrap();
        assert!(
            line.starts_with(&format!(
                "claude --session-id {session} --name 'Revisión · nexo' --agents '"
            )),
            "{line}"
        );
        assert!(
            line.contains(" --agent 'revisor' --model 'sonnet' --effort low "),
            "{line}"
        );
        assert!(
            line.ends_with(r"'Revisa src/ y di qué falla; no toques '\''main'\'''"),
            "{line}"
        );
        assert_eq!(launch.session.as_deref(), Some(session));
        assert_eq!(launch.kind, Some(Kind::Claude));
        for (key, value) in [
            ("permisos", "bypassPermissions"),
            ("permisos", "dontAsk"),
            ("esfuerzo", "todo"),
            ("modelo", "--dangerously"),
        ] {
            let r = call(
                &nexo,
                json!({ "cmd": "claude", "workspace": "api", key: value }),
            );
            assert_eq!(r["ok"], false, "{key}={value}");
        }
        let r = call(
            &nexo,
            json!({ "cmd": "claude", "workspace": "api", "agente": "no-existe" }),
        );
        assert!(r["error"].as_str().unwrap().contains(".claude/agents"));
        // Un encargo que empieza por «-» no se lee como una opción.
        let line = claude_line(&json!({ "encargo": "-rf todo" }), "s", None).unwrap();
        assert!(line.ends_with("' -rf todo'"), "{line}");
    }

    #[test]
    fn leer_y_esperar_dicen_el_estado_de_su_claude() {
        let nexo = Falso::default();
        nexo.terms
            .lock()
            .unwrap()
            .push(term(1, "/p/api", Some(session("active"))));
        let r = call(&nexo, json!({ "cmd": "leer", "terminal": 1 }));
        assert_eq!(r["data"]["estado"], "activo");
        assert_eq!(r["data"]["pantalla"], "$ ls\nsrc");
        let r = call(
            &nexo,
            json!({ "cmd": "esperar", "terminal": 1, "hasta": "inactivo", "max": 1 }),
        );
        assert_eq!(
            r["data"]["agotado"], true,
            "sigue activo: se agota la espera"
        );
        nexo.terms.lock().unwrap()[0].claude = Some(session("waiting"));
        let r = call(
            &nexo,
            json!({ "cmd": "esperar", "terminal": 1, "hasta": "espera", "max": 5 }),
        );
        assert_eq!(r["data"]["estado"], "espera");
        nexo.terms.lock().unwrap()[0].exited = true;
        let r = call(
            &nexo,
            json!({ "cmd": "esperar", "terminal": 1, "hasta": "fin" }),
        );
        assert_eq!(r["data"]["estado"], "fin");
        let r = call(
            &nexo,
            json!({ "cmd": "esperar", "terminal": 1, "hasta": "nunca" }),
        );
        assert_eq!(r["ok"], false);
    }

    #[test]
    fn el_gasto_se_filtra_por_workspace_y_cerrar_cierra() {
        let nexo = Falso::default();
        let r = call(&nexo, json!({ "cmd": "gasto", "workspace": "web" }));
        assert_eq!(r["data"]["agents"], json!([{ "id": "s:2" }]));
        let r = call(&nexo, json!({ "cmd": "gasto" }));
        assert_eq!(r["data"]["agents"].as_array().unwrap().len(), 2);
        nexo.terms.lock().unwrap().push(term(3, "/p/web", None));
        assert_eq!(
            call(&nexo, json!({ "cmd": "cerrar", "terminal": 3 }))["ok"],
            true
        );
        assert_eq!(*nexo.closed.lock().unwrap(), [3]);
        assert_eq!(
            call(&nexo, json!({ "cmd": "cerrar", "terminal": 4 }))["ok"],
            false
        );
    }

    #[test]
    fn un_hook_llega_sin_lo_que_no_debe_salir_de_claude_code() {
        let nexo = Falso::default();
        let raw = json!({ "hook_event_name": "UserPromptSubmit", "session_id": "s1", "prompt": "mi clave es sk-123",
                          "transcript_path": "/home/x/.claude/projects/p/s1.jsonl" });
        let hook = hook_from(&raw, 7);
        let r = call(&nexo, json!({ "cmd": "hook", "hook": hook }));
        assert_eq!(r["ok"], true);
        let got = nexo.hooks.lock().unwrap()[0].clone();
        assert_eq!(got.terminal, 7);
        assert_eq!(got.event, "UserPromptSubmit");
        assert!(!serde_json::to_string(&got).unwrap().contains("sk-123"));
    }

    #[test]
    fn los_argumentos_de_la_orden() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let r = request_of(
            &args(&["claude", "api", "--agente", "revisor", "-"]),
            || "encargo largo".into(),
        )
        .unwrap();
        assert_eq!(
            r,
            json!({ "cmd": "claude", "workspace": "api", "agente": "revisor", "encargo": "encargo largo" })
        );
        let r = request_of(&args(&["escribir", "3", "hola", "--enter"]), String::new).unwrap();
        assert_eq!(
            r,
            json!({ "cmd": "escribir", "terminal": "3", "texto": "hola", "enter": true })
        );
        let r = request_of(&args(&["leer", "3", "--lineas", "80"]), String::new).unwrap();
        assert_eq!(r["lineas"], 80);
        assert!(request_of(&args(&["leer"]), String::new).is_err());
        assert!(request_of(&args(&["borrar-todo"]), String::new).is_err());
        assert_eq!(
            request_of(&args(&["ayuda"]), String::new).unwrap_err(),
            HELP
        );
    }

    #[test]
    fn el_socket_lleva_una_peticion_y_trae_su_respuesta() {
        let dir = std::env::temp_dir().join(format!("nexo-control-socket-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("ctl.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let nexo: Arc<dyn Nexo> = Arc::new(Falso::default());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten().take(2) {
                serve(nexo.clone(), stream);
            }
        });
        let r = send(
            &socket,
            &json!({ "cmd": "workspaces" }),
            Some(Duration::from_secs(5)),
        )
        .unwrap();
        assert_eq!(r["ok"], true);
        assert_eq!(r["data"][1]["nombre"], "web");
        let r = send(
            &socket,
            &json!({ "cmd": "borrar" }),
            Some(Duration::from_secs(5)),
        )
        .unwrap();
        assert_eq!(r["ok"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn los_ultimos_mensajes_de_una_sesion() {
        let dir = std::env::temp_dir().join(format!("nexo-control-msgs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.jsonl");
        let lines = [
            json!({ "type": "user", "message": { "role": "user", "content": "Arregla el test" } }),
            json!({ "type": "user", "isMeta": true, "message": { "content": "<system-reminder>x</system-reminder>" } }),
            json!({ "type": "assistant", "message": { "content": [{ "type": "thinking", "thinking": "pienso" }, { "type": "text", "text": "Miro el test." }] } }),
            json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "name": "Bash", "input": {} }] } }),
            json!({ "type": "assistant", "isSidechain": true, "message": { "content": [{ "type": "text", "text": "de un subagente" }] } }),
            json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "Arreglado." }] } }),
        ];
        std::fs::write(
            &path,
            lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
        )
        .unwrap();
        let msgs = last_messages(&path, 5);
        assert_eq!(
            msgs,
            [
                ("Usuario".to_string(), "Arregla el test".to_string()),
                (
                    "Claude".to_string(),
                    "Miro el test.\nArreglado.".to_string()
                )
            ]
        );
        assert_eq!(last_messages(&path, 1).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
