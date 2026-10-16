# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Aquí solo va lo que vale en cualquier tarea. Lo de cada zona del código está en `.claude/rules/` (tabla de abajo) y se carga solo cuando se lee o se edita un archivo que coincide con el `paths` de la regla. Si una tarea toca una zona antes de abrir ninguno de sus archivos (planear, contestar una pregunta, revisar un diff), lee antes su regla.

## Qué es

**nexo**: un "NotebookLM propio" de escritorio (Tauri 2 + Vite, JS sin framework). Interfaz en español. Tres zonas: fuentes a la izquierda, en el centro (a pantalla completa, detrás de los paneles) la **galaxia** de puntos con las skills alrededor, el **grafo** de la wiki o (en la vista Agentes) el **registro de agentes** o las ventanas de un **workspace** (una carpeta de proyecto con su orquestador y sus agentes), y a la derecha el **chat con Claude**: Claude Code con la suscripción del usuario, sin terminal, pintado con el diseño de la app. Lo que va por la API de Anthropic (Nexo: compilar, preguntar, la clave) ya no tiene pestaña propia: se lanza desde el menú «/» de ese chat. Todo gira en torno a un vault de Obsidian elegido por el usuario (`raw/` → `wiki/` → `output/`, ver `backend.md`). Los comentarios, mensajes y textos de UI se escriben en español.

Backend en `src-tauri/src/` (crate `app`) y `crates/` (`nexo-core`, `nexo-mcp`); frontend en `src/` e `index.html`; pruebas de la interfaz en `tests/`.

## Comandos

```bash
npm run tauri dev          # app de escritorio con recarga en vivo (arranca Vite en :1420)
npm run dev                # solo la interfaz en el navegador (sin comandos de Tauri)
npm run build              # build del frontend (dist/)
npm run tauri build        # instaladores
npm test                   # pruebas de la interfaz (node --test tests/*.test.mjs)

cd src-tauri
cargo build --no-default-features   # compila el backend sin abrir ventana
cargo test --no-default-features                     # pruebas unitarias de la app
cargo test --no-default-features <nombre>            # una prueba
cargo test --no-default-features -- --ignored        # pruebas de integración (usan markitdown-mcp y ~/.claude reales)
cargo test -p nexo-core                              # pruebas de la lógica sin Tauri (grafo, vaultgit, guardianes de rutas)
cargo test -p nexo-mcp                               # pruebas de protocolo del servidor MCP (lanzan el binario)
cargo fmt --all                                      # formatea los dos crates
```

El CI (`.github/workflows/ci.yml`) corre en cada PR y en `main`: `npm test`, `npm run build`, `cargo fmt --check` y `cargo test --no-default-features` (instala antes las dependencias de sistema de Tauri). El job `core` compila, prueba, formatea y pasa clippy a `nexo-core` en un runner **sin** esas dependencias, y el job `mcp` hace lo mismo (sin clippy) con `nexo-mcp`.

Casos dorados del grafo: cada `expected.json` de `crates/nexo-core/tests/fixtures/grafo/` se escribe a mano y nunca se regenera con la salida del código, y las pruebas y casos dados no se cambian para que pasen: si uno falla, se dice cuál y qué da (procedimiento en `grafo-datos.md`).

Si `cargo` no está en el PATH de la sesión: `source ~/.cargo/env`.

## Reglas por zona

| Regla (`.claude/rules/`) | Qué cubre | Se carga al abrir |
| --- | --- | --- |
| `backend.md` | Workspace de Cargo, comandos de Tauri, streaming, dependencias externas, el vault y los guardianes de rutas, `vault.rs` | cualquier `.rs`, los `Cargo.toml`, `tauri.conf.json` |
| `grafo-datos.md` | Construcción del grafo, casos dorados, comandos del grafo y el vigilante de `wiki/` | `graph.rs` (app y `nexo-core`), `crates/nexo-core/tests/`, `docs/spec-grafo.md` |
| `vaultgit.md` | Commits automáticos del vault | `vaultgit.rs`, `nexo.rs`, `docs/spec-commits-automaticos.md` |
| `nexo-mcp.md` | Servidor MCP de solo lectura | `crates/nexo-mcp/`, `query.rs`, `docs/spec-mcp.md` |
| `fuentes.md` | Fuentes: `raw/`, fichas, MarkItDown, Playwright, Antigravity, «Cargar navegador» | `sources.rs`, `fichas.rs`, `mcp.rs`, `markitdown.rs`, `playwright.rs`, `agy.rs`, `browser.rs` |
| `api-nexo.md` | Messages API, el asistente Nexo (`compile`, `ask`), Conexiones | `claude.rs`, `nexo.rs`, `connections.rs` |
| `claude-code.md` | El chat con Claude Code: proceso, protocolo, vista, sesiones y recargas | `claude_code.rs`, `chat-model.js`, `chat.js` y sus pruebas |
| `uso-y-registro.md` | Límites de uso y registro de agentes | `usage.rs`, `registro.rs`, `registro.js` |
| `render.md` | Cómo pinta WebKitGTK (GPU y CPU de serie, escalera de seguridad si cae, composición entera, `drmWaitVBlank`) | `render.rs`, `main.rs`, `build.rs` |
| `rendimiento.md` | Rendimiento de lo que se pinta en WebKitGTK | `styles.css`, `galaxy.js`, `graph.js`, `notes.js`, `render.rs` |
| `skills-y-agentes.md` | Skills y agentes del vault, su órbita y la carta Agentes | `skills.rs`, `agents.rs`, `skills.js`, `agents.js` |
| `workspace.md` | Workspaces de la vista Agentes: carpetas de proyecto, su orquestador, el registro de sus agentes, los extras, las pestañas y las ventanas | `workspace.rs`, `workspace_run.rs`, `workspace.js`, `ws-*.js` y sus pruebas |
| `galaxia.md` | La galaxia inicial (WebGL, dos lienzos, `frameFor`) | `galaxy.js` y sus pruebas |
| `grafo-vista.md` | El grafo en canvas y la vista de notas en Markdown | `graph.js`, `note-view.js` |
| `notas.md` | Apartado Notas: datos, editor, lienzo, zoom y escritorios | `notes.js`, `note-editor.js`, `desk-layout.js`, `desks.js`, `notas.rs` |
| `interfaz.md` | `main.js`, barra superior, Configuración, vistas y escenas, cartas minimizadas y expandibles | `main.js`, `dom.js`, `settings.js`, `modal.js`, `index.html`, `styles.css` |
| `estilos.md` | Temas y sus tokens, tipografía, paleta, radios, contraste, anchos y alturas | `styles.css`, `index.html`, `themes.js`, `public/tema.js`, `tests/themes.test.mjs` |

## Siempre

- Toda ruta que llega desde la interfaz o desde herramientas del modelo pasa por un guardián que impide salir del vault (detalle en `backend.md`).
- Si un cambio contradice lo que dice una regla, se actualiza la regla en el mismo cambio. Un archivo de código nuevo se añade al `paths` de la regla de su zona (o a una regla nueva), y lo que valga para todo el repositorio va aquí y no en una regla.

## Interfaces: construir desde un diseño y revisar

Las skills `better-*` (`better-ui`, `better-colors`, `better-typography`, `better-layout`, `better-accessibility`, `better-writing` y `better-interface`, que las reúne), `build-design`, `interface-review`, `explain-interface`, `break`, `state-machine` y `variant` vienen de `jakubkrehel/skills` (instaladas con `npx skills add`, versión fijada en `skills-lock.json`). Se usan cuando se pide construir desde una imagen o un Figma (`build-design`), revisar un cambio (`interface-review`) o afinar color, superficies, tipografía, maquetación o accesibilidad; sus recomendaciones se aplican si el usuario lo pide. Como con las demás, las decisiones de este archivo y de `.claude/rules/` mandan sobre sus reglas genéricas (el tamaño de 44 px de los controles, nada de sombras grandes en lo que se mueve, la notación `rgba()`/hex de la hoja de estilos en vez de `oklch()`). Lo que se hizo con ellas en cada parte está en `estilos.md` y `claude-code.md`.

## Impeccable: crítica, auditoría y pulido

`.claude/skills/impeccable` viene de `pbakaus/impeccable` (instalada con `npx skills add`, versión fijada en `skills-lock.json`). Se usa cuando se pide: `critique`, `audit` o `polish` (sus guías están en `reference/`) y, a diferencia de las de abajo, sus recomendaciones sí se aplican si el usuario lo pide. Su lanzador (`scripts/impeccable`) descarga una vez su motor verificado a `~/.impeccable/bin/`; `impeccable detect --json index.html src` es el detector de patrones y las críticas se archivan en `.impeccable/critique/`. El aviso `side-tab` por el `border-left` de las citas de las notas es un falso positivo. Las decisiones de este archivo y de `.claude/rules/` mandan sobre sus reglas genéricas, como con las de abajo (la etiqueta sobre el título de cada carta, «Cuaderno», la pidió el usuario aunque Impeccable prohíbe los *eyebrows*).

## Skills de diseño: solo diagnóstico

`.claude/skills/design-taste-frontend` y `.claude/skills/redesign-existing-projects` vienen de `Leonxlnx/taste-skill` (instaladas con `npx skills add`, versión fijada en `skills-lock.json`). **Solo sirven para dar diagnósticos de diseño**: cuando se pida revisar o diagnosticar el diseño, se usan como lista de comprobación y la respuesta es un diagnóstico (qué falla, dónde, con `archivo:línea`, por qué y qué se propone), ordenado por impacto.

- Mientras se diagnostica no se edita ningún archivo, no se instalan paquetes ni fuentes y no se generan imágenes, aunque la skill lo pida (sus pasos «Fix», «Upgrade», el uso obligatorio de generadores de imágenes y el «Pre-Flight Check» de código no aplican). Los cambios se hacen solo si el usuario los pide después, como cualquier otra tarea.
- Están pensadas para webs y landings; nexo es una app de escritorio densa (`design-taste-frontend` excluye expresamente los paneles de producto). Se aplica solo lo que tenga sentido aquí y se dice cuándo una regla no encaja.
- Las decisiones de este archivo y de `.claude/rules/` mandan sobre sus recomendaciones genéricas: nada de `backdrop-filter`, ruido o sombras grandes en lo que se mueve (rendimiento en WebKitGTK), la tipografía y el estilo «Órbita» elegidos por el usuario, la escala real de la barra de contexto, etc. Si una de ellas choca con la skill, se menciona como observación, no como fallo.
