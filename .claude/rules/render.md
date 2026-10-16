---
paths:
  - "src-tauri/src/render.rs"
  - "src-tauri/src/main.rs"
  - "src-tauri/build.rs"
---

# Cómo pinta WebKitGTK

`main.rs` llama a `render::configure()` antes de crear nada; no quitarlo. Para medir el rendimiento, ver `rendimiento.md`.

- `render.rs`: cómo pinta WebKitGTK en Linux. **Por CPU, por ahora** (`WEBKIT_DISABLE_DMABUF_RENDERER`): por GPU (`WEBKIT_DMABUF_RENDERER_FORCE_SHM`, el renderizador DMA-BUF da «Error 71» con NVIDIA en Wayland) el proceso web de WebKitGTK 2.52 se caía a menudo dentro del driver de NVIDIA 615 (`SIGSEGV` en `libnvidia-eglcore.so`, en el hilo `SkiaGPUWorker` y en el principal; se ve en `journalctl` y `coredumpctl`). El usuario pidió quitar la opción «Volver a probar la GPU» y que funcione; ya no hay marca `render-cpu` ni reinicio a la CPU. Para probar la GPU a mano: arrancar con `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1` (si una de las dos variables viene del entorno, nexo no toca nada). Variables de Skia que admite esta versión, por si se vuelve a intentar: `WEBKIT_SKIA_ENABLE_CPU_RENDERING` (rasterizar por CPU y componer por GPU) y `WEBKIT_SKIA_GPU_PAINTING_THREADS`. **Si el proceso web se cae** (por lo que sea), la vista se vuelve a cargar, como mucho 3 veces por minuto: el backend sigue vivo y los chats se reconectan a sus procesos. El ejecutable **exporta su propio `drmWaitVBlank`** (`build.rs` añade `--export-dynamic-symbol`): NVIDIA rechaza esa llamada (`EOPNOTSUPP`) y WebKit caía a un temporizador fijo de 60 Hz; la de nexo prueba la real y, si falla, espera al siguiente refresco calculado con el modo del monitor (120/144 Hz). También desactiva la preferencia `PreferPageRenderingUpdatesNear60FPS`. Con GPU, al cerrar la ventana mata con SIGKILL los procesos `WebKit*` hijos (si salen solos mientras pintan por GPU, el driver los hace fallar). En Conexiones → Gráficos se dice cómo pinta.
