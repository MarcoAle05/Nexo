//! Lo que se construye sobre las operaciones: ordenar lo que hay y el diagrama declarativo. Los dos
//! acaban en un lote normal, así que siguen las mismas reglas que cualquier otro cambio.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Value, json};

use super::datos::{self, caja_de_texto};
use super::trazado::{Caja, por_niveles};
use super::zona::{Rect, abarca, reticula_x, reticula_y};
use super::{Lienzo, Quien, Resultado, rect_propio};

const SEPARACION: f64 = 44.0;
const SEP_FLUJO: (f64, f64) = (88.0, 44.0);
const MARGEN_MARCO: f64 = 44.0;
const CABECERA_MARCO: f64 = 44.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MovCarta {
    pub path: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct Orden {
    pub resultado: Resultado,
    /// Las cartas de notas no están aquí: las mueve la interfaz, que es quien las guarda.
    pub cartas: Vec<MovCarta>,
}

enum Pieza {
    Elemento(String),
    Carta(String),
}

/// En fila de través al flujo: en un diagrama hacia la derecha, en columna.
fn apilados(cajas: &[Caja], derecha: bool, sep: f64) -> super::trazado::Plano {
    let mut avance = 0.0;
    let pos: Vec<(f64, f64)> = cajas
        .iter()
        .map(|c| {
            let p = if derecha {
                (0.0, avance)
            } else {
                (avance, 0.0)
            };
            avance += if derecha { c.h } else { c.w } + sep;
            p
        })
        .collect();
    let w = pos
        .iter()
        .zip(cajas)
        .map(|(p, c)| p.0 + c.w)
        .fold(0.0, f64::max);
    let h = pos
        .iter()
        .zip(cajas)
        .map(|(p, c)| p.1 + c.h)
        .fold(0.0, f64::max);
    super::trazado::Plano { pos, w, h }
}

impl Lienzo {
    /// Ordena sin pisar nada: `fila`, `rejilla` o `flujo` (sigue las flechas). Por defecto, todo
    /// lo que hay; `ids` o `capa` lo acotan y `con_cartas` incluye las notas. Lo que el usuario
    /// esté usando se queda quieto. Un marco lleva consigo lo que tiene dentro.
    pub fn ordenar(
        &mut self,
        modo: &str,
        ids: Option<&[String]>,
        capa: Option<&str>,
        con_cartas: bool,
        ahora: u64,
    ) -> Result<Orden, String> {
        if !["fila", "rejilla", "flujo"].contains(&modo) {
            return Err("El modo vale fila, rejilla o flujo.".into());
        }
        let quiere = |id: &str, e_capa: &str| {
            ids.is_none_or(|l| l.iter().any(|i| i == id)) && capa.is_none_or(|c| c == e_capa)
        };
        let elegidos: Vec<_> = self
            .elementos
            .iter()
            .filter(|e| e.tipo != "flecha" && quiere(&e.id, &e.capa) && !self.esta_en_uso(&e.id))
            .cloned()
            .collect();
        // Lo que está dentro de un marco elegido va con él: no se ordena por su cuenta.
        let marcos: Vec<_> = elegidos
            .iter()
            .filter(|e| e.tipo == "marco")
            .cloned()
            .collect();
        let suelto = |r: &Rect, propio: &str| {
            !marcos
                .iter()
                .any(|m| m.id != propio && r.dentro_de(&rect_propio(m)))
        };
        let mut piezas: Vec<(Pieza, Rect)> = elegidos
            .iter()
            .filter(|e| suelto(&rect_propio(e), &e.id))
            .map(|e| (Pieza::Elemento(e.id.clone()), rect_propio(e)))
            .collect();
        if con_cartas {
            for (path, r) in &self.cartas {
                if suelto(r, "") {
                    piezas.push((Pieza::Carta(path.clone()), *r));
                }
            }
        }
        if piezas.len() < 2 {
            return Ok(Orden {
                resultado: self.aplicar(Quien::Orden, &[], ahora),
                cartas: Vec::new(),
            });
        }
        // Se empieza donde ya estaba lo que se ordena, de izquierda a derecha y de arriba abajo.
        piezas.sort_by(|a, b| {
            let clave = |r: &Rect| {
                if modo == "fila" {
                    (r.x, r.y)
                } else {
                    (r.y, r.x)
                }
            };
            let (ka, kb) = (clave(&a.1), clave(&b.1));
            ka.0.total_cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
        });
        let rects: Vec<Rect> = piezas.iter().map(|(_, r)| *r).collect();
        let origen = abarca(&rects).unwrap_or(Rect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        });
        let cajas: Vec<Caja> = rects.iter().map(|r| Caja { w: r.w, h: r.h }).collect();
        let posiciones: Vec<(f64, f64)> = match modo {
            "fila" => {
                let mut x = 0.0;
                cajas
                    .iter()
                    .map(|c| {
                        let p = (x, 0.0);
                        x += c.w + SEPARACION;
                        p
                    })
                    .collect()
            }
            "rejilla" => por_niveles(&cajas, &[], true, (SEPARACION, SEPARACION)).pos,
            _ => {
                let indice: BTreeMap<String, usize> = piezas
                    .iter()
                    .enumerate()
                    .filter_map(|(i, (p, _))| match p {
                        Pieza::Elemento(id) => Some((id.clone(), i)),
                        Pieza::Carta(_) => None,
                    })
                    .collect();
                let aristas: Vec<(usize, usize)> = self
                    .elementos
                    .iter()
                    .filter(|e| e.tipo == "flecha")
                    .filter_map(|e| {
                        let a = indice.get(e.datos["desde"]["id"].as_str()?)?;
                        let b = indice.get(e.datos["hasta"]["id"].as_str()?)?;
                        Some((*a, *b))
                    })
                    .collect();
                por_niveles(&cajas, &aristas, true, SEP_FLUJO).pos
            }
        };
        let mut lote = Vec::new();
        let mut cartas = Vec::new();
        for (((pieza, antes), (dx, dy)), _) in piezas.iter().zip(posiciones).zip(0..) {
            let (x, y) = (reticula_x(origen.x + dx), reticula_y(origen.y + dy));
            if x == antes.x && y == antes.y {
                continue;
            }
            match pieza {
                Pieza::Carta(path) => cartas.push(MovCarta {
                    path: path.clone(),
                    x,
                    y,
                }),
                Pieza::Elemento(id) => {
                    lote.push(json!({ "op": "cambiar", "id": id, "x": x, "y": y }));
                    // Lo de dentro de un marco se mueve lo mismo que él.
                    if marcos.iter().any(|m| &m.id == id) {
                        let (mx, my) = (x - antes.x, y - antes.y);
                        for e in &elegidos {
                            if &e.id != id && rect_propio(e).dentro_de(antes) {
                                lote.push(json!({ "op": "cambiar", "id": e.id, "x": e.x + mx, "y": e.y + my }));
                            }
                        }
                        if con_cartas {
                            for (path, r) in &self.cartas {
                                if r.dentro_de(antes) {
                                    cartas.push(MovCarta {
                                        path: path.clone(),
                                        x: r.x + mx,
                                        y: r.y + my,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(Orden {
            resultado: self.aplicar(Quien::Orden, &lote, ahora),
            cartas,
        })
    }

    /// Un diagrama declarativo: nodos y aristas, sin coordenadas. Se coloca solo, en una zona
    /// libre (o en `en`), y acaba en elementos normales: no queda ningún objeto «diagrama» y el
    /// usuario puede tocar cada pieza.
    pub fn diagrama(&mut self, capa: &str, spec: &Value, ahora: u64) -> Result<Resultado, String> {
        let nodos = spec["nodos"]
            .as_array()
            .ok_or("Faltan los «nodos» del diagrama.")?;
        if nodos.is_empty() {
            return Err("El diagrama no tiene nodos.".into());
        }
        if nodos.len() > datos::MAX_LISTA {
            return Err(format!(
                "El diagrama pasa de {} nodos: pártelo en varios.",
                datos::MAX_LISTA
            ));
        }
        let vacio = Vec::new();
        let aristas_crudas = spec["aristas"].as_array().unwrap_or(&vacio);
        let grupos_crudos = spec["grupos"].as_array().unwrap_or(&vacio);
        let derecha = spec["direccion"].as_str() != Some("abajo");

        // Cada nodo: su ref, el tipo y lo que ocupa.
        let refs: Vec<String> = nodos
            .iter()
            .enumerate()
            .map(|(i, n)| {
                n["ref"]
                    .as_str()
                    .map_or_else(|| format!("n{i}"), String::from)
            })
            .collect();
        let indice: BTreeMap<&str, usize> = refs
            .iter()
            .enumerate()
            .map(|(i, r)| (r.as_str(), i))
            .collect();
        if indice.len() != refs.len() {
            return Err("Hay dos nodos con la misma «ref».".into());
        }
        let mut tipos = Vec::new();
        let mut cajas = Vec::new();
        let mut limpios = Vec::new();
        for n in nodos {
            let tipo = n["tipo"].as_str().unwrap_or("figura").to_string();
            if tipo == "flecha" || tipo == "marco" {
                return Err("Un nodo del diagrama no puede ser una flecha ni un marco.".into());
            }
            let crudo = n.get("datos").cloned().unwrap_or_else(|| {
                let mut d = n.clone();
                if let Some(m) = d.as_object_mut() {
                    for k in ["ref", "tipo", "grupo"] {
                        m.remove(k);
                    }
                }
                d
            });
            let d = datos::normalizar(&tipo, &crudo)?;
            let (w, h) = if tipo == "figura" {
                caja_de_texto(d["texto"].as_str().unwrap_or(""), d["forma"].as_str())
            } else {
                datos::tamano_de_partida(&tipo, &d)
            };
            cajas.push(Caja { w, h });
            limpios.push(d);
            tipos.push(tipo);
        }
        let mut aristas = Vec::new();
        for a in aristas_crudas {
            let de = a["de"].as_str().and_then(|r| indice.get(r)).copied();
            let hasta = a["a"].as_str().and_then(|r| indice.get(r)).copied();
            match (de, hasta) {
                (Some(de), Some(hasta)) => aristas.push((de, hasta)),
                _ => return Err(format!("Una arista nombra un nodo que no está: {a}")),
            }
        }

        // Los grupos se colocan por dentro y luego cuentan como una sola caja: así un marco de
        // grupo contiene justo a los suyos, nunca a otros.
        let grupo_de: Vec<Option<usize>> = nodos
            .iter()
            .map(|n| {
                let g = n["grupo"].as_str()?;
                grupos_crudos
                    .iter()
                    .position(|c| c["ref"].as_str() == Some(g) || c["titulo"].as_str() == Some(g))
            })
            .collect();
        let dentro: Vec<Vec<usize>> = (0..grupos_crudos.len())
            .map(|g| {
                (0..nodos.len())
                    .filter(|i| grupo_de[*i] == Some(g))
                    .collect()
            })
            .collect();
        let mut interior: Vec<Option<super::trazado::Plano>> = Vec::new();
        for miembros in &dentro {
            if miembros.is_empty() {
                interior.push(None);
                continue;
            }
            let c: Vec<Caja> = miembros.iter().map(|i| cajas[*i]).collect();
            let a: Vec<(usize, usize)> = aristas
                .iter()
                .filter_map(|(x, y)| {
                    Some((
                        miembros.iter().position(|m| m == x)?,
                        miembros.iter().position(|m| m == y)?,
                    ))
                })
                .collect();
            // Sin aristas dentro, unos sobre otros, de través al flujo: las flechas que llegan al
            // grupo no tienen que atravesar a nadie para alcanzar a los de detrás.
            interior.push(Some(if a.is_empty() {
                apilados(&c, derecha, SEPARACION)
            } else {
                por_niveles(&c, &a, derecha, SEP_FLUJO)
            }));
        }
        // Las cajas de arriba: un grupo con algo dentro o un nodo sin grupo.
        let mut arriba: Vec<Caja> = Vec::new();
        let mut caja_de_nodo = vec![usize::MAX; nodos.len()];
        let mut caja_de_grupo = vec![usize::MAX; grupos_crudos.len()];
        for (g, plano) in interior.iter().enumerate() {
            if let Some(p) = plano {
                caja_de_grupo[g] = arriba.len();
                for i in &dentro[g] {
                    caja_de_nodo[*i] = arriba.len();
                }
                arriba.push(Caja {
                    w: p.w + 2.0 * MARGEN_MARCO,
                    h: p.h + MARGEN_MARCO + CABECERA_MARCO,
                });
            }
        }
        for i in 0..nodos.len() {
            if grupo_de[i].is_none() {
                caja_de_nodo[i] = arriba.len();
                arriba.push(cajas[i]);
            }
        }
        let aristas_arriba: Vec<(usize, usize)> = aristas
            .iter()
            .map(|(a, b)| (caja_de_nodo[*a], caja_de_nodo[*b]))
            .filter(|(a, b)| a != b)
            .collect();
        let plano = por_niveles(&arriba, &aristas_arriba, derecha, (110.0, 66.0));

        let titulo = spec["titulo"].as_str().unwrap_or("").trim().to_string();
        let con_marco = !titulo.is_empty();
        let (extra_x, extra_y) = if con_marco {
            (MARGEN_MARCO, CABECERA_MARCO)
        } else {
            (0.0, 0.0)
        };
        let total_w = plano.w + 2.0 * extra_x;
        let total_h = plano.h + extra_y + if con_marco { MARGEN_MARCO } else { 0.0 };
        let zona = match (spec["en"]["x"].as_f64(), spec["en"]["y"].as_f64()) {
            (Some(x), Some(y)) => Rect {
                x: reticula_x(x),
                y: reticula_y(y),
                w: total_w,
                h: total_h,
            },
            _ => self.zona_libre(total_w, total_h, spec["cerca_de"].as_str()),
        };
        if zona.x + total_w > super::MUNDO.0 / 2.0 || zona.y + total_h > super::MUNDO.1 / 2.0 {
            return Err("El diagrama no cabe en el lienzo desde ese sitio.".into());
        }

        let (ox, oy) = (zona.x + extra_x, zona.y + extra_y);
        let mut lote = Vec::new();
        if con_marco {
            lote.push(json!({ "op": "crear", "tipo": "marco", "x": zona.x, "y": zona.y, "w": total_w, "h": total_h,
                "datos": { "titulo": titulo, "color": spec["color"].as_str().unwrap_or("neutro") } }));
        }
        for (g, plano_g) in interior.iter().enumerate() {
            if plano_g.is_some() {
                let c = caja_de_grupo[g];
                let (x, y) = plano.pos[c];
                lote.push(json!({ "op": "crear", "tipo": "marco", "x": ox + x, "y": oy + y, "w": arriba[c].w, "h": arriba[c].h,
                    "datos": { "titulo": grupos_crudos[g]["titulo"].as_str().unwrap_or(""), "color": grupos_crudos[g]["color"].as_str().unwrap_or("neutro") } }));
            }
        }
        for i in 0..nodos.len() {
            let (x, y) = match grupo_de[i] {
                Some(g) => {
                    let c = caja_de_grupo[g];
                    let p = interior[g].as_ref().expect("grupo con nodos");
                    let k = dentro[g].iter().position(|m| *m == i).expect("miembro");
                    (
                        ox + plano.pos[c].0 + MARGEN_MARCO + p.pos[k].0,
                        oy + plano.pos[c].1 + CABECERA_MARCO + p.pos[k].1,
                    )
                }
                None => {
                    let c = caja_de_nodo[i];
                    (ox + plano.pos[c].0, oy + plano.pos[c].1)
                }
            };
            lote.push(json!({ "op": "crear", "ref": refs[i], "tipo": tipos[i], "x": x, "y": y, "w": cajas[i].w, "h": cajas[i].h, "datos": limpios[i] }));
        }
        for a in aristas_crudas {
            let mut d = serde_json::Map::new();
            for k in ["texto", "ruta", "inicio", "fin", "trazo", "color"] {
                if let Some(v) = a.get(k) {
                    d.insert(k.into(), v.clone());
                }
            }
            let punta = |clave: &str, columna: &str| {
                let mut p = json!({ "id": a[clave] });
                if let Some(c) = a[columna].as_str() {
                    p["columna"] = json!(c);
                }
                p
            };
            d.insert("desde".into(), punta("de", "de_columna"));
            d.insert("hasta".into(), punta("a", "a_columna"));
            lote.push(json!({ "op": "crear", "tipo": "flecha", "datos": d }));
        }
        Ok(self.aplicar(
            Quien::Agente {
                capa,
                del_usuario: false,
            },
            &lote,
            ahora,
        ))
    }
}
