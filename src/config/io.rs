use std::path::{Path, PathBuf};

use tracing::warn;

use super::{model::LoadedConfig, Config, CONFIG_PATH_ENV_VAR};

const KNOWN_TOP_LEVEL_CONFIG_KEYS: &[&str] = &[
    "advanced",
    "experimental",
    "keys",
    "onboarding",
    "remote",
    "session",
    "terminal",
    "theme",
    "ui",
    "update",
    "worktrees",
];

pub fn app_dir_name() -> &'static str {
    if cfg!(debug_assertions) {
        "herdr-dev"
    } else {
        "herdr"
    }
}

pub fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(dir).join(app_dir_name());
    }
    platform_config_dir()
}

pub fn state_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_STATE_HOME") {
        return PathBuf::from(dir).join(app_dir_name());
    }
    platform_state_dir()
}

#[cfg(windows)]
fn platform_config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("APPDATA") {
        return PathBuf::from(dir).join(app_dir_name());
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        return PathBuf::from(profile)
            .join("AppData")
            .join("Roaming")
            .join(app_dir_name());
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(format!(".config/{}", app_dir_name()));
    }
    std::env::temp_dir().join(app_dir_name())
}

#[cfg(not(windows))]
fn platform_config_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(format!(".config/{}", app_dir_name()))
    } else {
        std::env::temp_dir().join(app_dir_name())
    }
}

#[cfg(windows)]
fn platform_state_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(dir).join(app_dir_name());
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        return PathBuf::from(profile)
            .join("AppData")
            .join("Local")
            .join(app_dir_name());
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(format!(".local/state/{}", app_dir_name()));
    }
    std::env::temp_dir().join(format!("{}-state", app_dir_name()))
}

#[cfg(not(windows))]
fn platform_state_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(format!(".local/state/{}", app_dir_name()))
    } else {
        std::env::temp_dir().join(format!("{}-state", app_dir_name()))
    }
}

impl Config {
    pub fn load() -> LoadedConfig {
        let path = config_path();
        if !path.exists() {
            return LoadedConfig {
                config: Self::default(),
                diagnostics: Vec::new(),
                invalid_sections: Vec::new(),
            };
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                warn!(err = %err, "config read error, using defaults");
                return LoadedConfig {
                    config: Self::default(),
                    diagnostics: vec![format!("config read error: {err}; using defaults")],
                    invalid_sections: Vec::new(),
                };
            }
        };

        match toml::from_str::<Config>(&content) {
            Ok(config) => {
                let mut diagnostics = unknown_top_level_section_diagnostics_from_str(&content);
                // Flag legacy flat `[keys]` fields so the maintainer is told
                // their old keymap no longer applies instead of silently losing
                // it. The salvage path below routes through
                // `load_live_config_from_str`, which emits these itself.
                diagnostics.extend(legacy_keys_diagnostics_from_str(&content));
                diagnostics.extend(config.collect_diagnostics());
                LoadedConfig {
                    config,
                    diagnostics,
                    invalid_sections: Vec::new(),
                }
            }
            // A full-config parse failure (commonly a malformed `[keys]` under
            // the new schema) must not wipe theme/session/agent/update/worktree
            // settings. Fall back to section-isolated parsing so only the bad
            // section degrades to defaults.
            Err(err) => {
                warn!(err = %err, "config parse error, salvaging valid sections");
                match load_live_config_from_str(&content) {
                    Ok(mut loaded) => {
                        loaded
                            .diagnostics
                            .extend(loaded.config.collect_diagnostics());
                        loaded
                    }
                    Err(top_level_errors) => LoadedConfig {
                        config: Self::default(),
                        diagnostics: top_level_errors,
                        invalid_sections: Vec::new(),
                    },
                }
            }
        }
    }
}

/// Legacy `[keys]` field names from the pre-modal flat schema. Their presence
/// means the maintainer's old keymap no longer applies under the modal schema.
const LEGACY_KEYS_FIELDS: &[&str] = &[
    "prefix",
    "help",
    "settings",
    "new_workspace",
    "new_worktree",
    "open_worktree",
    "remove_worktree",
    "rename_workspace",
    "close_workspace",
    "workspace_picker",
    "goto",
    "navigate_workspace_up",
    "navigate_workspace_down",
    "navigate_pane_left",
    "navigate_pane_down",
    "navigate_pane_up",
    "navigate_pane_right",
    "detach",
    "reload_config",
    "open_notification_target",
    "previous_workspace",
    "next_workspace",
    "previous_agent",
    "next_agent",
    "focus_agent",
    "new_tab",
    "rename_tab",
    "previous_tab",
    "next_tab",
    "switch_tab",
    "switch_workspace",
    "close_tab",
    "rename_pane",
    "edit_scrollback",
    "copy_mode",
    "focus_pane_left",
    "focus_pane_down",
    "focus_pane_up",
    "focus_pane_right",
    "swap_pane_left",
    "swap_pane_down",
    "swap_pane_up",
    "swap_pane_right",
    "cycle_pane_next",
    "cycle_pane_previous",
    "last_pane",
    "split_vertical",
    "split_horizontal",
    "stack_pane",
    "unstack_pane",
    "close_pane",
    "break_pane_to_tab",
    "zoom",
    "fullscreen",
    "split_auto",
    "move_tab_left",
    "move_tab_right",
    "resize_grow",
    "resize_shrink",
    "resize_mode",
    "toggle_sidebar",
];

fn legacy_keys_diagnostics_from_str(content: &str) -> Vec<String> {
    let Ok(value) = content.parse::<toml::Value>() else {
        return Vec::new();
    };
    let Some(keys) = value.get("keys").and_then(toml::Value::as_table) else {
        return Vec::new();
    };

    // Custom commands ([[keys.command]]) and [keys.indexed] remain supported,
    // so they are intentionally absent from LEGACY_KEYS_FIELDS.
    let present: Vec<String> = LEGACY_KEYS_FIELDS
        .iter()
        .filter(|field| keys.contains_key(**field))
        .map(|field| format!("keys.{field}"))
        .collect();
    if present.is_empty() {
        return Vec::new();
    }

    // One aggregated message naming every dropped field, so the startup
    // notification (which truncates to a few lines) does not imply only the
    // first field is affected.
    let diagnostic = format!(
        "{} no longer recognized; herdr now uses the mode-structured [keys] schema (default_mode, mode_* entry keys, and [keys.shared]/[keys.pane]/[keys.tab]/[keys.resize]/[keys.move]/[keys.session]/[keys.tmux] tables); prefix mode is now keys.mode_tmux",
        present.join(", ")
    );
    warn!(message = %diagnostic, "config diagnostic");
    vec![diagnostic]
}

pub(super) fn resolve_config_relative_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }

    config_path()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(path)
}

pub fn config_path() -> PathBuf {
    if let Ok(path) = std::env::var(CONFIG_PATH_ENV_VAR) {
        return PathBuf::from(path);
    }
    config_dir().join("config.toml")
}

pub fn config_diagnostic_summary(diagnostics: &[String]) -> Option<String> {
    const MAX_VISIBLE_DIAGNOSTICS: usize = 4;

    if diagnostics.is_empty() {
        return None;
    }

    let mut lines: Vec<String> = diagnostics
        .iter()
        .take(MAX_VISIBLE_DIAGNOSTICS)
        .map(|diagnostic| diagnostic.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    let hidden = diagnostics.len().saturating_sub(MAX_VISIBLE_DIAGNOSTICS);
    if hidden > 0 {
        lines.push(format!("and {hidden} more config warnings"));
    }
    Some(lines.join("\n"))
}

pub fn load_live_config() -> Result<LoadedConfig, Vec<String>> {
    let path = config_path();
    if !path.exists() {
        return Ok(LoadedConfig {
            config: Config::default(),
            diagnostics: Vec::new(),
            invalid_sections: Vec::new(),
        });
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|err| vec![format!("config read error: {err}; keeping current config")])?;
    load_live_config_from_str(&content)
}

fn load_live_config_from_str(content: &str) -> Result<LoadedConfig, Vec<String>> {
    let value = content
        .parse::<toml::Value>()
        .map_err(|err| vec![format!("config parse error: {err}; keeping current config")])?;
    let table = value.as_table().ok_or_else(|| {
        vec![
            "config parse error: top-level config must be a table; keeping current config"
                .to_string(),
        ]
    })?;

    let mut config = Config::default();
    let mut diagnostics = unknown_top_level_section_diagnostics(table);
    // Flag legacy flat [keys] fields on reload too, so an in-session reload
    // surfaces a dropped keymap instead of silently ignoring it.
    diagnostics.extend(legacy_keys_diagnostics_from_str(content));
    let mut invalid_sections = Vec::new();

    if let Some(value) = table.get("onboarding") {
        match value.clone().try_into::<Option<bool>>() {
            Ok(onboarding) => config.onboarding = onboarding,
            Err(err) => diagnostics.push(format!(
                "invalid onboarding setting: {err}; keeping current onboarding state"
            )),
        }
    }

    load_live_section(
        table,
        "theme",
        "theme config",
        &mut diagnostics,
        &mut invalid_sections,
        |section| config.theme = section,
    );
    load_live_section(
        table,
        "keys",
        "keybinding config",
        &mut diagnostics,
        &mut invalid_sections,
        |section| config.keys = section,
    );
    load_live_section(
        table,
        "terminal",
        "terminal config",
        &mut diagnostics,
        &mut invalid_sections,
        |section| config.terminal = section,
    );
    load_live_section(
        table,
        "session",
        "session config",
        &mut diagnostics,
        &mut invalid_sections,
        |section| config.session = section,
    );
    load_live_section(
        table,
        "update",
        "update config",
        &mut diagnostics,
        &mut invalid_sections,
        |section| config.update = section,
    );
    load_live_section(
        table,
        "ui",
        "ui config",
        &mut diagnostics,
        &mut invalid_sections,
        |section| config.ui = section,
    );
    load_live_section(
        table,
        "advanced",
        "advanced config",
        &mut diagnostics,
        &mut invalid_sections,
        |section| config.advanced = section,
    );
    load_live_section(
        table,
        "worktrees",
        "worktree config",
        &mut diagnostics,
        &mut invalid_sections,
        |section| config.worktrees = section,
    );
    load_live_section(
        table,
        "experimental",
        "experimental config",
        &mut diagnostics,
        &mut invalid_sections,
        |section| config.experimental = section,
    );
    load_live_section(
        table,
        "remote",
        "remote config",
        &mut diagnostics,
        &mut invalid_sections,
        |section| config.remote = section,
    );

    Ok(LoadedConfig {
        config,
        diagnostics,
        invalid_sections,
    })
}

fn unknown_top_level_section_diagnostics_from_str(content: &str) -> Vec<String> {
    content
        .parse::<toml::Value>()
        .ok()
        .and_then(|value| value.as_table().map(unknown_top_level_section_diagnostics))
        .unwrap_or_default()
}

fn unknown_top_level_section_diagnostics(
    table: &toml::map::Map<String, toml::Value>,
) -> Vec<String> {
    table
        .iter()
        .filter_map(|(key, value)| unknown_top_level_section_diagnostic(key, value))
        .collect()
}

fn unknown_top_level_section_diagnostic(key: &str, value: &toml::Value) -> Option<String> {
    if KNOWN_TOP_LEVEL_CONFIG_KEYS.contains(&key) {
        return None;
    }

    let header = if value.is_table() {
        format!("[{key}]")
    } else if value
        .as_array()
        .is_some_and(|items| !items.is_empty() && items.iter().all(toml::Value::is_table))
    {
        format!("[[{key}]]")
    } else {
        return None;
    };

    if key == "toast" {
        Some(format!(
            "unknown config section {header}; did you mean [ui.toast]? ignoring section"
        ))
    } else {
        Some(format!("unknown config section {header}; ignoring section"))
    }
}

fn load_live_section<T>(
    table: &toml::map::Map<String, toml::Value>,
    section: &'static str,
    label: &str,
    diagnostics: &mut Vec<String>,
    invalid_sections: &mut Vec<String>,
    apply: impl FnOnce(T),
) where
    T: serde::de::DeserializeOwned,
{
    let Some(value) = table.get(section) else {
        return;
    };

    match value.clone().try_into::<T>() {
        Ok(section_config) => apply(section_config),
        Err(err) => {
            diagnostics.push(format!(
                "invalid {label}: {err}; keeping current {section} settings"
            ));
            invalid_sections.push(section.to_string());
        }
    }
}

pub(crate) fn upsert_top_level_bool(content: &str, key: &str, value: bool) -> String {
    let replacement = format!("{key} = {value}");
    let mut lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();
    let mut in_section = false;

    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_section = true;
            continue;
        }
        if in_section {
            continue;
        }
        if trimmed.starts_with(&format!("{key} ")) || trimmed.starts_with(&format!("{key}=")) {
            *line = replacement.clone();
            return lines.join("\n") + "\n";
        }
    }

    if lines.is_empty() {
        format!("{replacement}\n")
    } else {
        format!("{replacement}\n{}\n", lines.join("\n").trim_end())
    }
}

/// Write a key = value pair in a TOML section (creates section if missing).
pub fn upsert_section_value(content: &str, section: &str, key: &str, value: &str) -> String {
    upsert_section_raw(content, section, key, value)
}

pub fn upsert_section_bool(content: &str, section: &str, key: &str, value: bool) -> String {
    upsert_section_raw(content, section, key, &value.to_string())
}

pub fn remove_section_key(content: &str, section: &str, key: &str) -> String {
    let header = format!("[{section}]");
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::new();
    let mut i = 0;
    let mut in_section = false;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_section = trimmed == header;
            result.push(line.to_string());
            i += 1;
            continue;
        }

        if in_section
            && (trimmed.starts_with(&format!("{key} ")) || trimmed.starts_with(&format!("{key}=")))
        {
            i += 1;
            continue;
        }

        result.push(line.to_string());
        i += 1;
    }

    result.join("\n") + "\n"
}

pub fn remove_keybinding_config_sections(content: &str) -> (String, bool) {
    let mut result = Vec::new();
    let mut removed = false;
    let mut skipping_key_section = false;
    let mut in_table = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if let Some(table_name) = toml_table_header_name(trimmed) {
            in_table = true;
            skipping_key_section = is_keys_table_name(table_name);
            if skipping_key_section {
                removed = true;
                continue;
            }
        } else if skipping_key_section || (!in_table && is_top_level_keys_assignment(trimmed)) {
            removed = true;
            continue;
        }

        result.push(line.to_string());
    }

    let mut updated = result.join("\n");
    if content.ends_with('\n') || !updated.is_empty() {
        updated.push('\n');
    }
    (updated, removed)
}

fn toml_table_header_name(trimmed: &str) -> Option<&str> {
    if let Some(name) = trimmed
        .strip_prefix("[[")
        .and_then(|value| value.strip_suffix("]]"))
    {
        return Some(name.trim());
    }
    trimmed
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .map(str::trim)
}

fn is_keys_table_name(name: &str) -> bool {
    name == "keys" || name.starts_with("keys.")
}

fn is_top_level_keys_assignment(trimmed: &str) -> bool {
    trimmed.starts_with("keys ") || trimmed.starts_with("keys=") || trimmed.starts_with("keys.")
}

fn upsert_section_raw(content: &str, section: &str, key: &str, value: &str) -> String {
    let header = format!("[{section}]");
    let assignment = format!("{key} = {value}");
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::new();
    let mut i = 0;
    let mut found_section = false;
    let mut inserted = false;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed == header {
            found_section = true;
            result.push(line.to_string());
            i += 1;

            while i < lines.len() {
                let current = lines[i];
                let current_trimmed = current.trim();
                if current_trimmed.starts_with('[') && current_trimmed.ends_with(']') {
                    if !inserted {
                        result.push(assignment.clone());
                        inserted = true;
                    }
                    break;
                }

                if current_trimmed.starts_with(&format!("{key} "))
                    || current_trimmed.starts_with(&format!("{key}="))
                {
                    result.push(assignment.clone());
                    inserted = true;
                } else {
                    result.push(current.to_string());
                }
                i += 1;
            }

            continue;
        }

        result.push(line.to_string());
        i += 1;
    }

    if !found_section {
        if !result.is_empty() && !result.last().is_some_and(|line| line.trim().is_empty()) {
            result.push(String::new());
        }
        result.push(header);
        result.push(assignment);
    } else if !inserted {
        result.push(assignment);
    }

    result.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_top_level_bool_replaces_existing_value() {
        let content = "onboarding = true\n[keys]\nprefix = \"ctrl+b\"\n";
        let updated = upsert_top_level_bool(content, "onboarding", false);
        assert!(updated.contains("onboarding = false"));
        assert!(!updated.contains("onboarding = true"));
    }

    #[test]
    fn upsert_section_bool_adds_missing_section() {
        let updated = upsert_section_bool("", "ui.toast", "enabled", true);
        assert!(updated.contains("[ui.toast]"));
        assert!(updated.contains("enabled = true"));
    }

    #[test]
    fn remove_section_key_removes_matching_key_from_section() {
        let content =
            "[ui.toast]\nenabled = true\ndelivery = \"herdr\"\n[ui.sound]\nenabled = true\n";
        let updated = remove_section_key(content, "ui.toast", "enabled");
        assert!(!updated.contains("[ui.toast]\nenabled = true"));
        assert!(updated.contains("delivery = \"herdr\""));
        assert!(updated.contains("[ui.sound]\nenabled = true"));
    }

    #[test]
    fn config_diagnostic_summary_keeps_multiple_warnings_visible() {
        let diagnostics = vec![
            "one".to_string(),
            "two".to_string(),
            "three".to_string(),
            "four".to_string(),
            "five".to_string(),
        ];

        assert_eq!(
            config_diagnostic_summary(&diagnostics).as_deref(),
            Some("one\ntwo\nthree\nfour\nand 1 more config warnings")
        );
    }

    #[test]
    fn load_live_config_parses_session_section() {
        let loaded = load_live_config_from_str(
            r#"
[session]
resume_agents_on_restore = true
"#,
        )
        .unwrap();

        assert!(loaded.config.session.resume_agents_on_restore);
        assert!(loaded.diagnostics.is_empty());
        assert!(loaded.invalid_sections.is_empty());
    }

    #[test]
    fn load_live_config_warns_about_unknown_top_level_sections() {
        let loaded = load_live_config_from_str(
            r#"
[toast]
delivery = "system"

[ui.toast]
delivery = "herdr"
"#,
        )
        .unwrap();

        assert_eq!(
            loaded.diagnostics,
            vec!["unknown config section [toast]; did you mean [ui.toast]? ignoring section"]
        );
        assert!(loaded.invalid_sections.is_empty());
        assert_eq!(
            loaded.config.ui.toast.delivery,
            super::super::ToastDelivery::Herdr
        );
    }

    #[test]
    fn load_live_config_does_not_warn_about_unknown_top_level_scalar_values() {
        let loaded = load_live_config_from_str(
            r#"
plugin = []

[ui.toast]
delivery = "herdr"
"#,
        )
        .unwrap();

        assert!(loaded.diagnostics.is_empty());
        assert_eq!(
            loaded.config.ui.toast.delivery,
            super::super::ToastDelivery::Herdr
        );
    }

    #[test]
    fn startup_config_load_warns_about_unknown_top_level_sections() {
        let _guard = crate::config::test_config_env_lock().lock().unwrap();
        let path = std::env::temp_dir().join(format!(
            "herdr-config-unknown-section-{}.toml",
            std::process::id()
        ));
        std::fs::write(
            &path,
            r#"
[[plugin]]
id = "example"

[ui.toast]
delivery = "system"
"#,
        )
        .unwrap();
        std::env::set_var(CONFIG_PATH_ENV_VAR, &path);

        let loaded = Config::load();

        assert_eq!(
            loaded.diagnostics,
            vec!["unknown config section [[plugin]]; ignoring section"]
        );
        assert_eq!(
            loaded.config.ui.toast.delivery,
            super::super::ToastDelivery::System
        );

        std::env::remove_var(CONFIG_PATH_ENV_VAR);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn remove_keybinding_config_sections_removes_keys_tables_only() {
        let content = r#"onboarding = false

[theme]
name = "catppuccin"

[keys]
prefix = "ctrl+a"
new_tab = "c"

[[keys.command]]
key = "g"
command = "lazygit"

[keys.indexed]
tabs = "ctrl"

[ui]
mouse_capture = false
"#;

        let (updated, removed) = remove_keybinding_config_sections(content);

        assert!(removed);
        assert!(updated.contains("onboarding = false"));
        assert!(updated.contains("[theme]\nname = \"catppuccin\""));
        assert!(updated.contains("[ui]\nmouse_capture = false"));
        assert!(!updated.contains("[keys]"));
        assert!(!updated.contains("[[keys.command]]"));
        assert!(!updated.contains("[keys.indexed]"));
        assert!(toml::from_str::<toml::Value>(&updated).is_ok());
    }

    #[test]
    fn remove_keybinding_config_sections_reports_noop_without_keys() {
        let content = "[ui]\nmouse_capture = true\n";
        let (updated, removed) = remove_keybinding_config_sections(content);
        assert!(!removed);
        assert_eq!(updated, content);
    }

    #[test]
    fn malformed_keys_section_degrades_only_keybindings() {
        let _guard = crate::config::test_config_env_lock().lock().unwrap();
        let path = std::env::temp_dir().join(format!(
            "herdr-config-malformed-keys-{}.toml",
            std::process::id()
        ));
        // `mode_pane` must be a string; an integer makes the whole-config parse
        // fail. Other sections must still load via section-isolated salvage.
        std::fs::write(
            &path,
            r#"
[keys]
mode_pane = 42

[ui]
mouse_capture = false

[session]
resume_agents_on_restore = false
"#,
        )
        .unwrap();
        std::env::set_var(CONFIG_PATH_ENV_VAR, &path);

        let loaded = Config::load();

        // Keybindings fell back to defaults.
        assert_eq!(loaded.config.keys.mode_pane, "ctrl+p");
        // Other sections survived.
        assert!(!loaded.config.ui.mouse_capture);
        assert!(!loaded.config.session.resume_agents_on_restore);
        assert!(loaded
            .diagnostics
            .iter()
            .any(|d| d.contains("keybinding config")));

        std::env::remove_var(CONFIG_PATH_ENV_VAR);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn legacy_flat_keys_fields_produce_no_longer_recognized_diagnostics() {
        let _guard = crate::config::test_config_env_lock().lock().unwrap();
        let path = std::env::temp_dir().join(format!(
            "herdr-config-legacy-keys-{}.toml",
            std::process::id()
        ));
        std::fs::write(
            &path,
            r#"
[keys]
prefix = "ctrl+a"
focus_pane_left = "prefix+h"
new_tab = "prefix+t"
move_tab_left = "alt+i"
"#,
        )
        .unwrap();
        std::env::set_var(CONFIG_PATH_ENV_VAR, &path);

        let loaded = Config::load();

        // One aggregated diagnostic naming every dropped field, so a truncated
        // startup notification cannot imply only the first field is affected.
        let legacy: Vec<&String> = loaded
            .diagnostics
            .iter()
            .filter(|d| d.contains("no longer recognized"))
            .collect();
        assert_eq!(
            legacy.len(),
            1,
            "expected one aggregated legacy diagnostic: {legacy:?}"
        );
        for field in ["prefix", "focus_pane_left", "new_tab", "move_tab_left"] {
            assert!(
                legacy[0].contains(&format!("keys.{field}")),
                "aggregated legacy diagnostic missing {field}: {}",
                legacy[0]
            );
        }
        // The diagnostics feed the user-visible startup notification summary.
        let summary = config_diagnostic_summary(&loaded.diagnostics);
        assert!(summary.is_some_and(|s| s.contains("no longer recognized")));

        std::env::remove_var(CONFIG_PATH_ENV_VAR);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn legacy_keys_diagnostics_emitted_on_live_reload() {
        // An in-session reload must also flag a dropped legacy keymap, not only
        // the cold-start load.
        let loaded =
            load_live_config_from_str("[keys]\nprefix = \"ctrl+a\"\nnew_tab = \"prefix+t\"\n")
                .expect("valid top-level toml");
        let legacy: Vec<&String> = loaded
            .diagnostics
            .iter()
            .filter(|d| d.contains("no longer recognized"))
            .collect();
        assert_eq!(
            legacy.len(),
            1,
            "expected one legacy diagnostic on reload: {legacy:?}"
        );
        assert!(legacy[0].contains("keys.prefix") && legacy[0].contains("keys.new_tab"));
    }

    #[test]
    fn legacy_keys_not_duplicated_on_malformed_config_salvage() {
        // A whole-config parse failure routes through the salvage path; legacy
        // diagnostics must appear exactly once, not once per code path.
        let _guard = crate::config::test_config_env_lock().lock().unwrap();
        let path = std::env::temp_dir().join(format!(
            "herdr-config-legacy-salvage-{}.toml",
            std::process::id()
        ));
        // `mode_pane = 1` is a type error that fails the whole-config parse and
        // forces section-isolated salvage; `prefix` is a dropped legacy field.
        std::fs::write(&path, "[keys]\nprefix = \"ctrl+a\"\nmode_pane = 1\n").unwrap();
        std::env::set_var(CONFIG_PATH_ENV_VAR, &path);

        let loaded = Config::load();
        let legacy_count = loaded
            .diagnostics
            .iter()
            .filter(|d| d.contains("no longer recognized"))
            .count();
        assert_eq!(
            legacy_count, 1,
            "legacy diagnostic duplicated: {:?}",
            loaded.diagnostics
        );

        std::env::remove_var(CONFIG_PATH_ENV_VAR);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn legacy_keys_command_and_indexed_remain_supported() {
        let _guard = crate::config::test_config_env_lock().lock().unwrap();
        let path = std::env::temp_dir().join(format!(
            "herdr-config-retained-keys-{}.toml",
            std::process::id()
        ));
        std::fs::write(
            &path,
            r#"
[keys.indexed]
tabs = "ctrl"

[[keys.command]]
key = "prefix+g"
command = "lazygit"
"#,
        )
        .unwrap();
        std::env::set_var(CONFIG_PATH_ENV_VAR, &path);

        let loaded = Config::load();

        assert!(
            !loaded
                .diagnostics
                .iter()
                .any(|d| d.contains("no longer recognized")),
            "retained fields must not be flagged legacy: {:?}",
            loaded.diagnostics
        );

        std::env::remove_var(CONFIG_PATH_ENV_VAR);
        let _ = std::fs::remove_file(path);
    }
}
