//! Los comandos que hablan con nebula-server.
use crate::client::Client;
use crate::fmt::{self, num, str};
use crate::{Cli, Command, Fail, Res};
use serde_json::{json, Value};
use std::sync::Arc;

const EXIT_USAGE: i32 = 2;

pub async fn run(cli: &Cli, c: Client) -> Res {
    let json = cli.json;
    match &cli.command {
        Command::Add { sources, cat, priority, via, name, paused } => add(&c, json, sources, cat, priority, via, name, *paused).await,
        Command::Queue => queue(&c, json).await,
        Command::History { limit, failed } => history(&c, json, *limit, *failed).await,
        Command::Pause { id } => pause(&c, json, id.as_deref(), true).await,
        Command::Resume { id } => pause(&c, json, id.as_deref(), false).await,
        Command::Rm { ids, delete_files } => remove(&c, json, ids, *delete_files).await,
        Command::Speed { limit } => speed(&c, json, limit.as_deref()).await,
        Command::Search { query, indexer, newznab, grab, cat, limit } => {
            search(&c, json, &query.join(" "), indexer, newznab.as_deref(), *grab, cat, *limit).await
        }
        Command::Status => status(&c, json).await,
        _ => unreachable!(),
    }
}

fn done(json: bool, v: Value) {
    if json {
        fmt::print_json(&v);
    }
}

// ---- add ----

#[allow(clippy::too_many_arguments)]
async fn add(c: &Client, json: bool, sources: &[String], cat: &Option<String>, priority: &Option<String>, via: &Option<String>, name: &Option<String>, paused: bool) -> Res {
    let prio = match priority {
        Some(p) => Some(fmt::parse_priority(p).ok_or_else(|| Fail::code(EXIT_USAGE, t!("cli.badPriority", value = p)))?),
        None => None,
    };
    if name.is_some() && sources.len() > 1 {
        return Err(Fail::code(EXIT_USAGE, t!("cli.nameOne")));
    }
    if let Some(cat) = cat {
        check_category(c, cat).await?;
    }
    let mut added = Vec::new();
    let mut failed = 0;
    for src in sources {
        match add_one(c, src, cat.as_deref(), prio, via.as_deref(), name.as_deref(), paused, Vec::new()).await {
            Ok((id, n)) => {
                if !json {
                    println!("{}", t!("cli.added", name = n));
                }
                added.push(json!({ "source": src, "id": id, "name": n }));
            }
            Err(e) => {
                failed += 1;
                eprintln!("{} {}: {}", t!("cli.error"), src, e.msg);
                added.push(json!({ "source": src, "error": e.msg }));
            }
        }
    }
    done(json, json!({ "added": added }));
    if failed > 0 {
        return Err(Fail::new(t!("cli.someFailed", n = failed)));
    }
    Ok(())
}

/// Avisa (sin fallar) si la categoría no existe: Nebula la mandaría a Default, como SABnzbd.
async fn check_category(c: &Client, cat: &str) -> Res {
    let s = c.invoke("get_settings", Value::Null).await?;
    let known: Vec<&str> = s.get("categories").and_then(Value::as_array).map(|a| a.iter().map(|x| str(x, "name")).collect()).unwrap_or_default();
    if !cat.is_empty() && cat != "*" && !known.iter().any(|k| k.eq_ignore_ascii_case(cat)) {
        eprintln!("{}", t!("cli.unknownCategory", category = cat, known = if known.is_empty() { "—".to_string() } else { known.join(", ") }));
    }
    Ok(())
}

/// Analiza el NZB en el servidor y lo pone en la cola. Devuelve (id, nombre).
#[allow(clippy::too_many_arguments)]
pub async fn add_one(
    c: &Client,
    src: &str,
    cat: Option<&str>,
    prio: Option<i32>,
    via: Option<&str>,
    name: Option<&str>,
    paused: bool,
    newznab: Vec<u64>,
) -> Res<(String, String)> {
    add_source(c, Source::guess(src), cat, prio, via, name, paused, newznab).await
}

pub enum Source<'a> {
    Url(&'a str, Option<&'a str>),
    File(&'a str),
}

impl<'a> Source<'a> {
    fn guess(s: &'a str) -> Self {
        if s.starts_with("http://") || s.starts_with("https://") {
            Source::Url(s, None)
        } else {
            Source::File(s)
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn add_source(
    c: &Client,
    src: Source<'_>,
    cat: Option<&str>,
    prio: Option<i32>,
    via: Option<&str>,
    name: Option<&str>,
    paused: bool,
    newznab: Vec<u64>,
) -> Res<(String, String)> {
    let preview = match src {
        Source::Url(url, indexer) => c.invoke("inspect_nzb_url", json!({ "url": url, "indexerId": indexer })).await?,
        Source::File(path) => {
            let data = std::fs::read(path).map_err(|e| Fail::new(t!("cli.readFailed", path = path, error = e)))?;
            let file = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "nebula.nzb".into());
            let remote = c.import(&file, data).await?;
            c.invoke("inspect_nzb_file", json!({ "path": remote })).await?
        }
    };
    let name = name.map(str::to_string).unwrap_or_else(|| str(&preview, "name").to_string());
    let req = json!({
        "token": str(&preview, "token"),
        "name": name,
        "destDir": "",
        "password": preview.get("password").cloned().unwrap_or(Value::Null),
        "paused": paused,
        "top": false,
        "options": Value::Null,
        "via": via,
        "category": cat,
        "priority": prio,
        "newznab": newznab,
    });
    let id = c.invoke("add_job", json!({ "req": req })).await?;
    Ok((id.as_str().unwrap_or_default().to_string(), name))
}

// ---- cola e historial ----

async fn queue(c: &Client, json: bool) -> Res {
    let q = c.invoke("get_queue", Value::Null).await?;
    if json {
        fmt::print_json(&q);
        return Ok(());
    }
    let jobs = q.get("jobs").and_then(Value::as_array).cloned().unwrap_or_default();
    let state = if q.get("paused").and_then(Value::as_bool).unwrap_or(false) { t!("cli.queuePaused") } else { fmt::speed(num(&q, "speed")) };
    println!("{}", t!("cli.queueHead", count = jobs.len(), remaining = fmt::bytes(num(&q, "remaining")), eta = fmt::eta(num(&q, "eta")), state = state));
    if jobs.is_empty() {
        println!("{}", t!("cli.queueEmpty"));
        return Ok(());
    }
    let head = ["#", "ID", "cli.col.status", "cli.col.priority", "%", "cli.col.size", "cli.col.speed", "cli.col.eta", "cli.col.category", "cli.col.name"].map(col);
    let rows: Vec<Vec<String>> = jobs
        .iter()
        .enumerate()
        .map(|(i, j)| {
            vec![
                (i + 1).to_string(),
                short(str(j, "id")),
                fmt::status(str(j, "status")),
                fmt::priority(j.get("priority").and_then(Value::as_i64).unwrap_or(0)),
                format!("{:.1}", j.get("percent").and_then(Value::as_f64).unwrap_or(0.0)),
                format!("{} / {}", fmt::bytes(num(j, "done")), fmt::bytes(num(j, "total"))),
                fmt::speed(num(j, "speed")),
                fmt::eta(num(j, "eta")),
                cat_label(str(j, "category")),
                fmt::cut(str(j, "name"), 70),
            ]
        })
        .collect();
    fmt::table(&head, &rows);
    Ok(())
}

async fn history(c: &Client, json: bool, limit: usize, failed: bool) -> Res {
    let h = c.invoke("get_history", Value::Null).await?;
    let mut items: Vec<Value> = h.as_array().cloned().unwrap_or_default();
    if failed {
        items.retain(|x| str(x, "status") == "failed");
    }
    items.sort_by_key(|x| std::cmp::Reverse(num(x, "completedAt")));
    items.truncate(limit);
    if json {
        fmt::print_json(&Value::Array(items));
        return Ok(());
    }
    if items.is_empty() {
        println!("{}", t!("cli.historyEmpty"));
        return Ok(());
    }
    let head = ["ID", "cli.col.status", "cli.col.size", "cli.col.date", "cli.col.category", "cli.col.name"].map(col);
    let mut rows = Vec::new();
    for x in &items {
        let mut name = fmt::cut(str(x, "name"), 70);
        if let Some(err) = x.get("error").and_then(Value::as_str) {
            name = format!("{}\n    {}", name, err);
        }
        rows.push(vec![short(str(x, "id")), fmt::status(str(x, "status")), fmt::bytes(num(x, "size")), fmt::date(num(x, "completedAt")), cat_label(str(x, "category")), name]);
    }
    fmt::table(&head, &rows);
    Ok(())
}

fn col(k: &str) -> String {
    if k.starts_with("cli.") {
        nebula_core::i18n::text(k)
    } else {
        k.to_string()
    }
}

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

fn cat_label(c: &str) -> String {
    if c.is_empty() || c == "*" {
        "—".into()
    } else {
        c.to_string()
    }
}

/// Encuentra una descarga por su posición en la cola (1, 2…), su id o el principio del id.
fn find<'a>(items: &'a [Value], key: &str, allow_index: bool) -> Result<Option<&'a Value>, Fail> {
    if allow_index {
        if let Ok(n) = key.parse::<usize>() {
            if n >= 1 && n <= items.len() && key.len() < 6 {
                return Ok(Some(&items[n - 1]));
            }
        }
    }
    if let Some(x) = items.iter().find(|x| str(x, "id") == key) {
        return Ok(Some(x));
    }
    let m: Vec<&Value> = items.iter().filter(|x| str(x, "id").starts_with(key)).collect();
    match m.len() {
        0 => Ok(None),
        1 => Ok(Some(m[0])),
        _ => Err(Fail::code(EXIT_USAGE, t!("cli.ambiguous", id = key))),
    }
}

async fn jobs(c: &Client) -> Res<Vec<Value>> {
    let q = c.invoke("get_queue", Value::Null).await?;
    Ok(q.get("jobs").and_then(Value::as_array).cloned().unwrap_or_default())
}

async fn pause(c: &Client, json: bool, id: Option<&str>, pause: bool) -> Res {
    match id {
        None => {
            c.invoke("set_paused", json!({ "paused": pause })).await?;
            if !json {
                println!("{}", if pause { t!("cli.queuePausedNow") } else { t!("cli.queueResumed") });
            }
            done(json, json!({ "paused": pause }));
        }
        Some(key) => {
            let list = jobs(c).await?;
            let j = find(&list, key, true)?.ok_or_else(|| Fail::new(t!("cli.notFound", id = key)))?;
            c.invoke(if pause { "pause_job" } else { "resume_job" }, json!({ "id": str(j, "id") })).await?;
            if !json {
                println!("{}", if pause { t!("cli.jobPaused", name = str(j, "name")) } else { t!("cli.jobResumed", name = str(j, "name")) });
            }
            done(json, json!({ "id": str(j, "id"), "paused": pause }));
        }
    }
    Ok(())
}

async fn remove(c: &Client, json: bool, keys: &[String], delete_files: bool) -> Res {
    let list = jobs(c).await?;
    let hist: Vec<Value> = c.invoke("get_history", Value::Null).await?.as_array().cloned().unwrap_or_default();
    let mut removed = Vec::new();
    // Primero se resuelven todas (las posiciones cambian al borrar).
    let mut targets = Vec::new();
    for key in keys {
        if let Some(j) = find(&list, key, true)? {
            targets.push(("remove_job", str(j, "id").to_string(), str(j, "name").to_string()));
        } else if let Some(h) = find(&hist, key, false)? {
            targets.push(("remove_history", str(h, "id").to_string(), str(h, "name").to_string()));
        } else {
            return Err(Fail::new(t!("cli.notFound", id = key)));
        }
    }
    for (cmd, id, name) in targets {
        c.invoke(cmd, json!({ "id": id, "deleteFiles": delete_files })).await?;
        if !json {
            println!("{}", t!("cli.removed", name = name));
        }
        removed.push(json!({ "id": id, "name": name }));
    }
    done(json, json!({ "removed": removed }));
    Ok(())
}

async fn speed(c: &Client, json: bool, limit: Option<&str>) -> Res {
    let kbps = match limit {
        None => {
            let q = c.invoke("get_queue", Value::Null).await?;
            num(&q, "speedLimit")
        }
        Some(l) => {
            let k = fmt::parse_speed(l).ok_or_else(|| Fail::code(EXIT_USAGE, t!("cli.badSpeed", value = l)))?;
            c.invoke("set_speed_limit", json!({ "kbps": k })).await?;
            k
        }
    };
    if json {
        fmt::print_json(&json!({ "speedLimitKbps": kbps }));
    } else if kbps == 0 {
        println!("{}", t!("cli.noLimit"));
    } else {
        println!("{}", t!("cli.limit", speed = fmt::speed(kbps * 1024)));
    }
    Ok(())
}

// ---- búsqueda ----

#[allow(clippy::too_many_arguments)]
async fn search(c: &Client, json: bool, query: &str, only: &[String], newznab: Option<&str>, grab: Option<usize>, cat: &Option<String>, limit: usize) -> Res {
    let settings = c.invoke("get_settings", Value::Null).await?;
    let indexers: Vec<Value> = settings
        .get("indexers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|ix| ix.get("enabled").and_then(Value::as_bool).unwrap_or(true))
        .filter(|ix| only.is_empty() || only.iter().any(|o| str(ix, "id") == o || str(ix, "name").eq_ignore_ascii_case(o)))
        .collect();
    if indexers.is_empty() {
        return Err(Fail::new(t!("cli.noIndexers")));
    }
    let c = Arc::new(c.clone());
    let mut set = tokio::task::JoinSet::new();
    for ix in &indexers {
        let (c, id, q, cat) = (c.clone(), str(ix, "id").to_string(), query.to_string(), newznab.map(str::to_string));
        let label = if str(ix, "name").is_empty() { str(ix, "url").to_string() } else { str(ix, "name").to_string() };
        set.spawn(async move { (label, c.invoke("search_indexer", json!({ "id": id, "query": q, "category": cat, "offset": 0, "searchId": Value::Null })).await) });
    }
    let mut items: Vec<Value> = Vec::new();
    while let Some(r) = set.join_next().await {
        let Ok((label, r)) = r else { continue };
        match r {
            Ok(v) => items.extend(v.get("items").and_then(Value::as_array).cloned().unwrap_or_default()),
            Err(e) => eprintln!("{} {}: {}", t!("cli.warning"), label, e.msg),
        }
    }
    items.sort_by_key(|x| std::cmp::Reverse(num(x, "date")));
    items.truncate(limit);

    if let Some(n) = grab {
        let item = items.get(n.wrapping_sub(1)).ok_or_else(|| Fail::code(EXIT_USAGE, t!("cli.noResultN", n = n)))?;
        let newznab: Vec<u64> = item.get("categoryIds").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
        let (id, name) = add_source(&c, Source::Url(str(item, "link"), Some(str(item, "indexerId"))), cat.as_deref(), None, None, Some(str(item, "title")), false, newznab).await?;
        if json {
            fmt::print_json(&json!({ "grabbed": { "id": id, "name": name } }));
        } else {
            println!("{}", t!("cli.added", name = name));
        }
        return Ok(());
    }
    if json {
        fmt::print_json(&Value::Array(items));
        return Ok(());
    }
    if items.is_empty() {
        println!("{}", t!("cli.noResults"));
        return Ok(());
    }
    let now = nebula_core::util::now_secs();
    let head = ["#", "cli.col.age", "cli.col.size", "cli.col.grabs", "cli.col.indexer", "cli.col.name"].map(col);
    let rows: Vec<Vec<String>> = items
        .iter()
        .enumerate()
        .map(|(i, x)| {
            let age = now.saturating_sub(num(x, "date")) / 86400;
            vec![
                (i + 1).to_string(),
                if num(x, "date") == 0 { "—".into() } else { format!("{}d", age) },
                fmt::bytes(num(x, "size")),
                x.get("grabs").and_then(Value::as_u64).map(|g| g.to_string()).unwrap_or_else(|| "—".into()),
                fmt::cut(str(x, "indexerName"), 16),
                fmt::cut(str(x, "title"), 80),
            ]
        })
        .collect();
    fmt::table(&head, &rows);
    println!("\n{}", t!("cli.grabHint"));
    Ok(())
}

// ---- estado ----

async fn status(c: &Client, json: bool) -> Res {
    let info = c.invoke("app_info", Value::Null).await?;
    let q = c.invoke("get_queue", Value::Null).await?;
    let s = c.invoke("get_settings", Value::Null).await?;
    let disk = c.invoke("disk_free", json!({ "path": str(&s, "downloadDir") })).await.unwrap_or(Value::Null);
    let tools = c.invoke("tools_status", Value::Null).await.unwrap_or(Value::Null);
    if json {
        fmt::print_json(&json!({
            "url": c.base,
            "version": info.get("version"),
            "platform": info.get("platform"),
            "paused": q.get("paused"),
            "speed": q.get("speed"),
            "speedLimitKbps": q.get("speedLimit"),
            "remaining": q.get("remaining"),
            "eta": q.get("eta"),
            "jobs": q.get("jobs").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0),
            "activeConnections": q.get("activeConns"),
            "servers": q.get("servers"),
            "downloadDir": s.get("downloadDir"),
            "diskFree": disk,
            "tools": tools,
        }));
        return Ok(());
    }
    let jobs = q.get("jobs").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0);
    println!("Nebula {} · {}", str(&info, "version"), c.base);
    let state = if q.get("paused").and_then(Value::as_bool).unwrap_or(false) { t!("cli.queuePaused") } else { fmt::speed(num(&q, "speed")) };
    println!("{}", t!("cli.queueHead", count = jobs, remaining = fmt::bytes(num(&q, "remaining")), eta = fmt::eta(num(&q, "eta")), state = state));
    let limit = num(&q, "speedLimit");
    println!("{}", if limit == 0 { t!("cli.noLimit") } else { t!("cli.limit", speed = fmt::speed(limit * 1024)) });
    let free = disk.as_u64().map(fmt::bytes).unwrap_or_else(|| "—".into());
    println!("{}", t!("cli.downloadsAt", path = str(&s, "downloadDir"), free = free));
    if let Some(servers) = q.get("servers").and_then(Value::as_array) {
        for sv in servers {
            let err = sv.get("lastError").and_then(Value::as_str).map(|e| format!(" · {}", e)).unwrap_or_default();
            println!("  {} {}  {}/{}  {}{}", t!("cli.server"), str(sv, "name"), num(sv, "active"), num(sv, "maxConns"), str(sv, "state"), err);
        }
    }
    if let Some(tools) = tools.as_array() {
        for tl in tools.iter().filter(|t| !t.get("found").and_then(Value::as_bool).unwrap_or(true)) {
            println!("  {}", t!("cli.toolMissing", tool = str(tl, "tool")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_jobs() {
        let items = vec![json!({"id": "abc123", "name": "A"}), json!({"id": "abd999", "name": "B"})];
        assert_eq!(str(find(&items, "2", true).unwrap().unwrap(), "name"), "B");
        assert_eq!(str(find(&items, "abd", true).unwrap().unwrap(), "name"), "B");
        assert!(find(&items, "ab", true).is_err());
        assert!(find(&items, "zz", true).unwrap().is_none());
        assert!(find(&items, "2", false).unwrap().is_none());
    }
}
