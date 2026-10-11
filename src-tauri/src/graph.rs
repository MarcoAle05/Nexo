//! Grafo de la wiki: cada nota `.md` de `wiki/` es un nodo y cada enlace
//! `[[nota]]` o `[texto](nota.md)` es una arista, como en la vista de grafo de Obsidian.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_opener::OpenerExt;

use crate::vault::{resolve_inside, vault_dir, vault_root};

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
        let line = strip_inline_code(line);
        let line = line.as_str();
        let mut rest = line;
        while let Some(start) = rest.find("[[") {
            let after = &rest[start + 2..];
            let Some(end) = after.find("]]") else { break };
            let target = after[..end]
                .split(['|', '#', '^'])
                .next()
                .unwrap_or("")
                .trim();
            if !target.is_empty() && !target.contains("://") && !is_attachment(target) {
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

/// Destino que apunta a un adjunto (imagen, audio, vídeo, PDF), no a una nota.
fn is_attachment(target: &str) -> bool {
    const EXTENSIONS: [&str; 16] = [
        "png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "pdf", "mp3", "wav", "ogg", "m4a",
        "mp4", "webm", "mov", "mkv",
    ];
    target
        .rsplit_once('.')
        .is_some_and(|(_, ext)| EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

/// La línea sin los tramos entre acentos graves (código en línea). Un acento sin pareja
/// es texto normal, como en CommonMark.
fn strip_inline_code(line: &str) -> String {
    let parts: Vec<&str> = line.split('`').collect();
    let n = parts.len();
    parts
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 2 == 0 || (n % 2 == 0 && *i == n - 1))
        .map(|(_, p)| *p)
        .collect::<Vec<_>>()
        .join(" ")
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

/// Huella de `wiki/`: cada nota con su tamaño y fecha de modificación.
fn fingerprint(wiki: &Path) -> HashMap<String, (u64, std::time::SystemTime)> {
    let mut files = Vec::new();
    collect_notes(wiki, &mut files);
    files
        .iter()
        .filter_map(|p| {
            let meta = fs::metadata(p).ok()?;
            Some((rel_id(wiki, p), (meta.len(), meta.modified().ok()?)))
        })
        .collect()
}

#[derive(Clone, Serialize)]
struct WikiChanged {
    /// Notas nuevas desde la última comprobación.
    added: Vec<String>,
    /// Notas nuevas, modificadas o borradas (rutas relativas a `wiki/`).
    changed: Vec<String>,
}

#[derive(Clone, Serialize)]
struct AgentsChanged {}

#[derive(Clone, Serialize)]
struct SkillsChanged {
    /// Skills nuevas (carpetas de `.claude/skills/`).
    added: Vec<String>,
}

/// Fecha de modificación del escritorio de notas (`.nexo/escritorio.json`).
fn desk_mtime(wiki: &Path) -> Option<std::time::SystemTime> {
    let root = wiki.parent()?;
    fs::metadata(crate::notas::desk_path(root))
        .and_then(|m| m.modified())
        .ok()
}

/// Vigila `wiki/` para que el grafo se actualice cuando otro programa (Claude Code,
/// Obsidian, Antigravity) crea, cambia o borra notas: emite `wiki-changed`.
/// Se sondea cada segundo; para una wiki personal es barato y no necesita dependencias.
pub fn watch(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last: Option<(PathBuf, HashMap<String, (u64, std::time::SystemTime)>)> = None;
        let mut last_desk = None;
        let mut last_skills: Option<(PathBuf, Vec<(String, u64, u64)>)> = None;
        let mut last_agents: Option<(PathBuf, Vec<(String, u64, u64)>)> = None;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let Ok(wiki) = vault_dir(&app, "wiki") else {
                last = None;
                continue;
            };
            // Skills de Claude Code del vault (.claude/skills/): las crea el agente de nexo.
            if let Some(root) = wiki.parent() {
                let skills = crate::skills::fingerprint(root);
                if let Some((dir, before)) = &last_skills
                    && dir == root
                    && *before != skills
                {
                    let added = skills
                        .iter()
                        .filter(|(id, ..)| !before.iter().any(|(b, ..)| b == id))
                        .map(|(id, ..)| id.clone())
                        .collect();
                    let _ = app.emit("skills-changed", SkillsChanged { added });
                }
                last_skills = Some((root.to_path_buf(), skills));
                // Agentes de Claude Code (.claude/agents/): los muestra el apartado Agentes.
                let agents = crate::agents::fingerprint(root);
                if let Some((dir, before)) = &last_agents
                    && dir == root
                    && *before != agents
                {
                    let _ = app.emit("agents-changed", AgentsChanged {});
                }
                last_agents = Some((root.to_path_buf(), agents));
            }
            let mut now = fingerprint(&wiki);
            let desk = desk_mtime(&wiki);
            match &last {
                // Otro vault: se toma como punto de partida sin avisar.
                Some((dir, before)) if *dir == wiki => {
                    if *before != now {
                        let mut changed: Vec<String> = now
                            .iter()
                            .filter(|(k, v)| before.get(*k) != Some(*v))
                            .map(|(k, _)| k.clone())
                            .chain(before.keys().filter(|k| !now.contains_key(*k)).cloned())
                            .collect();
                        // Algo cambió en Notas (p. ej. Claude Code creó una nota): se ponen
                        // al día los índices de los temas antes de avisar.
                        if changed.iter().any(|k| k.starts_with("notas/"))
                            && let Some(root) = wiki.parent()
                            && crate::notas::sync(root).unwrap_or(false)
                        {
                            let synced = fingerprint(&wiki);
                            changed.extend(
                                synced
                                    .iter()
                                    .filter(|(k, v)| now.get(*k) != Some(*v))
                                    .map(|(k, _)| k.clone()),
                            );
                            now = synced;
                        }
                        let added = now
                            .keys()
                            .filter(|k| !before.contains_key(*k))
                            .cloned()
                            .collect();
                        changed.sort();
                        changed.dedup();
                        let _ = app.emit("wiki-changed", WikiChanged { added, changed });
                    }
                    // El escritorio de notas cambió fuera de nexo (Claude Code).
                    if desk != last_desk
                        && let Some(mtime) = desk
                        && !crate::notas::desk_written_by_us(mtime)
                    {
                        let _ = app.emit("desk-changed", ());
                    }
                }
                _ => {}
            }
            last = Some((wiki, now));
            last_desk = desk;
        }
    });
}

#[tauri::command]
pub fn read_graph(app: AppHandle) -> Result<Graph, String> {
    build_graph(&vault_root(&app)?)
}

/// Construye el grafo del vault en `root` (`root/wiki/**/*.md`). Es la función que cubren
/// los casos dorados de `tests/fixtures/grafo`; las reglas están en `docs/spec-grafo.md`.
pub fn build_graph(root: &Path) -> Result<Graph, String> {
    let wiki = root.join("wiki");
    let mut files = Vec::new();
    collect_notes(&wiki, &mut files);
    // G14: el orden del sistema de archivos no es estable; se ordena por id.
    files.sort_by_key(|p| rel_id(&wiki, p));
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
    let fichas: HashMap<String, usize> = crate::fichas::all(root)
        .into_iter()
        .filter_map(|(rel, path)| files.iter().position(|f| *f == path).map(|i| (rel, i)))
        .collect();

    let mut edges = BTreeSet::new();
    let mut missing = BTreeSet::new();
    // `title:` del frontmatter (notas del apartado Notas, índices de sus temas…).
    let mut titles: HashMap<usize, String> = HashMap::new();
    for (i, file) in files.iter().enumerate() {
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        if let Some((_, title)) = split_frontmatter(&text)
            .0
            .into_iter()
            .find(|(k, _)| k == "title")
            && !title.is_empty()
        {
            titles.insert(i, title);
        }
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
            let name = id.rsplit('/').next().unwrap_or(id);
            Node {
                id: id.clone(),
                label: source
                    .as_deref()
                    .map(crate::fichas::display_name)
                    .or_else(|| titles.get(&i).cloned())
                    .unwrap_or_else(|| label_for(id)),
                group: id
                    .split_once('/')
                    .map(|(g, _)| g.to_string())
                    .unwrap_or_default(),
                kind: if source.is_some() {
                    "source"
                } else if id.starts_with("conversaciones/") && name != "_index.md" {
                    "conversation"
                } else if name == "_index.md" || name == "_master-index.md" {
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

#[derive(Serialize)]
pub struct NoteView {
    id: String,
    /// Cuerpo en Markdown, sin el frontmatter.
    body: String,
    /// Campos del frontmatter (`clave: valor`), en orden.
    meta: Vec<(String, String)>,
    /// Última modificación, `AAAA-MM-DD HH:MM`.
    modified: Option<String>,
}

/// Separa el frontmatter YAML (`---` … `---` al principio) del cuerpo.
fn split_frontmatter(text: &str) -> (Vec<(String, String)>, &str) {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (Vec::new(), text);
    };
    let Some(end) = rest.find("\n---") else {
        return (Vec::new(), text);
    };
    let meta = rest[..end]
        .lines()
        .filter_map(|line| {
            // Elementos de lista, claves anidadas y comentarios: se mira la línea tal cual,
            // antes de recortar la clave.
            if line.starts_with([' ', '\t', '-', '#']) {
                return None;
            }
            let (key, value) = line.split_once(':')?;
            let key = key.trim();
            (!key.is_empty() && !key.starts_with(['-', ' ', '#']))
                .then(|| (key.to_string(), value.trim().trim_matches('"').to_string()))
        })
        .collect();
    let body = rest[end + 4..].trim_start_matches(['-']).trim_start();
    (meta, body)
}

/// Contenido de una nota de `wiki/` para verla dentro de nexo.
#[tauri::command]
pub fn note_view(app: AppHandle, id: String) -> Result<NoteView, String> {
    let file = resolve_inside(&vault_dir(&app, "wiki")?, &id)?;
    let text = fs::read_to_string(&file).map_err(|e| format!("No se pudo leer la nota: {e}"))?;
    let (meta, body) = split_frontmatter(&text);
    let modified = fs::metadata(&file)
        .and_then(|m| m.modified())
        .ok()
        .map(|t| {
            chrono::DateTime::<chrono::Local>::from(t)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        });
    Ok(NoteView {
        id,
        body: body.to_string(),
        meta,
        modified,
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
    fn separa_el_frontmatter() {
        let (meta, body) =
            split_frontmatter("---\ntype: tareas\nfuente: \"Classroom\"\n---\n\n# Tareas\n");
        assert_eq!(
            meta,
            vec![
                ("type".into(), "tareas".into()),
                ("fuente".into(), "Classroom".into())
            ]
        );
        assert_eq!(body, "# Tareas\n");
        let (meta, body) = split_frontmatter("# Sin frontmatter");
        assert!(meta.is_empty());
        assert_eq!(body, "# Sin frontmatter");
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

    // ── Casos dorados del grafo (ver docs/spec-grafo.md) ──────────────────────────

    use serde_json::Value;

    fn fixtures_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/grafo")
    }

    fn casos() -> Vec<PathBuf> {
        let mut casos: Vec<PathBuf> = fs::read_dir(fixtures_dir())
            .expect("falta src-tauri/tests/fixtures/grafo")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        casos.sort();
        casos
    }

    fn grafo_de(caso: &Path) -> Value {
        serde_json::to_value(build_graph(&caso.join("vault")).unwrap()).unwrap()
    }

    /// Nodos y aristas como textos ordenados: lo que importa de cada caso es el conjunto.
    fn conjuntos(g: &Value) -> (Vec<String>, Vec<String>) {
        let lista = |clave: &str| {
            let mut v: Vec<String> = g[clave]
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .map(Value::to_string)
                .collect();
            v.sort();
            v
        };
        (lista("nodes"), lista("edges"))
    }

    #[test]
    fn el_grafo_cumple_los_casos_dorados() {
        let casos = casos();
        assert!(casos.len() >= 15, "faltan casos dorados: {}", casos.len());
        let mut fallos = Vec::new();
        for caso in &casos {
            let nombre = caso.file_name().unwrap().to_string_lossy().into_owned();
            let esperado: Value =
                serde_json::from_str(&fs::read_to_string(caso.join("expected.json")).unwrap())
                    .unwrap_or_else(|e| panic!("{nombre}/expected.json no es JSON válido: {e}"));
            let (n_esp, e_esp) = conjuntos(&esperado);
            let (n_real, e_real) = conjuntos(&grafo_de(caso));
            let solo = |a: &[String], b: &[String]| -> Vec<String> {
                a.iter().filter(|x| !b.contains(x)).cloned().collect()
            };
            let partes = [
                ("nodos que faltan", solo(&n_esp, &n_real)),
                ("nodos que sobran", solo(&n_real, &n_esp)),
                ("aristas que faltan", solo(&e_esp, &e_real)),
                ("aristas que sobran", solo(&e_real, &e_esp)),
            ];
            let detalle: Vec<String> = partes
                .iter()
                .filter(|(_, v)| !v.is_empty())
                .map(|(t, v)| format!("    {t}: {}", v.join("  ")))
                .collect();
            if !detalle.is_empty() {
                fallos.push(format!("  {nombre}\n{}", detalle.join("\n")));
            }
        }
        assert!(
            fallos.is_empty(),
            "{} de {} casos no coinciden con su expected.json:\n{}",
            fallos.len(),
            casos.len(),
            fallos.join("\n")
        );
    }

    /// G14: el orden de salida no depende del sistema de archivos: notas por id y,
    /// al final, los fantasmas por id.
    #[test]
    fn el_orden_de_los_nodos_es_estable() {
        for caso in casos() {
            let g = grafo_de(&caso);
            let ids = |missing: bool| -> Vec<String> {
                g["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|n| (n["kind"] == "missing") == missing)
                    .map(|n| n["id"].as_str().unwrap().to_string())
                    .collect()
            };
            let todos: Vec<String> = g["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n["id"].as_str().unwrap().to_string())
                .collect();
            let (notas, fantasmas) = (ids(false), ids(true));
            let mut ordenadas = notas.clone();
            ordenadas.sort();
            let mut fant_ord = fantasmas.clone();
            fant_ord.sort();
            assert_eq!(notas, ordenadas, "{}: notas sin ordenar", caso.display());
            assert_eq!(
                fantasmas,
                fant_ord,
                "{}: fantasmas sin ordenar",
                caso.display()
            );
            assert_eq!(
                todos,
                [notas, fantasmas].concat(),
                "{}: los fantasmas van al final",
                caso.display()
            );
        }
    }
}
