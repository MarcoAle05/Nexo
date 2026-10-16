---
paths:
  - "src/styles.css"
  - "src/galaxy.js"
  - "src/graph.js"
  - "src/notes.js"
  - "src-tauri/src/render.rs"
---

# Rendimiento en WebKitGTK

- **Rendimiento en WebKitGTK**: evitar `backdrop-filter` (con el grafo animado detrás, cada fotograma obliga a redesenfocar) y sombras con difuminado grande en lo que se mueve; las animaciones que corren siempre deben repintar solo un rectángulo pequeño (como `.graph-fx`). nexo rasteriza por CPU y compone por GPU (ver `render.md`): cada repintado de una zona cuesta CPU, y cada fotograma se compone entero (WebKit 2.54 recomponía solo lo dañado y parpadeaba; ver `render.md`) y se copia por memoria compartida. Por eso el lienzo de la galaxia en reposo solo cubre su rectángulo (`galaxyExtent`, toda la ventana solo mientras se dispersa; el cambio de un tamaño a otro se hace intercambiando dos lienzos, nunca redimensionando el que se ve) y se repinta como mucho a 60 fps (`IDLE_FPS`; transiciones y ratón a la frecuencia del monitor). Medido a 120 Hz: ~35 % de un núcleo → ~19 % entre WebKit, la ventana y Hyprland. Para medir, `MiniBrowser` (`/usr/lib/webkit2gtk-4.1/`) con las mismas variables que pone `render.rs` es el mismo motor que la app.
