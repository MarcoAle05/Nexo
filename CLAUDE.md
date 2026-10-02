# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Qué es

**nexo**: un "NotebookLM propio" de escritorio (Tauri 2 + Vite, JS sin framework). Interfaz en español, estilo "Órbita" (negro/blanco, paneles de cristal). Tres zonas: fuentes a la izquierda, grafo de la wiki en el centro (a pantalla completa, detrás de los paneles) y terminal a la derecha con dos pestañas: **API** (Nexo, Claude por HTTP) y **Claude Code** (terminal real). Los comentarios, mensajes y textos de UI se escriben en español.

## Comandos

```bash
npm run tauri dev          # app de escritorio con recarga en vivo (arranca Vite en :1420)
npm run dev                # solo la interfaz en el navegador (sin comandos de Tauri)
npm run build              # build del frontend (dist/)
npm run tauri build        # instaladores

cd src-tauri
cargo build --no-default-features   # compila el backend sin abrir ventana
cargo test --no-default-features                     # pruebas unitarias
cargo test --no-default-features <nombre>            # una prueba
cargo test --no-default-features -- --ignored        # pruebas de integración (usan markitdown-mcp y ~/.claude reales)
cargo fmt
```

Si `cargo` no está en el PATH de la sesión: `source ~/.cargo/env`.

Dependencias externas en tiempo de ejecución: `markitdown-mcp` (instalado con `uv tool install --python 3.12 markitdown-mcp`, en `~/.local/bin`), `playwright-mcp` (`npm install -g --prefix ~/.local @playwright/mcp`; usa el Google Chrome instalado), `claude` (Claude Code CLI) y `agy` (Antigravity CLI). En Linux + NVIDIA + Wayland, `main.rs` fija `WEBKIT_DISABLE_DMABUF_RENDERER=1` antes de arrancar para evitar el "Error 71" de WebKitGTK; no quitarlo.

## Modelo de datos: el vault

Todo gira en torno a un vault de Obsidian elegido por el usuario (ruta guardada en `~/.config/com.nexo.app/config.json`, junto con la clave de API opcional; el archivo se escribe con permisos 0600):

- `raw/`: fuentes que vuelca el usuario. `raw/markdown/` guarda las conversiones de MarkItDown (misma ruta relativa, extensión `.md`) y **se oculta** del listado de fuentes. `raw/web/*.md` son páginas web leídas por Antigravity y se listan como tipo `web`.
- `wiki/`: notas organizadas por la IA. `_master-index.md` enlaza el `_index.md` de cada carpeta-tema; notas atómicas en kebab-case enlazadas con `[[...]]`.
- `wiki/fuentes/`: **una ficha por fuente** (frontmatter `source: raw/<ruta>`, resumen, ideas clave, `[[conceptos]]`), más `_index.md`. Las mantiene nexo (`fichas.rs`) y el resumen lo escribe Antigravity; `compile` tiene prohibido escribir ahí. Así cualquier fuente se encuentra por su nombre en Markdown.
- `output/`: respuestas (`ask` añade secciones a `output/query-results.md`).
- `wiki/conversaciones/`: conclusiones que Claude Code guarda al pulsar "Guardar en el grafo" (nodos `conversation`).
- `.nexo/compiled.json`: manifiesto de fuentes ya compiladas (tamaño:mtime) para no recompilar.

Toda ruta que llega desde la interfaz o desde herramientas del modelo pasa por `vault::resolve_inside` (fuentes/notas) o `nexo::wiki_path` (herramientas de compile) para impedir salir del vault.

## Arquitectura

**Backend (`src-tauri/src/`)**, todo expuesto como `#[tauri::command]` registrados en `lib.rs`:

- `vault.rs`: configuración, elección del vault, detección de vaults de Obsidian (lee `obsidian.json`) y creación de la estructura.
- `sources.rs`: listar/añadir (copia sin sobrescribir)/abrir/quitar (a la papelera)/renombrar fuentes de `raw/`. Rechaza PDF/Office/EPUB si no está `markitdown-mcp`. `list_sources` sincroniza las fichas (crea las que faltan, manda a la papelera las huérfanas). Renombrar arrastra la conversión, la ficha, el manifiesto y las citas `raw/…` de la wiki.
- `fichas.rs`: fichas de `wiki/fuentes/` (crear, leer para la pestaña de resumen, escribir resumen, renombrar, índice).
- `mcp.rs`: cliente MCP mínimo por stdio (JSON-RPC por líneas) compartido por MarkItDown y Playwright.
- `markitdown.rs`: `markitdown-mcp`, herramienta `convert_to_markdown`. Conversión local, sin IA.
- `playwright.rs`: `playwright-mcp --headless --isolated --browser chrome` (o, si hay navegador cargado y nadie lo tiene abierto, el perfil de `browser.rs` sin ventana): `browser_navigate` → `browser_wait_for` → `browser_evaluate` (extrae el HTML renderizado del contenido principal; el resultado viene en la sección `### Result` del texto) → MarkItDown convierte ese HTML a Markdown. Sin IA.
- `graph.rs`: lee `wiki/` y construye nodos/aristas a partir de `[[enlaces]]` y `[texto](nota.md)`, resolviendo como Obsidian (por ruta o por nombre; con nombres repetidos gana la misma carpeta). Los enlaces rotos son nodos `missing`; las fichas son nodos `source` (con su `source`), las notas de `conversaciones/` son nodos `conversation`, y toda nota que contenga `raw/<ruta>` se une a la ficha de esa fuente.
- `claude.rs`: cliente HTTP de la Messages API (Rust no tiene SDK oficial). Modelo `claude-opus-5-5`, `fallbacks: "default"` con beta `server-side-fallback-2026-07-01`, SSE reconstruido en bloques completos (incluido el pensamiento con su firma). Comprobar siempre `refusal()` antes de usar el contenido.
- `nexo.rs`: el asistente Nexo. `compile` (primero convierte con MarkItDown y envía el Markdown; bucle de herramientas `list_wiki`/`read_note`/`write_note` por fuente), `ask` y chat con la wiki como contexto (presupuesto de caracteres; los índices entran primero). El historial del chat vive en Rust (`ChatState`), es **solo de anexar** y congela el `system` al empezar: no editar ni podar mensajes anteriores (rompe los bloques de pensamiento preservados).
- `claude_code.rs`: PTY (`portable-pty`) que ejecuta `claude` con `--append-system-prompt` (papel de **orquestador**: responde sobre las fuentes buscando en `wiki/fuentes/`, delega la adquisición de fuentes en nexo/Antigravity y sabe guardar conversaciones). Recibe el MCP `playwright` con `--mcp-config` (`navegador/claude-mcp.json`): Chrome **con ventana** sobre el perfil de `browser.rs`. **Una sesión por apertura de nexo**: la primera vez `--session-id <uuid> --name "Nexo · fecha"` en el vault, después `--resume <uuid>` (solo si ya existe su `.jsonl`); al cerrar la app, la siguiente empieza limpia.
- `agy.rs`: Antigravity es el **encargado de las fuentes**. `add_web_source(url, playwright)`: sin Playwright, una sola llamada a agy (página → `raw/web/*.md` + resumen de la ficha); con Playwright, la página la lee `playwright.rs` y agy solo resume (sin permiso `read_url`). Ambas fases salen en la misma tarea de Actividad y `summarize_source` (copia el Markdown de la fuente como `fuente.md` en una carpeta vacía de caché y le pide leerlo con `view_file`). Ejecuta `agy "-p=<prompt>" --output-format stream-json --json-schema …`: el prompt va **como argumento** (con `-p=-` no le llega y se pone a explorar carpetas). Los trabajos se serializan (uno a la vez) y cada paso se emite como evento global `agy-activity` para el panel "Actividad Antigravity". Permisos: solo `read_url(<dominio>)` en `~/.gemini/antigravity-cli/settings.json`; nunca `read_url(*)`, `command(...)` ni `--dangerously-skip-permissions` (un permiso denegado aborta la tarea entera).
- `browser.rs`: botón **Cargar navegador** (en Conexiones). Copia el perfil del navegador predeterminado (`xdg-settings`; solo familia Chromium) a `<app_data>/navegador/perfil` sin cachés, extensiones ni sesiones de pestañas, lo abre con Playwright sin ventana para contar las cookies descifradas y solo entonces borra la copia anterior (si Chrome la tenía abierta, lo cierra). `navegador/playwright.json` quita `--password-store=basic`/`--use-mock-keychain` de los argumentos por defecto de Playwright y fija `--password-store=gnome-libsecret` fuera de KDE: sin eso las cookies `v11` (cifradas con el llavero) no se descifran. El perfil original nunca se modifica, y Chrome 136+ no permite automatizar el perfil por defecto, por eso se copia.
- `usage.rs`: límites de uso de las suscripciones, sin gastar cuota. Claude: `GET https://api.anthropic.com/api/oauth/usage` (beta `oauth-2025-04-20`) con el token de `~/.claude/.credentials.json`, solo lectura (si caducó, se pide abrir Claude Code; no se renueva aquí para no invalidar el de Claude Code). Antigravity: no tiene salida para máquinas, así que se abre `agy` en un PTY en una carpeta de caché, se acepta la confianza en la carpeta, se escribe `/usage` y se lee la pantalla «Models & Quota» (porcentaje *restante* por grupo de modelos y «Refreshes in»). ¡Ojo!: `agy -p=/usage` no sirve, en modo `-p` los comandos con barra se mandan al modelo como prompt. Caché de 60 s (Claude) y 120 s (agy); «Comprobar de nuevo» fuerza. En la interfaz es la mini pestaña junto a Claude Code y Antigravity en Conexiones.
- `connections.rs`: estado de MarkItDown (saludo MCP + `tools/list`), API (Models API, sin tokens), Antigravity y Claude Code.

El streaming hacia la interfaz usa `tauri::ipc::Channel` (eventos `{kind, text}`); la única excepción es `agy-activity`, un evento global porque el panel de actividad escucha todas las tareas de Antigravity.

**Frontend (`src/`)**:

- `main.js`: estado y cableado de toda la UI (vault, fuentes y su pestaña de resumen, conexiones, terminal/comandos, pestañas, actividad de Antigravity). Al añadir fuentes: ficha inmediata → conversión MarkItDown → resumen en la cola de Antigravity. En la pestaña API, los comandos conocidos (`COMMANDS`) se ejecutan y cualquier otro texto va al chat de Nexo.
- `graph.js`: grafo en canvas con d3-force/d3-zoom y paleta "galaxia" (neones suaves por tema, `colorFor`; cada nodo se dibuja como estrella con `drawStar`, las fuentes en ámbar y los índices de fuentes y conversaciones (`fuentes/_index.md`, `conversaciones/_index.md`) como minigalaxias espirales con `drawGalaxy`; sin fondo). `focus(id)` centra un nodo. Las capas superiores usan `pointer-events: none` para que el ratón llegue al canvas salvo en paneles y controles; `getViewport()` calcula el hueco central entre paneles para centrar.
- `note-view.js`: Markdown de las notas → HTML con `marked` (extensión para `[[enlaces]]`) **siempre pasado por DOMPurify** (las notas pueden traer contenido de webs) y `resolveLink`, que resuelve enlaces igual que `graph.rs`. Al elegir un nodo que no es fuente, `main.js` (`showNode`) muestra en la carta de la izquierda su contenido (`note_view`: cuerpo sin frontmatter + propiedades), sus conexiones y enlaces que llevan a otros nodos.
- `claude-code.js`: xterm.js conectado a los comandos `claude_code_*`. Las pulsaciones se envían en orden y agrupadas (cola `send`); `show()` reajusta, repinta y enfoca al volver a mostrarse (xterm deja de dibujar mientras está oculto). `say(text)` escribe y envía un mensaje (lo usa "Guardar en el grafo").
- Solo `main.js` distingue escritorio/navegador con `isTauri()`; en el navegador las funciones de Tauri muestran un aviso.

## Cosas que no se ven a simple vista

- **Altura fija**: `.app` mide exactamente `100dvh` y cada carta hace scroll por dentro. La cadena de contenedores flex necesita `min-height: 0` hasta la lista o terminal que se desplaza; si se quita en algún nivel, la app vuelve a crecer con el contenido.
- **Cartas expandibles**: las clases `expand-left`/`expand-right`/`expanded` de `.layout` pliegan el `.stage` (flex 0, margen negativo que absorbe un hueco) y hacen crecer la carta hasta el borde de la otra; con las dos, ambas pasan a `flex: 999 1 0` y se reparten el ancho. Solo en ≥768px; el estado vive en `localStorage` (`nexo.expanded`).
- **Recarga de Vite ≠ reinicio**: al guardar un `.js`, Vite recarga la ventana y se pierde el estado del frontend, pero el backend sigue vivo (sesión PTY de Claude Code, trabajos de agy). Por eso el lector del PTY sigue vaciando la salida aunque el `Channel` ya no tenga destinatario; si se detuviera, el PTY se llenaría y bloquearía a Claude Code. Cambiar código en `src-tauri/` sí reinicia la app.
- **La sesión de Claude Code depende de la carpeta**: Claude Code guarda sesiones y memoria por `cwd` (`~/.claude/projects/<cwd codificado>/`). Las sesiones de nexo arrancan en la raíz del vault; si se cambia esa carpeta, `--resume` no encuentra la sesión y la memoria del orquestador queda en otro sitio.
