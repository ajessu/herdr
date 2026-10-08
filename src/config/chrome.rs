//! Fork: settings for the fork's client chrome (hint bar, tab styling).

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

/// Tab and hint-bar styling (`[ui.tabs]`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct TabsConfig {
    /// Tab bar style: "upstream" or "zellij". Default: upstream.
    pub style: TabStyleConfig,
    /// Draw Powerline wedges (U+E0B0) between zellij-style tabs and hint-bar tiles.
    /// Needs a Powerline or Nerd Font. Default: true.
    pub powerline: bool,
}

impl Default for TabsConfig {
    fn default() -> Self {
        Self {
            style: TabStyleConfig::Upstream,
            powerline: true,
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
    fn hint_bar_and_powerline_parse() {
        let config: Config =
            toml::from_str("[ui]\nhint_bar = \"compact\"\ntabs.powerline = false\n").unwrap();
        assert_eq!(config.ui.hint_bar, HintBarStyleConfig::Compact);
        assert!(!config.ui.tabs.powerline);
        let config: Config = toml::from_str("[ui]\nhint_bar = \"full\"\n").unwrap();
        assert_eq!(config.ui.hint_bar, HintBarStyleConfig::Full);
    }
}
