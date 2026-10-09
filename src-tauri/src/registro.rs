//! Registro de agentes: cuánto gastó cada agente de Claude Code del vault (el orquestador de
//! cada sesión y sus subagentes) en la sesión de 5 h actual del plan de claude.ai, y su
//! contexto, como `/context`.
//!
//! - Tokens: de las transcripciones de Claude Code (`~/.claude/projects/<carpeta>/<sesión>.jsonl`
//!   y `<sesión>/subagents/agent-*.jsonl`, con su `.meta.json`). Cada respuesta del modelo se
//!   escribe en varias líneas (una por bloque) con el mismo `message.id` y se cuenta una vez.
//!   Los subagentes guardan el `output_tokens` del inicio del streaming, que se queda corto:
//!   si lo que escribieron da más (~2,5 caracteres por token), se usa eso.
//! - Coste: los precios por modelo del catálogo de Claude Code (con los que calcula `/cost`).
//! - % de la sesión de 5 h: `/api/oauth/usage` solo da el total de la cuenta, así que se
//!   reparte según el coste de cada agente frente al de todo Claude Code de este equipo en la
//!   ventana (de cualquier carpeta). Lo gastado desde otros equipos o claude.ai no aparece en
//!   las transcripciones y queda repartido también: por eso es aproximado.
//! - Contexto: `claude -p --input-format stream-json` con la petición de control
//!   `get_context_usage` (la de `/context`). No llama al modelo ni gasta tokens, y con
//!   `--fork-session --no-session-persistence` no toca la sesión original.
//!
//! Las transcripciones solo crecen: cada archivo se sigue leyendo desde donde se quedó.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use memchr::memmem;
use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Manager, State};

use crate::vault::vault_root;

/// La sesión de uso del plan dura 5 h desde el primer mensaje.
const WINDOW_MS: i64 = 5 * 60 * 60 * 1000;
/// Un subagente que no terminó pero lleva esto sin escribir se da por abandonado.
const STALE_MS: i64 = 15 * 60 * 1000;
/// Una sesión que no es la de esta apertura de nexo cuenta como activa si escribió hace menos.
const RECENT_MS: i64 = 90 * 1000;
/// Caracteres por token de salida (medido en las sesiones de nexo: texto en español,
/// Markdown y entradas JSON de herramientas).
const CHARS_PER_TOKEN: f64 = 2.5;

fn millis(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn now_ms() -> i64 {
    millis(SystemTime::now())
}

fn parse_ts(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|d| d.timestamp_millis())
}

/// Carpeta de `~/.claude/projects` de una ruta: Claude Code cambia por `-` todo lo que no
/// sea letra o número ASCII (`/home/ana/mi_vault` → `-home-ana-mi-vault`).
fn project_dir_name(root: &Path) -> String {
    root.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

// ---------------------------------------------------------------- precios

/// Dólares por millón de tokens, como en el catálogo de modelos de Claude Code.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Price {
    input: f64,
    output: f64,
    write_5m: f64,
    write_1h: f64,
    read: f64,
}

const fn price_of(input: f64, output: f64, write_5m: f64, write_1h: f64, read: f64) -> Price {
    Price {
        input,
        output,
        write_5m,
        write_1h,
        read,
    }
}

/// `claude-opus-4-1-20250805` → `claude-opus-4-1`; `claude-sonnet-5-5[1m]` → `claude-sonnet-5-5`.
fn base_model(model: &str) -> &str {
    let model = model.strip_suffix("[1m]").unwrap_or(model);
    match model.rsplit_once('-') {
        Some((base, date)) if date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => model,
    }
}

fn price(model: &str, fast: bool, prompt: u64) -> Price {
    let model = base_model(model);
    if fast {
        match model {
            "claude-opus-5-5" => return price_of(8.0, 40.0, 10.0, 16.0, 0.4),
            "claude-opus-4-8" | "claude-opus-5" => return price_of(10.0, 50.0, 12.5, 20.0, 1.0),
            "claude-opus-4-6" | "claude-opus-4-7" => {
                return price_of(30.0, 150.0, 37.5, 60.0, 3.0);
            }
            _ => {}
        }
    }
    match model {
        // Haiku 5.5 cobra más por encima de 100 000 tokens de prompt.
        "claude-haiku-5-5" if prompt > 100_000 => price_of(0.5, 2.5, 0.625, 1.0, 0.05),
        "claude-haiku-5-5" => price_of(0.1, 0.5, 0.125, 0.2, 0.01),
        "claude-haiku-4-5" => price_of(1.0, 5.0, 1.25, 2.0, 0.1),
        "claude-3-5-haiku" => price_of(0.8, 4.0, 1.0, 1.6, 0.08),
        "claude-sonnet-5" | "claude-sonnet-5-5" => price_of(2.0, 10.0, 2.5, 4.0, 0.2),
        m if m.starts_with("claude-sonnet-4")
            || (m.starts_with("claude-3") && m.ends_with("sonnet")) =>
        {
            price_of(3.0, 15.0, 3.75, 6.0, 0.3)
        }
        "claude-opus-4-0" | "claude-opus-4-1" => price_of(15.0, 75.0, 18.75, 30.0, 1.5),
        "claude-opus-5-5" => price_of(4.0, 20.0, 5.0, 8.0, 0.2),
        "claude-fable-5-1" | "claude-mythos-5-1" => price_of(10.0, 50.0, 12.5, 20.0, 0.25),
        "claude-fable-5" | "claude-mythos-5" => price_of(10.0, 50.0, 12.5, 20.0, 1.0),
        // Opus 4.5–5 y lo que no se conoce: el precio por defecto de Claude Code.
        _ => price_of(5.0, 25.0, 6.25, 10.0, 0.5),
    }
}

// ---------------------------------------------------------------- transcripciones

/// Una respuesta del modelo (una llamada a la API).
#[derive(Clone, Debug, Default, PartialEq)]
struct Call {
    ts: i64,
    model: String,
    input: u64,
    output: u64,
    cache_read: u64,
    write_5m: u64,
    write_1h: u64,
    fast: bool,
    us: bool,
    /// Caracteres escritos (texto y entradas de herramientas), para estimar la salida.
    chars: u64,
    tool_use: bool,
    /// Entregó su informe (los subagentes en segundo plano terminan así).
    handback: bool,
}

impl Call {
    fn output(&self) -> u64 {
        self.output
            .max((self.chars as f64 / CHARS_PER_TOKEN).round() as u64)
    }

    /// Tokens de entrada de esa llamada: lo que ocupaba el contexto.
    fn context(&self) -> u64 {
        self.input + self.cache_read + self.write_5m + self.write_1h
    }

    fn cost(&self) -> f64 {
        let p = price(&self.model, self.fast, self.context());
        let usd = (self.input as f64 * p.input
            + self.output() as f64 * p.output
            + self.cache_read as f64 * p.read
            + self.write_5m as f64 * p.write_5m
            + self.write_1h as f64 * p.write_1h)
            / 1e6;
        if self.us { usd * 1.1 } else { usd }
    }

    /// Las líneas de una misma respuesta repiten el uso y traen un bloque cada una.
    fn merge(&mut self, other: Call) {
        self.ts = self.ts.min(other.ts);
        self.input = self.input.max(other.input);
        self.output = self.output.max(other.output);
        self.cache_read = self.cache_read.max(other.cache_read);
        self.write_5m = self.write_5m.max(other.write_5m);
        self.write_1h = self.write_1h.max(other.write_1h);
        self.chars += other.chars;
        self.tool_use |= other.tool_use;
        self.handback |= other.handback;
    }
}

/// Una línea `"type":"assistant"` de la transcripción → (id de la respuesta, llamada).
fn parse_call(line: &[u8]) -> Option<(String, Call)> {
    let v: Value = serde_json::from_slice(line).ok()?;
    if v["type"] != "assistant" {
        return None;
    }
    let message = &v["message"];
    let model = message["model"].as_str()?;
    let usage = &message["usage"];
    // `<synthetic>`: mensajes que escribe Claude Code sin llamar al modelo.
    if model.starts_with('<') || !usage.is_object() {
        return None;
    }
    let id = message["id"]
        .as_str()
        .or(v["requestId"].as_str())
        .or(v["uuid"].as_str())?
        .to_string();
    let ts = v["timestamp"].as_str().and_then(parse_ts)?;
    let n = |key: &str| usage[key].as_u64().unwrap_or(0);
    let creation = n("cache_creation_input_tokens");
    let one_hour = usage["cache_creation"]["ephemeral_1h_input_tokens"]
        .as_u64()
        .unwrap_or(0)
        .min(creation);
    let mut call = Call {
        ts,
        model: model.to_string(),
        input: n("input_tokens"),
        output: n("output_tokens"),
        cache_read: n("cache_read_input_tokens"),
        write_5m: creation - one_hour,
        write_1h: one_hour,
        fast: usage["speed"] == "fast",
        us: usage["inference_geo"] == "us",
        ..Call::default()
    };
    for block in message["content"].as_array().into_iter().flatten() {
        match block["type"].as_str() {
            Some("text") => {
                call.chars += block["text"].as_str().map_or(0, |t| t.chars().count()) as u64;
            }
            Some("tool_use") => {
                call.tool_use = true;
                call.handback |= block["name"] == "SubagentHandback";
                call.chars += block["input"].to_string().chars().count() as u64;
            }
            _ => {}
        }
    }
    Some((id, call))
}

#[derive(Default)]
struct Transcript {
    /// Bytes ya leídos (siempre hasta el final de una línea).
    offset: u64,
    calls: HashMap<String, Call>,
    title: Option<String>,
}

impl Transcript {
    /// Lee lo que se añadió desde la última vez (si el archivo encogió, empieza de cero).
    fn update(&mut self, path: &Path, len: u64) -> std::io::Result<()> {
        if len < self.offset {
            *self = Self::default();
        }
        if len == self.offset {
            return Ok(());
        }
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(self.offset))?;
        let mut buf = Vec::with_capacity((len - self.offset) as usize);
        file.take(len - self.offset).read_to_end(&mut buf)?;
        // La última línea puede estar a medio escribir: se lee la próxima vez.
        let Some(end) = memchr::memrchr(b'\n', &buf) else {
            return Ok(());
        };
        for line in buf[..end].split(|&b| b == b'\n') {
            self.line(line);
        }
        self.offset += end as u64 + 1;
        Ok(())
    }

    fn line(&mut self, line: &[u8]) {
        if memmem::find(line, br#""type":"assistant""#).is_some() {
            if let Some((id, call)) = parse_call(line) {
                match self.calls.entry(id) {
                    Entry::Occupied(mut e) => e.get_mut().merge(call),
                    Entry::Vacant(e) => {
                        e.insert(call);
                    }
                }
            }
        } else if memmem::find(line, br#""type":"custom-title""#).is_some()
            && let Ok(v) = serde_json::from_slice::<Value>(line)
            && let Some(title) = v["customTitle"].as_str()
        {
            self.title = Some(title.to_string());
        }
    }
}

/// Un archivo de transcripción que tocó la ventana.
#[derive(Debug)]
struct Found {
    path: PathBuf,
    len: u64,
    modified: i64,
    session: String,
    /// Id del subagente; `None` en la conversación principal de la sesión.
    agent: Option<String>,
    /// Es del vault (si no, solo cuenta para el total del equipo).
    vault: bool,
}

/// Transcripciones de todas las carpetas de `~/.claude/projects` escritas desde `since`.
fn find_transcripts(projects: &Path, vault_dir: &Path, since: i64) -> Vec<Found> {
    let mut out = Vec::new();
    let push =
        |out: &mut Vec<Found>, path: PathBuf, session: &str, agent: Option<&str>, vault: bool| {
            let Ok(meta) = fs::metadata(&path) else {
                return;
            };
            let modified = meta.modified().map(millis).unwrap_or(0);
            if modified >= since {
                out.push(Found {
                    path,
                    len: meta.len(),
                    modified,
                    session: session.to_string(),
                    agent: agent.map(String::from),
                    vault,
                });
            }
        };
    let Ok(dirs) = fs::read_dir(projects) else {
        return out;
    };
    for dir in dirs.flatten() {
        let dir = dir.path();
        let vault = dir == vault_dir;
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(session) = name.strip_suffix(".jsonl") {
                push(&mut out, entry.path(), session, None, vault);
            } else if entry.file_type().is_ok_and(|t| t.is_dir())
                && let Ok(subs) = fs::read_dir(entry.path().join("subagents"))
            {
                for sub in subs.flatten() {
                    let file = sub.file_name().to_string_lossy().into_owned();
                    if let Some(agent) = file
                        .strip_prefix("agent-")
                        .and_then(|f| f.strip_suffix(".jsonl"))
                    {
                        push(&mut out, sub.path(), &name, Some(agent), vault);
                    }
                }
            }
        }
    }
    out
}

/// `agent-<id>.meta.json`: tipo de agente (`subagent_type`) y la descripción de la tarea.
fn read_meta(transcript: &Path) -> (Option<String>, Option<String>) {
    let path = transcript.with_extension("meta.json");
    let Some(meta) = fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    else {
        return (None, None);
    };
    let get = |key: &str| {
        meta[key]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    (get("agentType"), get("description"))
}

// ---------------------------------------------------------------- registro

#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Tokens {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
}

impl Tokens {
    fn add(&mut self, call: &Call) {
        self.input += call.input;
        self.output += call.output();
        self.cache_read += call.cache_read;
        self.cache_write += call.write_5m + call.write_1h;
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct ModelUse {
    model: String,
    calls: u64,
    tokens: Tokens,
    cost: f64,
}

#[derive(Serialize, Clone, Debug)]
pub struct AgentUse {
    /// `s:<sesión>` (el orquestador) o `a:<sesión>/<subagente>`.
    id: String,
    /// `main` o `sub`.
    kind: &'static str,
    session: String,
    name: String,
    /// Nombre de la sesión o la tarea que se le encargó al subagente.
    title: Option<String>,
    /// Último modelo que usó.
    model: Option<String>,
    effort: Option<String>,
    active: bool,
    started: i64,
    last: i64,
    calls: u64,
    tokens: Tokens,
    /// Lo que costaría en la API, en dólares.
    cost: f64,
    /// Parte de la sesión de 5 h (puntos porcentuales).
    share: Option<f64>,
    /// Tokens en el contexto en su última llamada.
    context: u64,
    models: Vec<ModelUse>,
}

#[derive(Serialize, Clone, Copy, Debug)]
pub struct Window {
    start: i64,
    end: i64,
}

#[derive(Serialize, Debug)]
pub struct AgentLog {
    /// Sesión de 5 h en curso; `None` si no hay (el registro sale vacío).
    window: Option<Window>,
    /// Porcentaje usado de la sesión de 5 h (toda la cuenta).
    used: Option<f64>,
    error: Option<String>,
    machine_cost: f64,
    nexo_cost: f64,
    nexo_share: Option<f64>,
    agents: Vec<AgentUse>,
}

/// Quién está en marcha: la sesión de esta apertura de nexo y si Claude Code está abierto.
type Current = Option<(String, bool)>;

fn agent_use(
    found: &Found,
    title: Option<&str>,
    calls: &[&Call],
    current: &Current,
    efforts: &HashMap<String, Option<String>>,
    now: i64,
) -> AgentUse {
    let mut tokens = Tokens::default();
    let mut models: Vec<ModelUse> = Vec::new();
    for call in calls {
        tokens.add(call);
        let i = match models.iter().position(|m| m.model == call.model) {
            Some(i) => i,
            None => {
                models.push(ModelUse {
                    model: call.model.clone(),
                    calls: 0,
                    tokens: Tokens::default(),
                    cost: 0.0,
                });
                models.len() - 1
            }
        };
        let m = &mut models[i];
        m.calls += 1;
        m.tokens.add(call);
        m.cost += call.cost();
    }
    models.sort_by(|a, b| b.cost.total_cmp(&a.cost));
    let last = calls.last().expect("al menos una llamada");
    let ours = current.as_ref().filter(|(id, _)| *id == found.session);
    let quiet = now - found.modified;
    let (kind, id, name, title, effort, active) = match &found.agent {
        None => {
            let title = title.map(String::from);
            let nexo = title.as_deref().is_some_and(|t| t.starts_with("Nexo"));
            let active = match ours {
                Some((_, running)) => *running,
                None => quiet < RECENT_MS,
            };
            let name = if nexo { "Orquestador" } else { "Claude Code" };
            (
                "main",
                format!("s:{}", found.session),
                name.to_string(),
                title,
                None,
                active,
            )
        }
        Some(agent) => {
            let (agent_type, description) = read_meta(&found.path);
            let finished = last.handback || (!last.tool_use && last.chars > 0);
            // Si la sesión de nexo que lo lanzó se cerró, el subagente terminó con ella.
            let orphan = ours.is_some_and(|(_, running)| !running);
            let active = !finished && !orphan && quiet < STALE_MS;
            let name = agent_type.unwrap_or_else(|| "Subagente".into());
            let effort = efforts.get(&name).cloned().flatten();
            (
                "sub",
                format!("a:{}/{agent}", found.session),
                name,
                description,
                effort,
                active,
            )
        }
    };
    AgentUse {
        id,
        kind,
        session: found.session.clone(),
        name,
        title,
        model: Some(last.model.clone()),
        effort,
        active,
        started: calls.first().map_or(0, |c| c.ts),
        last: calls.iter().map(|c| c.ts).max().unwrap_or(0),
        calls: calls.len() as u64,
        tokens,
        cost: calls.iter().map(|c| c.cost()).sum(),
        share: None,
        context: last.context(),
        models,
    }
}

/// Lee (lo nuevo de) las transcripciones de la ventana y devuelve el coste de todo Claude
/// Code del equipo y los agentes del vault, activos primero y luego del más reciente.
fn build(
    cache: &mut HashMap<PathBuf, Transcript>,
    projects: &Path,
    vault_dir: &Path,
    since: i64,
    current: &Current,
    efforts: &HashMap<String, Option<String>>,
    now: i64,
) -> (f64, Vec<AgentUse>) {
    let mut found = find_transcripts(projects, vault_dir, since);
    // Las del vault primero: si una respuesta está en dos archivos (una sesión bifurcada
    // copia su historia), cuenta una vez y para nexo.
    found.sort_by_key(|f| (!f.vault, f.modified));
    let keep: HashSet<&PathBuf> = found.iter().map(|f| &f.path).collect();
    cache.retain(|path, _| keep.contains(path));
    let mut seen: HashSet<String> = HashSet::new();
    let mut machine = 0.0;
    let mut agents = Vec::new();
    for f in &found {
        let transcript = cache.entry(f.path.clone()).or_default();
        if let Err(e) = transcript.update(&f.path, f.len) {
            log::warn!("No se pudo leer {}: {e}", f.path.display());
            continue;
        }
        let mut calls: Vec<&Call> = transcript
            .calls
            .iter()
            .filter(|(id, call)| call.ts >= since && seen.insert((*id).clone()))
            .map(|(_, call)| call)
            .collect();
        machine += calls.iter().map(|c| c.cost()).sum::<f64>();
        if !f.vault || calls.is_empty() {
            continue;
        }
        calls.sort_by_key(|c| c.ts);
        agents.push(agent_use(
            f,
            transcript.title.as_deref(),
            &calls,
            current,
            efforts,
            now,
        ));
    }
    agents.sort_by(|a, b| b.active.cmp(&a.active).then(b.last.cmp(&a.last)));
    (machine, agents)
}

/// Reparte el porcentaje usado de la sesión según el coste de cada agente.
fn assign_shares(agents: &mut [AgentUse], machine: f64, used: Option<f64>) -> Option<f64> {
    let used = used?;
    let rate = if machine > 0.0 { used / machine } else { 0.0 };
    for agent in agents.iter_mut() {
        agent.share = Some(agent.cost * rate);
    }
    Some(agents.iter().map(|a| a.cost).sum::<f64>() * rate)
}

#[derive(Default)]
pub struct RegistroState {
    transcripts: Mutex<HashMap<PathBuf, Transcript>>,
    /// Un sondeo de contexto a la vez; cada resultado vale unos segundos.
    context: tokio::sync::Mutex<HashMap<String, (Instant, Value)>>,
}

#[tauri::command]
pub async fn agent_log(app: AppHandle, force: bool) -> Result<AgentLog, String> {
    let root = vault_root(&app)?;
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let now = now_ms();
    let (window, used, error) = match crate::usage::claude_session(&app, force).await {
        // Sin `resets_at` (o ya pasado) no hay sesión de 5 h en curso: el registro se vacía.
        Ok(Some(limit)) => {
            let end = limit
                .resets_at
                .as_deref()
                .and_then(parse_ts)
                .filter(|&end| end > now);
            let window = end.map(|end| Window {
                start: end - WINDOW_MS,
                end,
            });
            (window, Some(limit.used), None)
        }
        // Sin el dato del plan, las últimas 5 h y sin porcentaje.
        Ok(None) => (
            Some(Window {
                start: now - WINDOW_MS,
                end: now,
            }),
            None,
            Some("Claude no informó de la sesión de 5 h.".to_string()),
        ),
        Err(e) => (
            Some(Window {
                start: now - WINDOW_MS,
                end: now,
            }),
            None,
            Some(e),
        ),
    };
    let Some(span) = window else {
        return Ok(AgentLog {
            window,
            used,
            error,
            machine_cost: 0.0,
            nexo_cost: 0.0,
            nexo_share: used.map(|_| 0.0),
            agents: Vec::new(),
        });
    };
    let current = crate::claude_code::current_session(&app);
    let efforts: HashMap<String, Option<String>> = crate::agents::list(&root)
        .into_iter()
        .map(|a| (a.name, a.effort))
        .collect();
    let projects = home.join(".claude/projects");
    let vault_dir = projects.join(project_dir_name(&root));
    let handle = app.clone();
    let (machine, mut agents) = tauri::async_runtime::spawn_blocking(move || {
        let state = handle.state::<RegistroState>();
        let mut cache = state.transcripts.lock().map_err(|e| e.to_string())?;
        Ok::<_, String>(build(
            &mut cache, &projects, &vault_dir, span.start, &current, &efforts, now,
        ))
    })
    .await
    .map_err(|e| e.to_string())??;
    let nexo_share = assign_shares(&mut agents, machine, used);
    Ok(AgentLog {
        window,
        used,
        error,
        machine_cost: machine,
        nexo_cost: agents.iter().map(|a| a.cost).sum(),
        nexo_share,
        agents,
    })
}

// ---------------------------------------------------------------- contexto (/context)

/// Pide a Claude Code el desglose del contexto (`get_context_usage`) sin hablar con el
/// modelo: lo responde con la transcripción y estimaciones locales.
fn probe(claude: &Path, cwd: &Path, args: &[String]) -> Result<Value, String> {
    let mut child = Command::new(claude)
        .args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--no-session-persistence",
            // Sin servidores MCP (sus herramientas van diferidas y no ocupan contexto) ni
            // hooks del usuario: el sondeo no debe hacer nada más.
            "--strict-mcp-config",
            "--settings",
            r#"{"disableAllHooks":true}"#,
        ])
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("No se pudo abrir Claude Code: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("Sin entrada")?;
    let stdout = child.stdout.take().ok_or("Sin salida")?;
    let request = json!({
        "type": "control_request",
        "request_id": "nexo-contexto",
        "request": { "subtype": "get_context_usage", "detail": "summary" }
    });
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if msg["type"] == "control_response" {
                let _ = tx.send(msg);
                return;
            }
        }
    });
    let sent = writeln!(stdin, "{request}").and_then(|_| stdin.flush());
    let answer = sent.map_err(|e| e.to_string()).and_then(|_| {
        rx.recv_timeout(Duration::from_secs(25))
            .map_err(|_| "Claude Code no respondió a tiempo.".to_string())
    });
    // Sin entrada, Claude Code termina solo (y cierra lo que haya abierto).
    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    let answer = answer?;
    let response = &answer["response"];
    if response["subtype"] != "success" {
        return Err(response["error"]
            .as_str()
            .unwrap_or("Claude Code no pudo medir el contexto.")
            .to_string());
    }
    let mut value = response["response"].clone();
    if let Some(obj) = value.as_object_mut() {
        obj.remove("gridRows");
    }
    Ok(value)
}

/// Contexto de un subagente: el total es el de su última llamada (exacto); el reparto entre
/// instrucciones, herramientas, skills… sale de un sondeo con su configuración (`--agent`),
/// ajustado para no pasar de lo que ocupaba su primera llamada. Lo demás es su conversación.
fn sub_context(first: u64, last: u64, model: &str, probe: Option<&Value>) -> Value {
    // Los modelos 5.x y Opus 4.7/4.8 tienen 1 M de contexto; los anteriores, 200 000.
    let base = base_model(model);
    let one_million = matches!(base, "claude-opus-4-7" | "claude-opus-4-8")
        || base.split('-').nth(2) == Some("5");
    let mut max = if one_million { 1_000_000 } else { 200_000 };
    let mut buffer = 33_000;
    let mut categories: Vec<(String, u64)> = Vec::new();
    let mut extra = serde_json::Map::new();
    let messages = if let Some(p) = probe {
        max = p["maxTokens"].as_u64().unwrap_or(max);
        for c in p["categories"].as_array().into_iter().flatten() {
            let name = c["name"].as_str().unwrap_or_default();
            let tokens = c["tokens"].as_u64().unwrap_or(0);
            match (c["kind"].as_str(), name) {
                (Some("buffer"), _) => buffer = tokens,
                (_, "Messages") | (Some("free"), _) => {}
                // Los subagentes reciben todas sus herramientas, sin diferir.
                (Some("deferred"), n) | (Some("used"), n) if tokens > 0 => {
                    let n = n.replace(" (deferred)", "");
                    match categories.iter_mut().find(|(name, _)| *name == n) {
                        Some((_, t)) => *t += tokens,
                        None => categories.push((n, tokens)),
                    }
                }
                _ => {}
            }
        }
        let base: u64 = categories.iter().map(|(_, t)| t).sum();
        if base > first && base > 0 {
            let scale = first as f64 / base as f64;
            for (_, t) in categories.iter_mut() {
                *t = (*t as f64 * scale).round() as u64;
            }
        }
        for key in ["skills", "agents", "memoryFiles", "mcpTools"] {
            if let Some(v) = p.get(key) {
                extra.insert(key.into(), v.clone());
            }
        }
        last.saturating_sub(categories.iter().map(|(_, t)| t).sum())
    } else {
        categories.push(("Initial context".into(), first.min(last)));
        last.saturating_sub(first)
    };
    categories.push(("Messages".into(), messages));
    let mut list: Vec<Value> = categories
        .into_iter()
        .map(|(name, tokens)| json!({ "name": name, "tokens": tokens, "kind": "used" }))
        .collect();
    list.push(json!({ "name": "Autocompact buffer", "tokens": buffer, "kind": "buffer" }));
    list.push(json!({
        "name": "Free space",
        "tokens": max.saturating_sub(last).saturating_sub(buffer),
        "kind": "free"
    }));
    let mut out = json!({
        "kind": "sub",
        "exact": false,
        "model": model,
        "totalTokens": last,
        "maxTokens": max,
        "percentage": (last as f64 * 100.0 / max as f64).round(),
        "categories": list,
        "firstTokens": first,
    });
    if let Some(obj) = out.as_object_mut() {
        obj.extend(extra);
    }
    out
}

fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

#[tauri::command]
pub async fn agent_context(
    app: AppHandle,
    state: State<'_, RegistroState>,
    id: String,
) -> Result<Value, String> {
    let root = vault_root(&app)?;
    let claude = crate::claude_code::find_claude(&app)
        .ok_or("No encontré Claude Code para medir el contexto.")?;
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let vault_dir = home.join(".claude/projects").join(project_dir_name(&root));
    let mut cache = state.context.lock().await;
    if let Some((at, value)) = cache.get(&id)
        && at.elapsed() < Duration::from_secs(10)
    {
        return Ok(value.clone());
    }
    let value = if let Some(session) = id.strip_prefix("s:") {
        if !safe_id(session) {
            return Err("Sesión no válida.".into());
        }
        let file = vault_dir.join(format!("{session}.jsonl"));
        if !file.is_file() {
            return Err("Esa sesión ya no existe.".into());
        }
        let mut transcript = Transcript::default();
        let len = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        let _ = transcript.update(&file, len);
        let mut args = vec![
            "--resume".into(),
            session.to_string(),
            "--fork-session".into(),
        ];
        // Las sesiones de nexo llevan el prompt del orquestador: cuenta en el contexto.
        if let Some(title) = transcript.title.filter(|t| t.starts_with("Nexo")) {
            args.push("--append-system-prompt".into());
            args.push(crate::claude_code::orchestrator_prompt(&root, &title));
        }
        let mut value = tauri::async_runtime::spawn_blocking(move || probe(&claude, &root, &args))
            .await
            .map_err(|e| e.to_string())??;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("kind".into(), "main".into());
            obj.insert("exact".into(), true.into());
        }
        value
    } else if let Some((session, agent)) = id.strip_prefix("a:").and_then(|r| r.split_once('/')) {
        if !safe_id(session) || !safe_id(agent) {
            return Err("Agente no válido.".into());
        }
        let file = vault_dir
            .join(session)
            .join("subagents")
            .join(format!("agent-{agent}.jsonl"));
        let mut transcript = Transcript::default();
        let len = fs::metadata(&file)
            .map(|m| m.len())
            .map_err(|_| "Ese agente ya no existe.")?;
        transcript.update(&file, len).map_err(|e| e.to_string())?;
        let mut calls: Vec<&Call> = transcript.calls.values().collect();
        calls.sort_by_key(|c| c.ts);
        let (Some(first), Some(last)) = (calls.first(), calls.last()) else {
            return Err("Ese agente aún no ha llamado al modelo.".into());
        };
        let (first, last, model) = (first.context(), last.context(), last.model.clone());
        let (agent_type, _) = read_meta(&file);
        let probed = match agent_type {
            Some(t) if !t.starts_with('-') => {
                let args = vec!["--agent".to_string(), t];
                tauri::async_runtime::spawn_blocking(move || probe(&claude, &root, &args))
                    .await
                    .ok()
                    .and_then(Result::ok)
            }
            _ => None,
        };
        sub_context(first, last, &model, probed.as_ref())
    } else {
        return Err("Agente no válido.".into());
    };
    cache.insert(id, (Instant::now(), value.clone()));
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nexo-registro-{name}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn iso(ms: i64) -> String {
        chrono::DateTime::from_timestamp_millis(ms)
            .unwrap()
            .to_rfc3339()
    }

    /// Línea de respuesta como las de Claude Code: un bloque por línea.
    fn line(id: &str, ms: i64, model: &str, usage: Value, block: Value) -> String {
        json!({
            "type": "assistant",
            "timestamp": iso(ms),
            "message": { "id": id, "model": model, "usage": usage, "content": [block] }
        })
        .to_string()
    }

    fn usage(input: u64, output: u64, read: u64, write: u64) -> Value {
        json!({
            "input_tokens": input,
            "output_tokens": output,
            "cache_read_input_tokens": read,
            "cache_creation_input_tokens": write,
            "cache_creation": { "ephemeral_5m_input_tokens": write, "ephemeral_1h_input_tokens": 0 }
        })
    }

    #[test]
    fn el_coste_coincide_con_el_de_claude_code() {
        // Uso de Opus 5.5 de una sesión de nexo y el costUSD que guardó Claude Code.
        let call = Call {
            model: "claude-opus-5-5".into(),
            input: 3170,
            output: 9495,
            cache_read: 2_448_381,
            write_1h: 245_287,
            ..Call::default()
        };
        assert!((call.cost() - 2.6545522).abs() < 1e-6, "{}", call.cost());
        // Haiku 5.5 sube de precio por encima de 100 000 tokens de prompt.
        let small = Call {
            model: "claude-haiku-5-5".into(),
            cache_read: 100_000,
            ..Call::default()
        };
        let big = Call {
            cache_read: 100_001,
            ..small.clone()
        };
        assert!(big.cost() > small.cost() * 4.0);
        assert_eq!(base_model("claude-opus-4-1-20250805"), "claude-opus-4-1");
        assert_eq!(base_model("claude-sonnet-5-5[1m]"), "claude-sonnet-5-5");
        assert_eq!(price("claude-3-7-sonnet-20250219", false, 0).input, 3.0);
    }

    #[test]
    fn junta_las_lineas_de_una_misma_respuesta() {
        let dir = temp("lineas");
        let file = dir.join("s.jsonl");
        let u = usage(2, 6, 1000, 500);
        let text = [
            line(
                "m1",
                1_000,
                "claude-sonnet-5-5",
                u.clone(),
                json!({"type": "thinking", "thinking": ""}),
            ),
            line(
                "m1",
                1_200,
                "claude-sonnet-5-5",
                u.clone(),
                json!({"type": "tool_use", "name": "Bash", "input": {"command": "x".repeat(500)}}),
            ),
            r#"{"type":"custom-title","customTitle":"Nexo · prueba"}"#.to_string(),
            line(
                "m2",
                2_000,
                "<synthetic>",
                u.clone(),
                json!({"type": "text", "text": "x"}),
            ),
        ]
        .join("\n");
        // La última línea queda a medias: no se lee hasta que se complete.
        fs::write(&file, format!("{text}\n{{\"type\":\"assist")).unwrap();
        let mut t = Transcript::default();
        let len = fs::metadata(&file).unwrap().len();
        t.update(&file, len).unwrap();
        assert_eq!(t.calls.len(), 1);
        let call = &t.calls["m1"];
        assert_eq!(call.ts, 1_000);
        assert!(call.tool_use && !call.handback);
        // La salida guardada (6) se queda corta: cuenta lo escrito.
        assert!(call.output() > 200, "{}", call.output());
        assert_eq!(call.context(), 1502);
        assert_eq!(t.title.as_deref(), Some("Nexo · prueba"));
        assert!(t.offset < len);

        // Se completa la línea y llega otra respuesta: solo se lee lo nuevo.
        let more = line(
            "m3",
            3_000,
            "claude-sonnet-5-5",
            usage(1, 50, 0, 0),
            json!({"type": "text", "text": "hola"}),
        );
        let mut all = format!("{text}\n");
        all.push_str(&more);
        all.push('\n');
        fs::write(&file, &all).unwrap();
        t.update(&file, all.len() as u64).unwrap();
        assert_eq!(t.calls.len(), 2);
        assert_eq!(t.offset, all.len() as u64);
        fs::remove_dir_all(dir).unwrap();
    }

    fn write_lines(path: &Path, lines: &[String]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, lines.join("\n") + "\n").unwrap();
    }

    #[test]
    fn registro_de_la_ventana_con_su_parte_de_la_sesion() {
        let projects = temp("ventana");
        let root = Path::new("/home/ana/mi_vault");
        let vault_dir = projects.join(project_dir_name(root));
        assert!(vault_dir.ends_with("-home-ana-mi-vault"));
        let now = now_ms();
        let since = now - WINDOW_MS;
        let opus = "claude-opus-5-5";
        // Sesión de nexo: una respuesta de antes de la ventana (no cuenta) y dos dentro.
        write_lines(
            &vault_dir.join("s1.jsonl"),
            &[
                r#"{"type":"custom-title","customTitle":"Nexo · 2026-10-08 16:58"}"#.into(),
                line(
                    "old",
                    since - 60_000,
                    opus,
                    usage(10, 10, 10, 10),
                    json!({"type": "text", "text": "a"}),
                ),
                line(
                    "a1",
                    now - 50_000,
                    opus,
                    usage(10, 100, 10_000, 1_000),
                    json!({"type": "text", "text": "b"}),
                ),
                line(
                    "a2",
                    now - 40_000,
                    opus,
                    usage(10, 100, 12_000, 1_000),
                    json!({"type": "tool_use", "name": "Agent", "input": {}}),
                ),
            ],
        );
        // Su subagente terminó entregando el informe.
        let sub = vault_dir.join("s1/subagents/agent-abc.jsonl");
        write_lines(
            &sub,
            &[
                line(
                    "b1",
                    now - 30_000,
                    "claude-haiku-5-5",
                    usage(5, 5, 20_000, 2_000),
                    json!({"type": "tool_use", "name": "Read", "input": {}}),
                ),
                line(
                    "b2",
                    now - 20_000,
                    "claude-haiku-5-5",
                    usage(5, 5, 22_000, 100),
                    json!({"type": "tool_use", "name": "SubagentHandback", "input": {"r": "ok"}}),
                ),
            ],
        );
        fs::write(
            sub.with_extension("meta.json"),
            r#"{"agentType":"revisor-classroom","description":"Revisar Classroom"}"#,
        )
        .unwrap();
        // Otro subagente sigue trabajando (su última respuesta pide una herramienta).
        write_lines(
            &vault_dir.join("s1/subagents/agent-def.jsonl"),
            &[line(
                "c1",
                now - 10_000,
                "claude-sonnet-5-5",
                usage(1, 1, 1_000, 100),
                json!({"type": "tool_use", "name": "Bash", "input": {}}),
            )],
        );
        // Otra carpeta: solo cuenta para el total. Repite a1 (sesión bifurcada): no se duplica.
        write_lines(
            &projects.join("-otra").join("s9.jsonl"),
            &[
                line(
                    "a1",
                    now - 50_000,
                    opus,
                    usage(10, 100, 10_000, 1_000),
                    json!({"type": "text", "text": "b"}),
                ),
                line(
                    "z1",
                    now - 5_000,
                    opus,
                    usage(10, 100, 10_000, 1_000),
                    json!({"type": "text", "text": "c"}),
                ),
            ],
        );
        let efforts =
            HashMap::from([("revisor-classroom".to_string(), Some("medium".to_string()))]);
        let current = Some(("s1".to_string(), true));
        let mut cache = HashMap::new();
        let (machine, mut agents) = build(
            &mut cache, &projects, &vault_dir, since, &current, &efforts, now,
        );
        assert_eq!(agents.len(), 3);
        let by = |id: &str| agents.iter().find(|a| a.id == id).unwrap();
        let main = by("s:s1");
        assert_eq!(main.name, "Orquestador");
        assert_eq!(main.calls, 2);
        assert!(main.active);
        assert_eq!(main.context, 13_010);
        let done = by("a:s1/abc");
        assert_eq!(done.name, "revisor-classroom");
        assert_eq!(done.title.as_deref(), Some("Revisar Classroom"));
        assert_eq!(done.effort.as_deref(), Some("medium"));
        assert!(!done.active);
        assert!(by("a:s1/def").active);
        // Activos primero.
        assert!(agents[0].active && agents[1].active && !agents[2].active);
        let ours: f64 = agents.iter().map(|a| a.cost).sum();
        let other = Call {
            model: opus.into(),
            input: 10,
            output: 100,
            cache_read: 10_000,
            write_5m: 1_000,
            ..Call::default()
        }
        .cost();
        assert!((machine - ours - other).abs() < 1e-9);
        let nexo = assign_shares(&mut agents, machine, Some(40.0)).unwrap();
        assert!((nexo - 40.0 * ours / machine).abs() < 1e-9);
        let sum: f64 = agents.iter().map(|a| a.share.unwrap()).sum();
        assert!((sum - nexo).abs() < 1e-9);

        // Si la sesión de nexo se cierra, sus subagentes ya no cuentan como activos.
        let closed = Some(("s1".to_string(), false));
        let (_, agents) = build(
            &mut cache, &projects, &vault_dir, since, &closed, &efforts, now,
        );
        assert!(agents.iter().all(|a| !a.active));
        // Una ventana nueva deja fuera todo lo anterior.
        let (machine, agents) = build(
            &mut cache,
            &projects,
            &vault_dir,
            now + 1,
            &current,
            &efforts,
            now,
        );
        assert!(agents.is_empty() && machine == 0.0);
        fs::remove_dir_all(projects).unwrap();
    }

    #[test]
    fn contexto_de_un_subagente() {
        let probe = json!({
            "maxTokens": 1_000_000,
            "categories": [
                { "name": "System prompt", "tokens": 2_000, "kind": "used" },
                { "name": "System tools", "tokens": 4_000, "kind": "used" },
                { "name": "System tools (deferred)", "tokens": 20_000, "kind": "deferred" },
                { "name": "Skills", "tokens": 7_000, "kind": "used" },
                { "name": "Autocompact buffer", "tokens": 33_000, "kind": "buffer" },
                { "name": "Free space", "tokens": 934_000, "kind": "free" }
            ],
            "skills": { "totalSkills": 1 }
        });
        let v = sub_context(35_000, 50_000, "claude-sonnet-5-5", Some(&probe));
        let cats = v["categories"].as_array().unwrap();
        let get = |n: &str| {
            cats.iter().find(|c| c["name"] == n).unwrap()["tokens"]
                .as_u64()
                .unwrap()
        };
        assert_eq!(get("System tools"), 24_000);
        assert_eq!(get("Messages"), 50_000 - 33_000);
        assert_eq!(get("Free space"), 1_000_000 - 50_000 - 33_000);
        assert_eq!(v["totalTokens"], 50_000);
        assert!(v["skills"].is_object());
        // Si la configuración ocupa más que la primera llamada, se ajusta a ella.
        let v = sub_context(16_500, 20_000, "claude-sonnet-5-5", Some(&probe));
        let used: u64 = v["categories"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["kind"] == "used")
            .map(|c| c["tokens"].as_u64().unwrap())
            .sum();
        assert_eq!(used, 20_000);
        // Sin sondeo: lo que tenía al empezar y lo que añadió su trabajo.
        let v = sub_context(10_000, 25_000, "claude-haiku-4-5", None);
        assert_eq!(v["maxTokens"], 200_000);
        assert_eq!(v["categories"][0]["name"], "Initial context");
        assert_eq!(v["categories"][1]["tokens"], 15_000);
    }

    #[test]
    fn rechaza_ids_raros() {
        assert!(safe_id("02926ccf-efac-4cb0-bb2c-bdfe14c76590"));
        assert!(safe_id("aff10fd1595e632e9"));
        for id in ["", "..", "a/b", "a b", "../x", "x\\y"] {
            assert!(!safe_id(id), "{id}");
        }
    }
}
