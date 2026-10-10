//! Conversión de documentos a Markdown con el servidor MCP de MarkItDown (`markitdown-mcp`).
//! Es una conversión local y determinista: no interviene ningún modelo de IA ni se gasta API.
//!
//! nexo actúa como cliente MCP por stdio (JSON-RPC, un mensaje por línea) y llama a la
//! herramienta `convert_to_markdown`. El resultado se guarda en `raw/markdown/`.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use crate::mcp::McpClient;
use crate::vault::vault_dir;

/// Carpeta dentro de `raw/` donde se guardan las conversiones.
pub const MARKDOWN_DIR: &str = "markdown";
const CONVERTIBLE: [&str; 6] = ["pdf", "docx", "pptx", "xlsx", "xls", "epub"];

pub fn is_convertible(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .is_some_and(|e| CONVERTIBLE.contains(&e.as_str()))
}

/// `raw/docs/paper.pdf` → `raw/markdown/docs/paper.md`
pub fn converted_path(raw: &Path, rel: &str) -> PathBuf {
    let rel = Path::new(rel);
    raw.join(MARKDOWN_DIR).join(rel.with_extension("md"))
}

/// La conversión existe y es posterior al archivo original.
pub fn is_fresh(raw: &Path, rel: &str) -> bool {
    let modified = |p: &Path| fs::metadata(p).and_then(|m| m.modified()).ok();
    match (
        modified(&raw.join(rel)),
        modified(&converted_path(raw, rel)),
    ) {
        (Some(src), Some(md)) => md >= src,
        _ => false,
    }
}

fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(byte as char)
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

pub fn find_server(app: &AppHandle) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "markitdown-mcp.exe"
    } else {
        "markitdown-mcp"
    };
    let from_path = std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .map(|p| p.join(name))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let local = app
        .path()
        .home_dir()
        .ok()
        .map(|h| h.join(".local/bin").join(name));
    from_path.into_iter().chain(local).find(|p| p.is_file())
}

fn start(server: &Path) -> Result<McpClient, String> {
    McpClient::start(server, &[], None, "markitdown-mcp")
}

/// Convierte un archivo local a Markdown con la herramienta `convert_to_markdown`.
pub fn convert_file(mcp: &mut McpClient, file: &Path) -> Result<String, String> {
    mcp.call_tool("convert_to_markdown", json!({"uri": file_uri(file)}))
}

/// Arranca el servidor, hace el saludo MCP y lista sus herramientas. Bloqueante.
pub fn probe(server: &Path) -> Result<Vec<String>, String> {
    start(server)?.list_tools()
}

#[derive(Clone, serde::Serialize)]
pub struct Progress {
    kind: &'static str,
    text: String,
}

/// Fuentes de la lista que se pueden convertir y no tienen una conversión al día.
pub fn pending(raw: &Path, rels: impl IntoIterator<Item = String>) -> Vec<String> {
    rels.into_iter()
        .filter(|rel| is_convertible(Path::new(rel)) && !is_fresh(raw, rel))
        .collect()
}

/// Convierte `pending` con un único proceso de markitdown-mcp. Bloqueante.
pub fn convert_blocking(
    app: &AppHandle,
    raw: &Path,
    pending: Vec<String>,
    send: impl Fn(&'static str, String),
) -> Result<usize, String> {
    if pending.is_empty() {
        return Ok(0);
    }
    let server = find_server(app).ok_or(
        "No encontré markitdown-mcp. Instálalo con: uv tool install --python 3.12 markitdown-mcp",
    )?;
    let mut mcp = start(&server)?;
    let mut done = 0;
    for rel in pending {
        let source = raw.join(&rel);
        match convert_file(&mut mcp, &source) {
            Ok(markdown) if !markdown.trim().is_empty() => {
                let target = converted_path(raw, &rel);
                if let Some(dir) = target.parent() {
                    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                let date = chrono::Local::now().format("%Y-%m-%d %H:%M");
                let doc = format!(
                    "---\nsource: raw/{rel}\nconverted: {date}\nconverter: markitdown-mcp\n---\n\n{}\n",
                    markdown.trim()
                );
                fs::write(&target, doc).map_err(|e| e.to_string())?;
                send("step", format!("markdown ← raw/{rel}"));
                done += 1;
            }
            Ok(_) => send(
                "step",
                format!("sin texto extraíble: raw/{rel} (¿PDF escaneado?)"),
            ),
            Err(error) => send("error", format!("no se pudo convertir raw/{rel}: {error}")),
        }
    }
    Ok(done)
}

/// Convierte a Markdown las fuentes indicadas (o todas las pendientes si `paths` está vacío).
/// Devuelve cuántas se convirtieron.
#[tauri::command]
pub async fn convert_sources(
    app: AppHandle,
    paths: Vec<String>,
    channel: Channel<Progress>,
) -> Result<usize, String> {
    let raw = vault_dir(&app, "raw")?;
    let targets = if paths.is_empty() {
        let mut all = Vec::new();
        crate::sources::walk_rel(&raw, &raw, &mut all);
        all
    } else {
        paths
    };
    let pending = pending(&raw, targets);
    if pending.is_empty() {
        return Ok(0);
    }
    tauri::async_runtime::spawn_blocking(move || {
        convert_blocking(&app, &raw, pending, |kind, text| {
            let _ = channel.send(Progress { kind, text });
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rutas_y_uris() {
        let raw = Path::new("/v/raw");
        assert_eq!(
            converted_path(raw, "docs/paper.pdf"),
            Path::new("/v/raw/markdown/docs/paper.md")
        );
        assert_eq!(
            file_uri(Path::new("/v/raw/mi paper ñ.pdf")),
            "file:///v/raw/mi%20paper%20%C3%B1.pdf"
        );
        assert!(is_convertible(Path::new("a.PDF")));
        assert!(!is_convertible(Path::new("a.md")));
    }
}

#[cfg(test)]
mod integracion {
    use super::*;

    /// Solo corre si markitdown-mcp está instalado: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn convierte_un_pdf_real() {
        let home = std::env::var("HOME").unwrap();
        let server = Path::new(&home).join(".local/bin/markitdown-mcp");
        let pdf = Path::new("/usr/share/doc/speexdsp/manual.pdf");
        let mut mcp = start(&server).expect("arranca el servidor");
        let md = convert_file(&mut mcp, pdf).expect("convierte");
        assert!(md.contains("Speex"));
    }
}
