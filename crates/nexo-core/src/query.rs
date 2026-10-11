//! Consultas de solo lectura sobre el vault: lo que expone el servidor MCP.
//! Las reglas están en `docs/spec-mcp.md`. Nada aquí escribe en disco.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs;
use std::path::{Component, Path};

use serde::Serialize;

use crate::graph::{Graph, Node, build_graph, label_for, split_frontmatter};
use crate::vault::resolve_inside;

pub const DEFAULT_LIST_LIMIT: usize = 200;
pub const MAX_LIST_LIMIT: usize = 1000;
pub const DEFAULT_SEARCH_LIMIT: usize = 20;
pub const MAX_SEARCH_LIMIT: usize = 100;
pub const MAX_DEPTH: usize = 3;
/// Tope del cuerpo que devuelve `read_note` (bytes); una nota mayor se corta.
pub const MAX_NOTE_BYTES: usize = 200_000;
const SNIPPETS_PER_NOTE: usize = 3;
const SNIPPET_CHARS: usize = 200;
const KINDS: [&str; 4] = ["note", "index", "source", "conversation"];

#[derive(Serialize, Debug, PartialEq)]
pub struct NoteSummary {
    pub id: String,
    pub label: String,
    pub group: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct NoteList {
    /// Notas que cumplen el filtro, antes de aplicar `limit`.
    pub total: usize,
    pub truncated: bool,
    pub notes: Vec<NoteSummary>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct NoteContent {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub meta: Vec<(String, String)>,
    pub body: String,
    pub truncated: bool,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Snippet {
    pub line: usize,
    pub text: String,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct SearchHit {
    pub id: String,
    pub label: String,
    pub in_label: bool,
    pub matches: usize,
    pub snippets: Vec<Snippet>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Neighbor {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub distance: usize,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct SourceEntry {
    /// Ruta de la fuente relativa a `raw/`.
    pub source: String,
    /// Id de su ficha en la wiki.
    pub ficha: String,
    pub label: String,
}

fn summary(n: &Node) -> NoteSummary {
    NoteSummary {
        id: n.id.clone(),
        label: n.label.clone(),
        group: n.group.clone(),
        kind: n.kind.to_string(),
        source: n.source.clone(),
    }
}

fn clamp(value: Option<usize>, default: usize, max: usize) -> usize {
    value.unwrap_or(default).clamp(1, max)
}

/// Carpeta o id dentro de `wiki/`: relativo, sin `..`, sin raíz y sin componentes ocultos.
fn clean_rel(rel: &str) -> Result<String, String> {
    let ok = !rel.is_empty()
        && Path::new(rel).components().all(|c| match c {
            Component::Normal(name) => !name.to_string_lossy().starts_with('.'),
            _ => false,
        });
    if ok {
        Ok(rel.trim_end_matches('/').to_string())
    } else {
        Err(format!("Ruta no válida: {rel}"))
    }
}

pub fn list_notes(
    root: &Path,
    folder: Option<&str>,
    kind: Option<&str>,
    limit: Option<usize>,
) -> Result<NoteList, String> {
    if let Some(kind) = kind
        && !KINDS.contains(&kind)
    {
        return Err(format!("kind debe ser uno de: {}", KINDS.join(", ")));
    }
    let prefix = folder
        .filter(|f| !f.trim().is_empty())
        .map(|f| clean_rel(f.trim()).map(|f| format!("{f}/")))
        .transpose()?;
    let graph = build_graph(root)?;
    let notes: Vec<NoteSummary> = graph
        .nodes
        .iter()
        .filter(|n| n.kind != "missing")
        .filter(|n| kind.is_none_or(|k| n.kind == k))
        .filter(|n| prefix.as_deref().is_none_or(|p| n.id.starts_with(p)))
        .map(summary)
        .collect();
    let limit = clamp(limit, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT);
    let total = notes.len();
    Ok(NoteList {
        total,
        truncated: total > limit,
        notes: notes.into_iter().take(limit).collect(),
    })
}

/// Lee un archivo de `wiki/` sin salir del vault (resuelve enlaces simbólicos).
fn read_inside(root: &Path, id: &str) -> Result<String, String> {
    let id = clean_rel(id)?;
    if !id.to_lowercase().ends_with(".md") {
        return Err(format!("Solo se leen notas .md: {id}"));
    }
    let file = resolve_inside(&root.join("wiki"), &id)?;
    if !file.is_file() {
        return Err(format!("No existe: {id}"));
    }
    fs::read_to_string(&file).map_err(|_| format!("No se pudo leer: {id}"))
}

fn truncate(text: &str, max_bytes: usize) -> (&str, bool) {
    if text.len() <= max_bytes {
        return (text, false);
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

pub fn read_note(root: &Path, id: &str) -> Result<NoteContent, String> {
    let text = read_inside(root, id)?;
    let id = clean_rel(id)?;
    let graph = build_graph(root)?;
    let node = graph.nodes.iter().find(|n| n.id == id);
    let (meta, body) = split_frontmatter(&text);
    let (body, truncated) = truncate(body, MAX_NOTE_BYTES);
    Ok(NoteContent {
        label: node
            .map(|n| n.label.clone())
            .unwrap_or_else(|| label_for(&id)),
        kind: node.map(|n| n.kind).unwrap_or("note").to_string(),
        id,
        meta,
        body: body.to_string(),
        truncated,
    })
}

fn snippet_text(line: &str) -> String {
    let line = line.trim();
    match line.char_indices().nth(SNIPPET_CHARS) {
        Some((end, _)) => format!("{}…", &line[..end]),
        None => line.to_string(),
    }
}

pub fn search_notes(
    root: &Path,
    query: &str,
    limit: Option<usize>,
) -> Result<Vec<SearchHit>, String> {
    let needle = query.trim().to_lowercase();
    if needle.chars().count() < 2 {
        return Err("La búsqueda necesita al menos 2 caracteres.".into());
    }
    let graph = build_graph(root)?;
    let mut hits = Vec::new();
    for node in graph.nodes.iter().filter(|n| n.kind != "missing") {
        // Cada archivo se lee por `read_inside`: un enlace simbólico que salga del vault no entra.
        let Ok(text) = read_inside(root, &node.id) else {
            continue;
        };
        let in_label = node.label.to_lowercase().contains(&needle);
        let mut matches = 0;
        let mut snippets = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let count = line.to_lowercase().matches(&needle).count();
            if count > 0 {
                matches += count;
                if snippets.len() < SNIPPETS_PER_NOTE {
                    snippets.push(Snippet {
                        line: i + 1,
                        text: snippet_text(line),
                    });
                }
            }
        }
        if in_label || matches > 0 {
            hits.push(SearchHit {
                id: node.id.clone(),
                label: node.label.clone(),
                in_label,
                matches,
                snippets,
            });
        }
    }
    hits.sort_by(|a, b| {
        b.in_label
            .cmp(&a.in_label)
            .then(b.matches.cmp(&a.matches))
            .then(a.id.cmp(&b.id))
    });
    hits.truncate(clamp(limit, DEFAULT_SEARCH_LIMIT, MAX_SEARCH_LIMIT));
    Ok(hits)
}

/// El grafo completo o solo el de un grupo (carpeta de primer nivel; `""` = raíz).
pub fn graph(root: &Path, group: Option<&str>) -> Result<Graph, String> {
    let full = build_graph(root)?;
    let Some(group) = group else {
        return Ok(full);
    };
    let keep: Vec<String> = full
        .nodes
        .iter()
        .filter(|n| n.group == group)
        .map(|n| n.id.clone())
        .collect();
    let edges = full
        .edges
        .iter()
        .filter(|(a, b)| keep.contains(a) && keep.contains(b))
        .cloned()
        .collect();
    let nodes = full
        .nodes
        .into_iter()
        .filter(|n| keep.contains(&n.id))
        .collect();
    Ok(Graph { nodes, edges })
}

pub fn neighbors(root: &Path, id: &str, depth: Option<usize>) -> Result<Vec<Neighbor>, String> {
    let depth = depth.unwrap_or(1);
    if !(1..=MAX_DEPTH).contains(&depth) {
        return Err(format!("depth debe estar entre 1 y {MAX_DEPTH}."));
    }
    let graph = build_graph(root)?;
    if !graph.nodes.iter().any(|n| n.id == id) {
        return Err(format!("No existe el nodo: {id}"));
    }
    let mut adjacent: HashMap<&str, Vec<&str>> = HashMap::new();
    for (a, b) in &graph.edges {
        adjacent.entry(a).or_default().push(b);
        adjacent.entry(b).or_default().push(a);
    }
    let mut seen: BTreeMap<&str, usize> = BTreeMap::from([(id, 0)]);
    let mut queue = VecDeque::from([(id, 0)]);
    while let Some((current, distance)) = queue.pop_front() {
        if distance == depth {
            continue;
        }
        for &next in adjacent.get(current).into_iter().flatten() {
            if !seen.contains_key(next) {
                seen.insert(next, distance + 1);
                queue.push_back((next, distance + 1));
            }
        }
    }
    let mut found: Vec<Neighbor> = graph
        .nodes
        .iter()
        .filter(|n| n.id != id)
        .filter_map(|n| {
            seen.get(n.id.as_str()).map(|&distance| Neighbor {
                id: n.id.clone(),
                label: n.label.clone(),
                kind: n.kind.to_string(),
                distance,
            })
        })
        .collect();
    found.sort_by(|a, b| a.distance.cmp(&b.distance).then(a.id.cmp(&b.id)));
    Ok(found)
}

pub fn list_sources(root: &Path) -> Result<Vec<SourceEntry>, String> {
    let mut sources: Vec<SourceEntry> = build_graph(root)?
        .nodes
        .into_iter()
        .filter(|n| n.kind == "source")
        .filter_map(|n| {
            Some(SourceEntry {
                source: n.source?,
                ficha: n.id,
                label: n.label,
            })
        })
        .collect();
    sources.sort_by(|a, b| a.source.cmp(&b.source));
    Ok(sources)
}
