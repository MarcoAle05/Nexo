//! Conversión de documentos a Markdown con el servidor MCP de MarkItDown (`markitdown-mcp`).
//! Es una conversión local y determinista: no interviene ningún modelo de IA ni se gasta API.
//!
//! nexo actúa como cliente MCP por stdio (JSON-RPC, un mensaje por línea) y llama a la
//! herramienta `convert_to_markdown`. El resultado se guarda en `raw/markdown/`.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

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

/// Cliente MCP mínimo sobre stdio.
struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Mcp {
    fn start(server: &Path) -> Result<Self, String> {
        let mut child = Command::new(server)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("No se pudo iniciar markitdown-mcp: {e}"))?;
        let stdin = child.stdin.take().ok_or("sin stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("sin stdout")?);
        let mut mcp = Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        mcp.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "nexo", "version": env!("CARGO_PKG_VERSION")}
            }),
        )?;
        mcp.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))?;
        Ok(mcp)
    }

    fn send(&mut self, message: &Value) -> Result<(), String> {
        writeln!(self.stdin, "{message}").map_err(|e| e.to_string())?;
        self.stdin.flush().map_err(|e| e.to_string())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        let mut line = String::new();
        loop {
            line.clear();
            if self
                .stdout
                .read_line(&mut line)
                .map_err(|e| e.to_string())?
                == 0
            {
                return Err("markitdown-mcp se cerró inesperadamente".into());
            }
            // Se ignoran notificaciones y cualquier línea que no sea la respuesta a esta petición.
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if message["id"] != json!(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(error["message"].as_str().unwrap_or("error MCP").to_string());
            }
            return Ok(message["result"].clone());
        }
    }

    fn convert(&mut self, file: &Path) -> Result<String, String> {
        let result = self.request(
            "tools/call",
            json!({"name": "convert_to_markdown", "arguments": {"uri": file_uri(file)}}),
        )?;
        let text: String = result["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c["text"].as_str())
            .collect();
        if result["isError"] == true {
            return Err(text);
        }
        Ok(text)
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Arranca el servidor, hace el saludo MCP y lista sus herramientas. Bloqueante.
pub fn probe(server: &Path) -> Result<Vec<String>, String> {
    let mut mcp = Mcp::start(server)?;
    let tools = mcp.request("tools/list", json!({}))?;
    Ok(tools["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect())
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
    let mut mcp = Mcp::start(&server)?;
    let mut done = 0;
    for rel in pending {
        let source = raw.join(&rel);
        match mcp.convert(&source) {
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
        let mut mcp = Mcp::start(&server).expect("arranca el servidor");
        let md = mcp.convert(pdf).expect("convierte");
        assert!(md.contains("Speex"));
    }
}
