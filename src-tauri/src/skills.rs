//! Skills del usuario: las de Claude Code del vault (`.claude/skills/<nombre>/SKILL.md`).
//! nexo las muestra como recuadros alrededor de la galaxia y en el apartado Agentes; las
//! escribe el agente `creador-de-skills` (`.claude/agents/creador-de-skills.md`), que nexo
//! instala en el vault y que Claude Code lanza con Sonnet 5.5 y esfuerzo medio.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::AppHandle;

use crate::vault::vault_root;

/// Nombre del agente que crea las skills (también es el `subagent_type` para Claude Code).
pub const AGENT_NAME: &str = "creador-de-skills";
/// Última línea del agente instalado: mientras siga ahí, nexo lo mantiene al día. Si el
/// usuario la borra, el archivo pasa a ser suyo y nexo no lo vuelve a tocar.
pub const AGENT_MARK: &str = "<!-- nexo: este agente lo mantiene nexo";

const AGENT: &str = r#"---
name: creador-de-skills
description: Crea y mejora skills y agentes de Claude Code para el usuario de nexo (en .claude/skills/ y .claude/agents/ del vault), con lo que sabe su grafo. Úsalo siempre que el usuario pida crear, cambiar o mejorar una skill o un agente, por ejemplo desde «Agregar skill» o «Agregar agente» en el apartado Agentes de nexo.
model: claude-sonnet-5-5
effort: medium
---

Eres el creador de skills y agentes de nexo, un NotebookLM personal. Escribes skills de Claude Code
(instrucciones que Claude Code sigue cuando el usuario las invoca con /<nombre> o le pide algo que encaja con
ellas) y agentes (subagentes con su propio modelo, esfuerzo y papel, a los que Claude Code encarga trabajo).
Trabajas en la raíz del vault de Obsidian del usuario y respondes siempre en español.

## Qué haces

1. Entiende la petición. Si falta un dato imprescindible que no puedes averiguar (por ejemplo, qué cuenta o qué
   clases usar), no lo inventes: termina tu respuesta con las preguntas concretas para que el orquestador se las
   haga al usuario y te vuelva a llamar con las respuestas.
2. Consulta el grafo. El grafo de nexo son las notas .md de wiki/ y sus [[enlaces]]: lee wiki/_master-index.md y
   busca con Grep/Glob en wiki/ lo relacionado con la skill (fichas de fuentes en wiki/fuentes/, notas del usuario
   en wiki/notas/, conversaciones guardadas en wiki/conversaciones/). Usa lo que encuentres para concretar la
   skill (nombres de cursos, personas, horarios, rutas, preferencias) y cita en la skill las notas útiles con su
   ruta para que Claude las lea al usarla.
3. Si la skill trabaja con una web, puedes abrirla con el navegador de Playwright (herramientas
   mcp__playwright__browser_*), que usa una copia de las sesiones del usuario, para conocer cómo se navega:
   direcciones, menús, nombres de botones y pestañas. Solo mira: no envíes, entregues, publiques, compres ni
   borres nada, y cierra el navegador (browser_close) al terminar.
4. Escribe la skill en .claude/skills/<nombre>/SKILL.md, con <nombre> corto, en minúsculas y kebab-case (por
   ejemplo classroom). Si ya existe una skill para lo mismo, mejórala en lugar de crear otra.

## Formato de SKILL.md

Frontmatter:
- name: el mismo <nombre> de la carpeta.
- description: una o dos frases en español que digan qué hace la skill, para una persona (nexo la muestra como
  resumen en la galaxia): sin detalles técnicos y en menos de 200 caracteres.
- when_to_use: cuándo usarla, con frases como las que diría el usuario.
No añadas allowed-tools: los permisos los decide el usuario.

Cuerpo: instrucciones en imperativo, breves y concretas, organizadas por tareas (por ejemplo «Revisar tareas
pendientes», «Hacer una tarea y subirla sin entregarla», «Leer los anuncios del tablón»), con las direcciones y la
forma de navegar que descubriste y qué devolver al usuario (un resumen corto y ordenado). Mantén SKILL.md por
debajo de unas 150 líneas; lo largo (referencias, plantillas, scripts) va en otros archivos de la misma carpeta,
enlazados desde SKILL.md.

## Agentes

Si te piden un agente, o una skill necesita uno propio para hacer el trabajo, créalo en
.claude/agents/<nombre>.md (si ya existe uno para lo mismo, mejóralo) con frontmatter:
- name: <nombre> en minúsculas y kebab-case.
- description: qué hace, en una o dos frases para una persona (nexo la muestra en Agentes como su resumen),
  y termina con «Úsalo cuando …» y las peticiones que lo activan (como las diría el usuario): así Claude Code
  se lo encarga solo, aunque el usuario no lo nombre.
- model: siempre explícito, el id completo que pida el usuario (claude-haiku-5-5, claude-sonnet-5-5 o
  claude-opus-5-5); si no dice nada, claude-sonnet-5-5.
- effort: siempre explícito, low, medium o high (el que pida el usuario; si no, medium).
- tools: solo si conviene limitar sus herramientas, separadas por comas.
Cuerpo: su papel, los pasos, qué devuelve y las reglas de seguridad de abajo. Una skill que delegue en él lo dice
(«encarga el trabajo al agente <nombre> y resume lo que devuelva»).

## Seguridad (escríbela también dentro de cada skill y agente que lo necesite)

- Pedir confirmación explícita al usuario antes de entregar, enviar, publicar, comprar, pagar, borrar o cambiar
  ajustes con sus cuentas. Si el usuario pide subir algo sin entregarlo, la skill sube el archivo y nunca pulsa
  «Entregar» (o equivalente).
- No pedir ni mostrar contraseñas, códigos ni cookies. Si la web pide iniciar sesión, decir al usuario que la
  inicie en su navegador y pulse «Cargar navegador» en Conexiones de nexo.
- Cerrar el navegador al terminar.

## Límites

No toques nada fuera de la skill (.claude/skills/<nombre>/) o el agente (.claude/agents/<nombre>.md) que te
pidan, ni la wiki ni otras skills o agentes, salvo que te lo pidan. Al terminar responde con el nombre de lo
que creaste, qué hace en una frase, cómo usarlo (/<nombre> o pidiéndolo con palabras; en un agente, su modelo y
esfuerzo) y las dudas que queden.

<!-- nexo: este agente lo mantiene nexo y lo pone al día al abrir Claude Code. Borra esta línea para cambiarlo a tu gusto sin que nexo lo sobrescriba. -->
"#;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct SkillInfo {
    /// Carpeta dentro de `.claude/skills/`.
    pub id: String,
    /// `name:` del frontmatter (o la carpeta).
    pub name: String,
    /// `description:`: el resumen que se enseña al usuario.
    pub description: String,
    /// `when_to_use:`, si lo tiene.
    pub when: Option<String>,
    /// Creación (o última modificación si el sistema no la guarda), en ms desde 1970.
    pub created: u64,
    /// Última modificación de SKILL.md, `AAAA-MM-DD HH:MM`.
    pub modified: Option<String>,
}

fn skills_dir(root: &Path) -> PathBuf {
    root.join(".claude").join("skills")
}

fn agent_path(root: &Path) -> PathBuf {
    root.join(".claude")
        .join("agents")
        .join(format!("{AGENT_NAME}.md"))
}

/// Campos de primer nivel del frontmatter YAML de una skill. Entiende lo que suele escribir
/// un modelo: valores simples, entre comillas, en bloque (`|` o `>`) y continuados en las
/// líneas sangradas siguientes. Las listas y los mapas anidados se ignoran.
pub fn frontmatter(text: &str) -> Vec<(String, String)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return Vec::new();
    };
    let lines: Vec<&str> = rest.lines().take_while(|l| l.trim_end() != "---").collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        if line.starts_with([' ', '\t', '#', '-']) {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = value.trim();
        // Líneas sangradas que siguen a la clave (bloque o continuación).
        let mut more = Vec::new();
        while i < lines.len() && (lines[i].trim().is_empty() || lines[i].starts_with([' ', '\t'])) {
            more.push(lines[i].trim());
            i += 1;
        }
        while more.last().is_some_and(|l| l.is_empty()) {
            more.pop();
        }
        let parsed = if let Some(style) = value.chars().next().filter(|c| *c == '|' || *c == '>') {
            if style == '|' {
                more.join("\n")
            } else {
                fold(&more)
            }
        } else if value.is_empty() {
            // Valor en las líneas siguientes; si es una lista o un mapa, no es un texto.
            if more
                .first()
                .is_some_and(|l| l.starts_with("- ") || l.contains(": "))
            {
                continue;
            }
            fold(&more)
        } else {
            let mut all = vec![value];
            all.extend(more.iter().copied());
            unquote(&fold(&all))
        };
        out.push((key.to_string(), parsed.trim().to_string()));
    }
    out
}

/// Pliega líneas como `>` en YAML: una línea vacía es un salto, el resto se une con espacios.
fn fold(lines: &[&str]) -> String {
    let mut out = String::new();
    for line in lines {
        if line.is_empty() {
            out.push('\n');
        } else {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push(' ');
            }
            out.push_str(line);
        }
    }
    out
}

fn unquote(value: &str) -> String {
    let v = value.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        let inner = &v[1..v.len() - 1];
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => {}
                }
            } else {
                out.push(c);
            }
        }
        out
    } else if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
        v[1..v.len() - 1].replace("''", "'")
    } else {
        v.to_string()
    }
}

fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Lee las skills del vault, de la más antigua a la más nueva (así una skill nueva no
/// mueve de sitio a las demás alrededor de la galaxia).
pub fn list(root: &Path) -> Vec<SkillInfo> {
    let Ok(entries) = fs::read_dir(skills_dir(root)) else {
        return Vec::new();
    };
    let mut skills: Vec<SkillInfo> = entries
        .flatten()
        .filter_map(|entry| {
            let id = entry.file_name().to_str()?.to_string();
            if id.starts_with('.') || !entry.file_type().ok()?.is_dir() {
                return None;
            }
            let file = entry.path().join("SKILL.md");
            let text = fs::read_to_string(&file).ok()?;
            let meta = frontmatter(&text);
            let get = |key: &str| {
                meta.iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.clone())
                    .filter(|v| !v.is_empty())
            };
            let info = fs::metadata(&file).ok();
            let modified = info.as_ref().and_then(|m| m.modified().ok());
            let created = info
                .as_ref()
                .and_then(|m| m.created().ok())
                .or(modified)
                .map(millis)
                .unwrap_or(0);
            Some(SkillInfo {
                name: get("name").unwrap_or_else(|| id.clone()),
                description: get("description").unwrap_or_default(),
                when: get("when_to_use"),
                created,
                modified: modified.map(|t| {
                    chrono::DateTime::<chrono::Local>::from(t)
                        .format("%Y-%m-%d %H:%M")
                        .to_string()
                }),
                id,
            })
        })
        .collect();
    skills.sort_by(|a, b| a.created.cmp(&b.created).then_with(|| a.id.cmp(&b.id)));
    skills
}

/// Huella de las skills para el vigilante: carpeta, tamaño y fecha de cada SKILL.md.
pub fn fingerprint(root: &Path) -> Vec<(String, u64, u64)> {
    let Ok(entries) = fs::read_dir(skills_dir(root)) else {
        return Vec::new();
    };
    let mut out: Vec<(String, u64, u64)> = entries
        .flatten()
        .filter_map(|entry| {
            let meta = fs::metadata(entry.path().join("SKILL.md")).ok()?;
            Some((
                entry.file_name().to_string_lossy().into_owned(),
                meta.len(),
                meta.modified().map(millis).unwrap_or(0),
            ))
        })
        .collect();
    out.sort();
    out
}

/// Instala o pone al día el agente creador de skills en el vault. Devuelve si escribió algo.
/// Si el usuario quitó la marca de nexo, el archivo es suyo y no se toca.
pub fn install_agent(root: &Path) -> std::io::Result<bool> {
    let path = agent_path(root);
    match fs::read_to_string(&path) {
        Ok(current) if current == AGENT || !current.contains(AGENT_MARK) => return Ok(false),
        _ => {}
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&path, AGENT)?;
    Ok(true)
}

#[tauri::command]
pub fn skills_list(app: AppHandle) -> Result<Vec<SkillInfo>, String> {
    Ok(list(&vault_root(&app)?))
}

/// Deja listo el agente antes de pedirle a Claude Code una skill.
#[tauri::command]
pub fn skills_prepare(app: AppHandle) -> Result<(), String> {
    install_agent(&vault_root(&app)?)
        .map(|_| ())
        .map_err(|e| format!("No se pudo instalar el agente {AGENT_NAME}: {e}"))
}

/// Carpeta de una skill a partir de su id, sin salir de `.claude/skills/`.
fn skill_folder(root: &Path, id: &str) -> Result<PathBuf, String> {
    if id.is_empty() || id.starts_with('.') || id.contains(['/', '\\']) {
        return Err("Nombre de skill no válido.".into());
    }
    let dir = skills_dir(root).join(id);
    if !dir.join("SKILL.md").is_file() {
        return Err(format!("No existe la skill {id}."));
    }
    Ok(dir)
}

/// Manda una skill (su carpeta entera) a la papelera.
#[tauri::command]
pub fn skills_remove(app: AppHandle, id: String) -> Result<(), String> {
    let dir = skill_folder(&vault_root(&app)?, &id)?;
    trash::delete(&dir).map_err(|e| format!("No se pudo mover a la papelera: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Carpeta temporal propia de cada prueba (sin dependencias extra). Se borra al salir.
    struct Temporal(PathBuf);

    impl Temporal {
        fn nueva(nombre: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "nexo-skills-{nombre}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        /// Escribe un archivo cualquiera (ruta relativa a la raíz del vault).
        fn escribe(&self, relativa: &str, texto: &str) {
            let ruta = self.0.join(relativa);
            fs::create_dir_all(ruta.parent().unwrap()).unwrap();
            fs::write(ruta, texto).unwrap();
        }

        /// Escribe `.claude/skills/<id>/SKILL.md`.
        fn skill(&self, id: &str, texto: &str) {
            self.escribe(&format!(".claude/skills/{id}/SKILL.md"), texto);
        }
    }

    impl Drop for Temporal {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Valor de una clave del frontmatter, si existe.
    fn valor<'a>(meta: &'a [(String, String)], clave: &str) -> Option<&'a str> {
        meta.iter()
            .find(|(k, _)| k == clave)
            .map(|(_, v)| v.as_str())
    }

    // ---------- frontmatter ----------

    #[test]
    fn frontmatter_valor_simple() {
        let meta =
            frontmatter("---\nname: classroom\ndescription: Tareas del curso\n---\nCuerpo\n");
        assert_eq!(valor(&meta, "name"), Some("classroom"));
        assert_eq!(valor(&meta, "description"), Some("Tareas del curso"));
    }

    #[test]
    fn frontmatter_comillas_dobles_con_escapes() {
        let meta = frontmatter(
            r#"---
description: "Dice \"hola\"\ny sigue"
---
"#,
        );
        assert_eq!(valor(&meta, "description"), Some("Dice \"hola\"\ny sigue"));
    }

    #[test]
    fn frontmatter_comillas_simples_con_doble_apostrofo() {
        let meta = frontmatter("---\ndescription: 'Es ''fácil'' de usar'\n---\n");
        assert_eq!(valor(&meta, "description"), Some("Es 'fácil' de usar"));
    }

    #[test]
    fn frontmatter_bloque_plegado() {
        // En `>` las líneas se unen con espacios y una línea vacía es un salto.
        let meta = frontmatter(
            "---\ndescription: >\n  Primera línea\n  segunda\n\n  párrafo\nname: x\n---\n",
        );
        assert_eq!(
            valor(&meta, "description"),
            Some("Primera línea segunda\npárrafo")
        );
        assert_eq!(valor(&meta, "name"), Some("x"));
    }

    #[test]
    fn frontmatter_bloque_plegado_con_chomping() {
        // `>-` se trata como `>`: el indicador de salto final no cambia el resultado tras recortar.
        let meta = frontmatter("---\ndescription: >-\n  uno\n  dos\n---\n");
        assert_eq!(valor(&meta, "description"), Some("uno dos"));
    }

    #[test]
    fn frontmatter_bloque_literal_con_saltos() {
        let meta = frontmatter("---\nwhen_to_use: |\n  uno\n  dos\n---\n");
        assert_eq!(valor(&meta, "when_to_use"), Some("uno\ndos"));
    }

    #[test]
    fn frontmatter_continuacion_en_lineas_sangradas() {
        let meta = frontmatter("---\ndescription: Primera parte\n  segunda parte\nname: x\n---\n");
        assert_eq!(
            valor(&meta, "description"),
            Some("Primera parte segunda parte")
        );
        assert_eq!(valor(&meta, "name"), Some("x"));
    }

    #[test]
    fn frontmatter_ignora_listas_y_mapas_anidados() {
        let meta = frontmatter(
            "---\nname: x\nallowed-tools:\n  - Bash\n  - Read\nmetadata:\n  autor: nexo\ndescription: ok\n---\n",
        );
        assert_eq!(valor(&meta, "allowed-tools"), None);
        assert_eq!(valor(&meta, "metadata"), None);
        assert_eq!(valor(&meta, "name"), Some("x"));
        assert_eq!(valor(&meta, "description"), Some("ok"));
    }

    #[test]
    fn frontmatter_acepta_bom_inicial() {
        let meta = frontmatter("\u{feff}---\nname: bom\n---\n");
        assert_eq!(valor(&meta, "name"), Some("bom"));
    }

    #[test]
    fn frontmatter_sin_frontmatter_esta_vacio() {
        assert!(frontmatter("# Solo texto\nname: x\n").is_empty());
        assert!(frontmatter("").is_empty());
    }

    #[test]
    fn frontmatter_acepta_crlf() {
        let meta =
            frontmatter("---\r\nname: crlf\r\ndescription: Con retornos\r\n---\r\nCuerpo\r\n");
        assert_eq!(valor(&meta, "name"), Some("crlf"));
        assert_eq!(valor(&meta, "description"), Some("Con retornos"));
    }

    // ---------- list ----------

    #[test]
    fn list_ignora_carpetas_ocultas_sin_skill_y_archivos_sueltos() {
        let t = Temporal::nueva("ignora");
        t.skill("alfa", "---\nname: alfa\ndescription: Primera\n---\n");
        t.escribe(".claude/skills/beta/notas.txt", "sin SKILL.md");
        t.skill(".oculta", "---\nname: oculta\n---\n");
        t.escribe(".claude/skills/suelto.md", "archivo suelto");
        let ids: Vec<String> = list(&t.0).into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["alfa"]);
    }

    #[test]
    fn list_sin_carpeta_de_skills_esta_vacia() {
        let t = Temporal::nueva("vacia");
        assert!(list(&t.0).is_empty());
    }

    #[test]
    fn list_nombre_cae_a_la_carpeta_si_falta() {
        let t = Temporal::nueva("sin-nombre");
        t.skill("gamma", "---\ndescription: Sin nombre\n---\n");
        let skills = list(&t.0);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "gamma");
        assert_eq!(skills[0].description, "Sin nombre");
        assert_eq!(skills[0].when, None);
    }

    #[test]
    fn list_lee_description_y_when_to_use() {
        let t = Temporal::nueva("campos");
        t.skill(
            "delta",
            "---\nname: delta\ndescription: Hace cosas\nwhen_to_use: Cuando el usuario pida tareas\n---\n",
        );
        let s = &list(&t.0)[0];
        assert_eq!(s.id, "delta");
        assert_eq!(s.name, "delta");
        assert_eq!(s.description, "Hace cosas");
        assert_eq!(s.when.as_deref(), Some("Cuando el usuario pida tareas"));
        assert!(s.modified.is_some());
    }

    #[test]
    fn list_orden_estable() {
        let t = Temporal::nueva("orden");
        for id in ["c", "a", "b"] {
            t.skill(id, &format!("---\nname: {id}\n---\n"));
        }
        let primera = list(&t.0);
        let segunda = list(&t.0);
        assert_eq!(primera, segunda);
        // De la más antigua a la más nueva; con la misma fecha, por carpeta.
        for par in primera.windows(2) {
            assert!(
                (par[0].created, &par[0].id) <= (par[1].created, &par[1].id),
                "orden incorrecto entre {} y {}",
                par[0].id,
                par[1].id
            );
        }
    }

    // ---------- fingerprint ----------

    #[test]
    fn fingerprint_cambia_al_editar_y_al_añadir_o_quitar() {
        let t = Temporal::nueva("huella");
        t.skill("alfa", "---\nname: alfa\n---\n");
        let inicial = fingerprint(&t.0);

        t.skill("alfa", "---\nname: alfa\ndescription: Más largo\n---\n");
        let editada = fingerprint(&t.0);
        assert_ne!(inicial, editada);

        t.skill("beta", "---\nname: beta\n---\n");
        let anadida = fingerprint(&t.0);
        assert_ne!(editada, anadida);

        fs::remove_dir_all(t.0.join(".claude/skills/beta")).unwrap();
        let quitada = fingerprint(&t.0);
        assert_ne!(anadida, quitada);
        assert_eq!(quitada, editada);
    }

    // ---------- install_agent ----------

    #[test]
    fn install_agent_escribe_la_primera_vez_y_no_repite() {
        let t = Temporal::nueva("agente-nuevo");
        assert!(install_agent(&t.0).unwrap());
        assert!(!install_agent(&t.0).unwrap());
        assert_eq!(fs::read_to_string(agent_path(&t.0)).unwrap(), AGENT);
    }

    #[test]
    fn install_agent_reescribe_version_vieja_con_marca() {
        let t = Temporal::nueva("agente-viejo");
        t.escribe(
            &format!(".claude/agents/{AGENT_NAME}.md"),
            &format!("---\nname: creador-de-skills\n---\nVersión vieja\n{AGENT_MARK}\n"),
        );
        assert!(install_agent(&t.0).unwrap());
        assert_eq!(fs::read_to_string(agent_path(&t.0)).unwrap(), AGENT);
    }

    #[test]
    fn install_agent_no_toca_el_agente_sin_marca() {
        let t = Temporal::nueva("agente-propio");
        let propio = "Mi agente, sin la marca de nexo\n";
        t.escribe(&format!(".claude/agents/{AGENT_NAME}.md"), propio);
        assert!(!install_agent(&t.0).unwrap());
        assert_eq!(fs::read_to_string(agent_path(&t.0)).unwrap(), propio);
    }

    #[test]
    fn agente_instalado_usa_sonnet_y_esfuerzo_medio() {
        let t = Temporal::nueva("agente-modelo");
        install_agent(&t.0).unwrap();
        let meta = frontmatter(&fs::read_to_string(agent_path(&t.0)).unwrap());
        assert_eq!(valor(&meta, "name"), Some(AGENT_NAME));
        assert_eq!(valor(&meta, "model"), Some("claude-sonnet-5-5"));
        assert_eq!(valor(&meta, "effort"), Some("medium"));
    }

    // ---------- skill_folder ----------

    #[test]
    fn skill_folder_rechaza_ids_peligrosos_o_inexistentes() {
        let t = Temporal::nueva("ids-malos");
        t.skill("alfa", "---\nname: alfa\n---\n");
        t.skill(".oculta", "---\nname: oculta\n---\n");
        for id in ["", "..", "../x", "a/b", "a\\b", ".oculta", "noexiste"] {
            assert!(skill_folder(&t.0, id).is_err(), "debería rechazar {id:?}");
        }
    }

    #[test]
    fn skill_folder_acepta_una_existente() {
        let t = Temporal::nueva("ids-buenos");
        t.skill("alfa", "---\nname: alfa\n---\n");
        assert_eq!(
            skill_folder(&t.0, "alfa").unwrap(),
            t.0.join(".claude").join("skills").join("alfa")
        );
    }
}
