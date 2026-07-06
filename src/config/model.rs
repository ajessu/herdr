use std::num::NonZeroUsize;

use crossterm::event::KeyModifiers;
use serde::{de, Deserialize, Deserializer, Serialize};

use super::{
    BindingConfig, CommandKeybindConfig, SoundConfig, ThemeConfig, DEFAULT_MOBILE_WIDTH_THRESHOLD,
    DEFAULT_MOUSE_SCROLL_LINES, DEFAULT_SCROLLBACK_LIMIT_BYTES,
};

pub const MAX_TOAST_DELAY_SECONDS: u64 = 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannelConfig {
    #[default]
    Stable,
    Preview,
}

impl UpdateChannelConfig {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Preview => "preview",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    #[serde(default = "default_update_channel")]
    pub channel: UpdateChannelConfig,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            channel: default_update_channel(),
        }
    }
}

fn default_update_channel() -> UpdateChannelConfig {
    if cfg!(windows) {
        UpdateChannelConfig::Preview
    } else {
        UpdateChannelConfig::Stable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ToastDelivery {
    #[default]
    Off,
    Herdr,
    Terminal,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ToastHerdrPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ToastClipboardPosition {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    #[default]
    BottomCenter,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentPanelSortConfig {
    #[default]
    #[serde(alias = "workspaces")]
    Spaces,
    Priority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TabStatusMode {
    #[default]
    Off,
    Attention,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HintBarStyle {
    #[default]
    Full,
    Compact,
    Off,
}

impl AgentPanelSortConfig {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Spaces => "spaces",
            Self::Priority => "priority",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RightClickPassthroughModifierConfig(Option<KeyModifiers>);

impl RightClickPassthroughModifierConfig {
    pub fn modifiers(self) -> Option<KeyModifiers> {
        self.0
    }
}

impl<'de> Deserialize<'de> for RightClickPassthroughModifierConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        parse_right_click_passthrough_modifier(&value)
            .map(Self)
            .ok_or_else(|| {
                de::Error::custom(
                    "right_click_passthrough_modifier must be empty, off, none, disabled, ctrl/control, alt/option, cmd/command/super, meta, hyper, or a + separated combination without shift",
                )
            })
    }
}

fn parse_right_click_passthrough_modifier(value: &str) -> Option<Option<KeyModifiers>> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("off")
        || trimmed.eq_ignore_ascii_case("none")
        || trimmed.eq_ignore_ascii_case("disabled")
    {
        return Some(None);
    }

    let mut modifiers = KeyModifiers::empty();
    for token in trimmed.split('+') {
        let token = token.trim().to_ascii_lowercase();
        let modifier = match token.as_str() {
            "ctrl" | "control" => KeyModifiers::CONTROL,
            "alt" | "option" => KeyModifiers::ALT,
            "cmd" | "command" | "super" => KeyModifiers::SUPER,
            "meta" => KeyModifiers::META,
            "hyper" => KeyModifiers::HYPER,
            "shift" => return None,
            _ => return None,
        };
        modifiers |= modifier;
    }

    (!modifiers.is_empty()).then_some(Some(modifiers))
}

#[derive(Debug, Clone)]
pub struct ToastConfig {
    pub delivery: ToastDelivery,
    pub delay_seconds: u64,
    pub herdr: HerdrToastConfig,
    pub clipboard: ClipboardToastConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct HerdrToastConfig {
    pub position: ToastHerdrPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct ClipboardToastConfig {
    pub enabled: bool,
    pub position: ToastClipboardPosition,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum NewTerminalCwdConfig {
    #[default]
    Follow,
    Home,
    Current,
    Path(String),
}

impl<'de> Deserialize<'de> for NewTerminalCwdConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.trim() {
            "" | "follow" => Ok(Self::Follow),
            "home" => Ok(Self::Home),
            "current" => Ok(Self::Current),
            _ => Ok(Self::Path(value)),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellModeConfig {
    #[default]
    Auto,
    Login,
    NonLogin,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    /// Executable used for new interactive panes. Empty means SHELL, then /bin/sh.
    pub default_shell: String,
    /// Startup mode for new interactive pane shells.
    pub shell_mode: ShellModeConfig,
    /// CWD policy for new interactive panes, tabs, and workspaces.
    pub new_cwd: NewTerminalCwdConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    /// Resume supported AI-agent panes into their native conversation sessions
    /// when restoring a Herdr session. Default: true.
    pub resume_agents_on_restore: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            resume_agents_on_restore: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigReloadStatus {
    Applied,
    Partial,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ConfigReloadReport {
    pub status: ConfigReloadStatus,
    pub diagnostics: Vec<String>,
}

/// Validate `[ui]` sidebar bound configuration.
///
/// Returns `Some((min, max))` when `min <= max`, `None` otherwise. The two
/// values are funneled through this helper before they reach any
/// `u16::clamp(min, max)` call site (`u16::clamp` panics when `min > max`).
pub fn validated_sidebar_bounds(min: u16, max: u16) -> Option<(u16, u16)> {
    if min <= max {
        Some((min, max))
    } else {
        None
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub onboarding: Option<bool>,
    pub theme: ThemeConfig,
    pub terminal: TerminalConfig,
    pub session: SessionConfig,
    pub update: UpdateConfig,
    pub keys: KeysConfig,
    pub ui: UiConfig,
    pub worktrees: WorktreesConfig,
    pub advanced: AdvancedConfig,
    pub experimental: ExperimentalConfig,
    pub remote: RemoteConfig,
}

#[derive(Debug)]
pub struct LoadedConfig {
    pub config: Config,
    pub diagnostics: Vec<String>,
    pub invalid_sections: Vec<String>,
}

/// Mode-structured keybinding configuration (zellij-compatible modal model).
///
/// The legacy flat per-action `[keys]` schema (`prefix`, `new_tab`,
/// `focus_pane_left`, …) is replaced by `default_mode`, the seven mode-entry
/// keys, and the per-mode binding tables. Auxiliary features that are not part
/// of the modal redesign — indexed shortcuts and custom commands — are retained.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct KeysConfig {
    /// Base interaction mode: "modal" (default, zellij Ctrl+letter modes) or
    /// "locked" (start locked; pair with `mode_tmux` for prefix-style use).
    pub default_mode: String,
    /// Enter Pane mode. Default: "ctrl+p".
    pub mode_pane: String,
    /// Enter Tab mode. Default: "ctrl+t".
    pub mode_tab: String,
    /// Enter Resize mode. Default: "ctrl+n".
    pub mode_resize: String,
    /// Enter Move mode. Default: "ctrl+h".
    pub mode_move: String,
    /// Enter Session mode. Default: "ctrl+o".
    pub mode_session: String,
    /// Enter Locked mode. Default: "ctrl+g".
    pub mode_locked: String,
    /// Enter Tmux/Prefix mode (one-shot). Also the prefix key. Default: "ctrl+b".
    pub mode_tmux: String,
    /// Bindings active in every non-locked mode (`shared_except "locked"`).
    pub shared: SharedKeysConfig,
    /// Bindings active within Pane mode.
    pub pane: PaneModeKeysConfig,
    /// Bindings active within Tab mode.
    pub tab: TabModeKeysConfig,
    /// Bindings active within Resize mode.
    pub resize: ResizeModeKeysConfig,
    /// Bindings active within Move mode.
    #[serde(rename = "move")]
    pub move_: MoveModeKeysConfig,
    /// Bindings active within Session mode.
    pub session: SessionModeKeysConfig,
    /// Bindings active within Tmux/Prefix mode (one-shot prefix dispatch).
    pub tmux: TmuxModeKeysConfig,
    /// Optional indexed shortcuts expanded over number keys 1-9.
    pub indexed: IndexedKeysConfig,
    /// Custom command bindings.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<CommandKeybindConfig>,
}

/// Shared bindings (`[keys.shared]`) active in all non-locked modes. Each maps
/// to an existing herdr action; values are chords (Alt/Ctrl) so they do not
/// collide with per-mode bare keys.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SharedKeysConfig {
    pub focus_left: BindingConfig,
    pub focus_down: BindingConfig,
    pub focus_up: BindingConfig,
    pub focus_right: BindingConfig,
    pub new_pane: BindingConfig,
    pub close_focus: BindingConfig,
    pub detach: BindingConfig,
    pub resize_increase: BindingConfig,
    pub resize_decrease: BindingConfig,
    pub move_tab_left: BindingConfig,
    pub move_tab_right: BindingConfig,
    pub new_tab: BindingConfig,
    pub rename_tab: BindingConfig,
    pub toggle_floating: BindingConfig,
}

/// Pane mode (`[keys.pane]`) bare-key bindings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct PaneModeKeysConfig {
    pub focus_left: BindingConfig,
    pub focus_down: BindingConfig,
    pub focus_up: BindingConfig,
    pub focus_right: BindingConfig,
    pub new_pane: BindingConfig,
    pub split_down: BindingConfig,
    pub split_right: BindingConfig,
    pub stack: BindingConfig,
    pub close: BindingConfig,
    pub zoom: BindingConfig,
    pub toggle_float: BindingConfig,
    pub rename: BindingConfig,
    pub cycle: BindingConfig,
}

/// Tab mode (`[keys.tab]`) bare-key bindings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct TabModeKeysConfig {
    pub previous: BindingConfig,
    pub next: BindingConfig,
    pub new: BindingConfig,
    pub close: BindingConfig,
    pub rename: BindingConfig,
    pub break_to_tab: BindingConfig,
    pub toggle: BindingConfig,
}

/// Resize mode (`[keys.resize]`) bare-key bindings. Directional increase
/// (h/j/k/l), directional decrease (H/J/K/L), and magnitude keys (+/=/-).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ResizeModeKeysConfig {
    pub increase_left: BindingConfig,
    pub increase_down: BindingConfig,
    pub increase_up: BindingConfig,
    pub increase_right: BindingConfig,
    pub decrease_left: BindingConfig,
    pub decrease_down: BindingConfig,
    pub decrease_up: BindingConfig,
    pub decrease_right: BindingConfig,
    pub increase: BindingConfig,
    pub decrease: BindingConfig,
}

/// Move mode (`[keys.move]`) bare-key bindings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct MoveModeKeysConfig {
    pub move_left: BindingConfig,
    pub move_down: BindingConfig,
    pub move_up: BindingConfig,
    pub move_right: BindingConfig,
    pub cycle_forward: BindingConfig,
    pub cycle_backward: BindingConfig,
}

/// Session mode (`[keys.session]`) bare-key bindings — herdr's workspace/agent/
/// worktree hub.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SessionModeKeysConfig {
    pub workspace_up: BindingConfig,
    pub workspace_down: BindingConfig,
    pub focus_left: BindingConfig,
    pub focus_right: BindingConfig,
    pub cycle: BindingConfig,
    pub goto: BindingConfig,
    pub workspace_picker: BindingConfig,
    pub new_workspace: BindingConfig,
    pub new_worktree: BindingConfig,
    pub rename_workspace: BindingConfig,
    pub close_workspace: BindingConfig,
    pub settings: BindingConfig,
    pub help: BindingConfig,
    pub detach: BindingConfig,
    pub previous_agent: BindingConfig,
    pub next_agent: BindingConfig,
}

/// Tmux/Prefix mode (`[keys.tmux]`) bindings — the configurable prefix-style
/// alternative. Values are the keys pressed after the prefix (`mode_tmux`),
/// mirroring herdr's released prefix keymap.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct TmuxModeKeysConfig {
    pub help: BindingConfig,
    pub settings: BindingConfig,
    pub new_workspace: BindingConfig,
    pub new_worktree: BindingConfig,
    pub rename_workspace: BindingConfig,
    pub close_workspace: BindingConfig,
    pub workspace_picker: BindingConfig,
    pub goto: BindingConfig,
    pub detach: BindingConfig,
    pub reload_config: BindingConfig,
    pub open_notification_target: BindingConfig,
    pub new_tab: BindingConfig,
    pub rename_tab: BindingConfig,
    pub previous_tab: BindingConfig,
    pub next_tab: BindingConfig,
    pub close_tab: BindingConfig,
    pub rename_pane: BindingConfig,
    pub edit_scrollback: BindingConfig,
    pub copy_mode: BindingConfig,
    pub focus_pane_left: BindingConfig,
    pub focus_pane_down: BindingConfig,
    pub focus_pane_up: BindingConfig,
    pub focus_pane_right: BindingConfig,
    pub swap_pane_left: BindingConfig,
    pub swap_pane_down: BindingConfig,
    pub swap_pane_up: BindingConfig,
    pub swap_pane_right: BindingConfig,
    pub cycle_pane_next: BindingConfig,
    pub cycle_pane_previous: BindingConfig,
    pub split_vertical: BindingConfig,
    pub split_horizontal: BindingConfig,
    pub stack_pane: BindingConfig,
    pub unstack_pane: BindingConfig,
    pub close_pane: BindingConfig,
    pub break_pane_to_tab: BindingConfig,
    pub zoom: BindingConfig,
    pub resize_mode: BindingConfig,
    pub toggle_sidebar: BindingConfig,
    pub toggle_floating: BindingConfig,
    pub new_floating_pane: BindingConfig,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct IndexedKeysConfig {
    /// Modifier combo for tab shortcuts 1-9. Unset by default.
    pub tabs: String,
    /// Modifier combo for workspace shortcuts 1-9. Unset by default.
    pub workspaces: String,
    /// Modifier combo for agent shortcuts 1-9. Unset by default.
    pub agents: String,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct WorktreesConfig {
    /// Root directory under which Herdr creates <repo>/<branch-slug> checkouts.
    pub directory: String,
}

/// Tab-bar appearance config, nested under `[ui.tabs]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct TabsConfig {
    /// Render Powerline arrow separators between tabs. When false, tabs are
    /// separated by alternating backgrounds with no Powerline codepoints, so a
    /// terminal whose font lacks the arrow glyph degrades cleanly. Default: true.
    pub powerline: bool,
}

impl Default for TabsConfig {
    fn default() -> Self {
        Self { powerline: true }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub sidebar_width: u16,
    /// Minimum sidebar width (columns) when expanded. Default: 18.
    pub sidebar_min_width: u16,
    /// Maximum sidebar width (columns) when expanded. Default: 36.
    pub sidebar_max_width: u16,
    /// Fraction of total terminal width for the sidebar (0.0 disables, uses fixed sidebar_width).
    pub sidebar_width_ratio: f32,
    /// Terminal width at or below which Herdr uses the mobile single-column layout. Default: 64.
    pub mobile_width_threshold: u16,
    /// Capture mouse input for Herdr's mouse UI. Default: true.
    pub mouse_capture: bool,
    /// Modifier that lets right-click gestures pass through to pane apps. Empty disables it.
    pub right_click_passthrough_modifier: RightClickPassthroughModifierConfig,
    /// Force a full host-terminal redraw when the outer terminal regains focus. Default: true.
    pub redraw_on_focus_gained: bool,
    /// Lines to scroll per mouse wheel notch. Default: 3.
    pub mouse_scroll_lines: Option<NonZeroUsize>,
    /// Ask for confirmation before closing a workspace. Default: true.
    pub confirm_close: bool,
    /// Ask for a tab name before creating a new tab. Default: true.
    pub prompt_new_tab_name: bool,
    /// Show agent labels in split pane borders when no manual pane label is set. Default: false.
    pub show_agent_labels_on_pane_borders: bool,
    /// Agent sidebar ordering. Saved values are "spaces" or "priority". Default: "spaces".
    pub agent_panel_sort: AgentPanelSortConfig,
    /// Accent color for highlights, borders, and navigation UI.
    /// Accepts hex (#89b4fa), named colors (cyan, blue), or RGB (rgb(137,180,250)).
    pub accent: String,
    /// Show agent status dots on tab bar labels.
    pub show_tab_status: TabStatusMode,
    /// Tab-bar appearance (Powerline separators, etc.).
    pub tabs: TabsConfig,
    /// Bottom hint bar showing mode-contextual keyboard shortcuts.
    pub hint_bar: HintBarStyle,
    /// Optional visual toast notifications for background workspace events.
    pub toast: ToastConfig,
    /// Play sounds when agents change state in background workspaces.
    pub sound: SoundConfig,
}

/// Cursor shape (DECSCUSR) used for the forced IME anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImeCursorShape {
    Block,
    #[default]
    SteadyBlock,
    Underline,
    SteadyUnderline,
    Bar,
    SteadyBar,
}

impl ImeCursorShape {
    /// Convert to DECSCUSR parameter (1–6).
    pub fn to_decscusr(self) -> u8 {
        match self {
            Self::Block => 1,
            Self::SteadyBlock => 2,
            Self::Underline => 3,
            Self::SteadyUnderline => 4,
            Self::Bar => 5,
            Self::SteadyBar => 6,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct AdvancedConfig {
    /// Maximum scrollback buffer size in bytes retained per pane terminal. Default: 10000000.
    #[serde(alias = "scrollback_lines")]
    pub scrollback_limit_bytes: usize,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct RemoteConfig {
    /// Add a keepalive fallback under the user's ssh config for the `--remote`
    /// bridge. Set false to run plain ssh unchanged. Default: true.
    pub manage_ssh_config: bool,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            manage_ssh_config: true,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct ExperimentalConfig {
    /// Allow launching herdr inside an existing herdr pane. Default: true.
    pub allow_nested: bool,
    /// Experimental local Kitty graphics rendering for attached clients. Default: false.
    pub kitty_graphics: bool,
    /// Persist pane screen history to session-history.json. Default: false.
    pub pane_history: bool,
    /// Expose the focused pane's cursor anchor to the outer terminal even when
    /// the pane requested `?25l`, so macOS native input methods keep tracking
    /// the candidate window when TUIs paint their own cursor (Claude Code, pi,
    /// codex, etc.). Default: false.
    ///
    /// When the pane reports no cursor position, falls back to the pane's
    /// top-left so a stable IME anchor is always available.
    ///
    /// Trade-off when enabled: an extra hardware cursor will be visible in the
    /// outer terminal for apps that hide the cursor without painting a
    /// replacement (vim normal mode, etc.). See #149.
    pub reveal_hidden_cursor_for_cjk_ime: bool,
    /// Restrict `reveal_hidden_cursor_for_cjk_ime` to focused panes whose
    /// detected agent matches one of these names (case-insensitive). Empty
    /// list means apply to any focused pane. Unknown agent names are ignored;
    /// if the list contains no valid names, the reveal does not apply.
    /// Accepted names: pi, claude, codex, gemini, cursor, devin, cline,
    /// opencode, copilot, kimi, kiro, droid, amp, grok, hermes, kilo,
    /// qodercli, qoder.
    /// Default: empty.
    pub cjk_ime_agents: Vec<String>,
    /// Cursor shape rendered for the IME anchor when
    /// `reveal_hidden_cursor_for_cjk_ime` is enabled. Default: "steady_block".
    pub cjk_ime_cursor_shape: ImeCursorShape,
    /// While prefix mode is active, temporarily switch the macOS host input
    /// source to an ASCII-capable keyboard layout so prefix commands are read
    /// as ASCII even when a CJK IME is active, then restore the previous input
    /// source when prefix mode exits. macOS only; a no-op elsewhere and a
    /// best-effort no-op if the switch fails. Default: false.
    pub switch_ascii_input_source_in_prefix: bool,
}

impl Default for ExperimentalConfig {
    fn default() -> Self {
        Self {
            allow_nested: true,
            kitty_graphics: false,
            pane_history: false,
            reveal_hidden_cursor_for_cjk_ime: false,
            cjk_ime_agents: Vec::new(),
            cjk_ime_cursor_shape: ImeCursorShape::default(),
            switch_ascii_input_source_in_prefix: false,
        }
    }
}

impl Default for KeysConfig {
    fn default() -> Self {
        Self {
            default_mode: "modal".into(),
            mode_pane: "ctrl+p".into(),
            mode_tab: "ctrl+t".into(),
            mode_resize: "ctrl+n".into(),
            mode_move: "ctrl+h".into(),
            mode_session: "ctrl+o".into(),
            mode_locked: "ctrl+g".into(),
            mode_tmux: "ctrl+b".into(),
            shared: SharedKeysConfig::default(),
            pane: PaneModeKeysConfig::default(),
            tab: TabModeKeysConfig::default(),
            resize: ResizeModeKeysConfig::default(),
            move_: MoveModeKeysConfig::default(),
            session: SessionModeKeysConfig::default(),
            tmux: TmuxModeKeysConfig::default(),
            indexed: IndexedKeysConfig::default(),
            command: Vec::new(),
        }
    }
}

impl Default for SharedKeysConfig {
    fn default() -> Self {
        Self {
            focus_left: BindingConfig::one("alt+h"),
            focus_down: BindingConfig::one("alt+j"),
            focus_up: BindingConfig::one("alt+k"),
            focus_right: BindingConfig::one("alt+l"),
            new_pane: BindingConfig::one("alt+n"),
            close_focus: BindingConfig::one("alt+x"),
            detach: BindingConfig::one("ctrl+q"),
            // "alt+plus" aliases the shifted key; "alt++" is unparseable and never emitted.
            resize_increase: BindingConfig::Many(vec!["alt+=".into(), "alt+plus".into()]),
            resize_decrease: BindingConfig::one("alt+-"),
            move_tab_left: BindingConfig::one("alt+i"),
            move_tab_right: BindingConfig::one("alt+o"),
            new_tab: BindingConfig::one("alt+t"),
            rename_tab: BindingConfig::one("alt+r"),
            toggle_floating: BindingConfig::one("alt+w"),
        }
    }
}

impl Default for PaneModeKeysConfig {
    fn default() -> Self {
        Self {
            focus_left: BindingConfig::Many(vec!["h".into(), "left".into()]),
            focus_down: BindingConfig::Many(vec!["j".into(), "down".into()]),
            focus_up: BindingConfig::Many(vec!["k".into(), "up".into()]),
            focus_right: BindingConfig::Many(vec!["l".into(), "right".into()]),
            new_pane: BindingConfig::one("n"),
            split_down: BindingConfig::one("d"),
            split_right: BindingConfig::one("r"),
            stack: BindingConfig::one("s"),
            close: BindingConfig::one("x"),
            zoom: BindingConfig::Many(vec!["f".into(), "z".into()]),
            toggle_float: BindingConfig::one("w"),
            rename: BindingConfig::one("c"),
            cycle: BindingConfig::one("p"),
        }
    }
}

impl Default for TabModeKeysConfig {
    fn default() -> Self {
        Self {
            previous: BindingConfig::Many(vec!["h".into(), "left".into(), "up".into(), "k".into()]),
            next: BindingConfig::Many(vec!["l".into(), "right".into(), "down".into(), "j".into()]),
            new: BindingConfig::one("n"),
            close: BindingConfig::one("x"),
            rename: BindingConfig::one("r"),
            break_to_tab: BindingConfig::one("b"),
            toggle: BindingConfig::one("tab"),
        }
    }
}

impl Default for ResizeModeKeysConfig {
    fn default() -> Self {
        Self {
            increase_left: BindingConfig::Many(vec!["h".into(), "left".into()]),
            increase_down: BindingConfig::Many(vec!["j".into(), "down".into()]),
            increase_up: BindingConfig::Many(vec!["k".into(), "up".into()]),
            increase_right: BindingConfig::Many(vec!["l".into(), "right".into()]),
            decrease_left: BindingConfig::one("H"),
            decrease_down: BindingConfig::one("J"),
            decrease_up: BindingConfig::one("K"),
            decrease_right: BindingConfig::one("L"),
            // "plus" aliases the +/= key; a literal "+" is unparseable (the
            // binding parser splits on '+').
            increase: BindingConfig::Many(vec!["plus".into(), "=".into()]),
            decrease: BindingConfig::one("-"),
        }
    }
}

impl Default for MoveModeKeysConfig {
    fn default() -> Self {
        Self {
            move_left: BindingConfig::Many(vec!["h".into(), "left".into()]),
            move_down: BindingConfig::Many(vec!["j".into(), "down".into()]),
            move_up: BindingConfig::Many(vec!["k".into(), "up".into()]),
            move_right: BindingConfig::Many(vec!["l".into(), "right".into()]),
            cycle_forward: BindingConfig::Many(vec!["n".into(), "tab".into()]),
            cycle_backward: BindingConfig::one("p"),
        }
    }
}

impl Default for SessionModeKeysConfig {
    fn default() -> Self {
        Self {
            workspace_up: BindingConfig::Many(vec!["up".into(), "k".into()]),
            workspace_down: BindingConfig::Many(vec!["down".into(), "j".into()]),
            focus_left: BindingConfig::Many(vec!["h".into(), "left".into()]),
            focus_right: BindingConfig::Many(vec!["l".into(), "right".into()]),
            cycle: BindingConfig::one("tab"),
            goto: BindingConfig::one("g"),
            workspace_picker: BindingConfig::one("w"),
            new_workspace: BindingConfig::one("n"),
            new_worktree: BindingConfig::one("N"),
            rename_workspace: BindingConfig::one("r"),
            close_workspace: BindingConfig::one("x"),
            settings: BindingConfig::one("s"),
            help: BindingConfig::one("?"),
            detach: BindingConfig::one("d"),
            previous_agent: BindingConfig::one("["),
            next_agent: BindingConfig::one("]"),
        }
    }
}

impl Default for TmuxModeKeysConfig {
    fn default() -> Self {
        Self {
            help: BindingConfig::one("?"),
            settings: BindingConfig::one("s"),
            new_workspace: BindingConfig::one("shift+n"),
            new_worktree: BindingConfig::one("shift+g"),
            rename_workspace: BindingConfig::one("shift+w"),
            close_workspace: BindingConfig::one("shift+d"),
            workspace_picker: BindingConfig::one("w"),
            goto: BindingConfig::one("g"),
            detach: BindingConfig::one("q"),
            reload_config: BindingConfig::one("shift+r"),
            open_notification_target: BindingConfig::one("o"),
            new_tab: BindingConfig::one("c"),
            rename_tab: BindingConfig::one("shift+t"),
            previous_tab: BindingConfig::one("p"),
            next_tab: BindingConfig::one("n"),
            close_tab: BindingConfig::one("shift+x"),
            rename_pane: BindingConfig::one("shift+p"),
            edit_scrollback: BindingConfig::one("e"),
            copy_mode: BindingConfig::one("["),
            focus_pane_left: BindingConfig::one("h"),
            focus_pane_down: BindingConfig::one("j"),
            focus_pane_up: BindingConfig::one("k"),
            focus_pane_right: BindingConfig::one("l"),
            swap_pane_left: BindingConfig::one("shift+h"),
            swap_pane_down: BindingConfig::one("shift+j"),
            swap_pane_up: BindingConfig::one("shift+k"),
            swap_pane_right: BindingConfig::one("shift+l"),
            cycle_pane_next: BindingConfig::one("tab"),
            cycle_pane_previous: BindingConfig::one("shift+tab"),
            split_vertical: BindingConfig::one("v"),
            split_horizontal: BindingConfig::one("minus"),
            stack_pane: BindingConfig::one("shift+s"),
            unstack_pane: BindingConfig::one("shift+u"),
            close_pane: BindingConfig::one("x"),
            break_pane_to_tab: BindingConfig::one("!"),
            zoom: BindingConfig::one("z"),
            resize_mode: BindingConfig::one("r"),
            toggle_sidebar: BindingConfig::one("b"),
            toggle_floating: BindingConfig::one("f"),
            new_floating_pane: BindingConfig::one("shift+f"),
        }
    }
}

impl Default for WorktreesConfig {
    fn default() -> Self {
        Self {
            directory: "~/.herdr/worktrees".into(),
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            sidebar_width: 26,
            sidebar_min_width: 18,
            sidebar_max_width: 36,
            sidebar_width_ratio: 0.18,
            mobile_width_threshold: DEFAULT_MOBILE_WIDTH_THRESHOLD,
            mouse_capture: true,
            right_click_passthrough_modifier: RightClickPassthroughModifierConfig::default(),
            redraw_on_focus_gained: true,
            mouse_scroll_lines: None,
            confirm_close: true,
            prompt_new_tab_name: true,
            show_agent_labels_on_pane_borders: false,
            agent_panel_sort: AgentPanelSortConfig::Spaces,
            accent: "cyan".into(),
            show_tab_status: TabStatusMode::Off,
            tabs: TabsConfig::default(),
            hint_bar: HintBarStyle::Full,
            toast: ToastConfig::default(),
            sound: SoundConfig::default(),
        }
    }
}

impl UiConfig {
    pub fn mouse_scroll_lines(&self) -> usize {
        self.mouse_scroll_lines
            .map(NonZeroUsize::get)
            .unwrap_or(DEFAULT_MOUSE_SCROLL_LINES)
    }

    pub fn right_click_passthrough_modifiers(&self) -> Option<KeyModifiers> {
        self.right_click_passthrough_modifier.modifiers()
    }
}

impl Default for ToastConfig {
    fn default() -> Self {
        Self {
            delivery: ToastDelivery::Off,
            delay_seconds: 1,
            herdr: HerdrToastConfig::default(),
            clipboard: ClipboardToastConfig::default(),
        }
    }
}

impl Default for HerdrToastConfig {
    fn default() -> Self {
        Self {
            position: ToastHerdrPosition::BottomRight,
        }
    }
}

impl Default for ClipboardToastConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            position: ToastClipboardPosition::BottomCenter,
        }
    }
}

impl<'de> Deserialize<'de> for ToastConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct RawToastConfig {
            delivery: Option<ToastDelivery>,
            enabled: Option<bool>,
            delay_seconds: Option<u64>,
            herdr: HerdrToastConfig,
            clipboard: ClipboardToastConfig,
        }

        let raw = RawToastConfig::deserialize(deserializer)?;
        let legacy_delivery = match raw.enabled {
            Some(true) => ToastDelivery::Herdr,
            Some(false) | None => ToastDelivery::Off,
        };
        let delivery = raw.delivery.unwrap_or(legacy_delivery);
        let default = Self::default();
        let delay_seconds = raw.delay_seconds.unwrap_or(default.delay_seconds);
        if delay_seconds > MAX_TOAST_DELAY_SECONDS {
            return Err(de::Error::custom(format!(
                "ui.toast.delay_seconds must be between 0 and {MAX_TOAST_DELAY_SECONDS}"
            )));
        }
        Ok(Self {
            delivery,
            delay_seconds,
            herdr: raw.herdr,
            clipboard: raw.clipboard,
        })
    }
}

impl Default for AdvancedConfig {
    fn default() -> Self {
        Self {
            scrollback_limit_bytes: DEFAULT_SCROLLBACK_LIMIT_BYTES,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_channel_defaults_for_platform_and_parses() {
        let default_config = Config::default();
        assert_eq!(default_config.update.channel, default_update_channel());

        let toml = r#"
[update]
channel = "preview"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.update.channel, UpdateChannelConfig::Preview);
        assert_eq!(config.update.channel.as_str(), "preview");
    }

    #[test]
    fn terminal_default_shell_defaults_empty_and_parses() {
        let default_config = Config::default();
        assert!(default_config.terminal.default_shell.is_empty());
        assert_eq!(default_config.terminal.shell_mode, ShellModeConfig::Auto);

        let toml = r#"
[terminal]
default_shell = "nu"
shell_mode = "non_login"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.terminal.default_shell, "nu");
        assert_eq!(config.terminal.shell_mode, ShellModeConfig::NonLogin);
    }

    #[test]
    fn terminal_new_cwd_defaults_follow_and_parses() {
        let default_config = Config::default();
        assert_eq!(
            default_config.terminal.new_cwd,
            NewTerminalCwdConfig::Follow
        );

        let config: Config = toml::from_str(
            r#"
[terminal]
new_cwd = "home"
"#,
        )
        .unwrap();
        assert_eq!(config.terminal.new_cwd, NewTerminalCwdConfig::Home);

        let config: Config = toml::from_str(
            r#"
[terminal]
new_cwd = "~/Projects"
"#,
        )
        .unwrap();
        assert_eq!(
            config.terminal.new_cwd,
            NewTerminalCwdConfig::Path("~/Projects".into())
        );
    }

    #[test]
    fn resume_agents_on_restore_defaults_on_and_parses() {
        let default_config = Config::default();
        assert!(default_config.session.resume_agents_on_restore);

        let toml = r#"
[session]
resume_agents_on_restore = false
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.session.resume_agents_on_restore);
    }

    #[test]
    fn agent_panel_sort_config_parses_alias_and_defaults() {
        assert_eq!(
            Config::default().ui.agent_panel_sort,
            AgentPanelSortConfig::Spaces
        );

        let toml = r#"
[ui]
agent_panel_sort = "priority"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.agent_panel_sort, AgentPanelSortConfig::Priority);

        let toml = r#"
[ui]
agent_panel_sort = "workspaces"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.agent_panel_sort, AgentPanelSortConfig::Spaces);

        let toml = r#"
[ui]
agent_panel_scope = "current"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.agent_panel_sort, AgentPanelSortConfig::Spaces);
    }

    #[test]
    fn pane_border_agent_labels_default_off_and_parse() {
        let default_config = Config::default();
        assert!(!default_config.ui.show_agent_labels_on_pane_borders);

        let toml = r#"
[ui]
show_agent_labels_on_pane_borders = true
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.ui.show_agent_labels_on_pane_borders);
    }

    #[test]
    fn worktrees_directory_defaults_and_parses() {
        let default_config = Config::default();
        assert_eq!(default_config.worktrees.directory, "~/.herdr/worktrees");

        let toml = r#"
[worktrees]
directory = "~/Projects/herdr-worktrees"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.worktrees.directory, "~/Projects/herdr-worktrees");
    }

    #[test]
    fn prompt_new_tab_name_defaults_on_and_parses() {
        let default_config = Config::default();
        assert!(default_config.ui.prompt_new_tab_name);

        let toml = r#"
[ui]
prompt_new_tab_name = false
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.ui.prompt_new_tab_name);
    }

    #[test]
    fn reveal_hidden_cursor_for_cjk_ime_default_off_and_parse() {
        let default_config = Config::default();
        assert!(!default_config.experimental.reveal_hidden_cursor_for_cjk_ime);

        let toml = r#"
[experimental]
reveal_hidden_cursor_for_cjk_ime = true
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.experimental.reveal_hidden_cursor_for_cjk_ime);
    }

    #[test]
    fn switch_ascii_input_source_in_prefix_default_off_and_parse() {
        let default_config = Config::default();
        assert!(
            !default_config
                .experimental
                .switch_ascii_input_source_in_prefix
        );

        let toml = r#"
[experimental]
switch_ascii_input_source_in_prefix = true
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.experimental.switch_ascii_input_source_in_prefix);
    }

    #[test]
    fn cjk_ime_cursor_shape_default_steady_block_and_parse() {
        let default_config = Config::default();
        assert_eq!(
            default_config.experimental.cjk_ime_cursor_shape,
            ImeCursorShape::SteadyBlock
        );

        let toml = r#"
[experimental]
cjk_ime_cursor_shape = "bar"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            config.experimental.cjk_ime_cursor_shape,
            ImeCursorShape::Bar
        );
    }

    #[test]
    fn cjk_ime_agents_default_empty_and_parse() {
        let default_config = Config::default();
        assert!(default_config.experimental.cjk_ime_agents.is_empty());

        let toml = r#"
[experimental]
cjk_ime_agents = ["claude", "codex"]
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            config.experimental.cjk_ime_agents,
            vec!["claude".to_string(), "codex".to_string()]
        );
    }

    #[test]
    fn sidebar_bounds_default_and_parse() {
        let default_config = Config::default();
        assert_eq!(default_config.ui.sidebar_min_width, 18);
        assert_eq!(default_config.ui.sidebar_max_width, 36);
        assert_eq!(default_config.ui.sidebar_width_ratio, 0.18);
        assert_eq!(
            default_config.ui.mobile_width_threshold,
            DEFAULT_MOBILE_WIDTH_THRESHOLD
        );

        let toml = r#"
[ui]
sidebar_min_width = 12
sidebar_max_width = 80
sidebar_width_ratio = 0.25
mobile_width_threshold = 96
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.sidebar_min_width, 12);
        assert_eq!(config.ui.sidebar_max_width, 80);
        assert_eq!(config.ui.sidebar_width_ratio, 0.25);
        assert_eq!(config.ui.mobile_width_threshold, 96);
    }

    #[test]
    fn validated_sidebar_bounds_rejects_inverted() {
        assert_eq!(validated_sidebar_bounds(18, 36), Some((18, 36)));
        assert_eq!(validated_sidebar_bounds(20, 20), Some((20, 20)));
        assert_eq!(validated_sidebar_bounds(0, u16::MAX), Some((0, u16::MAX)));
        assert_eq!(validated_sidebar_bounds(50, 30), None);
        assert_eq!(validated_sidebar_bounds(u16::MAX, 0), None);
    }

    #[test]
    fn mouse_capture_default_on_and_parse() {
        let default_config = Config::default();
        assert!(default_config.ui.mouse_capture);

        let toml = r#"
[ui]
mouse_capture = false
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.ui.mouse_capture);
    }

    #[test]
    fn right_click_passthrough_modifier_defaults_off_and_parses() {
        let default_config = Config::default();
        assert_eq!(default_config.ui.right_click_passthrough_modifiers(), None);

        for value in ["", "off", "none", "disabled"] {
            let toml = format!(
                r#"
[ui]
right_click_passthrough_modifier = "{value}"
"#
            );
            let config: Config = toml::from_str(&toml).unwrap();
            assert_eq!(
                config.ui.right_click_passthrough_modifiers(),
                None,
                "value {value:?} should disable passthrough"
            );
        }

        for (value, expected) in [
            ("ctrl", KeyModifiers::CONTROL),
            ("control", KeyModifiers::CONTROL),
            ("alt", KeyModifiers::ALT),
            ("option", KeyModifiers::ALT),
            ("cmd", KeyModifiers::SUPER),
            ("command", KeyModifiers::SUPER),
            ("super", KeyModifiers::SUPER),
            ("meta", KeyModifiers::META),
            ("hyper", KeyModifiers::HYPER),
        ] {
            let toml = format!(
                r#"
[ui]
right_click_passthrough_modifier = "{value}"
"#
            );
            let config: Config = toml::from_str(&toml).unwrap();
            assert_eq!(
                config.ui.right_click_passthrough_modifiers(),
                Some(expected),
                "value {value:?} should parse"
            );
        }

        let toml = r#"
[ui]
right_click_passthrough_modifier = "cmd+alt"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            config.ui.right_click_passthrough_modifiers(),
            Some(KeyModifiers::SUPER | KeyModifiers::ALT)
        );
    }

    #[test]
    fn right_click_passthrough_modifier_rejects_shift() {
        for value in ["shift", "shift+ctrl", "ctrl+", "ctrl++alt", "banana"] {
            let toml = format!(
                r#"
[ui]
right_click_passthrough_modifier = "{value}"
"#
            );
            assert!(
                toml::from_str::<Config>(&toml).is_err(),
                "value {value:?} should be rejected"
            );
        }
    }

    #[test]
    fn redraw_on_focus_gained_default_on_and_parse() {
        let default_config = Config::default();
        assert!(default_config.ui.redraw_on_focus_gained);

        let toml = r#"
[ui]
redraw_on_focus_gained = false
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.ui.redraw_on_focus_gained);
    }

    #[test]
    fn mouse_scroll_lines_defaults_to_three_and_parses() {
        let default_config = Config::default();
        assert_eq!(
            default_config.ui.mouse_scroll_lines(),
            DEFAULT_MOUSE_SCROLL_LINES
        );

        let toml = r#"
[ui]
mouse_scroll_lines = 1
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.mouse_scroll_lines(), 1);
    }

    #[test]
    fn mouse_scroll_lines_rejects_zero() {
        let toml = r#"
[ui]
mouse_scroll_lines = 0
"#;
        assert!(toml::from_str::<Config>(toml).is_err());
    }

    #[test]
    fn toast_config_parses() {
        let toml = r#"
[ui.toast]
delivery = "terminal"
delay_seconds = 2

[ui.toast.herdr]
position = "top-left"

[ui.toast.clipboard]
enabled = false
position = "top-center"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.toast.delivery, ToastDelivery::Terminal);
        assert_eq!(config.ui.toast.delay_seconds, 2);
        assert_eq!(config.ui.toast.herdr.position, ToastHerdrPosition::TopLeft);
        assert!(!config.ui.toast.clipboard.enabled);
        assert_eq!(
            config.ui.toast.clipboard.position,
            ToastClipboardPosition::TopCenter
        );
    }

    #[test]
    fn toast_config_defaults_preserve_existing_behavior_with_delay() {
        let config = Config::default();
        assert_eq!(config.ui.toast.delivery, ToastDelivery::Off);
        assert_eq!(config.ui.toast.delay_seconds, 1);
        assert_eq!(
            config.ui.toast.herdr.position,
            ToastHerdrPosition::BottomRight
        );
        assert!(config.ui.toast.clipboard.enabled);
        assert_eq!(
            config.ui.toast.clipboard.position,
            ToastClipboardPosition::BottomCenter
        );
    }

    #[test]
    fn toast_config_parses_system_delivery() {
        let toml = r#"
[ui.toast]
delivery = "system"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.toast.delivery, ToastDelivery::System);
    }

    #[test]
    fn toast_config_legacy_enabled_true_maps_to_herdr() {
        let toml = r#"
[ui.toast]
enabled = true
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.toast.delivery, ToastDelivery::Herdr);
    }

    #[test]
    fn toast_config_legacy_enabled_false_maps_to_off() {
        let toml = r#"
[ui.toast]
enabled = false
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.toast.delivery, ToastDelivery::Off);
    }

    #[test]
    fn toast_config_delivery_wins_over_legacy_enabled() {
        let toml = r#"
[ui.toast]
enabled = true
delivery = "terminal"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.toast.delivery, ToastDelivery::Terminal);
    }

    #[test]
    fn toast_config_rejects_unbounded_delay() {
        let toml = format!(
            r#"
[ui.toast]
delay_seconds = {}
"#,
            MAX_TOAST_DELAY_SECONDS + 1
        );

        let error = toml::from_str::<Config>(&toml).unwrap_err().to_string();

        assert!(error.contains("ui.toast.delay_seconds must be between 0 and 3600"));
    }

    #[test]
    fn missing_onboarding_shows_setup() {
        let config = Config::default();
        assert!(config.should_show_onboarding());
    }

    #[test]
    fn onboarding_false_skips_setup() {
        let config: Config = toml::from_str("onboarding = false").unwrap();
        assert!(!config.should_show_onboarding());
    }

    #[test]
    fn advanced_defaults_include_scrollback_limit_bytes() {
        let config = Config::default();
        assert_eq!(
            config.advanced.scrollback_limit_bytes,
            DEFAULT_SCROLLBACK_LIMIT_BYTES
        );
    }

    #[test]
    fn pane_history_persistence_is_opt_in() {
        assert!(!Config::default().experimental.pane_history);

        let toml = r#"
[experimental]
pane_history = true
"#;
        let config: Config = toml::from_str(toml).unwrap();

        assert!(config.experimental.pane_history);
    }

    #[test]
    fn kitty_graphics_default_off_and_parse() {
        let config = Config::default();
        assert!(!config.experimental.kitty_graphics);

        let toml = r#"
[experimental]
kitty_graphics = true
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.experimental.kitty_graphics);
    }

    #[test]
    fn experimental_config_parses() {
        let toml = r#"
[experimental]
allow_nested = true
kitty_graphics = true
pane_history = true
switch_ascii_input_source_in_prefix = true
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.experimental.allow_nested);
        assert!(config.experimental.kitty_graphics);
        assert!(config.experimental.pane_history);
        assert!(config.experimental.switch_ascii_input_source_in_prefix);
    }

    #[test]
    fn advanced_config_parses() {
        let toml = r#"
[advanced]
scrollback_limit_bytes = 12345
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.advanced.scrollback_limit_bytes, 12345);
    }

    #[test]
    fn advanced_legacy_scrollback_lines_alias_parses() {
        let toml = r#"
[advanced]
scrollback_lines = 12345
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.advanced.scrollback_limit_bytes, 12345);
    }

    #[test]
    fn tab_status_mode_defaults_off_and_parses() {
        let default_config = Config::default();
        assert_eq!(default_config.ui.show_tab_status, TabStatusMode::Off);

        let toml_off = r#"
[ui]
show_tab_status = "off"
"#;
        let config: Config = toml::from_str(toml_off).unwrap();
        assert_eq!(config.ui.show_tab_status, TabStatusMode::Off);

        let toml_attention = r#"
[ui]
show_tab_status = "attention"
"#;
        let config: Config = toml::from_str(toml_attention).unwrap();
        assert_eq!(config.ui.show_tab_status, TabStatusMode::Attention);

        let toml_all = r#"
[ui]
show_tab_status = "all"
"#;
        let config: Config = toml::from_str(toml_all).unwrap();
        assert_eq!(config.ui.show_tab_status, TabStatusMode::All);
    }

    #[test]
    fn tab_status_mode_rejects_unknown_value() {
        let toml = r#"
[ui]
show_tab_status = "blocked"
"#;
        assert!(toml::from_str::<Config>(toml).is_err());
    }

    #[test]
    fn tabs_powerline_defaults_on_and_parses() {
        // Default is ON.
        let default_config = Config::default();
        assert!(default_config.ui.tabs.powerline);

        // Explicit OFF parses, is not flagged unknown, and round-trips.
        let toml_off = r#"
[ui.tabs]
powerline = false
"#;
        let config: Config = toml::from_str(toml_off).unwrap();
        assert!(!config.ui.tabs.powerline);

        // Round-trip the nested struct through TOML.
        let serialized = toml::to_string(&config.ui.tabs).unwrap();
        let restored: TabsConfig = toml::from_str(&serialized).unwrap();
        assert!(!restored.powerline);

        // Explicit ON parses too.
        let toml_on = r#"
[ui.tabs]
powerline = true
"#;
        let config: Config = toml::from_str(toml_on).unwrap();
        assert!(config.ui.tabs.powerline);
    }

    #[test]
    fn break_pane_to_tab_default_and_override() {
        let default_config = Config::default();
        assert_eq!(
            default_config.keys.tmux.break_pane_to_tab,
            BindingConfig::one("!")
        );

        let toml = r#"
[keys.tmux]
break_pane_to_tab = "shift+b"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            config.keys.tmux.break_pane_to_tab,
            BindingConfig::one("shift+b")
        );
    }

    #[test]
    fn allow_nested_defaults_true_when_missing_from_toml() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.experimental.allow_nested);

        let config: Config = toml::from_str("[experimental]\n").unwrap();
        assert!(config.experimental.allow_nested);
    }

    #[test]
    fn allow_nested_respects_explicit_false() {
        let config: Config =
            toml::from_str("[experimental]\nallow_nested = false\n").unwrap();
        assert!(!config.experimental.allow_nested);
    }

    #[test]
    fn allow_nested_rust_default_is_true() {
        let config = Config::default();
        assert!(config.experimental.allow_nested);
    }
}
