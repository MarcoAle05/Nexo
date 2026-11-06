//! El motor del lienzo de Notas (`docs/spec-lienzo-elementos.md`): los elementos que no son notas
//! (cuadros de texto, figuras, flechas, tablas, tableros…) y sus capas.
//!
//! Hay un solo escritor. Todo cambio es una **operación** y pasa por `Lienzo::aplicar`, venga de
//! la interfaz o del agente de Nexo, así que nadie pisa a nadie. Las reglas de convivencia viven
//! aquí y no en la interfaz:
//!
//! - lo que hace el usuario se aplica siempre, y lo que toca de una capa del agente pasa a ser suyo;
//! - el agente solo escribe en su capa, nunca toca lo que el usuario tocó ni lo que está usando;
//! - lo que no se aplica no para el resto del lote: sale en `omitidas`, con el motivo y el estado
//!   actual, para que el agente siga con lo demás.
//!
//! El lienzo vive en `.nexo/lienzos/<id del escritorio>.json` (ver `almacen`), fuera de la wiki:
//! no sale en el grafo ni en el árbol de notas.

pub mod almacen;
mod compuestos;
pub mod datos;
mod orden;
pub mod trazado;
mod zona;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub use orden::{MovCarta, Orden};
pub use zona::{Rect, reticula_x, reticula_y};

pub const VERSION: u32 = 1;
/// La capa del usuario: existe siempre.
pub const CAPA_USUARIO: &str = "tu";
pub const MAX_ELEMENTOS: usize = 5000;
pub const MAX_LOTE: usize = 500;
/// El lienzo mide lo mismo que en la interfaz (`WORLD` de `notes.js`), con el (0, 0) en su centro.
pub const MUNDO: (f64, f64) = (40000.0, 28000.0);
/// Una capa que dice dibujar y lleva este tiempo sin hacer nada queda «sin terminar».
pub const INACTIVA_MS: u64 = 5 * 60 * 1000;

const MAX_ANOTADOS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Autor {
    Usuario,
    Nexo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstadoCapa {
    Dibujando,
    Terminada,
    /// El agente se detuvo o nexo se cerró a medias. Si no se sabe cómo acabó, nunca se da por
    /// bueno (la misma regla que los agentes de los workspaces).
    SinTerminar,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capa {
    pub id: String,
    pub nombre: String,
    pub autor: Autor,
    pub visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estado: Option<EstadoCapa>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Elemento {
    pub id: String,
    pub capa: String,
    pub tipo: String,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub w: f64,
    #[serde(default)]
    pub h: f64,
    #[serde(default)]
    pub z: i64,
    /// El usuario lo cambió: desde entonces es suyo.
    #[serde(default)]
    pub tocado: bool,
    /// En tablas y tableros, las partes que tocó (`f1:c2`, `t3`): el resto sigue libre.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tocadas: Vec<String>,
    /// La revisión en que cambió por última vez.
    #[serde(default)]
    pub rev: u64,
    #[serde(default)]
    pub datos: Value,
}

/// Quién pide las operaciones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quien<'a> {
    Usuario,
    /// El agente, dentro de su capa. `del_usuario`: el usuario le pidió en el chat cambiar algo suyo.
    Agente {
        capa: &'a str,
        del_usuario: bool,
    },
    /// Una orden de ordenar: solo mueve, y respeta lo que está en uso.
    Orden,
}

#[derive(Debug, Default, Serialize, Clone, PartialEq)]
pub struct Cambios {
    pub poner: Vec<Elemento>,
    pub quitar: Vec<String>,
    pub capas: Vec<Capa>,
    pub capas_quitadas: Vec<String>,
}

impl Cambios {
    pub fn vacio(&self) -> bool {
        self.poner.is_empty()
            && self.quitar.is_empty()
            && self.capas.is_empty()
            && self.capas_quitadas.is_empty()
    }
}

/// Lo que pasó al aplicar un lote.
#[derive(Debug, Default, Serialize, Clone, PartialEq)]
pub struct Resultado {
    pub rev: u64,
    /// Lo creado, con su `ref` si la traía y el id que le puso el motor.
    pub hechas: Vec<Value>,
    pub aplicadas: usize,
    /// Lo que no se aplicó: el motivo y el estado actual, para adaptarse.
    pub omitidas: Vec<Value>,
    /// Lo que hay que repintar: la interfaz solo pinta por aquí.
    pub cambios: Cambios,
    /// Las operaciones que deshacen este lote, ya en el orden en que hay que aplicarlas.
    pub inversas: Vec<Value>,
    /// Lo que el usuario cambió en esa capa desde la última orden del agente.
    pub usuario_cambio: Vec<Value>,
    /// El lote caía encima de algo ajeno y se movió entero: cuánto.
    pub desplazado: Option<(f64, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lienzo {
    pub version: u32,
    pub rev: u64,
    #[serde(default)]
    pub proximo: u64,
    pub capas: Vec<Capa>,
    pub elementos: Vec<Elemento>,
    /// Lo que el usuario está arrastrando o escribiendo: `e3` o `e3/f1:c2`. Solo en memoria.
    #[serde(skip)]
    en_uso: BTreeSet<String>,
    #[serde(skip)]
    anotados: BTreeMap<String, Vec<Value>>,
    #[serde(skip)]
    actividad: BTreeMap<String, u64>,
    #[serde(skip)]
    quitados: Vec<(u64, String)>,
    #[serde(skip)]
    pub(crate) reservas: Vec<(Rect, u64)>,
    /// Dónde están las cartas de notas (`.nexo/escritorio.json`): obstáculos y lo que se ordena.
    #[serde(skip)]
    pub(crate) cartas: Vec<(String, Rect)>,
    #[serde(skip)]
    pub(crate) vista: Option<(f64, f64)>,
}

impl Default for Lienzo {
    fn default() -> Self {
        Lienzo {
            version: VERSION,
            rev: 0,
            proximo: 0,
            capas: vec![Capa {
                id: CAPA_USUARIO.into(),
                nombre: "Tú".into(),
                autor: Autor::Usuario,
                visible: true,
                estado: None,
            }],
            elementos: Vec::new(),
            en_uso: BTreeSet::new(),
            anotados: BTreeMap::new(),
            actividad: BTreeMap::new(),
            quitados: Vec::new(),
            reservas: Vec::new(),
            cartas: Vec::new(),
            vista: None,
        }
    }
}

/// Dónde y a qué altura (`x`, `y`, `w`, `h`, `z`); lo que no se dice no cambia.
type Sitio = (
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<i64>,
);

/// Una operación que no se aplicó.
pub(crate) struct Omitida {
    pub motivo: String,
    pub actual: Option<Value>,
}

pub(crate) fn omitida<T>(motivo: impl Into<String>) -> Result<T, Omitida> {
    Err(Omitida {
        motivo: motivo.into(),
        actual: None,
    })
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
enum Op {
    Crear {
        #[serde(rename = "ref")]
        referencia: Option<String>,
        tipo: String,
        capa: Option<String>,
        x: Option<f64>,
        y: Option<f64>,
        w: Option<f64>,
        h: Option<f64>,
        z: Option<i64>,
        #[serde(default)]
        datos: Value,
    },
    Cambiar {
        id: String,
        x: Option<f64>,
        y: Option<f64>,
        w: Option<f64>,
        h: Option<f64>,
        z: Option<i64>,
        datos: Option<Value>,
    },
    Borrar {
        id: String,
    },
    Apilar {
        id: String,
        a: String,
    },
    /// Devuelve un elemento tal como estaba (deshacer). Solo el usuario.
    Poner {
        elemento: Box<Elemento>,
    },
    CapaVer {
        capa: String,
        visible: bool,
    },
    CapaRenombrar {
        capa: String,
        nombre: String,
    },
    CapaIntegrar {
        capa: String,
    },
    CapaBorrar {
        capa: String,
    },
    /// Devuelve una capa tal como estaba, con sus elementos (deshacer).
    CapaPoner {
        capa: Box<Capa>,
        #[serde(default)]
        elementos: Vec<Elemento>,
        /// Cómo se fue: rehacer tiene que volver a hacer lo mismo (integrarla o borrarla).
        #[serde(default)]
        integrada: bool,
    },
    #[serde(untagged)]
    Parte(compuestos::OpParte),
}

/// Lo que va juntando un lote.
#[derive(Default)]
pub(crate) struct Lote {
    pub refs: BTreeMap<String, String>,
    pub puestos: BTreeSet<String>,
    pub quitados: BTreeSet<String>,
    pub capas: BTreeSet<String>,
    pub capas_quitadas: BTreeSet<String>,
    pub hechas: Vec<Value>,
    pub inversas: Vec<Value>,
}

pub(crate) fn rect_propio(e: &Elemento) -> Rect {
    Rect {
        x: e.x,
        y: e.y,
        w: e.w,
        h: e.h,
    }
}

fn sujeta(v: f64, min: f64, max: f64) -> f64 {
    if !v.is_finite() {
        return min.max(0.0).min(max);
    }
    v.max(min).min(max)
}

fn redondo(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// La punta de una flecha como punto: un elemento se representa por su lado más cercano a `hacia`.
fn punto_de(fin: &Value, otros: &[Elemento], hacia: Option<(f64, f64)>) -> Option<(f64, f64)> {
    if let Some(id) = fin.get("id").and_then(Value::as_str) {
        let e = otros.iter().find(|e| e.id == id)?;
        let (cx, cy) = (e.x + e.w / 2.0, e.y + e.h / 2.0);
        let (hx, hy) = hacia.unwrap_or((cx, cy));
        let ancla = fin.get("ancla").and_then(Value::as_str).unwrap_or("auto");
        let lado = match ancla {
            "n" | "e" | "s" | "o" => ancla,
            _ => {
                let (dx, dy) = ((hx - cx) / e.w.max(1.0), (hy - cy) / e.h.max(1.0));
                if dx.abs() >= dy.abs() {
                    if dx >= 0.0 { "e" } else { "o" }
                } else if dy >= 0.0 {
                    "s"
                } else {
                    "n"
                }
            }
        };
        return Some(match lado {
            "n" => (cx, e.y),
            "s" => (cx, e.y + e.h),
            "e" => (e.x + e.w, cy),
            _ => (e.x, cy),
        });
    }
    Some((fin.get("x")?.as_f64()?, fin.get("y")?.as_f64()?))
}

fn centro_de(fin: &Value, otros: &[Elemento]) -> Option<(f64, f64)> {
    if let Some(id) = fin.get("id").and_then(Value::as_str) {
        let e = otros.iter().find(|e| e.id == id)?;
        return Some((e.x + e.w / 2.0, e.y + e.h / 2.0));
    }
    Some((fin.get("x")?.as_f64()?, fin.get("y")?.as_f64()?))
}

impl Lienzo {
    /// Lee lo guardado y lo deja en un estado válido. Lo que estaba dibujándose se cortó: nexo se
    /// cerró a medias, porque esto solo se lee al abrir.
    pub fn desde_json(texto: &str) -> Result<Lienzo, String> {
        let mut l: Lienzo = serde_json::from_str(texto).map_err(|e| e.to_string())?;
        if l.version > VERSION {
            return Err(format!("Es de una versión más nueva ({}).", l.version));
        }
        l.version = VERSION;
        if !l.capas.iter().any(|c| c.id == CAPA_USUARIO) {
            l.capas.insert(0, Lienzo::default().capas.remove(0));
        }
        for c in &mut l.capas {
            if c.estado == Some(EstadoCapa::Dibujando) {
                c.estado = Some(EstadoCapa::SinTerminar);
            }
        }
        let capas: BTreeSet<String> = l.capas.iter().map(|c| c.id.clone()).collect();
        for e in &mut l.elementos {
            if !capas.contains(&e.capa) {
                e.capa = CAPA_USUARIO.into();
            }
        }
        // Nunca volver a repartir un id que ya existe, aunque el contador venga mal.
        let mayor = l
            .elementos
            .iter()
            .filter_map(|e| e.id.strip_prefix('e')?.parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        l.proximo = l.proximo.max(mayor);
        Ok(l)
    }

    pub fn a_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }

    pub fn capa(&self, id: &str) -> Option<&Capa> {
        self.capas.iter().find(|c| c.id == id)
    }

    pub fn elemento(&self, id: &str) -> Option<&Elemento> {
        self.elementos.iter().find(|e| e.id == id)
    }

    pub(crate) fn elemento_mut(&mut self, id: &str) -> Option<&mut Elemento> {
        self.elementos.iter_mut().find(|e| e.id == id)
    }

    /// Dónde están las cartas de notas y qué trozo del lienzo se mira.
    pub fn fijar_entorno(&mut self, cartas: Vec<(String, Rect)>, vista: Option<(f64, f64)>) {
        self.cartas = cartas;
        self.vista = vista;
    }

    /// El usuario empieza (`true`) o acaba un gesto sobre un elemento (`e3`) o una parte (`e3/t1`).
    pub fn usar(&mut self, clave: &str, si: bool) {
        if si {
            self.en_uso.insert(clave.to_string());
        } else {
            self.en_uso.remove(clave);
        }
    }

    /// La interfaz se recargó: ya no hay ningún gesto en curso.
    pub fn soltar_todo(&mut self) {
        self.en_uso.clear();
    }

    pub(crate) fn esta_en_uso(&self, clave: &str) -> bool {
        self.en_uso.contains(clave)
    }

    /// Algo de un elemento, sea todo o una parte, está en uso.
    fn algo_en_uso(&self, id: &str) -> bool {
        let parte = format!("{id}/");
        self.en_uso.iter().any(|k| k == id || k.starts_with(&parte))
    }

    /// Empieza una capa del agente.
    pub fn nueva_capa(&mut self, nombre: &str, ahora: u64) -> Capa {
        self.proximo += 1;
        let limpio: String = nombre.split_whitespace().collect::<Vec<_>>().join(" ");
        let capa = Capa {
            id: format!("c{ahora:x}-{}", self.proximo),
            nombre: if limpio.is_empty() {
                "Sin título".into()
            } else {
                limpio.chars().take(datos::MAX_TITULO).collect()
            },
            autor: Autor::Nexo,
            visible: true,
            estado: Some(EstadoCapa::Dibujando),
        };
        self.capas.push(capa.clone());
        self.actividad.insert(capa.id.clone(), ahora);
        self.rev += 1;
        capa
    }

    fn cerrar_capa(&mut self, id: &str, estado: EstadoCapa) -> Option<Capa> {
        let rev = self.rev + 1;
        let capa = self.capas.iter_mut().find(|c| c.id == id)?;
        if capa.estado.is_none() || capa.estado == Some(estado) {
            return None;
        }
        capa.estado = Some(estado);
        let copia = capa.clone();
        self.actividad.remove(id);
        self.rev = rev;
        Some(copia)
    }

    /// El agente dice que acabó.
    pub fn terminar_capa(&mut self, id: &str) -> Option<Capa> {
        self.cerrar_capa(id, EstadoCapa::Terminada)
    }

    /// El turno del agente acabó, o lleva `INACTIVA_MS` sin hacer nada (`ahora`): lo que siga
    /// abierto se quedó sin terminar. Devuelve las capas que cambiaron.
    pub fn barrer(&mut self, ahora: Option<u64>) -> Vec<Capa> {
        let abiertas: Vec<String> = self
            .capas
            .iter()
            .filter(|c| c.estado == Some(EstadoCapa::Dibujando))
            .filter(|c| match ahora {
                None => true,
                Some(t) => self
                    .actividad
                    .get(&c.id)
                    .is_none_or(|a| t.saturating_sub(*a) >= INACTIVA_MS),
            })
            .map(|c| c.id.clone())
            .collect();
        abiertas
            .iter()
            .filter_map(|id| self.cerrar_capa(id, EstadoCapa::SinTerminar))
            .collect()
    }

    /// Hay algo dibujándose.
    pub fn dibujando(&self) -> bool {
        self.capas
            .iter()
            .any(|c| c.estado == Some(EstadoCapa::Dibujando))
    }

    /// El rectángulo que ocupa un elemento. Una flecha no guarda el suyo: sale de sus puntas.
    pub fn rect_de(&self, e: &Elemento) -> Rect {
        if e.tipo != "flecha" {
            return rect_propio(e);
        }
        let d = centro_de(&e.datos["desde"], &self.elementos).unwrap_or((0.0, 0.0));
        let h = centro_de(&e.datos["hasta"], &self.elementos).unwrap_or((0.0, 0.0));
        Rect {
            x: d.0.min(h.0),
            y: d.1.min(h.1),
            w: (d.0 - h.0).abs(),
            h: (d.1 - h.1).abs(),
        }
    }

    /// El orden de apilado también dice si algo va por encima de las cartas de notas (`z` de 1 en
    /// adelante) o por debajo (0 o menos): lo enviado al fondo y los marcos quedan detrás de las
    /// notas, y lo que se trae al frente o se crea, delante.
    fn al_frente(&self, capa: &str) -> i64 {
        let mayor = self
            .elementos
            .iter()
            .filter(|e| e.capa == capa)
            .map(|e| e.z)
            .max();
        mayor.map_or(1, |z| z + 1).max(1)
    }

    fn al_fondo(&self, capa: &str) -> i64 {
        let menor = self
            .elementos
            .iter()
            .filter(|e| e.capa == capa)
            .map(|e| e.z)
            .min();
        menor.map_or(0, |z| z - 1).min(0)
    }

    /// Lo que el usuario cambió de una capa del agente queda apuntado para decírselo.
    pub(crate) fn anotar(&mut self, capa: &str, nota: Value) {
        if capa == CAPA_USUARIO {
            return;
        }
        let lista = self.anotados.entry(capa.to_string()).or_default();
        if lista.len() < MAX_ANOTADOS {
            lista.push(nota);
        }
    }

    /// ¿Puede el agente cambiar este elemento entero?
    fn puede_entero(&self, q: Quien, e: &Elemento) -> Result<(), Omitida> {
        let actual = || Some(json!({ "x": e.x, "y": e.y, "w": e.w, "h": e.h, "datos": e.datos }));
        match q {
            Quien::Usuario => Ok(()),
            Quien::Orden | Quien::Agente { .. } if self.esta_en_uso(&e.id) => Err(Omitida {
                motivo: "El usuario lo está usando ahora mismo.".into(),
                actual: actual(),
            }),
            Quien::Orden => Ok(()),
            Quien::Agente { capa, del_usuario } => {
                if e.capa == CAPA_USUARIO {
                    return if del_usuario {
                        Ok(())
                    } else {
                        Err(Omitida {
                            motivo: "Es del usuario. Solo con --del-usuario, y solo si te lo pidió en la conversación.".into(),
                            actual: actual(),
                        })
                    };
                }
                if e.capa != capa {
                    return Err(Omitida {
                        motivo: "Es de otra capa.".into(),
                        actual: None,
                    });
                }
                if e.tocado {
                    return Err(Omitida {
                        motivo: "Lo cambió el usuario: déjalo como está.".into(),
                        actual: actual(),
                    });
                }
                Ok(())
            }
        }
    }

    /// ¿Puede el agente trabajar dentro de este elemento (una tabla, un tablero)? No mira si está
    /// tocado: eso va por partes.
    pub(crate) fn puede_dentro(&self, q: Quien, e: &Elemento) -> Result<(), Omitida> {
        match q {
            Quien::Usuario => Ok(()),
            Quien::Orden => omitida("Una orden de ordenar solo mueve."),
            Quien::Agente { capa, del_usuario } => {
                if e.capa == CAPA_USUARIO && !del_usuario {
                    return omitida(
                        "Es del usuario. Solo con --del-usuario, y solo si te lo pidió en la conversación.",
                    );
                }
                if e.capa != CAPA_USUARIO && e.capa != capa {
                    return omitida("Es de otra capa.");
                }
                Ok(())
            }
        }
    }

    /// El elemento por su id o por la `ref` que le puso este lote.
    pub(crate) fn resuelve(&self, lote: &Lote, id: &str) -> Result<String, Omitida> {
        let real = lote.refs.get(id).map_or(id, String::as_str);
        if self.elemento(real).is_some() {
            Ok(real.to_string())
        } else {
            omitida(format!("«{id}» ya no existe."))
        }
    }

    /// Las puntas de una flecha, con cada `ref` cambiada por su id; una punta que no existe la
    /// invalida (el usuario pudo borrar ese elemento hace un momento).
    fn puntas_reales(&self, lote: &Lote, datos: &mut Value) -> Result<(), Omitida> {
        for lado in ["desde", "hasta"] {
            let pedido = datos[lado]
                .get("id")
                .and_then(Value::as_str)
                .map(String::from);
            if let Some(pedido) = pedido {
                let real = self.resuelve(lote, &pedido).map_err(|_| Omitida {
                    motivo: format!("La flecha apunta a «{pedido}», que ya no existe."),
                    actual: None,
                })?;
                datos[lado]["id"] = json!(real);
            }
        }
        Ok(())
    }

    /// Aplica un lote. Cada operación es independiente: lo que no se puede se apunta en
    /// `omitidas` y se sigue con la siguiente.
    pub fn aplicar(&mut self, q: Quien, ops: &[Value], ahora: u64) -> Resultado {
        let mut r = Resultado::default();
        if ops.len() > MAX_LOTE {
            r.omitidas.push(json!({
                "motivo": format!("El lote trae {} operaciones y el tope es {MAX_LOTE}: mándalo por tandas.", ops.len())
            }));
            r.rev = self.rev;
            return r;
        }
        let mut lote = Lote::default();
        if let Quien::Agente { capa, .. } = q {
            match self.capa(capa) {
                Some(c) if c.autor == Autor::Nexo => {}
                Some(_) => {
                    r.omitidas
                        .push(json!({ "motivo": "Esa capa es del usuario: dibuja en la tuya." }));
                    r.rev = self.rev;
                    return r;
                }
                None => {
                    r.omitidas.push(json!({ "motivo": "Esa capa ya no existe: el usuario la integró o la borró." }));
                    r.rev = self.rev;
                    return r;
                }
            }
            // Volver a dibujar en una capa la vuelve a abrir.
            let c = self.capas.iter_mut().find(|c| c.id == capa);
            if let Some(c) = c
                && c.estado != Some(EstadoCapa::Dibujando)
            {
                c.estado = Some(EstadoCapa::Dibujando);
                lote.capas.insert(capa.to_string());
            }
            self.actividad.insert(capa.to_string(), ahora);
            r.usuario_cambio = self.anotados.remove(capa).unwrap_or_default();
        }
        // Lo del agente nunca cae encima de lo ajeno, diga lo que diga: no se fía de que pida zona.
        let colocados = match q {
            Quien::Agente { capa, .. } => self.sin_pisar(capa, ops),
            _ => None,
        };
        if let Some((_, delta)) = &colocados {
            r.desplazado = Some(*delta);
        }
        let ops: &[Value] = colocados.as_ref().map_or(ops, |(l, _)| l.as_slice());
        for raw in ops {
            let etiqueta = (
                raw.get("ref").cloned(),
                raw.get("id").cloned(),
                raw.get("op").cloned(),
            );
            let hecho = match serde_json::from_value::<Op>(raw.clone()) {
                Ok(op) => self.una(q, op, &mut lote),
                Err(e) => omitida(format!("No se entiende la operación: {e}")),
            };
            match hecho {
                Ok(()) => r.aplicadas += 1,
                Err(o) => {
                    let mut salida = Map::new();
                    if let Some(v) = etiqueta.2 {
                        salida.insert("op".into(), v);
                    }
                    if let Some(v) = etiqueta.0 {
                        salida.insert("ref".into(), v);
                    }
                    if let Some(v) = etiqueta.1 {
                        salida.insert("id".into(), v);
                    }
                    salida.insert("motivo".into(), o.motivo.into());
                    if let Some(a) = o.actual {
                        salida.insert("actual".into(), a);
                    }
                    r.omitidas.push(Value::Object(salida));
                }
            }
        }
        let cambio = !lote.puestos.is_empty()
            || !lote.quitados.is_empty()
            || !lote.capas.is_empty()
            || !lote.capas_quitadas.is_empty();
        if cambio {
            self.rev += 1;
            let rev = self.rev;
            for e in &mut self.elementos {
                if lote.puestos.contains(&e.id) {
                    e.rev = rev;
                }
            }
            self.quitados
                .extend(lote.quitados.iter().map(|id| (rev, id.clone())));
            if self.quitados.len() > 2000 {
                self.quitados.drain(..1000);
            }
        }
        r.rev = self.rev;
        r.hechas = lote.hechas;
        r.inversas = lote.inversas;
        r.cambios = Cambios {
            poner: self
                .elementos
                .iter()
                .filter(|e| lote.puestos.contains(&e.id))
                .cloned()
                .collect(),
            quitar: lote.quitados.into_iter().collect(),
            capas: self
                .capas
                .iter()
                .filter(|c| lote.capas.contains(&c.id))
                .cloned()
                .collect(),
            capas_quitadas: lote.capas_quitadas.into_iter().collect(),
        };
        r
    }

    /// Lo que ocupa lo de los demás, visto desde una capa: sus elementos y las cartas de notas.
    fn ajeno(&self, capa: &str) -> Vec<Rect> {
        let mut out: Vec<Rect> = self
            .elementos
            .iter()
            .filter(|e| e.capa != capa && e.tipo != "flecha")
            .map(rect_propio)
            .collect();
        out.extend(self.cartas.iter().map(|(_, r)| *r));
        out
    }

    /// Si lo que crea un lote con coordenadas cae encima de algo ajeno, lo mueve entero a un hueco:
    /// así conserva su forma. Da el lote nuevo y cuánto se movió; `None` si no hace falta.
    fn sin_pisar(&self, capa: &str, ops: &[Value]) -> Option<(Vec<Value>, (f64, f64))> {
        let mut cajas = Vec::new();
        for op in ops {
            if op["op"] != "crear" || op["tipo"] == "flecha" {
                continue;
            }
            let (Some(x), Some(y)) = (op["x"].as_f64(), op["y"].as_f64()) else {
                continue;
            };
            let tipo = op["tipo"].as_str().unwrap_or_default();
            let d = datos::normalizar(tipo, &op["datos"]).ok()?;
            let (dw, dh) = datos::tamano_de_partida(tipo, &d);
            cajas.push(Rect {
                x,
                y,
                w: op["w"].as_f64().unwrap_or(dw),
                h: op["h"].as_f64().unwrap_or(dh),
            });
        }
        let todo = zona::abarca(&cajas)?;
        let ajeno = self.ajeno(capa);
        if !cajas.iter().any(|c| ajeno.iter().any(|a| c.toca(a, 0.0))) {
            return None;
        }
        let z = self.zona_libre(todo.w, todo.h, None);
        let delta = (z.x - todo.x, z.y - todo.y);
        let nuevo = ops
            .iter()
            .map(|op| {
                let mut op = op.clone();
                if op["op"] == "crear" && op["x"].is_number() && op["y"].is_number() {
                    op["x"] = json!(op["x"].as_f64().unwrap_or(0.0) + delta.0);
                    op["y"] = json!(op["y"].as_f64().unwrap_or(0.0) + delta.1);
                }
                op
            })
            .collect();
        Some((nuevo, delta))
    }

    fn una(&mut self, q: Quien, op: Op, lote: &mut Lote) -> Result<(), Omitida> {
        match op {
            Op::Crear {
                referencia,
                tipo,
                capa,
                x,
                y,
                w,
                h,
                z,
                datos,
            } => self.crear(q, lote, referencia, tipo, capa, (x, y, w, h, z), datos),
            Op::Cambiar {
                id,
                x,
                y,
                w,
                h,
                z,
                datos,
            } => self.cambiar(q, lote, &id, (x, y, w, h, z), datos),
            Op::Borrar { id } => self.borrar(q, lote, &id),
            Op::Apilar { id, a } => {
                let id = self.resuelve(lote, &id)?;
                let (capa, antes) = {
                    let e = self.elemento(&id).ok_or(Omitida {
                        motivo: "Ya no existe.".into(),
                        actual: None,
                    })?;
                    self.puede_entero(q, e)?;
                    (e.capa.clone(), e.z)
                };
                let z = match a.as_str() {
                    "frente" => self.al_frente(&capa),
                    "fondo" => self.al_fondo(&capa),
                    _ => return omitida("«a» vale frente o fondo."),
                };
                if let Some(e) = self.elemento_mut(&id) {
                    e.z = z;
                }
                self.marcar_usuario(q, &id, "cambió su orden de apilado");
                lote.puestos.insert(id.clone());
                lote.inversas
                    .insert(0, json!({ "op": "cambiar", "id": id, "z": antes }));
                Ok(())
            }
            Op::Poner { elemento } => {
                if q != Quien::Usuario {
                    return omitida("Eso solo lo hace deshacer.");
                }
                let id = elemento.id.clone();
                if !self.capas.iter().any(|c| c.id == elemento.capa) {
                    return omitida("Su capa ya no existe.");
                }
                let inversa = match self.elementos.iter().position(|e| e.id == id) {
                    Some(i) => {
                        let antes = std::mem::replace(&mut self.elementos[i], *elemento);
                        json!({ "op": "poner", "elemento": antes })
                    }
                    None => {
                        self.elementos.push(*elemento);
                        json!({ "op": "borrar", "id": id })
                    }
                };
                lote.quitados.remove(&id);
                lote.puestos.insert(id);
                lote.inversas.insert(0, inversa);
                Ok(())
            }
            Op::CapaVer { capa, visible } => {
                self.solo_usuario(q)?;
                let c = self.capas.iter_mut().find(|c| c.id == capa);
                let Some(c) = c else {
                    return omitida("Esa capa ya no existe.");
                };
                let antes = c.visible;
                c.visible = visible;
                lote.capas.insert(capa.clone());
                lote.inversas.insert(
                    0,
                    json!({ "op": "capa-ver", "capa": capa, "visible": antes }),
                );
                Ok(())
            }
            Op::CapaRenombrar { capa, nombre } => {
                self.solo_usuario(q)?;
                let limpio: String = nombre.split_whitespace().collect::<Vec<_>>().join(" ");
                if limpio.is_empty() {
                    return omitida("Falta el nombre.");
                }
                let c = self.capas.iter_mut().find(|c| c.id == capa);
                let Some(c) = c else {
                    return omitida("Esa capa ya no existe.");
                };
                let antes = std::mem::replace(
                    &mut c.nombre,
                    limpio.chars().take(datos::MAX_TITULO).collect(),
                );
                lote.capas.insert(capa.clone());
                lote.inversas.insert(
                    0,
                    json!({ "op": "capa-renombrar", "capa": capa, "nombre": antes }),
                );
                Ok(())
            }
            Op::CapaIntegrar { capa } => self.capa_integrar(q, lote, &capa),
            Op::CapaBorrar { capa } => self.capa_borrar(q, lote, &capa),
            Op::CapaPoner {
                capa,
                elementos,
                integrada,
            } => {
                self.solo_usuario(q)?;
                let id = capa.id.clone();
                if id == CAPA_USUARIO {
                    return omitida("La capa del usuario siempre está.");
                }
                if !self.capas.iter().any(|c| c.id == id) {
                    self.capas.push(*capa);
                }
                lote.capas_quitadas.remove(&id);
                lote.capas.insert(id.clone());
                let mut ids = Vec::new();
                for e in elementos {
                    ids.push(e.id.clone());
                    lote.quitados.remove(&e.id);
                    lote.puestos.insert(e.id.clone());
                    match self.elementos.iter().position(|o| o.id == e.id) {
                        Some(i) => self.elementos[i] = e,
                        None => self.elementos.push(e),
                    }
                }
                let rehacer = if integrada {
                    "capa-integrar"
                } else {
                    "capa-borrar"
                };
                lote.inversas
                    .insert(0, json!({ "op": rehacer, "capa": id }));
                Ok(())
            }
            Op::Parte(parte) => compuestos::aplicar(self, q, parte, lote),
        }
    }

    fn solo_usuario(&self, q: Quien) -> Result<(), Omitida> {
        if q == Quien::Usuario {
            Ok(())
        } else {
            omitida("Las capas las gestiona el usuario.")
        }
    }

    fn marcar_usuario(&mut self, q: Quien, id: &str, que: &str) {
        if q != Quien::Usuario {
            return;
        }
        let Some(e) = self.elemento_mut(id) else {
            return;
        };
        let capa = e.capa.clone();
        if capa != CAPA_USUARIO {
            e.tocado = true;
            self.anotar(&capa, json!({ "id": id, "que": que }));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn crear(
        &mut self,
        q: Quien,
        lote: &mut Lote,
        referencia: Option<String>,
        tipo: String,
        capa: Option<String>,
        (x, y, w, h, z): Sitio,
        datos: Value,
    ) -> Result<(), Omitida> {
        if self.elementos.len() >= MAX_ELEMENTOS {
            return omitida(format!(
                "El lienzo ya tiene {MAX_ELEMENTOS} elementos, el tope por escritorio."
            ));
        }
        let capa = match q {
            Quien::Usuario => capa.unwrap_or_else(|| CAPA_USUARIO.to_string()),
            Quien::Agente { capa, .. } => capa.to_string(),
            Quien::Orden => return omitida("Una orden de ordenar solo mueve."),
        };
        if !self.capas.iter().any(|c| c.id == capa) {
            return omitida("Esa capa ya no existe.");
        }
        let mut datos = datos::normalizar(&tipo, &datos).map_err(|motivo| Omitida {
            motivo,
            actual: None,
        })?;
        if tipo == "flecha" {
            self.puntas_reales(lote, &mut datos)?;
        }
        let (dw, dh) = datos::tamano_de_partida(&tipo, &datos);
        let (w, h) = if tipo == "flecha" {
            (0.0, 0.0)
        } else {
            (
                sujeta(w.unwrap_or(dw), 22.0, 12000.0),
                sujeta(h.unwrap_or(dh), 22.0, 12000.0),
            )
        };
        // Sin coordenadas, el motor le busca sitio: junto a lo último que puso este lote o en una
        // zona libre. Nunca encima de nada.
        let (x, y) = match (x, y) {
            _ if tipo == "flecha" => (0.0, 0.0),
            (Some(x), Some(y)) => (x, y),
            _ => {
                let cerca = lote
                    .hechas
                    .last()
                    .and_then(|h| h["id"].as_str())
                    .map(String::from);
                let z = self.zona_libre(w, h, cerca.as_deref());
                (z.x, z.y)
            }
        };
        let (x, y) = (
            sujeta(x, -MUNDO.0 / 2.0, MUNDO.0 / 2.0 - w),
            sujeta(y, -MUNDO.1 / 2.0, MUNDO.1 / 2.0 - h),
        );
        // Un marco va detrás de lo que tenga encima.
        let z = z.unwrap_or_else(|| {
            if tipo == "marco" {
                self.al_fondo(&capa)
            } else {
                self.al_frente(&capa)
            }
        });
        self.proximo += 1;
        let id = format!("e{}", self.proximo);
        let e = Elemento {
            id: id.clone(),
            capa,
            tipo,
            x: redondo(x),
            y: redondo(y),
            w: redondo(w),
            h: redondo(h),
            z,
            tocado: false,
            tocadas: Vec::new(),
            rev: 0,
            datos,
        };
        self.elementos.push(e);
        lote.puestos.insert(id.clone());
        let mut hecha = json!({ "id": id });
        if let Some(r) = referencia {
            lote.refs.insert(r.clone(), id.clone());
            hecha["ref"] = json!(r);
        }
        lote.hechas.push(hecha);
        lote.inversas.insert(0, json!({ "op": "borrar", "id": id }));
        Ok(())
    }

    fn cambiar(
        &mut self,
        q: Quien,
        lote: &mut Lote,
        id: &str,
        (x, y, w, h, z): Sitio,
        parche: Option<Value>,
    ) -> Result<(), Omitida> {
        let id = self.resuelve(lote, id)?;
        let e = self.elemento(&id).cloned().ok_or(Omitida {
            motivo: "Ya no existe.".into(),
            actual: None,
        })?;
        self.puede_entero(q, &e)?;
        if q == Quien::Orden && parche.is_some() {
            return omitida("Una orden de ordenar solo mueve.");
        }
        let mut inversa = Map::new();
        inversa.insert("op".into(), "cambiar".into());
        inversa.insert("id".into(), id.clone().into());
        let mut nuevo = e.clone();
        if let Some(parche) = &parche {
            let Some(campos) = parche.as_object() else {
                return omitida("«datos» tiene que ser un objeto.");
            };
            if let Some(clave) = campos
                .keys()
                .find(|k| datos::por_partes(&e.tipo).contains(&k.as_str()))
            {
                return omitida(format!(
                    "«{clave}» no se cambia de golpe: usa las operaciones de celdas, filas, columnas o tarjetas."
                ));
            }
            let mut mezcla = e.datos.clone();
            let mut antes = Map::new();
            for (k, v) in campos {
                antes.insert(k.clone(), e.datos.get(k).cloned().unwrap_or(Value::Null));
                if v.is_null() {
                    mezcla.as_object_mut().map(|m| m.remove(k));
                } else {
                    mezcla[k] = v.clone();
                }
            }
            let mut limpio = datos::normalizar(&e.tipo, &mezcla).map_err(|motivo| Omitida {
                motivo,
                actual: None,
            })?;
            if e.tipo == "flecha" {
                self.puntas_reales(lote, &mut limpio)?;
            }
            nuevo.datos = limpio;
            inversa.insert("datos".into(), Value::Object(antes));
        }
        let geometria = x.is_some() || y.is_some() || w.is_some() || h.is_some() || z.is_some();
        if geometria && e.tipo != "flecha" {
            for (clave, antes) in [("x", e.x), ("y", e.y), ("w", e.w), ("h", e.h)] {
                let pedido = match clave {
                    "x" => x,
                    "y" => y,
                    "w" => w,
                    _ => h,
                };
                if pedido.is_some() {
                    inversa.insert(clave.into(), json!(antes));
                }
            }
            nuevo.w = redondo(sujeta(w.unwrap_or(e.w), 22.0, 12000.0));
            nuevo.h = redondo(sujeta(h.unwrap_or(e.h), 22.0, 12000.0));
            nuevo.x = redondo(sujeta(
                x.unwrap_or(e.x),
                -MUNDO.0 / 2.0,
                MUNDO.0 / 2.0 - nuevo.w,
            ));
            nuevo.y = redondo(sujeta(
                y.unwrap_or(e.y),
                -MUNDO.1 / 2.0,
                MUNDO.1 / 2.0 - nuevo.h,
            ));
        }
        if let Some(z) = z {
            inversa.insert("z".into(), json!(e.z));
            nuevo.z = z;
        }
        if nuevo == e {
            return Ok(());
        }
        // Mover lo suyo tampoco le deja caer encima de lo ajeno.
        if let Quien::Agente { capa, .. } = q
            && e.capa == capa
            && e.tipo != "flecha"
            && (nuevo.x, nuevo.y, nuevo.w, nuevo.h) != (e.x, e.y, e.w, e.h)
        {
            let r = rect_propio(&nuevo);
            if self.ajeno(capa).iter().any(|a| r.toca(a, 0.0)) {
                return omitida("Caería encima de algo del usuario: pide una zona.");
            }
        }
        if let Some(sitio) = self.elemento_mut(&id) {
            *sitio = nuevo;
        }
        let que = match (parche.is_some(), geometria) {
            (true, _) => "cambió su contenido",
            _ => "lo movió o cambió de tamaño",
        };
        self.marcar_usuario(q, &id, que);
        lote.puestos.insert(id);
        lote.inversas.insert(0, Value::Object(inversa));
        Ok(())
    }

    /// Una flecha que apuntaba a lo borrado no desaparece: esa punta queda suelta donde estaba.
    fn soltar_flechas(&mut self, lote: &mut Lote, borrados: &BTreeSet<String>) -> Vec<Value> {
        let mut inversas = Vec::new();
        let foto = self.elementos.clone();
        for e in &mut self.elementos {
            if e.tipo != "flecha" || borrados.contains(&e.id) {
                continue;
            }
            let antes = e.datos.clone();
            for (lado, otro) in [("desde", "hasta"), ("hasta", "desde")] {
                let suelta = antes[lado]
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| borrados.contains(id));
                if suelta {
                    let hacia = centro_de(&antes[otro], &foto);
                    let (x, y) = punto_de(&antes[lado], &foto, hacia).unwrap_or((0.0, 0.0));
                    e.datos[lado] = json!({ "x": x, "y": y });
                }
            }
            if e.datos != antes {
                lote.puestos.insert(e.id.clone());
                inversas.push(json!({ "op": "cambiar", "id": e.id, "datos": { "desde": antes["desde"], "hasta": antes["hasta"] } }));
            }
        }
        inversas
    }

    fn borrar(&mut self, q: Quien, lote: &mut Lote, id: &str) -> Result<(), Omitida> {
        let id = self.resuelve(lote, id)?;
        let e = self.elemento(&id).cloned().ok_or(Omitida {
            motivo: "Ya no existe.".into(),
            actual: None,
        })?;
        self.puede_entero(q, &e)?;
        if q == Quien::Orden {
            return omitida("Una orden de ordenar solo mueve.");
        }
        if q != Quien::Usuario && self.algo_en_uso(&id) {
            return omitida("El usuario lo está usando ahora mismo.");
        }
        let solo: BTreeSet<String> = [id.clone()].into();
        let sueltas = self.soltar_flechas(lote, &solo);
        self.elementos.retain(|o| o.id != id);
        if q == Quien::Usuario {
            self.anotar(&e.capa.clone(), json!({ "id": id, "que": "lo borró" }));
        }
        lote.puestos.remove(&id);
        lote.quitados.insert(id);
        for inv in sueltas {
            lote.inversas.insert(0, inv);
        }
        lote.inversas
            .insert(0, json!({ "op": "poner", "elemento": e }));
        Ok(())
    }

    fn capa_integrar(&mut self, q: Quien, lote: &mut Lote, capa: &str) -> Result<(), Omitida> {
        self.solo_usuario(q)?;
        if capa == CAPA_USUARIO {
            return omitida("Ya es la capa del usuario.");
        }
        let Some(pos) = self.capas.iter().position(|c| c.id == capa) else {
            return omitida("Esa capa ya no existe.");
        };
        let c = self.capas.remove(pos);
        let fotos: Vec<Elemento> = self
            .elementos
            .iter()
            .filter(|e| e.capa == capa)
            .cloned()
            .collect();
        // Lo que estaba delante de las notas sigue delante, por encima de lo del usuario; lo de
        // detrás, detrás.
        let base = self
            .elementos
            .iter()
            .filter(|e| e.capa == CAPA_USUARIO)
            .map(|e| e.z)
            .max()
            .unwrap_or(0)
            .max(0);
        for e in &mut self.elementos {
            if e.capa == capa {
                e.capa = CAPA_USUARIO.into();
                if e.z > 0 {
                    e.z += base;
                }
                e.tocado = false;
                e.tocadas.clear();
                lote.puestos.insert(e.id.clone());
            }
        }
        self.actividad.remove(capa);
        self.anotados.remove(capa);
        lote.capas_quitadas.insert(capa.to_string());
        lote.inversas.insert(
            0,
            json!({ "op": "capa-poner", "capa": c, "elementos": fotos, "integrada": true }),
        );
        Ok(())
    }

    fn capa_borrar(&mut self, q: Quien, lote: &mut Lote, capa: &str) -> Result<(), Omitida> {
        self.solo_usuario(q)?;
        if capa == CAPA_USUARIO {
            return omitida("La capa del usuario no se puede borrar.");
        }
        let Some(pos) = self.capas.iter().position(|c| c.id == capa) else {
            return omitida("Esa capa ya no existe.");
        };
        let c = self.capas.remove(pos);
        let fotos: Vec<Elemento> = self
            .elementos
            .iter()
            .filter(|e| e.capa == capa)
            .cloned()
            .collect();
        let borrados: BTreeSet<String> = fotos.iter().map(|e| e.id.clone()).collect();
        let sueltas = self.soltar_flechas(lote, &borrados);
        self.elementos.retain(|e| e.capa != capa);
        for id in &borrados {
            lote.puestos.remove(id);
            lote.quitados.insert(id.clone());
        }
        self.actividad.remove(capa);
        self.anotados.remove(capa);
        lote.capas_quitadas.insert(capa.to_string());
        for inv in sueltas {
            lote.inversas.insert(0, inv);
        }
        lote.inversas.insert(
            0,
            json!({ "op": "capa-poner", "capa": c, "elementos": fotos }),
        );
        Ok(())
    }

    /// Lo que hay, para el agente. `capa` filtra, `desde` da solo lo cambiado tras esa revisión.
    pub fn leer(&self, capa: Option<&str>, desde: Option<u64>) -> Value {
        let elementos: Vec<&Elemento> = self
            .elementos
            .iter()
            .filter(|e| capa.is_none_or(|c| e.capa == c))
            .filter(|e| desde.is_none_or(|d| e.rev > d))
            .collect();
        let quitados: Vec<&String> = desde.map_or(Vec::new(), |d| {
            self.quitados
                .iter()
                .filter(|(r, _)| *r > d)
                .map(|(_, id)| id)
                .collect()
        });
        json!({
            "rev": self.rev,
            "capas": self.resumen_capas(),
            "elementos": elementos,
            "quitados": quitados,
            "notas": self.cartas.iter().map(|(path, r)| json!({ "ruta": path, "x": r.x, "y": r.y, "w": r.w, "h": r.h })).collect::<Vec<_>>(),
        })
    }

    pub fn resumen_capas(&self) -> Vec<Value> {
        self.capas
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "nombre": c.nombre,
                    "autor": c.autor,
                    "visible": c.visible,
                    "estado": c.estado,
                    "elementos": self.elementos.iter().filter(|e| e.capa == c.id).count(),
                })
            })
            .collect()
    }

    /// Lo mismo que `leer`, en texto corto: gasta muchos menos tokens que el JSON.
    pub fn leer_texto(&self, capa: Option<&str>, desde: Option<u64>) -> String {
        let mut salida = format!("rev {}\n", self.rev);
        for c in &self.capas {
            let estado = match c.estado {
                Some(EstadoCapa::Dibujando) => " · dibujando",
                Some(EstadoCapa::Terminada) => " · terminada",
                Some(EstadoCapa::SinTerminar) => " · sin terminar",
                None => "",
            };
            salida.push_str(&format!("capa {} «{}»{estado}\n", c.id, c.nombre));
        }
        for (path, r) in &self.cartas {
            if capa.is_none() && desde.is_none() {
                salida.push_str(&format!("nota {path} @{},{} {}×{}\n", r.x, r.y, r.w, r.h));
            }
        }
        for e in &self.elementos {
            if capa.is_some_and(|c| e.capa != c) || desde.is_some_and(|d| e.rev <= d) {
                continue;
            }
            salida.push_str(&self.linea(e));
            salida.push('\n');
        }
        if let Some(d) = desde {
            for (r, id) in &self.quitados {
                if *r > d {
                    salida.push_str(&format!("quitado {id}\n"));
                }
            }
        }
        salida
    }

    fn linea(&self, e: &Elemento) -> String {
        let d = &e.datos;
        let s = |k: &str| d[k].as_str().unwrap_or("");
        let marca = if e.tocado { " [tocado]" } else { "" };
        let sitio = format!("@{},{} {}×{}", e.x, e.y, e.w, e.h);
        let punta = |fin: &Value| match fin.get("id").and_then(Value::as_str) {
            Some(id) => id.to_string(),
            None => format!("({},{})", fin["x"], fin["y"]),
        };
        match e.tipo.as_str() {
            "flecha" => format!(
                "{} flecha {} → {}{}{marca}",
                e.id,
                punta(&d["desde"]),
                punta(&d["hasta"]),
                if s("texto").is_empty() {
                    String::new()
                } else {
                    format!(" «{}»", s("texto"))
                },
            ),
            "figura" => format!(
                "{} figura/{} «{}» {sitio}{marca}",
                e.id,
                s("forma"),
                s("texto")
            ),
            "texto" | "adhesiva" => format!("{} {} «{}» {sitio}{marca}", e.id, e.tipo, s("texto")),
            "marco" => format!("{} marco «{}» {sitio}{marca}", e.id, s("titulo")),
            "tabla" => {
                let cols = d["columnas"].as_array().cloned().unwrap_or_default();
                let mut t = format!("{} tabla «{}» {sitio}{marca}\n", e.id, s("titulo"));
                t.push_str("  ");
                t.push_str(
                    &cols
                        .iter()
                        .map(|c| {
                            format!(
                                "{}={}",
                                c["id"].as_str().unwrap_or(""),
                                c["titulo"].as_str().unwrap_or("")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" | "),
                );
                for f in d["filas"].as_array().into_iter().flatten() {
                    t.push_str(&format!("\n  {}: ", f["id"].as_str().unwrap_or("")));
                    t.push_str(
                        &cols
                            .iter()
                            .map(|c| {
                                f["celdas"][c["id"].as_str().unwrap_or("")]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string()
                            })
                            .collect::<Vec<_>>()
                            .join(" | "),
                    );
                }
                t
            }
            "tablero" => {
                let mut t = format!("{} tablero «{}» {sitio}{marca}", e.id, s("titulo"));
                for c in d["columnas"].as_array().into_iter().flatten() {
                    t.push_str(&format!(
                        "\n  {} «{}»:",
                        c["id"].as_str().unwrap_or(""),
                        c["titulo"].as_str().unwrap_or("")
                    ));
                    for x in d["tarjetas"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|x| x["col"] == c["id"])
                    {
                        t.push_str(&format!(
                            " [{} {}]",
                            x["id"].as_str().unwrap_or(""),
                            x["titulo"].as_str().unwrap_or("")
                        ));
                    }
                }
                t
            }
            "entidad" => {
                let cols: Vec<String> = d["columnas"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| {
                        format!(
                            "{} {}{}{}",
                            c["nombre"].as_str().unwrap_or(""),
                            c["tipo"].as_str().unwrap_or(""),
                            if c["pk"] == true { " PK" } else { "" },
                            if c["fk"] == true { " FK" } else { "" }
                        )
                    })
                    .collect();
                format!(
                    "{} entidad «{}» {sitio}{marca}: {}",
                    e.id,
                    s("nombre"),
                    cols.join(", ")
                )
            }
            otro => {
                let mut t = serde_json::to_string(d).unwrap_or_default();
                if t.len() > 400 {
                    let corte = (0..=400)
                        .rev()
                        .find(|i| t.is_char_boundary(*i))
                        .unwrap_or(0);
                    t.truncate(corte);
                    t.push('…');
                }
                format!("{} {otro} {sitio}{marca} {t}", e.id)
            }
        }
    }
}
