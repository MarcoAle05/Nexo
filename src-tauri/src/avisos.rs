//! Avisos de escritorio de los workspaces (`docs/spec-avisos-workspaces.md`): cuando el
//! orquestador o uno de sus agentes pide permiso o pregunta algo, cuando el workspace queda
//! libre y cuando Claude Code se cierra solo con trabajo a medias.
//!
//! Aquí no se sabe nada de procesos: el lector (`workspace_run.rs`) cuenta qué pasó y este
//! módulo arma el texto, decide si se avisa (no si el usuario ya está mirando ese workspace) y
//! lo envía detrás del rasgo `Aviso`, que las pruebas sustituyen.
//!
//! Un aviso solo lleva el nombre del workspace, quién y el nombre de la herramienta: nunca la
//! entrada de la herramienta (órdenes, rutas, URL…), el texto de una pregunta o un plan ni nada
//! de lo que dice el modelo. Los avisos se quedan en el historial del escritorio y se ven con la
//! pantalla bloqueada.

use std::path::{Path, PathBuf};

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use crate::workspace;

/// Lo que pasó en un workspace y merece un aviso.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Suceso {
    /// Una petición `can_use_tool` nueva: `agent` es el `agent_id` (sin él, el orquestador).
    Pide { agent: Option<String>, tool: String },
    /// Pasó a libre: sin turno, sin tareas en segundo plano y sin peticiones pendientes.
    Termino { error: bool },
    /// Claude Code se cerró solo mientras trabajaba o esperaba una respuesta.
    Cayo,
}

/// Qué workspace tiene la interfaz en pantalla y si la ventana tiene el foco (lo dice la
/// interfaz con `workspace_watch`; sin noticias, sin foco).
#[derive(Debug, Clone, Default)]
pub(crate) struct Mirando {
    pub path: Option<PathBuf>,
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Texto {
    pub title: String,
    pub body: String,
}

/// El envío al escritorio, detrás de un rasgo para probar sin tocarlo.
pub(crate) trait Aviso {
    fn enviar(&self, texto: &Texto) -> Result<(), String>;
}

/// El escritorio de verdad, con `tauri-plugin-notification` (en Linux, D-Bus).
pub(crate) struct Escritorio(pub AppHandle);

impl Aviso for Escritorio {
    fn enviar(&self, texto: &Texto) -> Result<(), String> {
        self.0
            .notification()
            .builder()
            .title(&texto.title)
            .body(&texto.body)
            .show()
            .map_err(|e| e.to_string())
    }
}

fn workspace_name(root: &Path) -> String {
    root.file_name().map_or_else(
        || root.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Quién pide: el orquestador, el agente registrado en `.nexo/agentes/` cuya `task` es el
/// `agent_id` o, si no está, «Un agente».
pub(crate) fn quien(root: &Path, agent: Option<&str>) -> String {
    let Some(id) = agent else {
        return "El orquestador".into();
    };
    workspace::agents(root)
        .into_iter()
        .find(|a| a.task == id)
        .map_or_else(|| "Un agente".into(), |a| a.name)
}

/// Título y texto de un aviso. Solo con el nombre del workspace, quién y la herramienta.
pub(crate) fn texto(root: &Path, suceso: &Suceso) -> Texto {
    let body = match suceso {
        Suceso::Pide { agent, tool } => {
            let who = quien(root, agent.as_deref());
            match tool.as_str() {
                "AskUserQuestion" => format!("{who} te hace una pregunta"),
                "ExitPlanMode" => format!("{who} te propone un plan"),
                _ => format!("{who} pide permiso para usar {tool}"),
            }
        }
        Suceso::Termino { error: false } => "Terminó el trabajo".into(),
        Suceso::Termino { error: true } => "Terminó con un error".into(),
        Suceso::Cayo => "Se cerró con trabajo a medias".into(),
    };
    Texto {
        title: workspace_name(root),
        body,
    }
}

/// No se avisa de lo que el usuario ya tiene delante: la ventana con el foco y ese workspace en
/// pantalla. En cualquier otro caso, sí.
pub(crate) fn se_avisa(mirando: &Mirando, root: &Path) -> bool {
    !(mirando.focused && mirando.path.as_deref() == Some(root))
}

/// Decide, arma y envía. Si el escritorio no acepta el aviso, queda un `warn` y nada más: el
/// lector, el orquestador y la interfaz no dependen de él.
pub(crate) fn avisar(aviso: &dyn Aviso, mirando: &Mirando, root: &Path, suceso: &Suceso) {
    if !se_avisa(mirando, root) {
        return;
    }
    if let Err(e) = aviso.enviar(&texto(root, suceso)) {
        log::warn!("No se pudo enviar el aviso de escritorio: {e}");
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    /// Guarda los avisos en vez de enviarlos.
    #[derive(Default)]
    pub(crate) struct Guardados(pub RefCell<Vec<Texto>>);

    impl Aviso for Guardados {
        fn enviar(&self, texto: &Texto) -> Result<(), String> {
            self.0.borrow_mut().push(texto.clone());
            Ok(())
        }
    }

    pub(crate) fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join(format!("nexo-avisos-{name}-{}", std::process::id()))
            .join("proyecto");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn pide(agent: Option<&str>, tool: &str) -> Suceso {
        Suceso::Pide {
            agent: agent.map(String::from),
            tool: tool.into(),
        }
    }

    #[test]
    fn el_aviso_de_un_permiso_dice_quien_y_que_herramienta() {
        let root = temp("quien");
        workspace::register_task(
            &root,
            "s1",
            &json!({ "task_id": "a1", "task_type": "local_agent", "description": "Revisor de la API" }),
        )
        .unwrap();
        let t = texto(&root, &pide(None, "Bash"));
        assert_eq!(t.title, "proyecto");
        assert_eq!(t.body, "El orquestador pide permiso para usar Bash");
        assert_eq!(
            texto(&root, &pide(Some("a1"), "Edit")).body,
            "Revisor de la API pide permiso para usar Edit"
        );
        assert_eq!(
            texto(&root, &pide(Some("otro"), "Write")).body,
            "Un agente pide permiso para usar Write"
        );
        assert_eq!(
            texto(&root, &Suceso::Termino { error: false }).body,
            "Terminó el trabajo"
        );
        assert_eq!(
            texto(&root, &Suceso::Termino { error: true }).body,
            "Terminó con un error"
        );
        assert_eq!(
            texto(&root, &Suceso::Cayo).body,
            "Se cerró con trabajo a medias"
        );
    }

    #[test]
    fn el_aviso_de_una_pregunta_no_lleva_lo_que_pregunta() {
        let root = temp("pregunta");
        let pregunta = texto(&root, &pide(None, "AskUserQuestion"));
        assert_eq!(pregunta.body, "El orquestador te hace una pregunta");
        let plan = texto(&root, &pide(None, "ExitPlanMode"));
        assert_eq!(plan.body, "El orquestador te propone un plan");
    }

    #[test]
    fn los_avisos_no_llevan_la_entrada_de_la_herramienta() {
        // `Suceso` no tiene dónde llevar la entrada: el lector solo saca quién y la herramienta.
        // Aun así, con una línea real, nada de su entrada llega al texto.
        let root = temp("entrada");
        let line = json!({ "type": "control_request", "request_id": "r1", "request": {
            "subtype": "can_use_tool", "tool_name": "Bash",
            "input": { "command": "curl -H 'Authorization: Bearer sk-secreta' https://ejemplo.org/x" },
            "tool_use_id": "toolu_1" } });
        let suceso = pide(None, line["request"]["tool_name"].as_str().unwrap());
        let avisos = Guardados::default();
        avisar(&avisos, &Mirando::default(), &root, &suceso);
        let t = &avisos.0.borrow()[0];
        for secreto in ["curl", "sk-secreta", "ejemplo.org", "Authorization"] {
            assert!(!t.body.contains(secreto) && !t.title.contains(secreto));
        }
    }

    #[test]
    fn no_avisa_si_se_esta_mirando_ese_workspace() {
        let root = temp("mirando");
        let mirando = Mirando {
            path: Some(root.clone()),
            focused: true,
        };
        assert!(!se_avisa(&mirando, &root));
        let avisos = Guardados::default();
        avisar(&avisos, &mirando, &root, &Suceso::Termino { error: false });
        assert!(avisos.0.borrow().is_empty());
    }

    #[test]
    fn avisa_si_la_ventana_no_tiene_el_foco_o_se_mira_otra_cosa() {
        let root = temp("otra-cosa");
        let otro = root.with_file_name("otro");
        let caso = |path: Option<&Path>, focused| Mirando {
            path: path.map(Path::to_path_buf),
            focused,
        };
        assert!(se_avisa(&caso(Some(&root), false), &root), "sin foco");
        assert!(se_avisa(&caso(Some(&otro), true), &root), "otro workspace");
        assert!(se_avisa(&caso(Some(&otro), false), &root));
        assert!(se_avisa(&caso(None, true), &root), "grafo, notas…");
        assert!(se_avisa(&Mirando::default(), &root), "sin noticias");
    }
}
