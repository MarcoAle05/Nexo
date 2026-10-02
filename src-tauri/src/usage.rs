//! Cuánto queda de los límites de uso de las suscripciones: Claude Code (plan de claude.ai)
//! y Antigravity (agy). Ninguna consulta gasta cuota.
//!
//! - Claude: el mismo endpoint que usa `/usage` de Claude Code (`/api/oauth/usage`), con el
//!   token OAuth que Claude Code guarda en `~/.claude/.credentials.json`. Solo se lee: si
//!   caducó, se pide abrir Claude Code para que lo renueve (renovarlo aquí podría invalidar
//!   el de Claude Code).
//! - Antigravity: agy no tiene salida para máquinas, así que se abre en un PTY, se escribe
//!   `/usage` y se lee la pantalla «Models & Quota».

use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager, State};

use crate::agy;

#[derive(Serialize, Clone)]
pub struct Limit {
    label: String,
    /// Porcentaje usado (0–100).
    used: f64,
    /// Cuándo se renueva (RFC 3339); `None` si no hay consumo pendiente de renovar.
    resets_at: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct Group {
    name: String,
    /// Modelos que comparten estos límites.
    models: Option<String>,
    limits: Vec<Limit>,
}

#[derive(Serialize, Clone)]
pub struct Usage {
    groups: Vec<Group>,
    checked_at: String,
}

/// Última lectura de cada servicio, para no repetir la consulta en cada apertura del menú.
#[derive(Default)]
pub struct UsageState {
    claude: tokio::sync::Mutex<Option<(Instant, Usage)>>,
    agy: tokio::sync::Mutex<Option<(Instant, Usage)>>,
}

const CLAUDE_TTL: Duration = Duration::from_secs(60);
const AGY_TTL: Duration = Duration::from_secs(120);

fn now() -> String {
    chrono::Local::now().to_rfc3339()
}

// ---------------------------------------------------------------- Claude

fn claude_label(kind: &str) -> String {
    match kind {
        "session" => "Sesión · 5 h".into(),
        "weekly_all" => "Semanal".into(),
        "weekly_opus" => "Semanal · Opus".into(),
        "weekly_sonnet" => "Semanal · Sonnet".into(),
        other => other.replace('_', " "),
    }
}

fn parse_claude(data: &Value, plan: &str) -> Usage {
    let mut limits: Vec<Limit> = data["limits"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|l| {
                    Some(Limit {
                        label: claude_label(l["kind"].as_str()?),
                        used: l["percent"].as_f64()?,
                        resets_at: l["resets_at"].as_str().map(String::from),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    // Formato anterior, por si `limits` no viene.
    if limits.is_empty() {
        for (key, kind) in [("five_hour", "session"), ("seven_day", "weekly_all")] {
            if let Some(used) = data[key]["utilization"].as_f64() {
                limits.push(Limit {
                    label: claude_label(kind),
                    used,
                    resets_at: data[key]["resets_at"].as_str().map(String::from),
                });
            }
        }
    }
    Usage {
        groups: vec![Group {
            name: format!("Plan {plan}"),
            models: None,
            limits,
        }],
        checked_at: now(),
    }
}

async fn fetch_claude(app: &AppHandle) -> Result<Usage, String> {
    let path = app
        .path()
        .home_dir()
        .map_err(|e| e.to_string())?
        .join(".claude/.credentials.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|_| "Claude Code no tiene sesión iniciada con una cuenta de claude.ai.")?;
    let creds: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let oauth = &creds["claudeAiOauth"];
    let token = oauth["accessToken"]
        .as_str()
        .ok_or("Claude Code no tiene sesión iniciada con una cuenta de claude.ai.")?;
    let expired = oauth["expiresAt"]
        .as_i64()
        .is_some_and(|ms| ms <= chrono::Utc::now().timestamp_millis());
    if expired {
        return Err(
            "La sesión de Claude Code caducó: abre la pestaña Claude Code para renovarla.".into(),
        );
    }
    let plan = oauth["subscriptionType"].as_str().unwrap_or("claude.ai");
    let response = reqwest::Client::new()
        .get("https://api.anthropic.com/api/oauth/usage")
        .bearer_auth(token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "nexo")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("No se pudo consultar el uso de Claude: {e}"))?;
    match response.status().as_u16() {
        200 => {}
        401 | 403 => {
            return Err(
                "La sesión de Claude Code caducó: abre la pestaña Claude Code para renovarla."
                    .into(),
            );
        }
        code => return Err(format!("Claude respondió {code} al pedir el uso")),
    }
    let data: Value = response.json().await.map_err(|e| e.to_string())?;
    Ok(parse_claude(&data, &capitalize(plan)))
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn usage_claude(
    app: AppHandle,
    state: State<'_, UsageState>,
    force: bool,
) -> Result<Usage, String> {
    let mut cache = state.claude.lock().await;
    if let Some((at, usage)) = cache.as_ref()
        && !force
        && at.elapsed() < CLAUDE_TTL
    {
        return Ok(usage.clone());
    }
    let usage = fetch_claude(&app).await?;
    *cache = Some((Instant::now(), usage.clone()));
    Ok(usage)
}

// ---------------------------------------------------------------- Antigravity

/// Quita las secuencias de escape de la terminal (colores, cursor, títulos, gráficos).
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('\x40'..='\x7e').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') | Some('_') | Some('P') => {
                    while let Some(c) = chars.next() {
                        if c == '\x07' || (c == '\x1b' && chars.next_if_eq(&'\\').is_some()) {
                            break;
                        }
                    }
                }
                Some('(') | Some(')') => {
                    chars.next();
                }
                _ => {}
            },
            '\r' => out.push('\n'),
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// «121h 33m», «4h 55m», «2d 3h», «45m» → minutos.
fn parse_duration(text: &str) -> Option<i64> {
    let mut minutes = 0;
    let mut found = false;
    for part in text.split_whitespace() {
        let (num, unit) = part.split_at(part.find(|c: char| !c.is_ascii_digit())?);
        let n: i64 = num.parse().ok()?;
        minutes += match unit {
            "d" => n * 1440,
            "h" => n * 60,
            "m" => n,
            "s" => 0,
            _ => return None,
        };
        found = true;
    }
    found.then_some(minutes)
}

fn agy_label(line: &str) -> String {
    let name = line.trim().trim_end_matches("Remaining").trim();
    match name {
        "Weekly Limit" => "Semanal".into(),
        "Five Hour Limit" => "5 horas".into(),
        other => other.to_string(),
    }
}

fn group_name(header: &str) -> String {
    match header {
        "GEMINI MODELS" => "Gemini".into(),
        "CLAUDE AND GPT MODELS" => "Claude y GPT".into(),
        other => capitalize(&other.trim_end_matches(" MODELS").to_lowercase()),
    }
}

/// Lee la pantalla «Models & Quota» de `/usage`.
fn parse_agy(screen: &str, now: chrono::DateTime<chrono::Local>) -> Vec<Group> {
    let start = screen.rfind("Models & Quota").unwrap_or(0);
    let mut groups: Vec<Group> = Vec::new();
    let mut pending: Option<String> = None;
    for line in screen[start..].lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if line.ends_with("MODELS") && line.chars().all(|c| !c.is_lowercase()) {
            groups.push(Group {
                name: group_name(line),
                models: None,
                limits: Vec::new(),
            });
        } else if let Some(models) = line.strip_prefix("Models within this group:") {
            if let Some(g) = groups.last_mut() {
                g.models = Some(models.trim().to_string());
            }
        } else if line.ends_with("Remaining") {
            pending = Some(agy_label(line));
        } else if let (Some(label), Some(pct)) = (
            pending.as_ref(),
            line.rsplit_once(']')
                .and_then(|(_, p)| p.trim().trim_end_matches('%').trim().parse::<f64>().ok()),
        ) {
            if let Some(g) = groups.last_mut() {
                g.limits.push(Limit {
                    label: label.clone(),
                    used: ((100.0 - pct) * 100.0).round() / 100.0,
                    resets_at: None,
                });
            }
            pending = None;
        } else if let Some(rest) = line.strip_prefix("Refreshes in") {
            if let (Some(minutes), Some(limit)) = (
                parse_duration(rest),
                groups.last_mut().and_then(|g| g.limits.last_mut()),
            ) {
                limit.resets_at = Some((now + chrono::Duration::minutes(minutes)).to_rfc3339());
            }
        }
    }
    groups.retain(|g| !g.limits.is_empty());
    groups
}

/// Abre agy en un PTY dentro de `dir`, acepta la confianza en la carpeta (vacía, propia
/// de nexo), escribe `/usage` y devuelve la pantalla. Bloqueante.
fn agy_screen(agy: &Path, dir: &Path) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let pty = native_pty_system()
        .openpty(PtySize {
            rows: 60,
            cols: 140,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;
    let mut cmd = CommandBuilder::new(agy);
    cmd.cwd(dir);
    cmd.env("TERM", "xterm-256color");
    let mut child = pty
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("No se pudo abrir agy: {e}"))?;
    drop(pty.slave);
    let mut reader = pty.master.try_clone_reader().map_err(|e| e.to_string())?;
    let mut writer = pty.master.take_writer().map_err(|e| e.to_string())?;

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 16384];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    let deadline = Instant::now() + Duration::from_secs(40);
    let mut raw = Vec::new();
    let mut trusted = false;
    let mut asked: Option<usize> = None;
    let result = loop {
        if Instant::now() > deadline {
            let screen = strip_ansi(&String::from_utf8_lossy(&raw));
            break Err(if screen.contains("not signed in") {
                "agy no tiene sesión iniciada: ábrelo en una terminal y entra con tu cuenta.".into()
            } else {
                "agy no mostró su cuota a tiempo.".into()
            });
        }
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(200)) {
            raw.extend_from_slice(&chunk);
        }
        let screen = strip_ansi(&String::from_utf8_lossy(&raw));
        if !trusted && screen.contains("Do you trust") {
            trusted = true;
            let _ = writer.write_all(b"\r");
            let _ = writer.flush();
            continue;
        }
        match asked {
            None if screen.contains("for shortcuts") => {
                std::thread::sleep(Duration::from_millis(300));
                let _ = writer.write_all(b"/usage");
                let _ = writer.flush();
                std::thread::sleep(Duration::from_millis(800));
                let _ = writer.write_all(b"\r");
                let _ = writer.flush();
                asked = Some(screen.len());
            }
            Some(mark) => {
                let after = screen.get(mark..).unwrap_or(&screen);
                if after.contains("Models & Quota") && after.contains("Within each group") {
                    break Ok(after.to_string());
                }
            }
            None => {}
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    result
}

#[tauri::command]
pub async fn usage_agy(
    app: AppHandle,
    state: State<'_, UsageState>,
    force: bool,
) -> Result<Usage, String> {
    let mut cache = state.agy.lock().await;
    if let Some((at, usage)) = cache.as_ref()
        && !force
        && at.elapsed() < AGY_TTL
    {
        return Ok(usage.clone());
    }
    let agy = agy::find_agy(&app).ok_or("agy no está instalado.")?;
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("agy-uso");
    let screen = tauri::async_runtime::spawn_blocking(move || agy_screen(&agy, &dir))
        .await
        .map_err(|e| e.to_string())??;
    let groups = parse_agy(&screen, chrono::Local::now());
    if groups.is_empty() {
        return Err("No se pudo leer la cuota de agy.".into());
    }
    let usage = Usage {
        groups,
        checked_at: now(),
    };
    *cache = Some((Instant::now(), usage.clone()));
    Ok(usage)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: &str = "└ Models & Quota\n\n  Account: x@y.com\n\nGEMINI MODELS\n  Models within this group: Gemini Flash, Gemini Pro\n\n  Weekly Limit Remaining\n    [██████░░] 94.68%\n    Refreshes in 121h 33m\n\n  Five Hour Limit Remaining\n    [███████░] 98.67%\n    Refreshes in 4h 55m\n\n\nCLAUDE AND GPT MODELS\n  Models within this group: Claude Opus, Claude Sonnet, GPT-OSS\n\n  Weekly Limit Remaining\n    [████████] 100.00%\n    Quota available\n\n  Five Hour Limit Remaining\n    [████████] 100.00%\n    Quota available\n\n  │Within each group, models share a weekly limit";

    #[test]
    fn lee_la_cuota_de_agy() {
        let now = chrono::Local::now();
        let groups = parse_agy(SCREEN, now);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].name, "Gemini");
        assert_eq!(
            groups[0].models.as_deref(),
            Some("Gemini Flash, Gemini Pro")
        );
        assert_eq!(groups[0].limits[0].label, "Semanal");
        assert!((groups[0].limits[0].used - 5.32).abs() < 0.001);
        let reset =
            chrono::DateTime::parse_from_rfc3339(groups[0].limits[1].resets_at.as_ref().unwrap())
                .unwrap();
        assert_eq!((reset.timestamp() - now.timestamp()) / 60, 4 * 60 + 55);
        assert_eq!(groups[1].name, "Claude y GPT");
        assert_eq!(groups[1].limits[1].used, 0.0);
        assert!(groups[1].limits[1].resets_at.is_none());
    }

    #[test]
    fn quita_escapes_de_terminal() {
        assert_eq!(
            strip_ansi("\x1b[1mHola\x1b[0m\r\x1b]0;título\x07 mundo"),
            "Hola\n mundo"
        );
    }

    #[test]
    fn lee_el_uso_de_claude() {
        let data: Value = serde_json::from_str(r#"{"limits":[{"kind":"session","percent":19,"resets_at":"2026-10-02T06:40:00+00:00"},{"kind":"weekly_all","percent":28,"resets_at":"2026-10-07T00:00:00+00:00"}]}"#).unwrap();
        let usage = parse_claude(&data, "Pro");
        assert_eq!(usage.groups[0].name, "Plan Pro");
        assert_eq!(usage.groups[0].limits[0].label, "Sesión · 5 h");
        assert_eq!(usage.groups[0].limits[1].used, 28.0);
    }

    /// Abre agy de verdad: `cargo test -- --ignored cuota_agy_real --nocapture`.
    #[test]
    #[ignore]
    fn cuota_agy_real() {
        let dir = std::env::temp_dir().join("nexo-agy-uso");
        let screen = agy_screen(Path::new("/usr/bin/agy"), &dir).unwrap();
        let groups = parse_agy(&screen, chrono::Local::now());
        println!("{}", serde_json::to_string_pretty(&groups).unwrap());
        assert!(!groups.is_empty());
    }
}
