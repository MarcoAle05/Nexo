// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `nexo ctl …`: la orden de control habla con el nexo abierto y sale, sin abrir ventana.
    if let Some(code) = app_lib::control::cli() {
        std::process::exit(code);
    }
    // Cómo pinta WebKitGTK (GPU, refresco del monitor): ver render.rs. Tiene que ir antes de
    // crear cualquier hilo o ventana.
    app_lib::render::configure();
    app_lib::run();
}
