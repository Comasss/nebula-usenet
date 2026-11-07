//! `nebula get`: descarga un NZB sin servidor. Arranca el motor dentro del programa con una
//! carpeta de datos temporal, lo descarga (con reparación y extracción) y sale con un código
//! que dice cómo fue.
use crate::{Fail, Res, EXIT_DOWNLOAD, EXIT_ERROR, EXIT_EXTRACT, EXIT_INTERRUPTED, EXIT_REPAIR};
use indicatif::{ProgressBar, ProgressStyle};
use nebula_core::config::{AppPaths, ServerConfig, Settings};
use nebula_core::engine::{AddRequest, Engine, JobOptions};
use nebula_core::{logger, Headless};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// `nntps://usuario:contraseña@host:563?conns=20&verify=0` → servidor. `nntp://` sin cifrar.
pub fn parse_server(s: &str, priority: u32) -> Result<ServerConfig, String> {
    let bad = || t!("cli.badServer", value = s);
    let s = if s.contains("://") { s.to_string() } else { format!("nntps://{}", s) };
    let u = reqwest::Url::parse(&s).map_err(|_| bad())?;
    let ssl = match u.scheme() {
        "nntps" | "snews" | "ssl" => true,
        "nntp" | "news" => false,
        _ => return Err(bad()),
    };
    let host = u.host_str().filter(|h| !h.is_empty()).ok_or_else(bad)?.to_string();
    let mut srv = ServerConfig {
        name: host.clone(),
        host,
        port: u.port().unwrap_or(if ssl { 563 } else { 119 }),
        ssl,
        username: nebula_core::util::percent_decode(u.username()),
        password: nebula_core::util::percent_decode(u.password().unwrap_or("")),
        priority,
        enabled: true,
        ..Default::default()
    };
    for (k, v) in u.query_pairs() {
        match k.as_ref() {
            "conns" | "connections" => srv.connections = v.parse().map_err(|_| bad())?,
            "verify" => srv.verify_cert = !matches!(v.as_ref(), "0" | "false" | "no"),
            "name" => srv.name = v.to_string(),
            _ => {}
        }
    }
    Ok(srv)
}

/// Carpetas de datos de Nebula en esta máquina (la variable, la app de escritorio y el servidor).
fn local_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = std::env::var_os("NEBULA_DATA_DIR") {
        v.push(PathBuf::from(d));
    }
    if let Some(c) = dirs::config_dir() {
        v.push(c.join("Nebula"));
        v.push(c.join(if cfg!(windows) { "Nebula Server" } else { "nebula" }));
    }
    v.push(PathBuf::from("/config"));
    v
}

/// par2 y 7-Zip que dejó la app de escritorio, si existen (en Windows van con la app).
fn desktop_tool(names: &[&str]) -> Option<String> {
    let dir = dirs::data_local_dir()?.join("Nebula").join("tools").join(env!("CARGO_PKG_VERSION"));
    names.iter().map(|n| dir.join(n)).find(|p| p.is_file()).map(|p| p.to_string_lossy().to_string())
}

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn exit_for(stage: &str) -> i32 {
    match stage {
        "download" | "debrid" => EXIT_DOWNLOAD,
        "verify" | "repair" => EXIT_REPAIR,
        "extract" => EXIT_EXTRACT,
        _ => EXIT_ERROR,
    }
}

pub async fn run(nzb: &Path, output: &Path, servers: &[String], password: Option<String>, no_pp: bool, verbose: bool, json: bool) -> Res {
    if !nzb.is_file() {
        return Err(Fail::new(t!("cli.readFailed", path = nzb.display(), error = std::io::Error::from(std::io::ErrorKind::NotFound))));
    }
    // Servidores: los de --server o los del Nebula de esta máquina.
    let local = crate::config::local_settings(&local_dirs());
    let servers: Vec<ServerConfig> = if servers.is_empty() {
        local.as_ref().map(|s| s.servers.iter().filter(|s| s.enabled).cloned().collect()).unwrap_or_default()
    } else {
        servers.iter().enumerate().map(|(i, s)| parse_server(s, i as u32)).collect::<Result<_, _>>()?
    };
    if servers.is_empty() {
        return Err(Fail::new(t!("cli.get.noServers")));
    }
    std::fs::create_dir_all(output).map_err(|e| Fail::new(t!("cli.writeFailed", path = output.display(), error = e)))?;
    let output = std::fs::canonicalize(output).unwrap_or_else(|_| output.to_path_buf());

    let tmp = TempDir(std::env::temp_dir().join(format!("nebula-get-{}", uuid::Uuid::new_v4())));
    let paths = AppPaths::with(tmp.0.clone(), tmp.0.join("tools"));
    logger::init(&paths.logs);
    logger::echo(verbose && !json);

    let mut settings = Settings {
        servers,
        download_dir: output.to_string_lossy().to_string(),
        ask_destination: false,
        features_chosen: true,
        first_run_done: true,
        feature_nntp: true,
        update_check: false,
        notify_complete: false,
        notify_failed: false,
        language: nebula_core::i18n::system_language().to_string(),
        ..Default::default()
    };
    if let Some(l) = &local {
        settings.par2_path = l.par2_path.clone();
        settings.seven_zip_path = l.seven_zip_path.clone();
    }
    if settings.par2_path.is_empty() {
        settings.par2_path = desktop_tool(&["par2.exe", "par2"]).unwrap_or_default();
    }
    if settings.seven_zip_path.is_empty() {
        settings.seven_zip_path = desktop_tool(&["7z.exe", "7zz"]).unwrap_or_default();
    }

    let engine = Engine::new(Arc::new(Headless), paths, settings);
    engine.start();
    if !no_pp {
        for tl in engine.tools_status().iter().filter(|t| !t.found) {
            eprintln!("{} {}", t!("cli.warning"), t!("cli.toolMissing", tool = tl.tool));
        }
    }

    let preview = engine.inspect_nzb_file(nzb.to_string_lossy().to_string()).await.map_err(Fail::new)?;
    let mut req = AddRequest::with_defaults(&preview, false);
    req.dest_dir = output.to_string_lossy().to_string();
    if password.is_some() {
        req.password = password;
    }
    if no_pp {
        req.options = Some(JobOptions { par2: false, extract: false, delete_archives: false, delete_par2: false, create_folder: true });
    }
    let name = preview.name.clone();
    let id = engine.add_job(req).map_err(Fail::new)?;

    let bar = if json || verbose {
        ProgressBar::hidden()
    } else {
        let b = ProgressBar::new(preview.total_bytes.max(1));
        b.set_style(
            ProgressStyle::with_template("{msg:12} [{bar:32.cyan/blue}] {bytes}/{total_bytes} {binary_bytes_per_sec} {eta}")
                .unwrap()
                .progress_chars("━╸─"),
        );
        b.enable_steady_tick(Duration::from_millis(200));
        b
    };
    if !json {
        eprintln!("{}", t!("cli.get.downloading", name = name, path = output.display()));
    }

    let mut tick = tokio::time::interval(Duration::from_millis(300));
    let interrupted = loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break true,
            _ = tick.tick() => {}
        }
        let snap = engine.snapshot();
        match snap.jobs.iter().find(|j| j.id == id) {
            Some(j) => {
                bar.set_length(j.total.max(1));
                bar.set_position(j.done.min(j.total));
                let st = serde_json::to_value(j.status).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                bar.set_message(crate::fmt::status(&st));
            }
            // La foto de la cola se renueva cada medio segundo: al principio puede no estar aún.
            None if engine.history().iter().any(|h| h.id == id) => break false,
            None => {}
        }
    };
    bar.finish_and_clear();
    if interrupted {
        engine.shutdown();
        return Err(Fail::code(EXIT_INTERRUPTED, t!("cli.get.interrupted")));
    }
    let entry = engine.history().into_iter().find(|h| h.id == id);
    engine.shutdown();
    let Some(h) = entry else { return Err(Fail::new(t!("cli.get.lost"))) };
    let stage = h.failed_stage.clone().unwrap_or_default();
    if json {
        crate::fmt::print_json(&json!({
            "name": h.name,
            "status": h.status,
            "path": h.dest,
            "size": h.size,
            "seconds": h.download_secs,
            "error": h.error,
            "stage": h.failed_stage,
            "warning": h.warning,
        }));
    }
    if h.status == "completed" {
        if !json {
            if let Some(w) = &h.warning {
                eprintln!("{} {}", t!("cli.warning"), w);
            }
            println!("{}", t!("cli.get.done", path = h.dest));
        }
        Ok(())
    } else {
        let msg = h.error.unwrap_or_else(|| h.status.clone());
        Err(Fail::code(exit_for(&stage), if json { msg } else { t!("cli.get.failed", error = msg) }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servers() {
        let s = parse_server("nntps://u%40x:p%3Ass@news.example.com?conns=30&verify=0", 0).unwrap();
        assert_eq!((s.host.as_str(), s.port, s.ssl), ("news.example.com", 563, true));
        assert_eq!((s.username.as_str(), s.password.as_str(), s.connections, s.verify_cert), ("u@x", "p:ss", 30, false));
        let s = parse_server("nntp://127.0.0.1:1119", 1).unwrap();
        assert_eq!((s.port, s.ssl, s.priority), (1119, false, 1));
        assert_eq!(parse_server("news.example.com", 0).unwrap().port, 563);
        assert!(parse_server("http://x", 0).is_err());
    }

    #[test]
    fn exit_codes() {
        assert_eq!(exit_for("download"), EXIT_DOWNLOAD);
        assert_eq!(exit_for("repair"), EXIT_REPAIR);
        assert_eq!(exit_for("extract"), EXIT_EXTRACT);
    }
}
