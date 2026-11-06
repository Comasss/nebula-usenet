//! nebula-server: el motor de Nebula sin ventana, con la interfaz en el navegador y las APIs
//! Newznab y SABnzbd en el mismo puerto. Para servidores, NAS y Docker.
#[macro_use]
extern crate nebula_core;

mod auth;
mod http;
mod rpc;

use clap::Parser;
use nebula_core::config::{AppPaths, Settings};
use nebula_core::engine::Engine;
use nebula_core::{i18n, logger, Headless};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "nebula-server", version, about = "Nebula Usenet downloader: web interface, Newznab and SABnzbd APIs")]
struct Args {
    /// Data folder (settings, queue, history, logs). Default: /config in Docker, ~/.config/nebula elsewhere.
    #[arg(long, env = "NEBULA_DATA_DIR")]
    data_dir: Option<PathBuf>,
    /// Port for the web interface and the APIs.
    #[arg(long, env = "NEBULA_PORT", default_value_t = 5080)]
    port: u16,
    /// Address to listen on (0.0.0.0 = every network interface, 127.0.0.1 = this machine only).
    #[arg(long, env = "NEBULA_BIND", default_value = "0.0.0.0")]
    bind: String,
    /// Download folder for a new installation (ignored once the settings exist).
    #[arg(long, env = "NEBULA_DOWNLOAD_DIR")]
    download_dir: Option<PathBuf>,
}

/// /config dentro de Docker; si no, la carpeta de configuración del usuario.
fn default_data_dir() -> PathBuf {
    if std::env::var_os("NEBULA_DOCKER").is_some() || std::path::Path::new("/.dockerenv").exists() {
        return PathBuf::from("/config");
    }
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    if cfg!(windows) {
        base.join("Nebula Server")
    } else {
        base.join("nebula")
    }
}

/// Espera a Ctrl+C o a SIGTERM (docker stop, systemctl stop).
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    nebula_core::rt::set_handle(tokio::runtime::Handle::current());
    let data = args.data_dir.clone().unwrap_or_else(default_data_dir);
    // Las herramientas propias (si se ponen ahí) van en <datos>/tools; si no, las del sistema.
    let paths = AppPaths::with(data.clone(), data.join("tools"));
    logger::init(&paths.logs);
    logger::echo(true);

    let first_run = !paths.config_file().exists();
    let mut settings = Settings::load(&paths);
    if settings.language.is_empty() || !i18n::set(&settings.language) {
        i18n::set(i18n::system_language());
    }
    if first_run {
        if let Some(d) = &args.download_dir {
            settings.download_dir = d.to_string_lossy().to_string();
        }
        if let Err(e) = settings.save(&paths) {
            lerror!("{}", e);
        }
    }

    let addr: SocketAddr = match format!("{}:{}", args.bind, args.port).parse() {
        Ok(a) => a,
        Err(_) => {
            eprintln!("{}", t!("server.badAddress", address = format!("{}:{}", args.bind, args.port)));
            std::process::exit(2);
        }
    };
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{}", t!("server.bindFailed", address = addr, error = e));
            std::process::exit(1);
        }
    };

    nebula_core::remote::init(&paths.data);
    let engine = Engine::new(Arc::new(Headless), paths.clone(), settings);
    engine.start();
    nebula_core::rss::start(engine.clone());
    let auth = auth::Auth::load(&paths.data);
    linfo!("{}", t!("log.server.data", path = paths.data.display()));
    let shown = if addr.ip().is_unspecified() { format!("http://localhost:{}", addr.port()) } else { format!("http://{}", addr) };
    linfo!("{}", t!("log.server.listening", url = shown));
    if auth.needs_setup() {
        linfo!("{}", t!("log.server.setup"));
    }

    let state = Arc::new(http::AppState { engine: engine.clone(), auth });
    let app = http::router(state).into_make_service_with_connect_info::<SocketAddr>();
    // Al recibir la señal se dejan de aceptar conexiones; las que siguen abiertas (eventos de la
    // interfaz) tienen unos segundos antes de cerrarse.
    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    let serve = axum::serve(listener, app).with_graceful_shutdown(async move {
        shutdown_signal().await;
        linfo!("{}", t!("log.server.stopping"));
        let _ = stop_tx.send(true);
    });
    tokio::select! {
        r = serve => {
            if let Err(e) = r {
                lerror!("{}", e);
            }
        }
        _ = async {
            let _ = stop_rx.wait_for(|v| *v).await;
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        } => {}
    }
    // Se guarda la cola y el historial antes de salir.
    engine.shutdown();
}
