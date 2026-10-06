fn main() {
    // El ejecutable exporta su `drmWaitVBlank` para que WebKitGTK use esa y no la de libdrm
    // (ver src/render.rs).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg-bins=-Wl,--export-dynamic-symbol=drmWaitVBlank");
    }
    tauri_build::build()
}
