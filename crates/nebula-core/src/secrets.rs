//! Cifrado de las contraseñas y claves que se guardan en config.json (campos `*Enc`).
//! - Windows: DPAPI, ligado al usuario de Windows.
//! - Android: base64. config.json vive en la carpeta privada de la app, a la que no llegan
//!   otras apps.
//! - Linux, macOS…: ChaCha20-Poly1305 con una clave aleatoria en `secret.key` dentro de la carpeta
//!   de datos (solo legible por su dueño, 600). Se guarda como `v1:<base64(nonce + cifrado)>`;
//!   lo guardado antes en base64 se sigue leyendo y se cifra al volver a guardar.
use base64::Engine as _;
#[cfg(not(any(windows, target_os = "android")))]
use std::path::{Path, PathBuf};
#[cfg(not(any(windows, target_os = "android")))]
use std::sync::OnceLock;

#[cfg(not(any(windows, target_os = "android")))]
static KEY_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Carpeta donde vive la clave (la de datos). Sin llamar a esto, fuera de Windows y Android se
/// guarda en base64 como antes.
pub fn init(data_dir: &std::path::Path) {
    #[cfg(not(any(windows, target_os = "android")))]
    let _ = KEY_DIR.set(data_dir.to_path_buf());
    #[cfg(any(windows, target_os = "android"))]
    let _ = data_dir;
}

// ---- DPAPI: cifra la contraseña ligada al usuario de Windows ----

#[cfg(windows)]
pub fn protect(plain: &str) -> Option<String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};
    let data = plain.as_bytes();
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let ok = CryptProtectData(
            &input,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            &mut output,
        );
        if ok == 0 {
            return None;
        }
        let v = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as _);
        Some(base64::engine::general_purpose::STANDARD.encode(v))
    }
}

#[cfg(windows)]
pub fn unprotect(enc: &str) -> Option<String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};
    let data = base64::engine::general_purpose::STANDARD.decode(enc).ok()?;
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let ok = CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            &mut output,
        );
        if ok == 0 {
            return None;
        }
        let v = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as _);
        String::from_utf8(v).ok()
    }
}

#[cfg(target_os = "android")]
pub fn protect(plain: &str) -> Option<String> {
    Some(base64::engine::general_purpose::STANDARD.encode(plain.as_bytes()))
}

#[cfg(target_os = "android")]
pub fn unprotect(enc: &str) -> Option<String> {
    let v = base64::engine::general_purpose::STANDARD.decode(enc).ok()?;
    String::from_utf8(v).ok()
}

#[cfg(not(any(windows, target_os = "android")))]
const PREFIX: &str = "v1:";

/// La clave de `secret.key`, creada (600) la primera vez.
#[cfg(not(any(windows, target_os = "android")))]
fn key() -> Option<[u8; 32]> {
    static KEY: OnceLock<Option<[u8; 32]>> = OnceLock::new();
    *KEY.get_or_init(|| load_or_create(&KEY_DIR.get()?.join("secret.key")))
}

#[cfg(not(any(windows, target_os = "android")))]
fn load_or_create(path: &Path) -> Option<[u8; 32]> {
    if let Ok(v) = std::fs::read(path) {
        return v.try_into().ok();
    }
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(uuid::Uuid::new_v4().as_bytes());
    h.update(uuid::Uuid::new_v4().as_bytes());
    let key: [u8; 32] = h.finalize().into();
    let _ = std::fs::create_dir_all(path.parent()?);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    use std::io::Write;
    match opts.open(path) {
        Ok(mut f) => {
            f.write_all(&key).ok()?;
            Some(key)
        }
        // Otro proceso la creó a la vez: se usa la suya.
        Err(_) => std::fs::read(path).ok()?.try_into().ok(),
    }
}

#[cfg(not(any(windows, target_os = "android")))]
pub fn protect(plain: &str) -> Option<String> {
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
    let b64 = base64::engine::general_purpose::STANDARD;
    let Some(k) = key() else { return Some(b64.encode(plain.as_bytes())) };
    let nonce: [u8; 12] = uuid::Uuid::new_v4().as_bytes()[..12].try_into().ok()?;
    let ct = ChaCha20Poly1305::new(Key::from_slice(&k)).encrypt(Nonce::from_slice(&nonce), plain.as_bytes()).ok()?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&ct);
    Some(format!("{}{}", PREFIX, b64.encode(out)))
}

#[cfg(not(any(windows, target_os = "android")))]
pub fn unprotect(enc: &str) -> Option<String> {
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
    let b64 = base64::engine::general_purpose::STANDARD;
    let Some(rest) = enc.strip_prefix(PREFIX) else {
        // Guardado antes en base64.
        return String::from_utf8(b64.decode(enc).ok()?).ok();
    };
    let data = b64.decode(rest).ok()?;
    if data.len() < 12 {
        return None;
    }
    let plain = ChaCha20Poly1305::new(Key::from_slice(&key()?)).decrypt(Nonce::from_slice(&data[..12]), &data[12..]).ok()?;
    String::from_utf8(plain).ok()
}

#[cfg(all(test, not(any(windows, target_os = "android"))))]
mod tests {
    use super::*;

    #[test]
    fn key_file_round_trip_and_legacy() {
        let dir = std::env::temp_dir().join(format!("nebula-secret-{}", std::process::id()));
        let path = dir.join("secret.key");
        let k = load_or_create(&path).unwrap();
        assert_eq!(load_or_create(&path), Some(k));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
        // Lo que se guardó en base64 antes de cifrar se sigue leyendo.
        assert_eq!(unprotect("c2VjcmV0").as_deref(), Some("secret"));
        assert_eq!(unprotect(&protect("pässwörd").unwrap()).as_deref(), Some("pässwörd"));
    }
}
