#![cfg(feature = "web")]

mod support;

use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use http::Uri;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use support::{
    cleanup_test_base, register_runtime_dir, register_spawned_herdr_pid,
    unregister_spawned_herdr_pid,
};
use tokio_tungstenite::tungstenite;

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    PathBuf::from(format!(
        "/tmp/herdr-web-test-{}-{nanos}",
        std::process::id()
    ))
}

struct SpawnedHerdr {
    _master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
}

impl Drop for SpawnedHerdr {
    fn drop(&mut self) {
        let pid = self.child.process_id();
        let _ = self.child.kill();

        if let Some(pid) = pid {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                let mut status = 0;
                let result =
                    unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) };
                if result == pid as libc::pid_t || result == -1 {
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
            unregister_spawned_herdr_pid(Some(pid));
        }
    }
}

fn test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn wait_for_socket(path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() && UnixStream::connect(path).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("socket did not appear at {}", path.display());
}

fn spawn_server(config_home: &Path, runtime_dir: &Path, socket_path: &Path) -> SpawnedHerdr {
    fs::create_dir_all(config_home.join("herdr")).unwrap();
    fs::create_dir_all(runtime_dir).unwrap();
    register_runtime_dir(runtime_dir);
    fs::write(
        config_home.join("herdr/config.toml"),
        "onboarding = false\n",
    )
    .unwrap();

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();

    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_herdr"));
    cmd.arg("server");
    cmd.env("XDG_CONFIG_HOME", config_home);
    cmd.env("XDG_RUNTIME_DIR", runtime_dir);
    cmd.env("HERDR_SOCKET_PATH", socket_path);
    cmd.env_remove("HERDR_CLIENT_SOCKET_PATH");
    cmd.env("SHELL", "/bin/sh");
    cmd.env_remove("HERDR_ENV");

    let child = pair.slave.spawn_command(cmd).unwrap();
    register_spawned_herdr_pid(child.process_id());

    SpawnedHerdr {
        _master: pair.master,
        child,
    }
}

fn cleanup_spawned_herdr(spawned: SpawnedHerdr, base: PathBuf) {
    drop(spawned);
    cleanup_test_base(&base);
}

// ---------------------------------------------------------------------------
// JSON-line API helpers (synchronous, on the Unix socket)
// ---------------------------------------------------------------------------

struct JsonLineReader {
    stream: UnixStream,
    buf: Vec<u8>,
}

impl JsonLineReader {
    fn connect(socket_path: &Path) -> Self {
        Self {
            stream: UnixStream::connect(socket_path).unwrap(),
            buf: Vec::new(),
        }
    }

    fn send_line(&mut self, json: &str) {
        self.stream.write_all(json.as_bytes()).unwrap();
        self.stream.write_all(b"\n").unwrap();
        self.stream.flush().unwrap();
    }

    fn read_json_line(&mut self, timeout: Duration) -> serde_json::Value {
        let deadline = Instant::now() + timeout;
        self.stream.set_nonblocking(true).unwrap();

        loop {
            if Instant::now() >= deadline {
                self.stream.set_nonblocking(false).unwrap();
                panic!("timed out waiting for json line");
            }

            if let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
                let line = String::from_utf8(self.buf.drain(..=pos).collect()).unwrap();
                self.stream.set_nonblocking(false).unwrap();
                return serde_json::from_str(&line).unwrap();
            }

            let mut bytes = [0u8; 1024];
            match self.stream.read(&mut bytes) {
                Ok(0) => panic!("stream closed while waiting for json line"),
                Ok(n) => self.buf.extend_from_slice(&bytes[..n]),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("failed to read json line: {err}"),
            }
        }
    }
}

fn send_request(socket_path: &Path, json: &str) -> serde_json::Value {
    let mut reader = JsonLineReader::connect(socket_path);
    reader.send_line(json);
    reader.read_json_line(Duration::from_secs(5))
}

// ---------------------------------------------------------------------------
// Web helpers
// ---------------------------------------------------------------------------

struct WebTestEnv {
    base: PathBuf,
    spawned: SpawnedHerdr,
    #[allow(dead_code)]
    api_socket: PathBuf,
    web_url: String,
    token: String,
}

fn setup_web_env() -> WebTestEnv {
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("herdr.sock");

    let spawned = spawn_server(&config_home, &runtime_dir, &api_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));

    let created = send_request(
        &api_socket,
        &format!(
            r#"{{"id":"ws_create","method":"workspace.create","params":{{"cwd":"{}","focus":true}}}}"#,
            base.display()
        ),
    );
    assert_eq!(
        created["result"]["type"], "workspace_created",
        "workspace create failed: {created}"
    );

    let token = "test_token_abc123";
    let web_start = send_request(
        &api_socket,
        &format!(
            r#"{{"id":"web_start","method":"web.start","params":{{"bind_addr":"127.0.0.1:0","token":"{token}"}}}}"#,
        ),
    );
    let web_url = web_start["result"]["url"]
        .as_str()
        .unwrap_or_else(|| panic!("web.start failed: {web_start}"))
        .to_string();

    WebTestEnv {
        base,
        spawned,
        api_socket,
        web_url,
        token: token.to_string(),
    }
}

async fn authenticate(web_url: &str, token: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let uri: Uri = web_url.parse().unwrap();
    let host = uri.authority().unwrap().to_string();

    let mut stream = TcpStream::connect(&host).await.unwrap();
    let body = format!(r#"{{"token":"{token}"}}"#);
    let request = format!(
        "POST /auth HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();

    assert!(
        response.starts_with("HTTP/1.1 200"),
        "auth failed: {response}"
    );

    for line in response.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("set-cookie:") {
            let value = &line["set-cookie:".len()..].trim();
            if let Some(session) = value.strip_prefix("herdr_session=") {
                let end = session.find(';').unwrap_or(session.len());
                return session[..end].to_string();
            }
        }
    }
    panic!("no herdr_session cookie in response: {response}");
}

async fn connect_ws(
    web_url: &str,
    session_cookie: &str,
    cols: u16,
    rows: u16,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let uri: Uri = web_url.parse().unwrap();
    let host = uri.authority().unwrap().to_string();
    let ws_url = format!("ws://{host}/ws");

    let request = tungstenite::http::Request::builder()
        .uri(&ws_url)
        .header("Host", &host)
        .header("Origin", format!("http://{host}"))
        .header("Cookie", format!("herdr_session={session_cookie}"))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tungstenite::handshake::client::generate_key(),
        )
        .body(())
        .unwrap();

    let (ws_stream, _response) = tokio_tungstenite::connect_async(request).await.unwrap();

    let hello = format!(r#"{{"type":"hello","cols":{cols},"rows":{rows}}}"#);
    let (mut write, read) = ws_stream.split();
    write
        .send(tungstenite::Message::Text(hello.into()))
        .await
        .unwrap();

    read.reunite(write).unwrap()
}

async fn wait_for_output(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    expected: &str,
    timeout: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline - tokio::time::Instant::now();
        if remaining.is_zero() {
            return false;
        }
        match tokio::time::timeout(remaining, ws.next()).await {
            Ok(Some(Ok(tungstenite::Message::Binary(data)))) => {
                let text = String::from_utf8_lossy(&data);
                if text.contains(expected) {
                    return true;
                }
            }
            Ok(Some(Ok(_))) => continue,
            _ => return false,
        }
    }
}

async fn wait_for_any_binary(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    timeout: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline - tokio::time::Instant::now();
        if remaining.is_zero() {
            return false;
        }
        match tokio::time::timeout(remaining, ws.next()).await {
            Ok(Some(Ok(tungstenite::Message::Binary(_)))) => return true,
            Ok(Some(Ok(_))) => continue,
            _ => return false,
        }
    }
}

/// Drain binary frames for `window`, returning the concatenated render bytes
/// with ANSI escape sequences stripped. The server positions characters with
/// cursor-move escapes, so callers must strip control bytes before matching
/// visible text such as `stty size` output.
async fn collect_visible_text(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    window: Duration,
) -> String {
    let deadline = tokio::time::Instant::now() + window;
    let mut raw = Vec::new();
    loop {
        let remaining = deadline - tokio::time::Instant::now();
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, ws.next()).await {
            Ok(Some(Ok(tungstenite::Message::Binary(data)))) => raw.extend_from_slice(&data),
            Ok(Some(Ok(_))) => continue,
            _ => break,
        }
    }
    strip_ansi(&raw)
}

/// Remove CSI/OSC escape sequences and other C0 control bytes so visible glyphs
/// from adjacent grid cells concatenate into matchable text.
fn strip_ansi(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                Some('[') => {
                    // CSI: consume until a final byte in 0x40..=0x7e.
                    for f in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&f) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC: consume until BEL or ESC\.
                    while let Some(f) = chars.next() {
                        if f == '\u{07}' {
                            break;
                        }
                        if f == '\u{1b}' {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            },
            c if (c.is_control() && c != ' ') => {}
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Trust-proxy helpers
// ---------------------------------------------------------------------------

struct TrustProxyTestEnv {
    base: PathBuf,
    spawned: SpawnedHerdr,
    #[allow(dead_code)]
    api_socket: PathBuf,
    web_url: String,
}

fn setup_trust_proxy_env(public_origins: &[&str]) -> TrustProxyTestEnv {
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("herdr.sock");

    let spawned = spawn_server(&config_home, &runtime_dir, &api_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));

    let created = send_request(
        &api_socket,
        &format!(
            r#"{{"id":"ws_create","method":"workspace.create","params":{{"cwd":"{}","focus":true}}}}"#,
            base.display()
        ),
    );
    assert_eq!(
        created["result"]["type"], "workspace_created",
        "workspace create failed: {created}"
    );

    let origins_json: Vec<String> = public_origins
        .iter()
        .map(|o| format!("\"{o}\""))
        .collect();
    let origins_array = format!("[{}]", origins_json.join(","));

    let web_start = send_request(
        &api_socket,
        &format!(
            r#"{{"id":"web_start","method":"web.start","params":{{"bind_addr":"127.0.0.1:0","trust_proxy":true,"public_origins":{origins_array}}}}}"#,
        ),
    );
    let web_url = web_start["result"]["url"]
        .as_str()
        .unwrap_or_else(|| panic!("web.start trust-proxy failed: {web_start}"))
        .to_string();

    TrustProxyTestEnv {
        base,
        spawned,
        api_socket,
        web_url,
    }
}

async fn raw_http_get(web_url: &str, path: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let uri: http::Uri = web_url.parse().unwrap();
    let host = uri.authority().unwrap().to_string();

    let mut stream = TcpStream::connect(&host).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

async fn raw_http_post(web_url: &str, path: &str, body: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let uri: http::Uri = web_url.parse().unwrap();
    let host = uri.authority().unwrap().to_string();

    let mut stream = TcpStream::connect(&host).await.unwrap();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

async fn try_ws_upgrade(
    web_url: &str,
    origin: &str,
    cookie: Option<&str>,
) -> Result<
    (
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        tungstenite::http::Response<Option<Vec<u8>>>,
    ),
    tungstenite::Error,
> {
    let uri: Uri = web_url.parse().unwrap();
    let host = uri.authority().unwrap().to_string();
    let ws_url = format!("ws://{host}/ws");

    let mut builder = tungstenite::http::Request::builder()
        .uri(&ws_url)
        .header("Host", &host)
        .header("Origin", origin)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tungstenite::handshake::client::generate_key(),
        );

    if let Some(c) = cookie {
        builder = builder.header("Cookie", format!("herdr_session={c}"));
    }

    let request = builder.body(()).unwrap();
    tokio_tungstenite::connect_async(request).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn web_full_roundtrip() {
    let _lock = test_lock();
    let env = setup_web_env();

    let session = authenticate(&env.web_url, &env.token).await;
    let mut ws = connect_ws(&env.web_url, &session, 80, 24).await;

    assert!(
        wait_for_any_binary(&mut ws, Duration::from_secs(5)).await,
        "should receive initial render"
    );

    let input = b"echo hello-web-roundtrip\n";
    let (mut write, read) = ws.split();
    write
        .send(tungstenite::Message::Binary(input.to_vec().into()))
        .await
        .unwrap();
    let mut ws = read.reunite(write).unwrap();

    assert!(
        wait_for_output(&mut ws, "hello-web-roundtrip", Duration::from_secs(5)).await,
        "should receive echo output"
    );

    cleanup_spawned_herdr(env.spawned, env.base);
}

#[tokio::test]
async fn web_rejects_ws_without_cookie() {
    let _lock = test_lock();
    let env = setup_web_env();

    let uri: Uri = env.web_url.parse().unwrap();
    let host = uri.authority().unwrap().to_string();
    let ws_url = format!("ws://{host}/ws");

    let request = tungstenite::http::Request::builder()
        .uri(&ws_url)
        .header("Host", &host)
        .header("Origin", format!("http://{host}"))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tungstenite::handshake::client::generate_key(),
        )
        .body(())
        .unwrap();

    let result = tokio_tungstenite::connect_async(request).await;
    assert!(
        result.is_err(),
        "WebSocket connect without cookie should fail"
    );

    cleanup_spawned_herdr(env.spawned, env.base);
}

#[tokio::test]
async fn web_rejects_ws_with_wrong_origin() {
    let _lock = test_lock();
    let env = setup_web_env();

    let session = authenticate(&env.web_url, &env.token).await;

    let uri: Uri = env.web_url.parse().unwrap();
    let host = uri.authority().unwrap().to_string();
    let ws_url = format!("ws://{host}/ws");

    let request = tungstenite::http::Request::builder()
        .uri(&ws_url)
        .header("Host", &host)
        .header("Origin", "http://evil.com:1234")
        .header("Cookie", format!("herdr_session={session}"))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tungstenite::handshake::client::generate_key(),
        )
        .body(())
        .unwrap();

    let result = tokio_tungstenite::connect_async(request).await;
    assert!(
        result.is_err(),
        "WebSocket connect with wrong origin should fail"
    );

    cleanup_spawned_herdr(env.spawned, env.base);
}

#[tokio::test]
async fn web_resize_updates_terminal() {
    let _lock = test_lock();
    let env = setup_web_env();

    let session = authenticate(&env.web_url, &env.token).await;
    let mut ws = connect_ws(&env.web_url, &session, 80, 24).await;

    assert!(wait_for_any_binary(&mut ws, Duration::from_secs(5)).await);

    // The shell's column count reflects the pane width herdr derives from the
    // foreground client size (minus any chrome), so assert it grows after the
    // resize rather than hard-coding an exact value. A unique marker brackets
    // the number so it survives ANSI stripping.
    let cols_before = read_reported_cols(&mut ws, "BEFORE").await;

    let resize_msg = r#"{"type":"resize","cols":160,"rows":50}"#;
    ws.send(tungstenite::Message::Text(resize_msg.into()))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;

    let cols_after = read_reported_cols(&mut ws, "AFTER").await;

    assert!(
        cols_after > cols_before,
        "pane width should grow after resize: before={cols_before}, after={cols_after}"
    );

    cleanup_spawned_herdr(env.spawned, env.base);
}

/// Run `tput cols` behind a unique marker and return the reported column count.
///
/// The marker halves are concatenated by the shell at runtime (`'A''B'`), so the
/// assembled token `AB...` appears only in the command output — never in the
/// echoed command line that the terminal also renders.
async fn read_reported_cols(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    marker: &str,
) -> u32 {
    let (left, right) = marker.split_at(marker.len() / 2);
    let token = format!("{left}{right}");
    let cmd = format!("printf '{left}''{right}=%s=\\n' \"$(tput cols)\"\n");
    ws.send(tungstenite::Message::Binary(cmd.into_bytes().into()))
        .await
        .unwrap();

    let prefix = format!("{token}=");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let text = collect_visible_text(ws, Duration::from_millis(400)).await;
        if let Some(start) = text.find(&prefix) {
            let rest = &text[start + prefix.len()..];
            if let Some(end) = rest.find('=') {
                if let Ok(cols) = rest[..end].trim().parse::<u32>() {
                    return cols;
                }
            }
        }
    }
    panic!("did not observe reported cols for marker {token}");
}

#[tokio::test]
async fn web_multi_client_both_receive_frames() {
    let _lock = test_lock();
    let env = setup_web_env();

    let session = authenticate(&env.web_url, &env.token).await;

    let mut ws_a = connect_ws(&env.web_url, &session, 80, 24).await;
    assert!(wait_for_any_binary(&mut ws_a, Duration::from_secs(5)).await);

    let mut ws_b = connect_ws(&env.web_url, &session, 80, 24).await;
    assert!(wait_for_any_binary(&mut ws_b, Duration::from_secs(5)).await);

    let (mut write_b, read_b) = ws_b.split();
    write_b
        .send(tungstenite::Message::Binary(
            b"echo multi-client-marker\n".to_vec().into(),
        ))
        .await
        .unwrap();
    let mut ws_b = read_b.reunite(write_b).unwrap();

    let found_b = wait_for_output(&mut ws_b, "multi-client-marker", Duration::from_secs(5)).await;
    let found_a = wait_for_output(&mut ws_a, "multi-client-marker", Duration::from_secs(5)).await;

    assert!(found_b, "client B should receive output");
    assert!(found_a, "client A should receive output");

    cleanup_spawned_herdr(env.spawned, env.base);
}

#[tokio::test]
async fn web_reconnect_triggers_full_redraw() {
    let _lock = test_lock();
    let env = setup_web_env();

    let session = authenticate(&env.web_url, &env.token).await;

    let mut ws = connect_ws(&env.web_url, &session, 80, 24).await;
    assert!(wait_for_any_binary(&mut ws, Duration::from_secs(5)).await);

    let (mut write, read) = ws.split();
    write
        .send(tungstenite::Message::Binary(
            b"echo reconnect-marker\n".to_vec().into(),
        ))
        .await
        .unwrap();
    let mut ws = read.reunite(write).unwrap();
    assert!(wait_for_output(&mut ws, "reconnect-marker", Duration::from_secs(5)).await);

    let (mut write, _read) = ws.split();
    let _ = write.close().await;
    drop(write);

    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut ws2 = connect_ws(&env.web_url, &session, 80, 24).await;

    assert!(
        wait_for_any_binary(&mut ws2, Duration::from_secs(5)).await,
        "reconnected client should receive full redraw"
    );

    cleanup_spawned_herdr(env.spawned, env.base);
}

// ---------------------------------------------------------------------------
// Trust-proxy mode tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn trust_proxy_config_json_returns_mode() {
    let _lock = test_lock();
    let uri_str = "https://test-origin.example.com";
    let env = setup_trust_proxy_env(&[uri_str]);

    let response = raw_http_get(&env.web_url, "/config.json").await;
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "config.json should return 200: {response}"
    );
    assert!(
        response.contains("no-store"),
        "should have Cache-Control: no-store"
    );

    let body_start = response.find("\r\n\r\n").unwrap() + 4;
    let body = &response[body_start..];
    let parsed: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
    assert_eq!(parsed, serde_json::json!({"mode": "trust-proxy"}));

    cleanup_spawned_herdr(env.spawned, env.base);
}

#[tokio::test]
async fn trust_proxy_post_auth_returns_404() {
    let _lock = test_lock();
    let env = setup_trust_proxy_env(&["https://test-origin.example.com"]);

    let response = raw_http_post(&env.web_url, "/auth", r#"{"token":"anything"}"#).await;
    assert!(
        response.starts_with("HTTP/1.1 404"),
        "POST /auth in trust-proxy should return 404: {response}"
    );

    cleanup_spawned_herdr(env.spawned, env.base);
}

#[tokio::test]
async fn trust_proxy_ws_allowlisted_origin_succeeds() {
    let _lock = test_lock();
    let origin = "https://test-origin.example.com";
    let env = setup_trust_proxy_env(&[origin]);

    let result = try_ws_upgrade(&env.web_url, origin, None).await;
    assert!(result.is_ok(), "allowlisted origin should upgrade to 101");

    let (_ws, response) = result.unwrap();
    let has_set_cookie = response
        .headers()
        .get("set-cookie")
        .is_some();
    assert!(
        !has_set_cookie,
        "trust-proxy upgrade response must not have Set-Cookie"
    );

    cleanup_spawned_herdr(env.spawned, env.base);
}

#[tokio::test]
async fn trust_proxy_ws_non_allowlisted_origin_rejected() {
    let _lock = test_lock();
    let env = setup_trust_proxy_env(&["https://test-origin.example.com"]);

    let result = try_ws_upgrade(&env.web_url, "https://evil.example.com", None).await;
    assert!(
        result.is_err(),
        "non-allowlisted origin should be rejected (403)"
    );

    cleanup_spawned_herdr(env.spawned, env.base);
}

#[tokio::test]
async fn trust_proxy_ws_same_origin_loopback_not_allowlisted_rejected() {
    let _lock = test_lock();
    let env = setup_trust_proxy_env(&["https://test-origin.example.com"]);

    let uri: Uri = env.web_url.parse().unwrap();
    let host = uri.authority().unwrap().to_string();
    let same_origin = format!("http://{host}");

    let result = try_ws_upgrade(&env.web_url, &same_origin, None).await;
    assert!(
        result.is_err(),
        "same-origin loopback NOT allowlisted should be rejected in trust-proxy (keystone test)"
    );

    cleanup_spawned_herdr(env.spawned, env.base);
}

#[tokio::test]
async fn trust_proxy_requires_public_origin() {
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("herdr.sock");

    let spawned = spawn_server(&config_home, &runtime_dir, &api_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));

    let created = send_request(
        &api_socket,
        &format!(
            r#"{{"id":"ws_create","method":"workspace.create","params":{{"cwd":"{}","focus":true}}}}"#,
            base.display()
        ),
    );
    assert_eq!(created["result"]["type"], "workspace_created");

    let web_start = send_request(
        &api_socket,
        r#"{"id":"web_start","method":"web.start","params":{"bind_addr":"127.0.0.1:0","trust_proxy":true,"public_origins":[]}}"#,
    );
    let error_code = web_start["error"]["code"].as_str().unwrap_or("");
    assert_eq!(
        error_code, "trust_proxy_requires_public_origin",
        "trust-proxy without origins should fail: {web_start}"
    );

    cleanup_spawned_herdr(spawned, base);
}

#[tokio::test]
async fn standalone_config_json_returns_standalone() {
    let _lock = test_lock();
    let env = setup_web_env();

    let response = raw_http_get(&env.web_url, "/config.json").await;
    assert!(response.starts_with("HTTP/1.1 200"));

    let body_start = response.find("\r\n\r\n").unwrap() + 4;
    let body = &response[body_start..];
    let parsed: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
    assert_eq!(parsed, serde_json::json!({"mode": "standalone"}));

    cleanup_spawned_herdr(env.spawned, env.base);
}

#[tokio::test]
async fn standalone_with_public_origin_allows_non_same_origin() {
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("herdr.sock");

    let spawned = spawn_server(&config_home, &runtime_dir, &api_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));

    let created = send_request(
        &api_socket,
        &format!(
            r#"{{"id":"ws_create","method":"workspace.create","params":{{"cwd":"{}","focus":true}}}}"#,
            base.display()
        ),
    );
    assert_eq!(created["result"]["type"], "workspace_created");

    let token = "test_token_standalone_origin";
    let web_start = send_request(
        &api_socket,
        &format!(
            r#"{{"id":"web_start","method":"web.start","params":{{"bind_addr":"127.0.0.1:0","token":"{token}","public_origins":["https://allowed.example.com"]}}}}"#,
        ),
    );
    let web_url = web_start["result"]["url"]
        .as_str()
        .unwrap_or_else(|| panic!("web.start failed: {web_start}"))
        .to_string();

    let session = authenticate(&web_url, token).await;

    let result =
        try_ws_upgrade(&web_url, "https://allowed.example.com", Some(&session)).await;
    assert!(
        result.is_ok(),
        "standalone with allowlisted origin + valid session should succeed"
    );

    cleanup_spawned_herdr(spawned, base);
}

#[tokio::test]
async fn default_port_normalization() {
    let _lock = test_lock();
    let env = setup_trust_proxy_env(&["https://host.example.com"]);

    let result = try_ws_upgrade(&env.web_url, "https://host.example.com:443", None).await;
    assert!(
        result.is_ok(),
        "https://host:443 should match https://host (default port normalization)"
    );

    cleanup_spawned_herdr(env.spawned, env.base);
}

#[tokio::test]
async fn mixed_scheme_public_origins_rejected() {
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("herdr.sock");

    let spawned = spawn_server(&config_home, &runtime_dir, &api_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));

    let created = send_request(
        &api_socket,
        &format!(
            r#"{{"id":"ws_create","method":"workspace.create","params":{{"cwd":"{}","focus":true}}}}"#,
            base.display()
        ),
    );
    assert_eq!(created["result"]["type"], "workspace_created");

    let web_start = send_request(
        &api_socket,
        r#"{"id":"web_start","method":"web.start","params":{"bind_addr":"127.0.0.1:0","public_origins":["http://a.com","https://b.com"]}}"#,
    );
    let error_code = web_start["error"]["code"].as_str().unwrap_or("");
    assert_eq!(
        error_code, "mixed_scheme_public_origins",
        "mixed scheme origins should fail: {web_start}"
    );

    cleanup_spawned_herdr(spawned, base);
}

#[tokio::test]
async fn connection_cap_rejects_excess() {
    let _lock = test_lock();
    let origin = "https://cap-test.example.com";
    let env = setup_trust_proxy_env(&[origin]);

    let mut connections = Vec::new();
    for _ in 0..16 {
        let result = try_ws_upgrade(&env.web_url, origin, None).await;
        match result {
            Ok((ws, _)) => connections.push(ws),
            Err(_) => break,
        }
    }

    let excess = try_ws_upgrade(&env.web_url, origin, None).await;
    assert!(
        excess.is_err(),
        "17th connection should be rejected (cap=16)"
    );

    drop(connections.pop());
    tokio::time::sleep(Duration::from_millis(100)).await;

    let after_close = try_ws_upgrade(&env.web_url, origin, None).await;
    assert!(
        after_close.is_ok(),
        "after a slot is freed, upgrade should succeed again"
    );

    cleanup_spawned_herdr(env.spawned, env.base);
}
