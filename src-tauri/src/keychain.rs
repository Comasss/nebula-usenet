//! La clave que cifra las contraseñas, en el llavero del sistema: el Llavero en macOS y el
//! Secret Service (GNOME Keyring, KWallet) en Linux. Si no hay llavero o falla, el motor sigue
//! usando el archivo `secret.key` de la carpeta de datos.
use base64::Engine as _;
use nebula_core::secrets::KeyStore;

const SERVICE: &str = "Nebula";
const ACCOUNT: &str = "secret-key";

struct Keychain;

impl Keychain {
    fn entry() -> Option<keyring::Entry> {
        keyring::Entry::new(SERVICE, ACCOUNT).ok()
    }
}

impl KeyStore for Keychain {
    fn get(&self) -> Option<Vec<u8>> {
        let s = Self::entry()?.get_password().ok()?;
        base64::engine::general_purpose::STANDARD.decode(s).ok()
    }

    fn set(&self, key: &[u8]) -> bool {
        let Some(e) = Self::entry() else { return false };
        e.set_password(&base64::engine::general_purpose::STANDARD.encode(key)).is_ok()
    }
}

/// Usar el llavero del sistema, salvo con una carpeta de datos propia (NEBULA_DATA_DIR): esas
/// instancias de prueba no deben compartir ni pisar la clave de la instalación normal.
pub fn install() {
    if std::env::var_os("NEBULA_DATA_DIR").is_none() {
        nebula_core::secrets::set_key_store(Box::new(Keychain));
    }
}
