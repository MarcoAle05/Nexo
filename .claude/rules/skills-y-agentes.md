---
paths:
  - "src-tauri/src/{skills,agents}.rs"
  - "src/{skills,agents}.js"
  - "tests/{agents-labels,skills-layout}.test.mjs"
---

# Skills y agentes del vault

`ORBIT` va a juego con `frameFor` de `galaxy.js` (ver `galaxia.md`): si cambia uno, revisar el otro.

- `.claude/skills/<nombre>/SKILL.md`: las **skills** del usuario, que son skills de Claude Code del vault (la sesión de nexo arranca en la raíz y las carga). nexo lee su frontmatter (`name`, `description` = resumen que ve el usuario, `when_to_use`) y las dibuja alrededor de la galaxia. Las escribe el agente `.claude/agents/creador-de-skills.md` (Sonnet 5.5, `effort: medium`), que nexo instala y mantiene mientras conserve su línea final `<!-- nexo: … -->` (si el usuario la borra, el archivo es suyo).
- `.claude/agents/*.md`: los **agentes** (subagentes de Claude Code) del vault; también los crea `creador-de-skills`, siempre con `model` y `effort` explícitos. nexo los lista en Agentes → Agentes con su `description` como resumen, el modelo y el esfuerzo.
- `skills.rs`: skills del vault (`skills_list`, `skills_remove` a la papelera, `skills_prepare`), su frontmatter (entiende comillas y bloques `|`/`>`), la huella que usa el vigilante y `install_agent`, que deja `creador-de-skills` en `.claude/agents/`; `claude_code_start` lo instala antes de arrancar Claude Code (lee los agentes al empezar la sesión) y el prompt del orquestador le manda delegar en él toda skill nueva.
- `agents.rs`: agentes de `.claude/agents/` (también en subcarpetas; sin `description` no cuentan): `agents_list` (nombre, resumen, `model`, `effort`, `tools`, si lo mantiene nexo) y `agents_remove` a la papelera, más su huella para el vigilante.
- `skills.js`: recuadros de las skills en órbita alrededor de la galaxia (`ORBIT`, `orbitLayout` los reparte sin pisarse; con más de `ORBIT.max` aparece «+N»), el resumen al pulsar uno (`#skill-pop`, solo nombre, descripción y `/comando`) y la carta Agentes → Skills (lista, «Agregar skill» y filas «Creando skill…» hasta que el vigilante avisa de una skill nueva).
- `agents.js`: pestañas Skills / Agentes de la carta Agentes (`nexo.agentsTab`) y lista de agentes con el modelo y el esfuerzo legibles (`modelLabel`, `effortLabel`, `toolsLabel` agrupa las herramientas MCP por servidor); «Agregar agente» pide a Claude Code, con el modelo y el esfuerzo elegidos, que `creador-de-skills` lo escriba.
