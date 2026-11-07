//! Traducciones de los textos que genera el motor (errores, avisos, menú de la bandeja y
//! notificaciones). Los textos están en `crates/nebula-core/locales/<código>.json`; `es` es el original y
//! `en` el idioma de reserva cuando falta una cadena.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

/// Idiomas incluidos. El orden debe coincidir con `LANGUAGES` de `src/lib/i18n.svelte.js`.
const LOCALES: &[(&str, &str)] = &[
    ("es", include_str!("../locales/es.json")),
    ("en", include_str!("../locales/en.json")),
    ("zh", include_str!("../locales/zh.json")),
    ("hi", include_str!("../locales/hi.json")),
    ("pt", include_str!("../locales/pt.json")),
    ("fr", include_str!("../locales/fr.json")),
    ("de", include_str!("../locales/de.json")),
    ("ru", include_str!("../locales/ru.json")),
    ("ja", include_str!("../locales/ja.json")),
    ("ar", include_str!("../locales/ar.json")),
    ("it", include_str!("../locales/it.json")),
    ("ca", include_str!("../locales/ca.json")),
];
const FALLBACK: usize = 1; // en

/// Índice en LOCALES del idioma actual (por defecto español, el idioma original).
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static TABLES: OnceLock<Vec<HashMap<String, String>>> = OnceLock::new();

fn tables() -> &'static [HashMap<String, String>] {
    TABLES.get_or_init(|| {
        LOCALES
            .iter()
            .map(|(code, raw)| {
                serde_json::from_str(raw).unwrap_or_else(|e| {
                    crate::lerror!("Traducción {} no válida: {}", code, e);
                    HashMap::new()
                })
            })
            .collect()
    })
}

/// Cambia el idioma. Acepta códigos como "es", "pt-BR" o "zh_CN"; devuelve false si no está disponible.
pub fn set(lang: &str) -> bool {
    match index_of(lang) {
        Some(i) => {
            CURRENT.store(i, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

fn index_of(lang: &str) -> Option<usize> {
    let base = lang.trim().to_ascii_lowercase();
    let base = base.split(['-', '_']).next().unwrap_or("");
    LOCALES.iter().position(|(c, _)| *c == base)
}

/// Texto traducido de `key` (o la clave, si no existe en ningún idioma).
pub fn text(key: &str) -> String {
    let t = tables();
    t[CURRENT.load(Ordering::Relaxed)]
        .get(key)
        .or_else(|| t[FALLBACK].get(key))
        .or_else(|| t[0].get(key))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

/// Sustituye las variables `{nombre}` de una plantilla.
pub fn fill(mut s: String, vars: &[(&str, String)]) -> String {
    for (name, value) in vars {
        s = s.replace(&format!("{{{}}}", name), value);
    }
    s
}

/// Idioma preferido del sistema, si está traducido; si no, inglés.
pub fn system_language() -> &'static str {
    for lang in system_languages() {
        if let Some(i) = index_of(&lang) {
            return LOCALES[i].0;
        }
    }
    LOCALES[FALLBACK].0
}

#[cfg(windows)]
fn system_languages() -> Vec<String> {
    use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
    let mut count = 0u32;
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    let ok = unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, buf.as_mut_ptr(), &mut len) };
    if ok == 0 {
        return Vec::new();
    }
    // Lista de cadenas separadas por \0 y terminada en \0\0.
    buf[..(len as usize).min(buf.len())]
        .split(|c| *c == 0)
        .filter(|s| !s.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

#[cfg(not(windows))]
fn system_languages() -> Vec<String> {
    #[allow(unused_mut)]
    let mut out: Vec<String> =
        ["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|v| std::env::var(v).ok()).filter(|v| !v.is_empty()).collect();
    // Las apps de macOS abiertas desde Finder no tienen LANG: el idioma está en las preferencias.
    #[cfg(target_os = "macos")]
    if out.is_empty() {
        if let Ok(o) = std::process::Command::new("defaults").args(["read", "-g", "AppleLanguages"]).output() {
            out = parse_apple_languages(&String::from_utf8_lossy(&o.stdout));
        }
    }
    out
}

/// Salida de `defaults read -g AppleLanguages`: `(\n    "es-ES",\n    en\n)`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_apple_languages(text: &str) -> Vec<String> {
    text.split(|c: char| c == ',' || c == '\n' || c == '(' || c == ')')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Texto traducido, con variables opcionales: `t!("clave")` o `t!("clave", nombre = valor)`.
#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::i18n::text($key)
    };
    ($key:literal, $($name:ident = $val:expr),+ $(,)?) => {
        $crate::i18n::fill($crate::i18n::text($key), &[$((stringify!($name), ($val).to_string())),+])
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn apple_languages() {
        assert_eq!(super::parse_apple_languages("(\n    \"es-ES\",\n    en\n)\n"), vec!["es-ES", "en"]);
        assert!(super::parse_apple_languages("").is_empty());
    }

    use super::*;
    use std::collections::BTreeSet;

    fn placeholders(s: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut rest = s;
        while let Some(i) = rest.find('{') {
            rest = &rest[i + 1..];
            if let Some(j) = rest.find('}') {
                let name = &rest[..j];
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    out.insert(name.to_string());
                }
                rest = &rest[j + 1..];
            }
        }
        out
    }

    fn flatten(prefix: &str, v: &serde_json::Value, out: &mut HashMap<String, String>) {
        match v {
            serde_json::Value::Object(m) => {
                for (k, v) in m {
                    let key = if prefix.is_empty() { k.clone() } else { format!("{}.{}", prefix, k) };
                    flatten(&key, v, out);
                }
            }
            serde_json::Value::String(s) => {
                out.insert(prefix.to_string(), s.clone());
            }
            other => panic!("{}: valor no válido {:?}", prefix, other),
        }
    }

    /// Todas las traducciones tienen las mismas claves y variables que el original.
    fn check_same(name: &str, reference: &HashMap<String, String>, other: &HashMap<String, String>, plural_ok: bool) {
        let base = |k: &str| -> String {
            match k.rsplit_once('.') {
                Some((b, f)) if plural_ok && ["zero", "one", "two", "few", "many", "other"].contains(&f) => b.to_string(),
                _ => k.to_string(),
            }
        };
        let ref_keys: BTreeSet<String> = reference.keys().map(|k| base(k)).collect();
        let keys: BTreeSet<String> = other.keys().map(|k| base(k)).collect();
        let missing: Vec<_> = ref_keys.difference(&keys).collect();
        let extra: Vec<_> = keys.difference(&ref_keys).collect();
        assert!(missing.is_empty(), "{}: faltan claves {:?}", name, missing);
        assert!(extra.is_empty(), "{}: claves de más {:?}", name, extra);
        for (k, v) in other {
            let r = reference.get(k).or_else(|| reference.get(&format!("{}.other", base(k))));
            if let Some(r) = r {
                let mut want = placeholders(r);
                let got = placeholders(v);
                if plural_ok {
                    // En los plurales, {n} puede omitirse (p. ej. "un archivo").
                    want.remove("n");
                    let mut got = got.clone();
                    got.remove("n");
                    assert_eq!(want, got, "{}: variables distintas en {}", name, k);
                } else {
                    assert_eq!(want, got, "{}: variables distintas en {}", name, k);
                }
            }
        }
    }

    #[test]
    fn backend_locales_complete() {
        let t = tables();
        assert_eq!(t.len(), LOCALES.len());
        for (i, (code, _)) in LOCALES.iter().enumerate() {
            assert!(!t[i].is_empty(), "{} vacío", code);
            check_same(code, &t[0], &t[i], false);
        }
    }

    #[test]
    fn ui_locales_complete() {
        let ui: Vec<(&str, &str)> = vec![
            ("es", include_str!("../../../src/locales/es.json")),
            ("en", include_str!("../../../src/locales/en.json")),
            ("zh", include_str!("../../../src/locales/zh.json")),
            ("hi", include_str!("../../../src/locales/hi.json")),
            ("pt", include_str!("../../../src/locales/pt.json")),
            ("fr", include_str!("../../../src/locales/fr.json")),
            ("de", include_str!("../../../src/locales/de.json")),
            ("ru", include_str!("../../../src/locales/ru.json")),
            ("ja", include_str!("../../../src/locales/ja.json")),
            ("ar", include_str!("../../../src/locales/ar.json")),
            ("it", include_str!("../../../src/locales/it.json")),
            ("ca", include_str!("../../../src/locales/ca.json")),
        ];
        let parse = |code: &str, raw: &str| {
            let v: serde_json::Value = serde_json::from_str(raw).unwrap_or_else(|e| panic!("{}: {}", code, e));
            let mut out = HashMap::new();
            flatten("", &v, &mut out);
            out
        };
        let reference = parse("es", ui[0].1);
        for (code, raw) in &ui {
            check_same(code, &reference, &parse(code, raw), true);
        }
    }

    #[test]
    fn codes_and_fill() {
        // No se cambia el idioma global: otros tests comprueban textos en español.
        assert_eq!(index_of("en-US"), Some(1));
        assert_eq!(index_of("pt_BR"), Some(4));
        assert_eq!(index_of("xx"), None);
        assert_eq!(fill("{a} y {b}".into(), &[("a", "1".into()), ("b", "2".into())]), "1 y 2");
    }
}
