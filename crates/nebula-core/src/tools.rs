//! Dónde están par2 y 7-Zip, según la plataforma:
//! - Windows: los que Nebula lleva dentro y extrae en la carpeta `tools` (`par2.exe`, `7z.exe`).
//! - Android: van en el APK como bibliotecas nativas (`libpar2.so`, `lib7zz.so`), porque Android
//!   solo deja ejecutar archivos de esa carpeta.
//! - Resto (Linux, macOS): los de la carpeta `tools` si están; si no, los del sistema (PATH).
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Par2,
    SevenZip,
}

/// Nombres posibles de cada herramienta, en orden de preferencia.
fn names(tool: Tool) -> &'static [&'static str] {
    #[cfg(windows)]
    return match tool {
        Tool::Par2 => &["par2.exe"],
        Tool::SevenZip => &["7z.exe"],
    };
    #[cfg(target_os = "android")]
    return match tool {
        Tool::Par2 => &["libpar2.so"],
        Tool::SevenZip => &["lib7zz.so"],
    };
    #[cfg(not(any(windows, target_os = "android")))]
    return match tool {
        Tool::Par2 => &["par2"],
        // 7zz es el 7-Zip oficial; 7z y 7za, los de p7zip.
        Tool::SevenZip => &["7zz", "7z", "7za"],
    };
}

/// Ruta de la herramienta: en `dir` si está; si no (fuera de Windows y Android), la primera que
/// se encuentre en el PATH. Si no aparece, el nombre tal cual (fallará al ejecutarlo con un error
/// que lo dice).
pub fn path(dir: &Path, tool: Tool) -> PathBuf {
    let list = names(tool);
    for n in list {
        let p = dir.join(n);
        if p.is_file() {
            return p;
        }
    }
    #[cfg(not(any(windows, target_os = "android")))]
    if let Some(p) = list.iter().find_map(|n| in_path(n)) {
        return p;
    }
    #[cfg(any(windows, target_os = "android"))]
    return dir.join(list[0]);
    #[allow(unreachable_code)]
    PathBuf::from(list[0])
}

/// La herramienta que se va a usar: la de los ajustes si se eligió una, si no la automática.
pub fn resolve(s: &crate::config::Settings, dir: &Path, tool: Tool) -> PathBuf {
    let custom = match tool {
        Tool::Par2 => s.par2_path.trim(),
        Tool::SevenZip => s.seven_zip_path.trim(),
    };
    if custom.is_empty() {
        path(dir, tool)
    } else {
        PathBuf::from(custom)
    }
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    /// "par2" o "7z".
    pub tool: &'static str,
    pub path: String,
    pub found: bool,
}

/// Qué par2 y 7-Zip se usarán y si existen (para avisar en Ajustes → Post-procesado).
pub fn status(s: &crate::config::Settings, dir: &Path) -> Vec<ToolStatus> {
    [(Tool::Par2, "par2"), (Tool::SevenZip, "7z")]
        .into_iter()
        .map(|(t, name)| {
            let p = resolve(s, dir, t);
            ToolStatus { tool: name, found: p.is_file(), path: p.to_string_lossy().to_string() }
        })
        .collect()
}

/// Busca un ejecutable en el PATH.
pub fn in_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).map(|d| d.join(name)).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_the_tools_folder() {
        let dir = std::env::temp_dir().join(format!("nebula-tools-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let name = names(Tool::Par2)[0];
        std::fs::write(dir.join(name), b"").unwrap();
        assert_eq!(path(&dir, Tool::Par2), dir.join(name));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
