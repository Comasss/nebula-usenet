//! Importar la configuración de SABnzbd (`sabnzbd.ini`) y NZBGet (`nzbget.conf`), y los
//! indexadores de Prowlarr o NZBHydra2.
//!
//! La lectura no toca nada: devuelve un [`Imported`] que la interfaz enseña con casillas, y
//! [`apply`] añade a los ajustes actuales lo elegido. Los servidores y categorías que ya existen
//! no se duplican.
use crate::categories::{Category, PP_GLOBAL, PRIO_FORCE, PRIO_HIGH, PRIO_LOW, PRIO_NORMAL};
use crate::config::{IndexerConfig, ScheduleRule, ServerConfig, Settings};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Imported {
    /// "sabnzbd" o "nzbget".
    pub source: String,
    pub servers: Vec<ServerConfig>,
    pub categories: Vec<Category>,
    /// Ajustes sueltos con su nombre en `Settings` (camelCase): carpetas, velocidad, opciones…
    pub settings: Map<String, Value>,
    pub schedule: Vec<ScheduleRule>,
    /// Lo que no se puede importar, explicado.
    pub warnings: Vec<String>,
}

/// Qué aplicar de lo importado.
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Choice {
    /// Ids de los servidores.
    pub servers: Vec<String>,
    /// Nombres de las categorías.
    pub categories: Vec<String>,
    /// Claves de `settings`.
    pub settings: Vec<String>,
    pub schedule: bool,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub servers: usize,
    pub categories: usize,
    pub settings: usize,
    pub schedule: usize,
    /// Servidores y categorías que ya estaban y no se han añadido.
    pub skipped: usize,
}

/// Lee un archivo de configuración. `kind`: "sabnzbd", "nzbget" o vacío para adivinarlo.
pub fn parse(kind: &str, text: &str) -> Result<Imported, String> {
    let kind = match kind {
        "sabnzbd" | "nzbget" => kind.to_string(),
        _ => detect(text).ok_or_else(|| t!("imp.unknownFormat"))?.to_string(),
    };
    let imp = if kind == "sabnzbd" { parse_sabnzbd(text) } else { parse_nzbget(text) };
    if imp.servers.is_empty() && imp.categories.is_empty() && imp.settings.is_empty() && imp.schedule.is_empty() {
        return Err(t!("imp.nothing"));
    }
    Ok(imp)
}

pub fn detect(text: &str) -> Option<&'static str> {
    let has = |s: &str| text.lines().any(|l| l.trim_start().starts_with(s));
    if has("[misc]") || has("[servers]") || has("[[") {
        Some("sabnzbd")
    } else if text.lines().any(|l| {
        let l = l.trim_start();
        l.starts_with("Server1.") || l.starts_with("MainDir") || l.starts_with("DestDir") || l.starts_with("Category1.")
    }) {
        Some("nzbget")
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Utilidades
// ---------------------------------------------------------------------------

fn unquote(v: &str) -> String {
    let v = v.trim();
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return v[1..v.len() - 1].to_string();
        }
    }
    v.to_string()
}

fn truthy(v: &str) -> bool {
    matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "yes" | "true" | "on")
}

/// "500G", "100M", "1.5T", "1024" → bytes (base 1024).
pub fn parse_size(v: &str) -> Option<u64> {
    let v = v.trim().trim_end_matches(['b', 'B']);
    if v.is_empty() {
        return None;
    }
    let (num, mult) = match v.chars().last()?.to_ascii_uppercase() {
        'K' => (&v[..v.len() - 1], 1u64 << 10),
        'M' => (&v[..v.len() - 1], 1 << 20),
        'G' => (&v[..v.len() - 1], 1 << 30),
        'T' => (&v[..v.len() - 1], 1 << 40),
        _ => (v, 1),
    };
    let n: f64 = num.trim().parse().ok()?;
    (n >= 0.0 && n.is_finite()).then(|| (n * mult as f64) as u64)
}

fn is_absolute(p: &str) -> bool {
    let b = p.as_bytes();
    p.starts_with('/') || p.starts_with("\\\\") || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
}

fn new_rule(time: String, days: Vec<u8>, action: &str, value: u64) -> ScheduleRule {
    ScheduleRule { time, days, action: action.into(), value, ..Default::default() }
}

fn valid_category(name: &str) -> bool {
    !name.is_empty() && !crate::categories::is_default(name) && !name.chars().any(|c| c.is_control() || "/\\\"<>|?*".contains(c))
}

// ---------------------------------------------------------------------------
// SABnzbd
// ---------------------------------------------------------------------------

/// Secciones de un archivo ConfigObj: (sección, subsección) → claves.
type Ini = Vec<((String, Option<String>), HashMap<String, String>)>;

fn parse_ini(text: &str) -> Ini {
    let mut out: Ini = Vec::new();
    let mut top = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with("[[") && line.ends_with("]]") {
            let name = unquote(&line[2..line.len() - 2]);
            out.push(((top.clone(), Some(name)), HashMap::new()));
        } else if line.starts_with('[') && line.ends_with(']') {
            top = unquote(&line[1..line.len() - 1]).to_ascii_lowercase();
            out.push(((top.clone(), None), HashMap::new()));
        } else if let Some((k, v)) = line.split_once('=') {
            if out.is_empty() {
                out.push(((String::new(), None), HashMap::new()));
            }
            if let Some(last) = out.last_mut() {
                last.1.insert(k.trim().to_ascii_lowercase(), unquote(v));
            }
        }
    }
    out
}

fn sab_priority(v: &str) -> i32 {
    match v.trim().parse::<i32>().unwrap_or(-100) {
        -1 => PRIO_LOW,
        1 => PRIO_HIGH,
        2 => PRIO_FORCE,
        _ => PRIO_NORMAL,
    }
}

pub fn parse_sabnzbd(text: &str) -> Imported {
    let ini = parse_ini(text);
    let mut imp = Imported { source: "sabnzbd".into(), ..Default::default() };
    let empty = HashMap::new();
    let misc = ini.iter().find(|((s, sub), _)| s == "misc" && sub.is_none()).map(|(_, m)| m).unwrap_or(&empty);
    let g = |k: &str| misc.get(k).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    for ((sec, sub), m) in &ini {
        let Some(sub) = sub else { continue };
        let f = |k: &str| m.get(k).map(|s| s.trim().to_string()).unwrap_or_default();
        match sec.as_str() {
            "servers" => {
                let host = if f("host").is_empty() { sub.clone() } else { f("host") };
                if host.is_empty() {
                    continue;
                }
                let ssl = truthy(&f("ssl"));
                let mut s = ServerConfig {
                    name: [f("displayname"), f("name")].into_iter().find(|n| !n.is_empty() && *n != host).unwrap_or_default(),
                    host,
                    port: f("port").parse().unwrap_or(if ssl { 563 } else { 119 }),
                    ssl,
                    verify_cert: f("ssl_verify") != "0",
                    username: f("username"),
                    password: f("password"),
                    connections: f("connections").parse::<u32>().unwrap_or(8).clamp(1, 200),
                    priority: f("priority").parse().unwrap_or(0),
                    enabled: m.get("enable").map_or(true, |v| truthy(v)),
                    retention_days: f("retention").parse().unwrap_or(0),
                    ..Default::default()
                };
                if let Some(q) = parse_size(&f("quota")).filter(|q| *q > 0) {
                    s.quota_gb = (q as f64 / (1u64 << 30) as f64 * 1000.0).round() / 1000.0;
                    s.quota_period = "never".into();
                }
                imp.servers.push(s);
            }
            "categories" => {
                let name = if f("name").is_empty() { sub.clone() } else { f("name") };
                if !valid_category(&name) {
                    continue;
                }
                let dir = f("dir");
                let pp = match f("pp").as_str() {
                    "" => PP_GLOBAL,
                    v => v.parse::<i32>().ok().filter(|p| (0..=3).contains(p)).unwrap_or(PP_GLOBAL),
                };
                let newznab = f("newzbin");
                let newznab = if !newznab.is_empty() && newznab.chars().all(|c| c.is_ascii_digit() || " ,-".contains(c)) { newznab } else { String::new() };
                imp.categories.push(Category { name, dir, priority: sab_priority(&f("priority")), pp, newznab, ..Default::default() });
            }
            _ => {}
        }
    }

    let set = &mut imp.settings;
    for (key, target) in [("complete_dir", "downloadDir"), ("download_dir", "incompleteDir")] {
        if let Some(d) = g(key) {
            if is_absolute(&d) {
                set.insert(target.into(), json!(d));
            } else {
                imp.warnings.push(t!("imp.relativeDir", key = key, dir = d));
            }
        }
    }
    for (key, target) in [("pre_check", "precheck"), ("direct_unpack", "directUnpack"), ("deobfuscate_final_filenames", "deobfuscate")] {
        if let Some(v) = g(key) {
            set.insert(target.into(), json!(truthy(&v)));
        }
    }
    if let Some(v) = g("enable_par_cleanup") {
        set.insert("deletePar2".into(), json!(truthy(&v)));
    }
    if let Some(v) = g("enable_unrar").or_else(|| g("enable_7zip")) {
        set.insert("extractEnabled".into(), json!(truthy(&v)));
    }
    if let Some(v) = g("unwanted_extensions") {
        let list: Vec<String> = v.split(',').map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase()).filter(|e| !e.is_empty()).collect();
        if !list.is_empty() {
            set.insert("unwantedExtensions".into(), json!(list));
            let action = match g("action_on_unwanted_extensions").as_deref() {
                Some("2") => "fail",
                Some("1") => "warn",
                _ => "off",
            };
            set.insert("unwantedAction".into(), json!(action));
        }
    }
    if let Some(v) = g("no_dupes") {
        let a = match v.as_str() {
            "1" | "3" => "skip",
            "2" => "pause",
            "4" => "warn",
            _ => "off",
        };
        set.insert("duplicateAction".into(), json!(a));
    }
    if g("password_file").is_some() {
        imp.warnings.push(t!("imp.passwordFile"));
    }
    if g("quota_size").is_some() {
        imp.warnings.push(t!("imp.globalQuota"));
    }
    if ini.iter().any(|((s, sub), _)| s == "rss" && sub.is_some()) {
        imp.warnings.push(t!("imp.rss"));
    }
    if ini.iter().any(|((s, sub), m)| s == "categories" && sub.is_some() && m.get("script").is_some_and(|v| !v.is_empty() && v != "None" && v != "Default")) {
        imp.warnings.push(t!("imp.scripts"));
    }

    // Programador: "1 30 6 1234567 pause", "0 22 12345 speedlimit 50"…
    let max_bps = g("bandwidth_max").and_then(|v| parse_size(&v)).unwrap_or(0);
    if let Some(lines) = g("schedlines") {
        for item in lines.split(',') {
            let tok: Vec<&str> = item.split_whitespace().map(|s| s.trim_matches('"')).filter(|s| !s.is_empty()).collect();
            if tok.is_empty() {
                continue;
            }
            // Con o sin el primer campo "activa" (versiones antiguas no lo tienen).
            let numeric = tok.iter().take_while(|t| t.chars().all(|c| c.is_ascii_digit())).count();
            let (enabled, rest) = if numeric >= 4 { (tok[0] == "1", &tok[1..]) } else { (true, &tok[..]) };
            if rest.len() < 4 {
                continue;
            }
            let (Ok(min), Ok(hour)) = (rest[0].parse::<u32>(), rest[1].parse::<u32>()) else { continue };
            let days: Vec<u8> = rest[2].chars().filter_map(|c| c.to_digit(10)).filter(|d| (1..=7).contains(d)).map(|d| d as u8).collect();
            let days = if days.len() == 7 { Vec::new() } else { days };
            let time = format!("{:02}:{:02}", hour.min(23), min.min(59));
            let mut rule = match rest[3] {
                "pause" | "pause_all" => new_rule(time, days, "pause", 0),
                "resume" | "resume_all" => new_rule(time, days, "resume", 0),
                "speedlimit" => {
                    let arg = rest.get(4).copied().unwrap_or("0");
                    let kbps = if arg.chars().all(|c| c.is_ascii_digit() || c == '.') {
                        let pct: f64 = arg.parse().unwrap_or(0.0);
                        if pct <= 0.0 || pct >= 100.0 {
                            Some(0)
                        } else if max_bps > 0 {
                            Some((max_bps as f64 * pct / 100.0 / 1024.0) as u64)
                        } else {
                            None
                        }
                    } else {
                        parse_size(arg).map(|b| b / 1024)
                    };
                    match kbps {
                        Some(k) => new_rule(time, days, "speed", k),
                        None => {
                            imp.warnings.push(t!("imp.schedSkipped", task = item.trim()));
                            continue;
                        }
                    }
                }
                _ => {
                    imp.warnings.push(t!("imp.schedSkipped", task = item.trim()));
                    continue;
                }
            };
            rule.enabled = enabled;
            imp.schedule.push(rule);
        }
    }
    imp
}

// ---------------------------------------------------------------------------
// NZBGet
// ---------------------------------------------------------------------------

/// "1-5", "1,3,5-7", "" → días (vacío = todos).
fn weekdays(v: &str) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for part in v.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = part.split_once('-').unwrap_or((part, part));
        if let (Ok(a), Ok(b)) = (a.trim().parse::<u8>(), b.trim().parse::<u8>()) {
            for d in a.min(b)..=a.max(b) {
                if (1..=7).contains(&d) && !out.contains(&d) {
                    out.push(d);
                }
            }
        }
    }
    out.sort_unstable();
    if out.len() == 7 {
        Vec::new()
    } else {
        out
    }
}

pub fn parse_nzbget(text: &str) -> Imported {
    let mut imp = Imported { source: "nzbget".into(), ..Default::default() };
    let mut kv: HashMap<String, String> = HashMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            kv.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    // ${MainDir} y otras variables con el valor de otra opción.
    let expand = |v: &str, kv: &HashMap<String, String>| -> String {
        let mut s = v.to_string();
        for _ in 0..4 {
            let Some(a) = s.find("${") else { break };
            let Some(b) = s[a..].find('}') else { break };
            let name = s[a + 2..a + b].to_ascii_lowercase();
            let val = kv.get(&name).cloned().unwrap_or_default();
            s.replace_range(a..a + b + 1, &val);
        }
        s
    };
    let g = |k: &str| kv.get(k).map(|v| expand(v, &kv)).filter(|v| !v.is_empty());

    let mut n = 1;
    while let Some(host) = g(&format!("server{}.host", n)) {
        let f = |k: &str| g(&format!("server{}.{}", n, k)).unwrap_or_default();
        let ssl = truthy(&f("encryption"));
        let cert = f("certverification").to_ascii_lowercase();
        imp.servers.push(ServerConfig {
            name: f("name"),
            host,
            port: f("port").parse().unwrap_or(if ssl { 563 } else { 119 }),
            ssl,
            verify_cert: !matches!(cert.as_str(), "no" | "none"),
            username: f("username"),
            password: f("password"),
            connections: f("connections").parse::<u32>().unwrap_or(4).clamp(1, 200),
            priority: f("level").parse().unwrap_or(0),
            enabled: g(&format!("server{}.active", n)).map_or(true, |v| truthy(&v)),
            retention_days: f("retention").parse().unwrap_or(0),
            ..Default::default()
        });
        n += 1;
    }
    let mut n = 1;
    while let Some(name) = g(&format!("category{}.name", n)) {
        let f = |k: &str| g(&format!("category{}.{}", n, k)).unwrap_or_default();
        if valid_category(&name) {
            let pp = if f("unpack").eq_ignore_ascii_case("no") { 1 } else { PP_GLOBAL };
            imp.categories.push(Category { name, dir: f("destdir"), pp, ..Default::default() });
        }
        n += 1;
    }

    let set = &mut imp.settings;
    for (key, target) in [("destdir", "downloadDir"), ("interdir", "incompleteDir")] {
        if let Some(d) = g(key) {
            if is_absolute(&d) || d.starts_with('~') {
                set.insert(target.into(), json!(d));
            } else {
                imp.warnings.push(t!("imp.relativeDir", key = key, dir = d));
            }
        }
    }
    if let Some(v) = g("downloadrate").and_then(|v| v.parse::<u64>().ok()) {
        set.insert("speedLimitKbps".into(), json!(v));
    }
    if let Some(v) = g("parcheck") {
        set.insert("par2Enabled".into(), json!(!v.eq_ignore_ascii_case("manual")));
    }
    for (key, target) in [("unpack", "extractEnabled"), ("unpackcleanupdisk", "deleteArchives"), ("directunpack", "directUnpack")] {
        if let Some(v) = g(key) {
            set.insert(target.into(), json!(truthy(&v)));
        }
    }
    if let Some(v) = g("healthcheck") {
        set.insert("abortHopeless".into(), json!(!v.eq_ignore_ascii_case("none")));
    }
    if let Some(v) = g("dupecheck") {
        set.insert("duplicateAction".into(), json!(if truthy(&v) { "warn" } else { "off" }));
    }
    if g("unpackpassfile").is_some() {
        imp.warnings.push(t!("imp.passwordFile"));
    }
    if g("monthlyquota").is_some_and(|v| v != "0") || g("dailyquota").is_some_and(|v| v != "0") {
        imp.warnings.push(t!("imp.globalQuota"));
    }
    if g("extensions").is_some() || g("scriptorder").is_some() {
        imp.warnings.push(t!("imp.scripts"));
    }

    let mut n = 1;
    while let Some(times) = g(&format!("task{}.time", n)) {
        let f = |k: &str| g(&format!("task{}.{}", n, k)).unwrap_or_default();
        let days = weekdays(&f("weekdays"));
        let cmd = f("command").to_ascii_lowercase();
        let (action, value) = match cmd.as_str() {
            "pausedownload" | "pause" => ("pause", 0),
            "unpausedownload" | "unpause" => ("resume", 0),
            "downloadrate" => ("speed", f("param").parse::<u64>().unwrap_or(0)),
            _ => {
                imp.warnings.push(t!("imp.schedSkipped", task = format!("{} {}", times, f("command"))));
                n += 1;
                continue;
            }
        };
        for t in times.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            let Some((h, m)) = t.split_once(':') else { continue };
            let (Ok(h), Ok(m)) = (h.trim().parse::<u32>(), m.trim().parse::<u32>()) else {
                imp.warnings.push(t!("imp.schedSkipped", task = t));
                continue;
            };
            imp.schedule.push(new_rule(format!("{:02}:{:02}", h.min(23), m.min(59)), days.clone(), action, value));
        }
        n += 1;
    }
    imp
}

// ---------------------------------------------------------------------------
// Aplicar
// ---------------------------------------------------------------------------

fn same_server(a: &ServerConfig, b: &ServerConfig) -> bool {
    a.host.trim().eq_ignore_ascii_case(b.host.trim()) && a.port == b.port && a.username.trim() == b.username.trim()
}

/// Ajustes con lo elegido de `imp` añadido.
pub fn apply(current: &Settings, imp: &Imported, choice: &Choice) -> Result<(Settings, Applied), String> {
    let mut s = current.clone();
    let mut done = Applied::default();
    for srv in imp.servers.iter().filter(|x| choice.servers.contains(&x.id)) {
        if s.servers.iter().any(|x| same_server(x, srv)) {
            done.skipped += 1;
            continue;
        }
        let mut srv = srv.clone();
        srv.id = crate::util::new_id();
        s.servers.push(srv);
        done.servers += 1;
    }
    for c in imp.categories.iter().filter(|c| choice.categories.contains(&c.name)) {
        if s.categories.iter().any(|x| x.name.eq_ignore_ascii_case(&c.name)) {
            done.skipped += 1;
            continue;
        }
        s.categories.push(c.clone());
        done.categories += 1;
    }
    if choice.schedule {
        for r in &imp.schedule {
            if !s.schedule.iter().any(|x| x.time == r.time && x.days == r.days && x.action == r.action && x.value == r.value) {
                let mut r = r.clone();
                r.id = crate::util::new_id();
                s.schedule.push(r);
                done.schedule += 1;
            }
        }
    }
    let picked: Map<String, Value> = imp.settings.iter().filter(|(k, _)| choice.settings.contains(k)).map(|(k, v)| (k.clone(), v.clone())).collect();
    if !picked.is_empty() {
        let mut v = serde_json::to_value(&s).map_err(|e| e.to_string())?;
        if let Some(obj) = v.as_object_mut() {
            for (k, val) in &picked {
                if obj.contains_key(k) {
                    obj.insert(k.clone(), val.clone());
                    done.settings += 1;
                }
            }
        }
        s = serde_json::from_value(v).map_err(|e| e.to_string())?;
    }
    Ok((s, done))
}

// ---------------------------------------------------------------------------
// Prowlarr y NZBHydra2
// ---------------------------------------------------------------------------

/// Indexadores que ofrece un Prowlarr (cada indexador Usenet por su proxy Newznab) o un
/// endpoint Newznab como NZBHydra2 (uno solo, que busca en todos los suyos).
pub async fn discover_indexers(url: &str, api_key: &str) -> Result<Vec<IndexerConfig>, String> {
    let base = url.trim().trim_end_matches('/').trim_end_matches("/api").to_string();
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err(t!("indexer.scheme"));
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!("Nebula/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    // Prowlarr: /api/v1/indexer con la cabecera X-Api-Key.
    if let Ok(resp) = client.get(format!("{}/api/v1/indexer", base)).header("X-Api-Key", api_key.trim()).send().await {
        let status = resp.status();
        if status.is_success() {
            if let Ok(Value::Array(list)) = resp.text().await.map(|t| serde_json::from_str::<Value>(&t).unwrap_or(Value::Null)) {
                let found = prowlarr_indexers(&base, api_key, &list);
                return if found.is_empty() { Err(t!("imp.prowlarrEmpty")) } else { Ok(found) };
            }
        } else if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(t!("indexer.badKey"));
        }
    }
    // NZBHydra2 o cualquier Newznab.
    let ix = IndexerConfig { id: crate::util::new_id(), name: String::new(), url: base.clone(), api_key: api_key.trim().to_string(), api_path: "/api".into(), enabled: true, ..Default::default() };
    let test = crate::indexer::test(&ix).await;
    if !test.ok {
        return Err(test.message);
    }
    let name = crate::indexer::caps_title(&ix).await.unwrap_or_else(|| "NZBHydra2".into());
    Ok(vec![IndexerConfig { name, ..ix }])
}

fn prowlarr_indexers(base: &str, api_key: &str, list: &[Value]) -> Vec<IndexerConfig> {
    list.iter()
        .filter(|i| i.get("protocol").and_then(Value::as_str) == Some("usenet"))
        .filter_map(|i| {
            let id = i.get("id")?.as_i64()?;
            let name = i.get("name").and_then(Value::as_str).unwrap_or("Prowlarr");
            Some(IndexerConfig {
                id: crate::util::new_id(),
                name: format!("{} (Prowlarr)", name),
                url: format!("{}/{}", base, id),
                api_key: api_key.trim().to_string(),
                api_path: "/api".into(),
                enabled: i.get("enable").and_then(Value::as_bool).unwrap_or(true),
                ..Default::default()
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAB: &str = r#"
__version__ = 19
[misc]
complete_dir = /data/complete
download_dir = Downloads/incomplete
pre_check = 1
direct_unpack = 0
deobfuscate_final_filenames = 1
unwanted_extensions = exe, .com
action_on_unwanted_extensions = 2
no_dupes = 2
bandwidth_max = 100M
password_file = /config/pw.txt
schedlines = 1 30 6 12345 pause "", 1 0 22 1234567 resume "", 0 0 1 67 speedlimit 50, 1 15 2 1234567 speedlimit 5M, 1 0 3 1234567 rss_scan ""
[servers]
[[news.example.com]]
name = news.example.com
displayname = Example
host = news.example.com
port = 563
username = "user"
password = "p@ss = word"
connections = 30
ssl = 1
ssl_verify = 0
enable = 1
priority = 0
retention = 4000
quota = 500G
[[block.example.org]]
host = block.example.org
port = 119
ssl = 0
enable = 0
priority = 1
[categories]
[[*]]
name = *
pp = 3
dir = ""
[[tv]]
name = tv
pp = ""
dir = tv
newzbin = "5000-5999"
priority = 1
[[movies]]
name = movies
pp = 1
dir = /data/movies
newzbin = Movies > HD
priority = -100
"#;

    #[test]
    fn sabnzbd() {
        assert_eq!(detect(SAB), Some("sabnzbd"));
        let imp = parse("", SAB).unwrap();
        assert_eq!(imp.servers.len(), 2);
        let s = &imp.servers[0];
        assert_eq!((s.host.as_str(), s.port, s.ssl, s.verify_cert, s.connections), ("news.example.com", 563, true, false, 30));
        assert_eq!((s.name.as_str(), s.username.as_str(), s.password.as_str(), s.retention_days), ("Example", "user", "p@ss = word", 4000));
        assert_eq!((s.quota_gb, s.quota_period.as_str()), (500.0, "never"));
        assert!(!imp.servers[1].enabled);
        assert_eq!(imp.servers[1].priority, 1);
        assert_eq!(imp.categories.len(), 2);
        assert_eq!((imp.categories[0].name.as_str(), imp.categories[0].priority, imp.categories[0].pp), ("tv", PRIO_HIGH, PP_GLOBAL));
        assert_eq!(imp.categories[0].newznab, "5000-5999");
        assert_eq!((imp.categories[1].pp, imp.categories[1].newznab.as_str()), (1, ""));
        assert_eq!(imp.settings["downloadDir"], "/data/complete");
        assert!(!imp.settings.contains_key("incompleteDir"));
        assert_eq!(imp.settings["precheck"], true);
        assert_eq!(imp.settings["unwantedExtensions"], json!(["exe", "com"]));
        assert_eq!(imp.settings["unwantedAction"], "fail");
        assert_eq!(imp.settings["duplicateAction"], "pause");
        let sched: Vec<(String, String, u64, bool, Vec<u8>)> = imp.schedule.iter().map(|r| (r.time.clone(), r.action.clone(), r.value, r.enabled, r.days.clone())).collect();
        assert_eq!(
            sched,
            vec![
                ("06:30".into(), "pause".into(), 0, true, vec![1, 2, 3, 4, 5]),
                ("22:00".into(), "resume".into(), 0, true, vec![]),
                ("01:00".into(), "speed".into(), 51200, false, vec![6, 7]),
                ("02:15".into(), "speed".into(), 5120, true, vec![]),
            ]
        );
        // Carpeta relativa, archivo de contraseñas y la tarea rss_scan.
        assert_eq!(imp.warnings.len(), 3, "{:?}", imp.warnings);
    }

    const NZBGET: &str = r#"
MainDir=/srv/nzb
DestDir=${MainDir}/dst
InterDir=${MainDir}/inter
DownloadRate=2048
ParCheck=auto
Unpack=yes
UnpackCleanupDisk=no
DirectUnpack=yes
HealthCheck=none
DupeCheck=yes
Server1.Active=yes
Server1.Name=Main
Server1.Level=0
Server1.Host=news.example.com
Server1.Port=563
Server1.Username=u
Server1.Password=p
Server1.Encryption=yes
Server1.Connections=20
Server1.Retention=3000
Server2.Active=no
Server2.Host=fill.example.net
Server2.Level=1
Category1.Name=Movies
Category1.DestDir=
Category1.Unpack=yes
Category2.Name=Software
Category2.DestDir=/srv/soft
Category2.Unpack=no
Task1.Time=08:00,12:30
Task1.WeekDays=1-5
Task1.Command=PauseDownload
Task2.Time=18:00
Task2.WeekDays=
Task2.Command=DownloadRate
Task2.Param=500
Task3.Time=*:00
Task3.Command=UnpauseDownload
Task4.Time=03:00
Task4.Command=Script
"#;

    #[test]
    fn nzbget() {
        assert_eq!(detect(NZBGET), Some("nzbget"));
        let imp = parse("", NZBGET).unwrap();
        assert_eq!(imp.servers.len(), 2);
        let s = &imp.servers[0];
        assert_eq!((s.name.as_str(), s.host.as_str(), s.port, s.ssl, s.connections, s.retention_days), ("Main", "news.example.com", 563, true, 20, 3000));
        assert_eq!((imp.servers[1].port, imp.servers[1].enabled, imp.servers[1].priority), (119, false, 1));
        assert_eq!(imp.categories.iter().map(|c| (c.name.as_str(), c.pp)).collect::<Vec<_>>(), vec![("Movies", PP_GLOBAL), ("Software", 1)]);
        assert_eq!(imp.settings["downloadDir"], "/srv/nzb/dst");
        assert_eq!(imp.settings["incompleteDir"], "/srv/nzb/inter");
        assert_eq!(imp.settings["speedLimitKbps"], 2048);
        assert_eq!(imp.settings["deleteArchives"], false);
        assert_eq!(imp.settings["directUnpack"], true);
        assert_eq!(imp.settings["abortHopeless"], false);
        let sched: Vec<(String, String, u64, Vec<u8>)> = imp.schedule.iter().map(|r| (r.time.clone(), r.action.clone(), r.value, r.days.clone())).collect();
        assert_eq!(
            sched,
            vec![
                ("08:00".into(), "pause".into(), 0, vec![1, 2, 3, 4, 5]),
                ("12:30".into(), "pause".into(), 0, vec![1, 2, 3, 4, 5]),
                ("18:00".into(), "speed".into(), 500, vec![]),
            ]
        );
        // "*:00" y la tarea de script.
        assert_eq!(imp.warnings.len(), 2, "{:?}", imp.warnings);
    }

    #[test]
    fn applies_without_duplicates() {
        let imp = parse("sabnzbd", SAB).unwrap();
        let mut cur = Settings::default();
        cur.servers.push(ServerConfig { host: "NEWS.example.com".into(), port: 563, username: "user".into(), ..Default::default() });
        cur.categories.push(Category { name: "TV".into(), ..Default::default() });
        let choice = Choice {
            servers: imp.servers.iter().map(|s| s.id.clone()).collect(),
            categories: vec!["tv".into(), "movies".into()],
            settings: vec!["precheck".into(), "downloadDir".into(), "bogus".into()],
            schedule: true,
        };
        let (s, done) = apply(&cur, &imp, &choice).unwrap();
        assert_eq!(done, Applied { servers: 1, categories: 1, settings: 2, schedule: 4, skipped: 2 });
        assert_eq!(s.servers.len(), 2);
        assert_eq!(s.servers[1].host, "block.example.org");
        assert!(s.precheck);
        assert_eq!(s.download_dir, "/data/complete");
        assert_eq!(s.categories.len(), 2);
        // Aplicar otra vez no repite nada.
        let (_, again) = apply(&s, &imp, &choice).unwrap();
        assert_eq!((again.servers, again.categories, again.schedule), (0, 0, 0));
    }

    #[test]
    fn prowlarr_list() {
        let list: Vec<Value> = serde_json::from_str(
            r#"[{"id":1,"name":"NZBgeek","protocol":"usenet","enable":true},{"id":2,"name":"1337x","protocol":"torrent","enable":true},{"id":7,"name":"DrunkenSlug","protocol":"usenet","enable":false}]"#,
        )
        .unwrap();
        let v = prowlarr_indexers("http://prowlarr:9696", "key", &list);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].name.as_str(), v[0].url.as_str(), v[0].api_path.as_str()), ("NZBgeek (Prowlarr)", "http://prowlarr:9696/1", "/api"));
        assert!(!v[1].enabled);
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("500G"), Some(500 << 30));
        assert_eq!(parse_size("1.5M"), Some(1572864));
        assert_eq!(parse_size(""), None);
        assert_eq!(weekdays("1,3,5-7"), vec![1, 3, 5, 6, 7]);
        assert!(weekdays("1-7").is_empty());
        assert!(is_absolute("C:\\Data") && is_absolute("/srv") && !is_absolute("Downloads"));
    }
}
