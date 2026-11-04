//! Lo que hace el Claude Code de cada terminal de un workspace, contado por sus hooks
//! (`docs/spec-terminales-workspaces.md`): el `claude` de las terminales de nexo
//! (`terminales.rs`) le añade a la sesión unos hooks que ejecutan `nexo ctl hook`, y la orden
//! de control (`control.rs`) trae aquí cada evento, sin la entrada de la herramienta ni el
//! texto del prompt.
//!
//! - Una sesión está **activa** desde el prompt hasta el fin de turno (o mientras corre alguno
//!   de sus subagentes), **esperando respuesta** con un permiso, una pregunta o un plan sin
//!   contestar, e **inactiva** si no. Sus subagentes acaban como se decidió el 20-10-2026:
//!   terminado, falló, detenido o sin terminar (nunca se da por terminado lo que no se sabe).
//! - Cada workspace junta la de sus terminales en el evento global `workspace-attention`
//!   (`{ path, waiting, busy }`), el mismo de antes, y lo que pide o termina avisa al
//!   escritorio (`avisos.rs`).
//! - Si `disableAllHooks` apaga los hooks, aquí no llega nada: la terminal sigue siendo una
//!   terminal y el registro de agentes sigue contando su gasto por las transcripciones.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::avisos::{self, Mirando, Suceso};
use crate::terminales::{Term, Terminales, app_terms};
use crate::workspace::ws_root;

/// Un evento de hook, tal como lo manda `nexo ctl hook` (solo estos campos).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Hook {
    pub event: String,
    pub terminal: u64,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub transcript: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub agent_type: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Cómo acabó cada subagente: su id → (estado de Claude Code, resumen).
type Endings = HashMap<String, (String, String)>;

#[derive(Debug, Clone, PartialEq)]
struct Sub {
    id: String,
    kind: String,
    active: bool,
    /// Terminó con su `SubagentStop` (si no, lo cortó el final de la sesión).
    stopped: bool,
}

#[derive(Debug, Clone)]
struct Sesion {
    root: PathBuf,
    session: String,
    transcript: Option<PathBuf>,
    name: String,
    turn: bool,
    error: bool,
    /// Lo que espera respuesta: `""` es la sesión; si no, el id del subagente. Con su herramienta.
    pending: BTreeMap<String, String>,
    subs: Vec<Sub>,
    ended: bool,
    /// Cuándo empezó el turno o la petición que sigue abierta (ms): un fin de turno anterior a
    /// esto en la transcripción no la cierra.
    since: i64,
    /// Finales de los subagentes leídos de la transcripción (con su fecha, para no releerla).
    ends: Option<(SystemTime, Endings)>,
}

impl Sesion {
    fn attention(&self) -> Attention {
        if self.ended {
            return Attention::default();
        }
        Attention {
            waiting: self.pending.len(),
            busy: self.turn || self.subs.iter().any(|s| s.active),
        }
    }
}

/// Atención de un workspace: cuántas peticiones esperan respuesta y si hay trabajo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Attention {
    pub waiting: usize,
    pub busy: bool,
}

impl Attention {
    pub fn free(&self) -> bool {
        !self.busy && self.waiting == 0
    }
}

/// Lo que se emite en `workspace-attention` (y devuelve `workspace_attention`).
#[derive(Debug, Clone, Serialize)]
pub struct WsAttention {
    path: String,
    waiting: usize,
    busy: bool,
}

#[derive(Default)]
pub(crate) struct Estado {
    by_term: HashMap<u64, Sesion>,
    told: HashMap<PathBuf, Attention>,
}

#[derive(Default)]
pub struct Sesiones {
    estado: Mutex<Estado>,
    /// Qué workspace tiene la interfaz en pantalla y si la ventana tiene el foco.
    watch: Mutex<Mirando>,
}

/// El Claude de una terminal, para la interfaz y la orden de control.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct SesionView {
    pub session: String,
    pub name: String,
    /// `active`, `waiting` o `idle`.
    pub state: &'static str,
    /// La herramienta que espera respuesta.
    pub tool: Option<String>,
    pub subagents: Vec<SubView>,
    #[serde(skip)]
    pub transcript: Option<PathBuf>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct SubView {
    pub id: String,
    pub kind: String,
    /// `active`, `waiting`, `done`, `failed`, `stopped` o `unknown` (los de `STATE_LABEL` de `workspace.js`).
    pub state: String,
}

// ---------------------------------------------------------------- finales de los subagentes

/// Lo que hay entre `<nombre>` y `</nombre>` en un texto.
fn tag<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&format!("</{name}>"))? + start;
    Some(text[start..end].trim())
}

/// Los `<task-notification>` de una parte de una línea (tarea, estado y resumen). Según la
/// versión de Claude Code van en un mensaje del usuario (`message`), en una línea
/// `queue-operation` (`content`) o en un `attachment`, a veces repetidos; los resultados de
/// herramientas no cuentan (pueden ser un archivo que habla de ellos).
fn notifications(v: &Value, out: &mut Vec<(String, String, String)>) {
    match v {
        Value::String(s) if s.contains("<task-notification>") => {
            for block in s.split("<task-notification>").skip(1) {
                let block = block.split("</task-notification>").next().unwrap_or(block);
                if let (Some(task), Some(status)) = (tag(block, "task-id"), tag(block, "status")) {
                    let summary = tag(block, "summary").unwrap_or_default();
                    out.push((task.into(), status.into(), summary.into()));
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|i| notifications(i, out)),
        Value::Object(map) if map.get("type").and_then(Value::as_str) != Some("tool_result") => {
            map.values().for_each(|i| notifications(i, out))
        }
        _ => {}
    }
}

/// Cómo acabó cada subagente según la transcripción de su sesión: manda el último
/// `<task-notification>` de su tarea. Un SendMessage que lo retoma borra lo de antes hasta el
/// aviso siguiente (si no se pudo retomar, vuelve lo de antes).
pub(crate) fn endings(path: &Path, tasks: &[&str]) -> Endings {
    let mut ends: HashMap<String, Option<(String, String)>> = HashMap::new();
    let mut resumes: HashMap<String, (String, Option<(String, String)>)> = HashMap::new();
    let Ok(file) = fs::File::open(path) else {
        return HashMap::new();
    };
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { break };
        if !line.contains("<task-notification>")
            && !line.contains(r#""SendMessage""#)
            && !line.contains(r#""tool_result""#)
        {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let kind = v["type"].as_str().unwrap_or_default();
        if kind != "assistant" {
            let mut found = Vec::new();
            for part in ["message", "content", "attachment"] {
                notifications(&v[part], &mut found);
            }
            for (task, status, summary) in found {
                if tasks.contains(&task.as_str()) {
                    ends.insert(task, Some((status, summary)));
                }
            }
        }
        let Some(blocks) = v["message"]["content"].as_array() else {
            continue;
        };
        for block in blocks {
            match (kind, block["type"].as_str()) {
                ("assistant", Some("tool_use")) if block["name"] == "SendMessage" => {
                    let to = block["input"]["to"]
                        .as_str()
                        .or(block["input"]["recipient"].as_str());
                    if let (Some(to), Some(id)) = (to, block["id"].as_str())
                        && tasks.contains(&to)
                    {
                        let before = ends.insert(to.to_string(), None).flatten();
                        resumes.insert(id.to_string(), (to.to_string(), before));
                    }
                }
                ("user", Some("tool_result")) => {
                    let id = block["tool_use_id"].as_str().unwrap_or_default();
                    if let Some((task, before)) = resumes.remove(id)
                        && block["is_error"] == true
                    {
                        ends.insert(task, before);
                    }
                }
                _ => {}
            }
        }
    }
    ends.into_iter()
        .filter_map(|(task, end)| Some((task, end?)))
        .collect()
}

/// Un `stopped` cuyo resumen dice que lo cortó el final de la sesión anterior: sin terminar.
const CUT: &str = "didn't finish before the previous session ended";

/// El estado de la interfaz para el final que cuenta Claude Code.
pub(crate) fn end_state(status: &str, summary: &str) -> &'static str {
    match status {
        "completed" => "done",
        "failed" => "failed",
        "stopped" | "killed" if summary.contains(CUT) => "unknown",
        "stopped" | "killed" => "stopped",
        _ => "unknown",
    }
}

fn sub_views(s: &mut Sesion) -> Vec<SubView> {
    let ended: Vec<String> = s
        .subs
        .iter()
        .filter(|x| !x.active)
        .map(|x| x.id.clone())
        .collect();
    if !ended.is_empty()
        && let Some(path) = s.transcript.clone()
    {
        let modified = fs::metadata(&path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if s.ends.as_ref().is_none_or(|(at, _)| *at != modified) {
            let tasks: Vec<&str> = ended.iter().map(String::as_str).collect();
            s.ends = Some((modified, endings(&path, &tasks)));
        }
    }
    let ends = s.ends.as_ref().map(|(_, e)| e);
    s.subs
        .iter()
        .map(|x| {
            let state = if x.active {
                if s.pending.contains_key(&x.id) {
                    "waiting"
                } else {
                    "active"
                }
            } else if let Some((status, summary)) = ends.and_then(|e| e.get(&x.id)) {
                end_state(status, summary)
            } else if x.stopped {
                // Su `SubagentStop`: terminó de trabajar y no hay aviso de que fallara.
                "done"
            } else {
                "unknown"
            };
            SubView {
                id: x.id.clone(),
                kind: x.kind.clone(),
                state: state.to_string(),
            }
        })
        .collect()
}

fn view(s: &mut Sesion) -> SesionView {
    let subagents = sub_views(s);
    let main = s.pending.get("").cloned();
    let state = if main.is_some() {
        "waiting"
    } else if s.turn {
        "active"
    } else {
        "idle"
    };
    SesionView {
        session: s.session.clone(),
        name: s.name.clone(),
        state,
        tool: main.or_else(|| s.pending.values().next().cloned()),
        subagents,
        transcript: s.transcript.clone(),
    }
}

// ---------------------------------------------------------------- eventos

/// Quién pide: el nombre de la sesión o, si es un subagente, su tipo.
fn who(s: &Sesion, agent: Option<&str>) -> String {
    match agent {
        None | Some("") => s.name.clone(),
        Some(id) => s
            .subs
            .iter()
            .find(|x| x.id == id)
            .map(|x| x.kind.clone())
            .filter(|k| !k.is_empty())
            .unwrap_or_else(|| "Un agente".into()),
    }
}

/// Aplica un evento de la terminal (`root` y `name` son los suyos). Devuelve lo que merece un
/// aviso y si cambió la sesión que corre en ella (`Some(None)`: ya no corre ninguna).
pub(crate) fn apply(
    e: &mut Estado,
    root: &Path,
    name: &str,
    h: &Hook,
) -> (Vec<Suceso>, Option<Option<String>>) {
    let mut sucesos = Vec::new();
    let Some(session) = h.session.clone().filter(|s| !s.is_empty()) else {
        return (sucesos, None);
    };
    let transcript = h.transcript.as_deref().map(PathBuf::from);
    let fresh = |e: &Estado| Sesion {
        root: root.to_path_buf(),
        session: session.clone(),
        transcript: transcript.clone(),
        // Un /clear en la misma terminal sigue llamándose igual.
        name: e
            .by_term
            .get(&h.terminal)
            .map_or_else(|| name.to_string(), |s| s.name.clone()),
        turn: false,
        error: false,
        pending: BTreeMap::new(),
        subs: Vec::new(),
        ended: false,
        since: 0,
        ends: None,
    };
    let mut changed = None;
    let current = e.by_term.get(&h.terminal);
    let same = current.is_some_and(|s| s.session == session && !s.ended);
    if h.event == "SessionStart" {
        if !same {
            let s = fresh(e);
            e.by_term.insert(h.terminal, s);
            changed = Some(Some(session.clone()));
        }
        return (sucesos, changed);
    }
    if !same {
        // Un evento de una sesión que no se vio empezar (nexo se abrió con ella en marcha):
        // empieza a contar desde aquí. Uno de una sesión anterior que ya acabó no cuenta.
        if current.is_some_and(|s| s.session == session) {
            return (sucesos, None);
        }
        let s = fresh(e);
        e.by_term.insert(h.terminal, s);
        changed = Some(Some(session.clone()));
    }
    let s = e.by_term.get_mut(&h.terminal).expect("recién insertada");
    if s.transcript.is_none() {
        s.transcript = transcript;
    }
    let before = s.attention();
    let key = h.agent.clone().unwrap_or_default();
    match h.event.as_str() {
        "UserPromptSubmit" => {
            s.turn = true;
            s.error = false;
            s.pending.remove("");
            s.since = now_ms();
        }
        "PermissionRequest" | "PreToolUse" => {
            let tool = h.tool.clone().unwrap_or_else(|| "una herramienta".into());
            s.since = s.since.max(now_ms());
            if s.pending.get(&key) != Some(&tool) {
                s.pending.insert(key.clone(), tool.clone());
                sucesos.push(Suceso::Pide {
                    who: who(s, h.agent.as_deref()),
                    tool,
                });
            }
        }
        "PostToolUse" | "PostToolUseFailure" | "PermissionDenied" => {
            s.pending.remove(&key);
        }
        "Stop" | "StopFailure" => {
            s.turn = false;
            s.error = h.event == "StopFailure";
            s.pending.remove("");
        }
        "SubagentStart" => {
            if let Some(id) = h.agent.clone().filter(|a| !a.is_empty()) {
                let kind = h.agent_type.clone().unwrap_or_default();
                s.subs.retain(|x| x.id != id);
                s.subs.push(Sub {
                    id,
                    kind,
                    active: true,
                    stopped: false,
                });
            }
        }
        "SubagentStop" => {
            if let Some(sub) = s.subs.iter_mut().find(|x| Some(&x.id) == h.agent.as_ref()) {
                sub.active = false;
                sub.stopped = true;
            }
            s.pending.remove(&key);
        }
        "SessionEnd" => {
            s.ended = true;
            s.turn = false;
            s.pending.clear();
            for sub in &mut s.subs {
                sub.active = false;
            }
            changed = Some(None);
        }
        _ => {}
    }
    let after = s.attention();
    if before.busy && after.free() && !s.ended {
        sucesos.push(Suceso::Termino { error: s.error });
    }
    (sucesos, changed)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Si el último turno de una transcripción terminó después de `since`: su última línea de
/// conversación es el `system`/`turn_duration` que Claude Code escribe al acabar cada turno.
/// Hace falta porque un turno interrumpido (Escape, rechazar un permiso con Escape) no dispara
/// `Stop` y la sesión se quedaría «activa» o «esperando».
pub(crate) fn turn_over(transcript: &Path, since: i64) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = fs::File::open(transcript) else {
        return false;
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = file.seek(SeekFrom::Start(len.saturating_sub(256 * 1024)));
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return false;
    }
    let text = String::from_utf8_lossy(&bytes);
    for line in text.lines().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match v["type"].as_str() {
            Some("system") if v["subtype"] == "turn_duration" => {
                let at = v["timestamp"]
                    .as_str()
                    .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                    .map(|d| d.timestamp_millis())
                    .unwrap_or(0);
                return at >= since;
            }
            Some("user" | "assistant") => return false,
            _ => {}
        }
    }
    false
}

/// Una sesión cuyo turno terminó sin `Stop` (interrumpido) deja de estar activa y de esperar.
/// Devuelve si cambió algo.
fn settle_interrupted(s: &mut Sesion) -> bool {
    if s.ended || !(s.turn || s.pending.contains_key("")) {
        return false;
    }
    let Some(path) = s.transcript.clone() else {
        return false;
    };
    if !turn_over(&path, s.since) {
        return false;
    }
    s.turn = false;
    s.pending.remove("");
    true
}

/// La atención de un workspace: la suma de la de sus terminales.
fn attention_of(e: &Estado, root: &Path) -> Attention {
    e.by_term
        .values()
        .filter(|s| s.root == root)
        .map(Sesion::attention)
        .fold(Attention::default(), |acc, a| Attention {
            waiting: acc.waiting + a.waiting,
            busy: acc.busy || a.busy,
        })
}

/// Si cambió la atención de `root` desde lo último que se contó, la nueva.
pub(crate) fn check(e: &mut Estado, root: &Path) -> Option<Attention> {
    let now = attention_of(e, root);
    let told = e.told.get(root).copied().unwrap_or_default();
    if now == told {
        return None;
    }
    e.told.insert(root.to_path_buf(), now);
    Some(now)
}

/// La terminal ya no tiene a su Claude (terminó su programa o se cerró). Si nexo no la
/// cerró y estaba trabajando o esperando, es una caída.
pub(crate) fn gone(e: &mut Estado, terminal: u64, closing: bool) -> Vec<Suceso> {
    let Some(s) = e.by_term.remove(&terminal) else {
        return Vec::new();
    };
    let a = s.attention();
    if !closing && !s.ended && !a.free() {
        vec![Suceso::Cayo]
    } else {
        Vec::new()
    }
}

// ---------------------------------------------------------------- en la app

fn tell(app: &AppHandle, root: &Path, a: Attention) {
    let event = WsAttention {
        path: root.display().to_string(),
        waiting: a.waiting,
        busy: a.busy,
    };
    if let Err(e) = app.emit("workspace-attention", event) {
        log::warn!("No se pudo emitir workspace-attention: {e}");
    }
}

/// La marca de «esta terminal espera una respuesta»: con ella, los hooks que llegan con cada
/// herramienta llaman a nexo (ver `terminales::hooks_settings`).
fn mark_waiting(app: &AppHandle, terminal: u64, on: bool) {
    let Ok(terms) = app_terms(app) else {
        return;
    };
    let dir = terms.files.run.join("esperando");
    let file = dir.join(terminal.to_string());
    if on {
        let _ = fs::create_dir_all(&dir);
        let _ = fs::write(&file, b"");
    } else {
        let _ = fs::remove_file(&file);
    }
}

fn after(app: &AppHandle, term: &Term, sucesos: Vec<Suceso>, attention: Option<Attention>) {
    if let Some(a) = attention {
        tell(app, &term.root, a);
    }
    if !sucesos.is_empty() {
        let mirando = app
            .state::<Sesiones>()
            .watch
            .lock()
            .map(|m| m.clone())
            .unwrap_or_default();
        let escritorio = avisos::Escritorio(app.clone());
        for suceso in &sucesos {
            avisos::avisar(&escritorio, &mirando, &term.root, suceso);
        }
    }
    let _ = app.emit(
        "workspace-terminals",
        serde_json::json!({ "path": term.root.display().to_string() }),
    );
}

/// Un evento de hook (de `nexo ctl hook`).
pub(crate) fn hook(app: &AppHandle, h: Hook) {
    let Some(term) = app.state::<Terminales>().get(h.terminal) else {
        return;
    };
    let state = app.state::<Sesiones>();
    let (sucesos, changed, attention, waiting) = {
        let Ok(mut e) = state.estado.lock() else {
            return;
        };
        let (sucesos, changed) = apply(&mut e, &term.root, &term.name(), &h);
        let attention = check(&mut e, &term.root);
        let waiting = e
            .by_term
            .get(&h.terminal)
            .is_some_and(|s| !s.pending.is_empty() && !s.ended);
        (sucesos, changed, attention, waiting)
    };
    mark_waiting(app, h.terminal, waiting);
    if let Some(session) = changed
        && term.set_session(session)
        && let Ok(terms) = app_terms(app)
    {
        app.state::<Terminales>()
            .save_list(&terms.store, &term.root);
    }
    after(app, &term, sucesos, attention);
}

/// Terminó el programa de una terminal (el shell): su Claude, si quedaba, se va con él.
pub(crate) fn terminal_exited(app: &AppHandle, term: &Term) {
    let state = app.state::<Sesiones>();
    let (sucesos, attention) = {
        let Ok(mut e) = state.estado.lock() else {
            return;
        };
        let sucesos = gone(&mut e, term.id, term.closing());
        (sucesos, check(&mut e, &term.root))
    };
    mark_waiting(app, term.id, false);
    after(app, term, sucesos, attention);
}

/// Cada pocos segundos: un Claude que dice estar trabajando pero ya no corre en su terminal
/// (se cerró sin `SessionEnd`, p. ej. al caerse) deja de contar, y si trabajaba se avisa; uno
/// cuyo turno se interrumpió (sin `Stop`) deja de estar activo o esperando, sin avisar.
pub(crate) fn sweep(app: &AppHandle) {
    let state = app.state::<Sesiones>();
    let terms = app.state::<Terminales>();
    let settled: Vec<(u64, PathBuf, Option<Attention>, bool)> = state
        .estado
        .lock()
        .map(|mut e| {
            let ids: Vec<u64> = e.by_term.keys().copied().collect();
            let mut out = Vec::new();
            for id in ids {
                let Some(s) = e.by_term.get_mut(&id) else {
                    continue;
                };
                if settle_interrupted(s) {
                    let root = s.root.clone();
                    let waiting = !s.pending.is_empty();
                    let a = check(&mut e, &root);
                    out.push((id, root, a, waiting));
                }
            }
            out
        })
        .unwrap_or_default();
    for (id, root, attention, waiting) in settled {
        mark_waiting(app, id, waiting);
        if let Some(a) = attention {
            tell(app, &root, a);
        }
        let _ = app.emit(
            "workspace-terminals",
            serde_json::json!({ "path": root.display().to_string() }),
        );
    }
    let ids: Vec<u64> = state
        .estado
        .lock()
        .map(|e| {
            e.by_term
                .iter()
                .filter(|(_, s)| !s.ended)
                .map(|(id, _)| *id)
                .collect()
        })
        .unwrap_or_default();
    for id in ids {
        let Some(term) = terms.get(id) else {
            continue;
        };
        if term.busy() {
            continue;
        }
        let (sucesos, attention) = {
            let Ok(mut e) = state.estado.lock() else {
                return;
            };
            let sucesos = gone(&mut e, id, term.closing());
            (sucesos, check(&mut e, &term.root))
        };
        mark_waiting(app, id, false);
        if term.set_session(None)
            && let Ok(t) = app_terms(app)
        {
            terms.save_list(&t.store, &term.root);
        }
        after(app, &term, sucesos, attention);
    }
}

/// El Claude de una terminal, si corre uno.
pub(crate) fn view_of(app: &AppHandle, terminal: u64) -> Option<SesionView> {
    let state = app.try_state::<Sesiones>()?;
    let mut e = state.estado.lock().ok()?;
    let s = e.by_term.get_mut(&terminal).filter(|s| !s.ended)?;
    Some(view(s))
}

/// El estado de atención de todos los workspaces con algo en marcha, para la interfaz recién
/// cargada (también tras una recarga de Vite); después se entera por `workspace-attention`.
#[tauri::command]
pub fn workspace_attention(state: State<'_, Sesiones>) -> Result<Vec<WsAttention>, String> {
    let e = state.estado.lock().map_err(|e| e.to_string())?;
    let mut roots: Vec<&PathBuf> = e.by_term.values().map(|s| &s.root).collect();
    roots.sort();
    roots.dedup();
    Ok(roots
        .into_iter()
        .map(|root| {
            let a = attention_of(&e, root);
            WsAttention {
                path: root.display().to_string(),
                waiting: a.waiting,
                busy: a.busy,
            }
        })
        .filter(|a| a.busy || a.waiting > 0)
        .collect())
}

/// Qué workspace tiene la interfaz en pantalla (`None` fuera de un workspace) y si la ventana
/// tiene el foco: de ese workspace no se avisa mientras se mira.
#[tauri::command]
pub fn workspace_watch(
    app: AppHandle,
    state: State<'_, Sesiones>,
    path: Option<String>,
    focused: bool,
) -> Result<(), String> {
    let path = path.map(|p| ws_root(&app, &p)).transpose()?;
    *state.watch.lock().map_err(|e| e.to_string())? = Mirando { path, focused };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(event: &str) -> Hook {
        Hook {
            event: event.into(),
            terminal: 1,
            session: Some("s1".into()),
            transcript: None,
            ..Hook::default()
        }
    }

    fn with(mut h: Hook, f: impl FnOnce(&mut Hook)) -> Hook {
        f(&mut h);
        h
    }

    fn run(e: &mut Estado, hooks: &[Hook]) -> Vec<Suceso> {
        let root = Path::new("/p/proyecto");
        hooks
            .iter()
            .flat_map(|h| apply(e, root, "Claude Code", h).0)
            .collect()
    }

    #[test]
    fn un_turno_activa_y_al_acabar_avisa_una_vez() {
        let mut e = Estado::default();
        let root = Path::new("/p/proyecto");
        assert!(run(&mut e, &[ev("SessionStart")]).is_empty());
        assert_eq!(check(&mut e, root), None, "recién abierta no cambia nada");
        run(&mut e, &[ev("UserPromptSubmit")]);
        assert_eq!(
            check(&mut e, root),
            Some(Attention {
                waiting: 0,
                busy: true
            })
        );
        assert_eq!(check(&mut e, root), None, "solo se cuenta cuando cambia");
        let done = run(&mut e, &[ev("Stop")]);
        assert_eq!(done, [Suceso::Termino { error: false }]);
        assert_eq!(check(&mut e, root), Some(Attention::default()));
        // Un Stop sin turno no vuelve a avisar.
        assert!(run(&mut e, &[ev("Stop")]).is_empty());
        let failed = run(&mut e, &[ev("UserPromptSubmit"), ev("StopFailure")]);
        assert_eq!(failed, [Suceso::Termino { error: true }]);
    }

    #[test]
    fn un_permiso_espera_hasta_que_se_usa_la_herramienta_y_dice_quien() {
        let mut e = Estado::default();
        let root = Path::new("/p/proyecto");
        run(&mut e, &[ev("SessionStart"), ev("UserPromptSubmit")]);
        let pide = run(
            &mut e,
            &[with(ev("PermissionRequest"), |h| {
                h.tool = Some("Bash".into())
            })],
        );
        assert_eq!(
            pide,
            [Suceso::Pide {
                who: "Claude Code".into(),
                tool: "Bash".into()
            }]
        );
        assert_eq!(
            check(&mut e, root),
            Some(Attention {
                waiting: 1,
                busy: true
            })
        );
        assert_eq!(view(e.by_term.get_mut(&1).unwrap()).state, "waiting");
        // El mismo permiso repetido (PreToolUse y PermissionRequest de una pregunta) no avisa dos veces.
        assert!(
            run(
                &mut e,
                &[with(ev("PermissionRequest"), |h| h.tool = Some("Bash".into()))]
            )
            .is_empty()
        );
        run(&mut e, &[ev("PostToolUse")]);
        assert_eq!(
            check(&mut e, root),
            Some(Attention {
                waiting: 0,
                busy: true
            })
        );
        // Un subagente que pide permiso: se le nombra por su tipo.
        run(
            &mut e,
            &[with(ev("SubagentStart"), |h| {
                h.agent = Some("a1".into());
                h.agent_type = Some("revisor".into());
            })],
        );
        let pide = run(
            &mut e,
            &[with(ev("PermissionRequest"), |h| {
                h.agent = Some("a1".into());
                h.tool = Some("Edit".into());
            })],
        );
        assert_eq!(
            pide,
            [Suceso::Pide {
                who: "revisor".into(),
                tool: "Edit".into()
            }]
        );
        let v = view(e.by_term.get_mut(&1).unwrap());
        assert_eq!(v.state, "active", "la sesión sigue en su turno");
        assert_eq!(v.subagents[0].state, "waiting");
        assert_eq!(v.tool.as_deref(), Some("Edit"));
        run(
            &mut e,
            &[
                with(ev("PermissionDenied"), |h| h.agent = Some("a1".into())),
                with(ev("SubagentStop"), |h| h.agent = Some("a1".into())),
            ],
        );
        // Sin aviso de fallo, el SubagentStop es que terminó.
        assert_eq!(
            view(e.by_term.get_mut(&1).unwrap()).subagents[0].state,
            "done"
        );
        // Mientras un subagente trabaja, el workspace sigue ocupado aunque acabe el turno.
        run(
            &mut e,
            &[with(ev("SubagentStart"), |h| {
                h.agent = Some("a2".into());
                h.agent_type = Some("tester".into());
            })],
        );
        assert!(run(&mut e, &[ev("Stop")]).is_empty());
        let done = run(
            &mut e,
            &[with(ev("SubagentStop"), |h| h.agent = Some("a2".into()))],
        );
        assert_eq!(done, [Suceso::Termino { error: false }]);
    }

    #[test]
    fn el_aviso_no_lleva_la_entrada_de_la_herramienta() {
        // `Hook` no tiene dónde llevarla: `nexo ctl hook` solo saca estos campos.
        let raw = json!({ "hook_event_name": "PermissionRequest", "session_id": "s1",
            "tool_name": "Bash", "tool_input": { "command": "curl -H 'Authorization: Bearer sk-secreta' x" } });
        let h = crate::control::hook_from(&raw, 1);
        let text = serde_json::to_string(&h).unwrap();
        for secreto in ["curl", "sk-secreta", "Authorization"] {
            assert!(!text.contains(secreto), "{text}");
        }
        assert_eq!(h.tool.as_deref(), Some("Bash"));
    }

    #[test]
    fn una_sesion_nueva_en_la_terminal_sustituye_a_la_anterior() {
        let mut e = Estado::default();
        let root = Path::new("/p/proyecto");
        let (_, changed) = apply(&mut e, root, "Claude Code", &ev("SessionStart"));
        assert_eq!(changed, Some(Some("s1".into())));
        run(&mut e, &[ev("UserPromptSubmit")]);
        let (_, changed) = apply(&mut e, root, "Claude Code", &ev("SessionEnd"));
        assert_eq!(
            changed,
            Some(None),
            "al salir de Claude ya no corre ninguna"
        );
        assert_eq!(check(&mut e, root), None, "nunca se contó como ocupada");
        // Lo que llegue tarde de la sesión que acabó no la revive.
        run(&mut e, &[ev("UserPromptSubmit")]);
        assert!(e.by_term[&1].ended);
        let next = with(ev("SessionStart"), |h| h.session = Some("s2".into()));
        let (_, changed) = apply(&mut e, root, "Claude Code", &next);
        assert_eq!(changed, Some(Some("s2".into())));
        assert!(!e.by_term[&1].ended);
    }

    #[test]
    fn si_la_terminal_se_va_con_trabajo_es_una_caida_salvo_si_la_cerro_nexo() {
        let mut e = Estado::default();
        run(&mut e, &[ev("SessionStart"), ev("UserPromptSubmit")]);
        assert_eq!(gone(&mut e, 1, false), [Suceso::Cayo]);
        run(&mut e, &[ev("SessionStart"), ev("UserPromptSubmit")]);
        assert!(gone(&mut e, 1, true).is_empty(), "la cerró nexo");
        run(&mut e, &[ev("SessionStart")]);
        assert!(gone(&mut e, 1, false).is_empty(), "libre no es una caída");
    }

    #[test]
    fn un_turno_interrumpido_deja_de_estar_activo_por_la_transcripcion() {
        let dir = std::env::temp_dir().join(format!("nexo-sesiones-corte-{}", std::process::id()));
        let path = dir.join("s1.jsonl");
        let mut e = Estado::default();
        let start = with(ev("SessionStart"), |h| {
            h.transcript = Some(path.to_string_lossy().into_owned())
        });
        run(&mut e, &[start, ev("UserPromptSubmit")]);
        run(
            &mut e,
            &[with(ev("PermissionRequest"), |h| {
                h.tool = Some("Write".into())
            })],
        );
        let s = e.by_term.get_mut(&1).unwrap();
        let since = s.since;
        let at = |ms: i64| {
            chrono::DateTime::from_timestamp_millis(ms)
                .unwrap()
                .to_rfc3339()
        };
        // Un fin de turno de antes no la cierra.
        write(
            &path,
            &[
                json!({ "type": "system", "subtype": "turn_duration", "timestamp": at(since - 60_000) }),
                json!({ "type": "user", "message": { "content": "Crea adios.txt" } }),
                json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "name": "Write" }] } }),
            ],
        );
        assert!(!settle_interrupted(s));
        // Rechazado con Escape: el error, la interrupción y el fin del turno, sin Stop.
        write(
            &path,
            &[
                json!({ "type": "user", "message": { "content": "Crea adios.txt" } }),
                json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "name": "Write" }] } }),
                json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "is_error": true, "content": "The user doesn't want to proceed" }] } }),
                json!({ "type": "user", "message": { "content": [{ "type": "text", "text": "[Request interrupted by user for tool use]" }] } }),
                json!({ "type": "system", "subtype": "turn_duration", "timestamp": at(since + 2_000) }),
            ],
        );
        assert!(settle_interrupted(s));
        assert_eq!(view(s).state, "idle");
        assert!(!settle_interrupted(s), "una vez");
        let _ = fs::remove_dir_all(&dir);
    }

    fn write(path: &Path, lines: &[Value]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            lines
                .iter()
                .map(|l| l.to_string() + "\n")
                .collect::<String>(),
        )
        .unwrap();
    }

    #[test]
    fn el_final_de_cada_subagente_sale_de_la_transcripcion() {
        let dir =
            std::env::temp_dir().join(format!("nexo-sesiones-finales-{}", std::process::id()));
        let path = dir.join("s1.jsonl");
        let note = |task: &str, status: &str, summary: &str| {
            format!(
                "<task-notification>\n<task-id>{task}</task-id>\n<status>{status}</status>\n<summary>{summary}</summary>\n</task-notification>"
            )
        };
        let send = |id: &str, to: &str| json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": id, "name": "SendMessage", "input": { "to": to } }] } });
        let result = |id: &str, text: &str, error: bool| json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": id, "content": text, "is_error": error }] } });
        write(
            &path,
            &[
                // a1: falló; el aviso, en una línea `queue-operation`.
                json!({ "type": "queue-operation", "content": note("a1", "failed", "Agent \"Revisor\" failed: API Error") }),
                // a3: terminó y se retomó sin aviso después.
                json!({ "type": "user", "message": { "role": "user", "content": note("a3", "completed", "") } }),
                send("toolu_s3", "a3"),
                result("toolu_s3", "{\"success\":true}", false),
                // a4: detenido; el aviso, en un `attachment`, repetido.
                json!({ "type": "attachment", "attachment": { "type": "queued_command", "prompt": format!("{}{}", note("a4", "killed", "stopped"), note("a4", "killed", "stopped")) } }),
                // a5: un archivo que habla de uno no cuenta.
                result("toolu_r", &note("a5", "completed", ""), false),
                // a6: terminó; el SendMessage que no pudo retomarlo no cambia nada.
                json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "text", "text": note("a6", "completed", "") }] } }),
                send("toolu_s6", "a6"),
                result("toolu_s6", "Agent not found", true),
                // a7: lo cortó el final de la sesión anterior.
                json!({ "type": "user", "message": { "role": "user", "content": note("a7", "stopped", "Agent \"X\" didn't finish before the previous session ended") } }),
            ],
        );
        let ends = endings(&path, &["a1", "a3", "a4", "a5", "a6", "a7"]);
        let state = |task: &str| {
            ends.get(task)
                .map(|(status, summary)| end_state(status, summary))
                .unwrap_or("sin aviso")
        };
        assert_eq!(state("a1"), "failed");
        assert_eq!(ends["a1"].1, "Agent \"Revisor\" failed: API Error");
        assert_eq!(state("a3"), "sin aviso", "retomado: lo de antes ya no vale");
        assert_eq!(state("a4"), "stopped");
        assert_eq!(state("a5"), "sin aviso");
        assert_eq!(state("a6"), "done");
        assert_eq!(state("a7"), "unknown");
        // En la vista: el que falló sale como fallido aunque llegara su SubagentStop, y el
        // que cortó el final de la sesión, sin terminar.
        let mut e = Estado::default();
        let start = with(ev("SessionStart"), |h| {
            h.transcript = Some(path.to_string_lossy().into_owned())
        });
        run(&mut e, &[start]);
        for id in ["a1", "a8"] {
            run(
                &mut e,
                &[with(ev("SubagentStart"), |h| {
                    h.agent = Some(id.into());
                    h.agent_type = Some("general-purpose".into());
                })],
            );
        }
        run(
            &mut e,
            &[with(ev("SubagentStop"), |h| h.agent = Some("a1".into()))],
        );
        run(&mut e, &[ev("SessionEnd")]);
        let s = e.by_term.get_mut(&1).unwrap();
        let subs = sub_views(s);
        assert_eq!(subs[0].state, "failed");
        assert_eq!(
            subs[1].state, "unknown",
            "sin SubagentStop ni aviso: no se sabe"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
