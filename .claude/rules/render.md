---
paths:
  - "src-tauri/src/render.rs"
  - "src-tauri/src/main.rs"
  - "src-tauri/build.rs"
---

# Cómo pinta WebKitGTK

`main.rs` llama a `render::configure()` antes de crear nada; no quitarlo. Para medir el rendimiento, ver `rendimiento.md`.

- `render.rs`: cómo pinta WebKitGTK en Linux. Por GPU con `WEBKIT_DMABUF_RENDERER_FORCE_SHM` (el renderizador DMA-BUF da «Error 71» con NVIDIA en Wayland; antes se desactivaba y todo se pintaba por CPU). El ejecutable **exporta su propio `drmWaitVBlank`** (`build.rs` añade `--export-dynamic-symbol`): NVIDIA rechaza esa llamada (`EOPNOTSUPP`) y WebKit caía a un temporizador fijo de 60 Hz; la de nexo prueba la real y, si falla, espera al siguiente refresco calculado con el modo del monitor (120/144 Hz). También desactiva la preferencia `PreferPageRenderingUpdatesNear60FPS`. Al cerrar la ventana mata con SIGKILL los procesos `WebKit*` hijos (si salen solos mientras pintan por GPU, el driver los hace fallar). Si el proceso web falla en marcha, deja `<config>/render-cpu`, vuelve a la CPU y reinicia (Conexiones → Gráficos tiene «Volver a probar la GPU»). Ojo al probar: matar el proceso web desde fuera cuenta como fallo y deja esa marca. Si `WEBKIT_DISABLE_DMABUF_RENDERER` o `WEBKIT_DMABUF_RENDERER_FORCE_SHM` vienen del entorno, no toca nada.
