//! Chat con Claude Code: nexo lanza `claude` sin terminal (`-p`, con entrada y salida en
//! `stream-json`) dentro del vault y la interfaz pinta la conversación con su propio diseño.
//! Este módulo solo arranca el proceso, le pasa líneas y reenvía las que devuelve; el
//! protocolo (mensajes, procesos, permisos, cambio de modelo, esfuerzo y modo de permisos)
//! lo interpreta la interfaz (`chat-model.js`). Usa la suscripción de Claude Code; lo que va
//! por la API de Anthropic (compilar, preguntar) sigue en `nexo.rs`.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::vault::vault_root;
use crate::{browser, playwright};

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatEvent {
    /// Una línea JSON del protocolo de Claude Code, tal cual.
    Line { text: String },
    /// Claude Code terminó; `detail` trae el final de su salida de error.
    Exit { code: Option<i32>, detail: String },
}

struct Session {
    id: u64,
    child: Child,
    stdin: ChildStdin,
}

/// Sesión de Claude Code de esta apertura de nexo: se crea al abrir la app y se retoma
/// mientras siga abierta. Al cerrar nexo (o al pedir una conversación nueva), la siguiente
/// empieza limpia.
struct RunSession {
    id: String,
    name: String,
}

/// A dónde van las líneas y lo ya dicho en esta apertura. La interfaz puede recargarse (Vite)
/// con Claude Code en marcha: al volver a conectarse recibe la conversación desde aquí.
#[derive(Default)]
struct Shared {
    sink: Option<Channel<ChatEvent>>,
    backlog: Vec<String>,
}

#[derive(Default)]
pub struct ClaudeCodeState {
    session: Mutex<Option<Session>>,
    run: Mutex<Option<RunSession>>,
    shared: Arc<Mutex<Shared>>,
}

/// Lo que la interfaz elige antes de arrancar (después se cambia con peticiones de control).
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatOptions {
    pub(crate) model: Option<String>,
    pub(crate) effort: Option<String>,
    pub(crate) permission_mode: Option<String>,
}

#[derive(Serialize)]
pub struct Started {
    /// Nombre de la sesión («Nexo · fecha»).
    session: String,
    /// Se retomó una sesión guardada de esta apertura.
    resumed: bool,
    /// Claude Code ya estaba en marcha: la interfaz solo se ha vuelto a conectar.
    attached: bool,
}

/// Líneas que se guardan como mucho para volver a pintar la conversación.
pub(crate) const BACKLOG_MAX: usize = 6000;
pub(crate) const EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];
/// Modos de permiso que ofrece nexo. `bypassPermissions` y `dontAsk` quedan fuera a
/// propósito: Claude Code tiene aquí el navegador con las sesiones del usuario.
pub(crate) const PERMISSION_MODES: &[&str] = &["default", "acceptEdits", "plan", "auto"];

static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Variables que Claude Code pone a lo que lanza (su terminal y sus herramientas). Si nexo se
/// abre desde ahí (`npm run tauri dev` en una sesión de Claude Code) las hereda, y un Claude
/// Code con `CLAUDE_CODE_CHILD_SESSION` se toma por sesión hija: no guarda su transcripción
/// (el registro de agentes no la ve) ni el historial. Se quitan antes de lanzar `claude`.
pub(crate) const INHERITED_ENV: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
    "AI_AGENT",
];

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

/// Papel de Claude Code dentro de nexo: orquestador y quien responde sobre las fuentes.
/// Se añade al prompt de sistema en cada arranque (también al retomar la sesión).
pub(crate) fn orchestrator_prompt(root: &std::path::Path, session: &str) -> String {
    format!(
        "Estás dentro de nexo, un NotebookLM personal. Eres el orquestador: respondes al usuario sobre sus \
fuentes y su wiki, y coordinas el trabajo.\n\n\
Antes de cada tarea, aunque el usuario no nombre ninguna skill ni ningún agente, comprueba si ya hay uno suyo \
que la cubra: sus skills (las tienes listadas con su descripción y cuándo usarlas) y sus agentes (los de \
.claude/agents/, que aparecen en la herramienta Agent; si dudas, lista esa carpeta y lee su frontmatter, \
porque el usuario puede haber creado uno hace un momento). Si encaja una skill, síguela; si encaja un agente, \
encárgale el trabajo con su subagent_type. Dilo en una línea («Uso la skill classroom», «Se lo encargo al \
agente revisor-classroom») y después resume lo que devuelva. Si ninguno encaja, hazlo tú.\n\n\
Vault de Obsidian: {vault}\n\
- raw/: fuentes originales. raw/markdown/ tiene los PDF y documentos convertidos a Markdown con MarkItDown; \
raw/web/ tiene páginas web guardadas por Antigravity. Lee siempre la versión .md en lugar del PDF.\n\
- wiki/fuentes/: una ficha por fuente (nombre de la fuente, resumen, ideas clave, conceptos y la ruta raw/ original). \
Cuando el usuario mencione una fuente por su nombre, búscala primero aquí (por nombre de archivo o título) y \
después abre su Markdown.\n\
- wiki/: notas organizadas por temas, con _master-index.md y un _index.md por tema, enlazadas con [[...]].\n\
- output/: respuestas guardadas.\n\
- wiki/conversaciones/: conclusiones guardadas de conversaciones anteriores, una nota por conversación. Cada \
apertura de nexo empieza una sesión nueva contigo, así que consulta estas notas para recuperar decisiones previas.\n\n\
Grafo: nexo dibuja como nodo cada nota .md de wiki/ y como arista cada enlace [[nota]] (o [texto](nota.md)) \
entre ellas; se actualiza solo en cuanto escribes un archivo. Las notas fuera de wiki/ no aparecen. Cuando el \
usuario te pida añadir algo al grafo o al vault (un nodo, una nota, un tema):\n\
1. Busca primero lo relacionado: lista wiki/ y lee _master-index.md, los _index.md de los temas y las fichas de \
wiki/fuentes/ que vengan al caso. Si ya existe una nota del tema, actualízala en lugar de duplicarla.\n\
2. Crea la nota en wiki/<tema>/<nombre-en-kebab-case>.md (usa una carpeta-tema existente o crea una), con \
frontmatter (type, date: AAAA-MM-DD, tags), un título y el contenido.\n\
3. Relaciónala: termina con ## Relacionado y enlaces [[ruta/sin-extension|Título]] a las notas, fichas y \
conversaciones existentes con las que tenga que ver (enlaza solo notas que existan; un [[enlace]] a algo que no \
existe crea un nodo de palabra clave punteado, úsalo solo a propósito para conceptos). Añade también el enlace de \
vuelta en la sección ## Relacionado de esas notas (créala al final si no está), salvo en las fichas de wiki/fuentes/.\n\
4. Enlázala desde el _index.md de su tema (créalo si no existe, con [[...]] a cada nota) y asegúrate de que \
_master-index.md enlaza ese _index.\n\
5. Al terminar, di qué nodo creaste y con qué nodos lo uniste.\n\n\
Notas (el apartado «Notas» de nexo): las notas del usuario viven en wiki/notas/ y también son nodos del grafo. \
Puedes crearlas, editarlas, moverlas y borrarlas igual que el usuario, estés en el apartado que estés:\n\
- Un tema es una carpeta (kebab-case) con un _index.md cuyo frontmatter es `type: tema` y `title: <nombre visible>`; \
los subtemas son carpetas dentro de carpetas, sin límite de niveles (p. ej. wiki/notas/rutinas/push-pull-legs/pecho/). \
En cada _index.md nexo regenera solo la sección entre <!-- nexo:contenido --> y <!-- /nexo:contenido --> con los \
enlaces a sus subtemas y notas: no la escribas tú; el texto fuera de esa sección (descripción) sí puedes editarlo.\n\
- Una nota es un .md con frontmatter: title (nombre visible), created y updated (AAAA-MM-DD HH:MM; actualiza \
updated al editar); no lleva type. El usuario la escribe en nexo como un bloc de notas por bloques, así que el \
cuerpo va una línea por bloque: texto, títulos (#, ##, ###), casillas «- [ ] elemento» / «- [x] hecho», viñetas \
«- », listas «1. », citas «> », separador «---», bloques de código y tablas Markdown; los subniveles de una lista \
se sangran con tabulador. Para fechas y vencimientos escribe «📅 AAAA-MM-DD» en la línea (p. ej. «- [ ] Pagar la \
renta 📅 2026-10-31»): nexo la muestra como fecha y avisa si vence. Nombre de archivo en kebab-case; no lo \
cambies al renombrar (cambia title), para no romper enlaces.\n\
- Para mostrar notas en el escritorio de notas, edita .nexo/escritorio.json (en la raíz del vault): \
{{\"cards\":[{{\"path\":\"<ruta relativa a wiki/notas/, p. ej. rutinas/pecho/press.md>\",\"x\":24,\"y\":24,\"w\":380,\"h\":440,\"z\":1}}]}} \
(x, y, w, h en píxeles; z es la capa, mayor = encima). El escritorio es un lienzo enorme que ocupa toda la app: \
x e y son posiciones en él y pueden ser negativas. Una carta nueva va a la derecha de la última, a su misma \
altura y con su mismo tamaño (x = su x + su w + 16), para que queden en fila. Conserva las cartas que ya estén \
y los demás campos del archivo (como `view`, lo que se está viendo) y añade o quita solo las que te pidan; la \
app lo aplica al momento y lleva la vista a la carta nueva. Ese archivo es el escritorio que está abierto; los \
demás escritorios y los archivados están en .nexo/escritorios.json, que no debes tocar.\n\
- Historial y seguimiento: si el usuario quiere conservar lo anterior (p. ej. series y cargas de cada día) mientras \
la nota muestra lo más reciente, actualiza la nota con lo nuevo y guarda el registro en notas del grafo fuera de \
notas/ (p. ej. wiki/<tema>/historial/AAAA-MM-DD-<cosa>.md o una nota de progreso con una tabla por fecha), \
enlazadas con [[...]] a la nota y a su índice, siguiendo las reglas del apartado Grafo.\n\
- Las tareas que piden actuar en webs (compras, reservas, búsquedas) se hacen con el navegador de Playwright; \
apunta en la nota lo que hiciste (con fecha, precios, enlaces) y pide confirmación antes de pagar, comprar o enviar.\n\n\
Guardar la conversación: cuando el usuario lo pida (por ejemplo «guarda la conversación en el grafo»), escribe \
wiki/conversaciones/AAAA-MM-DD-<tema-en-kebab-case>.md con frontmatter (type: conversacion, date: AAAA-MM-DD, \
session: {session}), un título, y las secciones ## Conclusiones, ## Decisiones, ## Preguntas abiertas y \
## Relacionado (enlaces [[...]] a las fichas de wiki/fuentes/ y a las notas de la wiki que se trataron: esos \
enlaces la conectan en el grafo). Si ya guardaste esta conversación hoy, actualiza esa misma nota. Añade un \
enlace a la nota en wiki/conversaciones/_index.md (créalo si no existe) y asegúrate de que \
wiki/_master-index.md enlaza [[conversaciones/_index|Conversaciones]]. No modifiques otras notas.\n\n\
Reparto de trabajo: Antigravity (el CLI `agy`) es el encargado de las fuentes: leer páginas web, añadirlas \
y resumirlas. Para añadir una página, pide al usuario que pegue el enlace en el campo de fuentes de nexo \
(así queda registrada con su ficha); no guardes páginas en raw/ tú mismo. Tú no reescribas las fichas de \
wiki/fuentes/: las mantiene nexo. Cita las fuentes por su nombre y ruta, y las notas como [[nombre]].\n\n\
Si alguna vez debes ejecutar agy tú mismo: pasa el prompt como argumento (`agy \"-p=<prompt>\" --output-format json \
--json-schema '<esquema>'`; con -p=- no le llega), en modo no interactivo cualquier herramienta sin permiso aborta \
la tarea, así que solo se añade `read_url(<dominio>)` en ~/.gemini/antigravity-cli/settings.json (nunca read_url(*), \
command(...) ni --dangerously-skip-permissions), y para textos largos copia el contenido a fuente.md en una carpeta \
vacía y pídele que lo lea con view_file.\n\n\
Skills y agentes: las skills del usuario son las de Claude Code de este vault, en \
.claude/skills/<nombre>/SKILL.md (nexo las muestra alrededor de la galaxia y en Agentes → Skills), y sus \
agentes, los de .claude/agents/<nombre>.md (nexo los muestra en Agentes → Agentes con su description como \
resumen, su model y su effort, así que esos tres campos van siempre explícitos). Para crear, cambiar o mejorar \
una skill o un agente delega SIEMPRE en el agente `creador-de-skills` (herramienta Agent con subagent_type \
creador-de-skills; trabaja con Sonnet 5.5 y esfuerzo medio y puede leer el grafo): pásale la petición literal \
del usuario (con el modelo y el esfuerzo que pida) y el contexto que tengas, y no lo escribas tú. Si el agente devuelve preguntas, házselas al usuario y vuelve a llamarlo con \
las respuestas. Cuando el agente termine, di qué skill quedó y cómo se usa. Cuando el usuario invoque una \
skill con /<nombre>, síguela.\n\n\
Workspaces: las carpetas de proyecto del usuario (Agentes → Workspaces) se trabajan con terminales, como en \
Orca. Para trabajar con los workspaces (abrir una terminal, delegar un trabajo a un Claude Code o a un agente del \
vault, ver cómo va o cuánto gasta), sigue la skill nexo-orquestacion: tiene la orden de control de nexo y sus \
reglas.\n\n\
Navegador: tienes el servidor MCP `playwright` (herramientas mcp__playwright__browser_*), que abre un Chrome con \
ventana sobre una copia del perfil del navegador del usuario (sus sesiones iniciadas), cargada con el botón \
«Cargar navegador» de Conexiones. Úsalo cuando el usuario te pida consultar o hacer algo en una web, sobre todo \
si necesita sesión. Como tiene las cuentas reales del usuario: no publiques, compres, envíes mensajes, borres ni \
cambies ajustes sin su confirmación explícita, no muestres contraseñas ni cookies, y cierra el navegador \
(browser_close) al terminar.\n\n\
Formato: tus respuestas se ven en el chat de nexo, que entiende Markdown (párrafos, listas, negritas, \
`código`, bloques de código y tablas sencillas). Sé directo, responde en el idioma del usuario y cita las \
notas como [[nombre-de-la-nota]].",
        vault = root.display(),
        session = session
    )
}

/// Nombre de la sesión de esta apertura, si ya se creó (para Conexiones).
pub fn current_session_name(app: &AppHandle) -> Option<String> {
    let state = app.state::<ClaudeCodeState>();
    let run = state.run.lock().ok()?;
    run.as_ref().map(|r| r.name.clone())
}

/// Id de la sesión de esta apertura (si ya se creó) y si Claude Code está en marcha: el
/// registro de agentes marca así al orquestador activo.
pub fn current_session(app: &AppHandle) -> Option<(String, bool)> {
    let state = app.state::<ClaudeCodeState>();
    let id = state.run.lock().ok()?.as_ref()?.id.clone();
    let running = state.session.lock().is_ok_and(|s| s.is_some());
    Some((id, running))
}

/// Claude Code guarda cada sesión en `~/.claude/projects/<carpeta>/<id>.jsonl`; solo existe
/// cuando ya hubo al menos un mensaje. Sin ese archivo `--resume` fallaría.
fn session_saved(app: &AppHandle, id: &str) -> bool {
    let Ok(home) = app.path().home_dir() else {
        return false;
    };
    let Ok(projects) = std::fs::read_dir(home.join(".claude/projects")) else {
        return false;
    };
    projects
        .flatten()
        .any(|p| p.path().join(format!("{id}.jsonl")).is_file())
}

/// Configuración MCP con Playwright para Claude Code: navegador con ventana sobre el perfil
/// de nexo (el que rellena "Cargar navegador"). `None` si Playwright MCP no está instalado.
fn playwright_mcp_config(app: &AppHandle) -> Option<PathBuf> {
    let server = playwright::find_server(app)?;
    let mut args = browser::mcp_args(app, false).ok()?;
    // Las capturas y instantáneas van a la caché, no al vault.
    let output = app.path().app_cache_dir().ok()?.join("playwright-claude");
    args.extend(["--output-dir".into(), output.to_string_lossy().into_owned()]);
    let config = serde_json::json!({
        "mcpServers": {
            "playwright": { "type": "stdio", "command": server, "args": args }
        }
    });
    let path = browser::profile_dir(app)
        .ok()?
        .with_file_name("claude-mcp.json");
    std::fs::write(&path, serde_json::to_string_pretty(&config).ok()?).ok()?;
    Some(path)
}

fn stop(session: Option<Session>) {
    if let Some(mut s) = session {
        let _ = s.child.kill();
        let _ = s.child.wait();
    }
}

/// Un modelo es un alias (`opus`) o un id (`claude-opus-5-5`, `opus[1m]`): nada que pueda
/// pasar por otra opción de la línea de órdenes.
pub(crate) fn valid_model(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 80
        && !model.starts_with('-')
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.[]:".contains(c))
}

/// Argumentos de `claude` para el chat: sin terminal, con el protocolo en JSON por líneas en
/// los dos sentidos, los avisos de permiso por ese mismo canal y el texto a medida que llega.
fn launch_args(
    run: &RunSession,
    saved: bool,
    prompt: &str,
    mcp_config: Option<&Path>,
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
    // Una sesión por apertura de nexo: nueva la primera vez, retomada después.
    if saved {
        args.extend(["--resume".into(), run.id.clone()]);
    } else {
        args.extend([
            "--session-id".into(),
            run.id.clone(),
            "--name".into(),
            run.name.clone(),
        ]);
    }
    args.extend(["--append-system-prompt".into(), prompt.to_string()]);
    if let Some(config) = mcp_config {
        args.extend(["--mcp-config".into(), config.to_string_lossy().into_owned()]);
    }
    args.extend(option_args(options)?);
    Ok(args)
}

/// Modelo, esfuerzo y modo de permisos elegidos en la interfaz, validados: solo se pasan si
/// el usuario los eligió y nunca un modo que se salte los permisos. Los usa también el
/// orquestador de los workspaces (`workspace_run.rs`).
pub(crate) fn option_args(options: &ChatOptions) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    if let Some(model) = options.model.as_deref().filter(|m| *m != "default") {
        if !valid_model(model) {
            return Err(format!("Modelo no válido: {model}"));
        }
        args.extend(["--model".into(), model.into()]);
    }
    if let Some(effort) = options.effort.as_deref() {
        if !EFFORTS.contains(&effort) {
            return Err(format!("Esfuerzo no válido: {effort}"));
        }
        args.extend(["--effort".into(), effort.into()]);
    }
    if let Some(mode) = options.permission_mode.as_deref() {
        if !PERMISSION_MODES.contains(&mode) {
            return Err(format!("Modo de permisos no válido: {mode}"));
        }
        // «default» es no pasar nada: la opción de la línea de órdenes lo llama de otra forma.
        if mode != "default" {
            args.extend(["--permission-mode".into(), mode.into()]);
        }
    }
    Ok(args)
}

/// Lo que no hace falta para volver a pintar la conversación: los trozos del streaming (el
/// mensaje completo llega después), los hooks y los contadores que van cambiando.
pub(crate) fn keep_in_backlog(line: &str) -> bool {
    const SKIP: &[&str] = &[
        r#"{"type":"stream_event""#,
        r#"{"type":"system","subtype":"hook_"#,
        r#"{"type":"system","subtype":"thinking_tokens""#,
        r#"{"type":"rate_limit_event""#,
    ];
    !SKIP.iter().any(|prefix| line.starts_with(prefix))
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

/// Lee la salida de Claude Code línea a línea hasta que se cierra: guarda cada una para poder
/// repetir la conversación y la reenvía a la interfaz, si hay alguna escuchando.
fn pump(stdout: impl Read, shared: &Mutex<Shared>) {
    for line in BufReader::new(stdout).lines() {
        let Ok(text) = line else { break };
        if text.trim().is_empty() {
            continue;
        }
        let Ok(mut shared) = shared.lock() else { break };
        remember(&mut shared, &text);
        if let Some(sink) = &shared.sink {
            let _ = sink.send(ChatEvent::Line { text });
        }
    }
}

/// Arranca Claude Code en el vault (o se vuelve a conectar al que ya está en marcha) y envía
/// sus líneas por `channel`. Con `replay`, primero repite la conversación de esta apertura:
/// lo pide una interfaz recién cargada, que no la tiene.
#[tauri::command]
pub fn claude_code_start(
    app: AppHandle,
    state: State<'_, ClaudeCodeState>,
    options: ChatOptions,
    replay: bool,
    channel: Channel<ChatEvent>,
) -> Result<Started, String> {
    let root = vault_root(&app)?;
    let claude = find_claude(&app)
        .ok_or("No encontré Claude Code. Instálalo desde https://claude.com/claude-code y vuelve a intentarlo.")?;

    let mut session = state.session.lock().map_err(|e| e.to_string())?;
    let running = session
        .as_mut()
        .is_some_and(|s| matches!(s.child.try_wait(), Ok(None)));
    {
        let mut shared = state.shared.lock().map_err(|e| e.to_string())?;
        if replay {
            for text in &shared.backlog {
                let _ = channel.send(ChatEvent::Line { text: text.clone() });
            }
        }
        shared.sink = Some(channel);
    }
    if running {
        let name = current_session_name(&app).unwrap_or_default();
        return Ok(Started {
            session: name,
            resumed: true,
            attached: true,
        });
    }
    stop(session.take());

    let (args, name, resumed) = {
        let mut run = state.run.lock().map_err(|e| e.to_string())?;
        let run = run.get_or_insert_with(|| RunSession {
            id: uuid::Uuid::new_v4().to_string(),
            name: format!("Nexo · {}", chrono::Local::now().format("%Y-%m-%d %H:%M")),
        });
        let saved = session_saved(&app, &run.id);
        // El agente que escribe las skills tiene que estar antes de arrancar: Claude Code lee
        // los agentes del vault al empezar la sesión.
        if let Err(e) = crate::skills::install_agent(&root) {
            log::warn!("No se pudo instalar el agente de skills: {e}");
        }
        // La skill de los workspaces, también antes: Claude Code lee las skills al empezar. Sin
        // ella el chat funciona igual, así que solo se avisa.
        if let Err(e) = std::env::current_exe()
            .map_err(|e| e.to_string())
            .and_then(|exe| crate::orquestacion::install(&root, &exe).map_err(|e| e.to_string()))
        {
            log::warn!(
                "No se pudo instalar la skill {}: {e}",
                crate::orquestacion::NAME
            );
        }
        let prompt = orchestrator_prompt(&root, &run.name);
        let config = playwright_mcp_config(&app);
        (
            launch_args(run, saved, &prompt, config.as_deref(), &options)?,
            run.name.clone(),
            saved,
        )
    };

    let mut cmd = Command::new(&claude);
    cmd.args(&args)
        .current_dir(&root)
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

    // Lo último que escriba por la salida de error explica por qué terminó, si termina.
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

    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    *session = Some(Session { id, child, stdin });
    drop(session);

    // Lector en su propio hilo. Si la interfaz ya no escucha (se recargó), se sigue leyendo:
    // dejar de leer llenaría la tubería y bloquearía a Claude Code.
    let shared = state.shared.clone();
    let handle = app.clone();
    std::thread::spawn(move || {
        pump(stdout, &shared);
        // Fin de la salida: Claude Code terminó. Si la sesión ya es otra (se detuvo o se
        // reinició a propósito), no hay nada que avisar.
        let state = handle.state::<ClaudeCodeState>();
        let ended = match state.session.lock() {
            Ok(mut guard) if guard.as_ref().is_some_and(|s| s.id == id) => guard.take(),
            _ => None,
        };
        let Some(mut ended) = ended else { return };
        let code = ended.child.wait().ok().and_then(|status| status.code());
        let detail = errors
            .lock()
            .map(|t| t.trim().to_string())
            .unwrap_or_default();
        if let Ok(shared) = shared.lock() {
            if let Some(sink) = &shared.sink {
                let _ = sink.send(ChatEvent::Exit { code, detail });
            }
        }
    });

    Ok(Started {
        session: name,
        resumed,
        attached: false,
    })
}

/// Escribe una línea del protocolo (un mensaje del usuario, una petición o una respuesta de
/// control). `record` es lo que se guarda para volver a pintar la conversación: lo que
/// escribe el usuario y lo que contesta a los permisos no vuelve por la salida de Claude
/// Code, así que la interfaz lo deja anotado en su propio formato.
#[tauri::command]
pub async fn claude_code_send(
    state: State<'_, ClaudeCodeState>,
    line: String,
    record: Option<String>,
) -> Result<(), String> {
    let one_line = |text: &str| !text.contains('\n') && !text.trim().is_empty();
    if !one_line(&line) || record.as_deref().is_some_and(|r| !one_line(r)) {
        return Err("Cada mensaje para Claude Code va en una sola línea.".into());
    }
    {
        let mut guard = state.session.lock().map_err(|e| e.to_string())?;
        let session = guard.as_mut().ok_or("Claude Code no está en marcha.")?;
        session
            .stdin
            .write_all(format!("{line}\n").as_bytes())
            .and_then(|_| session.stdin.flush())
            .map_err(|e| e.to_string())?;
    }
    if let Some(record) = record {
        remember(
            &mut *state.shared.lock().map_err(|e| e.to_string())?,
            &record,
        );
    }
    Ok(())
}

/// Detiene Claude Code; la sesión de esta apertura se conserva y se retoma al volver a abrir.
#[tauri::command]
pub fn claude_code_stop(state: State<'_, ClaudeCodeState>) -> Result<(), String> {
    stop(state.session.lock().map_err(|e| e.to_string())?.take());
    Ok(())
}

/// Conversación nueva: detiene Claude Code y olvida la sesión y lo dicho en esta apertura.
#[tauri::command]
pub fn claude_code_reset(state: State<'_, ClaudeCodeState>) -> Result<(), String> {
    stop(state.session.lock().map_err(|e| e.to_string())?.take());
    *state.run.lock().map_err(|e| e.to_string())? = None;
    state
        .shared
        .lock()
        .map_err(|e| e.to_string())?
        .backlog
        .clear();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_prompt_explica_notas_y_escritorio() {
        let prompt = orchestrator_prompt(Path::new("/vault"), "Nexo · prueba");
        assert!(prompt.contains("wiki/notas/"));
        assert!(prompt.contains(r#"{"cards":[{"path":"#));
        assert!(prompt.contains("lienzo enorme"));
        assert!(prompt.contains(".nexo/escritorios.json, que no debes tocar"));
        assert!(prompt.contains("<!-- nexo:contenido -->"));
        assert!(prompt.contains("📅 AAAA-MM-DD"));
    }

    #[test]
    fn el_prompt_dice_que_el_chat_entiende_markdown() {
        let prompt = orchestrator_prompt(Path::new("/vault"), "Nexo · prueba");
        assert!(prompt.contains("el chat de nexo, que entiende Markdown"));
    }

    fn run() -> RunSession {
        RunSession {
            id: "0b1c".into(),
            name: "Nexo · prueba".into(),
        }
    }

    fn pos(args: &[String], flag: &str) -> Option<usize> {
        args.iter().position(|a| a == flag)
    }

    #[test]
    fn los_argumentos_piden_el_protocolo_sin_terminal() {
        let args = launch_args(&run(), false, "papel", None, &ChatOptions::default()).unwrap();
        assert_eq!(args[0], "-p");
        for (flag, value) in [
            ("--input-format", "stream-json"),
            ("--output-format", "stream-json"),
            ("--permission-prompt-tool", "stdio"),
            ("--session-id", "0b1c"),
            ("--name", "Nexo · prueba"),
            ("--append-system-prompt", "papel"),
        ] {
            let at = pos(&args, flag).unwrap_or_else(|| panic!("falta {flag}"));
            assert_eq!(args[at + 1], value);
        }
        assert!(pos(&args, "--include-partial-messages").is_some());
        // Sin elegir nada, manda la configuración de Claude Code del usuario.
        for flag in [
            "--model",
            "--effort",
            "--permission-mode",
            "--resume",
            "--mcp-config",
        ] {
            assert!(pos(&args, flag).is_none(), "sobra {flag}");
        }
    }

    #[test]
    fn el_papel_manda_a_la_skill_de_los_workspaces() {
        let prompt = orchestrator_prompt(Path::new("/v"), "s");
        assert!(prompt.contains("sigue la skill nexo-orquestacion"));
        assert!(!prompt.contains("repertorio"));
    }

    #[test]
    fn una_sesion_guardada_se_retoma() {
        let args = launch_args(&run(), true, "papel", None, &ChatOptions::default()).unwrap();
        assert_eq!(args[pos(&args, "--resume").unwrap() + 1], "0b1c");
        assert!(pos(&args, "--session-id").is_none());
        assert!(pos(&args, "--name").is_none());
    }

    #[test]
    fn modelo_esfuerzo_y_permisos_se_pasan_si_son_validos() {
        let options = ChatOptions {
            model: Some("claude-sonnet-5-5".into()),
            effort: Some("xhigh".into()),
            permission_mode: Some("acceptEdits".into()),
        };
        let config = Path::new("/cache/claude-mcp.json");
        let args = launch_args(&run(), false, "papel", Some(config), &options).unwrap();
        assert_eq!(
            args[pos(&args, "--model").unwrap() + 1],
            "claude-sonnet-5-5"
        );
        assert_eq!(args[pos(&args, "--effort").unwrap() + 1], "xhigh");
        assert_eq!(
            args[pos(&args, "--permission-mode").unwrap() + 1],
            "acceptEdits"
        );
        assert_eq!(
            args[pos(&args, "--mcp-config").unwrap() + 1],
            "/cache/claude-mcp.json"
        );

        // «default» (modelo o permisos) es no pasar la opción.
        let options = ChatOptions {
            model: Some("default".into()),
            effort: None,
            permission_mode: Some("default".into()),
        };
        let args = launch_args(&run(), false, "papel", None, &options).unwrap();
        assert!(pos(&args, "--model").is_none());
        assert!(pos(&args, "--permission-mode").is_none());
    }

    #[test]
    fn no_se_puede_saltar_los_permisos_ni_colar_opciones() {
        let with = |model: Option<&str>, effort: Option<&str>, mode: Option<&str>| {
            let options = ChatOptions {
                model: model.map(String::from),
                effort: effort.map(String::from),
                permission_mode: mode.map(String::from),
            };
            launch_args(&run(), false, "papel", None, &options)
        };
        assert!(with(None, None, Some("bypassPermissions")).is_err());
        assert!(with(None, None, Some("dontAsk")).is_err());
        assert!(with(Some("--dangerously-skip-permissions"), None, None).is_err());
        assert!(with(Some("opus --dangerously-skip-permissions"), None, None).is_err());
        assert!(with(None, Some("enorme"), None).is_err());
        assert!(with(Some("opus[1m]"), Some("max"), Some("plan")).is_ok());
    }

    #[test]
    fn el_historial_no_guarda_el_streaming() {
        assert!(!keep_in_backlog(r#"{"type":"stream_event","event":{}}"#));
        assert!(!keep_in_backlog(
            r#"{"type":"system","subtype":"hook_started"}"#
        ));
        assert!(!keep_in_backlog(
            r#"{"type":"system","subtype":"thinking_tokens"}"#
        ));
        assert!(!keep_in_backlog(r#"{"type":"rate_limit_event"}"#));
        assert!(keep_in_backlog(r#"{"type":"assistant","message":{}}"#));
        assert!(keep_in_backlog(
            r#"{"type":"user","message":{"role":"user","content":"hola"}}"#
        ));
        assert!(keep_in_backlog(
            r#"{"type":"control_request","request_id":"1"}"#
        ));
        assert!(keep_in_backlog(r#"{"type":"system","subtype":"init"}"#));
        // Lo que anota la interfaz: el mensaje del usuario y su respuesta a un permiso.
        assert!(keep_in_backlog(
            r#"{"type":"nexo_user","text":"hola","files":[]}"#
        ));
        assert!(keep_in_backlog(
            r#"{"type":"nexo_permission","request_id":"1","decision":"allow"}"#
        ));
    }

    #[test]
    fn el_lector_sigue_aunque_nadie_escuche() {
        let salida = concat!(
            r#"{"type":"system","subtype":"init"}"#,
            "\n\n",
            r#"{"type":"stream_event","event":{}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"año"}]}}"#,
            "\n",
            r#"{"type":"result","subtype":"success"}"#,
        );
        let shared = Mutex::new(Shared::default());
        pump(salida.as_bytes(), &shared);
        let backlog = &shared.lock().unwrap().backlog;
        assert_eq!(backlog.len(), 3, "sin la línea vacía ni el streaming");
        assert!(backlog[1].contains("año"));
        assert!(backlog[2].contains("result"));
    }

    /// Con un proceso de verdad: lo que se escribe en su entrada vuelve por el lector.
    #[cfg(unix)]
    #[test]
    fn una_linea_escrita_vuelve_por_el_lector() {
        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("cat");
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let line = r#"{"type":"user","message":{"role":"user","content":"¿qué hay en la wiki?"}}"#;
        stdin.write_all(format!("{line}\n").as_bytes()).unwrap();
        drop(stdin);
        let shared = Mutex::new(Shared::default());
        pump(stdout, &shared);
        assert_eq!(shared.lock().unwrap().backlog, vec![line.to_string()]);
        assert!(child.wait().unwrap().success());
    }

    #[test]
    fn el_historial_tiene_tope() {
        let mut shared = Shared::default();
        for i in 0..BACKLOG_MAX + 25 {
            remember(&mut shared, &format!(r#"{{"type":"assistant","n":{i}}}"#));
        }
        assert_eq!(shared.backlog.len(), BACKLOG_MAX);
        assert!(shared.backlog[0].contains(r#""n":25"#));
    }
}
