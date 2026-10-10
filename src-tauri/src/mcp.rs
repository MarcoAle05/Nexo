//! Cliente MCP mínimo por stdio (JSON-RPC, un mensaje por línea), compartido por los
//! servidores que usa nexo: MarkItDown (documentos → Markdown) y Playwright (navegador).

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

pub struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    label: String,
}

impl McpClient {
    /// Arranca el servidor y hace el saludo MCP. `label` se usa en los mensajes de error.
    pub fn start(
        program: &Path,
        args: &[&str],
        cwd: Option<&Path>,
        label: &str,
    ) -> Result<Self, String> {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("No se pudo iniciar {label}: {e}"))?;
        let stdin = child.stdin.take().ok_or("sin stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("sin stdout")?);
        let mut mcp = Self {
            child,
            stdin,
            stdout,
            next_id: 1,
            label: label.to_string(),
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

    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
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
                return Err(format!("{} se cerró inesperadamente", self.label));
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

    pub fn list_tools(&mut self) -> Result<Vec<String>, String> {
        let tools = self.request("tools/list", json!({}))?;
        Ok(tools["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t["name"].as_str().map(str::to_string))
            .collect())
    }

    /// Llama a una herramienta y devuelve su texto; `isError` se convierte en `Err`.
    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<String, String> {
        let result = self.request("tools/call", json!({"name": name, "arguments": arguments}))?;
        let text: String = result["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c["text"].as_str())
            .collect();
        if result["isError"] == true {
            Err(text)
        } else {
            Ok(text)
        }
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
