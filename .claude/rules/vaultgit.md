---
paths:
  - "crates/nexo-core/src/vaultgit.rs"
  - "src-tauri/src/nexo.rs"
  - "docs/spec-commits-automaticos.md"
---

# Commits automáticos del vault (`vaultgit`)

- `vaultgit` (en `nexo-core`): commits automáticos del vault. Si el vault es un repositorio git, `compile` guarda antes de empezar una foto de lo que hubiera sin guardar en `wiki/` (`nexo: antes de compilar`) y, por cada fuente, un commit con las notas que escribió más `.nexo/compiled.json` (`nexo: compila raw/<fuente>`). Llama a `git` como programa: nunca crea el repositorio, nunca empuja y un fallo de git solo deja una línea `git: …` en el chat, sin tumbar la compilación. Se apaga con `"auto_commit_off": true` en `config.json`. No cubre lo que escriben Claude Code, el editor de notas o Antigravity. Las reglas (C1–C11 e I1–I4) están en `docs/spec-commits-automaticos.md` y cada una tiene su prueba en el módulo.
