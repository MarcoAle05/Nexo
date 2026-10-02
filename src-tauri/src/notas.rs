//! Apartado Notas: notas del usuario en `wiki/notas/`, organizadas en temas y subtemas.
//!
//! - Un **tema** es una carpeta con su `_index.md` (frontmatter `type: tema`, `title`).
//!   nexo mantiene en cada índice la sección entre `<!-- nexo:contenido -->` y
//!   `<!-- /nexo:contenido -->` con enlaces a sus subtemas y notas; el resto del índice
//!   (la descripción) es libre. Así todo el árbol aparece en el grafo.
//! - Una **nota** es un `.md` con frontmatter `title`, `type` (`texto`, `lista` o `fecha`),
//!   `date` (notas con fecha), `created` y `updated`; el cuerpo es Markdown normal
//!   (las listas usan `- [ ]` / `- [x]`).
//! - El escritorio de notas (cartas abiertas, posición, tamaño y capa) vive en
//!   `.nexo/escritorio.json`, para que también Claude Code pueda abrir notas en él.
//!
//! Los archivos los puede crear o editar cualquiera (la interfaz, Claude Code, Obsidian):
//! nexo solo regenera los índices cuando cambia algo (`sync`).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

use crate::fichas::slug;
use crate::vault::vault_root;

pub const DIR: &str = "wiki/notas";
const INDEX: &str = "_index.md";
const START: &str = "<!-- nexo:contenido -->";
const END: &str = "<!-- /nexo:contenido -->";
pub const KINDS: [&str; 3] = ["texto", "lista", "fecha"];
const DESK: &str = ".nexo/escritorio.json";

fn notes_dir(root: &Path) -> PathBuf {
    root.join(DIR)
}

/// Ruta relativa a `wiki/notas/` → ruta real, sin salir de la carpeta.
fn resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim_matches('/');
    if rel.split('/').any(|p| p == ".." || p.starts_with('.')) || rel.contains('\\') {
        return Err("Ruta de nota no válida.".into());
    }
    Ok(notes_dir(root).join(rel))
}

fn rel_of(root: &Path, path: &Path) -> String {
    path.strip_prefix(notes_dir(root))
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

// ---------------------------------------------------------------- frontmatter

/// Frontmatter simple (`clave: valor` por línea) en orden, y el cuerpo.
pub fn parse(text: &str) -> (Vec<(String, String)>, String) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (Vec::new(), text.to_string());
    };
    let Some(end) = rest.find("\n---") else {
        return (Vec::new(), text.to_string());
    };
    let meta = rest[..end]
        .lines()
        .filter_map(|line| {
            let (k, v) = line.split_once(':')?;
            let k = k.trim();
            (!k.is_empty() && !line.starts_with([' ', '-', '#'])).then(|| {
                (
                    k.to_string(),
                    v.trim().trim_matches('"').trim_matches('\'').to_string(),
                )
            })
        })
        .collect();
    let body = rest[end + 4..]
        .trim_start_matches('-')
        .trim_start_matches(['\r', '\n'])
        .to_string();
    (meta, body)
}

fn get<'a>(meta: &'a [(String, String)], key: &str) -> Option<&'a str> {
    meta.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
}

fn set(meta: &mut Vec<(String, String)>, key: &str, value: Option<&str>) {
    match value.filter(|v| !v.trim().is_empty()) {
        Some(v) => match meta.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = v.trim().to_string(),
            None => meta.push((key.to_string(), v.trim().to_string())),
        },
        None => meta.retain(|(k, _)| k != key),
    }
}

/// Valor seguro en una línea de frontmatter (entre comillas si hace falta).
fn yaml_value(v: &str) -> String {
    let v = v.replace(['\n', '\r'], " ");
    if v.contains(": ")
        || v.starts_with(['[', '{', '*', '&', '!', '|', '>', '\'', '"', '%', '@', '#'])
    {
        format!("\"{}\"", v.replace('"', "'"))
    } else {
        v
    }
}

fn render(meta: &[(String, String)], body: &str) -> String {
    let mut out = String::from("---\n");
    for (k, v) in meta {
        out.push_str(&format!("{k}: {}\n", yaml_value(v)));
    }
    out.push_str("---\n\n");
    out.push_str(body.trim_start_matches(['\r', '\n']));
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

/// «push-pull-legs» → «Push pull legs».
fn pretty(stem: &str) -> String {
    let text = stem.replace(['-', '_'], " ");
    let mut chars = text.trim().chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

// ---------------------------------------------------------------- árbol

#[derive(Serialize, Clone)]
pub struct NoteInfo {
    /// Relativa a `wiki/notas/`, p. ej. `rutinas/pecho/press-con-mancuernas.md`.
    path: String,
    title: String,
    kind: String,
    date: Option<String>,
    updated: Option<String>,
    /// Primeras palabras del cuerpo, sin Markdown.
    preview: String,
    /// Casillas marcadas / totales.
    done: usize,
    total: usize,
}

#[derive(Serialize, Clone)]
pub struct Topic {
    /// Relativa a `wiki/notas/` (vacía para la raíz).
    path: String,
    title: String,
    description: String,
    topics: Vec<Topic>,
    notes: Vec<NoteInfo>,
}

fn is_task(line: &str) -> Option<bool> {
    let t = line.trim_start();
    let t = t
        .strip_prefix("- ")
        .or_else(|| t.strip_prefix("* "))
        .or_else(|| t.strip_prefix("+ "))?;
    if t.starts_with("[ ]") {
        Some(false)
    } else if t.starts_with("[x]") || t.starts_with("[X]") {
        Some(true)
    } else {
        None
    }
}

fn preview(body: &str) -> String {
    let text: String = body
        .lines()
        .map(|l| {
            l.trim()
                .trim_start_matches('#')
                .trim_start_matches(['-', '*', '+', '>'])
                .trim()
                .trim_start_matches("[ ]")
                .trim_start_matches("[x]")
                .trim_start_matches("[X]")
                .trim()
        })
        .filter(|l| !l.is_empty() && !l.starts_with("<!--"))
        .collect::<Vec<_>>()
        .join(" · ")
        .replace(['*', '`', '[', ']'], "");
    let mut out: String = text.chars().take(140).collect();
    if text.chars().count() > 140 {
        out.push('…');
    }
    out
}

fn note_info(root: &Path, path: &Path) -> Option<NoteInfo> {
    let text = fs::read_to_string(path).ok()?;
    let (meta, body) = parse(&text);
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let tasks: Vec<bool> = body.lines().filter_map(is_task).collect();
    Some(NoteInfo {
        path: rel_of(root, path),
        title: get(&meta, "title")
            .map(String::from)
            .unwrap_or_else(|| pretty(&stem)),
        kind: get(&meta, "type")
            .filter(|k| KINDS.contains(k))
            .unwrap_or("texto")
            .to_string(),
        date: get(&meta, "date").map(String::from),
        updated: get(&meta, "updated").map(String::from),
        preview: preview(&body),
        done: tasks.iter().filter(|d| **d).count(),
        total: tasks.len(),
    })
}

fn topic_title(dir: &Path, is_root: bool) -> String {
    let index = fs::read_to_string(dir.join(INDEX)).unwrap_or_default();
    let (meta, _) = parse(&index);
    get(&meta, "title").map(String::from).unwrap_or_else(|| {
        if is_root {
            "Notas".into()
        } else {
            pretty(&dir.file_name().unwrap_or_default().to_string_lossy())
        }
    })
}

/// Descripción libre del índice: lo que no es el título ni la sección de nexo.
fn description(body: &str) -> String {
    let before = body.split(START).next().unwrap_or(body);
    let after = body.split_once(END).map(|(_, a)| a).unwrap_or("");
    let text = format!("{before}\n{after}");
    text.lines()
        .filter(|l| !l.starts_with("# "))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn read_topic(root: &Path, dir: &Path) -> Topic {
    let is_root = dir == notes_dir(root);
    let index = fs::read_to_string(dir.join(INDEX)).unwrap_or_default();
    let (_, body) = parse(&index);
    let mut topics = Vec::new();
    let mut notes = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if name.starts_with('.') || name == INDEX {
                continue;
            }
            if path.is_dir() {
                topics.push(read_topic(root, &path));
            } else if name.to_lowercase().ends_with(".md")
                && let Some(info) = note_info(root, &path)
            {
                notes.push(info);
            }
        }
    }
    topics.sort_by_key(|t| t.title.to_lowercase());
    // Las notas con fecha, por fecha; el resto, por título.
    notes.sort_by(|a, b| match (&a.date, &b.date) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
    });
    Topic {
        path: if is_root {
            String::new()
        } else {
            rel_of(root, dir)
        },
        title: topic_title(dir, is_root),
        description: description(&body),
        topics,
        notes,
    }
}

// ---------------------------------------------------------------- índices

fn index_link(rel_dir: &str, file: &str) -> String {
    let base = if rel_dir.is_empty() {
        "notas".to_string()
    } else {
        format!("notas/{rel_dir}")
    };
    format!("{base}/{}", file.trim_end_matches(".md"))
}

fn index_text(dir: &Path, topic: &Topic, is_root: bool) -> String {
    let existing = fs::read_to_string(dir.join(INDEX)).unwrap_or_default();
    let (mut meta, body) = parse(&existing);
    set(&mut meta, "type", Some("tema"));
    set(&mut meta, "title", Some(&topic.title));
    let mut section = format!("{START}\n");
    if topic.topics.is_empty() && topic.notes.is_empty() {
        section.push_str("_Sin notas todavía._\n");
    }
    for t in &topic.topics {
        let name = t.path.rsplit('/').next().unwrap_or(&t.path);
        section.push_str(&format!(
            "- [[{}|{}]]\n",
            index_link(&topic.path, &format!("{name}/_index")),
            t.title
        ));
    }
    for n in &topic.notes {
        let file = n.path.rsplit('/').next().unwrap_or(&n.path);
        section.push_str(&format!(
            "- [[{}|{}]]\n",
            index_link(&topic.path, file),
            n.title
        ));
    }
    section.push_str(END);
    let desc = if body.contains(START) {
        topic.description.clone()
    } else {
        description(&body)
    };
    let intro = if is_root && desc.is_empty() {
        "Temas y notas del apartado Notas de nexo.".to_string()
    } else {
        desc
    };
    let mut doc = format!("# {}\n\n", topic.title);
    if !intro.is_empty() {
        doc.push_str(&intro);
        doc.push_str("\n\n");
    }
    doc.push_str(&section);
    render(&meta, &doc)
}

fn sync_topic(dir: &Path, topic: &Topic, is_root: bool) -> Result<bool, String> {
    let text = index_text(dir, topic, is_root);
    let mut changed = false;
    if fs::read_to_string(dir.join(INDEX)).ok().as_deref() != Some(text.as_str()) {
        fs::write(dir.join(INDEX), text).map_err(|e| e.to_string())?;
        changed = true;
    }
    for t in &topic.topics {
        let name = t.path.rsplit('/').next().unwrap_or(&t.path);
        changed |= sync_topic(&dir.join(name), t, false)?;
    }
    Ok(changed)
}

/// Crea `wiki/notas/` si falta y regenera los índices que no estén al día.
/// Devuelve si escribió algo.
pub fn sync(root: &Path) -> Result<bool, String> {
    let dir = notes_dir(root);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let tree = read_topic(root, &dir);
    let mut changed = sync_topic(&dir, &tree, true)?;

    let master = root.join("wiki/_master-index.md");
    let text = fs::read_to_string(&master).unwrap_or_else(|_| "# Índice maestro\n".into());
    let link = "[[notas/_index|Notas]]";
    if !text.contains(link) {
        fs::write(&master, format!("{}\n- {link}\n", text.trim_end()))
            .map_err(|e| e.to_string())?;
        changed = true;
    }
    Ok(changed)
}

// ---------------------------------------------------------------- comandos

#[tauri::command]
pub fn notes_tree(app: AppHandle) -> Result<Topic, String> {
    let root = vault_root(&app)?;
    sync(&root)?;
    Ok(read_topic(&root, &notes_dir(&root)))
}

#[derive(Serialize)]
pub struct NoteDoc {
    path: String,
    title: String,
    kind: String,
    date: Option<String>,
    updated: Option<String>,
    body: String,
}

#[tauri::command]
pub fn note_read(app: AppHandle, path: String) -> Result<NoteDoc, String> {
    let root = vault_root(&app)?;
    let file = resolve(&root, &path)?;
    let text = fs::read_to_string(&file).map_err(|_| "La nota ya no existe.".to_string())?;
    let (meta, body) = parse(&text);
    let stem = file.file_stem().unwrap_or_default().to_string_lossy();
    Ok(NoteDoc {
        title: get(&meta, "title")
            .map(String::from)
            .unwrap_or_else(|| pretty(&stem)),
        kind: get(&meta, "type")
            .filter(|k| KINDS.contains(k))
            .unwrap_or("texto")
            .to_string(),
        date: get(&meta, "date").map(String::from),
        updated: get(&meta, "updated").map(String::from),
        body,
        path,
    })
}

/// Nombre libre dentro de `dir`: `base`, `base-2`, `base-3`…
fn free_name(dir: &Path, base: &str, ext: &str) -> String {
    let mut name = format!("{base}{ext}");
    let mut n = 2;
    while dir.join(&name).exists() {
        name = format!("{base}-{n}{ext}");
        n += 1;
    }
    name
}

fn slug_or(text: &str, fallback: &str) -> String {
    let s = slug(text);
    if s == "fuente" && !text.to_lowercase().contains("fuente") {
        fallback.into()
    } else {
        s
    }
}

fn topic_dir(root: &Path, parent: &str) -> Result<PathBuf, String> {
    let dir = resolve(root, parent)?;
    if !dir.is_dir() {
        return Err("Ese tema ya no existe.".into());
    }
    Ok(dir)
}

#[tauri::command]
pub fn note_create(
    app: AppHandle,
    parent: String,
    title: String,
    kind: String,
    date: Option<String>,
) -> Result<String, String> {
    let root = vault_root(&app)?;
    sync(&root)?;
    let dir = topic_dir(&root, &parent)?;
    let title = title.trim();
    if title.is_empty() {
        return Err("La nota necesita un título.".into());
    }
    let kind = if KINDS.contains(&kind.as_str()) {
        kind
    } else {
        "texto".into()
    };
    let name = free_name(&dir, &slug_or(title, "nota"), ".md");
    let stamp = now();
    let mut meta = vec![
        ("title".to_string(), title.to_string()),
        ("type".to_string(), kind.clone()),
    ];
    if kind == "fecha" {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        set(&mut meta, "date", Some(date.as_deref().unwrap_or(&today)));
    }
    meta.push(("created".into(), stamp.clone()));
    meta.push(("updated".into(), stamp));
    fs::write(dir.join(&name), render(&meta, "")).map_err(|e| e.to_string())?;
    sync(&root)?;
    Ok(rel_of(&root, &dir.join(name)))
}

#[derive(Deserialize)]
pub struct NoteChanges {
    title: Option<String>,
    kind: Option<String>,
    /// `Some("")` quita la fecha.
    date: Option<String>,
    body: Option<String>,
}

#[tauri::command]
pub fn note_save(app: AppHandle, path: String, changes: NoteChanges) -> Result<NoteDoc, String> {
    let root = vault_root(&app)?;
    let file = resolve(&root, &path)?;
    let text = fs::read_to_string(&file).map_err(|_| "La nota ya no existe.".to_string())?;
    let (mut meta, mut body) = parse(&text);
    if let Some(title) = changes.title.as_deref().map(str::trim)
        && !title.is_empty()
    {
        set(&mut meta, "title", Some(title));
    }
    if let Some(kind) = changes.kind.as_deref()
        && KINDS.contains(&kind)
    {
        set(&mut meta, "type", Some(kind));
    }
    if let Some(date) = changes.date.as_deref() {
        set(&mut meta, "date", Some(date));
    }
    if let Some(new_body) = changes.body {
        body = new_body;
    }
    set(&mut meta, "updated", Some(&now()));
    fs::write(&file, render(&meta, &body)).map_err(|e| e.to_string())?;
    sync(&root)?;
    note_read(app, path)
}

#[tauri::command]
pub fn topic_create(app: AppHandle, parent: String, title: String) -> Result<String, String> {
    let root = vault_root(&app)?;
    sync(&root)?;
    let dir = topic_dir(&root, &parent)?;
    let title = title.trim();
    if title.is_empty() {
        return Err("El tema necesita un nombre.".into());
    }
    let name = free_name(&dir, &slug_or(title, "tema"), "");
    let new = dir.join(&name);
    fs::create_dir_all(&new).map_err(|e| e.to_string())?;
    let meta = vec![
        ("type".to_string(), "tema".to_string()),
        ("title".to_string(), title.to_string()),
    ];
    fs::write(new.join(INDEX), render(&meta, &format!("# {title}\n")))
        .map_err(|e| e.to_string())?;
    sync(&root)?;
    Ok(rel_of(&root, &new))
}

#[tauri::command]
pub fn topic_rename(app: AppHandle, path: String, title: String) -> Result<(), String> {
    let root = vault_root(&app)?;
    let dir = topic_dir(&root, &path)?;
    let title = title.trim();
    if title.is_empty() || path.is_empty() {
        return Err("Escribe un nombre para el tema.".into());
    }
    let index = fs::read_to_string(dir.join(INDEX)).unwrap_or_default();
    let (mut meta, body) = parse(&index);
    set(&mut meta, "title", Some(title));
    fs::write(dir.join(INDEX), render(&meta, &body)).map_err(|e| e.to_string())?;
    sync(&root)?;
    Ok(())
}

/// Manda a la papelera una nota o un tema entero (con todo lo que contiene).
#[tauri::command]
pub fn notes_delete(app: AppHandle, path: String) -> Result<(), String> {
    let root = vault_root(&app)?;
    if path.trim_matches('/').is_empty() {
        return Err("No se puede borrar la raíz de Notas.".into());
    }
    let target = resolve(&root, &path)?;
    if !target.exists() {
        return Err("Ya no existe.".into());
    }
    trash::delete(&target).map_err(|e| format!("No se pudo mover a la papelera: {e}"))?;
    sync(&root)?;
    Ok(())
}

// ---------------------------------------------------------------- escritorio

/// Última escritura del escritorio hecha por nexo, para que el vigilante no la confunda
/// con un cambio de fuera (de Claude Code).
static OWN_DESK_WRITE: Mutex<Option<SystemTime>> = Mutex::new(None);

pub fn desk_path(root: &Path) -> PathBuf {
    root.join(DESK)
}

/// `true` si la modificación `mtime` del escritorio la hizo nexo.
pub fn desk_written_by_us(mtime: SystemTime) -> bool {
    OWN_DESK_WRITE
        .lock()
        .ok()
        .and_then(|g| *g)
        .is_some_and(|t| t == mtime)
}

#[tauri::command]
pub fn desk_load(app: AppHandle) -> Result<Value, String> {
    let root = vault_root(&app)?;
    let text = fs::read_to_string(desk_path(&root)).unwrap_or_else(|_| "{}".into());
    Ok(serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({})))
}

#[tauri::command]
pub fn desk_save(app: AppHandle, state: Value) -> Result<(), String> {
    let root = vault_root(&app)?;
    let path = desk_path(&root);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&path, serde_json::to_string_pretty(&state).unwrap()).map_err(|e| e.to_string())?;
    if let Ok(mtime) = fs::metadata(&path).and_then(|m| m.modified())
        && let Ok(mut own) = OWN_DESK_WRITE.lock()
    {
        *own = Some(mtime);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nexo-notas-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("wiki")).unwrap();
        dir
    }

    #[test]
    fn frontmatter_ida_y_vuelta() {
        let text = render(
            &[
                ("title".into(), "Compras: octubre".into()),
                ("type".into(), "lista".into()),
            ],
            "- [ ] Leche\n- [x] Pan\n",
        );
        let (meta, body) = parse(&text);
        assert_eq!(get(&meta, "title"), Some("Compras: octubre"));
        assert_eq!(get(&meta, "type"), Some("lista"));
        assert_eq!(body, "- [ ] Leche\n- [x] Pan\n");
    }

    #[test]
    fn indices_de_temas_y_subtemas() {
        let root = tmp();
        sync(&root).unwrap();
        let dir = notes_dir(&root);
        fs::create_dir_all(dir.join("rutinas/pecho")).unwrap();
        fs::write(
            dir.join("rutinas/pecho/press.md"),
            render(
                &[
                    ("title".into(), "Press con mancuernas".into()),
                    ("type".into(), "lista".into()),
                ],
                "- [x] Serie 1\n- [ ] Serie 2\n",
            ),
        )
        .unwrap();
        assert!(sync(&root).unwrap());
        assert!(!sync(&root).unwrap(), "la segunda pasada no cambia nada");

        let tree = read_topic(&root, &dir);
        assert_eq!(tree.title, "Notas");
        let rutinas = &tree.topics[0];
        assert_eq!(rutinas.title, "Rutinas");
        let pecho = &rutinas.topics[0];
        assert_eq!(pecho.path, "rutinas/pecho");
        assert_eq!(pecho.notes[0].title, "Press con mancuernas");
        assert_eq!((pecho.notes[0].done, pecho.notes[0].total), (1, 2));

        let root_index = fs::read_to_string(dir.join(INDEX)).unwrap();
        assert!(root_index.contains("[[notas/rutinas/_index|Rutinas]]"));
        let pecho_index = fs::read_to_string(dir.join("rutinas/pecho/_index.md")).unwrap();
        assert!(pecho_index.contains("[[notas/rutinas/pecho/press|Press con mancuernas]]"));
        let master = fs::read_to_string(root.join("wiki/_master-index.md")).unwrap();
        assert!(master.contains("[[notas/_index|Notas]]"));

        // La descripción del usuario en un índice se conserva.
        let index = dir.join("rutinas/_index.md");
        let text = fs::read_to_string(&index).unwrap().replace(
            "# Rutinas\n\n",
            "# Rutinas\n\nMi plan de entrenamiento.\n\n",
        );
        fs::write(&index, text).unwrap();
        sync(&root).unwrap();
        assert!(
            fs::read_to_string(&index)
                .unwrap()
                .contains("Mi plan de entrenamiento.")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rutas_fuera_de_notas_se_rechazan() {
        let root = PathBuf::from("/tmp/v");
        assert!(resolve(&root, "../raw/x.md").is_err());
        assert!(resolve(&root, ".nexo/x").is_err());
        assert!(resolve(&root, "rutinas/pecho.md").is_ok());
    }
}
