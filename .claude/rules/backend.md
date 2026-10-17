---
paths:
  - "src-tauri/src/**/*.rs"
  - "src-tauri/build.rs"
  - "crates/**/*.rs"
  - "Cargo.toml"
  - "src-tauri/Cargo.toml"
  - "crates/*/Cargo.toml"
  - ".cargo/config.toml"
  - "src-tauri/tauri.conf.json"
  - "src-tauri/capabilities/**"
---

# Backend: workspace, comandos de Tauri y el vault

**Tres crates en un workspace de Cargo** (raíz del repo). `crates/nexo-core` tiene la lógica del vault que no depende de Tauri: `graph` (`build_graph` y los casos dorados), `vaultgit`, la lectura de fichas (`fichas::all`…) y los guardianes de rutas (`vault::resolve_inside`, `wiki::wiki_path`). Solo usa `serde` y `serde_json` y **nunca** `tauri`, para poder construir otros binarios (un servidor MCP) sin WebKitGTK; el job `core` del CI lo comprueba. `src-tauri` (crate `app`) depende de él y reexporta lo movido, así que el resto del código lo llama igual (`crate::vault::resolve_inside`, `crate::fichas::all`…); los `#[tauri::command]` se quedan en `app`. La carpeta de compilación sigue en `src-tauri/target` (`.cargo/config.toml`): una `target/` en la raíz la vigilaría Vite.

`crates/nexo-mcp` (servidor MCP de solo lectura) tiene su regla, `nexo-mcp.md`.

**Backend (`src-tauri/src/`)**, todo expuesto como `#[tauri::command]` registrados en `lib.rs`:

- `vault.rs`: configuración, elección del vault, detección de vaults de Obsidian (lee `obsidian.json`) y creación de la estructura.

Cada módulo está descrito en la regla de su zona: `vault.rs` aquí; `graph.rs` en `grafo-datos.md`; `sources.rs`, `fichas.rs`, `mcp.rs`, `markitdown.rs`, `playwright.rs`, `agy.rs` y `browser.rs` en `fuentes.md`; `claude.rs`, `nexo.rs` y `connections.rs` en `api-nexo.md`; `vaultgit` en `vaultgit.md`; `claude_code.rs` en `claude-code.md`; `usage.rs` y `registro.rs` en `uso-y-registro.md`; `workspace.rs`, `workspace_run.rs` y `worktree.rs` en `workspace.md`; `notas.rs` en `notas.md`; `render.rs` en `render.md`; `skills.rs` y `agents.rs` en `skills-y-agentes.md`.

El streaming hacia la interfaz usa `tauri::ipc::Channel` (eventos `{kind, text}`; el progreso de `worktree_create`, `{step, text}`); la única excepción es `agy-activity`, un evento global porque el panel de actividad escucha todas las tareas de Antigravity.

Dependencias externas en tiempo de ejecución: `markitdown-mcp` (instalado con `uv tool install --python 3.12 markitdown-mcp`, en `~/.local/bin`), `playwright-mcp` (`npm install -g --prefix ~/.local @playwright/mcp`; usa el Google Chrome instalado), `claude` (Claude Code CLI), `agy` (Antigravity CLI) y `git` (los worktrees; `gh`, si está, da el usuario de GitHub para sus ramas). En Linux, `main.rs` llama a `render::configure()` antes de crear nada: elige cómo pinta WebKitGTK (ver `render.rs`); no quitarlo.

## Modelo de datos: el vault

Todo gira en torno a un vault de Obsidian elegido por el usuario (ruta guardada en `~/.config/com.nexo.app/config.json`, junto con la clave de API opcional; el archivo se escribe con permisos 0600):

- `raw/`: fuentes que vuelca el usuario. `raw/markdown/` guarda las conversiones de MarkItDown (misma ruta relativa, extensión `.md`) y **se oculta** del listado de fuentes. `raw/web/*.md` son páginas web leídas por Antigravity y se listan como tipo `web`.
- `wiki/`: notas organizadas por la IA. `_master-index.md` enlaza el `_index.md` de cada carpeta-tema; notas atómicas en kebab-case enlazadas con `[[...]]`.
- `wiki/fuentes/`: **una ficha por fuente** (frontmatter `source: raw/<ruta>`, resumen, ideas clave, `[[conceptos]]`), más `_index.md`. Las mantiene nexo (`fichas.rs`) y el resumen lo escribe Antigravity; `compile` tiene prohibido escribir ahí. Así cualquier fuente se encuentra por su nombre en Markdown.
- `output/`: respuestas (`ask` añade secciones a `output/query-results.md`).
- `wiki/conversaciones/`: conclusiones que Claude Code guarda al pulsar "Guardar en el grafo" (nodos `conversation`).
- `wiki/notas/`, `.nexo/escritorio.json` y `.nexo/escritorios.json`: el apartado Notas y sus escritorios (ver `notas.md`).
- `.nexo/compiled.json`: manifiesto de fuentes ya compiladas (tamaño:mtime) para no recompilar.
- `.claude/skills/<nombre>/SKILL.md` y `.claude/agents/*.md`: las skills y los agentes del vault (ver `skills-y-agentes.md`).

Los **workspaces** (vista Agentes) son otras carpetas, de proyecto, fuera del vault: su lista está en la configuración de nexo y lo de cada uno en su propio `.nexo/` (ver `workspace.md`). Sus rutas pasan por `workspace::ws_root` y `resolve_inside`.

Toda ruta que llega desde la interfaz o desde herramientas del modelo pasa por un guardián que impide salir del vault. `vault::resolve_inside` (fuentes y grafo) y `wiki::wiki_path` (herramientas de compile, cuyas rutas las escribe el modelo), los dos en `nexo-core`, resuelven además los enlaces simbólicos: uno dentro del vault que apunte fuera se rechaza. `notas::resolve` y `agents::agent_file` solo miran el texto de la ruta; sus rutas vienen de la interfaz. En un workspace, `ws_root` solo acepta carpetas de su lista y `resolve_inside` hace lo mismo que en el vault.
