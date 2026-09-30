//! Estado de las conexiones de nexo: servidores MCP, API de Anthropic y Claude Code.
//! Ninguna comprobación consume tokens: el MCP solo hace el saludo y lista herramientas,
//! y la API se valida con la Models API.

use std::process::Command;

use serde::Serialize;
use tauri::AppHandle;

use crate::claude::{self, Claude};
use crate::claude_code::{SESSION_NAME, find_claude, find_named_session};
use crate::markitdown::{find_server, probe};

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
    let session = match find_named_session(app, SESSION_NAME) {
        Some(s) => format!("sesión {SESSION_NAME} en {}", s.cwd.display()),
        None => format!("sin sesión {SESSION_NAME}: se creará al abrirlo"),
    };
    base("ok", format!("{version} · {session}"))
}

#[tauri::command]
pub async fn connections_status(app: AppHandle) -> Vec<Connection> {
    let (markitdown, api) = tokio::join!(markitdown(&app), api(&app));
    let cli = claude_code(&app);
    vec![markitdown, api, cli]
}
