//! Lo que depende del sistema operativo en la app: Windows (winint.rs), Linux y macOS (unixint.rs)
//! y Android (android.rs). El motor lo usa a través de `TauriPlatform`.
use crate::engine::Engine;
use nebula_core::Platform;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

#[cfg(windows)]
pub use crate::winint::{open_path, refresh_registrations, register_app_identity, reveal_path};
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use crate::unixint::{open_path, open_url, refresh_registrations, reveal_path};

/// Lo que el motor le pide a la app: notificaciones, bandeja, ventanas y energía.
pub struct TauriPlatform {
    pub app: AppHandle,
    /// Carpeta de herramientas (ahí está el icono de las notificaciones de Windows).
    #[cfg_attr(not(windows), allow(dead_code))]
    pub tools: PathBuf,
}

impl Platform for TauriPlatform {
    fn notify(&self, title: &str, body: &str) {
        #[cfg(windows)]
        crate::winint::toast(&self.app, &self.tools, title, body);
        #[cfg(target_os = "android")]
        crate::android::notify(title, body);
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        crate::unixint::notify(&self.app, title, body);
        #[cfg(not(any(windows, target_os = "android", target_os = "linux", target_os = "macos")))]
        let _ = (title, body);
    }

    fn notify_power(&self, title: &str, body: &str, cancel: &str) {
        #[cfg(windows)]
        crate::winint::toast_power(&self.app, &self.tools, title, body, cancel);
        // Sin botón en la notificación: se cancela desde la ventana de la cuenta atrás.
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let _ = cancel;
            crate::unixint::notify(&self.app, title, body);
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        let _ = (title, body, cancel);
    }

    /// En escritorio va al icono de la bandeja; en Android, a la notificación permanente que
    /// mantiene viva la descarga con la pantalla apagada.
    fn queue_status(&self, active: usize, text: String) {
        #[cfg(desktop)]
        {
            let _ = active;
            if let Some(tray) = self.app.tray_by_id("main") {
                let _ = tray.set_tooltip(Some(text));
            }
        }
        #[cfg(target_os = "android")]
        crate::android::keep_alive(active > 0, &text);
        #[cfg(not(any(desktop, target_os = "android")))]
        let _ = (active, text);
    }

    fn tray_paused(&self, paused: bool) {
        crate::update_tray_pause(&self.app, paused);
    }

    fn refresh_power(&self) {
        crate::refresh_tray_power(&self.app);
    }

    fn show_main(&self) {
        if let Some(w) = self.app.get_webview_window("main") {
            let _ = w.show();
            #[cfg(desktop)]
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
    }

    fn power_window(&self, open: bool) {
        if open {
            open_power_window(&self.app);
        } else if let Some(w) = self.app.get_webview_window("power") {
            let _ = w.destroy();
        }
    }

    fn supports_power(&self) -> bool {
        cfg!(any(windows, target_os = "linux", target_os = "macos"))
    }

    fn power_action(&self, mode: &str) -> Result<(), String> {
        #[cfg(windows)]
        return crate::winint::power_action(mode);
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        return crate::unixint::power_action(mode);
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = mode;
            Err(t!("finish.windowsOnly"))
        }
    }

    fn set_association(&self, enable: bool) -> Result<(), String> {
        #[cfg(windows)]
        return crate::winint::set_association(enable);
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        return crate::unixint::set_association(enable);
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            // En Android el APK ya declara que abre archivos .nzb.
            let _ = enable;
            Ok(())
        }
    }

    fn set_autostart(&self, enable: bool) -> Result<(), String> {
        #[cfg(windows)]
        return crate::winint::set_autostart(enable);
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        return crate::unixint::set_autostart(enable);
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            // En Android no hay "iniciar con el sistema".
            let _ = enable;
            Ok(())
        }
    }
}

/// Ventana pequeña con la cuenta atrás de apagado.
#[cfg(desktop)]
fn open_power_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("power") {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let built = tauri::WebviewWindowBuilder::new(app, "power", tauri::WebviewUrl::App("index.html#power".into()))
        .title("Nebula")
        .inner_size(420.0, 250.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .always_on_top(true)
        .center()
        .focused(true)
        .background_color(tauri::window::Color(10, 11, 16, 255))
        .build();
    if let Err(e) = built {
        lerror!("{}", e);
    }
}

#[cfg(mobile)]
fn open_power_window(_app: &AppHandle) {}

pub fn start_clipboard_monitor(app: AppHandle, engine: Arc<Engine>) {
    // En Android el portapapeles solo se puede leer con la app en primer plano: lo revisa la
    // interfaz al volver a ella (comando `app_resumed`).
    #[cfg(windows)]
    crate::winint::start_clipboard_monitor(app, engine);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    crate::unixint::start_clipboard_monitor(app, engine);
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    let _ = (app, engine);
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn open_path(path: &str) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return crate::android::open_path(path);
    #[cfg(not(target_os = "android"))]
    std::process::Command::new("xdg-open").arg(path).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn reveal_path(path: &str) -> Result<(), String> {
    let p = std::path::Path::new(path);
    match p.parent() {
        Some(parent) if p.is_file() => open_path(&parent.to_string_lossy()),
        _ => open_path(path),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    return std::process::Command::new("explorer").arg(url).spawn().map(|_| ()).map_err(|e| e.to_string());
    #[cfg(target_os = "android")]
    return crate::android::open_url(url);
    #[cfg(not(any(windows, target_os = "android")))]
    std::process::Command::new("xdg-open").arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn refresh_registrations(_associate: bool, _autostart: bool) {}

#[cfg(not(windows))]
pub fn register_app_identity(_tools_dir: &std::path::Path) {}
