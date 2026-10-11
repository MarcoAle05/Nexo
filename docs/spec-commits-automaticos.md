# Especificación de los commits automáticos del vault

Objetivo: que lo que escribe la compilación de nexo quede en git, para poder revisarlo (`git show`) y
deshacerlo (`git revert`). Es la fuente de verdad; las pruebas de `crates/nexo-core/src/vaultgit.rs` fijan cada regla.

**Fuera de alcance (a propósito):** lo que escriben Claude Code, el editor de notas o Antigravity fuera de una
compilación; empujar a un remoto; crear el repositorio; cambiar la identidad o la configuración de git del usuario.

## Reglas del módulo `vaultgit`

### C1. Solo en un repositorio que ya existe, en la raíz del vault
Si `<vault>/.git` no existe no se hace nada, y **nunca** se ejecuta `git init`. Si el vault está dentro de otro
repositorio (el `.git` está en una carpeta superior) tampoco se hace nada: no se escribe en repositorios ajenos.
Pruebas: `sin_repositorio_no_hace_nada_ni_lo_crea`, `un_vault_dentro_de_otro_repositorio_se_deja_en_paz`.

### C2. Sin `git` instalado no es un fallo
Resultado `Skipped`, sin mensaje en la terminal. Prueba: `sin_git_instalado_no_es_un_fallo`.

### C3. Un commit por fuente, con exactamente las rutas indicadas
`commit(paths, mensaje)` guarda esas rutas y ninguna más. Lo demás —archivos modificados o nuevos, incluso dentro
de `wiki/`— sigue sin guardar. Lo que el usuario ya tenía en el área de preparación (`stage`) sigue ahí. Las rutas
repetidas cuentan una vez. Pruebas: `el_commit_lleva_solo_las_rutas_indicadas`, `no_toca_lo_que_el_usuario_ya_tenia_en_el_stage`.

### C4. Rutas válidas
Una ruta tiene que ser relativa al vault, sin `..`, sin raíz y fuera de `.git/`. Si alguna no lo es, no se
guarda nada y el resultado es `Failed`. Las rutas que no existen o que ignora `.gitignore` se omiten sin error.
Pruebas: `rechaza_rutas_que_salen_del_vault_o_entran_en_git`, `omite_las_rutas_ignoradas_y_las_que_no_existen`.

### C5. Nada que guardar, nada que crear
Sin cambios en esas rutas no se crea un commit vacío (`NothingToCommit`). Prueba: `sin_cambios_no_crea_commits_vacios`.

### C6. Foto previa
`snapshot()` guarda lo que haya sin guardar **dentro de `wiki/`** —nuevo, cambiado o borrado— con el mensaje
`nexo: antes de compilar`, y no toca nada fuera de `wiki/`. Pruebas: `la_foto_previa_guarda_solo_wiki`, `la_foto_previa_recoge_tambien_lo_borrado`.

### C7. Mensaje
`nexo: compila raw/<fuente>`, en una sola línea: los caracteres de control del nombre se cambian por espacios.
Prueba: `el_mensaje_va_en_una_sola_linea`.

### C8. Un fallo de git nunca tumba la compilación
Cualquier error de git (falta de identidad, hooks que fallan, índice bloqueado…) es `Failed("<primera línea del
error>")`; se muestra como `git: <error>` y la compilación sigue. Si un commit falla, el índice queda como
estaba (lo añadido para ese commit se quita). Prueba: `sin_identidad_falla_sin_romper_y_deja_el_indice_limpio`.

### C9. Nunca bloquea esperando al usuario
- No se piden credenciales (`GIT_TERMINAL_PROMPT=0`).
- Estos commits no se firman (`commit.gpgsign=false` solo para estas llamadas): una firma con contraseña dejaría la compilación esperando. Prueba: `una_firma_gpg_configurada_no_bloquea`.
- Plazo de 30 s por llamada. Si se pasa, **no se mata git** (podría dejar `.git/index.lock`): se avisa («git no respondió en N s; se deja terminar») y se sigue. Prueba: `si_git_tarda_demasiado_avisa_y_sigue`.

### C10. Nunca se empuja
No se ejecuta `push`, `fetch` ni `remote`. Un remoto roto no afecta. Prueba: `un_remoto_roto_no_afecta_porque_nunca_se_empuja`.

### C11. Lo que se cuenta en la terminal
`Committed(hash)` → `guardado en git <hash>`; `Failed(e)` → `git: <e>`; el resto no dice nada.
Prueba: `mensajes_para_la_terminal`.

## Reglas de integración en `compile` (no se pueden probar sin la API de Claude)

### I1. Qué rutas entran en el commit de una fuente
Las de `wiki/` que **la compilación de esa fuente** escribió con `write_note` más `.nexo/compiled.json`. Se recogen
en `run_tool` (ruta relativa al vault, con `/`, p. ej. `wiki/redes/tcp.md`). Las fichas de `wiki/fuentes/` no entran:
las mantiene nexo y `write_note` no puede escribirlas.

### I2. Cuándo
- Una vez, antes del bucle de fuentes y después de comprobar que hay algo que compilar: `snapshot()`.
- Por cada fuente que termina bien, justo después de escribir el manifiesto: `commit(rutas, source_message(fuente))`.
- Si una fuente falla o se omite, no hay commit. Lo que el modelo alcanzó a escribir queda sin guardar y la siguiente foto previa lo recoge.

### I3. Cómo
`compile` es `async` y git bloquea: cada llamada va en `tauri::async_runtime::spawn_blocking`. El resultado se emite
con `emit(&channel, "step", mensaje)` solo si `Outcome::message()` devuelve algo.

### I4. Interruptor
Encendido por defecto. Se apaga con `"auto_commit_off": true` en `config.json` (es una bandera negativa porque
`Config` deriva `Default`, que daría `false` a un campo positivo). Un botón en Conexiones es opcional y va aparte.

## Cómo cambiar una regla
Editar esta spec, cambiar o añadir la prueba en `vaultgit.rs` (un repositorio temporal real por prueba), verla fallar
por el motivo esperado, y luego cambiar el código. Un commit por regla.
