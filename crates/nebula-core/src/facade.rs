//! Fachada del motor: lo que piden las interfaces (la app Tauri, el servidor web, la CLI) y no
//! es una llamada directa a la cola. Así la lógica vive en un solo sitio y los comandos de cada
//! interfaz se quedan en envoltorios de una línea.
use crate::config::{DebridConfig, FinishAction, Settings};
use crate::engine::{Engine, NzbPreview};
use crate::stats::IndexerEvent;
use crate::{debrid, fetch, indexer, nzb};
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub tools_dir: String,
    pub log_dir: String,
    /// "windows", "android", "linux"…
    pub platform: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DirList {
    pub path: String,
    pub parent: Option<String>,
    pub dirs: Vec<String>,
}

/// Subcarpetas de una carpeta (sin las ocultas), para el selector de carpetas propio.
pub fn list_dirs(path: &str) -> Result<DirList, String> {
    let p = std::path::PathBuf::from(path);
    let rd = std::fs::read_dir(&p).map_err(|err| err.to_string())?;
    let mut dirs: Vec<String> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| !n.starts_with('.'))
        .collect();
    dirs.sort_by_key(|n| n.to_lowercase());
    Ok(DirList {
        path: p.to_string_lossy().to_string(),
        parent: p.parent().map(|x| x.to_string_lossy().to_string()),
        dirs,
    })
}

/// Crea una carpeta (botón "Nueva carpeta" del selector de carpetas).
pub fn make_dir(path: &str) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|err| err.to_string())
}

impl Engine {
    pub fn app_info(&self) -> AppInfo {
        AppInfo {
            version: crate::VERSION.into(),
            data_dir: self.paths.data.to_string_lossy().to_string(),
            tools_dir: self.paths.tools.to_string_lossy().to_string(),
            log_dir: self.paths.logs.to_string_lossy().to_string(),
            platform: std::env::consts::OS.into(),
        }
    }

    /// La interfaz está lista: devuelve si es la primera vez y entrega (por el bus) los .nzb y
    /// enlaces que llegaron antes.
    pub fn ui_ready(&self) -> bool {
        let first = !self.ui_ready.swap(true, Ordering::SeqCst);
        let pending: Vec<_> = self.pending_open.lock().drain(..).collect();
        if !pending.is_empty() {
            crate::bus::emit(crate::bus::Event::OpenItems(pending));
        }
        first
    }

    /// Enlaces a NZB en un texto, con los patrones del portapapeles.
    pub fn detect_nzb_urls(&self, text: &str) -> Vec<String> {
        let patterns = self.settings.read().clipboard_patterns.clone();
        crate::util::detect_nzb_urls(text, &patterns)
    }

    /// Analiza un .nzb del disco y lo deja pendiente para añadirlo (vista previa).
    pub async fn inspect_nzb_file(self: &Arc<Self>, path: String) -> Result<NzbPreview, String> {
        let data = tokio::fs::read(&path).await.map_err(|err| t!("err.readFile", error = err))?;
        let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        self.inspect_nzb_bytes(data, name, true, path).await
    }

    /// Analiza un NZB ya leído (subido desde el navegador, recibido por la API…).
    pub async fn inspect_nzb_bytes(self: &Arc<Self>, data: Vec<u8>, name: String, strong: bool, source: String) -> Result<NzbPreview, String> {
        let engine = self.clone();
        tokio::task::spawn_blocking(move || engine.register_pending(data, &name, strong, source))
            .await
            .map_err(|_| t!("err.internal"))?
    }

    /// Descarga un NZB de un enlace y devuelve sus datos y nombre. Si viene de un indexador,
    /// anota en las estadísticas si se pudo obtener y cuánto tardó.
    pub async fn grab_nzb(&self, url: &str, indexer_id: Option<&str>) -> Result<(Vec<u8>, fetch::Fetched), String> {
        let start = std::time::Instant::now();
        let res = async {
            let f = fetch::fetch(url).await?;
            let data = nzb::maybe_gunzip(&f.data)?;
            if !nzb::looks_like_nzb(&data) {
                return Err(fetch::explain_not_nzb(&data));
            }
            Ok((data, f))
        }
        .await;
        if let Some(id) = indexer_id {
            self.record_indexer(IndexerEvent {
                at: crate::util::now_secs(),
                id: id.to_string(),
                name: self.indexer_label(id),
                kind: "grab".into(),
                ms: start.elapsed().as_millis() as u64,
                ok: res.is_ok(),
                ..Default::default()
            });
        }
        res
    }

    fn indexer_label(&self, id: &str) -> String {
        self.settings.read().indexers.iter().find(|i| i.id == id).map(|i| i.label()).unwrap_or_default()
    }

    /// Descarga el NZB de un enlace y lo deja pendiente para añadirlo (vista previa).
    pub async fn inspect_nzb_url(self: &Arc<Self>, url: String, indexer_id: Option<String>) -> Result<NzbPreview, String> {
        let (data, f) = self.grab_nzb(&url, indexer_id.as_deref()).await?;
        let preview = self.inspect_nzb_bytes(data, f.filename.unwrap_or_default(), f.strong, url).await?;
        if let Some(id) = indexer_id {
            self.set_pending_indexer(&preview.token, id);
        }
        Ok(preview)
    }

    /// Busca en un indexador y lo anota en sus estadísticas.
    pub async fn search_indexer(
        &self,
        id: &str,
        query: &str,
        category: Option<&str>,
        offset: u32,
        search_id: Option<String>,
    ) -> Result<indexer::SearchResult, String> {
        let (ix, limit) = {
            let s = self.settings.read();
            let ix = s.indexers.iter().find(|i| i.id == id).cloned().ok_or_else(|| t!("err.indexerNotFound"))?;
            (ix, s.search_limit)
        };
        let start = std::time::Instant::now();
        let res = indexer::search(&ix, query, category, offset, limit).await;
        self.record_indexer(IndexerEvent {
            at: crate::util::now_secs(),
            id: ix.id.clone(),
            name: ix.label(),
            kind: "search".into(),
            ms: res.as_ref().map(|r| r.ms).unwrap_or_else(|_| start.elapsed().as_millis() as u64),
            ok: res.is_ok(),
            results: res.as_ref().map(|r| r.items.len() as u64).unwrap_or(0),
            offset,
            search: search_id,
        });
        res.map_err(|err| {
            crate::lwarn!("{}: {}", ix.label(), err);
            err
        })
    }

    /// Descarga un NZB desde un enlace y lo guarda en una carpeta.
    pub async fn save_nzb_url(&self, url: &str, dir: &str, name: &str, indexer_id: Option<&str>) -> Result<String, String> {
        let (data, f) = self.grab_nzb(url, indexer_id).await?;
        let name = if name.trim().is_empty() { f.filename.unwrap_or_default() } else { name.to_string() };
        fetch::save_nzb_file(&data, dir, &name)
    }

    /// Guarda el NZB de una vista previa ya analizada (sin volver a pedirlo al indexador).
    pub fn save_pending_nzb(&self, token: &str, dir: &str, name: &str) -> Result<String, String> {
        let (raw, pending_name) = self.pending_nzb(token).ok_or_else(|| t!("err.previewExpiredRetry"))?;
        let data = nzb::maybe_gunzip(&raw)?;
        let path = fetch::save_nzb_file(&data, dir, if name.trim().is_empty() { &pending_name } else { name })?;
        self.discard_pending(token);
        Ok(path)
    }

    /// Guarda un .nzb recibido como datos (selector de archivos del móvil, subida desde el
    /// navegador) en `incoming` y devuelve su ruta.
    pub fn import_nzb(&self, name: &str, data: &[u8]) -> Result<String, String> {
        let name = crate::util::sanitize_filename(name);
        let name = if name.trim().is_empty() { "nebula.nzb".to_string() } else { name };
        let dir = self.paths.data.join("incoming");
        std::fs::create_dir_all(&dir).map_err(|err| t!("err.writeFile", path = dir.display(), error = err))?;
        let path = dir.join(&name);
        std::fs::write(&path, data).map_err(|err| t!("err.writeFile", path = path.display(), error = err))?;
        Ok(path.to_string_lossy().to_string())
    }

    /// Servicios debrid que ya tienen en caché el NZB de una vista previa (descarga instantánea).
    pub async fn debrid_cached(&self, token: &str) -> Result<Vec<String>, String> {
        let providers: Vec<DebridConfig> = self.settings.read().debrid.iter().filter(|d| d.usable()).cloned().collect();
        if !providers.iter().any(|p| p.kind == "torbox") {
            return Ok(Vec::new());
        }
        let (raw, _) = self.pending_nzb(token).ok_or_else(|| t!("err.previewExpired"))?;
        let hash = debrid::nzb_hash(&nzb::maybe_gunzip(&raw)?);
        let mut out = Vec::new();
        for p in providers {
            if let Ok(true) = debrid::cached(&p, &hash).await {
                out.push(p.id);
            }
        }
        Ok(out)
    }

    /// Importa un código de ajustes de otro dispositivo y devuelve los ajustes resultantes.
    pub fn transfer_import(self: &Arc<Self>, code: &str) -> Result<Settings, String> {
        let next = crate::transfer::merge(&self.settings.read(), code)?;
        self.apply_settings(next.clone())?;
        crate::linfo!("{}", t!("log.transferImported"));
        Ok(next)
    }

    /// Botón "Probar" de una acción al terminar: la ejecuta con una descarga de ejemplo. La de
    /// energía solo hace una cuenta atrás corta, sin apagar nada.
    pub async fn finish_test(&self, action: FinishAction) -> Result<(), String> {
        if action.kind == "power" {
            if !self.platform.supports_power() {
                return Err(t!("finish.windowsOnly"));
            }
            crate::finish::start_power(self, &action.power, true);
            return Ok(());
        }
        let res = crate::finish::run(&action, &crate::finish::sample_event()).await;
        match &res {
            Ok(()) => crate::linfo!("{}", t!("finish.testOk", action = crate::finish::label(&action))),
            Err(err) => crate::lerror!("{}", t!("finish.error", action = crate::finish::label(&action), error = err)),
        }
        res
    }

    /// Apagado rápido: `mode` = "shutdown" / "sleep" / "hibernate", o nada para desactivarlo.
    pub fn power_quick(&self, mode: Option<&str>) -> Settings {
        crate::finish::quick_power(self, mode);
        self.settings.read().clone()
    }

    pub fn power_cancel(&self) -> bool {
        crate::finish::cancel_power(self)
    }

    /// Guarda los ajustes y avisa a la plataforma (marca de apagado en la bandeja).
    pub fn save_settings(self: &Arc<Self>, settings: Settings) -> Result<(), String> {
        self.apply_settings(settings)?;
        self.platform.refresh_power();
        Ok(())
    }

    /// Lee un `sabnzbd.ini` o `nzbget.conf` sin cambiar nada. `kind`: "sabnzbd", "nzbget" o vacío.
    pub fn import_preview(&self, kind: &str, text: &str) -> Result<crate::import::Imported, String> {
        crate::import::parse(kind, text)
    }

    /// Añade a los ajustes lo elegido de una importación y los guarda.
    pub fn import_apply(self: &Arc<Self>, imported: &crate::import::Imported, choice: &crate::import::Choice) -> Result<crate::import::Applied, String> {
        let (settings, done) = crate::import::apply(&self.settings.read().clone(), imported, choice)?;
        self.save_settings(settings)?;
        crate::linfo!("{}", t!("imp.applied", servers = done.servers, categories = done.categories, settings = done.settings, schedule = done.schedule));
        Ok(done)
    }

    /// par2 y 7-Zip que se usarán y si existen.
    pub fn tools_status(&self) -> Vec<crate::tools::ToolStatus> {
        crate::tools::status(&self.settings.read(), &self.paths.tools)
    }

    /// Cambia el idioma del motor. Devuelve si se cambió.
    pub fn set_language(&self, lang: &str) -> bool {
        crate::i18n::set(lang)
    }
}
