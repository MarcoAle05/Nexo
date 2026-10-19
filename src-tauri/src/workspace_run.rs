//! El orquestador de cada workspace: un Claude Code sin terminal (como el chat de la carta
//! derecha, `claude_code.rs`) que arranca en la carpeta del workspace con su propia sesión,
//! que se retoma al volver a abrirlo. Despliega los agentes con la herramienta Agent de Claude
//! Code; lo que hace cada uno llega por el mismo protocolo (con `parent_tool_use_id`) y la
//! interfaz lo reparte en sus ventanas (`ws-stream.js`).
//!
//! - Cada workspace tiene su proceso, su historial en memoria y a quién se lo manda. Al
//!   cambiar de workspace la interfaz se desconecta (`workspace_detach`): si el orquestador
//!   no está haciendo nada se cierra (la sesión se retoma al volver) y, si está trabajando, se
//!   cierra cuando termine él, sus agentes en segundo plano y el turno que Claude Code abre
//!   solo al acabar un agente.
//! - Al leer la salida, cada `task_started` de un subagente se registra en
//!   `.nexo/agentes/` (`workspace::register_task`): pase lo que pase en la interfaz, todo
//!   agente desplegado queda en su workspace.
//! - Al abrir un workspace con una sesión guardada, su conversación (y la de sus agentes) se
//!   lee de las transcripciones de Claude Code y se le repite a la interfaz (`history`).
//! - Lo que el orquestador sabe de nexo va en `--append-system-prompt` y en una skill de un
//!   plugin propio cargado con `--plugin-dir` (fuera de la carpeta del usuario).
//! - El lector lleva también lo que espera respuesta (los permisos sin contestar) y si hay
//!   trabajo: cada cambio sale en el evento global `workspace-attention` (lo oye la interfaz
//!   esté donde esté) y lo que pide o termina avisa al escritorio (`avisos.rs`).

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::{Map, Value, json};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::avisos::{self, Mirando, Suceso};

use crate::claude_code::{
    BACKLOG_MAX, ChatEvent, ChatOptions, INHERITED_ENV, find_claude, keep_in_backlog, option_args,
};
use crate::workspace::{
    self, Catalog, Extras, RegAgent, catalog_or_fallback, extras_settings, hooks_running,
    read_config, update_config, ws_root,
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
    /// Peticiones `can_use_tool` sin contestar, por `request_id`.
    pending: Mutex<HashMap<String, Pending>>,
    /// El último `result` fue un error.
    failed: AtomicBool,
    /// Terminó una tarea en segundo plano y Claude Code va a empezar solo otro turno: entre el
    /// `background_tasks_changed` vacío y su `init` no está libre (no se avisa dos veces).
    follow_up: AtomicBool,
    /// Lo último que se contó a la interfaz (`workspace-attention`).
    told: Mutex<Attention>,
    /// nexo lo cerró a propósito: lo que llegue después no avisa y el final no es una caída.
    closed: AtomicBool,
}

/// Una petición sin contestar: quién la hace (el `agent_id`; sin él, el orquestador) y con qué
/// herramienta. Nunca su entrada.
#[derive(Debug, Clone)]
struct Pending {
    agent: Option<String>,
    tool: String,
}

/// El estado de atención de un workspace: cuántas peticiones esperan respuesta y si hay
/// trabajo en marcha.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Attention {
    waiting: usize,
    busy: bool,
}

impl Attention {
    fn free(&self) -> bool {
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

/// Lo que el lector cuenta de cada línea: un estado nuevo para la interfaz o algo que avisar.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Cambio {
    Estado(Attention),
    Suceso(Suceso),
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
    /// Qué workspace tiene la interfaz en pantalla y si la ventana tiene el foco
    /// (`workspace_watch`): de eso no se avisa.
    watch: Mutex<Mirando>,
}

#[derive(Serialize)]
pub struct WsStarted {
    session: String,
    resumed: bool,
    attached: bool,
    applied: Extras,
}

/// Cierra el proceso. Si seguía vivo, lo cerró nexo a propósito: no es una caída, lo que quede
/// por leer ya no avisa y el workspace deja de esperar (devuelve su estado nuevo, si cambió,
/// para contárselo a la interfaz). Uno que ya había terminado lo cuenta su lector.
fn stop(proc: Option<Proc>) -> Option<Attention> {
    let mut p = proc?;
    let was_alive = matches!(p.child.try_wait(), Ok(None));
    if was_alive {
        p.live.closed.store(true, Ordering::Relaxed);
    }
    let _ = p.child.kill();
    let _ = p.child.wait();
    if was_alive { settle(&p.live) } else { None }
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

/// Lo que el orquestador tiene que saber de los extras: qué está encendido en el workspace,
/// qué no lo está y cómo se enciende (sin eso busca en `~/.claude`, ve lo instalado y no
/// entiende por qué no lo tiene).
pub(crate) fn extras_prompt(catalog: &Catalog, on: &Extras) -> String {
    let named = |name: &str, lit: bool| (name.to_string(), lit);
    let running: Vec<&str> = hooks_running(catalog, on)
        .iter()
        .map(|h| h.id.as_str())
        .collect();
    let groups: [(&str, Vec<(String, bool)>); 6] = [
        (
            "repertorio",
            catalog
                .repertoire
                .iter()
                .map(|i| {
                    if i.kind == "plugin" {
                        named(&i.name, on.plugins.contains(&i.id))
                    } else if i.kind == "mcp" {
                        named(&format!("MCP {}", i.name), on.mcp.contains(&i.id))
                    } else if i.kind == "profile-skill" {
                        named(&i.name, on.skills.contains(&i.id))
                    } else {
                        named(
                            &format!("repertorio:{}", i.id),
                            on.repertoire.contains(&i.id),
                        )
                    }
                })
                .collect(),
        ),
        (
            "plugins",
            catalog
                .plugins
                .iter()
                .map(|p| named(&p.name, on.plugins.contains(&p.id)))
                .collect(),
        ),
        (
            "servidores MCP",
            catalog
                .mcp
                .iter()
                .map(|s| named(&s.name, on.mcp.contains(&s.name)))
                .collect(),
        ),
        (
            "conexiones de claude.ai",
            catalog
                .connectors
                .iter()
                .map(|s| {
                    let name = s.name.strip_prefix("claude.ai ").unwrap_or(&s.name);
                    named(name, on.connectors.contains(&s.name))
                })
                .collect(),
        ),
        (
            "skills",
            catalog
                .skills
                .iter()
                .map(|s| named(&s.name, on.skills.contains(&s.name)))
                .collect(),
        ),
        (
            "hooks",
            catalog
                .hooks
                .iter()
                .map(|h| {
                    let name = if h.added {
                        h.name.clone()
                    } else {
                        // «los de tu perfil (orca · 13 eventos)»
                        format!("{} ({})", h.name.to_lowercase(), h.detail)
                    };
                    (name, running.contains(&h.id.as_str()))
                })
                .collect(),
        ),
    ];
    let list = |lit: bool| {
        let parts: Vec<String> = groups
            .iter()
            .filter_map(|(label, items)| {
                let names: Vec<&str> = items
                    .iter()
                    .filter(|(_, o)| *o == lit)
                    .map(|(n, _)| n.as_str())
                    .collect();
                (!names.is_empty()).then(|| format!("{label}: {}", names.join(", ")))
            })
            .collect();
        if parts.is_empty() {
            "nada".to_string()
        } else {
            parts.join("; ")
        }
    };
    let hooks_note = if catalog.hooks.is_empty() {
        ""
    } else {
        " Los hooks van juntos: Claude Code no los separa, así que con uno encendido corren también los de \
tu perfil y los de la carpeta."
    };
    format!(
        "Extras del workspace: nexo arranca Claude Code solo con lo que trae de serie (sus skills, agentes y \
herramientas) y la skill de nexo. De lo que el usuario tiene en su perfil global y en su repertorio de nexo (plugins, servidores MCP, \
conexiones de claude.ai, skills propias y hooks) solo carga lo que él encendió para esta carpeta.\n\
- Encendido: {on}.\n\
- Apagado (no está cargado: no lo puedes usar): {off}.\n\
Si el usuario te pide usar o activar algo apagado, no lo instales ni lo busques en ~/.claude ni cambies la \
configuración de Claude Code: dile que lo encienda en Extras, en la carta izquierda de nexo, y que pulse \
«Reiniciar espacio de Workspace» (vuelves a arrancar con eso cargado y la conversación sigue).{hooks_note} Si te \
pregunta qué skills, plugins, MCP o hooks tienes, contesta con los que tienes cargados en esta sesión y, si viene \
al caso, nombra los apagados.",
        on = list(true),
        off = list(false),
    )
}

/// Lo que lleva el orquestador de un workspace además de las opciones del chat.
pub(crate) struct Setup {
    /// Su papel, con los extras.
    pub prompt: String,
    /// Los ajustes que apagan lo que no está encendido (`--settings`).
    pub settings: Value,
    /// `--plugin-dir`: el plugin de nexo y, con skills del repertorio encendidas, el suyo.
    pub plugins: Vec<PathBuf>,
    /// `--mcp-config` con los MCP del repertorio encendidos, si hay alguno.
    pub mcp: Option<PathBuf>,
}

/// Prepara el arranque del orquestador de un workspace. El catálogo se vuelve a sondear con
/// `fresh`: lo instalado hace un momento también debe arrancar apagado.
pub(crate) fn launch_setup(app: &AppHandle, root: &Path, fresh: bool) -> Result<Setup, String> {
    let catalog = catalog_or_fallback(app, root, fresh);
    let on = read_config(root).extras;
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    launch_setup_in(&data, &home, root, &catalog, &on)
}

/// La preparación sin runtime de Tauri permite comprobar los argumentos con el CLI real
/// sin crear ventanas ni tocar la configuración de nexo.
fn launch_setup_in(
    data: &Path,
    home: &Path,
    root: &Path,
    catalog: &Catalog,
    on: &Extras,
) -> Result<Setup, String> {
    let prompt = format!(
        "{}\n\n{}",
        orchestrator_prompt(root),
        extras_prompt(catalog, on)
    );
    let mut plugins = vec![install_plugin_in(data)?];
    if let Some(dir) =
        repertoire_plugin_in(data, &workspace::repertoire_dir(home), root, catalog, on)?
    {
        plugins.push(dir);
    }
    let mcp = repertoire_mcp_in(data, &workspace::repertoire_mcp(home), root, catalog, on)?;
    Ok(Setup {
        prompt,
        settings: extras_settings(catalog, on),
        plugins,
        mcp,
    })
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
fn install_plugin_in(data: &Path) -> Result<PathBuf, String> {
    let dir = data.join("workspace-plugin").join("nexo");
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

/// Prepara el plugin de las skills sueltas elegidas, fuera de la carpeta del usuario.
fn repertoire_plugin_in(
    data: &Path,
    source: &Path,
    root: &Path,
    catalog: &Catalog,
    on: &Extras,
) -> Result<Option<PathBuf>, String> {
    // El registro y el arranque pueden prepararlo a la vez.
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().map_err(|e| e.to_string())?;
    let dir = data
        .join("repertorio")
        .join(crate::registro::project_dir_name(root))
        .join("repertorio");
    let skills = dir.join("skills");
    if skills.exists() {
        // remove_dir_all borra los enlaces, sin seguirlos hasta las skills originales.
        fs::remove_dir_all(&skills).map_err(|e| e.to_string())?;
    }
    let selected: Vec<_> = catalog
        .repertoire
        .iter()
        .filter(|i| i.kind == "skill" && workspace::safe_id(&i.id) && on.repertoire.contains(&i.id))
        .collect();
    if selected.is_empty() {
        return Ok(None);
    }
    fs::create_dir_all(dir.join(".claude-plugin")).map_err(|e| e.to_string())?;
    fs::create_dir_all(&skills).map_err(|e| e.to_string())?;
    fs::write(
        dir.join(".claude-plugin/plugin.json"),
        json!({
            "name": "repertorio", "version": env!("CARGO_PKG_VERSION"),
            "description": "Skills del repertorio que el usuario encendió para este workspace."
        })
        .to_string(),
    )
    .map_err(|e| e.to_string())?;
    for item in selected {
        let from = source.join(&item.id);
        let to = skills.join(&item.id);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&from, &to).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&from, &to).map_err(|e| e.to_string())?;
    }
    Ok(Some(dir))
}

/// Deja en `<datos>/repertorio/<carpeta>/mcp.json` los MCP del repertorio encendidos en el
/// workspace (solo los del catálogo, con su configuración tal cual: puede llevar claves, así
/// que solo lo lee el usuario). Sin ninguno encendido, no hay archivo.
fn repertoire_mcp_in(
    data: &Path,
    servers: &Map<String, Value>,
    root: &Path,
    catalog: &Catalog,
    on: &Extras,
) -> Result<Option<PathBuf>, String> {
    let file = data
        .join("repertorio")
        .join(crate::registro::project_dir_name(root))
        .join("mcp.json");
    let chosen: Map<String, Value> = catalog
        .repertoire
        .iter()
        .filter(|i| i.kind == "mcp" && on.mcp.contains(&i.id))
        .filter_map(|i| Some((i.id.clone(), servers.get(&i.id)?.clone())))
        .collect();
    if chosen.is_empty() {
        if file.exists() {
            fs::remove_file(&file).map_err(|e| e.to_string())?;
        }
        return Ok(None);
    }
    let text =
        serde_json::to_string(&json!({ "mcpServers": chosen })).map_err(|e| e.to_string())?;
    workspace::write_private(&file, &text)?;
    Ok(Some(file))
}

/// Argumentos del orquestador de un workspace: los del chat de nexo más su papel, el plugin
/// de nexo (y el del repertorio), sus MCP y los ajustes que apagan los extras.
fn launch_args(
    session: &str,
    saved: bool,
    name: &str,
    setup: &Setup,
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
    args.extend(["--append-system-prompt".into(), setup.prompt.clone()]);
    for plugin in &setup.plugins {
        args.extend(["--plugin-dir".into(), plugin.to_string_lossy().into_owned()]);
    }
    if let Some(mcp) = &setup.mcp {
        args.extend(["--mcp-config".into(), mcp.to_string_lossy().into_owned()]);
    }
    args.extend(["--settings".into(), setup.settings.to_string()]);
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

/// Lo que el lector tiene que mirar de cada línea: los agentes nuevos se registran, los
/// turnos y tareas en segundo plano dicen si el orquestador está haciendo algo y las
/// peticiones de permiso, si espera respuesta. Devuelve la petición si es nueva.
fn watch_line(line: &str, live: &Live, on_task: &dyn Fn(&Value)) -> Option<Suceso> {
    let head = &line[..line.len().min(96)];
    if let Some(result) = turn_result(line) {
        live.turn.store(false, Ordering::Relaxed);
        live.follow_up.store(false, Ordering::Relaxed);
        live.failed
            .store(result["is_error"] == true, Ordering::Relaxed);
    } else if head.starts_with(r#"{"type":"system","subtype":"init""#) {
        live.turn.store(true, Ordering::Relaxed);
        live.follow_up.store(false, Ordering::Relaxed);
    } else if head.starts_with(r#"{"type":"control_request""#) {
        return asked(line, live);
    } else if head.starts_with(r#"{"type":"control_cancel_request""#) {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            forget(live, v["request_id"].as_str());
        }
    } else if head.contains(r#""subtype":"task_started""#) {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            on_task(&v);
        }
    } else if head.contains(r#""subtype":"task_notification""#) {
        live.follow_up.store(true, Ordering::Relaxed);
    } else if head.contains(r#""subtype":"background_tasks_changed""#)
        && let Ok(v) = serde_json::from_str::<Value>(line)
    {
        let count = v["tasks"].as_array().map_or(0, |t| t.len());
        live.background.store(count, Ordering::Relaxed);
    }
    None
}

/// La línea que cierra un turno. Claude Code no la empieza por `type` como las demás (empieza
/// por `{"duration_api_ms":…` y `"type":"result"` va hacia el final; visto con 2.1.301 y
/// 2.1.302), así que se busca el campo y se comprueba que es el de fuera y no uno anidado.
fn turn_result(line: &str) -> Option<Value> {
    if !line.contains(r#""type":"result""#) {
        return None;
    }
    serde_json::from_str::<Value>(line)
        .ok()
        .filter(|v| v["type"] == "result")
}

/// Una petición de control: si es un permiso (`can_use_tool`, también las preguntas y los
/// planes) que no se tenía, queda pendiente. Repetida, no cuenta otra vez.
fn asked(line: &str, live: &Live) -> Option<Suceso> {
    let v: Value = serde_json::from_str(line).ok()?;
    let request = &v["request"];
    if request["subtype"] != "can_use_tool" {
        return None;
    }
    let id = v["request_id"].as_str()?;
    let pending = Pending {
        agent: request["agent_id"].as_str().map(String::from),
        tool: request["tool_name"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    };
    let mut all = live.pending.lock().ok()?;
    if all.contains_key(id) {
        return None;
    }
    all.insert(id.to_string(), pending.clone());
    Some(Suceso::Pide {
        agent: pending.agent,
        tool: pending.tool,
    })
}

fn forget(live: &Live, id: Option<&str>) {
    if let (Some(id), Ok(mut all)) = (id, live.pending.lock()) {
        all.remove(id);
    }
}

/// Lo que la interfaz le manda al proceso y cambia su estado: un mensaje abre un turno y la
/// respuesta a un permiso (`control_response`) lo quita de los pendientes.
fn sent(line: &str, live: &Live) {
    if line.starts_with(r#"{"type":"user""#) {
        live.turn.store(true, Ordering::Relaxed);
    } else if line.starts_with(r#"{"type":"control_response""#)
        && let Ok(v) = serde_json::from_str::<Value>(line)
    {
        forget(live, v["response"]["request_id"].as_str());
    }
}

fn idle(live: &Live) -> bool {
    !live.turn.load(Ordering::Relaxed) && live.background.load(Ordering::Relaxed) == 0
}

fn attention(live: &Live) -> Attention {
    Attention {
        waiting: live.pending.lock().map_or(0, |p| p.len()),
        busy: !idle(live) || live.follow_up.load(Ordering::Relaxed),
    }
}

/// Compara el estado con lo último que se contó y, si cambió, lo apunta y devuelve lo de antes
/// y lo de ahora. Mientras no cambia no se cuenta nada.
fn check(live: &Live) -> Option<(Attention, Attention)> {
    let now = attention(live);
    let mut told = live.told.lock().ok()?;
    if *told == now {
        return None;
    }
    Some((std::mem::replace(&mut *told, now), now))
}

/// El proceso se cerró: no queda turno, ni tareas, ni nada que esperar.
fn settle(live: &Live) -> Option<Attention> {
    if let Ok(mut all) = live.pending.lock() {
        all.clear();
    }
    live.turn.store(false, Ordering::Relaxed);
    live.background.store(0, Ordering::Relaxed);
    live.follow_up.store(false, Ordering::Relaxed);
    check(live).map(|(_, now)| now)
}

/// Al acabarse la salida: si no lo cerró nexo y había trabajo o algo esperando, Claude Code se
/// cayó con trabajo a medias. En cualquier caso, el workspace deja de esperar.
fn finish(live: &Live) -> (bool, Option<Attention>) {
    let fell = !live.closed.load(Ordering::Relaxed) && !attention(live).free();
    (fell, settle(live))
}

#[cfg(test)]
fn pump(
    stdout: impl Read,
    shared: &Mutex<Shared>,
    live: &Live,
    on_task: &dyn Fn(&Value),
    on_idle: &dyn Fn(),
) {
    pump_changes(stdout, shared, live, on_task, &|_| {}, on_idle);
}

/// El lector: guarda y reenvía cada línea, registra los agentes y cuenta lo que cambia
/// (`on_change`). Un orquestador desconectado avisa de que terminó antes de cerrarse.
fn pump_changes(
    stdout: impl Read,
    shared: &Mutex<Shared>,
    live: &Live,
    on_task: &dyn Fn(&Value),
    on_change: &dyn Fn(Cambio),
    on_idle: &dyn Fn(),
) {
    for line in BufReader::new(stdout).lines() {
        let Ok(text) = line else { break };
        if text.trim().is_empty() {
            continue;
        }
        let asked = watch_line(&text, live, on_task);
        {
            let Ok(mut shared) = shared.lock() else { break };
            remember(&mut shared, &text);
            if let Some(sink) = &shared.sink {
                let _ = sink.send(ChatEvent::Line { text });
            }
        }
        // Fuera del cerrojo de `shared`: contar el estado y avisar no frena a la interfaz.
        if !live.closed.load(Ordering::Relaxed) {
            if let Some((before, now)) = check(live) {
                on_change(Cambio::Estado(now));
                if !before.free() && now.free() {
                    let error = live.failed.load(Ordering::Relaxed);
                    on_change(Cambio::Suceso(Suceso::Termino { error }));
                }
            }
            if let Some(suceso) = asked {
                on_change(Cambio::Suceso(suceso));
            }
        }
        // Desconectado, se cierra cuando no queda nada: tampoco el turno que Claude Code abre
        // solo al acabar un agente (`follow_up`), que es donde el orquestador cuenta lo que
        // hizo. El aviso de fin ya salió arriba, al pasar a libre.
        if live.stop_when_idle.load(Ordering::Relaxed) && attention(live).free() {
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
    if let Some(a) = stop(run.proc.take()) {
        tell(app, root, a);
    }

    let setup = launch_setup(app, root, true)?;
    let args = launch_args(&session, saved, &session_name(root), &setup, &options)?;

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
            if let Some(a) = stop(taken) {
                tell(&handle, &folder, a);
            }
        };
        let escritorio = avisos::Escritorio(handle.clone());
        let notify = |suceso: &Suceso| {
            let state = handle.state::<WorkspaceRuns>();
            let mirando = state.watch.lock().map(|m| m.clone()).unwrap_or_default();
            avisos::avisar(&escritorio, &mirando, &folder, suceso);
        };
        let on_change = |cambio: Cambio| match cambio {
            Cambio::Estado(a) => tell(&handle, &folder, a),
            Cambio::Suceso(s) => notify(&s),
        };
        pump_changes(stdout, &shared, &live, &on_task, &on_change, &on_idle);
        let (fell, settled) = finish(&live);
        if fell {
            notify(&Suceso::Cayo);
        }
        if let Some(a) = settled {
            tell(&handle, &folder, a);
        }
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
    sent(&line, &proc.live);
    let changed = check(&proc.live);
    if let Some(record) = record {
        remember(&mut *run.shared.lock().map_err(|e| e.to_string())?, &record);
    }
    drop(runs);
    if let Some((_, now)) = changed {
        tell(&app, &root, now);
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
    // Parar a mano no avisa: solo se cuenta que ya no espera nada.
    if let Some(a) = stop(taken) {
        tell(&app, &root, a);
    }
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
    if let Some(a) = stop(run.proc.take()) {
        tell(&app, &root, a);
    }
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
    let quiet = run.proc.as_ref().is_some_and(|p| attention(&p.live).free());
    if quiet {
        if let Some(a) = stop(run.proc.take()) {
            tell(&app, &root, a);
        }
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

/// El estado de atención de todos los workspaces con proceso, para la interfaz recién cargada
/// (también tras una recarga de Vite); después se entera por `workspace-attention`.
#[tauri::command]
pub fn workspace_attention(state: State<'_, WorkspaceRuns>) -> Result<Vec<WsAttention>, String> {
    let mut runs = state.runs.lock().map_err(|e| e.to_string())?;
    Ok(runs
        .iter_mut()
        .filter_map(|(root, run)| {
            if !alive(&mut run.proc) {
                return None;
            }
            let a = attention(&run.proc.as_ref()?.live);
            Some(WsAttention {
                path: root.display().to_string(),
                waiting: a.waiting,
                busy: a.busy,
            })
        })
        .collect())
}

/// Qué workspace tiene la interfaz en pantalla (`None` fuera de un workspace) y si la ventana
/// tiene el foco: de ese workspace no se avisa mientras se mira.
#[tauri::command]
pub fn workspace_watch(
    app: AppHandle,
    state: State<'_, WorkspaceRuns>,
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

    fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("nexo-wsrun-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn opts() -> ChatOptions {
        ChatOptions::default()
    }

    fn setup(prompt: &str, plugins: &[&str], settings: Value) -> Setup {
        Setup {
            prompt: prompt.into(),
            settings,
            plugins: plugins.iter().map(PathBuf::from).collect(),
            mcp: None,
        }
    }

    #[test]
    fn el_orquestador_arranca_con_su_papel_su_plugin_y_los_extras() {
        let settings = json!({ "enabledPlugins": { "a@b": false } });
        let args = launch_args(
            "u-1",
            false,
            "proyecto · nexo",
            &setup("papel", &["/datos/nexo"], settings.clone()),
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
        assert!(at("--mcp-config").is_none());
        let resumed = launch_args(
            "u-1",
            true,
            "x",
            &setup("papel", &["/p"], settings.clone()),
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
        assert!(launch_args("u", false, "x", &setup("p", &["/p"], settings), &bad).is_err());
    }

    #[test]
    fn los_argumentos_admiten_dos_carpetas_de_plugins_y_los_mcp_del_repertorio() {
        let mut both = setup("papel", &["/datos/nexo", "/datos/repertorio"], json!({}));
        both.mcp = Some(PathBuf::from("/datos/mcp.json"));
        let args = launch_args("u", false, "prueba", &both, &opts()).unwrap();
        let paths: Vec<_> = args
            .windows(2)
            .filter(|w| w[0] == "--plugin-dir")
            .map(|w| w[1].as_str())
            .collect();
        assert_eq!(paths, ["/datos/nexo", "/datos/repertorio"]);
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--mcp-config" && w[1] == "/datos/mcp.json")
        );
    }

    #[cfg(unix)]
    #[test]
    fn el_plugin_del_repertorio_solo_enlaza_skills_del_catalogo_y_retira_las_viejas() {
        use crate::workspace::RepertoireItem;
        let root = temp("repertorio");
        let data = root.join("datos");
        let source = root.join("origen");
        for id in ["alfa", "beta"] {
            fs::create_dir_all(source.join(id)).unwrap();
            fs::write(
                source.join(id).join("SKILL.md"),
                format!("---\nname: {id}\n---\n"),
            )
            .unwrap();
        }
        let item = |id: &str| RepertoireItem {
            kind: "skill".into(),
            id: id.into(),
            name: id.into(),
            description: String::new(),
            skills: 1,
            servers: 0,
            hooks: false,
            transport: String::new(),
            status: String::new(),
            source: String::new(),
        };
        let catalog = Catalog {
            repertoire: vec![item("alfa"), item("beta"), item("../fuera")],
            ..Catalog::default()
        };
        let on = Extras {
            repertoire: vec!["alfa".into(), "desconocida".into(), "../fuera".into()],
            ..Extras::default()
        };
        let dir = repertoire_plugin_in(&data, &source, &root, &catalog, &on)
            .unwrap()
            .unwrap();
        assert_eq!(
            fs::read_link(dir.join("skills/alfa")).unwrap(),
            source.join("alfa")
        );
        assert_eq!(fs::read_dir(dir.join("skills")).unwrap().count(), 1);
        let manifest: Value = serde_json::from_str(
            &fs::read_to_string(dir.join(".claude-plugin/plugin.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["name"], "repertorio");
        let on = Extras {
            repertoire: vec!["beta".into()],
            ..Extras::default()
        };
        repertoire_plugin_in(&data, &source, &root, &catalog, &on).unwrap();
        assert!(!dir.join("skills/alfa").exists());
        assert!(
            source.join("alfa/SKILL.md").is_file(),
            "no se sigue el enlace al borrar"
        );
        assert!(dir.join("skills/beta/SKILL.md").is_file());
        assert!(
            repertoire_plugin_in(&data, &source, &root, &catalog, &Extras::default())
                .unwrap()
                .is_none()
        );
        assert!(!dir.join("skills").exists());
        assert!(source.join("beta/SKILL.md").is_file());
        let prompt = extras_prompt(&catalog, &on);
        assert!(prompt.contains("- Encendido: repertorio: repertorio:beta."));
        assert!(prompt.contains("repertorio:alfa"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn los_mcp_del_repertorio_van_por_un_archivo_propio_solo_con_los_encendidos() {
        use crate::workspace::RepertoireItem;
        let root = temp("repertorio-mcp");
        let data = root.join("datos");
        let item = |id: &str| RepertoireItem {
            kind: "mcp".into(),
            id: id.into(),
            name: id.into(),
            description: String::new(),
            skills: 0,
            servers: 1,
            hooks: false,
            transport: "stdio".into(),
            status: String::new(),
            source: String::new(),
        };
        let catalog = Catalog {
            repertoire: vec![item("rea"), item("github")],
            ..Catalog::default()
        };
        let servers = json!({
            "rea": {"command": "rea", "env": {"JAVA_HOME": "/opt/java"}},
            "github": {"type": "http", "url": "https://x"},
            "fuera": {"command": "nada"}
        });
        let servers = servers.as_object().unwrap();
        let on = Extras {
            mcp: vec!["rea".into(), "fuera".into()],
            ..Extras::default()
        };
        let file = repertoire_mcp_in(&data, servers, &root, &catalog, &on)
            .unwrap()
            .unwrap();
        let written: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        // Solo el encendido que está en el catálogo, con su configuración tal cual.
        assert_eq!(written, json!({"mcpServers": {"rea": servers["rea"]}}));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(
            repertoire_mcp_in(&data, servers, &root, &catalog, &Extras::default())
                .unwrap()
                .is_none()
        );
        assert!(!file.exists());
        let prompt = extras_prompt(&catalog, &on);
        assert!(prompt.contains("- Encendido: repertorio: MCP rea."));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "Usa Claude Code y el repertorio reales; solo initialize, sin turnos del modelo"]
    fn el_arranque_real_carga_solo_el_repertorio_encendido() {
        use std::sync::mpsc;
        use std::time::Duration;
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let root = temp("repertorio-real");
        let catalog = workspace::fallback_catalog(&home, &root, "prueba sin sondeo".into());
        assert!(
            catalog
                .repertoire
                .iter()
                .any(|i| i.id == "interfaces@interfaces")
        );
        assert!(
            catalog
                .repertoire
                .iter()
                .any(|i| i.id == "reverse-engineer-anything")
        );
        let project = home
            .join(".claude/projects")
            .join(crate::registro::project_dir_name(&root));
        // Los comandos que da `initialize` y los MCP de `--mcp-config` que da `mcp_status`.
        let probe = |on: &Extras| -> (Vec<String>, Vec<String>) {
            let setup = launch_setup_in(&root.join("datos"), &home, &root, &catalog, on).unwrap();
            let mut args = launch_args(
                &uuid::Uuid::new_v4().to_string(),
                false,
                "Prueba repertorio",
                &setup,
                &opts(),
            )
            .unwrap();
            args.push("--no-session-persistence".into());
            let mut command = Command::new("claude");
            // También fuera de Orca/Claude, sin heredar una sesión hija.
            for (key, _) in
                std::env::vars_os().filter(|(k, _)| k.to_string_lossy().starts_with("CLAUDE"))
            {
                command.env_remove(key);
            }
            let mut child = command
                .args(args)
                .current_dir(&root)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let mut stdin = child.stdin.take().unwrap();
            let stdout = child.stdout.take().unwrap();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Ok(v) = serde_json::from_str::<Value>(&line)
                        && v["type"] == "control_response"
                        && tx.send(v).is_err()
                    {
                        break;
                    }
                }
            });
            for (id, subtype) in [("prueba", "initialize"), ("prueba-mcp", "mcp_status")] {
                let request =
                    json!({"type":"control_request","request_id":id,"request":{"subtype":subtype}});
                writeln!(stdin, "{request}").unwrap();
            }
            stdin.flush().unwrap();
            let mut got: HashMap<String, Value> = HashMap::new();
            while got.len() < 2 {
                let Ok(v) = rx.recv_timeout(Duration::from_secs(40)) else {
                    break;
                };
                if let Some(id) = v["response"]["request_id"].as_str() {
                    got.insert(id.to_string(), v["response"].clone());
                }
            }
            let _ = child.kill();
            let _ = child.wait();
            // Solo se elimina si está vacía; nunca una transcripción del usuario.
            if project.exists() {
                fs::remove_dir(&project).expect("la carpeta de la sonda debe estar vacía");
                println!("Eliminada carpeta vacía: {}", project.display());
            }
            let answer = got.get("prueba").expect("initialize no contestó");
            assert_eq!(answer["subtype"], "success");
            let commands = answer["response"]["commands"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|c| c["name"].as_str().map(str::to_owned))
                .collect();
            let servers =
                got.get("prueba-mcp").expect("mcp_status no contestó")["response"]["mcpServers"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|s| s["source"] == "dynamic")
                    .filter_map(|s| s["name"].as_str().map(str::to_owned))
                    .collect();
            (commands, servers)
        };
        // Un MCP del repertorio, si ya hay alguno (`~/nexo/repertorio/.mcp.json`).
        let server = catalog
            .repertoire
            .iter()
            .find(|i| i.kind == "mcp")
            .map(|i| i.id.clone());
        let on = Extras {
            plugins: vec!["interfaces@interfaces".into()],
            repertoire: vec!["reverse-engineer-anything".into()],
            mcp: server.iter().cloned().collect(),
            ..Extras::default()
        };
        let (commands, servers) = probe(&on);
        assert!(commands.iter().any(|c| c == "interfaces:better-ui"));
        assert!(
            commands
                .iter()
                .any(|c| c == "repertorio:reverse-engineer-anything")
        );
        assert_eq!(servers, server.iter().cloned().collect::<Vec<_>>());
        let (commands, servers) = probe(&Extras::default());
        assert!(
            !commands
                .iter()
                .any(|c| c.starts_with("interfaces:") || c.starts_with("repertorio:"))
        );
        assert!(servers.is_empty());
        println!(
            "Claude Code: interfaces:better-ui, repertorio:reverse-engineer-anything y el MCP {server:?} presentes al encenderlos; ausentes con todo apagado."
        );
        fs::remove_dir_all(root).unwrap();
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
    fn el_orquestador_sabe_que_extras_tiene_y_como_se_encienden_los_demas() {
        use crate::workspace::{PluginItem, ServerItem, SkillItem};
        let server = |name: &str| ServerItem {
            name: name.into(),
            kind: "http".into(),
            detail: String::new(),
            status: String::new(),
            movable: false,
        };
        let plugin = |id: &str, name: &str| PluginItem {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            skills: 3,
            servers: 0,
            synced: false,
            hooks: false,
            movable: false,
        };
        let catalog = Catalog {
            plugins: vec![
                plugin("interfaces@interfaces", "interfaces"),
                plugin("data@synced", "data"),
            ],
            mcp: vec![server("github")],
            connectors: vec![server("claude.ai Mobbin")],
            skills: vec![SkillItem {
                name: "pdf".into(),
                description: String::new(),
                source: "claude.ai".into(),
                movable: false,
            }],
            hooks: vec![],
            error: None,
            ..Catalog::default()
        };
        let none = extras_prompt(&catalog, &Extras::default());
        assert!(none.contains("- Encendido: nada."));
        assert!(none.contains(
            "- Apagado (no está cargado: no lo puedes usar): plugins: interfaces, data; servidores MCP: github; \
conexiones de claude.ai: Mobbin; skills: pdf."
        ));
        assert!(none.contains("Extras, en la carta izquierda de nexo"));
        assert!(none.contains("«Reiniciar espacio de Workspace»"));
        assert!(none.contains("~/.claude"));
        let some = Extras {
            plugins: vec!["data@synced".into()],
            connectors: vec!["claude.ai Mobbin".into()],
            ..Extras::default()
        };
        let prompt = extras_prompt(&catalog, &some);
        assert!(prompt.contains("- Encendido: plugins: data; conexiones de claude.ai: Mobbin."));
        assert!(prompt.contains("plugins: interfaces; servidores MCP: github; skills: pdf."));
        assert!(
            !prompt.contains("Los hooks van juntos"),
            "sin hooks, nada que explicar"
        );
    }

    #[test]
    fn el_orquestador_sabe_que_hooks_corren_de_verdad() {
        use crate::workspace::HookItem;
        let hook = |id: &str, name: &str, detail: &str, added| HookItem {
            id: id.into(),
            name: name.into(),
            detail: detail.into(),
            added,
            settings: Value::Null,
        };
        let catalog = Catalog {
            hooks: vec![
                hook("rtk", "rtk", "Reescribe…", true),
                hook("perfil", "Los de tu perfil", "orca · 13 eventos", false),
            ],
            ..Catalog::default()
        };
        let none = extras_prompt(&catalog, &Extras::default());
        assert!(none.contains("- Encendido: nada."));
        assert!(none.contains("hooks: rtk, los de tu perfil (orca · 13 eventos)."));
        assert!(none.contains("Los hooks van juntos"));
        let rtk = Extras {
            hooks: vec!["rtk".into()],
            ..Extras::default()
        };
        // Con rtk encendido, los del perfil también corren: el orquestador lo sabe.
        assert!(
            extras_prompt(&catalog, &rtk)
                .contains("- Encendido: hooks: rtk, los de tu perfil (orca · 13 eventos).")
        );
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

    // ------------------------------------------------------------ avisos (spec de avisos)

    /// Un permiso con la forma real de Claude Code (`agent_id` si lo pide un agente).
    fn permiso(id: &str, tool: &str, agent: Option<&str>) -> String {
        let mut request = json!({ "subtype": "can_use_tool", "tool_name": tool,
            "input": { "command": "rm -rf /tmp/x" }, "tool_use_id": format!("toolu_{id}") });
        if let Some(a) = agent {
            request["agent_id"] = json!(a);
        }
        format!(r#"{{"type":"control_request","request_id":"{id}","request":{request}}}"#)
    }

    /// Las líneas de `allowLine` y `denyLine` (`src/chat-model.js`), tal cual las manda la
    /// interfaz por `workspace_send`.
    fn allow_line(id: &str) -> String {
        format!(
            r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"{id}","response":{{"behavior":"allow","updatedInput":{{}}}}}}}}"#
        )
    }

    fn deny_line(id: &str) -> String {
        format!(
            r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"{id}","response":{{"behavior":"deny","message":"El usuario no dio permiso."}}}}}}"#
        )
    }

    fn waiting(live: &Live) -> usize {
        attention(live).waiting
    }

    /// Una línea del fixture compacta (se guardó con espacios), con `type` y `subtype` delante.
    /// Ojo: así son casi todas las de Claude Code, pero no la de `result`, que los lleva hacia
    /// el final; eso lo prueba `el_final_del_turno_se_ve_aunque_type_no_vaya_delante`.
    fn compact(line: &str) -> String {
        let v: Value = serde_json::from_str(line).unwrap();
        let mut out = format!(r#"{{"type":{}"#, v["type"]);
        if !v["subtype"].is_null() {
            out += &format!(r#","subtype":{}"#, v["subtype"]);
        }
        for (k, val) in v.as_object().unwrap() {
            if k != "type" && k != "subtype" {
                out += &format!(",{}:{val}", Value::String(k.clone()));
            }
        }
        out + "}"
    }

    fn fixture() -> String {
        include_str!("../../tests/fixtures/workspace/agente-en-segundo-plano.jsonl")
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| compact(l) + "\n")
            .collect()
    }

    /// Pasa las líneas por el lector y apunta lo que cuenta (y cuándo se cerraría).
    fn leer(salida: &str, live: &Live) -> Vec<String> {
        let shared = Mutex::new(Shared::default());
        let seen = std::cell::RefCell::new(Vec::new());
        pump_changes(
            salida.as_bytes(),
            &shared,
            live,
            &|_| {},
            &|c| {
                seen.borrow_mut().push(match c {
                    Cambio::Estado(a) => format!("estado {} {}", a.waiting, a.busy),
                    Cambio::Suceso(s) => format!("{s:?}"),
                })
            },
            &|| seen.borrow_mut().push("cierra".into()),
        );
        seen.into_inner()
    }

    fn avisos_de(seen: &[String]) -> Vec<&String> {
        seen.iter().filter(|s| !s.starts_with("estado")).collect()
    }

    /// El final de un turno con los campos en el orden en que lo escribe Claude Code (visto
    /// con 2.1.302 en un turno real): `type` va hacia el final.
    const RESULT_REAL: &str = r#"{"duration_api_ms":3810,"stop_reason":"end_turn","session_id":"s1","is_error":false,"num_turns":2,"type":"result","subtype":"success"}"#;

    #[test]
    fn el_final_del_turno_se_ve_aunque_type_no_vaya_delante() {
        let live = Live::default();
        let none = |_: &Value| {};
        watch_line(r#"{"type":"system","subtype":"init"}"#, &live, &none);
        // Un `"type":"result"` anidado en otra línea no cierra el turno.
        watch_line(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"result","text":"x"}]}}"#,
            &live,
            &none,
        );
        assert!(!idle(&live));
        watch_line(RESULT_REAL, &live, &none);
        assert!(idle(&live));
        assert!(!live.failed.load(Ordering::Relaxed));
        watch_line(
            &RESULT_REAL.replace(r#""is_error":false"#, r#""is_error":true"#),
            &live,
            &none,
        );
        assert!(live.failed.load(Ordering::Relaxed));
    }

    #[test]
    fn desconectado_espera_al_turno_que_abre_claude_code_al_acabar_un_agente() {
        // El orden de un turno real (2.1.302): lanza un agente en segundo plano y termina; al
        // acabar el agente, Claude Code abre solo otro turno para contarlo.
        let salida = [
            r#"{"type":"system","subtype":"init"}"#,
            r#"{"type":"system","subtype":"background_tasks_changed","tasks":[{"id":"t1"}]}"#,
            r#"{"type":"system","subtype":"task_started","task_id":"t1","task_type":"local_agent"}"#,
            RESULT_REAL,
            r#"{"type":"system","subtype":"task_updated","task_id":"t1","patch":{"status":"completed"}}"#,
            r#"{"type":"system","subtype":"task_notification","task_id":"t1","status":"completed"}"#,
            r#"{"type":"system","subtype":"background_tasks_changed","tasks":[]}"#,
            r#"{"type":"system","subtype":"init"}"#,
            RESULT_REAL,
        ]
        .join("\n");
        let live = Live::default();
        live.stop_when_idle.store(true, Ordering::Relaxed);
        let seen = leer(&salida, &live);
        let at = seen.iter().position(|s| s == "cierra").unwrap();
        // No se cierra con el agente acabado y el turno de después por venir: solo al final.
        assert_eq!(seen[at..].iter().filter(|s| *s == "cierra").count(), 1);
        assert_eq!(avisos_de(&seen[..at]), ["Termino { error: false }"]);
        assert_eq!(seen[at - 1], "Termino { error: false }");
        assert_eq!(seen.len(), at + 1, "se cierra con la última línea");
    }

    #[test]
    fn el_lector_cuenta_los_permisos_sin_contestar() {
        let live = Live::default();
        let none = |_: &Value| {};
        watch_line(r#"{"type":"system","subtype":"init"}"#, &live, &none);
        let first = watch_line(&permiso("r1", "Bash", None), &live, &none);
        assert_eq!(
            first,
            Some(Suceso::Pide {
                agent: None,
                tool: "Bash".into()
            })
        );
        assert_eq!(
            watch_line(&permiso("r1", "Bash", None), &live, &none),
            None,
            "repetida no cuenta otra vez"
        );
        let agent = watch_line(&permiso("r2", "Edit", Some("a1")), &live, &none);
        assert_eq!(
            agent,
            Some(Suceso::Pide {
                agent: Some("a1".into()),
                tool: "Edit".into()
            })
        );
        // Otras peticiones de control no esperan respuesta del usuario.
        watch_line(
            r#"{"type":"control_request","request_id":"r3","request":{"subtype":"mcp_status"}}"#,
            &live,
            &none,
        );
        assert_eq!(waiting(&live), 2);
        assert!(!idle(&live), "una petición pendiente deja el turno abierto");
        assert_eq!(
            attention(&live),
            Attention {
                waiting: 2,
                busy: true
            }
        );
    }

    #[test]
    fn un_permiso_contestado_o_cancelado_deja_de_esperar() {
        let live = Live::default();
        let none = |_: &Value| {};
        for (id, tool) in [("r1", "Bash"), ("r2", "Write"), ("r3", "AskUserQuestion")] {
            watch_line(&permiso(id, tool, None), &live, &none);
        }
        assert_eq!(waiting(&live), 3);
        sent(&allow_line("r1"), &live);
        assert_eq!(waiting(&live), 2);
        sent(&deny_line("r2"), &live);
        assert_eq!(waiting(&live), 1);
        watch_line(
            r#"{"type":"control_cancel_request","request_id":"r3"}"#,
            &live,
            &none,
        );
        assert_eq!(waiting(&live), 0);
        sent(&allow_line("nunca-vista"), &live);
        assert_eq!(waiting(&live), 0);
    }

    #[test]
    fn al_cerrarse_el_proceso_no_espera_nada() {
        let live = Live::default();
        let none = |_: &Value| {};
        watch_line(r#"{"type":"system","subtype":"init"}"#, &live, &none);
        watch_line(&permiso("r1", "Bash", None), &live, &none);
        check(&live);
        let (_, settled) = finish(&live);
        assert_eq!(
            settled,
            Some(Attention {
                waiting: 0,
                busy: false
            })
        );
        assert_eq!(waiting(&live), 0);
        assert!(idle(&live));
        assert_eq!(finish(&live).1, None, "ya estaba contado");
    }

    #[test]
    fn solo_se_emite_cuando_cambia_el_estado() {
        let salida = [
            r#"{"type":"system","subtype":"init"}"#.to_string(),
            r#"{"type":"stream_event","event":{}}"#.to_string(),
            permiso("r1", "Bash", None),
            permiso("r1", "Bash", None),
            r#"{"type":"stream_event","event":{}}"#.to_string(),
            r#"{"type":"control_cancel_request","request_id":"r1"}"#.to_string(),
            r#"{"type":"result","subtype":"success","is_error":false}"#.to_string(),
            r#"{"type":"stream_event","event":{}}"#.to_string(),
        ]
        .join("\n");
        let live = Live::default();
        let seen = leer(&salida, &live);
        let estados: Vec<&String> = seen.iter().filter(|s| s.starts_with("estado")).collect();
        assert_eq!(
            estados,
            [
                "estado 0 true",
                "estado 1 true",
                "estado 0 true",
                "estado 0 false"
            ]
        );
        // Lo que manda la interfaz también cuenta una sola vez.
        let live = Live::default();
        watch_line(&permiso("r9", "Bash", None), &live, &|_| {});
        assert!(check(&live).is_some());
        sent(&allow_line("r9"), &live);
        assert!(check(&live).is_some());
        assert!(check(&live).is_none());
    }

    #[test]
    fn avisa_una_vez_al_quedar_libre_y_no_en_cada_turno() {
        // Un turno real que lanza un agente en segundo plano y termina; al acabar el agente,
        // Claude Code abre solo otro turno. Es un único trabajo: un aviso, al final.
        let live = Live::default();
        let seen = leer(&fixture(), &live);
        assert_eq!(avisos_de(&seen), ["Termino { error: false }"]);
        assert_eq!(seen.last().unwrap(), "Termino { error: false }");
        // Desconectado se cierra al quedar libre, y avisa antes de cerrarse.
        let live = Live::default();
        live.stop_when_idle.store(true, Ordering::Relaxed);
        let seen = leer(&fixture(), &live);
        let at = seen.iter().position(|s| s == "cierra").unwrap();
        assert_eq!(avisos_de(&seen[..at]), ["Termino { error: false }"]);
    }

    #[test]
    fn un_turno_con_error_avisa_del_error() {
        let salida = concat!(
            r#"{"type":"system","subtype":"init"}"#,
            "\n",
            r#"{"type":"result","subtype":"error_during_execution","is_error":true}"#,
            "\n",
            r#"{"type":"system","subtype":"init"}"#,
            "\n",
            r#"{"type":"result","subtype":"success","is_error":false}"#,
            "\n",
        );
        let seen = leer(salida, &Live::default());
        assert_eq!(
            avisos_de(&seen),
            ["Termino { error: true }", "Termino { error: false }"]
        );
    }

    #[test]
    fn parar_el_orquestador_a_mano_no_avisa() {
        let mut child = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let live = Arc::new(Live::default());
        let proc = Proc {
            id: 0,
            child,
            stdin,
            live: live.clone(),
        };
        let antes = format!(
            "{}\n{}\n",
            r#"{"type":"system","subtype":"init"}"#,
            permiso("r1", "Bash", None)
        );
        leer(&antes, &live);
        assert_eq!(
            stop(Some(proc)),
            Some(Attention {
                waiting: 0,
                busy: false
            }),
            "se cuenta que ya no espera"
        );
        // Lo que quedaba por leer tras pararlo no avisa, y el final no es una caída.
        let despues = format!(
            "{}\n{}\n",
            permiso("r2", "Bash", None),
            r#"{"type":"result","subtype":"success"}"#
        );
        assert!(leer(&despues, &live).is_empty());
        assert_eq!(finish(&live), (false, None));
    }

    #[test]
    fn si_claude_code_se_cae_con_trabajo_avisa() {
        let live = Live::default();
        leer(
            &format!(
                "{}\n{}\n",
                r#"{"type":"system","subtype":"init"}"#,
                permiso("r1", "Bash", None)
            ),
            &live,
        );
        let (fell, settled) = finish(&live);
        assert!(fell);
        assert_eq!(
            settled,
            Some(Attention {
                waiting: 0,
                busy: false
            })
        );
        // Con el trabajo terminado, cerrarse no es una caída.
        let live = Live::default();
        leer(&fixture(), &live);
        assert_eq!(finish(&live), (false, None));
    }

    #[test]
    fn un_aviso_que_falla_no_para_el_lector() {
        struct Falla(std::cell::Cell<usize>);
        impl avisos::Aviso for Falla {
            fn enviar(&self, _: &avisos::Texto) -> Result<(), String> {
                self.0.set(self.0.get() + 1);
                Err("sin servicio de avisos".into())
            }
        }
        let root = avisos::tests::temp("falla");
        let falla = Falla(std::cell::Cell::new(0));
        let shared = Mutex::new(Shared::default());
        let live = Live::default();
        let estados = std::cell::Cell::new(0);
        let salida = format!(
            "{}\n{}\n{}\n{}\n",
            r#"{"type":"system","subtype":"init"}"#,
            permiso("r1", "Bash", None),
            permiso("r2", "Read", Some("a1")),
            r#"{"type":"result","subtype":"success"}"#
        );
        pump_changes(
            salida.as_bytes(),
            &shared,
            &live,
            &|_| {},
            &|c| match c {
                Cambio::Estado(_) => estados.set(estados.get() + 1),
                Cambio::Suceso(s) => avisos::avisar(&falla, &Mirando::default(), &root, &s),
            },
            &|| {},
        );
        assert_eq!(falla.0.get(), 2, "los dos permisos se intentaron avisar");
        assert_eq!(shared.lock().unwrap().backlog.len(), 4, "y se leyó todo");
        assert_eq!(estados.get(), 4);
        assert_eq!(waiting(&live), 2);
    }
}
