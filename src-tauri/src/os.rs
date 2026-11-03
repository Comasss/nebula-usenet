//! Lo que depende del sistema operativo, con una misma interfaz para el resto del motor:
//! Windows (winint.rs), Android (android.rs) y, para compilar y probar, el resto de Unix.
use crate::engine::Engine;
use std::path::Path;
use std::sync::Arc;

#[cfg(windows)]
pub use crate::winint::{open_path, refresh_registrations, register_app_identity, reveal_path, set_association, set_autostart};

/// Notificación del sistema (fin de una descarga, enlace detectado…).
pub fn notify(app: &tauri::AppHandle, tools_dir: &Path, title: &str, body: &str) {
    #[cfg(windows)]
    crate::winint::toast(app, tools_dir, title, body);
    #[cfg(target_os = "android")]
    {
        let _ = (app, tools_dir);
        crate::android::notify(title, body);
    }
    #[cfg(not(any(windows, target_os = "android")))]
    let _ = (app, tools_dir, title, body);
}

/// Notificación de la cuenta atrás de apagado, con el botón Cancelar (solo Windows).
pub fn notify_power(app: &tauri::AppHandle, title: &str, body: &str, cancel: &str) {
    #[cfg(windows)]
    if let Some(e) = tauri::Manager::try_state::<Arc<Engine>>(app) {
        crate::winint::toast_power(app, &e.paths.tools, title, body, cancel);
    }
    #[cfg(not(windows))]
    let _ = (app, title, body, cancel);
}

/// Apagar, suspender o hibernar: solo en Windows.
pub fn power_action(mode: &str) -> Result<(), String> {
    #[cfg(windows)]
    return crate::winint::power_action(mode);
    #[cfg(not(windows))]
    {
        let _ = mode;
        Err(t!("finish.windowsOnly"))
    }
}

/// Estado de la cola: en Windows va al icono de la bandeja; en Android, a la notificación
/// permanente que mantiene viva la descarga con la pantalla apagada.
pub fn queue_status(app: &tauri::AppHandle, active: usize, text: String) {
    #[cfg(all(desktop, windows))]
    {
        let _ = active;
        if let Some(tray) = app.tray_by_id("main") {
            let _ = tray.set_tooltip(Some(text));
        }
    }
    #[cfg(target_os = "android")]
    {
        let _ = app;
        crate::android::keep_alive(active > 0, &text);
    }
    #[cfg(not(any(windows, target_os = "android")))]
    let _ = (app, active, text);
}

pub fn start_clipboard_monitor(engine: Arc<Engine>) {
    // En Android el portapapeles solo se puede leer con la app en primer plano: lo revisa la
    // interfaz al volver a ella (comando `android_resume`).
    #[cfg(windows)]
    crate::winint::start_clipboard_monitor(engine);
    #[cfg(not(windows))]
    let _ = engine;
}

#[cfg(windows)]
pub fn disk_free(path: &Path) -> Option<u64> {
    crate::winint::disk_free(path)
}

#[cfg(unix)]
pub fn disk_free(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let mut p = path.to_path_buf();
    while !p.exists() {
        p = p.parent()?.to_path_buf();
    }
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    Some(st.f_bavail as u64 * st.f_frsize as u64)
}

#[cfg(not(windows))]
pub fn open_path(path: &str) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return crate::android::open_path(path);
    #[cfg(not(target_os = "android"))]
    std::process::Command::new("xdg-open").arg(path).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(not(windows))]
pub fn reveal_path(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    match p.parent() {
        Some(parent) if p.is_file() => open_path(&parent.to_string_lossy()),
        _ => open_path(path),
    }
}

pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    return std::process::Command::new("explorer").arg(url).spawn().map(|_| ()).map_err(|e| e.to_string());
    #[cfg(target_os = "android")]
    return crate::android::open_url(url);
    #[cfg(not(any(windows, target_os = "android")))]
    std::process::Command::new("xdg-open").arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

// Asociación de .nzb e inicio con el sistema: solo existen en Windows. En Android el APK ya
// declara que abre archivos .nzb y no hay "iniciar con el sistema".
#[cfg(not(windows))]
pub fn set_association(_enable: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub fn set_autostart(_enable: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub fn refresh_registrations(_associate: bool, _autostart: bool) {}

#[cfg(not(windows))]
pub fn register_app_identity(_tools_dir: &Path) {}
