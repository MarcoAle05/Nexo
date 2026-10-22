# Especificación: avisos de los workspaces

Saber qué workspace necesita al usuario aunque no lo esté mirando: un **punto de «esperando
respuesta»** en las listas de workspaces y en el botón Agentes, y **avisos de escritorio** cuando un
orquestador o uno de sus agentes pide permiso o pregunta algo, y cuando un workspace termina su
trabajo. Es el punto 1 del informe «nexo frente a Orca» (`~/informe-nexo-vs-orca.pdf`).

**Estado:** aprobada el 17-10-2026. Las decisiones marcadas «(usuario)» las tomó el usuario ese día,
con opciones o al cerrar las preguntas abiertas.

**Desde el 22-10-2026** los workspaces trabajan con terminales y ya no tienen orquestador
(`spec-terminales-workspaces.md`): el punto, los avisos y sus reglas siguen igual, pero lo que pasa
sale de los hooks del Claude Code de cada terminal (`sesiones.rs`) y no de la salida del
orquestador. «El orquestador» de los textos es ahora el nombre de la sesión de Claude, y un turno
interrumpido (que no dispara `Stop`) se cierra al ver en su transcripción el fin del turno. Lo que
sigue describe el diseño de entonces.

## Objetivo

**El problema.** Al cambiar de workspace, la interfaz se desconecta (`workspace_detach`,
`src-tauri/src/workspace_run.rs:934-955`), pero si el orquestador estaba trabajando sigue vivo hasta
quedar libre. Si en ese momento él o un agente piden permiso o hacen una pregunta, Claude Code se
queda esperando una respuesta que nadie ve: no está libre, así que tampoco se cierra, y el trabajo se
para sin que el usuario lo sepa. Volver a ese workspace sí deja contestar (la petición se repite con
el historial), pero nada avisa de que hay que volver. Y nexo no tiene avisos de escritorio: tampoco
se entera nadie de que un trabajo terminó.

**Para quién.** El usuario de nexo que reparte trabajo entre varios workspaces (y sus worktrees) y
mientras tanto mira otro workspace, el grafo, las notas u otra ventana del escritorio.

**Cómo lo hace Orca** (1.4.216): marca qué necesita atención en todos los workspaces a la vez y avisa
al terminar una tarea (`agentTaskComplete`), salvo si el usuario ya está mirando esa ventana
(`suppressWhenFocused`).

**Qué es éxito.** Un workspace que espera respuesta se ve desde cualquier sitio de nexo, el escritorio
avisa en cuanto pasa (y al terminar un trabajo) y no avisa de lo que el usuario ya tiene delante.

## Supuestos

1. «Esperando respuesta» es exactamente lo que ya enseñan las ventanas (`agentState` de
   `ws-stream.js`): un `control_request` `can_use_tool` sin contestar. Eso incluye las preguntas
   (`AskUserQuestion`) y los planes (`ExitPlanMode`), que llegan como permisos.
2. Un workspace **está libre** cuando no hay turno en marcha y no quedan tareas en segundo plano
   (`idle` de `workspace_run.rs`). Es el mismo criterio con el que hoy se cierra un orquestador
   desconectado.
3. **El usuario está mirando un workspace** cuando la ventana de nexo tiene el foco y la vista Agentes
   enseña ese workspace. Con la ventana sin foco, o en Grafo, Notas, el panel de nexo u otro
   workspace, no lo está mirando.
4. Cada worktree es un workspace aparte (su propia ruta y su propio orquestador): el punto y los
   avisos van por ruta.
5. Al abrir nexo no corre ningún orquestador, así que nadie espera nada. Lo que se repite del
   historial (`history`, `replay`) nunca avisa: solo cuenta lo que llega en vivo.
6. El escritorio tiene un servicio de avisos (`org.freedesktop.Notifications`; en este equipo lo da
   Quickshell, `qs`). Si no lo hay, no se avisa y no pasa nada más.

## Decisiones

- **Aviso de fin: al quedar libre** (usuario). Un aviso cuando el orquestador y todos sus agentes en
  segundo plano han terminado, no uno por turno: un turno que solo lanza agentes y los espera no es
  el final.
- **Dónde va el punto** (usuario): en **Recientes** (la lista de workspaces del menú de carpetas), en
  la **lista de worktrees** (el menú del nombre del workspace) y en el **botón Agentes** de la barra
  superior. La entrada «Workspace» del panel de nexo no lo lleva.
- **Solo workspaces** (usuario). El chat de Nexo de la carta derecha no avisa.
- **El aviso solo informa** (usuario): pulsarlo no hace nada. Se usa el plugin oficial
  `tauri-plugin-notification` (en Linux va por D-Bus con `notify-rust`, sin librerías del sistema
  nuevas).
- **Lo decide el backend.** Solo el backend ve las líneas de un workspace desconectado, así que es él
  quien lleva el estado de atención de cada workspace, avisa al escritorio y se lo cuenta a la
  interfaz. La interfaz solo le dice qué workspace tiene en pantalla.

## Reglas

### A1. Estado de atención de cada workspace

El lector (`watch_line`) lleva, además de `turn` y `background`, las peticiones sin contestar de su
proceso: cada `control_request` `can_use_tool` entra con su `request_id`, quién la hace
(`agent_id`, o el orquestador si no lo trae) y su herramienta. Sale cuando la interfaz la contesta
(una línea `control_response` con ese `request_id`, que pasa por `workspace_send`), cuando Claude Code
la cancela (`control_cancel_request`) o cuando el proceso se cierra. El estado de un workspace es
`{ waiting: n.º de peticiones sin contestar, busy: !idle }`.

Pruebas: `el_lector_cuenta_los_permisos_sin_contestar`,
`un_permiso_contestado_o_cancelado_deja_de_esperar`, `al_cerrarse_el_proceso_no_espera_nada`.

### A2. La interfaz se entera de todos los workspaces

Cada vez que cambia el estado de un workspace, el backend emite el evento global
`workspace-attention` con `{ path, waiting, busy }` (global como `agy-activity`: lo escucha la
interfaz aunque esté en otro workspace o en otra vista). `workspace_attention` devuelve el estado de
todos los que tienen proceso, para la interfaz recién cargada (también tras una recarga de Vite).
Mientras el estado no cambia no se emite nada.

Pruebas: `solo_se_emite_cuando_cambia_el_estado`, y en `tests/ws-attention.test.mjs`
`el_estado_junta_los_eventos_de_todos_los_workspaces`.

### A3. El punto de «esperando respuesta»

Con `waiting > 0`, el workspace lleva un punto con el color del estado «Esperando respuesta» de las
pestañas de agentes (tokens del tema, sin colores sueltos):

- en su fila de **Recientes**;
- en su fila de la **lista de worktrees**, si es un worktree del repo abierto;
- en el **botón Agentes** de la barra superior, si algún workspace espera.

El color nunca va solo: la fila lleva además el texto «Espera tu respuesta» y el botón Agentes lo
dice en su nombre accesible («Agentes: 2 workspaces esperan tu respuesta»). El punto se quita en
cuanto el workspace deja de esperar.

Pruebas: en `tests/ws-attention.test.mjs`, `el_boton_agentes_dice_cuantos_esperan` y
`un_workspace_que_deja_de_esperar_pierde_el_punto`; lo pintado se comprueba en el banco (ver Pruebas).

### A4. Aviso al pedir permiso o preguntar

Cada petición nueva de A1 avisa al escritorio, salvo si el usuario está mirando ese workspace (A6).
**Un aviso por petición** (usuario): cinco permisos seguidos son cinco avisos, sin juntarlos ni
reemplazar el anterior. Título: el nombre del workspace. Texto, según la herramienta:

| Herramienta | Texto |
|---|---|
| `AskUserQuestion` | «‹quién› te hace una pregunta» |
| `ExitPlanMode` | «‹quién› te propone un plan» |
| cualquier otra | «‹quién› pide permiso para usar ‹herramienta›» |

‹quién› es «El orquestador» o el nombre del agente registrado en `.nexo/agentes/` cuya `task` es el
`agent_id`, o «Un agente» si no se encuentra.

Pruebas: `el_aviso_de_un_permiso_dice_quien_y_que_herramienta`,
`el_aviso_de_una_pregunta_no_lleva_lo_que_pregunta`.

### A5. Aviso al terminar

Cuando un workspace pasa de `busy` a libre sin peticiones pendientes, avisa (salvo A6): «Terminó el
trabajo» o, si el último `result` fue un error (`is_error`), «Terminó con un error». Es un aviso por
cada paso a libre, no por turno. Un orquestador desconectado avisa **antes** de cerrarse por estar
libre. Cerrarlo a propósito (`workspace_stop`, «Reiniciar espacio de Workspace», conversación nueva)
no avisa.

**Si Claude Code se cierra solo con trabajo a medias** (usuario): si el proceso termina mientras
estaba `busy` o con peticiones pendientes, y no lo cerró nexo, avisa «Se cerró con trabajo a medias»
(salvo A6) y el workspace deja de esperar (A1). Lo que cierra nexo a propósito (las paradas de arriba
y el cierre de un orquestador desconectado al quedar libre) no cuenta como caída.

Pruebas: `avisa_una_vez_al_quedar_libre_y_no_en_cada_turno`, `un_turno_con_error_avisa_del_error`,
`parar_el_orquestador_a_mano_no_avisa`, `si_claude_code_se_cae_con_trabajo_avisa`.

### A6. Sin avisos de lo que ya se está mirando

La interfaz le dice al backend qué workspace tiene en pantalla y si la ventana tiene el foco
(`workspace_watch({ path | null, focused })`), al cambiar de vista, de panel o de workspace y en los
`focus`/`blur` de la ventana. No se avisa si la ventana de nexo tiene el foco **y** ese es el
workspace en pantalla. En cualquier otro caso, sí. El foco lo cuenta la interfaz y no
`is_focused()` del backend porque así se comprueba en el banco sin abrir la app (si funciona bien en
Hyprland no se sabe sin lanzarla). El punto de A3 no depende de esto: se ve siempre que el workspace
espera.

Pruebas: `no_avisa_si_se_esta_mirando_ese_workspace`,
`avisa_si_la_ventana_no_tiene_el_foco_o_se_mira_otra_cosa`.

### A7. Lo que un aviso no lleva nunca

Ni la entrada de la herramienta (órdenes de Bash, rutas, contenido de archivos, URL), ni el texto de
una pregunta o un plan, ni nada de la respuesta del modelo: los avisos pueden quedar en el historial
del escritorio y verse con la pantalla bloqueada, y una orden puede llevar claves. Solo nombre del
workspace, quién y el nombre de la herramienta.

Prueba: `los_avisos_no_llevan_la_entrada_de_la_herramienta`.

### A8. Fallos

Si el escritorio no acepta el aviso (sin servicio de avisos, D-Bus caído), se escribe un `warn` en el
registro y todo sigue igual: el punto, el orquestador y la interfaz no dependen del aviso.

Prueba: `un_aviso_que_falla_no_para_el_lector` (el envío va detrás de un rasgo que la prueba
sustituye por uno que falla).

## Fuera de alcance

Avisos del chat de Nexo; pulsar un aviso para abrir el workspace; un interruptor en Configuración;
sonidos; contestar desde el aviso; cerrar solo un orquestador que lleva mucho esperando; confirmar al
cerrar nexo con trabajo en marcha (punto 3 del informe) y los estados Falló/Detenido de las ventanas
(punto 2), que son cambios aparte.

## Pila

Tauri 2.12 (Rust 2024) y JavaScript sin framework, como el resto de nexo. **Dependencia nueva:**
`tauri-plugin-notification = "2"` en `src-tauri/Cargo.toml` (decisión del usuario). Se llama desde
Rust, así que no hace falta su paquete de npm ni un permiso nuevo en `capabilities/default.json`.

## Comandos

```bash
npm test                                         # pruebas de la interfaz (node --test tests/*.test.mjs)
npm run build                                    # build del frontend
cd src-tauri && cargo test --no-default-features # pruebas del backend (sin abrir ventana)
cargo test --no-default-features aviso           # solo las de esta spec
cargo fmt --all --check                          # formato, como el CI
npm run tauri build -- --no-bundle               # binario para instalar (solo si el usuario lo pide)
```

## Estructura

| Archivo | Qué cambia |
|---|---|
| `src-tauri/src/workspace_run.rs` | A1, A2 y A5: peticiones pendientes en `Live`, cambios de estado en el lector, `workspace_attention` y `workspace_watch`, aviso antes de cerrar por libre |
| `src-tauri/src/avisos.rs` (nuevo) | A4 a A8: textos de los avisos, la decisión de A6 y el envío (detrás de un rasgo para probarlo) |
| `src-tauri/src/lib.rs` | el plugin y los dos comandos nuevos |
| `src-tauri/Cargo.toml`, `Cargo.lock` | `tauri-plugin-notification` |
| `src/ws-attention.js` (nuevo) | estado de todos los workspaces y textos del punto, sin DOM |
| `src/main.js` | escucha `workspace-attention`, punto del botón Agentes, `workspace_watch` al cambiar de vista |
| `src/workspace.js` | punto en Recientes y en la lista de worktrees, `workspace_watch` al abrir y dejar un workspace |
| `src/styles.css` | el punto, con tokens del tema |
| `tests/ws-attention.test.mjs` (nuevo) | pruebas de `ws-attention.js` |
| `.claude/rules/workspace.md`, `backend.md`, `interfaz.md` | lo nuevo, el evento global y la dependencia; los archivos nuevos en sus `paths` |

## Estilo

Como el código de alrededor: comentarios y textos en español, funciones cortas, el porqué en el
comentario y pruebas con nombre de frase. Por ejemplo, el lector ya decide así si hay trabajo:

```rust
/// Lo que el lector tiene que mirar de cada línea: los agentes nuevos se registran y los
/// turnos y tareas en segundo plano dicen si el orquestador está haciendo algo.
fn watch_line(line: &str, live: &Live, on_task: &dyn Fn(&Value)) {
    let head = &line[..line.len().min(96)];
    if head.starts_with(r#"{"type":"result""#) {
        live.turn.store(false, Ordering::Relaxed);
    } else if head.starts_with(r#"{"type":"system","subtype":"init""#) {
        live.turn.store(true, Ordering::Relaxed);
    }
    // …
}
```

Las peticiones se reconocen por el principio de la línea, como ahí, y solo se parsea la línea entera
cuando hace falta. En CSS, solo tokens (`estilos.md`); el punto no se anima.

## Pruebas

- **Backend** (`cargo test --no-default-features`): A1 a A8 con líneas reales de
  `tests/fixtures/workspace/agente-en-segundo-plano.jsonl` y peticiones `can_use_tool` escritas a
  mano con su forma real (sin rutas personales). El envío de avisos se sustituye por uno que los
  guarda, así se comprueba qué se avisaría sin tocar el escritorio.
- **Interfaz** (`npm test`): `ws-attention.js` sin DOM.
- **Banco sin ventana**: Vite con Tauri simulado (ver `workspace.md`) que emite `workspace-attention`
  para dos workspaces; se comprueba en Chrome sin ventana el punto en Recientes, en la lista de
  worktrees y en el botón Agentes, el texto «Espera tu respuesta» y el nombre accesible. Sin capturar
  la pantalla del usuario ni abrir su nexo.
- **En la app real**: lo comprueba el usuario con el binario instalado (un permiso en un workspace
  mientras mira otro). Un turno real de Claude Code solo con su permiso, con Haiku y esfuerzo bajo.
- Nada de lo existente se cambia para que pase: si una prueba falla, se dice cuál y qué da.

## Límites

- **Siempre:** pasar `npm test`, `cargo test --no-default-features` y `cargo fmt --check` antes de dar
  algo por hecho; actualizar las reglas en el mismo cambio; rutas por `ws_root`; textos en español.
- **Preguntar antes:** otra dependencia aparte de `tauri-plugin-notification`; lanzar un turno real
  de Claude Code; enviar un aviso de prueba al escritorio del usuario; commit, push o PR; instalar el
  binario.
- **Nunca:** contestar un permiso por el usuario ni cambiar el modo de permisos; poner en un aviso la
  entrada de una herramienta o lo que dice el modelo (A7); avisar de lo repetido del historial;
  cambiar cuándo se cierra un orquestador desconectado sin preguntar (el 17-10-2026 el usuario eligió que espere también al turno que Claude Code abre al acabar un agente, al arreglar la detección del final del turno); tocar la configuración de `~/.claude`; abrir
  la app desde la terminal, capturar la pantalla o cerrar el nexo del usuario.

## Criterios de éxito

1. Con un permiso pendiente en el workspace A mientras se mira el B, A lleva el punto y «Espera tu
   respuesta» en Recientes (y en la lista de worktrees si es uno), y el botón Agentes lleva el punto
   desde Grafo, Notas y el panel de nexo.
2. Al contestar ese permiso al volver a A, o si Claude Code lo cancela, los puntos se quitan.
3. El escritorio avisa «‹quién› pide permiso para usar ‹herramienta›» (o pregunta, o plan) con el
   nombre de A de título, sin la entrada de la herramienta.
4. Cuando A queda libre, avisa una sola vez «Terminó el trabajo» (o «Terminó con un error»), también
   si estaba desconectado y se cierra después.
5. Con nexo en primer plano mirando A, no hay avisos de A, pero sí de B.
6. Cinco permisos seguidos de A son cinco avisos.
7. Si Claude Code se cae en A con trabajo a medias, avisa «Se cerró con trabajo a medias» y el punto
   se quita.
8. El chat de Nexo, el historial repetido y parar un orquestador a mano no avisan nunca.
9. Sin servicio de avisos, nexo funciona igual y deja un `warn` en el registro.
10. Pasan `npm test`, `npm run build`, `cargo test --no-default-features` y `cargo fmt --check`.

## Preguntas abiertas

Ninguna: las dos que había (la caída con trabajo a medias y los permisos seguidos) las cerró el
usuario y están en A5 y A4.
