# nexo-core

La lógica del vault de nexo que no necesita la app de escritorio: el grafo de la wiki (`graph`), los commits automáticos (`vaultgit`), la lectura de fichas (`fichas`) y los guardianes de rutas (`vault`, `wiki`).

No depende de Tauri a propósito (solo `serde` y `serde_json`): así se pueden construir otros binarios sobre el mismo vault, como un servidor MCP, sin WebKitGTK. El job `core` del CI lo comprueba en una máquina sin las librerías de Tauri.
