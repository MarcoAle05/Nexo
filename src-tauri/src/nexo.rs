//! Nexo, el asistente del cuaderno (Claude):
//! - `compile`: integra fuentes de `raw/` en la wiki (bucle de herramientas sobre `wiki/`).
//! - `ask`: responde una pregunta con la wiki como contexto y la guarda en `output/`.
//! - chat: conversación continua con la wiki como contexto.

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use base64::Engine;
use serde::Serialize;
use serde_json::{Value, json};
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tokio::sync::Mutex;

use crate::claude::{self, Claude, refusal, text_of};
use crate::markitdown;
use crate::vault::{read_config, resolve_inside, vault_root, write_config};

/// Presupuesto de caracteres de la wiki que se envía como contexto (~350 mil tokens).
const WIKI_BUDGET: usize = 1_400_000;
const MAX_PDF_BYTES: u64 = 30 * 1024 * 1024;
const MAX_COMPILE_TURNS: usize = 40;

const PERSONA: &str = "Eres Nexo, el asistente de investigación de la app nexo. \
Trabajas sobre la wiki personal del usuario: un vault de Obsidian donde `raw/` guarda las fuentes originales, \
`wiki/` las notas organizadas y `output/` las respuestas. \
Responde en el idioma del usuario (español por defecto), de forma directa y sin relleno. \
Basa tus respuestas en la wiki y cita las notas que uses como [[nombre-de-la-nota]]. \
Si la wiki no cubre algo, dilo con claridad y separa lo que viene de la wiki de lo que aportas de tu conocimiento general. \
Tus respuestas se muestran como texto plano en una terminal: usa párrafos y listas simples con guiones, sin tablas ni encabezados.";

const COMPILER: &str = "Eres Nexo y mantienes la wiki del usuario, un vault de Obsidian. \
Tu tarea: integrar una fuente nueva de `raw/` en `wiki/` usando las herramientas.\n\n\
Estructura de la wiki:\n\
- `_master-index.md` en la raíz enlaza el `_index.md` de cada tema: [[tema/_index|Tema]].\n\
- Cada tema es una carpeta en kebab-case (p. ej. `rag-systems/`) con un `_index.md` que resume el tema y enlaza todas sus notas.\n\
- Las notas son atómicas (una idea por nota), con nombre en kebab-case y extensión `.md`.\n\
- Enlaza las notas relacionadas con [[nombre-de-nota]], también entre temas: los enlaces forman el grafo.\n\
- Cierra cada nota con una sección `## Fuentes` que nombre la fuente como texto: `raw/…` (sin [[ ]]).\n\n\
Cómo trabajar:\n\
1. Mira la lista de la wiki y lee las notas e índices relacionados antes de escribir.\n\
2. Reutiliza temas y notas existentes: amplía o corrige en lugar de duplicar. Al reescribir una nota conserva lo que ya tenía.\n\
3. Crea temas nuevos solo cuando la fuente no encaje en ninguno.\n\
4. Actualiza el `_index.md` de cada tema tocado y `_master-index.md` si hay temas nuevos.\n\
5. Escribe en español, salvo nombres propios y términos técnicos habituales en inglés.\n\
6. Al terminar, responde con un resumen de una o dos líneas de lo que cambiaste.";

#[derive(Clone, Serialize)]
pub struct Event {
    /// `text` (fragmento de respuesta), `step` (progreso) o `info`.
    kind: &'static str,
    text: String,
}

fn emit(channel: &Channel<Event>, kind: &'static str, text: impl Into<String>) {
    let _ = channel.send(Event {
        kind,
        text: text.into(),
    });
}

// ---------------------------------------------------------------- clave de API

#[derive(Serialize)]
pub struct KeyStatus {
    configured: bool,
    source: Option<&'static str>,
    model: &'static str,
}

#[tauri::command]
pub fn key_status(app: AppHandle) -> KeyStatus {
    let source = claude::key_source(&app);
    KeyStatus {
        configured: source.is_some(),
        source,
        model: claude::MODEL,
    }
}

#[tauri::command]
pub fn set_api_key(app: AppHandle, key: String) -> Result<(), String> {
    let key = key.trim().to_string();
    if !key.starts_with("sk-ant-") {
        return Err("Eso no parece una clave de Anthropic (empiezan por sk-ant-).".into());
    }
    let mut config = read_config(&app);
    config.api_key = Some(key);
    write_config(&app, &config)
}

// ---------------------------------------------------------------- contexto de la wiki

fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_md(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            out.push(path);
        }
    }
}

fn rel(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// La wiki como texto para el prompt. Los índices entran primero; si no cabe todo,
/// se avisa de qué notas quedaron fuera en lugar de recortarlas en silencio.
fn wiki_context(root: &Path) -> (String, usize, Vec<String>) {
    let wiki = root.join("wiki");
    let mut files = Vec::new();
    collect_md(&wiki, &mut files);
    files.sort_by_key(|p| {
        !p.file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('_'))
    });

    let mut out = String::new();
    let mut included = 0;
    let mut omitted = Vec::new();
    for file in files {
        let id = rel(&wiki, &file);
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let entry = format!("<note path=\"wiki/{id}\">\n{text}\n</note>\n");
        if out.len() + entry.len() > WIKI_BUDGET {
            omitted.push(id);
        } else {
            out.push_str(&entry);
            included += 1;
        }
    }
    if !omitted.is_empty() {
        out.push_str(&format!(
            "\n<omitted>Por tamaño no se incluyeron {} notas: {}</omitted>\n",
            omitted.len(),
            omitted.join(", ")
        ));
    }
    (out, included, omitted)
}

fn system_with_wiki(root: &Path, channel: &Channel<Event>) -> Value {
    let (wiki, included, omitted) = wiki_context(root);
    emit(
        channel,
        "info",
        format!("contexto: {included} notas de wiki/"),
    );
    if !omitted.is_empty() {
        emit(
            channel,
            "info",
            format!(
                "{} notas no caben en el contexto y quedaron fuera",
                omitted.len()
            ),
        );
    }
    let wiki = if wiki.is_empty() {
        "<wiki vacía: aún no hay notas>".to_string()
    } else {
        wiki
    };
    json!([
        {"type": "text", "text": PERSONA},
        {"type": "text", "text": format!("<wiki>\n{wiki}</wiki>"), "cache_control": {"type": "ephemeral"}}
    ])
}

// ---------------------------------------------------------------- chat

#[derive(Default)]
pub struct ChatState(Mutex<Option<Chat>>);

struct Chat {
    /// Se fija al empezar la conversación para no alterar el historial ya enviado.
    system: Value,
    messages: Vec<Value>,
}

#[tauri::command]
pub async fn chat_reset(state: State<'_, ChatState>) -> Result<(), String> {
    *state.0.lock().await = None;
    Ok(())
}

#[tauri::command]
pub async fn chat_send(
    app: AppHandle,
    state: State<'_, ChatState>,
    message: String,
    channel: Channel<Event>,
) -> Result<(), String> {
    let claude = Claude::new(&app)?;
    let root = vault_root(&app)?;
    let mut guard = state.0.lock().await;
    let chat = guard.get_or_insert_with(|| Chat {
        system: system_with_wiki(&root, &channel),
        messages: Vec::new(),
    });

    chat.messages
        .push(json!({"role": "user", "content": message}));
    let body = json!({
        "max_tokens": 64000,
        "output_config": {"effort": "medium"},
        "system": chat.system,
        "messages": chat.messages,
    });
    let result = claude
        .stream(body, |text| emit(&channel, "text", text))
        .await;
    let response = match result {
        Ok(response) => response,
        Err(error) => {
            chat.messages.pop(); // la pregunta no obtuvo respuesta: se puede reintentar tal cual
            return Err(error);
        }
    };
    if let Some(error) = refusal(&response) {
        chat.messages.pop();
        return Err(error);
    }
    // El contenido se guarda completo (pensamiento incluido) para continuar la conversación.
    chat.messages
        .push(json!({"role": "assistant", "content": response["content"]}));
    Ok(())
}

// ---------------------------------------------------------------- ask

#[tauri::command]
pub async fn ask(
    app: AppHandle,
    question: String,
    channel: Channel<Event>,
) -> Result<String, String> {
    let claude = Claude::new(&app)?;
    let root = vault_root(&app)?;
    let body = json!({
        "max_tokens": 64000,
        "output_config": {"effort": "high"},
        "system": system_with_wiki(&root, &channel),
        "messages": [{"role": "user", "content": question}],
    });
    let response = claude
        .stream(body, |text| emit(&channel, "text", text))
        .await?;
    if let Some(error) = refusal(&response) {
        return Err(error);
    }
    let answer = text_of(&response);

    let file = root.join("output/query-results.md");
    fs::create_dir_all(root.join("output")).map_err(|e| e.to_string())?;
    let mut doc =
        fs::read_to_string(&file).unwrap_or_else(|_| "# Resultados de consultas\n".into());
    let date = chrono::Local::now().format("%Y-%m-%d %H:%M");
    doc.push_str(&format!(
        "\n---\n\n## {}\n\n_{date} · Nexo ({})_\n\n{}\n",
        question.trim(),
        claude::MODEL,
        answer.trim()
    ));
    fs::write(&file, doc).map_err(|e| e.to_string())?;
    Ok("output/query-results.md".into())
}

// ---------------------------------------------------------------- compile

type Manifest = HashMap<String, String>;

fn manifest_path(root: &Path) -> PathBuf {
    root.join(".nexo/compiled.json")
}

fn fingerprint(path: &Path) -> String {
    let meta = fs::metadata(path).ok();
    let modified = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}:{modified}", meta.map(|m| m.len()).unwrap_or(0))
}

/// Ruta segura dentro de `wiki/` para las herramientas: relativa, sin `..` ni carpetas ocultas.
fn wiki_path(wiki: &Path, path: &str, must_be_md: bool) -> Result<PathBuf, String> {
    let clean = path.trim().trim_start_matches("wiki/");
    let p = Path::new(clean);
    let ok = !clean.is_empty()
        && p.components()
            .all(|c| matches!(c, Component::Normal(s) if !s.to_string_lossy().starts_with('.')));
    if !ok {
        return Err(format!("Ruta no válida: {path}"));
    }
    if must_be_md && !clean.to_lowercase().ends_with(".md") {
        return Err("Solo se pueden escribir notas .md".into());
    }
    Ok(wiki.join(p))
}

fn wiki_listing(wiki: &Path) -> String {
    let mut files = Vec::new();
    collect_md(wiki, &mut files);
    if files.is_empty() {
        return "(la wiki está vacía)".into();
    }
    files
        .iter()
        .map(|f| rel(wiki, f))
        .collect::<Vec<_>>()
        .join("\n")
}

fn tools() -> Value {
    json!([
        {
            "name": "list_wiki",
            "description": "Lista todas las notas .md de wiki/ (rutas relativas a wiki/).",
            "strict": true,
            "input_schema": {"type": "object", "properties": {}, "required": [], "additionalProperties": false}
        },
        {
            "name": "read_note",
            "description": "Lee el contenido completo de una nota de wiki/. Úsala antes de modificar una nota existente.",
            "strict": true,
            "input_schema": {
                "type": "object",
                "properties": {"path": {"type": "string", "description": "Ruta relativa a wiki/, p. ej. rag-systems/_index.md"}},
                "required": ["path"],
                "additionalProperties": false
            }
        },
        {
            "name": "write_note",
            "description": "Crea o reemplaza por completo una nota .md de wiki/. Crea las carpetas que falten.",
            "strict": true,
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Ruta relativa a wiki/, terminada en .md"},
                    "content": {"type": "string", "description": "Contenido Markdown completo de la nota"}
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }
        }
    ])
}

fn run_tool(
    wiki: &Path,
    name: &str,
    input: &Value,
    channel: &Channel<Event>,
) -> Result<String, String> {
    let path = input["path"].as_str().unwrap_or("");
    match name {
        "list_wiki" => Ok(wiki_listing(wiki)),
        "read_note" => {
            let file = wiki_path(wiki, path, false)?;
            emit(channel, "step", format!("lee {path}"));
            fs::read_to_string(&file).map_err(|_| format!("No existe {path}"))
        }
        "write_note" => {
            let file = wiki_path(wiki, path, true)?;
            let content = input["content"].as_str().ok_or("Falta content")?;
            if let Some(dir) = file.parent() {
                fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let verb = if file.exists() { "actualiza" } else { "crea" };
            fs::write(&file, content).map_err(|e| e.to_string())?;
            emit(channel, "step", format!("{verb} {path}"));
            Ok(format!("Guardada {path}"))
        }
        other => Err(format!("Herramienta desconocida: {other}")),
    }
}

/// Bloques de contenido para una fuente, o `None` si su tipo no se puede enviar.
fn source_blocks(file: &Path) -> Result<Option<Vec<Value>>, String> {
    let ext = file
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let b64 = |bytes: Vec<u8>| base64::engine::general_purpose::STANDARD.encode(bytes);
    let size = fs::metadata(file).map(|m| m.len()).unwrap_or(0);
    Ok(Some(match ext.as_str() {
        "pdf" => {
            if size > MAX_PDF_BYTES {
                return Err("PDF de más de 30 MB".into());
            }
            let data = fs::read(file).map_err(|e| e.to_string())?;
            vec![
                json!({"type": "document", "source": {"type": "base64", "media_type": "application/pdf", "data": b64(data)}}),
            ]
        }
        "png" | "jpg" | "jpeg" | "gif" | "webp" => {
            let media = match ext.as_str() {
                "jpg" | "jpeg" => "image/jpeg".to_string(),
                other => format!("image/{other}"),
            };
            let data = fs::read(file).map_err(|e| e.to_string())?;
            vec![
                json!({"type": "image", "source": {"type": "base64", "media_type": media, "data": b64(data)}}),
            ]
        }
        "mp3" | "m4a" | "wav" | "ogg" | "flac" | "mp4" | "mov" | "mkv" | "webm" => return Ok(None),
        _ => match fs::read_to_string(file) {
            Ok(text) if !text.trim().is_empty() => vec![json!({"type": "text", "text": text})],
            _ => return Ok(None),
        },
    }))
}

async fn compile_one(
    claude: &Claude,
    root: &Path,
    source: &str,
    blocks: Vec<Value>,
    channel: &Channel<Event>,
) -> Result<String, String> {
    let wiki = root.join("wiki");
    let mut content = blocks;
    content.push(json!({"type": "text", "text": format!(
        "Integra esta fuente en la wiki.\nFuente: raw/{source}\n\nNotas actuales de wiki/:\n{}",
        wiki_listing(&wiki)
    )}));
    let mut messages = vec![json!({"role": "user", "content": content})];

    for _ in 0..MAX_COMPILE_TURNS {
        let response = claude
            .create(json!({
                "max_tokens": 16000,
                "output_config": {"effort": "high"},
                "system": COMPILER,
                "tools": tools(),
                "messages": messages,
            }))
            .await?;
        if let Some(error) = refusal(&response) {
            return Err(error);
        }
        // Historial solo de anexar: el contenido vuelve tal cual, pensamiento incluido.
        messages.push(json!({"role": "assistant", "content": response["content"]}));

        match response["stop_reason"].as_str().unwrap_or("") {
            "tool_use" => {
                let results: Vec<Value> = response["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|b| b["type"] == "tool_use")
                    .map(|b| {
                        let name = b["name"].as_str().unwrap_or("");
                        match run_tool(&wiki, name, &b["input"], channel) {
                            Ok(out) => json!({"type": "tool_result", "tool_use_id": b["id"], "content": out}),
                            Err(err) => json!({"type": "tool_result", "tool_use_id": b["id"], "content": err, "is_error": true}),
                        }
                    })
                    .collect();
                messages.push(json!({"role": "user", "content": results}));
            }
            "max_tokens" => {
                return Err(
                    "La respuesta se cortó por longitud; la fuente puede haber quedado a medias."
                        .into(),
                );
            }
            _ => return Ok(text_of(&response).trim().to_string()),
        }
    }
    Err(format!(
        "Se alcanzó el límite de {MAX_COMPILE_TURNS} pasos sin terminar."
    ))
}

/// Integra en la wiki las fuentes indicadas (rutas relativas a `raw/`) que sean nuevas o hayan cambiado.
#[tauri::command]
pub async fn compile(
    app: AppHandle,
    sources: Vec<String>,
    channel: Channel<Event>,
) -> Result<usize, String> {
    let claude = Claude::new(&app)?;
    let root = vault_root(&app)?;
    let raw = root.join("raw");
    let manifest_file = manifest_path(&root);
    let mut manifest: Manifest = fs::read_to_string(&manifest_file)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let pending: Vec<(String, PathBuf)> = sources
        .into_iter()
        .filter_map(|s| resolve_inside(&raw, &s).ok().map(|p| (s, p)))
        .filter(|(s, p)| manifest.get(s) != Some(&fingerprint(p)))
        .collect();
    if pending.is_empty() {
        emit(
            &channel,
            "info",
            "no hay fuentes nuevas o modificadas en el contexto",
        );
        return Ok(0);
    }
    emit(
        &channel,
        "info",
        format!(
            "{} fuentes por compilar con {}",
            pending.len(),
            claude::MODEL
        ),
    );

    // Primero, conversión local a Markdown con MarkItDown (sin IA): Claude recibe texto
    // en lugar del PDF completo, lo que gasta mucho menos.
    let to_convert = markitdown::pending(&raw, pending.iter().map(|(s, _)| s.clone()));
    if !to_convert.is_empty() {
        emit(
            &channel,
            "info",
            format!(
                "convirtiendo {} a Markdown con MarkItDown",
                to_convert.len()
            ),
        );
        let (app2, raw2, ch) = (app.clone(), raw.clone(), channel.clone());
        let result = tauri::async_runtime::spawn_blocking(move || {
            markitdown::convert_blocking(&app2, &raw2, to_convert, |kind, text| {
                let kind = if kind == "error" { "step" } else { kind };
                emit(&ch, kind, text)
            })
        })
        .await
        .map_err(|e| e.to_string())?;
        if let Err(error) = result {
            emit(
                &channel,
                "step",
                format!("{error} · se enviará el archivo original"),
            );
        }
    }

    let mut done = 0;
    for (source, file) in pending {
        emit(&channel, "info", format!("raw/{source}"));
        let converted = markitdown::is_fresh(&raw, &source)
            .then(|| fs::read_to_string(markitdown::converted_path(&raw, &source)).ok())
            .flatten();
        let blocks = match converted {
            Some(markdown) => Ok(Some(vec![json!({"type": "text", "text": format!(
                "<document source=\"raw/{source}\" format=\"markdown (convertido con MarkItDown)\">\n{markdown}\n</document>"
            )})])),
            None => source_blocks(&file),
        };
        let blocks = match blocks {
            Ok(Some(blocks)) => blocks,
            Ok(None) => {
                emit(
                    &channel,
                    "step",
                    "omitida: tipo no compatible todavía (audio o vídeo sin transcribir)",
                );
                continue;
            }
            Err(error) => {
                emit(&channel, "step", format!("omitida: {error}"));
                continue;
            }
        };
        match compile_one(&claude, &root, &source, blocks, &channel).await {
            Ok(summary) => {
                if !summary.is_empty() {
                    emit(&channel, "step", summary);
                }
                manifest.insert(source, fingerprint(&file));
                if let Some(dir) = manifest_file.parent() {
                    let _ = fs::create_dir_all(dir);
                }
                let _ = fs::write(
                    &manifest_file,
                    serde_json::to_string_pretty(&manifest).unwrap_or_default(),
                );
                done += 1;
            }
            Err(error) => emit(&channel, "step", format!("error: {error}")),
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rutas_de_herramientas_seguras() {
        let wiki = Path::new("/v/wiki");
        assert!(wiki_path(wiki, "rag/_index.md", true).is_ok());
        assert!(wiki_path(wiki, "wiki/rag/nota.md", true).is_ok());
        assert!(wiki_path(wiki, "../raw/x.md", true).is_err());
        assert!(wiki_path(wiki, "/etc/passwd.md", true).is_err());
        assert!(wiki_path(wiki, ".obsidian/app.md", true).is_err());
        assert!(wiki_path(wiki, "rag/nota.txt", true).is_err());
    }
}
