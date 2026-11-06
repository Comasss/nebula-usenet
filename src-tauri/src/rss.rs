//! Descarga automática por RSS.
//!
//! - Feeds: cada indexador tiene el suyo (sus novedades, `t=search` sin texto) y se pueden añadir
//!   URLs RSS sueltas. Cada feed se consulta cada `interval_min` minutos (5 como mínimo), y solo si
//!   alguna regla activa lo usa. Las consultas a indexadores cuentan en sus estadísticas.
//! - Reglas: palabras o regex obligatorias, palabras excluidas, tamaño, categorías, método de
//!   descarga y carpeta. Por defecto solo cogen lo publicado después de crear la regla.
//! - Duplicados: un elemento ya cogido no se vuelve a coger, y una release cuyo título ya está en
//!   la cola o en el historial se salta.
//! - Las capturas siguen el flujo normal (NZB → vista previa → cola). Si un indexador no da el NZB,
//!   se prueba la misma release en los demás.
//! - Con la cola en pausa no se consulta nada.
use crate::config::{IndexerConfig, RssFeed, RssRule, Settings};
use crate::engine::{AddRequest, Engine};
use crate::indexer::{self, SearchItem};
use crate::stats::IndexerEvent;
use crate::{lerror, linfo, lwarn, util};
use parking_lot::Mutex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tauri::Emitter;

pub const DEFAULT_INTERVAL: u32 = 15;
pub const MIN_INTERVAL: u32 = 5;
/// Elementos cogidos que se recuerdan (los más antiguos se olvidan).
const MAX_SEEN: usize = 5000;
const MAX_LOG: usize = 300;
/// Cada cuánto se mira si toca consultar algún feed.
const TICK: Duration = Duration::from_secs(20);

/// Captura automática (o intento fallido) para el registro.
#[derive(Clone, Serialize, Deserialize, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Capture {
    pub at: u64,
    pub rule_id: String,
    pub rule: String,
    pub title: String,
    /// Indexador (o feed suelto) del que salió el NZB.
    pub indexer: String,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
struct FeedMark {
    last_check: u64,
    last_error: Option<String>,
    items: usize,
}

/// Lo que se guarda en `rss.json`.
#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Saved {
    /// Elementos ya cogidos (guid o enlace).
    seen: VecDeque<String>,
    /// Primera vez que se vio un elemento sin fecha (para "solo lo nuevo").
    first_seen: HashMap<String, u64>,
    log: Vec<Capture>,
    feeds: HashMap<String, FeedMark>,
}

struct Inner {
    saved: Saved,
    seen: HashSet<String>,
    /// Últimos elementos leídos de cada feed.
    items: HashMap<String, Vec<SearchItem>>,
    path: Option<std::path::PathBuf>,
}

static STATE: LazyLock<Mutex<Inner>> = LazyLock::new(|| {
    Mutex::new(Inner { saved: Saved::default(), seen: HashSet::new(), items: HashMap::new(), path: None })
});
static BUSY: AtomicBool = AtomicBool::new(false);

impl Inner {
    fn save(&self) {
        let Some(path) = &self.path else { return };
        match serde_json::to_vec(&self.saved) {
            Ok(d) => {
                if let Err(e) = crate::config::write_atomic(path, &d) {
                    lerror!("{}", e);
                }
            }
            Err(e) => lerror!("{}", e),
        }
    }
    fn mark_seen(&mut self, key: &str) {
        if self.seen.insert(key.to_string()) {
            self.saved.seen.push_back(key.to_string());
            while self.saved.seen.len() > MAX_SEEN {
                if let Some(old) = self.saved.seen.pop_front() {
                    self.seen.remove(&old);
                }
            }
        }
    }
    fn push_log(&mut self, c: Capture) {
        self.saved.log.push(c);
        if self.saved.log.len() > MAX_LOG {
            let extra = self.saved.log.len() - MAX_LOG;
            self.saved.log.drain(..extra);
        }
    }
}

// ---- Feeds ----

/// Id del feed de novedades de un indexador.
pub fn indexer_feed_id(indexer: &str) -> String {
    format!("ix-{}", indexer)
}

/// Feeds disponibles: el de cada indexador activo (con los cambios guardados, si los hay) y los
/// sueltos.
pub fn feeds(s: &Settings) -> Vec<RssFeed> {
    let mut out = Vec::new();
    if s.feature_indexers {
        for ix in s.indexers.iter().filter(|i| i.enabled && !i.url.trim().is_empty()) {
            let id = indexer_feed_id(&ix.id);
            let saved = s.rss_feeds.iter().find(|f| f.id == id);
            out.push(RssFeed {
                id,
                name: ix.label(),
                url: String::new(),
                indexer: ix.id.clone(),
                interval_min: saved.map_or(DEFAULT_INTERVAL, |f| f.interval_min),
                enabled: saved.map_or(true, |f| f.enabled),
            });
        }
    }
    for f in s.rss_feeds.iter().filter(|f| f.indexer.is_empty() && !f.url.trim().is_empty()) {
        out.push(f.clone());
    }
    out
}

fn interval_secs(f: &RssFeed) -> u64 {
    f.interval_min.max(MIN_INTERVAL) as u64 * 60
}

fn applies(rule: &RssRule, feed: &str) -> bool {
    rule.feeds.is_empty() || rule.feeds.iter().any(|f| f == feed)
}

/// Feeds activos que usa alguna regla activa.
fn active_feeds(s: &Settings) -> Vec<RssFeed> {
    let rules: Vec<&RssRule> = s.rss_rules.iter().filter(|r| r.enabled).collect();
    feeds(s).into_iter().filter(|f| f.enabled && rules.iter().any(|r| applies(r, &f.id))).collect()
}

fn item_key(it: &SearchItem) -> String {
    if it.guid.is_empty() {
        it.link.clone()
    } else {
        format!("{}|{}", it.indexer_id, it.guid)
    }
}

/// Lee un feed. Las consultas a indexadores se anotan en sus estadísticas como una búsqueda.
async fn fetch(e: &Engine, feed: &RssFeed) -> Result<Vec<SearchItem>, String> {
    if feed.indexer.is_empty() {
        let name = if feed.name.trim().is_empty() { host(&feed.url) } else { feed.name.clone() };
        return indexer::fetch_feed(&feed.url, &name).await;
    }
    let (ix, limit) = {
        let s = e.settings.read();
        let ix = s.indexers.iter().find(|i| i.id == feed.indexer).cloned().ok_or_else(|| t!("err.indexerNotFound"))?;
        (ix, s.search_limit)
    };
    let start = std::time::Instant::now();
    let res = indexer::search(&ix, "", None, 0, limit).await;
    e.record_indexer(IndexerEvent {
        at: util::now_secs(),
        id: ix.id.clone(),
        name: ix.label(),
        kind: "search".into(),
        ms: res.as_ref().map(|r| r.ms).unwrap_or_else(|_| start.elapsed().as_millis() as u64),
        ok: res.is_ok(),
        results: res.as_ref().map(|r| r.items.len() as u64).unwrap_or(0),
        offset: 0,
        search: Some(format!("rss-{}", util::new_id())),
    });
    res.map(|r| r.items)
}

fn host(url: &str) -> String {
    url.trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

// ---- Reglas ----

/// Una comprobación de una regla sobre un elemento: `key` la identifica en la interfaz.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub key: &'static str,
    pub ok: bool,
    pub detail: String,
}

fn words(text: &str) -> String {
    let w: Vec<String> = text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect();
    format!(" {} ", w.join(" "))
}

/// Términos de un campo: separados por comas o espacios, y "entre comillas" para frases.
fn terms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find('"') {
        out.extend(rest[..i].split([',', ' ', ';']).filter(|t| !t.trim().is_empty()).map(|t| t.trim().to_string()));
        let after = &rest[i + 1..];
        match after.find('"') {
            Some(j) => {
                if !after[..j].trim().is_empty() {
                    out.push(after[..j].trim().to_string());
                }
                rest = &after[j + 1..];
            }
            None => {
                rest = after;
            }
        }
    }
    out.extend(rest.split([',', ' ', ';']).filter(|t| !t.trim().is_empty()).map(|t| t.trim().to_string()));
    out
}

fn contains_term(title_words: &str, term: &str) -> bool {
    let w = words(term);
    w.trim().is_empty() || title_words.contains(&w)
}

pub fn compile(rule: &RssRule) -> Result<Option<Regex>, String> {
    if !rule.regex || rule.must.trim().is_empty() {
        return Ok(None);
    }
    regex::RegexBuilder::new(rule.must.trim())
        .case_insensitive(true)
        .size_limit(1 << 20)
        .build()
        .map(Some)
        .map_err(|e| t!("rss.badRegex", error = e.to_string().lines().last().unwrap_or("").trim()))
}

fn category_ok(item: &SearchItem, spec: &str) -> Option<String> {
    for c in spec.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        if let Ok(n) = c.parse::<u32>() {
            if item.category_ids.iter().any(|id| *id == n || (n % 1000 == 0 && id / 1000 == n / 1000)) {
                return Some(c.to_string());
            }
        } else if item.category.to_lowercase().contains(&c.to_lowercase()) {
            return Some(c.to_string());
        }
    }
    None
}

/// Fecha que cuenta para "solo lo nuevo": la de publicación o, sin ella, cuando se vio por primera vez.
fn item_date(it: &SearchItem, first_seen: &HashMap<String, u64>) -> u64 {
    if it.published > 0 {
        it.published
    } else {
        first_seen.get(&item_key(it)).copied().unwrap_or_else(util::now_secs)
    }
}

/// Comprueba una regla sobre un elemento. Devuelve si coincide y el detalle de cada comprobación.
pub fn evaluate(rule: &RssRule, re: Option<&Regex>, it: &SearchItem, date: u64) -> (bool, Vec<Check>) {
    let mut checks = Vec::new();
    let tw = words(&it.title);
    if let Some(re) = re {
        checks.push(Check { key: "regex", ok: re.is_match(&it.title), detail: rule.must.trim().to_string() });
    } else {
        let must = terms(&rule.must);
        if !must.is_empty() {
            let missing: Vec<String> = must.iter().filter(|t| !contains_term(&tw, t)).cloned().collect();
            checks.push(Check {
                key: "must",
                ok: missing.is_empty(),
                detail: if missing.is_empty() { must.join(", ") } else { missing.join(", ") },
            });
        }
    }
    let excluded: Vec<String> = terms(&rule.exclude).into_iter().filter(|t| contains_term(&tw, t)).collect();
    if !rule.exclude.trim().is_empty() {
        checks.push(Check { key: "exclude", ok: excluded.is_empty(), detail: excluded.join(", ") });
    }
    let mb = it.size / (1024 * 1024);
    if rule.min_size_mb > 0 {
        checks.push(Check { key: "minSize", ok: it.size > 0 && mb >= rule.min_size_mb, detail: rule.min_size_mb.to_string() });
    }
    if rule.max_size_mb > 0 {
        checks.push(Check { key: "maxSize", ok: it.size > 0 && mb <= rule.max_size_mb, detail: rule.max_size_mb.to_string() });
    }
    if !rule.categories.trim().is_empty() {
        let hit = category_ok(it, &rule.categories);
        checks.push(Check { key: "category", ok: hit.is_some(), detail: hit.unwrap_or_else(|| rule.categories.trim().to_string()) });
    }
    if rule.only_new {
        checks.push(Check { key: "date", ok: date >= rule.created_at, detail: String::new() });
    }
    let ok = checks.iter().all(|c| c.ok);
    (ok, checks)
}

/// Títulos (normalizados) que ya están en la cola o en el historial.
fn known_titles(e: &Engine) -> HashSet<String> {
    let st = e.st.lock();
    st.jobs
        .iter()
        .map(|j| j.name.as_str())
        .chain(st.history.iter().map(|h| h.name.as_str()))
        .map(crate::api::normalize_title)
        .filter(|t| !t.is_empty())
        .collect()
}

// ---- Bucle ----

pub fn start(e: Arc<Engine>) {
    {
        let mut st = STATE.lock();
        let path = e.paths.data.join("rss.json");
        st.saved = std::fs::read(&path).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default();
        st.seen = st.saved.seen.iter().cloned().collect();
        st.path = Some(path);
    }
    tauri::async_runtime::spawn(async move {
        // Se deja arrancar la app antes de la primera consulta.
        tokio::time::sleep(Duration::from_secs(8)).await;
        loop {
            if !e.st.lock().paused {
                let _ = run(&e, false).await;
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CheckSummary {
    pub feeds: usize,
    pub grabbed: usize,
    pub errors: usize,
}

/// Consulta los feeds a los que les toca (o todos con `force`) y coge lo que coincida.
pub async fn run(e: &Arc<Engine>, force: bool) -> Result<CheckSummary, String> {
    if e.st.lock().paused {
        return Err(t!("rss.paused"));
    }
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err(t!("rss.busy"));
    }
    let res = run_inner(e, force).await;
    BUSY.store(false, Ordering::SeqCst);
    let _ = e.app.emit("rss", ());
    Ok(res)
}

async fn run_inner(e: &Arc<Engine>, force: bool) -> CheckSummary {
    let mut sum = CheckSummary::default();
    let settings = e.settings.read().clone();
    let now = util::now_secs();
    let due: Vec<RssFeed> = {
        let st = STATE.lock();
        active_feeds(&settings)
            .into_iter()
            .filter(|f| force || st.saved.feeds.get(&f.id).map_or(true, |m| now >= m.last_check + interval_secs(f)))
            .collect()
    };
    if due.is_empty() {
        return sum;
    }
    let rules: Vec<(RssRule, Option<Regex>)> = settings
        .rss_rules
        .iter()
        .filter(|r| r.enabled)
        .filter_map(|r| match compile(r) {
            Ok(re) => Some((r.clone(), re)),
            Err(err) => {
                lwarn!("{}", t!("rss.ruleSkipped", rule = r.name, error = err));
                None
            }
        })
        .collect();
    for feed in due {
        sum.feeds += 1;
        let res = fetch(e, &feed).await;
        let items = {
            let mut st = STATE.lock();
            let mark = st.saved.feeds.entry(feed.id.clone()).or_default();
            mark.last_check = util::now_secs();
            match res {
                Ok(items) => {
                    mark.last_error = None;
                    mark.items = items.len();
                    for it in &items {
                        if it.published == 0 {
                            st.saved.first_seen.entry(item_key(it)).or_insert(now);
                        }
                    }
                    st.items.insert(feed.id.clone(), items.clone());
                    items
                }
                Err(err) => {
                    sum.errors += 1;
                    lwarn!("{}", t!("rss.feedFailed", feed = feed.name, error = err));
                    mark.last_error = Some(err);
                    Vec::new()
                }
            }
        };
        // Lo más antiguo primero, para respetar el orden de publicación.
        let mut items = items;
        items.sort_by_key(|i| i.published);
        for it in items {
            if e.st.lock().paused {
                break;
            }
            let key = item_key(&it);
            let date = {
                let st = STATE.lock();
                if st.seen.contains(&key) {
                    continue;
                }
                item_date(&it, &st.saved.first_seen)
            };
            let Some((rule, _)) = rules.iter().find(|(r, re)| applies(r, &feed.id) && evaluate(r, re.as_ref(), &it, date).0) else {
                continue;
            };
            STATE.lock().mark_seen(&key);
            let norm = crate::api::normalize_title(&it.title);
            if known_titles(e).contains(&norm) {
                linfo!("{}", t!("rss.skippedDupe", name = it.title));
                continue;
            }
            let capture = match grab(e, &settings, rule, &it, &feed.id).await {
                Ok(source) => {
                    sum.grabbed += 1;
                    linfo!("{}", t!("rss.grabbed", rule = rule.name, name = it.title, source = source));
                    Capture { at: util::now_secs(), rule_id: rule.id.clone(), rule: rule.name.clone(), title: it.title.clone(), indexer: source, ok: true, error: None }
                }
                Err(err) => {
                    sum.errors += 1;
                    lwarn!("{}", t!("rss.grabFailed", name = it.title, error = err));
                    Capture {
                        at: util::now_secs(),
                        rule_id: rule.id.clone(),
                        rule: rule.name.clone(),
                        title: it.title.clone(),
                        indexer: it.indexer_name.clone(),
                        ok: false,
                        error: Some(err),
                    }
                }
            };
            STATE.lock().push_log(capture);
        }
    }
    {
        let mut st = STATE.lock();
        // Las fechas de "primera vez visto" de hace más de un mes ya no sirven.
        let old = now.saturating_sub(30 * 86400);
        st.saved.first_seen.retain(|_, t| *t >= old);
        st.save();
    }
    sum
}

/// Coge una release: pide el NZB al indexador del feed y, si falla, prueba la misma release en
/// los demás (primero en lo que ya se leyó de sus feeds y luego buscándola).
async fn grab(e: &Arc<Engine>, s: &Settings, rule: &RssRule, it: &SearchItem, feed_id: &str) -> Result<String, String> {
    let norm = crate::api::normalize_title(&it.title);
    let mut tried: HashSet<String> = HashSet::new();
    let mut last_err = String::new();
    let mut candidates = vec![it.clone()];
    {
        let st = STATE.lock();
        for (fid, items) in &st.items {
            if fid == feed_id {
                continue;
            }
            candidates.extend(items.iter().filter(|x| crate::api::normalize_title(&x.title) == norm).cloned());
        }
    }
    for c in &candidates {
        if !tried.insert(c.link.clone()) {
            continue;
        }
        match add(e, s, rule, c).await {
            Ok(()) => return Ok(c.indexer_name.clone()),
            Err(err) => {
                lwarn!("{}", t!("rss.sourceFailed", source = c.indexer_name, error = err));
                last_err = err;
            }
        }
    }
    // Buscar la release en los indexadores que aún no se han probado.
    let tried_ix: HashSet<String> = candidates.iter().map(|c| c.indexer_id.clone()).collect();
    let others: Vec<IndexerConfig> = if s.feature_indexers {
        s.indexers.iter().filter(|i| i.enabled && !i.url.trim().is_empty() && !tried_ix.contains(&i.id)).cloned().collect()
    } else {
        Vec::new()
    };
    for ix in others {
        let start = std::time::Instant::now();
        let res = indexer::search(&ix, &it.title, None, 0, 25).await;
        e.record_indexer(IndexerEvent {
            at: util::now_secs(),
            id: ix.id.clone(),
            name: ix.label(),
            kind: "search".into(),
            ms: start.elapsed().as_millis() as u64,
            ok: res.is_ok(),
            results: res.as_ref().map(|r| r.items.len() as u64).unwrap_or(0),
            offset: 0,
            search: Some(format!("rss-{}", util::new_id())),
        });
        let Ok(r) = res else { continue };
        let Some(c) = r.items.into_iter().find(|x| crate::api::normalize_title(&x.title) == norm) else { continue };
        match add(e, s, rule, &c).await {
            Ok(()) => return Ok(c.indexer_name.clone()),
            Err(err) => {
                lwarn!("{}", t!("rss.sourceFailed", source = c.indexer_name, error = err));
                last_err = err;
            }
        }
    }
    Err(last_err)
}

/// Flujo normal de descarga: NZB → vista previa → cola, con las opciones de la regla.
async fn add(e: &Arc<Engine>, s: &Settings, rule: &RssRule, it: &SearchItem) -> Result<(), String> {
    let ix = (!it.indexer_id.is_empty()).then(|| it.indexer_id.clone());
    let (data, f) = crate::commands::grab_nzb(e, &it.link, ix.as_deref()).await?;
    let engine = e.clone();
    let name = f.filename.unwrap_or_default();
    let link = it.link.clone();
    let preview = tokio::task::spawn_blocking(move || engine.register_pending(data, &name, f.strong, link))
        .await
        .map_err(|_| t!("err.internal"))??;
    if let Some(id) = ix {
        e.set_pending_indexer(&preview.token, id);
    }
    let req = AddRequest {
        name: it.title.clone(),
        dest_dir: rule.dest_dir.trim().to_string(),
        via: (!rule.via.trim().is_empty()).then(|| rule.via.trim().to_string()),
        category: (!rule.category.trim().is_empty()).then(|| rule.category.trim().to_string()),
        newznab: it.category_ids.clone(),
        ..AddRequest::with_defaults(&preview, s.add_paused)
    };
    e.add_job(req).map(|_| ())
}

// ---- Interfaz ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedInfo {
    pub id: String,
    pub name: String,
    pub url: String,
    pub indexer: String,
    pub interval_min: u32,
    pub enabled: bool,
    /// Alguna regla activa lo usa (si no, no se consulta).
    pub used: bool,
    pub last_check: u64,
    pub next_check: u64,
    pub last_error: Option<String>,
    pub items: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub feeds: Vec<FeedInfo>,
    pub log: Vec<Capture>,
    pub busy: bool,
    pub paused: bool,
}

pub fn status(e: &Engine) -> Status {
    let s = e.settings.read().clone();
    let active: HashSet<String> = active_feeds(&s).into_iter().map(|f| f.id).collect();
    let st = STATE.lock();
    let feeds = feeds(&s)
        .into_iter()
        .map(|f| {
            let m = st.saved.feeds.get(&f.id).cloned().unwrap_or_default();
            FeedInfo {
                used: active.contains(&f.id),
                next_check: if m.last_check == 0 { 0 } else { m.last_check + interval_secs(&f) },
                last_check: m.last_check,
                last_error: m.last_error,
                items: m.items,
                id: f.id,
                name: f.name,
                url: f.url,
                indexer: f.indexer,
                interval_min: f.interval_min.max(MIN_INTERVAL),
                enabled: f.enabled,
            }
        })
        .collect();
    let mut log = st.saved.log.clone();
    log.reverse();
    Status { feeds, log, busy: BUSY.load(Ordering::SeqCst), paused: e.st.lock().paused }
}

pub fn clear_log(e: &Engine) {
    let mut st = STATE.lock();
    st.saved.log.clear();
    st.save();
    drop(st);
    let _ = e.app.emit("rss", ());
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewItem {
    pub title: String,
    pub feed: String,
    pub indexer: String,
    pub size: u64,
    pub date: u64,
    pub category: String,
    pub matched: bool,
    pub checks: Vec<Check>,
    /// Ya se cogió antes.
    pub seen: bool,
    /// El mismo título ya está en la cola o en el historial.
    pub duplicate: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub items: Vec<PreviewItem>,
    /// Feeds que no se pudieron leer: (nombre, error).
    pub errors: Vec<(String, String)>,
}

/// Qué elementos de los feeds actuales coincidirían con una regla (aún sin guardar) y por qué.
/// Los feeds que todavía no se han leído se leen ahora.
pub async fn preview(e: &Arc<Engine>, rule: RssRule) -> Result<Preview, String> {
    let re = compile(&rule)?;
    let s = e.settings.read().clone();
    let feeds: Vec<RssFeed> = feeds(&s).into_iter().filter(|f| applies(&rule, &f.id)).collect();
    if feeds.is_empty() {
        return Err(t!("rss.noFeeds"));
    }
    let mut errors = Vec::new();
    for f in &feeds {
        if STATE.lock().items.contains_key(&f.id) {
            continue;
        }
        match fetch(e, f).await {
            Ok(items) => {
                let now = util::now_secs();
                let mut st = STATE.lock();
                for it in &items {
                    if it.published == 0 {
                        st.saved.first_seen.entry(item_key(it)).or_insert(now);
                    }
                }
                st.items.insert(f.id.clone(), items);
            }
            Err(err) => errors.push((f.name.clone(), err)),
        }
    }
    let known = known_titles(e);
    let st = STATE.lock();
    let mut out = Vec::new();
    for f in &feeds {
        for it in st.items.get(&f.id).map(|v| v.as_slice()).unwrap_or(&[]) {
            let date = item_date(it, &st.saved.first_seen);
            let (matched, checks) = evaluate(&rule, re.as_ref(), it, date);
            out.push(PreviewItem {
                title: it.title.clone(),
                feed: f.name.clone(),
                indexer: it.indexer_name.clone(),
                size: it.size,
                date,
                category: it.category.clone(),
                matched,
                checks,
                seen: st.seen.contains(&item_key(it)),
                duplicate: known.contains(&crate::api::normalize_title(&it.title)),
            });
        }
    }
    out.sort_by(|a, b| b.matched.cmp(&a.matched).then(b.date.cmp(&a.date)));
    out.truncate(300);
    Ok(Preview { items: out, errors })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(title: &str, size_mb: u64, cats: &[u32], published: u64) -> SearchItem {
        SearchItem {
            title: title.into(),
            link: format!("https://idx.example/getnzb/{}", title),
            details: None,
            size: size_mb * 1024 * 1024,
            date: published,
            grabs: None,
            files: None,
            category: "TV > HD".into(),
            category_ids: cats.to_vec(),
            password: None,
            indexer_id: "a".into(),
            indexer_name: "A".into(),
            published,
            guid: title.into(),
        }
    }

    fn rule() -> RssRule {
        RssRule { name: "Serie".into(), created_at: 1000, ..Default::default() }
    }

    #[test]
    fn words_and_exclusions() {
        let mut r = rule();
        r.must = "some show 1080p".into();
        r.exclude = "hevc, \"x 265\"".into();
        let ok = item("Some.Show.S01E01.1080p.WEB.h264-GRP", 1500, &[5040], 2000);
        assert!(evaluate(&r, None, &ok, 2000).0);
        let wrong_res = item("Some.Show.S01E01.720p.WEB.h264-GRP", 1500, &[5040], 2000);
        let (m, checks) = evaluate(&r, None, &wrong_res, 2000);
        assert!(!m);
        assert_eq!(checks[0], Check { key: "must", ok: false, detail: "1080p".into() });
        let hevc = item("Some.Show.S01E01.1080p.WEB.HEVC-GRP", 1500, &[5040], 2000);
        assert!(!evaluate(&r, None, &hevc, 2000).0);
        let x265 = item("Some Show S01E01 1080p x 265", 1500, &[5040], 2000);
        assert!(!evaluate(&r, None, &x265, 2000).0);
        // Palabra entera: "show" no coincide con "showcase".
        let partial = item("Some.Showcase.1080p", 1500, &[5040], 2000);
        assert!(!evaluate(&r, None, &partial, 2000).0);
    }

    #[test]
    fn regex_size_category_date() {
        let mut r = rule();
        r.regex = true;
        r.must = r"^some\.show\.s\d+e\d+".into();
        r.min_size_mb = 500;
        r.max_size_mb = 4000;
        r.categories = "5000".into();
        let re = compile(&r).unwrap();
        let it = item("Some.Show.S02E03.1080p", 1500, &[5040], 2000);
        assert!(evaluate(&r, re.as_ref(), &it, 2000).0);
        assert!(!evaluate(&r, re.as_ref(), &item("Some.Show.S02E03", 100, &[5040], 2000), 2000).0);
        assert!(!evaluate(&r, re.as_ref(), &item("Some.Show.S02E03", 5000, &[5040], 2000), 2000).0);
        assert!(!evaluate(&r, re.as_ref(), &item("Some.Show.S02E03", 1500, &[2040], 2000), 2000).0);
        // Publicado antes de crear la regla.
        let (m, checks) = evaluate(&r, re.as_ref(), &it, 500);
        assert!(!m);
        assert!(checks.iter().any(|c| c.key == "date" && !c.ok));
        r.only_new = false;
        assert!(evaluate(&r, re.as_ref(), &it, 500).0);
        // Categoría por texto.
        r.categories = "movies, tv".into();
        assert!(evaluate(&r, re.as_ref(), &it, 500).0);
        r.must = "(".into();
        assert!(compile(&r).is_err());
    }

    #[test]
    fn feed_list() {
        let mut s = Settings::default();
        s.feature_indexers = true;
        s.indexers.push(IndexerConfig { id: "x".into(), name: "X".into(), url: "https://x.example".into(), ..Default::default() });
        s.indexers.push(IndexerConfig { id: "y".into(), url: "https://y.example".into(), enabled: false, ..Default::default() });
        s.rss_feeds.push(RssFeed { id: "ix-x".into(), indexer: "x".into(), interval_min: 30, enabled: true, ..Default::default() });
        s.rss_feeds.push(RssFeed { id: "f1".into(), name: "Suelto".into(), url: "https://feed.example/rss".into(), ..Default::default() });
        let f = feeds(&s);
        assert_eq!(f.len(), 2);
        assert_eq!((f[0].id.as_str(), f[0].name.as_str(), f[0].interval_min), ("ix-x", "X", 30));
        assert_eq!(f[1].id, "f1");
        // Sin reglas no se consulta nada; una regla de un solo feed activa solo ese.
        assert!(active_feeds(&s).is_empty());
        s.rss_rules.push(RssRule { feeds: vec!["f1".into()], ..rule() });
        assert_eq!(active_feeds(&s).iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), vec!["f1"]);
        s.rss_rules[0].feeds.clear();
        assert_eq!(active_feeds(&s).len(), 2);
    }

    #[test]
    fn quoted_terms() {
        assert_eq!(terms("a, b \"c d\" e"), vec!["a", "b", "c d", "e"]);
        assert_eq!(terms(""), Vec::<String>::new());
    }
}
