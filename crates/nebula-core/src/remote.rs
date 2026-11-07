//! Control remoto: la app de Android maneja el Nebula de un PC por la red local (o por Tailscale).
//!
//! - Descubrimiento: el PC responde a una sonda UDP en el puerto 5078 con su nombre, versión y
//!   puerto. El móvil la envía por difusión y lista los PCs que contestan.
//! - Emparejamiento: intercambio X25519. Los dos calculan un código de 6 dígitos a partir de la
//!   clave compartida y de las dos claves públicas; el PC lo muestra y pide aprobar, el móvil
//!   enseña el mismo código para comprobarlo. Al aceptar, el PC genera un token y lo manda cifrado.
//! - Sesión: TCP con mensajes de longitud fija. Tras un saludo con nonces de los dos lados, todo va
//!   cifrado con ChaCha20-Poly1305 y claves derivadas del token (una por sentido, nonce = contador).
//!   El móvil pide cosas (`{"id", "op", "args"}`) y el PC empuja la cola y el historial en directo.
//! - El móvil solo puede ver la cola, el historial y las estadísticas, añadir NZB, pausar,
//!   reanudar, mover y borrar trabajos y cambiar el límite de velocidad. Nada de ajustes.
use crate::engine::{AddRequest, Engine};
use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{broadcast, mpsc, oneshot, watch, Notify};
use x25519_dalek::{PublicKey, StaticSecret};

/// Puerto TCP por defecto y puerto UDP (fijo) de la sonda de descubrimiento.
pub const DEFAULT_PORT: u16 = 5078;
pub const DISCOVERY_PORT: u16 = 5078;
const PROBE: &[u8] = b"NEBULA-DISCOVER/1";
const PROTOCOL: u32 = 1;
/// Tamaño máximo de un mensaje antes de autenticarse y después (un NZB grande cabe de sobra).
const MAX_HELLO: usize = 8 * 1024;
const MAX_FRAME: usize = 96 * 1024 * 1024;
const PAIR_WAIT: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(6);

/// Lo que el móvil puede pedir. Todo lo demás se rechaza.
const OPS: &[&str] = &[
    "ping",
    "get_queue",
    "get_history",
    "get_stats",
    "get_indexer_stats",
    "get_job_files",
    "pause_job",
    "resume_job",
    "move_job",
    "remove_job",
    "set_paused",
    "set_speed_limit",
    "remove_history",
    "clear_history",
    "add_url",
    "add_nzb",
    "set_job_priority",
    "set_job_category",
    "get_vias",
];

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn random32() -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(uuid::Uuid::new_v4().as_bytes());
    h.update(uuid::Uuid::new_v4().as_bytes());
    h.update(uuid::Uuid::new_v4().as_bytes());
    h.finalize().into()
}

fn sha(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update((p.len() as u32).to_be_bytes());
        h.update(p);
    }
    h.finalize().into()
}

/// Nombre de este equipo (el que verá el móvil).
pub fn host_name() -> String {
    #[cfg(windows)]
    let name = std::env::var("COMPUTERNAME").unwrap_or_default();
    #[cfg(unix)]
    let name = {
        let mut buf = [0u8; 256];
        let r = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
        if r == 0 {
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            String::from_utf8_lossy(&buf[..end]).to_string()
        } else {
            String::new()
        }
    };
    #[cfg(not(any(windows, unix)))]
    let name = String::new();
    let name = name.trim().to_string();
    if name.is_empty() {
        "Nebula".into()
    } else {
        name
    }
}

// ---- Datos guardados: móviles emparejados (en el PC) y PCs emparejados (en el móvil) ----

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
struct Device {
    id: String,
    name: String,
    /// Token en base64. En disco va protegido como las contraseñas (DPAPI en Windows).
    token: String,
    paired_at: u64,
    last_seen: u64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
struct Pc {
    id: String,
    name: String,
    addr: String,
    token: String,
    paired_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
struct Store {
    /// Identificador de este dispositivo (estable).
    id: String,
    devices: Vec<Device>,
    pcs: Vec<Pc>,
}

static STORE: LazyLock<Mutex<(Option<PathBuf>, Store)>> = LazyLock::new(|| Mutex::new((None, Store::default())));

/// Carga los emparejamientos de `data/remote.json`.
pub fn init(data: &std::path::Path) {
    let path = data.join("remote.json");
    let mut s: Store = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    for d in &mut s.devices {
        d.token = crate::secrets::unprotect(&d.token).unwrap_or_default();
    }
    for p in &mut s.pcs {
        p.token = crate::secrets::unprotect(&p.token).unwrap_or_default();
    }
    s.devices.retain(|d| !d.token.is_empty());
    s.pcs.retain(|p| !p.token.is_empty());
    let fresh = s.id.is_empty();
    if fresh {
        s.id = crate::util::new_id();
    }
    *STORE.lock() = (Some(path), s);
    if fresh {
        save();
    }
}

fn save() {
    let (path, mut disk) = {
        let g = STORE.lock();
        (g.0.clone(), g.1.clone())
    };
    let Some(path) = path else { return };
    for d in &mut disk.devices {
        d.token = crate::secrets::protect(&d.token).unwrap_or_default();
    }
    for p in &mut disk.pcs {
        p.token = crate::secrets::protect(&p.token).unwrap_or_default();
    }
    if let Ok(json) = serde_json::to_string_pretty(&disk) {
        if let Err(e) = crate::config::write_atomic(&path, json.as_bytes()) {
            crate::lerror!("{}", e);
        }
    }
}

fn my_id() -> String {
    STORE.lock().1.id.clone()
}

fn token_of(b64token: &str) -> Option<[u8; 32]> {
    b64().decode(b64token).ok()?.try_into().ok()
}

// ---- Mensajes ----

async fn read_frame<R: AsyncRead + Unpin>(r: &mut R, max: usize) -> std::io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).await?;
    let n = u32::from_be_bytes(len) as usize;
    if n > max {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too big"));
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, data: &[u8]) -> std::io::Result<()> {
    w.write_all(&(data.len() as u32).to_be_bytes()).await?;
    w.write_all(data).await?;
    w.flush().await
}

async fn read_json<R: AsyncRead + Unpin>(r: &mut R) -> Result<Value, String> {
    let f = read_frame(r, MAX_HELLO).await.map_err(|e| e.to_string())?;
    serde_json::from_slice(&f).map_err(|_| t!("remote.protocol"))
}

async fn write_json<W: AsyncWrite + Unpin>(w: &mut W, v: &Value) -> Result<(), String> {
    write_frame(w, v.to_string().as_bytes()).await.map_err(|e| e.to_string())
}

/// Un sentido de la sesión cifrada. Cada mensaje usa el siguiente número como nonce, así que un
/// mensaje repetido, reordenado o de otra sesión no se puede descifrar.
struct Cipher {
    aead: ChaCha20Poly1305,
    ctr: u64,
}

impl Cipher {
    fn new(key: &[u8; 32]) -> Self {
        Self { aead: ChaCha20Poly1305::new(Key::from_slice(key)), ctr: 0 }
    }
    fn nonce(&mut self) -> [u8; 12] {
        let mut n = [0u8; 12];
        n[4..].copy_from_slice(&self.ctr.to_be_bytes());
        self.ctr += 1;
        n
    }
    fn seal(&mut self, plain: &[u8]) -> Vec<u8> {
        let n = self.nonce();
        self.aead.encrypt(Nonce::from_slice(&n), plain).unwrap_or_default()
    }
    fn open(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        let n = self.nonce();
        self.aead.decrypt(Nonce::from_slice(&n), data).ok()
    }
}

/// Claves de la sesión: (móvil → PC, PC → móvil).
fn session_keys(token: &[u8; 32], client_nonce: &[u8], server_nonce: &[u8]) -> ([u8; 32], [u8; 32]) {
    let base = sha(&[b"nebula-remote-v1", token, client_nonce, server_nonce]);
    (sha(&[&base, b"c2s"]), sha(&[&base, b"s2c"]))
}

/// Código de 6 dígitos que deben ver igual el PC y el móvil (si alguien se mete en medio, no coincide).
fn pair_code(shared: &[u8; 32], client_pub: &[u8; 32], server_pub: &[u8; 32]) -> String {
    let h = sha(&[b"nebula-pair-code", shared, client_pub, server_pub]);
    let n = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) % 1_000_000;
    format!("{:06}", n)
}

fn pair_box_key(shared: &[u8; 32]) -> [u8; 32] {
    sha(&[b"nebula-pair-key", shared])
}

fn decode32(v: &Value) -> Option<[u8; 32]> {
    b64().decode(v.as_str()?).ok()?.try_into().ok()
}

// ===========================================================================
// PC: servidor
// ===========================================================================

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    pub running: bool,
    pub error: Option<String>,
    /// Aviso que no impide funcionar (p. ej., no se pudo abrir el puerto de descubrimiento).
    pub warning: Option<String>,
    pub port: u16,
    pub name: String,
    /// Direcciones para añadir el PC a mano (IP:puerto).
    pub addresses: Vec<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    id: String,
    name: String,
    paired_at: u64,
    last_seen: u64,
    online: bool,
}

#[derive(Clone)]
enum Event {
    Queue(Arc<String>),
    History,
    Revoked(String),
}

static EVENTS: LazyLock<broadcast::Sender<Event>> = LazyLock::new(|| broadcast::channel(64).0);
static SERVER: LazyLock<tokio::sync::Mutex<Option<watch::Sender<bool>>>> = LazyLock::new(|| tokio::sync::Mutex::new(None));
static STATUS: LazyLock<Mutex<RemoteStatus>> = LazyLock::new(|| Mutex::new(RemoteStatus::default()));
static ONLINE: LazyLock<Mutex<HashMap<String, usize>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
/// Emparejamientos esperando la respuesta del usuario en el PC.
static PAIRING: LazyLock<Mutex<HashMap<String, oneshot::Sender<bool>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// El motor avisa de cada cambio de la cola; solo se serializa si hay móviles conectados.
pub fn publish_queue<T: Serialize>(snap: &T) {
    if EVENTS.receiver_count() == 0 {
        return;
    }
    if let Ok(s) = serde_json::to_string(snap) {
        let _ = EVENTS.send(Event::Queue(Arc::new(s)));
    }
}

pub fn publish_history() {
    if EVENTS.receiver_count() > 0 {
        let _ = EVENTS.send(Event::History);
    }
}

pub fn status() -> RemoteStatus {
    STATUS.lock().clone()
}

fn set_status(st: RemoteStatus) {
    *STATUS.lock() = st.clone();
    crate::bus::emit(crate::bus::Event::RemoteStatus(st));
}

pub fn devices() -> Vec<DeviceView> {
    let online = ONLINE.lock();
    let mut out: Vec<DeviceView> = STORE
        .lock()
        .1
        .devices
        .iter()
        .map(|d| DeviceView {
            id: d.id.clone(),
            name: d.name.clone(),
            paired_at: d.paired_at,
            last_seen: d.last_seen,
            online: online.get(&d.id).copied().unwrap_or(0) > 0,
        })
        .collect();
    out.sort_by(|a, b| b.paired_at.cmp(&a.paired_at));
    out
}

fn emit_devices() {
    crate::bus::emit(crate::bus::Event::RemoteDevices(devices()));
}

/// Quita un móvil emparejado y corta su conexión si está conectado.
pub fn revoke(id: &str) {
    let name = {
        let mut g = STORE.lock();
        let name = g.1.devices.iter().find(|d| d.id == id).map(|d| d.name.clone());
        g.1.devices.retain(|d| d.id != id);
        name
    };
    save();
    let _ = EVENTS.send(Event::Revoked(id.to_string()));
    if let Some(n) = name {
        crate::linfo!("{}", t!("log.remote.revoked", name = n));
    }
    emit_devices();
}

/// Respuesta del usuario del PC a una petición de emparejamiento.
pub fn answer_pairing(req: &str, ok: bool) {
    if let Some(tx) = PAIRING.lock().remove(req) {
        let _ = tx.send(ok);
    }
}

/// Arranca, para o reinicia el control remoto según los ajustes.
pub fn restart(e: &Arc<Engine>) {
    let e = e.clone();
    crate::rt::spawn(async move {
        let mut ctl = SERVER.lock().await;
        let was_running = ctl.is_some();
        if let Some(stop) = ctl.take() {
            let _ = stop.send(true);
            // Deja que el puerto se libere antes de volver a abrirlo.
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
        let (enabled, port) = {
            let s = e.settings.read();
            (s.remote_enabled, if s.remote_port == 0 { DEFAULT_PORT } else { s.remote_port })
        };
        let name = host_name();
        if !enabled {
            if was_running {
                crate::linfo!("{}", t!("log.remote.stopped"));
            }
            set_status(RemoteStatus { port, name, ..Default::default() });
            return;
        }
        let listener = match TcpListener::bind(("0.0.0.0", port)).await {
            Ok(l) => l,
            Err(err) => {
                let msg = if err.kind() == std::io::ErrorKind::AddrInUse {
                    t!("remote.portInUse", port = port)
                } else {
                    t!("remote.bindFailed", port = port, error = err)
                };
                crate::lerror!("{}", msg);
                set_status(RemoteStatus { error: Some(msg), port, name, ..Default::default() });
                return;
            }
        };
        let (stop_tx, stop_rx) = watch::channel(false);
        let mut warning = None;
        match bind_udp(DISCOVERY_PORT).await {
            Ok(sock) => {
                crate::rt::spawn(answer_probes(sock, port, stop_rx.clone()));
            }
            Err(err) => {
                let msg = t!("log.remote.noDiscovery", port = DISCOVERY_PORT, error = err);
                crate::lwarn!("{}", msg);
                warning = Some(msg);
            }
        }
        crate::rt::spawn(accept_loop(e.clone(), listener, stop_rx));
        *ctl = Some(stop_tx);
        let addresses = crate::api::lan_ip().map(|ip| vec![SocketAddr::new(ip, port).to_string()]).unwrap_or_default();
        crate::linfo!("{}", t!("log.remote.started", port = port));
        set_status(RemoteStatus { running: true, error: None, warning, port, name, addresses });
    });
}

async fn bind_udp(port: u16) -> std::io::Result<UdpSocket> {
    let s = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port))?;
    s.set_nonblocking(true)?;
    UdpSocket::from_std(s)
}

fn probe_reply(port: u16) -> Vec<u8> {
    json!({
        "app": "nebula",
        "v": PROTOCOL,
        "id": my_id(),
        "name": host_name(),
        "version": env!("CARGO_PKG_VERSION"),
        "port": port,
    })
    .to_string()
    .into_bytes()
}

async fn answer_probes(sock: UdpSocket, port: u16, mut stop: watch::Receiver<bool>) {
    let mut buf = [0u8; 256];
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            r = sock.recv_from(&mut buf) => match r {
                Ok((n, from)) if &buf[..n] == PROBE => {
                    let _ = sock.send_to(&probe_reply(port), from).await;
                }
                Ok(_) => {}
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }
}

async fn accept_loop(e: Arc<Engine>, listener: TcpListener, mut stop: watch::Receiver<bool>) {
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            r = listener.accept() => match r {
                Ok((sock, peer)) => {
                    let _ = sock.set_nodelay(true);
                    let e = e.clone();
                    let stop = stop.clone();
                    tokio::spawn(async move { serve_client(e, sock, peer, stop).await });
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }
}

async fn serve_client(e: Arc<Engine>, mut sock: TcpStream, peer: SocketAddr, stop: watch::Receiver<bool>) {
    let hello = match tokio::time::timeout(Duration::from_secs(10), read_json(&mut sock)).await {
        Ok(Ok(v)) => v,
        _ => return,
    };
    if hello["t"] != "hello" {
        return;
    }
    match hello["mode"].as_str() {
        Some("pair") => serve_pair(&e, sock, &hello, peer).await,
        Some("auth") => serve_session(e, sock, &hello, stop).await,
        _ => {}
    }
}

async fn serve_pair(e: &Arc<Engine>, mut sock: TcpStream, hello: &Value, peer: SocketAddr) {
    let device = hello["device"].as_str().unwrap_or("").trim().to_string();
    let name: String = hello["name"].as_str().unwrap_or("").trim().chars().take(60).collect();
    let name = if name.is_empty() { t!("remote.unnamedPhone") } else { name };
    let Some(client_pub) = decode32(&hello["pub"]) else { return };
    if device.is_empty() || device.len() > 64 {
        return;
    }
    if !PAIRING.lock().is_empty() {
        let _ = write_json(&mut sock, &json!({ "t": "error", "code": "busy" })).await;
        return;
    }
    let secret = StaticSecret::from(random32());
    let server_pub = PublicKey::from(&secret);
    let shared = *secret.diffie_hellman(&PublicKey::from(client_pub)).as_bytes();
    let code = pair_code(&shared, &client_pub, server_pub.as_bytes());
    let reply = json!({
        "t": "pairing",
        "pub": b64().encode(server_pub.as_bytes()),
        "id": my_id(),
        "name": host_name(),
        "version": env!("CARGO_PKG_VERSION"),
    });
    if write_json(&mut sock, &reply).await.is_err() {
        return;
    }
    let req = crate::util::new_id();
    let (tx, rx) = oneshot::channel();
    PAIRING.lock().insert(req.clone(), tx);
    crate::bus::emit(crate::bus::Event::RemotePairRequest(json!({ "req": req, "name": name, "code": code, "from": peer.ip().to_string() })));
    e.show_main();
    e.notify_os(&t!("remote.pairNotifyTitle"), &t!("remote.pairNotifyBody", name = name, code = code));
    // Se espera la respuesta en el PC; si el móvil se va antes, se cancela.
    let ok = tokio::select! {
        r = rx => r.unwrap_or(false),
        _ = tokio::time::sleep(PAIR_WAIT) => false,
        _ = read_frame(&mut sock, MAX_HELLO) => false,
    };
    PAIRING.lock().remove(&req);
    crate::bus::emit(crate::bus::Event::RemotePairClosed(json!({ "req": req })));
    if !ok {
        let _ = write_json(&mut sock, &json!({ "t": "error", "code": "rejected" })).await;
        return;
    }
    let token = random32();
    {
        let mut g = STORE.lock();
        g.1.devices.retain(|d| d.id != device);
        g.1.devices.push(Device {
            id: device,
            name: name.clone(),
            token: b64().encode(token),
            paired_at: crate::util::now_secs(),
            last_seen: crate::util::now_secs(),
        });
    }
    save();
    let mut c = Cipher::new(&pair_box_key(&shared));
    let sealed = c.seal(&token);
    let _ = write_json(&mut sock, &json!({ "t": "paired", "box": b64().encode(sealed) })).await;
    crate::linfo!("{}", t!("log.remote.paired", name = name));
    emit_devices();
}

struct OnlineGuard(String);
impl Drop for OnlineGuard {
    fn drop(&mut self) {
        {
            let mut o = ONLINE.lock();
            if let Some(n) = o.get_mut(&self.0) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    o.remove(&self.0);
                }
            }
        }
        emit_devices();
    }
}

async fn serve_session(e: Arc<Engine>, mut sock: TcpStream, hello: &Value, mut stop: watch::Receiver<bool>) {
    let device = hello["device"].as_str().unwrap_or("").to_string();
    let client_nonce = hello["nonce"].as_str().and_then(|n| b64().decode(n).ok()).unwrap_or_default();
    let found = {
        let g = STORE.lock();
        g.1.devices.iter().find(|d| d.id == device).map(|d| (d.name.clone(), token_of(&d.token)))
    };
    let (dev_name, Some(token)) = found.unwrap_or_default() else {
        let _ = write_json(&mut sock, &json!({ "t": "error", "code": "unauthorized" })).await;
        return;
    };
    if client_nonce.len() != 16 {
        return;
    }
    let server_nonce: [u8; 16] = random32()[..16].try_into().unwrap_or_default();
    let welcome = json!({
        "t": "welcome",
        "nonce": b64().encode(server_nonce),
        "id": my_id(),
        "name": host_name(),
        "version": env!("CARGO_PKG_VERSION"),
    });
    if write_json(&mut sock, &welcome).await.is_err() {
        return;
    }
    let (c2s, s2c) = session_keys(&token, &client_nonce, &server_nonce);
    let mut rx_cipher = Cipher::new(&c2s);
    let mut tx_cipher = Cipher::new(&s2c);
    let (mut rd, mut wr) = sock.into_split();
    // El primer mensaje del móvil demuestra que tiene el token: hasta entonces no se le manda nada.
    let first = match tokio::time::timeout(Duration::from_secs(10), read_frame(&mut rd, MAX_FRAME)).await {
        Ok(Ok(f)) => f,
        _ => return,
    };
    let Some(first) = rx_cipher.open(&first) else { return };

    {
        let mut g = STORE.lock();
        if let Some(d) = g.1.devices.iter_mut().find(|d| d.id == device) {
            d.last_seen = crate::util::now_secs();
        }
    }
    save();
    *ONLINE.lock().entry(device.clone()).or_default() += 1;
    let _guard = OnlineGuard(device.clone());
    emit_devices();

    let (out_tx, mut out_rx) = mpsc::channel::<String>(64);
    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            let sealed = tx_cipher.seal(msg.as_bytes());
            if write_frame(&mut wr, &sealed).await.is_err() {
                break;
            }
        }
        let _ = wr.shutdown().await;
    });
    let mut events = EVENTS.subscribe();
    // Estado inicial: la cola y el historial.
    let _ = out_tx.send(format!("{{\"ev\":\"queue\",\"data\":{}}}", serde_json::to_string(&e.snapshot()).unwrap_or_default())).await;
    let _ = out_tx.send(json!({ "ev": "history", "data": e.history() }).to_string()).await;
    spawn_request(&e, &dev_name, first, &out_tx);

    loop {
        tokio::select! {
            _ = stop.changed() => break,
            ev = events.recv() => match ev {
                Ok(Event::Queue(s)) => {
                    // Si el móvil va lento, se salta una actualización antes que acumularlas.
                    let _ = out_tx.try_send(format!("{{\"ev\":\"queue\",\"data\":{}}}", s));
                }
                Ok(Event::History) => {
                    let _ = out_tx.send(json!({ "ev": "history", "data": e.history() }).to_string()).await;
                }
                Ok(Event::Revoked(id)) if id == device => {
                    let _ = out_tx.send(json!({ "ev": "revoked" }).to_string()).await;
                    break;
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
            f = tokio::time::timeout(Duration::from_secs(90), read_frame(&mut rd, MAX_FRAME)) => {
                let Ok(Ok(f)) = f else { break };
                let Some(plain) = rx_cipher.open(&f) else { break };
                spawn_request(&e, &dev_name, plain, &out_tx);
            }
        }
    }
    drop(out_tx);
    let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;
}

fn spawn_request(e: &Arc<Engine>, dev_name: &str, plain: Vec<u8>, out: &mpsc::Sender<String>) {
    let e = e.clone();
    let out = out.clone();
    let dev_name = dev_name.to_string();
    tokio::spawn(async move {
        let Ok(req) = serde_json::from_slice::<Value>(&plain) else { return };
        let id = req["id"].clone();
        let op = req["op"].as_str().unwrap_or("").to_string();
        let res = handle_op(&e, &dev_name, &op, &req["args"]).await;
        let msg = match res {
            Ok(v) => json!({ "id": id, "ok": v }),
            Err(err) => json!({ "id": id, "err": err }),
        };
        let _ = out.send(msg.to_string()).await;
    });
}

fn arg_str<'a>(a: &'a Value, k: &str) -> &'a str {
    a[k].as_str().unwrap_or("")
}

/// Ejecuta en el PC lo que pide el móvil (solo lo que está en OPS).
async fn handle_op(e: &Arc<Engine>, dev_name: &str, op: &str, a: &Value) -> Result<Value, String> {
    if !OPS.contains(&op) {
        return Err(t!("remote.notAllowed"));
    }
    match op {
        "ping" => Ok(json!("pong")),
        "get_queue" => Ok(to_val(&e.snapshot())),
        "get_history" => Ok(to_val(&e.history())),
        "get_stats" => Ok(to_val(&e.stats())),
        "get_indexer_stats" => Ok(to_val(&e.indexer_stats())),
        "get_job_files" => Ok(to_val(&e.job_files(arg_str(a, "id")))),
        "pause_job" => {
            e.pause_job(arg_str(a, "id"));
            Ok(Value::Null)
        }
        "resume_job" => {
            e.resume_job(arg_str(a, "id"));
            Ok(Value::Null)
        }
        "move_job" => {
            e.move_job(arg_str(a, "id"), a["to"].as_u64().unwrap_or(0) as usize);
            Ok(Value::Null)
        }
        "set_job_priority" => {
            e.set_job_priority(arg_str(a, "id"), a["priority"].as_i64().unwrap_or(0) as i32);
            Ok(Value::Null)
        }
        "set_job_category" => {
            e.set_job_category(arg_str(a, "id"), arg_str(a, "category"));
            Ok(Value::Null)
        }
        "remove_job" => {
            e.remove_job(arg_str(a, "id"), a["deleteFiles"].as_bool().unwrap_or(false));
            Ok(Value::Null)
        }
        "set_paused" => {
            e.set_paused(a["paused"].as_bool().unwrap_or(false));
            Ok(Value::Null)
        }
        "set_speed_limit" => {
            e.set_speed_limit(a["kbps"].as_u64().unwrap_or(0))?;
            Ok(Value::Null)
        }
        "remove_history" => {
            e.remove_history(arg_str(a, "id"), a["deleteFiles"].as_bool().unwrap_or(false));
            history_changed();
            Ok(Value::Null)
        }
        "clear_history" => {
            e.clear_history(a["onlyCompleted"].as_bool().unwrap_or(true));
            history_changed();
            Ok(Value::Null)
        }
        "add_url" => {
            let url = arg_str(a, "url").trim().to_string();
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(t!("err.invalidLink"));
            }
            let (data, f) = e.grab_nzb( &url, None).await?;
            // Desde la búsqueda del móvil llega el título del resultado: manda sobre el del archivo.
            let (name, strong) = match arg_str(a, "name").trim() {
                "" => (f.filename.unwrap_or_default(), f.strong),
                n => (n.to_string(), true),
            };
            add_nzb(e, dev_name, data, name, strong, url, arg_add(a)).await
        }
        "add_nzb" => {
            let data = b64().decode(arg_str(a, "data")).map_err(|_| t!("remote.protocol"))?;
            let name = arg_str(a, "name").to_string();
            add_nzb(e, dev_name, data, name.clone(), true, name, arg_add(a)).await
        }
        "get_vias" => Ok(vias(e)),
        _ => Err(t!("remote.notAllowed")),
    }
}

/// Método pedido por el móvil: "nntp", "nntp:<servidor>" o el id de un servicio debrid de este PC.
fn arg_via(a: &Value) -> Option<String> {
    a["via"].as_str().map(str::trim).filter(|v| !v.is_empty() && v.len() <= 80).map(str::to_string)
}

/// Lo que el móvil puede elegir al añadir: método, categoría y categorías Newznab del resultado.
struct AddArgs {
    via: Option<String>,
    category: Option<String>,
    newznab: Vec<u32>,
}

fn arg_add(a: &Value) -> AddArgs {
    AddArgs {
        via: arg_via(a),
        category: a["category"].as_str().map(str::trim).filter(|v| !v.is_empty() && v.len() <= 80).map(str::to_string),
        newznab: a["newznab"]
            .as_array()
            .map(|l| l.iter().filter_map(Value::as_u64).take(20).map(|n| n as u32).collect())
            .unwrap_or_default(),
    }
}

/// Formas de descargar que tiene este PC (sin contraseñas ni API keys), para el menú del móvil.
fn vias(e: &Engine) -> Value {
    let s = e.settings.read();
    let servers: Vec<Value> = s
        .servers
        .iter()
        .filter(|x| x.enabled && !x.host.trim().is_empty())
        .map(|x| json!({ "id": x.id, "name": x.name, "host": x.host, "priority": x.priority }))
        .collect();
    let debrid: Vec<Value> = s
        .debrid
        .iter()
        .filter(|d| d.usable())
        .map(|d| json!({ "id": d.id, "kind": d.kind, "name": d.name }))
        .collect();
    let categories: Vec<&str> = s.categories.iter().map(|c| c.name.as_str()).collect();
    json!({ "servers": servers, "debrid": debrid, "downloadVia": s.download_via, "categories": categories })
}

fn to_val<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or_default()
}

fn history_changed() {
    crate::bus::emit(crate::bus::Event::History);
    publish_history();
}

/// Añade un NZB a la cola del PC con las opciones por defecto (como "Añadir con un clic").
async fn add_nzb(
    e: &Arc<Engine>,
    dev_name: &str,
    data: Vec<u8>,
    name: String,
    strong: bool,
    source: String,
    args: AddArgs,
) -> Result<Value, String> {
    let engine = e.clone();
    let preview = tokio::task::spawn_blocking(move || engine.register_pending(data, &name, strong, source))
        .await
        .map_err(|_| t!("err.internal"))??;
    let req = AddRequest {
        via: args.via,
        category: args.category,
        newznab: args.newznab,
        ..AddRequest::with_defaults(&preview, e.settings.read().add_paused)
    };
    let id = e.add_job(req)?;
    crate::linfo!("{}", t!("log.remote.added", device = dev_name, name = preview.name));
    e.toast("info", t!("remote.addedToast", device = dev_name, name = preview.name));
    Ok(json!({ "id": id, "name": preview.name }))
}

// ===========================================================================
// Móvil: cliente
// ===========================================================================

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub id: String,
    pub name: String,
    pub version: String,
    pub addr: String,
    pub paired: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PcView {
    pub id: String,
    pub name: String,
    pub addr: String,
    pub paired_at: u64,
}

pub fn pcs() -> Vec<PcView> {
    STORE
        .lock()
        .1
        .pcs
        .iter()
        .map(|p| PcView { id: p.id.clone(), name: p.name.clone(), addr: p.addr.clone(), paired_at: p.paired_at })
        .collect()
}

pub fn forget(id: &str) {
    STORE.lock().1.pcs.retain(|p| p.id != id);
    save();
    if SESSION.lock().as_ref().is_some_and(|s| s.pc_id == id) {
        disconnect();
    }
}

fn parse_found(data: &[u8], from: SocketAddr) -> Option<Found> {
    let v: Value = serde_json::from_slice(data).ok()?;
    if v["app"] != "nebula" {
        return None;
    }
    let port = v["port"].as_u64().filter(|p| *p > 0 && *p <= 65535)? as u16;
    let id = v["id"].as_str()?.to_string();
    let paired = STORE.lock().1.pcs.iter().any(|p| p.id == id);
    Some(Found {
        id,
        name: v["name"].as_str().unwrap_or("Nebula").to_string(),
        version: v["version"].as_str().unwrap_or("").to_string(),
        addr: SocketAddr::new(from.ip(), port).to_string(),
        paired,
    })
}

/// Busca PCs con Nebula en la red local (difusión UDP). Tarda `wait`.
pub async fn discover(wait: Duration) -> Vec<Found> {
    let Ok(sock) = bind_udp(0).await else { return Vec::new() };
    let _ = sock.set_broadcast(true);
    let mut targets: Vec<SocketAddr> = vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), DISCOVERY_PORT)];
    // Algunas redes no dejan pasar 255.255.255.255: también a la difusión de la subred (/24).
    if let Some(IpAddr::V4(ip)) = crate::api::lan_ip() {
        let o = ip.octets();
        targets.push(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(o[0], o[1], o[2], 255)), DISCOVERY_PORT));
    }
    let me = my_id();
    let mut found: Vec<Found> = Vec::new();
    let deadline = Instant::now() + wait;
    let mut next_probe = Instant::now();
    let mut buf = [0u8; 1024];
    while Instant::now() < deadline {
        if Instant::now() >= next_probe {
            for t in &targets {
                let _ = sock.send_to(PROBE, t).await;
            }
            next_probe = Instant::now() + Duration::from_millis(600);
        }
        let left = deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(300));
        if let Ok(Ok((n, from))) = tokio::time::timeout(left, sock.recv_from(&mut buf)).await {
            if let Some(f) = parse_found(&buf[..n], from) {
                if f.id != me && !found.iter().any(|x| x.id == f.id) {
                    found.push(f);
                }
            }
        }
    }
    found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    found
}

/// "192.168.1.20", "192.168.1.20:5078" o "[fd7a::1]:5078" → dirección con puerto.
pub fn normalize_addr(input: &str) -> Result<String, String> {
    let s = input.trim().trim_start_matches("http://").trim_end_matches('/');
    if s.is_empty() {
        return Err(t!("remote.badAddress"));
    }
    if let Ok(a) = s.parse::<SocketAddr>() {
        return Ok(a.to_string());
    }
    if let Ok(ip) = s.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, DEFAULT_PORT).to_string());
    }
    // Nombre de equipo (p. ej., MagicDNS de Tailscale), con o sin puerto.
    let (host, port) = match s.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|_| t!("remote.badAddress"))?),
        None => (s, DEFAULT_PORT),
    };
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return Err(t!("remote.badAddress"));
    }
    Ok(format!("{}:{}", host, port))
}

async fn dial(addr: &str) -> Result<TcpStream, String> {
    match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
        Ok(Ok(s)) => {
            let _ = s.set_nodelay(true);
            Ok(s)
        }
        _ => Err(t!("remote.unreachable", address = addr)),
    }
}

fn server_error(v: &Value, pc: &str) -> String {
    match v["code"].as_str() {
        Some("unauthorized") => t!("remote.unauthorized", name = pc),
        Some("rejected") => t!("remote.rejected"),
        Some("busy") => t!("remote.busy"),
        _ => t!("remote.protocol"),
    }
}

static PAIR_CANCEL: LazyLock<Notify> = LazyLock::new(Notify::new);

pub fn cancel_pairing() {
    PAIR_CANCEL.notify_waiters();
}

/// Empareja este móvil con el PC de `addr`. Mientras se espera la aprobación, emite
/// `remote-pair-code` con el código que debe salir también en el PC.
pub async fn pair(addr: &str, name: &str) -> Result<PcView, String> {
    let addr = normalize_addr(addr)?;
    let mut sock = dial(&addr).await?;
    let secret = StaticSecret::from(random32());
    let client_pub = PublicKey::from(&secret);
    let hello = json!({
        "t": "hello",
        "v": PROTOCOL,
        "mode": "pair",
        "device": my_id(),
        "name": name,
        "pub": b64().encode(client_pub.as_bytes()),
    });
    write_json(&mut sock, &hello).await.map_err(|_| t!("remote.unreachable", address = addr))?;
    let reply = tokio::time::timeout(Duration::from_secs(10), read_json(&mut sock))
        .await
        .map_err(|_| t!("remote.unreachable", address = addr))??;
    if reply["t"] != "pairing" {
        return Err(server_error(&reply, &addr));
    }
    let server_pub = decode32(&reply["pub"]).ok_or_else(|| t!("remote.protocol"))?;
    let pc_id = reply["id"].as_str().unwrap_or("").to_string();
    let pc_name = reply["name"].as_str().unwrap_or("Nebula").to_string();
    if pc_id.is_empty() {
        return Err(t!("remote.protocol"));
    }
    let shared = *secret.diffie_hellman(&PublicKey::from(server_pub)).as_bytes();
    let code = pair_code(&shared, client_pub.as_bytes(), &server_pub);
    crate::bus::emit(crate::bus::Event::RemotePairCode(json!({ "code": code, "name": pc_name })));
    let done = tokio::select! {
        r = tokio::time::timeout(PAIR_WAIT + Duration::from_secs(10), read_json(&mut sock)) => r,
        _ = PAIR_CANCEL.notified() => return Err(t!("remote.cancelled")),
    };
    let done = match done {
        Ok(Ok(v)) => v,
        Ok(Err(_)) => return Err(t!("remote.rejected")),
        Err(_) => return Err(t!("remote.pairTimeout")),
    };
    if done["t"] != "paired" {
        return Err(server_error(&done, &pc_name));
    }
    let sealed = done["box"].as_str().and_then(|s| b64().decode(s).ok()).ok_or_else(|| t!("remote.protocol"))?;
    let token = Cipher::new(&pair_box_key(&shared)).open(&sealed).ok_or_else(|| t!("remote.protocol"))?;
    let pc = Pc { id: pc_id, name: pc_name, addr, token: b64().encode(token), paired_at: crate::util::now_secs() };
    {
        let mut g = STORE.lock();
        g.1.pcs.retain(|p| p.id != pc.id);
        g.1.pcs.push(pc.clone());
    }
    save();
    Ok(PcView { id: pc.id, name: pc.name, addr: pc.addr, paired_at: pc.paired_at })
}

/// Conexión autenticada con un PC.
struct Conn {
    rd: OwnedReadHalf,
    wr: OwnedWriteHalf,
    tx: Cipher,
    rx: Cipher,
}

enum DialError {
    Unauthorized(String),
    Other(String),
}

async fn connect_auth(pc: &Pc) -> Result<Conn, DialError> {
    let token = token_of(&pc.token).ok_or_else(|| DialError::Unauthorized(t!("remote.unauthorized", name = pc.name)))?;
    let mut sock = dial(&pc.addr).await.map_err(DialError::Other)?;
    let client_nonce: [u8; 16] = random32()[..16].try_into().unwrap_or_default();
    let hello = json!({ "t": "hello", "v": PROTOCOL, "mode": "auth", "device": my_id(), "nonce": b64().encode(client_nonce) });
    let offline = || DialError::Other(t!("remote.offline", name = pc.name));
    write_json(&mut sock, &hello).await.map_err(|_| offline())?;
    let w = tokio::time::timeout(Duration::from_secs(8), read_json(&mut sock)).await.map_err(|_| offline())?.map_err(|_| offline())?;
    if w["t"] != "welcome" {
        return Err(if w["code"] == "unauthorized" {
            DialError::Unauthorized(t!("remote.unauthorized", name = pc.name))
        } else {
            DialError::Other(server_error(&w, &pc.name))
        });
    }
    if w["id"].as_str() != Some(pc.id.as_str()) {
        // Otro equipo en esa dirección (la IP del PC ha cambiado).
        return Err(offline());
    }
    let server_nonce = w["nonce"].as_str().and_then(|n| b64().decode(n).ok()).unwrap_or_default();
    let (c2s, s2c) = session_keys(&token, &client_nonce, &server_nonce);
    let (rd, wr) = sock.into_split();
    Ok(Conn { rd, wr, tx: Cipher::new(&c2s), rx: Cipher::new(&s2c) })
}

/// Sentido de salida de una conexión.
struct Out {
    wr: OwnedWriteHalf,
    tx: Cipher,
}

impl Out {
    async fn send(&mut self, v: &Value) -> std::io::Result<()> {
        let sealed = self.tx.seal(v.to_string().as_bytes());
        write_frame(&mut self.wr, &sealed).await
    }
}

/// Estado de la conexión que ve la interfaz.
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    pub id: String,
    pub name: String,
    /// "connecting", "online", "offline" o "unauthorized".
    pub status: String,
    pub error: Option<String>,
    pub version: String,
}

type Reply = oneshot::Sender<Result<Value, String>>;

struct Session {
    pc_id: String,
    calls: mpsc::Sender<(String, Value, Reply)>,
    stop: watch::Sender<bool>,
    wake: Arc<Notify>,
    state: Mutex<SessionState>,
}

static SESSION: LazyLock<Mutex<Option<Arc<Session>>>> = LazyLock::new(|| Mutex::new(None));

pub fn session_state() -> Option<SessionState> {
    SESSION.lock().as_ref().map(|s| s.state.lock().clone())
}

fn set_state(s: &Session, status: &str, error: Option<String>) {
    let st = {
        let mut st = s.state.lock();
        st.status = status.into();
        st.error = error;
        st.clone()
    };
    crate::bus::emit(crate::bus::Event::RemoteState(st));
}

fn find_pc(id: &str) -> Option<Pc> {
    STORE.lock().1.pcs.iter().find(|p| p.id == id).cloned()
}

/// Empieza a controlar el PC `id` (o sigue si ya se estaba controlando).
pub fn connect(id: &str) -> Result<SessionState, String> {
    let pc = find_pc(id).ok_or_else(|| t!("remote.notPaired"))?;
    if let Some(s) = SESSION.lock().as_ref() {
        if s.pc_id == id && !*s.stop.borrow() {
            s.wake.notify_one();
            return Ok(s.state.lock().clone());
        }
    }
    disconnect();
    let (calls_tx, calls_rx) = mpsc::channel(32);
    let (stop_tx, stop_rx) = watch::channel(false);
    let s = Arc::new(Session {
        pc_id: pc.id.clone(),
        calls: calls_tx,
        stop: stop_tx,
        wake: Arc::new(Notify::new()),
        state: Mutex::new(SessionState { id: pc.id.clone(), name: pc.name.clone(), status: "connecting".into(), ..Default::default() }),
    });
    *SESSION.lock() = Some(s.clone());
    let st = s.state.lock().clone();
    crate::rt::spawn(run_session(s, calls_rx, stop_rx));
    Ok(st)
}

pub fn disconnect() {
    if let Some(s) = SESSION.lock().take() {
        let _ = s.stop.send(true);
    }
}

/// La app vuelve a primer plano: se reintenta ya o se comprueba que la conexión sigue viva.
pub fn wake() {
    if let Some(s) = SESSION.lock().as_ref() {
        s.wake.notify_one();
    }
}

enum Ended {
    Stopped,
    Lost,
    Revoked,
}

async fn run_session(s: Arc<Session>, mut calls: mpsc::Receiver<(String, Value, Reply)>, mut stop: watch::Receiver<bool>) {
    let mut fails: u32 = 0;
    loop {
        if *stop.borrow() {
            break;
        }
        let Some(pc) = find_pc(&s.pc_id) else { break };
        if fails == 0 {
            set_state(&s, "connecting", None);
        }
        match connect_auth(&pc).await {
            Ok(conn) => {
                fails = 0;
                {
                    let mut st = s.state.lock();
                    st.name = pc.name.clone();
                }
                set_state(&s, "online", None);
                match run_conn(&s, conn, &mut calls, &mut stop).await {
                    Ended::Stopped => break,
                    Ended::Revoked => {
                        forget_quietly(&s.pc_id);
                        set_state(&s, "unauthorized", Some(t!("remote.unauthorized", name = pc.name)));
                        break;
                    }
                    Ended::Lost => set_state(&s, "offline", Some(t!("remote.offline", name = pc.name))),
                }
            }
            Err(DialError::Unauthorized(msg)) => {
                forget_quietly(&s.pc_id);
                set_state(&s, "unauthorized", Some(msg));
                break;
            }
            Err(DialError::Other(_)) => {
                fails += 1;
                set_state(&s, "offline", Some(t!("remote.offline", name = pc.name)));
                // La IP del PC puede haber cambiado: se busca otra vez en la red por su id.
                if fails % 2 == 0 {
                    if let Some(f) = discover(Duration::from_millis(1500)).await.into_iter().find(|f| f.id == pc.id) {
                        if f.addr != pc.addr {
                            let mut g = STORE.lock();
                            if let Some(p) = g.1.pcs.iter_mut().find(|p| p.id == pc.id) {
                                p.addr = f.addr.clone();
                            }
                            drop(g);
                            save();
                            continue;
                        }
                    }
                }
            }
        }
        // Espera antes de reintentar; lo que se pida mientras tanto falla enseguida.
        let wait = Duration::from_secs(u64::from(fails.clamp(1, 4)) * 3);
        let sleep = tokio::time::sleep(wait);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                _ = &mut sleep => break,
                _ = s.wake.notified() => break,
                _ = stop.changed() => break,
                c = calls.recv() => match c {
                    Some((_, _, reply)) => {
                        let _ = reply.send(Err(t!("remote.offline", name = pc.name)));
                    }
                    None => return,
                },
            }
        }
    }
    if *stop.borrow() {
        crate::bus::emit(crate::bus::Event::RemoteState(SessionState { id: s.pc_id.clone(), status: "stopped".into(), ..Default::default() }));
    }
}

fn forget_quietly(id: &str) {
    STORE.lock().1.pcs.retain(|p| p.id != id);
    save();
}

async fn run_conn(
    s: &Session,
    conn: Conn,
    calls: &mut mpsc::Receiver<(String, Value, Reply)>,
    stop: &mut watch::Receiver<bool>,
) -> Ended {
    let mut pending: HashMap<u64, Reply> = HashMap::new();
    let mut next_id: u64 = 1;
    let mut last_rx = Instant::now();
    let mut probe: Option<Instant> = None;
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    let Conn { mut rd, wr, tx, mut rx } = conn;
    let mut out = Out { wr, tx };
    // El PC no manda nada hasta recibir el primer mensaje (que demuestra que tenemos el token).
    if out.send(&json!({ "id": 0, "op": "ping" })).await.is_err() {
        return Ended::Lost;
    }
    // Lectura en una tarea aparte: read_exact no se puede cancelar a medias.
    let (frames_tx, mut frames) = mpsc::channel::<Vec<u8>>(64);
    let reader = tokio::spawn(async move {
        while let Ok(f) = read_frame(&mut rd, MAX_FRAME).await {
            if frames_tx.send(f).await.is_err() {
                break;
            }
        }
    });
    let ended = loop {
        tokio::select! {
            _ = stop.changed() => break Ended::Stopped,
            _ = s.wake.notified() => {
                probe = Some(Instant::now());
                if out.send(&json!({ "id": 0, "op": "ping" })).await.is_err() {
                    break Ended::Lost;
                }
            }
            c = calls.recv() => {
                let Some((op, args, reply)) = c else { break Ended::Stopped };
                let id = next_id;
                next_id += 1;
                if out.send(&json!({ "id": id, "op": op, "args": args })).await.is_err() {
                    let _ = reply.send(Err(t!("remote.offline", name = s.state.lock().name.clone())));
                    break Ended::Lost;
                }
                pending.insert(id, reply);
            }
            f = frames.recv() => {
                let Some(f) = f else { break Ended::Lost };
                let Some(plain) = rx.open(&f) else { break Ended::Lost };
                last_rx = Instant::now();
                probe = None;
                let Ok(v) = serde_json::from_slice::<Value>(&plain) else { continue };
                if let Some(ev) = v["ev"].as_str() {
                    match ev {
                        "queue" => crate::bus::emit(crate::bus::Event::RemoteQueue(v["data"].clone())),
                        "history" => crate::bus::emit(crate::bus::Event::RemoteHistory(v["data"].clone())),
                        "revoked" => break Ended::Revoked,
                        _ => {}
                    }
                } else if let Some(id) = v["id"].as_u64() {
                    if let Some(reply) = pending.remove(&id) {
                        let r = match v.get("err").and_then(Value::as_str) {
                            Some(err) => Err(err.to_string()),
                            None => Ok(v.get("ok").cloned().unwrap_or(Value::Null)),
                        };
                        let _ = reply.send(r);
                    }
                }
            }
            _ = tick.tick() => {
                let idle = last_rx.elapsed();
                // Un PC que se duerme no cierra la conexión: se nota porque deja de contestar.
                if idle > Duration::from_secs(25) || probe.is_some_and(|p| p.elapsed() > Duration::from_secs(5)) {
                    break Ended::Lost;
                }
                if idle > Duration::from_secs(8) && out.send(&json!({ "id": 0, "op": "ping" })).await.is_err() {
                    break Ended::Lost;
                }
            }
        }
    };
    reader.abort();
    let name = s.state.lock().name.clone();
    for (_, reply) in pending.drain() {
        let _ = reply.send(Err(t!("remote.offline", name = name)));
    }
    let _ = out.wr.shutdown().await;
    ended
}

/// Pide algo al PC que se está controlando.
pub async fn call(op: &str, args: Value) -> Result<Value, String> {
    if !OPS.contains(&op) {
        return Err(t!("remote.notAllowed"));
    }
    let s = SESSION.lock().clone().ok_or_else(|| t!("remote.notConnected"))?;
    let (tx, rx) = oneshot::channel();
    s.calls.send((op.to_string(), args, tx)).await.map_err(|_| t!("remote.notConnected"))?;
    let limit = if op.starts_with("add_") { Duration::from_secs(180) } else { Duration::from_secs(30) };
    match tokio::time::timeout(limit, rx).await {
        Ok(Ok(r)) => r,
        _ => Err(t!("remote.offline", name = s.state.lock().name.clone())),
    }
}

/// Una sola petición a un PC emparejado, sin dejar la conexión abierta.
async fn call_once(pc: &Pc, op: &str, args: Value) -> Result<Value, String> {
    let Conn { mut rd, wr, tx, mut rx } = connect_auth(pc).await.map_err(|e| match e {
        DialError::Unauthorized(m) | DialError::Other(m) => m,
    })?;
    let mut out = Out { wr, tx };
    out.send(&json!({ "id": 1, "op": op, "args": args })).await.map_err(|_| t!("remote.offline", name = pc.name))?;
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let f = tokio::time::timeout(left, read_frame(&mut rd, MAX_FRAME))
            .await
            .map_err(|_| t!("remote.offline", name = pc.name))?
            .map_err(|_| t!("remote.offline", name = pc.name))?;
        let plain = rx.open(&f).ok_or_else(|| t!("remote.protocol"))?;
        let v: Value = serde_json::from_slice(&plain).map_err(|_| t!("remote.protocol"))?;
        if v["id"].as_u64() == Some(1) {
            let _ = out.wr.shutdown().await;
            return match v.get("err").and_then(Value::as_str) {
                Some(err) => Err(err.to_string()),
                None => Ok(v.get("ok").cloned().unwrap_or(Value::Null)),
            };
        }
        if v["ev"] == "revoked" {
            forget_quietly(&pc.id);
            return Err(t!("remote.unauthorized", name = pc.name));
        }
    }
}

/// Manda un NZB (`kind` "file" con su ruta, o "url") al PC `id`.
#[allow(clippy::too_many_arguments)]
pub async fn send_nzb(
    id: &str,
    kind: &str,
    value: &str,
    via: Option<String>,
    name: Option<String>,
    category: Option<String>,
    newznab: Vec<u32>,
) -> Result<Value, String> {
    let pc = find_pc(id).ok_or_else(|| t!("remote.notPaired"))?;
    let (op, args) = if kind == "url" {
        ("add_url", json!({ "url": value, "via": via, "name": name, "category": category, "newznab": newznab }))
    } else {
        let data = tokio::fs::read(value).await.map_err(|err| t!("err.readFile", error = err))?;
        let name = std::path::Path::new(value).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        ("add_nzb", json!({ "name": name, "data": b64().encode(data), "via": via, "category": category }))
    };
    let active = SESSION.lock().as_ref().is_some_and(|s| s.pc_id == id && s.state.lock().status == "online");
    if active {
        call(op, args).await
    } else {
        call_once(&pc, op, args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ciphers_need_the_same_order() {
        let key = random32();
        let mut a = Cipher::new(&key);
        let mut b = Cipher::new(&key);
        let m1 = a.seal(b"uno");
        let m2 = a.seal(b"dos");
        assert_eq!(b.open(&m1).as_deref(), Some(&b"uno"[..]));
        assert_eq!(b.open(&m2).as_deref(), Some(&b"dos"[..]));
        // Repetir un mensaje no sirve.
        assert!(b.open(&m2).is_none());
        let mut c = Cipher::new(&random32());
        assert!(c.open(&a.seal(b"x")).is_none());
    }

    #[test]
    fn both_sides_see_the_same_code() {
        let cs = StaticSecret::from(random32());
        let ss = StaticSecret::from(random32());
        let (cp, sp) = (PublicKey::from(&cs), PublicKey::from(&ss));
        let k1 = *cs.diffie_hellman(&sp).as_bytes();
        let k2 = *ss.diffie_hellman(&cp).as_bytes();
        let c1 = pair_code(&k1, cp.as_bytes(), sp.as_bytes());
        assert_eq!(c1, pair_code(&k2, cp.as_bytes(), sp.as_bytes()));
        assert_eq!(c1.len(), 6);
        // Alguien en medio con otra clave obtiene otro código.
        let ms = StaticSecret::from(random32());
        let mp = PublicKey::from(&ms);
        let k3 = *cs.diffie_hellman(&mp).as_bytes();
        assert_ne!(c1, pair_code(&k3, cp.as_bytes(), mp.as_bytes()));
    }

    #[test]
    fn session_keys_differ_by_direction_and_nonce() {
        let t = random32();
        let (a, b) = session_keys(&t, b"cccccccccccccccc", b"ssssssssssssssss");
        assert_ne!(a, b);
        let (c, _) = session_keys(&t, b"cccccccccccccccc", b"tttttttttttttttt");
        assert_ne!(a, c);
    }

    #[test]
    fn addresses() {
        assert_eq!(normalize_addr("192.168.1.20").unwrap(), "192.168.1.20:5078");
        assert_eq!(normalize_addr(" 100.101.1.2:6000 ").unwrap(), "100.101.1.2:6000");
        assert_eq!(normalize_addr("mi-pc.tail1234.ts.net").unwrap(), "mi-pc.tail1234.ts.net:5078");
        assert_eq!(normalize_addr("fd7a:115c:a1e0::1").unwrap(), "[fd7a:115c:a1e0::1]:5078");
        assert!(normalize_addr("").is_err());
        assert!(normalize_addr("pc:abc").is_err());
        assert!(normalize_addr("a b").is_err());
    }

    #[test]
    fn probe_answer_is_understood() {
        let reply = json!({ "app": "nebula", "v": 1, "id": "pc1", "name": "SALON", "version": "1.7.0", "port": 5078 });
        let f = parse_found(reply.to_string().as_bytes(), "192.168.1.9:5078".parse().unwrap()).unwrap();
        assert_eq!(f.addr, "192.168.1.9:5078");
        assert_eq!(f.name, "SALON");
        assert!(parse_found(b"{\"app\":\"otra\"}", "1.2.3.4:1".parse().unwrap()).is_none());
    }

    #[test]
    fn only_allowed_ops() {
        for op in ["save_settings", "get_settings", "open_path", "retry_history", "transfer_export"] {
            assert!(!OPS.contains(&op), "{}", op);
        }
    }

    /// Prueba contra un Nebula de verdad con el control remoto activado y un móvil emparejado:
    /// `NEBULA_LIVE_TOKEN=<token b64> cargo test live_pc -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn live_pc() {
        let token = std::env::var("NEBULA_LIVE_TOKEN").unwrap();
        STORE.lock().1.id = std::env::var("NEBULA_LIVE_DEVICE").unwrap_or_else(|_| "phone1".into());
        let found = discover(Duration::from_millis(1500)).await;
        println!("encontrados: {:?}", found);
        let f = found.first().expect("ningún PC");
        let pc = Pc { id: f.id.clone(), name: f.name.clone(), addr: f.addr.clone(), token: token.clone(), paired_at: 0 };
        let q = call_once(&pc, "get_queue", Value::Null).await.unwrap();
        assert!(q["jobs"].is_array(), "{}", q);
        let nzb = std::fs::read(std::env::var("NEBULA_LIVE_NZB").unwrap()).unwrap();
        let r = call_once(&pc, "add_nzb", json!({ "name": "prueba.nzb", "data": b64().encode(&nzb) })).await.unwrap();
        println!("añadido: {}", r);
        // La cola que se ve se actualiza dos veces por segundo.
        tokio::time::sleep(Duration::from_millis(1200)).await;
        let q = call_once(&pc, "get_queue", Value::Null).await.unwrap();
        assert!(q["jobs"].as_array().unwrap().iter().any(|j| j["id"] == r["id"]), "{}", q);
        call_once(&pc, "pause_job", json!({ "id": r["id"] })).await.unwrap();
        call_once(&pc, "set_speed_limit", json!({ "kbps": 2048 })).await.unwrap();
        tokio::time::sleep(Duration::from_millis(1200)).await;
        let q = call_once(&pc, "get_queue", Value::Null).await.unwrap();
        assert_eq!(q["speedLimit"], 2048);
        call_once(&pc, "set_speed_limit", json!({ "kbps": 0 })).await.unwrap();
        for j in q["jobs"].as_array().unwrap() {
            call_once(&pc, "remove_job", json!({ "id": j["id"], "deleteFiles": true })).await.unwrap();
        }
        assert!(call_once(&pc, "save_settings", json!({})).await.is_err());
        assert!(call_once(&pc, "get_stats", Value::Null).await.unwrap().is_array());
        // Un token equivocado no pasa del saludo.
        let bad = Pc { token: b64().encode([7u8; 32]), ..pc.clone() };
        assert!(call_once(&bad, "get_queue", Value::Null).await.is_err());
        // Sesión con eventos: llega la cola sin pedirla.
        let Conn { mut rd, wr, tx, mut rx } = connect_auth(&pc).await.ok().unwrap();
        let mut out = Out { wr, tx };
        out.send(&json!({ "id": 0, "op": "ping" })).await.unwrap();
        let mut evs = Vec::new();
        for _ in 0..3 {
            let f = read_frame(&mut rd, MAX_FRAME).await.unwrap();
            let v: Value = serde_json::from_slice(&rx.open(&f).unwrap()).unwrap();
            evs.push(v["ev"].as_str().unwrap_or("reply").to_string());
        }
        assert!(evs.contains(&"queue".to_string()) && evs.contains(&"history".to_string()), "{:?}", evs);
    }

    #[tokio::test]
    async fn frames_round_trip() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        write_frame(&mut a, b"hola").await.unwrap();
        assert_eq!(read_frame(&mut b, 16).await.unwrap(), b"hola");
        write_frame(&mut a, &[0u8; 64]).await.unwrap();
        assert!(read_frame(&mut b, 16).await.is_err());
    }
}
