use crate::{
    config::Config,
    engine::{Engine, hash_password, verify_password},
};
use axum::{
    Json, Router,
    extract::Request,
    extract::{
        ConnectInfo, OriginalUri, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::broadcast;

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

#[derive(Clone)]
pub struct App {
    pub engine: Arc<Mutex<Engine>>,
    pub attempts: Arc<Mutex<HashMap<SocketAddr, (Instant, u32)>>>,
    pub changes: broadcast::Sender<()>,
    pub config: Arc<Config>,
    pub connections: Arc<tokio::sync::Semaphore>,
    pub password_jobs: Arc<tokio::sync::Semaphore>,
    pub stopping: Arc<std::sync::atomic::AtomicBool>,
}
type ApiError = (StatusCode, Json<serde_json::Value>);
fn error(code: StatusCode, message: &str) -> ApiError {
    (code, Json(json!({ "error": message })))
}

pub fn router(app: App) -> Router {
    let routes = Router::new()
        .route("/", get(assets))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/health", get(|| async { Json(json!({ "ok": true })) }))
        .route("/ws", get(upgrade))
        .fallback(assets);
    let routes = if app.config.base_url == "/" {
        routes
    } else {
        let base_url = app.config.base_url.clone();
        let prefix = base_url.trim_end_matches('/').to_owned();
        Router::new()
            .route(
                &prefix,
                get(move || async move { Redirect::permanent(&base_url) }),
            )
            .nest(&app.config.base_url, routes)
    };
    routes
        .layer(axum::extract::DefaultBodyLimit::max(4096))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(app.clone(), security))
        .with_state(app)
}
async fn assets(State(app): State<App>, OriginalUri(uri): OriginalUri) -> Response {
    let asset_path = uri
        .path()
        .strip_prefix(app.config.base_url.trim_end_matches('/'))
        .unwrap_or(uri.path());
    let path = if asset_path == "/" {
        "/index.html"
    } else {
        asset_path
    };
    if let Some((_, bytes)) = ASSETS.iter().find(|(name, _)| *name == path) {
        let content_type = if path.ends_with(".html") {
            "text/html; charset=utf-8"
        } else if path.ends_with(".js") {
            "text/javascript; charset=utf-8"
        } else if path.ends_with(".css") {
            "text/css; charset=utf-8"
        } else if path.ends_with(".svg") {
            "image/svg+xml"
        } else {
            "application/octet-stream"
        };
        if path == "/index.html" {
            let html = String::from_utf8_lossy(bytes).replace(
                "<head>",
                &format!("<head>\n    <base href=\"{}\">", app.config.base_url),
            );
            return ([(header::CONTENT_TYPE, content_type)], html).into_response();
        }
        return ([(header::CONTENT_TYPE, content_type)], *bytes).into_response();
    }
    (StatusCode::NOT_FOUND, "Asset not found. In development, open http://localhost:5173. Release: build the frontend before cargo build --release.").into_response()
}
async fn security(State(app): State<App>, request: Request, next: Next) -> Response {
    // Only a trusted, TLS-terminating proxy may reach the production listener.
    if app.config.production
        && !request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .is_some_and(|peer| trusted_proxy(&app.config, peer.0.ip()))
    {
        return (
            StatusCode::FORBIDDEN,
            "Request must come from the trusted proxy.",
        )
            .into_response();
    }
    if app.config.production
        && request
            .headers()
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            != Some("https")
    {
        return (
            StatusCode::BAD_REQUEST,
            "HTTPS is required through the trusted reverse proxy.",
        )
            .into_response();
    }
    let mut response = next.run(request).await;
    let h = response.headers_mut();
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if app.config.production {
        h.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000"),
        );
        let websocket_origin = app.config.origins[0].replacen("https://", "wss://", 1);
        let policy = format!(
            "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self' {websocket_origin}; frame-ancestors 'none'; base-uri 'none'; form-action 'self'; upgrade-insecure-requests"
        );
        h.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_str(&policy).expect("Validated origin"),
        );
    }
    response
}
fn trusted_proxy(config: &Config, peer: std::net::IpAddr) -> bool {
    peer.to_canonical() == config.trust.to_canonical()
}
fn check_origin(app: &App, headers: &HeaderMap) -> Result<(), ApiError> {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if app.config.origins.iter().any(|o| o == origin) {
        Ok(())
    } else {
        Err(error(StatusCode::FORBIDDEN, "Untrusted request origin."))
    }
}
fn token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| part.trim().strip_prefix("chat_session="))
}
fn authenticate(app: &App, headers: &HeaderMap) -> Result<String, ApiError> {
    if app.stopping.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Server is shutting down.",
        ));
    }
    token(headers)
        .and_then(|t| app.engine.lock().unwrap().session(t))
        .ok_or_else(|| error(StatusCode::UNAUTHORIZED, "Please log in."))
}
#[derive(Deserialize)]
struct Login {
    username: String,
    password: String,
}
async fn login(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<Login>,
) -> Result<Response, ApiError> {
    check_origin(&app, &headers)?;
    if input.username.len() > 32 || input.password.len() > 128 {
        return Err(error(
            StatusCode::UNAUTHORIZED,
            "Invalid username or password.",
        ));
    }
    {
        let mut attempts = app.attempts.lock().unwrap();
        attempts.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(60));
        // Key on IP, never the client's ephemeral TCP port.
        let key = SocketAddr::new(peer.ip(), 0);
        let entry = attempts.entry(key).or_insert((Instant::now(), 0));
        if entry.1 >= 10 {
            return Err(error(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many login attempts. Wait a minute.",
            ));
        }
        entry.1 += 1;
    }
    let account = app
        .engine
        .lock()
        .unwrap()
        .data
        .users
        .get(&input.username)
        .cloned()
        .filter(|u| !u.disabled);
    let expected = account.as_ref().map(|a| a.hash.clone());
    let password = input.password;
    let permit = app.password_jobs.clone().try_acquire_owned().map_err(|_| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Login busy. Please try again shortly.",
        )
    })?;
    let valid = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        expected.is_some_and(|h| verify_password(&password, &h))
    })
    .await
    .unwrap_or(false);
    if !valid {
        return Err(error(
            StatusCode::UNAUTHORIZED,
            "Invalid username or password.",
        ));
    }
    let value = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    app.engine
        .lock()
        .unwrap()
        .login(
            value.clone(),
            &input.username,
            &account.unwrap().hash,
            token(&headers),
        )
        .map_err(|e| error(StatusCode::SERVICE_UNAVAILABLE, &e))?;
    tracing::info!(user = %input.username, "Login succeeded");
    let mut response = Json(json!({ "ok": true })).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie(&app, &value, 43200)).unwrap(),
    );
    Ok(response)
}
fn cookie(app: &App, value: &str, age: u32) -> String {
    format!(
        "chat_session={value}; HttpOnly; SameSite=Strict; Path={}; Max-Age={age}{}",
        app.config.base_url,
        if app.config.production {
            "; Secure"
        } else {
            ""
        }
    )
}
async fn logout(State(app): State<App>, headers: HeaderMap) -> Result<Response, ApiError> {
    check_origin(&app, &headers)?;
    if let Some(token) = token(&headers) {
        app.engine
            .lock()
            .unwrap()
            .logout(token)
            .map_err(|e| error(StatusCode::SERVICE_UNAVAILABLE, &e))?;
    }
    let _ = app.changes.send(());
    let mut response = Json(json!({ "ok": true })).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie(&app, "", 0)).unwrap(),
    );
    Ok(response)
}
async fn me(
    State(app): State<App>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let username = authenticate(&app, &headers)?;
    let snapshot = app.engine.lock().unwrap().snapshot(&username);
    Ok(Json(json!(snapshot)))
}
async fn upgrade(
    State(app): State<App>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    check_origin(&app, &headers)?;
    let username = authenticate(&app, &headers)?;
    let permit = app
        .connections
        .clone()
        .try_acquire_owned()
        .map_err(|_| error(StatusCode::SERVICE_UNAVAILABLE, "Connection limit reached."))?;
    Ok(ws
        .max_message_size(65536)
        .max_frame_size(65536)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            connection(app, headers, username, socket).await;
        }))
}
#[derive(Deserialize)]
struct Input {
    id: u64,
    room: Option<String>,
    text: String,
}
async fn execute_input(
    app: &App,
    headers: &HeaderMap,
    user: &str,
    input: &Input,
) -> Result<(String, bool), String> {
    let parts: Vec<_> = input.text.split_whitespace().collect();
    if parts.first() == Some(&"/configs") {
        if !app.engine.lock().unwrap().is_admin(user) {
            return Err("Admin permission required.".into());
        }
        if parts.len() != 1 {
            return Err("Usage: /configs".into());
        }
        return app.config.display().map(|text| (text, false));
    }
    let passwd = parts.first() == Some(&"/passwd");
    if !passwd && !matches!(parts.first(), Some(&"/user") | Some(&"/reset")) {
        let mut engine = app.engine.lock().unwrap();
        let before = engine.revision;
        let reply = engine.execute(Some(user), input.room.as_deref(), &input.text)?;
        return Ok((reply, engine.revision != before));
    }
    if !passwd && !app.engine.lock().unwrap().is_admin(user) {
        return Err("Admin permission required.".into());
    }
    let reset = parts[0] == "/reset";
    if input.text.len() > 4000
        || ((reset || passwd) && parts.len() != 3)
        || (!reset && !passwd && !(3..=4).contains(&parts.len()))
    {
        return Err(if passwd {
            "Usage: /passwd old new"
        } else {
            "Usage: /user name password [admin|user] or /reset name password"
        }
        .into());
    }
    if !passwd
        && !reset
        && parts
            .get(3)
            .is_some_and(|role| !["admin", "user"].contains(role))
    {
        return Err("Role must be admin or user.".into());
    }
    let expected = if passwd {
        Some(
            app.engine
                .lock()
                .unwrap()
                .data
                .users
                .get(user)
                .filter(|u| !u.disabled)
                .ok_or("Account unavailable.")?
                .hash
                .clone(),
        )
    } else {
        None
    };
    let old = parts[1].to_owned();
    let password = parts[2].to_owned();
    let expected_job = expected.clone();
    let permit = app
        .password_jobs
        .clone()
        .try_acquire_owned()
        .map_err(|_| "Account management busy. Try again shortly.")?;
    let hash = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if let Some(hash) = expected_job
            && (old.len() > 128 || !verify_password(&old, &hash))
        {
            return Err("Old password is incorrect.".into());
        }
        hash_password(&password)
    })
    .await
    .map_err(|_| "Password hashing failed.")??;
    let mut engine = app.engine.lock().unwrap();
    let current = token(headers).ok_or("Please log in.")?;
    if engine.session(current).as_deref() != Some(user) {
        return Err("Please log in.".into());
    }
    if passwd {
        return engine
            .change_password(user, expected.as_deref().unwrap(), hash, current)
            .map(|text| (text, true));
    }
    // Recheck access after hashing, without constructing a chat snapshot.
    if !engine.is_admin(user) {
        return Err("Admin permission required.".into());
    }
    engine
        .provision(parts[1], hash, parts.get(3) == Some(&"admin"), reset)
        .map(|text| (text, true))
}
async fn snapshot(app: &App, user: &str, socket: &mut WebSocket) -> bool {
    let value = app
        .engine
        .lock()
        .unwrap()
        .snapshot(user)
        .map(|s| serde_json::to_string(&s).unwrap());
    match value {
        Some(value) => send(socket, Message::Text(value.into())).await,
        None => false,
    }
}
async fn send(socket: &mut WebSocket, message: Message) -> bool {
    tokio::time::timeout(Duration::from_secs(5), socket.send(message))
        .await
        .is_ok_and(|r| r.is_ok())
}
async fn connection(app: App, headers: HeaderMap, user: String, mut socket: WebSocket) {
    let mut changes = app.changes.subscribe();
    if !snapshot(&app, &user, &mut socket).await {
        return;
    }
    let mut heartbeat = tokio::time::interval(Duration::from_secs(25));
    let mut budget = (Instant::now(), 0u32);
    let mut last_seen = Instant::now();
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if authenticate(&app, &headers).is_err() || last_seen.elapsed() > Duration::from_secs(75) { break; }
                if !send(&mut socket, Message::Ping(Vec::new().into())).await { break; }
            },
            change = changes.recv() => {
                if matches!(change, Err(broadcast::error::RecvError::Closed)) || authenticate(&app, &headers).is_err() { break; }
                if !snapshot(&app, &user, &mut socket).await { break; }
            },
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { break; };
                last_seen = Instant::now();
                if authenticate(&app, &headers).is_err() { break; }
                match message {
                    Message::Text(raw) => {
                        if budget.0.elapsed() > Duration::from_secs(10) { budget = (Instant::now(), 0); }
                        budget.1 += 1;
                        let parsed = serde_json::from_str::<Input>(&raw);
                        let (id, result) = match parsed {
                            Ok(input) if budget.1 <= 30 => (input.id, execute_input(&app, &headers, &user, &input).await),
                            Ok(input) => (input.id, Err("Slow down; at most 30 messages per 10 seconds.".into())),
                            Err(_) => (0, Err("Invalid message format.".into())),
                        };
                        let reply = match &result { Ok((text, _)) => json!({ "kind": "notice", "id": id, "text": text }), Err(text) => json!({ "kind": "error", "id": id, "text": text }) };
                        if !send(&mut socket, Message::Text(reply.to_string().into())).await { break; }
                        if matches!(result, Ok((_, true))) { let _ = app.changes.send(()); }
                    },
                    Message::Close(_) => break,
                    Message::Binary(_) => break,
                    _ => {},
                }
            }
        }
    }
    let _ = send(&mut socket, Message::Close(None)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proxy_trust_uses_the_socket_peer() {
        let config = Config::default();
        assert!(trusted_proxy(&config, "127.0.0.1".parse().unwrap()));
        assert!(trusted_proxy(&config, "::ffff:127.0.0.1".parse().unwrap()));
        assert!(!trusted_proxy(&config, "127.0.0.2".parse().unwrap()));
        assert!(!trusted_proxy(&config, "10.0.0.2".parse().unwrap()));
    }
    #[test]
    fn cookies_are_http_only_and_secure_in_production() {
        let config = Config {
            bind: String::new(),
            origins: vec![],
            production: true,
            ..Config::default()
        };
        let (changes, _) = broadcast::channel(1);
        let app = App {
            engine: Arc::new(Mutex::new(
                Engine::open(std::path::Path::new(":memory:")).unwrap(),
            )),
            attempts: Default::default(),
            changes,
            config: Arc::new(config),
            connections: Arc::new(tokio::sync::Semaphore::new(128)),
            password_jobs: Arc::new(tokio::sync::Semaphore::new(2)),
            stopping: Default::default(),
        };
        let value = cookie(&app, "token", 43200);
        assert!(value.contains("HttpOnly"));
        assert!(value.contains("; Secure"));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://evil.example"),
        );
        assert!(check_origin(&app, &headers).is_err());
    }
}
