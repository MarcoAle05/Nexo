//! Lectura de páginas web con el servidor MCP de Playwright (`@playwright/mcp`), sin IA.
//!
//! Abre la página en Chrome sin ventana (sirve para webs que cargan su contenido con
//! JavaScript), extrae el HTML ya renderizado de su contenido principal y lo convierte a
//! Markdown con MarkItDown. Después Antigravity solo tiene que resumir ese Markdown.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use crate::markitdown;
use crate::mcp::McpClient;

/// Ejecutable que instala `npm install -g --prefix ~/.local @playwright/mcp`.
const SERVER: &str = "playwright-mcp";

/// Extrae del DOM renderizado el contenido principal, sin scripts ni navegación.
const EXTRACT: &str = r#"() => {
  const pick = document.querySelector('article') || document.querySelector('main') ||
    document.querySelector('[role="main"]') || document.body;
  const root = pick.cloneNode(true);
  root.querySelectorAll('script, style, noscript, iframe, svg, canvas, nav, footer, aside, form, button, dialog, [aria-hidden="true"], [role="navigation"], [role="banner"], [role="contentinfo"]')
    .forEach((el) => el.remove());
  const title = document.title || location.hostname;
  return { title, url: location.href, html: `<!doctype html><html><head><meta charset="utf-8"><title>${title}</title></head><body>${root.outerHTML}</body></html>` };
}"#;

pub fn find_server(app: &AppHandle) -> Option<PathBuf> {
    let from_path = std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .map(|p| p.join(SERVER))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let local = app
        .path()
        .home_dir()
        .ok()
        .map(|h| h.join(".local/bin").join(SERVER));
    from_path.into_iter().chain(local).find(|p| p.is_file())
}

/// Navegador que usará Playwright: Chrome instalado si existe; si no, el que traiga Playwright.
pub fn browser() -> Option<&'static str> {
    let chrome = [
        "/usr/bin/google-chrome-stable",
        "/usr/bin/google-chrome",
        "/opt/google/chrome/chrome",
    ]
    .iter()
    .any(|p| Path::new(p).is_file());
    chrome.then_some("chrome")
}

fn start(server: &Path, workdir: &Path) -> Result<McpClient, String> {
    let output = workdir.to_string_lossy().to_string();
    let mut args = vec!["--headless", "--isolated", "--output-dir", &output];
    if let Some(name) = browser() {
        args.extend(["--browser", name]);
    }
    McpClient::start(server, &args, Some(workdir), "Playwright MCP")
}

/// Saluda al servidor y lista sus herramientas, sin abrir el navegador. Bloqueante.
pub fn probe(server: &Path, workdir: &Path) -> Result<Vec<String>, String> {
    fs::create_dir_all(workdir).map_err(|e| e.to_string())?;
    start(server, workdir)?.list_tools()
}

/// Playwright devuelve el resultado de `browser_evaluate` en una sección `### Result`.
fn evaluate_result(text: &str) -> Result<Value, String> {
    let body = text
        .split("### Result")
        .nth(1)
        .ok_or("Playwright no devolvió resultado")?;
    let json = body.split("\n### ").next().unwrap_or(body).trim();
    serde_json::from_str(json).map_err(|e| format!("Resultado de Playwright ilegible: {e}"))
}

/// Frases de las páginas anti-bots (captchas, "demuestra que eres humano", Cloudflare…).
const BLOCK_MARKERS: [&str; 14] = [
    "prove your humanity",
    "verify you are human",
    "verify that you are human",
    "are you a robot",
    "not a robot",
    "complete the captcha",
    "checking your browser",
    "just a moment...",
    "enable javascript and cookies to continue",
    "unusual traffic from your computer",
    "access denied",
    "attention required! | cloudflare",
    "you've been blocked by network security",
    "please verify you are a human",
];

/// Si la página es una pantalla de bloqueo en lugar del contenido real, devuelve el motivo
/// con una sugerencia. Solo se mira en páginas cortas: un artículo largo que *hable* de
/// captchas no es un bloqueo.
pub fn detect_block(url: &str, title: &str, markdown: &str) -> Option<String> {
    if markdown.chars().count() > 4000 {
        return None;
    }
    let text = format!("{title}\n{markdown}").to_lowercase();
    BLOCK_MARKERS.iter().find(|m| text.contains(*m))?;
    let tip = if url.contains("://www.reddit.com") || url.contains("://reddit.com") {
        "Prueba el mismo enlace con old.reddit.com en lugar de www.reddit.com, o cambia a la otra opción de lectura."
    } else {
        "Prueba con la otra opción de lectura (Antigravity o Playwright)."
    };
    Some(format!(
        "La página mostró una comprobación anti-bots en lugar de su contenido; no se guardó. {tip}"
    ))
}

pub struct Page {
    pub title: String,
    pub url: String,
    pub markdown: String,
}

/// Abre `url` en el navegador, extrae el contenido y lo convierte a Markdown. Bloqueante.
/// `step` recibe el progreso para el panel de actividad.
pub fn read_page(
    app: &AppHandle,
    url: &str,
    workdir: &Path,
    step: impl Fn(&str),
) -> Result<Page, String> {
    let server = find_server(app).ok_or(
        "No encontré Playwright MCP. Instálalo con: npm install -g --prefix ~/.local @playwright/mcp",
    )?;
    let converter = markitdown::find_server(app)
        .ok_or("MarkItDown no está disponible para convertir la página.")?;
    read_page_with(&server, &converter, url, workdir, step)
}

fn read_page_with(
    server: &Path,
    converter: &Path,
    url: &str,
    workdir: &Path,
    step: impl Fn(&str),
) -> Result<Page, String> {
    fs::create_dir_all(workdir).map_err(|e| e.to_string())?;

    step(&format!(
        "Abre la página en {} (Playwright)",
        if browser().is_some() {
            "Chrome"
        } else {
            "el navegador"
        }
    ));
    let mut browser = start(server, workdir)?;
    browser.call_tool("browser_navigate", json!({"url": url}))?;
    // Margen para que terminen de cargar las páginas que pintan su contenido con JavaScript.
    let _ = browser.call_tool("browser_wait_for", json!({"time": 1.5}));
    step("Extrae el contenido renderizado");
    let data =
        evaluate_result(&browser.call_tool("browser_evaluate", json!({"function": EXTRACT}))?)?;
    let _ = browser.call_tool("browser_close", json!({}));
    drop(browser);

    let html = data["html"]
        .as_str()
        .ok_or("Playwright no devolvió el HTML")?;
    let file = workdir.join("pagina.html");
    fs::write(&file, html).map_err(|e| e.to_string())?;
    step("Convierte a Markdown (MarkItDown)");
    let mut mcp = McpClient::start(converter, &[], None, "markitdown-mcp")?;
    let markdown = markitdown::convert_file(&mut mcp, &file)?;
    if markdown.trim().is_empty() {
        return Err("La página no tiene texto que extraer.".into());
    }
    let title = data["title"].as_str().unwrap_or(url).trim().to_string();
    if let Some(reason) = detect_block(url, &title, &markdown) {
        return Err(reason);
    }
    Ok(Page {
        title,
        url: data["url"].as_str().unwrap_or(url).to_string(),
        markdown: markdown.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_el_resultado_de_evaluate() {
        let text = "### Result\n{\n  \"title\": \"Example Domain\",\n  \"html\": \"<p>hola</p>\"\n}\n### Ran Playwright code\n```js\n```";
        let value = evaluate_result(text).unwrap();
        assert_eq!(value["title"], "Example Domain");
        assert_eq!(value["html"], "<p>hola</p>");
    }

    /// Abre una página real con Chrome: `cargo test -- --ignored playwright_real`.
    #[test]
    #[ignore]
    fn playwright_real() {
        let home = std::env::var("HOME").unwrap();
        let bin = Path::new(&home).join(".local/bin");
        let dir = std::env::temp_dir().join(format!("nexo-pw-{}", std::process::id()));
        let page = read_page_with(
            &bin.join(SERVER),
            &bin.join("markitdown-mcp"),
            "https://example.com",
            &dir,
            |s| println!("· {s}"),
        )
        .expect("lee la página");
        println!("{} · {}\n{}", page.title, page.url, page.markdown);
        assert_eq!(page.title, "Example Domain");
        assert!(page.markdown.contains("documentation examples"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn detecta_paginas_de_bloqueo() {
        let reddit = detect_block(
            "https://www.reddit.com/r/x",
            "Reddit - Prove your humanity",
            "Please wait while we verify",
        )
        .unwrap();
        assert!(reddit.contains("old.reddit.com"));
        assert!(
            detect_block(
                "https://a.com",
                "Just a moment...",
                "Checking your browser before accessing"
            )
            .is_some()
        );
        assert!(
            detect_block(
                "https://a.com",
                "Guía de MCP",
                "El protocolo de contexto del modelo…"
            )
            .is_none()
        );
        let long = "captcha ".repeat(10) + &"texto largo sobre captchas. ".repeat(300);
        assert!(detect_block("https://a.com", "Cómo funcionan los captcha", &long).is_none());
    }
}
