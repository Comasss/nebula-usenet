//! Integración con Linux y macOS: notificaciones, inicio con el sistema, abrir .nzb con Nebula,
//! apagar o suspender al terminar y vigilar el portapapeles. Lo equivalente en Windows está en
//! `winint.rs`.
use crate::engine::{Engine, OpenItem};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;

pub fn notify(app: &tauri::AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body(body).show();
}

/// El ejecutable que hay que lanzar: en una AppImage, la propia AppImage (el binario de dentro
/// vive en un montaje temporal que cambia en cada arranque).
fn launcher() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("APPIMAGE") {
        return Ok(PathBuf::from(p));
    }
    std::env::current_exe().map_err(|e| e.to_string())
}

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| "HOME".to_string())
}

/// Entre comillas para la línea Exec de un .desktop.
#[cfg(target_os = "linux")]
fn desktop_quote(p: &std::path::Path) -> String {
    format!("\"{}\"", p.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"").replace('`', "\\`").replace('$', "\\$"))
}

#[cfg(target_os = "linux")]
fn config_dir() -> Result<PathBuf, String> {
    match std::env::var_os("XDG_CONFIG_HOME") {
        Some(d) if !d.is_empty() => Ok(PathBuf::from(d)),
        _ => Ok(home()?.join(".config")),
    }
}

#[cfg(target_os = "linux")]
fn data_dir() -> Result<PathBuf, String> {
    match std::env::var_os("XDG_DATA_HOME") {
        Some(d) if !d.is_empty() => Ok(PathBuf::from(d)),
        _ => Ok(home()?.join(".local/share")),
    }
}

// ---- Inicio con el sistema ----

#[cfg(target_os = "linux")]
pub fn set_autostart(enable: bool) -> Result<(), String> {
    let file = config_dir()?.join("autostart").join("nebula.desktop");
    if !enable {
        let _ = std::fs::remove_file(&file);
        return Ok(());
    }
    let exe = launcher()?;
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Nebula\nExec={} --minimized\nIcon=nebula\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        desktop_quote(&exe)
    );
    std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&file, entry).map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
pub fn set_autostart(enable: bool) -> Result<(), String> {
    let file = home()?.join("Library/LaunchAgents/com.nebula.downloader.plist");
    if !enable {
        let _ = std::fs::remove_file(&file);
        return Ok(());
    }
    let exe = launcher()?;
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.nebula.downloader</string>
  <key>ProgramArguments</key>
  <array><string>{}</string><string>--minimized</string></array>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#,
        esc(&exe.to_string_lossy())
    );
    std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&file, plist).map_err(|e| e.to_string())
}

// ---- Abrir .nzb con Nebula ----

/// Linux: un .desktop propio en ~/.local/share/applications (vale para la AppImage y el .deb), el
/// tipo MIME `application/x-nzb` por si el sistema no lo conoce, y Nebula como programa por
/// defecto para ese tipo.
#[cfg(target_os = "linux")]
pub fn set_association(enable: bool) -> Result<(), String> {
    let data = data_dir()?;
    let desktop = data.join("applications").join("nebula-nzb.desktop");
    if !enable {
        let _ = std::fs::remove_file(&desktop);
        let _ = std::process::Command::new("update-desktop-database").arg(data.join("applications")).status();
        return Ok(());
    }
    let exe = launcher()?;
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Nebula\nComment=Usenet downloader\nExec={} %F\nIcon=nebula\nTerminal=false\nNoDisplay=true\nMimeType=application/x-nzb;\nCategories=Network;FileTransfer;\n",
        desktop_quote(&exe)
    );
    std::fs::create_dir_all(desktop.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&desktop, entry).map_err(|e| e.to_string())?;
    let mime = data.join("mime").join("packages").join("nebula-nzb.xml");
    std::fs::create_dir_all(mime.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(
        &mime,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-nzb">
    <comment>NZB file</comment>
    <glob pattern="*.nzb"/>
  </mime-type>
</mime-info>
"#,
    )
    .map_err(|e| e.to_string())?;
    // Lo que falle aquí (herramientas que no están) no impide el resto.
    let _ = std::process::Command::new("update-mime-database").arg(data.join("mime")).status();
    let _ = std::process::Command::new("update-desktop-database").arg(data.join("applications")).status();
    let _ = std::process::Command::new("xdg-mime").args(["default", "nebula-nzb.desktop", "application/x-nzb"]).status();
    Ok(())
}

/// macOS: la app ya declara los .nzb en su Info.plist; Finder decide el programa por defecto.
#[cfg(target_os = "macos")]
pub fn set_association(_enable: bool) -> Result<(), String> {
    Ok(())
}

/// Al arrancar se vuelven a escribir, por si la AppImage o la app se han movido de sitio.
pub fn refresh_registrations(associate: bool, autostart: bool) {
    if associate {
        let _ = set_association(true);
    }
    if autostart {
        let _ = set_autostart(true);
    }
}

// ---- Apagar, suspender, hibernar ----

fn run(cmd: &str, args: &[&str]) -> Result<(), String> {
    let status = std::process::Command::new(cmd).args(args).status().map_err(|e| format!("{}: {}", cmd, e))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} {}", cmd, status))
    }
}

#[cfg(target_os = "linux")]
pub fn power_action(mode: &str) -> Result<(), String> {
    match mode {
        "sleep" => run("systemctl", &["suspend"]),
        "hibernate" => run("systemctl", &["hibernate"]),
        _ => run("systemctl", &["poweroff"]),
    }
}

#[cfg(target_os = "macos")]
pub fn power_action(mode: &str) -> Result<(), String> {
    match mode {
        // macOS decide por su cuenta si hiberna al dormir.
        "sleep" | "hibernate" => run("pmset", &["sleepnow"]),
        _ => run("osascript", &["-e", "tell application \"System Events\" to shut down"]),
    }
}

// ---- Abrir archivos y enlaces ----

#[cfg(target_os = "macos")]
const OPENER: &str = "open";
#[cfg(target_os = "linux")]
const OPENER: &str = "xdg-open";

pub fn open_path(path: &str) -> Result<(), String> {
    std::process::Command::new(OPENER).arg(path).spawn().map(|_| ()).map_err(|e| e.to_string())
}

pub fn reveal_path(path: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    if std::path::Path::new(path).exists() {
        return std::process::Command::new("open").args(["-R", path]).spawn().map(|_| ()).map_err(|e| e.to_string());
    }
    let p = std::path::Path::new(path);
    match p.parent() {
        Some(parent) if p.is_file() => open_path(&parent.to_string_lossy()),
        _ => open_path(path),
    }
}

pub fn open_url(url: &str) -> Result<(), String> {
    open_path(url)
}

// ---- Portapapeles ----

/// Mira el portapapeles cada segundo y avisa de los enlaces NZB nuevos, como en Windows.
pub fn start_clipboard_monitor(app: tauri::AppHandle, engine: Arc<Engine>) {
    std::thread::Builder::new()
        .name("clipboard".into())
        .spawn(move || {
            let mut clip: Option<arboard::Clipboard> = None;
            let mut last = String::new();
            let mut seen: Vec<String> = Vec::new();
            let mut first = true;
            loop {
                std::thread::sleep(Duration::from_millis(1000));
                let (enabled, patterns, show) = {
                    let s = engine.settings.read();
                    (s.clipboard_monitor, s.clipboard_patterns.clone(), s.clipboard_show_window)
                };
                if !enabled {
                    clip = None;
                    continue;
                }
                if clip.is_none() {
                    clip = arboard::Clipboard::new().ok();
                }
                let Some(text) = clip.as_mut().and_then(|c| c.get_text().ok()) else { continue };
                if text == last {
                    continue;
                }
                last = text;
                // Lo que ya estaba al arrancar no cuenta.
                if std::mem::take(&mut first) {
                    continue;
                }
                let urls: Vec<String> = crate::util::detect_nzb_urls(&last, &patterns).into_iter().filter(|u| !seen.contains(u)).collect();
                if urls.is_empty() {
                    continue;
                }
                seen.extend(urls.iter().cloned());
                if seen.len() > 200 {
                    seen.drain(..100);
                }
                linfo!("{}", t!("log.win.clipboard", n = urls.len()));
                let visible = app.get_webview_window("main").and_then(|w| w.is_visible().ok()).unwrap_or(false);
                if !visible {
                    if show {
                        engine.show_main();
                    } else {
                        engine.notify_os(&t!("notify.clipboardTitle"), &t!("notify.clipboardBody"));
                    }
                }
                if engine.ui_ready.load(Ordering::SeqCst) {
                    nebula_core::bus::emit(nebula_core::Event::ClipboardNzb(urls));
                } else {
                    engine.open_items(urls.into_iter().map(|u| OpenItem { kind: "clipboard".into(), value: u }).collect());
                }
            }
        })
        .ok();
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(desktop_quote(std::path::Path::new("/opt/Nebula.AppImage")), "\"/opt/Nebula.AppImage\"");
        assert_eq!(desktop_quote(std::path::Path::new("/home/a b/$x\"`")), "\"/home/a b/\\$x\\\"\\`\"");
    }

    #[test]
    fn autostart_and_association() {
        let dir = std::env::temp_dir().join(format!("nebula-unixint-{}", std::process::id()));
        std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
        std::env::set_var("XDG_DATA_HOME", dir.join("data"));
        std::env::set_var("APPIMAGE", "/home/u/Apps/Nebula x.AppImage");

        set_autostart(true).unwrap();
        let auto = dir.join("config/autostart/nebula.desktop");
        let text = std::fs::read_to_string(&auto).unwrap();
        assert!(text.contains("Exec=\"/home/u/Apps/Nebula x.AppImage\" --minimized\n"), "{text}");
        set_autostart(false).unwrap();
        assert!(!auto.exists());

        set_association(true).unwrap();
        let desktop = std::fs::read_to_string(dir.join("data/applications/nebula-nzb.desktop")).unwrap();
        assert!(desktop.contains("MimeType=application/x-nzb;"));
        assert!(desktop.contains("Exec=\"/home/u/Apps/Nebula x.AppImage\" %F"));
        assert!(dir.join("data/mime/packages/nebula-nzb.xml").exists());
        set_association(false).unwrap();
        assert!(!dir.join("data/applications/nebula-nzb.desktop").exists());

        std::env::remove_var("APPIMAGE");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
