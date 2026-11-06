//! API compatible con Newznab (como la de NZBHydra2): Sonarr, Radarr, Lidarr, Readarr, Prowlarr…
//! pueden usar Nebula como un único indexador que busca a la vez en todos los indexadores
//! configurados. Los enlaces de descarga apuntan a Nebula, que pide el .nzb al indexador
//! sin enseñar su API key. Un servidor HTTP mínimo: una petición por conexión.
//!
//! El mismo servidor atiende la API compatible con SABnzbd (sab.rs): las peticiones con `mode=`
//! van allí y las que llevan `t=`, a la de Newznab.
use crate::config::IndexerConfig;
use crate::engine::Engine;
use crate::indexer::{self, SearchItem};
use crate::stats::IndexerEvent;
use parking_lot::Mutex;
use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::net::IpAddr;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const DEFAULT_PORT: u16 = 5077;
/// Enlaces de descarga que se recuerdan (los más antiguos se olvidan).
const MAX_LINKS: usize = 20_000;
const MAX_HEAD: usize = 16 * 1024;
/// Tamaño máximo del cuerpo de un POST (un NZB subido con addfile).
const MAX_BODY: usize = 100 * 1024 * 1024;

#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ApiStatus {
    pub running: bool,
    pub error: Option<String>,
    pub port: u16,
    /// Direcciones a las que se pueden conectar los programas (localhost y, con red local, la IP del equipo).
    pub urls: Vec<String>,
}

struct Running {
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

static CTL: LazyLock<tokio::sync::Mutex<Option<Running>>> = LazyLock::new(|| tokio::sync::Mutex::new(None));
static STATUS: LazyLock<Mutex<ApiStatus>> = LazyLock::new(|| Mutex::new(ApiStatus::default()));

pub fn status() -> ApiStatus {
    STATUS.lock().clone()
}

fn set_status(st: ApiStatus) {
    *STATUS.lock() = st.clone();
    crate::bus::emit(crate::bus::Event::ApiStatus(st));
}

/// Arranca, para o reinicia el servidor según los ajustes (activada, puerto, red local).
pub fn restart(e: &Arc<Engine>) {
    let e = e.clone();
    crate::rt::spawn(async move {
        let mut ctl = CTL.lock().await;
        let was_running = ctl.is_some();
        if let Some(r) = ctl.take() {
            let _ = r.stop.send(());
            let _ = r.task.await;
        }
        let (enabled, port, lan) = {
            let s = e.settings.read();
            (s.api_enabled, if s.api_port == 0 { DEFAULT_PORT } else { s.api_port }, s.api_lan)
        };
        if !enabled {
            if was_running {
                crate::linfo!("{}", t!("log.api.stopped"));
            }
            set_status(ApiStatus { port, ..Default::default() });
            return;
        }
        let host = if lan { "0.0.0.0" } else { "127.0.0.1" };
        match TcpListener::bind((host, port)).await {
            Ok(listener) => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let task = crate::rt::spawn(serve(e.clone(), listener, rx, lan));
                *ctl = Some(Running { stop: tx, task });
                let mut urls = vec![format!("http://localhost:{}/api", port)];
                if lan {
                    if let Some(ip) = lan_ip() {
                        urls.push(format!("http://{}:{}/api", ip, port));
                    }
                }
                crate::linfo!("{}", t!("log.api.started", url = urls.last().cloned().unwrap_or_default()));
                set_status(ApiStatus { running: true, error: None, port, urls });
            }
            Err(err) => {
                let msg = if err.kind() == std::io::ErrorKind::AddrInUse {
                    t!("api.portInUse", port = port)
                } else {
                    t!("api.bindFailed", port = port, error = err)
                };
                crate::lerror!("{}", msg);
                set_status(ApiStatus { running: false, error: Some(msg), port, urls: Vec::new() });
            }
        }
    });
}

/// IP del equipo en la red local (la de la interfaz con salida; no se envía nada).
pub(crate) fn lan_ip() -> Option<IpAddr> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:80").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

async fn serve(e: Arc<Engine>, listener: TcpListener, mut stop: tokio::sync::oneshot::Receiver<()>, lan: bool) {
    loop {
        tokio::select! {
            _ = &mut stop => break,
            r = listener.accept() => match r {
                Ok((sock, peer)) => {
                    if !lan && !peer.ip().is_loopback() {
                        continue;
                    }
                    let e = e.clone();
                    tokio::spawn(async move {
                        let _ = tokio::time::timeout(Duration::from_secs(120), handle(e, sock, peer.ip())).await;
                    });
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }
}

// ---- HTTP ----

struct Request {
    method: String,
    path: String,
    /// Parámetros de la URL y, detrás, los del formulario del cuerpo (urlencoded o multipart).
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    files: Vec<crate::sab::Upload>,
}

impl Request {
    fn param(&self, key: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }
    fn header(&self, key: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }
}

struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    headers: Vec<(&'static str, String)>,
}

impl Response {
    fn xml(body: String) -> Self {
        Self { status: 200, content_type: "application/xml; charset=utf-8", body: body.into_bytes(), headers: Vec::new() }
    }
}

fn parse_request(head: &str) -> Option<Request> {
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_ascii_uppercase();
    let target = first.next()?;
    let (path, qs) = target.split_once('?').unwrap_or((target, ""));
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    Some(Request { method, path: decode(path), query: parse_query(qs), headers, files: Vec::new() })
}

/// Añade a la petición el formulario de su cuerpo: `application/x-www-form-urlencoded` o
/// `multipart/form-data` (los campos van a los parámetros y los archivos, a `files`).
fn parse_body(req: &mut Request, body: &[u8]) {
    if body.is_empty() {
        return;
    }
    let ctype = req.header("Content-Type").unwrap_or("").to_string();
    let lower = ctype.to_ascii_lowercase();
    if lower.starts_with("multipart/form-data") {
        let Some(boundary) = ctype.split(';').map(str::trim).find_map(|p| {
            let (k, v) = p.split_once('=')?;
            k.trim().eq_ignore_ascii_case("boundary").then(|| v.trim().trim_matches('"').to_string())
        }) else {
            return;
        };
        for (name, filename, data) in multipart(body, &boundary) {
            match filename {
                Some(f) => req.files.push(crate::sab::Upload { field: name, filename: f, data }),
                None => req.query.push((name, String::from_utf8_lossy(&data).to_string())),
            }
        }
    } else if lower.starts_with("application/x-www-form-urlencoded") || lower.is_empty() {
        if let Ok(text) = std::str::from_utf8(body) {
            req.query.extend(parse_query(text.trim()));
        }
    }
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= hay.len() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|i| i + from)
}

/// Partes de un cuerpo multipart: (campo, nombre de archivo si es un archivo, datos).
fn multipart(body: &[u8], boundary: &str) -> Vec<(String, Option<String>, Vec<u8>)> {
    let delim = format!("--{}", boundary).into_bytes();
    let mut out = Vec::new();
    let Some(mut pos) = find(body, &delim, 0) else { return out };
    loop {
        let start = pos + delim.len();
        if body[start..].starts_with(b"--") {
            break;
        }
        let Some(head_end) = find(body, b"\r\n\r\n", start) else { break };
        let Some(next) = find(body, &delim, head_end + 4) else { break };
        let head = String::from_utf8_lossy(&body[start..head_end]).to_string();
        let mut data_end = next;
        if data_end >= 2 && &body[data_end - 2..data_end] == b"\r\n" {
            data_end -= 2;
        }
        let data = body[head_end + 4..data_end.max(head_end + 4)].to_vec();
        let disp = head.lines().find(|l| l.to_ascii_lowercase().starts_with("content-disposition")).unwrap_or("");
        let attr = |key: &str| -> Option<String> {
            disp.split(';').map(str::trim).find_map(|p| {
                let (k, v) = p.split_once('=')?;
                k.trim().eq_ignore_ascii_case(key).then(|| v.trim().trim_matches('"').to_string())
            })
        };
        if let Some(name) = attr("name") {
            let filename = attr("filename*").map(|f| crate::util::percent_decode(f.rsplit("''").next().unwrap_or(&f))).or_else(|| attr("filename"));
            out.push((name, filename, data));
        }
        pos = next;
    }
    out
}

/// Decodifica un cuerpo `Transfer-Encoding: chunked` completo; None si aún está incompleto.
fn dechunk(raw: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let line_end = find(raw, b"\r\n", pos)?;
        let size_txt = std::str::from_utf8(&raw[pos..line_end]).ok()?;
        let size = usize::from_str_radix(size_txt.split(';').next()?.trim(), 16).ok()?;
        pos = line_end + 2;
        if size == 0 {
            return Some(out);
        }
        if raw.len() < pos + size + 2 {
            return None;
        }
        out.extend_from_slice(&raw[pos..pos + size]);
        pos += size + 2;
    }
}

fn parse_query(qs: &str) -> Vec<(String, String)> {
    qs.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect()
}

fn decode(s: &str) -> String {
    crate::util::percent_decode(&s.replace('+', " "))
}

async fn handle(e: Arc<Engine>, mut sock: TcpStream, peer: IpAddr) {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = vec![0u8; 64 * 1024];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return;
        }
        match tokio::time::timeout(Duration::from_secs(15), sock.read(&mut chunk)).await {
            Ok(Ok(n)) if n > 0 => buf.extend_from_slice(&chunk[..n]),
            _ => return,
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let Some(mut req) = parse_request(&head) else {
        let _ = write(&mut sock, false, plain(400, "Bad Request")).await;
        return;
    };
    // Cuerpo de los POST (addfile sube el NZB en un formulario multipart).
    let mut body = buf[head_end + 4..].to_vec();
    let chunked = req.header("Transfer-Encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    let length = req.header("Content-Length").and_then(|v| v.trim().parse::<usize>().ok());
    if length.is_some_and(|l| l > MAX_BODY) {
        let _ = write(&mut sock, false, plain(413, "Payload Too Large")).await;
        return;
    }
    if chunked || length.is_some_and(|l| l > 0) {
        if req.header("Expect").is_some_and(|v| v.eq_ignore_ascii_case("100-continue")) {
            let _ = sock.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await;
        }
        loop {
            if !chunked && body.len() >= length.unwrap_or(0) {
                body.truncate(length.unwrap_or(0));
                break;
            }
            if chunked {
                if let Some(b) = dechunk(&body) {
                    body = b;
                    break;
                }
            }
            if body.len() > MAX_BODY + 64 * 1024 {
                let _ = write(&mut sock, false, plain(413, "Payload Too Large")).await;
                return;
            }
            match tokio::time::timeout(Duration::from_secs(30), sock.read(&mut chunk)).await {
                Ok(Ok(n)) if n > 0 => body.extend_from_slice(&chunk[..n]),
                _ => return,
            }
        }
        parse_body(&mut req, &body);
    }
    let resp = match req.method.as_str() {
        "GET" | "HEAD" | "POST" => route(&e, &req, peer).await,
        "OPTIONS" => Response { status: 204, content_type: "text/plain", body: Vec::new(), headers: Vec::new() },
        _ => plain(405, "Method Not Allowed"),
    };
    let _ = write(&mut sock, req.method == "HEAD", resp).await;
}

fn plain(status: u16, text: &str) -> Response {
    Response { status, content_type: "text/plain; charset=utf-8", body: text.as_bytes().to_vec(), headers: Vec::new() }
}

async fn write(sock: &mut TcpStream, head_only: bool, r: Response) -> std::io::Result<()> {
    let reason = match r.status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        _ => "Error",
    };
    let mut out = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: X-Api-Key\r\nServer: Nebula/{}\r\n",
        r.status,
        reason,
        r.content_type,
        r.body.len(),
        env!("CARGO_PKG_VERSION")
    );
    for (k, v) in &r.headers {
        let _ = write!(out, "{}: {}\r\n", k, v);
    }
    out.push_str("\r\n");
    sock.write_all(out.as_bytes()).await?;
    if !head_only {
        sock.write_all(&r.body).await?;
    }
    sock.shutdown().await
}

// ---- Newznab ----

/// Parámetros que se pasan tal cual a los indexadores.
const FORWARD: [&str; 21] = [
    "q", "cat", "season", "ep", "imdbid", "tvdbid", "rid", "tvmazeid", "tmdbid", "traktid", "artist", "album", "label", "track",
    "year", "genre", "author", "title", "maxage", "minsize", "maxsize",
];

fn error(code: u16, description: &str) -> Response {
    Response::xml(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<error code=\"{}\" description=\"{}\"/>\n",
        code,
        esc(description)
    ))
}

async fn route(e: &Arc<Engine>, req: &Request, peer: IpAddr) -> Response {
    let path = req.path.trim_end_matches('/').to_ascii_lowercase();
    if path.is_empty() {
        return plain(200, "Nebula · Newznab API: /api?t=caps · SABnzbd API: /api?mode=version");
    }
    // API de SABnzbd: /api o /sabnzbd/api con mode=…
    if (path == "/api" || path == "/sabnzbd/api" || path == "/sabnzbd") && req.param("mode").is_some() && req.param("t").is_none() {
        let call = crate::sab::Call { params: &req.query, files: &req.files };
        let (status, v) = crate::sab::handle(e, &call).await;
        return json_response(status, &v);
    }
    if path != "/api" && path != "/newznab/api" && path != "/newznab" {
        return Response { status: 404, ..error(404, "Not found") };
    }
    let func = req.param("t").unwrap_or("").trim().to_ascii_lowercase();
    if func.is_empty() {
        return error(200, "Missing parameter (t)");
    }
    if func == "caps" {
        return Response::xml(caps());
    }
    let key = e.settings.read().api_key.trim().to_string();
    let given = req.param("apikey").or_else(|| req.header("X-Api-Key")).unwrap_or("").trim();
    if key.is_empty() || !same_key(given, &key) {
        return error(100, "Incorrect user credentials");
    }
    match func.as_str() {
        "search" | "tvsearch" | "tv-search" | "movie" | "movie-search" | "music" | "audio" | "audio-search" | "book" | "book-search" => {
            search(e, req, &func, peer).await
        }
        "get" => get_nzb(e, req.param("id").unwrap_or("")).await,
        "details" | "getnfo" | "comments" | "comments-add" | "cart-add" | "cart-del" | "register" | "user" => {
            error(203, "Function not available")
        }
        _ => error(202, "No such function"),
    }
}

fn json_response(status: u16, v: &serde_json::Value) -> Response {
    Response {
        status,
        content_type: "application/json; charset=utf-8",
        body: serde_json::to_vec(v).unwrap_or_default(),
        headers: Vec::new(),
    }
}

/// Compara sin cortar en el primer carácter distinto.
fn same_key(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

const CATEGORIES: &[(u32, &str, &[(u32, &str)])] = &[
    (1000, "Console", &[(1010, "NDS"), (1020, "PSP"), (1030, "Wii"), (1040, "XBox"), (1050, "XBox 360"), (1060, "Wiiware"), (1070, "XBox 360 DLC"), (1080, "PS3"), (1090, "Other"), (1110, "3DS"), (1120, "PS Vita"), (1130, "WiiU"), (1140, "XBox One"), (1180, "PS4")]),
    (2000, "Movies", &[(2010, "Foreign"), (2020, "Other"), (2030, "SD"), (2040, "HD"), (2045, "UHD"), (2050, "BluRay"), (2060, "3D"), (2070, "DVD"), (2080, "WEB-DL")]),
    (3000, "Audio", &[(3010, "MP3"), (3020, "Video"), (3030, "Audiobook"), (3040, "Lossless"), (3050, "Other"), (3060, "Foreign")]),
    (4000, "PC", &[(4010, "0day"), (4020, "ISO"), (4030, "Mac"), (4040, "Mobile-Other"), (4050, "Games"), (4060, "Mobile-iOS"), (4070, "Mobile-Android")]),
    (5000, "TV", &[(5010, "WEB-DL"), (5020, "Foreign"), (5030, "SD"), (5040, "HD"), (5045, "UHD"), (5050, "Other"), (5060, "Sport"), (5070, "Anime"), (5080, "Documentary")]),
    (6000, "XXX", &[(6010, "DVD"), (6020, "WMV"), (6030, "XviD"), (6040, "x264"), (6045, "UHD"), (6050, "Pack"), (6060, "ImgSet"), (6070, "Other")]),
    (7000, "Books", &[(7010, "Mags"), (7020, "EBook"), (7030, "Comics"), (7040, "Technical"), (7050, "Other"), (7060, "Foreign")]),
    (8000, "Other", &[(8010, "Misc"), (8020, "Hashed")]),
];

fn caps() -> String {
    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<caps>\n");
    let _ = writeln!(
        x,
        "  <server version=\"{}\" title=\"Nebula\" strapline=\"Nebula · Newznab API\" url=\"https://github.com/Comasss/nebula-usenet\"/>",
        env!("CARGO_PKG_VERSION")
    );
    x.push_str("  <limits max=\"500\" default=\"100\"/>\n  <registration available=\"no\" open=\"no\"/>\n  <searching>\n");
    x.push_str("    <search available=\"yes\" supportedParams=\"q\"/>\n");
    x.push_str("    <tv-search available=\"yes\" supportedParams=\"q,season,ep,imdbid,tvdbid,rid,tvmazeid\"/>\n");
    x.push_str("    <movie-search available=\"yes\" supportedParams=\"q,imdbid,tmdbid\"/>\n");
    x.push_str("    <audio-search available=\"yes\" supportedParams=\"q,artist,album\"/>\n");
    x.push_str("    <book-search available=\"yes\" supportedParams=\"q,author,title\"/>\n");
    x.push_str("  </searching>\n  <categories>\n");
    for (id, name, subs) in CATEGORIES {
        let _ = writeln!(x, "    <category id=\"{}\" name=\"{}\">", id, name);
        for (sid, sname) in *subs {
            let _ = writeln!(x, "      <subcat id=\"{}\" name=\"{}\"/>", sid, sname);
        }
        x.push_str("    </category>\n");
    }
    x.push_str("  </categories>\n</caps>\n");
    x
}

/// Indexadores activos, limitados a los pedidos con `indexers=` (nombres o ids separados por comas).
fn pick_indexers(all: &[IndexerConfig], filter: Option<&str>) -> Vec<IndexerConfig> {
    let wanted: Vec<String> = filter
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    all.iter()
        .filter(|ix| ix.enabled && !ix.url.trim().is_empty())
        .filter(|ix| wanted.is_empty() || wanted.iter().any(|w| *w == ix.id.to_lowercase() || *w == ix.label().to_lowercase()))
        .cloned()
        .collect()
}

async fn search(e: &Arc<Engine>, req: &Request, func: &str, peer: IpAddr) -> Response {
    let ifunc: &'static str = match func {
        "tvsearch" | "tv-search" => "tvsearch",
        "movie" | "movie-search" => "movie",
        "music" | "audio" | "audio-search" => "music",
        "book" | "book-search" => "book",
        _ => "search",
    };
    let (indexers, dedup) = {
        let s = e.settings.read();
        (pick_indexers(&s.indexers, req.param("indexers")), s.search_dedup)
    };
    let offset: u32 = req.param("offset").and_then(|v| v.trim().parse().ok()).unwrap_or(0);
    let limit: u32 = req.param("limit").and_then(|v| v.trim().parse().ok()).unwrap_or(100).clamp(1, 500);
    let params: Vec<(&'static str, String)> = FORWARD
        .iter()
        .filter_map(|k| req.param(k).map(|v| v.trim()).filter(|v| !v.is_empty()).map(|v| (*k, v.to_string())))
        .collect();
    let search_id = crate::util::new_id();
    let mut set = tokio::task::JoinSet::new();
    for ix in indexers {
        let params = params.clone();
        set.spawn(async move {
            let start = Instant::now();
            let r = indexer::query(&ix, ifunc, &params, offset, limit).await;
            (ix, r, start.elapsed().as_millis() as u64)
        });
    }
    let mut items = Vec::new();
    let mut total = 0u64;
    let mut order = Vec::new();
    let mut errors = Vec::new();
    let mut asked = 0;
    while let Some(Ok((ix, res, ms))) = set.join_next().await {
        asked += 1;
        e.record_indexer(IndexerEvent {
            at: crate::util::now_secs(),
            id: ix.id.clone(),
            name: ix.label(),
            kind: "search".into(),
            ms: res.as_ref().map(|r| r.ms).unwrap_or(ms),
            ok: res.is_ok(),
            results: res.as_ref().map(|r| r.items.len() as u64).unwrap_or(0),
            offset,
            search: Some(search_id.clone()),
        });
        match res {
            Ok(r) => {
                total += r.total.unwrap_or(offset as u64 + r.items.len() as u64);
                items.extend(r.items);
            }
            Err(err) => {
                crate::lwarn!("API · {}: {}", ix.label(), err);
                errors.push(format!("{}: {}", ix.label(), err));
            }
        }
        order.push(ix.id);
    }
    if asked > 0 && errors.len() == asked {
        return error(900, &errors.join(" · "));
    }
    // Prioridad como en la búsqueda de la app: orden de la lista de indexadores.
    let s_order: Vec<String> = e.settings.read().indexers.iter().map(|i| i.id.clone()).collect();
    let rank = |id: &str| s_order.iter().position(|x| x == id).unwrap_or(usize::MAX);
    items.sort_by(|a, b| rank(&a.indexer_id).cmp(&rank(&b.indexer_id)).then(b.grabs.unwrap_or(0).cmp(&a.grabs.unwrap_or(0))));
    if dedup {
        let before = items.len();
        items = dedupe(items);
        total = total.saturating_sub((before - items.len()) as u64);
    }
    items.sort_by(|a, b| b.date.cmp(&a.date));
    let what = req.param("q").or_else(|| req.param("imdbid")).or_else(|| req.param("tvdbid")).unwrap_or("").to_string();
    crate::linfo!("{}", t!("log.api.search", query = what, from = peer, results = items.len()));
    let base = base_url(e, req);
    let key = e.settings.read().api_key.trim().to_string();
    Response::xml(rss(&items, offset, total, &base, &key))
}

static REPOST_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)[-._ ](obfuscated|postbot|xpost|repost|scrambled|asrequested|buymore|rakuv\d*|whiterev)$").unwrap());
static EXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.(nzb|mkv|mp4|avi)$").unwrap());
static BRACKET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\[[^\]]*\]\s*$").unwrap());

/// Mismo criterio que la búsqueda de la app (src/lib/search.js): nombre normalizado.
pub(crate) fn normalize_title(title: &str) -> String {
    let mut s = title.trim().to_lowercase();
    s = EXT.replace(&s, "").to_string();
    s = BRACKET.replace(&s, "").to_string();
    loop {
        let next = REPOST_TAG.replace(&s, "").trim().to_string();
        if next == s {
            break;
        }
        s = next;
    }
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

fn same_size(a: u64, b: u64) -> bool {
    a == 0 || b == 0 || (a.abs_diff(b) as f64) / (a.max(b) as f64) < 0.03
}

/// Deja un resultado por release (nombre normalizado y tamaño ±3 %): el primero de la lista,
/// que ya viene ordenada por prioridad del indexador y descargas.
fn dedupe(items: Vec<SearchItem>) -> Vec<SearchItem> {
    let mut seen: HashMap<String, Vec<u64>> = HashMap::new();
    let mut out = Vec::with_capacity(items.len());
    for it in items {
        let sizes = seen.entry(normalize_title(&it.title)).or_default();
        if sizes.iter().any(|s| same_size(*s, it.size)) {
            continue;
        }
        sizes.push(it.size);
        out.push(it);
    }
    out
}

fn base_url(e: &Engine, req: &Request) -> String {
    let host = req
        .header("Host")
        .filter(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_alphanumeric() || ".-:[]_".contains(c)))
        .map(|h| h.to_string())
        .unwrap_or_else(|| format!("localhost:{}", e.settings.read().api_port));
    format!("http://{}", host)
}

// ---- Enlaces de descarga ----

struct Link {
    url: String,
    title: String,
    indexer_id: String,
    at: u64,
}

static LINKS: LazyLock<Mutex<HashMap<String, Link>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Identificador estable del resultado: el mismo enlace da siempre el mismo id.
fn remember(it: &SearchItem) -> String {
    use md5::{Digest, Md5};
    let id: String = Md5::digest(format!("{}\n{}", it.indexer_id, it.link)).iter().map(|b| format!("{:02x}", b)).collect();
    let mut links = LINKS.lock();
    links.insert(
        id.clone(),
        Link { url: it.link.clone(), title: it.title.clone(), indexer_id: it.indexer_id.clone(), at: crate::util::now_ms() },
    );
    if links.len() > MAX_LINKS {
        let mut ages: Vec<u64> = links.values().map(|l| l.at).collect();
        ages.sort_unstable();
        let cut = ages[links.len() - MAX_LINKS * 9 / 10];
        links.retain(|_, l| l.at >= cut);
    }
    id
}

async fn get_nzb(e: &Arc<Engine>, id: &str) -> Response {
    let found = LINKS.lock().get(id.trim()).map(|l| (l.url.clone(), l.title.clone(), l.indexer_id.clone()));
    let Some((url, title, indexer_id)) = found else {
        return error(300, "No such item (search again)");
    };
    match e.grab_nzb( &url, Some(&indexer_id)).await {
        Ok((data, f)) => {
            let mut name = f.filename.filter(|n| !n.trim().is_empty()).unwrap_or(title);
            if !name.to_ascii_lowercase().ends_with(".nzb") {
                name.push_str(".nzb");
            }
            let name = crate::util::sanitize_filename(&name);
            let ascii: String = name.chars().map(|c| if c.is_ascii() && c != '"' && c != '\\' { c } else { '_' }).collect();
            let disposition = format!("attachment; filename=\"{}\"; filename*=UTF-8''{}", ascii, pct(&name));
            Response { status: 200, content_type: "application/x-nzb", body: data, headers: vec![("Content-Disposition", disposition)] }
        }
        Err(err) => {
            crate::lwarn!("API · {}", err);
            error(300, &err)
        }
    }
}

fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) })
        .collect()
}

// ---- RSS ----

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => o.push(c),
        }
    }
    o
}

fn rss(items: &[SearchItem], offset: u32, total: u64, base: &str, key: &str) -> String {
    let mut x = String::with_capacity(1024 + items.len() * 1024);
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    x.push_str("<rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\" xmlns:newznab=\"http://www.newznab.com/DTD/2010/feeds/attributes/\">\n<channel>\n");
    let _ = writeln!(x, "<atom:link href=\"{}/api\" rel=\"self\" type=\"application/rss+xml\"/>", esc(base));
    let _ = writeln!(x, "<title>Nebula</title>\n<description>Nebula · Newznab API</description>\n<link>{}</link>", esc(base));
    let _ = writeln!(x, "<newznab:response offset=\"{}\" total=\"{}\"/>", offset, total.max(offset as u64 + items.len() as u64));
    for it in items {
        let id = remember(it);
        let link = format!("{}/api?t=get&id={}&apikey={}", base, id, pct(key));
        x.push_str("<item>\n");
        let _ = writeln!(x, " <title>{}</title>", esc(&it.title));
        let _ = writeln!(x, " <guid isPermaLink=\"false\">{}</guid>", id);
        let _ = writeln!(x, " <link>{}</link>", esc(&link));
        if let Some(d) = &it.details {
            let _ = writeln!(x, " <comments>{}</comments>", esc(d));
        }
        if it.date > 0 {
            let _ = writeln!(x, " <pubDate>{}</pubDate>", rfc2822(it.date));
        }
        if !it.category.is_empty() {
            let _ = writeln!(x, " <category>{}</category>", esc(&it.category));
        }
        let _ = writeln!(x, " <description>{}</description>", esc(&it.title));
        let _ = writeln!(x, " <enclosure url=\"{}\" length=\"{}\" type=\"application/x-nzb\"/>", esc(&link), it.size);
        for c in &it.category_ids {
            let _ = writeln!(x, " <newznab:attr name=\"category\" value=\"{}\"/>", c);
        }
        let _ = writeln!(x, " <newznab:attr name=\"size\" value=\"{}\"/>", it.size);
        if let Some(g) = it.grabs {
            let _ = writeln!(x, " <newznab:attr name=\"grabs\" value=\"{}\"/>", g);
        }
        if let Some(f) = it.files {
            let _ = writeln!(x, " <newznab:attr name=\"files\" value=\"{}\"/>", f);
        }
        if let Some(p) = it.password {
            let _ = writeln!(x, " <newznab:attr name=\"password\" value=\"{}\"/>", p);
        }
        if it.date > 0 {
            let _ = writeln!(x, " <newznab:attr name=\"usenetdate\" value=\"{}\"/>", rfc2822(it.date));
        }
        let _ = writeln!(x, " <newznab:attr name=\"hydraIndexerName\" value=\"{}\"/>", esc(&it.indexer_name));
        x.push_str("</item>\n");
    }
    x.push_str("</channel>\n</rss>\n");
    x
}

/// Tiempo unix a "Tue, 29 Sep 2026 10:00:00 +0000".
fn rfc2822(ts: u64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let days = (ts / 86400) as i64;
    let secs = ts % 86400;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} +0000",
        DAYS[(days.rem_euclid(7)) as usize],
        d,
        MONTHS[(m - 1) as usize],
        y,
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(title: &str, size: u64, indexer: &str, link: &str) -> SearchItem {
        SearchItem {
            title: title.into(),
            link: link.into(),
            details: Some("https://idx.example/details/1".into()),
            size,
            date: 1_790_000_000,
            grabs: Some(3),
            files: None,
            category: "TV > HD".into(),
            category_ids: vec![5000, 5040],
            password: Some(0),
            indexer_id: indexer.into(),
            indexer_name: format!("Idx {}", indexer),
            published: 0,
            guid: String::new(),
        }
    }

    #[test]
    fn dates_round_trip() {
        for ts in [60u64, 1_740_787_200, 1_790_000_000, 951_782_400] {
            assert_eq!(indexer::parse_rfc2822(&rfc2822(ts)), Some(ts), "{}", rfc2822(ts));
        }
        assert_eq!(rfc2822(1_740_787_200), "Sat, 01 Mar 2025 00:00:00 +0000");
    }

    #[test]
    fn requests() {
        let r = parse_request("GET /api?t=tvsearch&q=The+Show%20S01&apikey=abc&cat=5000,5040 HTTP/1.1\r\nHost: 192.168.1.5:5077\r\nX-Api-Key: zz").unwrap();
        assert_eq!(r.method, "GET");
        assert_eq!(r.path, "/api");
        assert_eq!(r.param("q"), Some("The Show S01"));
        assert_eq!(r.param("CAT"), Some("5000,5040"));
        assert_eq!(r.header("host"), Some("192.168.1.5:5077"));
        assert_eq!(r.header("x-api-key"), Some("zz"));
        assert!(parse_request("").is_none());
    }

    #[test]
    fn keys() {
        assert!(same_key("abc123", "abc123"));
        assert!(!same_key("abc124", "abc123"));
        assert!(!same_key("abc", "abc123"));
    }

    #[test]
    fn dedup_like_the_app() {
        assert_eq!(normalize_title("Some.Show.S01E01.1080p-GRP-Obfuscated"), normalize_title("Some Show S01E01 1080p GRP"));
        let list = vec![
            item("Some.Show.S01E01.1080p-GRP", 1000, "a", "https://a/1"),
            item("Some.Show.S01E01.1080p-GRP-xpost", 1010, "b", "https://b/1"),
            item("Some.Show.S01E01.1080p-GRP", 3000, "b", "https://b/2"),
            item("Other", 1000, "a", "https://a/2"),
        ];
        let out = dedupe(list);
        assert_eq!(out.iter().map(|i| i.link.as_str()).collect::<Vec<_>>(), ["https://a/1", "https://b/2", "https://a/2"]);
    }

    #[test]
    fn feed_is_valid_newznab() {
        let items = vec![item("A & B <x>", 1_500_000_000, "i1", "https://idx.example/getnzb/1?r=SECRET")];
        let xml = rss(&items, 0, 1, "http://127.0.0.1:5077", "k");
        assert!(!xml.contains("SECRET"), "la API key del indexador no debe salir");
        let (parsed, total) = indexer::parse_rss(&xml, "nebula", "Nebula").unwrap();
        assert_eq!(total, Some(1));
        let it = &parsed[0];
        assert_eq!(it.title, "A & B <x>");
        assert_eq!(it.size, 1_500_000_000);
        assert_eq!(it.category_ids, vec![5000, 5040]);
        assert_eq!(it.date, 1_790_000_000);
        let id = it.link.split("id=").nth(1).unwrap().split('&').next().unwrap();
        assert_eq!(LINKS.lock().get(id).unwrap().url, "https://idx.example/getnzb/1?r=SECRET");
        assert!(it.link.starts_with("http://127.0.0.1:5077/api?t=get&id="));
        assert!(it.link.ends_with("&apikey=k"));
        assert!(roxmltree::Document::parse(&caps()).is_ok());
    }

    #[test]
    fn indexer_filter() {
        let a = IndexerConfig { id: "a1".into(), name: "NZBGeek".into(), url: "https://g".into(), ..Default::default() };
        let b = IndexerConfig { id: "b1".into(), name: "Finder".into(), url: "https://f".into(), ..Default::default() };
        let c = IndexerConfig { id: "c1".into(), name: "Off".into(), url: "https://o".into(), enabled: false, ..Default::default() };
        let all = vec![a, b, c];
        assert_eq!(pick_indexers(&all, None).len(), 2);
        assert_eq!(pick_indexers(&all, Some("nzbgeek")).len(), 1);
        assert_eq!(pick_indexers(&all, Some("b1, off")).len(), 1);
    }

    /// Lo que hace el servidor con una petición HTTP completa (cabecera + cuerpo), con un motor simulado.
    fn sab_raw(b: &crate::sab::tests::Fake, raw: &[u8]) -> serde_json::Value {
        let end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let mut req = parse_request(&String::from_utf8_lossy(&raw[..end])).unwrap();
        let body = &raw[end + 4..];
        let body = if req.header("Transfer-Encoding").is_some() { dechunk(body).unwrap() } else { body.to_vec() };
        parse_body(&mut req, &body);
        let path = req.path.trim_end_matches('/').to_ascii_lowercase();
        assert!(path == "/api" || path == "/sabnzbd/api");
        assert!(req.param("mode").is_some());
        let call = crate::sab::Call { params: &req.query, files: &req.files };
        crate::rt::block_on(crate::sab::handle(b, &call)).1
    }

    fn get(b: &crate::sab::tests::Fake, q: &str) -> serde_json::Value {
        sab_raw(b, format!("GET /api?{} HTTP/1.1\r\nHost: localhost:5077\r\n\r\n", q).as_bytes())
    }

    /// Como lo sube Sonarr: POST multipart con el NZB en el campo `name` y el resto en la URL.
    fn sonarr_addfile(b: &crate::sab::tests::Fake, cat: &str, nzb_name: &str) -> serde_json::Value {
        let boundary = "----NzbDroneBoundary1234";
        let mut body = Vec::new();
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"name\"; filename=\"{nzb_name}\"\r\nContent-Type: application/x-nzb\r\n\r\n").as_bytes());
        body.extend_from_slice(crate::sab::tests::NZB.as_bytes());
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let head = format!(
            "POST /sabnzbd/api?mode=addfile&cat={cat}&priority=-100&apikey=secretkey&output=json HTTP/1.1\r\nHost: localhost:5077\r\nContent-Type: multipart/form-data; boundary=\"{boundary}\"\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        let mut raw = head.into_bytes();
        raw.extend_from_slice(&body);
        sab_raw(b, &raw)
    }

    /// La secuencia de Sonarr/Radarr: probar la conexión, mandar un NZB, seguirlo en la cola y
    /// encontrarlo terminado en el historial para importarlo.
    #[test]
    fn sonarr_sequence() {
        use crate::engine::{HistoryEntry, JobStatus};
        let b = crate::sab::tests::Fake::new();
        // 1. Test: versión, configuración (carpetas y categorías) y categorías.
        let v = get(&b, "mode=version&apikey=secretkey&output=json");
        assert_eq!(v["version"], crate::sab::SAB_VERSION);
        let c = get(&b, "mode=get_config&apikey=secretkey&output=json");
        assert_eq!(c["config"]["misc"]["complete_dir"], "/dl");
        assert!(c["config"]["categories"].as_array().unwrap().iter().any(|x| x["name"] == "tv"));
        // 2. Grab: addfile multipart.
        let a = sonarr_addfile(&b, "tv", "Some.Show.S01E01.1080p.nzb");
        assert_eq!(a["status"], true);
        let nzo = a["nzo_ids"][0].as_str().unwrap().to_string();
        assert!(nzo.starts_with("SABnzbd_nzo_"));
        let (_, opts) = b.added.lock()[0].clone();
        assert_eq!(opts.category.as_deref(), Some("tv"));
        assert_eq!(opts.priority, None, "-100 = la prioridad de la categoría");
        // 3. Cola: lo ve descargando con su categoría.
        b.jobs.lock()[0].status = JobStatus::Downloading;
        let q = get(&b, "mode=queue&start=0&limit=0&apikey=secretkey&output=json");
        let slot = &q["queue"]["slots"][0];
        assert_eq!(slot["nzo_id"], nzo.as_str());
        assert_eq!(slot["filename"], "Some.Show.S01E01.1080p");
        assert_eq!(slot["cat"], "tv");
        assert_eq!(slot["status"], "Downloading");
        // 4. Post-procesado: sale de la cola y aparece en el historial como Extracting.
        b.jobs.lock()[0].status = JobStatus::Extracting;
        assert_eq!(get(&b, "mode=queue&apikey=secretkey")["queue"]["slots"].as_array().unwrap().len(), 0);
        let h = get(&b, "mode=history&start=0&limit=60&category=tv&apikey=secretkey&output=json");
        assert_eq!(h["history"]["slots"][0]["status"], "Extracting");
        // 5. Terminado: Completed con la ruta final para importar.
        let id = crate::sab::job_id(&nzo).to_string();
        b.jobs.lock().clear();
        b.hist.lock().push(HistoryEntry {
            id: id.clone(),
            name: "Some.Show.S01E01.1080p".into(),
            status: "completed".into(),
            size: 1_500_000_000,
            dest: "/dl/tv/Some.Show.S01E01.1080p".into(),
            category: "tv".into(),
            completed_at: 1_790_000_000,
            download_secs: 95,
            ..Default::default()
        });
        let h = get(&b, "mode=history&start=0&limit=60&category=tv&apikey=secretkey&output=json");
        let s = &h["history"]["slots"][0];
        assert_eq!(s["nzo_id"], nzo.as_str());
        assert_eq!(s["status"], "Completed");
        assert_eq!(s["storage"], "/dl/tv/Some.Show.S01E01.1080p");
        assert_eq!(s["category"], "tv");
        assert_eq!(s["bytes"], 1_500_000_000u64);
        // 6. Tras importar, Sonarr lo quita del historial.
        get(&b, &format!("mode=history&name=delete&value={}&del_files=0&apikey=secretkey", nzo));
        assert!(b.hist.lock().is_empty());
    }

    #[test]
    fn form_bodies() {
        let b = crate::sab::tests::Fake::new();
        // Campos en el formulario (no en la URL) y archivo en el campo nzbfile.
        let boundary = "xyz";
        let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"apikey\"\r\n\r\nsecretkey\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"mode\"\r\n\r\naddfile\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"nzbname\"\r\n\r\nMi Película (2024)\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"nzbfile\"; filename=\"x.nzb\"\r\n\r\n").into_bytes();
        body.extend_from_slice(crate::sab::tests::NZB.as_bytes());
        body.extend_from_slice(format!("\r\n--{boundary}--").as_bytes());
        let mut raw = format!("POST /api HTTP/1.1\r\nContent-Type: multipart/form-data; boundary={boundary}\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
        raw.extend_from_slice(&body);
        let r = sab_raw(&b, &raw);
        assert_eq!(r["status"], true, "{r}");
        assert_eq!(b.added.lock()[0].1.name.as_deref(), Some("Mi Película (2024)"));
        // urlencoded + chunked.
        let form = "mode=addurl&name=https%3A%2F%2Fidx.example%2Fget%2F1&cat=movies&priority=2&apikey=secretkey";
        let raw = format!("POST /api HTTP/1.1\r\nContent-Type: application/x-www-form-urlencoded\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n", form.len(), form);
        let r = sab_raw(&b, raw.as_bytes());
        assert_eq!(r["status"], true, "{r}");
        let (_, o) = b.added.lock()[1].clone();
        assert_eq!((o.category.as_deref(), o.priority), (Some("movies"), Some(2)));
        let r = get(&b, "mode=addurl&name=https://bad.example/x&apikey=secretkey");
        assert_eq!(r["status"], false);
    }

    #[test]
    fn multipart_parsing() {
        let body = b"--b\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1\r\n--b\r\nContent-Disposition: form-data; name=\"f\"; filename*=UTF-8''Pel%C3%ADcula.nzb\r\nContent-Type: x\r\n\r\n\r\nline\r\n\r\n--b--\r\n";
        let parts = multipart(body, "b");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], ("a".into(), None, b"1".to_vec()));
        assert_eq!(parts[1].1.as_deref(), Some("Película.nzb"));
        assert_eq!(parts[1].2, b"\r\nline\r\n");
        assert_eq!(dechunk(b"3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n").unwrap(), b"abcde");
        assert!(dechunk(b"3\r\nab").is_none());
    }
}
