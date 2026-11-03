use crate::config::{write_atomic, AppPaths, ServerConfig, Settings};
use crate::debrid_job::{DebridJob, DebridPhase, DebridView, SavedDebrid};
use crate::nntp::{NntpConn, NntpError};
use crate::nzb::{self, Nzb};
use crate::stats::{self, IndexerEvent, ServerUse, StatRecord};
use crate::util::{self, Speedo};
use crate::yenc::YencDecoder;
use crate::{lerror, linfo, lwarn};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{Notify, Semaphore};

// ---------------------------------------------------------------------------
// Modelo
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum JobStatus {
    Queued,
    Downloading,
    Paused,
    Verifying,
    Repairing,
    Extracting,
    Moving,
    Completed,
    Failed,
}

impl JobStatus {
    pub fn is_pp(self) -> bool {
        matches!(self, JobStatus::Verifying | JobStatus::Repairing | JobStatus::Extracting | JobStatus::Moving)
    }
    pub fn is_download(self) -> bool {
        matches!(self, JobStatus::Queued | JobStatus::Downloading)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SegState {
    Pending,
    Active,
    Done,
    Failed,
}

pub struct Segment {
    pub bytes: u64,
    pub message_id: String,
    pub state: SegState,
    pub tried: u64,
    pub attempts: u32,
}

pub struct JobFile {
    pub name: String,
    pub bytes: u64,
    pub segments: Vec<Segment>,
    pub done_bytes: u64,
    pub failed_bytes: u64,
    pub failed_segs: u32,
    pub remaining: u32,
    pub held: bool,
    pub is_par2: bool,
    /// Volumen de recuperación PAR2 (queda en reserva hasta que la reparación lo pida).
    pub is_vol: bool,
    /// Bloques de recuperación según el nombre; 0 si el nombre no lo dice (ofuscado).
    pub vol_blocks: u32,
    pub complete: bool,
    pub yenc_name: Option<String>,
    pub handle: Option<Arc<File>>,
    /// Id del archivo en el servicio debrid (descargas debrid).
    pub remote: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct JobOptions {
    pub par2: bool,
    pub extract: bool,
    pub delete_archives: bool,
    pub delete_par2: bool,
    pub create_folder: bool,
}

impl Default for JobOptions {
    fn default() -> Self {
        Self { par2: true, extract: true, delete_archives: true, delete_par2: true, create_folder: true }
    }
}

#[derive(Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PostInfo {
    pub stage: String,
    pub percent: f64,
    pub speed: u64,
    pub detail: String,
}

#[derive(Default)]
pub struct Cancel {
    flag: AtomicBool,
    notify: Notify,
}

impl Cancel {
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }
    pub fn is(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
    pub async fn wait(&self) {
        let fut = self.notify.notified();
        if self.is() {
            return;
        }
        fut.await;
    }
}

pub struct Job {
    pub id: String,
    pub name: String,
    pub nzb_path: PathBuf,
    pub dest_dir: PathBuf,
    pub work_dir: PathBuf,
    pub password: Option<String>,
    pub options: JobOptions,
    pub files: Vec<JobFile>,
    pub status: JobStatus,
    pub total_bytes: u64,
    pub done_bytes: u64,
    pub failed_bytes: u64,
    pub failed_segs: u32,
    pub remaining: u32,
    pub fresh: VecDeque<(u32, u32)>,
    pub retry: Vec<(u32, u32)>,
    pub added_at: u64,
    pub started_at: Option<u64>,
    pub error: Option<String>,
    pub warning: Option<String>,
    pub post: Option<PostInfo>,
    pub speed: u64,
    pub speedo: Speedo,
    pub cancel: Arc<Cancel>,
    pub pp_only: bool,
    /// Descarga a través de un servicio debrid en lugar de NNTP.
    pub debrid: Option<DebridJob>,
    /// Bytes recibidos de fragmentos debrid aún incompletos.
    pub live_bytes: u64,
    /// Registro de estadísticas al que cuenta (el del primer intento si es un reintento).
    pub stats_id: String,
    /// Artículos servidos por cada servidor NNTP en este trabajo.
    pub server_use: Vec<ServerUse>,
    /// Indexador del que se obtuvo el NZB.
    pub indexer: Option<String>,
    /// Servidor NNTP elegido para este trabajo (id); sin él se usan todos por prioridad.
    pub server: Option<String>,
    /// Nombre de ese servidor (para el historial).
    pub server_name: Option<String>,
}

impl Job {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build(
        id: String,
        name: String,
        nzb_path: PathBuf,
        dest_dir: PathBuf,
        work_dir: PathBuf,
        password: Option<String>,
        options: JobOptions,
        nzb: &Nzb,
    ) -> Job {
        let has_main_par2 = nzb.files.iter().any(|f| f.is_par2() && !f.is_recovery_vol());
        // Sin verificación PAR2 no se descarga ningún .par2; con ella, solo el índice y
        // los volúmenes de recuperación quedan en reserva hasta que hagan falta.
        let use_par2 = options.par2;
        let mut files: Vec<JobFile> = nzb
            .files
            .iter()
            .map(|f| {
                let vb = f.vol_blocks().unwrap_or(0);
                let is_vol = f.is_recovery_vol();
                let held = is_vol || (!use_par2 && f.is_par2());
                JobFile {
                    name: f.filename.clone(),
                    bytes: f.bytes,
                    segments: f
                        .segments
                        .iter()
                        .map(|s| Segment {
                            bytes: s.bytes,
                            message_id: s.message_id.clone(),
                            state: SegState::Pending,
                            tried: 0,
                            attempts: 0,
                        })
                        .collect(),
                    done_bytes: 0,
                    failed_bytes: 0,
                    failed_segs: 0,
                    remaining: f.segments.len() as u32,
                    held,
                    is_par2: f.is_par2(),
                    is_vol,
                    vol_blocks: vb,
                    complete: false,
                    yenc_name: None,
                    handle: None,
                    remote: None,
                }
            })
            .collect();
        // Sin par2 principal: se usa el volumen más pequeño como índice.
        if use_par2 && !has_main_par2 {
            if let Some(f) = files.iter_mut().filter(|f| f.held).min_by_key(|f| f.bytes) {
                f.held = false;
            }
        }
        let mut job = Job {
            stats_id: id.clone(),
            server_use: Vec::new(),
            indexer: None,
            server: None,
            server_name: None,
            id,
            name,
            nzb_path,
            dest_dir,
            work_dir,
            password,
            options,
            files,
            status: JobStatus::Queued,
            total_bytes: 0,
            done_bytes: 0,
            failed_bytes: 0,
            failed_segs: 0,
            remaining: 0,
            fresh: VecDeque::new(),
            retry: Vec::new(),
            added_at: util::now_secs(),
            started_at: None,
            error: None,
            warning: None,
            post: None,
            speed: 0,
            speedo: Speedo::default(),
            cancel: Arc::new(Cancel::default()),
            pp_only: false,
            debrid: None,
            live_bytes: 0,
        };
        job.recount();
        job
    }

    /// Trabajo que descargará un servicio debrid. Los archivos se conocen cuando el servicio termina.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_debrid(
        id: String,
        name: String,
        nzb_path: PathBuf,
        dest_dir: PathBuf,
        work_dir: PathBuf,
        password: Option<String>,
        options: JobOptions,
        debrid: DebridJob,
    ) -> Job {
        let mut job = Job {
            stats_id: id.clone(),
            server_use: Vec::new(),
            indexer: None,
            server: None,
            server_name: None,
            id,
            name,
            nzb_path,
            dest_dir,
            work_dir,
            password,
            options,
            files: Vec::new(),
            status: JobStatus::Queued,
            total_bytes: 0,
            done_bytes: 0,
            failed_bytes: 0,
            failed_segs: 0,
            remaining: 0,
            fresh: VecDeque::new(),
            retry: Vec::new(),
            added_at: util::now_secs(),
            started_at: None,
            error: None,
            warning: None,
            post: None,
            speed: 0,
            speedo: Speedo::default(),
            cancel: Arc::new(Cancel::default()),
            pp_only: false,
            debrid: Some(debrid),
            live_bytes: 0,
        };
        job.recount();
        job
    }

    fn pp_only_job(e: &HistoryEntry, password: Option<String>) -> Job {
        Job {
            id: util::new_id(),
            name: e.name.clone(),
            nzb_path: PathBuf::from(&e.nzb_path),
            dest_dir: PathBuf::from(&e.dest_dir),
            work_dir: PathBuf::from(&e.work_dir),
            password,
            options: e.options.clone(),
            files: Vec::new(),
            status: JobStatus::Extracting,
            total_bytes: e.size,
            done_bytes: e.size,
            failed_bytes: 0,
            failed_segs: 0,
            remaining: 0,
            fresh: VecDeque::new(),
            retry: Vec::new(),
            added_at: e.added_at,
            started_at: Some(util::now_secs()),
            error: None,
            warning: None,
            post: Some(PostInfo { stage: "waiting".into(), ..Default::default() }),
            speed: 0,
            speedo: Speedo::default(),
            cancel: Arc::new(Cancel::default()),
            pp_only: true,
            debrid: None,
            live_bytes: 0,
            stats_id: e.stats_id.clone().unwrap_or_else(|| e.id.clone()),
            server_use: Vec::new(),
            indexer: e.indexer.clone(),
            server: None,
            server_name: None,
        }
    }

    /// Recalcula contadores y la cola de segmentos a partir del estado de cada segmento.
    pub(crate) fn recount(&mut self) {
        self.live_bytes = 0;
        self.total_bytes = 0;
        self.done_bytes = 0;
        self.failed_bytes = 0;
        self.failed_segs = 0;
        self.remaining = 0;
        self.fresh.clear();
        self.retry.clear();
        for (fi, f) in self.files.iter_mut().enumerate() {
            f.done_bytes = 0;
            f.failed_bytes = 0;
            f.failed_segs = 0;
            f.remaining = 0;
            for (si, s) in f.segments.iter_mut().enumerate() {
                match s.state {
                    SegState::Done => f.done_bytes += s.bytes,
                    SegState::Failed => {
                        f.failed_bytes += s.bytes;
                        f.failed_segs += 1;
                    }
                    SegState::Pending | SegState::Active => {
                        s.state = SegState::Pending;
                        f.remaining += 1;
                        if !f.held {
                            self.fresh.push_back((fi as u32, si as u32));
                        }
                    }
                }
            }
            f.complete = f.remaining == 0;
            if !f.held {
                self.total_bytes += f.bytes;
                self.done_bytes += f.done_bytes;
                self.failed_bytes += f.failed_bytes;
                self.failed_segs += f.failed_segs;
                self.remaining += f.remaining;
            }
        }
        if self.files.is_empty() {
            if let Some(d) = &self.debrid {
                self.total_bytes = d.estimate;
            }
        }
    }

    /// Ya no queda nada por descargar (en debrid, solo cuando los archivos del servicio están en el PC).
    pub(crate) fn download_done(&self) -> bool {
        self.remaining == 0 && self.debrid.as_ref().map_or(true, |d| d.phase == DebridPhase::Fetch && !self.files.is_empty())
    }

    fn take(&mut self, fi: u32, si: u32) -> Task {
        let seg = &mut self.files[fi as usize].segments[si as usize];
        seg.state = SegState::Active;
        if self.status == JobStatus::Queued {
            self.status = JobStatus::Downloading;
            if self.started_at.is_none() {
                self.started_at = Some(util::now_secs());
            }
        }
        Task {
            job_id: self.id.clone(),
            file: fi,
            seg: si,
            message_id: seg.message_id.clone(),
            bytes: seg.bytes,
        }
    }

    pub(crate) fn seg_done(&mut self, fi: usize, bytes: u64) {
        let f = &mut self.files[fi];
        f.done_bytes += bytes;
        f.remaining = f.remaining.saturating_sub(1);
        self.done_bytes += bytes;
        self.remaining = self.remaining.saturating_sub(1);
        if f.remaining == 0 {
            self.finish_file(fi);
        }
    }

    fn seg_failed(&mut self, fi: usize, bytes: u64) {
        let f = &mut self.files[fi];
        f.failed_bytes += bytes;
        f.failed_segs += 1;
        f.remaining = f.remaining.saturating_sub(1);
        self.failed_bytes += bytes;
        self.failed_segs += 1;
        self.remaining = self.remaining.saturating_sub(1);
        if f.remaining == 0 {
            self.finish_file(fi);
        }
    }

    fn finish_file(&mut self, fi: usize) {
        let work_dir = self.work_dir.clone();
        let f = &mut self.files[fi];
        f.handle = None;
        f.complete = true;
        if let Some(yn) = f.yenc_name.clone() {
            let yn = util::sanitize_filename(&yn);
            // Si yEnc trae un nombre ofuscado y el NZB uno bueno, se queda el del NZB.
            let worse = util::looks_obfuscated(&yn) && !util::looks_obfuscated(&f.name);
            if !yn.is_empty() && yn != f.name && !worse {
                let from = work_dir.join(&f.name);
                let to = work_dir.join(&yn);
                if from.exists() && !to.exists() && std::fs::rename(&from, &to).is_ok() {
                    f.name = yn;
                }
            }
        }
    }

    /// Estimación rápida: ¿faltan más datos de los que el PAR2 disponible puede reparar?
    fn is_hopeless(&self) -> bool {
        if self.failed_bytes == 0 || !self.options.par2 || !self.files.iter().any(|f| f.is_par2) {
            return false;
        }
        let capacity: u64 = self
            .files
            .iter()
            .filter(|f| f.is_vol)
            .map(|f| f.bytes.saturating_sub(f.failed_bytes))
            .sum();
        let data_failed: u64 = self.files.iter().filter(|f| !f.is_par2).map(|f| f.failed_bytes).sum();
        data_failed > capacity
    }

    fn view(&self) -> JobView {
        let processed = self.done_bytes + self.failed_bytes + self.live_bytes;
        let downloading = self.status == JobStatus::Downloading;
        let mut percent = if self.total_bytes > 0 {
            (processed as f64 / self.total_bytes as f64 * 100.0).min(100.0)
        } else {
            100.0
        };
        let left = self.total_bytes.saturating_sub(processed);
        let mut done = self.done_bytes + self.live_bytes;
        let mut speed = if downloading { self.speed } else { 0 };
        let mut eta = if self.speed > 0 && downloading { left / self.speed } else { 0 };
        let mut warning = self.warning.clone();
        let debrid = self.debrid.as_ref().map(|d| {
            // Mientras el servicio lo descarga, se muestra su progreso y su velocidad.
            // Mientras se sube el NZB, el progreso y la velocidad de la subida.
            if d.phase == DebridPhase::Submit {
                percent = d.upload_percent();
                done = 0;
                speed = if downloading { d.upload_speed } else { 0 };
                let left = d.upload_total.saturating_sub(d.upload_sent.load(Ordering::Relaxed));
                eta = if speed > 0 { left / speed } else { 0 };
            } else if d.phase != DebridPhase::Fetch {
                percent = d.remote_percent;
                done = (self.total_bytes as f64 * percent / 100.0) as u64;
                speed = if downloading { d.remote_speed } else { 0 };
                eta = if downloading { d.remote_eta } else { 0 };
            }
            if let Some(n) = &d.notice {
                warning = Some(match &warning {
                    Some(w) => format!("{} {}", w, n),
                    None => n.clone(),
                });
            }
            d.view()
        });
        JobView {
            id: self.id.clone(),
            name: self.name.clone(),
            status: self.status,
            total: self.total_bytes,
            done,
            failed: self.failed_bytes,
            failed_articles: self.failed_segs,
            percent,
            speed,
            eta,
            files: self.files.iter().filter(|f| !f.held).count(),
            files_done: self.files.iter().filter(|f| !f.held && f.complete).count(),
            held_par2: self.files.iter().filter(|f| f.held).count(),
            dest: self.dest_dir.to_string_lossy().to_string(),
            error: self.error.clone(),
            warning,
            post: self.post.clone(),
            added_at: self.added_at,
            debrid,
            server: self.server_name.clone(),
        }
    }

    fn final_dir(&self) -> PathBuf {
        if self.options.create_folder {
            self.dest_dir.join(util::sanitize_filename(&self.name))
        } else {
            self.dest_dir.clone()
        }
    }
}

pub struct Task {
    pub job_id: String,
    pub file: u32,
    pub seg: u32,
    pub message_id: String,
    pub bytes: u64,
}

pub enum Outcome {
    Ok { crc_ok: bool },
    Missing,
    Retry,
    Returned,
}

#[derive(Default)]
pub struct ConnCounters {
    pub connected: AtomicU32,
    pub active: AtomicU32,
}

pub struct ServerRt {
    pub id: String,
    pub name: String,
    pub priority: u32,
    pub max_conns: u32,
    pub counters: Arc<ConnCounters>,
    pub fails: u32,
    pub down_until: Option<Instant>,
    pub auth_failed: bool,
    pub conn_limited: bool,
    pub last_error: Option<String>,
    pub articles_ok: u64,
    pub articles_missing: u64,
    pub bytes: u64,
}

impl ServerRt {
    fn available(&self, now: Instant) -> bool {
        !self.auth_failed && self.down_until.map_or(true, |t| now >= t)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub name: String,
    pub status: String,
    pub size: u64,
    pub dest: String,
    pub dest_dir: String,
    pub work_dir: String,
    pub nzb_path: String,
    pub error: Option<String>,
    pub warning: Option<String>,
    pub failed_stage: Option<String>,
    pub can_retry_extract: bool,
    pub password: Option<String>,
    pub options: JobOptions,
    pub added_at: u64,
    pub completed_at: u64,
    pub download_secs: u64,
    pub avg_speed: u64,
    /// Método usado: "nntp" o el id del servicio debrid.
    pub via: Option<String>,
    /// Nombre del servicio debrid, si se usó.
    pub via_name: Option<String>,
    /// Registro de estadísticas de este NZB.
    pub stats_id: Option<String>,
    /// Indexador del que se obtuvo el NZB.
    pub indexer: Option<String>,
}

impl Default for HistoryEntry {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            status: "completed".into(),
            size: 0,
            dest: String::new(),
            dest_dir: String::new(),
            work_dir: String::new(),
            nzb_path: String::new(),
            error: None,
            warning: None,
            failed_stage: None,
            can_retry_extract: false,
            password: None,
            options: JobOptions::default(),
            added_at: 0,
            completed_at: 0,
            download_secs: 0,
            avg_speed: 0,
            via: None,
            via_name: None,
            stats_id: None,
            indexer: None,
        }
    }
}

pub struct PendingNzb {
    pub nzb: Nzb,
    pub raw: Vec<u8>,
    pub name: String,
    pub created: Instant,
    pub indexer: Option<String>,
}

// ---- Vistas para la interfaz ----

#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub id: String,
    pub name: String,
    pub status: JobStatus,
    pub total: u64,
    pub done: u64,
    pub failed: u64,
    pub failed_articles: u32,
    pub percent: f64,
    pub speed: u64,
    pub eta: u64,
    pub files: usize,
    pub files_done: usize,
    pub held_par2: usize,
    pub dest: String,
    pub error: Option<String>,
    pub warning: Option<String>,
    pub post: Option<PostInfo>,
    pub added_at: u64,
    pub debrid: Option<DebridView>,
    /// Servidor NNTP elegido para este trabajo, si se eligió uno.
    pub server: Option<String>,
}

#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServerView {
    pub id: String,
    pub name: String,
    pub priority: u32,
    pub connected: u32,
    pub active: u32,
    pub max_conns: u32,
    pub state: String,
    pub last_error: Option<String>,
    pub articles_ok: u64,
    pub articles_missing: u64,
    pub bytes: u64,
}

#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub paused: bool,
    pub speed: u64,
    pub remaining: u64,
    pub eta: u64,
    pub speed_limit: u64,
    pub active_conns: u32,
    pub jobs: Vec<JobView>,
    pub servers: Vec<ServerView>,
    pub no_servers: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub name: String,
    pub bytes: u64,
    pub done: u64,
    pub failed: u64,
    pub segments: usize,
    pub failed_segs: u32,
    pub held: bool,
    pub par2: bool,
    pub complete: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewFile {
    pub name: String,
    pub bytes: u64,
    pub par2: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NzbPreview {
    pub token: String,
    pub name: String,
    pub source: String,
    pub password: Option<String>,
    pub total_bytes: u64,
    pub data_bytes: u64,
    pub par2_bytes: u64,
    pub par2_blocks: u32,
    pub file_count: usize,
    pub segment_count: usize,
    pub has_archives: bool,
    pub groups: Vec<String>,
    pub oldest_post: u64,
    pub files: Vec<PreviewFile>,
    pub default_dir: String,
    pub retention_warning: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRequest {
    pub token: String,
    pub name: String,
    pub dest_dir: String,
    pub password: Option<String>,
    pub paused: bool,
    pub top: bool,
    pub options: JobOptions,
    /// "nntp", "debrid", id de un servicio debrid o nada (método por defecto).
    #[serde(default)]
    pub via: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct Toast {
    pub kind: &'static str,
    pub message: String,
}

// ---- Persistencia de la cola ----

#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct SavedQueue {
    paused: bool,
    jobs: Vec<SavedJob>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedJob {
    id: String,
    name: String,
    nzb_path: String,
    dest_dir: String,
    work_dir: String,
    password: Option<String>,
    options: JobOptions,
    status: JobStatus,
    added_at: u64,
    started_at: Option<u64>,
    #[serde(default)]
    pp_only: bool,
    #[serde(default)]
    total: u64,
    files: Vec<SavedFile>,
    #[serde(default)]
    debrid: Option<SavedDebrid>,
    #[serde(default)]
    stats_id: Option<String>,
    #[serde(default)]
    servers: Vec<ServerUse>,
    #[serde(default)]
    indexer: Option<String>,
    #[serde(default)]
    server: Option<String>,
    #[serde(default)]
    server_name: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct SavedFile {
    name: String,
    held: bool,
    done: String,
    failed: String,
    #[serde(default)]
    remote: Option<String>,
    #[serde(default)]
    bytes: u64,
}

fn bits_to_hex(bits: impl Iterator<Item = bool>) -> String {
    let mut out = String::new();
    let mut byte = 0u8;
    let mut n = 0;
    for b in bits {
        if b {
            byte |= 1 << (n % 8);
        }
        n += 1;
        if n % 8 == 0 {
            out.push_str(&format!("{:02x}", byte));
            byte = 0;
        }
    }
    if n % 8 != 0 {
        out.push_str(&format!("{:02x}", byte));
    }
    out
}

fn hex_to_bits(s: &str, len: usize) -> Vec<bool> {
    let bytes: Vec<u8> = (0..s.len() / 2).filter_map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()).collect();
    (0..len).map(|i| bytes.get(i / 8).map_or(false, |b| b & (1 << (i % 8)) != 0)).collect()
}

// ---------------------------------------------------------------------------
// Limitador de velocidad (token bucket con deuda)
// ---------------------------------------------------------------------------

pub struct RateLimiter {
    limit: AtomicU64,
    state: Mutex<(f64, Instant)>,
}

impl RateLimiter {
    fn new() -> Self {
        Self { limit: AtomicU64::new(0), state: Mutex::new((0.0, Instant::now())) }
    }
    pub fn set(&self, bytes_per_sec: u64) {
        self.limit.store(bytes_per_sec, Ordering::Relaxed);
        *self.state.lock() = (0.0, Instant::now());
    }
    pub async fn acquire(&self, n: u64) {
        let lim = self.limit.load(Ordering::Relaxed);
        if lim == 0 {
            return;
        }
        let wait = {
            let mut s = self.state.lock();
            let now = Instant::now();
            let elapsed = now.duration_since(s.1).as_secs_f64();
            s.1 = now;
            s.0 = (s.0 + elapsed * lim as f64).min(lim as f64 * 0.5);
            s.0 -= n as f64;
            if s.0 < 0.0 {
                Some(Duration::from_secs_f64((-s.0 / lim as f64).min(30.0)))
            } else {
                None
            }
        };
        if let Some(w) = wait {
            tokio::time::sleep(w).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Motor
// ---------------------------------------------------------------------------

pub struct State {
    pub jobs: Vec<Job>,
    pub servers: Vec<ServerRt>,
    pub paused: bool,
    pub history: Vec<HistoryEntry>,
    pub pending: HashMap<String, PendingNzb>,
    speed: u64,
    speedo: Speedo,
    last_snapshot: Option<Snapshot>,
}

pub struct Engine {
    pub app: AppHandle,
    pub paths: AppPaths,
    pub settings: RwLock<Settings>,
    pub st: Mutex<State>,
    pub work: Notify,
    pub gen: AtomicU64,
    pub rx_bytes: AtomicU64,
    pub limiter: RateLimiter,
    pub pp_sem: Semaphore,
    /// Descargas debrid bajando archivos a la vez (una: se termina antes cada trabajo).
    pub debrid_fetch: Semaphore,
    /// Serializa las peticiones de enlaces de descarga debrid.
    pub debrid_links: tokio::sync::Mutex<()>,
    pub dirty: AtomicBool,
    pub ui_ready: AtomicBool,
    pub pending_open: Mutex<Vec<OpenItem>>,
    pub stats: Mutex<Vec<StatRecord>>,
    pub indexer_stats: Mutex<Vec<IndexerEvent>>,
    pub indexer_dirty: AtomicBool,
    /// Acciones al terminar: recuento de la cola y cuenta atrás de apagado.
    pub finish: crate::finish::Runtime,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OpenItem {
    pub kind: String,
    pub value: String,
}

/// Servidor NNTP pedido con `via` = "nntp:<id>", si existe y está activo: (id, nombre).
pub fn pinned_server(s: &Settings, via: Option<&str>) -> Option<(String, String)> {
    let id = via?.trim().strip_prefix("nntp:")?;
    s.servers
        .iter()
        .find(|x| x.id == id && x.enabled && !x.host.trim().is_empty())
        .map(|x| (x.id.clone(), if x.name.trim().is_empty() { x.host.clone() } else { x.name.clone() }))
}

/// Índice (en `servers`) del servidor al que está limitado un trabajo, si sigue disponible en la lista.
fn pin_index(servers: &[ServerRt], job: &Job) -> Option<usize> {
    let id = job.server.as_deref()?;
    servers.iter().position(|r| r.id == id).filter(|i| *i < 64)
}

fn avail_mask(servers: &[ServerRt]) -> u64 {
    let now = Instant::now();
    servers
        .iter()
        .enumerate()
        .filter(|(_, r)| r.available(now))
        .fold(0u64, |m, (i, _)| m | (1 << i))
}

/// (máscara disponibles, máscara de servidores con más prioridad, ¿es del nivel principal?)
fn eligibility(servers: &[ServerRt], s: usize) -> Option<(u64, u64, bool)> {
    let now = Instant::now();
    let me = servers.get(s)?;
    if !me.available(now) {
        return None;
    }
    let mut avail = 0u64;
    let mut higher = 0u64;
    let mut min_prio = u32::MAX;
    for (i, r) in servers.iter().enumerate() {
        if r.available(now) {
            avail |= 1 << i;
            if r.priority < me.priority {
                higher |= 1 << i;
            }
            min_prio = min_prio.min(r.priority);
        }
    }
    Some((avail, higher, me.priority == min_prio))
}

#[cfg(unix)]
pub(crate) fn write_all_at(f: &File, buf: &[u8], off: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(f, buf, off)
}

#[cfg(windows)]
pub(crate) fn write_all_at(f: &File, mut buf: &[u8], mut off: u64) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        let n = f.seek_write(buf, off)?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::WriteZero, t!("err.emptyWrite")));
        }
        buf = &buf[n..];
        off += n as u64;
    }
    Ok(())
}

impl Engine {
    pub fn new(app: AppHandle, paths: AppPaths, settings: Settings) -> Arc<Self> {
        let limiter = RateLimiter::new();
        limiter.set(settings.speed_limit_kbps * 1024);
        Arc::new(Engine {
            app,
            paths,
            settings: RwLock::new(settings),
            st: Mutex::new(State {
                jobs: Vec::new(),
                servers: Vec::new(),
                paused: false,
                history: Vec::new(),
                pending: HashMap::new(),
                speed: 0,
                speedo: Speedo::default(),
                last_snapshot: None,
            }),
            work: Notify::new(),
            gen: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            limiter,
            pp_sem: Semaphore::new(1),
            debrid_fetch: Semaphore::new(1),
            debrid_links: tokio::sync::Mutex::new(()),
            dirty: AtomicBool::new(false),
            ui_ready: AtomicBool::new(false),
            pending_open: Mutex::new(Vec::new()),
            stats: Mutex::new(Vec::new()),
            indexer_stats: Mutex::new(Vec::new()),
            indexer_dirty: AtomicBool::new(false),
            finish: Default::default(),
        })
    }

    pub fn start(self: &Arc<Self>) {
        self.load_history();
        let stats = stats::load(&self.paths, &self.st.lock().history);
        *self.stats.lock() = stats;
        *self.indexer_stats.lock() = stats::load_indexer(&self.paths);
        let pp = self.load_queue();
        self.restart_workers();
        for id in pp {
            self.start_pp(id);
        }
        let e = self.clone();
        tauri::async_runtime::spawn(async move { e.sampler().await });
        crate::api::restart(self);
        crate::remote::restart(self);
        linfo!("{}", t!("log.started", version = env!("CARGO_PKG_VERSION")));
    }

    pub fn toast(&self, kind: &'static str, message: impl Into<String>) {
        let _ = self.app.emit("toast", Toast { kind, message: message.into() });
    }

    pub fn notify_os(&self, title: &str, body: &str) {
        crate::os::notify(&self.app, &self.paths.tools, title, body);
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(self.settings.read().timeout_secs.clamp(5, 300))
    }

    // ---- Workers ----

    pub fn restart_workers(self: &Arc<Self>) {
        let gen = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        let settings = self.settings.read().clone();
        let servers: Vec<ServerConfig> = settings
            .servers
            .iter()
            .filter(|s| s.enabled && !s.host.trim().is_empty())
            .take(64)
            .cloned()
            .collect();
        {
            let mut st = self.st.lock();
            let old: HashMap<String, (u64, u64, u64)> = st
                .servers
                .iter()
                .map(|r| (r.id.clone(), (r.articles_ok, r.articles_missing, r.bytes)))
                .collect();
            st.servers = servers
                .iter()
                .map(|s| {
                    let (ok, miss, bytes) = old.get(&s.id).copied().unwrap_or((0, 0, 0));
                    ServerRt {
                        id: s.id.clone(),
                        name: s.label(),
                        priority: s.priority,
                        max_conns: s.connections.clamp(1, 200),
                        counters: Arc::new(ConnCounters::default()),
                        fails: 0,
                        down_until: None,
                        auth_failed: false,
                        conn_limited: false,
                        last_error: None,
                        articles_ok: ok,
                        articles_missing: miss,
                        bytes,
                    }
                })
                .collect();
            // Los índices de servidor han podido cambiar: reiniciar los intentos.
            for job in st.jobs.iter_mut() {
                for f in job.files.iter_mut() {
                    for s in f.segments.iter_mut() {
                        if s.state == SegState::Pending {
                            s.tried = 0;
                        }
                    }
                }
            }
        }
        let counters: Vec<Arc<ConnCounters>> = self.st.lock().servers.iter().map(|r| r.counters.clone()).collect();
        for (idx, srv) in servers.into_iter().enumerate() {
            for _ in 0..srv.connections.clamp(1, 200) {
                let e = self.clone();
                let cfg = srv.clone();
                let c = counters[idx].clone();
                tauri::async_runtime::spawn(async move { worker(e, cfg, idx, gen, c).await });
            }
        }
        self.work.notify_waiters();
    }

    pub fn next_task(&self, s: usize) -> Option<Task> {
        let mut guard = self.st.lock();
        let st = &mut *guard;
        if st.paused {
            return None;
        }
        let (_avail, higher, top) = eligibility(&st.servers, s)?;
        let bit = 1u64 << s;
        let servers = &st.servers;
        for job in st.jobs.iter_mut() {
            if !job.status.is_download() || job.debrid.is_some() {
                continue;
            }
            // Un trabajo limitado a un servidor solo lo descarga ese servidor, sea cual sea su prioridad.
            let pinned = pin_index(servers, job);
            if pinned.is_some_and(|p| p != s) {
                continue;
            }
            let (higher, top) = if pinned.is_some() { (0, true) } else { (higher, top) };
            let mut i = 0;
            while i < job.retry.len() {
                let (fi, si) = job.retry[i];
                let seg = &job.files[fi as usize].segments[si as usize];
                if seg.state != SegState::Pending {
                    job.retry.swap_remove(i);
                    continue;
                }
                if seg.tried & bit == 0 && seg.tried & higher == higher {
                    job.retry.swap_remove(i);
                    return Some(job.take(fi, si));
                }
                i += 1;
            }
            if top {
                while let Some((fi, si)) = job.fresh.pop_front() {
                    let f = &job.files[fi as usize];
                    if f.held || f.segments[si as usize].state != SegState::Pending {
                        continue;
                    }
                    return Some(job.take(fi, si));
                }
            }
        }
        None
    }

    pub fn complete(self: &Arc<Self>, t: &Task, s: usize, gen: u64, outcome: Outcome) {
        let (max_retries, abort_hopeless) = {
            let st = self.settings.read();
            (st.max_retries, st.abort_hopeless)
        };
        let stale = gen != self.gen.load(Ordering::SeqCst);
        let outcome = match outcome {
            Outcome::Missing | Outcome::Retry if stale => Outcome::Returned,
            o => o,
        };
        let mut start_pp = None;
        let mut hopeless = None;
        {
            let mut guard = self.st.lock();
            let st = &mut *guard;
            let mut counted = None;
            if !stale {
                if let Some(r) = st.servers.get_mut(s) {
                    match outcome {
                        Outcome::Ok { .. } => {
                            r.articles_ok += 1;
                            r.bytes += t.bytes;
                            counted = Some(true);
                        }
                        Outcome::Missing => {
                            r.articles_missing += 1;
                            counted = Some(false);
                        }
                        _ => {}
                    }
                }
            }
            let avail = avail_mask(&st.servers);
            let Some(job) = st.jobs.iter_mut().find(|j| j.id == t.job_id) else { return };
            let avail = match pin_index(&st.servers, job) {
                Some(p) => 1u64 << p,
                None => avail,
            };
            if let (Some(ok), Some(r)) = (counted, st.servers.get(s)) {
                stats::count(&mut job.server_use, &r.id, &r.name, ok, t.bytes);
            }
            let fi = t.file as usize;
            let si = t.seg as usize;
            let bit = if s < 64 { 1u64 << s } else { 0 };
            let seg = &mut job.files[fi].segments[si];
            if seg.state != SegState::Active {
                return;
            }
            enum Next {
                Done,
                Failed,
                Requeue,
            }
            let next = match outcome {
                Outcome::Ok { crc_ok } => {
                    if !crc_ok {
                        seg.tried |= bit;
                        if seg.tried & avail != avail {
                            lwarn!("{}", t!("log.badCrc", id = t.message_id));
                            Next::Requeue
                        } else {
                            Next::Done
                        }
                    } else {
                        Next::Done
                    }
                }
                Outcome::Missing => {
                    seg.tried |= bit;
                    seg.attempts = 0;
                    if seg.tried & avail == avail {
                        Next::Failed
                    } else {
                        Next::Requeue
                    }
                }
                Outcome::Retry => {
                    seg.attempts += 1;
                    if seg.attempts > max_retries {
                        seg.tried |= bit;
                        seg.attempts = 0;
                    }
                    if avail != 0 && seg.tried & avail == avail {
                        Next::Failed
                    } else {
                        Next::Requeue
                    }
                }
                Outcome::Returned => Next::Requeue,
            };
            match next {
                Next::Done => {
                    seg.state = SegState::Done;
                    job.seg_done(fi, t.bytes);
                }
                Next::Failed => {
                    seg.state = SegState::Failed;
                    job.seg_failed(fi, t.bytes);
                }
                Next::Requeue => {
                    seg.state = SegState::Pending;
                    job.retry.push((t.file, t.seg));
                }
            }
            if job.status.is_download() {
                if abort_hopeless && job.is_hopeless() {
                    hopeless = Some(job.id.clone());
                } else if job.remaining == 0 {
                    job.status = JobStatus::Verifying;
                    job.post = Some(PostInfo { stage: "waiting".into(), ..Default::default() });
                    start_pp = Some(job.id.clone());
                }
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
        if matches!(outcome, Outcome::Returned) {
            self.work.notify_one();
        }
        if let Some(id) = hopeless {
            let msg = t!("err.hopeless");
            lerror!("{}", t!("log.hopeless", id = id));
            self.fail_job(&id, msg, "download", false);
        }
        if let Some(id) = start_pp {
            self.start_pp(id);
        }
    }

    pub fn file_handle(&self, t: &Task, dec: &YencDecoder) -> Result<Option<Arc<File>>, String> {
        let mut st = self.st.lock();
        let Some(job) = st.jobs.iter_mut().find(|j| j.id == t.job_id) else { return Ok(None) };
        let work_dir = job.work_dir.clone();
        let f = &mut job.files[t.file as usize];
        if f.yenc_name.is_none() {
            f.yenc_name = dec.name.clone();
        }
        if let Some(h) = &f.handle {
            return Ok(Some(h.clone()));
        }
        std::fs::create_dir_all(&work_dir)
            .map_err(|e| t!("err.createTemp", path = work_dir.display(), error = e))?;
        let path = work_dir.join(&f.name);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| t!("err.create", path = path.display(), error = e))?;
        if let Some(sz) = dec.total_size {
            if file.metadata().map(|m| m.len()).unwrap_or(0) < sz {
                let _ = file.set_len(sz);
            }
        }
        let h = Arc::new(file);
        f.handle = Some(h.clone());
        Ok(Some(h))
    }

    pub fn write_error(self: &Arc<Self>, t: &Task, s: usize, gen: u64, err: std::io::Error) {
        self.complete(t, s, gen, Outcome::Returned);
        let disk_full = matches!(err.raw_os_error(), Some(112) | Some(39));
        if disk_full {
            lerror!("{}", t!("log.diskFull"));
            self.set_paused(true);
            self.toast("error", t!("toast.diskFull"));
        } else {
            let msg = t!("err.diskWrite", error = err);
            lerror!("{}", msg);
            let mut st = self.st.lock();
            if let Some(job) = st.jobs.iter_mut().find(|j| j.id == t.job_id) {
                if job.status.is_download() {
                    job.status = JobStatus::Paused;
                    job.error = Some(msg.clone());
                }
            }
            drop(st);
            self.toast("error", msg);
        }
    }

    pub fn server_ok(&self, s: usize) {
        let mut st = self.st.lock();
        if let Some(r) = st.servers.get_mut(s) {
            if r.last_error.is_some() {
                linfo!("{}", t!("log.reconnected", name = r.name));
            }
            r.fails = 0;
            r.down_until = None;
            r.last_error = None;
        }
    }

    pub fn server_error(&self, s: usize, gen: u64, e: &NntpError) -> Duration {
        if gen != self.gen.load(Ordering::SeqCst) {
            return Duration::from_secs(1);
        }
        let msg = e.message();
        let mut toast = None;
        let wait = {
            let mut st = self.st.lock();
            let Some(r) = st.servers.get_mut(s) else { return Duration::from_secs(5) };
            let changed = r.last_error.as_deref() != Some(msg.as_str());
            if !matches!(e, NntpError::TooMany(_)) {
                r.last_error = Some(msg.clone());
            }
            match e {
                NntpError::Auth(_) => {
                    if !r.auth_failed {
                        lerror!("{}: {}", r.name, msg);
                        toast = Some(format!("{}: {}", r.name, msg));
                    }
                    r.auth_failed = true;
                    Duration::from_secs(10)
                }
                NntpError::TooMany(_) => {
                    if !r.conn_limited {
                        r.conn_limited = true;
                        lwarn!("{}", t!("log.connLimit", name = r.name, error = msg));
                    }
                    Duration::from_secs(30)
                }
                _ => {
                    r.fails += 1;
                    if changed {
                        lwarn!("{}: {}", r.name, msg);
                    }
                    if r.fails >= 3 {
                        let secs = (10u64 << (r.fails - 3).min(5)).min(300);
                        r.down_until = Some(Instant::now() + Duration::from_secs(secs));
                        if r.fails == 3 {
                            toast = Some(t!("toast.cantConnect", name = r.name, error = msg));
                        }
                    }
                    Duration::from_secs(3)
                }
            }
        };
        if let Some(t) = toast {
            self.toast("error", t);
        }
        wait
    }

    // ---- Cola ----

    pub fn register_pending(&self, raw: Vec<u8>, name_hint: &str, strong_hint: bool, source: String) -> Result<NzbPreview, String> {
        let nzb = nzb::parse(&raw)?;
        let hint = name_hint
            .trim()
            .trim_end_matches(".gz")
            .trim_end_matches(".GZ")
            .trim_end_matches(".nzb")
            .trim_end_matches(".NZB")
            .trim()
            .to_string();
        let fallback = || {
            nzb.files
                .iter()
                .find(|f| !f.is_par2())
                .map(|f| f.filename.split('.').next().unwrap_or(&f.filename).to_string())
                .unwrap_or_else(|| t!("job.defaultName"))
        };
        let raw_name = if strong_hint && !hint.is_empty() {
            hint.clone()
        } else if let Some(t) = &nzb.title {
            t.clone()
        } else if !hint.is_empty() {
            hint.clone()
        } else {
            fallback()
        };
        let (name, pw_in_name) = util::split_password(&raw_name);
        let name = util::sanitize_filename(&name);
        let password = nzb.password.clone().or(pw_in_name);

        let settings = self.settings.read().clone();
        let par2_bytes: u64 = nzb.files.iter().filter(|f| f.is_par2()).map(|f| f.bytes).sum();
        let total: u64 = nzb.files.iter().map(|f| f.bytes).sum();
        let mut groups: Vec<String> = Vec::new();
        for f in &nzb.files {
            for g in &f.groups {
                if !groups.contains(g) && groups.len() < 6 {
                    groups.push(g.clone());
                }
            }
        }
        let oldest = nzb.files.iter().map(|f| f.date).filter(|d| *d > 0).min().unwrap_or(0);
        let retention_warning = if oldest > 0 {
            let age_days = util::now_secs().saturating_sub(oldest) / 86400;
            let ret = settings.servers.iter().filter(|s| s.enabled).map(|s| s.retention_days).max().unwrap_or(0);
            if ret > 0 && age_days as u32 > ret {
                Some(t!("warn.retention", age = age_days, days = ret))
            } else {
                None
            }
        } else {
            None
        };
        let archive_re = regex::Regex::new(r"(?i)\.(rar|r\d{2}|7z|zip|\d{3})$").unwrap();
        let preview = NzbPreview {
            token: util::new_id(),
            name: name.clone(),
            source: source.clone(),
            password,
            total_bytes: total,
            data_bytes: total - par2_bytes,
            par2_bytes,
            par2_blocks: nzb.files.iter().filter_map(|f| f.vol_blocks()).sum(),
            file_count: nzb.files.len(),
            segment_count: nzb.files.iter().map(|f| f.segments.len()).sum(),
            has_archives: nzb.files.iter().any(|f| archive_re.is_match(&f.filename)),
            groups,
            oldest_post: oldest,
            files: nzb
                .files
                .iter()
                .take(400)
                .map(|f| PreviewFile { name: f.filename.clone(), bytes: f.bytes, par2: f.is_par2() })
                .collect(),
            default_dir: settings.download_dir.clone(),
            retention_warning,
        };
        self.st
            .lock()
            .pending
            .insert(preview.token.clone(), PendingNzb { nzb, raw, name, created: Instant::now(), indexer: None });
        Ok(preview)
    }

    /// Anota de qué indexador viene un NZB pendiente de añadir.
    pub fn set_pending_indexer(&self, token: &str, indexer: String) {
        if let Some(p) = self.st.lock().pending.get_mut(token) {
            p.indexer = Some(indexer);
        }
    }

    /// NZB ya descargado de una vista previa (para guardarlo sin volver a pedirlo).
    pub fn pending_nzb(&self, token: &str) -> Option<(Vec<u8>, String)> {
        let st = self.st.lock();
        st.pending.get(token).map(|p| (p.raw.clone(), p.name.clone()))
    }

    pub fn discard_pending(&self, token: &str) {
        self.st.lock().pending.remove(token);
    }

    fn work_dir_for(&self, dest: &Path, name: &str, id: &str) -> PathBuf {
        let inc = self.settings.read().incomplete_dir.trim().to_string();
        let base = if inc.is_empty() { dest.join("_incompleto") } else { PathBuf::from(inc) };
        let dir = base.join(name);
        if dir.exists() {
            base.join(format!("{}.{}", name, &id[..6]))
        } else {
            dir
        }
    }

    pub fn add_job(self: &Arc<Self>, req: AddRequest) -> Result<String, String> {
        let pending = self
            .st
            .lock()
            .pending
            .remove(&req.token)
            .ok_or_else(|| t!("err.previewExpiredReAdd"))?;
        let dest = if req.dest_dir.trim().is_empty() {
            PathBuf::from(self.settings.read().download_dir.clone())
        } else {
            PathBuf::from(req.dest_dir.trim())
        };
        std::fs::create_dir_all(&dest)
            .map_err(|e| t!("err.createDest", path = dest.display(), error = e))?;
        let id = util::new_id();
        let nzb_path = self.paths.nzb.join(format!("{}.nzb", id));
        std::fs::write(&nzb_path, &pending.raw).map_err(|e| t!("err.saveNzb", error = e))?;
        let name = if req.name.trim().is_empty() { pending.name.clone() } else { util::sanitize_filename(req.name.trim()) };
        let work_dir = self.work_dir_for(&dest, &name, &id);
        let password = req.password.filter(|p| !p.trim().is_empty());
        let via = crate::debrid_job::resolve(&self.settings.read(), req.via.as_deref());
        let via_label = via.as_ref().map(|(c, _)| format!(" ({})", t!("log.via", name = c.label()))).unwrap_or_default();
        let mut job = match via {
            Some((cfg, auto)) => {
                let estimate = pending.nzb.files.iter().filter(|f| !f.is_par2()).map(|f| f.bytes).sum();
                Job::new_debrid(id.clone(), name.clone(), nzb_path, dest, work_dir, password, req.options, DebridJob::new(&cfg, auto, estimate))
            }
            None => Job::build(id.clone(), name.clone(), nzb_path, dest, work_dir, password, req.options, &pending.nzb),
        };
        let pin = if job.debrid.is_none() { pinned_server(&self.settings.read(), req.via.as_deref()) } else { None };
        let via_label = match &pin {
            Some((_, n)) => format!(" ({})", t!("log.via", name = n)),
            None => via_label,
        };
        if let Some((sid, sname)) = pin {
            job.server = Some(sid);
            job.server_name = Some(sname);
        }
        if req.paused {
            job.status = JobStatus::Paused;
        }
        job.indexer = pending.indexer.clone();
        let empty = job.download_done();
        {
            let mut st = self.st.lock();
            if req.top {
                let pos = st.jobs.iter().position(|j| !j.status.is_pp()).unwrap_or(st.jobs.len());
                st.jobs.insert(pos, job);
            } else {
                st.jobs.push(job);
            }
        }
        linfo!("{}{}", t!("log.added", name = name), via_label);
        crate::finish::on_job_added(self);
        self.save_queue();
        self.work.notify_waiters();
        self.debrid_schedule();
        if empty && !req.paused {
            self.start_pp(id.clone());
        }
        Ok(id)
    }

    pub fn set_paused(&self, paused: bool) {
        self.st.lock().paused = paused;
        if !paused {
            self.work.notify_waiters();
        }
        linfo!("{}", if paused { t!("log.queuePaused") } else { t!("log.queueResumed") });
        crate::update_tray_pause(&self.app, paused);
        self.save_queue();
    }

    pub fn pause_job(&self, id: &str) {
        let mut st = self.st.lock();
        if let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) {
            if j.status.is_download() {
                j.status = JobStatus::Paused;
                j.speedo.reset();
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn resume_job(self: &Arc<Self>, id: &str) {
        let mut start = false;
        {
            let mut st = self.st.lock();
            if let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) {
                if j.status == JobStatus::Paused {
                    j.error = None;
                    if j.download_done() {
                        j.status = JobStatus::Verifying;
                        start = true;
                    } else {
                        j.status = JobStatus::Queued;
                    }
                }
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
        self.work.notify_waiters();
        self.debrid_schedule();
        if start {
            self.start_pp(id.to_string());
        }
    }

    pub fn remove_job(&self, id: &str, delete_files: bool) {
        let job = {
            let mut st = self.st.lock();
            let Some(pos) = st.jobs.iter().position(|j| j.id == id) else { return };
            st.jobs.remove(pos)
        };
        job.cancel.cancel();
        if let Some(d) = &job.debrid {
            self.debrid_forget(d);
        }
        linfo!("{}", t!("log.removed", name = job.name));
        let work = job.work_dir.clone();
        let nzb = job.nzb_path.clone();
        drop(job);
        if delete_files {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(500));
                util::remove_dir_retry(&work);
                if let Some(parent) = work.parent() {
                    let _ = std::fs::remove_dir(parent);
                }
                let _ = std::fs::remove_file(nzb);
            });
        }
        self.save_queue();
    }

    pub fn move_job(&self, id: &str, to: usize) {
        let mut st = self.st.lock();
        let Some(pos) = st.jobs.iter().position(|j| j.id == id) else { return };
        let job = st.jobs.remove(pos);
        let to = to.min(st.jobs.len());
        st.jobs.insert(to, job);
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn job_files(&self, id: &str) -> Vec<FileView> {
        let st = self.st.lock();
        st.jobs
            .iter()
            .find(|j| j.id == id)
            .map(|j| {
                j.files
                    .iter()
                    .map(|f| FileView {
                        name: f.name.clone(),
                        bytes: f.bytes,
                        done: f.done_bytes,
                        failed: f.failed_bytes,
                        segments: f.segments.len(),
                        failed_segs: f.failed_segs,
                        held: f.held,
                        par2: f.is_par2,
                        complete: f.complete,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Libera volúmenes PAR2 retenidos para cubrir `needed` bloques. Devuelve false si no hay suficientes.
    /// `block_size` (el que informa par2) sirve para estimar los bloques de los volúmenes ofuscados,
    /// cuyo nombre no dice cuántos traen; sin él se liberan todos los retenidos.
    pub fn unhold_vols(&self, id: &str, needed: u32, block_size: Option<u64>) -> bool {
        let mut st = self.st.lock();
        let Some(job) = st.jobs.iter_mut().find(|j| j.id == id) else { return false };
        let mut held: Vec<usize> = (0..job.files.len()).filter(|&i| job.files[i].held && job.files[i].is_vol).collect();
        let blocks_of = |f: &JobFile| -> Option<u32> {
            if f.vol_blocks > 0 {
                return Some(f.vol_blocks);
            }
            // Estimación a la baja: cada bloque va en un paquete con 68 bytes de cabecera, y el
            // tamaño del NZB incluye la sobrecarga de yEnc. Si se queda corta, par2 pedirá más.
            let bs = block_size.filter(|&b| b > 0)?;
            Some(((f.bytes * 97 / 100) / (bs + 68)).max(1).min(u32::MAX as u64) as u32)
        };
        let estimates: Vec<Option<u32>> = held.iter().map(|&i| blocks_of(&job.files[i])).collect();
        let exact = held.iter().all(|&i| job.files[i].vol_blocks > 0);
        let available: u32 = estimates.iter().map(|b| b.unwrap_or(0)).fold(0u32, |a, b| a.saturating_add(b));
        if held.is_empty() || (exact && available < needed) {
            return false;
        }
        let target = (needed + needed / 20 + 1).min(available);
        let chosen: Vec<usize> = if estimates.iter().any(|b| b.is_none()) || available < needed {
            // Sin forma de saber cuántos bloques traen: se bajan todos.
            held.clone()
        } else {
            let est = |i: usize| blocks_of(&job.files[i]).unwrap_or(0);
            held.sort_by_key(|&i| est(i));
            // Primero el volumen más pequeño que cubra todo; si no existe, se acumulan.
            if let Some(&one) = held.iter().find(|&&i| est(i) >= target) {
                vec![one]
            } else {
                let mut acc = 0;
                let mut v = Vec::new();
                for &i in held.iter().rev() {
                    if acc >= target {
                        break;
                    }
                    acc += est(i);
                    v.push(i);
                }
                v
            }
        };
        let mut blocks = 0u32;
        for i in chosen {
            blocks = blocks.saturating_add(blocks_of(&job.files[i]).unwrap_or(0));
            let f = &mut job.files[i];
            f.held = false;
            job.total_bytes += f.bytes;
            job.remaining += f.remaining;
            for (si, s) in f.segments.iter().enumerate() {
                if s.state == SegState::Pending {
                    job.fresh.push_back((i as u32, si as u32));
                }
            }
        }
        linfo!("{}", t!("log.par2Extra", name = job.name, n = blocks));
        job.status = JobStatus::Queued;
        job.post = None;
        drop(st);
        self.dirty.store(true, Ordering::Relaxed);
        self.work.notify_waiters();
        true
    }

    // ---- Post-procesado ----

    pub fn start_pp(self: &Arc<Self>, id: String) {
        {
            let mut st = self.st.lock();
            if let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) {
                if !j.status.is_pp() {
                    j.status = JobStatus::Verifying;
                }
                if j.post.is_none() {
                    j.post = Some(PostInfo { stage: "waiting".into(), ..Default::default() });
                }
            }
        }
        let e = self.clone();
        tauri::async_runtime::spawn(async move { crate::postproc::run(e, id).await });
    }

    pub fn pp_context(&self, id: &str) -> Option<crate::postproc::PpCtx> {
        let st = self.st.lock();
        let j = st.jobs.iter().find(|j| j.id == id)?;
        Some(crate::postproc::PpCtx {
            id: j.id.clone(),
            name: j.name.clone(),
            work_dir: j.work_dir.clone(),
            final_dir: j.final_dir(),
            password: j.password.clone(),
            options: j.options.clone(),
            pp_only: j.pp_only,
            failed_segs: j.failed_segs,
            total: j.total_bytes,
            cancel: j.cancel.clone(),
            tools: self.paths.tools.clone(),
        })
    }

    pub fn set_stage(&self, id: &str, status: JobStatus, stage: &str, percent: f64, speed: u64, detail: &str) {
        let mut st = self.st.lock();
        if let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) {
            j.status = status;
            j.post = Some(PostInfo {
                stage: stage.to_string(),
                percent: percent.clamp(0.0, 100.0),
                speed,
                detail: detail.to_string(),
            });
        }
    }

    fn history_from(job: &Job, status: &str) -> HistoryEntry {
        let now = util::now_secs();
        let secs = job.started_at.map(|s| now.saturating_sub(s)).unwrap_or(0);
        HistoryEntry {
            id: job.id.clone(),
            name: job.name.clone(),
            status: status.into(),
            size: job.total_bytes,
            dest: job.final_dir().to_string_lossy().to_string(),
            dest_dir: job.dest_dir.to_string_lossy().to_string(),
            work_dir: job.work_dir.to_string_lossy().to_string(),
            nzb_path: job.nzb_path.to_string_lossy().to_string(),
            error: None,
            warning: job.warning.clone(),
            failed_stage: None,
            can_retry_extract: false,
            password: job.password.clone(),
            options: job.options.clone(),
            added_at: job.added_at,
            completed_at: now,
            download_secs: secs,
            avg_speed: if secs > 0 { job.done_bytes / secs } else { 0 },
            via: Some(match (&job.debrid, &job.server) {
                (Some(d), _) => d.provider.clone(),
                (None, Some(srv)) => format!("nntp:{}", srv),
                (None, None) => "nntp".into(),
            }),
            via_name: job.debrid.as_ref().map(|d| d.label.clone()).or_else(|| job.server_name.clone()),
            stats_id: Some(job.stats_id.clone()),
            indexer: job.indexer.clone(),
        }
    }

    fn push_history(&self, entry: HistoryEntry) {
        let limit = self.settings.read().history_limit.max(10) as usize;
        let removed: Vec<HistoryEntry> = {
            let mut st = self.st.lock();
            st.history.insert(0, entry);
            if st.history.len() > limit {
                st.history.split_off(limit)
            } else {
                Vec::new()
            }
        };
        for r in removed {
            let _ = std::fs::remove_file(&r.nzb_path);
        }
        self.save_history();
        let _ = self.app.emit("history", ());
        crate::remote::publish_history();
    }

    pub fn finish_job(&self, id: &str, warning: Option<String>) {
        let job = {
            let mut st = self.st.lock();
            let Some(pos) = st.jobs.iter().position(|j| j.id == id) else { return };
            st.jobs.remove(pos)
        };
        let mut entry = Self::history_from(&job, "completed");
        entry.warning = warning.clone().or(entry.warning);
        linfo!("{}{}", t!("log.completed", name = job.name), warning.as_ref().map(|w| format!(" ({})", t!("log.warning", detail = w))).unwrap_or_default());
        if self.settings.read().notify_complete {
            self.notify_os(&t!("notify.completed"), &job.name);
        }
        self.toast("success", t!("log.completed", name = job.name));
        self.record_stats(&job, &entry);
        crate::finish::on_job(self, &job, &entry, true);
        self.push_history(entry);
        self.save_queue();
    }

    pub fn fail_job(&self, id: &str, error: String, stage: &str, retry_extract: bool) {
        let job = {
            let mut st = self.st.lock();
            let Some(pos) = st.jobs.iter().position(|j| j.id == id) else { return };
            st.jobs.remove(pos)
        };
        job.cancel.cancel();
        let mut entry = Self::history_from(&job, "failed");
        entry.error = Some(error.clone());
        entry.failed_stage = Some(stage.into());
        entry.can_retry_extract = retry_extract;
        lerror!("{}", t!("log.failed", name = job.name, error = error));
        if self.settings.read().notify_failed {
            self.notify_os(&t!("notify.failed"), &format!("{}: {}", job.name, error));
        }
        self.toast("error", format!("{}: {}", job.name, error));
        self.record_stats(&job, &entry);
        crate::finish::on_job(self, &job, &entry, false);
        self.push_history(entry);
        self.save_queue();
    }

    // ---- Estadísticas ----

    fn record_stats(&self, job: &Job, entry: &HistoryEntry) {
        let rec = StatRecord::from_history(entry, job.server_use.clone());
        let mut list = self.stats.lock();
        stats::upsert(&mut list, rec);
        stats::save(&self.paths, &list);
    }

    pub fn stats(&self) -> Vec<StatRecord> {
        self.stats.lock().clone()
    }

    /// Anota una petición a un indexador (se guarda en disco cada pocos segundos).
    pub fn record_indexer(&self, ev: IndexerEvent) {
        stats::push_indexer(&mut self.indexer_stats.lock(), ev);
        self.indexer_dirty.store(true, Ordering::Relaxed);
    }

    pub fn indexer_stats(&self) -> Vec<IndexerEvent> {
        self.indexer_stats.lock().clone()
    }

    fn save_indexer_stats(&self) {
        if self.indexer_dirty.swap(false, Ordering::Relaxed) {
            let events = self.indexer_stats.lock().clone();
            stats::save_indexer(&self.paths, &events);
        }
    }

    // ---- Historial ----

    pub fn history(&self) -> Vec<HistoryEntry> {
        self.st.lock().history.clone()
    }

    pub fn remove_history(&self, id: &str, delete_files: bool) {
        let entry = {
            let mut st = self.st.lock();
            let Some(pos) = st.history.iter().position(|h| h.id == id) else { return };
            st.history.remove(pos)
        };
        let _ = std::fs::remove_file(&entry.nzb_path);
        if delete_files && entry.status == "failed" {
            let w = PathBuf::from(&entry.work_dir);
            std::thread::spawn(move || util::remove_dir_retry(&w));
        }
        self.save_history();
    }

    pub fn clear_history(&self, only_completed: bool) {
        let removed: Vec<HistoryEntry> = {
            let mut st = self.st.lock();
            let (keep, rem): (Vec<_>, Vec<_>) =
                st.history.drain(..).partition(|h| only_completed && h.status != "completed");
            st.history = keep;
            rem
        };
        for r in removed {
            let _ = std::fs::remove_file(&r.nzb_path);
        }
        self.save_history();
    }

    pub fn retry_history(self: &Arc<Self>, id: &str) -> Result<String, String> {
        let entry = {
            let st = self.st.lock();
            st.history.iter().find(|h| h.id == id).cloned().ok_or_else(|| t!("err.historyNotFound"))?
        };
        let raw = std::fs::read(&entry.nzb_path).map_err(|_| t!("err.nzbGone"))?;
        let nzb = nzb::parse(&raw)?;
        let new_id = util::new_id();
        let via = crate::debrid_job::resolve(&self.settings.read(), entry.via.as_deref());
        let mut job = match via {
            Some((cfg, auto)) => Job::new_debrid(
                new_id.clone(),
                entry.name.clone(),
                PathBuf::from(&entry.nzb_path),
                PathBuf::from(&entry.dest_dir),
                PathBuf::from(&entry.work_dir),
                entry.password.clone(),
                entry.options.clone(),
                DebridJob::new(&cfg, auto, nzb.files.iter().filter(|f| !f.is_par2()).map(|f| f.bytes).sum()),
            ),
            None => Job::build(
                new_id.clone(),
                entry.name.clone(),
                PathBuf::from(&entry.nzb_path),
                PathBuf::from(&entry.dest_dir),
                PathBuf::from(&entry.work_dir),
                entry.password.clone(),
                entry.options.clone(),
                &nzb,
            ),
        };
        job.added_at = util::now_secs();
        job.stats_id = entry.stats_id.clone().unwrap_or_else(|| entry.id.clone());
        job.indexer = entry.indexer.clone();
        if job.debrid.is_none() {
            if let Some((sid, sname)) = pinned_server(&self.settings.read(), entry.via.as_deref()) {
                job.server = Some(sid);
                job.server_name = Some(sname);
            }
        }
        {
            let mut st = self.st.lock();
            st.history.retain(|h| h.id != id);
            st.jobs.push(job);
        }
        linfo!("{}", t!("log.retrying", name = entry.name));
        crate::finish::on_job_added(self);
        self.save_history();
        self.save_queue();
        self.work.notify_waiters();
        self.debrid_schedule();
        Ok(new_id)
    }

    pub fn retry_extract(self: &Arc<Self>, id: &str, password: Option<String>) -> Result<(), String> {
        let entry = {
            let st = self.st.lock();
            st.history.iter().find(|h| h.id == id).cloned().ok_or_else(|| t!("err.historyNotFound"))?
        };
        if !Path::new(&entry.work_dir).exists() {
            return Err(t!("err.tempGone"));
        }
        let pw = password.filter(|p| !p.is_empty()).or(entry.password.clone());
        let job = Job::pp_only_job(&entry, pw);
        let jid = job.id.clone();
        {
            let mut st = self.st.lock();
            st.history.retain(|h| h.id != id);
            st.jobs.insert(0, job);
        }
        linfo!("{}", t!("log.retryExtract", name = entry.name));
        self.save_history();
        self.save_queue();
        self.start_pp(jid);
        Ok(())
    }

    // ---- Instantáneas / muestreo ----

    fn snapshot_tick(&self) -> Snapshot {
        let total_rx = self.rx_bytes.load(Ordering::Relaxed);
        let limit = self.settings.read().speed_limit_kbps;
        let mut st = self.st.lock();
        st.speedo.push(total_rx);
        st.speed = st.speedo.rate();
        let paused = st.paused;
        for j in st.jobs.iter_mut() {
            if j.status == JobStatus::Downloading && !paused {
                j.speedo.push(j.done_bytes + j.failed_bytes + j.live_bytes);
                j.speed = j.speedo.rate();
            } else {
                j.speedo.reset();
                j.speed = 0;
            }
        }
        let remaining: u64 = st
            .jobs
            .iter()
            .filter(|j| j.status.is_download())
            .map(|j| j.total_bytes.saturating_sub(j.done_bytes + j.failed_bytes + j.live_bytes))
            .sum();
        let speed = if paused { 0 } else { st.speed };
        let now = Instant::now();
        let servers: Vec<ServerView> = st
            .servers
            .iter()
            .map(|r| {
                let connected = r.counters.connected.load(Ordering::Relaxed);
                let active = r.counters.active.load(Ordering::Relaxed);
                let state = if r.auth_failed {
                    "auth"
                } else if !r.available(now) {
                    "down"
                } else if r.last_error.is_some() {
                    "warn"
                } else if active > 0 {
                    "active"
                } else if connected > 0 {
                    "idle"
                } else {
                    "ready"
                };
                ServerView {
                    id: r.id.clone(),
                    name: r.name.clone(),
                    priority: r.priority,
                    connected,
                    active,
                    max_conns: r.max_conns,
                    state: state.into(),
                    last_error: r.last_error.clone(),
                    articles_ok: r.articles_ok,
                    articles_missing: r.articles_missing,
                    bytes: r.bytes,
                }
            })
            .collect();
        Snapshot {
            paused,
            speed,
            remaining,
            eta: if speed > 0 { remaining / speed } else { 0 },
            speed_limit: limit,
            active_conns: servers.iter().map(|s| s.active).sum(),
            no_servers: st.servers.is_empty() || avail_mask(&st.servers) == 0,
            jobs: st.jobs.iter().map(|j| j.view()).collect(),
            servers,
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let last = self.st.lock().last_snapshot.clone();
        last.unwrap_or_else(|| self.snapshot_tick())
    }

    /// Falla segmentos que ya han probado todos los servidores disponibles.
    fn sweep(self: &Arc<Self>) {
        let mut to_pp = Vec::new();
        {
            let mut guard = self.st.lock();
            let st = &mut *guard;
            let avail = avail_mask(&st.servers);
            if avail == 0 {
                return;
            }
            for job in st.jobs.iter_mut().filter(|j| j.status.is_download() && j.debrid.is_none()) {
                let avail = match pin_index(&st.servers, job) {
                    Some(p) => 1u64 << p,
                    None => avail,
                };
                let mut i = 0;
                while i < job.retry.len() {
                    let (fi, si) = job.retry[i];
                    let (state, tried, bytes) = {
                        let s = &job.files[fi as usize].segments[si as usize];
                        (s.state, s.tried, s.bytes)
                    };
                    if state == SegState::Pending && tried & avail == avail {
                        job.retry.swap_remove(i);
                        job.files[fi as usize].segments[si as usize].state = SegState::Failed;
                        job.seg_failed(fi as usize, bytes);
                    } else {
                        i += 1;
                    }
                }
                if job.remaining == 0 && job.status.is_download() {
                    job.status = JobStatus::Verifying;
                    to_pp.push(job.id.clone());
                }
            }
            let cutoff = Instant::now() - Duration::from_secs(3600);
            st.pending.retain(|_, p| p.created > cutoff);
        }
        for id in to_pp {
            self.start_pp(id);
        }
    }

    async fn sampler(self: Arc<Self>) {
        let mut iv = tokio::time::interval(Duration::from_millis(500));
        let mut tick: u64 = 0;
        loop {
            iv.tick().await;
            tick += 1;
            let snap = self.snapshot_tick();
            let changed = {
                let mut st = self.st.lock();
                if st.last_snapshot.as_ref() != Some(&snap) {
                    st.last_snapshot = Some(snap.clone());
                    true
                } else {
                    false
                }
            };
            if changed {
                let _ = self.app.emit("queue", &snap);
                crate::remote::publish_queue(&snap);
            }
            self.debrid_schedule();
            if tick % 4 == 0 {
                let active = snap.jobs.iter().filter(|j| j.status.is_download() || j.status.is_pp()).count();
                let tip = if snap.paused {
                    format!("Nebula — {}", t!("tray.paused", n = active))
                } else if snap.speed > 0 {
                    format!("Nebula — {}/s · {}", util::fmt_bytes(snap.speed), t!("tray.queued", n = active))
                } else {
                    format!("Nebula — {}", t!("tray.queued", n = active))
                };
                crate::os::queue_status(&self.app, if snap.paused { 0 } else { active }, tip);
                self.sweep();
            }
            if tick % 20 == 0 && self.dirty.swap(false, Ordering::Relaxed) {
                self.save_queue();
            }
            if tick % 20 == 0 {
                self.save_indexer_stats();
            }
        }
    }

    // ---- Persistencia ----

    pub fn save_queue(&self) {
        let saved = {
            let st = self.st.lock();
            SavedQueue {
                paused: st.paused,
                jobs: st
                    .jobs
                    .iter()
                    .map(|j| SavedJob {
                        id: j.id.clone(),
                        name: j.name.clone(),
                        nzb_path: j.nzb_path.to_string_lossy().to_string(),
                        dest_dir: j.dest_dir.to_string_lossy().to_string(),
                        work_dir: j.work_dir.to_string_lossy().to_string(),
                        password: j.password.clone(),
                        options: j.options.clone(),
                        status: j.status,
                        added_at: j.added_at,
                        started_at: j.started_at,
                        pp_only: j.pp_only,
                        total: j.total_bytes,
                        debrid: j.debrid.as_ref().map(|d| d.saved()),
                        stats_id: Some(j.stats_id.clone()),
                        servers: j.server_use.clone(),
                        indexer: j.indexer.clone(),
                        server: j.server.clone(),
                        server_name: j.server_name.clone(),
                        files: j
                            .files
                            .iter()
                            .map(|f| SavedFile {
                                name: f.name.clone(),
                                held: f.held,
                                done: bits_to_hex(f.segments.iter().map(|s| s.state == SegState::Done)),
                                failed: bits_to_hex(f.segments.iter().map(|s| s.state == SegState::Failed)),
                                remote: f.remote.clone(),
                                bytes: f.bytes,
                            })
                            .collect(),
                    })
                    .collect(),
            }
        };
        match serde_json::to_vec(&saved) {
            Ok(data) => {
                if let Err(e) = write_atomic(&self.paths.queue_file(), &data) {
                    lerror!("{}", e);
                }
            }
            Err(e) => lerror!("{}", t!("log.queueSerialize", error = e)),
        }
    }

    /// Carga la cola guardada. Devuelve los trabajos que deben reanudar su post-procesado.
    fn load_queue(&self) -> Vec<String> {
        let Ok(data) = std::fs::read(self.paths.queue_file()) else { return Vec::new() };
        let Ok(saved) = serde_json::from_slice::<SavedQueue>(&data) else {
            lerror!("{}", t!("log.queueCorrupt"));
            return Vec::new();
        };
        let mut pp = Vec::new();
        let mut jobs = Vec::new();
        for sj in saved.jobs {
            if sj.pp_only {
                let entry = HistoryEntry {
                    name: sj.name.clone(),
                    size: sj.total,
                    dest_dir: sj.dest_dir.clone(),
                    work_dir: sj.work_dir.clone(),
                    nzb_path: sj.nzb_path.clone(),
                    options: sj.options.clone(),
                    added_at: sj.added_at,
                    stats_id: sj.stats_id.clone(),
                    indexer: sj.indexer.clone(),
                    ..Default::default()
                };
                let mut job = Job::pp_only_job(&entry, sj.password.clone());
                job.id = sj.id.clone();
                pp.push(job.id.clone());
                jobs.push(job);
                continue;
            }
            if let Some(sd) = sj.debrid {
                let fetch = sd.phase == DebridPhase::Fetch;
                let mut job = Job::new_debrid(
                    sj.id.clone(),
                    sj.name.clone(),
                    PathBuf::from(&sj.nzb_path),
                    PathBuf::from(&sj.dest_dir),
                    PathBuf::from(&sj.work_dir),
                    sj.password.clone(),
                    sj.options.clone(),
                    DebridJob::from_saved(sd),
                );
                job.added_at = sj.added_at;
                job.started_at = sj.started_at;
                job.stats_id = sj.stats_id.clone().unwrap_or_else(|| sj.id.clone());
                job.server_use = sj.servers.clone();
                job.indexer = sj.indexer.clone();
                if fetch {
                    job.files = sj
                        .files
                        .iter()
                        .map(|sf| {
                            let mut f = crate::debrid_job::job_file(sf.name.clone(), sf.bytes, sf.remote.clone().unwrap_or_default());
                            let n = f.segments.len();
                            let done = hex_to_bits(&sf.done, n);
                            for (i, s) in f.segments.iter_mut().enumerate() {
                                if done[i] {
                                    s.state = SegState::Done;
                                }
                            }
                            f
                        })
                        .collect();
                }
                job.recount();
                if sj.status.is_pp() || (job.download_done() && sj.status != JobStatus::Paused) {
                    job.status = JobStatus::Verifying;
                    pp.push(job.id.clone());
                } else if sj.status == JobStatus::Paused {
                    job.status = JobStatus::Paused;
                }
                jobs.push(job);
                continue;
            }
            let nzb = match std::fs::read(&sj.nzb_path).map_err(|e| e.to_string()).and_then(|r| nzb::parse(&r)) {
                Ok(n) => n,
                Err(e) => {
                    lerror!("{}", t!("log.restoreFailed", name = sj.name, error = e));
                    continue;
                }
            };
            let mut job = Job::build(
                sj.id.clone(),
                sj.name.clone(),
                PathBuf::from(&sj.nzb_path),
                PathBuf::from(&sj.dest_dir),
                PathBuf::from(&sj.work_dir),
                sj.password.clone(),
                sj.options.clone(),
                &nzb,
            );
            job.added_at = sj.added_at;
            job.started_at = sj.started_at;
            job.stats_id = sj.stats_id.clone().unwrap_or_else(|| sj.id.clone());
            job.server_use = sj.servers.clone();
            job.indexer = sj.indexer.clone();
            job.server = sj.server.clone();
            job.server_name = sj.server_name.clone();
            if sj.files.len() == job.files.len() {
                for (f, sf) in job.files.iter_mut().zip(sj.files.iter()) {
                    let n = f.segments.len();
                    let done = hex_to_bits(&sf.done, n);
                    let failed = hex_to_bits(&sf.failed, n);
                    for (i, s) in f.segments.iter_mut().enumerate() {
                        if done[i] {
                            s.state = SegState::Done;
                        } else if failed[i] {
                            s.state = SegState::Failed;
                        }
                    }
                    f.name = sf.name.clone();
                    f.held = sf.held;
                }
                job.recount();
            }
            if sj.status.is_pp() || (job.download_done() && sj.status != JobStatus::Paused) {
                job.status = JobStatus::Verifying;
                pp.push(job.id.clone());
            } else if sj.status == JobStatus::Paused {
                job.status = JobStatus::Paused;
            }
            jobs.push(job);
        }
        let mut st = self.st.lock();
        st.paused = saved.paused;
        st.jobs = jobs;
        drop(st);
        if saved.paused {
            crate::update_tray_pause(&self.app, true);
        }
        pp
    }

    pub fn save_history(&self) {
        let data = {
            let st = self.st.lock();
            serde_json::to_vec(&st.history)
        };
        if let Ok(d) = data {
            if let Err(e) = write_atomic(&self.paths.history_file(), &d) {
                lerror!("{}", e);
            }
        }
    }

    fn load_history(&self) {
        if let Ok(data) = std::fs::read(self.paths.history_file()) {
            if let Ok(h) = serde_json::from_slice::<Vec<HistoryEntry>>(&data) {
                self.st.lock().history = h;
            }
        }
    }

    pub fn shutdown(&self) {
        self.gen.fetch_add(1, Ordering::SeqCst);
        {
            let st = self.st.lock();
            for j in &st.jobs {
                j.cancel.cancel();
            }
        }
        self.save_queue();
        self.save_history();
        self.save_indexer_stats();
        linfo!("{}", t!("log.closed"));
    }

    // ---- Apertura de archivos / enlaces externos ----

    pub fn open_items(&self, items: Vec<OpenItem>) {
        if items.is_empty() {
            return;
        }
        if self.ui_ready.load(Ordering::SeqCst) {
            let _ = self.app.emit("open-items", items);
        } else {
            self.pending_open.lock().extend(items);
        }
    }

    pub fn apply_settings(self: &Arc<Self>, new: Settings) -> Result<(), String> {
        for s in &new.servers {
            if s.enabled && s.host.trim().is_empty() {
                return Err(t!("err.serverNoHost"));
            }
        }
        let old = self.settings.read().clone();
        new.save(&self.paths)?;
        if old.associate_nzb != new.associate_nzb {
            if let Err(e) = crate::os::set_association(new.associate_nzb) {
                lerror!("{}", t!("err.association", error = e));
                self.toast("error", t!("err.association", error = e));
            }
        }
        if old.start_with_windows != new.start_with_windows {
            if let Err(e) = crate::os::set_autostart(new.start_with_windows) {
                lerror!("{}", t!("err.autostart", error = e));
            }
        }
        self.limiter.set(new.speed_limit_kbps * 1024);
        let servers_changed = serde_json::to_string(&old.servers).ok() != serde_json::to_string(&new.servers).ok()
            || old.timeout_secs != new.timeout_secs;
        let api_changed = old.api_enabled != new.api_enabled || old.api_port != new.api_port || old.api_lan != new.api_lan;
        let remote_changed = old.remote_enabled != new.remote_enabled || old.remote_port != new.remote_port;
        *self.settings.write() = new;
        if api_changed {
            crate::api::restart(self);
        }
        if remote_changed {
            crate::remote::restart(self);
        }
        if servers_changed {
            linfo!("{}", t!("log.serversUpdated"));
            self.restart_workers();
        }
        Ok(())
    }

    pub fn set_speed_limit(&self, kbps: u64) -> Result<(), String> {
        let mut s = self.settings.read().clone();
        s.speed_limit_kbps = kbps;
        s.save(&self.paths)?;
        *self.settings.write() = s;
        self.limiter.set(kbps * 1024);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Worker: una conexión NNTP
// ---------------------------------------------------------------------------

async fn worker(e: Arc<Engine>, cfg: ServerConfig, sidx: usize, gen: u64, counters: Arc<ConnCounters>) {
    let mut conn: Option<NntpConn> = None;
    let mut last_used = Instant::now();
    let mut dec = YencDecoder::new();
    loop {
        if e.gen.load(Ordering::SeqCst) != gen {
            break;
        }
        let Some(task) = e.next_task(sidx) else {
            if conn.is_some() && last_used.elapsed() > Duration::from_secs(40) {
                if let Some(c) = conn.take() {
                    c.quit().await;
                    counters.connected.fetch_sub(1, Ordering::Relaxed);
                }
            }
            let _ = tokio::time::timeout(Duration::from_millis(1000), e.work.notified()).await;
            continue;
        };
        last_used = Instant::now();
        let idle = e.timeout();
        if conn.is_none() {
            match NntpConn::connect(&cfg, idle).await {
                Ok(c) => {
                    conn = Some(c);
                    counters.connected.fetch_add(1, Ordering::Relaxed);
                    e.server_ok(sidx);
                }
                Err(err) => {
                    e.complete(&task, sidx, gen, Outcome::Returned);
                    let wait = e.server_error(sidx, gen, &err);
                    // Límite de conexiones del servidor: si otras conexiones funcionan, esta se retira.
                    if matches!(err, NntpError::TooMany(_)) && counters.connected.load(Ordering::Relaxed) > 0 {
                        break;
                    }
                    tokio::time::sleep(wait).await;
                    continue;
                }
            }
        }
        let Some(c) = conn.as_mut() else { continue };
        dec.reset();
        counters.active.fetch_add(1, Ordering::Relaxed);
        let res = c.body(&task.message_id, &mut dec).await;
        counters.active.fetch_sub(1, Ordering::Relaxed);
        match res {
            Ok(raw) => {
                e.rx_bytes.fetch_add(raw, Ordering::Relaxed);
                e.limiter.acquire(raw).await;
                match dec.finish() {
                    Ok(crc_ok) => {
                        let handle = match e.file_handle(&task, &dec) {
                            Ok(Some(h)) => h,
                            Ok(None) => continue,
                            Err(msg) => {
                                e.write_error(&task, sidx, gen, std::io::Error::other(msg));
                                continue;
                            }
                        };
                        let data = std::mem::take(&mut dec.data);
                        let offset = dec.offset();
                        let r = tokio::task::spawn_blocking(move || write_all_at(&handle, &data, offset).map(|_| data)).await;
                        match r {
                            Ok(Ok(buf)) => {
                                dec.data = buf;
                                e.complete(&task, sidx, gen, Outcome::Ok { crc_ok });
                            }
                            Ok(Err(err)) => e.write_error(&task, sidx, gen, err),
                            Err(_) => e.complete(&task, sidx, gen, Outcome::Returned),
                        }
                    }
                    Err(_) => {
                        lwarn!("{}", t!("log.badYenc", id = task.message_id));
                        e.complete(&task, sidx, gen, Outcome::Missing);
                    }
                }
            }
            Err(NntpError::NotFound) => e.complete(&task, sidx, gen, Outcome::Missing),
            Err(err) => {
                conn = None;
                counters.connected.fetch_sub(1, Ordering::Relaxed);
                let wait = match &err {
                    NntpError::Timeout | NntpError::Io(_) => {
                        e.complete(&task, sidx, gen, Outcome::Retry);
                        Duration::from_millis(500)
                    }
                    _ => {
                        e.complete(&task, sidx, gen, Outcome::Returned);
                        e.server_error(sidx, gen, &err)
                    }
                };
                if matches!(err, NntpError::TooMany(_)) && counters.connected.load(Ordering::Relaxed) > 0 {
                    break;
                }
                tokio::time::sleep(wait).await;
            }
        }
    }
    if let Some(c) = conn.take() {
        c.quit().await;
        counters.connected.fetch_sub(1, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Prueba de servidor
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub ok: bool,
    pub message: String,
    pub latency_ms: u64,
}

pub async fn test_server(cfg: ServerConfig) -> TestResult {
    let start = Instant::now();
    match NntpConn::connect(&cfg, Duration::from_secs(20)).await {
        Ok(mut c) => {
            let latency = start.elapsed().as_millis() as u64;
            let _ = c.date().await;
            let greeting = c.greeting.clone();
            c.quit().await;
            TestResult {
                ok: true,
                message: format!(
                    "{}{}",
                    t!("server.testOk"),
                    if greeting.is_empty() { String::new() } else { format!(": {}", greeting.chars().take(90).collect::<String>()) }
                ),
                latency_ms: latency,
            }
        }
        Err(err) => TestResult { ok: false, message: err.message(), latency_ms: start.elapsed().as_millis() as u64 },
    }
}

impl Engine {
    pub fn show_main(&self) {
        if let Some(w) = self.app.get_webview_window("main") {
            let _ = w.show();
            #[cfg(desktop)]
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
    }
}
