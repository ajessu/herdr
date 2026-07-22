use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::protocol::RenderEncoding;
use crate::server::client_transport::{ClientWriter, ServerEvent};

use super::messages::{BridgeMessage, ClientControl};
use super::origin::{validate_origin, OriginDecision};
use super::{bridge, AppState};

const WS_CHANNEL_SIZE: usize = 64;
const MAX_TEXT_FRAME: usize = 64 * 1024;
/// Matches `client_transport::MAX_INPUT_PAYLOAD` for parity with native clients.
const MAX_INPUT_PAYLOAD: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WsGate {
    Accept,
    Reject401,
    Reject403,
}

pub(crate) fn ws_gate_decision(
    trust_proxy: bool,
    session_valid: bool,
    origin: &OriginDecision,
) -> WsGate {
    match origin {
        OriginDecision::Accept => {}
        OriginDecision::Reject { .. } => return WsGate::Reject403,
    }

    if trust_proxy {
        return WsGate::Accept;
    }

    if session_valid {
        WsGate::Accept
    } else {
        WsGate::Reject401
    }
}

pub(crate) async fn ws_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let session_valid = if state.trust_proxy {
        false
    } else {
        extract_session_cookie(&headers)
            .is_some_and(|session_id| state.auth.validate_session(&session_id))
    };

    let allow_same_origin = !state.trust_proxy;
    let origin_decision = validate_origin(&headers, &state.public_origins, allow_same_origin);

    let gate = ws_gate_decision(state.trust_proxy, session_valid, &origin_decision);

    match gate {
        WsGate::Accept => {}
        WsGate::Reject401 => {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        WsGate::Reject403 => {
            if let OriginDecision::Reject { reason, raw } = &origin_decision {
                warn!(
                    reason = ?reason,
                    raw_origin = ?raw,
                    allowed = ?state.public_origins.as_slice(),
                    "websocket origin rejected"
                );
            }
            return StatusCode::FORBIDDEN.into_response();
        }
    }

    let active = state.active_connections.load(Ordering::Relaxed);
    if active >= state.max_connections {
        warn!(
            active,
            cap = state.max_connections,
            "connection cap reached, rejecting upgrade"
        );
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }

    state.active_connections.fetch_add(1, Ordering::Relaxed);
    let conn_guard = ConnectionGuard(state.active_connections.clone());

    ws.max_message_size(1024 * 1024)
        .on_upgrade(move |socket| handle_websocket(socket, state, conn_guard))
        .into_response()
}

struct ConnectionGuard(Arc<std::sync::atomic::AtomicUsize>);

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

fn extract_session_cookie(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get("cookie")?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let trimmed = part.trim();
        if let Some(value) = trimmed.strip_prefix("herdr_session=") {
            return Some(value.to_owned());
        }
    }
    None
}

async fn handle_websocket(mut socket: WebSocket, state: AppState, _conn_guard: ConnectionGuard) {
    let (cols, rows) = match wait_for_hello(&mut socket).await {
        Some(dims) => dims,
        None => {
            debug!("websocket closed before hello");
            return;
        }
    };

    let client_id = state.next_client_id.fetch_add(1, Ordering::Relaxed);

    info!(client_id, cols, rows, "web client connected");

    let (control_tx, control_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let (render_tx, render_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(1);
    let writer = ClientWriter::from_channels(control_tx, render_tx);

    if state
        .server_event_tx
        .send(ServerEvent::ClientConnected {
            client_id,
            cols,
            rows,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::TerminalAnsi,
            keybindings: None,
            direct_attach_requested: false,
            writer,
        })
        .await
        .is_err()
    {
        warn!(client_id, "server event channel closed");
        return;
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let (ws_tx, mut ws_rx) = mpsc::channel::<BridgeMessage>(WS_CHANNEL_SIZE);
    let bridge_handle = bridge::spawn_bridge_thread(
        control_rx,
        render_rx,
        ws_tx,
        state.server_event_tx.clone(),
        client_id,
        cancel.clone(),
    );

    loop {
        tokio::select! {
            msg = ws_rx.recv() => {
                match msg {
                    Some(BridgeMessage::Binary(data)) => {
                        if socket.send(Message::Binary(data.into())).await.is_err() {
                            break;
                        }
                    }
                    Some(BridgeMessage::Text(text)) => {
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Some(BridgeMessage::Close(reason)) => {
                        let _ = socket.send(Message::Close(Some(axum::extract::ws::CloseFrame {
                            code: 1000,
                            reason: reason.unwrap_or_default().into(),
                        }))).await;
                        break;
                    }
                    None => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        if data.len() > MAX_INPUT_PAYLOAD {
                            warn!(client_id, len = data.len(), "oversized binary input, dropping client");
                            break;
                        }
                        let _ = state.server_event_tx.send(ServerEvent::ClientInput {
                            client_id,
                            data: data.to_vec(),
                        }).await;
                    }
                    Some(Ok(Message::Text(text))) => {
                        if text.len() > MAX_TEXT_FRAME {
                            warn!(client_id, len = text.len(), "oversized text frame, ignoring");
                            continue;
                        }
                        handle_text_message(client_id, &text, &state.server_event_tx).await;
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = socket.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Err(err)) => {
                        debug!(client_id, err = %err, "websocket error");
                        break;
                    }
                }
            }
            _ = state.cancellation.cancelled() => {
                let _ = socket.send(Message::Close(Some(axum::extract::ws::CloseFrame {
                    code: 1001,
                    reason: "server shutting down".into(),
                }))).await;
                break;
            }
        }
    }

    cancel.store(true, Ordering::Relaxed);
    let _ = state
        .server_event_tx
        .send(ServerEvent::ClientDisconnected { client_id })
        .await;

    // spawn_blocking so the bridge thread's join doesn't stall the async runtime.
    tokio::task::spawn_blocking(move || {
        let _ = bridge_handle.join();
    })
    .await
    .ok();

    info!(client_id, "web client disconnected");
}

async fn wait_for_hello(socket: &mut WebSocket) -> Option<(u16, u16)> {
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(5), socket.recv()).await;
    match timeout {
        Ok(Some(Ok(Message::Text(text)))) => {
            let msg: ClientControl = serde_json::from_str(&text).ok()?;
            match msg {
                ClientControl::Hello { cols, rows } => Some((cols.max(1), rows.max(1))),
                _ => None,
            }
        }
        _ => None,
    }
}

async fn handle_text_message(
    client_id: u64,
    text: &str,
    server_event_tx: &mpsc::Sender<ServerEvent>,
) {
    let Ok(msg) = serde_json::from_str::<ClientControl>(text) else {
        warn!(client_id, "malformed text frame, ignoring");
        return;
    };

    match msg {
        ClientControl::Resize { cols, rows } => {
            let _ = server_event_tx
                .send(ServerEvent::ClientResize {
                    client_id,
                    cols: cols.max(1),
                    rows: rows.max(1),
                    cell_width_px: 0,
                    cell_height_px: 0,
                })
                .await;
        }
        ClientControl::Paste { text } => {
            if text.len() > MAX_INPUT_PAYLOAD {
                warn!(client_id, len = text.len(), "oversized paste, ignoring");
                return;
            }
            let _ = server_event_tx
                .send(ServerEvent::ClientInput {
                    client_id,
                    data: text.into_bytes(),
                })
                .await;
        }
        ClientControl::Hello { .. } => {
            // Duplicate hello after connection established — ignore.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::origin::{OriginDecision, OriginReject};

    #[test]
    fn gate_standalone_valid_session_valid_origin() {
        assert_eq!(
            ws_gate_decision(false, true, &OriginDecision::Accept),
            WsGate::Accept
        );
    }

    #[test]
    fn gate_standalone_invalid_session_valid_origin() {
        assert_eq!(
            ws_gate_decision(false, false, &OriginDecision::Accept),
            WsGate::Reject401
        );
    }

    #[test]
    fn gate_standalone_valid_session_invalid_origin() {
        let reject = OriginDecision::Reject {
            reason: OriginReject::NotAllowed,
            raw: Some("http://evil.com".into()),
        };
        assert_eq!(ws_gate_decision(false, true, &reject), WsGate::Reject403);
    }

    #[test]
    fn gate_standalone_invalid_session_invalid_origin() {
        let reject = OriginDecision::Reject {
            reason: OriginReject::NotAllowed,
            raw: Some("http://evil.com".into()),
        };
        assert_eq!(ws_gate_decision(false, false, &reject), WsGate::Reject403);
    }

    #[test]
    fn gate_trust_proxy_valid_origin() {
        assert_eq!(
            ws_gate_decision(true, false, &OriginDecision::Accept),
            WsGate::Accept
        );
    }

    #[test]
    fn gate_trust_proxy_invalid_origin() {
        let reject = OriginDecision::Reject {
            reason: OriginReject::NotAllowed,
            raw: Some("http://evil.com".into()),
        };
        assert_eq!(ws_gate_decision(true, false, &reject), WsGate::Reject403);
    }

    #[test]
    fn gate_trust_proxy_ignores_session() {
        assert_eq!(
            ws_gate_decision(true, true, &OriginDecision::Accept),
            WsGate::Accept
        );
        assert_eq!(
            ws_gate_decision(true, false, &OriginDecision::Accept),
            WsGate::Accept
        );
    }

    #[test]
    fn gate_trust_proxy_never_401() {
        let reject = OriginDecision::Reject {
            reason: OriginReject::Missing,
            raw: None,
        };
        assert_eq!(ws_gate_decision(true, false, &reject), WsGate::Reject403);
    }
}
