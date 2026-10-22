//! La skill `nexo-orquestacion` (lo pidió el usuario el 22-10-2026, ver
//! `docs/spec-terminales-workspaces.md`): le enseña al agente del chat de Nexo a trabajar en
//! los workspaces con la orden de control (`control.rs`). Va en las skills de proyecto del
//! vault (el proyecto del chat de Nexo), así que también la lanza el usuario con
//! `/nexo-orquestacion` y sale en la órbita de la galaxia.
//!
//! nexo la escribe al abrir el chat con la ruta de verdad de su ejecutable, como el agente
//! `creador-de-skills` (`skills.rs`): solo si falta o si es una versión anterior suya (lleva
//! la marca); si el usuario le quita la marca, ya no se toca.

use std::fs;
use std::path::{Path, PathBuf};

pub const NAME: &str = "nexo-orquestacion";
pub const MARK: &str = "<!-- nexo: esta skill la mantiene nexo";

const SKILL: &str = r#"---
name: nexo-orquestacion
description: Trabajar en los workspaces de nexo desde el chat de Nexo: ver los workspaces y sus terminales, abrir una terminal y ejecutar algo, delegar un trabajo a un Claude Code (también como uno de los agentes del vault), leer lo que hizo, esperar a que termine o necesite al usuario, y ver qué agentes trabajan y cuánto gastan. Úsala cuando el usuario pida hacer algo en uno de sus proyectos (un workspace), delegarlo a un agente, saber cómo va o cuánto se ha gastado.
---
<!-- nexo: esta skill la mantiene nexo; si quitas esta línea, deja de actualizarla -->

# Orquestar los workspaces de nexo

Los workspaces son las carpetas de proyecto que el usuario tiene en el panel de Agentes de nexo. En cada uno se trabaja con terminales normales, como en Orca: lo que lances aparece en sus ventanas, el usuario lo ve y puede meterse. Todo se hace con la orden de control de nexo:

```
{nexo} ctl <orden> …
```

La respuesta es JSON. Si dice que nexo no está abierto, díselo al usuario: no lo abras tú.

## Órdenes

| Para | Orden |
|---|---|
| Ver los workspaces del panel | `{nexo} ctl workspaces` |
| Ver sus terminales, con su Claude, su estado y sus subagentes | `{nexo} ctl estado [workspace]` |
| Abrir una terminal y ejecutar algo | `{nexo} ctl abrir-terminal <workspace> --nombre "Tests" --orden "npm test"` |
| Delegar un trabajo a Claude Code | `{nexo} ctl claude <workspace> --nombre "Revisión" "<encargo>"` |
| … como un agente del vault | añade `--agente <nombre>` (los de `.claude/agents/` de este vault) |
| … con otro modelo, esfuerzo o modo de permisos | `--modelo sonnet`, `--esfuerzo low\|medium\|high\|xhigh\|max`, `--permisos default\|acceptEdits\|plan\|auto` |
| … con un encargo largo | por la entrada: `{nexo} ctl claude <workspace> --nombre X - <<'FIN'` (el encargo) `FIN` |
| Leer lo que dijo o lo que enseña | `{nexo} ctl leer <terminal> [--lineas 80]` |
| Esperar a que acabe o necesite al usuario | `{nexo} ctl esperar <terminal> --hasta inactivo\|espera\|fin [--max 600]` |
| Escribirle (seguir la conversación, contestar a un programa) | `{nexo} ctl escribir <terminal> "texto" --enter` |
| Ver qué gasta cada agente | `{nexo} ctl gasto [workspace]` |
| Cerrar una terminal | `{nexo} ctl cerrar <terminal>` |

El workspace va por su nombre (el de `workspaces`); la terminal, por su número (el de `estado` o el que devuelven `claude` y `abrir-terminal`).

## Cómo trabajar

1. Mira `workspaces` y elige el del encargo; si no está claro cuál, pregunta.
2. Si un agente del vault encaja con el encargo, lánzalo con `--agente`. Si no, un Claude Code normal con un encargo completo: qué hacer, dónde y cómo se sabe que está hecho.
3. Di al usuario qué lanzaste, en qué workspace y cómo se llama su ventana (la ve en Agentes → ese workspace).
4. Para seguirlo, `esperar` y después `leer`, y resume. `esperar --hasta espera` vuelve cuando ese Claude necesita al usuario: díselo.

## Reglas

- Los permisos, las preguntas y los planes de otro Claude los contesta **solo el usuario**: nexo no deja escribir en una terminal que espera una respuesta. No lo intentes de otra forma; avisa al usuario de que le esperan.
- No cierres sin preguntar una terminal que no abriste tú: puede tener trabajo del usuario.
- Ningún modo de permisos se salta los permisos, y no le pidas a otro Claude que lo haga.
- Cada Claude que lances gasta de la sesión de 5 h del usuario: para algo grande, dilo antes y pregunta antes de lanzar varios a la vez.
- `gasto` da los tokens, el coste y la parte de la sesión de 5 h de cada agente; resúmelo en una tabla corta.
"#;

fn skill_path(root: &Path) -> PathBuf {
    root.join(".claude/skills").join(NAME).join("SKILL.md")
}

/// El ejecutable de nexo como se escribe en una orden: entre comillas solo si hace falta.
fn command_of(exe: &Path) -> String {
    let text = exe.to_string_lossy();
    if text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c))
    {
        text.into_owned()
    } else {
        crate::terminales::quote(&text)
    }
}

/// El texto de la skill con la ruta del ejecutable de nexo.
pub(crate) fn text(exe: &Path) -> String {
    SKILL.replace("{nexo}", &command_of(exe))
}

/// Deja la skill en el vault: si falta o si es una versión anterior suya. Devuelve si escribió.
pub fn install(root: &Path, exe: &Path) -> std::io::Result<bool> {
    let path = skill_path(root);
    let want = text(exe);
    match fs::read_to_string(&path) {
        Ok(current) if current == want || !current.contains(MARK) => return Ok(false),
        _ => {}
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&path, want)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nexo-orquestacion-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn la_skill_lleva_la_ruta_de_nexo_y_solo_se_reescribe_si_es_suya() {
        let root = temp("instalar");
        let exe = Path::new("/home/ana/.local/bin/nexo");
        assert!(install(&root, exe).unwrap());
        let written = fs::read_to_string(skill_path(&root)).unwrap();
        assert!(written.starts_with("---\nname: nexo-orquestacion\n"));
        assert!(written.contains("/home/ana/.local/bin/nexo ctl claude <workspace>"));
        assert!(!written.contains("{nexo}"));
        assert!(!install(&root, exe).unwrap(), "igual: no se toca");
        // Otra ruta del ejecutable: se pone al día.
        assert!(install(&root, Path::new("/opt/nexo bin/nexo")).unwrap());
        assert!(
            fs::read_to_string(skill_path(&root))
                .unwrap()
                .contains("'/opt/nexo bin/nexo' ctl workspaces")
        );
        // Sin la marca es del usuario.
        fs::write(
            skill_path(&root),
            "---\nname: nexo-orquestacion\n---\nMía\n",
        )
        .unwrap();
        assert!(!install(&root, exe).unwrap());
        assert!(
            fs::read_to_string(skill_path(&root))
                .unwrap()
                .contains("Mía")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn la_skill_nombra_todas_las_ordenes_de_control() {
        let text = text(Path::new("/x/nexo"));
        for orden in [
            "workspaces",
            "estado",
            "abrir-terminal",
            "claude",
            "leer",
            "esperar",
            "escribir",
            "gasto",
            "cerrar",
        ] {
            assert!(text.contains(&format!("/x/nexo ctl {orden}")), "{orden}");
        }
    }
}
