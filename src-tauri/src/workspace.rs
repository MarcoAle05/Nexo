//! Workspaces de la vista Agentes: carpetas de proyecto que el usuario elige o crea, cada una
//! con su propio orquestador de Claude Code (`workspace_run.rs`). Aquí está lo que no es el
//! proceso: la lista de workspaces conocidos, la configuración de cada uno, el explorador de
//! archivos, el registro de sus agentes y el catálogo de extras del perfil global.
//!
//! - Lista: `<configuración de nexo>/workspaces.json` (el actual y los recientes). Solo se
//!   puede leer o lanzar algo en una carpeta de esa lista: así una ruta de la interfaz no
//!   abre cualquier sitio del disco.
//! - Cada workspace guarda lo suyo en `<carpeta>/.nexo/`: `workspace.json` (la sesión del
//!   orquestador, los extras encendidos y cómo dejó el usuario las pestañas y ventanas) y
//!   `agentes/<nombre>.md`, un archivo por agente desplegado, con su nombre y su rol en el
//!   frontmatter, como una skill. Los tokens no se guardan: salen del protocolo en directo.
//! - Extras: plugins, servidores MCP, conexiones de claude.ai y skills sin plugin del perfil
//!   global (`~/.claude`). Están apagados salvo los que el usuario enciende; se apagan para
//!   la sesión con `--settings` (`enabledPlugins`, `deniedMcpServers`, `skillOverrides`), sin
//!   tocar la configuración de Claude Code. Qué hay se le pregunta a Claude Code con
//!   peticiones de control que no llaman al modelo (`initialize`, `mcp_status`,
//!   `get_context_usage`).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tauri::{AppHandle, Manager};

use crate::skills::frontmatter;

pub(crate) const NEXO_DIR: &str = ".nexo";
const CONFIG_FILE: &str = "workspace.json";
pub(crate) const AGENTS_DIR: &str = "agentes";
const LIST_FILE: &str = "workspaces.json";
const RECENT_MAX: usize = 12;
/// Lo que se enseña de un archivo como mucho: el resto se corta (y se dice).
const READ_MAX: usize = 1024 * 1024;
/// Entradas de una carpeta como mucho en el explorador.
const LIST_MAX: usize = 2000;
/// El catálogo de extras se vuelve a preguntar pasado este tiempo.
const CATALOG_TTL: Duration = Duration::from_secs(10 * 60);
/// Carpetas que el explorador no enseña: la de git y la de nexo (lo de nexo se ve en la carta).
const HIDDEN: &[&str] = &[".git", NEXO_DIR];

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn millis(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------- lista de workspaces

#[derive(Default, Serialize, Deserialize)]
struct List {
    #[serde(default)]
    current: Option<PathBuf>,
    #[serde(default)]
    recent: Vec<Recent>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Recent {
    path: PathBuf,
    name: String,
    opened: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct WsInfo {
    pub path: String,
    pub name: String,
    pub opened: i64,
    /// La carpeta sigue existiendo.
    pub exists: bool,
    pub current: bool,
}

#[derive(Serialize)]
pub struct WsList {
    current: Option<WsInfo>,
    recent: Vec<WsInfo>,
    /// Dónde se crean las carpetas nuevas si el usuario no elige otro sitio.
    parent: String,
}

fn list_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join(LIST_FILE))
}

fn read_list(app: &AppHandle) -> List {
    list_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_list(app: &AppHandle, list: &List) -> Result<(), String> {
    let path = list_path(app)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn info_of(r: &Recent, current: Option<&Path>) -> WsInfo {
    WsInfo {
        path: r.path.display().to_string(),
        name: r.name.clone(),
        opened: r.opened,
        exists: r.path.is_dir(),
        current: current.is_some_and(|c| c == r.path),
    }
}

/// Los workspaces de la lista (carpeta y nombre), para el registro de agentes.
pub(crate) fn known(app: &AppHandle) -> Vec<(PathBuf, String)> {
    read_list(app)
        .recent
        .into_iter()
        .map(|r| (r.path, r.name))
        .collect()
}

/// Carpeta de un workspace de la lista. Cualquier ruta que llegue de la interfaz pasa por
/// aquí antes de leer, listar o lanzar nada en ella.
pub(crate) fn ws_root(app: &AppHandle, path: &str) -> Result<PathBuf, String> {
    let wanted = Path::new(path)
        .canonicalize()
        .map_err(|_| "Esa carpeta ya no existe.".to_string())?;
    let list = read_list(app);
    if list.recent.iter().any(|r| r.path == wanted) {
        Ok(wanted)
    } else {
        Err("Esa carpeta no es un workspace de nexo: ábrela antes desde la carta.".into())
    }
}

/// Nombre de una carpeta nueva: sin barras, sin `.`/`..`, sin caracteres de control y corto.
fn valid_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Escribe un nombre para la carpeta.".into());
    }
    if name.chars().count() > 80 {
        return Err("El nombre es demasiado largo (80 caracteres como mucho).".into());
    }
    if name == "."
        || name == ".."
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
    {
        return Err("El nombre no puede llevar barras ni caracteres especiales.".into());
    }
    Ok(name)
}

/// Prepara `.nexo/` (con un `.gitignore` que lo deja fuera de git si la carpeta es nueva) y
/// `.nexo/agentes/`.
fn prepare(root: &Path) -> Result<(), String> {
    let dir = root.join(NEXO_DIR);
    if !dir.is_dir() {
        fs::create_dir_all(&dir).map_err(|e| format!("No se pudo crear {NEXO_DIR}/: {e}"))?;
        let _ = fs::write(
            dir.join(".gitignore"),
            "# Lo de nexo (agentes, pestañas): no va en git.\n*\n",
        );
    }
    fs::create_dir_all(dir.join(AGENTS_DIR)).map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn remember(app: &AppHandle, root: &Path) -> Result<WsInfo, String> {
    let mut list = read_list(app);
    list.recent.retain(|r| r.path != root);
    let recent = Recent {
        path: root.to_path_buf(),
        name: folder_name(root),
        opened: now_ms(),
    };
    list.recent.insert(0, recent.clone());
    list.recent.truncate(RECENT_MAX);
    list.current = Some(root.to_path_buf());
    write_list(app, &list)?;
    Ok(info_of(&recent, Some(root)))
}

#[tauri::command]
pub fn workspace_list(app: AppHandle) -> WsList {
    let list = read_list(&app);
    let current = list.current.as_deref();
    let parent = list
        .recent
        .first()
        .and_then(|r| r.path.parent().map(Path::to_path_buf))
        .filter(|p| p.is_dir())
        .or_else(|| app.path().home_dir().ok())
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    WsList {
        current: list
            .recent
            .iter()
            .find(|r| Some(r.path.as_path()) == current)
            .map(|r| info_of(r, current)),
        recent: list.recent.iter().map(|r| info_of(r, current)).collect(),
        parent,
    }
}

/// Abre una carpeta como workspace (la elige el usuario con el diálogo del sistema).
#[tauri::command]
pub fn workspace_open(app: AppHandle, path: String) -> Result<WsInfo, String> {
    let root = Path::new(&path)
        .canonicalize()
        .map_err(|_| "Esa carpeta no existe.".to_string())?;
    if !root.is_dir() {
        return Err("Elige una carpeta, no un archivo.".into());
    }
    if root.parent().is_none() {
        return Err("La raíz del disco no puede ser un workspace.".into());
    }
    prepare(&root)?;
    remember(&app, &root)
}

/// Crea `parent/name` y la abre como workspace.
#[tauri::command]
pub fn workspace_create(app: AppHandle, parent: String, name: String) -> Result<WsInfo, String> {
    let name = valid_name(&name)?;
    let parent = Path::new(&parent)
        .canonicalize()
        .map_err(|_| "La ubicación ya no existe.".to_string())?;
    if !parent.is_dir() {
        return Err("La ubicación tiene que ser una carpeta.".into());
    }
    let root = parent.join(name);
    if root.exists() {
        return Err(format!("Ya hay una carpeta «{name}» en esa ubicación."));
    }
    fs::create_dir(&root).map_err(|e| format!("No se pudo crear la carpeta: {e}"))?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    prepare(&root)?;
    remember(&app, &root)
}

/// Quita un workspace de la lista (la carpeta no se toca).
#[tauri::command]
pub fn workspace_forget(app: AppHandle, path: String) -> Result<(), String> {
    let target = Path::new(&path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&path));
    let mut list = read_list(&app);
    list.recent.retain(|r| r.path != target);
    if list.current.as_deref() == Some(target.as_path()) {
        list.current = None;
    }
    write_list(&app, &list)
}

// ---------------------------------------------------------------- configuración

/// Extras encendidos: ids de plugin (`nombre@marketplace`, `nombre@synced`) y nombres de
/// servidores MCP, de conexiones de claude.ai y de skills.
#[derive(Default, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Extras {
    #[serde(default)]
    pub plugins: Vec<String>,
    #[serde(default)]
    pub mcp: Vec<String>,
    #[serde(default)]
    pub connectors: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    /// Ids de `Catalog::hooks`.
    #[serde(default)]
    pub hooks: Vec<String>,
}

#[derive(Default, Serialize, Deserialize, Clone)]
pub struct WsConfig {
    /// Sesión del orquestador: se retoma al volver a abrir el workspace.
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub extras: Extras,
    /// Cómo dejó el usuario el workspace (pestañas, ventanas, carpetas abiertas, el modelo
    /// del orquestador). Lo escribe la interfaz tal cual.
    #[serde(default)]
    pub ui: Value,
}

fn config_path(root: &Path) -> PathBuf {
    root.join(NEXO_DIR).join(CONFIG_FILE)
}

pub(crate) fn read_config(root: &Path) -> WsConfig {
    fs::read_to_string(config_path(root))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// La configuración se escribe desde varios sitios (la interfaz, el proceso): un cerrojo
/// para que una escritura no pise otra a medias.
static CONFIG_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn update_config(
    root: &Path,
    change: impl FnOnce(&mut WsConfig),
) -> Result<WsConfig, String> {
    let _guard = CONFIG_LOCK.lock().map_err(|e| e.to_string())?;
    prepare(root)?;
    let mut config = read_config(root);
    change(&mut config);
    let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    let path = config_path(root);
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(config)
}

#[derive(Serialize)]
pub struct WsState {
    info: WsInfo,
    extras: Extras,
    ui: Value,
    agents: Vec<RegAgent>,
    /// Agentes y skills de la propia carpeta (`.claude/agents/`, `.claude/skills/`).
    defined: Vec<crate::agents::AgentInfo>,
    skills: Vec<crate::skills::SkillInfo>,
}

#[tauri::command]
pub fn workspace_state(app: AppHandle, path: String) -> Result<WsState, String> {
    let root = ws_root(&app, &path)?;
    let config = read_config(&root);
    let list = read_list(&app);
    let info = list
        .recent
        .iter()
        .find(|r| r.path == root)
        .map(|r| info_of(r, list.current.as_deref()))
        .ok_or("Ese workspace ya no está en la lista.")?;
    Ok(WsState {
        info,
        extras: config.extras,
        ui: config.ui,
        agents: agents(&root),
        defined: crate::agents::list(&root),
        skills: crate::skills::list(&root),
    })
}

#[tauri::command]
pub fn workspace_save_ui(app: AppHandle, path: String, ui: Value) -> Result<(), String> {
    let root = ws_root(&app, &path)?;
    if serde_json::to_string(&ui).map_or(0, |s| s.len()) > 512 * 1024 {
        return Err("El estado de la interfaz es demasiado grande.".into());
    }
    update_config(&root, |c| c.ui = ui).map(|_| ())
}

#[tauri::command]
pub fn workspace_set_extras(
    app: AppHandle,
    path: String,
    extras: Extras,
) -> Result<Extras, String> {
    let root = ws_root(&app, &path)?;
    update_config(&root, |c| c.extras = extras).map(|c| c.extras)
}

// ---------------------------------------------------------------- explorador

#[derive(Serialize, Debug, PartialEq)]
pub struct Entry {
    name: String,
    /// Ruta relativa a la carpeta del workspace, con `/`.
    path: String,
    dir: bool,
    size: u64,
    link: bool,
}

#[derive(Serialize)]
pub struct Listing {
    entries: Vec<Entry>,
    truncated: bool,
}

/// Ruta de la interfaz → ruta dentro del workspace. `""` es la propia carpeta.
fn inside(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim_matches('/');
    if rel.is_empty() {
        return Ok(root.to_path_buf());
    }
    crate::vault::resolve_inside(root, rel).map_err(|e| e.replace("vault", "workspace"))
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Carpetas primero y luego archivos; dentro de cada grupo, por nombre sin distinguir
/// mayúsculas y con los números en su orden (`2` antes de `10`).
fn natural_key(name: &str) -> Vec<(u8, u64, String)> {
    let mut out = Vec::new();
    let mut digits = String::new();
    let mut text = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_digit() {
            if !text.is_empty() {
                out.push((1, 0, std::mem::take(&mut text)));
            }
            digits.push(c);
        } else {
            if !digits.is_empty() {
                out.push((
                    0,
                    digits.parse().unwrap_or(u64::MAX),
                    std::mem::take(&mut digits),
                ));
            }
            text.push(c);
        }
    }
    if !digits.is_empty() {
        out.push((0, digits.parse().unwrap_or(u64::MAX), digits));
    }
    if !text.is_empty() {
        out.push((1, 0, text));
    }
    out
}

pub(crate) fn list_dir(root: &Path, dir: &Path) -> Result<Listing, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("No se pudo leer la carpeta: {e}"))?;
    let mut out: Vec<Entry> = Vec::new();
    let mut truncated = false;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if dir == root && HIDDEN.contains(&name.as_str()) {
            continue;
        }
        if out.len() >= LIST_MAX {
            truncated = true;
            break;
        }
        let path = entry.path();
        let link = entry.file_type().is_ok_and(|t| t.is_symlink());
        let meta = fs::metadata(&path).ok();
        out.push(Entry {
            name,
            path: relative(root, &path),
            dir: meta.as_ref().is_some_and(|m| m.is_dir()),
            size: meta.as_ref().map_or(0, |m| m.len()),
            link,
        });
    }
    out.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| natural_key(&a.name).cmp(&natural_key(&b.name)))
    });
    Ok(Listing {
        entries: out,
        truncated,
    })
}

#[tauri::command]
pub fn workspace_tree(app: AppHandle, path: String, dir: String) -> Result<Listing, String> {
    let root = ws_root(&app, &path)?;
    let target = inside(&root, &dir)?;
    list_dir(&root, &target)
}

#[derive(Serialize, Debug)]
pub struct FileView {
    path: String,
    size: u64,
    modified: i64,
    /// No es texto: no se enseña su contenido.
    binary: bool,
    /// Solo se enseña el primer mega.
    truncated: bool,
    text: Option<String>,
}

pub(crate) fn read_file(root: &Path, file: &Path) -> Result<FileView, String> {
    let meta = fs::metadata(file).map_err(|e| e.to_string())?;
    if meta.is_dir() {
        return Err("Es una carpeta.".into());
    }
    let size = meta.len();
    let modified = meta.modified().map(millis).unwrap_or(0);
    let mut bytes = Vec::with_capacity(size.min(READ_MAX as u64 + 1) as usize);
    {
        use std::io::Read;
        fs::File::open(file)
            .map_err(|e| e.to_string())?
            .take(READ_MAX as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
    }
    let path = relative(root, file);
    // Un NUL en el principio es casi siempre un binario (imagen, ejecutable, PDF…).
    if bytes.iter().take(8000).any(|&b| b == 0) {
        return Ok(FileView {
            path,
            size,
            modified,
            binary: true,
            truncated: false,
            text: None,
        });
    }
    let truncated = size > READ_MAX as u64;
    // Si el corte cayó en mitad de un carácter, se quita lo que sobra.
    let text = match String::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => {
            let valid = e.utf8_error().valid_up_to();
            let bytes = e.into_bytes();
            if truncated && bytes.len() - valid < 4 {
                String::from_utf8_lossy(&bytes[..valid]).into_owned()
            } else {
                String::from_utf8_lossy(&bytes).into_owned()
            }
        }
    };
    Ok(FileView {
        path,
        size,
        modified,
        binary: false,
        truncated,
        text: Some(text),
    })
}

#[tauri::command]
pub fn workspace_read(app: AppHandle, path: String, file: String) -> Result<FileView, String> {
    let root = ws_root(&app, &path)?;
    let target = inside(&root, &file)?;
    if target == root {
        return Err("Es la carpeta del workspace.".into());
    }
    read_file(&root, &target)
}

// ---------------------------------------------------------------- registro de agentes

/// Un agente que desplegó el orquestador: `.nexo/agentes/<archivo>.md`, con su nombre, su
/// rol y de dónde salió en el frontmatter, y el encargo inicial en el cuerpo.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct RegAgent {
    /// Nombre del archivo sin `.md`.
    pub file: String,
    pub name: String,
    pub role: String,
    /// Tipo de subagente de Claude Code (`general-purpose`, `Explore`, uno de `.claude/agents/`…).
    #[serde(rename = "type")]
    pub kind: String,
    /// Id de la tarea en Claude Code (el del subagente, el de su transcripción).
    pub task: String,
    /// Id del `tool_use` con el que se desplegó.
    pub tool_use: Option<String>,
    /// Sesión del orquestador en la que nació.
    pub session: Option<String>,
    pub created: String,
    pub prompt: String,
}

fn agents_dir(root: &Path) -> PathBuf {
    root.join(NEXO_DIR).join(AGENTS_DIR)
}

/// El frontmatter y el cuerpo de un archivo `---\n…\n---\n`.
fn body_of(text: &str) -> &str {
    let rest = text
        .strip_prefix("---")
        .map(|r| r.trim_start_matches(['\r', '\n']));
    match rest.and_then(|r| r.find("\n---").map(|i| &r[i + 4..])) {
        Some(body) => body.trim_start_matches(['\r', '\n']),
        None => text,
    }
}

pub(crate) fn parse_agent(file: &str, text: &str) -> Option<RegAgent> {
    let meta = frontmatter(text);
    let get = |key: &str| {
        meta.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let task = get("task")?;
    let name = get("name").unwrap_or_else(|| file.to_string());
    Some(RegAgent {
        file: file.to_string(),
        role: get("role").unwrap_or_else(|| name.clone()),
        name,
        kind: get("type").unwrap_or_else(|| "general-purpose".into()),
        task,
        tool_use: get("tool_use"),
        session: get("session"),
        created: get("created").unwrap_or_default(),
        prompt: body_of(text).trim().to_string(),
    })
}

/// Agentes registrados, del más antiguo al más nuevo.
pub(crate) fn agents(root: &Path) -> Vec<RegAgent> {
    let Ok(entries) = fs::read_dir(agents_dir(root)) else {
        return Vec::new();
    };
    let mut list: Vec<(i64, RegAgent)> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let file = path.file_stem()?.to_string_lossy().into_owned();
            if path.extension()? != "md" || file.starts_with('.') {
                return None;
            }
            let text = fs::read_to_string(&path).ok()?;
            let agent = parse_agent(&file, &text)?;
            let born = e
                .metadata()
                .ok()
                .and_then(|m| m.created().or_else(|_| m.modified()).ok())
                .map(millis)
                .unwrap_or(0);
            Some((born, agent))
        })
        .collect();
    list.sort_by(|a, b| {
        a.1.created
            .cmp(&b.1.created)
            .then(a.0.cmp(&b.0))
            .then_with(|| a.1.file.cmp(&b.1.file))
    });
    list.into_iter().map(|(_, a)| a).collect()
}

#[tauri::command]
pub fn workspace_agents(app: AppHandle, path: String) -> Result<Vec<RegAgent>, String> {
    let root = ws_root(&app, &path)?;
    Ok(agents(&root))
}

/// «Revisor de la API» → «revisor-de-la-api».
fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let c = match c {
            'á' | 'à' | 'ä' | 'â' | 'Á' | 'À' | 'Ä' | 'Â' => 'a',
            'é' | 'è' | 'ë' | 'ê' | 'É' | 'È' | 'Ë' | 'Ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' | 'Í' | 'Ì' | 'Ï' | 'Î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'Ó' | 'Ò' | 'Ö' | 'Ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' | 'Ú' | 'Ù' | 'Ü' | 'Û' => 'u',
            'ñ' | 'Ñ' => 'n',
            c => c.to_ascii_lowercase(),
        };
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= 48 {
            break;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "agente".into() } else { out }
}

/// Lo que cabe en una línea del frontmatter: sin saltos y, si lleva algo que YAML lee
/// distinto, entre comillas.
fn yaml_line(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty()
        || flat.contains([':', '#', '"', '\'', '[', ']', '{', '}', ','])
        || flat.starts_with(['-', '?', '!', '&', '*', '|', '>', '%', '@', '`'])
    {
        format!("\"{}\"", flat.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        flat
    }
}

/// Qué hace cada tipo de subagente de Claude Code, para el rol de los que se despliegan sin
/// uno propio.
fn builtin_role(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "general-purpose" => "Agente de uso general: investiga y hace tareas de varios pasos",
        "Explore" => "Explora el código y busca archivos sin cambiar nada",
        "Plan" => "Diseña el plan de una tarea antes de hacerla",
        "claude" => "Agente de uso general",
        "statusline-setup" => "Configura la línea de estado de Claude Code",
        _ => return None,
    })
}

/// Registra un subagente recién desplegado (la línea `task_started` de Claude Code). Si ya
/// tiene archivo (se retomó con SendMessage), no hace nada. Devuelve si escribió algo.
pub(crate) fn register_task(root: &Path, session: &str, task: &Value) -> Result<bool, String> {
    if task["task_type"]
        .as_str()
        .is_some_and(|t| t != "local_agent")
    {
        return Ok(false);
    }
    let Some(id) = task["task_id"].as_str().filter(|id| safe_id(id)) else {
        return Ok(false);
    };
    let dir = agents_dir(root);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if agents(root).iter().any(|a| a.task == id) {
        return Ok(false);
    }
    let kind = task["subagent_type"]
        .as_str()
        .unwrap_or("general-purpose")
        .trim();
    let description = task["description"]
        .as_str()
        .map(str::trim)
        .filter(|d| !d.is_empty());
    let name = description.unwrap_or(kind).to_string();
    // El rol: el de su definición en la carpeta (`.claude/agents/`), el de un tipo conocido o,
    // si no, lo que se le encargó. El orquestador puede cambiarlo editando el archivo.
    let role = crate::agents::list(root)
        .into_iter()
        .find(|a| a.name == kind || a.id == kind)
        .map(|a| a.description)
        .or_else(|| builtin_role(kind).map(String::from))
        .unwrap_or_else(|| name.clone());
    let short: String = id.chars().take(6).collect();
    let mut file = format!("{}-{short}", slug(&name));
    let mut n = 2;
    while dir.join(format!("{file}.md")).exists() {
        file = format!("{}-{short}-{n}", slug(&name));
        n += 1;
    }
    let mut text = format!(
        "---\nname: {}\nrole: {}\ntype: {}\ntask: {id}\n",
        yaml_line(&name),
        yaml_line(&role),
        yaml_line(kind)
    );
    if let Some(tool) = task["tool_use_id"]
        .as_str()
        .filter(|t| safe_id(t) || t.starts_with("toolu_"))
    {
        text.push_str(&format!("tool_use: {tool}\n"));
    }
    text.push_str(&format!(
        "session: {session}\ncreated: {}\n---\n\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M")
    ));
    if let Some(prompt) = task["prompt"]
        .as_str()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        text.push_str(prompt);
        text.push('\n');
    }
    fs::write(dir.join(format!("{file}.md")), text).map_err(|e| e.to_string())?;
    Ok(true)
}

pub(crate) fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

// ---------------------------------------------------------------- extras del perfil global

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct PluginItem {
    /// Lo que entiende `enabledPlugins`: `nombre@marketplace` o `nombre@synced`.
    pub id: String,
    pub name: String,
    pub description: String,
    pub skills: usize,
    pub servers: usize,
    /// Viene sincronizado desde claude.ai.
    pub synced: bool,
    /// Trae hooks: encendido, los del perfil del usuario también se ejecutan (ver
    /// `extras_settings`).
    pub hooks: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ServerItem {
    pub name: String,
    /// `stdio`, `http`, `sse`, `claudeai-proxy`…
    pub kind: String,
    /// El programa o el dominio (nada de cabeceras ni variables: pueden llevar claves).
    pub detail: String,
    pub status: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct SkillItem {
    pub name: String,
    pub description: String,
    /// `user` (las de `~/.claude/skills/`) o `claude.ai` (sincronizadas).
    pub source: String,
}

/// Hooks que se pueden encender en un workspace. Claude Code no sabe apagar unos y dejar
/// otros: con cualquiera encendido, los del perfil y los de la carpeta también se ejecutan.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct HookItem {
    /// `rtk` (lo añade nexo), `perfil` (`~/.claude/settings.json`) o `carpeta` (su `.claude/`).
    pub id: String,
    pub name: String,
    pub detail: String,
    /// Lo añade nexo a la sesión (no está en ningún archivo de ajustes): solo corre si se
    /// enciende, no arrastrado por otro.
    pub added: bool,
    /// Lo que nexo añade a los ajustes de la sesión (`{evento: [grupos]}`).
    #[serde(skip)]
    pub settings: Value,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    pub plugins: Vec<PluginItem>,
    pub mcp: Vec<ServerItem>,
    pub connectors: Vec<ServerItem>,
    pub skills: Vec<SkillItem>,
    pub hooks: Vec<HookItem>,
    /// Si Claude Code no contestó, por qué (el catálogo sale entonces de los archivos).
    pub error: Option<String>,
}

#[derive(Default)]
pub struct WorkspaceCatalogs {
    cache: Mutex<HashMap<PathBuf, (Instant, Catalog)>>,
}

/// Plugins instalados y sincronizados: nombre → (id, descripción, sincronizado).
fn known_plugins(home: &Path, root: &Path) -> HashMap<String, (String, String, bool)> {
    let mut out = HashMap::new();
    let read = |p: PathBuf| {
        fs::read_to_string(p)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    };
    let describe = |install: &str| {
        read(Path::new(install).join(".claude-plugin/plugin.json"))
            .and_then(|v| v["description"].as_str().map(String::from))
            .unwrap_or_default()
    };
    if let Some(installed) = read(home.join(".claude/plugins/installed_plugins.json")) {
        for (id, installs) in installed["plugins"].as_object().into_iter().flatten() {
            let Some((name, _)) = id.split_once('@') else {
                continue;
            };
            // Los de proyecto solo valen en su carpeta.
            let usable = installs.as_array().into_iter().flatten().find(|i| {
                i["scope"] == "user"
                    || i["projectPath"]
                        .as_str()
                        .is_some_and(|p| Path::new(p) == root)
            });
            if let Some(install) = usable {
                let description = install["installPath"]
                    .as_str()
                    .map(describe)
                    .unwrap_or_default();
                out.insert(name.to_string(), (id.clone(), description, false));
            }
        }
    }
    if let Ok(buckets) = fs::read_dir(home.join(".claude/plugins/synced")) {
        for bucket in buckets.flatten() {
            let Some(manifest) = read(bucket.path().join("manifest.json")) else {
                continue;
            };
            for p in manifest["plugins"].as_array().into_iter().flatten() {
                if let Some(name) = p["name"].as_str() {
                    let description = p["description"].as_str().unwrap_or_default().to_string();
                    out.entry(name.to_string()).or_insert((
                        format!("{name}@synced"),
                        description,
                        true,
                    ));
                }
            }
        }
    }
    out
}

fn has_hooks(dir: &Path) -> bool {
    dir.join("hooks/hooks.json").is_file()
        || fs::read_to_string(dir.join(".claude-plugin/plugin.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .is_some_and(|v| !v["hooks"].is_null())
}

/// Ids de los plugins instalados o sincronizados que traen hooks.
fn plugins_with_hooks(home: &Path) -> HashSet<String> {
    let mut out = HashSet::new();
    if let Some(installed) = fs::read_to_string(home.join(".claude/plugins/installed_plugins.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    {
        for (id, installs) in installed["plugins"].as_object().into_iter().flatten() {
            let hooked = installs.as_array().into_iter().flatten().any(|i| {
                i["installPath"]
                    .as_str()
                    .is_some_and(|p| has_hooks(Path::new(p)))
            });
            if hooked {
                out.insert(id.clone());
            }
        }
    }
    // Los sincronizados viven en `<cubo>/<nombre>~g<generación>`.
    for bucket in fs::read_dir(home.join(".claude/plugins/synced"))
        .into_iter()
        .flatten()
        .flatten()
    {
        for entry in fs::read_dir(bucket.path()).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some((plugin, _)) = name.split_once('~')
                && entry.path().is_dir()
                && has_hooks(&entry.path())
            {
                out.insert(format!("{plugin}@synced"));
            }
        }
    }
    out
}

/// Un programa en el `PATH` o donde se suelen instalar las herramientas del usuario (nexo se
/// abre desde el escritorio, con un `PATH` corto).
pub(crate) fn find_program(name: &str, home: &Path) -> Option<PathBuf> {
    let from_path = std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .map(|p| p.join(name))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    from_path
        .into_iter()
        .chain([
            home.join(".local/bin").join(name),
            home.join(".cargo/bin").join(name),
        ])
        .find(|p| p.is_file())
}

/// Qué es un hook, sin enseñar su orden (puede llevar claves): el programa que se reconoce.
fn hook_label(command: &str) -> String {
    if command.contains(".orca/") {
        return "orca".into();
    }
    if command.contains("rtk hook") || command.contains("rtk-rewrite") {
        return "rtk".into();
    }
    let first = command
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches(['"', '\'']);
    let shell = ["if", "case", "for", "while", "[", "[[", "{", "("];
    if first.is_empty() || shell.contains(&first) || first.contains('=') {
        return "un script".into();
    }
    Path::new(first)
        .file_name()
        .map_or_else(|| first.to_string(), |n| n.to_string_lossy().into_owned())
}

/// Los hooks de un archivo de ajustes: qué programas son y en qué eventos están.
fn hooks_in(settings: &Value) -> Option<(Vec<String>, HashSet<String>)> {
    let mut labels: Vec<String> = Vec::new();
    let mut events = HashSet::new();
    for (event, groups) in settings["hooks"].as_object()? {
        let hooks: Vec<&Value> = groups
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|g| g["hooks"].as_array().into_iter().flatten())
            .collect();
        if hooks.is_empty() {
            continue;
        }
        events.insert(event.clone());
        for hook in hooks {
            let label = match hook["command"].as_str() {
                Some(command) => hook_label(command),
                None => hook["type"].as_str().unwrap_or("hook").to_string(),
            };
            if !labels.contains(&label) {
                labels.push(label);
            }
        }
    }
    (!events.is_empty()).then_some((labels, events))
}

/// Los hooks que se pueden encender en un workspace: rtk si está instalado (y no viene ya en
/// el perfil), los del perfil y los de la carpeta.
pub(crate) fn hook_items(home: &Path, root: &Path, rtk: Option<PathBuf>) -> Vec<HookItem> {
    let read = |p: PathBuf| {
        fs::read_to_string(p)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .unwrap_or(Value::Null)
    };
    let summary = |(labels, events): (Vec<String>, HashSet<String>)| {
        let n = events.len();
        format!(
            "{} · {n} {}",
            labels.join(", "),
            if n == 1 { "evento" } else { "eventos" }
        )
    };
    let profile = hooks_in(&read(home.join(".claude/settings.json")));
    // La carpeta puede ser el propio home: entonces sus ajustes son los del perfil.
    let mut folder: Option<(Vec<String>, HashSet<String>)> = None;
    if root != home {
        for file in ["settings.json", "settings.local.json"] {
            if let Some((labels, events)) = hooks_in(&read(root.join(".claude").join(file))) {
                let (all, seen) = folder.get_or_insert_with(Default::default);
                for label in labels {
                    if !all.contains(&label) {
                        all.push(label);
                    }
                }
                seen.extend(events);
            }
        }
    }
    let mut items = Vec::new();
    let rtk_in_profile = profile
        .as_ref()
        .is_some_and(|(labels, _)| labels.iter().any(|l| l == "rtk"));
    if let Some(rtk) = rtk.filter(|_| !rtk_in_profile) {
        // El mismo hook que instala `rtk init -g`, con la ruta entera (el PATH de nexo es corto).
        let path = rtk.to_string_lossy().replace('\'', r"'\''");
        items.push(HookItem {
            id: "rtk".into(),
            name: "rtk".into(),
            detail: "Pasa los comandos de Bash por rtk para gastar menos tokens · lo añade nexo, no tu perfil".into(),
            added: true,
            settings: json!({ "PreToolUse": [{
                "matcher": "Bash",
                "hooks": [{ "type": "command", "command": format!("'{path}' hook claude") }]
            }] }),
        });
    }
    if let Some(found) = profile {
        items.push(HookItem {
            id: "perfil".into(),
            name: "Los de tu perfil".into(),
            detail: summary(found),
            added: false,
            settings: Value::Null,
        });
    }
    if let Some(found) = folder {
        items.push(HookItem {
            id: "carpeta".into(),
            name: "Los de esta carpeta".into(),
            detail: summary(found),
            added: false,
            settings: Value::Null,
        });
    }
    items
}

/// Si algún hook va a correr: uno encendido o un plugin encendido que trae los suyos.
fn hooks_on(catalog: &Catalog, on: &Extras) -> bool {
    catalog
        .plugins
        .iter()
        .any(|p| p.hooks && on.plugins.contains(&p.id))
        || catalog.hooks.iter().any(|h| on.hooks.contains(&h.id))
}

/// Los hooks que de verdad corren: los encendidos y, si corre alguno, todos los de los
/// archivos de ajustes (Claude Code no los separa).
pub(crate) fn hooks_running<'a>(catalog: &'a Catalog, on: &Extras) -> Vec<&'a HookItem> {
    let any = hooks_on(catalog, on);
    catalog
        .hooks
        .iter()
        .filter(|h| on.hooks.contains(&h.id) || (any && !h.added))
        .collect()
}

/// Plugins que el usuario tiene encendidos en `~/.claude/settings.json`.
fn enabled_plugins(home: &Path) -> Vec<String> {
    fs::read_to_string(home.join(".claude/settings.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["enabledPlugins"].as_object().cloned())
        .map(|m| {
            m.into_iter()
                .filter(|(_, on)| on == &Value::Bool(true))
                .map(|(id, _)| id)
                .collect()
        })
        .unwrap_or_default()
}

/// Quita el «(plugin) » con el que Claude Code empieza la descripción de las skills de plugin.
fn clean_description(text: &str) -> String {
    let text = text.trim();
    let text = match text.strip_prefix('(').and_then(|r| r.split_once(") ")) {
        Some((_, rest)) => rest,
        None => text,
    };
    text.to_string()
}

fn server_detail(config: &Value) -> String {
    if let Some(url) = config["url"].as_str() {
        return url
            .split("://")
            .nth(1)
            .unwrap_or(url)
            .split(['/', '?'])
            .next()
            .unwrap_or_default()
            .to_string();
    }
    config["command"]
        .as_str()
        .map(|c| {
            Path::new(c)
                .file_name()
                .map_or(c.to_string(), |n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
}

/// El catálogo a partir de lo que contestó Claude Code: `initialize` (comandos y skills),
/// `mcp_status` (servidores) y `get_context_usage` (de dónde viene cada skill).
pub(crate) fn build_catalog(
    init: &Value,
    mcp: &Value,
    context: &Value,
    plugins: &HashMap<String, (String, String, bool)>,
    enabled: &[String],
) -> Catalog {
    let descriptions: HashMap<&str, &str> = init["commands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some((
                c["name"].as_str()?,
                c["description"].as_str().unwrap_or_default(),
            ))
        })
        .collect();
    let mut catalog = Catalog::default();
    let mut plugin_skills: HashMap<String, usize> = HashMap::new();
    let mut plugin_servers: HashMap<String, usize> = HashMap::new();
    for skill in context["skills"]["skillFrontmatter"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let Some(name) = skill["name"].as_str() else {
            continue;
        };
        match skill["source"].as_str() {
            Some("plugin") => {
                if let Some(p) = skill["pluginName"].as_str() {
                    *plugin_skills.entry(p.to_string()).or_default() += 1;
                }
            }
            Some(source @ ("userSettings" | "syncedSkills")) => catalog.skills.push(SkillItem {
                name: name.to_string(),
                description: clean_description(descriptions.get(name).copied().unwrap_or_default()),
                source: if source == "userSettings" {
                    "user".into()
                } else {
                    "claude.ai".into()
                },
            }),
            _ => {}
        }
    }
    for server in mcp["mcpServers"].as_array().into_iter().flatten() {
        let Some(name) = server["name"].as_str() else {
            continue;
        };
        let item = || ServerItem {
            name: name.to_string(),
            kind: server["config"]["type"]
                .as_str()
                .unwrap_or("stdio")
                .to_string(),
            detail: server_detail(&server["config"]),
            status: server["status"].as_str().unwrap_or_default().to_string(),
        };
        match server["source"].as_str().or(server["scope"].as_str()) {
            Some("plugin") => {
                if let Some(p) = name
                    .strip_prefix("plugin:")
                    .and_then(|r| r.split(':').next())
                {
                    *plugin_servers.entry(p.to_string()).or_default() += 1;
                }
            }
            Some("claudeai") => catalog.connectors.push(item()),
            // Los de `.mcp.json` son de la propia carpeta: no son extras del perfil.
            Some("project") => {}
            _ => catalog.mcp.push(item()),
        }
    }
    let mut names: Vec<String> = plugin_skills
        .keys()
        .chain(plugin_servers.keys())
        .cloned()
        .collect();
    for id in enabled {
        if let Some((name, _)) = id.split_once('@') {
            names.push(name.to_string());
        }
    }
    let mut seen = HashSet::new();
    names.sort();
    for name in names {
        if !seen.insert(name.clone()) || name == "nexo" {
            continue;
        }
        let Some((id, description, synced)) = plugins.get(&name) else {
            continue;
        };
        catalog.plugins.push(PluginItem {
            id: id.clone(),
            name: name.clone(),
            description: description.clone(),
            skills: plugin_skills.get(&name).copied().unwrap_or(0),
            servers: plugin_servers.get(&name).copied().unwrap_or(0),
            synced: *synced,
            hooks: false,
        });
    }
    catalog.skills.sort_by_key(|a| a.name.to_lowercase());
    catalog
}

/// Ajustes de la sesión del orquestador que apagan todo lo del catálogo que no está
/// encendido en el workspace. Van por `--settings`: no tocan la configuración del usuario.
pub(crate) fn extras_settings(catalog: &Catalog, on: &Extras) -> Value {
    let plugins: Map<String, Value> = catalog
        .plugins
        .iter()
        .map(|p| (p.id.clone(), Value::Bool(on.plugins.contains(&p.id))))
        .collect();
    let denied: Vec<Value> = catalog
        .mcp
        .iter()
        .filter(|s| !on.mcp.contains(&s.name))
        .chain(
            catalog
                .connectors
                .iter()
                .filter(|s| !on.connectors.contains(&s.name)),
        )
        .map(|s| json!({ "serverName": s.name }))
        .collect();
    let skills: Map<String, Value> = catalog
        .skills
        .iter()
        .filter(|s| !on.skills.contains(&s.name))
        .map(|s| (s.name.clone(), Value::String("off".into())))
        .collect();
    let mut settings =
        json!({ "enabledPlugins": plugins, "deniedMcpServers": denied, "skillOverrides": skills });
    // Un catálogo de reserva puede no conocer todas las conexiones: si no hay ninguna
    // encendida, se apagan todas.
    if catalog.error.is_some() && on.connectors.is_empty() {
        settings["disableClaudeAiConnectors"] = Value::Bool(true);
    }
    // Los hooks tampoco son de serie. Claude Code solo sabe apagarlos todos a la vez: con uno
    // encendido (o un plugin encendido que trae los suyos) corren también los de los archivos
    // de ajustes. Los que añade nexo van en los ajustes de la sesión, solo si se encienden.
    if !hooks_on(catalog, on) {
        settings["disableAllHooks"] = Value::Bool(true);
    }
    let mut added = Map::new();
    for hook in catalog
        .hooks
        .iter()
        .filter(|h| h.added && on.hooks.contains(&h.id))
    {
        for (event, groups) in hook.settings.as_object().into_iter().flatten() {
            if let Some(list) = added
                .entry(event.clone())
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
            {
                list.extend(groups.as_array().into_iter().flatten().cloned());
            }
        }
    }
    if !added.is_empty() {
        settings["hooks"] = Value::Object(added);
    }
    settings
}

/// Le pregunta a Claude Code, en la carpeta del workspace y con la configuración del usuario,
/// qué carga, sin llamar al modelo: unas respuestas de control y se cierra.
fn probe_catalog(claude: &Path, root: &Path) -> Result<(Value, Value, Value), String> {
    let mut command = Command::new(claude);
    for key in crate::claude_code::INHERITED_ENV {
        command.env_remove(key);
    }
    let mut child = command
        .args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--no-session-persistence",
            "--settings",
            r#"{"disableAllHooks":true}"#,
        ])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("No se pudo abrir Claude Code: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("Sin entrada")?;
    let stdout = child.stdout.take().ok_or("Sin salida")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if !line.starts_with(r#"{"type":"control_response""#) {
                continue;
            }
            if let Ok(msg) = serde_json::from_str::<Value>(&line)
                && tx.send(msg).is_err()
            {
                return;
            }
        }
    });
    let mut sent = Ok(());
    for (id, subtype) in [
        ("nexo-ws-1", "initialize"),
        ("nexo-ws-2", "mcp_status"),
        ("nexo-ws-3", "get_context_usage"),
    ] {
        let mut request = json!({ "subtype": subtype });
        if subtype == "get_context_usage" {
            request["detail"] = "summary".into();
        }
        let line = json!({ "type": "control_request", "request_id": id, "request": request });
        sent = sent.and_then(|_| writeln!(stdin, "{line}"));
    }
    let _ = stdin.flush();
    let mut got: HashMap<String, Value> = HashMap::new();
    let deadline = Instant::now() + Duration::from_secs(25);
    while sent.is_ok() && got.len() < 3 {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        let Ok(msg) = rx.recv_timeout(left) else {
            break;
        };
        let response = &msg["response"];
        if let Some(id) = response["request_id"].as_str() {
            got.insert(id.to_string(), response["response"].clone());
        }
    }
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    if got.len() < 3 {
        return Err("Claude Code no contestó a tiempo.".into());
    }
    let take = |id: &str| got.get(id).cloned().unwrap_or(Value::Null);
    Ok((take("nexo-ws-1"), take("nexo-ws-2"), take("nexo-ws-3")))
}

/// Catálogo de reserva, leído de los archivos del perfil, por si Claude Code no contesta: así
/// lo apagado sigue apagado. Las conexiones de claude.ai que no se conocen se apagan todas
/// (`disableClaudeAiConnectors`, ver `extras_settings`).
pub(crate) fn fallback_catalog(home: &Path, root: &Path, why: String) -> Catalog {
    let read = |p: PathBuf| {
        fs::read_to_string(p)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    };
    let enabled = enabled_plugins(home);
    let hooked = plugins_with_hooks(home);
    let plugins = known_plugins(home, root)
        .into_iter()
        .filter(|(_, (id, _, synced))| *synced || enabled.contains(id))
        .map(|(name, (id, description, synced))| PluginItem {
            hooks: hooked.contains(&id),
            id,
            name,
            description,
            skills: 0,
            servers: 0,
            synced,
        })
        .collect();
    let config = read(home.join(".claude.json")).unwrap_or(Value::Null);
    let server = |name: &str| ServerItem {
        name: name.to_string(),
        kind: String::new(),
        detail: String::new(),
        status: String::new(),
    };
    let mut mcp: Vec<ServerItem> = config["mcpServers"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(n, _)| server(n))
        .collect();
    let local = &config["projects"][root.to_string_lossy().as_ref()]["mcpServers"];
    mcp.extend(
        local
            .as_object()
            .into_iter()
            .flatten()
            .map(|(n, _)| server(n)),
    );
    let connectors = config["claudeAiMcpEverConnected"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
        .map(server)
        .collect();
    let skill_dirs = |dir: PathBuf, source: &str| -> Vec<SkillItem> {
        fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().join("SKILL.md").is_file())
            .map(|e| SkillItem {
                name: e.file_name().to_string_lossy().into_owned(),
                description: String::new(),
                source: source.into(),
            })
            .collect()
    };
    let mut skills = skill_dirs(home.join(".claude/skills"), "user");
    for bucket in fs::read_dir(home.join(".claude/skills/synced"))
        .into_iter()
        .flatten()
        .flatten()
    {
        skills.extend(skill_dirs(bucket.path(), "claude.ai"));
    }
    Catalog {
        plugins,
        mcp,
        connectors,
        skills,
        hooks: hook_items(home, root, find_program("rtk", home)),
        error: Some(why),
    }
}

/// El catálogo de extras de un workspace (en caché unos minutos: sondear tarda un segundo).
pub(crate) fn catalog_for(app: &AppHandle, root: &Path, force: bool) -> Result<Catalog, String> {
    let state = app.state::<WorkspaceCatalogs>();
    if !force
        && let Some((at, catalog)) = state.cache.lock().map_err(|e| e.to_string())?.get(root)
        && at.elapsed() < CATALOG_TTL
    {
        return Ok(catalog.clone());
    }
    let claude = crate::claude_code::find_claude(app).ok_or("No encontré Claude Code.")?;
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let mut catalog = match probe_catalog(&claude, root) {
        Ok((init, mcp, context)) => build_catalog(
            &init,
            &mcp,
            &context,
            &known_plugins(&home, root),
            &enabled_plugins(&home),
        ),
        // Sin respuesta, lo de los archivos (sin guardarlo: la próxima vez se vuelve a probar).
        Err(e) => return Ok(fallback_catalog(&home, root, e)),
    };
    let hooked = plugins_with_hooks(&home);
    for plugin in &mut catalog.plugins {
        plugin.hooks = hooked.contains(&plugin.id);
    }
    catalog.hooks = hook_items(&home, root, find_program("rtk", &home));
    state
        .cache
        .lock()
        .map_err(|e| e.to_string())?
        .insert(root.to_path_buf(), (Instant::now(), catalog.clone()));
    Ok(catalog)
}

/// El catálogo con el que se arranca o se mide un orquestador. Si no se puede sondear, el de
/// los archivos; si ni eso, uno vacío que al menos apaga las conexiones.
pub(crate) fn catalog_or_fallback(app: &AppHandle, root: &Path, force: bool) -> Catalog {
    catalog_for(app, root, force).unwrap_or_else(|e| match app.path().home_dir() {
        Ok(home) => fallback_catalog(&home, root, e),
        Err(_) => Catalog {
            error: Some(e),
            ..Catalog::default()
        },
    })
}

#[tauri::command]
pub async fn workspace_extras(
    app: AppHandle,
    path: String,
    force: bool,
) -> Result<Catalog, String> {
    let root = ws_root(&app, &path)?;
    tauri::async_runtime::spawn_blocking(move || catalog_for(&app, &root, force))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("nexo-ws-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root.canonicalize().unwrap()
    }

    #[test]
    fn los_nombres_de_carpeta_nuevos_no_salen_de_su_sitio() {
        assert_eq!(valid_name("  Mi proyecto  "), Ok("Mi proyecto"));
        for bad in ["", "   ", ".", "..", "a/b", "a\\b", "uno\ndos"] {
            assert!(valid_name(bad).is_err(), "{bad:?} debería rechazarse");
        }
        assert!(valid_name(&"x".repeat(81)).is_err());
    }

    #[test]
    fn el_explorador_ordena_carpetas_primero_y_numeros_en_orden() {
        let root = temp("arbol");
        for dir in ["src", "Docs", ".git", ".nexo"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in ["b.txt", "a10.md", "a2.md", "README.md", ".env"] {
            fs::write(root.join(file), "x").unwrap();
        }
        let listing = list_dir(&root, &root).unwrap();
        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Docs",
                "src",
                ".env",
                "a2.md",
                "a10.md",
                "b.txt",
                "README.md"
            ]
        );
        assert!(listing.entries[0].dir && !listing.entries[2].dir);
        assert_eq!(listing.entries[1].path, "src");
        // Dentro de una subcarpeta, las rutas siguen siendo relativas a la raíz.
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        let sub = list_dir(&root, &root.join("src")).unwrap();
        assert_eq!(sub.entries[0].path, "src/main.rs");
    }

    #[test]
    fn las_rutas_del_explorador_no_salen_del_workspace() {
        let root = temp("guardia");
        fs::create_dir_all(root.join("dentro")).unwrap();
        assert_eq!(inside(&root, "").unwrap(), root);
        assert_eq!(inside(&root, "/dentro/").unwrap(), root.join("dentro"));
        assert!(inside(&root, "..").is_err());
        assert!(inside(&root, "dentro/../../").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc", root.join("fuera")).unwrap();
            assert!(inside(&root, "fuera/hostname").is_err());
        }
    }

    #[test]
    fn lee_texto_y_reconoce_binarios() {
        let root = temp("leer");
        fs::write(
            root.join("hola.rs"),
            "fn main() {\n    println!(\"año\");\n}\n",
        )
        .unwrap();
        fs::write(root.join("logo.png"), [0x89, b'P', b'N', b'G', 0, 0, 1]).unwrap();
        let text = read_file(&root, &root.join("hola.rs")).unwrap();
        assert!(!text.binary && !text.truncated);
        assert!(text.text.unwrap().contains("año"));
        let bin = read_file(&root, &root.join("logo.png")).unwrap();
        assert!(bin.binary && bin.text.is_none());
        let mut big = "ñ".repeat(READ_MAX / 2);
        big.push_str("fin");
        fs::write(root.join("grande.txt"), &big).unwrap();
        let cut = read_file(&root, &root.join("grande.txt")).unwrap();
        assert!(cut.truncated);
        assert!(
            cut.text.unwrap().chars().all(|c| c == 'ñ'),
            "el corte no deja medio carácter"
        );
    }

    #[test]
    fn registra_un_subagente_una_sola_vez_y_con_su_rol() {
        let root = temp("registro");
        fs::create_dir_all(root.join(".claude/agents")).unwrap();
        fs::write(
            root.join(".claude/agents/revisor.md"),
            "---\nname: revisor\ndescription: Revisa la API: endpoints y errores\nmodel: claude-sonnet-5-5\n---\nRevisa.\n",
        )
        .unwrap();
        let task = json!({
            "type": "system", "subtype": "task_started", "task_id": "a1b2c3d4e5",
            "tool_use_id": "toolu_01ABC", "description": "Revisar la API de pagos",
            "subagent_type": "revisor", "task_type": "local_agent", "prompt": "Lee src/api/ y apunta los errores."
        });
        assert_eq!(register_task(&root, "s-1", &task), Ok(true));
        assert_eq!(
            register_task(&root, "s-1", &task),
            Ok(false),
            "retomado: ya está"
        );
        let list = agents(&root);
        assert_eq!(list.len(), 1);
        let a = &list[0];
        assert_eq!(a.file, "revisar-la-api-de-pagos-a1b2c3");
        assert_eq!(a.name, "Revisar la API de pagos");
        assert_eq!(a.role, "Revisa la API: endpoints y errores");
        assert_eq!(a.kind, "revisor");
        assert_eq!(a.task, "a1b2c3d4e5");
        assert_eq!(a.tool_use.as_deref(), Some("toolu_01ABC"));
        assert_eq!(a.session.as_deref(), Some("s-1"));
        assert_eq!(a.prompt, "Lee src/api/ y apunta los errores.");

        // Un tipo de Claude Code conocido trae su rol; las tareas que no son agentes no cuentan.
        let explore = json!({ "task_id": "zz9", "description": "Buscar los tests", "subagent_type": "Explore", "task_type": "local_agent" });
        assert_eq!(register_task(&root, "s-1", &explore), Ok(true));
        let found = agents(&root).into_iter().find(|a| a.task == "zz9").unwrap();
        assert_eq!(
            found.role,
            "Explora el código y busca archivos sin cambiar nada"
        );
        let bash = json!({ "task_id": "b1", "description": "npm test", "task_type": "local_bash" });
        assert_eq!(register_task(&root, "s-1", &bash), Ok(false));
        let evil =
            json!({ "task_id": "../../etc", "description": "x", "task_type": "local_agent" });
        assert_eq!(register_task(&root, "s-1", &evil), Ok(false));
        assert_eq!(agents(&root).len(), 2);

        // El orquestador lo quita borrando su archivo.
        fs::remove_file(root.join(".nexo/agentes/buscar-los-tests-zz9.md")).unwrap();
        assert_eq!(agents(&root).len(), 1);
    }

    #[test]
    fn el_frontmatter_del_registro_aguanta_nombres_con_dos_puntos() {
        let root = temp("comillas");
        let task = json!({ "task_id": "q1", "description": "Paso 2: \"migrar\" la base", "subagent_type": "general-purpose", "task_type": "local_agent" });
        register_task(&root, "s", &task).unwrap();
        let a = &agents(&root)[0];
        assert_eq!(a.name, "Paso 2: \"migrar\" la base");
        assert_eq!(a.kind, "general-purpose");
    }

    #[test]
    fn el_catalogo_separa_plugins_mcp_conexiones_y_skills() {
        let init = json!({ "commands": [
            { "name": "computer-use", "description": "Drives the GUI" },
            { "name": "pdf", "description": "Use this skill for PDF" },
            { "name": "interfaces:better-ui", "description": "(interfaces) Polishes the surfaces" }
        ]});
        let mcp = json!({ "mcpServers": [
            { "name": "plugin:data:hex", "source": "plugin", "status": "needs-auth", "config": { "type": "http", "url": "https://app.hex.tech/mcp" } },
            { "name": "github", "source": "user", "status": "pending", "config": { "type": "http", "url": "https://api.githubcopilot.com/mcp/", "headers": { "Authorization": "secreto" } } },
            { "name": "nexo", "source": "user", "status": "connected", "config": { "type": "stdio", "command": "/home/ana/.local/bin/nexo-mcp", "args": ["--vault", "/x"] } },
            { "name": "propio", "source": "project", "status": "pending", "config": { "type": "stdio", "command": "x" } },
            { "name": "claude.ai Mobbin", "source": "claudeai", "status": "pending", "config": { "type": "claudeai-proxy", "url": "https://api.mobbin.com/mcp" } }
        ]});
        let context = json!({ "skills": { "skillFrontmatter": [
            { "name": "computer-use", "source": "userSettings" },
            { "name": "pdf", "source": "syncedSkills" },
            { "name": "dataviz", "source": "built-in" },
            { "name": "interfaces:better-ui", "source": "plugin", "pluginName": "interfaces" },
            { "name": "data:analyze", "source": "plugin", "pluginName": "data" }
        ]}});
        let mut plugins = HashMap::new();
        plugins.insert(
            "interfaces".to_string(),
            (
                "interfaces@interfaces".to_string(),
                "Better UI".to_string(),
                false,
            ),
        );
        plugins.insert(
            "data".to_string(),
            ("data@synced".to_string(), "Datos".to_string(), true),
        );
        plugins.insert(
            "cloudflare".to_string(),
            ("cloudflare@cloudflare".to_string(), String::new(), false),
        );
        let catalog = build_catalog(
            &init,
            &mcp,
            &context,
            &plugins,
            &["cloudflare@cloudflare".into()],
        );
        let ids: Vec<&str> = catalog.plugins.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "cloudflare@cloudflare",
                "data@synced",
                "interfaces@interfaces"
            ]
        );
        let data = &catalog.plugins[1];
        assert_eq!((data.skills, data.servers, data.synced), (1, 1, true));
        assert_eq!(
            catalog
                .mcp
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["github", "nexo"]
        );
        assert_eq!(catalog.mcp[0].detail, "api.githubcopilot.com");
        assert_eq!(catalog.mcp[1].detail, "nexo-mcp");
        assert!(
            !serde_json::to_string(&catalog).unwrap().contains("secreto"),
            "sin cabeceras"
        );
        assert_eq!(catalog.connectors.len(), 1);
        assert_eq!(
            catalog
                .skills
                .iter()
                .map(|s| (s.name.as_str(), s.source.as_str()))
                .collect::<Vec<_>>(),
            [("computer-use", "user"), ("pdf", "claude.ai")]
        );

        // Apagado por defecto: solo lo encendido queda fuera de las listas de apagar.
        let on = Extras {
            plugins: vec!["data@synced".into()],
            mcp: vec!["nexo".into()],
            connectors: vec![],
            skills: vec!["pdf".into()],
            hooks: vec![],
        };
        let settings = extras_settings(&catalog, &on);
        assert_eq!(settings["enabledPlugins"]["data@synced"], true);
        assert_eq!(settings["enabledPlugins"]["interfaces@interfaces"], false);
        assert_eq!(
            settings["deniedMcpServers"],
            json!([{ "serverName": "github" }, { "serverName": "claude.ai Mobbin" }])
        );
        assert_eq!(settings["skillOverrides"], json!({ "computer-use": "off" }));
        let none = extras_settings(&catalog, &Extras::default());
        assert_eq!(none["deniedMcpServers"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn los_hooks_del_perfil_van_apagados_salvo_con_un_plugin_que_los_trae() {
        let home = temp("hooks");
        let with = home.join("cache/ganchos");
        fs::create_dir_all(with.join("hooks")).unwrap();
        fs::write(with.join("hooks/hooks.json"), "{}").unwrap();
        let without = home.join("cache/limpio");
        fs::create_dir_all(without.join(".claude-plugin")).unwrap();
        fs::write(
            without.join(".claude-plugin/plugin.json"),
            r#"{"name":"limpio"}"#,
        )
        .unwrap();
        fs::create_dir_all(home.join(".claude/plugins")).unwrap();
        fs::write(
            home.join(".claude/plugins/installed_plugins.json"),
            json!({ "plugins": {
                "ganchos@tienda": [{ "scope": "user", "installPath": with }],
                "limpio@tienda": [{ "scope": "user", "installPath": without }]
            }})
            .to_string(),
        )
        .unwrap();
        let synced = home.join(".claude/plugins/synced/org_1/datos~g2/.claude-plugin");
        fs::create_dir_all(&synced).unwrap();
        fs::write(synced.join("plugin.json"), r#"{"hooks":"./hooks.json"}"#).unwrap();
        let hooked = plugins_with_hooks(&home);
        let mut ids: Vec<&str> = hooked.iter().map(String::as_str).collect();
        ids.sort();
        assert_eq!(ids, ["datos@synced", "ganchos@tienda"]);

        let plugin = |id: &str, hooks| PluginItem {
            id: id.into(),
            name: id.split('@').next().unwrap().into(),
            description: String::new(),
            skills: 1,
            servers: 0,
            synced: false,
            hooks,
        };
        let catalog = Catalog {
            plugins: vec![
                plugin("ganchos@tienda", true),
                plugin("limpio@tienda", false),
            ],
            ..Catalog::default()
        };
        let none = extras_settings(&catalog, &Extras::default());
        assert_eq!(none["disableAllHooks"], true, "de serie, sin hooks");
        let clean = Extras {
            plugins: vec!["limpio@tienda".into()],
            ..Extras::default()
        };
        assert_eq!(extras_settings(&catalog, &clean)["disableAllHooks"], true);
        let hooks = Extras {
            plugins: vec!["ganchos@tienda".into()],
            ..Extras::default()
        };
        assert!(
            extras_settings(&catalog, &hooks)["disableAllHooks"].is_null(),
            "un plugin encendido con hooks los necesita"
        );
    }

    #[test]
    fn los_hooks_se_encienden_desde_extras_y_rtk_lo_anade_nexo() {
        let home = temp("hooks-perfil");
        let root = temp("hooks-carpeta");
        fs::create_dir_all(home.join(".claude")).unwrap();
        let orca = "if [ -z \"${HOME-}\" ]; then printf '{}'; else /bin/sh \"$HOME/.orca/agent-hooks/claude-hook.sh\"; fi";
        let group = |command: &str| json!([{ "matcher": "*", "hooks": [{ "type": "command", "command": command }] }]);
        fs::write(
            home.join(".claude/settings.json"),
            json!({ "hooks": { "SessionStart": group(orca), "PreToolUse": group(orca), "Stop": [] } }).to_string(),
        )
        .unwrap();
        fs::create_dir_all(root.join(".claude")).unwrap();
        fs::write(
            root.join(".claude/settings.json"),
            json!({ "hooks": { "PostToolUse": group("TOKEN=secreto ./scripts/formatear.sh") } })
                .to_string(),
        )
        .unwrap();
        fs::write(
            root.join(".claude/settings.local.json"),
            json!({ "hooks": { "PostToolUse": group("/usr/bin/prettier --write"), "Stop": group("npm test") } }).to_string(),
        )
        .unwrap();
        let items = hook_items(
            &home,
            &root,
            Some(PathBuf::from("/home/ana/.local/bin/rtk")),
        );
        let ids: Vec<&str> = items.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["rtk", "perfil", "carpeta"]);
        assert!(items[0].added);
        assert_eq!(
            items[0].settings["PreToolUse"][0]["hooks"][0]["command"],
            "'/home/ana/.local/bin/rtk' hook claude"
        );
        assert_eq!(items[0].settings["PreToolUse"][0]["matcher"], "Bash");
        assert_eq!(
            items[1].detail, "orca · 2 eventos",
            "sin contar los eventos vacíos"
        );
        assert_eq!(items[2].detail, "un script, prettier, npm · 2 eventos");
        assert!(
            !serde_json::to_string(&items).unwrap().contains("secreto"),
            "las órdenes no salen"
        );

        let catalog = Catalog {
            hooks: items,
            ..Catalog::default()
        };
        let none = extras_settings(&catalog, &Extras::default());
        assert_eq!(none["disableAllHooks"], true, "de serie, ninguno");
        assert!(none["hooks"].is_null());
        assert!(hooks_running(&catalog, &Extras::default()).is_empty());

        let rtk = Extras {
            hooks: vec!["rtk".into()],
            ..Extras::default()
        };
        let settings = extras_settings(&catalog, &rtk);
        assert!(settings["disableAllHooks"].is_null());
        assert_eq!(
            settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "'/home/ana/.local/bin/rtk' hook claude"
        );
        // Con uno encendido corren también los de los archivos: Claude Code no los separa.
        let running: Vec<&str> = hooks_running(&catalog, &rtk)
            .iter()
            .map(|h| h.id.as_str())
            .collect();
        assert_eq!(running, ["rtk", "perfil", "carpeta"]);

        let profile = Extras {
            hooks: vec!["perfil".into()],
            ..Extras::default()
        };
        let settings = extras_settings(&catalog, &profile);
        assert!(settings["disableAllHooks"].is_null());
        assert!(
            settings["hooks"].is_null(),
            "rtk no se añade si no se enciende"
        );
        let running: Vec<&str> = hooks_running(&catalog, &profile)
            .iter()
            .map(|h| h.id.as_str())
            .collect();
        assert_eq!(running, ["perfil", "carpeta"]);

        // Si el perfil ya trae el hook de rtk, no se ofrece aparte.
        fs::write(
            home.join(".claude/settings.json"),
            json!({ "hooks": { "PreToolUse": group("rtk hook claude") } }).to_string(),
        )
        .unwrap();
        let ids: Vec<String> = hook_items(&home, &root, Some(PathBuf::from("/x/rtk")))
            .into_iter()
            .map(|h| h.id)
            .collect();
        assert_eq!(ids, ["perfil", "carpeta"]);
        assert!(
            hook_items(&home, &home, None)
                .iter()
                .all(|h| h.id != "carpeta")
        );
    }

    #[test]
    fn sin_respuesta_de_claude_code_lo_apagado_sigue_apagado() {
        let home = temp("perfil");
        let root = temp("perfil-ws");
        fs::create_dir_all(home.join(".claude/skills/mi-skill")).unwrap();
        fs::write(
            home.join(".claude/skills/mi-skill/SKILL.md"),
            "---\nname: mi-skill\n---\n",
        )
        .unwrap();
        fs::create_dir_all(home.join(".claude/skills/synced/b1/pdf")).unwrap();
        fs::write(home.join(".claude/skills/synced/b1/pdf/SKILL.md"), "x").unwrap();
        fs::write(
            home.join(".claude/settings.json"),
            r#"{"enabledPlugins":{"ui@tienda":true}}"#,
        )
        .unwrap();
        fs::create_dir_all(home.join(".claude/plugins")).unwrap();
        fs::write(
            home.join(".claude/plugins/installed_plugins.json"),
            r#"{"plugins":{"ui@tienda":[{"scope":"user","installPath":"/no"}],"viejo@tienda":[{"scope":"user","installPath":"/no"}]}}"#,
        )
        .unwrap();
        fs::write(
            home.join(".claude.json"),
            r#"{"mcpServers":{"github":{"type":"http","headers":{"Authorization":"x"}}},"claudeAiMcpEverConnected":["claude.ai Mobbin"]}"#,
        )
        .unwrap();
        let catalog = fallback_catalog(&home, &root, "sin respuesta".into());
        let ids: Vec<&str> = catalog.plugins.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["ui@tienda"], "solo los que están encendidos");
        assert_eq!(catalog.mcp[0].name, "github");
        assert_eq!(catalog.connectors[0].name, "claude.ai Mobbin");
        let mut names: Vec<&str> = catalog.skills.iter().map(|s| s.name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["mi-skill", "pdf"]);
        let settings = extras_settings(&catalog, &Extras::default());
        assert_eq!(settings["enabledPlugins"]["ui@tienda"], false);
        assert_eq!(
            settings["deniedMcpServers"],
            json!([{ "serverName": "github" }, { "serverName": "claude.ai Mobbin" }])
        );
        assert_eq!(settings["skillOverrides"]["pdf"], "off");
        assert_eq!(settings["disableClaudeAiConnectors"], true);
        assert!(
            !serde_json::to_string(&catalog)
                .unwrap()
                .contains("Authorization")
        );
    }

    #[test]
    fn la_configuracion_se_guarda_y_conserva_lo_que_no_cambia() {
        let root = temp("config");
        update_config(&root, |c| c.session = Some("abc".into())).unwrap();
        update_config(&root, |c| c.ui = json!({ "layout": "mosaico" })).unwrap();
        let config = read_config(&root);
        assert_eq!(config.session.as_deref(), Some("abc"));
        assert_eq!(config.ui["layout"], "mosaico");
        assert!(root.join(".nexo/.gitignore").is_file());
        assert!(root.join(".nexo/agentes").is_dir());
    }
}
