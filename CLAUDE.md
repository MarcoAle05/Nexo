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

Dependencias externas en tiempo de ejecución: `markitdown-mcp` (instalado con `uv tool install --python 3.12 markitdown-mcp`, en `~/.local/bin`) y `claude` (Claude Code CLI). En Linux + NVIDIA + Wayland, `main.rs` fija `WEBKIT_DISABLE_DMABUF_RENDERER=1` antes de arrancar para evitar el "Error 71" de WebKitGTK; no quitarlo.

## Modelo de datos: el vault

Todo gira en torno a un vault de Obsidian elegido por el usuario (ruta guardada en `~/.config/com.nexo.app/config.json`, junto con la clave de API opcional; el archivo se escribe con permisos 0600):

- `raw/`: fuentes que vuelca el usuario. `raw/markdown/` guarda las conversiones de MarkItDown (misma ruta relativa, extensión `.md`) y **se oculta** del listado de fuentes.
- `wiki/`: notas organizadas por la IA. `_master-index.md` enlaza el `_index.md` de cada carpeta-tema; notas atómicas en kebab-case enlazadas con `[[...]]`.
- `output/`: respuestas (`ask` añade secciones a `output/query-results.md`).
- `.nexo/compiled.json`: manifiesto de fuentes ya compiladas (tamaño:mtime) para no recompilar.

Toda ruta que llega desde la interfaz o desde herramientas del modelo pasa por `vault::resolve_inside` (fuentes/notas) o `nexo::wiki_path` (herramientas de compile) para impedir salir del vault.

## Arquitectura

**Backend (`src-tauri/src/`)**, todo expuesto como `#[tauri::command]` registrados en `lib.rs`:

- `vault.rs`: configuración, elección del vault, detección de vaults de Obsidian (lee `obsidian.json`) y creación de la estructura.
- `sources.rs`: listar/añadir (copia sin sobrescribir)/abrir/quitar (a la papelera) fuentes de `raw/`. Rechaza PDF/Office/EPUB si no está `markitdown-mcp`.
- `markitdown.rs`: cliente MCP mínimo por stdio (JSON-RPC por líneas) contra `markitdown-mcp`, herramienta `convert_to_markdown`. Conversión local, sin IA.
- `graph.rs`: lee `wiki/` y construye nodos/aristas a partir de `[[enlaces]]` y `[texto](nota.md)`, resolviendo como Obsidian (por ruta o por nombre; con nombres repetidos gana la misma carpeta). Los enlaces rotos son nodos `missing`.
- `claude.rs`: cliente HTTP de la Messages API (Rust no tiene SDK oficial). Modelo `claude-opus-5-5`, `fallbacks: "default"` con beta `server-side-fallback-2026-07-01`, SSE reconstruido en bloques completos (incluido el pensamiento con su firma). Comprobar siempre `refusal()` antes de usar el contenido.
- `nexo.rs`: el asistente Nexo. `compile` (primero convierte con MarkItDown y envía el Markdown; bucle de herramientas `list_wiki`/`read_note`/`write_note` por fuente), `ask` y chat con la wiki como contexto (presupuesto de caracteres; los índices entran primero). El historial del chat vive en Rust (`ChatState`), es **solo de anexar** y congela el `system` al empezar: no editar ni podar mensajes anteriores (rompe los bloques de pensamiento preservados).
- `claude_code.rs`: PTY (`portable-pty`) que ejecuta `claude`. Busca la sesión llamada `Nexo` en `~/.claude/projects/*/*.jsonl` (última entrada `custom-title`) y la retoma con `--resume <id>` **desde su `cwd` original**; si no existe, crea una con `--name Nexo` en el vault.
- `connections.rs`: estado de MarkItDown (saludo MCP + `tools/list`), API (Models API, sin tokens) y Claude Code.

El streaming hacia la interfaz usa `tauri::ipc::Channel` (eventos `{kind, text}`), no eventos globales.

**Frontend (`src/`)**:

- `main.js`: estado y cableado de toda la UI (vault, fuentes, conexiones, terminal/comandos, pestañas). En la pestaña API, los comandos conocidos (`COMMANDS`) se ejecutan y cualquier otro texto va al chat de Nexo.
- `graph.js`: grafo en canvas con d3-force/d3-zoom. Las capas superiores usan `pointer-events: none` para que el ratón llegue al canvas salvo en paneles y controles; `getViewport()` calcula el hueco central entre paneles para centrar.
- `claude-code.js`: xterm.js conectado a los comandos `claude_code_*`.
- Solo `main.js` distingue escritorio/navegador con `isTauri()`; en el navegador las funciones de Tauri muestran un aviso.
