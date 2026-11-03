//! Actualizaciones desde las Releases de GitHub: comprueba si hay una versión más nueva, descarga
//! el archivo que corresponde a esta instalación (instalador, ejecutable portable o APK), lo
//! verifica con SHA256SUMS.txt y lo instala.
//!
//! - Windows instalado: se ejecuta el instalador nuevo en modo silencioso, que cierra Nebula,
//!   la actualiza y la vuelve a abrir.
//! - Windows portable: el ejecutable en uso se renombra a `.old`, el nuevo ocupa su lugar y se
//!   reinicia. El `.old` se borra en el siguiente arranque.
//! - Android: el APK se abre con el instalador del sistema (pide permiso la primera vez).
//!
//! La interfaz no elige qué se descarga: solo puede pedir el archivo de la última comprobación.
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;

const REPO: &str = "Comasss/nebula-usenet";
const CURRENT: &str = env!("CARGO_PKG_VERSION");

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(concat!("Nebula/", env!("CARGO_PKG_VERSION"), " (updater)"))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .expect("cliente HTTP")
});

/// Lo que la interfaz necesita para enseñar la actualización.
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub available: bool,
    pub current: String,
    pub version: String,
    pub notes: String,
    /// Página de la release en GitHub.
    pub url: String,
    pub published_at: String,
    /// "installer", "portable", "apk" o "" (no hay archivo para este sistema: solo se puede abrir la página).
    pub kind: String,
    pub asset_name: String,
    pub asset_size: u64,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize, Clone)]
struct Asset {
    name: String,
    size: u64,
    browser_download_url: String,
}

/// Archivo elegido en la última comprobación y, una vez descargado, dónde está.
#[derive(Clone)]
struct Pending {
    version: String,
    #[cfg_attr(not(windows), allow(dead_code))]
    kind: String,
    asset: Asset,
    sums_url: Option<String>,
    file: Option<PathBuf>,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static BUSY: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

/// "v1.5.2" → [1, 5, 2]. Lo que va tras un guion (1.6.0-beta) se ignora.
fn parse_version(v: &str) -> Option<[u64; 3]> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let core = v.split(['-', '+']).next()?;
    let mut out = [0u64; 3];
    let mut parts = core.split('.');
    for slot in out.iter_mut() {
        match parts.next() {
            Some(p) => *slot = p.parse().ok()?,
            None => break,
        }
    }
    Some(out)
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// Qué tipo de archivo necesita esta instalación.
fn install_kind() -> &'static str {
    if cfg!(target_os = "android") {
        "apk"
    } else if cfg!(windows) {
        if is_installed() {
            "installer"
        } else {
            "portable"
        }
    } else {
        ""
    }
}

/// El instalador NSIS deja `uninstall.exe` junto al ejecutable; el portable va suelto.
fn is_installed() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("uninstall.exe").exists()))
        .unwrap_or(false)
}

fn pick_asset<'a>(assets: &'a [Asset], kind: &str) -> Option<&'a Asset> {
    let lower = |a: &&Asset| a.name.to_ascii_lowercase();
    match kind {
        "apk" => assets.iter().find(|a| lower(a).ends_with(".apk")),
        "installer" => assets.iter().find(|a| lower(a).ends_with("-setup.exe")),
        "portable" => assets.iter().find(|a| lower(a).ends_with("_portable.exe")),
        _ => None,
    }
}

fn net_error(e: reqwest::Error) -> String {
    if e.is_timeout() {
        t!("update.timeout")
    } else if e.is_connect() {
        t!("update.connect")
    } else {
        t!("update.netError", error = e.without_url())
    }
}

/// Consulta la última release publicada (sin borradores ni versiones de prueba).
pub async fn check() -> Result<UpdateInfo, String> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", REPO);
    let resp = CLIENT
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(net_error)?;
    let status = resp.status();
    if status.as_u16() == 404 {
        // Aún no hay ninguna release publicada.
        return Ok(UpdateInfo { current: CURRENT.into(), ..Default::default() });
    }
    if status.as_u16() == 403 || status.as_u16() == 429 {
        return Err(t!("update.rateLimited"));
    }
    if !status.is_success() {
        return Err(t!("update.httpError", status = status.as_u16()));
    }
    let body = resp.bytes().await.map_err(net_error)?;
    let rel: Release = serde_json::from_slice(&body).map_err(|_| t!("update.badResponse"))?;
    let version = rel.tag_name.trim().trim_start_matches(['v', 'V']).to_string();
    let available = !rel.draft && !rel.prerelease && is_newer(&version, CURRENT);
    let kind = install_kind();
    let asset = if available { pick_asset(&rel.assets, kind).cloned() } else { None };
    let sums_url = rel
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("SHA256SUMS.txt"))
        .map(|a| a.browser_download_url.clone());

    let info = UpdateInfo {
        available,
        current: CURRENT.into(),
        version: version.clone(),
        notes: rel.body.unwrap_or_default(),
        url: rel.html_url,
        published_at: rel.published_at.unwrap_or_default(),
        kind: if asset.is_some() { kind.into() } else { String::new() },
        asset_name: asset.as_ref().map(|a| a.name.clone()).unwrap_or_default(),
        asset_size: asset.as_ref().map(|a| a.size).unwrap_or(0),
    };
    let mut pending = PENDING.lock();
    match asset {
        // Si ya se había descargado esta misma versión, se conserva el archivo.
        Some(asset) => {
            let keep = pending.as_ref().filter(|p| p.version == version && p.asset.name == asset.name).and_then(|p| p.file.clone());
            *pending = Some(Pending { version, kind: kind.into(), asset, sums_url, file: keep });
        }
        None => *pending = None,
    }
    Ok(info)
}

fn updates_dir(data: &Path) -> PathBuf {
    data.join("updates")
}

/// Borra lo que quede de actualizaciones anteriores (al arrancar y antes de cada descarga).
pub fn cleanup(data: &Path) {
    let dir = updates_dir(data);
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_exe(&exe));
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    downloaded: u64,
    total: u64,
}

struct BusyGuard;
impl Drop for BusyGuard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

/// Descarga el archivo de la última comprobación. Avisa del avance con el evento `update-progress`.
pub async fn download(app: &AppHandle, data: &Path) -> Result<(), String> {
    let pending = PENDING.lock().clone().ok_or_else(|| t!("update.nothing"))?;
    if pending.file.as_ref().is_some_and(|f| f.exists()) {
        return Ok(());
    }
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err(t!("update.busy"));
    }
    let _guard = BusyGuard;
    CANCEL.store(false, Ordering::SeqCst);
    cleanup(data);
    let dir = updates_dir(data);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| t!("err.createDir", path = dir.display(), error = e))?;

    // Suma esperada (si la release trae SHA256SUMS.txt).
    let expected = match &pending.sums_url {
        Some(url) => {
            let text = CLIENT
                .get(url)
                .timeout(Duration::from_secs(30))
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(net_error)?
                .text()
                .await
                .map_err(net_error)?;
            sum_for(&text, &pending.asset.name)
        }
        None => None,
    };

    let name = crate::util::sanitize_filename(&pending.asset.name);
    let part = dir.join(format!("{}.part", name));
    let target = dir.join(&name);
    let mut resp = CLIENT
        .get(&pending.asset.browser_download_url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(net_error)?;
    let total = resp.content_length().unwrap_or(pending.asset.size);
    let mut file = tokio::fs::File::create(&part).await.map_err(|e| t!("err.writeFile", path = part.display(), error = e))?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0u64;
    let mut last = Instant::now();
    let _ = app.emit("update-progress", Progress { downloaded, total });
    loop {
        if CANCEL.load(Ordering::SeqCst) {
            drop(file);
            let _ = tokio::fs::remove_file(&part).await;
            return Err(t!("update.cancelled"));
        }
        let chunk = match tokio::time::timeout(Duration::from_secs(60), resp.chunk()).await {
            Ok(Ok(Some(c))) => c,
            Ok(Ok(None)) => break,
            Ok(Err(e)) => {
                drop(file);
                let _ = tokio::fs::remove_file(&part).await;
                return Err(net_error(e));
            }
            Err(_) => {
                drop(file);
                let _ = tokio::fs::remove_file(&part).await;
                return Err(t!("update.timeout"));
            }
        };
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(|e| t!("err.writeFile", path = part.display(), error = e))?;
        downloaded += chunk.len() as u64;
        if last.elapsed() >= Duration::from_millis(120) {
            last = Instant::now();
            let _ = app.emit("update-progress", Progress { downloaded, total });
        }
    }
    file.flush().await.map_err(|e| t!("err.writeFile", path = part.display(), error = e))?;
    drop(file);
    let _ = app.emit("update-progress", Progress { downloaded, total: total.max(downloaded) });

    if total > 0 && downloaded != total {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(t!("update.incomplete"));
    }
    let actual = to_hex(&hasher.finalize());
    if let Some(expected) = expected {
        if !expected.eq_ignore_ascii_case(&actual) {
            let _ = tokio::fs::remove_file(&part).await;
            crate::lerror!("{}", t!("update.badChecksum"));
            return Err(t!("update.badChecksum"));
        }
    }
    tokio::fs::rename(&part, &target).await.map_err(|e| t!("err.writeFile", path = target.display(), error = e))?;
    crate::linfo!("{}", t!("update.downloaded", version = pending.version));
    if let Some(p) = PENDING.lock().as_mut().filter(|p| p.version == pending.version) {
        p.file = Some(target);
    }
    Ok(())
}

pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// Línea de SHA256SUMS.txt ("<hash>  <archivo>" o "<hash> *<archivo>") que corresponde a `name`.
fn sum_for(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut it = line.split_whitespace();
        let hash = it.next()?;
        let file = it.next()?.trim_start_matches('*');
        (file == name && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Resultado de `install`: en Android puede faltar el permiso para instalar apps.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub started: bool,
    pub needs_permission: bool,
}

/// Instala lo descargado. En Windows cierra Nebula, que se vuelve a abrir ya actualizada.
pub async fn install(app: &AppHandle) -> Result<InstallResult, String> {
    let pending = PENDING.lock().clone().ok_or_else(|| t!("update.nothing"))?;
    let file = pending.file.clone().filter(|f| f.exists()).ok_or_else(|| t!("update.nothing"))?;
    crate::linfo!("{}", t!("update.installing", version = pending.version));
    #[cfg(target_os = "android")]
    {
        let _ = app;
        return crate::android::install_apk(&file.to_string_lossy()).await;
    }
    #[cfg(windows)]
    {
        match pending.kind.as_str() {
            "installer" => run_installer(&file)?,
            "portable" => swap_portable(&file)?,
            _ => return Err(t!("update.unsupported")),
        }
        // Un momento para que la interfaz muestre que se reinicia; al salir se guarda la cola.
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(400)).await;
            app.exit(0);
        });
        return Ok(InstallResult { started: true, needs_permission: false });
    }
    #[allow(unreachable_code)]
    {
        let _ = (app, file);
        Err(t!("update.unsupported"))
    }
}

#[cfg(windows)]
fn old_exe(exe: &Path) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(".old");
    PathBuf::from(s)
}

/// Instalador NSIS de Tauri: /S sin ventanas, /R vuelve a abrir Nebula al terminar y /UPDATE
/// respeta los accesos directos que el usuario haya quitado.
#[cfg(windows)]
fn run_installer(file: &Path) -> Result<(), String> {
    std::process::Command::new(file)
        .args(["/S", "/R", "/UPDATE"])
        .spawn()
        .map(|_| ())
        .map_err(|e| t!("update.launchFailed", error = e))
}

/// Windows deja renombrar un ejecutable en uso: el actual pasa a `.old` y el nuevo ocupa su sitio.
/// El nuevo proceso espera a que este termine antes de arrancar (`--after-update=<pid>`), porque
/// si no la instancia única le pasaría el control a este.
#[cfg(windows)]
fn swap_portable(file: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| t!("update.launchFailed", error = e))?;
    let old = old_exe(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| t!("update.replaceFailed", error = e))?;
    if let Err(e) = std::fs::copy(file, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(t!("update.replaceFailed", error = e));
    }
    let _ = std::fs::remove_file(file);
    std::process::Command::new(&exe)
        .arg(format!("--after-update={}", std::process::id()))
        .spawn()
        .map(|_| ())
        .map_err(|e| t!("update.launchFailed", error = e))
}

/// Al arrancar tras una actualización portable: espera a que se cierre la versión anterior.
pub fn wait_previous() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};
        const SYNCHRONIZE: u32 = 0x0010_0000;
        let pid = std::env::args().find_map(|a| a.strip_prefix("--after-update=").and_then(|p| p.parse::<u32>().ok()));
        if let Some(pid) = pid {
            unsafe {
                let h = OpenProcess(SYNCHRONIZE, 0, pid);
                if !h.is_null() {
                    WaitForSingleObject(h, 15_000);
                    CloseHandle(h);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("v1.5.2"), Some([1, 5, 2]));
        assert_eq!(parse_version("1.6"), Some([1, 6, 0]));
        assert_eq!(parse_version("2.0.0-beta.1"), Some([2, 0, 0]));
        assert_eq!(parse_version("nightly"), None);
        assert!(is_newer("1.5.10", "1.5.9"));
        assert!(is_newer("v2.0.0", "1.99.99"));
        assert!(!is_newer("1.5.2", "1.5.2"));
        assert!(!is_newer("1.4.9", "1.5.0"));
        assert!(!is_newer("latest", "1.5.0"));
    }

    #[test]
    fn assets() {
        let mk = |n: &str| Asset { name: n.into(), size: 1, browser_download_url: format!("https://x/{}", n) };
        let list = vec![
            mk("Nebula_1.6.0_x64-setup.exe"),
            mk("Nebula_1.6.0_x64_portable.exe"),
            mk("Nebula_1.6.0_android_arm64.apk"),
            mk("SHA256SUMS.txt"),
        ];
        assert_eq!(pick_asset(&list, "installer").unwrap().name, "Nebula_1.6.0_x64-setup.exe");
        assert_eq!(pick_asset(&list, "portable").unwrap().name, "Nebula_1.6.0_x64_portable.exe");
        assert_eq!(pick_asset(&list, "apk").unwrap().name, "Nebula_1.6.0_android_arm64.apk");
        assert!(pick_asset(&list, "").is_none());
    }

    #[test]
    fn checksums() {
        let h = "a".repeat(64);
        let text = format!("{h}  Nebula_1.6.0_x64-setup.exe\n{}  *Nebula_1.6.0_android_arm64.apk\n", "B".repeat(64));
        assert_eq!(sum_for(&text, "Nebula_1.6.0_x64-setup.exe"), Some(h));
        assert_eq!(sum_for(&text, "Nebula_1.6.0_android_arm64.apk"), Some("b".repeat(64)));
        assert_eq!(sum_for(&text, "otro.exe"), None);
        assert_eq!(to_hex(&[0, 15, 255]), "000fff");
    }
}
