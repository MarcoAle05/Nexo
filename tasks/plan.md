# Plan: avisos de los workspaces

Implementa `docs/spec-avisos-workspaces.md` (aprobada el 17-10-2026). Las tareas, con sus criterios y
cómo se comprueban, están en `tasks/todo.md`; aquí van el orden, las decisiones y los riesgos.

## Resumen

El backend aprende a ver cuándo un workspace espera respuesta, termina o se cae, aunque la interfaz
no lo esté mirando. Con eso avisa al escritorio y emite `workspace-attention`. La interfaz pinta el
punto en el botón Agentes, en Recientes y en la lista de worktrees, y le dice al backend qué
workspace tiene en pantalla para no avisar de lo que ya se ve.

## Decisiones de arquitectura

- **El estado vive en `Live`** (`workspace_run.rs`), junto a `turn` y `background`: las peticiones
  `can_use_tool` sin contestar (`request_id` → quién y qué herramienta), si el último `result` fue un
  error y la última instantánea emitida (`waiting`, `busy`) para no repetir eventos.
- **Tres sitios cambian el estado:** el lector (`watch_line`: llega una petición o se cancela, empieza
  o acaba un turno, cambian las tareas en segundo plano), `workspace_send` (la interfaz contesta un
  permiso con `control_response`) y el final del proceso. Los tres pasan por una sola función que
  compara con la última instantánea y, si cambió, emite el evento. Se emite **fuera** del cerrojo de
  `shared`.
- **`pump` recibe un tercer manejador** (`on_change`, como `on_task` y `on_idle`) con lo que pasó:
  pide algo, quedó libre o cambió el estado. Las pruebas lo sustituyen por uno que lo apunta, así
  que el lector se prueba sin Tauri, como ahora.
- **`avisos.rs` no sabe de procesos.** Arma los textos (tabla de A4, fin, error, caída), decide si se
  avisa (A6: ventana con foco y ese workspace en pantalla → no) y envía detrás de un rasgo
  (`Aviso`): el de verdad usa `tauri-plugin-notification`; el de las pruebas guarda los avisos o
  falla a propósito.
- **Una caída** se reconoce en el hilo del lector al acabar la salida: el proceso seguía siendo el del
  workspace (nadie lo sacó con `workspace_stop`, `workspace_reset` ni el cierre por libre) y estaba
  `busy` o esperando.
- **Qué hay en pantalla y el foco** los calcula `main.js`, que ya sabe la vista, el panel y el
  workspace (`onChange` de `workspace.js`) y oye `focus`/`blur` de la ventana, y se los dice al
  backend con `workspace_watch` solo cuando cambian. No se usa `is_focused()` del backend: saber si
  funciona en Hyprland obligaría a lanzar la app.

## Contrato entre backend e interfaz

Fijado antes de empezar para que las dos partes vayan en paralelo:

- Evento global `workspace-attention`: `{ path: string, waiting: number, busy: boolean }`. `path` es
  la carpeta tal como la da `ws_root` (la misma forma que las rutas de `workspace_list`).
- `workspace_attention()` → `[{ path, waiting, busy }]`, los workspaces con proceso.
- `workspace_watch({ path: string | null, focused: boolean })` → nada. Una ruta que no es de la lista
  se rechaza.
- **En la interfaz, un módulo sin DOM** (`ws-attention.js`) junta los eventos de todos los workspaces
  y da los textos («Espera tu respuesta», el nombre accesible del botón Agentes). Se prueba con node,
  como `ws-stream.js`.

## Orden

```
T1 estado en el lector ──→ T2 evento y comandos ──┬──→ T5 punto: botón Agentes ──→ T6 punto: listas ──→ T7 banco
                                                   │
T3 textos y decisión (avisos.rs) ──→ T4 plugin y envío ┘
                                                                         T8 reglas ──→ T9 cierre
```

- **Fase 1 (backend, T1-T4):** primero, porque es donde está el riesgo (la dependencia nueva y si
  `is_focused` funciona en Hyprland), y todo lo demás cuelga de ella. T3 no depende de T1-T2 y puede
  ir en paralelo; T4 necesita las dos ramas.
- **Fase 2 (interfaz, T5-T7):** el punto, de punta a punta, y su comprobación en el banco.
- **Fase 3 (T8-T9):** reglas al día y la comprobación final. La instalación y la prueba en la app real
  las hace o pide el usuario.

Puntos de control después de T4 y de T7 (ver `tasks/todo.md`).

## Riesgos

| Riesgo | Impacto | Cómo se cubre |
|---|---|---|
| El foco que cuenta la interfaz (`focus`/`blur`) no llega en algún caso (ventana minimizada, otro escritorio de Hyprland) | Medio: avisaría de más o de menos | Se comprueba en el banco; en la app real lo comprueba el usuario. Peor caso: un aviso de más, nunca uno de menos (sin noticias de foco se toma como sin foco) |
| `tauri-plugin-notification` no casa con Tauri 2.12 o alarga la compilación del CI | Medio | Se añade y compila en T4, lo primero de esa tarea. Usa D-Bus en Rust puro (`zbus`), sin librerías del sistema nuevas en el CI |
| La forma de `control_response` que manda la interfaz (`allowLine`/`denyLine` de `chat-model.js`) no es la que espera el lector | Medio: el punto no se quitaría | Las pruebas de T1 usan las líneas que generan esas funciones, copiadas tal cual |
| Emitir el evento con el cerrojo de `shared` cogido bloquea el lector | Medio | Se emite después de soltarlo (decisión de arriba) |
| El banco de pruebas de la interfaz ya no está (se limpió la carpeta temporal) | Bajo: solo cuesta rehacerlo | T7 rehace un simulado mínimo (las órdenes que pide la carta y el evento), fuera del repo |
| Avisos repetidos de una misma petición si Claude Code la reenvía | Bajo | Se cuenta por `request_id`: una petición que ya está no vuelve a avisar |

## Preguntas abiertas

- `tasks/plan.md` y `tasks/todo.md` son nuevos en el repo. ¿Se suben con el cambio o se quedan fuera
  de git? Propuesta: fuera; lo que dura es la spec en `docs/`.
