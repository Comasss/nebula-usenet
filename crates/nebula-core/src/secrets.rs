//! Cifrado de las contraseñas y claves que se guardan en config.json (campos `*Enc`).
//! - Windows: DPAPI, ligado al usuario de Windows.
//! - Resto: base64. En Android config.json vive en la carpeta privada de la app, a la que no
//!   llegan otras apps.
use base64::Engine as _;

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

// Fuera de Windows no hay DPAPI. En Android config.json vive en la carpeta privada de la app,
// a la que no llegan otras apps, así que basta con no guardar las claves en texto plano.
#[cfg(not(windows))]
pub fn protect(plain: &str) -> Option<String> {
    Some(base64::engine::general_purpose::STANDARD.encode(plain.as_bytes()))
}

#[cfg(not(windows))]
pub fn unprotect(enc: &str) -> Option<String> {
    let v = base64::engine::general_purpose::STANDARD.decode(enc).ok()?;
    String::from_utf8(v).ok()
}
