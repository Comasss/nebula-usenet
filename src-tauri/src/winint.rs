//! Integración con Windows: asociación .nzb, inicio automático, portapapeles, espacio en disco.
use crate::engine::{Engine, OpenItem};
use crate::linfo;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

const PROGID: &str = "Nebula.nzb";

fn exe_path() -> Result<String, String> {
    std::env::current_exe().map(|p| p.to_string_lossy().to_string()).map_err(|e| e.to_string())
}

fn notify_assoc_changed() {
    use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as _, SHCNF_IDLIST, std::ptr::null(), std::ptr::null()) };
}

pub fn set_association(enable: bool) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let exe = exe_path()?;
    let e = |x: std::io::Error| x.to_string();
    if enable {
        let (ext, _) = hkcu.create_subkey("Software\\Classes\\.nzb").map_err(e)?;
        ext.set_value("", &PROGID).map_err(e)?;
        ext.set_value("Content Type", &"application/x-nzb").map_err(e)?;
        let (owp, _) = hkcu.create_subkey("Software\\Classes\\.nzb\\OpenWithProgids").map_err(e)?;
        owp.set_value(PROGID, &"").map_err(e)?;
        let (prog, _) = hkcu.create_subkey(format!("Software\\Classes\\{}", PROGID)).map_err(e)?;
        prog.set_value("", &t!("win.nzbFileType")).map_err(e)?;
        let (icon, _) = hkcu.create_subkey(format!("Software\\Classes\\{}\\DefaultIcon", PROGID)).map_err(e)?;
        icon.set_value("", &format!("\"{}\",0", exe)).map_err(e)?;
        let (cmd, _) = hkcu.create_subkey(format!("Software\\Classes\\{}\\shell\\open\\command", PROGID)).map_err(e)?;
        cmd.set_value("", &format!("\"{}\" \"%1\"", exe)).map_err(e)?;
        linfo!("{}", t!("log.win.associated"));
    } else {
        let _ = hkcu.delete_subkey_all(format!("Software\\Classes\\{}", PROGID));
        if let Ok(ext) = hkcu.open_subkey_with_flags("Software\\Classes\\.nzb", winreg::enums::KEY_ALL_ACCESS) {
            let cur: String = ext.get_value("").unwrap_or_default();
            if cur == PROGID {
                let _ = ext.delete_value("");
            }
        }
        if let Ok(owp) = hkcu.open_subkey_with_flags("Software\\Classes\\.nzb\\OpenWithProgids", winreg::enums::KEY_ALL_ACCESS) {
            let _ = owp.delete_value(PROGID);
        }
        linfo!("{}", t!("log.win.unassociated"));
    }
    notify_assoc_changed();
    Ok(())
}

/// Si la asociación / el inicio automático están activos pero apuntan a otro ejecutable
/// (se movió el portable o se instaló), se actualizan a la ruta actual.
pub fn refresh_registrations(associate: bool, autostart: bool) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(exe) = exe_path() else { return };
    if associate {
        let current: String = hkcu
            .open_subkey(format!("Software\\Classes\\{}\\shell\\open\\command", PROGID))
            .and_then(|k| k.get_value(""))
            .unwrap_or_default();
        if current != format!("\"{}\" \"%1\"", exe) {
            let _ = set_association(true);
        }
    }
    if autostart {
        let current: String = hkcu
            .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
            .and_then(|k| k.get_value("Nebula"))
            .unwrap_or_default();
        if current != format!("\"{}\" --minimized", exe) {
            let _ = set_autostart(true);
        }
    }
}

pub fn set_autostart(enable: bool) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run, _) = hkcu
        .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
        .map_err(|e| e.to_string())?;
    if enable {
        run.set_value("Nebula", &format!("\"{}\" --minimized", exe_path()?)).map_err(|e| e.to_string())?;
    } else {
        let _ = run.delete_value("Nebula");
    }
    Ok(())
}

/// Identidad de la app para Windows (notificaciones y barra de tareas). Coincide con el
/// `identifier` de tauri.conf.json, que es el que usa el acceso directo del instalador.
pub const AUMID: &str = "com.nebula.downloader";

/// Registra el nombre y el icono de Nebula para que las notificaciones no aparezcan como
/// "Windows PowerShell", incluso en la versión portable (sin instalador).
pub fn register_app_identity(tools_dir: &Path) {
    use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok((key, _)) = hkcu.create_subkey(format!("Software\\Classes\\AppUserModelId\\{}", AUMID)) {
        let _ = key.set_value("DisplayName", &"Nebula");
        let _ = key.set_value("IconUri", &tools_dir.join("nebula.png").to_string_lossy().to_string());
        let _ = key.set_value("IconBackgroundColor", &"FF12131B");
    }
    let wide: Vec<u16> = std::ffi::OsStr::new(AUMID).encode_wide_nul();
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(wide.as_ptr());
    }
}

/// Notificación nativa de Windows. Al pulsarla se muestra la ventana de Nebula.
pub fn toast(app: &tauri::AppHandle, tools_dir: &Path, title: &str, body: &str) {
    let app = app.clone();
    let icon = tools_dir.join("nebula.png");
    let (title, body) = (title.to_string(), body.to_string());
    std::thread::spawn(move || {
        let handle = app.clone();
        let mut t = tauri_winrt_notification::Toast::new(AUMID).title(&title).text1(&body).on_activated(move |_| {
            if let Some(w) = handle.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
            Ok(())
        });
        if icon.exists() {
            t = t.icon(&icon, tauri_winrt_notification::IconCrop::Square, "Nebula");
        }
        if let Err(e) = t.show() {
            crate::lwarn!("{}", t!("log.win.notifyFailed", error = format!("{:?}", e)));
        }
    });
}

/// Notificación de la cuenta atrás de apagado, con un botón para cancelarla.
pub fn toast_power(app: &tauri::AppHandle, tools_dir: &Path, title: &str, body: &str, cancel: &str) {
    let app = app.clone();
    let icon = tools_dir.join("nebula.png");
    let (title, body, cancel) = (title.to_string(), body.to_string(), cancel.to_string());
    std::thread::spawn(move || {
        let handle = app.clone();
        let mut t = tauri_winrt_notification::Toast::new(AUMID)
            .title(&title)
            .text1(&body)
            .scenario(tauri_winrt_notification::Scenario::Reminder)
            .add_button(&cancel, "cancel-power")
            .on_activated(move |action| {
                if let Some(e) = handle.try_state::<Arc<Engine>>() {
                    if action.as_deref() == Some("cancel-power") {
                        crate::finish::cancel_power(&handle, &e.finish);
                    } else if let Some(w) = handle.get_webview_window("power") {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
                Ok(())
            });
        if icon.exists() {
            t = t.icon(&icon, tauri_winrt_notification::IconCrop::Square, "Nebula");
        }
        if let Err(e) = t.show() {
            crate::lwarn!("{}", t!("log.win.notifyFailed", error = format!("{:?}", e)));
        }
    });
}

/// Apaga, suspende o hiberna el equipo.
pub fn power_action(mode: &str) -> Result<(), String> {
    match mode {
        "sleep" => {
            use windows_sys::Win32::System::Power::SetSuspendState;
            // Sin hibernar, sin forzar y con los eventos de reactivación permitidos.
            if unsafe { SetSuspendState(0, 0, 0) } == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(())
        }
        _ => {
            let arg = if mode == "hibernate" { "/h" } else { "/s" };
            let mut cmd = std::process::Command::new("shutdown.exe");
            cmd.arg(arg);
            if arg == "/s" {
                cmd.args(["/t", "0"]);
            }
            let status = cmd.creation_flags(0x0800_0000).status().map_err(|e| e.to_string())?;
            if status.success() {
                Ok(())
            } else {
                Err(format!("shutdown.exe {}", status))
            }
        }
    }
}

pub fn disk_free(path: &Path) -> Option<u64> {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut p = path.to_path_buf();
    while !p.exists() {
        p = p.parent()?.to_path_buf();
    }
    let wide: Vec<u16> = p.as_os_str().encode_wide_nul();
    let mut avail: u64 = 0;
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, std::ptr::null_mut(), std::ptr::null_mut()) };
    if ok == 0 {
        None
    } else {
        Some(avail)
    }
}

trait EncodeWideNul {
    fn encode_wide_nul(&self) -> Vec<u16>;
}
impl EncodeWideNul for std::ffi::OsStr {
    fn encode_wide_nul(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(std::iter::once(0)).collect()
    }
}

pub fn open_path(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    let target = if p.exists() {
        p.to_path_buf()
    } else {
        // Abrir el primer ancestro existente
        let mut q = p.to_path_buf();
        while !q.exists() {
            match q.parent() {
                Some(par) => q = par.to_path_buf(),
                None => return Err(t!("err.folderGone")),
            }
        }
        q
    };
    std::process::Command::new("explorer").arg(target).spawn().map(|_| ()).map_err(|e| e.to_string())
}

pub fn reveal_path(path: &str) -> Result<(), String> {
    if !Path::new(path).exists() {
        return open_path(path);
    }
    std::process::Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", path))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn start_clipboard_monitor(engine: Arc<Engine>) {
    std::thread::Builder::new()
        .name("clipboard".into())
        .spawn(move || {
            use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;
            let mut last_seq = unsafe { GetClipboardSequenceNumber() };
            let mut seen: Vec<String> = Vec::new();
            loop {
                std::thread::sleep(Duration::from_millis(700));
                let seq = unsafe { GetClipboardSequenceNumber() };
                if seq == last_seq {
                    continue;
                }
                last_seq = seq;
                let (enabled, patterns, show) = {
                    let s = engine.settings.read();
                    (s.clipboard_monitor, s.clipboard_patterns.clone(), s.clipboard_show_window)
                };
                if !enabled {
                    continue;
                }
                let Ok(text) = clipboard_win::get_clipboard_string() else { continue };
                let urls: Vec<String> = crate::util::detect_nzb_urls(&text, &patterns).into_iter().filter(|u| !seen.contains(u)).collect();
                if urls.is_empty() {
                    continue;
                }
                for u in &urls {
                    seen.push(u.clone());
                }
                if seen.len() > 200 {
                    seen.drain(..100);
                }
                linfo!("{}", t!("log.win.clipboard", n = urls.len()));
                let visible = engine
                    .app
                    .get_webview_window("main")
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false);
                if !visible {
                    if show {
                        engine.show_main();
                    } else {
                        engine.notify_os(&t!("notify.clipboardTitle"), &t!("notify.clipboardBody"));
                    }
                }
                if engine.ui_ready.load(Ordering::SeqCst) {
                    let _ = engine.app.emit("clipboard-nzb", urls);
                } else {
                    engine.open_items(urls.into_iter().map(|u| OpenItem { kind: "clipboard".into(), value: u }).collect());
                }
            }
        })
        .ok();
}
