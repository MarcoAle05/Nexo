//! Fichas de fuente: una nota por fuente en `wiki/fuentes/<nombre>.md`.
//!
//! Cada ficha es un nodo del grafo: se enlaza a los conceptos de la wiki (`[[...]]`) y las
//! notas que citan `raw/<ruta>` quedan conectadas a ella. Guarda el resumen que hace
//! Antigravity, así que Claude Code y la API encuentran cada fuente por su nombre.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// La parte de solo lectura (la que usa el grafo) vive en `nexo-core`.
use nexo_core::fichas::front;
pub use nexo_core::fichas::{FICHAS_DIR, all, dir, display_name};

const INDEX: &str = "_index.md";
const PENDING: &str = "_Pendiente: Antigravity aún no ha resumido esta fuente._";

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub summary: String,
    #[serde(default)]
    pub key_points: Vec<String>,
    #[serde(default)]
    pub concepts: Vec<String>,
}

/// Lo que la interfaz muestra en la pestaña de resumen de una fuente.
#[derive(Serialize)]
pub struct FichaView {
    /// Ruta de la ficha relativa a `wiki/` (id del nodo en el grafo).
    pub id: String,
    pub name: String,
    pub summarized: bool,
    pub summarized_on: Option<String>,
    pub summary: String,
    pub key_points: Vec<String>,
    pub concepts: Vec<String>,
}

/// kebab-case sin acentos, apto como nombre de nota.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.to_lowercase().chars() {
        let c = match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(70).collect();
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "fuente".into() } else { out }
}

/// Contenido de la sección `## titulo` (hasta la siguiente sección).
fn section<'a>(text: &'a str, title: &str) -> &'a str {
    let marker = format!("\n## {title}\n");
    let Some(start) = text.find(&marker) else {
        return "";
    };
    let rest = &text[start + marker.len()..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    rest[..end].trim()
}

fn links_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else { break };
        let target = after[..end].split('|').next().unwrap_or("").trim();
        if !target.is_empty() {
            out.push(target.to_string());
        }
        rest = &after[end + 2..];
    }
    out
}

pub fn find(root: &Path, rel: &str) -> Option<PathBuf> {
    all(root).remove(rel)
}

fn free_path(root: &Path, name: &str) -> PathBuf {
    let base = slug(name);
    (1..)
        .map(|n| {
            if n == 1 {
                format!("{base}.md")
            } else {
                format!("{base}-{n}.md")
            }
        })
        .map(|file| dir(root).join(file))
        .find(|p| !p.exists())
        .expect("siempre hay un nombre libre")
}

fn render(
    name: &str,
    rel: &str,
    kind: &str,
    added: &str,
    summary: Option<(&Summary, &str)>,
) -> String {
    let mut doc = format!(
        "---\nsource: raw/{rel}\ntype: {kind}\nadded: {added}\nsummary: {}\n---\n\n# {name}\n\nFuente original: `raw/{rel}`\n\n## Resumen\n\n",
        summary
            .map(|(_, when)| format!("antigravity · {when}"))
            .unwrap_or_else(|| "pendiente".into())
    );
    match summary {
        None => doc.push_str(PENDING),
        Some((s, _)) => {
            doc.push_str(s.summary.trim());
            if !s.key_points.is_empty() {
                doc.push_str("\n\n## Ideas clave\n\n");
                for point in &s.key_points {
                    doc.push_str(&format!("- {}\n", point.trim()));
                }
            }
            if !s.concepts.is_empty() {
                doc.push_str("\n\n## Conceptos\n\n");
                let links: Vec<String> = s.concepts.iter().map(|c| format!("[[{c}]]")).collect();
                doc.push_str(&links.join(" · "));
            }
        }
    }
    doc.push('\n');
    doc
}

/// Crea la ficha de una fuente si no existe. Devuelve `true` si la creó.
pub fn ensure(root: &Path, rel: &str, kind: &str) -> Result<bool, String> {
    if find(root, rel).is_some() {
        return Ok(false);
    }
    fs::create_dir_all(dir(root)).map_err(|e| e.to_string())?;
    let name = display_name(rel);
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    fs::write(
        free_path(root, &name),
        render(&name, rel, kind, &today, None),
    )
    .map_err(|e| e.to_string())?;
    Ok(true)
}

/// Escribe el resumen de Antigravity en la ficha (la crea si hace falta).
pub fn write_summary(root: &Path, rel: &str, kind: &str, summary: &Summary) -> Result<(), String> {
    ensure(root, rel, kind)?;
    let path = find(root, rel).ok_or("No se encontró la ficha")?;
    let old = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let added =
        front(&old, "added").unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
    let when = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    let name = display_name(rel);
    fs::write(
        &path,
        render(&name, rel, kind, &added, Some((summary, &when))),
    )
    .map_err(|e| e.to_string())
}

pub fn view(root: &Path, rel: &str) -> Option<FichaView> {
    let path = find(root, rel)?;
    let text = fs::read_to_string(&path).ok()?;
    let status = front(&text, "summary").unwrap_or_default();
    let summarized = status.starts_with("antigravity");
    let summary = section(&text, "Resumen");
    Some(FichaView {
        id: format!("{FICHAS_DIR}/{}", path.file_name()?.to_string_lossy()),
        name: display_name(rel),
        summarized,
        summarized_on: summarized.then(|| status.trim_start_matches("antigravity · ").to_string()),
        summary: if summarized {
            summary.to_string()
        } else {
            String::new()
        },
        key_points: section(&text, "Ideas clave")
            .lines()
            .filter_map(|l| l.trim().strip_prefix("- ").map(str::to_string))
            .collect(),
        concepts: links_in(section(&text, "Conceptos")),
    })
}

pub fn remove(root: &Path, rel: &str) {
    if let Some(path) = find(root, rel) {
        let _ = trash::delete(&path);
    }
}

/// Tras renombrar una fuente: actualiza su ficha (ruta, título y nombre de archivo).
pub fn rename(root: &Path, old_rel: &str, new_rel: &str) -> Result<(), String> {
    let Some(path) = find(root, old_rel) else {
        return Ok(());
    };
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let (old_name, new_name) = (display_name(old_rel), display_name(new_rel));
    let text = text
        .replacen(
            &format!("source: raw/{old_rel}"),
            &format!("source: raw/{new_rel}"),
            1,
        )
        .replacen(
            &format!("\n# {old_name}\n"),
            &format!("\n# {new_name}\n"),
            1,
        )
        .replace(&format!("`raw/{old_rel}`"), &format!("`raw/{new_rel}`"));
    let target = free_path(root, &new_name);
    fs::write(&target, text).map_err(|e| e.to_string())?;
    if target != path {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Sincroniza fichas con las fuentes: crea las que faltan y manda a la papelera las
/// que ya no tienen fuente. Reescribe el índice si algo cambió. Devuelve si hubo cambios.
pub fn sync(root: &Path, sources: &[(String, &'static str)]) -> Result<bool, String> {
    let mut changed = false;
    for (rel, kind) in sources {
        changed |= ensure(root, rel, kind)?;
    }
    let live: HashSet<&str> = sources.iter().map(|(r, _)| r.as_str()).collect();
    for (rel, path) in all(root) {
        if !live.contains(rel.as_str()) {
            let _ = trash::delete(&path);
            changed = true;
        }
    }
    // También se rehace si alguien borró o añadió fichas por fuera (el índice no coincide).
    if changed || fs::read_to_string(dir(root).join(INDEX)).ok() != Some(index_text(root)) {
        write_index(root)?;
    }
    Ok(changed)
}

fn index_text(root: &Path) -> String {
    let mut doc = String::from(
        "# Fuentes\n\nUna ficha por cada fuente de `raw/`, con su resumen y los conceptos que toca. Mantenido por nexo.\n\n",
    );
    for (rel, path) in all(root) {
        let file = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        doc.push_str(&format!(
            "- [[{FICHAS_DIR}/{file}|{}]] · `raw/{rel}`\n",
            display_name(&rel)
        ));
    }
    doc
}

/// `wiki/fuentes/_index.md` con todas las fichas, enlazado desde `_master-index.md`.
pub fn write_index(root: &Path) -> Result<(), String> {
    fs::create_dir_all(dir(root)).map_err(|e| e.to_string())?;
    fs::write(dir(root).join(INDEX), index_text(root)).map_err(|e| e.to_string())?;

    let master = root.join("wiki/_master-index.md");
    let text = fs::read_to_string(&master).unwrap_or_else(|_| "# Índice maestro\n".into());
    let link = format!("[[{FICHAS_DIR}/_index|Fuentes]]");
    if !text.contains(&link) {
        let text = format!("{}\n\n- {link}\n", text.trim_end());
        fs::write(&master, text).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Nombres de nota existentes en la wiki (sin extensión), para proponer conceptos.
pub fn wiki_note_names(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if name.starts_with('.') || name == FICHAS_DIR {
                continue;
            }
            if path.is_dir() {
                walk(&path, out);
            } else if let Some(stem) = name.strip_suffix(".md") {
                if !stem.starts_with('_') {
                    out.push(stem.to_string());
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&root.join("wiki"), &mut out);
    out.sort();
    out.dedup();
    out
}

/// Conceptos propuestos por Antigravity → nombres de nota: si ya existe una nota
/// equivalente se usa su nombre; si no, se deja en kebab-case (aparece como nodo pendiente).
pub fn normalize_concepts(concepts: &[String], existing: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for concept in concepts {
        let key = slug(concept);
        if key == "fuente" {
            continue;
        }
        let name = existing
            .iter()
            .find(|e| slug(e) == key)
            .cloned()
            .unwrap_or(key);
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out.truncate(10);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nombres_y_slugs() {
        assert_eq!(display_name("docs/Mi Paper.v2.pdf"), "Mi Paper.v2");
        assert_eq!(slug("¿Qué es RAG? Guía"), "que-es-rag-guia");
        assert_eq!(
            normalize_concepts(
                &["Búsqueda híbrida".into(), "RAG".into(), "rag".into()],
                &["busqueda-hibrida".into()]
            ),
            vec!["busqueda-hibrida", "rag"]
        );
    }

    #[test]
    fn ficha_ida_y_vuelta() {
        let root = std::env::temp_dir().join(format!("nexo-fichas-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("wiki")).unwrap();
        assert!(ensure(&root, "web/guia-rag.md", "web").unwrap());
        assert!(!ensure(&root, "web/guia-rag.md", "web").unwrap());
        let pending = view(&root, "web/guia-rag.md").unwrap();
        assert!(!pending.summarized);
        let s = Summary {
            summary: "Explica RAG.".into(),
            key_points: vec!["Recupera antes de generar".into()],
            concepts: vec!["rag".into()],
        };
        write_summary(&root, "web/guia-rag.md", "web", &s).unwrap();
        let done = view(&root, "web/guia-rag.md").unwrap();
        assert!(done.summarized);
        assert_eq!(done.summary, "Explica RAG.");
        assert_eq!(done.concepts, vec!["rag"]);
        rename(&root, "web/guia-rag.md", "web/rag-basico.md").unwrap();
        let renamed = view(&root, "web/rag-basico.md").unwrap();
        assert_eq!(renamed.id, "fuentes/rag-basico.md");
        assert_eq!(renamed.summary, "Explica RAG.");
        write_index(&root).unwrap();
        let master = fs::read_to_string(root.join("wiki/_master-index.md")).unwrap();
        assert!(master.contains("[[fuentes/_index|Fuentes]]"));
        let _ = fs::remove_dir_all(&root);
    }
}
