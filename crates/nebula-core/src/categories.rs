//! Categorías: cada trabajo de la cola y del historial pertenece a una (tv, movies…), que decide
//! su carpeta, su método de descarga, su prioridad y su post-procesado.
//!
//! La categoría Default (nombre `*`, como en SABnzbd) es fija y equivale a los ajustes globales
//! de siempre: no se guarda en `Settings::categories`. Los trabajos y entradas del historial de
//! versiones anteriores, que no tienen categoría, cargan como Default.

use crate::config::Settings;
use crate::engine::JobOptions;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Nombre interno de la categoría Default.
pub const DEFAULT: &str = "*";

/// Prioridades (los mismos valores que SABnzbd).
pub const PRIO_LOW: i32 = -1;
pub const PRIO_NORMAL: i32 = 0;
#[allow(dead_code)]
pub const PRIO_HIGH: i32 = 1;
pub const PRIO_FORCE: i32 = 2;

/// Post-procesado: lo de los ajustes globales.
pub const PP_GLOBAL: i32 = -1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Category {
    /// Nombre único, sin distinguir mayúsculas (tv, movies…).
    pub name: String,
    /// Carpeta de destino. Vacía = la de descargas; relativa = dentro de la de descargas.
    pub dir: String,
    /// "" = método predeterminado, "nntp", "nntp:<servidor>", "debrid" o el id de un servicio.
    pub via: String,
    /// Prioridad por defecto de sus trabajos (-1 baja, 0 normal, 1 alta, 2 forzar).
    pub priority: i32,
    /// Post-procesado: -1 = global, 0 = nada, 1 = reparar, 2 = reparar y extraer,
    /// 3 = reparar, extraer y borrar archivos comprimidos y .par2.
    pub pp: i32,
    /// Categorías Newznab que van a esta categoría: "5000-5999, 7020".
    pub newznab: String,
}

impl Default for Category {
    fn default() -> Self {
        Self { name: String::new(), dir: String::new(), via: String::new(), priority: PRIO_NORMAL, pp: PP_GLOBAL, newznab: String::new() }
    }
}

pub fn default_name() -> String {
    DEFAULT.to_string()
}

pub fn is_default(name: &str) -> bool {
    let n = name.trim();
    n.is_empty() || n == DEFAULT || n.eq_ignore_ascii_case("default") || n.eq_ignore_ascii_case("none")
}

/// Nombre normalizado de la categoría `name` tal como existe (respetando sus mayúsculas), o `*`.
pub fn canonical(s: &Settings, name: Option<&str>) -> String {
    match name.map(str::trim).filter(|n| !is_default(n)) {
        Some(n) => s.categories.iter().find(|c| c.name.eq_ignore_ascii_case(n)).map(|c| c.name.clone()).unwrap_or_else(default_name),
        None => default_name(),
    }
}

pub fn find<'a>(s: &'a Settings, name: &str) -> Option<&'a Category> {
    if is_default(name) {
        return None;
    }
    s.categories.iter().find(|c| c.name.eq_ignore_ascii_case(name.trim()))
}

pub fn clamp_priority(p: i32) -> i32 {
    p.clamp(PRIO_LOW, PRIO_FORCE)
}

/// Carpeta de destino de una categoría.
pub fn dest_dir(s: &Settings, cat: Option<&Category>) -> PathBuf {
    let base = PathBuf::from(&s.download_dir);
    match cat.map(|c| c.dir.trim()).filter(|d| !d.is_empty()) {
        Some(d) => {
            let p = PathBuf::from(d);
            if p.is_absolute() || is_windows_absolute(d) {
                p
            } else {
                base.join(p)
            }
        }
        None => base,
    }
}

/// `C:\…` o `\\servidor\…` también cuentan como absolutas fuera de Windows (ajustes traídos de un PC).
pub fn is_windows_absolute(d: &str) -> bool {
    let b = d.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')) || d.starts_with("\\\\")
}

/// Opciones de post-procesado globales.
pub fn global_options(s: &Settings) -> JobOptions {
    JobOptions {
        par2: s.par2_enabled,
        extract: s.extract_enabled,
        delete_archives: s.delete_archives,
        delete_par2: s.delete_par2,
        create_folder: s.create_job_folder,
    }
}

/// Opciones de post-procesado para un nivel de SABnzbd (-1 = las globales).
pub fn options_for_pp(s: &Settings, pp: i32) -> JobOptions {
    let mut o = global_options(s);
    match pp {
        0 => {
            o.par2 = false;
            o.extract = false;
            o.delete_archives = false;
            o.delete_par2 = false;
        }
        1 => {
            o.par2 = true;
            o.extract = false;
            o.delete_archives = false;
            o.delete_par2 = false;
        }
        2 => {
            o.par2 = true;
            o.extract = true;
            o.delete_archives = false;
            o.delete_par2 = false;
        }
        3 => {
            o.par2 = true;
            o.extract = true;
            o.delete_archives = true;
            o.delete_par2 = true;
        }
        _ => {}
    }
    o
}

/// Nivel de post-procesado de SABnzbd equivalente a unas opciones.
#[allow(dead_code)]
pub fn pp_of(o: &JobOptions) -> i32 {
    match (o.par2, o.extract, o.delete_archives) {
        (_, true, true) => 3,
        (_, true, false) => 2,
        (true, false, _) => 1,
        (false, false, _) => 0,
    }
}

/// Rangos Newznab de una categoría: "5000-5999, 7020" → [(5000, 5999), (7020, 7020)].
pub fn parse_ranges(spec: &str) -> Vec<(u32, u32)> {
    let spec = regex::Regex::new(r"\s*-\s*").map(|re| re.replace_all(spec, "-").into_owned()).unwrap_or_else(|_| spec.to_string());
    spec.split([',', ';', ' '])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .filter_map(|p| match p.split_once('-') {
            Some((a, b)) => {
                let (a, b) = (a.trim().parse::<u32>().ok()?, b.trim().parse::<u32>().ok()?);
                Some((a.min(b), a.max(b)))
            }
            None => p.parse::<u32>().ok().map(|n| (n, n)),
        })
        .collect()
}

/// Categoría propia para unas categorías Newznab (la primera que encaje), si hay alguna.
/// Gana el rango más estrecho: "5040" pesa más que "5000-5999".
pub fn for_newznab(s: &Settings, ids: &[u32]) -> Option<String> {
    let mut best: Option<(u32, &Category)> = None;
    for c in &s.categories {
        for (a, b) in parse_ranges(&c.newznab) {
            if ids.iter().any(|id| *id >= a && *id <= b) {
                let width = b - a;
                if best.map_or(true, |(w, _)| width < w) {
                    best = Some((width, c));
                }
            }
        }
    }
    best.map(|(_, c)| c.name.clone())
}

/// Revisa y limpia las categorías antes de guardarlas: nombres únicos y válidos.
pub fn validate(list: &mut Vec<Category>) -> Result<(), String> {
    let mut seen: Vec<String> = Vec::new();
    for c in list.iter_mut() {
        c.name = c.name.trim().to_string();
        c.dir = c.dir.trim().to_string();
        c.via = c.via.trim().to_string();
        c.newznab = c.newznab.trim().to_string();
        c.priority = clamp_priority(c.priority);
        if !(PP_GLOBAL..=3).contains(&c.pp) {
            c.pp = PP_GLOBAL;
        }
        if c.name.is_empty() {
            return Err(t!("err.categoryNoName"));
        }
        if is_default(&c.name) || c.name.chars().any(|ch| ch.is_control() || "/\\\"<>|?*".contains(ch)) {
            return Err(t!("err.categoryBadName", name = c.name));
        }
        let key = c.name.to_lowercase();
        if seen.contains(&key) {
            return Err(t!("err.categoryDuplicate", name = c.name));
        }
        seen.push(key);
    }
    Ok(())
}

/// Lo que una categoría decide para un trabajo nuevo.
pub struct Resolved {
    pub category: String,
    pub dest_dir: PathBuf,
    pub via: Option<String>,
    pub priority: i32,
    pub options: JobOptions,
}

/// Valores por defecto de un trabajo de la categoría `name` (o de las categorías Newznab `newznab`
/// si no se indica ninguna).
pub fn resolve(s: &Settings, name: Option<&str>, newznab: &[u32]) -> Resolved {
    let explicit = name.map(str::trim).filter(|n| !n.is_empty());
    let name = match explicit {
        Some(n) => canonical(s, Some(n)),
        None => for_newznab(s, newznab).unwrap_or_else(default_name),
    };
    let cat = find(s, &name);
    Resolved {
        dest_dir: dest_dir(s, cat),
        via: cat.map(|c| c.via.clone()).filter(|v| !v.is_empty()),
        priority: cat.map(|c| c.priority).unwrap_or(PRIO_NORMAL),
        options: options_for_pp(s, cat.map(|c| c.pp).unwrap_or(PP_GLOBAL)),
        category: name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        let mut s = Settings::default();
        s.download_dir = if cfg!(windows) { "C:\\Descargas".into() } else { "/descargas".into() };
        s.categories = vec![
            Category { name: "tv".into(), dir: "Series".into(), newznab: "5000-5999".into(), priority: PRIO_HIGH, ..Default::default() },
            Category { name: "Movies".into(), dir: String::new(), newznab: "2000-2999".into(), pp: 1, via: "nntp".into(), ..Default::default() },
            Category { name: "anime".into(), newznab: "5070".into(), ..Default::default() },
        ];
        s
    }

    #[test]
    fn default_is_global_behaviour() {
        let s = settings();
        let r = resolve(&s, None, &[]);
        assert_eq!(r.category, DEFAULT);
        assert_eq!(r.dest_dir, PathBuf::from(&s.download_dir));
        assert_eq!(r.via, None);
        assert_eq!(r.priority, PRIO_NORMAL);
        let g = global_options(&s);
        assert_eq!((r.options.par2, r.options.extract, r.options.delete_archives), (g.par2, g.extract, g.delete_archives));
        // Nombres que significan Default y categorías que no existen.
        for n in ["", "*", "Default", "none", "nope"] {
            assert_eq!(resolve(&s, Some(n), &[]).category, DEFAULT, "{n}");
        }
    }

    #[test]
    fn category_values() {
        let s = settings();
        let r = resolve(&s, Some("TV"), &[]);
        assert_eq!(r.category, "tv");
        assert_eq!(r.dest_dir, PathBuf::from(&s.download_dir).join("Series"));
        assert_eq!(r.priority, PRIO_HIGH);
        let m = resolve(&s, Some("movies"), &[]);
        assert_eq!(m.category, "Movies");
        assert_eq!(m.dest_dir, PathBuf::from(&s.download_dir));
        assert_eq!(m.via.as_deref(), Some("nntp"));
        assert!(m.options.par2 && !m.options.extract);
    }

    #[test]
    fn newznab_mapping() {
        let s = settings();
        assert_eq!(for_newznab(&s, &[5040]).as_deref(), Some("tv"));
        assert_eq!(for_newznab(&s, &[2000, 2040]).as_deref(), Some("Movies"));
        // El rango más estrecho gana.
        assert_eq!(for_newznab(&s, &[5000, 5070]).as_deref(), Some("anime"));
        assert_eq!(for_newznab(&s, &[7020]), None);
        // Una categoría explícita manda sobre el mapeo.
        assert_eq!(resolve(&s, Some("movies"), &[5040]).category, "Movies");
        assert_eq!(resolve(&s, None, &[5040]).category, "tv");
        assert_eq!(parse_ranges(" 5000-5999 ,7020;100 - 50 x"), vec![(5000, 5999), (7020, 7020), (50, 100)]);
    }

    #[test]
    fn pp_levels_round_trip() {
        let s = Settings::default();
        for pp in 0..=3 {
            assert_eq!(pp_of(&options_for_pp(&s, pp)), pp);
        }
    }

    #[test]
    fn validation() {
        let mut ok = settings().categories;
        assert!(validate(&mut ok).is_ok());
        let mut dup = vec![Category { name: "tv".into(), ..Default::default() }, Category { name: " TV ".into(), ..Default::default() }];
        assert!(validate(&mut dup).is_err());
        for bad in ["", "*", "Default", "a/b"] {
            assert!(validate(&mut vec![Category { name: bad.into(), ..Default::default() }]).is_err(), "{bad}");
        }
        let mut fix = vec![Category { name: "x".into(), priority: 9, pp: 7, ..Default::default() }];
        validate(&mut fix).unwrap();
        assert_eq!((fix[0].priority, fix[0].pp), (PRIO_FORCE, PP_GLOBAL));
    }

    #[test]
    fn absolute_dirs_stay() {
        let mut s = settings();
        s.categories[0].dir = "D:\\Series".into();
        assert_eq!(dest_dir(&s, s.categories.first()), PathBuf::from("D:\\Series"));
    }
}
