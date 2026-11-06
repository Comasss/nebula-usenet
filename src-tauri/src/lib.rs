//! App de escritorio (Windows) y Android de Nebula: la interfaz Tauri sobre el motor
//! `nebula-core`. Aquí solo va lo que necesita Tauri: ventanas, bandeja, comandos, eventos y la
//! integración con cada sistema.
#[macro_use]
extern crate nebula_core;

#[cfg(target_os = "android")]
mod android;
mod commands;
mod os;
#[cfg(windows)]
mod tools;
mod updater;
#[cfg(windows)]
mod winint;

// Los módulos del motor, con las mismas rutas que cuando vivían en este crate.
pub(crate) use nebula_core::{config, engine, finish, i18n, logger, remote, rss, transfer, util};
#[allow(unused_imports)]
pub(crate) use nebula_core::{lerror, linfo, lwarn};

use engine::{Engine, OpenItem};
use std::sync::Arc;
#[cfg(desktop)]
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
#[cfg(desktop)]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
#[cfg(desktop)]
use tauri::Wry;
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};

#[cfg(desktop)]
struct TrayItems {
    show: MenuItem<Wry>,
    pause: MenuItem<Wry>,
    /// Apagar al vaciarse la cola (solo esta vez), o cancelar la cuenta atrás.
    power: CheckMenuItem<Wry>,
    quit: MenuItem<Wry>,
}

/// Texto y marca del apagado rápido en la bandeja: (texto, activo).
#[cfg(desktop)]
fn power_item(app: &AppHandle) -> (String, bool) {
    let Some(e) = app.try_state::<Arc<Engine>>() else { return (t!("tray.power.shutdown"), false) };
    if finish::power_status(&e.finish).is_some() {
        return (t!("tray.powerCancel"), true);
    }
    let s = e.settings.read();
    let armed = finish::armed(&s);
    let mode = armed
        .or_else(|| s.finish_actions.iter().find(|a| a.kind == "power"))
        .map(|a| a.power.clone())
        .unwrap_or_default();
    let text = match mode.as_str() {
        "sleep" => t!("tray.power.sleep"),
        "hibernate" => t!("tray.power.hibernate"),
        _ => t!("tray.power.shutdown"),
    };
    (text, armed.is_some())
}

/// Actualiza el apagado rápido de la bandeja (tras cambiar los ajustes o la cuenta atrás).
pub fn refresh_tray_power(app: &AppHandle) {
    #[cfg(mobile)]
    let _ = app;
    #[cfg(desktop)]
    if let Some(items) = app.try_state::<TrayItems>() {
        let (text, on) = power_item(app);
        let _ = items.power.set_text(text);
        let _ = items.power.set_checked(on);
    }
}

#[cfg(desktop)]
fn pause_label(paused: bool) -> String {
    if paused {
        t!("tray.resume")
    } else {
        t!("tray.pause")
    }
}

pub fn update_tray_pause(app: &AppHandle, paused: bool) {
    #[cfg(mobile)]
    let _ = (app, paused);
    #[cfg(desktop)]
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.pause.set_text(pause_label(paused));
    }
}

/// Vuelve a poner los textos del menú de la bandeja (tras cambiar de idioma).
pub fn refresh_tray_texts(app: &AppHandle, paused: bool) {
    #[cfg(mobile)]
    let _ = (app, paused);
    #[cfg(desktop)]
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.show.set_text(t!("tray.show"));
        let _ = items.pause.set_text(pause_label(paused));
        let _ = items.quit.set_text(t!("tray.quit"));
    }
    refresh_tray_power(app);
}

/// Convierte argumentos de línea de comandos en elementos a abrir (.nzb o enlaces).
fn parse_args(args: &[String], cwd: Option<&str>) -> Vec<OpenItem> {
    let mut out = Vec::new();
    for a in args {
        let a = a.trim().trim_matches('"');
        if a.is_empty() || a.starts_with("--") {
            continue;
        }
        let lower = a.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            out.push(OpenItem { kind: "url".into(), value: a.to_string() });
        } else if lower.ends_with(".nzb") || lower.ends_with(".nzb.gz") {
            let mut p = std::path::PathBuf::from(a);
            if p.is_relative() {
                if let Some(c) = cwd {
                    p = std::path::Path::new(c).join(p);
                }
            }
            if p.exists() {
                out.push(OpenItem { kind: "file".into(), value: p.to_string_lossy().to_string() });
            }
        }
    }
    out
}

#[cfg(desktop)]
fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", t!("tray.show"), true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", pause_label(false), true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let power = CheckMenuItem::with_id(app, "power", t!("tray.power.shutdown"), true, false, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", t!("tray.quit"), true, None::<&str>)?;
    // Apagar, suspender e hibernar solo existen en Windows.
    let menu = if cfg!(windows) {
        Menu::with_items(app, &[&show, &pause, &sep, &power, &sep2, &quit])?
    } else {
        Menu::with_items(app, &[&show, &pause, &sep, &quit])?
    };
    app.manage(TrayItems { show, pause, power, quit });
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Nebula")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(e) = app.try_state::<Arc<Engine>>() {
                    e.show_main();
                }
            }
            "pause" => {
                if let Some(e) = app.try_state::<Arc<Engine>>() {
                    let paused = e.st.lock().paused;
                    e.set_paused(!paused);
                }
            }
            "power" => {
                if let Some(e) = app.try_state::<Arc<Engine>>() {
                    if !finish::cancel_power(&e) {
                        let mode = {
                            let s = e.settings.read();
                            match finish::armed(&s) {
                                Some(_) => None,
                                None => Some(s.finish_actions.iter().find(|a| a.kind == "power").map(|a| a.power.clone()).unwrap_or_else(|| "shutdown".into())),
                            }
                        };
                        finish::quick_power(&e, mode.as_deref());
                    }
                }
                refresh_tray_power(app);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                if let Some(e) = tray.app_handle().try_state::<Arc<Engine>>() {
                    e.show_main();
                }
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Reenvía los eventos del motor a la interfaz con el mismo nombre y los mismos datos.
fn forward_events(app: AppHandle) {
    use tauri::Emitter;
    use tokio::sync::broadcast::error::RecvError;
    let mut rx = nebula_core::bus::subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let _ = app.emit(ev.name(), ev.payload());
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    });
}

/// Carpetas, registro, ajustes, idioma y herramientas: lo que hay que preparar antes del motor.
fn boot(paths: config::AppPaths) -> (config::AppPaths, config::Settings) {
    logger::init(&paths.logs);
    let settings = config::Settings::load(&paths);
    if settings.language.is_empty() || !i18n::set(&settings.language) {
        i18n::set(i18n::system_language());
    }
    #[cfg(windows)]
    if let Err(err) = tools::ensure(&paths.tools) {
        lerror!("{}", err);
    }
    os::register_app_identity(&paths.tools);
    (paths, settings)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Tras una actualización portable, la versión anterior tiene que cerrarse antes de seguir.
    updater::wait_previous();
    // En Android la carpeta de datos la da el sistema y solo se conoce ya dentro de `setup`.
    #[cfg(not(target_os = "android"))]
    let mut early = Some(boot(config::AppPaths::init()));
    #[cfg(target_os = "android")]
    let mut early: Option<(config::AppPaths, config::Settings)> = None;

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();
    // Una carpeta de datos propia (NEBULA_DATA_DIR) implica una instancia independiente.
    #[cfg(desktop)]
    if std::env::var_os("NEBULA_DATA_DIR").is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            let items = parse_args(argv.get(1..).unwrap_or(&[]), Some(&cwd));
            if let Some(e) = app.try_state::<Arc<Engine>>() {
                e.show_main();
                e.open_items(items);
            }
        }));
    }
    #[cfg(target_os = "android")]
    {
        builder = builder.plugin(android::plugin());
    }
    #[cfg(mobile)]
    {
        builder = builder.plugin(tauri_plugin_barcode_scanner::init());
    }
    let app = builder
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            #[cfg(target_os = "android")]
            if early.is_none() {
                let data = app.path().app_data_dir()?;
                android::prepare(&data);
                early = Some(boot(config::AppPaths::with(data, android::native_lib_dir())));
            }
            let (paths, settings) = early.take().expect("arranque");
            // El motor lanza sus tareas en el runtime de Tauri y avisa por su bus, que se
            // reenvía tal cual como eventos de Tauri.
            nebula_core::rt::set_handle(tauri::async_runtime::handle().inner().clone());
            forward_events(app.handle().clone());
            updater::cleanup(&paths.data);
            remote::init(&paths.data);
            #[cfg(desktop)]
            build_tray(app)?;
            os::refresh_registrations(settings.associate_nzb, settings.start_with_windows);
            let platform = os::TauriPlatform { app: app.handle().clone(), tools: paths.tools.clone() };
            let engine = Engine::new(Arc::new(platform), paths.clone(), settings.clone());
            app.manage(engine.clone());
            refresh_tray_power(app.handle());
            engine.start();
            rss::start(engine.clone());
            os::start_clipboard_monitor(app.handle().clone(), engine.clone());
            let args: Vec<String> = std::env::args().skip(1).collect();
            engine.open_items(parse_args(&args, None));
            // Si la interfaz no llega a avisar de que está lista, se muestra la ventana igualmente.
            let e2 = engine.clone();
            let minimized = args.iter().any(|a| a == "--minimized") || settings.start_minimized;
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(6)).await;
                if !minimized && !e2.ui_ready.load(std::sync::atomic::Ordering::SeqCst) {
                    lerror!("{}", t!("log.uiTimeout"));
                    e2.show_main();
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() != "main" {
                    return;
                }
                if let Some(e) = window.try_state::<Arc<Engine>>() {
                    if e.settings.read().close_to_tray {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::test_server,
            commands::inspect_nzb_file,
            commands::inspect_nzb_url,
            commands::discard_pending,
            commands::add_job,
            commands::get_queue,
            commands::get_job_files,
            commands::pause_job,
            commands::resume_job,
            commands::remove_job,
            commands::move_job,
            commands::set_job_priority,
            commands::set_job_category,
            commands::set_paused,
            commands::set_speed_limit,
            commands::reset_quota,
            commands::get_history,
            commands::get_stats,
            commands::get_indexer_stats,
            commands::remove_history,
            commands::clear_history,
            commands::retry_history,
            commands::retry_extract,
            commands::open_path,
            commands::reveal_path,
            commands::get_logs,
            commands::clear_logs,
            commands::disk_free,
            commands::default_patterns,
            commands::detect_nzb_urls,
            commands::app_info,
            commands::app_ready,
            commands::window_minimize,
            commands::window_close,
            commands::quit_app,
            commands::search_indexer,
            commands::test_indexer,
            commands::indexer_discover,
            commands::import_preview,
            commands::import_apply,
            commands::save_nzb_url,
            commands::save_pending_nzb,
            commands::open_url,
            commands::test_debrid,
            commands::debrid_cached,
            commands::set_language,
            commands::app_resumed,
            commands::request_storage_access,
            commands::import_nzb,
            commands::list_dirs,
            commands::make_dir,
            commands::app_background,
            commands::haptic,
            commands::api_status,
            commands::update_check,
            commands::update_download,
            commands::update_cancel,
            commands::update_install,
            commands::transfer_export,
            commands::transfer_preview,
            commands::transfer_import,
            commands::transfer_receive_start,
            commands::transfer_receive_stop,
            commands::transfer_send,
            commands::remote_status,
            commands::remote_devices,
            commands::remote_revoke,
            commands::remote_pair_answer,
            commands::remote_discover,
            commands::remote_pcs,
            commands::remote_pair,
            commands::remote_pair_cancel,
            commands::remote_forget,
            commands::remote_connect,
            commands::remote_disconnect,
            commands::remote_session,
            commands::remote_wake,
            commands::remote_call,
            commands::remote_send,
            commands::rss_status,
            commands::rss_check_now,
            commands::rss_preview,
            commands::rss_clear_log,
            commands::finish_test,
            commands::power_status,
            commands::power_cancel,
            commands::power_quick,
        ])
        .build(tauri::generate_context!())
        .expect("no se pudo iniciar Nebula");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            if let Some(e) = app.try_state::<Arc<Engine>>() {
                e.shutdown();
            }
        }
    });
}
