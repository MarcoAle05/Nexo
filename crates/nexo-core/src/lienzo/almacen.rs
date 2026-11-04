//! Dónde viven los lienzos: un archivo por escritorio en `.nexo/lienzos/`, cargado en memoria
//! mientras se usa. Es el único que los escribe, en un temporal que luego se renombra para no dejar
//! nunca un archivo cortado. Si uno no se entiende, se aparta sin borrarlo y se empieza de cero.
//!
//! Va aparte de `.nexo/escritorio.json` porque ese se reescribe a cada rato, lo editan a mano y se
//! copia entero a `escritorios.json` al cambiar de escritorio. Con un archivo por id, archivar y
//! restaurar un escritorio no tienen que saber nada de lo que hay dentro.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;

use super::datos::id_valido;
use super::{Capa, Lienzo, Rect};

const CARPETA: &str = ".nexo/lienzos";
const ESCRITORIO: &str = ".nexo/escritorio.json";
const ESCRITORIOS: &str = ".nexo/escritorios.json";
/// El escritorio que hay si nunca se ha guardado la lista (`normalize` de `desks.js` usa el mismo):
/// con un id inventado en cada arranque, los elementos del primer escritorio se perderían.
pub const ESCRITORIO_PRINCIPAL: &str = "principal";

pub fn ruta_lienzo(raiz: &Path, escritorio: &str) -> Result<PathBuf, String> {
    if !id_valido(escritorio) {
        return Err(format!("«{escritorio}» no es un escritorio."));
    }
    Ok(raiz.join(CARPETA).join(format!("{escritorio}.json")))
}

fn leer_json(ruta: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(ruta).ok()?).ok()
}

/// El escritorio abierto: el de la lista si es válido, el primero si no; sin lista, el principal.
pub fn escritorio_activo(raiz: &Path) -> String {
    let Some(lista) = leer_json(&raiz.join(ESCRITORIOS)) else {
        return ESCRITORIO_PRINCIPAL.into();
    };
    let ids: Vec<&str> = lista["desks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| d["id"].as_str())
        .collect();
    match lista["active"].as_str() {
        Some(a) if ids.contains(&a) => a.to_string(),
        _ => ids
            .first()
            .map_or(ESCRITORIO_PRINCIPAL.to_string(), |s| s.to_string()),
    }
}

type Entorno = (Vec<(String, Rect)>, Option<(f64, f64)>);

/// Las cartas de notas de un escritorio y dónde se está mirando: el activo está en
/// `escritorio.json`, y los demás, en la lista.
fn entorno(raiz: &Path, escritorio: &str) -> Entorno {
    let datos = if escritorio == escritorio_activo(raiz) {
        leer_json(&raiz.join(ESCRITORIO))
    } else {
        leer_json(&raiz.join(ESCRITORIOS)).and_then(|l| {
            let todos = l["desks"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(l["archived"].as_array().into_iter().flatten());
            todos
                .into_iter()
                .find(|d| d["id"].as_str() == Some(escritorio))
                .cloned()
        })
    };
    let Some(d) = datos else {
        return (Vec::new(), None);
    };
    let n = |c: &Value, k: &str, otro: f64| c[k].as_f64().unwrap_or(otro);
    let cartas = d["cards"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some((
                c["path"].as_str()?.to_string(),
                Rect {
                    x: n(c, "x", 0.0),
                    y: n(c, "y", 0.0),
                    w: n(c, "w", 380.0),
                    h: n(c, "h", 440.0),
                },
            ))
        })
        .collect();
    let vista = d["view"]["x"].as_f64().zip(d["view"]["y"].as_f64());
    (cartas, vista)
}

/// Escribe en un temporal y lo pone en su sitio.
pub fn escribir(ruta: &Path, texto: &str) -> Result<(), String> {
    if let Some(dir) = ruta.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = ruta.with_extension("json.tmp");
    fs::write(&tmp, texto).map_err(|e| e.to_string())?;
    fs::rename(&tmp, ruta).map_err(|e| e.to_string())
}

struct Abierto {
    lienzo: Lienzo,
    guardada: u64,
}

#[derive(Default)]
struct Dentro {
    raiz: Option<PathBuf>,
    abiertos: BTreeMap<String, Abierto>,
    avisos: Vec<String>,
}

#[derive(Default)]
pub struct Almacen {
    dentro: Mutex<Dentro>,
}

impl Dentro {
    /// Cambiar de vault cierra lo del anterior, después de guardarlo.
    fn entrar(&mut self, raiz: &Path) {
        if self.raiz.as_deref() != Some(raiz) {
            self.guardar();
            self.abiertos.clear();
            self.raiz = Some(raiz.to_path_buf());
        }
    }

    fn guardar(&mut self) {
        let Some(raiz) = self.raiz.clone() else {
            return;
        };
        let mut fallos = Vec::new();
        for (id, a) in &mut self.abiertos {
            if a.lienzo.rev == a.guardada {
                continue;
            }
            let hecho = ruta_lienzo(&raiz, id).and_then(|r| escribir(&r, &a.lienzo.a_json()));
            match hecho {
                Ok(()) => a.guardada = a.lienzo.rev,
                Err(e) => fallos.push(format!("No se pudo guardar el lienzo: {e}")),
            }
        }
        self.avisos.extend(fallos);
    }

    fn abierto(&mut self, raiz: &Path, escritorio: &str) -> Result<&mut Abierto, String> {
        self.entrar(raiz);
        if !self.abiertos.contains_key(escritorio) {
            let ruta = ruta_lienzo(raiz, escritorio)?;
            let lienzo = match fs::read_to_string(&ruta) {
                Err(_) => Lienzo::default(),
                Ok(texto) => match Lienzo::desde_json(&texto) {
                    Ok(l) => l,
                    Err(e) => {
                        // No se sobrescribe: el usuario puede querer rescatarlo.
                        let aparte = ruta.with_extension("roto.json");
                        let _ = fs::rename(&ruta, &aparte);
                        self.avisos.push(format!(
                            "El lienzo de este escritorio no se entiende ({e}): se dejó en {} y se empieza uno vacío.",
                            aparte.display()
                        ));
                        Lienzo::default()
                    }
                },
            };
            let guardada = lienzo.rev;
            self.abiertos
                .insert(escritorio.to_string(), Abierto { lienzo, guardada });
        }
        self.abiertos
            .get_mut(escritorio)
            .ok_or_else(|| "No se pudo abrir.".to_string())
    }
}

impl Almacen {
    /// Hace algo con el lienzo de un escritorio, con las cartas al día.
    pub fn con<R>(
        &self,
        raiz: &Path,
        escritorio: &str,
        f: impl FnOnce(&mut Lienzo) -> R,
    ) -> Result<R, String> {
        let mut d = self.dentro.lock().map_err(|e| e.to_string())?;
        let a = d.abierto(raiz, escritorio)?;
        let (cartas, vista) = entorno(raiz, escritorio);
        a.lienzo.fijar_entorno(cartas, vista);
        Ok(f(&mut a.lienzo))
    }

    /// El escritorio al que pertenece una capa. Una capa sigue en el suyo aunque el usuario cambie
    /// de escritorio, y su trabajo no se corta.
    pub fn escritorio_de_capa(&self, raiz: &Path, capa: &str) -> Result<String, String> {
        let mut d = self.dentro.lock().map_err(|e| e.to_string())?;
        d.entrar(raiz);
        if let Some(id) = d
            .abiertos
            .iter()
            .find(|(_, a)| a.lienzo.capa(capa).is_some())
            .map(|(id, _)| id.clone())
        {
            return Ok(id);
        }
        let carpeta = raiz.join(CARPETA);
        let mut archivos: Vec<PathBuf> = fs::read_dir(&carpeta)
            .map(|l| l.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        archivos.sort();
        for ruta in archivos {
            if ruta.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let Some(id) = ruta.file_stem().and_then(|s| s.to_str()).map(String::from) else {
                continue;
            };
            if !id_valido(&id) {
                continue;
            }
            if d.abierto(raiz, &id)?.lienzo.capa(capa).is_some() {
                return Ok(id);
            }
        }
        Err(
            "Esa capa ya no existe: el usuario la integró o la borró, o se borró su escritorio."
                .into(),
        )
    }

    /// Guarda lo que haya cambiado. Se llama cada poco desde un hilo y al cerrar nexo.
    pub fn guardar(&self) {
        if let Ok(mut d) = self.dentro.lock() {
            d.guardar();
        }
    }

    /// Lo que salió mal al leer o guardar, para que la interfaz lo diga.
    pub fn avisos(&self) -> Vec<String> {
        self.dentro
            .lock()
            .map(|mut d| std::mem::take(&mut d.avisos))
            .unwrap_or_default()
    }

    /// Se borró un escritorio archivado: deja de estar en memoria. El archivo lo quita quien llama,
    /// porque va a la papelera, que aquí no se conoce.
    pub fn olvidar(&self, raiz: &Path, escritorio: &str) -> Result<PathBuf, String> {
        let ruta = ruta_lienzo(raiz, escritorio)?;
        let mut d = self.dentro.lock().map_err(|e| e.to_string())?;
        d.entrar(raiz);
        d.abiertos.remove(escritorio);
        Ok(ruta)
    }

    /// El turno del agente acabó (`ahora` = `None`), o pasó el tiempo: las capas que seguían
    /// abiertas quedan sin terminar. Da las que cambiaron, con su escritorio.
    pub fn barrer(&self, ahora: Option<u64>) -> Vec<(String, Capa)> {
        let Ok(mut d) = self.dentro.lock() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (id, a) in &mut d.abiertos {
            for capa in a.lienzo.barrer(ahora) {
                out.push((id.clone(), capa));
            }
        }
        out
    }

    /// La interfaz se recargó: ya no hay gestos en curso.
    pub fn soltar_todo(&self) {
        if let Ok(mut d) = self.dentro.lock() {
            for a in d.abiertos.values_mut() {
                a.lienzo.soltar_todo();
            }
        }
    }
}
