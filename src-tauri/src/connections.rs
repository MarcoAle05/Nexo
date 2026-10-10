//! Estado de las conexiones de nexo: servidores MCP, API de Anthropic y Claude Code.
//! Ninguna comprobación consume tokens: el MCP solo hace el saludo y lista herramientas,
//! y la API se valida con la Models API.

use std::process::Command;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::agy;
use crate::claude::{self, Claude};
use crate::claude_code::{current_session_name, find_claude};
use crate::markitdown::{find_server, probe};
use crate::playwright;

#[derive(Serialize)]
pub struct Connection {
    id: &'static str,
    name: &'static str,
    /// `MCP`, `API` o `CLI`.
    kind: &'static str,
    /// `ok`, `missing` (no instalado / sin configurar) o `error`.
    state: &'static str,
    detail: String,
}

async fn markitdown(app: &AppHandle) -> Connection {
    let base = |state, detail| Connection {
        id: "markitdown",
        name: "MarkItDown",
        kind: "MCP",
        state,
        detail,
    };
    let Some(server) = find_server(app) else {
        return base(
            "missing",
            "No instalado · uv tool install --python 3.12 markitdown-mcp".into(),
        );
    };
    let probed = tauri::async_runtime::spawn_blocking(move || probe(&server)).await;
    match probed {
        Ok(Ok(tools)) if tools.iter().any(|t| t == "convert_to_markdown") => base(
            "ok",
            "Activo · convert_to_markdown (PDF, Office, EPUB → Markdown, sin IA)".into(),
        ),
        Ok(Ok(_)) => base(
            "error",
            "Responde, pero no ofrece convert_to_markdown".into(),
        ),
        Ok(Err(error)) => base("error", error),
        Err(error) => base("error", error.to_string()),
    }
}

async fn api(app: &AppHandle) -> Connection {
    let base = |state, detail| Connection {
        id: "anthropic",
        name: "API de Anthropic",
        kind: "API",
        state,
        detail,
    };
    let Some(source) = claude::key_source(app) else {
        return base(
            "missing",
            "Sin clave · en la terminal: clave sk-ant-…".into(),
        );
    };
    let origin = if source == "env" {
        "ANTHROPIC_API_KEY"
    } else {
        "clave guardada"
    };
    let client = match Claude::new(app) {
        Ok(client) => client,
        Err(error) => return base("error", error),
    };
    match client.verify().await {
        Ok(model) => base("ok", format!("Conectada · {model} · {origin}")),
        Err(error) => base("error", format!("{error} · {origin}")),
    }
}

async fn playwright(app: &AppHandle) -> Connection {
    let base = |state, detail| Connection {
        id: "playwright",
        name: "Playwright",
        kind: "MCP",
        state,
        detail,
    };
    let Some(server) = playwright::find_server(app) else {
        return base(
            "missing",
            "No instalado · npm install -g --prefix ~/.local @playwright/mcp".into(),
        );
    };
    let workdir = match app.path().app_cache_dir() {
        Ok(dir) => dir.join("playwright-probe"),
        Err(error) => return base("error", error.to_string()),
    };
    let probed =
        tauri::async_runtime::spawn_blocking(move || playwright::probe(&server, &workdir)).await;
    let browser = if playwright::browser().is_some() {
        "Chrome"
    } else {
        "navegador de Playwright"
    };
    match probed {
        Ok(Ok(tools)) if tools.iter().any(|t| t == "browser_navigate") => base(
            "ok",
            format!(
                "Activo · abre páginas con JavaScript en {browser} sin ventana y las pasa a Markdown"
            ),
        ),
        Ok(Ok(_)) => base("error", "Responde, pero no ofrece browser_navigate".into()),
        Ok(Err(error)) => base("error", error),
        Err(error) => base("error", error.to_string()),
    }
}

fn antigravity(app: &AppHandle) -> Connection {
    let base = |state, detail| Connection {
        id: "antigravity",
        name: "Antigravity",
        kind: "CLI",
        state,
        detail,
    };
    match agy::find_agy(app) {
        None => base(
            "missing",
            "agy no instalado · sin él no se pueden añadir páginas web".into(),
        ),
        Some(path) => {
            let version = agy::version(&path).unwrap_or_else(|| "versión desconocida".into());
            base(
                "ok",
                format!("agy {version} · lee páginas web y las guarda en raw/web/"),
            )
        }
    }
}

fn claude_code(app: &AppHandle) -> Connection {
    let base = |state, detail| Connection {
        id: "claude-code",
        name: "Claude Code",
        kind: "CLI",
        state,
        detail,
    };
    let Some(path) = find_claude(app) else {
        return base(
            "missing",
            "No instalado · https://claude.com/claude-code".into(),
        );
    };
    let version = Command::new(&path)
        .arg("--version")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "versión desconocida".into());
    let session = match current_session_name(app) {
        Some(name) => format!("sesión de esta apertura: {name}"),
        None => "una sesión nueva por cada apertura de nexo".into(),
    };
    base("ok", format!("{version} · {session}"))
}

#[tauri::command]
pub async fn connections_status(app: AppHandle) -> Vec<Connection> {
    let (markitdown, playwright, api) = tokio::join!(markitdown(&app), playwright(&app), api(&app));
    let cli = claude_code(&app);
    vec![markitdown, playwright, api, antigravity(&app), cli]
}
