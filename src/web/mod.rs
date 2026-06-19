//! Web server module — serves a browser-based terminal client over WebSocket.
//!
//! Conditionally compiled behind `#[cfg(feature = "web")]`.

mod assets;
mod auth;
mod bridge;
mod messages;
pub(crate) mod origin;
mod ws;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::server::client_transport::ServerEvent;

pub use auth::generate_token;
pub(crate) use origin::Origin;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

pub struct WebServerConfig {
    pub bind_addr: SocketAddr,
    pub token: String,
    pub tls: Option<TlsConfig>,
    pub session_ttl: Duration,
    pub idle_timeout: Duration,
    pub trust_proxy: bool,
    pub public_origins: Vec<Origin>,
}

#[allow(dead_code)]
pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

const MAX_CONCURRENT_WS: usize = 16;

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

#[derive(Debug, PartialEq, Eq)]
pub enum WebConfigError {
    TrustProxyRequiresPublicOrigin,
    InvalidPublicOrigin(String),
    MixedSchemePublicOrigins,
}

impl std::fmt::Display for WebConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TrustProxyRequiresPublicOrigin => {
                write!(f, "--trust-proxy requires at least one --public-origin")
            }
            Self::InvalidPublicOrigin(s) => {
                write!(f, "invalid --public-origin: {s:?}")
            }
            Self::MixedSchemePublicOrigins => {
                write!(
                    f,
                    "--public-origin values mix http and https schemes; all must use the same scheme"
                )
            }
        }
    }
}

pub fn validate_web_config(
    trust_proxy: bool,
    public_origins: &[Origin],
    raw_origins: &[String],
) -> Result<(), WebConfigError> {
    if trust_proxy && public_origins.is_empty() {
        return Err(WebConfigError::TrustProxyRequiresPublicOrigin);
    }

    for raw in raw_origins {
        if origin::normalize_origin(raw).is_none() {
            return Err(WebConfigError::InvalidPublicOrigin(raw.clone()));
        }
    }

    if public_origins.len() > 1 {
        let first_secure = public_origins[0].secure;
        if public_origins.iter().any(|o| o.secure != first_secure) {
            return Err(WebConfigError::MixedSchemePublicOrigins);
        }
    }

    Ok(())
}

pub fn derive_cookie_secure(tls_enabled: bool, public_origins: &[Origin]) -> bool {
    tls_enabled || public_origins.iter().any(|o| o.secure)
}

pub fn session_cookie(session_id: &str, ttl: Duration, secure: bool) -> String {
    let mut value = format!(
        "herdr_session={session_id}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
        ttl.as_secs()
    );
    if secure {
        value.push_str("; Secure");
    }
    value
}

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
    cookie_secure: bool,
    session_ttl: Duration,
    trust_proxy: bool,
    public_origins: Arc<Vec<Origin>>,
    active_connections: Arc<AtomicUsize>,
    max_connections: usize,
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
    let cookie_secure = derive_cookie_secure(tls_enabled, &config.public_origins);

    let shared = AppState {
        auth: Arc::new(auth_state),
        server_event_tx,
        next_client_id,
        idle_timeout: config.idle_timeout,
        cancellation: cancellation.clone(),
        cookie_secure,
        session_ttl: config.session_ttl,
        trust_proxy: config.trust_proxy,
        public_origins: Arc::new(config.public_origins),
        active_connections: Arc::new(AtomicUsize::new(0)),
        max_connections: MAX_CONCURRENT_WS,
    };

    if config.trust_proxy {
        warn!("trust-proxy mode: herdr is NOT authenticating clients; the upstream gateway MUST authenticate all requests");
    }

    let app = Router::new()
        .route("/", get(assets::serve_index))
        .route("/assets/{*path}", get(assets::serve_asset))
        .route("/auth", post(handle_auth))
        .route("/config.json", get(handle_config))
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

async fn handle_config(State(state): State<AppState>) -> impl IntoResponse {
    let mode = if state.trust_proxy {
        "trust-proxy"
    } else {
        "standalone"
    };
    let body = format!(r#"{{"mode":"{mode}"}}"#);
    (
        StatusCode::OK,
        [
            ("content-type", "application/json"),
            ("cache-control", "no-store"),
        ],
        body,
    )
}

#[derive(serde::Deserialize)]
struct AuthRequest {
    token: String,
}

async fn handle_auth(
    State(state): State<AppState>,
    axum::Json(body): axum::Json<AuthRequest>,
) -> impl IntoResponse {
    if state.trust_proxy {
        return (
            StatusCode::NOT_FOUND,
            axum::http::HeaderMap::new(),
            axum::Json(serde_json::json!({"error": "not_found"})),
        );
    }

    if !state.auth.validate_token(&body.token) {
        return (
            StatusCode::UNAUTHORIZED,
            axum::http::HeaderMap::new(),
            axum::Json(serde_json::json!({"error": "unauthorized"})),
        );
    }

    let session_id = state.auth.create_session();
    let cookie_value = session_cookie(&session_id, state.session_ttl, state.cookie_secure);

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_web_config_trust_proxy_requires_origins() {
        let err = validate_web_config(true, &[], &[]).unwrap_err();
        assert_eq!(err, WebConfigError::TrustProxyRequiresPublicOrigin);
    }

    #[test]
    fn validate_web_config_invalid_origin() {
        let err =
            validate_web_config(false, &[], &["not-a-url".to_string()]).unwrap_err();
        assert!(matches!(err, WebConfigError::InvalidPublicOrigin(_)));
    }

    #[test]
    fn validate_web_config_mixed_scheme() {
        let origins = vec![
            origin::normalize_origin("http://a.com").unwrap(),
            origin::normalize_origin("https://b.com").unwrap(),
        ];
        let err = validate_web_config(false, &origins, &[]).unwrap_err();
        assert_eq!(err, WebConfigError::MixedSchemePublicOrigins);
    }

    #[test]
    fn validate_web_config_valid_trust_proxy() {
        let origins = vec![origin::normalize_origin("https://test.com").unwrap()];
        assert!(validate_web_config(true, &origins, &[]).is_ok());
    }

    #[test]
    fn validate_web_config_valid_standalone_no_origins() {
        assert!(validate_web_config(false, &[], &[]).is_ok());
    }

    #[test]
    fn derive_cookie_secure_tls() {
        assert!(derive_cookie_secure(true, &[]));
    }

    #[test]
    fn derive_cookie_secure_https_origin() {
        let origins = vec![origin::normalize_origin("https://test.com").unwrap()];
        assert!(derive_cookie_secure(false, &origins));
    }

    #[test]
    fn derive_cookie_secure_no_tls_no_https() {
        assert!(!derive_cookie_secure(false, &[]));
    }

    #[test]
    fn session_cookie_basic() {
        let cookie = session_cookie("abc123", Duration::from_secs(3600), false);
        assert!(cookie.contains("herdr_session=abc123"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Path=/"));
        assert!(cookie.contains("Max-Age=3600"));
        assert!(!cookie.contains("Secure"));
    }

    #[test]
    fn session_cookie_secure() {
        let cookie = session_cookie("abc123", Duration::from_secs(3600), true);
        assert!(cookie.contains("; Secure"));
    }
}
