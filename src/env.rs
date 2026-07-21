use std::process::Command;

pub(crate) fn scrub_herdr_runtime_env(command: &mut Command) {
    tracing::debug!("scrubbing HERDR_* runtime env vars from subprocess");
    for key in [
        crate::api::SOCKET_PATH_ENV_VAR,
        crate::server::socket_paths::CLIENT_SOCKET_PATH_ENV_VAR,
        crate::session::SESSION_ENV_VAR,
        "HERDR_BIN_PATH",
        crate::HERDR_ENV_VAR,
        crate::integration::HERDR_WORKSPACE_ID_ENV_VAR,
        crate::integration::HERDR_TAB_ID_ENV_VAR,
        crate::integration::HERDR_PANE_ID_ENV_VAR,
    ] {
        command.env_remove(key);
    }
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("HERDR_PLUGIN_") {
            command.env_remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn scrub_removes_known_herdr_vars() {
        let mut cmd = Command::new("echo");
        cmd.env("HERDR_ENV", "1");
        cmd.env("HERDR_SESSION", "test");
        cmd.env("HERDR_SOCKET_PATH", "/tmp/api.sock");
        cmd.env("HERDR_CLIENT_SOCKET_PATH", "/tmp/client.sock");
        cmd.env("HERDR_BIN_PATH", "/usr/bin/herdr");
        cmd.env("HERDR_WORKSPACE_ID", "ws1");
        cmd.env("HERDR_TAB_ID", "t1");
        cmd.env("HERDR_PANE_ID", "p1");
        cmd.env("UNRELATED_VAR", "keep");

        scrub_herdr_runtime_env(&mut cmd);

        let envs: std::collections::HashMap<_, _> = cmd
            .get_envs()
            .map(|(k, v)| (k.to_string_lossy().to_string(), v.map(|v| v.to_owned())))
            .collect();

        assert_eq!(envs.get("HERDR_ENV"), Some(&None));
        assert_eq!(envs.get("HERDR_SESSION"), Some(&None));
        assert_eq!(envs.get("HERDR_SOCKET_PATH"), Some(&None));
        assert_eq!(envs.get("HERDR_CLIENT_SOCKET_PATH"), Some(&None));
        assert_eq!(envs.get("HERDR_BIN_PATH"), Some(&None));
        assert_eq!(envs.get("HERDR_WORKSPACE_ID"), Some(&None));
        assert_eq!(envs.get("HERDR_TAB_ID"), Some(&None));
        assert_eq!(envs.get("HERDR_PANE_ID"), Some(&None));
        // HERDR_PLUGIN_* removal scans the process environment at call time,
        // not the Command's env overrides, so it can't be tested in isolation.
        assert!(envs
            .get("UNRELATED_VAR")
            .and_then(|v| v.as_ref())
            .is_some());
    }

    #[test]
    fn scrub_herdr_vars_drift_test() {
        let scrub_list: &[&str] = &[
            crate::api::SOCKET_PATH_ENV_VAR,
            crate::server::socket_paths::CLIENT_SOCKET_PATH_ENV_VAR,
            crate::session::SESSION_ENV_VAR,
            "HERDR_BIN_PATH",
            crate::HERDR_ENV_VAR,
            crate::integration::HERDR_WORKSPACE_ID_ENV_VAR,
            crate::integration::HERDR_TAB_ID_ENV_VAR,
            crate::integration::HERDR_PANE_ID_ENV_VAR,
        ];

        // Vars that are intentionally NOT scrubbed because they are:
        // - part of the remote client protocol (RENDER_ENCODING, REATTACH, KEYBINDINGS)
        // - set on utility subprocesses unrelated to herdr server identity (CHANNEL, NOTIFY_ARGS)
        // - statusLine wrapper display/config consumed by the wrapper's own
        //   chained subprocess, not herdr server identity (STATUSLINE_CHAIN, STATUSLINE_DEBUG)
        let exemptions: &[&str] = &[
            "HERDR_RENDER_ENCODING",
            "HERDR_REATTACH_COMMAND",
            "HERDR_REMOTE_KEYBINDINGS",
            "HERDR_CHANNEL",
            "HERDR_NOTIFY_ARGS",
            "HERDR_STATUSLINE_CHAIN",
            "HERDR_STATUSLINE_DEBUG",
        ];

        let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let output = std::process::Command::new("grep")
            .args(["-rn", "--include=*.rs", r#".env("HERDR_"#, src_dir.to_str().unwrap()])
            .output()
            .expect("grep failed");
        let stdout = String::from_utf8_lossy(&output.stdout);

        let mut missing = Vec::new();
        for line in stdout.lines() {
            if line.contains("/env.rs:") {
                continue;
            }
            // Extract the var name from .env("HERDR_...",
            let Some(var_start) = line.find(".env(\"HERDR_") else {
                continue;
            };
            // Skip if the .env( call is inside a comment
            let code_part = line.split("//").next().unwrap_or(line);
            if !code_part.contains(".env(\"HERDR_") {
                continue;
            }
            let after_quote = &line[var_start + 6..]; // skip `.env("`
            let Some(end) = after_quote.find('"') else {
                continue;
            };
            let var_name = &after_quote[..end];
            if !scrub_list.contains(&var_name)
                && !exemptions.contains(&var_name)
                && !var_name.starts_with("HERDR_PLUGIN_")
            {
                missing.push(format!("{var_name} in {line}"));
            }
        }
        assert!(
            missing.is_empty(),
            "HERDR_* vars set on subprocesses but not in scrub list or exemptions:\n{}",
            missing.join("\n")
        );
    }
}
