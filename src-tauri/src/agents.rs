//! Agentes del usuario: los subagentes de Claude Code del vault (`.claude/agents/*.md`).
//! nexo los muestra en Agentes → Agentes con su resumen (`description`), el modelo y el
//! esfuerzo de su frontmatter. Los crea Claude Code (el agente `creador-de-skills`) cuando
//! el usuario se lo pide; `creador-de-skills` lo instala y mantiene nexo (`skills.rs`).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::AppHandle;

use crate::skills::{AGENT_MARK, frontmatter};
use crate::vault::vault_root;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct AgentInfo {
    /// Ruta dentro de `.claude/agents/`, sin `.md` (p. ej. `revisor-classroom`).
    pub id: String,
    /// `name:` del frontmatter (o el nombre del archivo).
    pub name: String,
    /// `description:`: qué hace, tal como lo lee Claude Code para decidir cuándo usarlo.
    pub description: String,
    /// `model:` (id completo, alias como `sonnet`, o `inherit`); sin él, el de la sesión.
    pub model: Option<String>,
    /// `effort:` (`low`, `medium`, `high`, `xhigh`, `max` o un número); sin él, el de la sesión.
    pub effort: Option<String>,
    /// `tools:` si el agente limita sus herramientas.
    pub tools: Option<String>,
    /// Lo mantiene nexo (conserva su marca): no se ofrece borrarlo.
    pub managed: bool,
    /// Creación (o última modificación), en ms desde 1970: orden estable de la lista.
    pub created: u64,
}

fn agents_dir(root: &Path) -> PathBuf {
    root.join(".claude").join("agents")
}

fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Archivos `.md` de agentes (Claude Code también los lee en subcarpetas), sin los ocultos.
fn agent_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        match entry.file_type() {
            Ok(t) if t.is_dir() && depth < 3 => agent_files(&path, depth + 1, out),
            Ok(t) if t.is_file() && path.extension().is_some_and(|e| e == "md") => out.push(path),
            _ => {}
        }
    }
}

/// Lee los agentes del vault, del más antiguo al más nuevo. Un archivo sin frontmatter con
/// `description` no es un agente válido para Claude Code y no se lista.
pub fn list(root: &Path) -> Vec<AgentInfo> {
    let dir = agents_dir(root);
    let mut files = Vec::new();
    agent_files(&dir, 0, &mut files);
    let mut agents: Vec<AgentInfo> = files
        .into_iter()
        .filter_map(|file| {
            let text = fs::read_to_string(&file).ok()?;
            let meta = frontmatter(&text);
            let get = |key: &str| {
                meta.iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.clone())
                    .filter(|v| !v.is_empty())
            };
            let description = get("description")?;
            let id = file
                .strip_prefix(&dir)
                .ok()?
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            let info = fs::metadata(&file).ok();
            let created = info
                .as_ref()
                .and_then(|m| m.created().or_else(|_| m.modified()).ok())
                .map(millis)
                .unwrap_or(0);
            Some(AgentInfo {
                name: get("name").unwrap_or_else(|| id.clone()),
                description,
                model: get("model"),
                effort: get("effort"),
                tools: get("tools"),
                managed: text.contains(AGENT_MARK),
                created,
                id,
            })
        })
        .collect();
    agents.sort_by(|a, b| a.created.cmp(&b.created).then_with(|| a.id.cmp(&b.id)));
    agents
}

/// Huella de los agentes para el vigilante: ruta, tamaño y fecha de cada archivo.
pub fn fingerprint(root: &Path) -> Vec<(String, u64, u64)> {
    let dir = agents_dir(root);
    let mut files = Vec::new();
    agent_files(&dir, 0, &mut files);
    let mut out: Vec<(String, u64, u64)> = files
        .into_iter()
        .filter_map(|file| {
            let meta = fs::metadata(&file).ok()?;
            Some((
                file.to_string_lossy().into_owned(),
                meta.len(),
                meta.modified().map(millis).unwrap_or(0),
            ))
        })
        .collect();
    out.sort();
    out
}

/// Archivo de un agente a partir de su id, sin salir de `.claude/agents/`.
fn agent_file(root: &Path, id: &str) -> Result<PathBuf, String> {
    let bad = id.is_empty()
        || id.contains('\\')
        || id
            .split('/')
            .any(|part| part.is_empty() || part.starts_with('.'));
    if bad {
        return Err("Nombre de agente no válido.".into());
    }
    let file = agents_dir(root).join(format!("{id}.md"));
    if !file.is_file() {
        return Err(format!("No existe el agente {id}."));
    }
    Ok(file)
}

#[tauri::command]
pub fn agents_list(app: AppHandle) -> Result<Vec<AgentInfo>, String> {
    Ok(list(&vault_root(&app)?))
}

/// Manda un agente a la papelera (Claude Code deja de tenerlo en la próxima sesión).
#[tauri::command]
pub fn agents_remove(app: AppHandle, id: String) -> Result<(), String> {
    let file = agent_file(&vault_root(&app)?, &id)?;
    trash::delete(&file).map_err(|e| format!("No se pudo mover a la papelera: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nexo-agentes-{name}-{}-{}",
            std::process::id(),
            millis(SystemTime::now())
        ));
        fs::create_dir_all(dir.join(".claude/agents/sub")).unwrap();
        dir
    }

    #[test]
    fn lista_agentes_con_modelo_y_esfuerzo() {
        let root = temp("lista");
        let dir = root.join(".claude/agents");
        fs::write(
            dir.join("revisor.md"),
            "---\nname: revisor\ndescription: Revisa cosas. Solo lee.\nmodel: claude-haiku-5-5\neffort: medium\ntools: Read, Grep\n---\n\nCuerpo",
        )
        .unwrap();
        fs::write(
            dir.join("sub/anidado.md"),
            "---\ndescription: >-\n  Hace algo\n  en dos líneas.\n---\n",
        )
        .unwrap();
        // Sin description no es un agente; los ocultos y los que no son .md, tampoco.
        fs::write(dir.join("roto.md"), "---\nname: roto\n---\n").unwrap();
        fs::write(dir.join(".oculto.md"), "---\ndescription: x\n---\n").unwrap();
        fs::write(dir.join("notas.txt"), "---\ndescription: x\n---\n").unwrap();
        let mut agents = list(&root);
        agents.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].id, "revisor");
        assert_eq!(agents[0].model.as_deref(), Some("claude-haiku-5-5"));
        assert_eq!(agents[0].effort.as_deref(), Some("medium"));
        assert_eq!(agents[0].tools.as_deref(), Some("Read, Grep"));
        assert!(!agents[0].managed);
        assert_eq!(agents[1].id, "sub/anidado");
        assert_eq!(agents[1].name, "sub/anidado");
        assert_eq!(agents[1].description, "Hace algo en dos líneas.");
        assert_eq!(agents[1].model, None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn el_agente_de_nexo_va_marcado() {
        let root = temp("marca");
        crate::skills::install_agent(&root).unwrap();
        let agents = list(&root);
        let creador = agents.iter().find(|a| a.id == "creador-de-skills").unwrap();
        assert!(creador.managed);
        assert_eq!(creador.model.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(creador.effort.as_deref(), Some("medium"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn la_huella_cambia_al_crear_un_agente() {
        let root = temp("huella");
        let before = fingerprint(&root);
        fs::write(
            root.join(".claude/agents/nuevo.md"),
            "---\ndescription: x\n---\n",
        )
        .unwrap();
        assert_ne!(before, fingerprint(&root));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rechaza_rutas_fuera_de_agents() {
        let root = temp("rutas");
        fs::write(
            root.join(".claude/agents/ok.md"),
            "---\ndescription: x\n---\n",
        )
        .unwrap();
        for id in [
            "",
            "..",
            "../x",
            "a//b",
            ".oculto",
            "sub/../../x",
            "a\\b",
            "no-existe",
        ] {
            assert!(agent_file(&root, id).is_err(), "{id}");
        }
        assert!(agent_file(&root, "ok").is_ok());
        fs::remove_dir_all(root).unwrap();
    }
}
