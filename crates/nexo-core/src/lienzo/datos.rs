//! Lo que lleva dentro cada tipo de elemento: se valida y se deja en su forma canónica antes de
//! entrar en el lienzo. Solo pasan los campos conocidos, los colores son roles (cada tema los
//! traduce) y lo que excede un tope se rechaza entero: nunca se corta en silencio.
//!
//! El agente puede escribir las tablas y los tableros de la forma cómoda (`columnas: ["A"]`,
//! `filas: [["x"]]`): aquí reciben sus identificadores estables, los que usan las operaciones.

use serde_json::{Map, Value, json};

pub const MAX_TEXTO: usize = 20_000;
pub const MAX_TITULO: usize = 200;
pub const MAX_FILAS: usize = 200;
pub const MAX_COLUMNAS: usize = 30;
pub const MAX_TARJETAS: usize = 500;
pub const MAX_LISTA: usize = 300;

pub const TIPOS: [&str; 13] = [
    "texto",
    "figura",
    "flecha",
    "marco",
    "adhesiva",
    "tabla",
    "tablero",
    "entidad",
    "secuencia",
    "codigo",
    "linea-tiempo",
    "referencia",
    "imagen",
];
pub const COLORES: [&str; 6] = ["neutro", "acento", "azul", "verde", "ambar", "rojo"];
pub const FORMAS: [&str; 16] = [
    "rectangulo",
    "redondeado",
    "elipse",
    "rombo",
    "paralelogramo",
    "hexagono",
    "documento",
    "nube",
    "cilindro",
    "servicio",
    "cola",
    "cache",
    "api",
    "persona",
    "externo",
    "inicio",
];
pub const EXTREMOS: [&str; 7] = [
    "nada",
    "flecha",
    "triangulo",
    "rombo",
    "uno",
    "muchos",
    "cero-muchos",
];
pub const RUTAS: [&str; 3] = ["angulada", "recta", "curva"];
pub const TRAZOS: [&str; 2] = ["continuo", "discontinuo"];
pub const MENSAJES: [&str; 3] = ["sincrono", "asincrono", "respuesta"];

fn cadena(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn texto(d: &Value, clave: &str, max: usize) -> Result<String, String> {
    let s = d.get(clave).and_then(cadena).unwrap_or_default();
    if s.chars().count() > max {
        return Err(format!("«{clave}» pasa de {max} caracteres."));
    }
    Ok(s)
}

fn uno_de(d: &Value, clave: &str, lista: &[&str], otro: &str) -> Result<String, String> {
    match d.get(clave) {
        None | Some(Value::Null) => Ok(otro.to_string()),
        Some(Value::String(s)) if lista.contains(&s.as_str()) => Ok(s.clone()),
        Some(v) => Err(format!(
            "«{clave}» no puede ser {v}. Vale: {}.",
            lista.join(", ")
        )),
    }
}

fn color(d: &Value, otro: &str) -> Result<String, String> {
    uno_de(d, "color", &COLORES, otro)
}

fn lista<'a>(d: &'a Value, clave: &str, max: usize) -> Result<Vec<&'a Value>, String> {
    match d.get(clave) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(a)) if a.len() <= max => Ok(a.iter().collect()),
        Some(Value::Array(_)) => Err(format!("«{clave}» pasa de {max} entradas.")),
        Some(_) => Err(format!("«{clave}» tiene que ser una lista.")),
    }
}

/// Un identificador que llega de fuera: solo letras, números, `-` y `_`.
pub fn id_valido(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Identificador nuevo de una parte (`f3`, `c2`, `t9`), sin repetir nunca dentro del elemento.
fn nuevo(prefijo: &str, sig: &mut u64) -> String {
    *sig += 1;
    format!("{prefijo}{sig}")
}

fn id_o_nuevo(v: &Value, prefijo: &str, sig: &mut u64) -> String {
    match v.get("id").and_then(Value::as_str) {
        Some(id) if id_valido(id) => id.to_string(),
        _ => nuevo(prefijo, sig),
    }
}

fn sig_de(d: &Value) -> u64 {
    d.get("sig").and_then(Value::as_u64).unwrap_or(0)
}

/// Una punta de flecha: un elemento (`{ id, ancla?, columna? }`) o un punto suelto (`{ x, y }`).
fn punta(v: Option<&Value>, nombre: &str) -> Result<Value, String> {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok(json!({ "x": 0.0, "y": 0.0 }));
    };
    if let Some(id) = v.get("id").and_then(Value::as_str) {
        let mut out = json!({ "id": id });
        if let Some(a) = v.get("ancla").and_then(Value::as_str) {
            if !["auto", "n", "e", "s", "o"].contains(&a) {
                return Err(format!("El ancla de «{nombre}» no puede ser «{a}»."));
            }
            out["ancla"] = json!(a);
        }
        if let Some(c) = v.get("columna").and_then(Value::as_str) {
            out["columna"] = json!(c);
        }
        return Ok(out);
    }
    match (
        v.get("x").and_then(Value::as_f64),
        v.get("y").and_then(Value::as_f64),
    ) {
        (Some(x), Some(y)) => Ok(json!({ "x": x, "y": y })),
        _ => Err(format!("«{nombre}» va con un id o con x e y.")),
    }
}

fn tabla(d: &Value) -> Result<Value, String> {
    let mut sig = sig_de(d);
    let columnas = lista(d, "columnas", MAX_COLUMNAS)?;
    let filas = lista(d, "filas", MAX_FILAS)?;
    let mut cols = Vec::new();
    for c in &columnas {
        let (id, titulo, ancho) = match c {
            Value::Object(_) => (
                id_o_nuevo(c, "c", &mut sig),
                texto(c, "titulo", MAX_TITULO)?,
                c.get("ancho").and_then(Value::as_f64),
            ),
            otro => (nuevo("c", &mut sig), cadena(otro).unwrap_or_default(), None),
        };
        cols.push(json!({ "id": id, "titulo": titulo, "ancho": ancho.unwrap_or(132.0).clamp(44.0, 1200.0) }));
    }
    if cols.is_empty() {
        let n = filas
            .iter()
            .filter_map(|f| f.as_array().map(Vec::len))
            .max()
            .unwrap_or(3)
            .clamp(1, MAX_COLUMNAS);
        for _ in 0..n {
            cols.push(json!({ "id": nuevo("c", &mut sig), "titulo": "", "ancho": 132.0 }));
        }
    }
    let mut salida = Vec::new();
    for f in &filas {
        let mut celdas = Map::new();
        let id = match f {
            Value::Array(valores) => {
                if valores.len() > cols.len() {
                    return Err(format!(
                        "Una fila trae {} valores y la tabla tiene {} columnas.",
                        valores.len(),
                        cols.len()
                    ));
                }
                for (i, v) in valores.iter().enumerate() {
                    let valor = cadena(v).unwrap_or_default();
                    if valor.chars().count() > MAX_TEXTO {
                        return Err("Una celda pasa del tope de texto.".into());
                    }
                    if !valor.is_empty() {
                        celdas.insert(
                            cols[i]["id"].as_str().unwrap_or_default().into(),
                            valor.into(),
                        );
                    }
                }
                nuevo("f", &mut sig)
            }
            Value::Object(_) => {
                if let Some(Value::Object(m)) = f.get("celdas") {
                    for (k, v) in m {
                        let valor = cadena(v).unwrap_or_default();
                        if valor.chars().count() > MAX_TEXTO {
                            return Err("Una celda pasa del tope de texto.".into());
                        }
                        if !valor.is_empty()
                            && cols.iter().any(|c| c["id"].as_str() == Some(k.as_str()))
                        {
                            celdas.insert(k.clone(), valor.into());
                        }
                    }
                }
                id_o_nuevo(f, "f", &mut sig)
            }
            _ => return Err("Cada fila es una lista de valores.".into()),
        };
        salida.push(json!({ "id": id, "celdas": celdas }));
    }
    if salida.is_empty() && d.get("filas").is_none() {
        for _ in 0..3 {
            salida.push(json!({ "id": nuevo("f", &mut sig), "celdas": {} }));
        }
    }
    Ok(json!({
        "titulo": texto(d, "titulo", MAX_TITULO)?,
        "cabecera": d.get("cabecera").and_then(Value::as_bool).unwrap_or(true),
        "color": color(d, "neutro")?,
        "columnas": cols,
        "filas": salida,
        "sig": sig,
    }))
}

pub fn tarjeta_limpia(t: &Value, col: &str, sig: &mut u64) -> Result<Value, String> {
    let puntos = t
        .get("puntos")
        .and_then(Value::as_f64)
        .filter(|p| *p >= 0.0);
    Ok(json!({
        "id": id_o_nuevo(t, "t", sig),
        "col": col,
        "titulo": texto(t, "titulo", MAX_TEXTO)?,
        "puntos": puntos,
        "responsable": texto(t, "responsable", MAX_TITULO)?,
        "etiqueta": texto(t, "etiqueta", MAX_TITULO)?,
    }))
}

fn tablero(d: &Value) -> Result<Value, String> {
    let mut sig = sig_de(d);
    let mut cols = Vec::new();
    for c in lista(d, "columnas", MAX_COLUMNAS)? {
        let (id, titulo, limite) = match c {
            Value::Object(_) => (
                id_o_nuevo(c, "c", &mut sig),
                texto(c, "titulo", MAX_TITULO)?,
                c.get("limite").and_then(Value::as_u64),
            ),
            otro => (nuevo("c", &mut sig), cadena(otro).unwrap_or_default(), None),
        };
        cols.push(json!({ "id": id, "titulo": titulo, "limite": limite }));
    }
    if cols.is_empty() {
        for titulo in ["Por hacer", "En curso", "Hecho"] {
            cols.push(json!({ "id": nuevo("c", &mut sig), "titulo": titulo, "limite": null }));
        }
    }
    let mut tarjetas = Vec::new();
    for t in lista(d, "tarjetas", MAX_TARJETAS)? {
        // La columna va por su id o por su título; sin decirla, la primera.
        let pedida = t.get("col").and_then(Value::as_str).unwrap_or_default();
        let col = cols
            .iter()
            .find(|c| c["id"].as_str() == Some(pedida) || c["titulo"].as_str() == Some(pedida))
            .or(cols.first())
            .and_then(|c| c["id"].as_str())
            .unwrap_or_default()
            .to_string();
        tarjetas.push(tarjeta_limpia(t, &col, &mut sig)?);
    }
    Ok(json!({
        "titulo": texto(d, "titulo", MAX_TITULO)?,
        "columnas": cols,
        "tarjetas": tarjetas,
        "sig": sig,
    }))
}

fn entidad(d: &Value) -> Result<Value, String> {
    let mut sig = sig_de(d);
    let mut cols = Vec::new();
    for c in lista(d, "columnas", MAX_FILAS)? {
        let (nombre, tipo) = match c {
            Value::Object(_) => (
                texto(c, "nombre", MAX_TITULO)?,
                texto(c, "tipo", MAX_TITULO)?,
            ),
            otro => (cadena(otro).unwrap_or_default(), String::new()),
        };
        let marca = |k: &str| c.get(k).and_then(Value::as_bool).unwrap_or(false);
        cols.push(json!({
            "id": id_o_nuevo(c, "c", &mut sig),
            "nombre": nombre,
            "tipo": tipo,
            "pk": marca("pk"),
            "fk": marca("fk"),
            "nulo": marca("nulo"),
            "unico": marca("unico"),
        }));
    }
    Ok(json!({
        "nombre": texto(d, "nombre", MAX_TITULO)?,
        "color": color(d, "azul")?,
        "columnas": cols,
        "sig": sig,
    }))
}

fn secuencia(d: &Value) -> Result<Value, String> {
    let mut sig = sig_de(d);
    let mut gente = Vec::new();
    for p in lista(d, "participantes", 30)? {
        let (id, nombre) = match p {
            Value::Object(_) => (
                id_o_nuevo(p, "p", &mut sig),
                texto(p, "nombre", MAX_TITULO)?,
            ),
            otro => (nuevo("p", &mut sig), cadena(otro).unwrap_or_default()),
        };
        gente.push(json!({ "id": id, "nombre": nombre }));
    }
    let busca = |v: Option<&Value>, gente: &[Value]| -> Option<String> {
        let pedido = v?.as_str()?;
        gente
            .iter()
            .find(|p| p["id"].as_str() == Some(pedido) || p["nombre"].as_str() == Some(pedido))
            .and_then(|p| p["id"].as_str().map(String::from))
    };
    let mut mensajes = Vec::new();
    for m in lista(d, "mensajes", MAX_LISTA)? {
        let (Some(de), Some(a)) = (busca(m.get("de"), &gente), busca(m.get("a"), &gente)) else {
            return Err("Un mensaje nombra a un participante que no está en la lista.".into());
        };
        mensajes.push(json!({
            "id": id_o_nuevo(m, "m", &mut sig),
            "de": de,
            "a": a,
            "texto": texto(m, "texto", MAX_TITULO * 3)?,
            "tipo": uno_de(m, "tipo", &MENSAJES, "sincrono")?,
        }));
    }
    Ok(json!({
        "titulo": texto(d, "titulo", MAX_TITULO)?,
        "participantes": gente,
        "mensajes": mensajes,
        "sig": sig,
    }))
}

fn fecha_valida(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && s.chars()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn fecha(d: &Value, clave: &str) -> Result<String, String> {
    let s = texto(d, clave, 10)?;
    if s.is_empty() || fecha_valida(&s) {
        Ok(s)
    } else {
        Err(format!("«{clave}» va como AAAA-MM-DD; llegó «{s}»."))
    }
}

fn linea_tiempo(d: &Value) -> Result<Value, String> {
    let mut sig = sig_de(d);
    let mut tareas = Vec::new();
    for t in lista(d, "tareas", MAX_LISTA)? {
        let dep: Vec<Value> = lista(t, "dep", 30)?
            .into_iter()
            .filter_map(cadena)
            .map(Value::from)
            .collect();
        tareas.push(json!({
            "id": id_o_nuevo(t, "t", &mut sig),
            "titulo": texto(t, "titulo", MAX_TITULO)?,
            "inicio": fecha(t, "inicio")?,
            "fin": fecha(t, "fin")?,
            "dep": dep,
            "avance": t.get("avance").and_then(Value::as_f64).unwrap_or(0.0).clamp(0.0, 100.0),
        }));
    }
    let mut hitos = Vec::new();
    for h in lista(d, "hitos", 60)? {
        hitos.push(json!({
            "id": id_o_nuevo(h, "h", &mut sig),
            "titulo": texto(h, "titulo", MAX_TITULO)?,
            "fecha": fecha(h, "fecha")?,
        }));
    }
    Ok(json!({
        "titulo": texto(d, "titulo", MAX_TITULO)?,
        "tareas": tareas,
        "hitos": hitos,
        "sig": sig,
    }))
}

/// Una ruta relativa dentro de lo suyo: ni absoluta ni con `..`. El guardián de verdad está al
/// abrirla (`notas::resolve`, `vault::resolve_inside`); aquí solo se evita guardar un disparate.
fn ruta_relativa(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('/')
        && !s.contains('\\')
        && !s.contains("://")
        && s.split('/').all(|p| p != ".." && p != ".")
}

fn referencia(d: &Value) -> Result<Value, String> {
    let ruta = texto(d, "ruta", 1000)?;
    if !ruta.is_empty() && !ruta_relativa(&ruta) {
        return Err(format!("La referencia «{ruta}» no es una ruta del vault."));
    }
    Ok(json!({
        "tipo": uno_de(d, "tipo", &["nota", "nodo", "fuente"], "nota")?,
        "ruta": ruta,
        "titulo": texto(d, "titulo", MAX_TITULO)?,
    }))
}

/// Un recurso de `.nexo/lienzos/recursos/`: su nombre sale de su contenido, nunca una URL.
pub fn recurso_valido(s: &str) -> bool {
    let mut partes = s.split('.');
    let (Some(nombre), Some(ext), None) = (partes.next(), partes.next(), partes.next()) else {
        return false;
    };
    id_valido(nombre) && ["png", "jpg", "jpeg", "webp", "gif"].contains(&ext)
}

fn imagen(d: &Value) -> Result<Value, String> {
    let recurso = texto(d, "recurso", 100)?;
    if !recurso.is_empty() && !recurso_valido(&recurso) {
        return Err("Una imagen solo puede venir de un archivo pegado o soltado en nexo.".into());
    }
    Ok(json!({ "recurso": recurso, "alt": texto(d, "alt", MAX_TITULO)? }))
}

/// Los datos de un elemento, en su forma canónica. `Err` dice por qué no valen.
pub fn normalizar(tipo: &str, d: &Value) -> Result<Value, String> {
    let d = if d.is_null() { &json!({}) } else { d };
    if !d.is_object() {
        return Err("«datos» tiene que ser un objeto.".into());
    }
    match tipo {
        "texto" => Ok(json!({
            "texto": texto(d, "texto", MAX_TEXTO)?,
            "color": color(d, "neutro")?,
            "tamano": uno_de(d, "tamano", &["s", "m", "l", "xl"], "m")?,
            "alineado": uno_de(d, "alineado", &["izq", "centro", "der"], "izq")?,
        })),
        "figura" => Ok(json!({
            "forma": uno_de(d, "forma", &FORMAS, "rectangulo")?,
            "texto": texto(d, "texto", MAX_TEXTO)?,
            "color": color(d, "neutro")?,
        })),
        "flecha" => {
            let mut out = json!({
                "desde": punta(d.get("desde"), "desde")?,
                "hasta": punta(d.get("hasta"), "hasta")?,
                "ruta": uno_de(d, "ruta", &RUTAS, "angulada")?,
                "inicio": uno_de(d, "inicio", &EXTREMOS, "nada")?,
                "fin": uno_de(d, "fin", &EXTREMOS, "flecha")?,
                "trazo": uno_de(d, "trazo", &TRAZOS, "continuo")?,
                "texto": texto(d, "texto", MAX_TITULO)?,
                "color": color(d, "neutro")?,
            });
            if out["texto"] == ""
                && let Some(m) = out.as_object_mut()
            {
                m.remove("texto");
            }
            Ok(out)
        }
        "marco" => Ok(json!({
            "titulo": texto(d, "titulo", MAX_TITULO)?,
            "color": color(d, "neutro")?,
        })),
        "adhesiva" => Ok(json!({
            "texto": texto(d, "texto", MAX_TEXTO)?,
            "color": color(d, "ambar")?,
        })),
        "tabla" => tabla(d),
        "tablero" => tablero(d),
        "entidad" => entidad(d),
        "secuencia" => secuencia(d),
        "codigo" => Ok(json!({
            "lenguaje": texto(d, "lenguaje", 40)?,
            "texto": texto(d, "texto", MAX_TEXTO)?,
            "titulo": texto(d, "titulo", MAX_TITULO)?,
        })),
        "linea-tiempo" => linea_tiempo(d),
        "referencia" => referencia(d),
        "imagen" => imagen(d),
        otro => Err(format!(
            "No existe el tipo «{otro}». Hay: {}.",
            TIPOS.join(", ")
        )),
    }
}

/// Las claves que no se cambian de golpe porque se cambian por partes: así un agente no puede
/// pisar la celda o la tarjeta que tocó el usuario reescribiendo la lista entera.
pub fn por_partes(tipo: &str) -> &'static [&'static str] {
    match tipo {
        "tabla" => &["columnas", "filas", "sig"],
        "tablero" => &["columnas", "tarjetas", "sig"],
        _ => &[],
    }
}

/// Tamaño de partida si no se dice otro.
pub fn tamano_de_partida(tipo: &str, d: &Value) -> (f64, f64) {
    let n = |clave: &str| d[clave].as_array().map_or(0, Vec::len) as f64;
    match tipo {
        "texto" => {
            let (_, h) = alto_de_texto(
                d["texto"].as_str().unwrap_or(""),
                264.0,
                d["tamano"].as_str(),
            );
            (264.0, h.max(66.0))
        }
        "figura" => caja_de_texto(d["texto"].as_str().unwrap_or(""), d["forma"].as_str()),
        "marco" => (660.0, 440.0),
        "adhesiva" => {
            let (_, h) = alto_de_texto(d["texto"].as_str().unwrap_or(""), 176.0 - 24.0, None);
            (176.0, (h + 24.0).max(176.0))
        }
        "tabla" => {
            let ancho: f64 = d["columnas"]
                .as_array()
                .map_or(0.0, |c| c.iter().filter_map(|c| c["ancho"].as_f64()).sum());
            (
                (ancho + 2.0).max(176.0),
                (34.0 * (n("filas") + 1.0) + 2.0 + if d["titulo"] == "" { 0.0 } else { 34.0 })
                    .max(70.0),
            )
        }
        "tablero" => (
            (n("columnas") * 242.0 + 22.0).max(264.0),
            (132.0 + 22.0 * (n("tarjetas") * 4.0)).clamp(396.0, 880.0),
        ),
        "entidad" => (264.0, 44.0 + 30.0 * n("columnas").max(1.0)),
        "secuencia" => (
            (n("participantes") * 176.0 + 44.0).max(264.0),
            (n("mensajes") * 44.0 + 132.0).max(176.0),
        ),
        "codigo" => {
            let lineas = d["texto"].as_str().map_or(1, |t| t.lines().count().max(1)) as f64;
            (484.0, (lineas * 19.0 + 66.0).clamp(88.0, 660.0))
        }
        "linea-tiempo" => (
            660.0,
            88.0 + 36.0 * (n("tareas") + n("hitos").min(1.0)).max(1.0),
        ),
        "referencia" => (264.0, 44.0),
        "imagen" => (330.0, 220.0),
        _ => (0.0, 0.0),
    }
}

/// Cuánto ocupa un texto que se reparte en un ancho dado, en la retícula. Se estima por lo alto.
fn alto_de_texto(texto: &str, ancho: f64, tamano: Option<&str>) -> (f64, f64) {
    let (por_caracter, alto) = match tamano {
        Some("s") => (6.6, 18.0),
        Some("l") => (9.6, 26.0),
        Some("xl") => (14.0, 34.0),
        _ => (7.6, 22.0),
    };
    let por_linea = ((ancho - 12.0) / por_caracter).floor().max(4.0) as usize;
    let lineas: usize = texto
        .split('\n')
        .map(|p| p.chars().count().div_ceil(por_linea).max(1))
        .sum();
    let h = lineas as f64 * alto + 10.0;
    (ancho, (h / 22.0).ceil() * 22.0)
}

/// Lo que ocupa un texto en una caja: se estima, de forma conservadora, porque aquí no se puede
/// medir la fuente (el motor funciona con la vista Notas oculta). Múltiplos de la retícula.
pub fn caja_de_texto(texto: &str, forma: Option<&str>) -> (f64, f64) {
    const POR_CARACTER: f64 = 7.4;
    const ALTO_LINEA: f64 = 20.0;
    const MAX_CARACTERES: usize = 28;
    let mut lineas = 0usize;
    let mut mayor = 0usize;
    for parrafo in texto.split('\n') {
        let mut actual = 0usize;
        lineas += 1;
        for palabra in parrafo.split_whitespace() {
            let n = palabra.chars().count();
            if actual > 0 && actual + 1 + n > MAX_CARACTERES {
                mayor = mayor.max(actual);
                lineas += 1;
                actual = n;
            } else {
                actual += n + usize::from(actual > 0);
            }
        }
        mayor = mayor.max(actual);
    }
    let (mut w, mut h) = (
        mayor as f64 * POR_CARACTER + 36.0,
        lineas.max(1) as f64 * ALTO_LINEA + 32.0,
    );
    // En estas formas el texto solo cabe en una parte de la caja.
    let (fw, fh) = match forma {
        Some("rombo") => (1.7, 1.9),
        Some("elipse" | "nube") => (1.3, 1.5),
        Some("hexagono" | "paralelogramo") => (1.3, 1.1),
        Some("cilindro" | "cola" | "cache") => (1.05, 1.5),
        Some("documento") => (1.0, 1.3),
        Some("persona") => (1.0, 2.6),
        _ => (1.0, 1.0),
    };
    w = (w * fw).max(132.0);
    h = (h * fh).max(66.0);
    let arriba = |v: f64| (v / 22.0).ceil() * 22.0;
    (arriba(w), arriba(h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_pasan_los_campos_y_valores_conocidos() {
        let d = normalizar(
            "figura",
            &json!({ "forma": "rombo", "texto": "¿Sí?", "color": "azul", "onclick": "x", "html": "<b>" }),
        )
        .unwrap();
        assert_eq!(
            d,
            json!({ "forma": "rombo", "texto": "¿Sí?", "color": "azul" })
        );
        assert!(normalizar("figura", &json!({ "color": "#ff0000" })).is_err());
        assert!(normalizar("figura", &json!({ "forma": "pentagrama" })).is_err());
        assert!(normalizar("video", &json!({})).is_err());
        assert!(normalizar("figura", &json!("texto")).is_err());
    }

    #[test]
    fn una_tabla_de_la_forma_comoda_recibe_sus_identificadores() {
        let d = normalizar(
            "tabla",
            &json!({ "columnas": ["Nombre", "Años"], "filas": [["Ana", 30], ["Luis"]] }),
        )
        .unwrap();
        let c0 = d["columnas"][0]["id"].as_str().unwrap();
        let c1 = d["columnas"][1]["id"].as_str().unwrap();
        assert_ne!(c0, c1);
        assert_eq!(d["filas"][0]["celdas"][c0], "Ana");
        assert_eq!(d["filas"][0]["celdas"][c1], "30");
        assert!(d["filas"][1]["celdas"].get(c1).is_none());
        // Volver a pasarla no cambia nada: es su forma canónica.
        assert_eq!(normalizar("tabla", &d).unwrap(), d);
    }

    #[test]
    fn lo_que_pasa_de_un_tope_se_rechaza_entero() {
        let filas: Vec<Value> = (0..=MAX_FILAS).map(|_| json!(["x"])).collect();
        let e = normalizar("tabla", &json!({ "columnas": ["A"], "filas": filas })).unwrap_err();
        assert!(e.contains("200"), "{e}");
        assert!(
            normalizar(
                "tabla",
                &json!({ "columnas": ["A"], "filas": [["a", "b"]] })
            )
            .is_err()
        );
        assert!(normalizar("texto", &json!({ "texto": "x".repeat(MAX_TEXTO + 1) })).is_err());
    }

    #[test]
    fn un_tablero_sin_columnas_trae_las_de_scrum_y_coloca_cada_tarjeta() {
        let d = normalizar(
            "tablero",
            &json!({ "tarjetas": [{ "titulo": "A", "col": "En curso", "puntos": 3 }, { "titulo": "B" }] }),
        )
        .unwrap();
        let titulos: Vec<_> = d["columnas"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["titulo"].as_str().unwrap())
            .collect();
        assert_eq!(titulos, ["Por hacer", "En curso", "Hecho"]);
        assert_eq!(d["tarjetas"][0]["col"], d["columnas"][1]["id"]);
        assert_eq!(d["tarjetas"][1]["col"], d["columnas"][0]["id"]);
        assert_eq!(d["tarjetas"][0]["puntos"], 3.0);
    }

    #[test]
    fn un_mensaje_de_secuencia_tiene_que_nombrar_a_alguien_que_esta() {
        let d = normalizar(
            "secuencia",
            &json!({ "participantes": ["Chat", "Motor"], "mensajes": [{ "de": "Chat", "a": "Motor", "texto": "pregunta" }] }),
        )
        .unwrap();
        assert_eq!(d["mensajes"][0]["de"], d["participantes"][0]["id"]);
        assert_eq!(d["mensajes"][0]["tipo"], "sincrono");
        assert!(
            normalizar(
                "secuencia",
                &json!({ "participantes": ["A"], "mensajes": [{ "de": "A", "a": "Z" }] })
            )
            .is_err()
        );
    }

    #[test]
    fn una_referencia_o_una_imagen_no_salen_de_lo_suyo() {
        for ruta in [
            "../../etc/passwd",
            "/etc/passwd",
            "a/../b",
            "https://mal.example/x",
            "a\\b",
        ] {
            assert!(
                normalizar("referencia", &json!({ "ruta": ruta })).is_err(),
                "{ruta}"
            );
        }
        assert!(normalizar("referencia", &json!({ "ruta": "rutinas/pecho.md" })).is_ok());
        for r in [
            "https://mal.example/x.png",
            "../x.png",
            "a.b.png",
            "x.svg",
            "x",
        ] {
            assert!(
                normalizar("imagen", &json!({ "recurso": r })).is_err(),
                "{r}"
            );
        }
        assert!(normalizar("imagen", &json!({ "recurso": "ab12.png" })).is_ok());
    }

    #[test]
    fn un_texto_largo_nace_con_el_alto_de_lo_que_lleva() {
        let corto = normalizar("texto", &json!({ "texto": "hola" })).unwrap();
        assert_eq!(tamano_de_partida("texto", &corto), (264.0, 66.0));
        let largo = normalizar("texto", &json!({ "texto": "palabra ".repeat(100) })).unwrap();
        assert!(tamano_de_partida("texto", &largo).1 > 300.0);
        let nota = normalizar("adhesiva", &json!({ "texto": "x".repeat(600) })).unwrap();
        assert!(tamano_de_partida("adhesiva", &nota).1 > 176.0);
    }

    #[test]
    fn la_caja_crece_con_el_texto_y_cae_en_la_reticula() {
        let (w, h) = caja_de_texto("Chunks", None);
        assert_eq!((w % 22.0, h % 22.0), (0.0, 0.0));
        let (w2, h2) = caja_de_texto(
            "un texto bastante más largo que necesita varias líneas seguro",
            None,
        );
        assert!(w2 >= w && h2 > h);
        let (_, hr) = caja_de_texto("¿Hay índice?", Some("rombo"));
        assert!(hr > h, "un rombo deja menos sitio al texto");
    }
}
