//! El orquestador de cada workspace: un Claude Code sin terminal (como el chat de la carta
//! derecha, `claude_code.rs`) que arranca en la carpeta del workspace con su propia sesión,
//! que se retoma al volver a abrirlo. Despliega los agentes con la herramienta Agent de Claude
//! Code; lo que hace cada uno llega por el mismo protocolo (con `parent_tool_use_id`) y la
//! interfaz lo reparte en sus ventanas (`ws-stream.js`).
//!
//! - Cada workspace tiene su proceso, su historial en memoria y a quién se lo manda. Al
//!   cambiar de workspace la interfaz se desconecta (`workspace_detach`): si el orquestador
//!   no está haciendo nada se cierra (la sesión se retoma al volver) y, si está trabajando, se
//!   cierra cuando termine él y sus agentes en segundo plano.
//! - Al leer la salida, cada `task_started` de un subagente se registra en
//!   `.nexo/agentes/` (`workspace::register_task`): pase lo que pase en la interfaz, todo
//!   agente desplegado queda en su workspace.
//! - Al abrir un workspace con una sesión guardada, su conversación (y la de sus agentes) se
//!   lee de las transcripciones de Claude Code y se le repite a la interfaz (`history`).
//! - Lo que el orquestador sabe de nexo va en `--append-system-prompt` y en una skill de un
//!   plugin propio cargado con `--plugin-dir` (fuera de la carpeta del usuario).

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::{Value, json};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::claude_code::{
    BACKLOG_MAX, ChatEvent, ChatOptions, INHERITED_ENV, find_claude, keep_in_backlog, option_args,
};
use crate::workspace::{
    self, Extras, RegAgent, catalog_for, extras_settings, read_config, update_config, ws_root,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Lo que se sabe del proceso mientras corre (lo actualiza el lector).
#[derive(Default)]
struct Live {
    /// Hay un turno en marcha.
    turn: AtomicBool,
    /// Tareas en segundo plano vivas (subagentes, órdenes largas).
    background: AtomicUsize,
    /// La interfaz se fue a otro workspace: cerrar en cuanto no quede nada en marcha.
    stop_when_idle: AtomicBool,
}

struct Proc {
    id: u64,
    child: Child,
    stdin: ChildStdin,
    live: Arc<Live>,
}

#[derive(Default)]
struct Shared {
    sink: Option<Channel<ChatEvent>>,
    backlog: Vec<String>,
}

#[derive(Default)]
struct Run {
    proc: Option<Proc>,
    shared: Arc<Mutex<Shared>>,
    /// Los extras con los que se lanzó el proceso que corre.
    applied: Option<Extras>,
    /// Ya se cargó el historial de las transcripciones en esta apertura de nexo.
    loaded: bool,
}

#[derive(Default)]
pub struct WorkspaceRuns {
    runs: Mutex<HashMap<PathBuf, Run>>,
}

#[derive(Serialize)]
pub struct WsStarted {
    session: String,
    resumed: bool,
    attached: bool,
    applied: Extras,
}

fn stop(proc: Option<Proc>) {
    if let Some(mut p) = proc {
        let _ = p.child.kill();
        let _ = p.child.wait();
    }
}

fn alive(proc: &mut Option<Proc>) -> bool {
    proc.as_mut()
        .is_some_and(|p| matches!(p.child.try_wait(), Ok(None)))
}

fn remember(shared: &mut Shared, line: &str) {
    if !keep_in_backlog(line) {
        return;
    }
    if shared.backlog.len() >= BACKLOG_MAX {
        let extra = shared.backlog.len() + 1 - BACKLOG_MAX;
        shared.backlog.drain(..extra);
    }
    shared.backlog.push(line.to_string());
}

/// Nombre de la sesión del orquestador, como lo verá el usuario en `claude --resume`.
fn session_name(root: &Path) -> String {
    format!(
        "{} · nexo",
        root.file_name()
            .map_or_else(|| "Workspace".into(), |n| n.to_string_lossy().into_owned())
    )
}

fn projects_dir(app: &AppHandle, root: &Path) -> Result<PathBuf, String> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    Ok(home
        .join(".claude/projects")
        .join(crate::registro::project_dir_name(root)))
}

// ---------------------------------------------------------------- papel del orquestador

/// Prefijo de los mensajes que el usuario escribe en la ventana de un agente: el orquestador
/// se los pasa (y el historial los reconoce para pintarlos en la ventana de ese agente).
pub(crate) const TO_AGENT: &str = "[nexo → agente ";

/// Línea propia que cierra la conversación repetida a una interfaz recién conectada.
const REPLAY_END: &str = r#"{"type":"nexo_replay_end"}"#;

pub(crate) fn orchestrator_prompt(root: &Path) -> String {
    format!(
        "Estás dentro de nexo, en el workspace «{name}», la carpeta {path}. Eres el orquestador, el agente principal \
de esta carpeta: hablas con el usuario, decides cómo hacer cada tarea y repartes el trabajo entre agentes cuando \
conviene (tareas largas, en paralelo o de un especialista).\n\n\
Agentes del workspace: cada agente que despliegas con la herramienta Agent aparece en nexo como una ventana propia, \
con su nombre, su rol, si está activo, esperando respuesta o inactivo, y los tokens que gasta; nexo lo registra solo en \
.nexo/agentes/<nombre>.md. Antes de desplegar o continuar un agente sigue la skill agentes-del-workspace (del plugin \
nexo). Lo esencial:\n\
- La description que le das a Agent es su nombre en nexo: de dos a cinco palabras, en el idioma del usuario, que \
digan qué hace («Revisor de la API»). Dale un subagent_type que encaje con su papel.\n\
- Antes de crear uno nuevo, lista .nexo/agentes/: si ya hay un agente con ese papel, continúalo con SendMessage \
(cárgala con ToolSearch si no la tienes) en lugar de duplicarlo.\n\
- Solo tú puedes quitar agentes del workspace: el usuario no puede cerrar sus ventanas. Cuando uno ya no haga falta, \
borra su archivo de .nexo/agentes/. Puedes cambiar su rol editando el campo role de ese archivo. No toques el resto \
de .nexo/.\n\
- Los mensajes que el usuario escribe en la ventana de un agente te llegan empezando por «{to}<id>]»: pásaselos a ese \
agente con SendMessage (si ya terminó y no responde, vuelve a encargárselo con Agent y el mismo subagent_type) y no \
los contestes tú.\n\n\
Formato: tus respuestas se ven en el chat de nexo, que entiende Markdown (párrafos, listas, negritas, `código`, \
bloques de código y tablas sencillas). Sé directo y responde en el idioma del usuario.",
        name = root.file_name().map_or_else(
            || root.display().to_string(),
            |n| n.to_string_lossy().into_owned()
        ),
        path = root.display(),
        to = TO_AGENT,
    )
}

const SKILL: &str = r#"---
name: agentes-del-workspace
description: Cómo desplegar, nombrar, dar rol, continuar, hablar con y quitar los agentes de un workspace de nexo. Úsala antes de lanzar un agente con la herramienta Agent, al continuarlo con SendMessage, cuando el usuario escriba a un agente desde su ventana o cuando haya que retirar uno.
---

# Agentes del workspace de nexo

nexo enseña cada agente que despliegas como una ventana con su chat, su nombre, su rol, su estado (activo,
esperando respuesta o inactivo) y lo que gasta. Para eso cada agente queda registrado en su workspace.

## Desplegar un agente

1. Lista `.nexo/agentes/`. Si un agente registrado ya tiene el papel que buscas y sigue disponible, continúalo
   con `SendMessage` (su id está en el campo `task`) en vez de crear otro.
2. Si hace falta uno nuevo, usa la herramienta `Agent`:
   - `description`: su nombre en nexo, de dos a cinco palabras («Revisor de la API», «Buscador de tests»).
   - `subagent_type`: el que encaje. Si en `.claude/agents/` hay uno definido para ese papel, úsalo; si no,
     `general-purpose` (o `Explore` para buscar sin cambiar nada).
   - `prompt`: el encargo completo, con el contexto que necesite. El agente no ve tu conversación.
   - Para trabajos largos o en paralelo, en segundo plano (`run_in_background: true`).
3. nexo escribe solo su archivo `.nexo/agentes/<nombre>.md`. Si su rol no queda claro con el tipo, edita el
   campo `role` con una frase que diga de qué se encarga.

## El registro (`.nexo/agentes/<nombre>.md`)

```
---
name: Revisor de la API
role: Revisa los endpoints de la API y apunta los errores
type: general-purpose
task: <id del agente en Claude Code>
tool_use: <id de la llamada a Agent>
session: <tu sesión>
created: AAAA-MM-DD HH:MM
---

<encargo inicial>
```

Puedes cambiar `name` y `role`. No cambies `task`, `tool_use` ni `session`: con ellos nexo encuentra su
conversación y lo que gasta. No toques `.nexo/workspace.json`.

## Hablar con un agente

El usuario puede escribir a un agente desde su ventana. Te llega un mensaje que empieza por
`[nexo → agente <id>] <nombre>`, con el texto del usuario debajo. Pásaselo a ese agente con `SendMessage`
(destinatario: su id) sin contestarlo tú. Si el agente ya terminó y no lo puedes continuar, vuelve a encargarle
el trabajo con `Agent` y el mismo `subagent_type`, y di que es una continuación.

## Quitar un agente

Solo tú puedes quitar agentes: el usuario no puede cerrar sus ventanas. Cuando un agente ya no haga falta (terminó
su papel o el usuario lo pide), si sigue en marcha detenlo (`TaskStop`) y borra su archivo de `.nexo/agentes/`.
Su ventana desaparece de nexo. Di en una línea qué agente quitaste.

## Agentes reutilizables

Si un papel se va a repetir en este workspace, defínelo en `.claude/agents/<nombre>.md` con `name`,
`description` (su papel), `model` y `effort` explícitos, y despliégalo con ese `subagent_type`: nexo toma su
`description` como rol.
"#;

/// Deja el plugin de nexo (la skill de los agentes del workspace) en la carpeta de datos de
/// nexo y devuelve su ruta para `--plugin-dir`.
fn install_plugin(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("workspace-plugin")
        .join("nexo");
    let skill = dir.join("skills/agentes-del-workspace");
    fs::create_dir_all(dir.join(".claude-plugin")).map_err(|e| e.to_string())?;
    fs::create_dir_all(&skill).map_err(|e| e.to_string())?;
    let manifest = json!({
        "name": "nexo",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Lo que nexo le enseña al orquestador de un workspace: cómo se despliegan y registran sus agentes."
    });
    let write = |path: PathBuf, text: String| -> Result<(), String> {
        if fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            fs::write(&path, text).map_err(|e| e.to_string())?;
        }
        Ok(())
    };
    write(
        dir.join(".claude-plugin/plugin.json"),
        serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?,
    )?;
    write(skill.join("SKILL.md"), SKILL.to_string())?;
    Ok(dir)
}

/// Argumentos del orquestador de un workspace: los del chat de nexo más su papel, el plugin
/// de nexo y los ajustes que apagan los extras.
#[allow(clippy::too_many_arguments)]
fn launch_args(
    session: &str,
    saved: bool,
    name: &str,
    prompt: &str,
    plugin: &Path,
    settings: &Value,
    options: &ChatOptions,
) -> Result<Vec<String>, String> {
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--forward-subagent-text",
        "--permission-prompt-tool",
        "stdio",
    ]
    .map(String::from)
    .to_vec();
    if saved {
        args.extend(["--resume".into(), session.into()]);
    } else {
        args.extend([
            "--session-id".into(),
            session.into(),
            "--name".into(),
            name.into(),
        ]);
    }
    args.extend(["--append-system-prompt".into(), prompt.into()]);
    args.extend(["--plugin-dir".into(), plugin.to_string_lossy().into_owned()]);
    args.extend(["--settings".into(), settings.to_string()]);
    args.extend(option_args(options)?);
    Ok(args)
}

// ---------------------------------------------------------------- historial

/// El texto de un mensaje del usuario en una transcripción, si es algo que escribió él (no
/// un resultado de herramienta, un aviso del sistema ni una orden local).
fn typed_text(message: &Value) -> Option<String> {
    let text = match &message["content"] {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => {
            if blocks.iter().any(|b| b["type"] == "tool_result") {
                return None;
            }
            blocks
                .iter()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => return None,
    };
    let trimmed = text.trim();
    const SYSTEM: &[&str] = &[
        "<command-",
        "<local-command",
        "<system-reminder>",
        "<task-notification>",
        "Caveat:",
        "[Request interrupted",
    ];
    if trimmed.is_empty() || SYSTEM.iter().any(|p| trimmed.starts_with(p)) {
        return None;
    }
    Some(trimmed.to_string())
}

/// Lo que escribió el usuario como la línea propia que la interfaz guarda (`nexo_user`): los
/// archivos adjuntos vuelven a ser adjuntos y los mensajes a un agente, suyos.
fn user_record(text: &str, parent: Option<&str>) -> Value {
    const FILES: &str = "\n\nArchivos adjuntos (léelos con Read):\n";
    let (body, files) = match text.split_once(FILES) {
        Some((body, list)) => (
            body.to_string(),
            list.lines()
                .filter_map(|l| l.strip_prefix("- "))
                .map(String::from)
                .collect::<Vec<_>>(),
        ),
        None if text.starts_with("Archivos adjuntos (léelos con Read):\n") => (
            String::new(),
            text.lines()
                .skip(1)
                .filter_map(|l| l.strip_prefix("- "))
                .map(String::from)
                .collect(),
        ),
        None => (text.to_string(), Vec::new()),
    };
    let mut record = json!({ "type": "nexo_user", "text": body, "files": files });
    if let Some(rest) = body.strip_prefix(TO_AGENT)
        && let Some((id, rest)) = rest.split_once(']')
    {
        // «[nexo → agente <id>] <nombre>\n<texto>\n\n—\n<instrucción>»
        let message = rest.split_once('\n').map_or("", |(_, m)| m);
        let message = message.split("\n\n—\n").next().unwrap_or(message).trim();
        record["text"] = Value::String(message.to_string());
        record["to"] = Value::String(id.trim().to_string());
    }
    if let Some(p) = parent {
        record["parent_tool_use_id"] = Value::String(p.to_string());
    }
    record
}

/// Las líneas de una transcripción que sirven para volver a pintar la conversación: lo que
/// escribió el usuario, las respuestas del modelo y los resultados de las herramientas. Las
/// de un subagente llevan `parent_tool_use_id`, como en directo.
fn transcript_lines(path: &Path, parent: Option<&str>, skip_prompt: bool) -> Vec<String> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut prompt_skipped = !skip_prompt;
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { break };
        let Ok(mut v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let kind = v["type"].as_str().unwrap_or_default().to_string();
        if (parent.is_none() && v["isSidechain"] == true)
            || v["isMeta"] == true
            || v["isCompactSummary"] == true
        {
            continue;
        }
        match kind.as_str() {
            "user" => {
                if let Some(text) = typed_text(&v["message"]) {
                    if !prompt_skipped {
                        prompt_skipped = true;
                        continue;
                    }
                    out.push(user_record(&text, parent).to_string());
                } else if v["message"]["content"].is_array() {
                    let mut line = json!({ "type": "user", "message": v["message"].take() });
                    if let Some(p) = parent {
                        line["parent_tool_use_id"] = Value::String(p.into());
                    }
                    out.push(line.to_string());
                }
            }
            "assistant" => {
                if v["message"]["model"]
                    .as_str()
                    .is_some_and(|m| m.starts_with('<'))
                    && parent.is_some()
                {
                    continue;
                }
                let mut line = json!({ "type": "assistant", "message": v["message"].take() });
                if let Some(p) = parent {
                    line["parent_tool_use_id"] = Value::String(p.into());
                }
                out.push(line.to_string());
            }
            _ => {}
        }
    }
    out
}

/// La transcripción de un subagente: `<sesión>/subagents/agent-<id>.jsonl` (o en una
/// subcarpeta de `subagents`).
fn agent_transcript(projects: &Path, session: &str, task: &str) -> Option<PathBuf> {
    let dir = projects.join(session).join("subagents");
    let direct = dir.join(format!("agent-{task}.jsonl"));
    if direct.is_file() {
        return Some(direct);
    }
    let name = format!("agent-{task}.jsonl");
    fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path().join(&name))
        .find(|p| p.is_file())
}

/// Toda la conversación guardada del workspace: la del orquestador y, detrás, la de cada
/// agente registrado, cada una entre un `task_started` y un `task_notification` (los agentes
/// de antes ya no están en marcha).
pub(crate) fn history(projects: &Path, session: &str, agents: &[RegAgent]) -> Vec<String> {
    let mut out = transcript_lines(&projects.join(format!("{session}.jsonl")), None, false);
    for agent in agents {
        let Some(tool) = agent.tool_use.as_deref() else {
            continue;
        };
        let own = agent.session.as_deref().unwrap_or(session);
        if !workspace::safe_id(own) || !workspace::safe_id(&agent.task) {
            continue;
        }
        let lines = agent_transcript(projects, own, &agent.task)
            .map(|p| transcript_lines(&p, Some(tool), true))
            .unwrap_or_default();
        out.push(
            json!({
                "type": "system", "subtype": "task_started", "task_id": agent.task, "tool_use_id": tool,
                "description": agent.name, "subagent_type": agent.kind, "task_type": "local_agent",
                "prompt": agent.prompt, "nexo_history": true
            })
            .to_string(),
        );
        out.extend(lines);
        out.push(
            json!({
                "type": "system", "subtype": "task_notification", "task_id": agent.task, "tool_use_id": tool,
                "status": "completed", "summary": "", "nexo_history": true
            })
            .to_string(),
        );
    }
    out
}

// ---------------------------------------------------------------- lector

/// Lo que el lector tiene que mirar de cada línea: los agentes nuevos se registran y los
/// turnos y tareas en segundo plano dicen si el orquestador está haciendo algo.
fn watch_line(line: &str, live: &Live, on_task: &dyn Fn(&Value)) {
    let head = &line[..line.len().min(96)];
    if head.starts_with(r#"{"type":"result""#) {
        live.turn.store(false, Ordering::Relaxed);
    } else if head.starts_with(r#"{"type":"system","subtype":"init""#) {
        live.turn.store(true, Ordering::Relaxed);
    } else if head.contains(r#""subtype":"task_started""#) {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            on_task(&v);
        }
    } else if head.contains(r#""subtype":"background_tasks_changed""#)
        && let Ok(v) = serde_json::from_str::<Value>(line)
    {
        let count = v["tasks"].as_array().map_or(0, |t| t.len());
        live.background.store(count, Ordering::Relaxed);
    }
}

fn idle(live: &Live) -> bool {
    !live.turn.load(Ordering::Relaxed) && live.background.load(Ordering::Relaxed) == 0
}

fn pump(
    stdout: impl Read,
    shared: &Mutex<Shared>,
    live: &Live,
    on_task: &dyn Fn(&Value),
    on_idle: &dyn Fn(),
) {
    for line in BufReader::new(stdout).lines() {
        let Ok(text) = line else { break };
        if text.trim().is_empty() {
            continue;
        }
        watch_line(&text, live, on_task);
        {
            let Ok(mut shared) = shared.lock() else { break };
            remember(&mut shared, &text);
            if let Some(sink) = &shared.sink {
                let _ = sink.send(ChatEvent::Line { text });
            }
        }
        if live.stop_when_idle.load(Ordering::Relaxed) && idle(live) {
            on_idle();
        }
    }
}

// ---------------------------------------------------------------- comandos

#[tauri::command]
pub async fn workspace_start(
    app: AppHandle,
    path: String,
    options: ChatOptions,
    replay: bool,
    channel: Channel<ChatEvent>,
) -> Result<WsStarted, String> {
    let root = ws_root(&app, &path)?;
    // Lanzar puede tardar (el catálogo de extras abre Claude Code un momento): fuera del hilo
    // de la interfaz.
    tauri::async_runtime::spawn_blocking(move || start(&app, &root, options, replay, channel))
        .await
        .map_err(|e| e.to_string())?
}

fn start(
    app: &AppHandle,
    root: &Path,
    options: ChatOptions,
    replay: bool,
    channel: Channel<ChatEvent>,
) -> Result<WsStarted, String> {
    let claude = find_claude(app).ok_or("No encontré Claude Code. Instálalo desde https://claude.com/claude-code y vuelve a intentarlo.")?;
    let projects = projects_dir(app, root)?;
    let mut config = read_config(root);
    let session = match config.session.clone() {
        Some(s) if workspace::safe_id(&s) => s,
        _ => {
            let fresh = uuid::Uuid::new_v4().to_string();
            config = update_config(root, |c| c.session = Some(fresh.clone()))?;
            fresh
        }
    };
    let saved = projects.join(format!("{session}.jsonl")).is_file();

    let state = app.state::<WorkspaceRuns>();
    let mut runs = state.runs.lock().map_err(|e| e.to_string())?;
    let run = runs.entry(root.to_path_buf()).or_default();
    // La primera vez en esta apertura de nexo, la conversación sale de las transcripciones.
    if !run.loaded {
        run.loaded = true;
        if saved {
            let lines = history(&projects, &session, &workspace::agents(root));
            if let Ok(mut shared) = run.shared.lock() {
                for line in lines {
                    remember(&mut shared, &line);
                }
            }
        }
    }
    {
        let mut shared = run.shared.lock().map_err(|e| e.to_string())?;
        if replay {
            for text in &shared.backlog {
                let _ = channel.send(ChatEvent::Line { text: text.clone() });
            }
            // Hasta aquí, lo repetido: lo que llegue después ya es de ahora.
            let _ = channel.send(ChatEvent::Line {
                text: REPLAY_END.into(),
            });
        }
        shared.sink = Some(channel);
    }
    if alive(&mut run.proc) {
        if let Some(p) = &run.proc {
            p.live.stop_when_idle.store(false, Ordering::Relaxed);
        }
        return Ok(WsStarted {
            session,
            resumed: true,
            attached: true,
            applied: run.applied.clone().unwrap_or_default(),
        });
    }
    stop(run.proc.take());

    let catalog = catalog_for(app, root, false).unwrap_or_default();
    let settings = extras_settings(&catalog, &config.extras);
    let plugin = install_plugin(app)?;
    let args = launch_args(
        &session,
        saved,
        &session_name(root),
        &orchestrator_prompt(root),
        &plugin,
        &settings,
        &options,
    )?;

    let mut cmd = Command::new(&claude);
    cmd.args(&args)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in INHERITED_ENV {
        cmd.env_remove(key);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("No se pudo iniciar Claude Code: {e}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or("Claude Code no aceptó la entrada.")?;
    let stdout = child.stdout.take().ok_or("Claude Code no dio salida.")?;
    let stderr = child.stderr.take();
    let errors = Arc::new(Mutex::new(String::new()));
    if let Some(mut stderr) = stderr {
        let errors = errors.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if let Ok(mut text) = errors.lock() {
                    text.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if text.len() > 4000 {
                        let cut = text.len() - 2000;
                        let cut = (cut..text.len())
                            .find(|i| text.is_char_boundary(*i))
                            .unwrap_or(0);
                        text.drain(..cut);
                    }
                }
            }
        });
    }

    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let live = Arc::new(Live::default());
    run.proc = Some(Proc {
        id,
        child,
        stdin,
        live: live.clone(),
    });
    run.applied = Some(config.extras.clone());
    let shared = run.shared.clone();
    drop(runs);

    let handle = app.clone();
    let folder = root.to_path_buf();
    let owner = session.clone();
    std::thread::spawn(move || {
        let on_task = |task: &Value| {
            if let Err(e) = workspace::register_task(&folder, &owner, task) {
                log::warn!("No se pudo registrar el agente: {e}");
            }
        };
        let on_idle = || {
            let state = handle.state::<WorkspaceRuns>();
            let taken = match state.runs.lock() {
                Ok(mut runs) => runs.get_mut(&folder).and_then(|r| {
                    if r.proc.as_ref().is_some_and(|p| p.id == id) {
                        r.proc.take()
                    } else {
                        None
                    }
                }),
                Err(_) => None,
            };
            stop(taken);
        };
        pump(stdout, &shared, &live, &on_task, &on_idle);
        // Fin de la salida: si el proceso sigue siendo el de este workspace, terminó solo.
        let state = handle.state::<WorkspaceRuns>();
        let ended = match state.runs.lock() {
            Ok(mut runs) => runs.get_mut(&folder).and_then(|r| {
                if r.proc.as_ref().is_some_and(|p| p.id == id) {
                    r.proc.take()
                } else {
                    None
                }
            }),
            Err(_) => None,
        };
        let Some(mut ended) = ended else { return };
        let code = ended.child.wait().ok().and_then(|s| s.code());
        let detail = errors
            .lock()
            .map(|t| t.trim().to_string())
            .unwrap_or_default();
        if let Ok(shared) = shared.lock()
            && let Some(sink) = &shared.sink
        {
            let _ = sink.send(ChatEvent::Exit { code, detail });
        }
    });

    Ok(WsStarted {
        session,
        resumed: saved,
        attached: false,
        applied: config.extras,
    })
}

#[tauri::command]
pub async fn workspace_send(
    app: AppHandle,
    state: State<'_, WorkspaceRuns>,
    path: String,
    line: String,
    record: Option<String>,
) -> Result<(), String> {
    let root = ws_root(&app, &path)?;
    let one_line = |text: &str| !text.contains('\n') && !text.trim().is_empty();
    if !one_line(&line) || record.as_deref().is_some_and(|r| !one_line(r)) {
        return Err("Cada mensaje para Claude Code va en una sola línea.".into());
    }
    let mut runs = state.runs.lock().map_err(|e| e.to_string())?;
    let run = runs
        .get_mut(&root)
        .ok_or("El orquestador no está en marcha.")?;
    let proc = run
        .proc
        .as_mut()
        .ok_or("El orquestador no está en marcha.")?;
    proc.stdin
        .write_all(format!("{line}\n").as_bytes())
        .and_then(|_| proc.stdin.flush())
        .map_err(|e| e.to_string())?;
    if line.starts_with(r#"{"type":"user""#) {
        proc.live.turn.store(true, Ordering::Relaxed);
    }
    if let Some(record) = record {
        remember(&mut *run.shared.lock().map_err(|e| e.to_string())?, &record);
    }
    Ok(())
}

/// Cierra el orquestador; su sesión y lo dicho se conservan (`workspace_start` lo retoma con
/// los extras que haya entonces: así se aplica «Reiniciar espacio de Workspace»).
#[tauri::command]
pub fn workspace_stop(
    app: AppHandle,
    state: State<'_, WorkspaceRuns>,
    path: String,
) -> Result<(), String> {
    let root = ws_root(&app, &path)?;
    let taken = state
        .runs
        .lock()
        .map_err(|e| e.to_string())?
        .get_mut(&root)
        .and_then(|r| r.proc.take());
    stop(taken);
    Ok(())
}

/// Conversación nueva con el orquestador: otra sesión. Los agentes registrados se quedan.
#[tauri::command]
pub fn workspace_reset(
    app: AppHandle,
    state: State<'_, WorkspaceRuns>,
    path: String,
) -> Result<(), String> {
    let root = ws_root(&app, &path)?;
    let mut runs = state.runs.lock().map_err(|e| e.to_string())?;
    let run = runs.entry(root.clone()).or_default();
    stop(run.proc.take());
    run.loaded = true;
    if let Ok(mut shared) = run.shared.lock() {
        shared.backlog.clear();
    }
    let fresh = uuid::Uuid::new_v4().to_string();
    update_config(&root, |c| c.session = Some(fresh)).map(|_| ())
}

/// La interfaz deja de mirar este workspace: el orquestador se cierra si no hace nada o en
/// cuanto termine lo que esté haciendo (la sesión se retoma al volver).
#[tauri::command]
pub fn workspace_detach(
    app: AppHandle,
    state: State<'_, WorkspaceRuns>,
    path: String,
) -> Result<(), String> {
    let root = ws_root(&app, &path)?;
    let mut runs = state.runs.lock().map_err(|e| e.to_string())?;
    let Some(run) = runs.get_mut(&root) else {
        return Ok(());
    };
    if let Ok(mut shared) = run.shared.lock() {
        shared.sink = None;
    }
    let quiet = run.proc.as_ref().is_some_and(|p| idle(&p.live));
    if quiet {
        stop(run.proc.take());
    } else if let Some(p) = &run.proc {
        p.live.stop_when_idle.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// Qué orquestadores están en marcha y con qué extras (para «Reiniciar espacio de Workspace»).
#[tauri::command]
pub fn workspace_running(
    app: AppHandle,
    state: State<'_, WorkspaceRuns>,
    path: String,
) -> Result<Option<Extras>, String> {
    let root = ws_root(&app, &path)?;
    let mut runs = state.runs.lock().map_err(|e| e.to_string())?;
    Ok(runs.get_mut(&root).and_then(|r| {
        if alive(&mut r.proc) {
            r.applied.clone()
        } else {
            None
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("nexo-wsrun-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn opts() -> ChatOptions {
        ChatOptions::default()
    }

    #[test]
    fn el_orquestador_arranca_con_su_papel_su_plugin_y_los_extras() {
        let settings = json!({ "enabledPlugins": { "a@b": false } });
        let args = launch_args(
            "u-1",
            false,
            "proyecto · nexo",
            "papel",
            Path::new("/datos/nexo"),
            &settings,
            &opts(),
        )
        .unwrap();
        let at = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .map(|i| args[i + 1].clone())
        };
        assert_eq!(at("--session-id").as_deref(), Some("u-1"));
        assert_eq!(at("--name").as_deref(), Some("proyecto · nexo"));
        assert_eq!(at("--append-system-prompt").as_deref(), Some("papel"));
        assert_eq!(at("--plugin-dir").as_deref(), Some("/datos/nexo"));
        assert_eq!(
            at("--settings").as_deref(),
            Some(r#"{"enabledPlugins":{"a@b":false}}"#)
        );
        assert!(args.contains(&"--forward-subagent-text".to_string()));
        assert!(at("--resume").is_none());
        let resumed = launch_args(
            "u-1",
            true,
            "x",
            "papel",
            Path::new("/p"),
            &settings,
            &opts(),
        )
        .unwrap();
        assert!(
            resumed
                .windows(2)
                .any(|w| w[0] == "--resume" && w[1] == "u-1")
        );
        assert!(!resumed.contains(&"--session-id".to_string()));
        let bad = ChatOptions {
            model: None,
            effort: None,
            permission_mode: Some("bypassPermissions".into()),
        };
        assert!(launch_args("u", false, "x", "p", Path::new("/p"), &settings, &bad).is_err());
    }

    #[test]
    fn el_papel_explica_como_se_quitan_y_se_habla_con_los_agentes() {
        let prompt = orchestrator_prompt(Path::new("/home/ana/proyecto"));
        assert!(prompt.contains("«proyecto»"));
        assert!(prompt.contains(".nexo/agentes/"));
        assert!(prompt.contains("SendMessage"));
        assert!(prompt.contains(TO_AGENT));
        assert!(SKILL.starts_with("---\nname: agentes-del-workspace\n"));
        assert!(SKILL.contains("TaskStop"));
    }

    #[test]
    fn el_lector_sabe_si_el_orquestador_esta_trabajando() {
        let live = Live::default();
        let tasks = std::cell::RefCell::new(Vec::new());
        let on_task = |v: &Value| {
            tasks
                .borrow_mut()
                .push(v["task_id"].as_str().unwrap_or_default().to_string())
        };
        watch_line(
            r#"{"type":"system","subtype":"init","session_id":"s"}"#,
            &live,
            &on_task,
        );
        assert!(!idle(&live));
        watch_line(
            r#"{"type":"system","subtype":"task_started","task_id":"a1","task_type":"local_agent"}"#,
            &live,
            &on_task,
        );
        watch_line(
            r#"{"type":"system","subtype":"background_tasks_changed","tasks":[{"task_id":"a1"}]}"#,
            &live,
            &on_task,
        );
        watch_line(r#"{"type":"result","subtype":"success"}"#, &live, &on_task);
        assert!(!idle(&live), "un agente sigue en segundo plano");
        watch_line(
            r#"{"type":"system","subtype":"background_tasks_changed","tasks":[]}"#,
            &live,
            &on_task,
        );
        assert!(idle(&live));
        assert_eq!(*tasks.borrow(), ["a1"]);
    }

    #[test]
    fn el_lector_registra_y_reenvia_y_cierra_al_quedar_libre() {
        let salida = concat!(
            r#"{"type":"system","subtype":"init"}"#,
            "\n",
            r#"{"type":"stream_event","event":{}}"#,
            "\n",
            r#"{"type":"system","subtype":"task_started","task_id":"t9","task_type":"local_agent"}"#,
            "\n",
            r#"{"type":"result","subtype":"success"}"#,
            "\n",
        );
        let shared = Mutex::new(Shared::default());
        let live = Live::default();
        live.stop_when_idle.store(true, Ordering::Relaxed);
        let seen = std::cell::Cell::new(0);
        let idled = std::cell::Cell::new(0);
        pump(
            salida.as_bytes(),
            &shared,
            &live,
            &|_| seen.set(seen.get() + 1),
            &|| idled.set(idled.get() + 1),
        );
        assert_eq!(seen.get(), 1);
        assert_eq!(idled.get(), 1, "solo al terminar el turno");
        assert_eq!(shared.lock().unwrap().backlog.len(), 3, "sin el streaming");
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
    fn el_historial_sale_de_las_transcripciones() {
        let projects = temp("historial");
        write(
            &projects.join("s1.jsonl"),
            &[
                json!({ "type": "user", "message": { "role": "user", "content": "Revisa la API\n\nArchivos adjuntos (léelos con Read):\n- /p/api.md" } }),
                json!({ "type": "user", "isMeta": true, "message": { "role": "user", "content": "<system-reminder>x</system-reminder>" } }),
                json!({ "type": "user", "message": { "role": "user", "content": "<command-name>/model</command-name>" } }),
                json!({ "type": "assistant", "message": { "id": "m1", "model": "claude-opus-5-5", "content": [{ "type": "tool_use", "id": "toolu_1", "name": "Agent", "input": { "description": "Revisor" } }] } }),
                json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "toolu_1", "content": "hecho" }] } }),
                json!({ "type": "user", "message": { "role": "user", "content": format!("{TO_AGENT}a1] Revisor\nMira también los tests\n\n—\nPásaselo con SendMessage.") } }),
                json!({ "type": "attachment", "attachment": {} }),
                json!({ "type": "summary", "summary": "x" }),
            ],
        );
        write(
            &projects.join("s1/subagents/agent-a1.jsonl"),
            &[
                json!({ "type": "user", "isSidechain": true, "message": { "role": "user", "content": "Encargo: revisa src/api" } }),
                json!({ "type": "assistant", "isSidechain": true, "message": { "id": "m2", "model": "claude-sonnet-5-5", "content": [{ "type": "text", "text": "Reviso." }] } }),
            ],
        );
        let agent = RegAgent {
            file: "revisor-a1".into(),
            name: "Revisor".into(),
            role: "Revisa".into(),
            kind: "general-purpose".into(),
            task: "a1".into(),
            tool_use: Some("toolu_1".into()),
            session: Some("s1".into()),
            created: String::new(),
            prompt: "Encargo: revisa src/api".into(),
        };
        let lines: Vec<Value> = history(&projects, "s1", &[agent])
            .iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let kinds: Vec<String> = lines
            .iter()
            .map(|l| {
                l["subtype"]
                    .as_str()
                    .or(l["type"].as_str())
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "nexo_user",
                "assistant",
                "user",
                "nexo_user",
                "task_started",
                "assistant",
                "task_notification"
            ]
        );
        assert_eq!(lines[0]["text"], "Revisa la API");
        assert_eq!(lines[0]["files"], json!(["/p/api.md"]));
        assert_eq!(lines[3]["text"], "Mira también los tests");
        assert_eq!(lines[3]["to"], "a1");
        assert_eq!(lines[4]["tool_use_id"], "toolu_1");
        assert_eq!(
            lines[5]["parent_tool_use_id"], "toolu_1",
            "el encargo no se repite: ya va en task_started"
        );
        assert_eq!(lines[6]["status"], "completed");
    }
}
