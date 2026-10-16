//! Cómo pinta WebKitGTK la interfaz en Linux y a cuántos fotogramas por segundo.
//!
//! - **GPU y CPU a la vez, de serie** (modo `hibrido`). La GPU compone las capas de la página
//!   (y dibuja el WebGL de la galaxia) y la CPU rasteriza su contenido
//!   (`WEBKIT_SKIA_ENABLE_CPU_RENDERING`). Rasterizar por GPU es lo que hacía caer el proceso web
//!   con WebKitGTK 2.52 y el driver de NVIDIA 615: `SIGSEGV` dentro de `libnvidia-eglcore`, en el
//!   hilo `SkiaGPUWorker`. La composición va por GPU con `WEBKIT_DMABUF_RENDERER_FORCE_SHM` (el
//!   renderizador DMA-BUF da «Error 71» con NVIDIA en Wayland; con memoria compartida, no).
//! - **Escalera de seguridad.** Si aun así el proceso web se cae dos veces en media hora, nexo
//!   baja un escalón y lo recuerda en `<config>/render.json`: primero sin WebGL (la galaxia pasa a
//!   su canvas 2D; se aplica recargando la vista) y, si sigue cayendo, todo por CPU
//!   (`WEBKIT_DISABLE_DMABUF_RENDERER`, que pide reiniciar nexo). Cuando cambia la versión de
//!   WebKit o la del driver, vuelve a empezar por el modo híbrido. Cada caída suelta recarga la
//!   vista (el backend sigue vivo y los chats se reconectan), como mucho tres veces por minuto.
//! - **El compositor de WebKitGTK 2.52.** La 2.54 compone las capas con Skia
//!   (`UseSkiaForComposition`) y solo recompone lo que cree que cambió
//!   (`UseDamagingInformationForCompositing`). Con nexo eso deja en pantalla píxeles de
//!   fotogramas anteriores: al cambiar de vista, las ventanas de un workspace y las notas vuelven
//!   un instante al principio de su animación de entrada. Se apagan las dos, como en 2.52 (la
//!   primera solo con GPU: sin ella, la 2.54 solo compone con Skia). WebKit las lee al crear la
//!   página y la ventana ya empezó a cargar antes de `attach`, así que al arrancar se abre de
//!   nuevo el proceso web.
//! - **El refresco del monitor (120 o 144 Hz).** WebKit marca el ritmo de los fotogramas con
//!   `drmWaitVBlank`, que el driver de NVIDIA rechaza a las aplicaciones (`EOPNOTSUPP`); entonces
//!   WebKit cae a un temporizador fijo de 60 Hz. nexo exporta su propio `drmWaitVBlank`: prueba
//!   el real y, si falla, espera hasta el siguiente refresco calculado con el modo del monitor.
//!   Además se quita la preferencia de WebKit por pintar cerca de 60 fps (con 144 Hz se quedaría
//!   en 72).
//! - **Al cerrar**, si el proceso web termina por su cuenta mientras usa la GPU, el driver de
//!   NVIDIA lo hace fallar (y queda un volcado de memoria): nexo lo termina antes.
//!
//! Si `WEBKIT_DISABLE_DMABUF_RENDERER` o `WEBKIT_DMABUF_RENDERER_FORCE_SHM` ya vienen definidas,
//! nexo no toca el renderizador ni la escalera (así se prueba a mano la GPU entera).

use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
pub use linux::*;

/// Escalones del pintado, del que más usa la GPU al que menos.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// La GPU compone (y el WebGL); la CPU rasteriza.
    #[default]
    Hibrido,
    /// Igual, sin WebGL.
    SinWebgl,
    /// Todo por CPU.
    Cpu,
}

impl Mode {
    fn next(self) -> Mode {
        match self {
            Mode::Hibrido => Mode::SinWebgl,
            Mode::SinWebgl | Mode::Cpu => Mode::Cpu,
        }
    }
}

/// Lo que se recuerda entre aperturas (`<config>/render.json`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    #[serde(default)]
    pub modo: Mode,
    /// Versiones de WebKit y del driver con las que se decidió el modo.
    #[serde(default)]
    pub huella: String,
    /// Caídas recientes del proceso web en este modo (ms desde 1970).
    #[serde(default)]
    pub caidas: Vec<i64>,
    /// Por qué se bajó de escalón, si se bajó.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motivo: Option<String>,
}

/// Caídas en este tiempo que hacen bajar un escalón.
const CRASH_WINDOW_MS: i64 = 30 * 60 * 1000;
const CRASHES_TO_STEP: usize = 2;

/// Al arrancar: con otras versiones de WebKit o del driver, se vuelve a probar el híbrido.
pub fn start(saved: &Saved, fingerprint: &str) -> Saved {
    if saved.huella != fingerprint {
        return Saved {
            huella: fingerprint.to_string(),
            ..Saved::default()
        };
    }
    saved.clone()
}

/// Qué hacer cuando el proceso web se cae.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Volver a cargar la vista en el mismo modo.
    Reload,
    /// Bajar a este escalón.
    StepDown(Mode),
}

pub fn on_crash(saved: &mut Saved, now: i64) -> Action {
    saved.caidas.retain(|at| now - at < CRASH_WINDOW_MS);
    saved.caidas.push(now);
    if saved.modo == Mode::Cpu || saved.caidas.len() < CRASHES_TO_STEP {
        return Action::Reload;
    }
    let next = saved.modo.next();
    saved.motivo = Some(format!(
        "El proceso de WebKit se cayó {} veces en media hora en el modo «{}».",
        saved.caidas.len(),
        match saved.modo {
            Mode::Hibrido => "GPU y CPU",
            Mode::SinWebgl => "GPU y CPU sin WebGL",
            Mode::Cpu => "CPU",
        }
    ));
    saved.modo = next;
    saved.caidas.clear();
    Action::StepDown(next)
}

/// Estado del pintado para la interfaz (Conexiones → Gráficos).
#[derive(Serialize)]
pub struct RenderInfo {
    /// `hibrido`, `sin-webgl`, `cpu` o, si lo fija el entorno, `forzado-gpu`/`forzado-cpu`.
    modo: String,
    /// Por qué se bajó de escalón (la última vez).
    motivo: Option<String>,
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
            modo: "hibrido".into(),
            motivo: None,
            hz: None,
        }
    }
}
#[cfg(not(target_os = "linux"))]
pub use other::*;

#[cfg(target_os = "linux")]
mod linux {
    use std::env;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use tauri::Manager;

    use super::{Action, Mode, Saved};

    const IDENTIFIER: &str = "com.nexo.app";
    const STATE_FILE: &str = "render.json";

    /// Usa la GPU (para cerrar con cuidado).
    static GPU: AtomicBool = AtomicBool::new(false);
    static CLOSING: AtomicBool = AtomicBool::new(false);
    /// El renderizador lo fijó el entorno: ni escalera ni `render.json`.
    static FORCED: AtomicBool = AtomicBool::new(false);
    /// Escalón en marcha (0 híbrido, 1 sin WebGL, 2 CPU).
    static MODE: AtomicU8 = AtomicU8::new(0);
    static STATE: Mutex<Option<Saved>> = Mutex::new(None);

    /// Recargas por caída del proceso web que se permiten en un minuto.
    const RELOADS_PER_MINUTE: usize = 3;

    fn mode() -> Mode {
        match MODE.load(Ordering::Relaxed) {
            0 => Mode::Hibrido,
            1 => Mode::SinWebgl,
            _ => Mode::Cpu,
        }
    }

    fn set_mode(mode: Mode) {
        MODE.store(
            match mode {
                Mode::Hibrido => 0,
                Mode::SinWebgl => 1,
                Mode::Cpu => 2,
            },
            Ordering::Relaxed,
        );
    }

    /// La misma carpeta que `app_config_dir` de Tauri, sin necesitar la app (aún no existe).
    fn config_dir() -> Option<PathBuf> {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|d| d.join(IDENTIFIER))
    }

    fn read_state() -> Saved {
        config_dir()
            .and_then(|d| std::fs::read_to_string(d.join(STATE_FILE)).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn write_state(saved: &Saved) {
        let Some(dir) = config_dir() else { return };
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(json) = serde_json::to_string_pretty(saved) {
            let _ = std::fs::write(dir.join(STATE_FILE), json);
        }
    }

    /// Versiones de WebKit y del driver de NVIDIA (si lo hay): si cambian, se vuelve a probar.
    fn fingerprint() -> String {
        // SAFETY: funciones sin argumentos de WebKitGTK, se pueden llamar antes de iniciar GTK.
        let webkit = unsafe {
            format!(
                "{}.{}.{}",
                webkit2gtk::ffi::webkit_get_major_version(),
                webkit2gtk::ffi::webkit_get_minor_version(),
                webkit2gtk::ffi::webkit_get_micro_version()
            )
        };
        let driver = std::fs::read_to_string("/sys/module/nvidia/version")
            .map(|v| format!("nvidia {}", v.trim()))
            .unwrap_or_else(|_| "sin nvidia".into());
        format!("webkit {webkit} · {driver}")
    }

    fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

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
            FORCED.store(true, Ordering::Relaxed);
            GPU.store(
                env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none(),
                Ordering::Relaxed,
            );
            return;
        }
        let before = read_state();
        let saved = super::start(&before, &fingerprint());
        if saved != before {
            write_state(&saved);
        }
        set_mode(saved.modo);
        let gpu = saved.modo != Mode::Cpu;
        // SAFETY: se ejecuta al inicio de main, antes de crear cualquier hilo.
        unsafe {
            if gpu {
                env::set_var("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1");
                // La rasterización, por CPU: por GPU es donde se caía el driver de NVIDIA.
                if env::var_os("WEBKIT_SKIA_ENABLE_CPU_RENDERING").is_none() {
                    env::set_var("WEBKIT_SKIA_ENABLE_CPU_RENDERING", "1");
                }
            } else {
                env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            }
        }
        GPU.store(gpu, Ordering::Relaxed);
        if let Ok(mut state) = STATE.lock() {
            *state = Some(saved);
        }
    }

    /// Con la ventana creada: fotogramas al ritmo del monitor, el escalón elegido y qué hacer si
    /// el proceso web se cae.
    pub fn attach(app: &tauri::App) {
        let Some(window) = app.get_webview_window("main") else {
            return;
        };
        let handle = app.handle().clone();
        let _ = window.with_webview(move |webview| {
            use webkit2gtk::{SettingsExt, WebProcessTerminationReason, WebViewExt};
            let view = webview.inner();
            if let Some(settings) = WebViewExt::settings(&view) {
                use webkit2gtk::glib::translate::ToGlibPtr;
                let ptr: *mut webkit2gtk::ffi::WebKitSettings = settings.to_glib_none().0;
                // SAFETY: `ptr` es el WebKitSettings vivo de esta vista.
                unsafe { features::disable(ptr.cast(), c"PreferPageRenderingUpdatesNear60FPS") };
                if !FORCED.load(Ordering::Relaxed) {
                    if mode() == Mode::SinWebgl {
                        settings.set_enable_webgl(false);
                    }
                    // SAFETY: igual que arriba.
                    let mut changed = unsafe {
                        features::disable(ptr.cast(), c"UseDamagingInformationForCompositing")
                    };
                    // Sin GPU, WebKit 2.54 solo sabe componer con Skia.
                    if GPU.load(Ordering::Relaxed) {
                        // SAFETY: igual que arriba.
                        changed |=
                            unsafe { features::disable(ptr.cast(), c"UseSkiaForComposition") };
                    }
                    if changed {
                        // La página ya se creó con el valor de antes: otro proceso web lo toma.
                        let uri = view.uri();
                        view.terminate_web_process();
                        match uri {
                            Some(uri) => view.load_uri(&uri),
                            None => view.reload(),
                        }
                    }
                }
            }
            static RELOADS: Mutex<Vec<Instant>> = Mutex::new(Vec::new());
            view.connect_web_process_terminated(move |view, reason| {
                if reason != WebProcessTerminationReason::Crashed || CLOSING.load(Ordering::Relaxed)
                {
                    return;
                }
                if !FORCED.load(Ordering::Relaxed) {
                    let action = STATE.lock().ok().and_then(|mut state| {
                        let saved = state.as_mut()?;
                        let action = super::on_crash(saved, now_ms());
                        write_state(saved);
                        Some(action)
                    });
                    match action {
                        Some(Action::StepDown(Mode::Cpu)) => {
                            // Pasar a la CPU cambia el renderizador: solo se puede al arrancar.
                            log::warn!(
                                "WebKit se cae con la GPU: nexo vuelve a abrirse pintando por CPU."
                            );
                            set_mode(Mode::Cpu);
                            handle.restart();
                        }
                        Some(Action::StepDown(next)) => {
                            log::warn!("WebKit se cae con la GPU: nexo sigue sin WebGL.");
                            set_mode(next);
                            if let Some(settings) = WebViewExt::settings(view) {
                                settings.set_enable_webgl(false);
                            }
                        }
                        _ => {}
                    }
                }
                let Ok(mut reloads) = RELOADS.lock() else {
                    return;
                };
                reloads.retain(|at| at.elapsed() < Duration::from_secs(60));
                if reloads.len() >= RELOADS_PER_MINUTE {
                    log::error!("El proceso de WebKit se cae una y otra vez: no se recarga más.");
                    return;
                }
                reloads.push(Instant::now());
                log::warn!("El proceso de WebKit se cayó: se vuelve a cargar la vista.");
                view.reload();
            });
        });
    }

    #[tauri::command]
    pub fn render_info() -> super::RenderInfo {
        let period = super::vblank::LAST_PERIOD.load(Ordering::Relaxed);
        let modo = if FORCED.load(Ordering::Relaxed) {
            if GPU.load(Ordering::Relaxed) {
                "forzado-gpu"
            } else {
                "forzado-cpu"
            }
        } else {
            match mode() {
                Mode::Hibrido => "hibrido",
                Mode::SinWebgl => "sin-webgl",
                Mode::Cpu => "cpu",
            }
        };
        super::RenderInfo {
            modo: modo.into(),
            motivo: STATE
                .lock()
                .ok()
                .and_then(|s| s.as_ref().and_then(|s| s.motivo.clone())),
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
            fn webkit_settings_get_feature_enabled(
                settings: *mut c_void,
                feature: *mut c_void,
            ) -> c_int;
            fn webkit_settings_set_feature_enabled(
                settings: *mut c_void,
                feature: *mut c_void,
                enabled: c_int,
            );
            fn webkit_feature_list_unref(list: *mut c_void);
        }

        /// Apaga la función `name` si esta versión de WebKit la tiene; dice si estaba encendida.
        ///
        /// # Safety
        /// `settings` debe ser un `WebKitSettings*` válido.
        pub unsafe fn disable(settings: *mut c_void, name: &CStr) -> bool {
            let mut changed = false;
            unsafe {
                let list = webkit_settings_get_all_features();
                if list.is_null() {
                    return false;
                }
                for i in 0..webkit_feature_list_get_length(list) {
                    let feature = webkit_feature_list_get(list, i);
                    let id = webkit_feature_get_identifier(feature);
                    if !id.is_null()
                        && CStr::from_ptr(id) == name
                        && webkit_settings_get_feature_enabled(settings, feature) != 0
                    {
                        webkit_settings_set_feature_enabled(settings, feature, 0);
                        changed = true;
                    }
                }
                webkit_feature_list_unref(list);
            }
            changed
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empieza_por_el_hibrido_y_vuelve_a_el_al_cambiar_de_version() {
        let fresh = start(&Saved::default(), "webkit 2.52.6 · nvidia 615.71.09");
        assert_eq!(fresh.modo, Mode::Hibrido);
        let mut cpu = fresh.clone();
        cpu.modo = Mode::Cpu;
        assert_eq!(
            start(&cpu, "webkit 2.52.6 · nvidia 615.71.09").modo,
            Mode::Cpu,
            "se recuerda"
        );
        let updated = start(&cpu, "webkit 2.52.7 · nvidia 615.71.09");
        assert_eq!(
            updated.modo,
            Mode::Hibrido,
            "con otra versión se vuelve a probar"
        );
        assert!(updated.caidas.is_empty());
    }

    #[test]
    fn dos_caidas_en_media_hora_bajan_un_escalon() {
        let mut s = Saved::default();
        let min = 60_000;
        assert_eq!(
            on_crash(&mut s, 0),
            Action::Reload,
            "una caída suelta solo recarga"
        );
        // La siguiente, pasada la media hora, tampoco.
        assert_eq!(on_crash(&mut s, 31 * min), Action::Reload);
        assert_eq!(on_crash(&mut s, 40 * min), Action::StepDown(Mode::SinWebgl));
        assert!(s.motivo.as_deref().unwrap().contains("GPU y CPU"));
        assert!(s.caidas.is_empty(), "en el escalón nuevo se cuenta de cero");
        assert_eq!(on_crash(&mut s, 41 * min), Action::Reload);
        assert_eq!(on_crash(&mut s, 42 * min), Action::StepDown(Mode::Cpu));
        assert_eq!(s.modo, Mode::Cpu);
        // En la CPU ya no hay a dónde bajar: solo se recarga.
        assert_eq!(on_crash(&mut s, 43 * min), Action::Reload);
        assert_eq!(on_crash(&mut s, 44 * min), Action::Reload);
    }

    #[test]
    fn el_estado_se_guarda_en_json_legible() {
        let s = Saved {
            modo: Mode::SinWebgl,
            huella: "webkit 2.52.6 · nvidia 615.71.09".into(),
            caidas: vec![1],
            motivo: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""modo":"sin-webgl""#));
        assert_eq!(serde_json::from_str::<Saved>(&json).unwrap(), s);
        assert_eq!(
            serde_json::from_str::<Saved>("{}").unwrap().modo,
            Mode::Hibrido
        );
    }
}
