---
paths:
  - "src-tauri/src/{sources,fichas,mcp,markitdown,playwright,agy,browser}.rs"
  - "crates/nexo-core/src/fichas.rs"
---

# Fuentes: `raw/`, fichas, MarkItDown, Playwright y Antigravity

Al añadir fuentes (`main.js`): ficha inmediata → conversión MarkItDown → resumen en la cola de Antigravity. `raw/` y `wiki/fuentes/` están descritos en `backend.md`.

- `sources.rs`: listar/añadir (copia sin sobrescribir)/abrir/quitar (a la papelera)/renombrar fuentes de `raw/`. Rechaza PDF/Office/EPUB si no está `markitdown-mcp`. `list_sources` sincroniza las fichas (crea las que faltan, manda a la papelera las huérfanas). Renombrar arrastra la conversión, la ficha, el manifiesto y las citas `raw/…` de la wiki.
- `fichas.rs`: fichas de `wiki/fuentes/` (crear, leer para la pestaña de resumen, escribir resumen, renombrar, índice).
- `mcp.rs`: cliente MCP mínimo por stdio (JSON-RPC por líneas) compartido por MarkItDown y Playwright.
- `markitdown.rs`: `markitdown-mcp`, herramienta `convert_to_markdown`. Conversión local, sin IA.
- `playwright.rs`: `playwright-mcp --headless --isolated --browser chrome` (o, si hay navegador cargado y nadie lo tiene abierto, el perfil de `browser.rs` sin ventana): `browser_navigate` → `browser_wait_for` → `browser_evaluate` (extrae el HTML renderizado del contenido principal; el resultado viene en la sección `### Result` del texto) → MarkItDown convierte ese HTML a Markdown. Sin IA.
- `agy.rs`: Antigravity es el **encargado de las fuentes**. `add_web_source(url, playwright)`: sin Playwright, una sola llamada a agy (página → `raw/web/*.md` + resumen de la ficha); con Playwright, la página la lee `playwright.rs` y agy solo resume (sin permiso `read_url`). Ambas fases salen en la misma tarea de Actividad y `summarize_source` (copia el Markdown de la fuente como `fuente.md` en una carpeta vacía de caché y le pide leerlo con `view_file`). Ejecuta `agy "-p=<prompt>" --output-format stream-json --json-schema …`: el prompt va **como argumento** (con `-p=-` no le llega y se pone a explorar carpetas). Los trabajos se serializan (uno a la vez) y cada paso se emite como evento global `agy-activity` para el panel "Actividad Antigravity". Permisos: solo `read_url(<dominio>)` en `~/.gemini/antigravity-cli/settings.json`; nunca `read_url(*)`, `command(...)` ni `--dangerously-skip-permissions` (un permiso denegado aborta la tarea entera).
- `browser.rs`: botón **Cargar navegador** (en Conexiones). Copia el perfil del navegador predeterminado (`xdg-settings`; solo familia Chromium) a `<app_data>/navegador/perfil` sin cachés, extensiones ni sesiones de pestañas, lo abre con Playwright sin ventana para contar las cookies descifradas y solo entonces borra la copia anterior (si Chrome la tenía abierta, lo cierra). `navegador/playwright.json` quita `--password-store=basic`/`--use-mock-keychain` de los argumentos por defecto de Playwright y fija `--password-store=gnome-libsecret` fuera de KDE: sin eso las cookies `v11` (cifradas con el llavero) no se descifran. El perfil original nunca se modifica, y Chrome 136+ no permite automatizar el perfil por defecto, por eso se copia.
