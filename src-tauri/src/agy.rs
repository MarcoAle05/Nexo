//! Antigravity (`agy`) es el encargado de las fuentes: lee páginas web y resume fuentes.
//!
//! Cada tarea es un trabajo con sus pasos en vivo (salida `stream-json` de `agy`), que la
//! interfaz muestra en "Actividad Antigravity" mediante el evento global `agy-activity`.
//! Los trabajos se ejecutan de uno en uno: los demás esperan en cola.
//!
//! Permisos: en modo no interactivo `agy` no puede preguntar, así que antes de leer una URL
//! se añade `read_url(<dominio>)` a su `settings.json` (solo ese dominio, nunca `read_url(*)`).
//! No se usa `--dangerously-skip-permissions`. Cada trabajo corre en una carpeta vacía de la
//! caché; para resumir se copia ahí el Markdown de la fuente y nexo escribe los resultados.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::fichas::{self, Summary};
use crate::vault::{resolve_inside, vault_root};

/// Carpeta dentro de `raw/` donde se guardan las páginas web.
pub const WEB_DIR: &str = "web";
const TIMEOUT_SECS: u64 = 300;
const MAX_JOBS: usize = 40;

const WEB_PROMPT: &str = "Usa la herramienta read_url_content para leer esta página web: {url}\n\
No ejecutes comandos de terminal y no crees ni modifiques archivos.\n\n\
Devuelve:\n\
- title: el título real de la página.\n\
- markdown: su contenido principal como Markdown fiel al original. Conserva encabezados, párrafos, listas, \
tablas, bloques de código y enlaces. No resumas ni traduzcas. Omite menús, pies de página, anuncios, \
banners de cookies y botones para compartir.\n\
- summary: un resumen en español de 3 a 5 frases de lo que dice la página (no de tu tarea).\n\
- key_points: de 3 a 6 ideas clave, en español, una frase cada una.\n\
- concepts: de 3 a 8 conceptos o temas que trata, en kebab-case. Si alguno coincide con estas notas \
existentes de la wiki, usa exactamente su nombre: {notes}";

const SUMMARY_PROMPT: &str = "En tu carpeta de trabajo está el archivo fuente.md con el contenido de una \
fuente llamada «{name}». Léelo con view_file (solo ese archivo). No ejecutes comandos de terminal y no \
crees ni modifiques archivos.\n\n\
Devuelve, en español:\n\
- summary: un resumen de 3 a 5 frases de lo que dice la fuente.\n\
- key_points: de 3 a 6 ideas clave, una frase cada una.\n\
- concepts: de 3 a 8 conceptos o temas que trata, en kebab-case. Si alguno coincide con estas notas \
existentes de la wiki, usa exactamente su nombre: {notes}";

const WEB_SCHEMA: &str = r#"{"type":"object","properties":{"title":{"type":"string"},"markdown":{"type":"string"},"summary":{"type":"string"},"key_points":{"type":"array","items":{"type":"string"}},"concepts":{"type":"array","items":{"type":"string"}}},"required":["title","markdown","summary","key_points","concepts"],"additionalProperties":false}"#;
const SUMMARY_SCHEMA: &str = r#"{"type":"object","properties":{"summary":{"type":"string"},"key_points":{"type":"array","items":{"type":"string"}},"concepts":{"type":"array","items":{"type":"string"}}},"required":["summary","key_points","concepts"],"additionalProperties":false}"#;

// ---------------------------------------------------------------- trabajos y actividad

#[derive(Clone, Serialize)]
pub struct Step {
    index: u64,
    label: String,
    /// `active`, `done` o `error`.
    state: &'static str,
    seconds: Option<f64>,
}

#[derive(Clone, Serialize)]
pub struct Job {
    id: u64,
    /// `web` o `resumen`.
    kind: &'static str,
    title: String,
    /// `queued`, `running`, `done` o `error`.
    status: &'static str,
    started: u64,
    finished: Option<u64>,
    steps: Vec<Step>,
    tokens: u64,
    result: Option<String>,
    error: Option<String>,
}

#[derive(Default)]
pub struct AgyState {
    jobs: Mutex<Vec<Job>>,
    next: Mutex<u64>,
    /// Un solo `agy` a la vez.
    queue: tokio::sync::Mutex<()>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn update(app: &AppHandle, id: u64, change: impl FnOnce(&mut Job)) {
    let state = app.state::<AgyState>();
    let Ok(mut jobs) = state.jobs.lock() else {
        return;
    };
    if let Some(job) = jobs.iter_mut().find(|j| j.id == id) {
        change(job);
        let _ = app.emit("agy-activity", job.clone());
    }
}

fn new_job(app: &AppHandle, kind: &'static str, title: String) -> u64 {
    let state = app.state::<AgyState>();
    let id = {
        let mut next = state.next.lock().unwrap();
        *next += 1;
        *next
    };
    let job = Job {
        id,
        kind,
        title,
        status: "queued",
        started: now_ms(),
        finished: None,
        steps: Vec::new(),
        tokens: 0,
        result: None,
        error: None,
    };
    if let Ok(mut jobs) = state.jobs.lock() {
        jobs.insert(0, job.clone());
        // Se descartan los terminados más antiguos, nunca los que siguen en marcha.
        while jobs.len() > MAX_JOBS {
            match jobs.iter().rposition(|j| j.finished.is_some()) {
                Some(i) => {
                    jobs.remove(i);
                }
                None => break,
            }
        }
    }
    let _ = app.emit("agy-activity", job);
    id
}

fn finish(app: &AppHandle, id: u64, outcome: &Result<String, String>) {
    update(app, id, |job| {
        job.finished = Some(now_ms());
        match outcome {
            Ok(result) => {
                job.status = "done";
                job.result = Some(result.clone());
            }
            Err(error) => {
                job.status = "error";
                job.error = Some(error.clone());
            }
        }
        for step in job.steps.iter_mut().filter(|s| s.state == "active") {
            step.state = if outcome.is_ok() { "done" } else { "error" };
        }
    });
}

#[tauri::command]
pub fn agy_jobs(state: State<'_, AgyState>) -> Vec<Job> {
    state.jobs.lock().map(|j| j.clone()).unwrap_or_default()
}

#[tauri::command]
pub fn agy_clear_finished(state: State<'_, AgyState>) {
    if let Ok(mut jobs) = state.jobs.lock() {
        jobs.retain(|j| j.finished.is_none());
    }
}

/// Texto legible para un paso de `agy`.
fn step_label(step: &Value) -> String {
    let params = &step["tool_info"]["parameters"];
    let first = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| params[*k].as_str())
            .unwrap_or("")
            .to_string()
    };
    let short = |s: String| {
        if s.chars().count() > 70 {
            format!("{}…", s.chars().take(70).collect::<String>())
        } else {
            s
        }
    };
    match step["step_type"].as_str().unwrap_or("") {
        "user_input" => "Recibe la tarea".into(),
        "agent_response" => "Razona".into(),
        "tool" => match step["tool_name"].as_str().unwrap_or("herramienta") {
            "read_url_content" => format!("Lee {}", short(first(&["Url", "url"]))),
            "view_file" => {
                let path = first(&["AbsolutePath", "path"]);
                format!("Abre {}", path.rsplit('/').next().unwrap_or(&path))
            }
            "run_command" => format!("Intenta ejecutar «{}»", short(first(&["CommandLine"]))),
            "search_web" => format!("Busca «{}»", short(first(&["query", "Query"]))),
            "finish" => "Termina".into(),
            other => other.replace('_', " "),
        },
        "finish" => "Termina".into(),
        other => other.replace('_', " "),
    }
}

/// Ejecuta `agy` con la salida en `stream-json` y devuelve `structured_output`.
async fn run(
    app: &AppHandle,
    id: u64,
    prompt: String,
    schema: &'static str,
    workdir: PathBuf,
) -> Result<Value, String> {
    let state = app.state::<AgyState>();
    let _turn = state.queue.lock().await;
    update(app, id, |job| {
        job.status = "running";
        job.started = now_ms();
    });

    let agy = find_agy(app).ok_or("No encontré Antigravity (agy). Revisa Conexiones.")?;
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut child = Command::new(agy)
            .current_dir(&workdir)
            .arg(format!("-p={prompt}"))
            .args(["--output-format", "stream-json", "--json-schema", schema])
            .args(["--print-timeout", &format!("{TIMEOUT_SECS}s")])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("No se pudo ejecutar agy: {e}"))?;
        let stdout = child.stdout.take().ok_or("sin salida de agy")?;
        let mut result = Value::Null;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Ok(event) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match event["event"].as_str() {
                Some("step_update") => {
                    let step = &event["step_update"];
                    let index = step["step_index"].as_u64().unwrap_or(0);
                    let state = match step["state"].as_str() {
                        Some("ACTIVE") => "active",
                        Some("DONE") => "done",
                        _ => "error",
                    };
                    let label = step_label(step);
                    let seconds = step["duration_seconds"].as_f64();
                    let tokens = step["usage"]["total_tokens"].as_u64().unwrap_or(0);
                    update(&app2, id, |job| {
                        job.tokens += tokens;
                        match job.steps.iter_mut().find(|s| s.index == index) {
                            Some(s) => {
                                s.state = state;
                                s.seconds = seconds.or(s.seconds);
                            }
                            None => job.steps.push(Step {
                                index,
                                label,
                                state,
                                seconds,
                            }),
                        }
                    });
                }
                Some("result") => result = event["result"].clone(),
                _ => {}
            }
        }
        let stderr = child
            .stderr
            .take()
            .map(|e| {
                BufReader::new(e)
                    .lines()
                    .map_while(Result::ok)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let _ = child.wait();
        let _ = fs::remove_dir_all(&workdir);

        if result.is_null() {
            return Err(format!(
                "agy terminó sin resultado. {}",
                stderr.last().map(String::as_str).unwrap_or("")
            ));
        }
        if let Some(tokens) = result["usage"]["total_tokens"].as_u64() {
            update(&app2, id, |job| job.tokens = tokens);
        }
        if let Some(denied) = result["denied_actions"]
            .as_array()
            .filter(|d| !d.is_empty())
        {
            let names: Vec<_> = denied
                .iter()
                .filter_map(|d| d["display_name"].as_str().or(d["action"].as_str()))
                .collect();
            return Err(format!(
                "Antigravity no tuvo permiso para: {}",
                names.join(", ")
            ));
        }
        if result["status"] != "SUCCESS" {
            // Se muestra el motivo que dé agy (respuesta, error o su última línea de registro).
            let reason = [
                &result["error"]["message"],
                &result["error"],
                &result["response"],
            ]
            .iter()
            .find_map(|v| {
                v.as_str()
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
            })
            .or_else(|| stderr.iter().rev().find(|l| !l.trim().is_empty()).cloned())
            .unwrap_or_else(|| "sin detalles".into());
            let reason: String = reason.chars().take(300).collect();
            return Err(format!(
                "Antigravity terminó con estado {}: {reason}",
                result["status"].as_str().unwrap_or("desconocido")
            ));
        }
        let data = result["structured_output"].clone();
        if data.is_object() {
            Ok(data)
        } else {
            Err("Antigravity no devolvió el resultado con el formato pedido.".into())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

fn workdir(app: &AppHandle, id: u64) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("agy")
        .join(id.to_string());
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn summary_from(data: &Value, existing: &[String]) -> Summary {
    let list = |key: &str| -> Vec<String> {
        data[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect()
    };
    Summary {
        summary: data["summary"].as_str().unwrap_or("").trim().to_string(),
        key_points: list("key_points").into_iter().take(6).collect(),
        concepts: fichas::normalize_concepts(&list("concepts"), existing),
    }
}

fn notes_hint(existing: &[String]) -> String {
    if existing.is_empty() {
        return "(la wiki aún no tiene notas)".into();
    }
    existing
        .iter()
        .take(300)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------- utilidades

pub fn find_agy(app: &AppHandle) -> Option<PathBuf> {
    let name = if cfg!(windows) { "agy.exe" } else { "agy" };
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

pub fn version(agy: &Path) -> Option<String> {
    Command::new(agy)
        .arg("--version")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Dominio de una URL http(s), en minúsculas y sin puerto ni credenciales.
fn host_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority
        .rsplit('@')
        .next()?
        .split(':')
        .next()?
        .to_lowercase();
    let valid = !host.is_empty()
        && host.contains('.')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    valid.then_some(host)
}

/// Añade `read_url(<host>)` a `permissions.allow` del settings.json de agy, conservando el resto.
fn allow_domain(app: &AppHandle, host: &str) -> Result<bool, String> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let path = home.join(".gemini/antigravity-cli/settings.json");
    let mut settings: Value = fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    let rule = json!(format!("read_url({host})"));
    let allow = settings
        .as_object_mut()
        .ok_or("El settings.json de agy no tiene el formato esperado.")?
        .entry("permissions")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("permissions no es un objeto")?
        .entry("allow")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("permissions.allow no es una lista")?;
    if allow.contains(&rule) {
        return Ok(false);
    }
    allow.push(rule);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(true)
}

// ---------------------------------------------------------------- comandos

/// Guarda una página como fuente en `raw/web/` y devuelve su ruta relativa a `raw/`.
fn save_web(
    root: &Path,
    title: &str,
    url: &str,
    markdown: &str,
    fetched_by: &str,
) -> Result<String, String> {
    let dir = root.join("raw").join(WEB_DIR);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let base = fichas::slug(title);
    let target = (1..)
        .map(|n| {
            if n == 1 {
                dir.join(format!("{base}.md"))
            } else {
                dir.join(format!("{base}-{n}.md"))
            }
        })
        .find(|p| !p.exists())
        .expect("siempre hay un nombre libre");
    let date = chrono::Local::now().format("%Y-%m-%d %H:%M");
    let doc = format!(
        "---\ntitle: \"{}\"\nsource: {url}\nfetched: {date}\nfetched_by: {fetched_by}\n---\n\n{markdown}\n",
        title.replace('"', "'")
    );
    fs::write(&target, doc).map_err(|e| e.to_string())?;
    Ok(format!(
        "{WEB_DIR}/{}",
        target.file_name().unwrap().to_string_lossy()
    ))
}

fn push_step(app: &AppHandle, id: u64, index: u64, label: String, state: &'static str) {
    update(app, id, |job| {
        match job.steps.iter_mut().find(|s| s.index == index) {
            Some(step) => step.state = state,
            None => job.steps.push(Step {
                index,
                label,
                state,
                seconds: None,
            }),
        }
    });
}

/// Añade una página web como fuente con su ficha y resumen.
/// - Sin Playwright: Antigravity lee la URL y resume en una sola llamada.
/// - Con Playwright: nexo abre la página en Chrome sin ventana, la convierte a Markdown con
///   MarkItDown (sin IA) y Antigravity solo resume ese Markdown (no necesita permiso de lectura web).
#[tauri::command]
pub async fn add_web_source(
    app: AppHandle,
    url: String,
    playwright: Option<bool>,
) -> Result<String, String> {
    let url = url.trim().to_string();
    let host =
        host_of(&url).ok_or("Escribe un enlace completo que empiece por http:// o https://")?;
    let root = vault_root(&app)?;
    let id = new_job(&app, "web", url.clone());
    let outcome = async {
        let existing = fichas::wiki_note_names(&root);
        if playwright.unwrap_or(false) {
            update(&app, id, |job| job.status = "running");
            let dir = workdir(&app, id)?;
            let (app2, url2, dir2) = (app.clone(), url.clone(), dir.clone());
            let page = tauri::async_runtime::spawn_blocking(move || {
                let counter = std::cell::Cell::new(1000u64);
                crate::playwright::read_page(&app2, &url2, &dir2.join("navegador"), |label| {
                    let index = counter.get();
                    if index > 1000 {
                        push_step(&app2, id, index - 1, String::new(), "done");
                    }
                    push_step(&app2, id, index, label.to_string(), "active");
                    counter.set(index + 1);
                })
                .inspect(|_| push_step(&app2, id, counter.get() - 1, String::new(), "done"))
            })
            .await
            .map_err(|e| e.to_string())??;
            let title = if page.title.is_empty() {
                host.clone()
            } else {
                page.title.clone()
            };
            let rel = save_web(
                &root,
                &title,
                &page.url,
                &page.markdown,
                "playwright + markitdown",
            )?;

            // Antigravity resume el Markdown desde su carpeta aislada.
            fs::write(dir.join("fuente.md"), &page.markdown).map_err(|e| e.to_string())?;
            let prompt = SUMMARY_PROMPT
                .replace("{name}", &title)
                .replace("{notes}", &notes_hint(&existing));
            let data = run(&app, id, prompt, SUMMARY_SCHEMA, dir).await?;
            fichas::write_summary(&root, &rel, "web", &summary_from(&data, &existing))?;
            fichas::write_index(&root)?;
            return Ok(rel);
        }

        if allow_domain(&app, &host)? {
            push_step(
                &app,
                id,
                900,
                format!("Permiso de lectura para {host}"),
                "done",
            );
        }
        let prompt = WEB_PROMPT
            .replace("{url}", &url)
            .replace("{notes}", &notes_hint(&existing));
        let data = run(&app, id, prompt, WEB_SCHEMA, workdir(&app, id)?).await?;
        let markdown = data["markdown"].as_str().unwrap_or("").trim();
        if markdown.is_empty() {
            return Err("Antigravity no pudo extraer contenido de la página.".into());
        }
        let title = data["title"]
            .as_str()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or(&host)
            .to_string();
        if let Some(reason) = crate::playwright::detect_block(&url, &title, markdown) {
            return Err(reason);
        }
        let rel = save_web(&root, &title, &url, markdown, "antigravity (agy)")?;
        fichas::write_summary(&root, &rel, "web", &summary_from(&data, &existing))?;
        fichas::write_index(&root)?;
        Ok(rel)
    }
    .await;
    finish(
        &app,
        id,
        &outcome
            .as_ref()
            .map(|rel| format!("raw/{rel}"))
            .map_err(Clone::clone),
    );
    outcome
}

/// Texto de una fuente para resumir: su conversión a Markdown o el propio archivo de texto.
fn source_text(root: &Path, rel: &str) -> Option<String> {
    let raw = root.join("raw");
    if crate::markitdown::is_fresh(&raw, rel) {
        return fs::read_to_string(crate::markitdown::converted_path(&raw, rel)).ok();
    }
    let lower = rel.to_lowercase();
    let text_like = [".md", ".markdown", ".txt", ".html", ".htm"]
        .iter()
        .any(|e| lower.ends_with(e));
    text_like
        .then(|| fs::read_to_string(raw.join(rel)).ok())
        .flatten()
        .filter(|t| !t.trim().is_empty())
}

/// Antigravity resume una fuente y actualiza su ficha (nodo del grafo).
#[tauri::command]
pub async fn summarize_source(app: AppHandle, path: String) -> Result<String, String> {
    let root = vault_root(&app)?;
    resolve_inside(&root.join("raw"), &path)?;
    let name = fichas::display_name(&path);
    let id = new_job(&app, "resumen", name.clone());
    let outcome = async {
        let text = source_text(&root, &path)
            .ok_or("Esta fuente no tiene texto: convierte el PDF o añade una fuente de texto.")?;
        let dir = workdir(&app, id)?;
        fs::write(dir.join("fuente.md"), text).map_err(|e| e.to_string())?;
        let existing = fichas::wiki_note_names(&root);
        let prompt = SUMMARY_PROMPT
            .replace("{name}", &name)
            .replace("{notes}", &notes_hint(&existing));
        let data = run(&app, id, prompt, SUMMARY_SCHEMA, dir).await?;
        let summary = summary_from(&data, &existing);
        if summary.summary.is_empty() {
            return Err("Antigravity devolvió un resumen vacío.".into());
        }
        let kind = crate::sources::kind_for(&path);
        fichas::write_summary(&root, &path, kind, &summary)?;
        fichas::write_index(&root)?;
        Ok(format!("ficha de {name} actualizada"))
    }
    .await;
    finish(&app, id, &outcome);
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dominios() {
        assert_eq!(
            host_of("https://Example.com/a?b").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            host_of("http://user:pw@docs.rs:8080/x").as_deref(),
            Some("docs.rs")
        );
        assert_eq!(host_of("ftp://example.com"), None);
        assert_eq!(host_of("https://localhost/"), None);
        assert_eq!(host_of("https://evil.com)(*"), None);
    }

    #[test]
    fn etiquetas_de_pasos() {
        let read = json!({"step_type": "tool", "tool_name": "read_url_content", "tool_info": {"parameters": {"Url": "https://a.com"}}});
        assert_eq!(step_label(&read), "Lee https://a.com");
        assert_eq!(
            step_label(&json!({"step_type": "agent_response"})),
            "Razona"
        );
    }
}
