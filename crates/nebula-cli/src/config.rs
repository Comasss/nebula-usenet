//! A qué Nebula se conecta la CLI y con qué clave.
//!
//! Por orden: `--url`/`--api-key` (o NEBULA_URL/NEBULA_API_KEY), `cli.toml` en la carpeta de
//! configuración (`~/.config/nebula/cli.toml`, `%APPDATA%\nebula\cli.toml` en Windows) y, si
//! falta la clave, los ajustes del nebula-server de esta misma máquina (o de este contenedor).
use crate::client::Client;
use crate::{Cli, Fail, Res};
use nebula_core::config::{AppPaths, Settings};
use serde::Deserialize;
use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 5080;

#[derive(Deserialize, Default, Debug)]
#[serde(default, rename_all = "snake_case")]
pub struct FileConfig {
    pub url: Option<String>,
    pub api_key: Option<String>,
}

pub fn file_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("nebula").join("cli.toml")
}

fn read_file() -> Res<FileConfig> {
    let path = file_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(|e| Fail::new(t!("cli.badConfig", path = path.display(), error = e))),
        Err(_) => Ok(FileConfig::default()),
    }
}

/// Carpetas de datos donde puede estar un nebula-server local (como las elige el propio servidor).
fn server_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = std::env::var_os("NEBULA_DATA_DIR") {
        v.push(PathBuf::from(d));
    }
    v.push(PathBuf::from("/config"));
    if let Some(c) = dirs::config_dir() {
        v.push(c.join(if cfg!(windows) { "Nebula Server" } else { "nebula" }));
    }
    v
}

/// Ajustes de un Nebula local, si hay uno y se pueden leer (la clave de cifrado es del usuario
/// que corre el servidor).
pub fn local_settings(dirs: &[PathBuf]) -> Option<Settings> {
    let dir = dirs.iter().find(|d| d.join("config.json").is_file())?;
    // Sin crear nada: AppPaths::with crea subcarpetas, así que solo se usa si ya es una carpeta de Nebula.
    let paths = AppPaths::with(dir.clone(), dir.join("tools"));
    Some(Settings::load(&paths))
}

fn normalize(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    if u.starts_with("http://") || u.starts_with("https://") {
        u.to_string()
    } else {
        format!("http://{}", u)
    }
}

/// El cliente para los comandos que hablan con el servidor.
pub fn connect(cli: &Cli) -> Res<Client> {
    let file = read_file()?;
    let port = std::env::var("NEBULA_PORT").ok().and_then(|p| p.parse::<u16>().ok()).unwrap_or(DEFAULT_PORT);
    let url = cli.url.clone().or(file.url).map(|u| normalize(&u)).unwrap_or_else(|| format!("http://localhost:{}", port));
    let key = match cli.api_key.clone().or(file.api_key).filter(|k| !k.trim().is_empty()) {
        Some(k) => k,
        None => local_settings(&server_dirs())
            .map(|s| s.api_key)
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| Fail::new(t!("cli.noKey", path = file_path().display())))?,
    };
    Ok(Client::new(&url, key.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_file() {
        assert_eq!(normalize("nas:5080/"), "http://nas:5080");
        assert_eq!(normalize("https://nebula.example.com/"), "https://nebula.example.com");
        let f: FileConfig = toml::from_str("url = \"http://nas:5080\"\napi_key = \"abc\"\n").unwrap();
        assert_eq!(f.url.as_deref(), Some("http://nas:5080"));
        assert_eq!(f.api_key.as_deref(), Some("abc"));
    }
}
