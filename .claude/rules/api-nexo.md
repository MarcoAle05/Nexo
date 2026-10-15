---
paths:
  - "src-tauri/src/{claude,nexo,connections}.rs"
---

# La API de Anthropic: cliente, asistente Nexo y Conexiones

Desde la interfaz se llega por las órdenes «/» del chat (`LOCAL_COMMANDS` y `runNexo` de `main.js`, ver `interfaz.md`).

- `claude.rs`: cliente HTTP de la Messages API (Rust no tiene SDK oficial). Modelo `claude-opus-5-5`, `fallbacks: "default"` con beta `server-side-fallback-2026-07-01`, SSE reconstruido en bloques completos (incluido el pensamiento con su firma). Comprobar siempre `refusal()` antes de usar el contenido.
- `nexo.rs`: el asistente Nexo. `compile` (primero convierte con MarkItDown y envía el Markdown; bucle de herramientas `list_wiki`/`read_note`/`write_note` por fuente), `ask` y chat con la wiki como contexto (presupuesto de caracteres; los índices entran primero). El chat por la API (`chat_send`, `chat_reset`) ya no tiene entrada en la interfaz: la carta derecha es el chat con Claude Code. Su historial vive en Rust (`ChatState`), es **solo de anexar** y congela el `system` al empezar: no editar ni podar mensajes anteriores (rompe los bloques de pensamiento preservados).
- `connections.rs`: estado de MarkItDown (saludo MCP + `tools/list`), API (Models API, sin tokens), Antigravity y Claude Code.
