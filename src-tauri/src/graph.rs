//! Comandos y vigilante del grafo de la wiki. El grafo en sí (`build_graph` y sus reglas,
//! `docs/spec-grafo.md`) vive en `nexo-core`, que no depende de Tauri.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use nexo_core::graph::{Graph, build_graph, collect_notes, rel_id, split_frontmatter};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_opener::OpenerExt;

use crate::vault::{resolve_inside, vault_dir, vault_root};

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
