//! Fork: per-mode key tables for the zellij-style modal layer.
//!
//! Each table is one independent keyspace that is active only while its sticky
//! mode is (`[keys.pane]` while Pane mode is active, and so on), so bare keys
//! such as `h` or `n` are allowed here even though they are rejected as direct
//! bindings. The mode-entry keys and the extra direct shortcuts live as flat
//! fields on `KeysConfig`; only the tables are kept in this file so the fork's
//! footprint in upstream's `model.rs` stays small.

use serde::{Deserialize, Serialize};

use super::{BindingConfig, KeysConfig};

fn keys(values: &[&str]) -> BindingConfig {
    match values {
        [value] => BindingConfig::one(*value),
        values => BindingConfig::Many(values.iter().map(|value| (*value).to_string()).collect()),
    }
}

/// Add the fork's direct shortcuts on top of upstream's flat defaults, so an
/// upstream default change still keeps the fork key. A field the user sets in
/// config.toml replaces the whole default list, these keys included.
pub(crate) fn append_fork_direct_defaults(keys: &mut KeysConfig) {
    for (binding, extra) in [
        (&mut keys.focus_pane_left, "alt+h"),
        (&mut keys.focus_pane_down, "alt+j"),
        (&mut keys.focus_pane_up, "alt+k"),
        (&mut keys.focus_pane_right, "alt+l"),
        (&mut keys.close_pane, "alt+x"),
        (&mut keys.zoom, "alt+z"),
        (&mut keys.new_tab, "alt+t"),
        (&mut keys.rename_tab, "alt+r"),
        (&mut keys.detach, "ctrl+q"),
    ] {
        let mut values: Vec<String> = match std::mem::take(binding) {
            BindingConfig::One(value) if value.trim().is_empty() => Vec::new(),
            BindingConfig::One(value) => vec![value],
            BindingConfig::Many(values) => values,
        };
        values.push(extra.to_string());
        *binding = BindingConfig::Many(values);
    }
}

/// Pane mode (`[keys.pane]`), entered with `keys.mode_pane`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct PaneModeKeysConfig {
    /// Focus the pane to the left. Default: ["h", "left"].
    pub focus_left: BindingConfig,
    /// Focus the pane below. Default: ["j", "down"].
    pub focus_down: BindingConfig,
    /// Focus the pane above. Default: ["k", "up"].
    pub focus_up: BindingConfig,
    /// Focus the pane to the right. Default: ["l", "right"].
    pub focus_right: BindingConfig,
    /// Split along the focused pane's longer side. Default: "n".
    pub split_auto: BindingConfig,
    /// Split the focused pane downward. Default: "d".
    pub split_down: BindingConfig,
    /// Split the focused pane to the right. Default: "r".
    pub split_right: BindingConfig,
    /// Stack the focused pane with its adjacent sibling. Default: "s".
    pub stack: BindingConfig,
    /// Close the focused pane. Default: "x".
    pub close: BindingConfig,
    /// Toggle zoom for the focused pane. Default: ["f", "z"].
    pub zoom: BindingConfig,
    /// Rename the focused pane. Default: "c".
    pub rename: BindingConfig,
    /// Cycle to the next pane. Default: "p".
    pub cycle_next: BindingConfig,
}

impl Default for PaneModeKeysConfig {
    fn default() -> Self {
        Self {
            focus_left: keys(&["h", "left"]),
            focus_down: keys(&["j", "down"]),
            focus_up: keys(&["k", "up"]),
            focus_right: keys(&["l", "right"]),
            split_auto: keys(&["n"]),
            split_down: keys(&["d"]),
            split_right: keys(&["r"]),
            stack: keys(&["s"]),
            close: keys(&["x"]),
            zoom: keys(&["f", "z"]),
            rename: keys(&["c"]),
            cycle_next: keys(&["p"]),
        }
    }
}

/// Tab mode (`[keys.tab]`), entered with `keys.mode_tab`. Digits 1-9 always
/// switch to that tab.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct TabModeKeysConfig {
    /// Select the previous tab. Default: ["h", "left", "k", "up"].
    pub previous: BindingConfig,
    /// Select the next tab. Default: ["l", "right", "j", "down"].
    pub next: BindingConfig,
    /// Create a new tab. Default: "n".
    pub new: BindingConfig,
    /// Close the active tab. Default: "x".
    pub close: BindingConfig,
    /// Rename the active tab. Default: "r".
    pub rename: BindingConfig,
    /// Focus the last focused pane across workspaces and tabs. Default: "tab".
    pub last_pane: BindingConfig,
}

impl Default for TabModeKeysConfig {
    fn default() -> Self {
        Self {
            previous: keys(&["h", "left", "k", "up"]),
            next: keys(&["l", "right", "j", "down"]),
            new: keys(&["n"]),
            close: keys(&["x"]),
            rename: keys(&["r"]),
            last_pane: keys(&["tab"]),
        }
    }
}

/// Resize mode (`[keys.resize]`), entered with `keys.mode_resize` or
/// `keys.resize_mode`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct ResizeModeKeysConfig {
    /// Resize the focused pane toward the left. Default: ["h", "left"].
    pub increase_left: BindingConfig,
    /// Resize the focused pane downward. Default: ["j", "down"].
    pub increase_down: BindingConfig,
    /// Resize the focused pane upward. Default: ["k", "up"].
    pub increase_up: BindingConfig,
    /// Resize the focused pane toward the right. Default: ["l", "right"].
    pub increase_right: BindingConfig,
    /// Shrink the focused pane from the left. Default: "H".
    pub decrease_left: BindingConfig,
    /// Shrink the focused pane from below. Default: "J".
    pub decrease_down: BindingConfig,
    /// Shrink the focused pane from above. Default: "K".
    pub decrease_up: BindingConfig,
    /// Shrink the focused pane from the right. Default: "L".
    pub decrease_right: BindingConfig,
    /// Grow the focused pane. Default: ["plus", "="].
    pub grow: BindingConfig,
    /// Shrink the focused pane. Default: "-".
    pub shrink: BindingConfig,
}

impl Default for ResizeModeKeysConfig {
    fn default() -> Self {
        Self {
            increase_left: keys(&["h", "left"]),
            increase_down: keys(&["j", "down"]),
            increase_up: keys(&["k", "up"]),
            increase_right: keys(&["l", "right"]),
            decrease_left: keys(&["H"]),
            decrease_down: keys(&["J"]),
            decrease_up: keys(&["K"]),
            decrease_right: keys(&["L"]),
            grow: keys(&["plus", "="]),
            shrink: keys(&["-"]),
        }
    }
}

/// Move mode (`[keys.move]`), entered with `keys.mode_move`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct MoveModeKeysConfig {
    /// Swap the focused pane with the pane to the left. Default: ["h", "left"].
    pub swap_left: BindingConfig,
    /// Swap the focused pane with the pane below. Default: ["j", "down"].
    pub swap_down: BindingConfig,
    /// Swap the focused pane with the pane above. Default: ["k", "up"].
    pub swap_up: BindingConfig,
    /// Swap the focused pane with the pane to the right. Default: ["l", "right"].
    pub swap_right: BindingConfig,
    /// Cycle to the next pane. Default: ["n", "tab"].
    pub cycle_next: BindingConfig,
    /// Cycle to the previous pane. Default: "p".
    pub cycle_previous: BindingConfig,
}

impl Default for MoveModeKeysConfig {
    fn default() -> Self {
        Self {
            swap_left: keys(&["h", "left"]),
            swap_down: keys(&["j", "down"]),
            swap_up: keys(&["k", "up"]),
            swap_right: keys(&["l", "right"]),
            cycle_next: keys(&["n", "tab"]),
            cycle_previous: keys(&["p"]),
        }
    }
}

/// Session mode (`[keys.session]`), entered with `keys.mode_session`. Enter
/// focuses the selected workspace and stays in the mode; digits 1-9 switch tab.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct SessionModeKeysConfig {
    /// Move the workspace selection up. Default: ["k", "up"].
    pub workspace_up: BindingConfig,
    /// Move the workspace selection down. Default: ["j", "down"].
    pub workspace_down: BindingConfig,
    /// Focus the pane to the left. Default: ["h", "left"].
    pub focus_left: BindingConfig,
    /// Focus the pane to the right. Default: ["l", "right"].
    pub focus_right: BindingConfig,
    /// Cycle to the next pane. Default: "tab".
    pub cycle_next: BindingConfig,
    /// Open the session navigator. Default: "g".
    pub goto: BindingConfig,
    /// Open the workspace navigation surface. Default: "w".
    pub workspace_picker: BindingConfig,
    /// Create a new workspace. Default: "n".
    pub new_workspace: BindingConfig,
    /// Create a Git worktree from the selected workspace. Default: "N".
    pub new_worktree: BindingConfig,
    /// Rename the selected workspace. Default: "r".
    pub rename_workspace: BindingConfig,
    /// Close the selected workspace. Default: "x".
    pub close_workspace: BindingConfig,
    /// Open settings. Default: "s".
    pub settings: BindingConfig,
    /// Open keybinding help. Default: "?".
    pub help: BindingConfig,
    /// Detach the current client from its Herdr server. Default: "d".
    pub detach: BindingConfig,
    /// Focus the previous agent shown in the agent panel. Default: "[".
    pub previous_agent: BindingConfig,
    /// Focus the next agent shown in the agent panel. Default: "]".
    pub next_agent: BindingConfig,
}

impl Default for SessionModeKeysConfig {
    fn default() -> Self {
        Self {
            workspace_up: keys(&["k", "up"]),
            workspace_down: keys(&["j", "down"]),
            focus_left: keys(&["h", "left"]),
            focus_right: keys(&["l", "right"]),
            cycle_next: keys(&["tab"]),
            goto: keys(&["g"]),
            workspace_picker: keys(&["w"]),
            new_workspace: keys(&["n"]),
            new_worktree: keys(&["N"]),
            rename_workspace: keys(&["r"]),
            close_workspace: keys(&["x"]),
            settings: keys(&["s"]),
            help: keys(&["?"]),
            detach: keys(&["d"]),
            previous_agent: keys(&["["]),
            next_agent: keys(&["]"]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, KeysConfig};

    fn parse(toml_src: &str) -> Config {
        toml::from_str(toml_src).expect("config parses")
    }

    #[test]
    fn defaults_match_the_fork_keymap() {
        let keys = KeysConfig::default();
        assert_eq!(keys.default_mode, "modal");
        assert_eq!(keys.mode_pane, BindingConfig::one("ctrl+p"));
        assert_eq!(keys.mode_tab, BindingConfig::one("ctrl+t"));
        assert_eq!(keys.mode_resize, BindingConfig::one("ctrl+n"));
        assert_eq!(keys.mode_move, BindingConfig::one("ctrl+h"));
        assert_eq!(keys.mode_session, BindingConfig::one("ctrl+o"));
        assert_eq!(keys.mode_locked, BindingConfig::one("ctrl+g"));
        assert_eq!(keys.split_auto, BindingConfig::one("alt+n"));
        assert_eq!(keys.resize_grow, keys_of(&["alt+=", "alt+plus"]));
        assert_eq!(keys.pane.split_auto, BindingConfig::one("n"));
        assert_eq!(keys.tab.last_pane, BindingConfig::one("tab"));
        assert_eq!(keys.resize.decrease_left, BindingConfig::one("H"));
        assert_eq!(keys.move_.cycle_next, keys_of(&["n", "tab"]));
        assert_eq!(keys.session.new_worktree, BindingConfig::one("N"));
    }

    fn keys_of(values: &[&str]) -> BindingConfig {
        keys(values)
    }

    #[test]
    fn partial_mode_table_keeps_remaining_defaults() {
        let config = parse(
            r#"
[keys]
mode_pane = "ctrl+y"

[keys.move]
swap_left = "a"
"#,
        );
        let keys = &config.keys;
        assert_eq!(keys.mode_pane, BindingConfig::one("ctrl+y"));
        assert_eq!(keys.mode_tab, BindingConfig::one("ctrl+t"));
        assert_eq!(keys.move_.swap_left, BindingConfig::one("a"));
        assert_eq!(keys.move_.swap_down, keys_of(&["j", "down"]));
        assert_eq!(keys.pane, PaneModeKeysConfig::default());
        assert!(keys.key_field_is_user_configured("mode_pane"));
        assert!(keys.key_field_is_user_configured("move_"));
        assert!(!keys.key_field_is_user_configured("pane"));
    }

    #[test]
    fn unknown_keys_in_the_modal_layer_are_still_reported() {
        let value: toml::Value = r#"
mode_pain = "ctrl+p"

[pane]
splitt = "n"

[move]
swap_left = "a"
"#
        .parse()
        .expect("toml parses");
        let mut ignored = Vec::new();
        let keys: KeysConfig =
            serde_ignored::deserialize(value, |path| ignored.push(path.to_string()))
                .expect("keys deserialize");
        // `?` is serde_ignored's Option layer; io.rs drops it, so users see keys.pane.splitt.
        assert_eq!(ignored, vec!["mode_pain", "pane.?.splitt"]);
        assert_eq!(keys.move_.swap_left, BindingConfig::one("a"));
    }

    #[test]
    fn keybinding_profile_carries_user_modal_settings_but_not_default_mode() {
        let config = parse(
            r#"
[keys]
default_mode = "locked"
mode_pane = "ctrl+y"

[keys.tab]
new = "a"
"#,
        );
        let profile = config
            .local_keybindings_profile_toml()
            .expect("profile serializes");
        assert!(!profile.contains("default_mode"), "{profile}");
        assert!(profile.contains("[keys.tab]"), "{profile}");
        assert!(!profile.contains("[keys.pane]"), "{profile}");

        let reparsed = parse(&profile);
        assert_eq!(reparsed.keys.default_mode, "modal");
        assert_eq!(reparsed.keys.mode_pane, BindingConfig::one("ctrl+y"));
        assert_eq!(reparsed.keys.tab.new, BindingConfig::one("a"));
        assert_eq!(reparsed.keys.tab.close, BindingConfig::one("x"));
    }

    #[test]
    fn serialized_keys_config_round_trips_the_move_table() {
        let mut keys = KeysConfig::default();
        keys.move_.swap_up = BindingConfig::one("w");
        let text = toml::to_string(&keys).expect("keys serialize");
        assert!(text.contains("[move]"), "{text}");
        let reparsed: KeysConfig = toml::from_str(&text).expect("keys reparse");
        assert_eq!(reparsed.move_, keys.move_);
        assert_eq!(reparsed.session, keys.session);
    }
}
