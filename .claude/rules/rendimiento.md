---
paths:
  - "src/styles.css"
  - "src/galaxy.js"
  - "src/graph.js"
  - "src/notes.js"
  - "src-tauri/src/render.rs"
  - "src/card-resize.js"
---

# Rendimiento en WebKitGTK

- **Rendimiento en WebKitGTK**: evitar `backdrop-filter` (con el grafo animado detrás, cada fotograma obliga a redesenfocar) y sombras con difuminado grande en lo que se mueve; las animaciones que corren siempre deben repintar solo un rectángulo pequeño (como `.graph-fx`). nexo rasteriza por CPU y compone por GPU (ver `render.md`): cada repintado de una zona cuesta CPU, y cada fotograma se compone entero (WebKit 2.54 recomponía solo lo dañado y parpadeaba; ver `render.md`) y se copia por memoria compartida. Por eso el lienzo de la galaxia en reposo solo cubre su rectángulo (`galaxyExtent`, toda la ventana solo mientras se dispersa; el cambio de un tamaño a otro se hace intercambiando dos lienzos, nunca redimensionando el que se ve) y se repinta como mucho a 60 fps (`IDLE_FPS`; transiciones y ratón a la frecuencia del monitor). Medido a 120 Hz: ~35 % de un núcleo → ~19 % entre WebKit, la ventana y Hyprland. Para medir, `MiniBrowser` (`/usr/lib/webkit2gtk-4.1/`) con las mismas variables que pone `render.rs` es el mismo motor que la app.
- **Nada pesado se maqueta en cada fotograma de una transición de ancho** (las cartas laterales, ver «Cartas expandibles» en `interfaz.md`): lo de dentro toma su medida final de una vez y se recorta (`card-resize.js`). Lo que cuesta en esas transiciones es maquetar y pintar texto, no componer: con cuatro chats de 40 mensajes en el centro, minimizar el chat iba a 3-15 fps en el banco de WebKitGTK y a 37-61 con el centro oculto. Un cambio de estilo que se hereda o un selector con descendientes (`inert`, `.x .stage *`) en algo que cubre el centro vuelve a resolver miles de elementos (unos 60 ms): va al acabar la transición o no va.
- **Las conversaciones largas solo maquetan lo que se ve**: las piezas del chat lejos del final llevan `content-visibility: auto` (`chat-far`, ver `claude-code.md`). Volver a colocar las cuatro ventanas de un workspace (abrir, minimizar o cerrar una) bajó de 43-46 ms de maquetación a 9-12 ms. WebKitGTK 2.54 tiene `content-visibility` y el anclaje del desplazamiento (`overflow-anchor`); no tiene `overflow-clip-margin`.
