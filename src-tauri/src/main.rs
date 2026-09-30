// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // WebKitGTK + NVIDIA en Wayland falla con "Error 71 (Protocol error)";
    // desactivar el renderizador DMA-BUF lo evita. Respeta un valor ya definido.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: se ejecuta al inicio de main, antes de crear cualquier hilo.
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }

    app_lib::run();
}
