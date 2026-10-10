//! Cliente mínimo de la API de Claude (Messages) por HTTP.
//! Rust no tiene SDK oficial de Anthropic, así que se habla con `POST /v1/messages` directamente.

use futures_util::StreamExt;
use serde_json::{Value, json};
use tauri::AppHandle;

use crate::vault::read_config;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
pub const MODEL: &str = "claude-opus-5-5";
/// Si los clasificadores de seguridad rechazan la petición, la API la repite
/// en el modelo que Anthropic recomienda para esa categoría.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// De dónde sale la clave: la variable de entorno manda sobre la guardada en la app.
pub fn key_source(app: &AppHandle) -> Option<&'static str> {
    if std::env::var("ANTHROPIC_API_KEY").is_ok_and(|k| !k.trim().is_empty()) {
        Some("env")
    } else if read_config(app)
        .api_key
        .is_some_and(|k| !k.trim().is_empty())
    {
        Some("app")
    } else {
        None
    }
}

pub struct Claude {
    http: reqwest::Client,
    key: String,
}

impl Claude {
    pub fn new(app: &AppHandle) -> Result<Self, String> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
            .or_else(|| read_config(app).api_key)
            .filter(|k| !k.trim().is_empty())
            .ok_or("Falta la clave de la API de Anthropic. Escribe: clave sk-ant-…")?;
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { http, key })
    }

    fn request(&self, mut body: Value) -> reqwest::RequestBuilder {
        body["model"] = json!(MODEL);
        body["fallbacks"] = json!("default");
        self.http
            .post(API_URL)
            .header("x-api-key", &self.key)
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", FALLBACK_BETA)
            .json(&body)
    }

    async fn check(response: reqwest::Response) -> Result<reqwest::Response, String> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let detail = response
            .json::<Value>()
            .await
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
            .unwrap_or_default();
        Err(match status.as_u16() {
            401 => "La clave de API no es válida (401). Revísala con: clave sk-ant-…".into(),
            403 => format!("Sin permiso para esta petición (403). {detail}"),
            429 => "Límite de uso alcanzado (429). Espera un momento y vuelve a intentarlo.".into(),
            529 => "La API está saturada (529). Inténtalo de nuevo en unos segundos.".into(),
            code => format!("Error de la API ({code}): {detail}"),
        })
    }

    /// Comprueba la clave consultando el modelo en la Models API (no consume tokens).
    pub async fn verify(&self) -> Result<String, String> {
        let response = self
            .http
            .get(format!("https://api.anthropic.com/v1/models/{MODEL}"))
            .header("x-api-key", &self.key)
            .header("anthropic-version", API_VERSION)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| format!("Sin conexión con la API: {e}"))?;
        let model: Value = Self::check(response)
            .await?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        Ok(model["display_name"].as_str().unwrap_or(MODEL).to_string())
    }

    /// Petición sin streaming; devuelve el mensaje completo.
    pub async fn create(&self, body: Value) -> Result<Value, String> {
        let response = self
            .request(body)
            .send()
            .await
            .map_err(|e| format!("Sin conexión con la API: {e}"))?;
        Self::check(response)
            .await?
            .json()
            .await
            .map_err(|e| e.to_string())
    }

    /// Petición con streaming: llama a `on_text` con cada fragmento de texto y
    /// devuelve el mensaje reconstruido (bloques de pensamiento incluidos, sin tocar).
    pub async fn stream(
        &self,
        mut body: Value,
        mut on_text: impl FnMut(&str),
    ) -> Result<Value, String> {
        body["stream"] = json!(true);
        let response = self
            .request(body)
            .send()
            .await
            .map_err(|e| format!("Sin conexión con la API: {e}"))?;
        let mut bytes = Self::check(response).await?.bytes_stream();

        let mut message = json!({});
        let mut blocks: Vec<Value> = Vec::new();
        let mut partial_json: Vec<String> = Vec::new();
        let mut buffer = String::new();

        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.map_err(|e| format!("Se cortó la conexión: {e}"))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(end) = buffer.find("\n\n") {
                let event: String = buffer.drain(..end + 2).collect();
                let Some(data) = event.lines().find_map(|l| l.strip_prefix("data: ")) else {
                    continue;
                };
                let Ok(data) = serde_json::from_str::<Value>(data) else {
                    continue;
                };
                match data["type"].as_str().unwrap_or("") {
                    "message_start" => message = data["message"].clone(),
                    "content_block_start" => {
                        blocks.push(data["content_block"].clone());
                        partial_json.push(String::new());
                    }
                    "content_block_delta" => {
                        let i = data["index"].as_u64().unwrap_or(0) as usize;
                        let (Some(block), delta) = (blocks.get_mut(i), &data["delta"]) else {
                            continue;
                        };
                        match delta["type"].as_str().unwrap_or("") {
                            "text_delta" => {
                                let text = delta["text"].as_str().unwrap_or("");
                                append(block, "text", text);
                                on_text(text);
                            }
                            "thinking_delta" => {
                                append(block, "thinking", delta["thinking"].as_str().unwrap_or(""))
                            }
                            "signature_delta" => block["signature"] = delta["signature"].clone(),
                            "input_json_delta" => partial_json[i]
                                .push_str(delta["partial_json"].as_str().unwrap_or("")),
                            _ => {}
                        }
                    }
                    "content_block_stop" => {
                        let i = data["index"].as_u64().unwrap_or(0) as usize;
                        if let (Some(block), Some(raw)) = (blocks.get_mut(i), partial_json.get(i)) {
                            if !raw.is_empty() {
                                block["input"] = serde_json::from_str(raw).unwrap_or(json!({}));
                            }
                        }
                    }
                    "message_delta" => {
                        for key in ["stop_reason", "stop_details"] {
                            if !data["delta"][key].is_null() {
                                message[key] = data["delta"][key].clone();
                            }
                        }
                    }
                    "error" => {
                        return Err(format!(
                            "Error durante la respuesta: {}",
                            data["error"]["message"].as_str().unwrap_or("desconocido")
                        ));
                    }
                    _ => {}
                }
            }
        }
        message["content"] = Value::Array(blocks);
        Ok(message)
    }
}

fn append(block: &mut Value, field: &str, text: &str) {
    let current = block[field].as_str().unwrap_or("").to_string();
    block[field] = json!(current + text);
}

/// Texto visible de una respuesta (solo bloques `text`).
pub fn text_of(message: &Value) -> String {
    message["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

/// Mensaje de error si la respuesta terminó en rechazo; hay que comprobarlo antes de usar el contenido.
pub fn refusal(message: &Value) -> Option<String> {
    (message["stop_reason"] == "refusal").then(|| {
        let category = message["stop_details"]["category"]
            .as_str()
            .unwrap_or("sin categoría");
        format!("Claude rechazó esta petición ({category}). Prueba a reformularla.")
    })
}
