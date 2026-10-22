//! Workspaces de la vista Agentes: carpetas de proyecto que el usuario elige o crea, en las
//! que trabaja con terminales (`terminales.rs`, `docs/spec-terminales-workspaces.md`). Aquí
//! está lo que no son las terminales: la lista de workspaces conocidos, lo que guarda cada uno
//! y el explorador y el lector de archivos.
//!
//! - Lista: `<configuración de nexo>/workspaces.json` (el actual, los recientes y los del
//!   panel). Solo se puede leer o lanzar algo en una carpeta de esa lista: así una ruta de la
//!   interfaz no abre cualquier sitio del disco.
//! - Cada workspace guarda en `<carpeta>/.nexo/workspace.json` cómo dejó el usuario sus
//!   pestañas y ventanas (`ui`). Lo que hubiera de antes (la sesión del orquestador, sus
//!   extras) se conserva tal cual y no se usa.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager};

pub(crate) const NEXO_DIR: &str = ".nexo";
const CONFIG_FILE: &str = "workspace.json";
const LIST_FILE: &str = "workspaces.json";
const RECENT_MAX: usize = 12;
/// Lo que se enseña de un archivo como mucho: el resto se corta (y se dice).
const READ_MAX: usize = 1024 * 1024;
/// Entradas de una carpeta como mucho en el explorador.
const LIST_MAX: usize = 2000;
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
    /// Los del panel de Agentes, en su orden: entra cada uno que se abre o se crea y sale al
    /// quitarlo (sigue en `recent`, así que `ws_root` lo sigue aceptando).
    #[serde(default)]
    open: Vec<PathBuf>,
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
    /// Terminales abiertas en esta apertura de nexo (solo en los del panel; en los demás, 0).
    pub terminals: usize,
}

#[derive(Serialize)]
pub struct WsList {
    current: Option<WsInfo>,
    recent: Vec<WsInfo>,
    /// Los del panel de Agentes, en su orden.
    open: Vec<WsInfo>,
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
    let mut list: List = list_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    migrate(&mut list);
    list
}

/// Una lista de antes del panel no tiene `open`: el que estaba abierto pasa a estar en él. Si
/// el usuario los quitó todos, tampoco hay actual y no vuelve.
fn migrate(list: &mut List) {
    if list.open.is_empty()
        && let Some(current) = &list.current
    {
        list.open.push(current.clone());
    }
}

/// Lo añade al final del panel si no estaba.
fn add_open(list: &mut List, root: &Path) {
    if !list.open.iter().any(|p| p == root) {
        list.open.push(root.to_path_buf());
    }
}

/// Lo quita del panel; si era el actual, ya no hay actual.
fn close_open(list: &mut List, root: &Path) {
    list.open.retain(|p| p != root);
    if list.current.as_deref() == Some(root) {
        list.current = None;
    }
}

/// Deja `recent` en `RECENT_MAX` quitando los más viejos que no están en el panel: uno del
/// panel que saliera de `recent` dejaría de pasar por `ws_root`.
fn trim_recent(list: &mut List) {
    let mut extra = list.recent.len().saturating_sub(RECENT_MAX);
    let open = &list.open;
    let mut keep = vec![true; list.recent.len()];
    for (i, r) in list.recent.iter().enumerate().rev() {
        if extra == 0 {
            break;
        }
        if !open.contains(&r.path) {
            keep[i] = false;
            extra -= 1;
        }
    }
    let mut flags = keep.into_iter();
    list.recent.retain(|_| flags.next().unwrap_or(true));
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
        terminals: 0,
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

/// Prepara `.nexo/` (con un `.gitignore` que lo deja fuera de git si la carpeta es nueva).
fn prepare(root: &Path) -> Result<(), String> {
    let dir = root.join(NEXO_DIR);
    if !dir.is_dir() {
        fs::create_dir_all(&dir).map_err(|e| format!("No se pudo crear {NEXO_DIR}/: {e}"))?;
        let _ = fs::write(
            dir.join(".gitignore"),
            "# Lo de nexo (pestañas, ventanas): no va en git.\n*\n",
        );
    }
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
    list.current = Some(root.to_path_buf());
    add_open(&mut list, root);
    trim_recent(&mut list);
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
        open: list
            .open
            .iter()
            .filter_map(|p| list.recent.iter().find(|r| &r.path == p))
            .map(|r| WsInfo {
                terminals: app
                    .state::<crate::terminales::Terminales>()
                    .of(Some(&r.path))
                    .len(),
                ..info_of(r, current)
            })
            .collect(),
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

/// Quita un workspace de la lista (la carpeta no se toca), también del panel, y cierra y
/// olvida sus terminales.
#[tauri::command]
pub fn workspace_forget(app: AppHandle, path: String) -> Result<(), String> {
    let target = Path::new(&path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&path));
    let mut list = read_list(&app);
    list.recent.retain(|r| r.path != target);
    close_open(&mut list, &target);
    write_list(&app, &list)?;
    crate::terminales::leave_workspace(&app, &target, true);
    Ok(())
}

/// Quita un workspace del panel de Agentes: cierra sus terminales (si alguna trabajaba, la
/// interfaz ya lo preguntó) y sigue en Recientes (y en `ws_root`); al volver a abrirlo se
/// recuperan. La carpeta no se toca.
#[tauri::command]
pub fn workspace_close(app: AppHandle, path: String) -> Result<(), String> {
    let target = Path::new(&path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&path));
    let mut list = read_list(&app);
    close_open(&mut list, &target);
    write_list(&app, &list)?;
    crate::terminales::leave_workspace(&app, &target, false);
    Ok(())
}

/// Los workspaces del panel (nombre, carpeta y si es el actual), para la orden de control.
pub(crate) fn panel(app: &AppHandle) -> Vec<(String, PathBuf, bool)> {
    let list = read_list(app);
    list.open
        .iter()
        .filter_map(|p| list.recent.iter().find(|r| &r.path == p))
        .map(|r| {
            (
                r.name.clone(),
                r.path.clone(),
                list.current.as_deref() == Some(r.path.as_path()),
            )
        })
        .collect()
}

// ---------------------------------------------------------------- configuración

#[derive(Default, Serialize, Deserialize, Clone)]
pub struct WsConfig {
    /// Cómo dejó el usuario el workspace (pestañas, ventanas, carpetas abiertas). Lo escribe la
    /// interfaz tal cual.
    #[serde(default)]
    pub ui: Value,
    /// Lo demás que hubiera en el archivo (de antes: la sesión del orquestador, sus extras): se
    /// conserva tal cual.
    #[serde(flatten)]
    pub other: Map<String, Value>,
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

/// La configuración se escribe desde varios sitios: un cerrojo para que una escritura no pise
/// otra a medias.
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
    ui: Value,
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
        ui: config.ui,
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

pub(crate) fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Escribe un archivo que puede llevar claves (la salida de una terminal): solo lo lee el
/// usuario y se cambia de una vez, por un temporal.
pub(crate) fn write_private(path: &Path, data: impl AsRef<[u8]>) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp).map_err(|e| e.to_string())?;
    // `mode` solo vale al crearlo: un temporal que quedara de antes también se cierra.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    }
    file.write_all(data.as_ref()).map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(&tmp, path).map_err(|e| e.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("nexo-ws-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root.canonicalize().unwrap()
    }

    fn recent(path: &str) -> Recent {
        Recent {
            path: PathBuf::from(path),
            name: folder_name(Path::new(path)),
            opened: 0,
        }
    }

    #[test]
    fn el_panel_guarda_los_abiertos_en_su_orden_y_quitar_no_toca_recientes() {
        let mut list = List::default();
        for p in ["/w/a", "/w/b", "/w/a"] {
            add_open(&mut list, Path::new(p));
        }
        assert_eq!(list.open, [PathBuf::from("/w/a"), PathBuf::from("/w/b")]);

        list.recent = vec![recent("/w/b"), recent("/w/a")];
        list.current = Some(PathBuf::from("/w/b"));
        close_open(&mut list, Path::new("/w/b"));
        assert_eq!(list.open, [PathBuf::from("/w/a")]);
        assert_eq!(list.current, None);
        assert_eq!(
            list.recent.len(),
            2,
            "quitar del panel deja el workspace en Recientes"
        );

        // Quitar uno que no es el actual no cambia el actual.
        list.current = Some(PathBuf::from("/w/a"));
        close_open(&mut list, Path::new("/w/b"));
        assert_eq!(list.current.as_deref(), Some(Path::new("/w/a")));
    }

    #[test]
    fn una_lista_de_antes_del_panel_conserva_el_abierto() {
        let mut old: List = serde_json::from_str(
            r#"{"current":"/w/a","recent":[{"path":"/w/a","name":"a","opened":1}]}"#,
        )
        .unwrap();
        migrate(&mut old);
        assert_eq!(old.open, [PathBuf::from("/w/a")]);

        // Los quitó todos: sin actual, no vuelve ninguno.
        let mut empty: List = serde_json::from_str(
            r#"{"current":null,"recent":[{"path":"/w/a","name":"a","opened":1}],"open":[]}"#,
        )
        .unwrap();
        migrate(&mut empty);
        assert!(empty.open.is_empty());
    }

    #[test]
    fn recortar_recientes_nunca_deja_fuera_uno_del_panel() {
        let mut list = List::default();
        // Del más nuevo al más viejo; el más viejo de todos está en el panel.
        list.recent = (0..RECENT_MAX + 3)
            .map(|i| recent(&format!("/w/{i}")))
            .collect();
        let oldest = PathBuf::from(format!("/w/{}", RECENT_MAX + 2));
        list.open = vec![oldest.clone()];
        trim_recent(&mut list);
        assert_eq!(list.recent.len(), RECENT_MAX);
        assert!(list.recent.iter().any(|r| r.path == oldest));
        assert_eq!(
            list.recent[0].path,
            PathBuf::from("/w/0"),
            "los nuevos se quedan"
        );
        assert!(
            !list
                .recent
                .iter()
                .any(|r| r.path == Path::new(&format!("/w/{}", RECENT_MAX + 1)))
        );

        // Con todos en el panel no se quita ninguno.
        let mut full = List::default();
        full.recent = (0..RECENT_MAX + 2)
            .map(|i| recent(&format!("/p/{i}")))
            .collect();
        full.open = full.recent.iter().map(|r| r.path.clone()).collect();
        trim_recent(&mut full);
        assert_eq!(full.recent.len(), RECENT_MAX + 2);
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
    fn la_configuracion_se_guarda_y_conserva_lo_que_no_cambia() {
        let root = temp("config");
        // Lo de antes (la sesión del orquestador, sus extras) se queda tal cual.
        prepare(&root).unwrap();
        fs::write(
            root.join(".nexo/workspace.json"),
            r#"{ "session": "abc", "extras": { "plugins": ["x@y"] }, "ui": {} }"#,
        )
        .unwrap();
        update_config(&root, |c| c.ui = serde_json::json!({ "layout": "mosaico" })).unwrap();
        let config = read_config(&root);
        assert_eq!(config.ui["layout"], "mosaico");
        assert_eq!(config.other["session"], "abc");
        assert_eq!(config.other["extras"]["plugins"][0], "x@y");
        assert!(root.join(".nexo/.gitignore").is_file());
    }
}
