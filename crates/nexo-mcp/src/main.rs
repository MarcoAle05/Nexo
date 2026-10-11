//! `nexo-mcp --vault <ruta>`: servidor MCP por stdio sobre un vault de Nexo, solo lectura.
//! stdout es exclusivamente del protocolo: todo mensaje humano va a stderr.

mod server;

use std::path::PathBuf;
use std::process::ExitCode;

use rmcp::{ServiceExt, transport::stdio};

const USAGE: &str = "Uso: nexo-mcp --vault <ruta del vault>\n\
También vale la variable NEXO_VAULT. El vault debe tener una carpeta wiki/.";

fn vault_from_args(args: &[String], env: Option<String>) -> Result<PathBuf, String> {
    let mut vault = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--vault" => {
                vault = Some(rest.next().ok_or("--vault necesita una ruta")?.clone());
            }
            other => match other.strip_prefix("--vault=") {
                Some(path) => vault = Some(path.to_string()),
                None => return Err(format!("Argumento desconocido: {other}")),
            },
        }
    }
    vault
        .or(env)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "Falta el vault (--vault o NEXO_VAULT).".into())
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("nexo-mcp {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let root = match vault_from_args(&args, std::env::var("NEXO_VAULT").ok()) {
        Ok(root) => root,
        Err(error) => {
            eprintln!("nexo-mcp: {error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let root = match root.canonicalize() {
        Ok(root) if root.join("wiki").is_dir() => root,
        _ => {
            eprintln!(
                "nexo-mcp: {} no es un vault (falta la carpeta wiki/).",
                root.display()
            );
            return ExitCode::from(2);
        }
    };
    match server::NexoMcp::new(root).serve(stdio()).await {
        Ok(service) => {
            let _ = service.waiting().await;
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("nexo-mcp: no se pudo arrancar: {error}");
            ExitCode::FAILURE
        }
    }
}
