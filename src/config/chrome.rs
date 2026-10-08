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

/// Tab and hint-bar styling (`[ui.tabs]`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct TabsConfig {
    /// Draw Powerline wedges (U+E0B0) between hint-bar tiles. Needs a Powerline or
    /// Nerd Font. Default: true.
    pub powerline: bool,
}

impl Default for TabsConfig {
    fn default() -> Self {
        Self { powerline: true }
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
    fn hint_bar_and_powerline_parse() {
        let config: Config =
            toml::from_str("[ui]\nhint_bar = \"compact\"\ntabs.powerline = false\n").unwrap();
        assert_eq!(config.ui.hint_bar, HintBarStyleConfig::Compact);
        assert!(!config.ui.tabs.powerline);
        let config: Config = toml::from_str("[ui]\nhint_bar = \"full\"\n").unwrap();
        assert_eq!(config.ui.hint_bar, HintBarStyleConfig::Full);
    }
}
