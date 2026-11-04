//! El lienzo de Notas en la app (`docs/spec-lienzo-elementos.md`). El motor y sus reglas están en
//! `nexo_core::lienzo`; aquí, lo que lo une con todo lo demás:
//!
//! - los comandos de la interfaz (`canvas_*`), donde el que pide es siempre el usuario;
//! - la orden de control `nexo ctl lienzo …` (`orden`), donde el que pide es siempre el agente;
//! - el guardado periódico y el barrido de capas que se quedaron a medias (`keep_saving`);
//! - las imágenes pegadas, en `.nexo/lienzos/recursos/`.
//!
//! La interfaz solo oye lo que no ha pedido ella (`canvas-changed`): lo suyo le llega en la
//! respuesta. Lo del agente nunca le mueve la vista, el foco ni la selección.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use nexo_core::lienzo::almacen::{Almacen, escritorio_activo};
use nexo_core::lienzo::datos::recurso_valido;
use nexo_core::lienzo::{Cambios, Capa, MovCarta, Quien, Resultado};
use serde::Deserialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::control::Nexo;

/// Hasta aquí una imagen pegada.
const MAX_IMAGEN: usize = 20 * 1024 * 1024;

pub(crate) fn ahora_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------- la interfaz: el usuario

#[tauri::command]
pub fn canvas_load(
    app: AppHandle,
    almacen: State<'_, Almacen>,
    desk: String,
) -> Result<Value, String> {
    let raiz = crate::vault::vault_root(&app)?;
    // Una interfaz recién cargada no tiene ningún gesto en curso.
    almacen.soltar_todo();
    let datos = almacen.con(
        &raiz,
        &desk,
        |l| json!({ "rev": l.rev, "capas": l.capas, "elementos": l.elementos }),
    )?;
    Ok(json!({ "lienzo": datos, "avisos": almacen.avisos() }))
}

#[tauri::command]
pub fn canvas_apply(
    app: AppHandle,
    almacen: State<'_, Almacen>,
    desk: String,
    ops: Vec<Value>,
) -> Result<Resultado, String> {
    let raiz = crate::vault::vault_root(&app)?;
    almacen.con(&raiz, &desk, |l| {
        l.aplicar(Quien::Usuario, &ops, ahora_ms())
    })
}

/// El usuario empieza o acaba un gesto: mientras dure, el agente no puede tocar eso.
#[tauri::command]
pub fn canvas_use(
    app: AppHandle,
    almacen: State<'_, Almacen>,
    desk: String,
    key: String,
    on: bool,
) -> Result<(), String> {
    let raiz = crate::vault::vault_root(&app)?;
    almacen.con(&raiz, &desk, |l| l.usar(&key, on))
}

/// Se borró un escritorio archivado: sus elementos van a la papelera (no están en ningún otro
/// sitio, a diferencia de sus notas).
#[tauri::command]
pub fn canvas_forget(
    app: AppHandle,
    almacen: State<'_, Almacen>,
    desk: String,
) -> Result<(), String> {
    let raiz = crate::vault::vault_root(&app)?;
    let ruta = almacen.olvidar(&raiz, &desk)?;
    if ruta.exists() {
        trash::delete(&ruta).map_err(|e| format!("No se pudo mover a la papelera: {e}"))?;
    }
    Ok(())
}

/// Cuántos elementos tiene cada escritorio, para la lista de escritorios.
#[tauri::command]
pub fn canvas_counts(app: AppHandle, almacen: State<'_, Almacen>) -> Result<Value, String> {
    let raiz = crate::vault::vault_root(&app)?;
    let mut out = serde_json::Map::new();
    let archivos = fs::read_dir(raiz.join(".nexo/lienzos"))
        .map(|l| l.flatten().map(|e| e.path()).collect::<Vec<_>>())
        .unwrap_or_default();
    for ruta in archivos {
        let Some(id) = ruta
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".json"))
        else {
            continue;
        };
        // Lo apartado por no entenderse lleva otro punto, y no pasa por aquí.
        if let Ok(n) = almacen.con(&raiz, id, |l| l.elementos.len()) {
            out.insert(id.to_string(), n.into());
        }
    }
    Ok(Value::Object(out))
}

/// Acabó el turno del agente del chat: lo que siguiera abierto ya no se está dibujando.
#[tauri::command]
pub fn canvas_turn_end(app: AppHandle, almacen: State<'_, Almacen>) {
    avisar_barrido(&app, almacen.barrer(None));
}

fn avisar_barrido(app: &AppHandle, capas: Vec<(String, Capa)>) {
    for (desk, capa) in capas {
        let cambios = Cambios {
            capas: vec![capa],
            ..Cambios::default()
        };
        let _ = app.emit(
            "canvas-changed",
            json!({ "escritorio": desk, "cambios": cambios }),
        );
    }
}

fn recursos(raiz: &Path) -> PathBuf {
    raiz.join(".nexo/lienzos/recursos")
}

/// El nombre de una imagen sale de su contenido: la misma imagen pegada dos veces es un archivo.
fn nombre_de_imagen(bytes: &[u8], ext: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}-{:x}.{ext}", bytes.len())
}

fn tipo_de_imagen(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("jpg")
    } else if bytes.starts_with(b"GIF8") {
        Some("gif")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

fn guardar_imagen(raiz: &Path, datos_b64: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(datos_b64.trim())
        .map_err(|_| "La imagen no se entiende.".to_string())?;
    if bytes.len() > MAX_IMAGEN {
        return Err("La imagen pasa de 20 MB.".into());
    }
    let ext =
        tipo_de_imagen(&bytes).ok_or("Solo se pueden pegar imágenes PNG, JPG, GIF o WebP.")?;
    let nombre = nombre_de_imagen(&bytes, ext);
    let carpeta = recursos(raiz);
    fs::create_dir_all(&carpeta).map_err(|e| e.to_string())?;
    let ruta = carpeta.join(&nombre);
    if !ruta.exists() {
        fs::write(&ruta, &bytes).map_err(|e| e.to_string())?;
    }
    Ok(nombre)
}

fn leer_imagen(raiz: &Path, recurso: &str) -> Result<String, String> {
    if !recurso_valido(recurso) {
        return Err("Ese recurso no vale.".into());
    }
    let bytes = fs::read(recursos(raiz).join(recurso)).map_err(|_| "Ya no existe.".to_string())?;
    let tipo = match recurso.rsplit('.').next() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "image/png",
    };
    Ok(format!(
        "data:{tipo};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[tauri::command]
pub fn canvas_image_save(app: AppHandle, data: String) -> Result<String, String> {
    guardar_imagen(&crate::vault::vault_root(&app)?, &data)
}

/// Se sirve como `data:`: así no hace falta tocar la CSP para enseñar un archivo local.
#[tauri::command]
pub fn canvas_image_read(app: AppHandle, resource: String) -> Result<String, String> {
    leer_imagen(&crate::vault::vault_root(&app)?, &resource)
}

/// Guarda cada poco lo que haya cambiado y da por cortado lo que lleva rato sin moverse.
pub(crate) fn keep_saving(app: AppHandle) {
    std::thread::spawn(move || {
        let mut vueltas = 0u32;
        loop {
            std::thread::sleep(Duration::from_millis(400));
            let almacen = app.state::<Almacen>();
            almacen.guardar();
            vueltas = vueltas.wrapping_add(1);
            if vueltas.is_multiple_of(75) {
                avisar_barrido(&app, almacen.barrer(Some(ahora_ms())));
            }
        }
    });
}

// ---------------------------------------------------------------- la orden: el agente

/// Lo que cambió el agente, para que la interfaz lo pinte. `deshacer`: lo que entra en el Ctrl+Z
/// del usuario (ordenar y lo que le pidió cambiar de lo suyo).
fn avisar(
    nexo: &dyn Nexo,
    escritorio: &str,
    r: &Resultado,
    deshacer: Option<&[Value]>,
    cartas: &[MovCarta],
) {
    if r.cambios.vacio() && cartas.is_empty() {
        return;
    }
    nexo.lienzo_aviso(
        "canvas-changed",
        json!({
            "escritorio": escritorio,
            "rev": r.rev,
            "cambios": r.cambios,
            "deshacer": deshacer,
            "cartas": cartas,
        }),
    );
}

/// Lo que le interesa al agente de un lote: no le hace falta lo que se repinta ni cómo deshacer.
fn para_el_agente(r: &Resultado) -> Value {
    let mut v = json!({
        "rev": r.rev,
        "hechas": r.hechas,
        "aplicadas": r.aplicadas,
        "omitidas": r.omitidas,
        "usuario_cambio": r.usuario_cambio,
    });
    if let Some((dx, dy)) = r.desplazado {
        v["desplazado"] = json!({ "dx": dx, "dy": dy });
    }
    v
}

#[derive(Deserialize)]
#[serde(untagged)]
enum LoteDeOps {
    Lista(Vec<Value>),
    Objeto { ops: Vec<Value> },
}

fn texto<'a>(req: &'a Value, clave: &str) -> Option<&'a str> {
    req[clave].as_str().map(str::trim).filter(|s| !s.is_empty())
}

/// `nexo ctl lienzo …`. El que pide es siempre el agente, nunca el usuario.
pub(crate) fn orden(nexo: &dyn Nexo, req: &Value) -> Result<Value, String> {
    let raiz = nexo.vault()?;
    let almacen = nexo.lienzos();
    let ahora = ahora_ms();
    let capa = texto(req, "capa");
    let escritorio = |capa: Option<&str>| match capa {
        Some(c) => almacen.escritorio_de_capa(&raiz, c),
        None => Ok(escritorio_activo(&raiz)),
    };
    match texto(req, "sub").unwrap_or_default() {
        "estado" => {
            let desk = escritorio(None)?;
            almacen.con(&raiz, &desk, |l| {
                json!({
                    "escritorio": desk,
                    "rev": l.rev,
                    "capas": l.resumen_capas(),
                    "elementos": l.elementos.len(),
                    "notas": l.leer(None, Some(u64::MAX))["notas"],
                })
            })
        }
        "capa-nueva" => {
            let nombre = texto(req, "nombre").ok_or("Falta el nombre de la capa.")?;
            let desk = escritorio(None)?;
            let c = almacen.con(&raiz, &desk, |l| l.nueva_capa(nombre, ahora))?;
            nexo.lienzo_aviso(
                "canvas-changed",
                json!({ "escritorio": desk, "cambios": Cambios { capas: vec![c.clone()], ..Cambios::default() } }),
            );
            Ok(json!({ "capa": c.id, "nombre": c.nombre, "escritorio": desk }))
        }
        "capa-terminar" => {
            let c = capa.ok_or("Falta la capa.")?;
            let desk = escritorio(Some(c))?;
            let hecha = almacen.con(&raiz, &desk, |l| l.terminar_capa(c))?;
            if let Some(hecha) = hecha {
                nexo.lienzo_aviso(
                    "canvas-changed",
                    json!({ "escritorio": desk, "cambios": Cambios { capas: vec![hecha], ..Cambios::default() } }),
                );
            }
            Ok(json!({ "capa": c, "estado": "terminada" }))
        }
        "zona" => {
            let w = req["ancho"].as_f64().ok_or("Falta --ancho.")?;
            let h = req["alto"].as_f64().ok_or("Falta --alto.")?;
            let desk = escritorio(capa)?;
            let cerca = texto(req, "cerca-de");
            let z = almacen.con(&raiz, &desk, |l| l.reservar_zona(w, h, cerca, ahora))?;
            serde_json::to_value(z).map_err(|e| e.to_string())
        }
        "aplicar" => {
            let c = capa.ok_or("Falta la capa.")?;
            let desk = escritorio(Some(c))?;
            let ops = match serde_json::from_value::<LoteDeOps>(req["ops"].clone()) {
                Ok(LoteDeOps::Lista(ops) | LoteDeOps::Objeto { ops }) => ops,
                Err(_) => return Err("El lote va como una lista de operaciones.".into()),
            };
            let del_usuario = req["del-usuario"] == true;
            let r = almacen.con(&raiz, &desk, |l| {
                l.aplicar(
                    Quien::Agente {
                        capa: c,
                        del_usuario,
                    },
                    &ops,
                    ahora,
                )
            })?;
            let deshacer = del_usuario.then_some(r.inversas.as_slice());
            avisar(nexo, &desk, &r, deshacer, &[]);
            Ok(para_el_agente(&r))
        }
        "diagrama" => {
            let c = capa.ok_or("Falta la capa.")?;
            let desk = escritorio(Some(c))?;
            let r = almacen.con(&raiz, &desk, |l| l.diagrama(c, &req["diagrama"], ahora))??;
            avisar(nexo, &desk, &r, None, &[]);
            Ok(para_el_agente(&r))
        }
        "leer" => {
            let desk = escritorio(capa)?;
            let desde = req["desde"].as_u64();
            almacen.con(&raiz, &desk, |l| {
                if req["texto"] == true {
                    json!({ "escritorio": desk, "texto": l.leer_texto(capa, desde) })
                } else {
                    let mut v = l.leer(capa, desde);
                    v["escritorio"] = json!(desk);
                    v
                }
            })
        }
        "ordenar" => {
            let modo = texto(req, "modo").unwrap_or("rejilla");
            let desk = escritorio(capa)?;
            let ids: Option<Vec<String>> = texto(req, "ids").map(|s| {
                s.split(',')
                    .map(|i| i.trim().to_string())
                    .filter(|i| !i.is_empty())
                    .collect()
            });
            let con_cartas = req["notas"] == true;
            let o = almacen.con(&raiz, &desk, |l| {
                l.ordenar(modo, ids.as_deref(), capa, con_cartas, ahora)
            })??;
            avisar(
                nexo,
                &desk,
                &o.resultado,
                Some(&o.resultado.inversas),
                &o.cartas,
            );
            Ok(json!({
                "rev": o.resultado.rev,
                "movidos": o.resultado.aplicadas,
                "notas_movidas": o.cartas.len(),
                "omitidas": o.resultado.omitidas,
            }))
        }
        "ir-a" => {
            let id = texto(req, "id").ok_or("Falta a qué elemento o capa ir.")?;
            let desk = match escritorio(Some(id)) {
                Ok(desk) => desk,
                Err(_) => escritorio(capa)?,
            };
            let existe = almacen.con(&raiz, &desk, |l| {
                l.capa(id).is_some() || l.elemento(id).is_some()
            })?;
            if !existe {
                return Err(format!("«{id}» ya no existe."));
            }
            nexo.lienzo_aviso("canvas-goto", json!({ "escritorio": desk, "id": id }));
            Ok(json!({ "id": id, "escritorio": desk }))
        }
        "" => Err("Falta la orden del lienzo. Prueba `nexo ctl ayuda`.".into()),
        otra => Err(format!(
            "El lienzo no tiene la orden «{otra}». Prueba `nexo ctl ayuda`."
        )),
    }
}

// ---------------------------------------------------------------- la skill

pub const SKILL_NAME: &str = "nexo-lienzo";

const SKILL: &str = r#"---
name: nexo-lienzo
description: Planear en visual en el lienzo de Notas de nexo: dibujar diagramas, arquitectura, flujos, tablas relacionales, diagramas de secuencia, tablas, tableros Scrum y líneas de tiempo, y ordenar el lienzo. Úsala cuando el usuario quiera planear, diagramar o ver algo de forma visual o en el lienzo, o pida ordenar el lienzo.
---
<!-- nexo: esta skill la mantiene nexo; si quitas esta línea, deja de actualizarla -->

# Dibujar en el lienzo de nexo

El lienzo es el fondo de la vista Notas. Lo que dibujas aquí **no son notas**: no crea archivos en la wiki ni sale en el grafo. Dibujas en **tu propia capa**, a la vez que el usuario trabaja, y nexo se encarga de que no os estorbéis.

Todo se hace con la orden de control. La respuesta es JSON. Si dice que nexo no está abierto, díselo al usuario: no lo abras tú. Nunca edites `.nexo/lienzos/` a mano.

```
{nexo} ctl lienzo <orden> …
```

| Para | Orden |
|---|---|
| Ver qué hay: capas, elementos, notas abiertas | `{nexo} ctl lienzo estado` |
| Empezar tu capa (devuelve su id) | `{nexo} ctl lienzo capa-nueva "RAG del grafo"` |
| Pedir un hueco libre | `{nexo} ctl lienzo zona --ancho 1320 --alto 660 [--cerca-de <id>]` |
| Dibujar un diagrama sin coordenadas | `{nexo} ctl lienzo diagrama <capa> '<json>'` |
| Crear o cambiar elementos | `{nexo} ctl lienzo aplicar <capa> '<json>'` |
| Leer cómo está, también lo que cambió el usuario | `{nexo} ctl lienzo leer [--capa <capa>] [--desde <rev>] --texto` |
| Ordenar lo que hay | `{nexo} ctl lienzo ordenar fila\|rejilla\|flujo [--capa <capa>] [--ids a,b] [--notas]` |
| Llevar al usuario a algo | `{nexo} ctl lienzo ir-a <id o capa>` |
| Acabar | `{nexo} ctl lienzo capa-terminar <capa>` |

**Escribe cada orden entera, sola y con su ruta tal cual:** sin variables (`NEXO=…`), sin `&&` ni `|`, y sin `<<'FIN'`. **El JSON va como un argumento entre comillas simples.** Así no le pide permiso al usuario por cada trazo; de otra forma, sí. Puede ocupar varias líneas. Un apóstrofo dentro del texto se escribe `\u0027`, nunca `'\''`.

## Cómo trabajar

1. **Lee antes de dibujar.** Busca en la wiki con lo que ya sabes hacer y, si hace falta código de un proyecto, delega con la skill `nexo-orquestacion`. No inventes: lo que supongas, márcalo «por confirmar» en el texto.
2. **Si va a ser algo grande, dilo antes:** gasta de la sesión de 5 h del usuario.
3. **`capa-nueva`** con un nombre claro. Es el nombre que verá el usuario.
4. **Por secciones, cada una con su marco y su título,** de izquierda a derecha. Para un plan: Contexto → Arquitectura → Datos → Flujo → Plan → Decisiones. Elige la forma por la pregunta:

| La pregunta | El elemento |
|---|---|
| ¿Qué piezas hay y cómo se unen? | `diagrama` con figuras de arquitectura |
| ¿Qué pasos y decisiones hay? | `diagrama` con rombos para las decisiones |
| ¿Qué datos hay? | `entidad` (tablas relacionales) con flechas de cardinalidad |
| ¿Quién habla con quién y en qué orden? | `secuencia` |
| ¿Qué hay que hacer? | `tablero` |
| ¿Cuándo? ¿Qué depende de qué? | `linea-tiempo` |
| ¿Cuál elegimos? | `tabla` con opciones por criterios |
| Un dato, una lista, una comparación | `tabla` o `texto` |
| ¿Dónde está lo que ya sé? | `referencia` a la nota o el nodo |

5. **Por tandas de 20 o 30 operaciones,** no todo de golpe: el usuario lo ve aparecer y te puede corregir a tiempo.
6. **Mira cada respuesta.** `omitidas` dice lo que no se aplicó y por qué; `usuario_cambio`, lo que el usuario tocó de tu capa. **Lo que cambió es suyo: no lo rehagas ni lo discutas.** Sigue con lo demás, teniéndolo en cuenta.
7. **`capa-terminar`** y di en el chat qué dibujaste y dónde está. No lleves la vista allí (`ir-a`) salvo que te lo pida.

## Reglas

- **Nunca muevas al usuario:** ni la vista, ni nada de lo suyo. `ir-a` solo si te lo pide.
- **Lo suyo no se toca.** `--del-usuario` en `aplicar` solo si te pidió en la conversación cambiar algo concreto suyo; `ordenar` solo si te pidió ordenar. Lo que tenga en las manos no lo puedes cambiar de ninguna forma.
- **Las capas las gestiona él:** integrarlas o borrarlas es cosa suya. Si tu capa ya no existe, no insistas: dilo.
- **Abrir una nota para enseñarla** sigue como siempre (`.nexo/escritorio.json`); para citarla en un plan, usa `referencia`.

## `diagrama`: sin coordenadas

```
{nexo} ctl lienzo diagrama c1 '{
  "titulo": "Arquitectura",
  "direccion": "derecha",
  "grupos": [{ "ref": "idx", "titulo": "Índice" }],
  "nodos": [
    { "ref": "chat", "texto": "Chat de Nexo", "forma": "servicio", "color": "azul" },
    { "ref": "motor", "texto": "Motor de búsqueda", "forma": "servicio" },
    { "ref": "vec", "texto": "Vectores", "forma": "cilindro", "grupo": "idx" },
    { "ref": "dudas", "texto": "¿Reindexar? (por confirmar)", "forma": "rombo", "color": "ambar" }
  ],
  "aristas": [
    { "de": "chat", "a": "motor", "texto": "pregunta" },
    { "de": "motor", "a": "vec", "trazo": "discontinuo" }
  ]
}'
```

`direccion`: `derecha` o `abajo`. Se coloca solo en un hueco libre, o en `"en": { "x": …, "y": … }`. Un nodo también puede ser otro elemento: `{ "ref": "users", "tipo": "entidad", "datos": { … } }`. Una arista lleva `de_columna` y `a_columna` para unir filas de dos entidades. Acaba en elementos normales: el usuario puede tocar cada pieza.

## `aplicar`: un lote de operaciones

```
{nexo} ctl lienzo aplicar c1 '[
  { "op": "crear", "ref": "a", "tipo": "marco", "x": 0, "y": 0, "w": 660, "h": 440, "datos": { "titulo": "Datos" } },
  { "op": "crear", "ref": "u", "tipo": "entidad", "x": 44, "y": 66, "datos": { "nombre": "chunks", "color": "azul", "columnas": [
    { "nombre": "id", "tipo": "uuid", "pk": true },
    { "nombre": "nota_id", "tipo": "uuid", "fk": true }
  ] } },
  { "op": "crear", "tipo": "flecha", "datos": { "desde": { "id": "u" }, "hasta": { "id": "e7" }, "inicio": "uno", "fin": "muchos" } }
]'
```

Una `ref` vale como id dentro del mismo lote; `hechas` dice qué id le puso nexo a cada una. Sin `x` e `y`, nexo le busca sitio. La retícula es de 22 px: usa múltiplos. **Pide antes una `zona`:** si un lote con coordenadas cae encima de algo del usuario, nexo lo mueve entero a un hueco y la respuesta trae `desplazado` (`dx`, `dy`): súmalo a lo que dibujes después en esa sección.

Operaciones: `crear` (`tipo`, `x`, `y`, `w`, `h`, `datos`), `cambiar` (`id`, lo que cambie; `datos` se mezcla con lo que hay), `borrar` (`id`), `apilar` (`id`, `a`: `frente` o `fondo`). Tablas: `celdas` (`id`, `celdas`: `[{ "fila", "col", "valor" }]`), `fila-nueva` (`id`, `valores`, `pos`), `fila-borrar`, `columna-nueva`, `columna-borrar`, `columna-cambiar`. Tableros: `tarjeta-nueva` (`id`, `col`, `titulo`, `puntos`, `responsable`, `etiqueta`), `tarjeta-cambiar` (`id`, `tarjeta`, …), `tarjeta-mover` (`id`, `tarjeta`, `col`, `pos`), `tarjeta-borrar`, `columna-nueva`, `columna-cambiar`. Las `fila`, `col` y `tarjeta` son los ids que da `leer`; en un tablero, `col` vale también por su título.

## Qué lleva cada elemento en `datos`

Los colores son roles: `neutro`, `acento`, `azul`, `verde`, `ambar`, `rojo`. Úsalos con mesura y con un sentido (por ejemplo, ámbar para lo que está por confirmar). Nunca hexadecimales.

- `texto`: `texto` (admite Markdown), `color`, `tamano` (`s`, `m`, `l`, `xl`), `alineado` (`izq`, `centro`, `der`).
- `figura`: `texto`, `color`, `forma`: `rectangulo`, `redondeado`, `elipse`, `rombo`, `paralelogramo`, `hexagono`, `documento`, `nube`, `cilindro`, `inicio`, y de arquitectura `servicio`, `cola`, `cache`, `api`, `persona`, `externo`.
- `flecha`: `desde` y `hasta` (`{ "id", "ancla": "n|e|s|o", "columna" }` o `{ "x", "y" }`), `ruta` (`angulada`, `recta`, `curva`), `inicio` y `fin` (`nada`, `flecha`, `triangulo`, `rombo`, `uno`, `muchos`, `cero-muchos`), `trazo` (`continuo`, `discontinuo`), `texto`, `color`.
- `marco`: `titulo`, `color`. Lo que quede dentro se mueve con él.
- `adhesiva`: `texto`, `color`.
- `tabla`: `titulo`, `columnas` (`["Opción", "Coste"]`), `filas` (`[["A", "3"], ["B", "4"], ["Total", "=SUMA(B2:B3)"]]`). Con cabecera, la fila 1 es la cabecera y los datos empiezan en la 2. Fórmulas: `SUMA`, `PROMEDIO`, `MIN`, `MAX`, `CONTAR`, `SI`, aritmética y referencias como `B2`. Hasta 200 filas por 30 columnas.
- `tablero`: `titulo`, `columnas` (por defecto Por hacer, En curso, Hecho; también `{ "titulo", "limite" }`), `tarjetas` (`[{ "titulo", "col", "puntos", "responsable", "etiqueta" }]`).
- `entidad`: `nombre`, `color`, `columnas` (`[{ "nombre", "tipo", "pk", "fk", "nulo", "unico" }]`).
- `secuencia`: `titulo`, `participantes` (`["Chat", "Motor"]`), `mensajes` (`[{ "de", "a", "texto", "tipo": "sincrono|asincrono|respuesta" }]`).
- `codigo`: `lenguaje`, `texto`, `titulo`.
- `linea-tiempo`: `titulo`, `tareas` (`[{ "id", "titulo", "inicio": "AAAA-MM-DD", "fin", "dep": ["id"], "avance": 40 }]`), `hitos` (`[{ "titulo", "fecha" }]`).
- `referencia`: `tipo` (`nota`, `nodo`, `fuente`), `ruta` (relativa a `wiki/notas/` en una nota, a `wiki/` en un nodo, a `raw/` en una fuente), `titulo`.

Las imágenes solo las pone el usuario: tú no puedes traer una de fuera.
"#;

pub(crate) fn skill_text(exe: &Path) -> String {
    SKILL.replace("{nexo}", &crate::orquestacion::command_of(exe))
}

/// Deja la skill en el vault, con las mismas condiciones que `nexo-orquestacion`.
pub fn install_skill(root: &Path, exe: &Path) -> std::io::Result<bool> {
    crate::orquestacion::write_managed(root, SKILL_NAME, &skill_text(exe))
}

/// La regla que deja al chat usar `nexo ctl lienzo` sin pedir permiso por cada trazo: esa orden
/// solo escribe en los lienzos del vault. Comprobado con Claude Code 2.1.308 y un turno de Haiku:
/// casa con un argumento entre comillas simples, también en varias líneas y con `;`, `&&` o `$`
/// dentro; no casa con un heredoc ni con el escape `'\''`, y no deja pasar otra orden de `nexo ctl`.
/// Si la ruta no es de las que se escriben sin comillas no se pone, y el chat pregunta como siempre.
pub(crate) fn permission_rule(exe: &Path) -> Option<String> {
    let ruta = exe.to_str()?;
    ruta.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c))
        .then(|| format!("Bash({ruta} ctl lienzo:*)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_skill_lleva_la_ruta_de_nexo_y_todas_las_ordenes() {
        let text = skill_text(Path::new("/x/nexo"));
        assert!(text.starts_with("---\nname: nexo-lienzo\n"));
        assert!(text.contains(crate::orquestacion::MARK));
        assert!(!text.contains("{nexo}"));
        for orden in [
            "estado",
            "capa-nueva",
            "zona",
            "diagrama",
            "aplicar",
            "leer",
            "ordenar",
            "ir-a",
            "capa-terminar",
        ] {
            assert!(
                text.contains(&format!("/x/nexo ctl lienzo {orden}")),
                "{orden}"
            );
        }
        // Lo que aprendimos de la regla de permiso tiene que estar dicho.
        assert!(text.contains(r"\u0027"));
        assert!(text.contains("comillas simples"));
    }

    #[test]
    fn la_skill_solo_se_reescribe_si_es_suya() {
        let root = std::env::temp_dir().join(format!("nexo-lienzo-skill-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let exe = Path::new("/x/nexo");
        assert!(install_skill(&root, exe).unwrap());
        assert!(!install_skill(&root, exe).unwrap(), "igual: no se toca");
        assert!(
            install_skill(&root, Path::new("/y/nexo")).unwrap(),
            "otra ruta: se pone al día"
        );
        let ruta = root.join(".claude/skills/nexo-lienzo/SKILL.md");
        fs::write(&ruta, "---\nname: nexo-lienzo\n---\nMía\n").unwrap();
        assert!(
            !install_skill(&root, exe).unwrap(),
            "sin la marca es del usuario"
        );
        assert!(fs::read_to_string(&ruta).unwrap().contains("Mía"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn la_regla_de_permiso_solo_abarca_el_lienzo() {
        assert_eq!(
            permission_rule(Path::new("/home/ana/.local/bin/nexo")).unwrap(),
            "Bash(/home/ana/.local/bin/nexo ctl lienzo:*)"
        );
        assert!(permission_rule(Path::new("/opt/mi nexo/nexo")).is_none());
        assert!(permission_rule(Path::new("/x/ne(xo)")).is_none());
    }

    #[test]
    fn una_imagen_se_guarda_por_su_contenido_y_solo_si_es_una_imagen() {
        let root = std::env::temp_dir().join(format!("nexo-lienzo-img-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        let png = b"\x89PNG\r\n\x1a\nresto";
        let a = guardar_imagen(&root, &b64(png)).unwrap();
        assert!(a.ends_with(".png") && recurso_valido(&a), "{a}");
        assert_eq!(
            guardar_imagen(&root, &b64(png)).unwrap(),
            a,
            "la misma, el mismo archivo"
        );
        assert!(
            leer_imagen(&root, &a)
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        assert!(guardar_imagen(&root, &b64(b"<svg onload=alert(1)>")).is_err());
        assert!(guardar_imagen(&root, "no es base64 !!").is_err());
        for mal in ["../../etc/passwd", "../x.png", "x.exe", "a/b.png"] {
            assert!(leer_imagen(&root, mal).is_err(), "{mal}");
        }
        let _ = fs::remove_dir_all(&root);
    }
}
