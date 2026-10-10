//! Vault de Obsidian que nexo usa como memoria de fuentes:
//! `raw/` (lo que vuelca el usuario) → `wiki/` (lo organiza la IA) → `output/` (respuestas).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const FOLDERS: [&str; 3] = ["raw", "wiki", "output"];
const MASTER_INDEX: &str = "wiki/_master-index.md";
const CONFIG_FILE: &str = "config.json";

#[derive(Default, Serialize, Deserialize)]
pub struct Config {
    pub vault: Option<PathBuf>,
    /// Clave de la API de Anthropic guardada desde la app (la variable de entorno tiene prioridad).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

#[derive(Serialize)]
pub struct VaultInfo {
    path: String,
    name: String,
    /// Carpetas o archivos que nexo creó en esta llamada (vacío si ya existían).
    created: Vec<String>,
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(CONFIG_FILE))
}

pub fn read_config(app: &AppHandle) -> Config {
    config_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn write_config(app: &AppHandle, config: &Config) -> Result<(), String> {
    let path = config_path(app)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())?;
    // Contiene la clave de API: solo legible por el usuario.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Raíz del vault activo.
pub fn vault_root(app: &AppHandle) -> Result<PathBuf, String> {
    match read_config(app).vault {
        Some(root) if root.is_dir() => Ok(root),
        _ => Err("Primero elige un vault.".into()),
    }
}

/// Crea `raw/`, `wiki/`, `output/` y `wiki/_master-index.md` si faltan. Nunca sobrescribe.
fn ensure_structure(root: &Path) -> Result<Vec<String>, String> {
    let mut created = Vec::new();
    for folder in FOLDERS {
        let dir = root.join(folder);
        if !dir.is_dir() {
            fs::create_dir_all(&dir).map_err(|e| format!("No se pudo crear {folder}/: {e}"))?;
            created.push(format!("{folder}/"));
        }
    }
    let index = root.join(MASTER_INDEX);
    if !index.exists() {
        fs::write(&index, "# Índice maestro\n\nMantenido por nexo. Cada tema de `wiki/` enlaza aquí su `_index.md`.\n")
            .map_err(|e| format!("No se pudo crear {MASTER_INDEX}: {e}"))?;
        created.push(MASTER_INDEX.to_string());
    }
    Ok(created)
}

fn info(root: &Path, created: Vec<String>) -> VaultInfo {
    VaultInfo {
        path: root.display().to_string(),
        name: root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string()),
        created,
    }
}

/// Subcarpeta (`raw`, `wiki`…) del vault activo; error si aún no hay vault elegido.
pub fn vault_dir(app: &AppHandle, folder: &str) -> Result<PathBuf, String> {
    match read_config(app).vault {
        Some(root) if root.is_dir() => Ok(root.join(folder)),
        _ => Err("Primero elige un vault.".into()),
    }
}

/// Resuelve `rel` dentro de `base` y rechaza cualquier ruta que se salga de ella.
pub fn resolve_inside(base: &Path, rel: &str) -> Result<PathBuf, String> {
    let base = base.canonicalize().map_err(|e| e.to_string())?;
    let path = base
        .join(rel)
        .canonicalize()
        .map_err(|_| format!("No existe: {rel}"))?;
    if path.starts_with(&base) && path != base {
        Ok(path)
    } else {
        Err("Ruta fuera del vault.".into())
    }
}

#[derive(Deserialize)]
struct ObsidianConfig {
    #[serde(default)]
    vaults: std::collections::HashMap<String, ObsidianVault>,
}

#[derive(Deserialize)]
struct ObsidianVault {
    path: PathBuf,
    #[serde(default)]
    ts: u64,
}

/// Vaults que Obsidian conoce en este equipo (el más reciente primero).
#[tauri::command]
pub fn detect_vaults(app: AppHandle) -> Vec<VaultInfo> {
    let Ok(home) = app.path().home_dir() else {
        return Vec::new();
    };
    let candidates = [
        home.join(".config/obsidian/obsidian.json"),
        home.join(".var/app/md.obsidian.Obsidian/config/obsidian/obsidian.json"),
        home.join("snap/obsidian/current/.config/obsidian/obsidian.json"),
        home.join("Library/Application Support/obsidian/obsidian.json"),
        home.join("AppData/Roaming/obsidian/obsidian.json"),
    ];
    let mut vaults: Vec<ObsidianVault> = candidates
        .iter()
        .filter_map(|p| fs::read_to_string(p).ok())
        .filter_map(|s| serde_json::from_str::<ObsidianConfig>(&s).ok())
        .flat_map(|c| c.vaults.into_values())
        .filter(|v| v.path.is_dir())
        .collect();
    vaults.sort_by(|a, b| b.ts.cmp(&a.ts));
    vaults.dedup_by(|a, b| a.path == b.path);
    vaults
        .into_iter()
        .map(|v| info(&v.path, Vec::new()))
        .collect()
}

/// Vault guardado en la configuración, si sigue existiendo en disco.
#[tauri::command]
pub fn get_vault(app: AppHandle) -> Result<Option<VaultInfo>, String> {
    match read_config(&app).vault {
        Some(root) if root.is_dir() => Ok(Some(info(&root, ensure_structure(&root)?))),
        _ => Ok(None),
    }
}

/// Usa `path` como vault: prepara su estructura y lo recuerda para el próximo arranque.
#[tauri::command]
pub fn set_vault(app: AppHandle, path: String) -> Result<VaultInfo, String> {
    let root = PathBuf::from(path);
    if !root.is_dir() {
        return Err("La carpeta seleccionada no existe.".into());
    }
    let created = ensure_structure(&root)?;
    let mut config = read_config(&app);
    config.vault = Some(root.clone());
    write_config(&app, &config)?;
    Ok(info(&root, created))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `vault/raw/` con `nota.md` y `sub/`, la carpeta vecina `vault/raw-otro/` y, fuera
    /// del vault, `ajeno/secreto.md`.
    fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("nexo-vault-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("vault/raw/sub")).unwrap();
        fs::create_dir_all(root.join("vault/raw-otro")).unwrap();
        fs::create_dir_all(root.join("ajeno")).unwrap();
        fs::write(root.join("vault/raw/nota.md"), "dentro").unwrap();
        fs::write(root.join("vault/raw-otro/nota.md"), "vecina").unwrap();
        fs::write(root.join("ajeno/secreto.md"), "ajeno").unwrap();
        root
    }

    const OUTSIDE: &str = "Ruta fuera del vault.";

    #[test]
    fn resolve_inside_acepta_lo_que_hay_dentro() {
        let root = temp("dentro");
        let base = root.join("vault/raw");
        let real = base.canonicalize().unwrap();

        assert_eq!(resolve_inside(&base, "nota.md"), Ok(real.join("nota.md")));
        assert_eq!(resolve_inside(&base, "sub"), Ok(real.join("sub")));
        // Un rodeo que vuelve a entrar sigue dentro.
        assert_eq!(
            resolve_inside(&base, "sub/../nota.md"),
            Ok(real.join("nota.md"))
        );
    }

    #[test]
    fn resolve_inside_rechaza_lo_que_sale() {
        let root = temp("fuera");
        let base = root.join("vault/raw");

        for rel in [
            "..",
            "../../ajeno/secreto.md",
            "sub/../../../ajeno/secreto.md",
            // Empieza igual que la base, pero es otra carpeta.
            "../raw-otro/nota.md",
        ] {
            assert_eq!(resolve_inside(&base, rel), Err(OUTSIDE.into()), "{rel}");
        }
        // Una ruta absoluta sustituye a la base al unirlas.
        let absolute = root.join("ajeno/secreto.md");
        assert_eq!(
            resolve_inside(&base, absolute.to_str().unwrap()),
            Err(OUTSIDE.into())
        );
    }

    #[test]
    fn resolve_inside_rechaza_la_propia_base() {
        let root = temp("base");
        let base = root.join("vault/raw");

        for rel in ["", ".", "sub/.."] {
            assert_eq!(resolve_inside(&base, rel), Err(OUTSIDE.into()), "{rel:?}");
        }
    }

    #[test]
    fn resolve_inside_rechaza_lo_que_no_existe() {
        let root = temp("falta");
        let base = root.join("vault/raw");

        assert_eq!(
            resolve_inside(&base, "falta.md"),
            Err("No existe: falta.md".into())
        );
        assert!(resolve_inside(&root.join("vault/no-hay"), "nota.md").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_inside_sigue_los_enlaces_simbolicos() {
        use std::os::unix::fs::symlink;
        let root = temp("enlaces");
        let base = root.join("vault/raw");
        let real = base.canonicalize().unwrap();
        symlink(root.join("ajeno"), base.join("carpeta")).unwrap();
        symlink(root.join("ajeno/secreto.md"), base.join("archivo.md")).unwrap();
        symlink(root.join("ajeno/falta.md"), base.join("roto.md")).unwrap();
        symlink(base.join("sub"), base.join("alias")).unwrap();

        for rel in ["carpeta", "carpeta/secreto.md", "archivo.md"] {
            assert_eq!(resolve_inside(&base, rel), Err(OUTSIDE.into()), "{rel}");
        }
        assert_eq!(
            resolve_inside(&base, "roto.md"),
            Err("No existe: roto.md".into())
        );
        // El que apunta dentro vale, y devuelve la ruta real.
        assert_eq!(resolve_inside(&base, "alias"), Ok(real.join("sub")));
    }
}
