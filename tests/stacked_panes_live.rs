//! Live end-to-end verification for stacked panes through a running headless
//! server. Exercises the step-5 public API (`layout.apply` / `layout.export`)
//! over the real Unix socket — not the in-process unit-test path — so the
//! schema, server dispatch, tree construction, and re-export all run against a
//! spawned `herdr server`.

mod support;

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde_json::Value;
use support::{
    cleanup_test_base, register_runtime_dir, register_spawned_herdr_pid,
    unregister_spawned_herdr_pid, wait_for_socket,
};

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    PathBuf::from(format!(
        "/tmp/herdr-stacked-panes-live-{}-{nanos}",
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

fn spawn_server(config_home: &Path, runtime_dir: &Path, api_socket_path: &Path) -> SpawnedHerdr {
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
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();

    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_herdr"));
    cmd.arg("server");
    cmd.env("XDG_CONFIG_HOME", config_home);
    cmd.env("XDG_RUNTIME_DIR", runtime_dir);
    cmd.env("HERDR_SOCKET_PATH", api_socket_path);
    cmd.env_remove("HERDR_CLIENT_SOCKET_PATH");
    cmd.env("SHELL", "/bin/sh");
    cmd.env_remove("HERDR_ENV");

    let child = pair.slave.spawn_command(cmd).unwrap();
    register_spawned_herdr_pid(child.process_id());
    drop(pair.slave);

    SpawnedHerdr {
        _master: pair.master,
        child,
    }
}

fn send_json_request(socket_path: &Path, request: &str) -> Value {
    let mut stream = UnixStream::connect(socket_path).expect("should connect to API socket");
    writeln!(stream, "{request}").unwrap();
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response).unwrap();
    serde_json::from_str(&response).expect("response should be valid JSON")
}

#[test]
fn live_layout_apply_and_export_round_trips_a_stack() {
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("herdr.sock");

    let server = spawn_server(&config_home, &runtime_dir, &api_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));

    // Create a workspace to apply the stacked layout into.
    let ws = send_json_request(
        &api_socket,
        "{\"id\":\"ws\",\"method\":\"workspace.create\",\"params\":{\"label\":\"stack-live\"}}",
    );
    assert!(ws.get("error").is_none(), "workspace.create failed: {ws}");
    let workspace_id = ws
        .pointer("/result/workspace/workspace_id")
        .and_then(Value::as_str)
        .expect("workspace id")
        .to_string();

    // Apply a 3-member stack with the middle member expanded.
    let apply_req = format!(
        "{{\"id\":\"apply\",\"method\":\"layout.apply\",\"params\":{{\
           \"workspace_id\":\"{workspace_id}\",\"tab_label\":\"agents\",\"focus\":true,\
           \"root\":{{\"type\":\"stack\",\"expanded\":1,\"panes\":[\
             {{\"label\":\"agent-1\"}},{{\"label\":\"agent-2\"}},{{\"label\":\"agent-3\"}}\
           ]}}}}}}"
    );
    let apply = send_json_request(&api_socket, &apply_req);
    assert!(apply.get("error").is_none(), "layout.apply failed: {apply}");

    // The apply response carries the materialized layout: assert it came back
    // as a stack with the expanded index and member labels preserved.
    let root = apply
        .pointer("/result/layout/root")
        .expect("apply result should include layout root");
    assert_eq!(
        root.pointer("/type").and_then(Value::as_str),
        Some("stack"),
        "applied root should be a stack: {root}"
    );
    assert_eq!(
        root.pointer("/expanded").and_then(Value::as_u64),
        Some(1),
        "expanded index should round-trip"
    );
    let panes = root
        .pointer("/panes")
        .and_then(Value::as_array)
        .expect("stack should have panes array");
    assert_eq!(panes.len(), 3, "stack should have 3 members");
    let labels: Vec<&str> = panes
        .iter()
        .filter_map(|p| p.pointer("/label").and_then(Value::as_str))
        .collect();
    assert_eq!(labels, vec!["agent-1", "agent-2", "agent-3"]);

    // Independent export must report the same stack shape (export path, not the
    // apply response echo).
    let tab_id = apply
        .pointer("/result/layout/tab_id")
        .and_then(Value::as_str)
        .expect("apply should report tab id")
        .to_string();
    let export = send_json_request(
        &api_socket,
        &format!(
            "{{\"id\":\"export\",\"method\":\"layout.export\",\"params\":{{\"tab_id\":\"{tab_id}\"}}}}"
        ),
    );
    assert!(
        export.get("error").is_none(),
        "layout.export failed: {export}"
    );
    let export_root = export
        .pointer("/result/layout/root")
        .expect("export should include layout root");
    assert_eq!(
        export_root.pointer("/type").and_then(Value::as_str),
        Some("stack"),
        "exported root should be a stack: {export_root}"
    );
    assert_eq!(
        export_root.pointer("/expanded").and_then(Value::as_u64),
        Some(1)
    );

    // Every stack member must be a real pane (pane identity invariant).
    // `workspace.create` seeds an initial tab, and applying with `workspace_id`
    // adds a new tab, so the workspace has more than 3 panes overall — filter to
    // the stacked tab and assert exactly its 3 members are real, labeled panes.
    let panes_resp = send_json_request(
        &api_socket,
        &format!(
            "{{\"id\":\"plist\",\"method\":\"pane.list\",\"params\":{{\"workspace_id\":\"{workspace_id}\"}}}}"
        ),
    );
    let listed = panes_resp
        .pointer("/result/panes")
        .and_then(Value::as_array)
        .expect("pane.list should return panes");
    let stacked_tab_labels: Vec<&str> = listed
        .iter()
        .filter(|p| p.pointer("/tab_id").and_then(Value::as_str) == Some(tab_id.as_str()))
        .filter_map(|p| p.pointer("/label").and_then(Value::as_str))
        .collect();
    assert_eq!(
        stacked_tab_labels,
        vec!["agent-1", "agent-2", "agent-3"],
        "all 3 stack members should be real, labeled panes in the stacked tab: {panes_resp}"
    );

    drop(server);
    cleanup_test_base(&base);
}

#[test]
fn live_layout_apply_clamps_out_of_range_expanded() {
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("herdr.sock");

    let server = spawn_server(&config_home, &runtime_dir, &api_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));

    let ws = send_json_request(
        &api_socket,
        "{\"id\":\"ws\",\"method\":\"workspace.create\",\"params\":{\"label\":\"stack-clamp\"}}",
    );
    let workspace_id = ws
        .pointer("/result/workspace/workspace_id")
        .and_then(Value::as_str)
        .expect("workspace id")
        .to_string();

    // expanded:9 is out of range for a 2-member stack; the server should clamp
    // it to the last member rather than reject or panic.
    let apply_req = format!(
        "{{\"id\":\"apply\",\"method\":\"layout.apply\",\"params\":{{\
           \"workspace_id\":\"{workspace_id}\",\"tab_label\":\"clamp\",\"focus\":true,\
           \"root\":{{\"type\":\"stack\",\"expanded\":9,\"panes\":[\
             {{\"label\":\"a\"}},{{\"label\":\"b\"}}\
           ]}}}}}}"
    );
    let apply = send_json_request(&api_socket, &apply_req);
    assert!(
        apply.get("error").is_none(),
        "layout.apply should clamp, not fail: {apply}"
    );
    let root = apply.pointer("/result/layout/root").expect("layout root");
    assert_eq!(root.pointer("/type").and_then(Value::as_str), Some("stack"));
    assert_eq!(
        root.pointer("/expanded").and_then(Value::as_u64),
        Some(1),
        "out-of-range expanded should clamp to the last member"
    );

    drop(server);
    cleanup_test_base(&base);
}
