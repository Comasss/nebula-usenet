//! API compatible con SABnzbd: Sonarr, Radarr, Lidarr, Readarr, Prowlarr… pueden añadir Nebula como
//! cliente de descargas «SABnzbd». Comparte servidor (y puerto) con la API Newznab de api.rs: las
//! peticiones con `mode=` vienen aquí.
//!
//! Igual que SABnzbd, lo que está en post-procesado (verificar, reparar, extraer, mover) sale en
//! `history` con su estado y no en `queue`: Sonarr lo busca ahí y lo importa al pasar a Completed.
//! Ver docs/SABNZBD.md.

use crate::categories::{self, DEFAULT};
use crate::config::Settings;
use crate::debrid_job::DebridPhase;
use crate::engine::{HistoryEntry, JobStatus, JobView, Snapshot};
use serde_json::{json, Value};
use std::future::Future;

/// Versión de SABnzbd que se anuncia (Sonarr y Radarr exigen una reciente).
pub const SAB_VERSION: &str = "4.5.1";
const NZO_PREFIX: &str = "SABnzbd_nzo_";
const MB: f64 = 1024.0 * 1024.0;

/// Archivo subido en un formulario multipart.
#[derive(Debug, Clone)]
pub struct Upload {
    pub field: String,
    pub filename: String,
    pub data: Vec<u8>,
}

/// Lo que pide `addfile` / `addurl` además del NZB.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AddOptions {
    /// Nombre del trabajo (`nzbname`).
    pub name: Option<String>,
    /// Categoría (`cat`); None = Default o la del mapeo.
    pub category: Option<String>,
    /// Prioridad -1..2; None = la de la categoría.
    pub priority: Option<i32>,
    /// Nivel de post-procesado 0..3; None = el de la categoría.
    pub pp: Option<i32>,
    /// Prioridad -2: se añade en pausa.
    pub paused: bool,
}

/// Lo que la API necesita del motor. `Engine` lo implementa; los tests usan uno simulado.
pub trait SabBackend: Send + Sync {
    fn settings(&self) -> Settings;
    fn queue(&self) -> Snapshot;
    fn history(&self) -> Vec<HistoryEntry>;
    /// Añade un NZB y devuelve el id del trabajo.
    fn add_nzb(&self, data: Vec<u8>, filename: String, opts: AddOptions) -> impl Future<Output = Result<String, String>> + Send;
    /// Descarga un NZB de un enlace: (datos, nombre del archivo si lo trae).
    fn fetch_nzb(&self, url: String) -> impl Future<Output = Result<(Vec<u8>, String), String>> + Send;
    fn pause_job(&self, id: &str);
    fn resume_job(&self, id: &str);
    fn remove_job(&self, id: &str, delete_files: bool);
    fn set_priority(&self, id: &str, priority: i32) -> bool;
    fn set_category(&self, id: &str, category: &str) -> bool;
    fn set_paused(&self, paused: bool);
    fn set_speed_limit(&self, kbps: u64) -> Result<(), String>;
    fn remove_history(&self, id: &str, delete_files: bool);
    fn retry(&self, id: &str) -> Result<String, String>;
    fn disk_free(&self, path: &str) -> Option<u64>;
}

/// Petición ya decodificada: parámetros (query + formulario) y archivos.
pub struct Call<'a> {
    pub params: &'a [(String, String)],
    pub files: &'a [Upload],
}

impl Call<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }
    fn str(&self, key: &str) -> &str {
        self.get(key).map(str::trim).unwrap_or("")
    }
    fn flag(&self, key: &str) -> bool {
        matches!(self.str(key), "1" | "true" | "True" | "yes")
    }
    fn int(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|v| v.trim().parse().ok())
    }
}

pub fn nzo_id(id: &str) -> String {
    format!("{}{}", NZO_PREFIX, id)
}

/// Id del trabajo a partir de un nzo_id (con o sin prefijo).
pub fn job_id(nzo: &str) -> &str {
    let nzo = nzo.trim();
    nzo.strip_prefix(NZO_PREFIX).unwrap_or(nzo)
}

fn ok() -> Value {
    json!({ "status": true })
}

pub fn fail(msg: &str) -> Value {
    json!({ "status": false, "error": msg })
}

fn same_key(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Atiende una llamada `mode=…`. Devuelve el código HTTP y el JSON.
pub async fn handle<B: SabBackend>(b: &B, call: &Call<'_>) -> (u16, Value) {
    let mode = call.str("mode").to_ascii_lowercase();
    // Como SABnzbd: la versión y el modo de autenticación no piden clave.
    match mode.as_str() {
        "version" => return (200, json!({ "version": SAB_VERSION })),
        "auth" => return (200, json!({ "auth": "apikey" })),
        _ => {}
    }
    let settings = b.settings();
    let key = settings.api_key.trim();
    let given = call.get("apikey").or_else(|| call.get("nzbkey")).unwrap_or("").trim();
    if given.is_empty() {
        return (200, fail("API Key Required"));
    }
    if key.is_empty() || !same_key(given, key) {
        return (200, fail("API Key Incorrect"));
    }
    let v = match mode.as_str() {
        "get_config" => get_config(b, &settings, call),
        "get_cats" => json!({ "categories": category_names(&settings) }),
        "get_scripts" => json!({ "scripts": ["None"] }),
        "addfile" => add_file(b, call).await,
        "addurl" => add_url(b, call).await,
        "queue" => queue(b, &settings, call),
        "history" => history(b, &settings, call),
        "pause" => {
            b.set_paused(true);
            ok()
        }
        "resume" => {
            b.set_paused(false);
            ok()
        }
        "config" => config(b, call),
        "change_cat" => {
            let id = job_id(call.str("value"));
            let cat = call.str("value2");
            json!({ "status": b.set_category(id, if cat.is_empty() { DEFAULT } else { cat }) })
        }
        "retry" => match b.retry(job_id(call.str("value"))) {
            Ok(id) => json!({ "status": true, "nzo_id": nzo_id(&id) }),
            Err(e) => fail(&e),
        },
        "fullstatus" | "status" => fullstatus(b, &settings),
        "warnings" => json!({ "warnings": [] }),
        "server_stats" => json!({ "total": 0, "month": 0, "week": 0, "day": 0, "servers": {} }),
        "" => fail("not implemented"),
        _ => fail("not implemented"),
    };
    (200, v)
}

fn category_names(s: &Settings) -> Vec<String> {
    std::iter::once(DEFAULT.to_string()).chain(s.categories.iter().map(|c| c.name.clone())).collect()
}

fn get_config<B: SabBackend>(b: &B, s: &Settings, call: &Call<'_>) -> Value {
    let global_pp = categories::pp_of(&categories::global_options(s));
    let mut cats = vec![json!({
        "name": DEFAULT, "order": 0, "pp": global_pp.to_string(), "script": "None",
        "dir": "", "newzbin": "", "priority": 0
    })];
    for (i, c) in s.categories.iter().enumerate() {
        cats.push(json!({
            "name": c.name,
            "order": i + 1,
            "pp": if c.pp < 0 { String::new() } else { c.pp.to_string() },
            "script": "Default",
            "dir": c.dir,
            "newzbin": c.newznab,
            "priority": c.priority,
        }));
    }
    let misc = json!({
        "complete_dir": s.download_dir,
        "download_dir": incomplete_dir(s),
        "dirscan_dir": "",
        "nzb_backup_dir": "",
        "script_dir": "",
        "version": SAB_VERSION,
        "pre_check": 0,
        "enable_tv_sorting": 0,
        "enable_movie_sorting": 0,
        "enable_date_sorting": 0,
        "history_retention": "",
        "history_retention_option": "all",
        "history_retention_number": 0,
        "api_key": "",
        "nzb_key": "",
        "port": s.api_port,
        "https_port": "",
        "queue_complete": "",
        "bandwidth_max": "",
        "bandwidth_perc": 100,
    });
    let _ = b;
    let full = json!({ "misc": misc, "categories": cats, "servers": [], "sorters": [], "rss": [], "scheduling": [] });
    match call.str("section") {
        "" => json!({ "config": full }),
        sec => json!({ "config": { sec: full.get(sec).cloned().unwrap_or(Value::Null) } }),
    }
}

fn incomplete_dir(s: &Settings) -> String {
    if s.incomplete_dir.trim().is_empty() {
        std::path::Path::new(&s.download_dir).join("_incompleto").to_string_lossy().to_string()
    } else {
        s.incomplete_dir.clone()
    }
}

fn add_options(call: &Call<'_>) -> AddOptions {
    let prio = call.int("priority").map(|p| p as i32);
    let pp = call.int("pp").map(|p| p as i32).filter(|p| (0..=3).contains(p));
    let name = call.get("nzbname").map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
    let cat = call.get("cat").map(str::trim).filter(|c| !c.is_empty()).map(str::to_string);
    AddOptions {
        name,
        category: cat,
        priority: prio.filter(|p| (-1..=2).contains(p)),
        pp,
        paused: prio == Some(-2),
    }
}

async fn add_file<B: SabBackend>(b: &B, call: &Call<'_>) -> Value {
    let Some(f) = call
        .files
        .iter()
        .find(|f| f.field.eq_ignore_ascii_case("name") || f.field.eq_ignore_ascii_case("nzbfile"))
        .or_else(|| call.files.first())
    else {
        return fail("No file");
    };
    match b.add_nzb(f.data.clone(), f.filename.clone(), add_options(call)).await {
        Ok(id) => json!({ "status": true, "nzo_ids": [nzo_id(&id)] }),
        Err(e) => fail(&e),
    }
}

async fn add_url<B: SabBackend>(b: &B, call: &Call<'_>) -> Value {
    let url = call.str("name").to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return fail("Invalid URL");
    }
    let opts = add_options(call);
    let (data, filename) = match b.fetch_nzb(url).await {
        Ok(r) => r,
        Err(e) => return fail(&e),
    };
    match b.add_nzb(data, filename, opts).await {
        Ok(id) => json!({ "status": true, "nzo_ids": [nzo_id(&id)] }),
        Err(e) => fail(&e),
    }
}

/// "1:02:03" (horas sin límite, como SABnzbd).
pub fn timeleft(secs: u64) -> String {
    format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

fn priority_name(p: i32) -> &'static str {
    match p {
        2.. => "Force",
        1 => "High",
        i32::MIN..=-1 => "Low",
        _ => "Normal",
    }
}

fn mb(bytes: u64) -> String {
    format!("{:.2}", bytes as f64 / MB)
}

/// Estado de SABnzbd para un trabajo de la cola. Ver la tabla de docs/SABNZBD.md.
pub fn sab_status(j: &JobView) -> &'static str {
    match j.status {
        JobStatus::Queued => "Queued",
        JobStatus::Paused => "Paused",
        JobStatus::Downloading if j.checking.is_some() => "Checking",
        JobStatus::Downloading => match j.debrid.as_ref().map(|d| d.phase) {
            Some(DebridPhase::Submit) => "Grabbing",
            _ => "Downloading",
        },
        JobStatus::Verifying => "Verifying",
        JobStatus::Repairing => "Repairing",
        JobStatus::Extracting => "Extracting",
        JobStatus::Moving => "Moving",
        JobStatus::Completed => "Completed",
        JobStatus::Failed => "Failed",
    }
}

fn cat_name(c: &str) -> String {
    if categories::is_default(c) {
        DEFAULT.to_string()
    } else {
        c.to_string()
    }
}

fn ids(value: &str) -> Vec<String> {
    value.split(',').map(job_id).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

fn queue<B: SabBackend>(b: &B, s: &Settings, call: &Call<'_>) -> Value {
    let value = call.str("value");
    match call.str("name").to_ascii_lowercase().as_str() {
        "delete" => {
            let snap = b.queue();
            let del = call.flag("del_files");
            let targets: Vec<String> = if value.eq_ignore_ascii_case("all") {
                snap.jobs.iter().filter(|j| !j.status.is_pp()).map(|j| j.id.clone()).collect()
            } else {
                ids(value)
            };
            let found: Vec<String> = targets.iter().filter(|id| snap.jobs.iter().any(|j| &j.id == *id)).cloned().collect();
            for id in &found {
                b.remove_job(id, del);
            }
            return json!({ "status": true, "nzo_ids": found.iter().map(|i| nzo_id(i)).collect::<Vec<_>>() });
        }
        "pause" if !value.is_empty() => {
            let list = ids(value);
            for id in &list {
                b.pause_job(id);
            }
            return json!({ "status": true, "nzo_ids": list.iter().map(|i| nzo_id(i)).collect::<Vec<_>>() });
        }
        "resume" if !value.is_empty() => {
            let list = ids(value);
            for id in &list {
                b.resume_job(id);
            }
            return json!({ "status": true, "nzo_ids": list.iter().map(|i| nzo_id(i)).collect::<Vec<_>>() });
        }
        "pause" => {
            b.set_paused(true);
            return ok();
        }
        "resume" => {
            b.set_paused(false);
            return ok();
        }
        "priority" => {
            let p = call.int("value2").unwrap_or(0) as i32;
            let id = job_id(value).to_string();
            if p == -2 {
                b.pause_job(&id);
            } else if p != -100 {
                b.set_priority(&id, p);
            }
            let pos = b.queue().jobs.iter().filter(|j| !j.status.is_pp()).position(|j| j.id == id).map(|p| p as i64).unwrap_or(-1);
            return json!({ "position": pos });
        }
        "change_cat" => {
            let cat = call.str("value2");
            return json!({ "status": b.set_category(job_id(value), if cat.is_empty() { DEFAULT } else { cat }) });
        }
        _ => {}
    }
    let snap = b.queue();
    let cat_filter = call.get("category").or_else(|| call.get("cat")).map(str::trim).filter(|c| !c.is_empty()).map(cat_name);
    let search = call.str("search").to_lowercase();
    let nzo_filter: Vec<String> = ids(call.str("nzo_ids"));
    let all: Vec<&JobView> = snap
        .jobs
        .iter()
        .filter(|j| !j.status.is_pp() && !matches!(j.status, JobStatus::Completed | JobStatus::Failed))
        .collect();
    let filtered: Vec<(usize, &JobView)> = all
        .iter()
        .enumerate()
        .filter(|(_, j)| cat_filter.as_ref().map_or(true, |c| cat_name(&j.category).eq_ignore_ascii_case(c)))
        .filter(|(_, j)| search.is_empty() || j.name.to_lowercase().contains(&search))
        .filter(|(_, j)| nzo_filter.is_empty() || nzo_filter.contains(&j.id))
        .map(|(i, j)| (i, *j))
        .collect();
    let start = call.int("start").unwrap_or(0).max(0) as usize;
    let limit = call.int("limit").unwrap_or(0).max(0) as usize;
    let page = filtered.iter().skip(start).take(if limit == 0 { usize::MAX } else { limit });
    let slots: Vec<Value> = page
        .map(|(i, j)| {
            let left = j.total.saturating_sub(j.done + j.failed);
            json!({
                "index": i,
                "nzo_id": nzo_id(&j.id),
                "filename": j.name,
                "cat": cat_name(&j.category),
                "status": sab_status(j),
                "priority": priority_name(j.priority),
                "mb": mb(j.total),
                "mbleft": mb(left),
                "size": crate::util::fmt_bytes(j.total),
                "sizeleft": crate::util::fmt_bytes(left),
                "percentage": format!("{}", j.percent.floor() as u64),
                "timeleft": timeleft(j.eta),
                "eta": "unknown",
                "avg_age": "",
                "labels": [],
                "password": "",
                "script": "None",
                "unpackopts": "3",
                "direct_unpack": Value::Null,
                "missing": j.failed_articles,
                "has_rating": false,
            })
        })
        .collect();
    let left: u64 = all.iter().map(|j| j.total.saturating_sub(j.done + j.failed)).sum();
    let total: u64 = all.iter().map(|j| j.total).sum();
    let downloading = all.iter().any(|j| j.status == JobStatus::Downloading);
    let free = b.disk_free(&s.download_dir);
    json!({ "queue": {
        "status": if snap.paused { "Paused" } else if downloading { "Downloading" } else { "Idle" },
        "paused": snap.paused,
        "paused_all": snap.paused,
        "speedlimit": if snap.speed_limit > 0 { "1" } else { "100" },
        "speedlimit_abs": if snap.speed_limit > 0 { (snap.speed_limit * 1024).to_string() } else { String::new() },
        "kbpersec": format!("{:.2}", snap.speed as f64 / 1024.0),
        "speed": format!("{} ", crate::util::fmt_bytes(snap.speed)).trim().to_string(),
        "mbleft": mb(left),
        "mb": mb(total),
        "sizeleft": crate::util::fmt_bytes(left),
        "size": crate::util::fmt_bytes(total),
        "timeleft": timeleft(snap.eta),
        "eta": "unknown",
        "noofslots": filtered.len(),
        "noofslots_total": all.len(),
        "start": start,
        "limit": limit,
        "finish": 0,
        "diskspace1": free.map(|f| format!("{:.2}", f as f64 / 1024.0 / MB)).unwrap_or_default(),
        "diskspace2": free.map(|f| format!("{:.2}", f as f64 / 1024.0 / MB)).unwrap_or_default(),
        "diskspacetotal1": "",
        "diskspacetotal2": "",
        "have_warnings": "0",
        "pause_int": "0",
        "refresh_rate": "1",
        "version": SAB_VERSION,
        "finishaction": Value::Null,
        "slots": slots,
    }})
}

fn history<B: SabBackend>(b: &B, _s: &Settings, call: &Call<'_>) -> Value {
    let value = call.str("value");
    if call.str("name").eq_ignore_ascii_case("delete") {
        let del = call.flag("del_files");
        let list = b.history();
        let targets: Vec<String> = match value.to_ascii_lowercase().as_str() {
            "all" => list.iter().map(|h| h.id.clone()).collect(),
            "failed" => list.iter().filter(|h| h.status == "failed").map(|h| h.id.clone()).collect(),
            "completed" => list.iter().filter(|h| h.status == "completed").map(|h| h.id.clone()).collect(),
            _ => ids(value),
        };
        for id in &targets {
            b.remove_history(id, del);
        }
        return ok();
    }
    let cat_filter = call.get("category").or_else(|| call.get("cat")).map(str::trim).filter(|c| !c.is_empty()).map(cat_name);
    let failed_only = call.flag("failed_only");
    let search = call.str("search").to_lowercase();
    let nzo_filter: Vec<String> = ids(call.str("nzo_ids"));
    let keep = |name: &str, cat: &str, id: &str, failed: bool| {
        cat_filter.as_ref().map_or(true, |c| cat_name(cat).eq_ignore_ascii_case(c))
            && (!failed_only || failed)
            && (search.is_empty() || name.to_lowercase().contains(&search))
            && (nzo_filter.is_empty() || nzo_filter.iter().any(|n| n == id))
    };
    // Primero lo que está en post-procesado (como SABnzbd), luego lo terminado.
    let snap = b.queue();
    let mut slots: Vec<Value> = snap
        .jobs
        .iter()
        .filter(|j| j.status.is_pp() && keep(&j.name, &j.category, &j.id, false))
        .map(|j| {
            json!({
                "nzo_id": nzo_id(&j.id),
                "name": j.name,
                "nzb_name": format!("{}.nzb", j.name),
                "category": cat_name(&j.category),
                "status": sab_status(j),
                "fail_message": "",
                "storage": "",
                "path": j.dest,
                "bytes": j.total,
                "size": crate::util::fmt_bytes(j.total),
                "download_time": 0,
                "postproc_time": 0,
                "completed": 0,
                "downloaded": j.done,
                "pp": "D",
                "script": "None",
                "action_line": j.post.as_ref().map(|p| p.detail.clone()).unwrap_or_default(),
                "stage_log": [],
                "loaded": false,
                "retry": 0,
                "url": "",
                "report": "",
                "meta": Value::Null,
                "md5sum": "",
                "password": "",
            })
        })
        .collect();
    let hist = b.history();
    let mut total_size = 0u64;
    for h in hist.iter() {
        if h.status == "completed" {
            total_size += h.size;
        }
        let failed = h.status != "completed";
        if !keep(&h.name, &h.category, &h.id, failed) {
            continue;
        }
        slots.push(json!({
            "nzo_id": nzo_id(&h.id),
            "name": h.name,
            "nzb_name": format!("{}.nzb", h.name),
            "category": cat_name(&h.category),
            "status": if failed { "Failed" } else { "Completed" },
            "fail_message": if failed { h.error.clone().unwrap_or_else(|| "Failed".into()) } else { String::new() },
            "storage": if failed { h.work_dir.clone() } else { h.dest.clone() },
            "path": h.work_dir,
            "bytes": h.size,
            "size": crate::util::fmt_bytes(h.size),
            "download_time": h.download_secs,
            "postproc_time": 0,
            "completed": h.completed_at,
            "downloaded": h.size,
            "pp": "D",
            "script": "None",
            "action_line": "",
            "stage_log": [],
            "loaded": false,
            "retry": if failed { 1 } else { 0 },
            "url": "",
            "report": "",
            "meta": Value::Null,
            "md5sum": "",
            "password": h.password.clone().unwrap_or_default(),
            "fail_stage": h.failed_stage.clone().unwrap_or_default(),
        }));
    }
    let n = slots.len();
    let start = call.int("start").unwrap_or(0).max(0) as usize;
    let limit = call.int("limit").unwrap_or(0).max(0) as usize;
    let page: Vec<Value> = slots.into_iter().skip(start).take(if limit == 0 { usize::MAX } else { limit }).collect();
    json!({ "history": {
        "noofslots": n,
        "total_size": crate::util::fmt_bytes(total_size),
        "month_size": "",
        "week_size": "",
        "day_size": "",
        "ppslots": snap.jobs.iter().filter(|j| j.status.is_pp()).count(),
        "slots": page,
    }})
}

/// `mode=config&name=speedlimit&value=…`. Con sufijo K/M/G es absoluto; Nebula no tiene un
/// máximo de línea, así que un número sin sufijo son KB/s (0 o 100 = sin límite).
fn config<B: SabBackend>(b: &B, call: &Call<'_>) -> Value {
    if !call.str("name").eq_ignore_ascii_case("speedlimit") {
        return fail("not implemented");
    }
    let Some(kbps) = parse_speed(call.str("value")) else { return fail("Invalid speed") };
    match b.set_speed_limit(kbps) {
        Ok(()) => ok(),
        Err(e) => fail(&e),
    }
}

pub fn parse_speed(v: &str) -> Option<u64> {
    let v = v.trim().trim_end_matches(['b', 'B']).trim_end_matches('/').trim();
    if v.is_empty() {
        return Some(0);
    }
    let (num, mult) = match v.chars().last()? {
        'k' | 'K' => (&v[..v.len() - 1], 1.0),
        'm' | 'M' => (&v[..v.len() - 1], 1024.0),
        'g' | 'G' => (&v[..v.len() - 1], 1024.0 * 1024.0),
        '%' => {
            let p: f64 = v[..v.len() - 1].trim().parse().ok()?;
            return (p <= 0.0 || p >= 100.0).then_some(0);
        }
        _ => (v, 0.0),
    };
    let n: f64 = num.trim().replace(',', ".").parse().ok()?;
    if n < 0.0 {
        return None;
    }
    if mult == 0.0 {
        // Sin sufijo: KB/s, y 100 se trata como «el 100 %» (sin límite), como lo manda Sonarr al quitarlo.
        return Some(if n == 100.0 { 0 } else { n.round() as u64 });
    }
    Some((n * mult).round() as u64)
}

fn fullstatus<B: SabBackend>(b: &B, s: &Settings) -> Value {
    let snap = b.queue();
    json!({ "status": {
        "version": SAB_VERSION,
        "paused": snap.paused,
        "completedir": s.download_dir,
        "downloaddir": incomplete_dir(s),
        "kbpersec": format!("{:.2}", snap.speed as f64 / 1024.0),
        "speedlimit_abs": if snap.speed_limit > 0 { (snap.speed_limit * 1024).to_string() } else { String::new() },
        "have_warnings": "0",
        "localipv4": "",
        "ipv6": Value::Null,
        "servers": [],
    }})
}

// ---- El motor real ----

impl SabBackend for std::sync::Arc<crate::engine::Engine> {
    fn settings(&self) -> Settings {
        self.settings.read().clone()
    }
    fn queue(&self) -> Snapshot {
        self.snapshot()
    }
    fn history(&self) -> Vec<HistoryEntry> {
        crate::engine::Engine::history(self)
    }
    fn add_nzb(&self, data: Vec<u8>, filename: String, opts: AddOptions) -> impl Future<Output = Result<String, String>> + Send {
        let e = self.clone();
        async move {
            let strong = opts.name.is_some();
            let hint = opts.name.clone().unwrap_or_else(|| filename.clone());
            let engine = e.clone();
            let source = if filename.is_empty() { "SABnzbd API".to_string() } else { filename };
            let preview = tokio::task::spawn_blocking(move || engine.register_pending(data, &hint, strong, source))
                .await
                .map_err(|_| t!("err.internal"))??;
            let settings = e.settings.read().clone();
            // Sin categoría, Default (Sonarr siempre manda la suya). La prioridad y el post-procesado
            // que no se indican salen de la categoría.
            let cat_pp = categories::find(&settings, opts.category.as_deref().unwrap_or(DEFAULT)).map(|c| c.pp).unwrap_or(categories::PP_GLOBAL);
            let pp = opts.pp.unwrap_or(cat_pp);
            let req = crate::engine::AddRequest {
                category: Some(opts.category.clone().unwrap_or_else(|| DEFAULT.to_string())),
                priority: opts.priority,
                options: Some(categories::options_for_pp(&settings, pp)),
                ..crate::engine::AddRequest::with_defaults(&preview, opts.paused || settings.add_paused)
            };
            let name = preview.name.clone();
            let id = e.add_job(req)?;
            crate::linfo!("{}", t!("log.sab.added", name = name));
            Ok(id)
        }
    }
    fn fetch_nzb(&self, url: String) -> impl Future<Output = Result<(Vec<u8>, String), String>> + Send {
        let e = self.clone();
        async move {
            let (data, f) = e.grab_nzb( &url, None).await?;
            Ok((data, f.filename.unwrap_or_default()))
        }
    }
    fn pause_job(&self, id: &str) {
        crate::engine::Engine::pause_job(self, id)
    }
    fn resume_job(&self, id: &str) {
        crate::engine::Engine::resume_job(self, id)
    }
    fn remove_job(&self, id: &str, delete_files: bool) {
        crate::engine::Engine::remove_job(self, id, delete_files)
    }
    fn set_priority(&self, id: &str, priority: i32) -> bool {
        self.set_job_priority(id, priority)
    }
    fn set_category(&self, id: &str, category: &str) -> bool {
        self.set_job_category(id, category)
    }
    fn set_paused(&self, paused: bool) {
        crate::engine::Engine::set_paused(self, paused)
    }
    fn set_speed_limit(&self, kbps: u64) -> Result<(), String> {
        crate::engine::Engine::set_speed_limit(self, kbps)
    }
    fn remove_history(&self, id: &str, delete_files: bool) {
        crate::engine::Engine::remove_history(self, id, delete_files);
        crate::bus::emit(crate::bus::Event::History);
        crate::remote::publish_history();
    }
    fn retry(&self, id: &str) -> Result<String, String> {
        let r = self.retry_history(id);
        crate::bus::emit(crate::bus::Event::History);
        crate::remote::publish_history();
        r
    }
    fn disk_free(&self, path: &str) -> Option<u64> {
        crate::sys::disk_free(std::path::Path::new(path))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::engine::{JobOptions, PostInfo};
    use parking_lot::Mutex;

    /// Motor simulado: una cola y un historial en memoria.
    pub struct Fake {
        pub settings: Settings,
        pub jobs: Mutex<Vec<JobView>>,
        pub hist: Mutex<Vec<HistoryEntry>>,
        pub paused: Mutex<bool>,
        pub limit: Mutex<u64>,
        pub added: Mutex<Vec<(String, AddOptions)>>,
        pub removed: Mutex<Vec<(String, bool)>>,
    }

    pub fn job(id: &str, name: &str, status: JobStatus, cat: &str, prio: i32) -> JobView {
        JobView {
            id: id.into(),
            name: name.into(),
            status,
            total: 100 * 1024 * 1024,
            done: 25 * 1024 * 1024,
            failed: 0,
            failed_articles: 0,
            percent: 25.0,
            speed: 2 * 1024 * 1024,
            eta: 3725,
            files: 10,
            files_done: 2,
            held_par2: 0,
            dest: "/dl/tv".into(),
            error: None,
            warning: None,
            post: None,
            added_at: 1,
            debrid: None,
            server: None,
            category: cat.into(),
            priority: prio,
            checking: None,
        }
    }

    impl Fake {
        pub fn new() -> Self {
            let mut s = Settings::default();
            s.api_key = "secretkey".into();
            s.download_dir = "/dl".into();
            s.categories = vec![
                categories::Category { name: "tv".into(), dir: "tv".into(), newznab: "5000-5999".into(), ..Default::default() },
                categories::Category { name: "movies".into(), dir: "/media/movies".into(), pp: 3, priority: 1, ..Default::default() },
            ];
            Fake {
                settings: s,
                jobs: Mutex::new(Vec::new()),
                hist: Mutex::new(Vec::new()),
                paused: Mutex::new(false),
                limit: Mutex::new(0),
                added: Mutex::new(Vec::new()),
                removed: Mutex::new(Vec::new()),
            }
        }
    }

    impl SabBackend for Fake {
        fn settings(&self) -> Settings {
            self.settings.clone()
        }
        fn queue(&self) -> Snapshot {
            Snapshot {
                paused: *self.paused.lock(),
                speed: 2 * 1024 * 1024,
                remaining: 0,
                eta: 61,
                speed_limit: *self.limit.lock(),
                active_conns: 0,
                jobs: self.jobs.lock().clone(),
                servers: Vec::new(),
                no_servers: false,
            }
        }
        fn history(&self) -> Vec<HistoryEntry> {
            self.hist.lock().clone()
        }
        fn add_nzb(&self, data: Vec<u8>, filename: String, opts: AddOptions) -> impl Future<Output = Result<String, String>> + Send {
            let r = if crate::nzb::looks_like_nzb(&data) {
                let id = format!("job{}", self.added.lock().len() + 1);
                let name = opts.name.clone().unwrap_or_else(|| filename.trim_end_matches(".nzb").to_string());
                let cat = categories::canonical(&self.settings, opts.category.as_deref());
                let prio = opts.priority.unwrap_or(0);
                let mut j = job(&id, &name, if opts.paused { JobStatus::Paused } else { JobStatus::Queued }, &cat, prio);
                j.done = 0;
                j.percent = 0.0;
                self.jobs.lock().push(j);
                self.added.lock().push((id.clone(), opts));
                Ok(id)
            } else {
                Err("Not an NZB".to_string())
            };
            async move { r }
        }
        fn fetch_nzb(&self, url: String) -> impl Future<Output = Result<(Vec<u8>, String), String>> + Send {
            async move {
                if url.contains("bad") {
                    Err("HTTP 404".into())
                } else {
                    Ok((NZB.as_bytes().to_vec(), "From.Url.nzb".to_string()))
                }
            }
        }
        fn pause_job(&self, id: &str) {
            if let Some(j) = self.jobs.lock().iter_mut().find(|j| j.id == id) {
                j.status = JobStatus::Paused;
            }
        }
        fn resume_job(&self, id: &str) {
            if let Some(j) = self.jobs.lock().iter_mut().find(|j| j.id == id) {
                j.status = JobStatus::Queued;
            }
        }
        fn remove_job(&self, id: &str, delete_files: bool) {
            self.jobs.lock().retain(|j| j.id != id);
            self.removed.lock().push((id.to_string(), delete_files));
        }
        fn set_priority(&self, id: &str, priority: i32) -> bool {
            let mut jobs = self.jobs.lock();
            let Some(pos) = jobs.iter().position(|j| j.id == id) else { return false };
            let mut j = jobs.remove(pos);
            j.priority = priority;
            let to = jobs.iter().position(|x| x.priority < priority).unwrap_or(jobs.len());
            jobs.insert(to, j);
            true
        }
        fn set_category(&self, id: &str, category: &str) -> bool {
            let cat = categories::canonical(&self.settings, Some(category));
            self.jobs.lock().iter_mut().find(|j| j.id == id).map(|j| j.category = cat).is_some()
        }
        fn set_paused(&self, paused: bool) {
            *self.paused.lock() = paused;
        }
        fn set_speed_limit(&self, kbps: u64) -> Result<(), String> {
            *self.limit.lock() = kbps;
            Ok(())
        }
        fn remove_history(&self, id: &str, _delete_files: bool) {
            self.hist.lock().retain(|h| h.id != id);
        }
        fn retry(&self, id: &str) -> Result<String, String> {
            let h = self.hist.lock().iter().find(|h| h.id == id).cloned().ok_or("not found")?;
            self.hist.lock().retain(|x| x.id != id);
            let new = format!("{}-r", id);
            self.jobs.lock().push(job(&new, &h.name, JobStatus::Queued, &h.category, 0));
            Ok(new)
        }
        fn disk_free(&self, _path: &str) -> Option<u64> {
            Some(500 * 1024 * 1024 * 1024)
        }
    }

    pub const NZB: &str = r#"<?xml version="1.0"?><nzb xmlns="http://www.newzbin.com/DTD/2003/nzb"><file subject="a"><groups><group>a.b</group></groups><segments><segment bytes="1" number="1">a@b</segment></segments></file></nzb>"#;

    fn params(q: &str) -> Vec<(String, String)> {
        q.split('&').filter_map(|p| p.split_once('=')).map(|(k, v)| (k.to_string(), crate::util::percent_decode(v))).collect()
    }

    fn run(b: &Fake, q: &str) -> Value {
        let p = params(q);
        let call = Call { params: &p, files: &[] };
        crate::rt::block_on(handle(b, &call)).1
    }

    #[test]
    fn auth_like_sabnzbd() {
        let b = Fake::new();
        assert_eq!(run(&b, "mode=version")["version"], SAB_VERSION);
        assert_eq!(run(&b, "mode=queue")["error"], "API Key Required");
        assert_eq!(run(&b, "mode=queue&apikey=nope")["error"], "API Key Incorrect");
        assert_eq!(run(&b, "mode=queue&apikey=secretkey")["queue"]["noofslots"], 0);
        assert_eq!(run(&b, "mode=queue&nzbkey=secretkey")["queue"]["noofslots"], 0);
        assert_eq!(run(&b, "mode=nope&apikey=secretkey")["status"], false);
    }

    #[test]
    fn config_and_categories() {
        let b = Fake::new();
        let c = run(&b, "mode=get_config&apikey=secretkey");
        assert_eq!(c["config"]["misc"]["complete_dir"], "/dl");
        assert_eq!(c["config"]["misc"]["enable_tv_sorting"], 0);
        let cats = c["config"]["categories"].as_array().unwrap();
        assert_eq!(cats.iter().map(|c| c["name"].as_str().unwrap()).collect::<Vec<_>>(), ["*", "tv", "movies"]);
        assert_eq!(cats[1]["dir"], "tv");
        assert_eq!(cats[1]["pp"], "");
        assert_eq!(cats[2]["pp"], "3");
        assert_eq!(cats[2]["priority"], 1);
        assert_eq!(run(&b, "mode=get_cats&apikey=secretkey")["categories"], json!(["*", "tv", "movies"]));
        let misc = run(&b, "mode=get_config&section=misc&apikey=secretkey");
        assert_eq!(misc["config"]["misc"]["complete_dir"], "/dl");
    }

    #[test]
    fn queue_shape_and_states() {
        let b = Fake::new();
        b.jobs.lock().push(job("a", "Show.S01E01", JobStatus::Downloading, "tv", 0));
        b.jobs.lock().push(job("b", "Film", JobStatus::Paused, "*", 1));
        b.jobs.lock().push(job("c", "Extracting.One", JobStatus::Extracting, "tv", 0));
        let q = run(&b, "mode=queue&start=0&limit=0&output=json&apikey=secretkey");
        let slots = q["queue"]["slots"].as_array().unwrap();
        assert_eq!(slots.len(), 2, "lo que está en post-procesado va al historial");
        assert_eq!(slots[0]["nzo_id"], "SABnzbd_nzo_a");
        assert_eq!(slots[0]["status"], "Downloading");
        assert_eq!(slots[0]["cat"], "tv");
        assert_eq!(slots[0]["mb"], "100.00");
        assert_eq!(slots[0]["mbleft"], "75.00");
        assert_eq!(slots[0]["percentage"], "25");
        assert_eq!(slots[0]["timeleft"], "1:02:05");
        assert_eq!(slots[0]["priority"], "Normal");
        assert_eq!(slots[1]["status"], "Paused");
        assert_eq!(slots[1]["cat"], "*");
        assert_eq!(slots[1]["priority"], "High");
        assert_eq!(q["queue"]["paused"], false);
        assert_eq!(q["queue"]["kbpersec"], "2048.00");
        // Filtro por categoría.
        let tv = run(&b, "mode=queue&category=tv&apikey=secretkey");
        assert_eq!(tv["queue"]["slots"].as_array().unwrap().len(), 1);
        // En post-procesado, en el historial con su estado.
        let h = run(&b, "mode=history&apikey=secretkey");
        assert_eq!(h["history"]["slots"][0]["status"], "Extracting");
        assert_eq!(h["history"]["slots"][0]["nzo_id"], "SABnzbd_nzo_c");
    }

    #[test]
    fn queue_actions() {
        let b = Fake::new();
        b.jobs.lock().push(job("a", "A", JobStatus::Queued, "tv", 0));
        b.jobs.lock().push(job("b", "B", JobStatus::Queued, "tv", 0));
        run(&b, "mode=queue&name=pause&value=SABnzbd_nzo_a&apikey=secretkey");
        assert_eq!(b.jobs.lock()[0].status, JobStatus::Paused);
        run(&b, "mode=queue&name=resume&value=a&apikey=secretkey");
        assert_eq!(b.jobs.lock()[0].status, JobStatus::Queued);
        let p = run(&b, "mode=queue&name=priority&value=SABnzbd_nzo_b&value2=2&apikey=secretkey");
        assert_eq!(p["position"], 0);
        assert_eq!(b.jobs.lock()[0].id, "b");
        run(&b, "mode=queue&name=priority&value=a&value2=-2&apikey=secretkey");
        assert_eq!(b.jobs.lock()[1].status, JobStatus::Paused);
        run(&b, "mode=pause&apikey=secretkey");
        assert!(*b.paused.lock());
        assert_eq!(run(&b, "mode=queue&apikey=secretkey")["queue"]["status"], "Paused");
        run(&b, "mode=resume&apikey=secretkey");
        assert!(!*b.paused.lock());
        run(&b, "mode=change_cat&value=a&value2=movies&apikey=secretkey");
        assert_eq!(b.jobs.lock()[1].category, "movies");
        let d = run(&b, "mode=queue&name=delete&value=SABnzbd_nzo_a,nope&del_files=1&apikey=secretkey");
        assert_eq!(d["nzo_ids"], json!(["SABnzbd_nzo_a"]));
        assert_eq!(b.removed.lock()[0], ("a".to_string(), true));
        run(&b, "mode=config&name=speedlimit&value=5M&apikey=secretkey");
        assert_eq!(*b.limit.lock(), 5120);
        assert_eq!(run(&b, "mode=queue&apikey=secretkey")["queue"]["speedlimit_abs"], (5120u64 * 1024).to_string());
    }

    #[test]
    fn speeds() {
        assert_eq!(parse_speed("500K"), Some(500));
        assert_eq!(parse_speed("2.5M"), Some(2560));
        assert_eq!(parse_speed("1G"), Some(1024 * 1024));
        assert_eq!(parse_speed("800"), Some(800));
        assert_eq!(parse_speed("100"), Some(0));
        assert_eq!(parse_speed("0"), Some(0));
        assert_eq!(parse_speed(""), Some(0));
        assert_eq!(parse_speed("50%"), None);
        assert_eq!(parse_speed("100%"), Some(0));
        assert_eq!(parse_speed("x"), None);
    }

    #[test]
    fn add_options_from_params() {
        let p = params("cat=tv&priority=-100&pp=2&nzbname=My.Show");
        let o = add_options(&Call { params: &p, files: &[] });
        assert_eq!(o, AddOptions { name: Some("My.Show".into()), category: Some("tv".into()), priority: None, pp: Some(2), paused: false });
        let p = params("priority=-2&pp=9");
        let o = add_options(&Call { params: &p, files: &[] });
        assert!(o.paused && o.priority.is_none() && o.pp.is_none());
    }

    #[test]
    fn history_and_retry() {
        let b = Fake::new();
        b.hist.lock().push(HistoryEntry {
            id: "h1".into(),
            name: "Done".into(),
            status: "completed".into(),
            size: 1234,
            dest: "/dl/tv/Done".into(),
            category: "tv".into(),
            completed_at: 99,
            download_secs: 7,
            options: JobOptions::default(),
            ..Default::default()
        });
        b.hist.lock().push(HistoryEntry {
            id: "h2".into(),
            name: "Broken".into(),
            status: "failed".into(),
            error: Some("Too many missing articles".into()),
            work_dir: "/dl/_incompleto/Broken".into(),
            ..Default::default()
        });
        let h = run(&b, "mode=history&start=0&limit=60&category=tv&apikey=secretkey");
        let s = &h["history"]["slots"];
        assert_eq!(h["history"]["noofslots"], 1);
        assert_eq!(s[0]["status"], "Completed");
        assert_eq!(s[0]["storage"], "/dl/tv/Done");
        assert_eq!(s[0]["bytes"], 1234);
        assert_eq!(s[0]["completed"], 99);
        assert_eq!(s[0]["download_time"], 7);
        assert_eq!(s[0]["category"], "tv");
        let f = run(&b, "mode=history&failed_only=1&apikey=secretkey");
        assert_eq!(f["history"]["slots"][0]["status"], "Failed");
        assert_eq!(f["history"]["slots"][0]["fail_message"], "Too many missing articles");
        assert_eq!(f["history"]["slots"][0]["category"], "*");
        let r = run(&b, "mode=retry&value=SABnzbd_nzo_h2&apikey=secretkey");
        assert_eq!(r["nzo_id"], "SABnzbd_nzo_h2-r");
        run(&b, "mode=history&name=delete&value=SABnzbd_nzo_h1&del_files=1&apikey=secretkey");
        assert!(b.hist.lock().is_empty());
        let _ = PostInfo::default();
    }
}
