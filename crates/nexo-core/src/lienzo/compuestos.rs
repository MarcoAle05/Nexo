//! Las operaciones por partes de las tablas y los tableros. Lo que el usuario toca es la celda o
//! la tarjeta, no el elemento entero: así, mover una tarjeta no le impide al agente seguir
//! añadiendo otras. Añadir nunca choca con nadie.

use serde::Deserialize;
use serde_json::{Value, json};

use super::datos::{self, MAX_COLUMNAS, MAX_FILAS, MAX_TARJETAS, MAX_TEXTO, MAX_TITULO};
use super::{Elemento, Lienzo, Lote, Omitida, Quien, omitida};

#[derive(Debug, Deserialize)]
pub(crate) struct Celda {
    fila: String,
    col: String,
    #[serde(default)]
    valor: Value,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub(crate) enum OpParte {
    Celdas {
        id: String,
        celdas: Vec<Celda>,
    },
    FilaNueva {
        id: String,
        fila: Option<String>,
        pos: Option<usize>,
        #[serde(default)]
        valores: Value,
    },
    FilaBorrar {
        id: String,
        fila: String,
    },
    FilasOrden {
        id: String,
        orden: Vec<String>,
    },
    ColumnaNueva {
        id: String,
        col: Option<String>,
        pos: Option<usize>,
        titulo: Option<String>,
        ancho: Option<f64>,
        limite: Option<u64>,
        #[serde(default)]
        valores: Value,
        #[serde(default)]
        tarjetas: Vec<Value>,
    },
    ColumnaBorrar {
        id: String,
        col: String,
    },
    ColumnaCambiar {
        id: String,
        col: String,
        titulo: Option<String>,
        ancho: Option<f64>,
        #[serde(default, deserialize_with = "doble_opcion")]
        limite: Option<Option<u64>>,
    },
    ColumnasOrden {
        id: String,
        orden: Vec<String>,
    },
    TarjetaNueva {
        id: String,
        #[serde(rename = "ref")]
        referencia: Option<String>,
        tarjeta: Option<String>,
        col: Option<String>,
        pos: Option<usize>,
        #[serde(default)]
        titulo: String,
        puntos: Option<f64>,
        #[serde(default)]
        responsable: String,
        #[serde(default)]
        etiqueta: String,
    },
    TarjetaCambiar {
        id: String,
        tarjeta: String,
        titulo: Option<String>,
        #[serde(default, deserialize_with = "doble_opcion")]
        puntos: Option<Option<f64>>,
        responsable: Option<String>,
        etiqueta: Option<String>,
    },
    TarjetaMover {
        id: String,
        tarjeta: String,
        col: String,
        pos: Option<usize>,
    },
    TarjetaBorrar {
        id: String,
        tarjeta: String,
    },
}

/// Distingue «no lo dijo» (`None`) de «lo dijo vacío» (`Some(None)`).
fn doble_opcion<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

fn cadena(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        otro => otro.to_string(),
    }
}

fn parte_nueva(d: &mut Value, prefijo: &str) -> String {
    let sig = d["sig"].as_u64().unwrap_or(0) + 1;
    d["sig"] = json!(sig);
    format!("{prefijo}{sig}")
}

fn busca(lista: &Value, id: &str) -> Option<usize> {
    lista
        .as_array()?
        .iter()
        .position(|v| v["id"].as_str() == Some(id))
}

fn inserta(lista: &mut Value, pos: Option<usize>, v: Value) -> usize {
    let a = lista.as_array_mut().expect("lista");
    let i = pos.unwrap_or(a.len()).min(a.len());
    a.insert(i, v);
    i
}

/// El elemento, que tiene que ser del tipo pedido y al que el agente tiene que poder entrar.
fn abre(l: &Lienzo, q: Quien, lote: &Lote, id: &str, tipo: &[&str]) -> Result<Elemento, Omitida> {
    let id = l.resuelve(lote, id)?;
    let e = l.elemento(&id).cloned().ok_or(Omitida {
        motivo: "Ya no existe.".into(),
        actual: None,
    })?;
    if !tipo.contains(&e.tipo.as_str()) {
        return omitida(format!(
            "Esa operación no vale para un elemento «{}».",
            e.tipo
        ));
    }
    l.puede_dentro(q, &e)?;
    Ok(e)
}

/// ¿Puede quien pide cambiar esta parte? El usuario siempre; el agente, si el usuario no la está
/// usando (ni el elemento entero) y no la ha tocado.
fn libre(
    l: &Lienzo,
    q: Quien,
    e: &Elemento,
    parte: &str,
    actual: impl Fn() -> Value,
) -> Result<(), Omitida> {
    if q == Quien::Usuario {
        return Ok(());
    }
    if l.esta_en_uso(&e.id) || l.esta_en_uso(&format!("{}/{parte}", e.id)) {
        return Err(Omitida {
            motivo: "El usuario lo está usando ahora mismo.".into(),
            actual: Some(actual()),
        });
    }
    if e.tocadas.iter().any(|t| t == parte) {
        return Err(Omitida {
            motivo: "Lo cambió el usuario: déjalo como está.".into(),
            actual: Some(actual()),
        });
    }
    Ok(())
}

fn guarda(
    l: &mut Lienzo,
    q: Quien,
    lote: &mut Lote,
    mut e: Elemento,
    partes: &[String],
    que: &str,
) {
    if q == Quien::Usuario {
        for p in partes {
            if !e.tocadas.contains(p) {
                e.tocadas.push(p.clone());
            }
        }
        if e.capa != super::CAPA_USUARIO {
            let capa = e.capa.clone();
            l.anotar(&capa, json!({ "id": e.id, "que": que, "partes": partes }));
        } else {
            // En la capa del usuario todo es suyo: no hace falta apuntar nada.
            e.tocadas.clear();
        }
    }
    lote.puestos.insert(e.id.clone());
    if let Some(sitio) = l.elemento_mut(&e.id) {
        *sitio = e;
    }
}

pub(crate) fn aplicar(
    l: &mut Lienzo,
    q: Quien,
    op: OpParte,
    lote: &mut Lote,
) -> Result<(), Omitida> {
    match op {
        OpParte::Celdas { id, celdas } => {
            let mut e = abre(l, q, lote, &id, &["tabla"])?;
            let mut antes = Vec::new();
            let mut partes = Vec::new();
            let mut omitidas = Vec::new();
            for c in &celdas {
                let valor = cadena(&c.valor);
                if valor.chars().count() > MAX_TEXTO {
                    omitidas.push(format!("{}:{} pasa del tope de texto", c.fila, c.col));
                    continue;
                }
                let (Some(f), true) = (
                    busca(&e.datos["filas"], &c.fila),
                    busca(&e.datos["columnas"], &c.col).is_some(),
                ) else {
                    omitidas.push(format!("{}:{} ya no existe", c.fila, c.col));
                    continue;
                };
                let parte = format!("{}:{}", c.fila, c.col);
                let viejo = e.datos["filas"][f]["celdas"]
                    .get(&c.col)
                    .cloned()
                    .unwrap_or(json!(""));
                if let Err(o) = libre(l, q, &e, &parte, || viejo.clone()) {
                    omitidas.push(format!("{parte}: {} (ahora: {})", o.motivo, cadena(&viejo)));
                    continue;
                }
                if cadena(&viejo) == valor {
                    continue;
                }
                antes.push(json!({ "fila": c.fila, "col": c.col, "valor": viejo }));
                let m = e.datos["filas"][f]["celdas"]
                    .as_object_mut()
                    .expect("celdas");
                if valor.is_empty() {
                    m.remove(&c.col);
                } else {
                    m.insert(c.col.clone(), valor.into());
                }
                partes.push(parte);
            }
            if !partes.is_empty() {
                let id = e.id.clone();
                guarda(l, q, lote, e, &partes, "cambió unas celdas");
                lote.inversas
                    .insert(0, json!({ "op": "celdas", "id": id, "celdas": antes }));
            }
            if omitidas.is_empty() {
                Ok(())
            } else {
                omitida(omitidas.join("; "))
            }
        }
        OpParte::FilaNueva {
            id,
            fila,
            pos,
            valores,
        } => {
            let mut e = abre(l, q, lote, &id, &["tabla"])?;
            if e.datos["filas"].as_array().map_or(0, Vec::len) >= MAX_FILAS {
                return omitida(format!("La tabla ya tiene {MAX_FILAS} filas, el tope."));
            }
            let fila = fila
                .filter(|f| datos::id_valido(f))
                .unwrap_or_else(|| parte_nueva(&mut e.datos, "f"));
            if busca(&e.datos["filas"], &fila).is_some() {
                return omitida("Esa fila ya existe.");
            }
            let mut celdas = serde_json::Map::new();
            let cols = e.datos["columnas"].clone();
            let cols = cols.as_array().cloned().unwrap_or_default();
            match &valores {
                Value::Array(a) => {
                    for (c, v) in cols.iter().zip(a) {
                        let s = cadena(v);
                        if !s.is_empty() {
                            celdas.insert(c["id"].as_str().unwrap_or_default().into(), s.into());
                        }
                    }
                }
                Value::Object(m) => {
                    for (k, v) in m {
                        let s = cadena(v);
                        if !s.is_empty() && cols.iter().any(|c| c["id"].as_str() == Some(k)) {
                            celdas.insert(k.clone(), s.into());
                        }
                    }
                }
                _ => {}
            }
            inserta(
                &mut e.datos["filas"],
                pos,
                json!({ "id": fila, "celdas": celdas }),
            );
            let eid = e.id.clone();
            lote.hechas.push(json!({ "id": eid, "fila": fila }));
            guarda(l, q, lote, e, &[], "añadió una fila");
            lote.inversas
                .insert(0, json!({ "op": "fila-borrar", "id": eid, "fila": fila }));
            Ok(())
        }
        OpParte::FilaBorrar { id, fila } => {
            let mut e = abre(l, q, lote, &id, &["tabla"])?;
            let Some(pos) = busca(&e.datos["filas"], &fila) else {
                return omitida("Esa fila ya no existe.");
            };
            let previa = format!("{fila}:");
            if q != Quien::Usuario
                && (e.tocadas.iter().any(|t| t.starts_with(&previa)) || l.esta_en_uso(&e.id))
            {
                return Err(Omitida {
                    motivo: "El usuario cambió esa fila: déjala como está.".into(),
                    actual: Some(e.datos["filas"][pos].clone()),
                });
            }
            let vieja = e.datos["filas"].as_array_mut().expect("filas").remove(pos);
            e.tocadas.retain(|t| !t.starts_with(&previa));
            let eid = e.id.clone();
            guarda(l, q, lote, e, &[], "borró una fila");
            lote.inversas.insert(0, json!({ "op": "fila-nueva", "id": eid, "fila": fila, "pos": pos, "valores": vieja["celdas"] }));
            Ok(())
        }
        OpParte::FilasOrden { id, orden } => {
            let mut e = abre(l, q, lote, &id, &["tabla"])?;
            let antes = reordena(&mut e.datos["filas"], &orden).map_err(|motivo| Omitida {
                motivo,
                actual: None,
            })?;
            if let Some(antes) = antes {
                let eid = e.id.clone();
                lote.inversas
                    .insert(0, json!({ "op": "filas-orden", "id": eid, "orden": antes }));
                guarda(l, q, lote, e, &[], "reordenó las filas");
            }
            Ok(())
        }
        OpParte::ColumnaNueva {
            id,
            col,
            pos,
            titulo,
            ancho,
            limite,
            valores,
            tarjetas,
        } => {
            let mut e = abre(l, q, lote, &id, &["tabla", "tablero"])?;
            if e.datos["columnas"].as_array().map_or(0, Vec::len) >= MAX_COLUMNAS {
                return omitida(format!("Ya tiene {MAX_COLUMNAS} columnas, el tope."));
            }
            let col = col
                .filter(|c| datos::id_valido(c))
                .unwrap_or_else(|| parte_nueva(&mut e.datos, "c"));
            if busca(&e.datos["columnas"], &col).is_some() {
                return omitida("Esa columna ya existe.");
            }
            let titulo: String = titulo
                .unwrap_or_default()
                .chars()
                .take(MAX_TITULO)
                .collect();
            if e.tipo == "tabla" {
                inserta(
                    &mut e.datos["columnas"],
                    pos,
                    json!({ "id": col, "titulo": titulo, "ancho": ancho.unwrap_or(132.0).clamp(44.0, 1200.0) }),
                );
                if let Value::Object(m) = &valores {
                    for f in e.datos["filas"].as_array_mut().expect("filas") {
                        if let Some(v) = f["id"].as_str().and_then(|fid| m.get(fid)) {
                            f["celdas"][&col] = v.clone();
                        }
                    }
                }
            } else {
                inserta(
                    &mut e.datos["columnas"],
                    pos,
                    json!({ "id": col, "titulo": titulo, "limite": limite }),
                );
                for t in tarjetas {
                    e.datos["tarjetas"]
                        .as_array_mut()
                        .expect("tarjetas")
                        .push(t);
                }
            }
            let eid = e.id.clone();
            lote.hechas.push(json!({ "id": eid, "col": col }));
            guarda(l, q, lote, e, &[], "añadió una columna");
            lote.inversas
                .insert(0, json!({ "op": "columna-borrar", "id": eid, "col": col }));
            Ok(())
        }
        OpParte::ColumnaBorrar { id, col } => {
            let mut e = abre(l, q, lote, &id, &["tabla", "tablero"])?;
            let Some(pos) = busca(&e.datos["columnas"], &col) else {
                return omitida("Esa columna ya no existe.");
            };
            let eid = e.id.clone();
            if e.datos["columnas"].as_array().map_or(0, Vec::len) <= 1 {
                return omitida("Tiene que quedar al menos una columna.");
            }
            let vieja = e.datos["columnas"][pos].clone();
            if e.tipo == "tabla" {
                let sufijo = format!(":{col}");
                if q != Quien::Usuario
                    && (e.tocadas.iter().any(|t| t.ends_with(&sufijo)) || l.esta_en_uso(&e.id))
                {
                    return omitida("El usuario cambió esa columna: déjala como está.");
                }
                let mut valores = serde_json::Map::new();
                for f in e.datos["filas"].as_array_mut().expect("filas") {
                    let fid = f["id"].as_str().unwrap_or_default().to_string();
                    if let Some(v) = f["celdas"].as_object_mut().and_then(|m| m.remove(&col)) {
                        valores.insert(fid, v);
                    }
                }
                e.datos["columnas"]
                    .as_array_mut()
                    .expect("columnas")
                    .remove(pos);
                e.tocadas.retain(|t| !t.ends_with(&sufijo));
                guarda(l, q, lote, e, &[], "borró una columna");
                lote.inversas.insert(0, json!({ "op": "columna-nueva", "id": eid, "col": col, "pos": pos, "titulo": vieja["titulo"], "ancho": vieja["ancho"], "valores": valores }));
            } else {
                let tarjetas = e.datos["tarjetas"].as_array().cloned().unwrap_or_default();
                let (fuera, quedan): (Vec<Value>, Vec<Value>) = tarjetas
                    .into_iter()
                    .partition(|t| t["col"].as_str() == Some(col.as_str()));
                if q != Quien::Usuario
                    && fuera.iter().any(|t| {
                        e.tocadas
                            .iter()
                            .any(|p| Some(p.as_str()) == t["id"].as_str())
                    })
                {
                    return omitida(
                        "El usuario movió o cambió tarjetas de esa columna: déjala como está.",
                    );
                }
                e.datos["tarjetas"] = Value::Array(quedan);
                e.datos["columnas"]
                    .as_array_mut()
                    .expect("columnas")
                    .remove(pos);
                guarda(l, q, lote, e, &[], "borró una columna");
                lote.inversas.insert(0, json!({ "op": "columna-nueva", "id": eid, "col": col, "pos": pos, "titulo": vieja["titulo"], "limite": vieja["limite"], "tarjetas": fuera }));
            }
            Ok(())
        }
        OpParte::ColumnaCambiar {
            id,
            col,
            titulo,
            ancho,
            limite,
        } => {
            let mut e = abre(l, q, lote, &id, &["tabla", "tablero"])?;
            let Some(pos) = busca(&e.datos["columnas"], &col) else {
                return omitida("Esa columna ya no existe.");
            };
            let parte = format!("col:{col}");
            let vieja = e.datos["columnas"][pos].clone();
            libre(l, q, &e, &parte, || vieja.clone())?;
            let mut inversa = json!({ "op": "columna-cambiar", "id": e.id, "col": col });
            let c = &mut e.datos["columnas"][pos];
            if let Some(t) = titulo {
                inversa["titulo"] = vieja["titulo"].clone();
                c["titulo"] = json!(t.chars().take(MAX_TITULO).collect::<String>());
            }
            if let Some(a) = ancho {
                inversa["ancho"] = vieja["ancho"].clone();
                c["ancho"] = json!(a.clamp(44.0, 1200.0));
            }
            if let Some(lim) = limite {
                inversa["limite"] = vieja["limite"].clone();
                c["limite"] = json!(lim);
            }
            guarda(l, q, lote, e, &[parte], "cambió una columna");
            lote.inversas.insert(0, inversa);
            Ok(())
        }
        OpParte::ColumnasOrden { id, orden } => {
            let mut e = abre(l, q, lote, &id, &["tabla", "tablero"])?;
            if let Some(antes) =
                reordena(&mut e.datos["columnas"], &orden).map_err(|motivo| Omitida {
                    motivo,
                    actual: None,
                })?
            {
                let eid = e.id.clone();
                lote.inversas.insert(
                    0,
                    json!({ "op": "columnas-orden", "id": eid, "orden": antes }),
                );
                guarda(l, q, lote, e, &[], "reordenó las columnas");
            }
            Ok(())
        }
        OpParte::TarjetaNueva {
            id,
            referencia,
            tarjeta,
            col,
            pos,
            titulo,
            puntos,
            responsable,
            etiqueta,
        } => {
            let mut e = abre(l, q, lote, &id, &["tablero"])?;
            if e.datos["tarjetas"].as_array().map_or(0, Vec::len) >= MAX_TARJETAS {
                return omitida(format!(
                    "El tablero ya tiene {MAX_TARJETAS} tarjetas, el tope."
                ));
            }
            let cols = e.datos["columnas"].as_array().cloned().unwrap_or_default();
            let pedida = col.unwrap_or_default();
            let col = cols
                .iter()
                .find(|c| {
                    c["id"].as_str() == Some(&pedida) || c["titulo"].as_str() == Some(&pedida)
                })
                .or(cols.first())
                .and_then(|c| c["id"].as_str())
                .unwrap_or_default()
                .to_string();
            let mut sig = e.datos["sig"].as_u64().unwrap_or(0);
            let mut crudo = json!({ "titulo": titulo, "puntos": puntos, "responsable": responsable, "etiqueta": etiqueta });
            if let Some(t) = tarjeta.filter(|t| datos::id_valido(t)) {
                if busca(&e.datos["tarjetas"], &t).is_some() {
                    return omitida("Esa tarjeta ya existe.");
                }
                crudo["id"] = json!(t);
            }
            let limpia =
                datos::tarjeta_limpia(&crudo, &col, &mut sig).map_err(|motivo| Omitida {
                    motivo,
                    actual: None,
                })?;
            e.datos["sig"] = json!(sig);
            let tid = limpia["id"].as_str().unwrap_or_default().to_string();
            coloca(&mut e.datos["tarjetas"], limpia, &col, pos);
            let eid = e.id.clone();
            let mut hecha = json!({ "id": eid, "tarjeta": tid });
            if let Some(r) = referencia {
                hecha["ref"] = json!(r);
            }
            lote.hechas.push(hecha);
            guarda(l, q, lote, e, &[], "añadió una tarjeta");
            lote.inversas.insert(
                0,
                json!({ "op": "tarjeta-borrar", "id": eid, "tarjeta": tid }),
            );
            Ok(())
        }
        OpParte::TarjetaCambiar {
            id,
            tarjeta,
            titulo,
            puntos,
            responsable,
            etiqueta,
        } => {
            let mut e = abre(l, q, lote, &id, &["tablero"])?;
            let Some(i) = busca(&e.datos["tarjetas"], &tarjeta) else {
                return omitida("Esa tarjeta ya no existe.");
            };
            let vieja = e.datos["tarjetas"][i].clone();
            libre(l, q, &e, &tarjeta, || vieja.clone())?;
            let mut inversa = json!({ "op": "tarjeta-cambiar", "id": e.id, "tarjeta": tarjeta });
            let t = &mut e.datos["tarjetas"][i];
            for (clave, valor, tope) in [
                ("titulo", titulo, MAX_TEXTO),
                ("responsable", responsable, MAX_TITULO),
                ("etiqueta", etiqueta, MAX_TITULO),
            ] {
                if let Some(v) = valor {
                    if v.chars().count() > tope {
                        return omitida(format!("«{clave}» pasa de {tope} caracteres."));
                    }
                    inversa[clave] = vieja[clave].clone();
                    t[clave] = json!(v);
                }
            }
            if let Some(p) = puntos {
                inversa["puntos"] = vieja["puntos"].clone();
                t["puntos"] = json!(p);
            }
            guarda(
                l,
                q,
                lote,
                e,
                std::slice::from_ref(&tarjeta),
                "cambió una tarjeta",
            );
            lote.inversas.insert(0, inversa);
            Ok(())
        }
        OpParte::TarjetaMover {
            id,
            tarjeta,
            col,
            pos,
        } => {
            let mut e = abre(l, q, lote, &id, &["tablero"])?;
            let Some(i) = busca(&e.datos["tarjetas"], &tarjeta) else {
                return omitida("Esa tarjeta ya no existe.");
            };
            let cols = e.datos["columnas"].as_array().cloned().unwrap_or_default();
            let Some(col) = cols
                .iter()
                .find(|c| c["id"].as_str() == Some(&col) || c["titulo"].as_str() == Some(&col))
                .and_then(|c| c["id"].as_str().map(String::from))
            else {
                return omitida("Esa columna ya no existe.");
            };
            let vieja = e.datos["tarjetas"][i].clone();
            libre(l, q, &e, &tarjeta, || vieja.clone())?;
            let col_antes = vieja["col"].as_str().unwrap_or_default().to_string();
            let pos_antes = e.datos["tarjetas"]
                .as_array()
                .expect("tarjetas")
                .iter()
                .filter(|t| t["col"].as_str() == Some(&col_antes))
                .position(|t| t["id"].as_str() == Some(&tarjeta));
            let mut t = e.datos["tarjetas"]
                .as_array_mut()
                .expect("tarjetas")
                .remove(i);
            t["col"] = json!(col);
            coloca(&mut e.datos["tarjetas"], t, &col, pos);
            let eid = e.id.clone();
            guarda(
                l,
                q,
                lote,
                e,
                std::slice::from_ref(&tarjeta),
                "movió una tarjeta",
            );
            lote.inversas.insert(0, json!({ "op": "tarjeta-mover", "id": eid, "tarjeta": tarjeta, "col": col_antes, "pos": pos_antes }));
            Ok(())
        }
        OpParte::TarjetaBorrar { id, tarjeta } => {
            let mut e = abre(l, q, lote, &id, &["tablero"])?;
            let Some(i) = busca(&e.datos["tarjetas"], &tarjeta) else {
                return omitida("Esa tarjeta ya no existe.");
            };
            let vieja = e.datos["tarjetas"][i].clone();
            libre(l, q, &e, &tarjeta, || vieja.clone())?;
            let col = vieja["col"].as_str().unwrap_or_default().to_string();
            let pos = e.datos["tarjetas"]
                .as_array()
                .expect("tarjetas")
                .iter()
                .filter(|t| t["col"].as_str() == Some(&col))
                .position(|t| t["id"].as_str() == Some(&tarjeta));
            e.datos["tarjetas"]
                .as_array_mut()
                .expect("tarjetas")
                .remove(i);
            e.tocadas.retain(|t| t != &tarjeta);
            let eid = e.id.clone();
            guarda(l, q, lote, e, &[], "borró una tarjeta");
            lote.inversas.insert(0, json!({ "op": "tarjeta-nueva", "id": eid, "tarjeta": tarjeta, "col": col, "pos": pos,
                "titulo": vieja["titulo"], "puntos": vieja["puntos"], "responsable": vieja["responsable"], "etiqueta": vieja["etiqueta"] }));
            Ok(())
        }
    }
}

/// Pone una tarjeta en el puesto `pos` de su columna (las demás columnas no se mueven).
fn coloca(lista: &mut Value, t: Value, col: &str, pos: Option<usize>) {
    let a = lista.as_array_mut().expect("tarjetas");
    let en_col: Vec<usize> = a
        .iter()
        .enumerate()
        .filter(|(_, x)| x["col"].as_str() == Some(col))
        .map(|(i, _)| i)
        .collect();
    let i = match pos {
        Some(p) if p < en_col.len() => en_col[p],
        _ => en_col.last().map_or(a.len(), |u| u + 1),
    };
    a.insert(i, t);
}

/// Deja la lista en el orden pedido. Lo que el orden no nombre (el agente pudo añadir algo
/// mientras tanto) se queda al final, en vez de perderse. Devuelve el orden de antes.
fn reordena(lista: &mut Value, orden: &[String]) -> Result<Option<Vec<String>>, String> {
    let a = lista.as_array().cloned().ok_or("Lista rota.")?;
    let antes: Vec<String> = a
        .iter()
        .filter_map(|v| v["id"].as_str().map(String::from))
        .collect();
    let mut nueva: Vec<Value> = orden
        .iter()
        .filter_map(|id| a.iter().find(|v| v["id"].as_str() == Some(id)).cloned())
        .collect();
    let puestos: Vec<&str> = nueva
        .iter()
        .filter_map(|v| v["id"].as_str())
        .collect::<Vec<_>>();
    let resto: Vec<Value> = a
        .iter()
        .filter(|v| !v["id"].as_str().is_some_and(|i| puestos.contains(&i)))
        .cloned()
        .collect();
    nueva.extend(resto);
    let despues: Vec<String> = nueva
        .iter()
        .filter_map(|v| v["id"].as_str().map(String::from))
        .collect();
    if despues == antes {
        return Ok(None);
    }
    *lista = Value::Array(nueva);
    Ok(Some(antes))
}
