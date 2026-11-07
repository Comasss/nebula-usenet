//! Cifrado de las contraseñas y claves que se guardan en config.json (campos `*Enc`).
//! - Windows: DPAPI, ligado al usuario de Windows.
//! - Android: base64. config.json vive en la carpeta privada de la app, a la que no llegan
//!   otras apps.
//! - Linux, macOS…: ChaCha20-Poly1305 con una clave aleatoria en `secret.key` dentro de la carpeta
//!   de datos (solo legible por su dueño, 600). Se guarda como `v1:<base64(nonce + cifrado)>`;
//!   lo guardado antes en base64 se sigue leyendo y se cifra al volver a guardar.
//!   La app de escritorio guarda esa clave en el llavero del sistema (Llavero de macOS, libsecret
//!   en Linux) con [`set_key_store`]; el archivo queda solo como reserva si el llavero no funciona.
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

/// Un llavero del sistema donde guardar la clave en vez de en `secret.key`.
pub trait KeyStore: Send + Sync {
    fn get(&self) -> Option<Vec<u8>>;
    fn set(&self, key: &[u8]) -> bool;
}

#[cfg(not(any(windows, target_os = "android")))]
static STORE: OnceLock<Box<dyn KeyStore>> = OnceLock::new();

/// Usar un llavero para la clave. Hay que llamarlo antes de leer los ajustes.
pub fn set_key_store(store: Box<dyn KeyStore>) {
    #[cfg(not(any(windows, target_os = "android")))]
    let _ = STORE.set(store);
    #[cfg(any(windows, target_os = "android"))]
    let _ = store;
}

/// La clave de `secret.key`, creada (600) la primera vez.
#[cfg(not(any(windows, target_os = "android")))]
fn key() -> Option<[u8; 32]> {
    static KEY: OnceLock<Option<[u8; 32]>> = OnceLock::new();
    *KEY.get_or_init(|| {
        let file = KEY_DIR.get()?.join("secret.key");
        match STORE.get() {
            Some(store) => with_store(store.as_ref(), &file),
            None => load_or_create(&file),
        }
    })
}

/// La clave con llavero: la del llavero si hay; si no, la del archivo (o una nueva) pasa al
/// llavero, y el archivo se borra solo cuando se ha comprobado que el llavero la devuelve igual.
/// Si el llavero falla, todo sigue con el archivo como antes.
#[cfg(not(any(windows, target_os = "android")))]
fn with_store(store: &dyn KeyStore, file: &Path) -> Option<[u8; 32]> {
    if let Some(k) = store.get().and_then(|v| <[u8; 32]>::try_from(v).ok()) {
        // Un archivo que sobró de antes, con la misma clave: ya no hace falta.
        if std::fs::read(file).ok().as_deref() == Some(&k[..]) {
            let _ = std::fs::remove_file(file);
        }
        return Some(k);
    }
    let k = load_or_create(file)?;
    if store.set(&k) && store.get().as_deref() == Some(&k[..]) {
        let _ = std::fs::remove_file(file);
    }
    Some(k)
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
        // Con llavero: el archivo pasa al llavero y se borra; después sale del llavero.
        struct Mem(std::sync::Mutex<Option<Vec<u8>>>, bool);
        impl KeyStore for Mem {
            fn get(&self) -> Option<Vec<u8>> {
                self.0.lock().unwrap().clone()
            }
            fn set(&self, key: &[u8]) -> bool {
                if self.1 {
                    *self.0.lock().unwrap() = Some(key.to_vec());
                }
                self.1
            }
        }
        let k = load_or_create(&path).unwrap();
        let ok = Mem(Default::default(), true);
        assert_eq!(with_store(&ok, &path), Some(k));
        assert!(!path.exists());
        assert_eq!(with_store(&ok, &path), Some(k));
        // Un llavero que no guarda: el archivo se queda.
        let broken = Mem(Default::default(), false);
        let k2 = with_store(&broken, &path).unwrap();
        assert!(path.exists());
        assert_eq!(load_or_create(&path), Some(k2));
        let _ = std::fs::remove_dir_all(&dir);
        // Lo que se guardó en base64 antes de cifrar se sigue leyendo.
        assert_eq!(unprotect("c2VjcmV0").as_deref(), Some("secret"));
        assert_eq!(unprotect(&protect("pässwörd").unwrap()).as_deref(), Some("pässwörd"));
    }
}
