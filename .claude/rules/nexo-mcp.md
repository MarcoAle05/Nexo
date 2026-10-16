---
paths:
  - "crates/nexo-mcp/**"
  - "crates/nexo-core/src/query.rs"
  - "docs/spec-mcp.md"
---

# Servidor MCP (`crates/nexo-mcp`)

`crates/nexo-mcp` es un servidor MCP por stdio, **de solo lectura**, sobre un vault (`nexo-mcp --vault <ruta>` o `NEXO_VAULT`): seis herramientas (`list_notes`, `read_note`, `search_notes`, `get_graph`, `get_neighbors`, `list_sources`) que son una capa fina de `rmcp` sobre `nexo_core::query`. Ninguna escribe en el vault y solo leen dentro de `wiki/`. Las reglas (M1–M11) están en `docs/spec-mcp.md`; las pruebas (`crates/nexo-mcp/tests/protocol.rs`) lanzan el binario real contra `tests/vault-demo/` y los casos de `tests/cases/`. No se registra solo en ningún cliente.
