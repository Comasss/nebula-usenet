//! Rutas HTTP del servidor, todas en un mismo puerto:
//! - `/` y lo demás: la interfaz (Svelte), embebida en el ejecutable.
//! - `/api/v1/auth…`: estado del acceso, primer arranque, entrar y salir.
//! - `/api/v1/invoke/<comando>`: los comandos de la interfaz (ver rpc.rs).
//! - `/api/v1/events` (WebSocket) y `/api/v1/events/sse`: los eventos del motor.
//! - `/api`, `/sabnzbd/api`, `/newznab/api`: las APIs Newznab y SABnzbd (Sonarr, Radarr…).
//! - `/health`: para Docker y los monitores.
use crate::auth::{self, Auth};
use axum::body::{Body, Bytes};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use nebula_core::engine::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;

/// Como la API propia: un NZB subido por SABnzbd o desde la interfaz puede ser grande.
const MAX_BODY: usize = 100 * 1024 * 1024;

pub struct AppState {
    pub engine: Arc<Engine>,
    pub auth: Auth,
}

type S = Arc<AppState>;

pub fn router(state: S) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/auth", get(auth_status))
        .route("/api/v1/auth/setup", post(auth_setup))
        .route("/api/v1/auth/login", post(auth_login))
        .route("/api/v1/auth/logout", post(auth_logout))
        .route("/api/v1/invoke/{cmd}", post(invoke))
        .route("/api/v1/events", get(events_ws))
        .route("/api/v1/events/sse", get(events_sse))
        .route("/api", any(compat))
        .route("/api/", any(compat))
        .route("/sabnzbd", any(compat))
        .route("/sabnzbd/api", any(compat))
        .route("/newznab", any(compat))
        .route("/newznab/api", any(compat))
        .fallback(ui)
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(state)
}

fn error(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(json!({ "error": msg.into() }))).into_response()
}

/// Sesión de la interfaz (cookie) o la API key de Nebula en `X-Api-Key` (CLI, scripts).
fn authorized(st: &AppState, headers: &HeaderMap) -> bool {
    if let Some(c) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()).and_then(|h| auth::cookie_value(h, auth::COOKIE)) {
        if st.auth.verify(c) {
            return true;
        }
    }
    let key = st.engine.settings.read().api_key.trim().to_string();
    let given = headers.get("x-api-key").and_then(|v| v.to_str().ok()).unwrap_or("").trim();
    !key.is_empty() && given.len() == key.len() && given.bytes().zip(key.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0
}

fn session_cookie(value: &str, max_age: u64) -> HeaderValue {
    HeaderValue::from_str(&format!("{}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}", auth::COOKIE, value, max_age))
        .unwrap_or_else(|_| HeaderValue::from_static(""))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "version": nebula_core::VERSION }))
}

// ---- Acceso ----

async fn auth_status(State(st): State<S>, headers: HeaderMap) -> Json<Value> {
    let ok = !st.auth.needs_setup() && authorized(&st, &headers);
    Json(json!({
        "needsSetup": st.auth.needs_setup(),
        "user": if ok { Some(st.auth.username()) } else { None },
        "minPassword": auth::MIN_PASSWORD,
        "version": nebula_core::VERSION,
    }))
}

#[derive(Deserialize)]
struct Credentials {
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
}

fn logged_in(st: &AppState) -> Response {
    let Some(cookie) = st.auth.issue() else { return error(StatusCode::INTERNAL_SERVER_ERROR, t!("err.internal")) };
    let mut r = Json(json!({ "user": st.auth.username() })).into_response();
    r.headers_mut().insert(header::SET_COOKIE, session_cookie(&cookie, auth::SESSION_SECS));
    r
}

/// Primer arranque: crea el usuario. Solo mientras no exista ninguno.
async fn auth_setup(State(st): State<S>, Json(c): Json<Credentials>) -> Response {
    if !st.auth.needs_setup() {
        return error(StatusCode::FORBIDDEN, t!("server.auth.alreadySetup"));
    }
    if let Err(e) = st.auth.set(&c.username, &c.password) {
        return error(StatusCode::BAD_REQUEST, e);
    }
    linfo!("{}", t!("log.server.userCreated", user = st.auth.username()));
    logged_in(&st)
}

async fn auth_login(State(st): State<S>, ConnectInfo(peer): ConnectInfo<SocketAddr>, Json(c): Json<Credentials>) -> Response {
    if st.auth.check(&c.username, &c.password) {
        return logged_in(&st);
    }
    lwarn!("{}", t!("log.server.badLogin", from = peer.ip()));
    // Frena los intentos a ciegas.
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    error(StatusCode::UNAUTHORIZED, t!("server.auth.bad"))
}

async fn auth_logout() -> Response {
    let mut r = Json(json!({})).into_response();
    r.headers_mut().insert(header::SET_COOKIE, session_cookie("", 0));
    r
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PasswordChange {
    #[serde(default)]
    current: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
}

// ---- Comandos ----

async fn invoke(State(st): State<S>, Path(cmd): Path<String>, headers: HeaderMap, body: Bytes) -> Response {
    if !authorized(&st, &headers) {
        return error(StatusCode::UNAUTHORIZED, t!("server.auth.required"));
    }
    let e = &st.engine;
    let res = match cmd.as_str() {
        // El .nzb llega tal cual en el cuerpo, con el nombre en `x-name` (como en la app).
        "import_nzb" => {
            let name = headers.get("x-name").and_then(|v| v.to_str().ok()).map(nebula_core::util::percent_decode).unwrap_or_default();
            e.import_nzb(&name, &body).map(Value::from)
        }
        "auth_change" => match serde_json::from_slice::<PasswordChange>(&body) {
            Ok(c) if st.auth.check(&st.auth.username(), &c.current) => {
                let user = if c.username.trim().is_empty() { st.auth.username() } else { c.username };
                match st.auth.set(&user, &c.password) {
                    Ok(()) => return logged_in(&st),
                    Err(e) => Err(e),
                }
            }
            Ok(_) => Err(t!("server.auth.badCurrent")),
            Err(err) => Err(err.to_string()),
        },
        _ => {
            let args: Value = if body.is_empty() { Value::Null } else { serde_json::from_slice(&body).unwrap_or(Value::Null) };
            crate::rpc::call(e, &cmd, &args).await
        }
    };
    match res {
        Ok(v) => Json(v).into_response(),
        Err(msg) => error(StatusCode::BAD_REQUEST, msg),
    }
}

// ---- Eventos ----

async fn events_ws(State(st): State<S>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    if !authorized(&st, &headers) {
        return error(StatusCode::UNAUTHORIZED, t!("server.auth.required"));
    }
    ws.on_upgrade(forward_ws)
}

async fn forward_ws(mut socket: WebSocket) {
    let mut rx = nebula_core::bus::subscribe();
    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Ok(ev) => {
                    let text = serde_json::to_string(&ev).unwrap_or_default();
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            },
            msg = socket.recv() => match msg {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
        }
    }
}

async fn events_sse(State(st): State<S>, headers: HeaderMap) -> Response {
    if !authorized(&st, &headers) {
        return error(StatusCode::UNAUTHORIZED, t!("server.auth.required"));
    }
    let rx = nebula_core::bus::subscribe();
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let data = serde_json::to_string(&ev.payload()).unwrap_or_default();
                    let item: Result<SseEvent, Infallible> = Ok(SseEvent::default().event(ev.name()).data(data));
                    return Some((item, rx));
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

// ---- APIs Newznab y SABnzbd ----

async fn compat(State(st): State<S>, ConnectInfo(peer): ConnectInfo<SocketAddr>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    let hs: Vec<(String, String)> = headers.iter().filter_map(|(k, v)| Some((k.as_str().to_string(), v.to_str().ok()?.to_string()))).collect();
    let r = nebula_core::api::handle_http(&st.engine, method.as_str(), uri.path(), uri.query().unwrap_or(""), hs, &body, peer.ip()).await;
    let mut resp = Response::builder()
        .status(StatusCode::from_u16(r.status).unwrap_or(StatusCode::OK))
        .header(header::CONTENT_TYPE, r.content_type)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    for (k, v) in r.headers {
        resp = resp.header(k, v);
    }
    let body = if method == Method::HEAD { Body::empty() } else { Body::from(r.body) };
    resp.body(body).unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

// ---- Interfaz ----

#[derive(rust_embed::Embed)]
#[folder = "$CARGO_MANIFEST_DIR/../../dist"]
#[allow_missing = true]
struct Ui;

const NO_UI: &str = "<!doctype html><meta charset=utf-8><title>Nebula</title><body style=\"font-family:sans-serif;background:#0a0b10;color:#eee;padding:2em\"><h1>Nebula</h1><p>This build has no web interface. Run <code>npm run build</code> and build nebula-server again.</p><p>The APIs work: <a style=\"color:#9ad\" href=\"/health\">/health</a>.</p>";

async fn ui(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    if let Some(f) = Ui::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        // Los archivos de assets/ llevan un hash en el nombre: no cambian nunca.
        let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
        return ([(header::CONTENT_TYPE, mime.as_ref().to_string()), (header::CACHE_CONTROL, cache.to_string())], f.data.into_owned()).into_response();
    }
    // Rutas de la aplicación (sin extensión): la página principal.
    if !path.rsplit('/').next().unwrap_or("").contains('.') {
        return match Ui::get("index.html") {
            Some(f) => ([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], f.data.into_owned()).into_response(),
            None => ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], NO_UI).into_response(),
        };
    }
    StatusCode::NOT_FOUND.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::connect_info::MockConnectInfo;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use nebula_core::config::{AppPaths, Settings};
    use tower::ServiceExt;

    fn app() -> (Router, Arc<AppState>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("nebula-server-{}", uuid::Uuid::new_v4()));
        let paths = AppPaths::with(dir.clone(), dir.join("tools"));
        let settings = Settings { api_key: "clave123".into(), ..Default::default() };
        let engine = Engine::new(Arc::new(nebula_core::Headless), paths, settings);
        let st = Arc::new(AppState { engine, auth: Auth::load(&dir) });
        let r = router(st.clone()).layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 9))));
        (r, st, dir)
    }

    async fn send(r: &Router, method: &str, uri: &str, headers: &[(&str, &str)], body: &str) -> (StatusCode, HeaderMap, Value) {
        let mut b = Request::builder().method(method).uri(uri);
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        let resp = r.clone().oneshot(b.body(Body::from(body.to_string())).unwrap()).await.unwrap();
        let (parts, body) = resp.into_parts();
        let bytes = body.collect().await.unwrap().to_bytes();
        (parts.status, parts.headers, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn setup_login_and_invoke() {
        let (r, _st, dir) = app();
        let json_ct = ("content-type", "application/json");
        let (s, _, v) = send(&r, "GET", "/health", &[], "").await;
        assert_eq!((s, v["status"].as_str()), (StatusCode::OK, Some("ok")));
        let (_, _, v) = send(&r, "GET", "/api/v1/auth", &[], "").await;
        assert_eq!(v["needsSetup"], true);
        // Sin usuario no hay comandos.
        let (s, _, _) = send(&r, "POST", "/api/v1/invoke/get_queue", &[], "").await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        // Primer arranque: crea el usuario y deja la sesión abierta.
        let (s, h, _) = send(&r, "POST", "/api/v1/auth/setup", &[json_ct], r#"{"username":"admin","password":"contraseña segura"}"#).await;
        assert_eq!(s, StatusCode::OK);
        let cookie = h.get(header::SET_COOKIE).unwrap().to_str().unwrap().split(';').next().unwrap().to_string();
        // Otra vez no se puede.
        let (s, _, _) = send(&r, "POST", "/api/v1/auth/setup", &[json_ct], r#"{"username":"x","password":"otra contraseña"}"#).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, _, v) = send(&r, "POST", "/api/v1/invoke/get_queue", &[("cookie", &cookie)], "").await;
        assert_eq!(s, StatusCode::OK);
        assert!(v["jobs"].is_array());
        let (_, _, v) = send(&r, "GET", "/api/v1/auth", &[("cookie", &cookie)], "").await;
        assert_eq!(v["user"], "admin");
        // Con la API key también (CLI, scripts).
        let (s, _, _) = send(&r, "POST", "/api/v1/invoke/get_history", &[("x-api-key", "clave123")], "").await;
        assert_eq!(s, StatusCode::OK);
        let (s, _, _) = send(&r, "POST", "/api/v1/invoke/get_history", &[("x-api-key", "mala")], "").await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        // Argumentos en camelCase como en la app, y errores con su texto.
        let (s, _, v) = send(&r, "POST", "/api/v1/invoke/remove_job", &[("cookie", &cookie), json_ct], r#"{"id":"nada","deleteFiles":true}"#).await;
        assert_eq!((s, v), (StatusCode::OK, Value::Null));
        let (s, _, v) = send(&r, "POST", "/api/v1/invoke/no_existe", &[("cookie", &cookie)], "").await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert!(v["error"].as_str().unwrap().contains("no_existe"));
        // Un .nzb subido llega a incoming/ con su nombre.
        let (s, _, v) = send(&r, "POST", "/api/v1/invoke/import_nzb", &[("cookie", &cookie), ("x-name", "Mi%20serie.nzb")], "<nzb/>").await;
        assert_eq!(s, StatusCode::OK);
        assert!(v.as_str().unwrap().ends_with("Mi serie.nzb"));
        // Entrar con la contraseña equivocada / buena.
        let (s, _, _) = send(&r, "POST", "/api/v1/auth/login", &[json_ct], r#"{"username":"admin","password":"mal"}"#).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, h, _) = send(&r, "POST", "/api/v1/auth/login", &[json_ct], r#"{"username":"ADMIN","password":"contraseña segura"}"#).await;
        assert_eq!(s, StatusCode::OK);
        assert!(h.get(header::SET_COOKIE).unwrap().to_str().unwrap().contains("HttpOnly"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn compat_apis_on_the_same_port() {
        let (r, _st, dir) = app();
        let (s, _, v) = send(&r, "GET", "/sabnzbd/api?mode=version", &[], "").await;
        assert_eq!((s, v["version"].as_str()), (StatusCode::OK, Some(nebula_core::sab::SAB_VERSION)));
        let (_, _, v) = send(&r, "GET", "/api?mode=queue&apikey=mala", &[], "").await;
        assert_eq!(v["error"], "API Key Incorrect");
        let (s, _, v) = send(&r, "GET", "/api?mode=queue&apikey=clave123", &[], "").await;
        assert_eq!(s, StatusCode::OK);
        assert!(v["queue"]["slots"].is_array());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
