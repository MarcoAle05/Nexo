//! Grafo de la wiki: cada nota `.md` de `wiki/` es un nodo y cada enlace
//! `[[nota]]` o `[texto](nota.md)` es una arista, como en la vista de grafo de Obsidian.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::vault::{resolve_inside, vault_dir};

#[derive(Serialize)]
pub struct Node {
    /// Ruta relativa a `wiki/` (o `?destino` para enlaces a notas que no existen).
    id: String,
    label: String,
    /// Carpeta de primer nivel dentro de `wiki/` (el tema); vacío en la raíz.
    group: String,
    /// `note`, `index`, `source` (ficha de una fuente), `conversation` (conclusiones guardadas) o `missing`.
    kind: &'static str,
    /// Para las fichas: la fuente, relativa a `raw/`.
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
}

#[derive(Serialize)]
pub struct Graph {
    nodes: Vec<Node>,
    edges: Vec<(String, String)>,
}

fn rel_id(wiki: &Path, path: &Path) -> String {
    path.strip_prefix(wiki)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn collect_notes(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_notes(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            out.push(path);
        }
    }
}

/// Destinos de enlace escritos en la nota, sin alias ni encabezados y fuera de bloques de código.
fn extract_links(text: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut in_code = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let mut rest = line;
        while let Some(start) = rest.find("[[") {
            let after = &rest[start + 2..];
            let Some(end) = after.find("]]") else { break };
            let target = after[..end]
                .split(['|', '#', '^'])
                .next()
                .unwrap_or("")
                .trim();
            if !target.is_empty() {
                links.push(target.to_string());
            }
            rest = &after[end + 2..];
        }
        let mut rest = line;
        while let Some(start) = rest.find("](") {
            let after = &rest[start + 2..];
            let Some(end) = after.find(')') else { break };
            let target = after[..end].split('#').next().unwrap_or("").trim();
            if !target.contains("://") && target.to_lowercase().ends_with(".md") {
                links.push(target.replace("%20", " "));
            }
            rest = &after[end + 1..];
        }
    }
    links
}

fn strip_md(s: &str) -> &str {
    if s.to_lowercase().ends_with(".md") {
        &s[..s.len() - 3]
    } else {
        s
    }
}

fn label_for(id: &str) -> String {
    let stem = strip_md(id.rsplit('/').next().unwrap_or(id));
    match stem {
        "_master-index" => "Índice maestro".into(),
        "_index" => id
            .rsplit('/')
            .nth(1)
            .map(|folder| folder.to_string())
            .unwrap_or_else(|| "Índice".into()),
        _ => stem.to_string(),
    }
}

#[tauri::command]
pub fn read_graph(app: AppHandle) -> Result<Graph, String> {
    let wiki = vault_dir(&app, "wiki")?;
    let mut files = Vec::new();
    collect_notes(&wiki, &mut files);
    let ids: Vec<String> = files.iter().map(|p| rel_id(&wiki, p)).collect();

    // Obsidian resuelve por nombre de nota o por ruta; indexamos ambos en minúsculas.
    let mut by_stem: HashMap<String, Vec<usize>> = HashMap::new();
    let mut by_path: HashMap<String, usize> = HashMap::new();
    for (i, id) in ids.iter().enumerate() {
        let no_ext = strip_md(id).to_lowercase();
        let stem = no_ext.rsplit('/').next().unwrap_or(&no_ext).to_string();
        by_stem.entry(stem).or_default().push(i);
        by_path.insert(no_ext, i);
    }

    // Fichas de fuente: ruta de la fuente → índice del nodo.
    let root = wiki.parent().map(Path::to_path_buf).unwrap_or_default();
    let fichas: HashMap<String, usize> = crate::fichas::all(&root)
        .into_iter()
        .filter_map(|(rel, path)| files.iter().position(|f| *f == path).map(|i| (rel, i)))
        .collect();

    let mut edges = BTreeSet::new();
    let mut missing = BTreeSet::new();
    for (i, file) in files.iter().enumerate() {
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        // Toda nota que cita `raw/<fuente>` queda unida a la ficha de esa fuente.
        for (rel, &j) in &fichas {
            if j != i && text.contains(&format!("raw/{rel}")) {
                let (a, b) = (ids[i].clone(), ids[j].clone());
                edges.insert(if a < b { (a, b) } else { (b, a) });
            }
        }
        let dir = ids[i]
            .rsplit_once('/')
            .map(|(d, _)| d.to_lowercase())
            .unwrap_or_default();
        for link in extract_links(&text) {
            let key = strip_md(link.trim_start_matches("./")).to_lowercase();
            let relative = if dir.is_empty() {
                key.clone()
            } else {
                format!("{dir}/{key}")
            };
            let target = by_path
                .get(&key)
                .or_else(|| by_path.get(&relative))
                .copied()
                .or_else(|| {
                    let stem = key.rsplit('/').next().unwrap_or(&key);
                    let found = by_stem.get(stem)?;
                    // Con nombres repetidos (p. ej. `_index`) gana el de la misma carpeta.
                    found
                        .iter()
                        .find(|&&j| ids[j].to_lowercase().starts_with(&format!("{dir}/")))
                        .or(found.first())
                        .copied()
                });
            let (a, b) = match target {
                Some(j) if j != i => (ids[i].clone(), ids[j].clone()),
                Some(_) => continue,
                None => {
                    let ghost = format!("?{}", link.trim());
                    missing.insert(ghost.clone());
                    (ids[i].clone(), ghost)
                }
            };
            edges.insert(if a < b { (a, b) } else { (b, a) });
        }
    }

    let source_of: HashMap<usize, &String> = fichas.iter().map(|(rel, &i)| (i, rel)).collect();
    let mut nodes: Vec<Node> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let source = source_of.get(&i).map(|rel| rel.to_string());
            Node {
                id: id.clone(),
                label: source
                    .as_deref()
                    .map(crate::fichas::display_name)
                    .unwrap_or_else(|| label_for(id)),
                group: id
                    .split_once('/')
                    .map(|(g, _)| g.to_string())
                    .unwrap_or_default(),
                kind: if source.is_some() {
                    "source"
                } else if id.starts_with("conversaciones/") && !id.ends_with("_index.md") {
                    "conversation"
                } else if id.ends_with("_index.md") || id.ends_with("_master-index.md") {
                    "index"
                } else {
                    "note"
                },
                source,
            }
        })
        .collect();
    nodes.extend(missing.into_iter().map(|id| Node {
        label: id[1..].to_string(),
        id,
        group: String::new(),
        kind: "missing",
        source: None,
    }));

    Ok(Graph {
        nodes,
        edges: edges.into_iter().collect(),
    })
}

/// Abre una nota de `wiki/` con la aplicación predeterminada (Obsidian si está asociada a .md).
#[tauri::command]
pub fn open_note(app: AppHandle, id: String) -> Result<(), String> {
    let file = resolve_inside(&vault_dir(&app, "wiki")?, &id)?;
    app.opener()
        .open_path(file.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrae_enlaces_wiki_y_markdown() {
        let text = "Ver [[RAG|recuperación]] y [[agentes#memoria]].\n```\n[[ignorado]]\n```\n[doc](temas/chunking.md) [web](https://x.com/a.md)";
        assert_eq!(
            extract_links(text),
            vec!["RAG", "agentes", "temas/chunking.md"]
        );
    }

    #[test]
    fn etiqueta_de_indices() {
        assert_eq!(label_for("ai-agents/_index.md"), "ai-agents");
        assert_eq!(label_for("_master-index.md"), "Índice maestro");
        assert_eq!(
            label_for("rag/chunking-strategies.md"),
            "chunking-strategies"
        );
    }
}
