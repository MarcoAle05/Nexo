//! Cómo pinta WebKitGTK la interfaz en Linux y a cuántos fotogramas por segundo.
//!
//! - **Por CPU, por ahora.** Por GPU (`WEBKIT_DMABUF_RENDERER_FORCE_SHM`) el proceso web de
//!   WebKitGTK 2.52 se cae a menudo dentro del driver de NVIDIA 615 (`libnvidia-eglcore`, en el
//!   hilo `SkiaGPUWorker` que rasteriza la página y en el principal): nexo pinta siempre por CPU
//!   (`WEBKIT_DISABLE_DMABUF_RENDERER`). El renderizador DMA-BUF además da «Error 71» con
//!   NVIDIA en Wayland. Para volver a probar la GPU, arrancar nexo con
//!   `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1`.
//! - **Si el proceso web se cae**, la vista se vuelve a cargar (como una recarga de Vite: el
//!   backend sigue vivo y los chats se reconectan a sus procesos). Como mucho unas cuantas
//!   veces por minuto, para no entrar en un bucle.
//! - **El refresco del monitor (120 o 144 Hz).** WebKit marca el ritmo de los fotogramas con
//!   `drmWaitVBlank`, que el driver de NVIDIA rechaza a las aplicaciones (`EOPNOTSUPP`); entonces
//!   WebKit cae a un temporizador fijo de 60 Hz. nexo exporta su propio `drmWaitVBlank`: prueba
//!   el real y, si falla, espera hasta el siguiente refresco calculado con el modo del monitor.
//!   Además se quita la preferencia de WebKit por pintar cerca de 60 fps (con 144 Hz se quedaría
//!   en 72).
//! - **Al cerrar**, si el proceso web termina por su cuenta mientras pinta por GPU, el driver de
//!   NVIDIA lo hace fallar (y queda un volcado de memoria): nexo lo termina antes.
//!
//! Si `WEBKIT_DISABLE_DMABUF_RENDERER` o `WEBKIT_DMABUF_RENDERER_FORCE_SHM` ya vienen definidas,
//! nexo no toca el renderizador.

#[cfg(target_os = "linux")]
pub use linux::*;

/// Estado del pintado para la interfaz (Conexiones → Gráficos).
#[derive(serde::Serialize)]
pub struct RenderInfo {
    /// WebKit pinta por GPU (solo si se forzó con una variable de entorno).
    gpu: bool,
    /// Refresco que nexo le marca a WebKit (cuando el driver no da la señal), en Hz.
    hz: Option<u32>,
}

#[cfg(not(target_os = "linux"))]
mod other {
    pub fn configure() {}
    pub fn attach(_app: &tauri::App) {}
    pub fn shutdown() {}

    #[tauri::command]
    pub fn render_info() -> super::RenderInfo {
        super::RenderInfo {
            gpu: true,
            hz: None,
        }
    }
}
#[cfg(not(target_os = "linux"))]
pub use other::*;

#[cfg(target_os = "linux")]
mod linux {
    use std::env;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use tauri::Manager;

    static GPU: AtomicBool = AtomicBool::new(false);
    static CLOSING: AtomicBool = AtomicBool::new(false);

    /// Recargas por caída del proceso web que se permiten en un minuto.
    const RELOADS_PER_MINUTE: usize = 3;

    /// Antes de crear la ventana: elige cómo pinta WebKit.
    pub fn configure() {
        // Asegura que el `drmWaitVBlank` de nexo entra en el ejecutable.
        std::hint::black_box(super::vblank::drmWaitVBlank as *const ());
        let preset = [
            "WEBKIT_DISABLE_DMABUF_RENDERER",
            "WEBKIT_DMABUF_RENDERER_FORCE_SHM",
        ]
        .iter()
        .any(|v| env::var_os(v).is_some());
        if preset {
            GPU.store(
                env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none(),
                Ordering::Relaxed,
            );
            return;
        }
        // Por CPU: por GPU, el driver de NVIDIA hace caer el proceso web (ver arriba).
        // SAFETY: se ejecuta al inicio de main, antes de crear cualquier hilo.
        unsafe { env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
        GPU.store(false, Ordering::Relaxed);
    }

    /// Con la ventana creada: fotogramas al ritmo del monitor y, si el proceso web se cae, la
    /// vista se vuelve a cargar.
    pub fn attach(app: &tauri::App) {
        let Some(window) = app.get_webview_window("main") else {
            return;
        };
        let _ = window.with_webview(move |webview| {
            use webkit2gtk::{WebProcessTerminationReason, WebViewExt};
            let view = webview.inner();
            if let Some(settings) = WebViewExt::settings(&view) {
                use webkit2gtk::glib::translate::ToGlibPtr;
                let ptr: *mut webkit2gtk::ffi::WebKitSettings = settings.to_glib_none().0;
                // SAFETY: `ptr` es el WebKitSettings vivo de esta vista.
                unsafe { features::disable(ptr.cast(), c"PreferPageRenderingUpdatesNear60FPS") };
            }
            static CRASHES: Mutex<Vec<Instant>> = Mutex::new(Vec::new());
            view.connect_web_process_terminated(move |view, reason| {
                if reason != WebProcessTerminationReason::Crashed || CLOSING.load(Ordering::Relaxed)
                {
                    return;
                }
                let Ok(mut crashes) = CRASHES.lock() else {
                    return;
                };
                crashes.retain(|at| at.elapsed() < Duration::from_secs(60));
                if crashes.len() >= RELOADS_PER_MINUTE {
                    log::error!("El proceso de WebKit se cae una y otra vez: no se recarga más.");
                    return;
                }
                crashes.push(Instant::now());
                log::warn!("El proceso de WebKit se cayó: se vuelve a cargar la vista.");
                view.reload();
            });
        });
    }

    #[tauri::command]
    pub fn render_info() -> super::RenderInfo {
        let period = super::vblank::LAST_PERIOD.load(Ordering::Relaxed);
        super::RenderInfo {
            gpu: GPU.load(Ordering::Relaxed),
            hz: (period > 0).then(|| (1e9 / period as f64).round() as u32),
        }
    }

    /// Al cerrar: termina los procesos de WebKit antes de que salgan por su cuenta.
    pub fn shutdown() {
        if CLOSING.swap(true, Ordering::Relaxed) || !GPU.load(Ordering::Relaxed) {
            return;
        }
        let me = std::process::id().to_string();
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
                continue;
            };
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
                continue;
            };
            // «pid (nombre) estado ppid …»; el nombre puede llevar espacios o paréntesis.
            let Some((name, rest)) = stat.split_once(" (").and_then(|(_, r)| r.rsplit_once(") "))
            else {
                continue;
            };
            let ppid = rest.split_whitespace().nth(1);
            if ppid == Some(me.as_str()) && name.starts_with("WebKit") {
                // SAFETY: señal a un proceso hijo nuestro.
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
        }
    }

    /// API de «features» de WebKitGTK (2.42+); el crate webkit2gtk aún no la expone.
    mod features {
        use std::ffi::{CStr, c_char, c_int, c_void};

        #[link(name = "webkit2gtk-4.1")]
        unsafe extern "C" {
            fn webkit_settings_get_all_features() -> *mut c_void;
            fn webkit_feature_list_get_length(list: *mut c_void) -> usize;
            fn webkit_feature_list_get(list: *mut c_void, index: usize) -> *mut c_void;
            fn webkit_feature_get_identifier(feature: *mut c_void) -> *const c_char;
            fn webkit_settings_set_feature_enabled(
                settings: *mut c_void,
                feature: *mut c_void,
                enabled: c_int,
            );
            fn webkit_feature_list_unref(list: *mut c_void);
        }

        /// # Safety
        /// `settings` debe ser un `WebKitSettings*` válido.
        pub unsafe fn disable(settings: *mut c_void, name: &CStr) {
            unsafe {
                let list = webkit_settings_get_all_features();
                if list.is_null() {
                    return;
                }
                for i in 0..webkit_feature_list_get_length(list) {
                    let feature = webkit_feature_list_get(list, i);
                    let id = webkit_feature_get_identifier(feature);
                    if !id.is_null() && CStr::from_ptr(id) == name {
                        webkit_settings_set_feature_enabled(settings, feature, 0);
                    }
                }
                webkit_feature_list_unref(list);
            }
        }
    }
}

/// Señal de refresco para WebKit cuando el driver no la da (ver arriba). WebKit la pide desde
/// un hilo propio, con `DRM_VBLANK_RELATIVE` y la pantalla codificada en el tipo.
#[cfg(target_os = "linux")]
pub mod vblank {
    use std::ffi::{CStr, c_int, c_long, c_uint, c_ulong, c_void};
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};

    /// Último periodo de refresco calculado (ns): el de la pantalla donde está la ventana.
    pub static LAST_PERIOD: AtomicU64 = AtomicU64::new(0);

    const RELATIVE: c_uint = 0x1;
    const SECONDARY: c_uint = 0x2000_0000;
    const HIGH_CRTC_MASK: c_uint = 0x3e;
    const HIGH_CRTC_SHIFT: c_uint = 1;

    #[repr(C)]
    #[allow(dead_code)]
    #[derive(Clone, Copy)]
    pub struct Request {
        kind: c_uint,
        sequence: c_uint,
        signal: c_ulong,
    }

    #[repr(C)]
    #[allow(dead_code)]
    #[derive(Clone, Copy)]
    pub struct Reply {
        kind: c_uint,
        sequence: c_uint,
        tval_sec: c_long,
        tval_usec: c_long,
    }

    /// `drmVBlank` de libdrm.
    #[repr(C)]
    pub union VBlank {
        request: Request,
        reply: Reply,
    }

    /// `drmModeModeInfo` de libdrm.
    #[repr(C)]
    #[allow(dead_code)]
    struct ModeInfo {
        clock: u32,
        hdisplay: u16,
        hsync_start: u16,
        hsync_end: u16,
        htotal: u16,
        hskew: u16,
        vdisplay: u16,
        vsync_start: u16,
        vsync_end: u16,
        vtotal: u16,
        vscan: u16,
        vrefresh: u32,
        flags: u32,
        kind: u32,
        name: [u8; 32],
    }

    /// Principio de `drmModeCrtc` (solo se leen estos campos).
    #[repr(C)]
    #[allow(dead_code)]
    struct Crtc {
        crtc_id: u32,
        buffer_id: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        mode_valid: c_int,
        mode: ModeInfo,
    }

    /// Principio de `drmModeRes`.
    #[repr(C)]
    #[allow(dead_code)]
    struct Resources {
        count_fbs: c_int,
        fbs: *mut u32,
        count_crtcs: c_int,
        crtcs: *mut u32,
    }

    type WaitFn = unsafe extern "C" fn(c_int, *mut VBlank) -> c_int;

    unsafe fn symbol<T>(handle: *mut c_void, name: &CStr) -> Option<T> {
        // SAFETY: el llamador da el tipo correcto de la función.
        let ptr = unsafe { libc::dlsym(handle, name.as_ptr()) };
        (!ptr.is_null()).then(|| unsafe { std::mem::transmute_copy(&ptr) })
    }

    /// Duración de un refresco de la pantalla `index`, en nanosegundos.
    fn period(fd: c_int, index: usize) -> Option<u64> {
        static CACHE: Mutex<[u64; 32]> = Mutex::new([0; 32]);
        if let Some(&p) = CACHE.lock().ok()?.get(index).filter(|p| **p > 0) {
            return Some(p);
        }
        let d = libc::RTLD_DEFAULT;
        // SAFETY: firmas de libdrm (xf86drmMode.h); libdrm ya está cargada por WebKit.
        let (get_res, free_res, get_crtc, free_crtc) = unsafe {
            (
                symbol::<unsafe extern "C" fn(c_int) -> *mut Resources>(d, c"drmModeGetResources")?,
                symbol::<unsafe extern "C" fn(*mut Resources)>(d, c"drmModeFreeResources")?,
                symbol::<unsafe extern "C" fn(c_int, u32) -> *mut Crtc>(d, c"drmModeGetCrtc")?,
                symbol::<unsafe extern "C" fn(*mut Crtc)>(d, c"drmModeFreeCrtc")?,
            )
        };
        let mut out = None;
        // SAFETY: punteros devueltos por libdrm, liberados con sus funciones.
        unsafe {
            let res = get_res(fd);
            if res.is_null() {
                return None;
            }
            if index < (*res).count_crtcs.max(0) as usize {
                let crtc = get_crtc(fd, *(*res).crtcs.add(index));
                if !crtc.is_null() {
                    let m = &(*crtc).mode;
                    if (*crtc).mode_valid != 0 && m.clock > 0 && m.htotal > 0 && m.vtotal > 0 {
                        out = Some(
                            u64::from(m.htotal) * u64::from(m.vtotal) * 1_000_000
                                / u64::from(m.clock),
                        );
                    }
                    free_crtc(crtc);
                }
            }
            free_res(res);
        }
        if let (Some(p), Ok(mut cache)) = (out, CACHE.lock())
            && let Some(slot) = cache.get_mut(index)
        {
            *slot = p;
        }
        out
    }

    fn monotonic() -> libc::timespec {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `ts` es un timespec válido.
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
        ts
    }

    const fn nanos(ts: &libc::timespec) -> u64 {
        ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
    }

    /// Sustituye a `drmWaitVBlank` de libdrm para todo el proceso (el ejecutable la exporta).
    ///
    /// # Safety
    /// Mismo contrato que la de libdrm: `vbl` apunta a un `drmVBlank` válido.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn drmWaitVBlank(fd: c_int, vbl: *mut VBlank) -> c_int {
        static REAL: OnceLock<Option<WaitFn>> = OnceLock::new();
        static EMULATE: AtomicBool = AtomicBool::new(false);
        static SEQUENCE: AtomicU32 = AtomicU32::new(0);
        // SAFETY: `vbl` es válido (contrato de la función).
        let Request { kind, sequence, .. } = unsafe { (*vbl).request };
        if !EMULATE.load(Ordering::Relaxed) {
            let real = *REAL.get_or_init(|| unsafe { symbol(libc::RTLD_NEXT, c"drmWaitVBlank") });
            let Some(real) = real else { return -1 };
            // SAFETY: la función real de libdrm con los mismos argumentos.
            let ret = unsafe { real(fd, vbl) };
            if ret == 0 || kind & RELATIVE == 0 {
                return ret;
            }
            // El driver no da la señal (NVIDIA): a partir de aquí se calcula.
            EMULATE.store(true, Ordering::Relaxed);
        }
        let index = if kind & SECONDARY != 0 {
            1
        } else {
            ((kind & HIGH_CRTC_MASK) >> HIGH_CRTC_SHIFT) as usize
        };
        let Some(period) = period(fd, index) else {
            return -1;
        };
        LAST_PERIOD.store(period, Ordering::Relaxed);
        let mut now = monotonic();
        if sequence > 0 {
            let target = (nanos(&now) / period + u64::from(sequence)) * period;
            let ts = libc::timespec {
                tv_sec: (target / 1_000_000_000) as libc::time_t,
                tv_nsec: (target % 1_000_000_000) as libc::c_long,
            };
            // SAFETY: espera absoluta con un timespec válido; se repite si la interrumpe una señal.
            while unsafe {
                libc::clock_nanosleep(
                    libc::CLOCK_MONOTONIC,
                    libc::TIMER_ABSTIME,
                    &ts,
                    std::ptr::null_mut(),
                )
            } == libc::EINTR
            {}
            now = ts;
        }
        let reply = Reply {
            kind,
            sequence: SEQUENCE.fetch_add(1, Ordering::Relaxed).wrapping_add(1),
            tval_sec: now.tv_sec as c_long,
            tval_usec: (now.tv_nsec / 1000) as c_long,
        };
        // SAFETY: `vbl` es válido.
        unsafe { (*vbl).reply = reply };
        0
    }
}
