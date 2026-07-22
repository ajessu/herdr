use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::protocol::{NotifyKind, ServerMessage, TerminalFrame};
use crate::server::client_transport::ServerEvent;

use super::messages::{BridgeMessage, ServerControl};

const RECV_TIMEOUT: Duration = Duration::from_millis(100);

pub(crate) fn spawn_bridge_thread(
    control_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    render_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    ws_tx: mpsc::Sender<BridgeMessage>,
    server_event_tx: mpsc::Sender<ServerEvent>,
    client_id: u64,
    cancel: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        bridge_loop(
            control_rx,
            render_rx,
            ws_tx,
            server_event_tx,
            client_id,
            cancel,
        );
    })
}

fn bridge_loop(
    control_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    render_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    ws_tx: mpsc::Sender<BridgeMessage>,
    server_event_tx: mpsc::Sender<ServerEvent>,
    client_id: u64,
    cancel: Arc<AtomicBool>,
) {
    loop {
        if cancel.load(Ordering::Relaxed) || ws_tx.is_closed() {
            break;
        }

        // Prioritize control messages.
        match control_rx.try_recv() {
            Ok(data) => {
                if let Some(msg) = decode_and_convert(&data) {
                    if ws_tx.blocking_send(msg).is_err() {
                        break;
                    }
                }
                continue;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }

        match render_rx.recv_timeout(RECV_TIMEOUT) {
            Ok(data) => {
                // Mirror native client_writer_loop ordering: signal drained as
                // soon as the slot is empty so the server can produce the next
                // frame in parallel with the WS write.
                let _ =
                    server_event_tx.blocking_send(ServerEvent::ClientWriterDrained { client_id });
                if let Some(msg) = decode_and_convert(&data) {
                    if ws_tx.blocking_send(msg).is_err() {
                        break;
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    debug!(client_id, "bridge thread exiting");
}

fn decode_and_convert(data: &[u8]) -> Option<BridgeMessage> {
    let msg = deserialize_server_message(data)?;
    convert_server_message(msg)
}

pub(crate) fn deserialize_server_message(data: &[u8]) -> Option<ServerMessage> {
    if data.len() < 4 {
        warn!("bridge: frame too short ({} bytes)", data.len());
        return None;
    }

    let len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let payload = &data[4..];

    if payload.len() < len {
        warn!(
            "bridge: payload shorter than declared length ({} < {len})",
            payload.len()
        );
        return None;
    }

    let config = bincode::config::standard();
    match bincode::serde::decode_from_slice::<ServerMessage, _>(&payload[..len], config) {
        Ok((msg, _)) => Some(msg),
        Err(err) => {
            warn!("bridge: bincode decode failed: {err}");
            None
        }
    }
}

fn as_text(ctrl: ServerControl) -> Option<BridgeMessage> {
    serde_json::to_string(&ctrl).ok().map(BridgeMessage::Text)
}

fn convert_server_message(msg: ServerMessage) -> Option<BridgeMessage> {
    match msg {
        ServerMessage::Terminal(TerminalFrame { bytes, .. }) => Some(BridgeMessage::Binary(bytes)),
        ServerMessage::WindowTitle { title } => as_text(ServerControl::WindowTitle { title }),
        ServerMessage::Clipboard { data } => as_text(ServerControl::Clipboard { data }),
        ServerMessage::Notify {
            kind,
            message,
            body,
        } => {
            let kind_str = match kind {
                NotifyKind::Sound => "sound",
                NotifyKind::Toast => "toast",
                NotifyKind::SystemToast => "system_toast",
            };
            as_text(ServerControl::Notify {
                kind: kind_str.to_owned(),
                message,
                body,
            })
        }
        ServerMessage::MouseCapture { enabled } => as_text(ServerControl::MouseCapture { enabled }),
        ServerMessage::ServerShutdown { reason } => Some(BridgeMessage::Close(reason)),
        ServerMessage::Graphics { .. } | ServerMessage::ReloadSoundConfig => None,
        // ASCII input-source switching is a local-terminal IME concern; the
        // web client has no OS input source to swap.
        ServerMessage::PrefixInputSource { .. } => None,
        ServerMessage::Welcome { .. } | ServerMessage::Frame(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{NotifyKind, ServerMessage, TerminalFrame};

    fn encode_message(msg: &ServerMessage) -> Vec<u8> {
        let config = bincode::config::standard();
        let payload = bincode::serde::encode_to_vec(msg, config).unwrap();
        let len = (payload.len() as u32).to_le_bytes();
        let mut data = Vec::with_capacity(4 + payload.len());
        data.extend_from_slice(&len);
        data.extend_from_slice(&payload);
        data
    }

    #[test]
    fn deserialize_terminal_frame() {
        let msg = ServerMessage::Terminal(TerminalFrame {
            seq: 1,
            width: 80,
            height: 24,
            full: true,
            bytes: vec![0x1b, 0x5b, 0x48],
        });
        let data = encode_message(&msg);
        let decoded = deserialize_server_message(&data).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn convert_terminal_to_binary() {
        let msg = ServerMessage::Terminal(TerminalFrame {
            seq: 1,
            width: 80,
            height: 24,
            full: false,
            bytes: vec![1, 2, 3],
        });
        let result = convert_server_message(msg).unwrap();
        match result {
            BridgeMessage::Binary(bytes) => assert_eq!(bytes, vec![1, 2, 3]),
            _ => panic!("expected Binary"),
        }
    }

    #[test]
    fn convert_window_title_to_json() {
        let msg = ServerMessage::WindowTitle {
            title: Some("test".into()),
        };
        let result = convert_server_message(msg).unwrap();
        match result {
            BridgeMessage::Text(json) => {
                assert!(json.contains("window_title"));
                assert!(json.contains("test"));
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn convert_clipboard_to_json() {
        let msg = ServerMessage::Clipboard {
            data: "base64data".into(),
        };
        let result = convert_server_message(msg).unwrap();
        match result {
            BridgeMessage::Text(json) => {
                assert!(json.contains("clipboard"));
                assert!(json.contains("base64data"));
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn convert_notify_to_json() {
        let msg = ServerMessage::Notify {
            kind: NotifyKind::Sound,
            message: "agent done".into(),
            body: None,
        };
        let result = convert_server_message(msg).unwrap();
        match result {
            BridgeMessage::Text(json) => {
                assert!(json.contains("notify"));
                assert!(json.contains("sound"));
                assert!(json.contains("agent done"));
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn convert_mouse_capture_to_json() {
        let msg = ServerMessage::MouseCapture { enabled: true };
        let result = convert_server_message(msg).unwrap();
        match result {
            BridgeMessage::Text(json) => {
                assert!(json.contains("mouse_capture"));
                assert!(json.contains("true"));
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn convert_shutdown_to_close() {
        let msg = ServerMessage::ServerShutdown {
            reason: Some("bye".into()),
        };
        let result = convert_server_message(msg).unwrap();
        match result {
            BridgeMessage::Close(reason) => assert_eq!(reason, Some("bye".into())),
            _ => panic!("expected Close"),
        }
    }

    #[test]
    fn graphics_message_dropped() {
        let msg = ServerMessage::Graphics {
            bytes: vec![1, 2, 3],
        };
        assert!(convert_server_message(msg).is_none());
    }

    #[test]
    fn reload_sound_config_dropped() {
        let msg = ServerMessage::ReloadSoundConfig;
        assert!(convert_server_message(msg).is_none());
    }

    #[test]
    fn short_data_returns_none() {
        assert!(deserialize_server_message(&[0, 0]).is_none());
    }

    #[test]
    fn truncated_payload_returns_none() {
        let mut data = vec![10, 0, 0, 0]; // claims 10 bytes
        data.extend_from_slice(&[0, 1, 2]); // only 3 bytes
        assert!(deserialize_server_message(&data).is_none());
    }
}
