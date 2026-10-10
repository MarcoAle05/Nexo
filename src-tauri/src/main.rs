// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Cómo pinta WebKitGTK (GPU, refresco del monitor): ver render.rs. Tiene que ir antes de
    // crear cualquier hilo o ventana.
    app_lib::render::configure();
    app_lib::run();
}
