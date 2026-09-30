//! Fuentes del cuaderno: todo lo que hay dentro de `raw/` del vault.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::markitdown::{MARKDOWN_DIR, converted_path, is_convertible, is_fresh};
use crate::vault::{resolve_inside, vault_dir};

fn raw_dir(app: &AppHandle) -> Result<PathBuf, String> {
    vault_dir(app, "raw")
}

#[derive(Serialize)]
pub struct Source {
    name: String,
    /// Ruta relativa a `raw/`, con `/` como separador.
    path: String,
    kind: &'static str,
    size: u64,
    /// Milisegundos desde 1970, para ordenar y mostrar.
    modified: u64,
    /// Se puede convertir a Markdown con MarkItDown (PDF, Office, EPUB).
    convertible: bool,
    /// Ya tiene una conversión al día en `raw/markdown/`.
    converted: bool,
}

#[derive(Serialize)]
pub struct AddResult {
    added: Vec<Source>,
    skipped: Vec<String>,
    /// Archivos convertibles rechazados porque MarkItDown no está disponible.
    rejected: Vec<String>,
}

fn kind_of(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => "pdf",
        "html" | "htm" | "url" | "webloc" => "web",
        "md" | "markdown" | "txt" => "nota",
        "mp3" | "m4a" | "wav" | "ogg" | "flac" | "mp4" | "mov" | "mkv" | "webm" | "png" | "jpg"
        | "jpeg" | "gif" | "webp" | "svg" => "media",
        _ => "otro",
    }
}

fn to_source(raw: &Path, path: &Path) -> Option<Source> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let rel = rel_path(raw, path)?;
    Some(Source {
        name: path.file_name()?.to_string_lossy().into_owned(),
        convertible: is_convertible(path),
        converted: is_fresh(raw, &rel),
        path: rel,
        kind: kind_of(path),
        size: meta.len(),
        modified,
    })
}

fn rel_path(raw: &Path, path: &Path) -> Option<String> {
    Some(
        path.strip_prefix(raw)
            .ok()?
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// Archivos de `raw/` sin ocultos ni la carpeta de conversiones (`raw/markdown/`).
fn walk_files(raw: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') || path == raw.join(MARKDOWN_DIR) {
            continue;
        }
        if path.is_dir() {
            walk_files(raw, &path, out);
        } else {
            out.push(path);
        }
    }
}

/// Rutas relativas a `raw/` de todas las fuentes.
pub fn walk_rel(raw: &Path, dir: &Path, out: &mut Vec<String>) {
    let mut files = Vec::new();
    walk_files(raw, dir, &mut files);
    out.extend(files.iter().filter_map(|p| rel_path(raw, p)));
}

fn walk(raw: &Path, dir: &Path, out: &mut Vec<Source>) {
    let mut files = Vec::new();
    walk_files(raw, dir, &mut files);
    out.extend(files.iter().filter_map(|p| to_source(raw, p)));
}

/// Destino libre dentro de `raw/`: `nombre.ext`, `nombre (2).ext`, …
fn free_target(raw: &Path, file_name: &str) -> PathBuf {
    let candidate = raw.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let original = Path::new(file_name);
    let stem = original
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = original
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| raw.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("siempre hay un nombre libre")
}

/// Todas las fuentes de `raw/`, de la más reciente a la más antigua.
#[tauri::command]
pub fn list_sources(app: AppHandle) -> Result<Vec<Source>, String> {
    let raw = raw_dir(&app)?;
    let mut sources = Vec::new();
    walk(&raw, &raw, &mut sources);
    sources.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(sources)
}

/// Copia archivos a `raw/` sin sobrescribir nada. Las carpetas se omiten.
#[tauri::command]
pub fn add_sources(app: AppHandle, paths: Vec<String>) -> Result<AddResult, String> {
    let raw = raw_dir(&app)?;
    fs::create_dir_all(&raw).map_err(|e| e.to_string())?;
    let markitdown_ready = crate::markitdown::find_server(&app).is_some();
    let mut result = AddResult {
        added: Vec::new(),
        skipped: Vec::new(),
        rejected: Vec::new(),
    };
    for path in paths.iter().map(PathBuf::from) {
        let label = path.display().to_string();
        let Some(file_name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            result.skipped.push(label);
            continue;
        };
        if !path.is_file() || path.starts_with(&raw) {
            result.skipped.push(label);
            continue;
        }
        // Los PDF (y Office/EPUB) solo entran si se pueden convertir a Markdown al momento.
        if is_convertible(&path) && !markitdown_ready {
            result.rejected.push(file_name);
            continue;
        }
        let target = free_target(&raw, &file_name);
        match fs::copy(&path, &target) {
            Ok(_) => result.added.extend(to_source(&raw, &target)),
            Err(_) => result.skipped.push(label),
        }
    }
    Ok(result)
}

/// Abre una fuente con la aplicación predeterminada del sistema.
#[tauri::command]
pub fn open_source(app: AppHandle, path: String) -> Result<(), String> {
    let file = resolve_inside(&raw_dir(&app)?, &path)?;
    app.opener()
        .open_path(file.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Quita una fuente de `raw/` enviándola a la papelera (recuperable).
#[tauri::command]
pub fn remove_source(app: AppHandle, path: String) -> Result<(), String> {
    let raw = raw_dir(&app)?;
    let file = resolve_inside(&raw, &path)?;
    trash::delete(&file).map_err(|e| format!("No se pudo mover a la papelera: {e}"))?;
    // Su conversión a Markdown ya no tiene original: también va a la papelera.
    let converted = converted_path(&raw, &path);
    if converted.is_file() {
        let _ = trash::delete(&converted);
    }
    Ok(())
}
