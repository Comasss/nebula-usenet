//! Desofuscación: muchos releases llegan con nombres aleatorios ("a8f3c1….mkv" o sin extensión).
//! 1) Con el `.par2`: cada archivo descrito allí lleva su nombre real y el MD5 de sus primeros
//!    16 KB; los archivos que coinciden recuperan ese nombre.
//! 2) Al terminar: si el archivo principal sigue con un nombre aleatorio, toma el del trabajo.
use crate::util;
use md5::{Digest, Md5};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"PAR2\0PKT";
const FILE_DESC: &[u8; 16] = b"PAR 2.0\0FileDesc";

#[derive(Debug, Clone, PartialEq)]
pub struct Par2File {
    pub name: String,
    pub hash16k: [u8; 16],
    pub size: u64,
}

/// Archivos que describe un `.par2` (paquetes FileDesc).
pub fn par2_files(data: &[u8]) -> Vec<Par2File> {
    let mut out: Vec<Par2File> = Vec::new();
    let mut i = 0usize;
    while i + 64 <= data.len() {
        if &data[i..i + 8] != MAGIC {
            // Datos que no son un paquete: buscar la siguiente cabecera.
            match data[i + 1..].windows(8).position(|w| w == MAGIC) {
                Some(p) => {
                    i += 1 + p;
                    continue;
                }
                None => break,
            }
        }
        let len = u64::from_le_bytes(data[i + 8..i + 16].try_into().unwrap_or_default()) as usize;
        if len < 64 || len % 4 != 0 || i + len > data.len() {
            i += 8;
            continue;
        }
        if &data[i + 48..i + 64] == FILE_DESC && len >= 64 + 56 {
            let body = &data[i + 64..i + len];
            let mut hash16k = [0u8; 16];
            hash16k.copy_from_slice(&body[32..48]);
            let size = u64::from_le_bytes(body[48..56].try_into().unwrap_or_default());
            let raw = &body[56..];
            let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
            let name = String::from_utf8_lossy(&raw[..end]).to_string();
            // Rutas dentro del par2: solo vale el nombre.
            let name = name.rsplit(['/', '\\']).next().unwrap_or("").to_string();
            let name = util::sanitize_filename(&name);
            if !name.is_empty() && !out.iter().any(|f| f.name == name) {
                out.push(Par2File { name, hash16k, size });
            }
        }
        i += len;
    }
    out
}

fn hash16k(path: &Path) -> Option<[u8; 16]> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 16 * 1024];
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(_) => return None,
        }
    }
    let mut h = Md5::new();
    h.update(&buf[..n]);
    Some(h.finalize().into())
}

fn is_par2(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".par2")
}

/// Renombra los archivos de `dir` con los nombres que trae el PAR2. Devuelve cuántos cambió.
pub fn rename_from_par2(dir: &Path) -> usize {
    let files = util::list_files(dir);
    // El índice (el .par2 más pequeño) describe todos los archivos; los volúmenes también,
    // así que basta con leer uno que no sea enorme.
    let mut par2s: Vec<&(PathBuf, u64)> = files.iter().filter(|(p, _)| is_par2(&util::file_name(p))).collect();
    par2s.sort_by_key(|(_, s)| *s);
    let mut described: Vec<Par2File> = Vec::new();
    for (p, size) in par2s.iter().take(3) {
        if *size > 64 * 1024 * 1024 {
            continue;
        }
        if let Ok(data) = std::fs::read(p) {
            described = par2_files(&data);
            if !described.is_empty() {
                break;
            }
        }
    }
    if described.is_empty() {
        return 0;
    }
    let present: HashSet<String> = files.iter().map(|(p, _)| util::file_name(p)).collect();
    let by_hash: HashMap<([u8; 16], u64), &Par2File> = described.iter().filter(|d| !present.contains(&d.name)).map(|d| ((d.hash16k, d.size), d)).collect();
    if by_hash.is_empty() {
        return 0;
    }
    let mut renamed = 0;
    for (p, size) in &files {
        let name = util::file_name(p);
        if is_par2(&name) || described.iter().any(|d| d.name == name) {
            continue;
        }
        let Some(h) = hash16k(p) else { continue };
        if let Some(d) = by_hash.get(&(h, *size)) {
            let to = dir.join(&d.name);
            if !to.exists() && std::fs::rename(p, &to).is_ok() {
                crate::linfo!("{}", t!("log.pp.deobfuscated", from = name, to = d.name));
                renamed += 1;
            }
        }
    }
    renamed
}

/// ¿Parece un nombre aleatorio? (sin contar la extensión).
pub fn is_random_name(name: &str) -> bool {
    if util::looks_obfuscated(name) {
        return true;
    }
    let stem = match name.rsplit_once('.') {
        Some((s, _)) if !s.is_empty() => s,
        _ => name,
    };
    if stem.len() < 16 || stem.chars().any(|c| matches!(c, ' ' | '.' | '_' | '-' | '(' | '[')) {
        return false;
    }
    let hex = stem.chars().all(|c| c.is_ascii_hexdigit());
    let mixed = stem.chars().any(|c| c.is_ascii_digit()) && stem.chars().any(|c| c.is_ascii_uppercase()) && stem.chars().any(|c| c.is_ascii_lowercase());
    hex || mixed
}

/// Archivos (hasta dos niveles) de `dir`.
pub fn walk(dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut out = util::list_files(dir);
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                out.extend(util::list_files(&e.path()));
            }
        }
    }
    out
}

/// Si el archivo principal de los nuevos (`files`) tiene nombre aleatorio, le pone `job_name`
/// (y lo mismo a sus acompañantes con el mismo nombre: subtítulos, .nfo…). Devuelve el nuevo nombre.
pub fn rename_main(files: &[(PathBuf, u64)], job_name: &str) -> Option<String> {
    let mut sorted: Vec<&(PathBuf, u64)> = files.iter().filter(|(p, _)| !is_par2(&util::file_name(p))).collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    let (main, size) = sorted.first()?;
    // Varios archivos parecidos (una temporada): no se sabe cuál es cuál.
    if sorted.get(1).is_some_and(|(_, s2)| *s2 * 2 > *size) {
        return None;
    }
    let name = util::file_name(main);
    if !is_random_name(&name) {
        return None;
    }
    let stem_of = |n: &str| n.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or_else(|| n.to_string());
    let old_stem = stem_of(&name);
    let new_stem = util::sanitize_filename(job_name.trim());
    if new_stem.is_empty() {
        return None;
    }
    let mut result = None;
    for (p, _) in &sorted {
        let n = util::file_name(p);
        if stem_of(&n) != old_stem || p.parent() != main.parent() {
            continue;
        }
        let ext = n.rsplit_once('.').map(|(_, e)| format!(".{}", e)).unwrap_or_default();
        let to = p.with_file_name(format!("{}{}", new_stem, ext));
        if !to.exists() && std::fs::rename(p, &to).is_ok() {
            crate::linfo!("{}", t!("log.pp.deobfuscated", from = n, to = util::file_name(&to)));
            if p == main {
                result = Some(util::file_name(&to));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(kind: &[u8; 16], body: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(MAGIC);
        let mut body = body.to_vec();
        while body.len() % 4 != 0 {
            body.push(0);
        }
        v.extend_from_slice(&((64 + body.len()) as u64).to_le_bytes());
        v.extend_from_slice(&[0u8; 32]);
        v.extend_from_slice(kind);
        v.extend_from_slice(&body);
        v
    }

    fn desc(name: &str, content: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 32];
        let h: [u8; 16] = Md5::digest(&content[..content.len().min(16384)]).into();
        b.extend_from_slice(&h);
        b.extend_from_slice(&(content.len() as u64).to_le_bytes());
        b.extend_from_slice(name.as_bytes());
        packet(FILE_DESC, &b)
    }

    #[test]
    fn renames_from_par2() {
        let dir = std::env::temp_dir().join(format!("nebula-deob-{}", util::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let movie: Vec<u8> = (0..40000u32).map(|i| (i * 7 % 251) as u8).collect();
        let nfo = b"hello".to_vec();
        let mut par2 = packet(b"PAR 2.0\0Main\0\0\0\0", &[0u8; 12]);
        par2.extend(desc("Real.Movie.2020.mkv", &movie));
        par2.extend(b"junk");
        par2.extend(desc("Real.Movie.2020.nfo", &nfo));
        assert_eq!(par2_files(&par2).len(), 2);
        std::fs::write(dir.join("a8f3c1d2e4b5a6978877.par2"), &par2).unwrap();
        std::fs::write(dir.join("a8f3c1d2e4b5a6978877"), &movie).unwrap();
        std::fs::write(dir.join("zz.bin"), &nfo).unwrap();
        std::fs::write(dir.join("other.txt"), b"other").unwrap();
        assert_eq!(rename_from_par2(&dir), 2);
        assert!(dir.join("Real.Movie.2020.mkv").exists());
        assert!(dir.join("Real.Movie.2020.nfo").exists());
        assert!(dir.join("other.txt").exists());
        assert_eq!(rename_from_par2(&dir), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn random_names() {
        assert!(is_random_name("6a9d9cde8057274c68ee918fa1f8dbbf.mkv"));
        assert!(is_random_name("Xk3mQ9pLz7Rt2Wv8Ny.mkv"));
        assert!(is_random_name("abc"));
        assert!(!is_random_name("Some.Show.S01E01.1080p.mkv"));
        assert!(!is_random_name("Movie (2020).mkv"));
        assert!(!is_random_name("BigBuckBunny.mkv"));
    }

    #[test]
    fn renames_main() {
        let dir = std::env::temp_dir().join(format!("nebula-deob-{}", util::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Xk3mQ9pLz7Rt2Wv8Ny.mkv"), vec![0u8; 5000]).unwrap();
        std::fs::write(dir.join("Xk3mQ9pLz7Rt2Wv8Ny.srt"), vec![0u8; 50]).unwrap();
        std::fs::write(dir.join("sample.mkv"), vec![0u8; 500]).unwrap();
        let files = walk(&dir);
        assert_eq!(rename_main(&files, "My Movie (2020)").as_deref(), Some("My Movie (2020).mkv"));
        assert!(dir.join("My Movie (2020).srt").exists());
        assert!(dir.join("sample.mkv").exists());
        // Dos archivos grandes parecidos: no se toca nada.
        std::fs::write(dir.join("Ab3mQ9pLz7Rt2Wv8Ny.mkv"), vec![0u8; 4000]).unwrap();
        assert_eq!(rename_main(&walk(&dir), "X"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
