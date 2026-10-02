//! Terminal real dentro de nexo que ejecuta Claude Code (`claude`) en la carpeta del vault.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::vault::vault_root;
use crate::{browser, playwright};

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

/// Sesión de Claude Code de esta apertura de nexo: se crea la primera vez que se abre la
/// pestaña y se retoma mientras la app siga abierta. Al cerrar nexo, la siguiente empieza limpia.
struct RunSession {
    id: String,
    name: String,
}

#[derive(Default)]
pub struct ClaudeCodeState {
    session: Mutex<Option<Session>>,
    run: Mutex<Option<RunSession>>,
}

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

/// Papel de Claude Code dentro de nexo: orquestador y quien responde sobre las fuentes.
/// Se añade al prompt de sistema en cada arranque (también al retomar la sesión).
fn orchestrator_prompt(root: &std::path::Path, session: &str) -> String {
    format!(
        "Estás dentro de nexo, un NotebookLM personal. Eres el orquestador: respondes al usuario sobre sus \
fuentes y su wiki, y coordinas el trabajo.\n\n\
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
Navegador: tienes el servidor MCP `playwright` (herramientas mcp__playwright__browser_*), que abre un Chrome con \
ventana sobre una copia del perfil del navegador del usuario (sus sesiones iniciadas), cargada con el botón \
«Cargar navegador» de Conexiones. Úsalo cuando el usuario te pida consultar o hacer algo en una web, sobre todo \
si necesita sesión. Como tiene las cuentas reales del usuario: no publiques, compres, envíes mensajes, borres ni \
cambies ajustes sin su confirmación explícita, no muestres contraseñas ni cookies, y cierra el navegador \
(browser_close) al terminar.",
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

    stop(state.session.lock().map_err(|e| e.to_string())?.take());

    let pty = native_pty_system()
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;
    // Una sesión por apertura de nexo: nueva la primera vez, retomada después.
    let mut cmd = CommandBuilder::new(&claude);
    let (session_name, description) = {
        let mut run = state.run.lock().map_err(|e| e.to_string())?;
        let run = run.get_or_insert_with(|| RunSession {
            id: uuid::Uuid::new_v4().to_string(),
            name: format!("Nexo · {}", chrono::Local::now().format("%Y-%m-%d %H:%M")),
        });
        if session_saved(&app, &run.id) {
            cmd.args(["--resume", &run.id]);
            (run.name.clone(), format!("{} · retomada", run.name))
        } else {
            cmd.args(["--session-id", &run.id, "--name", &run.name]);
            (run.name.clone(), format!("{} · sesión nueva", run.name))
        }
    };
    let cwd = root.clone();
    cmd.args([
        "--append-system-prompt",
        &orchestrator_prompt(&root, &session_name),
    ]);
    if let Some(config) = playwright_mcp_config(&app) {
        cmd.arg("--mcp-config");
        cmd.arg(config);
    }
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
                    // Si la interfaz ya no escucha (p. ej. se recargó), se sigue vaciando la salida:
                    // dejar de leer llenaría el PTY y bloquearía a Claude Code.
                    if !text.is_empty() {
                        let _ = events.send(PtyEvent::Data { text });
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
    *state.session.lock().map_err(|e| e.to_string())? = Some(session);

    // Aviso de salida: se comprueba el proceso en segundo plano.
    let app_handle = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(400));
            let state = app_handle.state::<ClaudeCodeState>();
            let Ok(mut guard) = state.session.lock() else {
                break;
            };
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
pub async fn claude_code_write(
    state: State<'_, ClaudeCodeState>,
    data: String,
) -> Result<(), String> {
    let mut guard = state.session.lock().map_err(|e| e.to_string())?;
    let session = guard.as_mut().ok_or("Claude Code no está en marcha.")?;
    session
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| e.to_string())?;
    session.writer.flush().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn claude_code_resize(
    state: State<'_, ClaudeCodeState>,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let guard = state.session.lock().map_err(|e| e.to_string())?;
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
    stop(state.session.lock().map_err(|e| e.to_string())?.take());
    Ok(())
}
