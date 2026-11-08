//! Fork: settings for the fork's client chrome (hint bar, tab styling, sidebar).

use serde::{Deserialize, Serialize};

/// What the contextual hint bar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HintBarStyleConfig {
    /// Every hint that fits.
    Full,
    /// The four most important hints of the current mode.
    Compact,
    /// No hint bar; upstream's mode bar covers the pane's last row instead.
    #[default]
    Off,
}

/// How the desktop tab bar is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TabStyleConfig {
    /// Upstream's centered labels with scroll arrows when tabs overflow.
    #[default]
    Upstream,
    /// zellij-style tiles, centered on the active tab, with `+N` overflow tiles.
    Zellij,
}

/// Which agent states the zellij tab style marks with a status dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TabStatusModeConfig {
    /// No dots.
    #[default]
    Off,
    /// Blocked agents and finished agents you have not looked at yet.
    Attention,
    /// Every known agent state.
    All,
}

/// How the desktop sidebar and the phone switcher's rows are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SidebarStyleConfig {
    /// Upstream's 4-column rail and plain lists.
    #[default]
    Upstream,
    /// A 7-column rail with attention markers, `+N` overflow badges that count
    /// waiting agents, and agent labels that dim once settled. The phone switcher
    /// uses the same rows and badges, and nests tabs under each space on the active
    /// machine.
    Zellij,
}

/// Tab and hint-bar styling (`[ui.tabs]`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct TabsConfig {
    /// Tab bar style: "upstream" or "zellij". Default: upstream.
    pub style: TabStyleConfig,
    /// Draw Powerline wedges (U+E0B0) between zellij-style tabs and hint-bar tiles.
    /// Needs a Powerline or Nerd Font. Default: true.
    pub powerline: bool,
    /// Fork: custom `$name` pane token that marks a tab: a tab with any pane
    /// holding a non-empty value shows `mark_glyph` before its name.
    #[serde(
        deserialize_with = "super::sidebar::deserialize_divider_token",
        serialize_with = "super::sidebar::serialize_divider_token",
        skip_serializing_if = "Option::is_none"
    )]
    pub mark_token: Option<String>,
    /// Fork: the glyph drawn for `mark_token`. Default: "⚑".
    pub mark_glyph: String,
}

impl Default for TabsConfig {
    fn default() -> Self {
        Self {
            style: TabStyleConfig::Upstream,
            powerline: true,
            mark_token: None,
            mark_glyph: "⚑".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn hint_bar_is_off_and_powerline_on_by_default() {
        let config = Config::default();
        assert_eq!(config.ui.hint_bar, HintBarStyleConfig::Off);
        assert!(config.ui.tabs.powerline);
    }

    #[test]
    fn tab_style_and_status_default_to_upstream_and_off_and_parse() {
        let config = Config::default();
        assert_eq!(config.ui.tabs.style, TabStyleConfig::Upstream);
        assert_eq!(config.ui.show_tab_status, TabStatusModeConfig::Off);
        let config: Config =
            toml::from_str("[ui]\nshow_tab_status = \"all\"\ntabs.style = \"zellij\"\n").unwrap();
        assert_eq!(config.ui.tabs.style, TabStyleConfig::Zellij);
        assert_eq!(config.ui.show_tab_status, TabStatusModeConfig::All);
        let config: Config = toml::from_str("[ui]\nshow_tab_status = \"attention\"\n").unwrap();
        assert_eq!(config.ui.show_tab_status, TabStatusModeConfig::Attention);
    }

    #[test]
    fn sidebar_style_defaults_to_upstream_and_parses() {
        assert_eq!(
            Config::default().ui.sidebar.style,
            SidebarStyleConfig::Upstream
        );
        let config: Config = toml::from_str("[ui]\nsidebar.style = \"zellij\"\n").unwrap();
        assert_eq!(config.ui.sidebar.style, SidebarStyleConfig::Zellij);
    }

    #[test]
    fn tab_mark_token_is_off_by_default_and_parses_a_custom_token() {
        let config = Config::default();
        assert_eq!(config.ui.tabs.mark_token, None);
        assert_eq!(config.ui.tabs.mark_glyph, "⚑");
        assert!(!toml::to_string(&config.ui.tabs)
            .unwrap()
            .contains("mark_token"));

        let config: Config =
            toml::from_str("[ui.tabs]\nmark_token = \"$agent_mark\"\nmark_glyph = \"*\"\n")
                .unwrap();
        assert_eq!(config.ui.tabs.mark_token.as_deref(), Some("agent_mark"));
        assert_eq!(config.ui.tabs.mark_glyph, "*");
        let encoded = toml::to_string(&config.ui.tabs).unwrap();
        assert!(
            encoded.contains("mark_token = \"$agent_mark\""),
            "{encoded}"
        );
        assert_eq!(
            toml::from_str::<TabsConfig>(&encoded).unwrap(),
            config.ui.tabs
        );

        for value in ["\"agent_mark\"", "\"$\"", "\"$bad.name\""] {
            let input = format!("[ui.tabs]\nmark_token = {value}\n");
            assert!(
                toml::from_str::<Config>(&input).is_err(),
                "accepted {value}"
            );
        }
    }

    #[test]
    fn hint_bar_and_powerline_parse() {
        let config: Config =
            toml::from_str("[ui]\nhint_bar = \"compact\"\ntabs.powerline = false\n").unwrap();
        assert_eq!(config.ui.hint_bar, HintBarStyleConfig::Compact);
        assert!(!config.ui.tabs.powerline);
        let config: Config = toml::from_str("[ui]\nhint_bar = \"full\"\n").unwrap();
        assert_eq!(config.ui.hint_bar, HintBarStyleConfig::Full);
    }
}
