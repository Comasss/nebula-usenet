//! Comandos de la interfaz: envoltorios finos sobre el motor (`nebula_core`). La lógica está en
//! el motor (sobre todo en `facade.rs`) para que la usen igual el servidor y la línea de comandos.
use crate::os;
use nebula_core::config::{DebridConfig, IndexerConfig, ServerConfig, Settings};
use nebula_core::engine::{AddRequest, Engine, FileView, HistoryEntry, NzbPreview, Snapshot, TestResult};
use nebula_core::facade::{AppInfo, DirList};
use nebula_core::logger::{self, LogEntry};
use nebula_core::stats::{IndexerEvent, StatRecord};
use nebula_core::{debrid, indexer};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State, WebviewWindow};

type Eng<'a> = State<'a, Arc<Engine>>;

#[tauri::command]
pub fn get_settings(e: Eng) -> Settings {
    e.settings.read().clone()
}

/// La interfaz avisa del idioma que usa (el elegido o el del sistema) para el menú de la bandeja,
/// las notificaciones y los mensajes del motor.
#[tauri::command]
pub fn set_language(app: AppHandle, e: Eng, lang: String) {
    if e.set_language(&lang) {
        let paused = e.st.lock().paused;
        crate::refresh_tray_texts(&app, paused);
    }
}

#[tauri::command]
pub fn save_settings(e: Eng, settings: Settings) -> Result<(), String> {
    e.save_settings(settings)
}

#[tauri::command]
pub async fn test_server(server: ServerConfig) -> TestResult {
    crate::engine::test_server(server).await
}

#[tauri::command]
pub async fn inspect_nzb_file(e: Eng<'_>, path: String) -> Result<NzbPreview, String> {
    e.inspect_nzb_file(path).await
}

#[tauri::command]
pub async fn inspect_nzb_url(e: Eng<'_>, url: String, indexer_id: Option<String>) -> Result<NzbPreview, String> {
    e.inspect_nzb_url(url, indexer_id).await
}

#[tauri::command]
pub fn discard_pending(e: Eng, token: String) {
    e.discard_pending(&token);
}

#[tauri::command]
pub fn add_job(e: Eng, req: AddRequest) -> Result<String, String> {
    e.inner().add_job(req)
}

#[tauri::command]
pub fn get_stats(e: Eng) -> Vec<StatRecord> {
    e.stats()
}

#[tauri::command]
pub fn get_queue(e: Eng) -> Snapshot {
    e.snapshot()
}

#[tauri::command]
pub fn get_job_files(e: Eng, id: String) -> Vec<FileView> {
    e.job_files(&id)
}

#[tauri::command]
pub fn pause_job(e: Eng, id: String) {
    e.pause_job(&id);
}

#[tauri::command]
pub fn resume_job(e: Eng, id: String) {
    e.inner().resume_job(&id);
}

#[tauri::command]
pub fn remove_job(e: Eng, id: String, delete_files: bool) {
    e.remove_job(&id, delete_files);
}

#[tauri::command]
pub fn move_job(e: Eng, id: String, to: usize) {
    e.move_job(&id, to);
}

#[tauri::command]
pub fn set_job_priority(e: Eng, id: String, priority: i32) {
    e.set_job_priority(&id, priority);
}

#[tauri::command]
pub fn set_job_category(e: Eng, id: String, category: String) {
    e.set_job_category(&id, &category);
}

#[tauri::command]
pub fn set_paused(e: Eng, paused: bool) {
    e.set_paused(paused);
}

#[tauri::command]
pub fn set_speed_limit(e: Eng, kbps: u64) -> Result<(), String> {
    e.set_speed_limit(kbps)
}

#[tauri::command]
pub fn reset_quota(e: Eng, id: String) {
    e.reset_quota(&id)
}

#[tauri::command]
pub fn get_history(e: Eng) -> Vec<HistoryEntry> {
    e.history()
}

#[tauri::command]
pub fn remove_history(e: Eng, id: String, delete_files: bool) {
    e.remove_history(&id, delete_files);
}

#[tauri::command]
pub fn clear_history(e: Eng, only_completed: bool) {
    e.clear_history(only_completed);
}

#[tauri::command]
pub fn retry_history(e: Eng, id: String) -> Result<String, String> {
    e.inner().retry_history(&id)
}

#[tauri::command]
pub fn retry_extract(e: Eng, id: String, password: Option<String>) -> Result<(), String> {
    e.inner().retry_extract(&id, password)
}

#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    os::open_path(&path)
}

#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), String> {
    os::reveal_path(&path)
}

#[tauri::command]
pub fn get_logs() -> Vec<LogEntry> {
    logger::entries()
}

#[tauri::command]
pub fn clear_logs() {
    logger::clear();
}

#[tauri::command]
pub fn disk_free(path: String) -> Option<u64> {
    nebula_core::sys::disk_free(std::path::Path::new(&path))
}

#[tauri::command]
pub fn default_patterns() -> Vec<String> {
    nebula_core::config::default_patterns()
}

#[tauri::command]
pub fn detect_nzb_urls(e: Eng, text: String) -> Vec<String> {
    e.detect_nzb_urls(&text)
}

#[tauri::command]
pub fn app_info(e: Eng) -> AppInfo {
    e.app_info()
}

/// La interfaz ya está lista: se muestra la ventana y se entregan los archivos pendientes.
#[tauri::command]
pub fn app_ready(app: AppHandle, e: Eng) {
    if e.ui_ready() {
        let minimized = std::env::args().any(|a| a == "--minimized") || e.settings.read().start_minimized;
        if let Some(w) = app.get_webview_window("main") {
            if !minimized {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
    }
}

#[tauri::command]
pub fn window_minimize(window: WebviewWindow, e: Eng) {
    if e.settings.read().minimize_to_tray {
        let _ = window.hide();
    } else {
        #[cfg(desktop)]
        let _ = window.minimize();
    }
}

#[tauri::command]
pub fn window_close(app: AppHandle, window: WebviewWindow, e: Eng) {
    if e.settings.read().close_to_tray {
        let _ = window.hide();
    } else {
        app.exit(0);
    }
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

// ---- Indexadores ----

#[tauri::command]
pub async fn search_indexer(
    e: Eng<'_>,
    id: String,
    query: String,
    category: Option<String>,
    offset: u32,
    search_id: Option<String>,
) -> Result<indexer::SearchResult, String> {
    e.search_indexer(&id, &query, category.as_deref(), offset, search_id).await
}

/// Estado de la API Newznab (si está escuchando, en qué direcciones o por qué no arrancó).
#[tauri::command]
pub fn api_status() -> nebula_core::api::ApiStatus {
    nebula_core::api::status()
}

#[tauri::command]
pub fn get_indexer_stats(e: Eng) -> Vec<IndexerEvent> {
    e.indexer_stats()
}

#[tauri::command]
pub async fn test_indexer(indexer: IndexerConfig) -> indexer::IndexerTest {
    indexer::test(&indexer).await
}

/// Indexadores de un Prowlarr o un NZBHydra2.
#[tauri::command]
pub async fn indexer_discover(url: String, api_key: String) -> Result<Vec<IndexerConfig>, String> {
    nebula_core::import::discover_indexers(&url, &api_key).await
}

/// Lee un sabnzbd.ini o nzbget.conf sin cambiar nada.
#[tauri::command]
pub fn import_preview(e: Eng, kind: Option<String>, text: String) -> Result<nebula_core::import::Imported, String> {
    e.import_preview(&kind.unwrap_or_default(), &text)
}

/// Añade a los ajustes lo elegido de una importación.
#[tauri::command]
pub fn import_apply(e: Eng, imported: nebula_core::import::Imported, choice: nebula_core::import::Choice) -> Result<nebula_core::import::Applied, String> {
    e.import_apply(&imported, &choice)
}

/// Descarga un NZB desde un enlace y lo guarda en una carpeta del PC.
#[tauri::command]
pub async fn save_nzb_url(e: Eng<'_>, url: String, dir: String, name: String, indexer_id: Option<String>) -> Result<String, String> {
    e.save_nzb_url(&url, &dir, &name, indexer_id.as_deref()).await
}

/// Guarda el NZB de una vista previa ya analizada (sin volver a pedirlo al indexador).
#[tauri::command]
pub fn save_pending_nzb(e: Eng, token: String, dir: String, name: String) -> Result<String, String> {
    e.save_pending_nzb(&token, &dir, &name)
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(t!("err.invalidLink"));
    }
    os::open_url(&url)
}

// ---- Servicios debrid ----

#[tauri::command]
pub async fn test_debrid(provider: DebridConfig) -> debrid::DebridTest {
    debrid::test(&provider).await
}

/// Servicios debrid que ya tienen en caché el NZB de una vista previa (descarga instantánea).
#[tauri::command]
pub async fn debrid_cached(e: Eng<'_>, token: String) -> Result<Vec<String>, String> {
    e.debrid_cached(&token).await
}

// ---- Móvil ----

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Resumed {
    /// .nzb abiertos o compartidos desde otra app.
    files: Vec<String>,
    /// Enlaces compartidos con Nebula.
    urls: Vec<String>,
    /// Enlaces a NZB encontrados en el portapapeles.
    clipboard: Vec<String>,
    /// Si se puede escribir en el almacenamiento compartido.
    storage: bool,
}

/// La interfaz vuelve a primer plano (en Android): entrega lo que haya llegado mientras tanto.
#[tauri::command]
pub async fn app_resumed(e: Eng<'_>) -> Result<Resumed, String> {
    #[cfg(target_os = "android")]
    {
        let (enabled, patterns) = {
            let s = e.settings.read();
            (s.clipboard_monitor, s.clipboard_patterns.clone())
        };
        let r = crate::android::resume(enabled).await;
        let clipboard = if enabled { nebula_core::util::detect_nzb_urls(&r.clipboard, &patterns) } else { Vec::new() };
        Ok(Resumed { files: r.files, urls: r.urls, clipboard, storage: r.storage })
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = e;
        Ok(Resumed { storage: true, ..Default::default() })
    }
}

/// Pide acceso a los archivos del móvil. Devuelve si ya se tiene.
#[tauri::command]
pub async fn request_storage_access() -> bool {
    #[cfg(target_os = "android")]
    return crate::android::request_storage_access().await;
    #[cfg(not(target_os = "android"))]
    true
}

/// Guarda un .nzb elegido con el selector de archivos del sistema (llega su contenido, no una
/// ruta) y devuelve la ruta donde ha quedado. El nombre va en la cabecera `x-name`, codificado
/// con encodeURIComponent.
#[tauri::command]
pub fn import_nzb(e: Eng, request: tauri::ipc::Request<'_>) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else {
        return Err(t!("err.internal"));
    };
    let name = request
        .headers()
        .get("x-name")
        .and_then(|v| v.to_str().ok())
        .map(nebula_core::util::percent_decode)
        .unwrap_or_default();
    e.import_nzb(&name, data)
}

/// Subcarpetas de una carpeta, para el selector de carpetas propio de la versión móvil.
#[tauri::command]
pub fn list_dirs(path: String) -> Result<DirList, String> {
    nebula_core::facade::list_dirs(&path)
}

/// Crea una carpeta (botón "Nueva carpeta" del selector de carpetas).
#[tauri::command]
pub fn make_dir(path: String) -> Result<(), String> {
    nebula_core::facade::make_dir(&path)
}

/// Android: vibración breve al tocar ("tap", "tick", "long", "success" o "error").
#[tauri::command]
pub fn haptic(kind: String) {
    #[cfg(target_os = "android")]
    crate::android::haptic(&kind);
    #[cfg(not(target_os = "android"))]
    let _ = kind;
}

/// Android: el botón Atrás desde la vista principal deja la app en segundo plano.
#[tauri::command]
pub fn app_background() {
    #[cfg(target_os = "android")]
    crate::android::move_to_back();
}

/// Busca una versión nueva en las Releases de GitHub.
#[tauri::command]
pub async fn update_check() -> Result<crate::updater::UpdateInfo, String> {
    crate::updater::check().await
}

/// Descarga la actualización encontrada (avance en el evento `update-progress`).
#[tauri::command]
pub async fn update_download(app: AppHandle, e: Eng<'_>) -> Result<(), String> {
    let data = e.paths.data.clone();
    crate::updater::download(&app, &data).await
}

#[tauri::command]
pub fn update_cancel() {
    crate::updater::cancel();
}

/// Instala la actualización descargada. En Windows Nebula se cierra y se vuelve a abrir.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<crate::updater::InstallResult, String> {
    crate::updater::install(&app).await
}

// ---- Pasar los ajustes a otro dispositivo ----

#[tauri::command]
pub fn transfer_export(e: Eng) -> Result<nebula_core::transfer::QrCode, String> {
    nebula_core::transfer::export_qr(&e.settings.read())
}

#[tauri::command]
pub fn transfer_preview(code: String) -> Result<crate::transfer::Preview, String> {
    crate::transfer::preview(&code)
}

/// Importa un código de ajustes y devuelve los ajustes resultantes.
#[tauri::command]
pub fn transfer_import(e: Eng, code: String) -> Result<Settings, String> {
    e.transfer_import(&code)
}

#[tauri::command]
pub async fn transfer_receive_start() -> Result<nebula_core::transfer::QrCode, String> {
    nebula_core::transfer::start_receive().await
}

#[tauri::command]
pub fn transfer_receive_stop() {
    crate::transfer::stop_receive();
}

#[tauri::command]
pub async fn transfer_send(e: Eng<'_>, target: String) -> Result<(), String> {
    let s = e.settings.read().clone();
    crate::transfer::send(&s, &target).await
}

// ---- Control remoto ----

/// PC: estado del control remoto (si escucha, en qué puerto y direcciones).
#[tauri::command]
pub fn remote_status() -> crate::remote::RemoteStatus {
    crate::remote::status()
}

/// PC: móviles emparejados.
#[tauri::command]
pub fn remote_devices() -> Vec<crate::remote::DeviceView> {
    crate::remote::devices()
}

#[tauri::command]
pub fn remote_revoke(id: String) {
    crate::remote::revoke(&id);
}

/// PC: el usuario acepta o rechaza el emparejamiento de un móvil.
#[tauri::command]
pub fn remote_pair_answer(req: String, ok: bool) {
    crate::remote::answer_pairing(&req, ok);
}

/// Móvil: PCs con Nebula en la red local.
#[tauri::command]
pub async fn remote_discover() -> Vec<crate::remote::Found> {
    crate::remote::discover(std::time::Duration::from_millis(2200)).await
}

/// Móvil: PCs emparejados.
#[tauri::command]
pub fn remote_pcs() -> Vec<crate::remote::PcView> {
    crate::remote::pcs()
}

#[tauri::command]
pub async fn remote_pair(addr: String, name: String) -> Result<crate::remote::PcView, String> {
    crate::remote::pair(&addr, &name).await
}

#[tauri::command]
pub fn remote_pair_cancel() {
    crate::remote::cancel_pairing();
}

#[tauri::command]
pub fn remote_forget(id: String) {
    crate::remote::forget(&id);
}

#[tauri::command]
pub fn remote_connect(id: String) -> Result<crate::remote::SessionState, String> {
    crate::remote::connect(&id)
}

#[tauri::command]
pub fn remote_disconnect() {
    crate::remote::disconnect();
}

#[tauri::command]
pub fn remote_session() -> Option<crate::remote::SessionState> {
    crate::remote::session_state()
}

#[tauri::command]
pub fn remote_wake() {
    crate::remote::wake();
}

/// Móvil: pide algo al PC que se está controlando (solo lo permitido: cola, historial…).
#[tauri::command]
pub async fn remote_call(op: String, args: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    crate::remote::call(&op, args.unwrap_or(serde_json::Value::Null)).await
}

/// Móvil: manda un .nzb (`kind` "file", ruta) o un enlace (`kind` "url") a un PC emparejado.
#[tauri::command]
pub async fn remote_send(
    id: String,
    kind: String,
    value: String,
    via: Option<String>,
    name: Option<String>,
    category: Option<String>,
    newznab: Option<Vec<u32>>,
) -> Result<serde_json::Value, String> {
    crate::remote::send_nzb(&id, &kind, &value, via, name, category, newznab.unwrap_or_default()).await
}

// ---- Descarga automática por RSS ----

#[tauri::command]
pub fn rss_status(e: Eng) -> crate::rss::Status {
    crate::rss::status(e.inner())
}

/// "Comprobar ahora": consulta todos los feeds activos.
#[tauri::command]
pub async fn rss_check_now(e: Eng<'_>) -> Result<crate::rss::CheckSummary, String> {
    crate::rss::run(e.inner(), true).await
}

/// Vista previa de una regla (aún sin guardar) con los elementos de los feeds actuales.
#[tauri::command]
pub async fn rss_preview(e: Eng<'_>, rule: crate::config::RssRule) -> Result<crate::rss::Preview, String> {
    crate::rss::preview(e.inner(), rule).await
}

#[tauri::command]
pub fn rss_clear_log() {
    crate::rss::clear_log();
}

// ---- Acciones al terminar ----

/// Botón "Probar" de una acción: la ejecuta con una descarga de ejemplo. La de energía solo hace
/// una cuenta atrás corta, sin apagar nada.
#[tauri::command]
pub async fn finish_test(e: Eng<'_>, action: crate::config::FinishAction) -> Result<(), String> {
    e.finish_test(action).await
}

#[tauri::command]
pub fn power_status(e: Eng) -> Option<crate::finish::Countdown> {
    crate::finish::power_status(&e.finish)
}

#[tauri::command]
pub fn power_cancel(e: Eng) -> bool {
    e.power_cancel()
}

/// Apagado rápido desde el panel de velocidad: `mode` = "shutdown" / "sleep" / "hibernate", o nada para desactivarlo.
#[tauri::command]
pub fn power_quick(e: Eng, mode: Option<String>) -> Settings {
    e.power_quick(mode.as_deref())
}
