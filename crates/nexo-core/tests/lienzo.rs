//! El lienzo con palabras del dominio: qué pasa cuando el usuario y el agente trabajan a la vez.

use std::fs;
use std::path::PathBuf;

use nexo_core::lienzo::almacen::{Almacen, ESCRITORIO_PRINCIPAL, escritorio_activo, ruta_lienzo};
use nexo_core::lienzo::{CAPA_USUARIO, EstadoCapa, Lienzo, MAX_ELEMENTOS, MAX_LOTE, Quien, Rect};
use serde_json::{Value, json};

const AHORA: u64 = 1_000_000;

fn agente(capa: &str) -> Quien<'_> {
    Quien::Agente {
        capa,
        del_usuario: false,
    }
}

fn caja(texto: &str) -> Value {
    json!({ "op": "crear", "ref": texto, "tipo": "figura", "x": 0, "y": 0, "datos": { "texto": texto } })
}

/// Un lienzo con una capa del agente y una caja suya.
fn con_caja() -> (Lienzo, String, String) {
    let mut l = Lienzo::default();
    let capa = l.nueva_capa("RAG del grafo", AHORA).id;
    let r = l.aplicar(agente(&capa), &[caja("A")], AHORA);
    let id = r.hechas[0]["id"].as_str().unwrap().to_string();
    (l, capa, id)
}

fn temporal(nombre: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nexo-lienzo-{nombre}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".nexo")).unwrap();
    dir
}

// ------------------------------------------------------------ convivencia

#[test]
fn el_agente_no_puede_cambiar_lo_que_el_usuario_toco_y_se_entera() {
    let (mut l, capa, id) = con_caja();
    let r = l.aplicar(
        Quien::Usuario,
        &[json!({ "op": "cambiar", "id": id, "x": 500 })],
        AHORA,
    );
    assert_eq!(r.aplicadas, 1);

    let r = l.aplicar(
        agente(&capa),
        &[json!({ "op": "cambiar", "id": id, "datos": { "texto": "Otro" } })],
        AHORA,
    );
    assert_eq!(r.aplicadas, 0);
    assert!(
        r.omitidas[0]["motivo"]
            .as_str()
            .unwrap()
            .contains("usuario"),
        "{r:?}"
    );
    assert_eq!(
        r.omitidas[0]["actual"]["x"], 500.0,
        "se le dice cómo está ahora"
    );
    assert_eq!(l.elemento(&id).unwrap().datos["texto"], "A");
    assert_eq!(r.usuario_cambio.len(), 1, "y lo que hizo el usuario");
    assert_eq!(r.usuario_cambio[0]["id"], id);

    // Solo se lo dice una vez.
    let r = l.aplicar(agente(&capa), &[], AHORA);
    assert!(r.usuario_cambio.is_empty());
}

#[test]
fn si_el_usuario_borra_algo_el_resto_del_lote_sigue() {
    let (mut l, capa, id) = con_caja();
    l.aplicar(
        Quien::Usuario,
        &[json!({ "op": "borrar", "id": id })],
        AHORA,
    );
    let r = l.aplicar(
        agente(&capa),
        &[
            caja("B"),
            json!({ "op": "crear", "tipo": "flecha", "datos": { "desde": { "id": "B" }, "hasta": { "id": id } } }),
            caja("C"),
            json!({ "op": "crear", "tipo": "flecha", "datos": { "desde": { "id": "B" }, "hasta": { "id": "C" } } }),
        ],
        AHORA,
    );
    assert_eq!(r.aplicadas, 3, "{r:?}");
    assert_eq!(r.omitidas.len(), 1);
    assert!(
        r.omitidas[0]["motivo"]
            .as_str()
            .unwrap()
            .contains("ya no existe")
    );
    assert_eq!(r.usuario_cambio[0]["que"], "lo borró");
    let flecha = l.elementos.iter().find(|e| e.tipo == "flecha").unwrap();
    assert_eq!(
        flecha.datos["desde"]["id"], r.hechas[0]["id"],
        "la ref se cambió por su id"
    );
}

#[test]
fn lo_que_esta_en_uso_no_cambia_ni_con_permiso() {
    let (mut l, capa, id) = con_caja();
    l.usar(&id, true);
    for del_usuario in [false, true] {
        let q = Quien::Agente {
            capa: &capa,
            del_usuario,
        };
        for op in [
            json!({ "op": "cambiar", "id": id, "x": 9 }),
            json!({ "op": "borrar", "id": id }),
        ] {
            let r = l.aplicar(q, &[op], AHORA);
            assert_eq!(r.aplicadas, 0, "{r:?}");
        }
    }
    // Una orden de ordenar tampoco lo mueve.
    let r = l.aplicar(
        Quien::Orden,
        &[json!({ "op": "cambiar", "id": id, "x": 9 })],
        AHORA,
    );
    assert_eq!(r.aplicadas, 0);
    l.usar(&id, false);
    let r = l.aplicar(
        agente(&capa),
        &[json!({ "op": "cambiar", "id": id, "x": 9 })],
        AHORA,
    );
    assert_eq!(r.aplicadas, 1, "al soltarlo vuelve a estar libre");
}

#[test]
fn el_agente_no_toca_lo_del_usuario_salvo_que_se_lo_haya_pedido() {
    let mut l = Lienzo::default();
    let capa = l.nueva_capa("Plan", AHORA).id;
    let r = l.aplicar(Quien::Usuario, &[caja("Mía")], AHORA);
    let mia = r.hechas[0]["id"].as_str().unwrap().to_string();
    assert_eq!(l.elemento(&mia).unwrap().capa, CAPA_USUARIO);

    let cambio = json!({ "op": "cambiar", "id": mia, "datos": { "texto": "Suya" } });
    let r = l.aplicar(agente(&capa), std::slice::from_ref(&cambio), AHORA);
    assert_eq!(r.aplicadas, 0);
    assert!(
        r.omitidas[0]["motivo"]
            .as_str()
            .unwrap()
            .contains("--del-usuario")
    );

    let r = l.aplicar(
        Quien::Agente {
            capa: &capa,
            del_usuario: true,
        },
        &[cambio],
        AHORA,
    );
    assert_eq!(r.aplicadas, 1);
    // Y se puede deshacer.
    l.aplicar(Quien::Usuario, &r.inversas, AHORA);
    assert_eq!(l.elemento(&mia).unwrap().datos["texto"], "Mía");
}

#[test]
fn el_agente_siempre_crea_en_su_capa_y_dos_agentes_no_se_cruzan() {
    let mut l = Lienzo::default();
    let uno = l.nueva_capa("Uno", AHORA).id;
    let dos = l.nueva_capa("Dos", AHORA).id;
    let r = l.aplicar(
        agente(&uno),
        &[json!({ "op": "crear", "tipo": "texto", "capa": CAPA_USUARIO, "x": 0, "y": 0 })],
        AHORA,
    );
    let suyo = r.hechas[0]["id"].as_str().unwrap().to_string();
    assert_eq!(l.elemento(&suyo).unwrap().capa, uno, "«capa» no cuela");
    let r = l.aplicar(
        agente(&dos),
        &[json!({ "op": "borrar", "id": suyo })],
        AHORA,
    );
    assert_eq!(r.aplicadas, 0);
    assert!(
        r.omitidas[0]["motivo"]
            .as_str()
            .unwrap()
            .contains("otra capa")
    );
    // No se puede dibujar en la capa del usuario haciéndose pasar por ella.
    let r = l.aplicar(agente(CAPA_USUARIO), &[caja("X")], AHORA);
    assert_eq!(r.aplicadas, 0);
    // Las capas las gestiona el usuario.
    let r = l.aplicar(
        agente(&uno),
        &[json!({ "op": "capa-borrar", "capa": dos })],
        AHORA,
    );
    assert_eq!(r.aplicadas, 0);
    assert!(l.capa(&dos).is_some());
}

#[test]
fn en_un_tablero_lo_tocado_es_la_tarjeta_no_el_tablero() {
    let mut l = Lienzo::default();
    let capa = l.nueva_capa("Plan", AHORA).id;
    let r = l.aplicar(
        agente(&capa),
        &[json!({ "op": "crear", "tipo": "tablero", "x": 0, "y": 0, "datos": { "tarjetas": [{ "titulo": "A" }, { "titulo": "B" }] } })],
        AHORA,
    );
    let id = r.hechas[0]["id"].as_str().unwrap().to_string();
    let datos = l.elemento(&id).unwrap().datos.clone();
    let (a, b) = (
        datos["tarjetas"][0]["id"].as_str().unwrap(),
        datos["tarjetas"][1]["id"].as_str().unwrap(),
    );
    let hecho = datos["columnas"][2]["id"].as_str().unwrap();

    let r = l.aplicar(
        Quien::Usuario,
        &[json!({ "op": "tarjeta-mover", "id": id, "tarjeta": a, "col": hecho })],
        AHORA,
    );
    assert_eq!(r.aplicadas, 1, "{r:?}");

    let r = l.aplicar(
        agente(&capa),
        &[
            json!({ "op": "tarjeta-cambiar", "id": id, "tarjeta": a, "titulo": "pisada" }),
            json!({ "op": "tarjeta-cambiar", "id": id, "tarjeta": b, "titulo": "B bis" }),
            json!({ "op": "tarjeta-nueva", "id": id, "titulo": "C", "col": "Por hacer" }),
        ],
        AHORA,
    );
    assert_eq!(r.aplicadas, 2, "{r:?}");
    assert!(
        r.omitidas[0]["motivo"]
            .as_str()
            .unwrap()
            .contains("usuario")
    );
    let d = &l.elemento(&id).unwrap().datos;
    assert_eq!(d["tarjetas"].as_array().unwrap().len(), 3);
    let tarjeta = |t: &str| {
        d["tarjetas"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == t)
            .unwrap()
            .clone()
    };
    assert_eq!(tarjeta(a)["titulo"], "A");
    assert_eq!(
        tarjeta(a)["col"],
        hecho,
        "la tarjeta sigue donde la dejó el usuario"
    );
    assert_eq!(tarjeta(b)["titulo"], "B bis");
    assert_eq!(r.usuario_cambio[0]["que"], "movió una tarjeta");
}

#[test]
fn en_una_tabla_lo_tocado_es_la_celda_y_lo_que_se_esta_escribiendo_se_respeta() {
    let mut l = Lienzo::default();
    let capa = l.nueva_capa("Plan", AHORA).id;
    let r = l.aplicar(
        agente(&capa),
        &[json!({ "op": "crear", "tipo": "tabla", "x": 0, "y": 0, "datos": { "columnas": ["A", "B"], "filas": [["1", "2"], ["3", "4"]] } })],
        AHORA,
    );
    let id = r.hechas[0]["id"].as_str().unwrap().to_string();
    let d = l.elemento(&id).unwrap().datos.clone();
    let (c0, c1) = (
        d["columnas"][0]["id"].as_str().unwrap(),
        d["columnas"][1]["id"].as_str().unwrap(),
    );
    let (f0, f1) = (
        d["filas"][0]["id"].as_str().unwrap(),
        d["filas"][1]["id"].as_str().unwrap(),
    );

    l.aplicar(Quien::Usuario, &[json!({ "op": "celdas", "id": id, "celdas": [{ "fila": f0, "col": c0, "valor": "mío" }] })], AHORA);
    l.usar(&format!("{id}/{f1}:{c1}"), true);

    let r = l.aplicar(
        agente(&capa),
        &[json!({ "op": "celdas", "id": id, "celdas": [
            { "fila": f0, "col": c0, "valor": "pisada" },
            { "fila": f0, "col": c1, "valor": "libre" },
            { "fila": f1, "col": c1, "valor": "pisada" },
        ] })],
        AHORA,
    );
    // Se aplica lo que se puede y se dice lo que no.
    let d = &l.elemento(&id).unwrap().datos;
    assert_eq!(d["filas"][0]["celdas"][c0], "mío");
    assert_eq!(d["filas"][0]["celdas"][c1], "libre");
    assert_eq!(d["filas"][1]["celdas"][c1], "4");
    let motivo = r.omitidas[0]["motivo"].as_str().unwrap();
    assert!(
        motivo.contains(&format!("{f0}:{c0}")) && motivo.contains(&format!("{f1}:{c1}")),
        "{motivo}"
    );

    // Añadir una fila no choca con nadie, ni borrar una que no se tocó choca con nada.
    let r = l.aplicar(
        agente(&capa),
        &[json!({ "op": "fila-nueva", "id": id, "valores": ["5", "6"] })],
        AHORA,
    );
    assert_eq!(r.aplicadas, 1, "{r:?}");
    let r = l.aplicar(
        agente(&capa),
        &[json!({ "op": "fila-borrar", "id": id, "fila": f0 })],
        AHORA,
    );
    assert_eq!(r.aplicadas, 0, "esa fila tiene una celda del usuario");
    // Y no se puede reescribir la tabla de golpe para saltárselo.
    let r = l.aplicar(
        agente(&capa),
        &[json!({ "op": "cambiar", "id": id, "datos": { "filas": [] } })],
        AHORA,
    );
    assert_eq!(r.aplicadas, 0);
    assert_eq!(
        l.elemento(&id).unwrap().datos["filas"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn el_agente_que_vuelve_a_una_capa_cerrada_la_reabre_y_la_integrada_ya_no_es_suya() {
    let (mut l, capa, _) = con_caja();
    assert!(l.terminar_capa(&capa).is_some());
    assert_eq!(l.capa(&capa).unwrap().estado, Some(EstadoCapa::Terminada));
    let r = l.aplicar(agente(&capa), &[caja("B")], AHORA);
    assert_eq!(l.capa(&capa).unwrap().estado, Some(EstadoCapa::Dibujando));
    assert_eq!(r.cambios.capas.len(), 1, "la interfaz se entera");

    l.aplicar(
        Quien::Usuario,
        &[json!({ "op": "capa-integrar", "capa": capa })],
        AHORA,
    );
    let r = l.aplicar(agente(&capa), &[caja("C")], AHORA);
    assert_eq!(r.aplicadas, 0);
    assert!(
        r.omitidas[0]["motivo"]
            .as_str()
            .unwrap()
            .contains("ya no existe")
    );
    assert!(l.elementos.iter().all(|e| e.capa == CAPA_USUARIO));
}

#[test]
fn una_capa_abierta_queda_sin_terminar_si_se_corta() {
    let (mut l, capa, _) = con_caja();
    // Sigue activa antes del plazo…
    assert!(l.barrer(Some(AHORA + 1000)).is_empty());
    // …y se queda sin terminar si no vuelve a hacer nada.
    assert_eq!(l.barrer(Some(AHORA + 6 * 60 * 1000)).len(), 1);
    assert_eq!(l.capa(&capa).unwrap().estado, Some(EstadoCapa::SinTerminar));

    // Se acabó el turno del agente con la capa abierta.
    let otra = l.nueva_capa("Otra", AHORA).id;
    assert_eq!(l.barrer(None).len(), 1);
    assert_eq!(l.capa(&otra).unwrap().estado, Some(EstadoCapa::SinTerminar));

    // nexo se cerró a medias.
    let mut abierta = Lienzo::default();
    let id = abierta.nueva_capa("Cortada", AHORA).id;
    let leido = Lienzo::desde_json(&abierta.a_json()).unwrap();
    assert_eq!(
        leido.capa(&id).unwrap().estado,
        Some(EstadoCapa::SinTerminar)
    );
}

#[test]
fn aunque_el_agente_de_coordenadas_nunca_cae_encima_de_lo_ajeno() {
    let mut l = Lienzo::default();
    l.aplicar(
        Quien::Usuario,
        &[json!({ "op": "crear", "tipo": "figura", "x": 100, "y": 100, "w": 200, "h": 100 })],
        AHORA,
    );
    let carta = Rect {
        x: 1000.0,
        y: 1000.0,
        w: 380.0,
        h: 440.0,
    };
    l.fijar_entorno(vec![("a.md".into(), carta)], None);
    let capa = l.nueva_capa("Plan", AHORA).id;
    let mia = Rect {
        x: 100.0,
        y: 100.0,
        w: 200.0,
        h: 100.0,
    };

    // El lote entero se mueve junto y conserva su forma: el marco sigue conteniendo lo suyo.
    let r = l.aplicar(
        agente(&capa),
        &[
            json!({ "op": "crear", "ref": "m", "tipo": "marco", "x": 0, "y": 0, "w": 600, "h": 400 }),
            json!({ "op": "crear", "ref": "f", "tipo": "figura", "x": 44, "y": 66, "w": 132, "h": 66 }),
            json!({ "op": "crear", "tipo": "flecha", "datos": { "desde": { "id": "f" }, "hasta": { "x": 900, "y": 0 } } }),
        ],
        AHORA,
    );
    assert_eq!(r.aplicadas, 3, "{r:?}");
    let (dx, dy) = r.desplazado.expect("se lo tiene que decir");
    let marco = l.elemento(r.hechas[0]["id"].as_str().unwrap()).unwrap();
    let figura = l.elemento(r.hechas[1]["id"].as_str().unwrap()).unwrap();
    assert!(no_se_tocan(&rect_de(marco), &mia), "{marco:?}");
    assert!(no_se_tocan(&rect_de(marco), &carta), "{marco:?}");
    assert_eq!((figura.x - marco.x, figura.y - marco.y), (44.0, 66.0));
    assert_eq!((marco.x, marco.y), (dx, dy));

    // Encima de una carta de notas, tampoco.
    let r = l.aplicar(
        agente(&capa),
        &[json!({ "op": "crear", "tipo": "figura", "x": 1100, "y": 1100 })],
        AHORA,
    );
    assert!(r.desplazado.is_some());
    assert!(no_se_tocan(
        &rect_de(l.elemento(r.hechas[0]["id"].as_str().unwrap()).unwrap()),
        &carta
    ));

    // Donde no pisa nada, se respeta lo que pidió.
    let r = l.aplicar(
        agente(&capa),
        &[json!({ "op": "crear", "ref": "x", "tipo": "figura", "x": 5000, "y": 5000 })],
        AHORA,
    );
    assert_eq!(r.desplazado, None);
    let id = r.hechas[0]["id"].as_str().unwrap().to_string();
    assert_eq!(l.elemento(&id).unwrap().x, 5000.0);

    // Dentro de su propia capa sí puede solapar: un marco contiene cosas.
    let r = l.aplicar(
        agente(&capa),
        &[json!({ "op": "crear", "tipo": "figura", "x": 5010, "y": 5010 })],
        AHORA,
    );
    assert_eq!(r.desplazado, None);

    // Y mover lo suyo encima de lo ajeno no cuela.
    let r = l.aplicar(
        agente(&capa),
        &[json!({ "op": "cambiar", "id": id, "x": 110, "y": 110 })],
        AHORA,
    );
    assert_eq!(r.aplicadas, 0);
    assert!(r.omitidas[0]["motivo"].as_str().unwrap().contains("encima"));
}

fn rect_de(e: &nexo_core::lienzo::Elemento) -> Rect {
    Rect {
        x: e.x,
        y: e.y,
        w: e.w,
        h: e.h,
    }
}

#[test]
fn lo_enviado_al_fondo_y_los_marcos_van_detras_de_las_notas_y_lo_nuevo_delante() {
    let mut l = Lienzo::default();
    let r = l.aplicar(
        Quien::Usuario,
        &[
            json!({ "op": "crear", "tipo": "marco", "x": 0, "y": 0, "w": 600, "h": 400 }),
            caja("A"),
            caja("B"),
        ],
        AHORA,
    );
    let id = |i: usize| r.hechas[i]["id"].as_str().unwrap().to_string();
    let z = |l: &Lienzo, i: usize| l.elemento(&id(i)).unwrap().z;
    assert!(z(&l, 0) <= 0, "el marco, detrás de las notas");
    assert!(
        z(&l, 1) >= 1 && z(&l, 2) > z(&l, 1),
        "lo nuevo, delante y encima de lo anterior"
    );
    l.aplicar(
        Quien::Usuario,
        &[json!({ "op": "apilar", "id": id(2), "a": "fondo" })],
        AHORA,
    );
    assert!(
        z(&l, 2) < z(&l, 0) && z(&l, 2) <= 0,
        "al fondo: detrás de todo, también de las notas"
    );
    l.aplicar(
        Quien::Usuario,
        &[json!({ "op": "apilar", "id": id(2), "a": "frente" })],
        AHORA,
    );
    assert!(z(&l, 2) > z(&l, 1));
    // Con solo un marco en la capa, lo siguiente sigue naciendo delante de las notas.
    let mut solo = Lienzo::default();
    solo.aplicar(
        Quien::Usuario,
        &[json!({ "op": "crear", "tipo": "marco", "x": 0, "y": 0 })],
        AHORA,
    );
    let r = solo.aplicar(Quien::Usuario, &[caja("C")], AHORA);
    assert_eq!(
        solo.elemento(r.hechas[0]["id"].as_str().unwrap())
            .unwrap()
            .z,
        1
    );
}

#[test]
fn integrar_una_capa_conserva_lo_que_iba_detras_de_las_notas() {
    let mut l = Lienzo::default();
    l.aplicar(Quien::Usuario, &[caja("Mía")], AHORA);
    let capa = l.nueva_capa("Plan", AHORA).id;
    let r = l.aplicar(
        agente(&capa),
        &[
            json!({ "op": "crear", "tipo": "marco", "x": 900, "y": 0 }),
            json!({ "op": "crear", "tipo": "figura", "x": 950, "y": 50 }),
        ],
        AHORA,
    );
    l.aplicar(
        Quien::Usuario,
        &[json!({ "op": "capa-integrar", "capa": capa })],
        AHORA,
    );
    let z = |i: usize| l.elemento(r.hechas[i]["id"].as_str().unwrap()).unwrap().z;
    assert!(z(0) <= 0);
    assert!(z(1) > 1, "por encima de lo del usuario");
}

// ------------------------------------------------------------ deshacer

fn sin_rev(l: &Lienzo) -> (Vec<Value>, Vec<Value>) {
    let elementos = l
        .elementos
        .iter()
        .map(|e| {
            let mut v = serde_json::to_value(e).unwrap();
            v.as_object_mut().unwrap().remove("rev");
            v.as_object_mut().unwrap().remove("tocado");
            v.as_object_mut().unwrap().remove("tocadas");
            // El contador de identificadores no vuelve atrás a propósito: nunca se repite un id.
            if let Some(d) = v["datos"].as_object_mut() {
                d.remove("sig");
            }
            v
        })
        .collect();
    (
        elementos,
        l.capas
            .iter()
            .map(|c| serde_json::to_value(c).unwrap())
            .collect(),
    )
}

/// Aplica, deshace y comprueba que quedó igual; y que rehacer vuelve a lo hecho.
fn ida_y_vuelta(l: &mut Lienzo, ops: &[Value]) {
    let antes = sin_rev(l);
    let r = l.aplicar(Quien::Usuario, ops, AHORA);
    assert!(r.omitidas.is_empty(), "{:?}", r.omitidas);
    let despues = sin_rev(l);
    let v = l.aplicar(Quien::Usuario, &r.inversas, AHORA);
    assert!(v.omitidas.is_empty(), "deshacer: {:?}", v.omitidas);
    let mut ordenado = sin_rev(l);
    let mut esperado = antes;
    let id = |v: &Value| v["id"].as_str().unwrap().to_string();
    ordenado.0.sort_by_key(id);
    esperado.0.sort_by_key(id);
    ordenado.1.sort_by_key(id);
    esperado.1.sort_by_key(id);
    assert_eq!(ordenado, esperado, "deshacer {ops:?}");
    let w = l.aplicar(Quien::Usuario, &v.inversas, AHORA);
    assert!(w.omitidas.is_empty(), "rehacer: {:?}", w.omitidas);
    let mut rehecho = sin_rev(l);
    let mut hecho = despues;
    rehecho.0.sort_by_key(id);
    hecho.0.sort_by_key(id);
    rehecho.1.sort_by_key(id);
    hecho.1.sort_by_key(id);
    assert_eq!(rehecho, hecho, "rehacer {ops:?}");
}

#[test]
fn todo_lo_que_hace_el_usuario_se_deshace_y_se_rehace() {
    let (mut l, capa, a) = con_caja();
    let r = l.aplicar(Quien::Usuario, &[caja("B"), json!({ "op": "crear", "ref": "f", "tipo": "flecha", "datos": { "desde": { "id": "B" }, "hasta": { "id": a } } })], AHORA);
    let b = r.hechas[0]["id"].as_str().unwrap().to_string();

    ida_y_vuelta(&mut l, &[caja("C")]);
    ida_y_vuelta(
        &mut l,
        &[
            json!({ "op": "cambiar", "id": b, "x": 40, "y": 50, "w": 300, "datos": { "texto": "Otro", "color": "azul" } }),
        ],
    );
    ida_y_vuelta(&mut l, &[json!({ "op": "apilar", "id": b, "a": "fondo" })]);
    ida_y_vuelta(&mut l, &[json!({ "op": "borrar", "id": b })]);
    ida_y_vuelta(
        &mut l,
        &[json!({ "op": "capa-ver", "capa": capa, "visible": false })],
    );
    ida_y_vuelta(
        &mut l,
        &[json!({ "op": "capa-renombrar", "capa": capa, "nombre": "Otra" })],
    );
    ida_y_vuelta(&mut l, &[json!({ "op": "capa-integrar", "capa": capa })]);
}

#[test]
fn deshacer_borrar_una_capa_devuelve_sus_elementos_y_las_flechas_que_la_tocaban() {
    let (mut l, capa, a) = con_caja();
    let r = l.aplicar(Quien::Usuario, &[caja("Mía"), json!({ "op": "crear", "tipo": "flecha", "datos": { "desde": { "id": "Mía" }, "hasta": { "id": a } } })], AHORA);
    let flecha = r.hechas[1]["id"].as_str().unwrap().to_string();

    let r = l.aplicar(
        Quien::Usuario,
        &[json!({ "op": "capa-borrar", "capa": capa })],
        AHORA,
    );
    assert!(l.capa(&capa).is_none());
    assert!(l.elemento(&a).is_none());
    assert!(
        l.elemento(&flecha).unwrap().datos["hasta"]
            .get("x")
            .is_some(),
        "la flecha queda con esa punta suelta"
    );

    l.aplicar(Quien::Usuario, &r.inversas, AHORA);
    assert!(l.capa(&capa).is_some());
    assert!(l.elemento(&a).is_some());
    assert_eq!(l.elemento(&flecha).unwrap().datos["hasta"]["id"], a);
}

#[test]
fn deshacer_en_tablas_y_tableros_no_se_lleva_lo_que_anadio_el_agente() {
    let mut l = Lienzo::default();
    let capa = l.nueva_capa("Plan", AHORA).id;
    let r = l.aplicar(
        agente(&capa),
        &[
            json!({ "op": "crear", "ref": "t", "tipo": "tabla", "x": 0, "y": 0, "datos": { "columnas": ["A"], "filas": [["1"]] } }),
            json!({ "op": "crear", "ref": "b", "tipo": "tablero", "x": 900, "y": 0, "datos": { "tarjetas": [{ "titulo": "X" }, { "titulo": "Y" }] } }),
        ],
        AHORA,
    );
    let (t, b) = (
        r.hechas[0]["id"].as_str().unwrap().to_string(),
        r.hechas[1]["id"].as_str().unwrap().to_string(),
    );
    let d = l.elemento(&t).unwrap().datos.clone();
    let (c, f) = (
        d["columnas"][0]["id"].as_str().unwrap().to_string(),
        d["filas"][0]["id"].as_str().unwrap().to_string(),
    );
    let d = l.elemento(&b).unwrap().datos.clone();
    let x = d["tarjetas"][0]["id"].as_str().unwrap().to_string();
    let hecho = d["columnas"][2]["id"].as_str().unwrap().to_string();

    let celda = l.aplicar(
        Quien::Usuario,
        &[json!({ "op": "celdas", "id": t, "celdas": [{ "fila": f, "col": c, "valor": "mío" }] })],
        AHORA,
    );
    let mover = l.aplicar(
        Quien::Usuario,
        &[json!({ "op": "tarjeta-mover", "id": b, "tarjeta": x, "col": hecho })],
        AHORA,
    );
    l.aplicar(
        agente(&capa),
        &[
            json!({ "op": "fila-nueva", "id": t, "valores": ["2"] }),
            json!({ "op": "tarjeta-nueva", "id": b, "titulo": "Z" }),
        ],
        AHORA,
    );

    l.aplicar(Quien::Usuario, &mover.inversas, AHORA);
    l.aplicar(Quien::Usuario, &celda.inversas, AHORA);
    let tabla = &l.elemento(&t).unwrap().datos;
    assert_eq!(
        tabla["filas"].as_array().unwrap().len(),
        2,
        "la fila del agente sigue"
    );
    assert_eq!(tabla["filas"][0]["celdas"][&c], "1");
    let tablero = &l.elemento(&b).unwrap().datos;
    assert_eq!(
        tablero["tarjetas"].as_array().unwrap().len(),
        3,
        "la tarjeta del agente sigue"
    );
    assert_eq!(
        tablero["tarjetas"][0]["id"], x,
        "y la movida vuelve a su puesto"
    );
    assert_eq!(tablero["tarjetas"][0]["col"], tablero["columnas"][0]["id"]);
}

#[test]
fn deshacer_filas_columnas_y_tarjetas() {
    let mut l = Lienzo::default();
    let r = l.aplicar(
        Quien::Usuario,
        &[
            json!({ "op": "crear", "tipo": "tabla", "x": 0, "y": 0, "datos": { "columnas": ["A", "B"], "filas": [["1", "2"], ["3", "4"]] } }),
            json!({ "op": "crear", "tipo": "tablero", "x": 900, "y": 0, "datos": { "tarjetas": [{ "titulo": "X", "puntos": 3 }, { "titulo": "Y" }] } }),
        ],
        AHORA,
    );
    let (t, b) = (
        r.hechas[0]["id"].as_str().unwrap().to_string(),
        r.hechas[1]["id"].as_str().unwrap().to_string(),
    );
    let d = l.elemento(&t).unwrap().datos.clone();
    let (c0, c1) = (
        d["columnas"][0]["id"].as_str().unwrap(),
        d["columnas"][1]["id"].as_str().unwrap(),
    );
    let (f0, f1) = (
        d["filas"][0]["id"].as_str().unwrap(),
        d["filas"][1]["id"].as_str().unwrap(),
    );
    let d2 = l.elemento(&b).unwrap().datos.clone();
    let x = d2["tarjetas"][0]["id"].as_str().unwrap();
    let col = d2["columnas"][0]["id"].as_str().unwrap();

    ida_y_vuelta(
        &mut l,
        &[json!({ "op": "fila-nueva", "id": t, "pos": 1, "valores": ["a", "b"] })],
    );
    ida_y_vuelta(
        &mut l,
        &[json!({ "op": "fila-borrar", "id": t, "fila": f0 })],
    );
    ida_y_vuelta(
        &mut l,
        &[json!({ "op": "filas-orden", "id": t, "orden": [f1, f0] })],
    );
    ida_y_vuelta(
        &mut l,
        &[json!({ "op": "columna-nueva", "id": t, "pos": 1, "titulo": "M" })],
    );
    ida_y_vuelta(
        &mut l,
        &[json!({ "op": "columna-borrar", "id": t, "col": c0 })],
    );
    ida_y_vuelta(
        &mut l,
        &[json!({ "op": "columna-cambiar", "id": t, "col": c1, "titulo": "Z", "ancho": 200 })],
    );
    ida_y_vuelta(
        &mut l,
        &[json!({ "op": "columnas-orden", "id": t, "orden": [c1, c0] })],
    );
    ida_y_vuelta(
        &mut l,
        &[
            json!({ "op": "tarjeta-cambiar", "id": b, "tarjeta": x, "titulo": "XX", "puntos": null }),
        ],
    );
    ida_y_vuelta(
        &mut l,
        &[json!({ "op": "tarjeta-borrar", "id": b, "tarjeta": x })],
    );
    ida_y_vuelta(
        &mut l,
        &[json!({ "op": "columna-cambiar", "id": b, "col": col, "limite": 3 })],
    );
    ida_y_vuelta(
        &mut l,
        &[json!({ "op": "columna-borrar", "id": b, "col": col })],
    );
}

// ------------------------------------------------------------ zonas y diagramas

fn no_se_tocan(a: &Rect, b: &Rect) -> bool {
    !a.toca(b, 0.0)
}

#[test]
fn una_zona_nunca_toca_nada_ni_una_carta() {
    let mut l = Lienzo::default();
    l.aplicar(
        Quien::Usuario,
        &[json!({ "op": "crear", "tipo": "marco", "x": 0, "y": 0, "w": 600, "h": 400 })],
        AHORA,
    );
    let carta = Rect {
        x: 660.0,
        y: 0.0,
        w: 380.0,
        h: 440.0,
    };
    l.fijar_entorno(vec![("a.md".into(), carta)], None);
    let z = l.reservar_zona(800.0, 600.0, None, AHORA);
    let marco = Rect {
        x: 0.0,
        y: 0.0,
        w: 600.0,
        h: 400.0,
    };
    assert!(no_se_tocan(&z, &carta) && no_se_tocan(&z, &marco), "{z:?}");
    // Dos seguidas no reciben el mismo sitio, aunque aún no se haya dibujado nada en la primera.
    let otra = l.reservar_zona(800.0, 600.0, None, AHORA);
    assert!(no_se_tocan(&z, &otra), "{z:?} {otra:?}");
    // La promesa caduca.
    let tarde = l.reservar_zona(800.0, 600.0, None, AHORA + 10 * 60 * 1000);
    assert_eq!(tarde, z);
}

#[test]
fn con_el_lienzo_vacio_la_zona_cae_donde_mira_el_usuario() {
    let mut l = Lienzo::default();
    l.fijar_entorno(Vec::new(), Some((5000.0, 3000.0)));
    let z = l.zona_libre(400.0, 300.0, None);
    assert!(
        (5000.0..6000.0).contains(&z.x) && (3000.0..3400.0).contains(&z.y),
        "{z:?}"
    );
}

#[test]
fn sin_coordenadas_el_motor_le_busca_sitio() {
    let (mut l, capa, a) = con_caja();
    let r = l.aplicar(
        agente(&capa),
        &[json!({ "op": "crear", "tipo": "figura", "datos": { "texto": "B" } })],
        AHORA,
    );
    let b = r.hechas[0]["id"].as_str().unwrap();
    let (ra, rb) = (
        l.rect_de(l.elemento(&a).unwrap()),
        l.rect_de(l.elemento(b).unwrap()),
    );
    assert!(no_se_tocan(&ra, &rb), "{ra:?} {rb:?}");
}

#[test]
fn un_diagrama_acaba_en_elementos_normales_sin_solapes() {
    let mut l = Lienzo::default();
    l.aplicar(
        Quien::Usuario,
        &[json!({ "op": "crear", "tipo": "marco", "x": 0, "y": 0, "w": 600, "h": 400 })],
        AHORA,
    );
    let capa = l.nueva_capa("RAG", AHORA).id;
    let spec = json!({
        "titulo": "Arquitectura",
        "direccion": "derecha",
        "grupos": [{ "ref": "g", "titulo": "Índice" }],
        "nodos": [
            { "ref": "chat", "texto": "Chat de Nexo", "forma": "servicio", "color": "azul" },
            { "ref": "motor", "texto": "Motor de búsqueda", "forma": "servicio" },
            { "ref": "chunks", "texto": "Chunks", "forma": "cilindro", "grupo": "g" },
            { "ref": "vec", "texto": "Vectores", "forma": "cilindro", "grupo": "g" },
            { "ref": "suelto", "texto": "Sin aristas" },
        ],
        "aristas": [
            { "de": "chat", "a": "motor", "texto": "pregunta" },
            { "de": "motor", "a": "chunks" },
            { "de": "motor", "a": "vec", "trazo": "discontinuo" },
            { "de": "chunks", "a": "motor" },
        ],
    });
    let r = l.diagrama(&capa, &spec, AHORA).unwrap();
    assert!(r.omitidas.is_empty(), "{:?}", r.omitidas);
    let suyos: Vec<_> = l
        .elementos
        .iter()
        .filter(|e| e.capa == capa)
        .cloned()
        .collect();
    let figuras: Vec<_> = suyos.iter().filter(|e| e.tipo == "figura").collect();
    assert_eq!(figuras.len(), 5);
    assert_eq!(suyos.iter().filter(|e| e.tipo == "flecha").count(), 4);
    assert_eq!(suyos.iter().filter(|e| e.tipo == "marco").count(), 2);
    for (i, a) in figuras.iter().enumerate() {
        for b in &figuras[i + 1..] {
            assert!(
                no_se_tocan(&l.rect_de(a), &l.rect_de(b)),
                "{} pisa a {}",
                a.datos["texto"],
                b.datos["texto"]
            );
        }
    }
    // Cada flecha apunta a un elemento real, y el grupo contiene justo a los suyos.
    for f in suyos.iter().filter(|e| e.tipo == "flecha") {
        assert!(
            l.elemento(f.datos["desde"]["id"].as_str().unwrap())
                .is_some()
        );
        assert!(
            l.elemento(f.datos["hasta"]["id"].as_str().unwrap())
                .is_some()
        );
    }
    let grupo = suyos
        .iter()
        .find(|e| e.tipo == "marco" && e.datos["titulo"] == "Índice")
        .unwrap();
    let rg = l.rect_de(grupo);
    for f in &figuras {
        let dentro = l.rect_de(f).dentro_de(&rg);
        let debe = ["Chunks", "Vectores"].contains(&f.datos["texto"].as_str().unwrap());
        assert_eq!(dentro, debe, "{}", f.datos["texto"]);
    }
    // No pisa lo que ya había.
    let exterior = suyos
        .iter()
        .find(|e| e.tipo == "marco" && e.datos["titulo"] == "Arquitectura")
        .unwrap();
    assert!(no_se_tocan(
        &l.rect_de(exterior),
        &Rect {
            x: 0.0,
            y: 0.0,
            w: 600.0,
            h: 400.0
        }
    ));
    // Con la misma entrada sale lo mismo.
    let colocar = |l: &Lienzo, capa: &str| -> Vec<(f64, f64)> {
        l.elementos
            .iter()
            .filter(|e| e.capa == capa && e.tipo == "figura")
            .map(|e| (e.x, e.y))
            .collect()
    };
    let mut otro = Lienzo::default();
    otro.aplicar(
        Quien::Usuario,
        &[json!({ "op": "crear", "tipo": "marco", "x": 0, "y": 0, "w": 600, "h": 400 })],
        AHORA,
    );
    let c2 = otro.nueva_capa("RAG", AHORA).id;
    otro.diagrama(&c2, &spec, AHORA).unwrap();
    assert_eq!(colocar(&l, &capa), colocar(&otro, &c2));
}

#[test]
fn un_diagrama_mal_escrito_se_rechaza_con_su_motivo() {
    let mut l = Lienzo::default();
    let capa = l.nueva_capa("X", AHORA).id;
    for (spec, dice) in [
        (json!({}), "nodos"),
        (json!({ "nodos": [] }), "no tiene nodos"),
        (
            json!({ "nodos": [{ "ref": "a" }, { "ref": "a" }] }),
            "misma",
        ),
        (
            json!({ "nodos": [{ "ref": "a" }], "aristas": [{ "de": "a", "a": "z" }] }),
            "no está",
        ),
        (json!({ "nodos": [{ "tipo": "flecha" }] }), "flecha"),
        (json!({ "nodos": [{ "forma": "pentagrama" }] }), "forma"),
    ] {
        let e = l.diagrama(&capa, &spec, AHORA).unwrap_err();
        assert!(e.contains(dice), "{e}");
    }
    assert!(l.elementos.is_empty(), "no deja nada a medias");
}

#[test]
fn ordenar_no_mueve_lo_que_esta_en_uso_y_se_deshace_de_una_vez() {
    let mut l = Lienzo::default();
    let r = l.aplicar(
        Quien::Usuario,
        &[
            json!({ "op": "crear", "tipo": "figura", "x": 700, "y": 90, "w": 176, "h": 88 }),
            json!({ "op": "crear", "tipo": "figura", "x": 10, "y": 500, "w": 176, "h": 88 }),
            json!({ "op": "crear", "tipo": "figura", "x": 300, "y": 300, "w": 176, "h": 88 }),
            json!({ "op": "crear", "tipo": "figura", "x": -400, "y": 40, "w": 176, "h": 88 }),
        ],
        AHORA,
    );
    let ids: Vec<String> = r
        .hechas
        .iter()
        .map(|h| h["id"].as_str().unwrap().to_string())
        .collect();
    let quieta = l.elemento(&ids[3]).unwrap().clone();
    l.usar(&ids[3], true);
    let antes: Vec<(f64, f64)> = l.elementos.iter().map(|e| (e.x, e.y)).collect();

    let o = l.ordenar("fila", None, None, false, AHORA).unwrap();
    assert!(o.resultado.omitidas.is_empty());
    let y: Vec<f64> = ids[..3].iter().map(|i| l.elemento(i).unwrap().y).collect();
    assert!(y.iter().all(|v| *v == y[0]), "en fila: {y:?}");
    let x: Vec<f64> = ids[..3].iter().map(|i| l.elemento(i).unwrap().x).collect();
    assert!(
        x[1] < x[2] && x[2] < x[0],
        "en el orden en que estaban: {x:?}"
    );
    assert_eq!(
        l.elemento(&ids[3]).unwrap().x,
        quieta.x,
        "la que se está usando no se mueve"
    );

    l.aplicar(Quien::Usuario, &o.resultado.inversas, AHORA);
    let despues: Vec<(f64, f64)> = l.elementos.iter().map(|e| (e.x, e.y)).collect();
    assert_eq!(despues, antes);
}

#[test]
fn ordenar_mueve_tambien_las_cartas_si_se_pide_y_un_marco_lleva_lo_suyo() {
    let mut l = Lienzo::default();
    let r = l.aplicar(
        Quien::Usuario,
        &[
            json!({ "op": "crear", "tipo": "marco", "x": 1000, "y": 0, "w": 300, "h": 300 }),
            json!({ "op": "crear", "tipo": "figura", "x": 1050, "y": 50, "w": 132, "h": 66 }),
            json!({ "op": "crear", "tipo": "figura", "x": 0, "y": 0, "w": 132, "h": 66 }),
        ],
        AHORA,
    );
    let (marco, dentro) = (
        r.hechas[0]["id"].as_str().unwrap().to_string(),
        r.hechas[1]["id"].as_str().unwrap().to_string(),
    );
    l.fijar_entorno(
        vec![(
            "a.md".into(),
            Rect {
                x: 500.0,
                y: 900.0,
                w: 380.0,
                h: 440.0,
            },
        )],
        None,
    );

    let antes = l.elemento(&marco).unwrap().x;
    let o = l.ordenar("fila", None, None, true, AHORA).unwrap();
    assert_eq!(o.cartas.len(), 1);
    assert_eq!(o.cartas[0].path, "a.md");
    let (m, d) = (l.elemento(&marco).unwrap(), l.elemento(&dentro).unwrap());
    assert_eq!(o.cartas[0].y, m.y, "la carta entra en la fila");
    assert_ne!(m.x, antes);
    assert_eq!(d.x - m.x, 50.0, "lo de dentro no se separa de su marco");
    assert_eq!(d.y - m.y, 50.0);

    assert!(l.ordenar("en-circulo", None, None, false, AHORA).is_err());
}

// ------------------------------------------------------------ datos

#[test]
fn se_guarda_y_se_relee_igual() {
    let raiz = temporal("guardar");
    let a = Almacen::default();
    let capa = a
        .con(&raiz, "d1", |l| l.nueva_capa("Plan", AHORA).id)
        .unwrap();
    a.con(&raiz, "d1", |l| {
        l.aplicar(agente(&capa), &[caja("A"), json!({ "op": "crear", "tipo": "tabla", "x": 500, "y": 0, "datos": { "columnas": ["A"], "filas": [["x"]] } })], AHORA);
        l.terminar_capa(&capa);
    })
    .unwrap();
    a.guardar();
    let ruta = ruta_lienzo(&raiz, "d1").unwrap();
    assert!(ruta.ends_with(".nexo/lienzos/d1.json"));
    let texto = fs::read_to_string(&ruta).unwrap();

    let otra = Almacen::default();
    let leido = otra.con(&raiz, "d1", |l| l.a_json()).unwrap();
    assert_eq!(leido, texto);
    // Sin el temporal de por medio.
    assert!(!ruta.with_extension("json.tmp").exists());
    let _ = fs::remove_dir_all(&raiz);
}

#[test]
fn un_archivo_roto_no_se_sobrescribe() {
    let raiz = temporal("roto");
    let ruta = ruta_lienzo(&raiz, "d1").unwrap();
    fs::create_dir_all(ruta.parent().unwrap()).unwrap();
    fs::write(&ruta, "{ esto no es JSON").unwrap();
    let a = Almacen::default();
    a.con(&raiz, "d1", |l| {
        l.aplicar(Quien::Usuario, &[caja("Nueva")], AHORA)
    })
    .unwrap();
    a.guardar();
    assert_eq!(
        fs::read_to_string(ruta.with_extension("roto.json")).unwrap(),
        "{ esto no es JSON"
    );
    assert!(fs::read_to_string(&ruta).unwrap().contains("Nueva"));
    let avisos = a.avisos();
    assert_eq!(avisos.len(), 1);
    assert!(avisos[0].contains("no se entiende"));
    assert!(a.avisos().is_empty(), "solo se avisa una vez");
    let _ = fs::remove_dir_all(&raiz);
}

#[test]
fn el_primer_escritorio_tiene_un_id_estable_sin_lista_guardada() {
    let raiz = temporal("activo");
    assert_eq!(escritorio_activo(&raiz), ESCRITORIO_PRINCIPAL);
    fs::write(
        raiz.join(".nexo/escritorios.json"),
        r#"{ "active": "d2", "desks": [{ "id": "d1" }, { "id": "d2" }] }"#,
    )
    .unwrap();
    assert_eq!(escritorio_activo(&raiz), "d2");
    fs::write(
        raiz.join(".nexo/escritorios.json"),
        r#"{ "active": "no", "desks": [{ "id": "d1" }] }"#,
    )
    .unwrap();
    assert_eq!(escritorio_activo(&raiz), "d1");
    fs::write(raiz.join(".nexo/escritorios.json"), "roto").unwrap();
    assert_eq!(escritorio_activo(&raiz), ESCRITORIO_PRINCIPAL);
    let _ = fs::remove_dir_all(&raiz);
}

#[test]
fn un_id_de_escritorio_raro_no_forma_una_ruta() {
    let raiz = temporal("rutas");
    let a = Almacen::default();
    for id in [
        "../fuera",
        "a/b",
        "",
        ".oculto",
        "a b",
        "a\\b",
        &"x".repeat(100),
    ] {
        assert!(ruta_lienzo(&raiz, id).is_err(), "{id}");
        assert!(a.con(&raiz, id, |_| ()).is_err(), "{id}");
    }
    let _ = fs::remove_dir_all(&raiz);
}

#[test]
fn una_capa_sigue_yendo_a_su_escritorio_aunque_cambie_el_activo() {
    let raiz = temporal("capa");
    fs::write(
        raiz.join(".nexo/escritorios.json"),
        r#"{ "active": "d1", "desks": [{ "id": "d1" }, { "id": "d2" }] }"#,
    )
    .unwrap();
    let a = Almacen::default();
    let capa = a
        .con(&raiz, "d1", |l| l.nueva_capa("Plan", AHORA).id)
        .unwrap();
    // El usuario cambia de escritorio, y hasta se reinicia la app.
    fs::write(
        raiz.join(".nexo/escritorios.json"),
        r#"{ "active": "d2", "desks": [{ "id": "d1" }, { "id": "d2" }] }"#,
    )
    .unwrap();
    assert_eq!(a.escritorio_de_capa(&raiz, &capa).unwrap(), "d1");
    a.guardar();
    let otra = Almacen::default();
    assert_eq!(otra.escritorio_de_capa(&raiz, &capa).unwrap(), "d1");
    assert!(otra.escritorio_de_capa(&raiz, "c-no-existe").is_err());
    let _ = fs::remove_dir_all(&raiz);
}

#[test]
fn las_cartas_de_notas_cuentan_como_obstaculos() {
    let raiz = temporal("cartas");
    fs::write(raiz.join(".nexo/escritorio.json"), r#"{ "cards": [{ "path": "a.md", "x": 0, "y": 0, "w": 380, "h": 440 }], "view": { "x": 10, "y": 20 } }"#).unwrap();
    let a = Almacen::default();
    let z = a
        .con(&raiz, ESCRITORIO_PRINCIPAL, |l| {
            l.zona_libre(500.0, 300.0, None)
        })
        .unwrap();
    assert!(
        no_se_tocan(
            &z,
            &Rect {
                x: 0.0,
                y: 0.0,
                w: 380.0,
                h: 440.0
            }
        ),
        "{z:?}"
    );
    let _ = fs::remove_dir_all(&raiz);
}

#[test]
fn los_topes_salen_como_omitidas() {
    let mut l = Lienzo::default();
    let lote = vec![caja("x"); MAX_LOTE + 1];
    let r = l.aplicar(Quien::Usuario, &lote, AHORA);
    assert_eq!(r.aplicadas, 0);
    assert!(r.omitidas[0]["motivo"].as_str().unwrap().contains("tandas"));

    for tanda in 0..(MAX_ELEMENTOS / MAX_LOTE) {
        let lote: Vec<Value> = (0..MAX_LOTE)
            .map(|i| json!({ "op": "crear", "tipo": "texto", "x": i, "y": tanda }))
            .collect();
        assert_eq!(l.aplicar(Quien::Usuario, &lote, AHORA).aplicadas, MAX_LOTE);
    }
    let r = l.aplicar(Quien::Usuario, &[caja("una más")], AHORA);
    assert_eq!(r.aplicadas, 0);
    assert!(r.omitidas[0]["motivo"].as_str().unwrap().contains("tope"));
}

#[test]
fn una_operacion_que_no_se_entiende_no_para_el_lote() {
    let mut l = Lienzo::default();
    let r = l.aplicar(
        Quien::Usuario,
        &[
            json!({ "op": "volar" }),
            json!("texto"),
            json!({ "op": "crear", "tipo": "video" }),
            caja("A"),
        ],
        AHORA,
    );
    assert_eq!(r.aplicadas, 1, "{r:?}");
    assert_eq!(r.omitidas.len(), 3);
}

#[test]
fn leer_da_solo_lo_cambiado_desde_una_revision() {
    let (mut l, capa, a) = con_caja();
    let rev = l.rev;
    let r = l.aplicar(
        Quien::Usuario,
        &[caja("B"), json!({ "op": "cambiar", "id": a, "x": 40 })],
        AHORA,
    );
    let b = r.hechas[0]["id"].as_str().unwrap().to_string();
    let v = l.leer(None, Some(rev));
    let ids: Vec<&str> = v["elementos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [a.as_str(), b.as_str()]);
    let rev2 = l.rev;
    l.aplicar(Quien::Usuario, &[json!({ "op": "borrar", "id": b })], AHORA);
    let v = l.leer(None, Some(rev2));
    assert_eq!(v["quitados"], json!([b]));
    assert!(v["elementos"].as_array().unwrap().is_empty());
    let texto = l.leer_texto(Some(&capa), None);
    assert!(
        texto.contains("RAG del grafo")
            && texto.contains("figura/rectangulo «A»")
            && texto.contains("[tocado]"),
        "{texto}"
    );
    assert!(!texto.contains("«B»"));
}
