//! Web server module — serves a browser-based terminal client over WebSocket.
//!
//! Conditionally compiled behind `#[cfg(feature = "web")]`.

mod assets;
mod auth;
mod bridge;
mod messages;
mod origin;
mod ws;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::server::client_transport::ServerEvent;

pub use auth::generate_token;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

pub struct WebServerConfig {
    pub bind_addr: SocketAddr,
    pub token: String,
    pub tls: Option<TlsConfig>,
    pub session_ttl: Duration,
    pub idle_timeout: Duration,
}

#[allow(dead_code)]
pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum WebServerError {
    BindFailed(std::io::Error),
    NonLocalhostWithoutTls,
}

impl std::fmt::Display for WebServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BindFailed(err) => write!(f, "failed to bind web server: {err}"),
            Self::NonLocalhostWithoutTls => {
                write!(f, "non-localhost bind requires TLS certificate and key")
            }
        }
    }
}

impl std::error::Error for WebServerError {}

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) struct AppState {
    auth: Arc<auth::WebAuthState>,
    server_event_tx: tokio::sync::mpsc::Sender<ServerEvent>,
    next_client_id: Arc<AtomicU64>,
    #[allow(dead_code)]
    idle_timeout: Duration,
    cancellation: CancellationToken,
    tls_enabled: bool,
    session_ttl: Duration,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn is_localhost(addr: &SocketAddr) -> bool {
    addr.ip().is_loopback()
}

/// Synchronously validate config and bind the TCP listener. Separated from
/// `serve_web_server` so the caller can report bind failures (port in use,
/// NonLocalhostWithoutTls, etc.) synchronously instead of via a spawned task.
pub fn bind_web_server(config: &WebServerConfig) -> Result<std::net::TcpListener, WebServerError> {
    if !is_localhost(&config.bind_addr) && config.tls.is_none() {
        return Err(WebServerError::NonLocalhostWithoutTls);
    }
    let listener =
        std::net::TcpListener::bind(config.bind_addr).map_err(WebServerError::BindFailed)?;
    listener
        .set_nonblocking(true)
        .map_err(WebServerError::BindFailed)?;
    Ok(listener)
}

pub async fn serve_web_server(
    listener: std::net::TcpListener,
    config: WebServerConfig,
    server_event_tx: tokio::sync::mpsc::Sender<ServerEvent>,
    next_client_id: Arc<AtomicU64>,
    cancellation: CancellationToken,
) -> Result<(), WebServerError> {
    let auth_state = auth::WebAuthState::new(&config.token, config.session_ttl);
    let tls_enabled = config.tls.is_some();

    let shared = AppState {
        auth: Arc::new(auth_state),
        server_event_tx,
        next_client_id,
        idle_timeout: config.idle_timeout,
        cancellation: cancellation.clone(),
        tls_enabled,
        session_ttl: config.session_ttl,
    };

    let app = Router::new()
        .route("/", get(assets::serve_index))
        .route("/assets/{*path}", get(assets::serve_asset))
        .route("/auth", post(handle_auth))
        .route("/ws", get(ws::ws_handler))
        .route("/healthz", get(handle_healthz))
        .with_state(shared);

    let tokio_listener = TcpListener::from_std(listener).map_err(WebServerError::BindFailed)?;
    let local_addr = tokio_listener
        .local_addr()
        .map_err(WebServerError::BindFailed)?;
    info!(addr = %local_addr, "web server listening");

    axum::serve(tokio_listener, app)
        .with_graceful_shutdown(cancellation.cancelled_owned())
        .await
        .map_err(WebServerError::BindFailed)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Route handlers
// ---------------------------------------------------------------------------

async fn handle_healthz() -> impl IntoResponse {
    StatusCode::OK
}

#[derive(serde::Deserialize)]
struct AuthRequest {
    token: String,
}

async fn handle_auth(
    State(state): State<AppState>,
    axum::Json(body): axum::Json<AuthRequest>,
) -> impl IntoResponse {
    if !state.auth.validate_token(&body.token) {
        return (
            StatusCode::UNAUTHORIZED,
            axum::http::HeaderMap::new(),
            axum::Json(serde_json::json!({"error": "unauthorized"})),
        );
    }

    let session_id = state.auth.create_session();

    let mut cookie_value = format!(
        "herdr_session={session_id}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
        state.session_ttl.as_secs()
    );
    if state.tls_enabled {
        cookie_value.push_str("; Secure");
    }

    let mut response_headers = axum::http::HeaderMap::new();
    if let Ok(value) = cookie_value.parse() {
        response_headers.insert(axum::http::header::SET_COOKIE, value);
    }

    (
        StatusCode::OK,
        response_headers,
        axum::Json(serde_json::json!({"ok": true})),
    )
}
