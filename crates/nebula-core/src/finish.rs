//! Acciones al terminar: lo que se ejecuta cuando una descarga termina, cuando falla o cuando la
//! cola se queda vacía. Cada acción elige con cuáles de esos tres casos se dispara.
//! - Notificaciones (Discord, Telegram, ntfy y webhook genérico): en todas las plataformas.
//! - Script o programa: solo en el escritorio, con un tiempo límite y los datos en variables NEBULA_*.
//! - Energía (apagar, suspender, hibernar): solo en Windows y solo con la cola vacía, tras una
//!   cuenta atrás de 60 s que se puede cancelar desde una ventana, la notificación o la bandeja.
//!
//! Nada de esto bloquea las descargas: cada acción va en su propia tarea y sus errores van al Log.
use crate::config::{FinishAction, Settings};
use crate::engine::{Engine, HistoryEntry, Job};
use crate::util::fmt_bytes;
use crate::{lerror, linfo, lwarn};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;
use std::time::Duration;
use crate::bus;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trigger {
    Complete,
    Failed,
    QueueEmpty,
}

impl Trigger {
    fn id(self) -> &'static str {
        match self {
            Trigger::Complete => "completed",
            Trigger::Failed => "failed",
            Trigger::QueueEmpty => "queueEmpty",
        }
    }
}

/// Lo que se cuenta de una descarga (o de la cola, al vaciarse).
#[derive(Clone, Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub event: String,
    pub name: String,
    /// "completed", "failed" o "queueEmpty".
    pub status: String,
    pub path: String,
    pub size: u64,
    pub duration_secs: u64,
    /// Bytes por segundo.
    pub avg_speed: u64,
    /// Servidor NNTP o servicio debrid usado.
    pub via: String,
    pub error: Option<String>,
    /// Fase en la que falló (vacía si terminó bien).
    pub stage: String,
    /// Con la cola vacía: descargas terminadas y fallidas desde la última vez que se vació.
    pub completed: u32,
    pub failed: u32,
}

/// Recuento de la cola desde la última vez que se vació.
#[derive(Default)]
pub struct Runtime {
    totals: Mutex<Totals>,
    power: Mutex<Option<Countdown>>,
    power_gen: AtomicU64,
}

#[derive(Default)]
struct Totals {
    completed: u32,
    failed: u32,
    bytes: u64,
    secs: u64,
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Countdown {
    #[serde(skip)]
    gen: u64,
    /// "shutdown", "sleep" o "hibernate".
    pub mode: String,
    pub remaining: u32,
    /// Prueba desde Ajustes: al terminar la cuenta atrás no se apaga nada.
    pub test: bool,
}

/// Segundos de la cuenta atrás antes de apagar, suspender o hibernar.
pub const POWER_SECS: u32 = 60;
/// Cuenta atrás de la prueba (no se apaga nada al terminar).
const POWER_TEST_SECS: u32 = 10;

fn via_label(job: &Job, entry: &HistoryEntry) -> String {
    if let Some(v) = entry.via_name.as_ref().filter(|v| !v.trim().is_empty()) {
        return v.clone();
    }
    let used: Vec<&str> = job.server_use.iter().filter(|u| u.ok > 0).map(|u| u.name.as_str()).collect();
    if used.is_empty() {
        "Usenet".into()
    } else {
        used.join(", ")
    }
}

fn event_from(job: &Job, entry: &HistoryEntry, trigger: Trigger) -> Event {
    Event {
        event: trigger.id().into(),
        name: entry.name.clone(),
        status: entry.status.clone(),
        path: entry.dest.clone(),
        size: entry.size,
        duration_secs: entry.download_secs,
        avg_speed: entry.avg_speed,
        via: via_label(job, entry),
        error: entry.error.clone(),
        stage: entry.failed_stage.clone().unwrap_or_default(),
        completed: 0,
        failed: 0,
    }
}

fn matches(a: &FinishAction, trigger: Trigger) -> bool {
    a.enabled
        && match trigger {
            Trigger::Complete => a.on_complete && a.kind != "power",
            Trigger::Failed => a.on_failed && a.kind != "power",
            Trigger::QueueEmpty => a.on_queue_empty,
        }
}

/// Una descarga ha terminado o fallado (ya fuera de la cola). Lanza sus acciones y, si no queda
/// ningún trabajo (ni en pausa ni en espera), las de la cola vacía.
pub fn on_job(e: &Engine, job: &Job, entry: &HistoryEntry, ok: bool) {
    let trigger = if ok { Trigger::Complete } else { Trigger::Failed };
    {
        let mut t = e.finish.totals.lock();
        if ok {
            t.completed += 1;
        } else {
            t.failed += 1;
        }
        t.bytes += entry.size;
        t.secs += entry.download_secs;
    }
    dispatch(e, event_from(job, entry, trigger), trigger);
    if e.st.lock().jobs.is_empty() {
        let t = std::mem::take(&mut *e.finish.totals.lock());
        let ev = Event {
            event: Trigger::QueueEmpty.id().into(),
            status: Trigger::QueueEmpty.id().into(),
            size: t.bytes,
            duration_secs: t.secs,
            avg_speed: t.bytes.checked_div(t.secs).unwrap_or(0),
            completed: t.completed,
            failed: t.failed,
            ..Default::default()
        };
        dispatch(e, ev, Trigger::QueueEmpty);
    }
}

/// Se añadió algo a la cola: si había una cuenta atrás de apagado, se cancela.
pub fn on_job_added(e: &Engine) {
    if e.finish.power.lock().is_some() {
        cancel_power(e);
    }
}

fn dispatch(e: &Engine, ev: Event, trigger: Trigger) {
    let actions: Vec<FinishAction> = e.settings.read().finish_actions.iter().filter(|a| matches(a, trigger)).cloned().collect();
    for a in actions {
        if a.kind == "power" {
            if trigger == Trigger::QueueEmpty && e.platform.supports_power() {
                if a.once {
                    disarm_once(e, &a.id);
                }
                start_power(e, &a.power, false);
            }
            continue;
        }
        let ev = ev.clone();
        crate::rt::spawn(async move {
            if let Err(err) = run(&a, &ev).await {
                lerror!("{}", t!("finish.error", action = label(&a), error = err));
            }
        });
    }
}

/// "Solo esta vez": la acción se desactiva al dispararse.
fn disarm_once(e: &Engine, id: &str) {
    let mut s = e.settings.read().clone();
    let Some(a) = s.finish_actions.iter_mut().find(|a| a.id == id) else { return };
    a.enabled = false;
    save(e, s);
}

fn save(e: &Engine, s: Settings) {
    if let Err(err) = s.save(&e.paths) {
        lerror!("{}", err);
    }
    *e.settings.write() = s.clone();
    bus::emit(bus::Event::Settings(Box::new(s)));
    e.platform.refresh_power();
}

/// Nombre de la acción para el Log y la interfaz.
pub fn label(a: &FinishAction) -> String {
    if !a.name.trim().is_empty() {
        return a.name.trim().to_string();
    }
    match a.kind.as_str() {
        "discord" => "Discord".into(),
        "telegram" => "Telegram".into(),
        "ntfy" => "ntfy".into(),
        "webhook" => "Webhook".into(),
        "script" => {
            let file = std::path::Path::new(a.path.trim()).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            if file.is_empty() {
                t!("finish.kind.script")
            } else {
                file
            }
        }
        "power" => power_name(&a.power),
        other => other.to_string(),
    }
}

fn power_name(mode: &str) -> String {
    match mode {
        "sleep" => t!("finish.power.sleep"),
        "hibernate" => t!("finish.power.hibernate"),
        _ => t!("finish.power.shutdown"),
    }
}

/// Ejecuta una acción (no la de energía) con el evento dado.
pub async fn run(a: &FinishAction, ev: &Event) -> Result<(), String> {
    match a.kind.as_str() {
        "discord" | "telegram" | "ntfy" | "webhook" => notify(a, ev).await,
        "script" => run_script(a, ev).await,
        other => Err(t!("finish.unknownKind", kind = other)),
    }
}

// ---------------------------------------------------------------------------
// Texto del mensaje
// ---------------------------------------------------------------------------

pub fn fmt_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 {
        format!("{} h {} min", h, m)
    } else if m > 0 {
        format!("{} min {} s", m, s)
    } else {
        format!("{} s", s)
    }
}

fn title(ev: &Event) -> String {
    match ev.event.as_str() {
        "completed" => t!("finish.msg.completed"),
        "failed" => t!("finish.msg.failed"),
        _ => t!("finish.msg.queueEmpty"),
    }
}

fn status_text(ev: &Event) -> String {
    match ev.event.as_str() {
        "completed" => t!("finish.status.completed"),
        "failed" => t!("finish.status.failed"),
        _ => t!("finish.status.queueEmpty"),
    }
}

/// Líneas "Campo: valor" del mensaje.
fn lines(ev: &Event) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if ev.event == "queueEmpty" {
        out.push((t!("finish.field.status"), status_text(ev)));
        out.push((t!("finish.field.completed"), ev.completed.to_string()));
        if ev.failed > 0 {
            out.push((t!("finish.field.failed"), ev.failed.to_string()));
        }
    } else {
        out.push((t!("finish.field.name"), ev.name.clone()));
        out.push((t!("finish.field.status"), status_text(ev)));
    }
    out.push((t!("finish.field.size"), fmt_bytes(ev.size)));
    if ev.duration_secs > 0 {
        out.push((t!("finish.field.duration"), fmt_duration(ev.duration_secs)));
    }
    if ev.avg_speed > 0 {
        out.push((t!("finish.field.speed"), format!("{}/s", fmt_bytes(ev.avg_speed))));
    }
    if !ev.via.is_empty() {
        out.push((t!("finish.field.via"), ev.via.clone()));
    }
    if let Some(err) = ev.error.as_ref().filter(|e| !e.is_empty()) {
        out.push((t!("finish.field.error"), err.clone()));
    }
    out
}

pub fn message(ev: &Event) -> (String, String) {
    let body = lines(ev).into_iter().map(|(k, v)| format!("{}: {}", k, v)).collect::<Vec<_>>().join("\n");
    (title(ev), body)
}

/// Evento de ejemplo para el botón "Probar".
pub fn sample_event() -> Event {
    Event {
        event: "completed".into(),
        name: t!("finish.testName"),
        status: "completed".into(),
        path: std::env::temp_dir().to_string_lossy().to_string(),
        size: 3 * 1024 * 1024 * 1024 / 2,
        duration_secs: 95,
        avg_speed: 3 * 1024 * 1024 * 1024 / 2 / 95,
        via: "Nebula".into(),
        error: None,
        stage: String::new(),
        completed: 0,
        failed: 0,
    }
}

// ---------------------------------------------------------------------------
// Notificaciones
// ---------------------------------------------------------------------------

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(concat!("Nebula/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .build()
        .expect("cliente HTTP")
});

fn http_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw.trim()).map_err(|_| t!("finish.badUrl"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(t!("finish.badUrl"));
    }
    Ok(url)
}

/// URL y cuerpo de la petición de cada servicio.
pub fn request(a: &FinishAction, ev: &Event) -> Result<(reqwest::Url, serde_json::Value), String> {
    let (title, body) = message(ev);
    let color = match ev.event.as_str() {
        "completed" => 0x22c55e,
        "failed" => 0xef4444,
        _ => 0x7c5cff,
    };
    match a.kind.as_str() {
        "discord" => {
            if a.url.trim().is_empty() {
                return Err(t!("finish.missing.url"));
            }
            let fields: Vec<_> = lines(ev)
                .into_iter()
                .map(|(k, v)| json!({ "name": k, "value": truncate(&v, 1000), "inline": v.chars().count() < 40 }))
                .collect();
            Ok((http_url(&a.url)?, json!({ "username": "Nebula", "embeds": [{ "title": title, "color": color, "fields": fields }] })))
        }
        "telegram" => {
            let token = a.token.trim();
            if token.is_empty() {
                return Err(t!("finish.missing.token"));
            }
            if a.chat_id.trim().is_empty() {
                return Err(t!("finish.missing.chat"));
            }
            let url = http_url(&format!("https://api.telegram.org/bot{}/sendMessage", token))?;
            Ok((url, json!({ "chat_id": a.chat_id.trim(), "text": format!("{}\n\n{}", title, body), "disable_web_page_preview": true })))
        }
        "ntfy" => {
            if a.topic.trim().is_empty() {
                return Err(t!("finish.missing.topic"));
            }
            let server = if a.url.trim().is_empty() { "https://ntfy.sh" } else { a.url.trim() };
            let tag = match ev.event.as_str() {
                "completed" => "white_check_mark",
                "failed" => "x",
                _ => "inbox_tray",
            };
            // Publicar en JSON en la raíz del servidor admite tildes y emojis en el título.
            Ok((
                http_url(server)?,
                json!({ "topic": a.topic.trim(), "title": title, "message": body, "tags": [tag], "priority": if ev.event == "failed" { 4 } else { 3 } }),
            ))
        }
        "webhook" => {
            if a.url.trim().is_empty() {
                return Err(t!("finish.missing.url"));
            }
            let mut v = serde_json::to_value(ev).map_err(|e| e.to_string())?;
            if let Some(m) = v.as_object_mut() {
                m.insert("app".into(), json!("Nebula"));
                m.insert("version".into(), json!(env!("CARGO_PKG_VERSION")));
                m.insert("title".into(), json!(title));
                m.insert("message".into(), json!(body));
            }
            Ok((http_url(&a.url)?, v))
        }
        other => Err(t!("finish.unknownKind", kind = other)),
    }
}

/// Mensaje de error de la respuesta: el campo de texto si es JSON (Telegram: "description";
/// Discord y ntfy: "message"/"error"), o el cuerpo tal cual.
fn error_detail(raw: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        for k in ["description", "message", "error"] {
            if let Some(s) = v.get(k).and_then(|x| x.as_str()).filter(|s| !s.trim().is_empty()) {
                return s.trim().to_string();
            }
        }
    }
    raw.trim().to_string()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

async fn notify(a: &FinishAction, ev: &Event) -> Result<(), String> {
    let (url, body) = request(a, ev)?;
    let mut req = HTTP
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_vec(&body).map_err(|e| e.to_string())?);
    if a.kind == "ntfy" && !a.token.trim().is_empty() {
        req = req.bearer_auth(a.token.trim());
    }
    // Sin la URL en el error: lleva el token de Telegram o el secreto del webhook.
    let res = req.send().await.map_err(|e| e.without_url().to_string())?;
    let status = res.status();
    if status.is_success() {
        return Ok(());
    }
    let raw = res.text().await.unwrap_or_default();
    let detail = truncate(&error_detail(&raw), 200);
    Err(if detail.is_empty() {
        t!("finish.httpStatus", status = status.as_u16())
    } else {
        t!("finish.httpDetail", status = status.as_u16(), detail = detail)
    })
}

// ---------------------------------------------------------------------------
// Script o programa
// ---------------------------------------------------------------------------

/// Variables de entorno que recibe el script.
pub fn script_env(ev: &Event) -> Vec<(&'static str, String)> {
    vec![
        ("NEBULA_EVENT", ev.event.clone()),
        ("NEBULA_NAME", ev.name.clone()),
        ("NEBULA_STATUS", ev.status.clone()),
        ("NEBULA_PATH", ev.path.clone()),
        ("NEBULA_SIZE", ev.size.to_string()),
        ("NEBULA_ERROR", ev.error.clone().unwrap_or_default()),
        ("NEBULA_STAGE", ev.stage.clone()),
    ]
}

#[cfg(target_os = "android")]
async fn run_script(_a: &FinishAction, _ev: &Event) -> Result<(), String> {
    Err(t!("finish.desktopOnly"))
}

#[cfg(not(target_os = "android"))]
async fn run_script(a: &FinishAction, ev: &Event) -> Result<(), String> {
    let path = std::path::PathBuf::from(a.path.trim());
    if a.path.trim().is_empty() {
        return Err(t!("finish.missing.script"));
    }
    if !path.is_file() {
        return Err(t!("finish.scriptNotFound", path = path.display()));
    }
    let ext = path.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let mut cmd = if ext == "ps1" {
        let mut c = tokio::process::Command::new("powershell.exe");
        c.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"]).arg(&path);
        c
    } else {
        tokio::process::Command::new(&path)
    };
    if let Some(dir) = path.parent() {
        cmd.current_dir(dir);
    }
    cmd.envs(script_env(ev))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let secs = a.timeout_secs.clamp(1, 3600);
    let child = cmd.spawn().map_err(|e| t!("finish.scriptStart", error = e))?;
    // Si se pasa del tiempo, al soltar el proceso (kill_on_drop) se termina.
    let out = match tokio::time::timeout(Duration::from_secs(secs as u64), child.wait_with_output()).await {
        Err(_) => return Err(t!("finish.scriptTimeout", secs = secs)),
        Ok(r) => r.map_err(|e| e.to_string())?,
    };
    if out.status.success() {
        linfo!("{}", t!("finish.scriptDone", action = label(a)));
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let tail = stderr.trim().lines().last().or_else(|| stdout.trim().lines().last()).unwrap_or("").trim().to_string();
    let code = out.status.code().map(|c| c.to_string()).unwrap_or_else(|| "?".into());
    Err(if tail.is_empty() {
        t!("finish.scriptExit", code = code)
    } else {
        t!("finish.scriptExitDetail", code = code, detail = truncate(&tail, 200))
    })
}

// ---------------------------------------------------------------------------
// Energía (Windows)
// ---------------------------------------------------------------------------

/// Cuenta atrás en curso, si la hay.
pub fn power_status(rt: &Runtime) -> Option<Countdown> {
    rt.power.lock().clone()
}

/// Empieza la cuenta atrás (sustituye a la que hubiera). Muestra la ventana y la notificación.
pub fn start_power(e: &Engine, mode: &str, test: bool) {
    let rt = &e.finish;
    let mode = match mode {
        "sleep" | "hibernate" => mode.to_string(),
        _ => "shutdown".to_string(),
    };
    let gen = rt.power_gen.fetch_add(1, Ordering::SeqCst) + 1;
    let secs = if test { POWER_TEST_SECS } else { POWER_SECS };
    let cd = Countdown { gen, mode: mode.clone(), remaining: secs, test };
    *rt.power.lock() = Some(cd.clone());
    linfo!("{}", t!("finish.powerStarted", action = power_name(&mode), secs = secs));
    bus::emit(bus::Event::PowerCountdown(cd));
    e.platform.refresh_power();
    e.platform.power_window(true);
    e.platform.notify_power(&power_name(&mode), &t!("finish.powerBody", secs = secs), &t!("finish.cancel"));
    let Some(e) = e.arc() else { return };
    crate::rt::spawn(async move {
        for left in (0..secs).rev() {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let mut cur = e.finish.power.lock();
            match cur.as_mut() {
                Some(c) if c.gen == gen => {
                    c.remaining = left;
                    bus::emit(bus::Event::PowerCountdown(c.clone()));
                }
                _ => return,
            }
            if left == 0 {
                *cur = None;
            }
        }
        e.platform.power_window(false);
        e.platform.refresh_power();
        if test {
            linfo!("{}", t!("finish.powerTestDone", action = power_name(&mode)));
            return;
        }
        linfo!("{}", t!("finish.powerNow", action = power_name(&mode)));
        e.save_queue();
        e.save_history();
        if let Err(err) = e.platform.power_action(&mode) {
            lerror!("{}", t!("finish.error", action = power_name(&mode), error = err));
        }
    });
}

/// Cancela la cuenta atrás. Devuelve si había una.
pub fn cancel_power(e: &Engine) -> bool {
    let Some(cd) = e.finish.power.lock().take() else { return false };
    lwarn!("{}", t!("finish.powerCancelled", action = power_name(&cd.mode)));
    bus::emit(bus::Event::PowerCancelled);
    e.platform.power_window(false);
    e.platform.refresh_power();
    true
}

/// Acción de energía que se dispara con la cola vacía, si hay una activa.
pub fn armed(s: &Settings) -> Option<&FinishAction> {
    s.finish_actions.iter().find(|a| a.kind == "power" && a.enabled && a.on_queue_empty)
}

/// Activa ("solo esta vez" si no lo estaba) o desactiva el apagado al vaciarse la cola: desde el
/// panel de velocidad y la bandeja.
pub fn set_quick_power(s: &mut Settings, mode: Option<&str>) {
    match mode {
        None => {
            for a in s.finish_actions.iter_mut().filter(|a| a.kind == "power") {
                a.enabled = false;
            }
        }
        Some(mode) => {
            let idx = s
                .finish_actions
                .iter()
                .position(|a| a.kind == "power" && a.enabled)
                .or_else(|| s.finish_actions.iter().position(|a| a.kind == "power" && a.once))
                .or_else(|| s.finish_actions.iter().position(|a| a.kind == "power"));
            let a = match idx {
                Some(i) => &mut s.finish_actions[i],
                None => {
                    s.finish_actions.push(FinishAction {
                        kind: "power".into(),
                        enabled: false,
                        on_complete: false,
                        on_failed: false,
                        ..Default::default()
                    });
                    s.finish_actions.last_mut().expect("acción")
                }
            };
            if !a.enabled {
                a.once = true;
            }
            a.enabled = true;
            a.on_queue_empty = true;
            a.power = mode.to_string();
        }
    }
}

/// Cambia el apagado rápido y lo guarda.
pub fn quick_power(e: &Engine, mode: Option<&str>) {
    let mut s = e.settings.read().clone();
    set_quick_power(&mut s, mode);
    save(e, s);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(kind: &str) -> FinishAction {
        FinishAction { kind: kind.into(), ..Default::default() }
    }

    #[test]
    fn requests_for_each_service() {
        let ev = sample_event();
        let mut d = action("discord");
        assert!(request(&d, &ev).is_err());
        d.url = "https://discord.com/api/webhooks/1/abc".into();
        let (url, body) = request(&d, &ev).unwrap();
        assert_eq!(url.as_str(), "https://discord.com/api/webhooks/1/abc");
        assert_eq!(body["embeds"][0]["fields"][0]["value"], ev.name);

        let mut tg = action("telegram");
        tg.token = "123:abc".into();
        assert!(request(&tg, &ev).is_err());
        tg.chat_id = "-1001".into();
        let (url, body) = request(&tg, &ev).unwrap();
        assert_eq!(url.as_str(), "https://api.telegram.org/bot123:abc/sendMessage");
        assert_eq!(body["chat_id"], "-1001");
        assert!(body["text"].as_str().unwrap().contains(&ev.name));

        let mut n = action("ntfy");
        n.topic = "descargas".into();
        let (url, body) = request(&n, &ev).unwrap();
        assert_eq!(url.as_str(), "https://ntfy.sh/");
        assert_eq!(body["topic"], "descargas");

        let mut w = action("webhook");
        w.url = "ftp://x".into();
        assert!(request(&w, &ev).is_err());
        w.url = "http://127.0.0.1:9000/hook".into();
        let (_, body) = request(&w, &ev).unwrap();
        assert_eq!(body["event"], "completed");
        assert_eq!(body["size"], ev.size);
        assert_eq!(body["app"], "Nebula");
    }

    #[test]
    fn message_has_every_field() {
        let mut ev = sample_event();
        ev.event = "failed".into();
        ev.status = "failed".into();
        ev.error = Some("Faltan artículos".into());
        let (_, body) = message(&ev);
        assert_eq!(body.lines().count(), 7, "{}", body);
        assert!(body.contains("Faltan artículos"));
        assert!(body.contains("1 min 35 s"));
        assert_eq!(fmt_duration(3720), "1 h 2 min");
    }

    #[test]
    fn error_details() {
        assert_eq!(error_detail(r#"{"ok":false,"error_code":401,"description":"Unauthorized"}"#), "Unauthorized");
        assert_eq!(error_detail(r#"{"message": "Unknown Webhook", "code": 10015}"#), "Unknown Webhook");
        assert_eq!(error_detail(" texto "), "texto");
    }

    #[test]
    fn triggers() {
        let mut a = action("discord");
        a.on_complete = true;
        a.on_failed = false;
        assert!(matches(&a, Trigger::Complete));
        assert!(!matches(&a, Trigger::Failed));
        assert!(!matches(&a, Trigger::QueueEmpty));
        a.enabled = false;
        assert!(!matches(&a, Trigger::Complete));
        // La energía solo con la cola vacía.
        let mut p = action("power");
        p.on_complete = true;
        p.on_queue_empty = true;
        assert!(!matches(&p, Trigger::Complete));
        assert!(matches(&p, Trigger::QueueEmpty));
    }

    #[test]
    fn quick_power_toggle() {
        let mut s = Settings::default();
        set_quick_power(&mut s, Some("hibernate"));
        let a = armed(&s).unwrap();
        assert!(a.once && a.power == "hibernate" && !a.on_complete);
        set_quick_power(&mut s, Some("sleep"));
        assert_eq!(s.finish_actions.len(), 1);
        assert_eq!(armed(&s).unwrap().power, "sleep");
        set_quick_power(&mut s, None);
        assert!(armed(&s).is_none());
        // Una acción fija (no "solo esta vez") que ya estaba activa se queda así.
        s.finish_actions[0].once = false;
        s.finish_actions[0].enabled = true;
        set_quick_power(&mut s, Some("shutdown"));
        assert!(!armed(&s).unwrap().once);
    }

    #[test]
    fn secrets_are_encrypted_on_disk() {
        let dir = std::env::temp_dir().join(format!("nebula-finish-{}", crate::util::new_id()));
        let paths = crate::config::AppPaths { data: dir.clone(), nzb: dir.join("nzb"), logs: dir.join("logs"), tools: dir.join("tools") };
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = Settings::default();
        let mut tg = action("telegram");
        tg.token = "123:secreto".into();
        let mut d = action("discord");
        d.url = "https://discord.com/api/webhooks/1/secreto".into();
        s.finish_actions = vec![tg, d];
        s.save(&paths).unwrap();
        let raw = std::fs::read_to_string(paths.config_file()).unwrap();
        assert!(!raw.contains("secreto"), "{}", raw);
        let back = Settings::load(&paths);
        assert_eq!(back.finish_actions[0].token, "123:secreto");
        assert_eq!(back.finish_actions[1].url, "https://discord.com/api/webhooks/1/secreto");
        assert!(back.finish_actions[0].token_enc.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn script_gets_env_and_times_out() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir = std::env::temp_dir().join(format!("nebula-script-{}", crate::util::new_id()));
            std::fs::create_dir_all(&dir).unwrap();
            let out = dir.join("out.txt");
            let script = dir.join("fin.sh");
            std::fs::write(&script, format!("#!/bin/sh\necho \"$NEBULA_NAME|$NEBULA_STATUS|$NEBULA_SIZE|$NEBULA_STAGE\" > '{}'\n", out.display())).unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            let mut a = action("script");
            a.path = script.to_string_lossy().to_string();
            let ev = sample_event();
            run(&a, &ev).await.unwrap();
            let got = std::fs::read_to_string(&out).unwrap();
            assert_eq!(got.trim(), format!("{}|completed|{}|", ev.name, ev.size));

            std::fs::write(&script, "#!/bin/sh\nsleep 5\n").unwrap();
            a.timeout_secs = 1;
            let start = std::time::Instant::now();
            assert!(run(&a, &ev).await.is_err());
            assert!(start.elapsed() < Duration::from_secs(4));

            std::fs::write(&script, "#!/bin/sh\necho roto >&2\nexit 3\n").unwrap();
            let err = run(&a, &ev).await.unwrap_err();
            assert!(err.contains('3') && err.contains("roto"), "{}", err);
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
