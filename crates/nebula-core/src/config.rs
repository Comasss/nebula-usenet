use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone)]
pub struct AppPaths {
    pub data: PathBuf,
    pub nzb: PathBuf,
    pub logs: PathBuf,
    pub tools: PathBuf,
}

impl AppPaths {
    #[cfg(not(target_os = "android"))]
    pub fn init() -> Self {
        // NEBULA_DATA_DIR permite una carpeta de datos alternativa (modo portable / pruebas).
        let data = std::env::var_os("NEBULA_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("Nebula"));
        let local = dirs::data_local_dir().unwrap_or_else(|| data.clone()).join("Nebula");
        Self::with(data, local.join("tools").join(env!("CARGO_PKG_VERSION")))
    }

    /// Con la carpeta de datos y la de herramientas dadas. En Android la de datos la da el
    /// sistema y las herramientas (7-Zip, par2) vienen dentro del APK como bibliotecas nativas,
    /// que es lo único que Android deja ejecutar.
    pub fn with(data: PathBuf, tools: PathBuf) -> Self {
        let p = Self {
            nzb: data.join("nzb"),
            logs: data.join("logs"),
            tools,
            data,
        };
        for d in [&p.data, &p.nzb, &p.logs, &p.tools] {
            let _ = std::fs::create_dir_all(d);
        }
        crate::secrets::init(&p.data);
        p
    }
    pub fn config_file(&self) -> PathBuf {
        self.data.join("config.json")
    }
    pub fn queue_file(&self) -> PathBuf {
        self.data.join("queue.json")
    }
    pub fn history_file(&self) -> PathBuf {
        self.data.join("history.json")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ServerConfig {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub ssl: bool,
    pub verify_cert: bool,
    pub username: String,
    pub password: String,
    /// Contraseña cifrada con DPAPI (solo en disco).
    pub password_enc: String,
    pub connections: u32,
    /// 0 = máxima prioridad (principal). Los números mayores son servidores de respaldo.
    pub priority: u32,
    pub enabled: bool,
    /// Días de retención del servidor (0 = desconocido/ilimitado).
    pub retention_days: u32,
    /// Cuota de descarga en GB (0 = sin cuota).
    pub quota_gb: f64,
    /// Cuándo se reinicia la cuota: "day", "week", "month" o "never".
    pub quota_period: String,
    /// Día del reinicio: del mes (1-28) o de la semana (1 = lunes … 7 = domingo).
    pub quota_reset_day: u32,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            id: crate::util::new_id(),
            name: String::new(),
            host: String::new(),
            port: 563,
            ssl: true,
            verify_cert: true,
            username: String::new(),
            password: String::new(),
            password_enc: String::new(),
            connections: 10,
            priority: 0,
            enabled: true,
            retention_days: 0,
            quota_gb: 0.0,
            quota_period: "month".into(),
            quota_reset_day: 1,
        }
    }
}

impl ServerConfig {
    pub fn label(&self) -> String {
        if self.name.trim().is_empty() {
            self.host.clone()
        } else {
            self.name.clone()
        }
    }

    /// Cuota en bytes (0 = sin cuota).
    pub fn quota_bytes(&self) -> u64 {
        if self.quota_gb > 0.0 && self.quota_gb.is_finite() {
            (self.quota_gb * 1024.0 * 1024.0 * 1024.0) as u64
        } else {
            0
        }
    }
}

/// Tarea del programador: a una hora de ciertos días, pausar, reanudar o cambiar el límite de velocidad.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ScheduleRule {
    pub id: String,
    pub enabled: bool,
    /// Días de la semana (1 = lunes … 7 = domingo). Vacío = todos.
    pub days: Vec<u8>,
    /// "HH:MM", hora local.
    pub time: String,
    /// "pause", "resume" o "speed".
    pub action: String,
    /// KB/s para "speed" (0 = sin límite).
    pub value: u64,
}

impl Default for ScheduleRule {
    fn default() -> Self {
        Self { id: crate::util::new_id(), enabled: true, days: Vec::new(), time: "00:00".into(), action: "pause".into(), value: 0 }
    }
}

/// Indexador Newznab (NZBGeek, NZBFinder, DrunkenSlug…).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct IndexerConfig {
    pub id: String,
    pub name: String,
    pub url: String,
    pub api_key: String,
    /// API key cifrada con DPAPI (solo en disco).
    pub api_key_enc: String,
    pub api_path: String,
    pub enabled: bool,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            id: crate::util::new_id(),
            name: String::new(),
            url: String::new(),
            api_key: String::new(),
            api_key_enc: String::new(),
            api_path: "/api".into(),
            enabled: true,
        }
    }
}

impl IndexerConfig {
    pub fn label(&self) -> String {
        if self.name.trim().is_empty() {
            self.url.trim().trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/').to_string()
        } else {
            self.name.clone()
        }
    }
}

/// Servicio debrid que descarga NZB en sus servidores (TorBox Pro, Premiumize).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DebridConfig {
    pub id: String,
    /// "torbox" o "premiumize".
    pub kind: String,
    pub name: String,
    pub api_key: String,
    /// API key cifrada con DPAPI (solo en disco).
    pub api_key_enc: String,
    pub enabled: bool,
}

impl Default for DebridConfig {
    fn default() -> Self {
        Self {
            id: crate::util::new_id(),
            kind: "torbox".into(),
            name: String::new(),
            api_key: String::new(),
            api_key_enc: String::new(),
            enabled: true,
        }
    }
}

impl DebridConfig {
    pub fn label(&self) -> String {
        if self.name.trim().is_empty() {
            crate::debrid::kind_label(&self.kind).to_string()
        } else {
            self.name.clone()
        }
    }
    /// Activo y con API key: se puede usar para descargar.
    pub fn usable(&self) -> bool {
        self.enabled && !self.api_key.trim().is_empty() && crate::debrid::is_known(&self.kind)
    }
}

/// Feed RSS. Los de los indexadores no se guardan hasta que se cambian: cada indexador tiene el
/// suyo con id `ix-<id del indexador>` (ver `rss::feeds`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RssFeed {
    pub id: String,
    pub name: String,
    /// URL de un feed suelto (vacía en los de indexador).
    pub url: String,
    /// Id del indexador si es su feed de novedades.
    pub indexer: String,
    /// Minutos entre consultas (mínimo 5).
    pub interval_min: u32,
    pub enabled: bool,
}

impl Default for RssFeed {
    fn default() -> Self {
        Self {
            id: crate::util::new_id(),
            name: String::new(),
            url: String::new(),
            indexer: String::new(),
            interval_min: crate::rss::DEFAULT_INTERVAL,
            enabled: true,
        }
    }
}

/// Acción que se ejecuta al terminar una descarga, al fallar o al quedarse vacía la cola (ver finish.rs).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FinishAction {
    pub id: String,
    /// "discord", "telegram", "ntfy", "webhook", "script" o "power".
    pub kind: String,
    pub name: String,
    pub enabled: bool,
    pub on_complete: bool,
    pub on_failed: bool,
    pub on_queue_empty: bool,
    /// URL del webhook (Discord / genérico) o del servidor ntfy.
    pub url: String,
    /// URL cifrada (solo en disco).
    pub url_enc: String,
    /// Token del bot de Telegram o token de acceso de ntfy (opcional).
    pub token: String,
    /// Token cifrado (solo en disco).
    pub token_enc: String,
    /// Chat de Telegram.
    pub chat_id: String,
    /// Tema de ntfy.
    pub topic: String,
    /// Script o programa a ejecutar.
    pub path: String,
    /// Tiempo máximo del script, en segundos.
    pub timeout_secs: u32,
    /// Energía: "shutdown", "sleep" o "hibernate".
    pub power: String,
    /// Energía: se desactiva sola al dispararse.
    pub once: bool,
}

impl Default for FinishAction {
    fn default() -> Self {
        Self {
            id: crate::util::new_id(),
            kind: "discord".into(),
            name: String::new(),
            enabled: true,
            on_complete: true,
            on_failed: true,
            on_queue_empty: false,
            url: String::new(),
            url_enc: String::new(),
            token: String::new(),
            token_enc: String::new(),
            chat_id: String::new(),
            topic: String::new(),
            path: String::new(),
            timeout_secs: 120,
            power: "shutdown".into(),
            once: false,
        }
    }
}

/// Regla de descarga automática por RSS.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RssRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// Feeds en los que aplica (vacío = todos).
    pub feeds: Vec<String>,
    /// Palabras que deben aparecer todas, o una expresión regular si `regex`.
    pub must: String,
    pub regex: bool,
    /// Palabras que descartan la release si aparece cualquiera.
    pub exclude: String,
    /// Tamaño en MB (0 = sin límite).
    pub min_size_mb: u64,
    pub max_size_mb: u64,
    /// Categorías separadas por comas: números Newznab (5000, 2040…) o texto ("TV", "Movies").
    pub categories: String,
    /// "" = método predeterminado, "nntp" o el id de un servicio debrid.
    pub via: String,
    /// Carpeta de destino (vacía = la de la categoría). No viaja a otros dispositivos.
    pub dest_dir: String,
    /// Categoría (vacía = según las categorías Newznab del elemento, o Default).
    pub category: String,
    /// Fecha de creación (unix): por defecto solo se coge lo publicado después.
    pub created_at: u64,
    pub only_new: bool,
}

impl Default for RssRule {
    fn default() -> Self {
        Self {
            id: crate::util::new_id(),
            name: String::new(),
            enabled: true,
            feeds: Vec::new(),
            must: String::new(),
            regex: false,
            exclude: String::new(),
            min_size_mb: 0,
            max_size_mb: 0,
            categories: String::new(),
            via: String::new(),
            dest_dir: String::new(),
            category: String::new(),
            created_at: 0,
            only_new: true,
        }
    }
}

impl FinishAction {
    /// Notificaciones: las únicas que existen en Android y que se pasan a otro dispositivo.
    pub fn is_notification(&self) -> bool {
        matches!(self.kind.as_str(), "discord" | "telegram" | "ntfy" | "webhook")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Funciones activas: descargas Usenet (NNTP) y/o búsqueda en indexadores.
    pub feature_nntp: bool,
    pub feature_indexers: bool,
    /// El usuario ya eligió las funciones en el asistente.
    pub features_chosen: bool,
    pub indexers: Vec<IndexerConfig>,
    /// Agrupar resultados repetidos entre indexadores.
    pub search_dedup: bool,
    /// Resultados por indexador y búsqueda.
    pub search_limit: u32,
    /// Añadir a la cola con un clic desde la búsqueda (sin diálogo).
    pub search_quick_add: bool,
    /// Carpeta donde guardar los .nzb descargados desde la búsqueda.
    pub nzb_save_dir: String,
    pub nzb_ask_save_dir: bool,
    pub servers: Vec<ServerConfig>,
    /// Servicios debrid en orden de preferencia.
    pub debrid: Vec<DebridConfig>,
    /// Método por defecto para descargar un NZB: "nntp" o "debrid" (el primer servicio disponible).
    pub download_via: String,
    /// Si el servicio debrid falla (y no quedan otros), se descarga por NNTP.
    pub debrid_fallback_nntp: bool,
    /// Conexiones HTTP simultáneas al bajar los archivos del servicio debrid.
    pub debrid_connections: u32,
    /// Borrar la descarga del servicio debrid cuando ya está en el PC.
    pub debrid_delete_after: bool,
    pub download_dir: String,
    /// Carpeta temporal. Vacío = subcarpeta "_incompleto" dentro del destino.
    pub incomplete_dir: String,
    pub ask_destination: bool,
    pub create_job_folder: bool,
    pub par2_enabled: bool,
    pub extract_enabled: bool,
    pub delete_archives: bool,
    pub delete_par2: bool,
    pub abort_hopeless: bool,
    pub clipboard_monitor: bool,
    pub clipboard_patterns: Vec<String>,
    pub clipboard_show_window: bool,
    /// Descargar y analizar el NZB al detectarlo para mostrar su nombre y tamaño.
    pub clipboard_inspect: bool,
    pub notify_complete: bool,
    pub notify_failed: bool,
    pub minimize_to_tray: bool,
    pub close_to_tray: bool,
    pub start_minimized: bool,
    pub start_with_windows: bool,
    pub associate_nzb: bool,
    /// Ruta de par2 y de 7-Zip. Vacía = automática (la que trae Nebula o la del sistema).
    pub par2_path: String,
    pub seven_zip_path: String,
    /// KB/s, 0 = sin límite.
    pub speed_limit_kbps: u64,
    pub max_retries: u32,
    pub timeout_secs: u64,
    pub animations: bool,
    /// Android: vibración breve al tocar botones y pestañas.
    pub haptics: bool,
    pub accent: String,
    /// Idioma de la interfaz ("es", "en"…). Vacío = el del sistema.
    pub language: String,
    pub confirm_delete: bool,
    pub add_paused: bool,
    pub history_limit: u32,
    pub first_run_done: bool,
    /// API compatible con Newznab/NZBHydra2 para Sonarr, Radarr, Prowlarr…
    pub api_enabled: bool,
    pub api_key: String,
    /// API key cifrada con DPAPI (solo en disco).
    pub api_key_enc: String,
    pub api_port: u16,
    /// Escuchar en la red local (0.0.0.0) y no solo en este equipo (127.0.0.1).
    pub api_lan: bool,
    /// Buscar actualizaciones al abrir Nebula (y cada pocas horas).
    pub update_check: bool,
    /// Versión que el usuario decidió omitir: no se vuelve a avisar de ella.
    pub update_skipped: String,
    /// Control remoto: los móviles emparejados pueden ver y manejar la cola de este equipo.
    pub remote_enabled: bool,
    pub remote_port: u16,
    /// Descarga automática por RSS.
    pub rss_feeds: Vec<RssFeed>,
    pub rss_rules: Vec<RssRule>,
    /// Acciones al terminar una descarga, al fallar o al vaciarse la cola.
    pub finish_actions: Vec<FinishAction>,
    /// Categorías propias (tv, movies…). La Default no está aquí: son los ajustes globales.
    pub categories: Vec<crate::categories::Category>,
    /// Comprobar con STAT que los artículos existen antes de descargar.
    pub precheck: bool,
    /// Poner nombres legibles a los archivos ofuscados (con el PAR2 y con el nombre del trabajo).
    pub deobfuscate: bool,
    /// Extraer cada archivo comprimido en cuanto está completo, sin esperar al resto.
    pub direct_unpack: bool,
    /// Contraseñas que se prueban si la del trabajo no abre el archivo.
    pub extra_passwords: Vec<String>,
    /// Extensiones no deseadas (sin punto) y qué hacer con ellas: "off", "warn", "skip" o "fail".
    pub unwanted_extensions: Vec<String>,
    pub unwanted_action: String,
    /// Descargas repetidas (en la cola o ya completadas): "off", "warn", "pause" o "skip".
    pub duplicate_action: String,
    /// Programador: pausar, reanudar o limitar la velocidad a ciertas horas.
    pub schedule: Vec<ScheduleRule>,
}

impl Default for Settings {
    fn default() -> Self {
        // nebula-server (Docker, systemd) puede fijar la carpeta de descargas por defecto.
        let (dl, nzb_dir) = match std::env::var_os("NEBULA_DOWNLOAD_DIR").filter(|v| !v.is_empty()) {
            Some(d) => (PathBuf::from(&d), PathBuf::from(d)),
            None => {
                let downloads = default_download_root();
                (downloads.join("Nebula"), downloads)
            }
        };
        Self {
            feature_nntp: true,
            feature_indexers: false,
            features_chosen: false,
            indexers: Vec::new(),
            search_dedup: true,
            search_limit: 100,
            search_quick_add: true,
            nzb_save_dir: nzb_dir.to_string_lossy().to_string(),
            nzb_ask_save_dir: false,
            servers: Vec::new(),
            debrid: Vec::new(),
            download_via: "nntp".into(),
            debrid_fallback_nntp: true,
            debrid_connections: 8,
            debrid_delete_after: true,
            download_dir: dl.to_string_lossy().to_string(),
            incomplete_dir: String::new(),
            ask_destination: true,
            create_job_folder: true,
            par2_enabled: true,
            extract_enabled: true,
            delete_archives: true,
            delete_par2: true,
            abort_hopeless: true,
            clipboard_monitor: true,
            clipboard_patterns: default_patterns(),
            clipboard_show_window: true,
            clipboard_inspect: true,
            notify_complete: true,
            notify_failed: true,
            minimize_to_tray: false,
            close_to_tray: true,
            start_minimized: false,
            start_with_windows: false,
            associate_nzb: false,
            par2_path: String::new(),
            seven_zip_path: String::new(),
            speed_limit_kbps: 0,
            max_retries: 3,
            timeout_secs: 30,
            animations: true,
            haptics: true,
            accent: "nebula".into(),
            language: String::new(),
            confirm_delete: true,
            add_paused: false,
            history_limit: 300,
            first_run_done: false,
            api_enabled: false,
            api_key: String::new(),
            api_key_enc: String::new(),
            api_port: crate::api::DEFAULT_PORT,
            api_lan: false,
            update_check: true,
            update_skipped: String::new(),
            remote_enabled: false,
            remote_port: crate::remote::DEFAULT_PORT,
            rss_feeds: Vec::new(),
            rss_rules: Vec::new(),
            finish_actions: Vec::new(),
            categories: Vec::new(),
            precheck: false,
            deobfuscate: true,
            direct_unpack: false,
            extra_passwords: Vec::new(),
            unwanted_extensions: default_unwanted(),
            unwanted_action: "warn".into(),
            duplicate_action: "warn".into(),
            schedule: Vec::new(),
        }
    }
}

pub fn default_unwanted() -> Vec<String> {
    ["exe", "scr", "com", "bat", "cmd", "lnk", "vbs", "msi", "ps1", "jar"].iter().map(|s| s.to_string()).collect()
}

pub fn default_patterns() -> Vec<String> {
    vec!["getnzb".into(), "/api?t=get".into(), "&t=get&".into(), "/nzb/".into()]
}

impl Settings {
    pub fn load(paths: &AppPaths) -> Self {
        let mut s: Settings = std::fs::read_to_string(paths.config_file())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        for srv in &mut s.servers {
            if !srv.password_enc.is_empty() {
                if let Some(p) = crate::secrets::unprotect(&srv.password_enc) {
                    srv.password = p;
                }
            }
            srv.password_enc.clear();
        }
        for ix in &mut s.indexers {
            if !ix.api_key_enc.is_empty() {
                if let Some(k) = crate::secrets::unprotect(&ix.api_key_enc) {
                    ix.api_key = k;
                }
            }
            ix.api_key_enc.clear();
        }
        for d in &mut s.debrid {
            if !d.api_key_enc.is_empty() {
                if let Some(k) = crate::secrets::unprotect(&d.api_key_enc) {
                    d.api_key = k;
                }
            }
            d.api_key_enc.clear();
        }
        if !s.api_key_enc.is_empty() {
            if let Some(k) = crate::secrets::unprotect(&s.api_key_enc) {
                s.api_key = k;
            }
        }
        s.api_key_enc.clear();
        for a in &mut s.finish_actions {
            if !a.url_enc.is_empty() {
                if let Some(v) = crate::secrets::unprotect(&a.url_enc) {
                    a.url = v;
                }
            }
            if !a.token_enc.is_empty() {
                if let Some(v) = crate::secrets::unprotect(&a.token_enc) {
                    a.token = v;
                }
            }
            a.url_enc.clear();
            a.token_enc.clear();
        }
        // Siempre debe quedar al menos una función activa.
        if !s.feature_nntp && !s.feature_indexers {
            s.feature_nntp = true;
        }
        s
    }

    pub fn save(&self, paths: &AppPaths) -> Result<(), String> {
        let mut disk = self.clone();
        for srv in &mut disk.servers {
            srv.password_enc = if srv.password.is_empty() {
                String::new()
            } else {
                crate::secrets::protect(&srv.password).unwrap_or_default()
            };
            srv.password.clear();
        }
        for ix in &mut disk.indexers {
            ix.api_key_enc = if ix.api_key.is_empty() { String::new() } else { crate::secrets::protect(&ix.api_key).unwrap_or_default() };
            ix.api_key.clear();
        }
        for d in &mut disk.debrid {
            d.api_key_enc = if d.api_key.is_empty() { String::new() } else { crate::secrets::protect(&d.api_key).unwrap_or_default() };
            d.api_key.clear();
        }
        disk.api_key_enc = if disk.api_key.is_empty() { String::new() } else { crate::secrets::protect(&disk.api_key).unwrap_or_default() };
        disk.api_key.clear();
        for a in &mut disk.finish_actions {
            a.url_enc = if a.url.is_empty() { String::new() } else { crate::secrets::protect(&a.url).unwrap_or_default() };
            a.url.clear();
            a.token_enc = if a.token.is_empty() { String::new() } else { crate::secrets::protect(&a.token).unwrap_or_default() };
            a.token.clear();
        }
        let json = serde_json::to_string_pretty(&disk).map_err(|e| e.to_string())?;
        write_atomic(&paths.config_file(), json.as_bytes())
    }
}

pub fn write_atomic(path: &std::path::Path, data: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data).map_err(|e| t!("err.writeFile", path = tmp.display(), error = e))?;
    std::fs::rename(&tmp, path).map_err(|e| t!("err.saveFile", path = path.display(), error = e))
}

#[cfg(not(target_os = "android"))]
fn default_download_root() -> PathBuf {
    dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("C:\\"))
}

/// Carpeta pública de descargas del móvil (visible desde el gestor de archivos).
#[cfg(target_os = "android")]
fn default_download_root() -> PathBuf {
    PathBuf::from("/storage/emulated/0/Download")
}

