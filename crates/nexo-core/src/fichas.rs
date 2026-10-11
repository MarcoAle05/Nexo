//! Fichas de fuente (`wiki/fuentes/<nombre>.md`): la parte de solo lectura, que es la que
//! necesita el grafo. Crear, resumir y borrar fichas sigue en la app.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Carpeta de las fichas dentro de `wiki/`.
pub const FICHAS_DIR: &str = "fuentes";

pub fn dir(root: &Path) -> PathBuf {
    root.join("wiki").join(FICHAS_DIR)
}

/// Nombre visible de una fuente: el nombre de archivo sin extensión.
pub fn display_name(rel: &str) -> String {
    let file = rel.rsplit('/').next().unwrap_or(rel);
    match file.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => file.to_string(),
    }
}

/// Campo `clave: valor` del frontmatter.
pub fn front(text: &str, key: &str) -> Option<String> {
    let body = text.strip_prefix("---\n")?;
    let end = body.find("\n---")?;
    body[..end].lines().find_map(|line| {
        line.strip_prefix(key)
            .and_then(|rest| rest.strip_prefix(':'))
            .map(|v| v.trim().trim_matches('"').to_string())
    })
}

/// Fichas existentes: ruta de la fuente (relativa a `raw/`) → archivo de la ficha.
pub fn all(root: &Path) -> BTreeMap<String, PathBuf> {
    let mut map = BTreeMap::new();
    let Ok(entries) = fs::read_dir(dir(root)) else {
        return map;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('_') || name.starts_with('.') || !name.ends_with(".md") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if let Some(source) =
            front(&text, "source").and_then(|s| s.strip_prefix("raw/").map(str::to_string))
        {
            map.insert(source, path);
        }
    }
    map
}
