#[cfg(test)]
use crossterm::event::KeyEvent;
use crossterm::event::{KeyCode, KeyModifiers};
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::model::KeysConfig;
use super::Config;
use crate::input::TerminalKey;

pub type KeyCombo = (KeyCode, KeyModifiers);

#[derive(Debug, Clone)]
pub struct LiveKeybindConfig {
    pub prefix: KeyCombo,
    pub keybinds: Keybinds,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BindingConfig {
    One(String),
    Many(Vec<String>),
}

impl Default for BindingConfig {
    fn default() -> Self {
        Self::One(String::new())
    }
}

impl BindingConfig {
    pub fn one(value: impl Into<String>) -> Self {
        Self::One(value.into())
    }

    pub fn empty() -> Self {
        Self::One(String::new())
    }

    fn values(&self) -> Vec<&str> {
        match self {
            Self::One(value) => vec![value.as_str()],
            Self::Many(values) => values.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CommandKeybindType {
    #[default]
    Shell,
    Pane,
    PluginAction,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct CommandKeybindConfig {
    /// Key that runs a command. Use `prefix+g` for prefix mode or a modified chord for direct mode.
    pub key: BindingConfig,
    /// Command executed either in the background shell or inside a pane.
    pub command: String,
    /// Command execution mode. Default: "shell".
    #[serde(rename = "type")]
    pub action_type: CommandKeybindType,
    /// Optional user-defined description for this custom command.
    pub description: Option<String>,
}

impl Default for CommandKeybindConfig {
    fn default() -> Self {
        Self {
            key: BindingConfig::empty(),
            command: String::new(),
            action_type: CommandKeybindType::Shell,
            description: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomCommandAction {
    Shell,
    Pane,
    PluginAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingTrigger {
    Direct(KeyCombo),
    Prefix(KeyCombo),
}

impl BindingTrigger {
    pub fn combo(self) -> KeyCombo {
        match self {
            Self::Direct(combo) | Self::Prefix(combo) => combo,
        }
    }

    pub fn is_direct(self) -> bool {
        matches!(self, Self::Direct(_))
    }

    pub fn is_prefix(self) -> bool {
        matches!(self, Self::Prefix(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBinding {
    pub trigger: BindingTrigger,
    pub label: String,
}

impl ResolvedBinding {
    #[cfg(test)]
    fn matches_key_event(&self, key: &KeyEvent) -> bool {
        key_event_matches_combo(key, self.trigger.combo())
    }

    fn matches_terminal_key(&self, key: TerminalKey) -> bool {
        terminal_key_matches_combo(key, self.trigger.combo())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionKeybinds {
    pub bindings: Vec<ResolvedBinding>,
}

impl ActionKeybinds {
    #[cfg(test)]
    pub fn prefix(label: &str) -> Self {
        let raw = if label.starts_with("prefix+") {
            label.to_string()
        } else {
            format!("prefix+{label}")
        };
        let trigger = parse_binding_string(&raw)
            .and_then(|parsed| match parsed {
                ParsedBinding::Single(binding) => Some(binding),
                ParsedBinding::Range(_) => None,
            })
            .expect("prefix binding should parse");
        Self {
            bindings: vec![trigger],
        }
    }

    #[cfg(test)]
    pub fn direct(label: &str) -> Self {
        let trigger = parse_binding_string(label)
            .and_then(|parsed| match parsed {
                ParsedBinding::Single(binding) => Some(binding),
                ParsedBinding::Range(_) => None,
            })
            .expect("direct binding should parse");
        Self {
            bindings: vec![trigger],
        }
    }

    #[cfg(test)]
    pub fn matches_prefix(&self, key: &KeyEvent) -> bool {
        self.bindings
            .iter()
            .any(|binding| binding.trigger.is_prefix() && binding.matches_key_event(key))
    }

    pub fn matches_prefix_key(&self, key: TerminalKey) -> bool {
        self.bindings
            .iter()
            .any(|binding| binding.trigger.is_prefix() && binding.matches_terminal_key(key))
    }

    pub fn matches_direct_key(&self, key: TerminalKey) -> bool {
        self.bindings
            .iter()
            .any(|binding| binding.trigger.is_direct() && binding.matches_terminal_key(key))
    }

    pub fn labels(&self) -> Vec<String> {
        self.bindings
            .iter()
            .map(|binding| binding.label.clone())
            .collect()
    }

    pub fn label(&self) -> Option<String> {
        let labels = self.labels();
        if labels.is_empty() {
            None
        } else {
            Some(labels.join(" / "))
        }
    }

    pub fn prefix_rhs_label(&self) -> Option<String> {
        let labels: Vec<String> = self
            .bindings
            .iter()
            .filter(|binding| binding.trigger.is_prefix())
            .map(|binding| {
                binding
                    .label
                    .strip_prefix("prefix+")
                    .unwrap_or(&binding.label)
                    .to_string()
            })
            .collect();
        if labels.is_empty() {
            None
        } else {
            Some(labels.join(" / "))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedKeybind {
    pub trigger: BindingTrigger,
    pub label: String,
}

impl IndexedKeybind {
    pub fn matched_index(&self, key: TerminalKey) -> Option<usize> {
        let KeyCode::Char(c @ '1'..='9') = key.code else {
            return None;
        };
        if terminal_key_matches_combo(key, self.trigger.combo()) {
            Some((c as usize) - ('1' as usize))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct CustomCommandKeybind {
    pub bindings: ActionKeybinds,
    pub label: String,
    pub command: String,
    pub action: CustomCommandAction,
    pub description: Option<String>,
}

/// Parsed keybinds for Herdr actions.
#[derive(Debug, Clone)]
pub struct NavigateKeybinds {
    pub workspace_up: ActionKeybinds,
    pub workspace_down: ActionKeybinds,
    pub pane_left: ActionKeybinds,
    pub pane_down: ActionKeybinds,
    pub pane_up: ActionKeybinds,
    pub pane_right: ActionKeybinds,
}

/// Base interaction mode selected by `keys.default_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DefaultMode {
    /// Zellij-style Ctrl+letter modal interaction.
    #[default]
    Modal,
    /// Start in Locked mode (prefix-style users pair this with `mode_tmux`).
    Locked,
}

/// Resolved mode-entry key combos. `None` means the key was dropped during
/// validation (parse failure with no usable fallback, or a duplicate of an
/// earlier mode-entry key), so that mode is not reachable by its entry key.
///
/// These are resolved and validated at config load; the input dispatcher does
/// not yet read them while the modal system is being built out.
#[derive(Debug, Clone, Copy, Default)]
#[allow(dead_code)] // not yet read by the input dispatcher
pub struct ModeEntryKeys {
    pub pane: Option<KeyCombo>,
    pub tab: Option<KeyCombo>,
    pub resize: Option<KeyCombo>,
    pub move_: Option<KeyCombo>,
    pub session: Option<KeyCombo>,
    pub locked: Option<KeyCombo>,
    pub tmux: Option<KeyCombo>,
}

/// Parsed keybinds for Herdr actions.
#[derive(Debug, Clone)]
pub struct Keybinds {
    pub navigate: NavigateKeybinds,
    pub help: ActionKeybinds,
    pub settings: ActionKeybinds,
    pub new_workspace: ActionKeybinds,
    pub new_worktree: ActionKeybinds,
    pub open_worktree: ActionKeybinds,
    pub remove_worktree: ActionKeybinds,
    pub rename_workspace: ActionKeybinds,
    pub close_workspace: ActionKeybinds,
    pub workspace_picker: ActionKeybinds,
    pub goto: ActionKeybinds,
    pub detach: ActionKeybinds,
    pub reload_config: ActionKeybinds,
    pub open_notification_target: ActionKeybinds,
    pub previous_workspace: ActionKeybinds,
    pub next_workspace: ActionKeybinds,
    pub previous_agent: ActionKeybinds,
    pub next_agent: ActionKeybinds,
    pub focus_agent: Vec<IndexedKeybind>,
    pub new_tab: ActionKeybinds,
    pub rename_tab: ActionKeybinds,
    pub previous_tab: ActionKeybinds,
    pub next_tab: ActionKeybinds,
    pub switch_tab: Vec<IndexedKeybind>,
    pub switch_workspace: Vec<IndexedKeybind>,
    pub close_tab: ActionKeybinds,
    pub rename_pane: ActionKeybinds,
    pub edit_scrollback: ActionKeybinds,
    pub copy_mode: ActionKeybinds,
    pub focus_pane_left: ActionKeybinds,
    pub focus_pane_down: ActionKeybinds,
    pub focus_pane_up: ActionKeybinds,
    pub focus_pane_right: ActionKeybinds,
    pub swap_pane_left: ActionKeybinds,
    pub swap_pane_down: ActionKeybinds,
    pub swap_pane_up: ActionKeybinds,
    pub swap_pane_right: ActionKeybinds,
    pub cycle_pane_next: ActionKeybinds,
    pub cycle_pane_previous: ActionKeybinds,
    pub last_pane: ActionKeybinds,
    pub split_vertical: ActionKeybinds,
    pub split_horizontal: ActionKeybinds,
    pub stack_pane: ActionKeybinds,
    pub unstack_pane: ActionKeybinds,
    pub close_pane: ActionKeybinds,
    pub break_pane_to_tab: ActionKeybinds,
    pub zoom: ActionKeybinds,
    pub split_auto: ActionKeybinds,
    pub move_tab_left: ActionKeybinds,
    pub move_tab_right: ActionKeybinds,
    pub resize_grow: ActionKeybinds,
    pub resize_shrink: ActionKeybinds,
    pub resize_mode: ActionKeybinds,
    pub toggle_sidebar: ActionKeybinds,
    pub toggle_floating: ActionKeybinds,
    pub new_floating_pane: ActionKeybinds,
    pub close_floating_pane: ActionKeybinds,
    pub move_floating_left: ActionKeybinds,
    pub move_floating_down: ActionKeybinds,
    pub move_floating_up: ActionKeybinds,
    pub move_floating_right: ActionKeybinds,
    pub resize_floating_grow: ActionKeybinds,
    pub resize_floating_shrink: ActionKeybinds,
    pub cycle_floating_next: ActionKeybinds,
    pub cycle_floating_previous: ActionKeybinds,
    pub custom_commands: Vec<CustomCommandKeybind>,
    /// Base interaction mode from `keys.default_mode`.
    pub default_mode: DefaultMode,
    /// Resolved, validated mode-entry key combos for the modal system.
    pub mode_entry: ModeEntryKeys,
}

impl Default for Keybinds {
    fn default() -> Self {
        Config::default().keybinds()
    }
}

#[derive(Clone)]
enum ParsedBinding {
    Single(ResolvedBinding),
    Range(Vec<ResolvedBinding>),
}

struct BindingRegistry {
    prefix_combo: KeyCombo,
    direct: std::collections::HashMap<KeyCombo, String>,
    prefix: std::collections::HashMap<KeyCombo, String>,
}

impl BindingRegistry {
    fn new(prefix_combo: KeyCombo) -> Self {
        Self {
            prefix_combo: normalize_key_combo(prefix_combo),
            direct: std::collections::HashMap::new(),
            prefix: std::collections::HashMap::new(),
        }
    }

    fn reserve_direct(&mut self, combo: KeyCombo, field: &str) {
        self.direct
            .entry(normalize_key_combo(combo))
            .or_insert_with(|| field.to_string());
    }

    fn prefix_rhs_is_reserved(&self, combo: KeyCombo) -> bool {
        normalize_key_combo(combo) == self.prefix_combo
    }

    fn conflict(&self, binding: &ResolvedBinding) -> Option<&str> {
        match binding.trigger {
            BindingTrigger::Direct(combo) => self
                .direct
                .get(&normalize_key_combo(combo))
                .map(String::as_str),
            BindingTrigger::Prefix(combo) => self
                .prefix
                .get(&normalize_key_combo(combo))
                .map(String::as_str),
        }
    }

    fn register(&mut self, binding: &ResolvedBinding, field: &str) {
        match binding.trigger {
            BindingTrigger::Direct(combo) => {
                self.direct
                    .insert(normalize_key_combo(combo), field.to_string());
            }
            BindingTrigger::Prefix(combo) => {
                self.prefix
                    .insert(normalize_key_combo(combo), field.to_string());
            }
        }
    }
}

/// Transitional bridge: the previously-released flat keymap that feeds the
/// dispatched `Keybinds` unchanged while the modal schema is introduced. The
/// config schema is now mode-structured and validated, but runtime dispatch
/// still reads these released bindings, so behavior is preserved until dispatch
/// is rewired onto the modal tables (at which point this struct is removed).
/// The `prefix`-relative bindings resolve against the configured
/// `keys.mode_tmux` (default `ctrl+b`, the previous prefix default).
struct ReleasedKeys {
    help: BindingConfig,
    settings: BindingConfig,
    new_workspace: BindingConfig,
    new_worktree: BindingConfig,
    open_worktree: BindingConfig,
    remove_worktree: BindingConfig,
    rename_workspace: BindingConfig,
    close_workspace: BindingConfig,
    workspace_picker: BindingConfig,
    goto: BindingConfig,
    navigate_workspace_up: BindingConfig,
    navigate_workspace_down: BindingConfig,
    navigate_pane_left: BindingConfig,
    navigate_pane_down: BindingConfig,
    navigate_pane_up: BindingConfig,
    navigate_pane_right: BindingConfig,
    detach: BindingConfig,
    reload_config: BindingConfig,
    open_notification_target: BindingConfig,
    previous_workspace: BindingConfig,
    next_workspace: BindingConfig,
    previous_agent: BindingConfig,
    next_agent: BindingConfig,
    focus_agent: BindingConfig,
    new_tab: BindingConfig,
    rename_tab: BindingConfig,
    previous_tab: BindingConfig,
    next_tab: BindingConfig,
    switch_tab: BindingConfig,
    switch_workspace: BindingConfig,
    close_tab: BindingConfig,
    rename_pane: BindingConfig,
    edit_scrollback: BindingConfig,
    copy_mode: BindingConfig,
    focus_pane_left: BindingConfig,
    focus_pane_down: BindingConfig,
    focus_pane_up: BindingConfig,
    focus_pane_right: BindingConfig,
    swap_pane_left: BindingConfig,
    swap_pane_down: BindingConfig,
    swap_pane_up: BindingConfig,
    swap_pane_right: BindingConfig,
    cycle_pane_next: BindingConfig,
    cycle_pane_previous: BindingConfig,
    last_pane: BindingConfig,
    split_vertical: BindingConfig,
    split_horizontal: BindingConfig,
    stack_pane: BindingConfig,
    unstack_pane: BindingConfig,
    close_pane: BindingConfig,
    break_pane_to_tab: BindingConfig,
    zoom: BindingConfig,
    split_auto: BindingConfig,
    move_tab_left: BindingConfig,
    move_tab_right: BindingConfig,
    resize_grow: BindingConfig,
    resize_shrink: BindingConfig,
    resize_mode: BindingConfig,
    toggle_sidebar: BindingConfig,
    toggle_floating: BindingConfig,
    new_floating_pane: BindingConfig,
    close_floating_pane: BindingConfig,
    move_floating_left: BindingConfig,
    move_floating_down: BindingConfig,
    move_floating_up: BindingConfig,
    move_floating_right: BindingConfig,
    resize_floating_grow: BindingConfig,
    resize_floating_shrink: BindingConfig,
    cycle_floating_next: BindingConfig,
    cycle_floating_previous: BindingConfig,
}

impl Default for ReleasedKeys {
    fn default() -> Self {
        Self {
            help: BindingConfig::one("prefix+?"),
            settings: BindingConfig::one("prefix+s"),
            new_workspace: BindingConfig::one("prefix+shift+n"),
            new_worktree: BindingConfig::one("prefix+shift+g"),
            open_worktree: BindingConfig::empty(),
            remove_worktree: BindingConfig::empty(),
            rename_workspace: BindingConfig::one("prefix+shift+w"),
            close_workspace: BindingConfig::one("prefix+shift+d"),
            workspace_picker: BindingConfig::one("prefix+w"),
            goto: BindingConfig::one("prefix+g"),
            navigate_workspace_up: BindingConfig::one("up"),
            navigate_workspace_down: BindingConfig::one("down"),
            navigate_pane_left: BindingConfig::one("h"),
            navigate_pane_down: BindingConfig::one("j"),
            navigate_pane_up: BindingConfig::one("k"),
            navigate_pane_right: BindingConfig::one("l"),
            detach: BindingConfig::one("prefix+q"),
            reload_config: BindingConfig::one("prefix+shift+r"),
            open_notification_target: BindingConfig::one("prefix+o"),
            previous_workspace: BindingConfig::empty(),
            next_workspace: BindingConfig::empty(),
            previous_agent: BindingConfig::empty(),
            next_agent: BindingConfig::empty(),
            focus_agent: BindingConfig::empty(),
            new_tab: BindingConfig::one("prefix+c"),
            rename_tab: BindingConfig::one("prefix+shift+t"),
            previous_tab: BindingConfig::one("prefix+p"),
            next_tab: BindingConfig::one("prefix+n"),
            switch_tab: BindingConfig::one("prefix+1..9"),
            switch_workspace: BindingConfig::empty(),
            close_tab: BindingConfig::one("prefix+shift+x"),
            rename_pane: BindingConfig::one("prefix+shift+p"),
            edit_scrollback: BindingConfig::one("prefix+e"),
            copy_mode: BindingConfig::one("prefix+["),
            focus_pane_left: BindingConfig::Many(vec!["prefix+h".into(), "alt+h".into()]),
            focus_pane_down: BindingConfig::Many(vec!["prefix+j".into(), "alt+j".into()]),
            focus_pane_up: BindingConfig::Many(vec!["prefix+k".into(), "alt+k".into()]),
            focus_pane_right: BindingConfig::Many(vec!["prefix+l".into(), "alt+l".into()]),
            swap_pane_left: BindingConfig::one("prefix+shift+h"),
            swap_pane_down: BindingConfig::one("prefix+shift+j"),
            swap_pane_up: BindingConfig::one("prefix+shift+k"),
            swap_pane_right: BindingConfig::one("prefix+shift+l"),
            cycle_pane_next: BindingConfig::one("prefix+tab"),
            cycle_pane_previous: BindingConfig::one("prefix+shift+tab"),
            last_pane: BindingConfig::empty(),
            split_vertical: BindingConfig::one("prefix+v"),
            split_horizontal: BindingConfig::one("prefix+minus"),
            stack_pane: BindingConfig::one("prefix+shift+s"),
            unstack_pane: BindingConfig::one("prefix+shift+u"),
            close_pane: BindingConfig::Many(vec!["prefix+x".into(), "alt+x".into()]),
            break_pane_to_tab: BindingConfig::one("prefix+!"),
            zoom: BindingConfig::Many(vec!["prefix+z".into(), "alt+z".into()]),
            split_auto: BindingConfig::one("alt+n"),
            move_tab_left: BindingConfig::one("alt+i"),
            move_tab_right: BindingConfig::one("alt+o"),
            resize_grow: BindingConfig::one("alt+="),
            resize_shrink: BindingConfig::one("alt+-"),
            resize_mode: BindingConfig::one("prefix+r"),
            toggle_sidebar: BindingConfig::one("prefix+b"),
            toggle_floating: BindingConfig::one("prefix+f"),
            new_floating_pane: BindingConfig::one("prefix+shift+f"),
            close_floating_pane: BindingConfig::empty(),
            move_floating_left: BindingConfig::empty(),
            move_floating_down: BindingConfig::empty(),
            move_floating_up: BindingConfig::empty(),
            move_floating_right: BindingConfig::empty(),
            resize_floating_grow: BindingConfig::empty(),
            resize_floating_shrink: BindingConfig::empty(),
            cycle_floating_next: BindingConfig::empty(),
            cycle_floating_previous: BindingConfig::empty(),
        }
    }
}

impl Config {
    pub(super) fn validated_keybinds(&self) -> (Option<String>, KeyCombo, Vec<String>, Keybinds) {
        let mut diagnostics = Vec::new();

        // Prefix (tmux/one-shot) key now lives under `keys.mode_tmux`.
        let (prefix, prefix_diag) = parse_key_combo_with_diagnostic(
            &self.keys.mode_tmux,
            "keys.mode_tmux",
            (KeyCode::Char('b'), KeyModifiers::CONTROL),
        );
        if let Some(diag) = &prefix_diag {
            warn!(message = %diag, "config diagnostic");
        }

        // Dispatch reads the released keymap unchanged (see `ReleasedKeys`).
        let released = ReleasedKeys::default();
        let mut registry = BindingRegistry::new(prefix);
        registry.reserve_direct(prefix, "keys.mode_tmux");
        let mut navigate_registry = BindingRegistry::new(prefix);
        navigate_registry.reserve_direct(prefix, "keys.mode_tmux");
        reserve_navigate_runtime_keys(&mut navigate_registry);

        macro_rules! action {
            ($field:literal, $config:expr) => {
                parse_action_bindings($field, $config, false, &mut registry, &mut diagnostics)
            };
        }
        macro_rules! indexed {
            ($field:literal, $config:expr) => {
                parse_indexed_bindings($field, $config, &mut registry, &mut diagnostics)
            };
        }

        let mut keybinds = Keybinds {
            navigate: NavigateKeybinds {
                workspace_up: parse_navigate_bindings(
                    "keys.navigate_workspace_up",
                    &released.navigate_workspace_up,
                    &mut navigate_registry,
                    &mut diagnostics,
                ),
                workspace_down: parse_navigate_bindings(
                    "keys.navigate_workspace_down",
                    &released.navigate_workspace_down,
                    &mut navigate_registry,
                    &mut diagnostics,
                ),
                pane_left: parse_navigate_bindings(
                    "keys.navigate_pane_left",
                    &released.navigate_pane_left,
                    &mut navigate_registry,
                    &mut diagnostics,
                ),
                pane_down: parse_navigate_bindings(
                    "keys.navigate_pane_down",
                    &released.navigate_pane_down,
                    &mut navigate_registry,
                    &mut diagnostics,
                ),
                pane_up: parse_navigate_bindings(
                    "keys.navigate_pane_up",
                    &released.navigate_pane_up,
                    &mut navigate_registry,
                    &mut diagnostics,
                ),
                pane_right: parse_navigate_bindings(
                    "keys.navigate_pane_right",
                    &released.navigate_pane_right,
                    &mut navigate_registry,
                    &mut diagnostics,
                ),
            },
            help: action!("keys.help", &released.help),
            settings: action!("keys.settings", &released.settings),
            new_workspace: action!("keys.new_workspace", &released.new_workspace),
            new_worktree: action!("keys.new_worktree", &released.new_worktree),
            open_worktree: action!("keys.open_worktree", &released.open_worktree),
            remove_worktree: action!("keys.remove_worktree", &released.remove_worktree),
            rename_workspace: action!("keys.rename_workspace", &released.rename_workspace),
            close_workspace: action!("keys.close_workspace", &released.close_workspace),
            workspace_picker: action!("keys.workspace_picker", &released.workspace_picker),
            goto: action!("keys.goto", &released.goto),
            detach: action!("keys.detach", &released.detach),
            reload_config: action!("keys.reload_config", &released.reload_config),
            open_notification_target: action!(
                "keys.open_notification_target",
                &released.open_notification_target
            ),
            previous_workspace: action!("keys.previous_workspace", &released.previous_workspace),
            next_workspace: action!("keys.next_workspace", &released.next_workspace),
            previous_agent: action!("keys.previous_agent", &released.previous_agent),
            next_agent: action!("keys.next_agent", &released.next_agent),
            focus_agent: indexed!("keys.focus_agent", &released.focus_agent),
            new_tab: action!("keys.new_tab", &released.new_tab),
            rename_tab: action!("keys.rename_tab", &released.rename_tab),
            previous_tab: action!("keys.previous_tab", &released.previous_tab),
            next_tab: action!("keys.next_tab", &released.next_tab),
            switch_tab: indexed!("keys.switch_tab", &released.switch_tab),
            switch_workspace: indexed!("keys.switch_workspace", &released.switch_workspace),
            close_tab: action!("keys.close_tab", &released.close_tab),
            rename_pane: action!("keys.rename_pane", &released.rename_pane),
            edit_scrollback: action!("keys.edit_scrollback", &released.edit_scrollback),
            copy_mode: action!("keys.copy_mode", &released.copy_mode),
            focus_pane_left: action!("keys.focus_pane_left", &released.focus_pane_left),
            focus_pane_down: action!("keys.focus_pane_down", &released.focus_pane_down),
            focus_pane_up: action!("keys.focus_pane_up", &released.focus_pane_up),
            focus_pane_right: action!("keys.focus_pane_right", &released.focus_pane_right),
            swap_pane_left: action!("keys.swap_pane_left", &released.swap_pane_left),
            swap_pane_down: action!("keys.swap_pane_down", &released.swap_pane_down),
            swap_pane_up: action!("keys.swap_pane_up", &released.swap_pane_up),
            swap_pane_right: action!("keys.swap_pane_right", &released.swap_pane_right),
            last_pane: action!("keys.last_pane", &released.last_pane),
            cycle_pane_next: action!("keys.cycle_pane_next", &released.cycle_pane_next),
            cycle_pane_previous: action!("keys.cycle_pane_previous", &released.cycle_pane_previous),
            split_vertical: action!("keys.split_vertical", &released.split_vertical),
            split_horizontal: action!("keys.split_horizontal", &released.split_horizontal),
            stack_pane: action!("keys.stack_pane", &released.stack_pane),
            unstack_pane: action!("keys.unstack_pane", &released.unstack_pane),
            close_pane: action!("keys.close_pane", &released.close_pane),
            break_pane_to_tab: action!("keys.break_pane_to_tab", &released.break_pane_to_tab),
            zoom: action!("keys.zoom", &released.zoom),
            split_auto: action!("keys.split_auto", &released.split_auto),
            move_tab_left: action!("keys.move_tab_left", &released.move_tab_left),
            move_tab_right: action!("keys.move_tab_right", &released.move_tab_right),
            resize_grow: action!("keys.resize_grow", &released.resize_grow),
            resize_shrink: action!("keys.resize_shrink", &released.resize_shrink),
            resize_mode: action!("keys.resize_mode", &released.resize_mode),
            toggle_sidebar: action!("keys.toggle_sidebar", &released.toggle_sidebar),
            toggle_floating: action!("keys.toggle_floating", &released.toggle_floating),
            new_floating_pane: action!("keys.new_floating_pane", &released.new_floating_pane),
            close_floating_pane: action!("keys.close_floating_pane", &released.close_floating_pane),
            move_floating_left: action!("keys.move_floating_left", &released.move_floating_left),
            move_floating_down: action!("keys.move_floating_down", &released.move_floating_down),
            move_floating_up: action!("keys.move_floating_up", &released.move_floating_up),
            move_floating_right: action!("keys.move_floating_right", &released.move_floating_right),
            resize_floating_grow: action!(
                "keys.resize_floating_grow",
                &released.resize_floating_grow
            ),
            resize_floating_shrink: action!(
                "keys.resize_floating_shrink",
                &released.resize_floating_shrink
            ),
            cycle_floating_next: action!("keys.cycle_floating_next", &released.cycle_floating_next),
            cycle_floating_previous: action!(
                "keys.cycle_floating_previous",
                &released.cycle_floating_previous
            ),
            custom_commands: Vec::new(),
            default_mode: DefaultMode::default(),
            mode_entry: ModeEntryKeys::default(),
        };

        append_legacy_indexed_bindings(
            &mut keybinds.switch_tab,
            "keys.indexed.tabs",
            &self.keys.indexed.tabs,
            &mut registry,
            &mut diagnostics,
        );
        append_legacy_indexed_bindings(
            &mut keybinds.switch_workspace,
            "keys.indexed.workspaces",
            &self.keys.indexed.workspaces,
            &mut registry,
            &mut diagnostics,
        );
        append_legacy_indexed_bindings(
            &mut keybinds.focus_agent,
            "keys.indexed.agents",
            &self.keys.indexed.agents,
            &mut registry,
            &mut diagnostics,
        );

        for (index, command) in self.keys.command.iter().enumerate() {
            let key_field = format!("keys.command[{index}].key");
            let command_field = format!("keys.command[{index}].command");

            if command.command.trim().is_empty() {
                let diag =
                    format!("empty custom command: {command_field}; disabling custom command");
                warn!(message = %diag, "config diagnostic");
                diagnostics.push(diag);
                continue;
            }

            let bindings = parse_action_bindings_owned(
                &key_field,
                &command.key,
                false,
                &mut registry,
                &mut diagnostics,
            );
            if bindings.bindings.is_empty() {
                continue;
            }

            let action = match command.action_type {
                CommandKeybindType::Shell => CustomCommandAction::Shell,
                CommandKeybindType::Pane => CustomCommandAction::Pane,
                CommandKeybindType::PluginAction => CustomCommandAction::PluginAction,
            };
            let label = bindings.label().unwrap_or_else(|| "unset".to_string());
            keybinds.custom_commands.push(CustomCommandKeybind {
                bindings,
                label,
                command: command.command.clone(),
                action,
                description: command.description.clone(),
            });
        }

        // Validate the modal schema (mode-entry keys, shared/per-mode tables,
        // default_mode) and surface diagnostics. The dispatcher does not yet
        // read the resolved modal bindings; this resolves and validates them.
        let (default_mode, mode_entry) = validate_modal_keys(&self.keys, prefix, &mut diagnostics);
        keybinds.default_mode = default_mode;
        keybinds.mode_entry = mode_entry;

        (prefix_diag, prefix, diagnostics, keybinds)
    }
}

/// Validate the modal keybinding schema and resolve the base mode + mode-entry
/// keys. Enforces the three-way precedence between binding namespaces:
/// mode-entry keys must be mutually distinct; a mode-entry key beats a shared
/// binding on the same combo; a shared binding beats a per-mode bare key (the
/// shadowed bare key gets a diagnostic). Also resolves `default_mode` and the
/// locked-mode reachability invariant. Resolution only — no dispatch.
fn validate_modal_keys(
    keys: &KeysConfig,
    prefix: KeyCombo,
    diagnostics: &mut Vec<String>,
) -> (DefaultMode, ModeEntryKeys) {
    use std::collections::HashMap;

    // 1. Mode-entry keys: parse with per-key fallback, enforce mutual
    //    distinctness (first declared wins, later duplicate disabled). Parsed
    //    in declaration order so "first declared wins" is deterministic.
    let mut entry_seen: HashMap<KeyCombo, &'static str> = HashMap::new();
    let pane = parse_mode_entry_key(
        "keys.mode_pane",
        &keys.mode_pane,
        (KeyCode::Char('p'), KeyModifiers::CONTROL),
        &mut entry_seen,
        diagnostics,
    );
    let tab = parse_mode_entry_key(
        "keys.mode_tab",
        &keys.mode_tab,
        (KeyCode::Char('t'), KeyModifiers::CONTROL),
        &mut entry_seen,
        diagnostics,
    );
    let resize = parse_mode_entry_key(
        "keys.mode_resize",
        &keys.mode_resize,
        (KeyCode::Char('n'), KeyModifiers::CONTROL),
        &mut entry_seen,
        diagnostics,
    );
    let move_ = parse_mode_entry_key(
        "keys.mode_move",
        &keys.mode_move,
        (KeyCode::Char('h'), KeyModifiers::CONTROL),
        &mut entry_seen,
        diagnostics,
    );
    let session = parse_mode_entry_key(
        "keys.mode_session",
        &keys.mode_session,
        (KeyCode::Char('o'), KeyModifiers::CONTROL),
        &mut entry_seen,
        diagnostics,
    );
    let locked = parse_mode_entry_key(
        "keys.mode_locked",
        &keys.mode_locked,
        (KeyCode::Char('g'), KeyModifiers::CONTROL),
        &mut entry_seen,
        diagnostics,
    );
    // `mode_tmux` is the prefix key, already parsed (and already diagnosed on
    // failure) by the caller. Reuse that resolved combo so an invalid value is
    // not reported twice; still register it for mode-entry distinctness.
    let tmux = register_mode_entry_combo("keys.mode_tmux", prefix, &mut entry_seen, diagnostics);
    let mode_entry = ModeEntryKeys {
        pane,
        tab,
        resize,
        move_,
        session,
        locked,
        tmux,
    };

    // 2. Shared bindings: one global keyspace alongside mode-entry keys. A
    //    shared key that collides with a mode-entry key is dropped (mode-entry
    //    wins). Shared bindings remain global direct binds, so the
    //    unsafe-printable guard still applies. Resolved shared combos feed the
    //    per-mode shadow check below.
    let mut shared_combos: HashMap<KeyCombo, String> = HashMap::new();
    for (field, config) in shared_fields(&keys.shared) {
        for raw in config.values() {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            let Some(combo) = parse_key_combo(raw) else {
                push_diagnostic(
                    diagnostics,
                    format!("invalid keybinding: {field} = {raw:?}; disabling binding"),
                );
                continue;
            };
            let combo = normalize_key_combo(combo);
            if let Some(entry_field) = entry_seen.get(&combo) {
                push_diagnostic(
                    diagnostics,
                    format!(
                        "{}: kept mode-entry {entry_field}, disabled {field}",
                        format_key_combo(combo)
                    ),
                );
                continue;
            }
            if is_unmodified_printable(combo) {
                push_diagnostic(
                    diagnostics,
                    format!(
                        "unsafe shared keybinding: {field} = {raw:?} would intercept typing; use a modified chord; disabling binding"
                    ),
                );
                continue;
            }
            if let Some(first) = shared_combos.get(&combo) {
                push_diagnostic(
                    diagnostics,
                    format!(
                        "{}: kept {first}, disabled {field}",
                        format_key_combo(combo)
                    ),
                );
                continue;
            }
            shared_combos.insert(combo, field.to_string());
        }
    }

    // 3. Per-mode tables: each mode has an independent keyspace (bare printable
    //    keys are allowed). Intra-mode conflicts are first-wins + diagnostic; a
    //    bare key also claimed by a mode-entry key or a shared binding is
    //    shadowed (both win at dispatch) and gets a load-time diagnostic.
    validate_mode_table(
        "pane",
        &pane_mode_fields(&keys.pane),
        &shared_combos,
        &entry_seen,
        diagnostics,
    );
    validate_mode_table(
        "tab",
        &tab_mode_fields(&keys.tab),
        &shared_combos,
        &entry_seen,
        diagnostics,
    );
    validate_mode_table(
        "resize",
        &resize_mode_fields(&keys.resize),
        &shared_combos,
        &entry_seen,
        diagnostics,
    );
    validate_mode_table(
        "move",
        &move_mode_fields(&keys.move_),
        &shared_combos,
        &entry_seen,
        diagnostics,
    );
    validate_mode_table(
        "session",
        &session_mode_fields(&keys.session),
        &shared_combos,
        &entry_seen,
        diagnostics,
    );
    validate_mode_table(
        "tmux",
        &tmux_mode_fields(&keys.tmux),
        &shared_combos,
        &entry_seen,
        diagnostics,
    );

    // 4. default_mode: accept only "modal"/"locked"; otherwise fall back to
    //    modal with a diagnostic.
    let default_mode = match keys.default_mode.trim() {
        "modal" => DefaultMode::Modal,
        "locked" => DefaultMode::Locked,
        other => {
            push_diagnostic(
                diagnostics,
                format!("invalid default_mode {other:?}; falling back to \"modal\""),
            );
            DefaultMode::Modal
        }
    };

    // 5. Locked-mode reachability invariant: refuse to start locked if
    //    mode_locked did not resolve to a usable, distinct key.
    let default_mode = if matches!(default_mode, DefaultMode::Locked) && mode_entry.locked.is_none()
    {
        push_diagnostic(
            diagnostics,
            "default_mode = \"locked\" but keys.mode_locked is unreachable or disabled; falling back to \"modal\" so the session can never be stranded locked".to_string(),
        );
        DefaultMode::Modal
    } else {
        default_mode
    };

    (default_mode, mode_entry)
}

fn push_diagnostic(diagnostics: &mut Vec<String>, diag: String) {
    warn!(message = %diag, "config diagnostic");
    diagnostics.push(diag);
}

fn parse_mode_entry_key(
    field: &'static str,
    raw: &str,
    fallback: KeyCombo,
    seen: &mut std::collections::HashMap<KeyCombo, &'static str>,
    diagnostics: &mut Vec<String>,
) -> Option<KeyCombo> {
    let combo = match parse_key_combo(raw) {
        Some(combo) => combo,
        None => {
            push_diagnostic(
                diagnostics,
                format!("invalid keybinding: {field} = {raw:?}; using fallback"),
            );
            normalize_key_combo(fallback)
        }
    };
    register_mode_entry_combo(field, combo, seen, diagnostics)
}

/// Register an already-resolved mode-entry combo, enforcing mutual distinctness
/// (first declared wins). Returns `None` if the combo duplicates an earlier
/// mode-entry key (so that mode is unreachable by its entry key).
fn register_mode_entry_combo(
    field: &'static str,
    combo: KeyCombo,
    seen: &mut std::collections::HashMap<KeyCombo, &'static str>,
    diagnostics: &mut Vec<String>,
) -> Option<KeyCombo> {
    let combo = normalize_key_combo(combo);
    if let Some(first) = seen.get(&combo) {
        push_diagnostic(
            diagnostics,
            format!(
                "{}: kept mode-entry {first}, disabled {field}",
                format_key_combo(combo)
            ),
        );
        return None;
    }
    seen.insert(combo, field);
    Some(combo)
}

fn validate_mode_table(
    mode_label: &str,
    fields: &[(&str, &BindingConfig)],
    shared_combos: &std::collections::HashMap<KeyCombo, String>,
    entry_combos: &std::collections::HashMap<KeyCombo, &'static str>,
    diagnostics: &mut Vec<String>,
) {
    let mut registry: std::collections::HashMap<KeyCombo, String> =
        std::collections::HashMap::new();
    for (field, config) in fields {
        for raw in config.values() {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            let Some(combo) = parse_key_combo(raw) else {
                push_diagnostic(
                    diagnostics,
                    format!("invalid keybinding: {field} = {raw:?}; disabling binding"),
                );
                continue;
            };
            let combo = normalize_key_combo(combo);
            if let Some(first) = registry.get(&combo) {
                push_diagnostic(
                    diagnostics,
                    format!(
                        "{}: kept {first}, disabled {field}",
                        format_key_combo(combo)
                    ),
                );
                continue;
            }
            // A mode-entry key and a shared binding both win over a per-mode
            // key on the same combo (they are checked first in dispatch), so a
            // per-mode binding that collides with either is shadowed.
            if let Some(entry_field) = entry_combos.get(&combo) {
                push_diagnostic(
                    diagnostics,
                    format!(
                        "{} in {mode_label} mode is shadowed by mode-entry key {entry_field}; the mode-entry key wins",
                        format_key_combo(combo)
                    ),
                );
            } else if let Some(shared_field) = shared_combos.get(&combo) {
                push_diagnostic(
                    diagnostics,
                    format!(
                        "{} in {mode_label} mode is shadowed by shared binding {shared_field}; the shared binding wins",
                        format_key_combo(combo)
                    ),
                );
            }
            registry.insert(combo, (*field).to_string());
        }
    }
}

fn shared_fields(c: &super::model::SharedKeysConfig) -> Vec<(&'static str, &BindingConfig)> {
    vec![
        ("keys.shared.focus_left", &c.focus_left),
        ("keys.shared.focus_down", &c.focus_down),
        ("keys.shared.focus_up", &c.focus_up),
        ("keys.shared.focus_right", &c.focus_right),
        ("keys.shared.new_pane", &c.new_pane),
        ("keys.shared.close_focus", &c.close_focus),
        ("keys.shared.detach", &c.detach),
        ("keys.shared.resize_increase", &c.resize_increase),
        ("keys.shared.resize_decrease", &c.resize_decrease),
        ("keys.shared.move_tab_left", &c.move_tab_left),
        ("keys.shared.move_tab_right", &c.move_tab_right),
        ("keys.shared.new_tab", &c.new_tab),
        ("keys.shared.rename_tab", &c.rename_tab),
        ("keys.shared.toggle_floating", &c.toggle_floating),
    ]
}

fn pane_mode_fields(c: &super::model::PaneModeKeysConfig) -> Vec<(&'static str, &BindingConfig)> {
    vec![
        ("keys.pane.focus_left", &c.focus_left),
        ("keys.pane.focus_down", &c.focus_down),
        ("keys.pane.focus_up", &c.focus_up),
        ("keys.pane.focus_right", &c.focus_right),
        ("keys.pane.new_pane", &c.new_pane),
        ("keys.pane.split_down", &c.split_down),
        ("keys.pane.split_right", &c.split_right),
        ("keys.pane.stack", &c.stack),
        ("keys.pane.close", &c.close),
        ("keys.pane.zoom", &c.zoom),
        ("keys.pane.toggle_float", &c.toggle_float),
        ("keys.pane.rename", &c.rename),
        ("keys.pane.cycle", &c.cycle),
    ]
}

fn tab_mode_fields(c: &super::model::TabModeKeysConfig) -> Vec<(&'static str, &BindingConfig)> {
    vec![
        ("keys.tab.previous", &c.previous),
        ("keys.tab.next", &c.next),
        ("keys.tab.new", &c.new),
        ("keys.tab.close", &c.close),
        ("keys.tab.rename", &c.rename),
        ("keys.tab.break_to_tab", &c.break_to_tab),
        ("keys.tab.toggle", &c.toggle),
    ]
}

fn resize_mode_fields(
    c: &super::model::ResizeModeKeysConfig,
) -> Vec<(&'static str, &BindingConfig)> {
    vec![
        ("keys.resize.increase_left", &c.increase_left),
        ("keys.resize.increase_down", &c.increase_down),
        ("keys.resize.increase_up", &c.increase_up),
        ("keys.resize.increase_right", &c.increase_right),
        ("keys.resize.decrease_left", &c.decrease_left),
        ("keys.resize.decrease_down", &c.decrease_down),
        ("keys.resize.decrease_up", &c.decrease_up),
        ("keys.resize.decrease_right", &c.decrease_right),
        ("keys.resize.increase", &c.increase),
        ("keys.resize.decrease", &c.decrease),
    ]
}

fn move_mode_fields(c: &super::model::MoveModeKeysConfig) -> Vec<(&'static str, &BindingConfig)> {
    vec![
        ("keys.move.move_left", &c.move_left),
        ("keys.move.move_down", &c.move_down),
        ("keys.move.move_up", &c.move_up),
        ("keys.move.move_right", &c.move_right),
        ("keys.move.cycle_forward", &c.cycle_forward),
        ("keys.move.cycle_backward", &c.cycle_backward),
    ]
}

fn session_mode_fields(
    c: &super::model::SessionModeKeysConfig,
) -> Vec<(&'static str, &BindingConfig)> {
    vec![
        ("keys.session.workspace_up", &c.workspace_up),
        ("keys.session.workspace_down", &c.workspace_down),
        ("keys.session.focus_left", &c.focus_left),
        ("keys.session.focus_right", &c.focus_right),
        ("keys.session.cycle", &c.cycle),
        ("keys.session.goto", &c.goto),
        ("keys.session.workspace_picker", &c.workspace_picker),
        ("keys.session.new_workspace", &c.new_workspace),
        ("keys.session.new_worktree", &c.new_worktree),
        ("keys.session.rename_workspace", &c.rename_workspace),
        ("keys.session.close_workspace", &c.close_workspace),
        ("keys.session.settings", &c.settings),
        ("keys.session.help", &c.help),
        ("keys.session.detach", &c.detach),
        ("keys.session.previous_agent", &c.previous_agent),
        ("keys.session.next_agent", &c.next_agent),
    ]
}

fn tmux_mode_fields(c: &super::model::TmuxModeKeysConfig) -> Vec<(&'static str, &BindingConfig)> {
    vec![
        ("keys.tmux.help", &c.help),
        ("keys.tmux.settings", &c.settings),
        ("keys.tmux.new_workspace", &c.new_workspace),
        ("keys.tmux.new_worktree", &c.new_worktree),
        ("keys.tmux.rename_workspace", &c.rename_workspace),
        ("keys.tmux.close_workspace", &c.close_workspace),
        ("keys.tmux.workspace_picker", &c.workspace_picker),
        ("keys.tmux.goto", &c.goto),
        ("keys.tmux.detach", &c.detach),
        ("keys.tmux.reload_config", &c.reload_config),
        (
            "keys.tmux.open_notification_target",
            &c.open_notification_target,
        ),
        ("keys.tmux.new_tab", &c.new_tab),
        ("keys.tmux.rename_tab", &c.rename_tab),
        ("keys.tmux.previous_tab", &c.previous_tab),
        ("keys.tmux.next_tab", &c.next_tab),
        ("keys.tmux.close_tab", &c.close_tab),
        ("keys.tmux.rename_pane", &c.rename_pane),
        ("keys.tmux.edit_scrollback", &c.edit_scrollback),
        ("keys.tmux.copy_mode", &c.copy_mode),
        ("keys.tmux.focus_pane_left", &c.focus_pane_left),
        ("keys.tmux.focus_pane_down", &c.focus_pane_down),
        ("keys.tmux.focus_pane_up", &c.focus_pane_up),
        ("keys.tmux.focus_pane_right", &c.focus_pane_right),
        ("keys.tmux.swap_pane_left", &c.swap_pane_left),
        ("keys.tmux.swap_pane_down", &c.swap_pane_down),
        ("keys.tmux.swap_pane_up", &c.swap_pane_up),
        ("keys.tmux.swap_pane_right", &c.swap_pane_right),
        ("keys.tmux.cycle_pane_next", &c.cycle_pane_next),
        ("keys.tmux.cycle_pane_previous", &c.cycle_pane_previous),
        ("keys.tmux.split_vertical", &c.split_vertical),
        ("keys.tmux.split_horizontal", &c.split_horizontal),
        ("keys.tmux.stack_pane", &c.stack_pane),
        ("keys.tmux.unstack_pane", &c.unstack_pane),
        ("keys.tmux.close_pane", &c.close_pane),
        ("keys.tmux.break_pane_to_tab", &c.break_pane_to_tab),
        ("keys.tmux.zoom", &c.zoom),
        ("keys.tmux.resize_mode", &c.resize_mode),
        ("keys.tmux.toggle_sidebar", &c.toggle_sidebar),
        ("keys.tmux.toggle_floating", &c.toggle_floating),
        ("keys.tmux.new_floating_pane", &c.new_floating_pane),
    ]
}

fn reserve_navigate_runtime_keys(registry: &mut BindingRegistry) {
    for combo in [
        (KeyCode::Esc, KeyModifiers::empty()),
        (KeyCode::Enter, KeyModifiers::empty()),
        (KeyCode::Tab, KeyModifiers::empty()),
        (KeyCode::BackTab, KeyModifiers::empty()),
        (KeyCode::Tab, KeyModifiers::SHIFT),
        (KeyCode::Left, KeyModifiers::empty()),
        (KeyCode::Right, KeyModifiers::empty()),
    ] {
        registry.reserve_direct(combo, "navigate reserved keys");
    }

    for idx in '1'..='9' {
        registry.reserve_direct(
            (KeyCode::Char(idx), KeyModifiers::empty()),
            "navigate reserved keys",
        );
    }
}

fn parse_action_bindings(
    field: &'static str,
    config: &BindingConfig,
    allow_ranges: bool,
    registry: &mut BindingRegistry,
    diagnostics: &mut Vec<String>,
) -> ActionKeybinds {
    parse_action_bindings_owned(field, config, allow_ranges, registry, diagnostics)
}

fn parse_action_bindings_owned(
    field: &str,
    config: &BindingConfig,
    allow_ranges: bool,
    registry: &mut BindingRegistry,
    diagnostics: &mut Vec<String>,
) -> ActionKeybinds {
    let mut bindings = Vec::new();
    for raw in config.values() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        match parse_binding_string(raw) {
            Some(ParsedBinding::Single(binding)) => {
                if reject_binding(field, &binding, registry, diagnostics) {
                    continue;
                }
                registry.register(&binding, field);
                bindings.push(binding);
            }
            Some(ParsedBinding::Range(_)) if !allow_ranges => {
                let diag = format!("range keybinding is only valid for indexed actions: {field} = {raw:?}; disabling binding");
                warn!(message = %diag, "config diagnostic");
                diagnostics.push(diag);
            }
            Some(ParsedBinding::Range(range)) => {
                for binding in range {
                    if reject_binding(field, &binding, registry, diagnostics) {
                        continue;
                    }
                    registry.register(&binding, field);
                    bindings.push(binding);
                }
            }
            None => {
                let diag = format!("invalid keybinding: {field} = {raw:?}; disabling binding");
                warn!(message = %diag, "config diagnostic");
                diagnostics.push(diag);
            }
        }
    }
    ActionKeybinds { bindings }
}

fn parse_navigate_bindings(
    field: &'static str,
    config: &BindingConfig,
    registry: &mut BindingRegistry,
    diagnostics: &mut Vec<String>,
) -> ActionKeybinds {
    let mut bindings = Vec::new();
    for raw in config.values() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        match parse_binding_string(raw) {
            Some(ParsedBinding::Single(binding)) => {
                if reject_navigate_binding(field, &binding, registry, diagnostics) {
                    continue;
                }
                registry.register(&binding, field);
                bindings.push(binding);
            }
            Some(ParsedBinding::Range(_)) => {
                let diag = format!("range keybinding is only valid for indexed actions: {field} = {raw:?}; disabling binding");
                warn!(message = %diag, "config diagnostic");
                diagnostics.push(diag);
            }
            None => {
                let diag = format!("invalid keybinding: {field} = {raw:?}; disabling binding");
                warn!(message = %diag, "config diagnostic");
                diagnostics.push(diag);
            }
        }
    }
    ActionKeybinds { bindings }
}

fn parse_indexed_bindings(
    field: &'static str,
    config: &BindingConfig,
    registry: &mut BindingRegistry,
    diagnostics: &mut Vec<String>,
) -> Vec<IndexedKeybind> {
    parse_action_bindings(field, config, true, registry, diagnostics)
        .bindings
        .into_iter()
        .filter_map(|binding| {
            if matches!(binding.trigger.combo().0, KeyCode::Char('1'..='9')) {
                Some(IndexedKeybind {
                    trigger: binding.trigger,
                    label: binding.label,
                })
            } else {
                let diag = format!(
                    "indexed keybinding must use 1..9: {field} = {:?}; disabling binding",
                    binding.label
                );
                warn!(message = %diag, "config diagnostic");
                diagnostics.push(diag);
                None
            }
        })
        .collect()
}

fn append_legacy_indexed_bindings(
    target: &mut Vec<IndexedKeybind>,
    field: &'static str,
    configured_label: &str,
    registry: &mut BindingRegistry,
    diagnostics: &mut Vec<String>,
) {
    if configured_label.trim().is_empty() {
        return;
    }
    let Some(modifiers) = parse_modifier_combo(configured_label) else {
        let diag = format!(
            "invalid indexed keybinding: {field} = {configured_label:?}; disabling binding"
        );
        warn!(message = %diag, "config diagnostic");
        diagnostics.push(diag);
        return;
    };

    for idx in 1..=9 {
        let combo = (
            KeyCode::Char(char::from_digit(idx, 10).unwrap_or('1')),
            modifiers,
        );
        let binding = ResolvedBinding {
            trigger: BindingTrigger::Direct(combo),
            label: format!("{}+{idx}", configured_label.trim()),
        };
        if reject_binding(field, &binding, registry, diagnostics) {
            continue;
        }
        registry.register(&binding, field);
        target.push(IndexedKeybind {
            trigger: binding.trigger,
            label: binding.label,
        });
    }
}

fn reject_navigate_binding(
    field: &str,
    binding: &ResolvedBinding,
    registry: &BindingRegistry,
    diagnostics: &mut Vec<String>,
) -> bool {
    if binding.trigger.is_prefix() {
        let diag = format!(
            "navigate keybinding must not include prefix: {field} = {:?}; disabling binding",
            binding.label
        );
        warn!(message = %diag, "config diagnostic");
        diagnostics.push(diag);
        return true;
    }

    if matches!(normalize_key_combo(binding.trigger.combo()).0, KeyCode::Esc) {
        let diag = format!(
            "navigate keybinding cannot use esc: {field} = {:?}; disabling binding",
            binding.label
        );
        warn!(message = %diag, "config diagnostic");
        diagnostics.push(diag);
        return true;
    }

    if let Some(first_field) = registry.conflict(binding) {
        let diag = format!("{}: kept {first_field}, disabled {field}", binding.label);
        warn!(message = %diag, "config diagnostic");
        diagnostics.push(diag);
        return true;
    }

    false
}

fn reject_binding(
    field: &str,
    binding: &ResolvedBinding,
    registry: &BindingRegistry,
    diagnostics: &mut Vec<String>,
) -> bool {
    if binding.trigger.is_prefix() && registry.prefix_rhs_is_reserved(binding.trigger.combo()) {
        let diag = format!(
            "reserved keybinding: {field} = {:?} uses keys.prefix as the prefix-mode key; pressing the prefix twice sends a literal prefix key, so this binding is disabled",
            binding.label
        );
        warn!(message = %diag, "config diagnostic");
        diagnostics.push(diag);
        return true;
    }

    if let Some(first_field) = registry.conflict(binding) {
        let diag = format!("{}: kept {first_field}, disabled {field}", binding.label);
        warn!(message = %diag, "config diagnostic");
        diagnostics.push(diag);
        return true;
    }

    if binding.trigger.is_direct() && is_unmodified_printable(binding.trigger.combo()) {
        let suggestion = format!("prefix+{}", binding.label);
        let diag = format!(
            "unsafe direct keybinding: {field} = {:?} would intercept typing; use {:?} to require the prefix; disabling binding",
            binding.label, suggestion
        );
        warn!(message = %diag, "config diagnostic");
        diagnostics.push(diag);
        return true;
    }

    false
}

fn parse_binding_string(raw: &str) -> Option<ParsedBinding> {
    let trimmed = raw.trim();
    let (trigger_prefix, body) = if let Some(rest) = trimmed.strip_prefix("prefix+") {
        (true, rest)
    } else {
        (false, trimmed)
    };

    if let Some(range_modifiers) = parse_range_modifiers(body) {
        let bindings = (1..=9)
            .map(|idx| {
                let combo = (
                    KeyCode::Char(char::from_digit(idx, 10).unwrap_or('1')),
                    range_modifiers,
                );
                let key_label = format_key_combo(combo);
                ResolvedBinding {
                    trigger: if trigger_prefix {
                        BindingTrigger::Prefix(combo)
                    } else {
                        BindingTrigger::Direct(combo)
                    },
                    label: if trigger_prefix {
                        format!("prefix+{key_label}")
                    } else {
                        key_label
                    },
                }
            })
            .collect();
        return Some(ParsedBinding::Range(bindings));
    }

    let combo = parse_key_combo(body)?;
    let label = if trigger_prefix {
        format!("prefix+{}", format_key_combo(combo))
    } else {
        format_key_combo(combo)
    };
    Some(ParsedBinding::Single(ResolvedBinding {
        trigger: if trigger_prefix {
            BindingTrigger::Prefix(combo)
        } else {
            BindingTrigger::Direct(combo)
        },
        label,
    }))
}

pub fn format_key_combo(binding: KeyCombo) -> String {
    let (code, modifiers) = binding;
    let mut parts = Vec::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl".to_string());
    }
    if modifiers.contains(KeyModifiers::ALT) {
        parts.push("alt".to_string());
    }
    if modifiers.contains(KeyModifiers::SHIFT) && !matches!(code, KeyCode::BackTab) {
        parts.push("shift".to_string());
    }
    if modifiers.contains(KeyModifiers::SUPER) {
        parts.push(super_modifier_label().to_string());
    }
    if modifiers.contains(KeyModifiers::HYPER) {
        parts.push("hyper".to_string());
    }
    if modifiers.contains(KeyModifiers::META) {
        parts.push("meta".to_string());
    }

    let key = match code {
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Esc => "esc".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::BackTab => "shift+tab".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::F(n) => format!("f{n}"),
        _ => format!("{:?}", code).to_lowercase(),
    };

    if matches!(code, KeyCode::BackTab) {
        return if parts.is_empty() {
            key
        } else {
            format!("{}+{key}", parts.join("+"))
        };
    }

    parts.push(key);
    parts.join("+")
}

fn super_modifier_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "super"
    }
}

fn parse_modifier_token(token: &str) -> Option<KeyModifiers> {
    match token.to_lowercase().as_str() {
        "ctrl" | "control" => Some(KeyModifiers::CONTROL),
        "shift" => Some(KeyModifiers::SHIFT),
        "alt" | "option" | "meta" => Some(KeyModifiers::ALT),
        "cmd" | "command" | "super" => Some(KeyModifiers::SUPER),
        "hyper" => Some(KeyModifiers::HYPER),
        _ => None,
    }
}

fn parse_range_modifiers(s: &str) -> Option<KeyModifiers> {
    let mut modifiers = KeyModifiers::empty();
    let mut saw_range = false;
    for part in s.split('+') {
        let trimmed = part.trim();
        if trimmed == "1..9" {
            if saw_range {
                return None;
            }
            saw_range = true;
        } else {
            modifiers |= parse_modifier_token(trimmed)?;
        }
    }
    saw_range.then_some(modifiers)
}

fn parse_modifier_combo(s: &str) -> Option<KeyModifiers> {
    let mut modifiers = KeyModifiers::empty();
    let parts: Vec<&str> = s.split('+').collect();
    if parts.is_empty() {
        return None;
    }

    for part in &parts {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            return None;
        }
        modifiers |= parse_modifier_token(trimmed)?;
    }

    if modifiers.is_empty() {
        None
    } else {
        Some(modifiers)
    }
}

pub(crate) fn parse_key_combo(s: &str) -> Option<KeyCombo> {
    let parts: Vec<&str> = s.split('+').collect();
    let mut modifiers = KeyModifiers::empty();
    let mut key_str: Option<&str> = None;

    for part in &parts {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Some(modifier) = parse_modifier_token(trimmed) {
            modifiers |= modifier;
        } else if key_str.is_some() {
            return None;
        } else {
            key_str = Some(trimmed);
        }
    }

    let key_str = key_str?;
    let single_char = single_key_char(key_str);
    let lower = key_str.to_lowercase();
    let code = match lower.as_str() {
        "space" | " " => KeyCode::Char(' '),
        "enter" | "return" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" if modifiers.contains(KeyModifiers::SHIFT) => {
            modifiers.remove(KeyModifiers::SHIFT);
            KeyCode::BackTab
        }
        "tab" => KeyCode::Tab,
        "backspace" | "bs" => KeyCode::Backspace,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "minus" => KeyCode::Char('-'),
        "comma" => KeyCode::Char(','),
        "period" => KeyCode::Char('.'),
        "slash" => KeyCode::Char('/'),
        "backslash" => KeyCode::Char('\\'),
        "quote" => KeyCode::Char('\''),
        "double_quote" | "double-quote" => KeyCode::Char('"'),
        "semicolon" => KeyCode::Char(';'),
        "colon" => KeyCode::Char(':'),
        "percent" => KeyCode::Char('%'),
        "ampersand" => KeyCode::Char('&'),
        "backtick" => KeyCode::Char('`'),
        "plus" => KeyCode::Char('+'),
        _ if single_char.is_some() => {
            let ch = single_char?;
            if ch.is_ascii_uppercase() {
                modifiers |= KeyModifiers::SHIFT;
                KeyCode::Char(ch.to_ascii_lowercase())
            } else {
                KeyCode::Char(ch)
            }
        }
        s if s.starts_with('f') => s[1..].parse::<u8>().ok().map(KeyCode::F)?,
        _ => return None,
    };

    Some(normalize_key_combo((code, modifiers)))
}

fn single_key_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let ch = chars.next()?;
    if chars.next().is_none() {
        Some(ch)
    } else {
        None
    }
}

fn parse_key_combo_with_diagnostic(
    s: &str,
    field: &str,
    fallback: KeyCombo,
) -> (KeyCombo, Option<String>) {
    match parse_key_combo(s) {
        Some(binding) => (binding, None),
        None => {
            let diag = format!("invalid keybinding: {field} = {s:?}; using fallback");
            warn!(message = %diag, "config diagnostic");
            (fallback, Some(diag))
        }
    }
}

pub fn normalize_key_combo((mut code, mut modifiers): KeyCombo) -> KeyCombo {
    if matches!(code, KeyCode::Tab) && modifiers.contains(KeyModifiers::SHIFT) {
        code = KeyCode::BackTab;
        modifiers.remove(KeyModifiers::SHIFT);
    } else if matches!(code, KeyCode::BackTab) {
        modifiers.remove(KeyModifiers::SHIFT);
    }
    (code, modifiers)
}

#[cfg(test)]
pub fn key_event_matches_combo(key: &KeyEvent, combo: KeyCombo) -> bool {
    key_parts_match_combo(key.code, key.modifiers, None, combo)
}

pub fn terminal_key_matches_combo(key: TerminalKey, combo: KeyCombo) -> bool {
    key_parts_match_combo(key.code, key.modifiers, key.shifted_codepoint, combo)
}

fn key_parts_match_combo(
    actual_code: KeyCode,
    actual_modifiers: KeyModifiers,
    shifted_codepoint: Option<u32>,
    combo: KeyCombo,
) -> bool {
    let (actual_code, actual_modifiers) = normalize_key_combo((actual_code, actual_modifiers));
    let (expected_code, expected_modifiers) = normalize_key_combo(combo);

    if actual_modifiers == expected_modifiers
        && key_codes_match(
            actual_code,
            actual_modifiers,
            expected_code,
            expected_modifiers,
            shifted_codepoint,
        )
    {
        return true;
    }

    let actual_without_shift = actual_modifiers.difference(KeyModifiers::SHIFT);
    actual_modifiers.contains(KeyModifiers::SHIFT)
        && actual_without_shift == expected_modifiers
        && shifted_char_matches_expected(actual_code, shifted_codepoint, expected_code)
        || legacy_shifted_ascii_letter_matches(
            actual_code,
            actual_modifiers,
            expected_code,
            expected_modifiers,
        )
}

fn key_codes_match(
    actual: KeyCode,
    actual_modifiers: KeyModifiers,
    expected: KeyCode,
    expected_modifiers: KeyModifiers,
    shifted_codepoint: Option<u32>,
) -> bool {
    match (actual, expected) {
        (KeyCode::Char(actual), KeyCode::Char(expected))
            if actual.is_ascii_alphabetic() && expected.is_ascii_alphabetic() =>
        {
            actual == expected
                || actual_modifiers.contains(KeyModifiers::SHIFT)
                    && expected_modifiers.contains(KeyModifiers::SHIFT)
                    && actual.eq_ignore_ascii_case(&expected)
        }
        (KeyCode::Char(actual), KeyCode::Char(expected)) => {
            actual == expected
                || shifted_char_matches_expected(
                    KeyCode::Char(actual),
                    shifted_codepoint,
                    KeyCode::Char(expected),
                )
        }
        (actual, expected) => actual == expected,
    }
}

fn legacy_shifted_ascii_letter_matches(
    actual_code: KeyCode,
    actual_modifiers: KeyModifiers,
    expected_code: KeyCode,
    expected_modifiers: KeyModifiers,
) -> bool {
    if actual_modifiers.contains(KeyModifiers::SHIFT) {
        return false;
    }
    let (KeyCode::Char(actual), KeyCode::Char(expected)) = (actual_code, expected_code) else {
        return false;
    };
    actual.is_ascii_uppercase()
        && expected.is_ascii_lowercase()
        && actual.to_ascii_lowercase() == expected
        && actual_modifiers | KeyModifiers::SHIFT == expected_modifiers
}

fn shifted_char_matches_expected(
    actual_code: KeyCode,
    shifted_codepoint: Option<u32>,
    expected_code: KeyCode,
) -> bool {
    let KeyCode::Char(expected) = expected_code else {
        return false;
    };
    if shifted_codepoint.and_then(char::from_u32) == Some(expected) {
        return true;
    }
    matches!(actual_code, KeyCode::Char(actual) if actual == expected && is_shifted_punctuation(expected))
}

fn is_shifted_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '!' | '@'
            | '#'
            | '$'
            | '%'
            | '^'
            | '&'
            | '*'
            | '('
            | ')'
            | '_'
            | '+'
            | '{'
            | '}'
            | '|'
            | ':'
            | '"'
            | '<'
            | '>'
            | '?'
            | '~'
    )
}

fn is_unmodified_printable(combo: KeyCombo) -> bool {
    matches!(combo.0, KeyCode::Char(ch) if !ch.is_control())
        && combo.1.difference(KeyModifiers::SHIFT).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, input::TerminalKey};

    fn binding_triggers(bindings: &ActionKeybinds) -> Vec<BindingTrigger> {
        bindings
            .bindings
            .iter()
            .map(|binding| binding.trigger)
            .collect()
    }

    #[test]
    fn parse_simple_char_combo() {
        assert_eq!(
            parse_key_combo("v"),
            Some((KeyCode::Char('v'), KeyModifiers::empty()))
        );
    }

    #[test]
    fn default_stack_keybinds_are_registered_without_conflicts() {
        let config = Config::default();
        let (_prefix_diag, _prefix, diagnostics, keybinds) = config.validated_keybinds();

        // The default stack/unstack bindings must not collide with any existing
        // default binding; a conflict would emit a diagnostic and drop a binding.
        assert!(
            diagnostics.is_empty(),
            "default keybinds produced diagnostics: {diagnostics:?}"
        );
        assert!(!keybinds.stack_pane.bindings.is_empty());
        assert!(!keybinds.unstack_pane.bindings.is_empty());
        // `prefix+s` stays on settings rather than stacking.
        assert!(!keybinds.settings.bindings.is_empty());
    }

    #[test]
    fn parse_unicode_char_combo() {
        assert_eq!(
            parse_key_combo("ö"),
            Some((KeyCode::Char('ö'), KeyModifiers::empty()))
        );
        assert_eq!(
            parse_key_combo("alt+é"),
            Some((KeyCode::Char('é'), KeyModifiers::ALT))
        );
    }

    #[test]
    fn unicode_prefix_config_is_valid() {
        let config: Config = toml::from_str(
            r#"
[keys]
mode_tmux = "ö"
"#,
        )
        .unwrap();
        assert_eq!(
            config.prefix_key(),
            (KeyCode::Char('ö'), KeyModifiers::empty())
        );
        assert!(config.collect_diagnostics().is_empty());
    }

    #[test]
    fn parse_shift_tab_as_backtab() {
        assert_eq!(
            parse_key_combo("shift+tab"),
            Some((KeyCode::BackTab, KeyModifiers::empty()))
        );
    }

    #[test]
    fn parse_named_punctuation() {
        assert_eq!(
            parse_key_combo("minus"),
            Some((KeyCode::Char('-'), KeyModifiers::empty()))
        );
        assert_eq!(
            parse_key_combo("comma"),
            Some((KeyCode::Char(','), KeyModifiers::empty()))
        );
        assert_eq!(
            parse_key_combo("ampersand"),
            Some((KeyCode::Char('&'), KeyModifiers::empty()))
        );
    }

    #[test]
    fn prefix_binding_is_not_direct_binding() {
        let config: Config = toml::from_str(
            r#"
[keys]
next_tab = "prefix+n"
"#,
        )
        .unwrap();
        let kb = config.keybinds();
        assert_eq!(
            binding_triggers(&kb.next_tab),
            vec![BindingTrigger::Prefix((
                KeyCode::Char('n'),
                KeyModifiers::empty()
            ))]
        );
    }

    #[test]
    fn new_worktree_defaults_to_prefix_shift_g() {
        let kb = Config::default().keybinds();
        assert_eq!(
            binding_triggers(&kb.new_worktree),
            vec![BindingTrigger::Prefix((
                KeyCode::Char('g'),
                KeyModifiers::SHIFT
            ))]
        );
    }

    #[test]
    fn goto_defaults_to_prefix_g() {
        let kb = Config::default().keybinds();
        assert_eq!(
            binding_triggers(&kb.goto),
            vec![BindingTrigger::Prefix((
                KeyCode::Char('g'),
                KeyModifiers::empty()
            ))]
        );
    }

    #[test]
    fn open_and_remove_worktree_keybinds_are_unset_by_default() {
        let kb = Config::default().keybinds();
        assert!(kb.open_worktree.bindings.is_empty());
        assert!(kb.remove_worktree.bindings.is_empty());
    }

    #[test]
    fn copy_mode_uses_tmux_prefix_bracket_by_default() {
        let kb = Config::default().keybinds();
        assert_eq!(
            binding_triggers(&kb.copy_mode),
            vec![BindingTrigger::Prefix((
                KeyCode::Char('['),
                KeyModifiers::empty()
            ))]
        );
    }

    #[test]
    fn back_and_forth_keybinds_are_unset_by_default() {
        let kb = Config::default().keybinds();
        assert!(kb.last_pane.bindings.is_empty());
    }

    #[test]
    fn unsafe_direct_printable_binding_is_disabled_with_diagnostic() {
        // The unsafe-printable guard still applies to custom command bindings
        // (a global direct keyspace). Per-mode bare keys are exempt; that is
        // covered by the modal-schema tests.
        let config: Config = toml::from_str(
            r#"
[[keys.command]]
key = "c"
command = "echo no"
"#,
        )
        .unwrap();
        let diagnostics = config.collect_diagnostics();
        let keybinds = config.keybinds();
        assert!(keybinds.custom_commands.is_empty());
        assert!(diagnostics
            .iter()
            .any(|diag| diag.contains("unsafe direct keybinding")
                && diag.contains("keys.command[0].key")));
    }

    #[test]
    fn shifted_letter_binding_matches_uppercase_key_event() {
        let bindings = ActionKeybinds::prefix("shift+n");
        assert!(bindings.matches_prefix(&KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT)));
    }

    #[test]
    fn shifted_letter_binding_matches_legacy_uppercase_key_event() {
        let bindings = ActionKeybinds::prefix("shift+n");
        assert!(bindings
            .matches_prefix_key(TerminalKey::new(KeyCode::Char('N'), KeyModifiers::empty(),)));
    }

    #[test]
    fn shifted_letter_direct_binding_matches_legacy_uppercase_key_event() {
        let bindings = ActionKeybinds::direct("shift+n");
        assert!(bindings
            .matches_direct_key(TerminalKey::new(KeyCode::Char('N'), KeyModifiers::empty(),)));
    }

    #[test]
    fn shifted_letter_binding_matches_modern_modified_key_event() {
        let bindings = ActionKeybinds::direct("cmd+shift+j");
        assert!(bindings.matches_direct_key(TerminalKey::new(
            KeyCode::Char('J'),
            KeyModifiers::SUPER | KeyModifiers::SHIFT,
        )));
    }

    #[test]
    fn legacy_uppercase_key_event_does_not_match_unshifted_letter_binding() {
        let bindings = ActionKeybinds::prefix("n");
        assert!(!bindings
            .matches_prefix_key(TerminalKey::new(KeyCode::Char('N'), KeyModifiers::empty(),)));
    }

    #[test]
    fn legacy_uppercase_shift_fallback_is_limited_to_ascii_letters() {
        let shifted_number = ActionKeybinds::prefix("shift+1");
        assert!(!shifted_number
            .matches_prefix_key(TerminalKey::new(KeyCode::Char('!'), KeyModifiers::empty(),)));

        let shifted_non_ascii = ActionKeybinds::prefix("shift+ö");
        assert!(!shifted_non_ascii
            .matches_prefix_key(TerminalKey::new(KeyCode::Char('Ö'), KeyModifiers::empty(),)));
    }

    #[test]
    fn shifted_tab_inputs_match_backtab_canonical_binding() {
        let bindings = ActionKeybinds::prefix("shift+tab");
        assert!(
            bindings.matches_prefix_key(TerminalKey::new(KeyCode::BackTab, KeyModifiers::empty()))
        );
        assert!(
            bindings.matches_prefix_key(TerminalKey::new(KeyCode::BackTab, KeyModifiers::SHIFT))
        );
        assert!(bindings.matches_prefix_key(TerminalKey::new(KeyCode::Tab, KeyModifiers::SHIFT)));
        assert!(!ActionKeybinds::prefix("tab")
            .matches_prefix_key(TerminalKey::new(KeyCode::Tab, KeyModifiers::SHIFT)));
        assert_eq!(
            normalize_key_combo((KeyCode::Tab, KeyModifiers::CONTROL | KeyModifiers::SHIFT)),
            (KeyCode::BackTab, KeyModifiers::CONTROL)
        );
    }

    #[test]
    fn format_modified_backtab_keeps_shift_label() {
        assert_eq!(
            format_key_combo((KeyCode::BackTab, KeyModifiers::CONTROL)),
            "ctrl+shift+tab"
        );
        assert_eq!(
            format_key_combo((KeyCode::BackTab, KeyModifiers::CONTROL | KeyModifiers::ALT)),
            "ctrl+alt+shift+tab"
        );
    }

    #[test]
    fn shifted_punctuation_matches_enhanced_input() {
        let help = ActionKeybinds::prefix("?");
        assert!(help.matches_prefix_key(TerminalKey::new(KeyCode::Char('?'), KeyModifiers::SHIFT)));
        assert!(help.matches_prefix_key(
            TerminalKey::new(KeyCode::Char('/'), KeyModifiers::SHIFT)
                .with_shifted_codepoint('?' as u32)
        ));

        let bang = ActionKeybinds::prefix("!");
        assert!(bang.matches_prefix_key(
            TerminalKey::new(KeyCode::Char('1'), KeyModifiers::SHIFT)
                .with_shifted_codepoint('!' as u32)
        ));
    }

    #[test]
    fn navigate_bindings_default_to_arrow_and_hjkl() {
        // Navigate movement reads the released defaults: up/down come from
        // arrow keys, h/j/k/l resolve pane movement.
        let keybinds = Config::default().keybinds();
        let diagnostics = Config::default().collect_diagnostics();

        assert!(keybinds
            .navigate
            .workspace_up
            .matches_direct_key(TerminalKey::new(KeyCode::Up, KeyModifiers::empty())));
        assert!(keybinds
            .navigate
            .workspace_down
            .matches_direct_key(TerminalKey::new(KeyCode::Down, KeyModifiers::empty())));
        assert!(keybinds
            .navigate
            .pane_down
            .matches_direct_key(TerminalKey::new(KeyCode::Char('j'), KeyModifiers::empty())));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn custom_command_prefix_rhs_equal_to_configured_prefix_is_rejected() {
        let config: Config = toml::from_str(
            r#"
[keys]
prefix = "ctrl+b"

[[keys.command]]
key = "prefix+ctrl+b"
command = "echo no"
"#,
        )
        .unwrap();
        let diagnostics = config.collect_diagnostics();
        assert!(config.keybinds().custom_commands.is_empty());
        assert!(diagnostics.iter().any(|diag| {
            diag.contains("reserved keybinding") && diag.contains("keys.command[0].key")
        }));
    }

    #[test]
    fn direct_custom_printable_binding_is_rejected_as_unsafe() {
        let config: Config = toml::from_str(
            r#"
[keys]

[[keys.command]]
key = "g"
command = "echo no"
"#,
        )
        .unwrap();
        let diagnostics = config.collect_diagnostics();
        assert!(config.keybinds().custom_commands.is_empty());
        assert!(diagnostics.iter().any(|diag| {
            diag.contains("unsafe direct keybinding") && diag.contains("keys.command[0].key")
        }));
    }

    #[test]
    fn direct_custom_binding_conflicting_with_builtin_is_disabled() {
        // `split_auto` is a released direct binding on alt+n; a custom command
        // claiming the same combo is rejected (first-wins: the builtin keeps it).
        let config: Config = toml::from_str(
            r#"
[[keys.command]]
key = "alt+n"
command = "echo no"
"#,
        )
        .unwrap();
        let diagnostics = config.collect_diagnostics();
        let keybinds = config.keybinds();
        assert!(!keybinds.split_auto.bindings.is_empty());
        assert!(keybinds.custom_commands.is_empty());
        assert!(diagnostics.iter().any(|diag| {
            diag.contains("kept keys.split_auto") && diag.contains("disabled keys.command[0].key")
        }));
    }

    #[test]
    fn indexed_workspace_bindings_support_modifiers() {
        // The retained `[keys.indexed]` mechanism expands a modifier combo over
        // number keys 1-9.
        let config: Config = toml::from_str(
            r#"
[keys.indexed]
workspaces = "ctrl+alt"
"#,
        )
        .unwrap();
        let kb = config.keybinds();
        assert_eq!(kb.switch_workspace.len(), 9);
        assert_eq!(
            kb.switch_workspace[0].trigger,
            BindingTrigger::Direct((
                KeyCode::Char('1'),
                KeyModifiers::CONTROL | KeyModifiers::ALT
            ))
        );
        assert_eq!(kb.switch_workspace[0].label, "ctrl+alt+1");
    }

    #[test]
    fn default_keymap_is_prefix_first_and_tab_centered() {
        let kb = Config::default().keybinds();
        assert_eq!(
            binding_triggers(&kb.next_tab),
            vec![BindingTrigger::Prefix((
                KeyCode::Char('n'),
                KeyModifiers::empty()
            ))]
        );
        assert_eq!(
            binding_triggers(&kb.previous_tab),
            vec![BindingTrigger::Prefix((
                KeyCode::Char('p'),
                KeyModifiers::empty()
            ))]
        );
        assert_eq!(kb.switch_tab.len(), 9);
        assert!(kb
            .switch_tab
            .iter()
            .all(|binding| binding.trigger.is_prefix()));
        assert!(kb
            .new_tab
            .bindings
            .iter()
            .all(|binding| binding.trigger.is_prefix()));
        assert_eq!(
            binding_triggers(&kb.swap_pane_left),
            vec![BindingTrigger::Prefix((
                KeyCode::Char('h'),
                KeyModifiers::SHIFT
            ))]
        );
        assert_eq!(
            binding_triggers(&kb.swap_pane_down),
            vec![BindingTrigger::Prefix((
                KeyCode::Char('j'),
                KeyModifiers::SHIFT
            ))]
        );
        assert_eq!(
            binding_triggers(&kb.swap_pane_up),
            vec![BindingTrigger::Prefix((
                KeyCode::Char('k'),
                KeyModifiers::SHIFT
            ))]
        );
        assert_eq!(
            binding_triggers(&kb.swap_pane_right),
            vec![BindingTrigger::Prefix((
                KeyCode::Char('l'),
                KeyModifiers::SHIFT
            ))]
        );
    }

    #[test]
    fn custom_command_with_description_parses() {
        let config: Config = toml::from_str(
            r#"
[[keys.command]]
key = "prefix+y"
command = "echo hello"
description = "say hello"
"#,
        )
        .unwrap();
        let keybinds = config.keybinds();
        assert_eq!(keybinds.custom_commands.len(), 1);
        assert_eq!(
            keybinds.custom_commands[0].description,
            Some("say hello".to_string())
        );
    }

    #[test]
    fn default_config_binds_alt_keys() {
        let config = Config::default();
        let kb = config.keybinds();
        let triggers = binding_triggers(&kb.focus_pane_left);
        assert!(triggers.contains(&BindingTrigger::Prefix((
            KeyCode::Char('h'),
            KeyModifiers::empty()
        ))));
        assert!(triggers.contains(&BindingTrigger::Direct((
            KeyCode::Char('h'),
            KeyModifiers::ALT
        ))));
    }

    #[test]
    fn resize_mode_unchanged() {
        let config = Config::default();
        let kb = config.keybinds();
        let triggers = binding_triggers(&kb.resize_mode);
        assert_eq!(
            triggers,
            vec![BindingTrigger::Prefix((
                KeyCode::Char('r'),
                KeyModifiers::empty()
            ))]
        );
    }

    #[test]
    fn split_auto_default_binds_alt_n() {
        let config = Config::default();
        let kb = config.keybinds();
        let triggers = binding_triggers(&kb.split_auto);
        assert_eq!(
            triggers,
            vec![BindingTrigger::Direct((
                KeyCode::Char('n'),
                KeyModifiers::ALT
            ))]
        );
    }

    #[test]
    fn move_tab_defaults_bind_alt_i_o() {
        let config = Config::default();
        let kb = config.keybinds();
        assert_eq!(
            binding_triggers(&kb.move_tab_left),
            vec![BindingTrigger::Direct((
                KeyCode::Char('i'),
                KeyModifiers::ALT
            ))]
        );
        assert_eq!(
            binding_triggers(&kb.move_tab_right),
            vec![BindingTrigger::Direct((
                KeyCode::Char('o'),
                KeyModifiers::ALT
            ))]
        );
    }

    #[test]
    fn resize_grow_shrink_bind_alt_eq_minus() {
        let config = Config::default();
        let kb = config.keybinds();
        assert_eq!(
            binding_triggers(&kb.resize_grow),
            vec![BindingTrigger::Direct((
                KeyCode::Char('='),
                KeyModifiers::ALT
            ))]
        );
        assert_eq!(
            binding_triggers(&kb.resize_shrink),
            vec![BindingTrigger::Direct((
                KeyCode::Char('-'),
                KeyModifiers::ALT
            ))]
        );
    }

    #[test]
    fn keysconfig_toml_roundtrip_lossless() {
        use crate::config::model::KeysConfig;
        let original = KeysConfig::default();
        let toml_str = toml::to_string(&original).unwrap();
        let restored: KeysConfig = toml::from_str(&toml_str).unwrap();
        let toml_str2 = toml::to_string(&restored).unwrap();
        assert_eq!(toml_str, toml_str2);
    }

    #[test]
    fn default_config_has_no_conflict_diagnostics() {
        let config = Config::default();
        let diagnostics = config.collect_diagnostics();
        assert!(
            diagnostics.is_empty(),
            "Default config produced diagnostics: {diagnostics:?}"
        );
    }

    #[test]
    fn parse_alt_symbol_keys() {
        assert_eq!(
            parse_key_combo("alt+="),
            Some((KeyCode::Char('='), KeyModifiers::ALT))
        );
        assert_eq!(
            parse_key_combo("alt+-"),
            Some((KeyCode::Char('-'), KeyModifiers::ALT))
        );
    }

    // ---- Modal schema ----

    #[test]
    fn modal_overrides_do_not_change_dispatched_keybinds() {
        // The modal tables are validated but not yet dispatched: runtime
        // dispatch reads the released keymap regardless of modal config. This
        // locks that invariant — heavy modal overrides must not move any
        // dispatched binding. (Guards the eventual dispatch-rewire commit.)
        let default_kb = Config::default().keybinds();
        let overridden: Config = toml::from_str(
            r#"
[keys]
default_mode = "locked"
mode_pane = "ctrl+y"
mode_tab = "ctrl+e"

[keys.shared]
focus_left = "alt+a"
new_pane = "alt+b"

[keys.pane]
close = "q"
new_pane = "g"

[keys.tmux]
new_tab = "z"
close_pane = "Q"
"#,
        )
        .unwrap();
        let over_kb = overridden.keybinds();

        // A representative slice of dispatched actions across surfaces.
        assert_eq!(default_kb.new_tab.labels(), over_kb.new_tab.labels());
        assert_eq!(default_kb.close_pane.labels(), over_kb.close_pane.labels());
        assert_eq!(default_kb.split_auto.labels(), over_kb.split_auto.labels());
        assert_eq!(
            default_kb.focus_pane_left.labels(),
            over_kb.focus_pane_left.labels()
        );
        assert_eq!(
            default_kb.resize_mode.labels(),
            over_kb.resize_mode.labels()
        );
        assert_eq!(
            default_kb.navigate.workspace_up.labels(),
            over_kb.navigate.workspace_up.labels()
        );
        // But the resolved modal fields DO reflect the overrides.
        assert_eq!(
            over_kb.mode_entry.pane,
            Some((KeyCode::Char('y'), KeyModifiers::CONTROL))
        );
        assert_eq!(over_kb.default_mode, DefaultMode::Locked);
    }

    #[test]
    fn modal_schema_parses_all_sections() {
        let config: Config = toml::from_str(
            r#"
[keys]
default_mode = "modal"
mode_pane = "ctrl+y"

[keys.shared]
focus_left = "alt+h"

[keys.pane]
focus_left = "h"

[keys.tab]
new = "n"

[keys.resize]
increase = "="

[keys.move]
move_left = "h"

[keys.session]
goto = "g"

[keys.tmux]
new_tab = "c"
"#,
        )
        .unwrap();
        assert_eq!(config.keys.default_mode, "modal");
        assert_eq!(config.keys.mode_pane, "ctrl+y");
        assert_eq!(config.keys.pane.focus_left, BindingConfig::one("h"));
        assert!(config.collect_diagnostics().is_empty());
    }

    #[test]
    fn mode_entry_defaults_match_zellij_keymap() {
        let kb = Config::default().keybinds();
        assert_eq!(
            kb.mode_entry.pane,
            Some((KeyCode::Char('p'), KeyModifiers::CONTROL))
        );
        assert_eq!(
            kb.mode_entry.tab,
            Some((KeyCode::Char('t'), KeyModifiers::CONTROL))
        );
        assert_eq!(
            kb.mode_entry.resize,
            Some((KeyCode::Char('n'), KeyModifiers::CONTROL))
        );
        assert_eq!(
            kb.mode_entry.move_,
            Some((KeyCode::Char('h'), KeyModifiers::CONTROL))
        );
        assert_eq!(
            kb.mode_entry.session,
            Some((KeyCode::Char('o'), KeyModifiers::CONTROL))
        );
        assert_eq!(
            kb.mode_entry.locked,
            Some((KeyCode::Char('g'), KeyModifiers::CONTROL))
        );
        assert_eq!(
            kb.mode_entry.tmux,
            Some((KeyCode::Char('b'), KeyModifiers::CONTROL))
        );
        assert_eq!(kb.default_mode, DefaultMode::Modal);
    }

    #[test]
    fn per_mode_defaults_match_zellij_keymap() {
        let keys = crate::config::model::KeysConfig::default();
        // Shared: alt chords.
        assert_eq!(keys.shared.focus_left, BindingConfig::one("alt+h"));
        assert_eq!(keys.shared.new_pane, BindingConfig::one("alt+n"));
        assert_eq!(keys.shared.detach, BindingConfig::one("ctrl+q"));
        // Pane: bare keys, h/Left alias.
        assert_eq!(
            keys.pane.focus_left,
            BindingConfig::Many(vec!["h".into(), "left".into()])
        );
        assert_eq!(keys.pane.stack, BindingConfig::one("s"));
        // Session.
        assert_eq!(keys.session.new_worktree, BindingConfig::one("N"));
        assert_eq!(keys.session.previous_agent, BindingConfig::one("["));
    }

    #[test]
    fn every_per_mode_struct_field_is_validated() {
        // Each `*_mode_fields` helper is hand-maintained in lockstep with its
        // struct. A field missing from its helper would silently skip conflict
        // and unsafe-printable validation, so assert full coverage: every TOML
        // key the struct serializes appears as `keys.<mode>.<key>` in the
        // helper's field list.
        use crate::config::model::KeysConfig;
        let keys = KeysConfig::default();

        fn struct_keys<T: serde::Serialize>(section: &T) -> Vec<String> {
            let value = toml::Value::try_from(section).expect("serialize mode section");
            value
                .as_table()
                .expect("mode section is a table")
                .keys()
                .cloned()
                .collect()
        }

        fn helper_keys(mode: &str, fields: &[(&str, &BindingConfig)]) -> Vec<String> {
            let prefix = format!("keys.{mode}.");
            fields
                .iter()
                .map(|(field, _)| {
                    field
                        .strip_prefix(&prefix)
                        .unwrap_or_else(|| panic!("{field} is not under {prefix}"))
                        .to_string()
                })
                .collect()
        }

        let cases: [(&str, Vec<String>, Vec<String>); 7] = [
            (
                "shared",
                struct_keys(&keys.shared),
                helper_keys("shared", &shared_fields(&keys.shared)),
            ),
            (
                "pane",
                struct_keys(&keys.pane),
                helper_keys("pane", &pane_mode_fields(&keys.pane)),
            ),
            (
                "tab",
                struct_keys(&keys.tab),
                helper_keys("tab", &tab_mode_fields(&keys.tab)),
            ),
            (
                "resize",
                struct_keys(&keys.resize),
                helper_keys("resize", &resize_mode_fields(&keys.resize)),
            ),
            (
                "move",
                struct_keys(&keys.move_),
                helper_keys("move", &move_mode_fields(&keys.move_)),
            ),
            (
                "session",
                struct_keys(&keys.session),
                helper_keys("session", &session_mode_fields(&keys.session)),
            ),
            (
                "tmux",
                struct_keys(&keys.tmux),
                helper_keys("tmux", &tmux_mode_fields(&keys.tmux)),
            ),
        ];

        for (mode, mut struct_fields, mut helper_fields) in cases {
            struct_fields.sort();
            helper_fields.sort();
            assert_eq!(
                struct_fields, helper_fields,
                "{mode}_mode_fields() is out of sync with its config struct"
            );
        }
    }

    #[test]
    fn resize_increase_uses_plus_alias_never_double_plus() {
        let keys = crate::config::model::KeysConfig::default();
        // The default uses the "plus" alias plus "=", never the unparseable "+".
        assert_eq!(
            keys.resize.increase,
            BindingConfig::Many(vec!["plus".into(), "=".into()])
        );
        assert_eq!(
            parse_key_combo("plus"),
            Some((KeyCode::Char('+'), KeyModifiers::empty()))
        );
        // Defaults must not produce diagnostics (a literal "+" would).
        assert!(Config::default().collect_diagnostics().is_empty());
    }

    #[test]
    fn per_mode_bare_printable_keys_accepted() {
        // Bare printable keys are the whole point inside a mode; they must NOT
        // trip the unsafe-printable guard that applies to shared/direct binds.
        let config: Config = toml::from_str(
            r#"
[keys.pane]
close = "q"
new_pane = "g"
"#,
        )
        .unwrap();
        assert!(config.collect_diagnostics().is_empty());
    }

    #[test]
    fn shared_bare_printable_key_rejected() {
        let config: Config = toml::from_str(
            r#"
[keys.shared]
new_pane = "g"
"#,
        )
        .unwrap();
        let diagnostics = config.collect_diagnostics();
        assert!(diagnostics.iter().any(|d| {
            d.contains("unsafe shared keybinding") && d.contains("keys.shared.new_pane")
        }));
    }

    #[test]
    fn intra_mode_conflict_is_first_wins_with_diagnostic() {
        let config: Config = toml::from_str(
            r#"
[keys.pane]
focus_left = "h"
new_pane = "h"
"#,
        )
        .unwrap();
        let diagnostics = config.collect_diagnostics();
        assert!(diagnostics.iter().any(|d| {
            d.contains("kept keys.pane.focus_left") && d.contains("disabled keys.pane.new_pane")
        }));
    }

    #[test]
    fn each_mode_has_independent_keyspace() {
        // The same bare key in two different modes is not a conflict.
        let config: Config = toml::from_str(
            r#"
[keys.pane]
close = "x"

[keys.tab]
close = "x"
"#,
        )
        .unwrap();
        assert!(config.collect_diagnostics().is_empty());
    }

    #[test]
    fn mode_entry_keys_must_be_distinct() {
        let config: Config = toml::from_str(
            r#"
[keys]
mode_pane = "ctrl+p"
mode_tab = "ctrl+p"
"#,
        )
        .unwrap();
        let diagnostics = config.collect_diagnostics();
        let kb = config.keybinds();
        assert!(diagnostics.iter().any(|d| {
            d.contains("kept mode-entry keys.mode_pane") && d.contains("disabled keys.mode_tab")
        }));
        // The duplicate is disabled (mode unreachable by its entry key).
        assert_eq!(kb.mode_entry.tab, None);
    }

    #[test]
    fn mode_entry_beats_shared_binding() {
        // A shared binding colliding with a mode-entry key is dropped.
        let config: Config = toml::from_str(
            r#"
[keys]
mode_pane = "ctrl+p"

[keys.shared]
new_pane = "ctrl+p"
"#,
        )
        .unwrap();
        let diagnostics = config.collect_diagnostics();
        assert!(diagnostics.iter().any(|d| {
            d.contains("kept mode-entry keys.mode_pane")
                && d.contains("disabled keys.shared.new_pane")
        }));
    }

    #[test]
    fn shared_shadows_per_mode_bare_key_with_diagnostic() {
        // A shared chord that also appears as a per-mode binding shadows the
        // per-mode binding (shared wins at dispatch); emit a load diagnostic.
        let config: Config = toml::from_str(
            r#"
[keys.shared]
new_pane = "ctrl+e"

[keys.pane]
rename = "ctrl+e"
"#,
        )
        .unwrap();
        let diagnostics = config.collect_diagnostics();
        assert!(diagnostics
            .iter()
            .any(|d| { d.contains("shadowed by shared binding") && d.contains("pane mode") }));
    }

    #[test]
    fn mode_entry_shadows_per_mode_bare_key_with_diagnostic() {
        // A per-mode binding on a mode-entry combo is shadowed (the mode-entry
        // key is checked first in dispatch); emit a load diagnostic.
        let config: Config = toml::from_str(
            r#"
[keys]
mode_pane = "ctrl+e"

[keys.tab]
new = "ctrl+e"
"#,
        )
        .unwrap();
        let diagnostics = config.collect_diagnostics();
        assert!(diagnostics.iter().any(|d| {
            d.contains("shadowed by mode-entry key keys.mode_pane") && d.contains("tab mode")
        }));
    }

    #[test]
    fn default_mode_invalid_falls_back_to_modal() {
        let config: Config = toml::from_str(
            r#"
[keys]
default_mode = "weird"
"#,
        )
        .unwrap();
        let kb = config.keybinds();
        let diagnostics = config.collect_diagnostics();
        assert_eq!(kb.default_mode, DefaultMode::Modal);
        assert!(diagnostics
            .iter()
            .any(|d| d.contains("invalid default_mode") && d.contains("modal")));
    }

    #[test]
    fn default_mode_locked_is_accepted_when_unlock_key_reachable() {
        let config: Config = toml::from_str(
            r#"
[keys]
default_mode = "locked"
"#,
        )
        .unwrap();
        let kb = config.keybinds();
        assert_eq!(kb.default_mode, DefaultMode::Locked);
        assert!(config.collect_diagnostics().is_empty());
    }

    #[test]
    fn default_mode_locked_with_unreachable_unlock_falls_back_to_modal() {
        // mode_locked duplicates mode_pane, so it is disabled — locking would
        // strand the session. Refuse to start locked.
        let config: Config = toml::from_str(
            r#"
[keys]
default_mode = "locked"
mode_pane = "ctrl+g"
mode_locked = "ctrl+g"
"#,
        )
        .unwrap();
        let kb = config.keybinds();
        let diagnostics = config.collect_diagnostics();
        assert_eq!(kb.mode_entry.locked, None);
        assert_eq!(kb.default_mode, DefaultMode::Modal);
        assert!(diagnostics
            .iter()
            .any(|d| d.contains("default_mode = \"locked\"") && d.contains("falling back")));
    }

    #[test]
    fn modal_keysconfig_toml_roundtrip_lossless_with_overrides() {
        use crate::config::model::KeysConfig;
        let original: KeysConfig = toml::from_str(
            r#"
default_mode = "locked"
mode_pane = "ctrl+y"

[shared]
focus_left = "alt+a"

[pane]
close = "q"
"#,
        )
        .unwrap();
        let toml_str = toml::to_string(&original).unwrap();
        let restored: KeysConfig = toml::from_str(&toml_str).unwrap();
        let toml_str2 = toml::to_string(&restored).unwrap();
        assert_eq!(toml_str, toml_str2);
    }
}
