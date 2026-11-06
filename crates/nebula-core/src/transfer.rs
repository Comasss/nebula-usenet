//! Pasar los ajustes a otro dispositivo (del PC al móvil y al revés) escaneando un QR con el móvil.
//! - Enviar: el QR lleva el código de ajustes (JSON comprimido) y el otro dispositivo lo importa.
//! - Recibir: el QR lleva la dirección de este equipo en la red local y una clave de un solo uso.
//!   El móvil lo escanea y manda sus ajustes cifrados con esa clave; aquí se piden confirmar.
//! Las rutas de las carpetas y las opciones propias de cada sistema no viajan (tampoco la
//! carpeta de destino de las reglas RSS, aunque sí los feeds y las reglas).
use crate::config::Settings;
use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::LazyLock;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Código con los ajustes.
const PREFIX: &str = "NEBULA1:";
/// Código para enviar los ajustes a un equipo que los espera: `NEBULARX1:<ip:puerto>/<clave>`.
const RX_PREFIX: &str = "NEBULARX1:";
const RX_PATH: &str = "/nebula/transfer";
/// Tiempo que un equipo espera los ajustes antes de cerrar el puerto.
const RX_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_BODY: usize = 256 * 1024;
const MAX_JSON: u64 = 1024 * 1024;

/// Ajustes que dependen del dispositivo: se quedan como están en el destino.
const LOCAL_KEYS: &[&str] = &[
    "downloadDir",
    "incompleteDir",
    "nzbSaveDir",
    "minimizeToTray",
    "closeToTray",
    "startMinimized",
    "startWithWindows",
    "associateNzb",
    "clipboardShowWindow",
    "haptics",
    "firstRunDone",
    "featuresChosen",
    "updateSkipped",
    "apiKeyEnc",
    "remoteEnabled",
    "remotePort",
    "par2Path",
    "sevenZipPath",
];
/// Campos que solo existen en disco (claves cifradas).
const DISK_KEYS: &[&str] = &["passwordEnc", "apiKeyEnc", "urlEnc", "tokenEnc"];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QrCode {
    pub code: String,
    pub svg: String,
}

/// Lo que contiene un código escaneado o pegado.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Preview {
    /// Ajustes para importar.
    #[serde(rename_all = "camelCase")]
    Settings { from: String, servers: usize, indexers: usize, debrid: usize, rss: usize, api: bool },
    /// Un equipo que espera nuestros ajustes.
    #[serde(rename_all = "camelCase")]
    Receiver { address: String },
}

fn platform() -> &'static str {
    if cfg!(target_os = "android") {
        "android"
    } else {
        "windows"
    }
}

/// Quita de `obj` lo que coincide con `def` (salvo el id) y los campos que solo van a disco.
fn strip(obj: &mut Map<String, Value>, def: &Value) {
    obj.retain(|k, v| !DISK_KEYS.contains(&k.as_str()) && (k == "id" || def.get(k) != Some(v)));
}

fn strip_list(obj: &mut Map<String, Value>, key: &str, def: Value) {
    if let Some(Value::Array(items)) = obj.get_mut(key) {
        for it in items {
            if let Value::Object(m) = it {
                strip(m, &def);
            }
        }
    }
}

/// Código con los ajustes que se pueden pasar a otro dispositivo.
pub fn export(s: &Settings) -> Result<String, String> {
    let def = serde_json::to_value(Settings::default()).map_err(|e| e.to_string())?;
    let Value::Object(mut obj) = serde_json::to_value(s).map_err(|e| e.to_string())? else {
        return Err(t!("transfer.invalid"));
    };
    obj.retain(|k, v| !LOCAL_KEYS.contains(&k.as_str()) && def.get(k) != Some(v));
    strip_list(&mut obj, "servers", serde_json::to_value(crate::config::ServerConfig::default()).unwrap_or_default());
    strip_list(&mut obj, "indexers", serde_json::to_value(crate::config::IndexerConfig::default()).unwrap_or_default());
    strip_list(&mut obj, "debrid", serde_json::to_value(crate::config::DebridConfig::default()).unwrap_or_default());
    strip_list(&mut obj, "rssFeeds", serde_json::to_value(crate::config::RssFeed::default()).unwrap_or_default());
    strip_list(&mut obj, "rssRules", serde_json::to_value(crate::config::RssRule::default()).unwrap_or_default());
    strip_list(&mut obj, "categories", serde_json::to_value(crate::categories::Category::default()).unwrap_or_default());
    if let Some(Value::Array(rules)) = obj.get_mut("rssRules") {
        for r in rules {
            if let Value::Object(m) = r {
                m.remove("destDir");
            }
        }
    }
    // De las acciones al terminar solo viajan las notificaciones: los scripts y la energía son de este equipo.
    let notify: Vec<_> = s.finish_actions.iter().filter(|a| a.is_notification()).collect();
    if notify.is_empty() {
        obj.remove("finishActions");
    } else {
        obj.insert("finishActions".into(), serde_json::to_value(notify).map_err(|e| e.to_string())?);
        strip_list(&mut obj, "finishActions", serde_json::to_value(crate::config::FinishAction::default()).unwrap_or_default());
    }
    obj.insert("_from".into(), Value::String(platform().into()));
    let json = serde_json::to_vec(&obj).map_err(|e| e.to_string())?;
    let mut z = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(&json).map_err(|e| e.to_string())?;
    let packed = z.finish().map_err(|e| e.to_string())?;
    Ok(format!("{}{}", PREFIX, b64().encode(packed)))
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
}

fn parse(code: &str) -> Result<Map<String, Value>, String> {
    let body = code.trim().strip_prefix(PREFIX).ok_or_else(|| t!("transfer.invalid"))?;
    let packed = b64().decode(body.trim()).map_err(|_| t!("transfer.invalid"))?;
    let mut json = Vec::new();
    flate2::read::DeflateDecoder::new(&packed[..])
        .take(MAX_JSON)
        .read_to_end(&mut json)
        .map_err(|_| t!("transfer.invalid"))?;
    match serde_json::from_slice(&json) {
        Ok(Value::Object(m)) => Ok(m),
        _ => Err(t!("transfer.invalid")),
    }
}

fn count(m: &Map<String, Value>, key: &str) -> usize {
    m.get(key).and_then(Value::as_array).map_or(0, Vec::len)
}

pub fn preview(code: &str) -> Result<Preview, String> {
    let code = code.trim();
    if code.starts_with(RX_PREFIX) {
        let (addr, _) = parse_rx(code)?;
        return Ok(Preview::Receiver { address: addr.to_string() });
    }
    let m = parse(code)?;
    Ok(Preview::Settings {
        from: m.get("_from").and_then(Value::as_str).unwrap_or("").to_string(),
        servers: count(&m, "servers"),
        indexers: count(&m, "indexers"),
        debrid: count(&m, "debrid"),
        rss: count(&m, "rssRules"),
        api: m.get("apiEnabled").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// Ajustes de `current` con los del código: lo que no viene en el código vuelve a su valor por
/// defecto (el código no incluye lo que ya lo está) y lo propio del dispositivo se mantiene.
pub fn merge(current: &Settings, code: &str) -> Result<Settings, String> {
    let incoming = parse(code)?;
    let def = serde_json::to_value(Settings::default()).map_err(|e| e.to_string())?;
    let Value::Object(mut cur) = serde_json::to_value(current).map_err(|e| e.to_string())? else {
        return Err(t!("transfer.invalid"));
    };
    if let Value::Object(def) = def {
        for (k, dv) in def {
            if LOCAL_KEYS.contains(&k.as_str()) {
                continue;
            }
            let v = incoming.get(&k).cloned().unwrap_or(dv);
            cur.insert(k, v);
        }
    }
    let mut s: Settings = serde_json::from_value(Value::Object(cur)).map_err(|_| t!("transfer.invalid"))?;
    // Las reglas RSS que ya existían aquí conservan su carpeta de destino.
    for r in &mut s.rss_rules {
        r.dest_dir = current.rss_rules.iter().find(|x| x.id == r.id).map(|x| x.dest_dir.clone()).unwrap_or_default();
    }
    // Las categorías que ya existían aquí conservan su carpeta; las nuevas solo traen una relativa
    // (dentro de la carpeta de descargas de este dispositivo).
    for c in &mut s.categories {
        c.dir = match current.categories.iter().find(|x| x.name.eq_ignore_ascii_case(&c.name)) {
            Some(x) => x.dir.clone(),
            None if std::path::Path::new(&c.dir).is_absolute() || crate::categories::is_windows_absolute(&c.dir) => String::new(),
            None => c.dir.clone(),
        };
    }
    // Las notificaciones llegan del código; los scripts y la energía de este equipo se quedan.
    s.finish_actions.retain(|a| a.is_notification());
    s.finish_actions.extend(current.finish_actions.iter().filter(|a| !a.is_notification()).cloned());
    s.first_run_done = true;
    s.features_chosen = true;
    if !s.feature_nntp && !s.feature_indexers {
        s.feature_nntp = true;
    }
    Ok(s)
}

/// QR en SVG. Con nivel de corrección bajo caben unos 2,9 KB: de sobra en una pantalla.
pub fn qr_svg(data: &str) -> Result<String, String> {
    use qrcode::render::svg;
    let qr = qrcode::QrCode::with_error_correction_level(data.as_bytes(), qrcode::EcLevel::L).map_err(|_| t!("transfer.tooBig"))?;
    Ok(qr
        .render::<svg::Color>()
        .min_dimensions(256, 256)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .quiet_zone(true)
        .build())
}

pub fn export_qr(s: &Settings) -> Result<QrCode, String> {
    let code = export(s)?;
    // Si no cabe en un QR, la interfaz ofrece copiar el código igualmente.
    let svg = qr_svg(&code).unwrap_or_default();
    Ok(QrCode { code, svg })
}

// ---- Recibir por la red local ----

struct Receiving {
    stop: tokio::sync::oneshot::Sender<()>,
}

static RX: LazyLock<Mutex<Option<Receiving>>> = LazyLock::new(|| Mutex::new(None));

fn random_key() -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(uuid::Uuid::new_v4().as_bytes());
    h.update(uuid::Uuid::new_v4().as_bytes());
    h.finalize().into()
}

fn parse_rx(code: &str) -> Result<(SocketAddr, [u8; 32]), String> {
    let rest = code.trim().strip_prefix(RX_PREFIX).ok_or_else(|| t!("transfer.invalid"))?;
    let (addr, key) = rest.rsplit_once('/').ok_or_else(|| t!("transfer.invalid"))?;
    let addr: SocketAddr = addr.parse().map_err(|_| t!("transfer.invalid"))?;
    let key: [u8; 32] = b64().decode(key).ok().and_then(|k| k.try_into().ok()).ok_or_else(|| t!("transfer.invalid"))?;
    Ok((addr, key))
}

fn seal(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>, String> {
    let id = uuid::Uuid::new_v4();
    let nonce = &id.as_bytes()[..12];
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut out = nonce.to_vec();
    out.extend(cipher.encrypt(Nonce::from_slice(nonce), plain).map_err(|_| t!("transfer.invalid"))?);
    Ok(out)
}

fn open(key: &[u8; 32], data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 12 {
        return None;
    }
    let (nonce, ct) = data.split_at(12);
    ChaCha20Poly1305::new(Key::from_slice(key)).decrypt(Nonce::from_slice(nonce), ct).ok()
}

pub fn stop_receive() {
    if let Some(r) = RX.lock().take() {
        let _ = r.stop.send(());
    }
}

/// Abre un puerto en la red local para recibir los ajustes de otro dispositivo y devuelve su QR.
pub async fn start_receive() -> Result<QrCode, String> {
    stop_receive();
    let ip = crate::api::lan_ip().ok_or_else(|| t!("transfer.noLan"))?;
    let listener = TcpListener::bind(("0.0.0.0", 0)).await.map_err(|err| t!("transfer.bindFailed", error = err))?;
    let port = listener.local_addr().map_err(|err| t!("transfer.bindFailed", error = err))?.port();
    let key = random_key();
    let code = format!("{}{}/{}", RX_PREFIX, SocketAddr::new(ip, port), b64().encode(key));
    let svg = qr_svg(&code)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    *RX.lock() = Some(Receiving { stop: tx });
    crate::rt::spawn(serve(listener, key, rx));
    Ok(QrCode { code, svg })
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Received {
    code: String,
    preview: Preview,
}

async fn serve(listener: TcpListener, key: [u8; 32], mut stop: tokio::sync::oneshot::Receiver<()>) {
    let deadline = tokio::time::sleep(RX_TIMEOUT);
    tokio::pin!(deadline);
    let mut failures = 0;
    loop {
        tokio::select! {
            _ = &mut stop => return,
            _ = &mut deadline => break,
            r = listener.accept() => {
                let Ok((sock, _)) = r else { continue };
                match tokio::time::timeout(Duration::from_secs(20), handle(sock, &key)).await {
                    Ok(Some(code)) => {
                        if let Ok(preview) = preview(&code) {
                            crate::bus::emit(crate::bus::Event::TransferReceived(Received { code, preview }));
                            break;
                        }
                    }
                    _ => {
                        // Muchos intentos con otra clave: se deja de escuchar.
                        failures += 1;
                        if failures >= 5 {
                            break;
                        }
                    }
                }
            }
        }
    }
    RX.lock().take();
    crate::bus::emit(crate::bus::Event::TransferClosed);
}

/// Lee una petición POST con el cuerpo cifrado; devuelve el código de ajustes si la clave es la buena.
async fn handle(mut sock: TcpStream, key: &[u8; 32]) -> Option<String> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > 16 * 1024 {
            return None;
        }
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or("");
    let ok_target = first.starts_with("POST ") && first.split_whitespace().nth(1) == Some(RX_PATH);
    let len: usize = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0);
    if !ok_target || len == 0 || len > MAX_BODY {
        let _ = reply(&mut sock, 400, "Bad Request").await;
        return None;
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    match open(key, &body).and_then(|p| String::from_utf8(p).ok()) {
        Some(code) => {
            let _ = reply(&mut sock, 200, "OK").await;
            Some(code)
        }
        None => {
            let _ = reply(&mut sock, 403, "Forbidden").await;
            None
        }
    }
}

async fn reply(sock: &mut TcpStream, status: u16, text: &str) -> std::io::Result<()> {
    let out = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        text,
        text.len(),
        text
    );
    sock.write_all(out.as_bytes()).await?;
    sock.shutdown().await
}

/// Envía nuestros ajustes al equipo del QR `target` (`NEBULARX1:…`).
pub async fn send(s: &Settings, target: &str) -> Result<(), String> {
    let (addr, key) = parse_rx(target)?;
    let body = seal(&key, export(s)?.as_bytes())?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(8))
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())?;
    let r = client
        .post(format!("http://{}{}", addr, RX_PATH))
        .header("Content-Type", "application/octet-stream")
        .body(body)
        .send()
        .await
        .map_err(|_| t!("transfer.unreachable", address = addr))?;
    match r.status().as_u16() {
        200 => Ok(()),
        403 => Err(t!("transfer.rejected")),
        _ => Err(t!("transfer.unreachable", address = addr)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DebridConfig, FinishAction, IndexerConfig, RssFeed, RssRule, ServerConfig};

    fn sample() -> Settings {
        let mut s = Settings::default();
        s.feature_indexers = true;
        s.download_dir = "C:\\Users\\yo\\Downloads\\Nebula".into();
        s.start_with_windows = true;
        s.api_enabled = true;
        s.api_key = "0123456789abcdef0123456789abcdef".into();
        s.language = "ca".into();
        for i in 0..3 {
            s.servers.push(ServerConfig {
                name: format!("Servidor {}", i),
                host: format!("news{}.proveedor-usenet.com", i),
                username: "usuario_largo_123".into(),
                password: "contraseña-muy-segura-123".into(),
                connections: 50,
                priority: i,
                ..Default::default()
            });
        }
        for i in 0..6 {
            s.indexers.push(IndexerConfig {
                name: format!("Indexador {}", i),
                url: format!("https://api.indexador{}.info", i),
                api_key: uuid::Uuid::new_v4().simple().to_string(),
                ..Default::default()
            });
        }
        s.debrid.push(DebridConfig { api_key: uuid::Uuid::new_v4().to_string(), ..Default::default() });
        s.debrid.push(DebridConfig { kind: "premiumize".into(), api_key: uuid::Uuid::new_v4().simple().to_string(), ..Default::default() });
        s.rss_feeds.push(RssFeed { name: "Feed suelto".into(), url: "https://feed.example/rss?key=abc".into(), interval_min: 30, ..Default::default() });
        s.rss_rules.push(RssRule {
            id: "regla1".into(),
            name: "Mi serie".into(),
            must: "some show 1080p".into(),
            exclude: "hevc".into(),
            max_size_mb: 4000,
            dest_dir: "C:\\Series".into(),
            created_at: 1_790_000_000,
            ..Default::default()
        });
        s.categories.push(crate::categories::Category { name: "tv".into(), dir: "Series".into(), newznab: "5000-5999".into(), ..Default::default() });
        s.categories.push(crate::categories::Category { name: "movies".into(), dir: "C:\\Pelis".into(), pp: 3, ..Default::default() });
        s.finish_actions.push(FinishAction { kind: "telegram".into(), token: "123:abc".into(), chat_id: "42".into(), ..Default::default() });
        s.finish_actions.push(FinishAction { kind: "script".into(), path: "C:\\scripts\\fin.bat".into(), ..Default::default() });
        s.finish_actions.push(FinishAction { kind: "power".into(), on_queue_empty: true, ..Default::default() });
        s
    }

    #[test]
    fn scripts_and_power_stay_on_the_device() {
        let code = export(&sample()).unwrap();
        let mut dst = Settings::default();
        dst.finish_actions.push(FinishAction { kind: "script".into(), path: "/bin/true".into(), ..Default::default() });
        dst.finish_actions.push(FinishAction { kind: "discord".into(), url: "https://viejo".into(), ..Default::default() });
        let out = merge(&dst, &code).unwrap();
        let kinds: Vec<&str> = out.finish_actions.iter().map(|a| a.kind.as_str()).collect();
        assert_eq!(kinds, ["telegram", "script"]);
        assert_eq!(out.finish_actions[1].path, "/bin/true");
    }

    #[test]
    fn round_trip_keeps_local_settings() {
        let src = sample();
        let code = export(&src).unwrap();
        // Con 3 servidores, 6 indexadores y 2 debrid tiene que caber de sobra en un QR.
        eprintln!("código de ejemplo: {} bytes", code.len());
        assert!(code.len() < 2000, "código de {} bytes", code.len());
        assert!(!qr_svg(&code).unwrap().is_empty());

        let mut dst = Settings::default();
        dst.download_dir = "/storage/emulated/0/Download/Nebula".into();
        dst.haptics = false;
        dst.search_limit = 42;
        let out = merge(&dst, &code).unwrap();
        assert_eq!(out.download_dir, dst.download_dir);
        assert!(!out.haptics);
        assert!(!out.start_with_windows);
        assert_eq!(out.search_limit, Settings::default().search_limit);
        assert_eq!(out.language, "ca");
        assert!(out.api_enabled && out.first_run_done && out.features_chosen);
        assert_eq!(out.api_key, src.api_key);
        assert_eq!(serde_json::to_value(&out.servers).unwrap(), serde_json::to_value(&src.servers).unwrap());
        assert_eq!(serde_json::to_value(&out.indexers).unwrap(), serde_json::to_value(&src.indexers).unwrap());
        assert_eq!(serde_json::to_value(&out.debrid).unwrap(), serde_json::to_value(&src.debrid).unwrap());
        assert_eq!(serde_json::to_value(&out.rss_feeds).unwrap(), serde_json::to_value(&src.rss_feeds).unwrap());
        // Las reglas viajan, pero sin la carpeta de destino.
        assert_eq!(out.rss_rules.len(), 1);
        assert_eq!(out.rss_rules[0].must, "some show 1080p");
        assert_eq!(out.rss_rules[0].created_at, 1_790_000_000);
        assert_eq!(out.rss_rules[0].dest_dir, "");
        assert!(parse(&code).unwrap()["rssRules"][0].get("destDir").is_none());
        // Si la regla ya existía en el destino, conserva su carpeta.
        let mut dst2 = Settings::default();
        dst2.rss_rules.push(RssRule { id: "regla1".into(), dest_dir: "/storage/emulated/0/Series".into(), ..Default::default() });
        let out2 = merge(&dst2, &code).unwrap();
        assert_eq!(out2.rss_rules[0].dest_dir, "/storage/emulated/0/Series");
        assert_eq!(out2.rss_rules[0].name, "Mi serie");
        // Las categorías viajan; las carpetas absolutas no, las relativas sí, y las que ya
        // existían aquí conservan la suya.
        let names: Vec<&str> = out.categories.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["tv", "movies"]);
        assert_eq!(out.categories[0].dir, "Series");
        assert_eq!(out.categories[0].newznab, "5000-5999");
        assert_eq!(out.categories[1].dir, "");
        let mut dst3 = Settings::default();
        dst3.categories.push(crate::categories::Category { name: "Movies".into(), dir: "/mnt/pelis".into(), ..Default::default() });
        assert_eq!(merge(&dst3, &code).unwrap().categories[1].dir, "/mnt/pelis");
        assert_eq!(out.finish_actions.len(), 1);
        assert_eq!(out.finish_actions[0].kind, "telegram");
        assert_eq!(out.finish_actions[0].token, "123:abc");
        assert_eq!(
            preview(&code).unwrap(),
            Preview::Settings { from: platform().into(), servers: 3, indexers: 6, debrid: 2, rss: 1, api: true }
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(preview("hola").is_err());
        assert!(preview("NEBULA1:!!!").is_err());
        assert!(preview("NEBULARX1:no-es-una-ip/abc").is_err());
    }

    #[test]
    fn receiver_code_and_encryption() {
        let key = random_key();
        let code = format!("{}{}/{}", RX_PREFIX, "192.168.1.20:50123", b64().encode(key));
        assert_eq!(preview(&code).unwrap(), Preview::Receiver { address: "192.168.1.20:50123".into() });
        let (addr, k) = parse_rx(&code).unwrap();
        assert_eq!(addr.port(), 50123);
        let sealed = seal(&k, b"NEBULA1:abc").unwrap();
        assert_eq!(open(&key, &sealed).unwrap(), b"NEBULA1:abc");
        assert!(open(&random_key(), &sealed).is_none());
    }

    #[tokio::test]
    async fn send_over_local_network() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let key = random_key();
        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            handle(sock, &key).await
        });
        let src = sample();
        let target = format!("{}{}/{}", RX_PREFIX, addr, b64().encode(key));
        send(&src, &target).await.unwrap();
        let code = server.await.unwrap().expect("ajustes recibidos");
        let out = merge(&Settings::default(), &code).unwrap();
        assert_eq!(out.servers.len(), 3);
        assert_eq!(out.servers[0].password, src.servers[0].password);

        // Con otra clave se rechaza.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            handle(sock, &random_key()).await
        });
        let target = format!("{}{}/{}", RX_PREFIX, addr, b64().encode(key));
        assert!(send(&src, &target).await.is_err());
        assert!(server.await.unwrap().is_none());
    }
}
