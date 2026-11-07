//! Acceso a la interfaz web: un usuario con contraseña, creado en el primer arranque.
//!
//! `auth.json` (en la carpeta de datos, 600) guarda el usuario, el hash argon2 de la contraseña y
//! la clave con la que se firman las cookies de sesión. La cookie lleva el usuario y su caducidad
//! firmados con HMAC-SHA256, así que sobrevive a los reinicios sin guardar sesiones; cambiar la
//! contraseña cambia la clave y cierra todas las sesiones.
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::path::{Path, PathBuf};

pub const COOKIE: &str = "nebula_session";
/// Duración de una sesión: 30 días.
pub const SESSION_SECS: u64 = 30 * 24 * 3600;
pub const MIN_PASSWORD: usize = 8;

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default, rename_all = "camelCase")]
struct Stored {
    username: String,
    password_hash: String,
    cookie_secret: String,
}

pub struct Auth {
    path: PathBuf,
    stored: RwLock<Stored>,
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
}

fn now() -> u64 {
    nebula_core::util::now_secs()
}

fn random_secret() -> String {
    let mut v = uuid::Uuid::new_v4().as_bytes().to_vec();
    v.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    b64().encode(v)
}

fn salt() -> SaltString {
    SaltString::encode_b64(uuid::Uuid::new_v4().as_bytes()).expect("salt")
}

impl Auth {
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join("auth.json");
        let stored = std::fs::read(&path).ok().and_then(|v| serde_json::from_slice(&v).ok()).unwrap_or_default();
        Auth { path, stored: RwLock::new(stored) }
    }

    /// Falta crear el usuario (primer arranque).
    pub fn needs_setup(&self) -> bool {
        let s = self.stored.read();
        s.username.is_empty() || s.password_hash.is_empty()
    }

    pub fn username(&self) -> String {
        self.stored.read().username.clone()
    }

    fn save(&self, s: &Stored) -> Result<(), String> {
        let data = serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("json.tmp");
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        use std::io::Write;
        let mut f = opts.open(&tmp).map_err(|e| e.to_string())?;
        f.write_all(&data).map_err(|e| e.to_string())?;
        drop(f);
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    /// Crea (o cambia) el usuario y la contraseña. Cierra las sesiones que hubiera.
    pub fn set(&self, username: &str, password: &str) -> Result<(), String> {
        let username = username.trim();
        if username.is_empty() {
            return Err(t!("server.auth.noUser"));
        }
        if password.chars().count() < MIN_PASSWORD {
            return Err(t!("server.auth.short", n = MIN_PASSWORD));
        }
        let hash = Argon2::default().hash_password(password.as_bytes(), &salt()).map_err(|e| e.to_string())?.to_string();
        let next = Stored { username: username.to_string(), password_hash: hash, cookie_secret: random_secret() };
        self.save(&next)?;
        *self.stored.write() = next;
        Ok(())
    }

    /// Comprueba usuario y contraseña (el usuario sin distinguir mayúsculas).
    pub fn check(&self, username: &str, password: &str) -> bool {
        let s = self.stored.read().clone();
        if s.password_hash.is_empty() || !s.username.eq_ignore_ascii_case(username.trim()) {
            return false;
        }
        let Ok(hash) = PasswordHash::new(&s.password_hash) else { return false };
        Argon2::default().verify_password(password.as_bytes(), &hash).is_ok()
    }

    fn mac(&self, msg: &[u8]) -> Option<Vec<u8>> {
        let secret = self.stored.read().cookie_secret.clone();
        if secret.is_empty() {
            return None;
        }
        let mut m = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).ok()?;
        m.update(msg);
        Some(m.finalize().into_bytes().to_vec())
    }

    /// Valor de la cookie de una sesión nueva.
    pub fn issue(&self) -> Option<String> {
        let msg = format!("{}|{}", self.username(), now() + SESSION_SECS);
        let sig = self.mac(msg.as_bytes())?;
        Some(format!("{}.{}", b64().encode(msg), b64().encode(sig)))
    }

    /// Si la cookie es de una sesión válida (firmada con la clave actual y sin caducar).
    pub fn verify(&self, cookie: &str) -> bool {
        let Some((msg, sig)) = cookie.split_once('.') else { return false };
        let (Ok(msg), Ok(sig)) = (b64().decode(msg), b64().decode(sig)) else { return false };
        let secret = self.stored.read().cookie_secret.clone();
        if secret.is_empty() {
            return false;
        }
        let Ok(mut m) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else { return false };
        m.update(&msg);
        if m.verify_slice(&sig).is_err() {
            return false;
        }
        let text = String::from_utf8_lossy(&msg);
        let Some((user, exp)) = text.rsplit_once('|') else { return false };
        user.eq_ignore_ascii_case(&self.username()) && exp.parse::<u64>().is_ok_and(|e| e > now())
    }
}

/// Valor de la cookie `name` en la cabecera Cookie.
pub fn cookie_value<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').map(str::trim).find_map(|p| p.strip_prefix(name)?.strip_prefix('='))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("nebula-auth-{}-{}", std::process::id(), uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn setup_login_and_sessions() {
        let dir = temp();
        let a = Auth::load(&dir);
        assert!(a.needs_setup());
        assert!(a.set("admin", "corta").is_err());
        a.set("admin", "una contraseña larga").unwrap();
        assert!(!a.needs_setup());
        assert!(a.check("Admin", "una contraseña larga"));
        assert!(!a.check("admin", "otra"));
        let c = a.issue().unwrap();
        assert!(a.verify(&c));
        assert!(!a.verify(&c.replace('.', ".x")));
        // Se conserva al reiniciar…
        let b = Auth::load(&dir);
        assert!(b.verify(&c) && b.check("admin", "una contraseña larga"));
        // …y cambiar la contraseña cierra las sesiones.
        b.set("admin", "la nueva contraseña").unwrap();
        assert!(!b.verify(&c));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join("auth.json")).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cookies() {
        assert_eq!(cookie_value("a=1; nebula_session=xyz; b=2", COOKIE), Some("xyz"));
        assert_eq!(cookie_value("nebula_sessionx=1", COOKIE), None);
    }
}
