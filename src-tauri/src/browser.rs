//! "Cargar navegador": copia el perfil del navegador predeterminado del usuario (sesiones
//! iniciadas, cookies, contraseñas guardadas, preferencias) a un perfil propio de nexo que
//! usa Playwright, tanto el lector de enlaces como el chat de Claude Code.
//!
//! El perfil original nunca se toca: se copia a `perfil.nuevo`, se comprueba abriéndolo con
//! Playwright (las cookies tienen que poder descifrarse) y solo entonces sustituye a la copia
//! anterior. Chrome cifra las cookies con una clave del llavero del sistema que es la misma
//! para cualquier perfil del mismo navegador, así que la copia sigue funcionando si se abre
//! con el mismo ejecutable y el mismo llavero (Playwright, por defecto, fuerza
//! `--password-store=basic`; aquí se le quita).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use crate::mcp::McpClient;
use crate::playwright;

/// Navegador basado en Chromium del que se puede copiar el perfil.
struct Family {
    /// Prefijo del `.desktop` que devuelve `xdg-settings`.
    desktop: &'static str,
    name: &'static str,
    /// Carpeta de datos dentro de `~/.config`.
    config: &'static str,
    /// Canal de Playwright (`chrome`, `msedge`) o, si no tiene, ejecutables posibles.
    channel: Option<&'static str>,
    executables: &'static [&'static str],
}

const FAMILIES: [Family; 5] = [
    Family {
        desktop: "google-chrome",
        name: "Google Chrome",
        config: "google-chrome",
        channel: Some("chrome"),
        executables: &[],
    },
    Family {
        desktop: "chromium",
        name: "Chromium",
        config: "chromium",
        channel: None,
        executables: &["/usr/bin/chromium", "/usr/bin/chromium-browser"],
    },
    Family {
        desktop: "brave",
        name: "Brave",
        config: "BraveSoftware/Brave-Browser",
        channel: None,
        executables: &[
            "/usr/bin/brave",
            "/usr/bin/brave-browser",
            "/opt/brave.com/brave/brave",
        ],
    },
    Family {
        desktop: "microsoft-edge",
        name: "Microsoft Edge",
        config: "microsoft-edge",
        channel: Some("msedge"),
        executables: &[],
    },
    Family {
        desktop: "vivaldi",
        name: "Vivaldi",
        config: "vivaldi",
        channel: None,
        executables: &["/usr/bin/vivaldi-stable", "/usr/bin/vivaldi"],
    },
];

/// Carpetas del perfil que no hacen falta para las sesiones y solo ocupan espacio
/// (cachés, extensiones, service workers) o reabrirían las pestañas del usuario (sesiones).
const SKIP: [&str; 16] = [
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "GrShaderCache",
    "ShaderCache",
    "Service Worker",
    "Extensions",
    "Shared Dictionary",
    "Sessions",
    "Sessions_Encrypted",
    "Current Session",
    "Current Tabs",
    "Last Session",
    "Last Tabs",
];

/// Lo que se guarda tras una copia correcta (`navegador/estado.json`).
#[derive(Serialize, Deserialize, Clone)]
pub struct Copy {
    browser: String,
    profile: String,
    copied_at: String,
    cookies: usize,
    sites: usize,
}

#[derive(Serialize)]
pub struct Status {
    /// Navegador predeterminado que se copiaría, o por qué no se puede.
    source: Result<String, String>,
    copy: Option<Copy>,
}

fn home(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().home_dir().map_err(|e| e.to_string())
}

fn dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("navegador"))
}

/// Carpeta del perfil de Playwright (existe aunque no se haya cargado nada: Chrome la crea).
pub fn profile_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(dir(app)?.join("perfil"))
}

fn read_copy(app: &AppHandle) -> Option<Copy> {
    let text = fs::read_to_string(dir(app).ok()?.join("estado.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// El navegador predeterminado según `xdg-settings`, si es de la familia Chromium.
fn default_family(app: &AppHandle) -> Result<&'static Family, String> {
    let home = home(app)?;
    let desktop = Command::new("xdg-settings")
        .args(["get", "default-web-browser"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_lowercase())
        .unwrap_or_default();
    if let Some(family) = FAMILIES.iter().find(|f| desktop.starts_with(f.desktop)) {
        return if home
            .join(".config")
            .join(family.config)
            .join("Local State")
            .is_file()
        {
            Ok(family)
        } else {
            Err(format!(
                "{} no tiene un perfil en ~/.config/{}",
                family.name, family.config
            ))
        };
    }
    if !desktop.is_empty() {
        let name = desktop.trim_end_matches(".desktop");
        return Err(format!(
            "Tu navegador predeterminado es {name}: solo se pueden copiar navegadores basados en Chromium (Chrome, Chromium, Brave, Edge, Vivaldi)."
        ));
    }
    // Sin xdg-settings: el primero que tenga perfil.
    FAMILIES
        .iter()
        .find(|f| {
            home.join(".config")
                .join(f.config)
                .join("Local State")
                .is_file()
        })
        .ok_or_else(|| "No encontré ningún navegador basado en Chromium con perfil.".into())
}

fn family_by_name(name: &str) -> Option<&'static Family> {
    FAMILIES.iter().find(|f| f.name == name)
}

/// Opciones de arranque de Playwright para abrir el perfil con el navegador de origen.
fn launch_options(family: Option<&Family>) -> Value {
    let mut options = json!({
        // Playwright fuerza un llavero falso; sin quitarlo, Chrome no descifra las cookies copiadas.
        "ignoreDefaultArgs": ["--password-store=basic", "--use-mock-keychain"],
    });
    let kde = std::env::var("XDG_CURRENT_DESKTOP")
        .map(|d| d.to_uppercase().contains("KDE"))
        .unwrap_or(false);
    if !kde {
        // Fuera de KDE/GNOME (p. ej. Hyprland) Chrome podría elegir el llavero básico.
        options["args"] = json!(["--password-store=gnome-libsecret"]);
    }
    match family {
        Some(f) => {
            if let Some(channel) = f.channel {
                options["channel"] = json!(channel);
            } else if let Some(exe) = f.executables.iter().find(|p| Path::new(p).is_file()) {
                options["executablePath"] = json!(exe);
            }
        }
        None => {
            if let Some(channel) = playwright::browser() {
                options["channel"] = json!(channel);
            }
        }
    }
    options
}

/// Escribe `navegador/playwright.json` (configuración de `playwright-mcp`) y devuelve su ruta.
fn write_config(app: &AppHandle, family: Option<&Family>) -> Result<PathBuf, String> {
    let dir = dir(app)?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let config = json!({
        "browser": {
            "browserName": "chromium",
            "launchOptions": launch_options(family),
        }
    });
    let path = dir.join("playwright.json");
    fs::write(&path, serde_json::to_string_pretty(&config).unwrap()).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Argumentos de `playwright-mcp` para abrir el perfil de nexo (con ventana salvo `headless`).
pub fn mcp_args(app: &AppHandle, headless: bool) -> Result<Vec<String>, String> {
    let family = read_copy(app)
        .and_then(|c| family_by_name(&c.browser))
        .or_else(|| default_family(app).ok());
    let config = write_config(app, family)?;
    let mut args = vec![
        "--config".to_string(),
        config.to_string_lossy().into_owned(),
        "--user-data-dir".to_string(),
        profile_dir(app)?.to_string_lossy().into_owned(),
    ];
    if headless {
        args.push("--headless".into());
    }
    Ok(args)
}

/// Proceso de Chrome que tiene abierto un perfil, según su `SingletonLock` (`host-pid`).
fn lock_holder(profile: &Path) -> Option<u32> {
    let target = fs::read_link(profile.join("SingletonLock")).ok()?;
    let pid: u32 = target.to_string_lossy().rsplit('-').next()?.parse().ok()?;
    Path::new(&format!("/proc/{pid}")).exists().then_some(pid)
}

/// Si el perfil cargado está disponible para el lector de enlaces: hay copia y nadie
/// (p. ej. el navegador de Claude Code) lo tiene abierto.
pub fn ready_for_reader(app: &AppHandle) -> bool {
    read_copy(app).is_some()
        && profile_dir(app).is_ok_and(|p| p.is_dir() && lock_holder(&p).is_none())
}

/// Copia recursiva sin enlaces simbólicos (los `Singleton*` de Chrome lo son) ni `SKIP`.
fn copy_tree(from: &Path, to: &Path, top: bool) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if top && SKIP.contains(&name_str.as_ref()) {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let target = to.join(&name);
        if kind.is_dir() {
            copy_tree(&entry.path(), &target, false)?;
        } else if kind.is_file() {
            // Un archivo que no se deja leer (bloqueado o efímero) no impide la copia;
            // las cookies se comprueban después abriendo el perfil.
            let _ = fs::copy(entry.path(), &target);
        }
    }
    Ok(())
}

/// Abre el perfil copiado sin ventana y cuenta las cookies que el navegador logra descifrar.
fn verify(app: &AppHandle, profile: &Path, family: &Family) -> Result<(usize, usize), String> {
    let server = playwright::find_server(app).ok_or(
        "Hace falta Playwright MCP para comprobar la copia: npm install -g --prefix ~/.local @playwright/mcp",
    )?;
    let config = write_config(app, Some(family))?;
    let workdir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("navegador-prueba");
    fs::create_dir_all(&workdir).map_err(|e| e.to_string())?;
    let args = [
        "--config".to_string(),
        config.to_string_lossy().into_owned(),
        "--user-data-dir".to_string(),
        profile.to_string_lossy().into_owned(),
        "--headless".to_string(),
        "--caps".to_string(),
        "storage".to_string(),
        "--output-dir".to_string(),
        workdir.to_string_lossy().into_owned(),
    ];
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut browser = McpClient::start(&server, &args, Some(&workdir), "Playwright MCP")?;
    browser.call_tool("browser_navigate", json!({"url": "about:blank"}))?;
    let listed = browser.call_tool("browser_cookie_list", json!({}));
    let _ = browser.call_tool("browser_close", json!({}));
    drop(browser);
    Ok(count_cookies(&listed?))
}

/// Cuenta cookies y sitios distintos en la salida de `browser_cookie_list`
/// (`nombre=valor (domain: x, path: /)` por línea). Los valores no se guardan.
fn count_cookies(text: &str) -> (usize, usize) {
    let mut sites = std::collections::BTreeSet::new();
    let mut cookies = 0;
    for line in text.lines() {
        let Some((_, rest)) = line.rsplit_once("(domain: ") else {
            continue;
        };
        let Some((domain, _)) = rest.split_once(',') else {
            continue;
        };
        cookies += 1;
        sites.insert(domain.trim().trim_start_matches('.').to_string());
    }
    (cookies, sites.len())
}

/// Copia el perfil del navegador predeterminado y sustituye la copia anterior. Bloqueante.
fn import(app: &AppHandle) -> Result<Copy, String> {
    let family = default_family(app)?;
    let source_root = home(app)?.join(".config").join(family.config);
    let local_state =
        fs::read_to_string(source_root.join("Local State")).map_err(|e| e.to_string())?;
    let profile = serde_json::from_str::<Value>(&local_state)
        .ok()
        .and_then(|v| v["profile"]["last_used"].as_str().map(String::from))
        .filter(|p| source_root.join(p).is_dir())
        .unwrap_or_else(|| "Default".into());

    let dir = dir(app)?;
    let fresh = dir.join("perfil.nuevo");
    if fresh.exists() {
        fs::remove_dir_all(&fresh).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&fresh).map_err(|e| e.to_string())?;
    fs::copy(source_root.join("Local State"), fresh.join("Local State"))
        .map_err(|e| e.to_string())?;
    copy_tree(&source_root.join(&profile), &fresh.join(&profile), true)?;

    let (cookies, sites) = match verify(app, &fresh, family) {
        Ok(counts) => counts,
        Err(error) => {
            let _ = fs::remove_dir_all(&fresh);
            return Err(format!("No se pudo abrir la copia: {error}"));
        }
    };
    let had_cookies =
        fs::metadata(source_root.join(&profile).join("Cookies")).is_ok_and(|m| m.len() > 32 * 1024);
    if cookies == 0 && had_cookies {
        let _ = fs::remove_dir_all(&fresh);
        return Err(format!(
            "Se copió el perfil, pero {} no pudo descifrar las cookies (llavero del sistema). La copia anterior se mantiene.",
            family.name
        ));
    }

    // Sustituye la copia anterior; si el navegador de Claude Code la tiene abierta, se cierra.
    let current = dir.join("perfil");
    if let Some(pid) = lock_holder(&current) {
        let _ = Command::new("kill").arg(pid.to_string()).status();
        for _ in 0..30 {
            if !Path::new(&format!("/proc/{pid}")).exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    if current.exists() {
        fs::remove_dir_all(&current)
            .map_err(|e| format!("No se pudo borrar la copia anterior: {e}"))?;
    }
    fs::rename(&fresh, &current).map_err(|e| e.to_string())?;

    let copy = Copy {
        browser: family.name.to_string(),
        profile,
        copied_at: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
        cookies,
        sites,
    };
    fs::write(
        dir.join("estado.json"),
        serde_json::to_string_pretty(&copy).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    write_config(app, Some(family))?;
    Ok(copy)
}

#[tauri::command]
pub fn browser_status(app: AppHandle) -> Status {
    Status {
        source: default_family(&app).map(|f| f.name.to_string()),
        copy: read_copy(&app),
    }
}

#[tauri::command]
pub async fn browser_import(app: AppHandle) -> Result<Copy, String> {
    tauri::async_runtime::spawn_blocking(move || import(&app))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuenta_cookies_y_sitios() {
        let text = "### Result\na=1 (domain: github.com, path: /)\nb=2 (domain: .github.com, path: /)\nc=x(y) (domain: .google.com, path: /)\n### Ran Playwright code\n```js\nawait page.context().cookies();\n```";
        assert_eq!(count_cookies(text), (3, 2));
    }

    /// Copia el perfil real de Chrome y lo abre con Playwright: `cargo test -- --ignored perfil_real`.
    #[test]
    #[ignore]
    fn perfil_real() {
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        let family = &FAMILIES[0];
        let source = home.join(".config").join(family.config);
        let tmp = std::env::temp_dir().join(format!("nexo-perfil-{}", std::process::id()));
        let profile = tmp.join("perfil");
        fs::create_dir_all(&profile).unwrap();
        fs::copy(source.join("Local State"), profile.join("Local State")).unwrap();
        copy_tree(&source.join("Default"), &profile.join("Default"), true).unwrap();
        let config = tmp.join("playwright.json");
        let value = json!({"browser": {"browserName": "chromium", "launchOptions": launch_options(Some(family))}});
        fs::write(&config, value.to_string()).unwrap();
        let (c, p, o) = (
            config.to_string_lossy().into_owned(),
            profile.to_string_lossy().into_owned(),
            tmp.to_string_lossy().into_owned(),
        );
        let args = [
            "--config",
            &c,
            "--user-data-dir",
            &p,
            "--headless",
            "--caps",
            "storage",
            "--output-dir",
            &o,
        ];
        let mut browser = McpClient::start(
            &home.join(".local/bin/playwright-mcp"),
            &args,
            Some(&tmp),
            "pw",
        )
        .unwrap();
        browser
            .call_tool("browser_navigate", json!({"url": "about:blank"}))
            .unwrap();
        let (cookies, sites) =
            count_cookies(&browser.call_tool("browser_cookie_list", json!({})).unwrap());
        let _ = browser.call_tool("browser_close", json!({}));
        drop(browser);
        println!("{cookies} cookies de {sites} sitios");
        let _ = fs::remove_dir_all(&tmp);
        assert!(cookies > 0);
    }
}
