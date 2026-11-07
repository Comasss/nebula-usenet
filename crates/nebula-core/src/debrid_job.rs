//! Descargas a través de un servicio debrid: envío del NZB, espera y descarga HTTP de los archivos.
//!
//! Cada archivo remoto se trocea en fragmentos de `CHUNK` bytes que se piden por rangos con varias
//! conexiones a la vez y se escriben directamente en su posición. Los fragmentos reutilizan los
//! segmentos del motor, así que el progreso, la pausa y la reanudación tras cerrar Nebula funcionan
//! igual que con NNTP. Al terminar se pasa por el mismo post-procesado (PAR2, extracción y mover).
use crate::config::{DebridConfig, Settings};
use crate::debrid::{self, DebridError, ErrKind, RemoteFile, RemoteState, Upload};
use crate::engine::{write_all_at, Cancel, Engine, Job, JobFile, JobOptions, JobStatus, PostInfo, SegState, Segment};
use crate::{lerror, linfo, lwarn, nzb, util};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Tamaño de cada fragmento pedido por rango.
pub const CHUNK: u64 = 32 * 1024 * 1024;
/// Datos acumulados antes de escribir en disco.
const FLUSH: usize = 4 * 1024 * 1024;
/// Los enlaces de TorBox caducan a las 3 h si no se han empezado a usar: se renuevan antes.
const LINK_TTL: Duration = Duration::from_secs(2 * 3600);
/// Intervalo de consulta del estado en el servicio (TorBox lo actualiza cada 5 s).
const POLL: Duration = Duration::from_secs(5);
/// Tiempo máximo sin recibir datos de una conexión.
const READ_TIMEOUT: Duration = Duration::from_secs(60);
/// Tras una subida sin confirmar, tiempo que se busca el NZB en el servicio antes de volver a subirlo.
const CONFIRM_WAIT: Duration = Duration::from_secs(15 * 60);
/// Subidas sin confirmar seguidas antes de darse por vencido con este servicio.
const MAX_UNCONFIRMED: u32 = 3;
/// Con el NZB ya subido, cada cuánto se mira si el servicio lo ha añadido sin esperar a su respuesta.
const LOOKUP_EVERY: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Modelo
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub enum DebridPhase {
    /// Pendiente de enviar el NZB al servicio.
    #[default]
    Submit,
    /// El servicio lo está descargando.
    Remote,
    /// Bajando los archivos al PC.
    Fetch,
}

pub struct DebridJob {
    /// Id del servicio en la configuración.
    pub provider: String,
    pub kind: String,
    pub label: String,
    pub phase: DebridPhase,
    pub remote_id: String,
    pub remote_hash: String,
    pub remote_folder: String,
    /// Elegido por defecto (no a mano): si falla se puede probar el siguiente servicio.
    pub auto: bool,
    /// Servicios que ya fallaron con este NZB.
    pub tried: Vec<String>,
    /// Tamaño estimado hasta conocer los archivos finales.
    pub estimate: u64,
    pub remote_percent: f64,
    pub remote_speed: u64,
    pub remote_eta: u64,
    pub remote_state: String,
    /// Aviso temporal (reintentos, esperas).
    pub notice: Option<String>,
    /// Hay un gestor en marcha para este trabajo.
    pub running: bool,
    /// Enlaces de descarga por índice de archivo.
    pub links: HashMap<u32, (String, Instant)>,
    /// Progreso de la subida del NZB: bytes entregados, total y velocidad.
    pub upload_sent: Arc<AtomicU64>,
    pub upload_total: u64,
    pub upload_speed: u64,
    /// La subida no se confirmó: hasta cuándo se busca el NZB por su hash antes de reenviarlo.
    pub confirm_until: Option<Instant>,
    /// Subidas seguidas que el servicio no confirmó.
    pub unconfirmed: u32,
    /// Desde cuándo se espera al servicio con el NZB ya subido (para mostrar el tiempo).
    pub wait_since: Option<Instant>,
}

impl DebridJob {
    pub fn new(cfg: &DebridConfig, auto: bool, estimate: u64) -> Self {
        Self {
            provider: cfg.id.clone(),
            kind: cfg.kind.clone(),
            label: cfg.label(),
            phase: DebridPhase::Submit,
            remote_id: String::new(),
            remote_hash: String::new(),
            remote_folder: String::new(),
            auto,
            tried: Vec::new(),
            estimate,
            remote_percent: 0.0,
            remote_speed: 0,
            remote_eta: 0,
            remote_state: t!("dj.willSend", name = cfg.label()),
            notice: None,
            running: false,
            links: HashMap::new(),
            upload_sent: Arc::default(),
            upload_total: 0,
            upload_speed: 0,
            confirm_until: None,
            unconfirmed: 0,
            wait_since: None,
        }
    }

    pub fn saved(&self) -> SavedDebrid {
        SavedDebrid {
            provider: self.provider.clone(),
            kind: self.kind.clone(),
            label: self.label.clone(),
            phase: self.phase,
            remote_id: self.remote_id.clone(),
            remote_hash: self.remote_hash.clone(),
            remote_folder: self.remote_folder.clone(),
            auto: self.auto,
            tried: self.tried.clone(),
            estimate: self.estimate,
            confirming: self.confirm_until.is_some(),
            unconfirmed: self.unconfirmed,
        }
    }

    pub fn from_saved(s: SavedDebrid) -> Self {
        let state = match s.phase {
            DebridPhase::Submit => t!("dj.willSend", name = s.label),
            DebridPhase::Remote => t!("dj.checking", name = s.label),
            DebridPhase::Fetch => t!("dj.fetching", name = s.label),
        };
        Self {
            provider: s.provider,
            kind: s.kind,
            label: s.label,
            phase: s.phase,
            remote_id: s.remote_id,
            remote_hash: s.remote_hash,
            remote_folder: s.remote_folder,
            auto: s.auto,
            tried: s.tried,
            estimate: s.estimate,
            remote_percent: 0.0,
            remote_speed: 0,
            remote_eta: 0,
            remote_state: state,
            notice: None,
            running: false,
            links: HashMap::new(),
            upload_sent: Arc::default(),
            upload_total: 0,
            upload_speed: 0,
            // Se cerró Nebula mientras se comprobaba: se vuelve a dar el plazo completo.
            confirm_until: s.confirming.then(|| Instant::now() + CONFIRM_WAIT),
            unconfirmed: s.unconfirmed,
            wait_since: s.confirming.then(Instant::now),
        }
    }

    /// Porcentaje subido del NZB.
    pub fn upload_percent(&self) -> f64 {
        if self.upload_total == 0 {
            return 0.0;
        }
        (self.upload_sent.load(Ordering::Relaxed) as f64 / self.upload_total as f64 * 100.0).min(100.0)
    }

    pub fn view(&self) -> DebridView {
        DebridView {
            provider: self.label.clone(),
            kind: self.kind.clone(),
            phase: self.phase,
            state: self.remote_state.clone(),
            upload: if self.phase == DebridPhase::Submit { self.upload_percent() } else { 100.0 },
            waiting: self.wait_since.is_some(),
            remote: match self.phase {
                DebridPhase::Submit => 0.0,
                DebridPhase::Remote => self.remote_percent,
                DebridPhase::Fetch => 100.0,
            },
        }
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct SavedDebrid {
    pub provider: String,
    pub kind: String,
    pub label: String,
    pub phase: DebridPhase,
    pub remote_id: String,
    pub remote_hash: String,
    pub remote_folder: String,
    pub auto: bool,
    pub tried: Vec<String>,
    pub estimate: u64,
    pub confirming: bool,
    pub unconfirmed: u32,
}

#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DebridView {
    pub provider: String,
    pub kind: String,
    pub phase: DebridPhase,
    pub state: String,
    /// Progreso de cada fase (0-100): subida del NZB y descarga en el servicio.
    pub upload: f64,
    pub remote: f64,
    /// NZB subido; se espera a que el servicio lo acepte (sin progreso que mostrar).
    pub waiting: bool,
}

/// Fragmentos de un archivo de `size` bytes (al menos uno, aunque esté vacío).
pub fn chunk_segments(size: u64) -> Vec<Segment> {
    let n = size.div_ceil(CHUNK).max(1);
    (0..n)
        .map(|i| Segment {
            bytes: (size - (i * CHUNK).min(size)).min(CHUNK),
            message_id: String::new(),
            state: SegState::Pending,
            tried: 0,
            attempts: 0,
        })
        .collect()
}

/// Archivo de la cola para un archivo remoto. `name` es la ruta relativa con `/`.
pub fn job_file(name: String, size: u64, remote: String) -> JobFile {
    let segments = chunk_segments(size);
    let lower = name.to_ascii_lowercase();
    JobFile {
        is_par2: lower.ends_with(".par2"),
        is_vol: nzb::vol_blocks_of(&name).is_some(),
        vol_blocks: nzb::vol_blocks_of(&name).unwrap_or(0),
        remaining: segments.len() as u32,
        name,
        bytes: size,
        segments,
        done_bytes: 0,
        failed_bytes: 0,
        failed_segs: 0,
        held: false,
        complete: false,
        yenc_name: None,
        handle: None,
        remote: Some(remote),
    }
}

/// Ruta local (dentro de la carpeta temporal) de un archivo con ruta relativa `a/b/c`.
pub fn local_path(work_dir: &Path, name: &str) -> PathBuf {
    let mut p = work_dir.to_path_buf();
    for c in name.split('/').filter(|c| !c.is_empty()) {
        p.push(c);
    }
    p
}

/// Nombres locales seguros y únicos para los archivos remotos.
fn local_names(files: &[RemoteFile]) -> Vec<String> {
    let mut seen = HashSet::new();
    files
        .iter()
        .map(|f| {
            let clean = f.path.split('/').filter(|c| !c.is_empty()).map(util::sanitize_filename).collect::<Vec<_>>().join("/");
            let clean = if clean.is_empty() { "archivo".to_string() } else { clean };
            let mut name = clean.clone();
            let mut n = 2;
            while !seen.insert(name.to_ascii_lowercase()) {
                name = format!("{} ({})", clean, n);
                n += 1;
            }
            name
        })
        .collect()
}

/// Post-procesado que se pide a TorBox según las opciones del trabajo.
pub fn pp_mode(o: &JobOptions) -> i32 {
    match (o.par2, o.extract, o.delete_archives) {
        (_, true, true) => -1,
        (_, true, false) => 2,
        (true, false, _) => 1,
        (false, false, _) => 0,
    }
}

fn has_nntp(s: &Settings) -> bool {
    s.servers.iter().any(|x| x.enabled && !x.host.trim().is_empty())
}

/// Servicio con el que descargar un NZB nuevo. `via`: "nntp", "debrid", id de un servicio o nada
/// (el método por defecto). Devuelve el servicio y si se eligió automáticamente.
/// Sin servidores NNTP configurados, el método por defecto es el primer servicio debrid.
pub fn resolve(s: &Settings, via: Option<&str>) -> Option<(DebridConfig, bool)> {
    let first = || s.debrid.iter().find(|d| d.usable()).cloned().map(|d| (d, true));
    let default = || if s.download_via == "debrid" || !has_nntp(s) { first() } else { None };
    match via.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) if v == "nntp" || v.starts_with("nntp:") => None,
        Some("debrid") => first(),
        Some(id) => s.debrid.iter().find(|d| d.id == id && d.usable()).cloned().map(|d| (d, false)).or_else(default),
        None => default(),
    }
}

fn with_label(msg: &str, label: &str) -> String {
    if msg.contains(label) {
        msg.to_string()
    } else {
        format!("{}: {}", label, msg)
    }
}

// ---------------------------------------------------------------------------
// Gestor de cada trabajo
// ---------------------------------------------------------------------------

/// Datos del trabajo que necesita el gestor en cada paso.
struct Snap {
    phase: DebridPhase,
    provider: String,
    label: String,
    remote_id: String,
    remote_hash: String,
    name: String,
    password: Option<String>,
    options: JobOptions,
    nzb_path: PathBuf,
    cancel: Arc<Cancel>,
}

enum Next {
    Now,
    After(Duration),
    Stop,
}

/// Marca el gestor como terminado al salir (por cualquier camino).
struct Running {
    e: Arc<Engine>,
    id: String,
}

impl Drop for Running {
    fn drop(&mut self) {
        let mut st = self.e.st.lock();
        if let Some(d) = st.jobs.iter_mut().find(|j| j.id == self.id).and_then(|j| j.debrid.as_mut()) {
            d.running = false;
            d.remote_speed = 0;
        }
    }
}

async fn sleep_or_cancel(cancel: &Cancel, d: Duration) {
    tokio::select! {
        _ = tokio::time::sleep(d) => {}
        _ = cancel.wait() => {}
    }
}

async fn drive(e: Arc<Engine>, id: String) {
    let _running = Running { e: e.clone(), id: id.clone() };
    let mut failures = 0u32;
    loop {
        let Some(s) = e.debrid_snap(&id) else { return };
        let cfg = e.settings.read().debrid.iter().find(|d| d.id == s.provider).cloned();
        let Some(cfg) = cfg.filter(|c| c.usable()) else {
            e.debrid_failed(&id, t!("dj.notConfigured", name = s.label)).await;
            return;
        };
        let res = match s.phase {
            DebridPhase::Submit => submit(&e, &id, &cfg, &s).await,
            DebridPhase::Remote => poll(&e, &id, &cfg, &s).await,
            DebridPhase::Fetch => fetch(&e, &id, &cfg, &s).await,
        };
        match res {
            Ok(Next::Now) => failures = 0,
            Ok(Next::After(d)) => {
                failures = 0;
                sleep_or_cancel(&s.cancel, d).await;
            }
            Ok(Next::Stop) => return,
            Err(err) => match err.kind {
                ErrKind::Transient | ErrKind::Busy => {
                    failures += 1;
                    if err.kind == ErrKind::Transient && failures > 30 {
                        e.debrid_failed(&id, err.msg).await;
                        return;
                    }
                    let wait = if err.kind == ErrKind::Busy { 60 } else { (5u64 << failures.min(5)).min(120) };
                    if failures == 1 || failures % 10 == 0 {
                        lwarn!("«{}»: {}", s.name, with_label(&err.msg, &s.label));
                    }
                    e.debrid_mut(&id, |_, d| d.notice = Some(format!("{} {}", err.msg, t!("dj.retryIn", secs = wait))));
                    sleep_or_cancel(&s.cancel, Duration::from_secs(wait)).await;
                }
                ErrKind::Auth | ErrKind::Fatal => {
                    e.debrid_failed(&id, err.msg).await;
                    return;
                }
            },
        }
    }
}

async fn submit(e: &Arc<Engine>, id: &str, cfg: &DebridConfig, s: &Snap) -> Result<Next, DebridError> {
    let raw = tokio::fs::read(&s.nzb_path)
        .await
        .map_err(|err| DebridError::new(ErrKind::Fatal, t!("dj.readNzb", error = err)))?;
    let data = nzb::maybe_gunzip(&raw).map_err(|m| DebridError::new(ErrKind::Fatal, m))?;
    let total = data.len() as u64;
    let Some(sent) = e.debrid_mut(id, |_, d| {
        d.upload_sent.store(0, Ordering::Relaxed);
        d.upload_total = total;
        d.upload_speed = 0;
        d.wait_since = None;
        d.remote_state = t!("dj.uploading", name = s.label, done = util::fmt_bytes(0), total = util::fmt_bytes(total));
        d.upload_sent.clone()
    }) else {
        return Ok(Next::Stop);
    };
    let up = Upload {
        nzb: &data,
        name: &s.name,
        password: s.password.as_deref(),
        post_processing: pp_mode(&s.options),
        sent: sent.clone(),
    };
    let hash = debrid::nzb_hash(&data);
    let send = debrid::submit(cfg, &up);
    tokio::pin!(send);
    // Mientras se sube se actualizan el progreso, la velocidad y el estado una vez por segundo.
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut speed = util::Speedo::new(5000);
    let mut uploaded: Option<Instant> = None;
    let mut next_lookup = Instant::now();
    let res = loop {
        tokio::select! {
            r = &mut send => break r,
            _ = s.cancel.wait() => return Ok(Next::Now),
            _ = tick.tick() => {
                let now = sent.load(Ordering::Relaxed);
                speed.push(now);
                let rate = speed.rate();
                if now >= total && uploaded.is_none() {
                    let at = Instant::now();
                    uploaded = Some(at);
                    next_lookup = at + LOOKUP_EVERY;
                    e.debrid_mut(id, |_, d| d.wait_since = Some(at));
                }
                e.debrid_mut(id, |_, d| {
                    if let Some(at) = uploaded {
                        d.upload_speed = 0;
                        d.remote_state = t!("dj.processing", name = s.label, time = fmt_wait(at.elapsed()));
                    } else {
                        d.upload_speed = rate;
                        d.remote_state = t!("dj.uploading", name = s.label, done = util::fmt_bytes(now), total = util::fmt_bytes(total));
                    }
                });
                // TorBox analiza el NZB antes de contestar y con NZB grandes tarda (o su pasarela corta con 504),
                // pero lo añade a la cuenta en cuanto termina: se busca por su hash para seguir sin esperar a la respuesta.
                if uploaded.is_some() && Instant::now() >= next_lookup && debrid::finds_by_hash(cfg) {
                    next_lookup = Instant::now() + LOOKUP_EVERY;
                    if let Ok(st) = debrid::status(cfg, "", &hash).await {
                        if !st.missing {
                            break Ok(debrid::Submitted { id: st.id.unwrap_or_default(), hash: hash.clone(), unconfirmed: false });
                        }
                    }
                }
            }
        }
    };
    e.debrid_mut(id, |_, d| {
        d.upload_speed = 0;
        d.wait_since = None;
    });
    let r = res?;
    if r.unconfirmed {
        lwarn!("{}", t!("log.dj.unconfirmed", name = s.name, provider = s.label));
    }
    let updated = e.debrid_mut(id, |_, d| {
        d.remote_id = r.id.clone();
        d.remote_hash = r.hash.clone();
        d.phase = DebridPhase::Remote;
        d.remote_state = if r.unconfirmed {
            t!("dj.confirming", name = s.label, time = fmt_wait(Duration::ZERO))
        } else {
            t!("dj.queuedAt", name = s.label)
        };
        d.remote_percent = 0.0;
        d.notice = None;
        if r.unconfirmed {
            d.confirm_until = Some(Instant::now() + CONFIRM_WAIT);
            d.unconfirmed += 1;
            d.wait_since = Some(Instant::now());
        } else {
            d.confirm_until = None;
            d.unconfirmed = 0;
        }
    });
    if updated.is_none() {
        // Se quitó de la cola mientras se enviaba.
        if e.settings.read().debrid_delete_after && !r.id.is_empty() {
            let _ = debrid::remove(cfg, &r.id, "").await;
        }
        return Ok(Next::Stop);
    }
    linfo!("{}", t!("log.dj.sent", name = s.name, provider = s.label));
    e.save_queue();
    Ok(Next::Now)
}

/// Tiempo de espera como m:ss.
fn fmt_wait(d: Duration) -> String {
    let s = d.as_secs();
    format!("{}:{:02}", s / 60, s % 60)
}

async fn poll(e: &Arc<Engine>, id: &str, cfg: &DebridConfig, s: &Snap) -> Result<Next, DebridError> {
    let st = debrid::status(cfg, &s.remote_id, &s.remote_hash).await?;
    let new_id = st.id;
    // Subida sin confirmar: mientras no aparezca se sigue buscando y, pasado el plazo, se vuelve a subir.
    let confirm = e.debrid_mut(id, |_, d| {
        if !st.missing {
            d.confirm_until = None;
            d.wait_since = None;
        }
        d.confirm_until.map(|t| (t, d.unconfirmed))
    });
    if let Some(Some((until, rounds))) = confirm {
        if Instant::now() < until {
            e.debrid_mut(id, |_, d| {
                let waited = d.wait_since.map(|t| t.elapsed()).unwrap_or_default();
                d.remote_state = t!("dj.confirming", name = s.label, time = fmt_wait(waited));
                d.notice = None;
            });
            return Ok(Next::After(LOOKUP_EVERY));
        }
        if rounds >= MAX_UNCONFIRMED {
            return Err(DebridError::new(ErrKind::Fatal, t!("dj.neverConfirmed", name = s.label)));
        }
        lwarn!("{}", t!("dj.notConfirmed", name = s.label));
        e.debrid_mut(id, |_, d| {
            d.confirm_until = None;
            d.wait_since = None;
            d.phase = DebridPhase::Submit;
            d.remote_hash.clear();
            d.notice = Some(t!("dj.notConfirmed", name = s.label));
        });
        e.save_queue();
        return Ok(Next::Now);
    }
    match st.state {
        RemoteState::Working { percent, speed, eta, state } => {
            e.debrid_mut(id, |_, d| {
                if let Some(i) = new_id {
                    d.remote_id = i;
                }
                d.remote_percent = percent;
                d.remote_speed = speed;
                d.remote_eta = eta;
                d.remote_state = t!("dj.stateAt", state = state, name = s.label);
                d.notice = None;
            });
            Ok(Next::After(POLL))
        }
        RemoteState::Ready { files, folder } => {
            let n = files.len();
            let total: u64 = files.iter().map(|f| f.size).sum();
            e.debrid_ready(id, new_id, files, folder);
            linfo!("{}", t!("log.dj.ready", name = s.name, provider = s.label, n = n, size = util::fmt_bytes(total)));
            e.save_queue();
            Ok(Next::Now)
        }
        RemoteState::Failed(msg) => Err(DebridError::new(ErrKind::Fatal, msg)),
    }
}

async fn fetch(e: &Arc<Engine>, id: &str, cfg: &DebridConfig, s: &Snap) -> Result<Next, DebridError> {
    if e.debrid_done(id) {
        e.debrid_finish(id, cfg, s);
        return Ok(Next::Stop);
    }
    let permit = match e.debrid_fetch.try_acquire() {
        Ok(p) => p,
        Err(_) => {
            e.debrid_mut(id, |_, d| d.remote_state = t!("dj.readyWaiting", name = s.label));
            tokio::select! {
                p = e.debrid_fetch.acquire() => match p {
                    Ok(p) => p,
                    Err(_) => return Ok(Next::Stop),
                },
                _ = s.cancel.wait() => return Ok(Next::Stop),
            }
        }
    };
    if !e.debrid_active(id) {
        return Ok(Next::Now);
    }
    e.debrid_mut(id, |_, d| {
        d.remote_state = t!("dj.fetching", name = s.label);
        d.notice = None;
    });
    let conns = e.settings.read().debrid_connections.clamp(1, 32) as usize;
    let run = Arc::new(Cancel::default());
    let failure: Arc<Mutex<Option<ChunkErr>>> = Arc::new(Mutex::new(None));
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..conns {
        set.spawn(worker(e.clone(), id.to_string(), cfg.clone(), s.cancel.clone(), run.clone(), failure.clone()));
    }
    while set.join_next().await.is_some() {}
    drop(permit);
    let failure = failure.lock().take();
    match failure {
        Some(ChunkErr::Api(err)) => return Err(err),
        Some(ChunkErr::Disk(err)) => {
            e.debrid_write_error(id, err);
            return Ok(Next::Stop);
        }
        _ => {}
    }
    if e.debrid_done(id) {
        e.debrid_finish(id, cfg, s);
        return Ok(Next::Stop);
    }
    Ok(Next::Now)
}

// ---------------------------------------------------------------------------
// Descarga HTTP por fragmentos
// ---------------------------------------------------------------------------

pub struct ChunkTask {
    fi: u32,
    ci: u32,
    offset: u64,
    len: u64,
    remote: String,
    name: String,
}

enum ChunkErr {
    /// Pausa, cancelación o fallo de otra conexión.
    Stop,
    Api(DebridError),
    Disk(std::io::Error),
}

async fn worker(
    e: Arc<Engine>,
    id: String,
    cfg: DebridConfig,
    cancel: Arc<Cancel>,
    run: Arc<Cancel>,
    failure: Arc<Mutex<Option<ChunkErr>>>,
) {
    loop {
        if run.is() || cancel.is() {
            return;
        }
        let Some(t) = e.debrid_take(&id) else { return };
        let mut written = 0u64;
        match chunk(&e, &id, &cfg, &t, &cancel, &run, &mut written).await {
            Ok(()) => e.debrid_chunk_done(&id, &t, written),
            Err(err) => {
                e.debrid_chunk_back(&id, &t, written);
                if matches!(err, ChunkErr::Stop) {
                    return;
                }
                {
                    let mut f = failure.lock();
                    if f.is_none() {
                        *f = Some(err);
                    }
                }
                run.cancel();
                return;
            }
        }
    }
}

fn backoff(tries: u32) -> Duration {
    Duration::from_secs((1u64 << tries.min(5)).min(30))
}

/// Baja un fragmento (reanudando dentro de él si se corta la conexión).
async fn chunk(
    e: &Arc<Engine>,
    id: &str,
    cfg: &DebridConfig,
    t: &ChunkTask,
    cancel: &Cancel,
    run: &Cancel,
    written: &mut u64,
) -> Result<(), ChunkErr> {
    let file = match e.debrid_file(id, t.fi) {
        Ok(Some(f)) => f,
        Ok(None) => return Err(ChunkErr::Stop),
        Err(err) => return Err(ChunkErr::Disk(err)),
    };
    if t.len == 0 {
        return Ok(());
    }
    let mut tries = 0u32;
    let mut refresh = false;
    loop {
        if cancel.is() || run.is() || !e.debrid_active(id) {
            return Err(ChunkErr::Stop);
        }
        let url = match e.debrid_link(id, cfg, t, refresh).await {
            Ok(u) => u,
            Err(err) if matches!(err.kind, ErrKind::Transient | ErrKind::Busy) && tries < 5 => {
                tries += 1;
                sleep_or_cancel(cancel, backoff(tries)).await;
                continue;
            }
            Err(err) => return Err(ChunkErr::Api(err)),
        };
        refresh = false;
        let start = t.offset + *written;
        let end = t.offset + t.len - 1;
        let req = debrid::FILES.get(&url).header(reqwest::header::RANGE, format!("bytes={}-{}", start, end)).send();
        let resp = match tokio::time::timeout(READ_TIMEOUT, req).await {
            Ok(Ok(r)) => r,
            Ok(Err(err)) => {
                tries += 1;
                if tries > 6 {
                    return Err(ChunkErr::Api(DebridError::new(
                        ErrKind::Transient,
                        t!("dj.fetchFailed", file = t.name, error = err.without_url()),
                    )));
                }
                sleep_or_cancel(cancel, backoff(tries)).await;
                continue;
            }
            Err(_) => {
                tries += 1;
                if tries > 6 {
                    return Err(ChunkErr::Api(DebridError::new(ErrKind::Transient, t!("dj.noResponse", name = cfg.label(), file = t.name))));
                }
                continue;
            }
        };
        let status = resp.status().as_u16();
        // Si el servidor no admite rangos (200), se descartan los bytes anteriores al fragmento.
        let skip = match status {
            206 => 0,
            200 => start,
            401 | 403 | 404 | 410 => {
                tries += 1;
                if tries > 4 {
                    return Err(ChunkErr::Api(DebridError::new(
                        ErrKind::Transient,
                        t!("dj.badLink", file = t.name, status = status),
                    )));
                }
                refresh = true;
                continue;
            }
            416 => {
                return Err(ChunkErr::Api(DebridError::new(
                    ErrKind::Fatal,
                    t!("dj.sizeMismatch", file = t.name, name = cfg.label()),
                )))
            }
            429 | 500..=599 => {
                tries += 1;
                if tries > 6 {
                    return Err(ChunkErr::Api(DebridError::new(
                        ErrKind::Transient,
                        t!("dj.httpError", status = status, file = t.name),
                    )));
                }
                sleep_or_cancel(cancel, backoff(tries)).await;
                continue;
            }
            c => {
                return Err(ChunkErr::Api(DebridError::new(ErrKind::Fatal, t!("dj.httpError", status = c, file = t.name))))
            }
        };
        match stream(e, id, resp, &file, start, end + 1 - start, skip, cancel, run, written).await {
            Ok(()) => return Ok(()),
            Err(StreamErr::Stop) => return Err(ChunkErr::Stop),
            Err(StreamErr::Disk(err)) => return Err(ChunkErr::Disk(err)),
            Err(StreamErr::Net(msg)) => {
                tries += 1;
                if tries > 6 {
                    return Err(ChunkErr::Api(DebridError::new(ErrKind::Transient, format!("{} ({})", msg, t.name))));
                }
                sleep_or_cancel(cancel, backoff(tries)).await;
            }
        }
    }
}

enum StreamErr {
    Stop,
    Net(String),
    Disk(std::io::Error),
}

#[allow(clippy::too_many_arguments)]
async fn stream(
    e: &Arc<Engine>,
    id: &str,
    mut resp: reqwest::Response,
    file: &Arc<File>,
    mut off: u64,
    want: u64,
    mut skip: u64,
    cancel: &Cancel,
    run: &Cancel,
    written: &mut u64,
) -> Result<(), StreamErr> {
    let mut remaining = want;
    let mut buf: Vec<u8> = Vec::with_capacity(FLUSH);
    while remaining > 0 {
        let next = tokio::select! {
            r = tokio::time::timeout(READ_TIMEOUT, resp.chunk()) => r,
            _ = run.wait() => return Err(StreamErr::Stop),
            _ = cancel.wait() => return Err(StreamErr::Stop),
        };
        let bytes = match next {
            Err(_) => return Err(StreamErr::Net(t!("dj.stalled"))),
            Ok(Err(err)) => return Err(StreamErr::Net(t!("dj.interrupted", error = err.without_url()))),
            Ok(Ok(None)) => return Err(StreamErr::Net(t!("dj.closedEarly"))),
            Ok(Ok(Some(b))) => b,
        };
        let mut data = &bytes[..];
        if skip > 0 {
            let n = (skip as usize).min(data.len());
            data = &data[n..];
            skip -= n as u64;
        }
        if data.len() as u64 > remaining {
            data = &data[..remaining as usize];
        }
        if data.is_empty() {
            continue;
        }
        let n = data.len() as u64;
        e.rx_bytes.fetch_add(n, Ordering::Relaxed);
        e.limiter.acquire(n).await;
        buf.extend_from_slice(data);
        remaining -= n;
        if buf.len() >= FLUSH || remaining == 0 {
            let len = buf.len() as u64;
            let data = std::mem::take(&mut buf);
            let f = file.clone();
            let at = off;
            let back = tokio::task::spawn_blocking(move || write_all_at(&f, &data, at).map(|_| data))
                .await
                .map_err(|_| StreamErr::Disk(std::io::Error::other(t!("dj.writeInterrupted"))))?
                .map_err(StreamErr::Disk)?;
            buf = back;
            buf.clear();
            off += len;
            *written += len;
            e.debrid_live(id, len as i64);
            if remaining > 0 && !e.debrid_active(id) {
                return Err(StreamErr::Stop);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Estado compartido (motor)
// ---------------------------------------------------------------------------

impl Engine {
    /// Lanza el gestor de cada trabajo debrid que deba avanzar (lo llama el muestreador).
    pub fn debrid_schedule(self: &Arc<Self>) {
        let ids: Vec<String> = {
            let mut st = self.st.lock();
            if st.paused {
                return;
            }
            st.jobs
                .iter_mut()
                .filter(|j| j.status.is_download() && !j.cancel.is())
                .filter_map(|j| {
                    let d = j.debrid.as_mut()?;
                    if d.running {
                        return None;
                    }
                    d.running = true;
                    Some(j.id.clone())
                })
                .collect()
        };
        for id in ids {
            let e = self.clone();
            crate::rt::spawn(async move { drive(e, id).await });
        }
    }

    /// Modifica el estado debrid de un trabajo. None si ya no existe.
    fn debrid_mut<R>(&self, id: &str, f: impl FnOnce(&mut Job, &mut DebridJob) -> R) -> Option<R> {
        let mut st = self.st.lock();
        let j = st.jobs.iter_mut().find(|j| j.id == id)?;
        let mut d = j.debrid.take()?;
        let r = f(j, &mut d);
        j.debrid = Some(d);
        Some(r)
    }

    fn debrid_snap(&self, id: &str) -> Option<Snap> {
        let mut st = self.st.lock();
        let paused = st.paused;
        let j = st.jobs.iter_mut().find(|j| j.id == id)?;
        if paused || !j.status.is_download() || j.cancel.is() {
            return None;
        }
        if j.status == JobStatus::Queued {
            j.status = JobStatus::Downloading;
            if j.started_at.is_none() {
                j.started_at = Some(util::now_secs());
            }
        }
        let d = j.debrid.as_ref()?;
        Some(Snap {
            phase: d.phase,
            provider: d.provider.clone(),
            label: d.label.clone(),
            remote_id: d.remote_id.clone(),
            remote_hash: d.remote_hash.clone(),
            name: j.name.clone(),
            password: j.password.clone(),
            options: j.options.clone(),
            nzb_path: j.nzb_path.clone(),
            cancel: j.cancel.clone(),
        })
    }

    /// ¿Debe seguir descargándose? (no pausado, no eliminado)
    fn debrid_active(&self, id: &str) -> bool {
        let st = self.st.lock();
        !st.paused && st.jobs.iter().any(|j| j.id == id && j.debrid.is_some() && j.status.is_download() && !j.cancel.is())
    }

    fn debrid_done(&self, id: &str) -> bool {
        let st = self.st.lock();
        st.jobs.iter().any(|j| j.id == id && j.debrid.is_some() && j.download_done())
    }

    fn debrid_ready(&self, id: &str, new_id: Option<String>, files: Vec<RemoteFile>, folder: String) {
        let names = local_names(&files);
        self.debrid_mut(id, |j, d| {
            if let Some(i) = new_id {
                d.remote_id = i;
            }
            d.remote_folder = folder;
            d.phase = DebridPhase::Fetch;
            d.remote_percent = 100.0;
            d.remote_speed = 0;
            d.remote_eta = 0;
            d.notice = None;
            d.links.clear();
            j.files = files.into_iter().zip(names).map(|(f, name)| job_file(name, f.size, f.id)).collect();
        });
        let mut st = self.st.lock();
        if let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) {
            j.recount();
        }
        drop(st);
        self.dirty.store(true, Ordering::Relaxed);
    }

    fn debrid_take(&self, id: &str) -> Option<ChunkTask> {
        let mut guard = self.st.lock();
        let st = &mut *guard;
        if st.paused {
            return None;
        }
        let j = st.jobs.iter_mut().find(|j| j.id == id)?;
        if !j.status.is_download() || j.cancel.is() || j.debrid.as_ref()?.phase != DebridPhase::Fetch {
            return None;
        }
        loop {
            let (fi, si) = match j.retry.pop() {
                Some(x) => x,
                None => j.fresh.pop_front()?,
            };
            let f = &mut j.files[fi as usize];
            let Some(remote) = f.remote.clone() else { continue };
            let seg = &mut f.segments[si as usize];
            if seg.state != SegState::Pending || f.held {
                continue;
            }
            seg.state = SegState::Active;
            return Some(ChunkTask {
                fi,
                ci: si,
                offset: si as u64 * CHUNK,
                len: seg.bytes,
                remote,
                name: f.name.clone(),
            });
        }
    }

    fn debrid_live(&self, id: &str, delta: i64) {
        let mut st = self.st.lock();
        if let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) {
            j.live_bytes = j.live_bytes.saturating_add_signed(delta);
        }
    }

    fn debrid_chunk_done(&self, id: &str, t: &ChunkTask, written: u64) {
        let mut st = self.st.lock();
        let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) else { return };
        j.live_bytes = j.live_bytes.saturating_sub(written);
        let Some(seg) = j.files.get_mut(t.fi as usize).and_then(|f| f.segments.get_mut(t.ci as usize)) else { return };
        if seg.state != SegState::Active {
            return;
        }
        seg.state = SegState::Done;
        let bytes = seg.bytes;
        j.seg_done(t.fi as usize, bytes);
        drop(st);
        self.dirty.store(true, Ordering::Relaxed);
    }

    fn debrid_chunk_back(&self, id: &str, t: &ChunkTask, written: u64) {
        let mut st = self.st.lock();
        let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) else { return };
        j.live_bytes = j.live_bytes.saturating_sub(written);
        let Some(seg) = j.files.get_mut(t.fi as usize).and_then(|f| f.segments.get_mut(t.ci as usize)) else { return };
        if seg.state == SegState::Active {
            seg.state = SegState::Pending;
            j.retry.push((t.fi, t.ci));
        }
    }

    /// Archivo local abierto (y con su tamaño final reservado). None si el trabajo ya no existe.
    fn debrid_file(&self, id: &str, fi: u32) -> std::io::Result<Option<Arc<File>>> {
        let mut st = self.st.lock();
        let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) else { return Ok(None) };
        let work_dir = j.work_dir.clone();
        let Some(f) = j.files.get_mut(fi as usize) else { return Ok(None) };
        if let Some(h) = &f.handle {
            return Ok(Some(h.clone()));
        }
        let path = local_path(&work_dir, &f.name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path)?;
        if file.metadata().map(|m| m.len()).unwrap_or(0) != f.bytes {
            file.set_len(f.bytes)?;
        }
        let h = Arc::new(file);
        f.handle = Some(h.clone());
        Ok(Some(h))
    }

    /// Enlace de descarga de un archivo (se reutiliza mientras no caduque).
    async fn debrid_link(&self, id: &str, cfg: &DebridConfig, t: &ChunkTask, force: bool) -> Result<String, DebridError> {
        let cached = |e: &Engine| -> Result<Option<String>, String> {
            let st = e.st.lock();
            let d = st.jobs.iter().find(|j| j.id == id).and_then(|j| j.debrid.as_ref()).ok_or_else(String::new)?;
            if force {
                return Ok(None);
            }
            Ok(d.links.get(&t.fi).filter(|(_, at)| at.elapsed() < LINK_TTL).map(|(u, _)| u.clone()))
        };
        let stopped = || DebridError::new(ErrKind::Transient, t!("dj.notQueued"));
        // Varias conexiones empiezan a la vez con el mismo archivo: una sola petición de enlace.
        let _lock = self.debrid_links.lock().await;
        if let Some(u) = cached(self).map_err(|_| stopped())? {
            return Ok(u);
        }
        let remote_id = {
            let st = self.st.lock();
            st.jobs.iter().find(|j| j.id == id).and_then(|j| j.debrid.as_ref()).map(|d| d.remote_id.clone()).ok_or_else(stopped)?
        };
        let url = debrid::file_link(cfg, &remote_id, &t.remote).await?;
        self.debrid_mut(id, |_, d| {
            d.links.insert(t.fi, (url.clone(), Instant::now()));
        });
        Ok(url)
    }

    /// Todos los archivos están en el PC: se borra del servicio (si procede) y se post-procesa.
    fn debrid_finish(self: &Arc<Self>, id: &str, cfg: &DebridConfig, s: &Snap) {
        let delete = self.settings.read().debrid_delete_after;
        let ok = self
            .debrid_mut(id, |j, d| {
                j.status = JobStatus::Verifying;
                j.post = Some(PostInfo { stage: "waiting".into(), ..Default::default() });
                d.remote_state = t!("dj.downloadedFrom", name = s.label);
                d.notice = None;
                for f in j.files.iter_mut() {
                    f.handle = None;
                }
                (d.remote_id.clone(), d.remote_folder.clone())
            });
        let Some((rid, folder)) = ok else { return };
        linfo!("{}", t!("log.dj.downloaded", name = s.name, provider = s.label));
        if delete && !rid.is_empty() {
            let cfg = cfg.clone();
            let name = s.name.clone();
            crate::rt::spawn(async move {
                if let Err(err) = debrid::remove(&cfg, &rid, &folder).await {
                    lwarn!("{}", t!("log.dj.removeFailed", name = name, provider = cfg.label(), error = err));
                }
            });
        }
        self.save_queue();
        self.start_pp(id.to_string());
    }

    fn debrid_write_error(&self, id: &str, err: std::io::Error) {
        let disk_full = matches!(err.raw_os_error(), Some(112) | Some(39));
        if disk_full {
            lerror!("{}", t!("log.diskFull"));
            self.set_paused(true);
            self.toast("error", t!("toast.diskFull"));
            return;
        }
        let msg = t!("err.diskWrite", error = err);
        lerror!("{}", msg);
        {
            let mut st = self.st.lock();
            if let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) {
                if j.status.is_download() {
                    j.status = JobStatus::Paused;
                    j.error = Some(msg.clone());
                }
            }
        }
        self.toast("error", msg);
    }

    /// Borra del servicio la descarga de un trabajo que se ha quitado de la cola.
    pub fn debrid_forget(&self, d: &DebridJob) {
        if d.remote_id.is_empty() || !self.settings.read().debrid_delete_after {
            return;
        }
        let Some(cfg) = self.settings.read().debrid.iter().find(|c| c.id == d.provider).cloned() else { return };
        let (rid, folder) = (d.remote_id.clone(), d.remote_folder.clone());
        crate::rt::spawn(async move {
            if let Err(err) = debrid::remove(&cfg, &rid, &folder).await {
                lwarn!("{}", t!("log.dj.removeRemoteFailed", provider = cfg.label(), error = err));
            }
        });
    }

    /// Vacía la carpeta temporal antes de volver a empezar con otro método.
    async fn debrid_clear_work(&self, id: &str) {
        let dir = {
            let mut st = self.st.lock();
            let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) else { return };
            for f in j.files.iter_mut() {
                f.handle = None;
            }
            j.work_dir.clone()
        };
        let _ = tokio::task::spawn_blocking(move || util::remove_dir_retry(&dir)).await;
    }

    /// El servicio no puede con este NZB: se prueba el siguiente servicio, NNTP o se da por fallido.
    pub async fn debrid_failed(self: &Arc<Self>, id: &str, msg: String) {
        enum Then {
            Next(DebridConfig),
            Nntp,
            Fail,
        }
        let settings = self.settings.read().clone();
        let found = self.debrid_mut(id, |_, d| {
            if !d.tried.contains(&d.provider) {
                d.tried.push(d.provider.clone());
            }
            let next = if d.auto {
                settings.debrid.iter().find(|p| p.usable() && !d.tried.contains(&p.id)).cloned()
            } else {
                None
            };
            let then = match next {
                Some(p) => Then::Next(p),
                None if settings.debrid_fallback_nntp && has_nntp(&settings) => Then::Nntp,
                None => Then::Fail,
            };
            (then, d.label.clone(), d.provider.clone(), d.remote_id.clone(), d.remote_folder.clone(), d.tried.clone(), d.estimate)
        });
        let Some((then, label, provider, remote_id, folder, tried, estimate)) = found else { return };
        let name = self.st.lock().jobs.iter().find(|j| j.id == id).map(|j| j.name.clone()).unwrap_or_default();
        let msg = with_label(&msg, &label);
        // Lo que quede en el servicio ya no sirve.
        if !remote_id.is_empty() && settings.debrid_delete_after {
            if let Some(cfg) = settings.debrid.iter().find(|c| c.id == provider).cloned() {
                crate::rt::spawn(async move {
                    let _ = debrid::remove(&cfg, &remote_id, &folder).await;
                });
            }
        }
        match then {
            Then::Fail => self.fail_job(id, msg, "download", false),
            Then::Next(p) => {
                lwarn!("{}", t!("log.dj.tryNext", name = name, error = msg.trim_end_matches('.'), provider = p.label()));
                self.debrid_clear_work(id).await;
                let notice = format!("{}. {}", msg.trim_end_matches('.'), t!("dj.tryingNext", name = p.label()));
                self.debrid_mut(id, |j, d| {
                    // `running` lo libera el gestor actual al salir (evita dos gestores a la vez).
                    let running = d.running;
                    *d = DebridJob::new(&p, true, estimate);
                    d.running = running;
                    d.tried = tried;
                    j.files.clear();
                    j.warning = Some(notice);
                    j.status = JobStatus::Queued;
                });
                if let Some(j) = self.st.lock().jobs.iter_mut().find(|j| j.id == id) {
                    j.recount();
                }
                self.save_queue();
            }
            Then::Nntp => {
                lwarn!("{}", t!("log.dj.fallbackNntp", name = name, error = msg.trim_end_matches('.')));
                self.debrid_to_nntp(id, msg).await
            }
        }
    }

    /// Convierte el trabajo en una descarga NNTP normal a partir del NZB guardado.
    async fn debrid_to_nntp(self: &Arc<Self>, id: &str, msg: String) {
        let nzb_path = {
            let st = self.st.lock();
            let Some(j) = st.jobs.iter().find(|j| j.id == id) else { return };
            j.nzb_path.clone()
        };
        let parsed = tokio::fs::read(&nzb_path).await.map_err(|e| e.to_string()).and_then(|raw| nzb::parse(&raw));
        let nzb = match parsed {
            Ok(n) => n,
            Err(err) => {
                self.fail_job(id, format!("{} {}", msg, t!("dj.nntpAlsoFailed", error = err)), "download", false);
                return;
            }
        };
        self.debrid_clear_work(id).await;
        let empty = {
            let mut st = self.st.lock();
            let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) else { return };
            let fresh = Job::build(
                j.id.clone(),
                j.name.clone(),
                j.nzb_path.clone(),
                j.dest_dir.clone(),
                j.work_dir.clone(),
                j.password.clone(),
                j.options.clone(),
                &nzb,
            );
            j.files = fresh.files;
            j.debrid = None;
            j.warning = Some(format!("{}. {}", msg.trim_end_matches('.'), t!("dj.viaNntp")));
            if j.status.is_download() {
                j.status = JobStatus::Queued;
            }
            j.recount();
            j.download_done() && j.status.is_download()
        };
        self.save_queue();
        self.work.notify_waiters();
        if empty {
            self.start_pp(id.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks() {
        assert_eq!(chunk_segments(0).len(), 1);
        assert_eq!(chunk_segments(0)[0].bytes, 0);
        assert_eq!(chunk_segments(CHUNK).len(), 1);
        let s = chunk_segments(CHUNK * 2 + 5);
        assert_eq!(s.iter().map(|x| x.bytes).collect::<Vec<_>>(), vec![CHUNK, CHUNK, 5]);
    }

    #[test]
    fn names() {
        let f = |p: &str| RemoteFile { id: "1".into(), path: p.into(), size: 1 };
        let n = local_names(&[f("a.mkv"), f("A.mkv"), f("Subs/b:c.srt")]);
        assert_eq!(n, vec!["a.mkv", "A.mkv (2)", "Subs/b_c.srt"]);
        assert_eq!(local_path(Path::new("w"), "Subs/b.srt"), Path::new("w").join("Subs").join("b.srt"));
    }

    #[test]
    fn routes() {
        let tb = DebridConfig { id: "tb".into(), kind: "torbox".into(), api_key: "k".into(), ..Default::default() };
        let pm = DebridConfig { id: "pm".into(), kind: "premiumize".into(), api_key: "k".into(), ..Default::default() };
        let mut s = Settings { debrid: vec![tb, pm], ..Default::default() };
        // Sin servidores NNTP se usa el servicio debrid aunque el método por defecto sea NNTP.
        assert_eq!(resolve(&s, None).map(|(c, _)| c.id), Some("tb".into()));
        s.servers.push(crate::config::ServerConfig { host: "news.example".into(), ..Default::default() });
        assert!(resolve(&s, None).is_none());
        assert_eq!(resolve(&s, Some("pm")).map(|(c, a)| (c.id, a)), Some(("pm".into(), false)));
        assert_eq!(resolve(&s, Some("debrid")).map(|(c, a)| (c.id, a)), Some(("tb".into(), true)));
        s.download_via = "debrid".into();
        assert_eq!(resolve(&s, None).map(|(c, _)| c.id), Some("tb".into()));
        assert!(resolve(&s, Some("nntp")).is_none());
        // Un servidor NNTP concreto nunca pasa a debrid, aunque ese sea el método por defecto.
        assert!(resolve(&s, Some("nntp:s1")).is_none());
        s.servers[0].id = "s1".into();
        s.servers[0].name = "Principal".into();
        assert_eq!(crate::engine::pinned_server(&s, Some("nntp:s1")), Some(("s1".into(), "Principal".into())));
        assert_eq!(crate::engine::pinned_server(&s, Some("nntp:otro")), None);
        assert_eq!(crate::engine::pinned_server(&s, Some("nntp")), None);
        s.servers[0].enabled = false;
        assert_eq!(crate::engine::pinned_server(&s, Some("nntp:s1")), None);
        s.servers[0].enabled = true;
        s.debrid[0].enabled = false;
        assert_eq!(resolve(&s, None).map(|(c, _)| c.id), Some("pm".into()));
        assert_eq!(resolve(&s, Some("tb")).map(|(c, _)| c.id), Some("pm".into()));
        s.debrid[1].api_key.clear();
        assert!(resolve(&s, None).is_none());
    }

    #[test]
    fn pp() {
        let mut o = JobOptions::default();
        assert_eq!(pp_mode(&o), -1);
        o.delete_archives = false;
        assert_eq!(pp_mode(&o), 2);
        o.extract = false;
        assert_eq!(pp_mode(&o), 1);
        o.par2 = false;
        assert_eq!(pp_mode(&o), 0);
    }
}
