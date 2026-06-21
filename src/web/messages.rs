use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Browser → Server (text frame JSON)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ClientControl {
    Hello {
        cols: u16,
        rows: u16,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    Paste {
        text: String,
    },
}

// ---------------------------------------------------------------------------
// Server → Browser (text frame JSON)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ServerControl {
    WindowTitle {
        title: Option<String>,
    },
    Clipboard {
        data: String,
    },
    Notify {
        kind: String,
        message: String,
        body: Option<String>,
    },
    MouseCapture {
        enabled: bool,
    },
}

// ---------------------------------------------------------------------------
// Bridge → WebSocket write task
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) enum BridgeMessage {
    Binary(Vec<u8>),
    Text(String),
    Close(Option<String>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_hello() {
        let json = r#"{"type": "hello", "cols": 80, "rows": 24}"#;
        let msg: ClientControl = serde_json::from_str(json).unwrap();
        match msg {
            ClientControl::Hello { cols, rows } => {
                assert_eq!(cols, 80);
                assert_eq!(rows, 24);
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn deserialize_resize() {
        let json = r#"{"type": "resize", "cols": 120, "rows": 40}"#;
        let msg: ClientControl = serde_json::from_str(json).unwrap();
        match msg {
            ClientControl::Resize { cols, rows } => {
                assert_eq!(cols, 120);
                assert_eq!(rows, 40);
            }
            _ => panic!("expected Resize"),
        }
    }

    #[test]
    fn deserialize_paste() {
        let json = r#"{"type": "paste", "text": "hello world"}"#;
        let msg: ClientControl = serde_json::from_str(json).unwrap();
        match msg {
            ClientControl::Paste { text } => assert_eq!(text, "hello world"),
            _ => panic!("expected Paste"),
        }
    }

    #[test]
    fn serialize_window_title() {
        let msg = ServerControl::WindowTitle {
            title: Some("test".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"window_title\""));
        assert!(json.contains("\"title\":\"test\""));
    }

    #[test]
    fn serialize_clipboard() {
        let msg = ServerControl::Clipboard { data: "abc".into() };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"clipboard\""));
        assert!(json.contains("\"data\":\"abc\""));
    }

    #[test]
    fn serialize_notify() {
        let msg = ServerControl::Notify {
            kind: "sound".into(),
            message: "done".into(),
            body: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"notify\""));
        assert!(json.contains("\"message\":\"done\""));
    }

    #[test]
    fn serialize_mouse_capture() {
        let msg = ServerControl::MouseCapture { enabled: true };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"mouse_capture\""));
        assert!(json.contains("\"enabled\":true"));
    }
}
