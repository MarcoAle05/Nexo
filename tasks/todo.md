# Tareas: avisos de los workspaces

Spec: `docs/spec-avisos-workspaces.md`. Plan: `tasks/plan.md`. Las reglas de la spec (A1…A8) dicen
qué prueba va en cada tarea.

## Fase 1: backend

### T1. El lector sabe qué espera respuesta (A1)

En `Live`, las peticiones `can_use_tool` sin contestar (por `request_id`, con quién y qué herramienta)
y si el último `result` fue un error. Entran con el `control_request`; salen con la
`control_response` que manda la interfaz por `workspace_send`, con `control_cancel_request` o al
cerrarse el proceso.

- [x] Una petición nueva cuenta una vez aunque llegue repetida.
- [x] Se quita al contestarla (con las líneas reales de `allowLine` y `denyLine`) o al cancelarla.
- [x] `idle` sigue igual: una petición pendiente deja el turno abierto.

Comprobar: `cargo test --no-default-features` con `el_lector_cuenta_los_permisos_sin_contestar`,
`un_permiso_contestado_o_cancelado_deja_de_esperar` y `al_cerrarse_el_proceso_no_espera_nada`; las
pruebas del lector que ya hay pasan sin tocarlas.
Depende de: nada. Archivos: `src-tauri/src/workspace_run.rs`. Tamaño: S.

### T2. Evento `workspace-attention` y comandos (A2, la parte de A6 del backend)

Una sola función compara el estado (`waiting`, `busy`) con el último emitido y, si cambió, emite
`workspace-attention` con `{ path, waiting, busy }`, fuera del cerrojo. La llaman el lector,
`workspace_send` y el final del proceso. Comandos nuevos: `workspace_attention` (el estado de todos
los que tienen proceso) y `workspace_watch({ path | null, focused })` (qué workspace hay en pantalla
y si la ventana tiene el foco, guardado en `WorkspaceRuns`; sin noticias, sin foco).

- [x] Sin cambios no se emite nada; cada cambio se emite una vez.
- [x] `workspace_attention` da los workspaces con proceso y su estado; `workspace_watch` pasa por
      `ws_root` (una ruta fuera de la lista se rechaza).
- [x] Los dos comandos están registrados en `lib.rs`.

Comprobar: `cargo test --no-default-features` con `solo_se_emite_cuando_cambia_el_estado`;
`cargo fmt --all --check`.
Depende de: T1. Archivos: `src-tauri/src/workspace_run.rs`, `src-tauri/src/lib.rs`. Tamaño: S.

### T3. `avisos.rs`: textos y decisión (A4, A5, A6, A7)

Módulo nuevo sin procesos: arma título y texto de cada aviso (permiso, pregunta, plan, terminó,
terminó con error, se cerró con trabajo a medias), busca quién es el agente en el registro del
workspace (o «Un agente») y decide si se avisa: no, si la ventana tiene el foco y ese es el workspace
en pantalla. El envío va detrás del rasgo `Aviso`.

- [x] Los textos son los de la tabla de A4 y los de A5, con el nombre del workspace de título.
- [x] Ningún aviso lleva la entrada de la herramienta, el texto de una pregunta o de un plan (A7).
- [x] La decisión de A6 cubre los cuatro casos (foco sí/no × ese workspace sí/no).

Comprobar: `cargo test --no-default-features aviso` con
`el_aviso_de_un_permiso_dice_quien_y_que_herramienta`,
`el_aviso_de_una_pregunta_no_lleva_lo_que_pregunta`, `los_avisos_no_llevan_la_entrada_de_la_herramienta`,
`no_avisa_si_se_esta_mirando_ese_workspace` y `avisa_si_la_ventana_no_tiene_el_foco_o_se_mira_otra_cosa`.
Depende de: nada (puede ir en paralelo con T1-T2). Archivos: `src-tauri/src/avisos.rs` (nuevo),
`src-tauri/src/lib.rs` (`mod avisos`). Tamaño: S.

### T4. Plugin y envío: permisos, fin y caída (A4, A5, A8)

Primero, añadir `tauri-plugin-notification = "2"` y comprobar que compila. Luego conectar el lector con `avisos.rs`: cada petición nueva
avisa (una por petición), el paso a libre avisa una vez («Terminó el trabajo» o «Terminó con un
error»), un orquestador desconectado avisa antes de cerrarse por libre y una caída con trabajo a
medias avisa «Se cerró con trabajo a medias». Un aviso que falla deja un `warn` y nada más.

- [x] Avisa una vez al quedar libre y no en cada turno; parar a mano (`workspace_stop`,
      `workspace_reset`) no avisa.
- [x] Una caída con el proceso `busy` o esperando avisa; un cierre por libre no.
- [x] Un envío que falla no para el lector.

Comprobar: `cargo test --no-default-features` con
`avisa_una_vez_al_quedar_libre_y_no_en_cada_turno`, `un_turno_con_error_avisa_del_error`,
`parar_el_orquestador_a_mano_no_avisa`, `si_claude_code_se_cae_con_trabajo_avisa` y
`un_aviso_que_falla_no_para_el_lector`; `cargo build --no-default-features`;
`cargo fmt --all --check`.
Depende de: T2, T3. Archivos: `src-tauri/Cargo.toml`, `Cargo.lock`, `src-tauri/src/lib.rs`,
`src-tauri/src/workspace_run.rs`, `src-tauri/src/avisos.rs`. Tamaño: M.

### Punto de control: backend

- [ ] `cargo test --no-default-features`, `cargo test -p nexo-core` y `cargo fmt --all --check` pasan.
- [ ] Se repasa con el usuario antes de tocar la interfaz.

## Fase 2: interfaz

### T5. Punto en el botón Agentes y `workspace_watch` (A3, A6)

`ws-attention.js` (sin DOM): junta los eventos de todos los workspaces y da los textos. `main.js`
carga el estado con `workspace_attention`, escucha `workspace-attention`, pone el punto y el nombre
accesible en el botón Agentes, y llama a `workspace_watch` cuando cambia lo que hay en pantalla
(vista, panel, workspace) o el foco de la ventana (`focus`/`blur`). El punto, en `styles.css`, con el color de «Esperando respuesta» de las
pestañas y sin animación.

- [x] Con algún workspace esperando, el botón Agentes lleva el punto y dice «Agentes: N workspaces
      esperan tu respuesta» (o «1 workspace espera…»); sin ninguno, ni punto ni texto de más.
- [x] `workspace_watch` se llama solo cuando cambia lo que hay en pantalla o el foco, con `path: null`
      fuera de un workspace.

Comprobar: `npm test` con `tests/ws-attention.test.mjs` (`el estado junta los eventos de todos los
workspaces`, `el botón Agentes dice cuántos esperan`, `un workspace que deja de esperar pierde el
punto`); `npm run build`; `node --test tests/themes.test.mjs` (los tokens del punto).
Depende de: T2. Archivos: `src/ws-attention.js` (nuevo), `tests/ws-attention.test.mjs` (nuevo),
`src/main.js`, `src/styles.css`. Tamaño: M.

### T6. Punto en Recientes y en la lista de worktrees (A3)

Cada fila de Recientes y de la lista de worktrees de un workspace que espera lleva el punto y el texto
«Espera tu respuesta»; las listas se repintan si cambia el estado con el menú abierto.

- [x] La fila dice «Espera tu respuesta» (también para lectores de pantalla) y lleva el punto.
- [x] Al dejar de esperar, la fila vuelve a ser como antes sin cerrar el menú.

Comprobar: `npm test`; `npm run build`; el banco de T7.
Depende de: T5. Archivos: `src/workspace.js`, `src/styles.css`. Tamaño: S.

### T7. Banco sin ventana

Un simulado mínimo de Tauri fuera del repo (en la carpeta temporal de la sesión) con dos workspaces,
uno con worktrees, que emite `workspace-attention`. En Chrome sin ventana, sobre una copia propia de
la interfaz (nunca la pantalla del usuario ni su nexo): el punto en el botón Agentes desde Grafo y
Notas, en Recientes y en la lista de worktrees, el nombre accesible y que se quita al dejar de esperar,
en Órbita y en un tema claro.

- [x] Las tres listas y el botón se ven y se leen bien en un tema oscuro y uno claro.
- [x] No hay errores en la consola atribuibles al cambio.

Comprobar: capturas del banco revisadas.
Depende de: T6. Archivos: ninguno del repo. Tamaño: S.

### Punto de control: interfaz

- [ ] `npm test` y `npm run build` pasan.
- [ ] Las capturas del banco se repasan con el usuario.

## Fase 3: cierre

### T8. Reglas al día

`workspace.md` (estado de atención, comandos, `avisos.rs` y `ws-attention.js`, y los dos en `paths`),
`backend.md` (el evento global junto a `agy-activity`, la dependencia nueva, el módulo en la lista) e
`interfaz.md` (el punto del botón Agentes).

- [x] Ninguna regla contradice lo hecho y los archivos nuevos están en el `paths` de su regla.

Comprobar: leer las tres reglas contra el diff.
Depende de: T4, T6. Archivos: `.claude/rules/workspace.md`, `.claude/rules/backend.md`,
`.claude/rules/interfaz.md`. Tamaño: S.

### T9. Comprobación final

- [x] Los diez criterios de éxito de la spec, uno por uno: los automáticos con sus pruebas y los de la
      app real anotados para que los compruebe el usuario.
- [x] `npm test`, `npm run build`, `cargo test --no-default-features`, `cargo test -p nexo-core` y
      `cargo fmt --all --check` pasan.

Depende de: T1-T8. Sin commit, push, instalación ni turnos reales de Claude Code salvo que el usuario
lo pida.
