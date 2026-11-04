# Especificación: los workspaces trabajan en terminales

Se quitan de los workspaces el chat propio, el orquestador y el repertorio, y se trabaja como en Orca: cada workspace abre terminales normales en su carpeta. En ellas corre lo que sea, también Claude Code con la configuración de siempre del usuario. El chat de Nexo (la carta derecha) no cambia, y para trabajar con los workspaces usa una skill nueva, `nexo-orquestacion`.

**Estado:** aprobada e implementada el 22-10-2026. Lo que cambió al implementarla está en «Notas de la implementación», al final. Las marcadas «(usuario)» las decidió el usuario ese día:
- el alcance (la petición de `/to-spec`);
- que las terminales se recuperen al reiniciar nexo (lo pidió al empezar a implementarla);
- las tres costuras de prueba;
- que el estado de Claude se sepa con hooks solo en las terminales de nexo;
- devolver al perfil global lo que hay en el repertorio;
- publicar la especificación en `docs/`.

## Problema

- **El chat de los workspaces copia a Claude Code y siempre va por detrás.** Lo que Claude Code añade (órdenes, vistas, cambios en su interfaz) llega tarde o no llega. Además, nexo tiene que interpretar su protocolo `stream-json`, que cambia entre versiones: la línea `result` con `type` al final, o el aviso de un agente que puede venir en tres sitios distintos.
- **El orquestador impone un papel.** Cada workspace arranca un agente con instrucciones que el usuario no eligió. No se puede abrir una terminal y lanzar `npm test`, `git` o `claude` a su manera.
- **El repertorio y Extras cambian la configuración de Claude Code en cada workspace.** Lo que hay en un workspace de nexo no es lo mismo que en una terminal normal o en Orca. Y los plugins que se sacaron del perfil ya no cargan fuera de nexo. El usuario quiere su configuración normal.
- **El chat de Nexo no puede repartir trabajo entre los workspaces.** Hoy solo sabe gestionar el repertorio.

## Solución

- **Abrir un workspace muestra una terminal vacía** en su carpeta, con los colores del tema actual.
- **Arriba de las ventanas hay un «+»** que abre un menú pequeño con dos opciones:
  - «Terminal»: abre una terminal vacía;
  - «Claude Code»: abre una terminal que ejecuta `claude`.
- **Las terminales son normales:** el shell del usuario, con su configuración y lo que quiera ejecutar.
- **Claude Code corre con la configuración normal del usuario**, sin repertorio, sin Extras y sin papel de orquestador.
- **nexo sigue sabiendo qué hace cada Claude:**
  - si trabaja, si espera una respuesta o si está libre;
  - lo enseña con el punto de las pestañas y con avisos de escritorio;
  - lo que gasta se ve en el registro de agentes.
- **El repertorio desaparece** y lo que tenía vuelve al perfil global.
- **El chat de Nexo sigue igual.** Con la skill `nexo-orquestacion`, que el usuario también puede lanzar con `/nexo-orquestacion`, su agente puede:
  - ver los workspaces;
  - abrir terminales;
  - lanzar agentes de Claude Code con un encargo, también como uno de los agentes del vault;
  - leer lo que hicieron, saber quién trabaja y cuánto gasta.

## Historias de usuario

**Terminales**

1. Como usuario de nexo, quiero que al abrir un workspace se abra una terminal vacía en su carpeta, para empezar a trabajar sin preparar nada.
2. Como usuario, quiero que la terminal use mi shell con mi configuración (alias, `PATH`, prompt), para trabajar igual que en cualquier terminal.
3. Como usuario, quiero ejecutar cualquier programa (tests, git, servidores, `claude`), para no depender de lo que nexo sepa hacer.
4. Como usuario, quiero que una terminal siga viva al cambiar a otro workspace, a las notas o al grafo, para que no se corte lo que corre.
5. Como usuario, quiero que al volver a un workspace sus terminales estén como las dejé, con lo que salió mientras no miraba, para no perder nada.
6. Como usuario, quiero que una terminal no se reabra sola si ya hay alguna viva al volver al workspace, para no acumular terminales vacías.
7. Como usuario, quiero cerrar una terminal con su «×», para quitar lo que ya no uso.
8. Como usuario, quiero que nexo me pregunte antes de cerrar una terminal con algo en marcha, para no matar un trabajo sin querer.
9. Como usuario, quiero que, si el programa de una terminal termina, la ventana diga cómo acabó y siga ahí hasta que la cierre, para leer lo que salió.
10. Como usuario, quiero copiar con Ctrl+Mayús+C, pegar con Ctrl+Mayús+V y seleccionar con el ratón, para moverme como en cualquier terminal.
11. Como usuario, quiero que los enlaces de la terminal se abran en el navegador, para ir rápido a lo que imprime un servidor o un test.
12. Como usuario, quiero que la terminal se adapte al tamaño de su ventana (mosaico, sola, carta más ancha), para que los programas a pantalla completa se vean bien.
13. Como usuario, quiero hasta 10 000 líneas de historial por terminal, para revisar lo que salió hace rato.
14. Como usuario, quiero que, si recargo la interfaz mientras desarrollo nexo, las terminales vuelvan con lo que tenían, porque el backend sigue vivo.
15. Como usuario, quiero que al cerrar nexo se terminen sus terminales, para no dejar procesos huérfanos.
15b. Como usuario, quiero que al volver a abrir nexo cada workspace recupere sus terminales (mismo nombre, mismo orden, lo que habían escrito y, si tenían Claude, su sesión retomada), para seguir donde lo dejé.

**El «+» y las ventanas**

16. Como usuario, quiero un «+» al final de la barra de pestañas de las ventanas, para abrir una ventana nueva desde donde las veo.
17. Como usuario, quiero que el «+» abra un menú pequeño con «Terminal» y «Claude Code», para elegir qué abrir.
18. Como usuario, quiero que «Claude Code» abra una terminal que ya ejecuta `claude`, para no escribirlo yo.
19. Como usuario, quiero que la ventana nueva entre en el mosaico si cabe y, si no, quede en su pestaña, igual que pasaba con los agentes, para no perder de vista las demás.
20. Como usuario, quiero que la ventana nueva reciba el foco, para escribir enseguida.
21. Como usuario de teclado, quiero abrir el menú del «+» con Intro o Espacio, recorrerlo con las flechas y cerrarlo con Escape volviendo al «+», para no depender del ratón.
22. Como usuario de teclado, quiero una tecla que saque el foco de la terminal (F6 y Mayús+F6 pasan de una zona a otra), porque la terminal se queda con Tab y Escape.
23. Como usuario, quiero que las pestañas de las terminales se llamen «Terminal», «Terminal 2»…, y las de Claude, «Claude Code» o el nombre que les dio quien las abrió, para distinguirlas.
24. Como usuario, quiero seguir abriendo archivos del explorador en ventanas junto a las terminales, para leer código mientras trabajo.
25. Como usuario, quiero que la ventana del Setup de un worktree siga saliendo como hasta ahora, para ver cómo va.

**Tema**

26. Como usuario, quiero que el fondo, el texto, el cursor y la selección de la terminal vayan con el tema que tengo puesto, para que la terminal sea parte de nexo.
27. Como usuario, quiero que los colores ANSI de cada tema se lean bien sobre su fondo, también en los temas claros, para que la salida de colores no desaparezca.
28. Como usuario, quiero que al cambiar de tema las terminales abiertas cambien al momento, sin cerrarlas, para no perder lo que tienen.
29. Como usuario, quiero que la terminal use la misma letra monoespaciada que el código de nexo (Geist Mono), para que todo sea coherente.

**Claude Code en una terminal**

30. Como usuario, quiero que el Claude de una terminal cargue mi configuración normal (plugins, MCP, skills, hooks, CLAUDE.md), para tener lo mismo que fuera de nexo.
31. Como usuario, quiero que el Claude que escribo a mano en una terminal de nexo también le diga a nexo su estado, para que el punto y los avisos funcionen igual.
32. Como usuario, quiero que la pestaña de una terminal con Claude lleve su punto de estado (Activo, Esperando respuesta, Inactivo), para saber de un vistazo cuál me necesita.
33. Como usuario, quiero ver en la pestaña Agentes de la carta cada Claude que corre en las terminales, con sus subagentes y el estado de cada uno, para saber quién trabaja en el proyecto.
34. Como usuario, quiero que un subagente que falló, se detuvo o se cortó lo diga (Falló, Detenido, Sin terminar), igual que se decidió el 20-10-2026, para no confundirlo con uno que terminó bien.
35. Como usuario, quiero pulsar un Claude o un subagente en Agentes y que se enfoque su terminal, para ir donde trabaja.
36. Como usuario, quiero ver los tokens y el coste de cada Claude y subagente del workspace, para controlar el gasto.
37. Como usuario, quiero que nada de esto cambie mi `~/.claude`, para que fuera de nexo todo siga igual.

**Avisos y atención**

38. Como usuario, quiero un aviso de escritorio cuando un Claude de un workspace pide permiso, me pregunta algo o me propone un plan, para no dejarlo esperando.
39. Como usuario, quiero un aviso cuando un Claude termina su trabajo, para volver cuando haga falta.
40. Como usuario, quiero un aviso si una terminal se cierra con trabajo a medias sin que yo la cerrara, para saber que algo se cortó.
41. Como usuario, quiero que no me avise de lo que ya estoy mirando, como hasta ahora, para que los avisos no molesten.
42. Como usuario, quiero que el punto de «Espera tu respuesta» siga en las listas de workspaces y en el botón Agentes, para ver desde cualquier sitio qué workspace me necesita.
43. Como usuario, quiero que los avisos nunca lleven lo que la herramienta iba a hacer ni el texto de una pregunta, como hasta ahora, para que no se filtre nada al escritorio.

**El chat de Nexo y `nexo-orquestacion`**

44. Como usuario, quiero que el chat de Nexo siga siendo el de siempre, con su diseño, para no perder lo que funciona.
45. Como usuario, quiero tener una skill `nexo-orquestacion` que pueda lanzar con `/nexo-orquestacion`, para pedir trabajo sobre mis workspaces desde el chat de Nexo.
46. Como agente de Nexo, quiero ver la lista de workspaces del panel y sus terminales, para saber dónde puedo trabajar.
47. Como agente de Nexo, quiero abrir una terminal en un workspace y ejecutar una orden, para hacer tareas sencillas en el proyecto.
48. Como agente de Nexo, quiero lanzar un Claude Code en una terminal de un workspace con un encargo, para delegar trabajo.
49. Como agente de Nexo, quiero lanzarlo como uno de los agentes del vault, con su modelo y su esfuerzo si hace falta, para usar la configuración de nexo que el usuario preparó.
50. Como usuario, quiero que lo que lance el agente de Nexo se vea en las ventanas del workspace como cualquier terminal, para seguirlo y meterme si quiero.
51. Como agente de Nexo, quiero leer lo último que dijo un Claude o lo que muestra una terminal, para contarle al usuario cómo va.
52. Como agente de Nexo, quiero esperar a que un Claude termine o me necesite, para encadenar trabajos sin estar preguntando cada rato.
53. Como agente de Nexo, quiero escribir en una terminal, para seguir una conversación con un Claude que lancé o responder a un programa.
54. Como usuario, quiero que el agente de Nexo no pueda contestar los permisos ni las preguntas de otro Claude, para que esas decisiones sean siempre mías.
55. Como agente de Nexo, quiero saber qué Claude y qué subagentes trabajan y cuánto gasta cada uno, para informar al usuario.
56. Como usuario, quiero que el agente de Nexo me pregunte antes de cerrar una terminal que no abrió él, para no perder trabajo mío.
57. Como usuario, quiero que la orden de la skill no abra nexo si está cerrado y diga que no está abierto, para que nada se lance a escondidas.
58. Como usuario, quiero que la skill solo pueda trabajar en carpetas de mis workspaces, para que un agente no salga de ellas.

**Repertorio y Extras**

59. Como usuario, quiero que desaparezcan el repertorio y la pestaña Extras, porque Claude Code ya usa mi configuración normal.
60. Como usuario, quiero que los plugins, los MCP y las skills que mandé al repertorio vuelvan a mi perfil global, para tenerlos en todas partes como antes.
61. Como usuario, quiero que un worktree nuevo siga funcionando igual (con su formulario, su setup y su carpeta), aunque ya no herede los Extras.

## Decisiones de implementación

### 1. Qué se quita

- **El orquestador de los workspaces** (módulo `workspace_run`):
  - el proceso `claude -p` de cada workspace, sus instrucciones de papel y el plugin con la skill `agentes-del-workspace`;
  - retomar su sesión y rehacer el historial;
  - pasar mensajes a los agentes y el registro automático en `.nexo/agentes/`;
  - las órdenes de Tauri `workspace_start`, `workspace_send`, `workspace_stop`, `workspace_reset`, `workspace_detach` y `workspace_running`.
- **En la interfaz:**
  - la ventana del orquestador y las de chat de los agentes;
  - el reparto de líneas por agente (`ws-stream`) y sus notas de final;
  - el pie «Reiniciar espacio de Workspace»;
  - la pestaña Extras.
- **Extras y repertorio** (módulo `workspace`):
  - la sonda del catálogo, los ajustes por sesión (`extras_settings`) y el grupo Hooks;
  - la carpeta del repertorio y «Mover al repertorio»;
  - el vigilante del repertorio y de los `extras`;
  - las órdenes `workspace_extras`, `workspace_set_extras`, `workspace_repertoire_open` y `workspace_repertoire_move`.
  - La clave `extras` de `workspace.json` se ignora al leer: no se migra ni se borra de las carpetas del usuario.
- **En el chat de Nexo:** el plugin propio con la skill `nexo:repertorio` (y su `--plugin-dir`) y la frase de su papel que la menciona.
- **Las pruebas que solo cubren lo que se quita se borran con su código.** No se cambia ninguna para que pase.
- **Se queda:**
  - la lista y el panel de workspaces, el explorador y el lector de archivos;
  - los worktrees (el nuevo ya no hereda Extras) y la ventana del Setup;
  - `ws-attention`, las formas y textos del punto de estado y el registro de agentes.

### 2. Gestor de terminales (backend, módulo nuevo)

- **Qué ofrece:**
  - abrir una terminal en un workspace, opcionalmente con una orden y un nombre, y devolver su id;
  - escribir bytes en ella;
  - cambiar su tamaño (columnas y filas);
  - cerrarla;
  - listar las de un workspace;
  - enviar su salida por un `Channel`, repitiéndola entera a una interfaz que se vuelve a conectar.
- **Cada terminal es un PTY** (`portable-pty`, que nexo ya usa para `agy`) en la carpeta del workspace. La ruta pasa por el guardián `ws_root`.
- **Shell:** el del usuario (`$SHELL`, o el de su entrada en el sistema; si no hay ninguno, `/bin/bash`), en modo interactivo.
- **Entorno:**
  - `TERM=xterm-256color`, `COLORTERM=truecolor`, `NEXO_TERMINAL=<id>` y `NEXO_WORKSPACE=<carpeta>`;
  - se quitan las mismas variables heredadas que en el chat (`INHERITED_ENV`), para que Claude Code guarde sus transcripciones aunque nexo se abriera desde una terminal de Claude Code.
- **Salida:** un hilo lector reenvía los trozos de salida y guarda dos cosas por terminal:
  - un búfer circular de la salida en bruto (alrededor de 1 MB), para repetírsela a una interfaz recargada;
  - un modelo de pantalla sin ventana (un emulador VT en Rust, como el crate `vt100`), para leer la pantalla de un shell desde la orden de control.
- **Vida:**
  - las terminales viven mientras nexo esté abierto, también al cambiar de workspace;
  - al cerrar nexo se manda SIGHUP a su grupo de procesos y, tras una espera corta, SIGKILL;
  - si el proceso termina, la ventana dice «El proceso terminó (código N)».
- **Recuperar al reiniciar (usuario):** cada workspace guarda en la carpeta de datos de nexo (fuera de la carpeta del proyecto, archivos 0600: una terminal puede haber enseñado una clave) la lista de sus terminales (nombre, orden, si era de Claude y su sesión) y lo último de su salida (el búfer circular). Se guarda cada pocos segundos si algo cambió y al cerrar nexo. Al abrir el workspace sin terminales vivas, cada una vuelve con su nombre y su orden: primero su salida de antes, una línea tenue «— nexo se cerró · ‹fecha› —» y un shell nuevo en la carpeta; la de Claude con sesión conocida arranca `claude --resume <sesión>` (retomar no gasta nada hasta que se le escribe). Cerrar una terminal con su «×» la quita también de lo guardado.
- **Topes:** 12 terminales por workspace y 32 en total. Pasado eso, abrir otra da un error claro, también a la skill.

### 3. El `claude` de nexo y sus hooks (usuario)

- **El lanzador.** En la carpeta de datos de nexo hay un lanzador `claude` que busca el `claude` de verdad en el `PATH` (saltándose su propia carpeta) y lo ejecuta añadiendo hooks **solo a esa sesión**, con `--settings`.
  - El `~/.claude` del usuario no se toca y sus propios hooks siguen corriendo: Claude Code suma los de `--settings` a los suyos.
- **Ponerlo delante después de la configuración del usuario.** Su `.bashrc` pone `~/.local/bin` delante del `PATH`, y ahí está el `claude` de verdad (2.1.308 el 22-10-2026). Un `PATH` puesto antes de arrancar el shell quedaría tapado. Por eso:
  - **bash** arranca con un `--rcfile` de nexo que carga el `.bashrc` del usuario y luego pone delante la carpeta del lanzador;
  - **zsh** hace lo mismo con `ZDOTDIR`, y **fish** con `--init-command`;
  - con otro shell solo se antepone al arrancar. Si algo lo tapa, el estado sale solo de las transcripciones (ver abajo).
- **Qué eventos se escuchan:** inicio y fin de sesión, prompt del usuario, petición de permiso o notificación, fin de turno, e inicio y fin de subagente. Los nombres exactos se comprueban contra el Claude Code instalado al implementarlo.
- **Qué hace cada hook:** ejecuta `nexo ctl hook`, que lee el JSON del hook por la entrada y manda a nexo:
  - la terminal (`NEXO_TERMINAL`), el evento, el id de sesión y la ruta de la transcripción;
  - el nombre de la herramienta, si pide permiso;
  - el id del agente, si es de un subagente.

  **Nunca** manda la entrada de la herramienta ni el texto del prompt.
- **Un hook nunca molesta a Claude:** sale siempre con 0, sin escribir nada, con un tope de alrededor de 1 s. Si nexo no contesta, no hace nada.
- **Si el perfil o la carpeta tienen `disableAllHooks`**, no llega ningún evento. El estado sale entonces de las transcripciones: sabe si trabaja o terminó, pero no «Esperando respuesta». La pestaña Agentes lo dice.
- **Sesiones y terminales:** el evento trae la terminal y el id de sesión. Una terminal puede tener varias sesiones de Claude seguidas (salir y volver a lanzar); la suya es la última que dio señales.

### 4. Estado de cada Claude y de sus subagentes

- **Una sesión de Claude es una conversación.** Sus estados son:
  - **Activo**: desde el prompt hasta el fin de turno;
  - **Esperando respuesta**: un permiso o una pregunta pendientes, hasta el siguiente evento de herramienta, prompt o fin de turno;
  - **Inactivo**: libre.

  Al salir de Claude deja de contar como «trabajando» y sigue en el registro.
- **Los subagentes** están en Activo mientras corren. Su final sale de la transcripción, con la misma lectura que ya rehace el historial (`endings`: el último `<task-notification>` de su tarea o el resultado de su llamada). Quedan como Terminado, Falló, Detenido o Sin terminar, con los textos y las formas del punto del 20-10-2026.
- **La atención de cada workspace** junta la de sus terminales y sale con el mismo evento `workspace-attention { path, waiting, busy }` y las mismas órdenes `workspace_attention` y `workspace_watch`. Así `ws-attention`, el punto de las listas y el del botón Agentes no cambian.
- **Avisos de escritorio:** las reglas y los textos de `spec-avisos-workspaces.md`, ahora con los hooks como fuente:
  - «‹quién› pide permiso para usar ‹herramienta›», «… te hace una pregunta» (`AskUserQuestion`) y «… te propone un plan» (`ExitPlanMode`). ‹quién› es el nombre de la sesión o del subagente.
  - «Terminó el trabajo», al acabar un turno que estaba activo.
  - «Se cerró con trabajo a medias», si el proceso de la terminal termina estando activo o esperando y no lo cerró nexo.
  - Sin aviso si la ventana tiene el foco y ese workspace está en pantalla.
  - «Terminó con un error» solo si el evento lo dice. No se deduce.

### 5. Interfaz del workspace

- **La carta** tiene dos pestañas, Archivos y Agentes:
  - Agentes enseña una fila por cada Claude de las terminales (nombre, punto de estado, tokens y coste del registro) y, dentro, sus subagentes;
  - pulsar una fila enfoca su terminal.
  - Desaparecen la fila del orquestador («Principal») y la de Nexo.
- **La barra de pestañas** tiene terminales, archivos y Setup.
  - Al final, antes de «Todas» y del interruptor Mosaico, va el «+»: 36 px, el mismo de «Añadir fuente» y de los worktrees, con la etiqueta «Nueva ventana» y `aria-haspopup="menu"`.
  - Abre un menú con «Terminal» y «Claude Code», con el patrón de menú de la APG de los selectores del chat: flechas, Inicio y Fin, y Escape devuelve el foco al «+».
- **La ventana nueva** entra en el mosaico si cabe (`fits` de `ws-layout`) y, si no, queda en su pestaña. Recibe el foco.
- **La ventana de una terminal:**
  - xterm.js con su complemento de ajuste al tamaño;
  - Geist Mono a 13 px, 10 000 líneas de historial;
  - enlaces `http(s)` que se abren con el plugin `opener`;
  - Ctrl+Mayús+C y Ctrl+Mayús+V para copiar y pegar;
  - cabecera con el nombre, el punto (si es de Claude), minimizar y «×»;
  - el «×» pregunta «Cerrar y terminar ‹programa›» si hay un programa en marcha además del shell, o si su Claude está activo o esperando.
- **F6 y Mayús+F6** pasan el foco de una zona a otra (terminal, barra de pestañas, carta). Es la salida de teclado, porque la terminal se queda con Tab y Escape.
- **Dibujo:**
  - el complemento WebGL de xterm solo en el modo `hibrido` de `render.rs`; en `sin-webgl` y `cpu`, el dibujo por DOM;
  - el tamaño se ajusta al acabar cada transición de ancho y al cambiar el mosaico, no en cada fotograma (`rendimiento.md`), y se manda al PTY.
- **Al abrir un workspace sin ninguna terminal viva** se recuperan las que tenía la última vez (usuario); si no tenía ninguna, se abre una vacía con el foco. Se recupera también lo de `ui` (archivos abiertos, modo, carpetas).
- **Tema:**
  - cada tema de `themes.js` lleva su paleta de terminal: fondo (el tono de las ventanas de ese tema), texto, cursor, selección y los 16 colores ANSI;
  - se mide el contraste de cada color sobre su fondo, con la lista de comprobación de `better-colors`, y un color normal que se use para texto no baja de 4,5:1;
  - al cambiar de tema se actualizan las terminales abiertas.
- **Anuncios** en la región de estado que ya existe: «‹nombre› espera tu respuesta» y «‹nombre› terminó».
- **Nombre accesible** de la terminal: «Terminal ‹nombre› de ‹workspace›».

### 6. Orden de control y skill `nexo-orquestacion`

- **Servidor de control dentro de nexo:**
  - un socket Unix en `$XDG_RUNTIME_DIR/nexo/` (carpeta 0700, socket 0600);
  - una petición JSON por línea y una respuesta JSON por línea;
  - lo que hace cada petición es una función sobre el estado de la app (gestor de terminales, workspaces, registro), separada del socket. Es la costura de pruebas.
- **El cliente** son subórdenes del propio ejecutable, `nexo ctl …`, así no hay que instalar otro programa. `main` mira los argumentos antes de crear la ventana. Si nexo no está abierto, dice «nexo no está abierto», sale con código 2 y no lo abre.
- **El contrato** (salida en JSON):
  - `workspaces`: los del panel, con su nombre, su ruta, si es el actual, sus terminales y su atención.
  - `terminales [workspace]`: id, workspace, nombre, qué corre, y su sesión de Claude con el estado.
  - `abrir-terminal <workspace> [--nombre N] [--orden "…"]`: abre una ventana de terminal y devuelve su id. Si el workspace no está en pantalla, la terminal corre y se ve al entrar.
  - `claude <workspace> [--agente A] [--modelo M] [--esfuerzo E] [--permisos modo] [--nombre N] "<encargo>"`:
    - abre una terminal con Claude Code interactivo y el encargo como primer mensaje;
    - la sesión se llama «N · nexo» y tiene un id fijado con `--session-id`;
    - `--agente` usa un agente del vault: su definición va con `--agents` y se elige con `--agent`;
    - **nunca** `bypassPermissions` ni `dontAsk`;
    - devuelve el id de la terminal y el de la sesión.
  - `escribir <terminal> "<texto>" [--enter]`: escribe en la terminal. **Se niega** mientras el Claude de esa terminal espera un permiso o una respuesta: eso lo contesta solo el usuario.
  - `leer <terminal> [--lineas N]`:
    - en una terminal con Claude, los últimos mensajes de su sesión sacados de la transcripción (solo texto, sin razonamiento);
    - en un shell, la pantalla y las últimas N líneas del modelo de pantalla, sin códigos de escape.
  - `esperar <terminal> [--hasta inactivo|espera|fin] [--max segundos]`: espera a que pase eso, como mucho 600 s si no se dice otra cosa.
  - `estado [workspace]`: las sesiones de Claude y sus subagentes, con su estado.
  - `gasto [workspace]`: del registro de agentes, los tokens, el coste y el % de la sesión de 5 h de cada uno.
  - `cerrar <terminal>`: la cierra.
  - `hook`: interna, la usan los hooks del lanzador.
- **Guardias:**
  - el workspace se resuelve por nombre o ruta contra la lista del panel (`ws_root`), y nunca se sale de ella;
  - nada de lo que devuelve lleva claves ni tokens de autenticación.
- **La skill:**
  - **Cuándo usarla:** delegar trabajo a un proyecto, ver cómo va o ver cuánto se gasta.
  - **Cómo:** las órdenes de arriba con ejemplos y la ruta real del ejecutable.
  - **Reglas:**
    - si un agente del vault encaja con el encargo, se usa (como ya hace el papel del chat de Nexo);
    - decir al usuario qué lanzó y dónde verlo;
    - no contestar nunca permisos ni preguntas de otro Claude;
    - preguntar antes de cerrar una terminal que no abrió la propia skill;
    - dar el gasto cuando se pida.
- **Dónde vive:**
  - nexo la escribe al arrancar en las skills de proyecto del vault (`.claude/skills/nexo-orquestacion/`), que es el proyecto del chat de Nexo. Solo la reescribe si falta o es de una versión anterior (lleva su versión).
  - Sale en la órbita de la galaxia como cualquier skill del vault, y el usuario la lanza con `/nexo-orquestacion`.
  - El papel del chat de Nexo cambia la frase del repertorio por: «para trabajar con los workspaces, sigue la skill nexo-orquestacion».

### 7. Registro de agentes y gasto

- **El registro** sigue leyendo las transcripciones de las carpetas de los workspaces.
  - Sin orquestador, cada sesión de Claude de un workspace es un agente principal: con el nombre de `--name` si la lanzó nexo, o «Claude Code» si no. Sus subagentes van debajo. `places` no cambia.
- **`agent_context`** sondea las sesiones de un workspace con la configuración normal, sin el arranque del orquestador.
- **Se cuenta por carpeta:** una sesión lanzada en la carpeta de un workspace desde fuera de nexo (otra terminal, Orca) también cuenta como de nexo. Se acepta.

### 8. Devolver el repertorio al perfil (usuario)

Se hace **una vez, a mano**, al implementar, con el permiso del usuario en ese momento. No es código de la app. Cada paso se comprueba en los archivos:

1. Volver a encender `interfaces@interfaces`, `agent-skills@addy-agent-skills` y `cloudflare@cloudflare` en el perfil del usuario.
2. Devolver los MCP `github`, `nexo` y `rea` del `.mcp.json` del repertorio al perfil del usuario, sin enseñar sus valores.
3. Volver a mover `reverse-engineer-anything` a `~/.claude/skills/`. Si era un enlace, se mueve el enlace.
4. Quitar de `skillOverrides` los `"off"` que puso nexo.
5. Borrar `~/nexo/repertorio/` y las carpetas de datos de nexo con los plugins y los `mcp.json` del repertorio.
6. Actualizar la memoria del perfil y el repertorio.

### 9. Reglas y documentación

- **Reglas que se reescriben o cambian:**
  - `workspace.md`, con terminales, control, hooks y la nueva carta;
  - `claude-code.md`: el chat de Nexo ya no lleva plugin propio y entra `nexo-orquestacion`;
  - `uso-y-registro.md`: no hay orquestador;
  - `skills-y-agentes.md`: `list_in`, si deja de usarse;
  - `estilos.md`: la paleta de terminal de los temas.
- **Otros documentos:** la fila de workspaces de la tabla de `CLAUDE.md`, la sección Workspaces del `README` y `spec-avisos-workspaces.md` (los sucesos ahora salen de los hooks).
- Los archivos de código nuevos entran en el `paths` de `workspace.md`.

## Decisiones de pruebas

**Qué hace buena una prueba aquí:**
- prueba lo que se ve desde la costura, no cómo está hecho por dentro;
- usa un PTY y procesos reales antes que simulacros;
- los fixtures se graban de verdad, sin rutas personales, y nunca se rehacen con la salida del código;
- lo que falle se dice; no se cambia una prueba para que pase.

**Costura 1: el gestor de terminales en Rust (usuario).** Se prueba con un PTY real y `sh` en una carpeta temporal:
- abrir, escribir `echo` y leer la salida;
- cambiar el tamaño y comprobarlo con `stty size`;
- al cerrar, un `sleep` hijo desaparece con su grupo;
- una interfaz que se reconecta recibe la salida anterior;
- en el entorno no están las `INHERITED_ENV` y sí `NEXO_TERMINAL`;
- con un `HOME` temporal cuyo `.bashrc` antepone otra carpeta, el lanzador de nexo sigue el primero.

Precedentes: el PTY de `agy` en `usage` y las pruebas con carpetas temporales de `workspace_run`.

**Costura 2: la orden de control (usuario).** Cada orden se prueba contra un estado de prueba:
- un workspace desconocido o una ruta fuera del panel se rechazan;
- `escribir` se niega mientras se espera un permiso;
- `claude --agente` monta `--agents` y `--agent` y nunca los modos prohibidos;
- los eventos de los hooks cambian el estado y emiten `workspace-attention` una sola vez por cambio. Precedentes: las pruebas de `check` y `told`, y las de `se_avisa` en `avisos`.
- `leer` de una sesión sale de una transcripción grabada. Precedente: la prueba de `endings`.
- `gasto` filtra el registro por workspace. Precedente: las pruebas de `registro`.

Los eventos de hook se graban en un fixture con un turno real de Haiku (esfuerzo bajo, `--no-session-persistence`, carpeta temporal), sin rutas personales.

**Costura 3: los módulos de interfaz sin DOM (usuario):**
- `ws-layout`: el «+» añade ventanas de terminal, ajusta el mosaico o minimiza la de uso más antiguo, y las terminales se pueden cerrar;
- temas: cada tema tiene la paleta de terminal completa y cumple los contrastes.

Precedentes: `tests/themes.test.mjs` y las pruebas de `ws-layout`.

**Se borran** las pruebas de `ws-stream` y las de Rust del orquestador, los Extras y el repertorio, junto con su código.

**A mano:** en el banco (Vite con Tauri simulado y un PTY de prueba) se comprueban el menú del «+», la ventana de terminal y el cambio de tema. El uso real en nexo lo prueba el usuario.

## Fuera del alcance

- «Retomar» una sesión anterior de Claude desde la interfaz. Se puede hacer en la terminal con `claude --resume`.
- Dividir una ventana en paneles, como los *splits* de Orca.
- Ventanas de chat con el diseño de nexo para las sesiones de las terminales.
- Un ajuste para el modo de lector de pantalla de xterm.
- Terminales remotas o por SSH.
- Cambios en el chat de Nexo aparte de su skill y la frase de su papel.
- Instalar plugins o skills desde nexo.

## Notas

- **El lanzador va después del `.bashrc`** porque la configuración del usuario antepone `~/.local/bin`, donde está `claude`.
- **Orca** consigue lo mismo con hooks globales en `~/.claude/settings.json` que solo actúan con sus variables `ORCA_*`; se ven en el perfil del usuario. nexo no toca el perfil.
- **Riesgo: los hooks cambian entre versiones de Claude Code**, tanto los nombres de los eventos como sus datos. Se comprueban contra la versión instalada, se graban y los desconocidos se ignoran.
- **Riesgo de rendimiento: xterm en WebKitGTK** con cuatro terminales a la vista escribiendo mucho. Se mide en el banco (`rendimiento.md`) antes de dar el cambio por bueno.
- **En las carpetas de los workspaces** quedan `.nexo/agentes/` y `session`/`extras` en `workspace.json`. Se ignoran: nexo no borra archivos de las carpetas del usuario.
- **«Orquestador» desaparece de los workspaces.** El chat de Nexo conserva su propio papel de orquestador de las skills y los agentes del vault, que es otra cosa.

## Notas de la implementación (22-10-2026)

- **Comprobado con Claude Code 2.1.308** y turnos reales de Haiku, sin nexo (el lanzador y los
  hooks del código, un socket falso y `claude` en un PTY): `SessionStart` (con `source`, la sesión
  y la transcripción), `UserPromptSubmit`, `PermissionRequest` (con la herramienta), `Stop` y
  `SessionEnd` (con su motivo) llegan sin el texto del prompt. **Un turno interrumpido (Escape,
  también al rechazar un permiso) no dispara `Stop` ni `PermissionDenied`**: la sesión se queda
  «activa» o «esperando» hasta que el barrido de cada 5 s ve en su transcripción el
  `system`/`turn_duration` del fin del turno (posterior a su principio); entonces deja de estarlo,
  sin aviso. Con el modo automático del usuario no hay permisos que pedir.
- **La paleta de la terminal** no va en `themes.js`: sale de los tokens de la ventana en
  `styles.css` (los del código del lector más `--term-red` y `--term-cyan`, ver `estilos.md`). Así
  queda donde están los demás colores de la interfaz y la misma prueba de contraste la cubre.
- **`nexo ctl hook`** tarda unos 37 ms (binario de depuración); los hooks que salen con cada
  herramienta solo llaman a nexo si su terminal espera una respuesta (una marca en la carpeta de
  ejecución).
- **Un `claude` dentro de otro** (un `claude -p` que lanza una herramienta) no lleva los hooks
  (`CLAUDECODE`): si no, se contaría como la sesión de la terminal.
- **Orden nueva** `terminal_busy_in` (si algo corre en las terminales de un workspace, sin
  recuperarlas), para preguntar antes de quitarlo del panel. Quitarlo las cierra y conserva lo
  guardado; olvidarlo lo borra.
- **xterm.js** se carga con la primera terminal que se ve: va aparte (452 kB) y el paquete
  principal se queda en 414 kB.
- **El chat de Nexo** pierde también lo que solo servía a las ventanas de los agentes (chats
  alimentados, `relay`, «Abrir la ventana de…», el modo `prompts` del modelo).
- **Ruta del socket**: un socket Unix no admite más de 108 caracteres; el de nexo va en
  `$XDG_RUNTIME_DIR/nexo/` y cabe.
- **Pendiente** de comprobar con la app abierta: que el `claude` que el usuario escribe en una terminal
  lleve los hooks con su `.bashrc` real, los subagentes en vivo (`SubagentStart`/`SubagentStop`),
  recuperar las terminales tras reiniciar y el rendimiento de cuatro terminales escribiendo a la vez
  en WebKitGTK con la GPU.

