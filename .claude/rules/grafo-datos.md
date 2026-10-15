---
paths:
  - "src-tauri/src/graph.rs"
  - "crates/nexo-core/src/graph.rs"
  - "crates/nexo-core/tests/**"
  - "docs/spec-grafo.md"
---

# Grafo: construcción, casos dorados y vigilante de `wiki/`

El grafo se prueba con casos dorados: cada carpeta de `crates/nexo-core/tests/fixtures/grafo/` trae un `vault/` y su `expected.json`, y `graph::build_graph` tiene que dar exactamente eso (`cargo test -p nexo-core graph`). Las reglas (G1–G14) están en `docs/spec-grafo.md` y se cambian siguiendo su apartado «Cómo cambiar una regla»: primero la regla y el caso, con el `expected.json` escrito a mano, y después el código.

Nunca se regenera un `expected.json` con la salida del código ni se cambian las pruebas o los casos dados para que pasen: si uno falla, se dice cuál y qué da.

- `graph.rs`: comandos del grafo (`read_graph`, `note_view`, `open_note`) y el vigilante de `wiki/`. La construcción está en `nexo-core` (`graph::build_graph`): lee `wiki/` y construye nodos/aristas a partir de `[[enlaces]]` y `[texto](nota.md)`, resolviendo como Obsidian (por ruta o por nombre; con nombres repetidos gana la misma carpeta). El `title:` del frontmatter, si lo hay, es la etiqueta del nodo. Los enlaces rotos son nodos `missing`; las fichas son nodos `source` (con su `source`), las notas de `conversaciones/` son nodos `conversation`, y toda nota que contenga `raw/<ruta>` se une a la ficha de esa fuente.

- **El vigilante de `wiki/`** (`graph::watch`, cada segundo) emite `wiki-changed` con `added` y `changed`, llama a `notas::sync` si cambió algo en `notas/`, emite `desk-changed` si `.nexo/escritorio.json` cambió fuera de nexo, `skills-changed` (con las skills nuevas) si cambió `.claude/skills/` y `agents-changed` si cambió `.claude/agents/`.
