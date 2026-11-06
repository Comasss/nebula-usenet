use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn now_secs() -> u64 {
    now_ms() / 1000
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Limpia un nombre para que sea un nombre de archivo válido en Windows.
pub fn sanitize_filename(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 32 => '_',
            c => c,
        })
        .collect();
    out = out.trim().trim_end_matches(['.', ' ']).to_string();
    if out.is_empty() {
        out = "descarga".into();
    }
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        out.insert(0, '_');
    }
    if out.chars().count() > 180 {
        out = out.chars().take(180).collect();
    }
    out
}

/// Velocímetro: calcula la velocidad media de un contador acumulado en una ventana deslizante.
pub struct Speedo {
    samples: VecDeque<(Instant, u64)>,
    window_ms: u128,
}

impl Speedo {
    pub fn new(window_ms: u128) -> Self {
        Self { samples: VecDeque::new(), window_ms }
    }

    pub fn push(&mut self, total: u64) {
        let now = Instant::now();
        self.samples.push_back((now, total));
        while let Some((t, _)) = self.samples.front() {
            if now.duration_since(*t).as_millis() > self.window_ms && self.samples.len() > 2 {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn rate(&self) -> u64 {
        let (Some(a), Some(b)) = (self.samples.front(), self.samples.back()) else {
            return 0;
        };
        let dt = b.0.duration_since(a.0).as_secs_f64();
        if dt < 0.2 || b.1 < a.1 {
            return 0;
        }
        ((b.1 - a.1) as f64 / dt) as u64
    }

    pub fn reset(&mut self) {
        self.samples.clear();
    }
}

impl Default for Speedo {
    fn default() -> Self {
        Self::new(3000)
    }
}

pub fn fmt_bytes(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", b, U[0])
    } else {
        format!("{:.1} {}", v, U[i])
    }
}

/// Mueve un archivo; si está en otra unidad, copia y borra.
pub fn move_file(src: &Path, dst: &Path) -> io::Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    if dst.exists() {
        let _ = fs::remove_file(dst);
    }
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            fs::copy(src, dst)?;
            fs::remove_file(src)
        }
    }
}

/// Mueve un directorio completo (recursivo) combinando con el destino.
pub fn move_tree(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let p = entry.path();
        let target = dst.join(entry.file_name());
        if p.is_dir() {
            move_tree(&p, &target)?;
            let _ = fs::remove_dir(&p);
        } else {
            move_file(&p, &target)?;
        }
    }
    Ok(())
}

pub fn list_files(dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            if let Ok(md) = e.metadata() {
                if md.is_file() {
                    out.push((e.path(), md.len()));
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

pub fn file_name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

/// ¿Parece un nombre ofuscado? Sin extensión reconocible (`a6a50c…c81e`) o una ristra larga
/// de letras y números sin separadores. Sirve para no cambiar un buen nombre por uno así.
pub fn looks_obfuscated(name: &str) -> bool {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, e),
        _ => return true,
    };
    let ext_ok = (1..=5).contains(&ext.len())
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && ext.chars().any(|c| c.is_ascii_alphabetic() || ext.len() == 3);
    if !ext_ok {
        return true;
    }
    stem.len() >= 24 && stem.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Borra un directorio con reintentos (Windows puede tardar en liberar handles).
pub fn remove_dir_retry(dir: &Path) {
    for i in 0..5 {
        match fs::remove_dir_all(dir) {
            Ok(()) => return,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return,
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(200 * (i + 1))),
        }
    }
}

/// Extrae una contraseña con la convención "Nombre {{password}}".
pub fn split_password(name: &str) -> (String, Option<String>) {
    if let (Some(a), Some(b)) = (name.find("{{"), name.rfind("}}")) {
        if b > a + 2 {
            let pw = name[a + 2..b].trim().to_string();
            let clean = format!("{}{}", &name[..a], &name[b + 2..]).trim().to_string();
            if !pw.is_empty() {
                return (clean, Some(pw));
            }
        }
    }
    (name.to_string(), None)
}

/// Busca enlaces a NZB en un texto (una o varias líneas).
pub fn detect_nzb_urls(text: &str, patterns: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    if text.len() > 20_000 {
        return out;
    }
    for line in text.lines() {
        let t = line.trim().trim_matches(['"', '\'', '<', '>']);
        if t.is_empty() || t.len() > 4096 || t.contains(char::is_whitespace) {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        if !(lower.starts_with("http://") || lower.starts_with("https://")) {
            continue;
        }
        let path = lower.split(['?', '#']).next().unwrap_or("");
        let hit = path.ends_with(".nzb")
            || path.ends_with(".nzb.gz")
            || patterns.iter().any(|p| !p.trim().is_empty() && lower.contains(&p.trim().to_ascii_lowercase()));
        if hit && !out.contains(&t.to_string()) {
            out.push(t.to_string());
        }
    }
    out
}

/// Deshace encodeURIComponent (%XX en UTF-8).
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && s.is_char_boundary(i + 1) && s.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obfuscated_names() {
        assert!(looks_obfuscated("6a9d9cde8057274c68ee918fa1f8dbbfb855bc387a236d197e49d6764285"));
        assert!(looks_obfuscated("a6a50c17a23322d596c91488a858b8b51d5c4decc0224d4976de1182c81e.par2"));
        assert!(!looks_obfuscated("Zero.Day.S01E03.2025.1080p.NF.MULTI.WEB-DL.DDP5.1.H.264-SPWEB.mkv"));
        assert!(!looks_obfuscated("movie.part01.rar"));
        assert!(!looks_obfuscated("movie.7z.001"));
        assert!(!looks_obfuscated("x.vol07+08.par2"));
    }

    #[test]
    fn detect() {
        let p = crate::config::default_patterns();
        assert_eq!(detect_nzb_urls("https://x.org/file.nzb", &p).len(), 1);
        assert_eq!(detect_nzb_urls("https://x.org/getnzb/abc?i=1&r=k", &p).len(), 1);
        assert_eq!(detect_nzb_urls("https://x.org/api?t=get&id=abc&apikey=z", &p).len(), 1);
        assert!(detect_nzb_urls("https://google.com/search?q=nzb", &p).is_empty());
        assert!(detect_nzb_urls("hola que tal", &p).is_empty());
    }

    #[test]
    fn percent() {
        assert_eq!(percent_decode("Pel%C3%ADcula%20(2024).nzb"), "Película (2024).nzb");
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("100%"), "100%");
    }
}
