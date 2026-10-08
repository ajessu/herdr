//! Fork: the contextual hint bar, a fixed row below the panes that shows the
//! current mode and its keys (zellij-style).
//!
//! The row's height depends only on config and terminal size, never on the
//! mode: mode changes repaint without resizing, and the surface-patch fast path
//! assumes the pane rect stays put.
//!
//! Tiles, section prefixes and width fitting are ported from the fork's
//! pre-v0.9.0 `src/ui/hint_bar.rs`; the hint sets are built from the live
//! keymap, so every configured key shows.

use std::borrow::Cow;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use crate::app::state::Palette;
use crate::config::{ActionKeybinds, HintBarStyleConfig, Keybinds, StickyMode};

use super::modal::ModalMode;
use super::{ClientShellMode, ClientShellState};

/// Take the hint row off the bottom of the pane region. Returns the remaining
/// pane rect and the hint row, which is empty when the bar is off or the panes
/// would be left with no rows.
pub(super) fn reserve_row(style: HintBarStyleConfig, panes: Rect) -> (Rect, Rect) {
    if style == HintBarStyleConfig::Off || panes.height < 2 {
        return (panes, Rect::default());
    }
    let hint = Rect::new(panes.x, panes.bottom() - 1, panes.width, 1);
    (
        Rect {
            height: panes.height - 1,
            ..panes
        },
        hint,
    )
}

impl ClientShellState {
    /// Paint the hint row. Copy mode and endpoint errors keep upstream's mode
    /// bar (it carries the live search prompt and the error text), drawn in
    /// the hint row instead of over the panes.
    pub(super) fn render_hint_row(&self, buffer: &mut Buffer, area: Rect, update_ready: bool) {
        if self.endpoint_error.is_some() || self.mode == ClientShellMode::Copy {
            super::render::render_mode_bar(
                buffer,
                area,
                self.mode,
                self.copy_mode.as_ref(),
                self.endpoint_error.as_deref(),
                update_ready,
                &self.config.keybinds,
                &self.config.palette,
            );
            return;
        }
        let hint_set = hints(
            self.mode,
            self.modal_locked,
            &self.config.keybinds.keybinds,
            &self.config.keybinds.prefix,
        );
        let line = build_hint_line(
            &hint_set,
            self.config.hint_bar,
            &self.config.palette,
            area.width,
            self.config.powerline,
        );
        let palette = &self.config.palette;
        buffer.set_style(area, Style::default().bg(palette.panel_bg));
        buffer.set_line(area.x, area.y, &line, area.width);

        let session = matches!(
            self.mode,
            ClientShellMode::Navigate | ClientShellMode::Modal(ModalMode::Session)
        );
        if session && update_ready {
            let text = " update ready";
            let width = (text.width() as u16).min(area.width);
            let status = Rect::new(area.right() - width, area.y, width, 1);
            buffer.set_style(status, Style::default().bg(palette.panel_bg));
            buffer.set_stringn(
                status.x,
                status.y,
                text,
                usize::from(width),
                Style::default()
                    .fg(palette.accent)
                    .bg(palette.panel_bg)
                    .add_modifier(Modifier::BOLD),
            );
        }
    }
}

/// Minimum blank columns kept between the left and right hint sections so they
/// stay visually distinct and never touch.
const MIN_SECTION_GAP: usize = 2;

pub(super) struct Hint {
    pub(super) key: Cow<'static, str>,
    pub(super) label: &'static str,
    /// Lower is more important; `compact` keeps the first four.
    pub(super) priority: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BadgeColor {
    Accent,
    Mauve,
    Locked,
}

pub(super) struct Badge {
    pub(super) label: &'static str,
    accent: BadgeColor,
}

pub(super) struct HintSet {
    pub(super) badge: Badge,
    pub(super) hints: Vec<Hint>,
    pub(super) alt_hints: Vec<Hint>,
}

fn sanitize_key(s: &str) -> Cow<'_, str> {
    let needs_sanitize = s.chars().any(|c| c.is_control() || is_bidi_override(c));
    if needs_sanitize {
        Cow::Owned(
            s.chars()
                .filter(|c| !c.is_control() && !is_bidi_override(*c))
                .collect(),
        )
    } else {
        Cow::Borrowed(s)
    }
}

fn is_bidi_override(c: char) -> bool {
    matches!(
        c,
        '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}'
    )
}

/// `shift+h` reads better as `H` in a hint tile.
fn display_key(label: &str) -> String {
    match label.strip_prefix("shift+") {
        Some(rest) if rest.len() == 1 && rest.chars().all(|c| c.is_ascii_lowercase()) => {
            rest.to_ascii_uppercase()
        }
        _ => label.to_owned(),
    }
}

fn first_label(bindings: &ActionKeybinds) -> Option<String> {
    bindings
        .bindings
        .first()
        .map(|binding| display_key(&binding.label))
}

/// The first direct, alt-modified binding of an action (e.g. `alt+h`).
fn alt_direct_label(bindings: &ActionKeybinds) -> Option<String> {
    bindings
        .bindings
        .iter()
        .find(|binding| {
            binding.trigger.is_direct()
                && binding
                    .trigger
                    .combo()
                    .1
                    .contains(crossterm::event::KeyModifiers::ALT)
        })
        .map(|binding| binding.label.clone())
}

fn prefix_rhs(bindings: &ActionKeybinds) -> Option<String> {
    bindings.prefix_rhs_label()
}

struct HintList(Vec<Hint>);

impl HintList {
    fn push(&mut self, key: Option<String>, label: &'static str) {
        if let Some(key) = key {
            let priority = u8::try_from(self.0.len()).unwrap_or(u8::MAX);
            self.0.push(Hint {
                key: Cow::Owned(key),
                label,
                priority,
            });
        }
    }

    fn push_static(&mut self, key: &'static str, label: &'static str) {
        self.push(Some(key.to_owned()), label);
    }
}

fn joined(labels: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    let labels: Vec<String> = labels.into_iter().flatten().collect();
    (!labels.is_empty()).then(|| labels.join("/"))
}

/// The hints for the current mode. `locked` only matters in `Terminal`.
pub(super) fn hints(
    mode: ClientShellMode,
    locked: bool,
    keybinds: &Keybinds,
    prefix: &[crate::config::KeyCombo],
) -> HintSet {
    match mode {
        ClientShellMode::Terminal | ClientShellMode::Copy if locked => locked_hints(keybinds),
        ClientShellMode::Terminal | ClientShellMode::Copy => terminal_hints(keybinds),
        ClientShellMode::Prefix => prefix_hints(keybinds, prefix),
        ClientShellMode::Navigate => navigate_hints(keybinds),
        ClientShellMode::Resize => mode_hints(keybinds, StickyMode::Resize, "RESIZE"),
        ClientShellMode::Modal(ModalMode::Pane) => mode_hints(keybinds, StickyMode::Pane, "PANE"),
        ClientShellMode::Modal(ModalMode::Tab) => mode_hints(keybinds, StickyMode::Tab, "TAB"),
        ClientShellMode::Modal(ModalMode::Move) => mode_hints(keybinds, StickyMode::Move, "MOVE"),
        ClientShellMode::Modal(ModalMode::Session) => {
            mode_hints(keybinds, StickyMode::Session, "SESSION")
        }
    }
}

fn terminal_hints(keybinds: &Keybinds) -> HintSet {
    let entry = &keybinds.modal.entry;
    let mut hints = HintList(Vec::new());
    hints.push(first_label(&entry.pane), "PANE");
    hints.push(first_label(&entry.tab), "TAB");
    hints.push(first_label(&entry.resize), "RESIZE");
    hints.push(first_label(&entry.move_), "MOVE");
    hints.push(first_label(&entry.session), "SESSION");
    hints.push(first_label(&entry.locked), "LOCK");

    let modal = &keybinds.modal;
    let mut alt = HintList(Vec::new());
    alt.push(
        joined(
            [
                &keybinds.focus_pane_left,
                &keybinds.focus_pane_down,
                &keybinds.focus_pane_up,
                &keybinds.focus_pane_right,
            ]
            .map(alt_direct_label),
        ),
        "FOCUS",
    );
    alt.push(alt_direct_label(&modal.split_auto), "SPLIT");
    alt.push(alt_direct_label(&keybinds.close_pane), "CLOSE");
    alt.push(
        joined([&modal.resize_grow, &modal.resize_shrink].map(alt_direct_label)),
        "RESIZE",
    );
    alt.push(
        joined([&modal.move_tab_left, &modal.move_tab_right].map(alt_direct_label)),
        "MOV TAB",
    );
    HintSet {
        badge: Badge {
            label: "NORMAL",
            accent: BadgeColor::Accent,
        },
        hints: hints.0,
        alt_hints: alt.0,
    }
}

fn locked_hints(keybinds: &Keybinds) -> HintSet {
    let mut hints = HintList(Vec::new());
    hints.push(
        Some(first_label(&keybinds.modal.entry.locked).unwrap_or_else(|| "(unbound)".into())),
        "unlock",
    );
    hints.push_static("*", "keys pass through");
    HintSet {
        badge: Badge {
            label: "LOCKED",
            accent: BadgeColor::Locked,
        },
        hints: hints.0,
        alt_hints: Vec::new(),
    }
}

fn prefix_hints(keybinds: &Keybinds, prefix: &[crate::config::KeyCombo]) -> HintSet {
    let mut hints = HintList(Vec::new());
    hints.push_static("esc", "cancel");
    // The primary prefix, like upstream's mode bar; the others work too.
    let primary = &prefix[..prefix.len().min(1)];
    hints.push(
        Some(crate::config::format_prefix_combos(primary)),
        "send prefix",
    );
    hints.push(prefix_rhs(&keybinds.workspace_picker), "workspace nav");
    hints.push(prefix_rhs(&keybinds.help), "keybinds");
    HintSet {
        badge: Badge {
            label: "PREFIX",
            accent: BadgeColor::Accent,
        },
        hints: hints.0,
        alt_hints: Vec::new(),
    }
}

fn navigate_hints(keybinds: &Keybinds) -> HintSet {
    let navigate = &keybinds.navigate;
    let mut hints = HintList(Vec::new());
    hints.push(
        joined([
            first_label(&navigate.workspace_up),
            first_label(&navigate.workspace_down),
        ]),
        "workspace",
    );
    hints.push_static("tab", "pane");
    hints.push_static("enter", "open");
    hints.push_static("1-9", "switch");
    hints.push_static("esc", "back");
    HintSet {
        badge: Badge {
            label: "NAVIGATE",
            accent: BadgeColor::Accent,
        },
        hints: hints.0,
        alt_hints: Vec::new(),
    }
}

/// A sticky mode's hints, straight from its `[keys.<mode>]` table. Runs of
/// entries whose descriptions share a first word collapse into one tile
/// (`h/j/k/l focus`).
fn mode_hints(keybinds: &Keybinds, mode: StickyMode, badge: &'static str) -> HintSet {
    let mut groups: Vec<(Vec<String>, &'static str, &'static str)> = Vec::new();
    for entry in &keybinds.modal.mode(mode).entries {
        let Some(key) = first_label(&entry.bindings) else {
            continue;
        };
        let word = entry
            .description
            .split_whitespace()
            .next()
            .unwrap_or(entry.description);
        match groups.last_mut() {
            Some((keys, last_word, _)) if *last_word == word => keys.push(key),
            _ => groups.push((vec![key], word, entry.description)),
        }
    }
    let mut hints = HintList(Vec::new());
    for (keys, word, description) in groups {
        let label = if keys.len() == 1 { description } else { word };
        hints.push(Some(keys.join("/")), label);
    }
    if matches!(mode, StickyMode::Tab | StickyMode::Session) {
        hints.push_static("1-9", "goto tab");
    }
    if mode == StickyMode::Session {
        hints.push_static("enter", "open");
    }
    hints.push_static("esc", "exit");
    HintSet {
        badge: Badge {
            label: badge,
            accent: BadgeColor::Mauve,
        },
        hints: hints.0,
        alt_hints: Vec::new(),
    }
}

/// Detect a common modifier prefix shared by ALL keys in a section.
/// Returns the display prefix ("Ctrl +" or "Alt +") if all keys share one.
fn detect_section_prefix(hints: &[&Hint]) -> Option<&'static str> {
    if hints.is_empty() {
        return None;
    }
    let all_ctrl = hints.iter().all(|h| {
        let key = sanitize_key(&h.key);
        key.split('/').all(|k| k.starts_with("ctrl+"))
    });
    if all_ctrl {
        return Some("Ctrl +");
    }
    let all_alt = hints.iter().all(|h| {
        let key = sanitize_key(&h.key);
        key.split('/').all(|k| k.starts_with("alt+"))
    });
    if all_alt {
        return Some("Alt +");
    }
    None
}

/// Strip the modifier prefix ("ctrl+" or "alt+") from each sub-key in a
/// "/"-separated compound key string.
fn strip_key_modifier(key: &str, display_prefix: &str) -> String {
    let strip = if display_prefix == "Ctrl +" {
        "ctrl+"
    } else {
        "alt+"
    };
    key.split('/')
        .map(|k| k.strip_prefix(strip).unwrap_or(k))
        .collect::<Vec<_>>()
        .join("/")
}

/// Shared color bundle for hint-bar tile rendering. Each tile owns its own left
/// and right arrow, both blending against `outer_bg` (zellij convention).
#[derive(Clone, Copy)]
struct TileColors {
    tile_bg: Color,
    key_fg: Color,
    label_fg: Color,
    outer_bg: Color,
}

/// The Powerline "right arrow" between hint-bar tiles.
const POWERLINE_ARROW: &str = "\u{e0b0}";

/// Width of a hint section: the modifier prefix text (when present) plus each
/// tile's footprint (2 arrows + ` <key> LABEL `). Tiles abut directly.
fn compute_section_width(hints: &[&Hint], prefix: Option<&str>, powerline: bool) -> usize {
    if hints.is_empty() && prefix.is_none() {
        return 0;
    }
    let arrow_w: usize = if powerline { 2 } else { 0 };
    let mut total = 0usize;

    if let Some(pfx) = prefix {
        total += 1 + pfx.width() + 1;
    }

    for hint in hints {
        let key = sanitize_key(&hint.key);
        let bare_key = if let Some(pfx) = prefix {
            strip_key_modifier(&key, pfx)
        } else {
            key.into_owned()
        };
        // [left arrow][ <key> label ][right arrow] = arrow_w + 5 + key + label
        total += arrow_w + 5 + bare_key.width() + hint.label.width();
    }

    total
}

/// Append a single `<key> LABEL` tile (with its arrows when powerline is on).
/// Only the key body gets the accent; the brackets and label use the dim label
/// color, as in zellij's status bar.
fn push_tile(
    spans: &mut Vec<Span<'static>>,
    bare_key: &str,
    label: &str,
    colors: &TileColors,
    powerline: bool,
) {
    if powerline {
        spans.push(Span::styled(
            POWERLINE_ARROW,
            Style::default().fg(colors.outer_bg).bg(colors.tile_bg),
        ));
    }
    let bracket_style = Style::default().fg(colors.label_fg).bg(colors.tile_bg);
    let key_style = Style::default()
        .fg(colors.key_fg)
        .bg(colors.tile_bg)
        .add_modifier(Modifier::BOLD);
    spans.push(Span::styled(
        String::from(" "),
        Style::default().bg(colors.tile_bg),
    ));
    spans.push(Span::styled(String::from("<"), bracket_style));
    spans.push(Span::styled(bare_key.to_string(), key_style));
    spans.push(Span::styled(String::from(">"), bracket_style));
    spans.push(Span::styled(format!(" {label} "), bracket_style));
    if powerline {
        spans.push(Span::styled(
            POWERLINE_ARROW,
            Style::default().fg(colors.tile_bg).bg(colors.outer_bg),
        ));
    }
}

/// Append a section: an optional plain bold modifier prefix on `outer_bg`, then
/// the tiles, abutting so adjacent arrows form back-to-back wedges.
fn emit_section(
    spans: &mut Vec<Span<'static>>,
    hints: &[&Hint],
    prefix: Option<&str>,
    prefix_fg: Color,
    colors: &TileColors,
    powerline: bool,
) {
    if let Some(pfx) = prefix {
        spans.push(Span::styled(
            format!(" {pfx} "),
            Style::default()
                .fg(prefix_fg)
                .bg(colors.outer_bg)
                .add_modifier(Modifier::BOLD),
        ));
    }

    for hint in hints {
        let key = sanitize_key(&hint.key);
        let bare_key = if let Some(pfx) = prefix {
            strip_key_modifier(&key, pfx)
        } else {
            key.into_owned()
        };
        push_tile(spans, &bare_key, hint.label, colors, powerline);
    }
}

fn panel_contrast_fg(palette: &Palette) -> Color {
    match palette.panel_bg {
        Color::Reset => palette.surface_dim,
        color => color,
    }
}

/// Lay a hint set out in `width` columns. Tier 1 shows both sections; tier 2
/// drops the Alt section; tier 3 shows unprefixed tiles until one does not fit,
/// then ` …`. The badge is never dropped.
pub(super) fn build_hint_line(
    hint_set: &HintSet,
    style: HintBarStyleConfig,
    palette: &Palette,
    width: u16,
    powerline: bool,
) -> Line<'static> {
    let width = width as usize;
    let badge_color = match hint_set.badge.accent {
        BadgeColor::Accent => palette.accent,
        BadgeColor::Mauve => palette.mauve,
        BadgeColor::Locked => palette.peach,
    };
    let badge_style = Style::default()
        .fg(panel_contrast_fg(palette))
        .bg(badge_color)
        .add_modifier(Modifier::BOLD);

    let badge_text = format!(" {} ", hint_set.badge.label);
    let badge_width = badge_text.width();

    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled(badge_text, badge_style));

    if badge_width >= width {
        return Line::from(spans);
    }

    let left_hints: Vec<&Hint> = if style == HintBarStyleConfig::Compact {
        let mut sorted: Vec<&Hint> = hint_set.hints.iter().collect();
        sorted.sort_by_key(|h| h.priority);
        sorted.truncate(4);
        sorted
    } else {
        hint_set.hints.iter().collect()
    };

    let right_hints: Vec<&Hint> = hint_set.alt_hints.iter().collect();
    let has_right = !right_hints.is_empty();

    let remaining = width.saturating_sub(badge_width);

    let left_prefix = detect_section_prefix(&left_hints);
    let right_prefix = if has_right {
        detect_section_prefix(&right_hints)
    } else {
        None
    };

    // Low-color palettes can have surface0 == panel_bg; fall back to
    // surface_dim so the tile arrows stay visible.
    let tile_bg = if palette.surface0 == palette.panel_bg {
        palette.surface_dim
    } else {
        palette.surface0
    };
    let colors = TileColors {
        tile_bg,
        key_fg: palette.accent,
        label_fg: palette.overlay1,
        outer_bg: palette.panel_bg,
    };
    let left_prefix_fg = if palette.text == palette.panel_bg {
        palette.overlay1
    } else {
        palette.text
    };
    let right_prefix_fg = if palette.peach == palette.panel_bg {
        palette.overlay1
    } else {
        palette.peach
    };

    if has_right {
        let left_full = compute_section_width(&left_hints, left_prefix, powerline);
        let right_full = compute_section_width(&right_hints, right_prefix, powerline);

        // Tier 1: both sections, the Alt section flush right.
        if left_full + MIN_SECTION_GAP + right_full <= remaining {
            let whitespace = remaining - left_full - right_full;
            emit_section(
                &mut spans,
                &left_hints,
                left_prefix,
                left_prefix_fg,
                &colors,
                powerline,
            );
            spans.push(Span::raw(" ".repeat(whitespace)));
            emit_section(
                &mut spans,
                &right_hints,
                right_prefix,
                right_prefix_fg,
                &colors,
                powerline,
            );
            return Line::from(spans);
        }
    }

    // Tier 2 (or no right section): the left section only.
    let left_w = compute_section_width(&left_hints, left_prefix, powerline);
    if left_w <= remaining {
        emit_section(
            &mut spans,
            &left_hints,
            left_prefix,
            left_prefix_fg,
            &colors,
            powerline,
        );
        return Line::from(spans);
    }

    // Tier 3: no prefix, so each tile keeps its full key (`<ctrl+p>`).
    let dim_style = Style::default().fg(palette.overlay0);
    let mut used = badge_width;
    let arrow_cost: usize = if powerline { 2 } else { 0 };
    for hint in &left_hints {
        let bare_key = sanitize_key(&hint.key).into_owned();
        let entry_width = arrow_cost + 5 + bare_key.width() + hint.label.width();

        let ellipsis_width = 2; // " …"
        if used + entry_width + ellipsis_width > width && used + entry_width > width {
            if used + ellipsis_width <= width {
                spans.push(Span::styled(" \u{2026}", dim_style));
            }
            return Line::from(spans);
        }

        push_tile(&mut spans, &bare_key, hint.label, &colors, powerline);
        used += entry_width;
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyModifiers};

    use super::*;

    const PREFIX: &[(KeyCode, KeyModifiers)] = &[(KeyCode::Char('b'), KeyModifiers::CONTROL)];

    fn set_for(mode: ClientShellMode) -> HintSet {
        hints(mode, false, &Keybinds::default(), PREFIX)
    }

    fn normal() -> HintSet {
        set_for(ClientShellMode::Terminal)
    }

    fn labels(set: &HintSet) -> Vec<&'static str> {
        set.hints.iter().map(|hint| hint.label).collect()
    }

    fn key_for<'a>(set: &'a HintSet, label: &str) -> &'a str {
        set.hints
            .iter()
            .chain(&set.alt_hints)
            .find(|hint| hint.label == label)
            .unwrap_or_else(|| panic!("no {label:?} hint"))
            .key
            .as_ref()
    }

    fn text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn render(
        set: &HintSet,
        style: HintBarStyleConfig,
        width: u16,
        powerline: bool,
    ) -> Line<'static> {
        build_hint_line(set, style, &Palette::catppuccin(), width, powerline)
    }

    fn hint(key: &'static str, label: &'static str) -> Hint {
        Hint {
            key: Cow::Borrowed(key),
            label,
            priority: 0,
        }
    }

    fn badge() -> Badge {
        Badge {
            label: "T",
            accent: BadgeColor::Accent,
        }
    }

    #[test]
    fn normal_mode_shows_mode_entry_keys() {
        let set = normal();
        assert_eq!(set.badge.label, "NORMAL");
        assert_eq!(
            labels(&set),
            vec!["PANE", "TAB", "RESIZE", "MOVE", "SESSION", "LOCK"]
        );
        assert_eq!(key_for(&set, "PANE"), "ctrl+p");
        assert_eq!(key_for(&set, "LOCK"), "ctrl+g");
    }

    #[test]
    fn navigate_has_its_own_badge() {
        let set = set_for(ClientShellMode::Navigate);
        assert_eq!(set.badge.label, "NAVIGATE");
        assert_eq!(key_for(&set, "workspace"), "up/down");
        assert!(labels(&set).contains(&"back"));
    }

    #[test]
    fn pane_mode_hints_follow_the_pane_table() {
        let set = set_for(ClientShellMode::Modal(ModalMode::Pane));
        assert_eq!(set.badge.label, "PANE");
        assert_eq!(set.badge.accent, BadgeColor::Mauve);
        assert_eq!(key_for(&set, "focus"), "h/j/k/l");
        assert_eq!(key_for(&set, "split"), "n/d/r");
        for label in ["stack", "close", "zoom", "rename", "next pane", "exit"] {
            assert!(labels(&set).contains(&label), "{label}");
        }
    }

    #[test]
    fn tab_mode_hints() {
        let set = set_for(ClientShellMode::Modal(ModalMode::Tab));
        assert_eq!(set.badge.label, "TAB");
        for label in [
            "previous tab",
            "next tab",
            "new tab",
            "close tab",
            "rename tab",
            "last pane",
        ] {
            assert!(labels(&set).contains(&label), "{label}");
        }
        assert_eq!(key_for(&set, "goto tab"), "1-9");
        assert!(!labels(&set).iter().any(|label| label.contains("break")));
    }

    #[test]
    fn resize_mode_hints_show_shifted_keys_in_capitals() {
        let set = set_for(ClientShellMode::Resize);
        assert_eq!(set.badge.label, "RESIZE");
        assert_eq!(key_for(&set, "increase"), "h/j/k/l");
        assert_eq!(key_for(&set, "decrease"), "H/J/K/L");
        assert_eq!(key_for(&set, "grow"), "+");
        assert_eq!(key_for(&set, "shrink"), "-");
        assert!(labels(&set).contains(&"exit"));
    }

    #[test]
    fn move_mode_hints() {
        let set = set_for(ClientShellMode::Modal(ModalMode::Move));
        assert_eq!(set.badge.label, "MOVE");
        assert_eq!(key_for(&set, "swap"), "h/j/k/l");
        assert!(labels(&set).contains(&"exit"));
    }

    #[test]
    fn session_hints_contain_expected_actions() {
        let set = set_for(ClientShellMode::Modal(ModalMode::Session));
        assert_eq!(set.badge.label, "SESSION");
        assert_eq!(key_for(&set, "workspace"), "k/j");
        assert_eq!(key_for(&set, "new"), "n/N");
        for label in ["navigator", "detach", "help", "goto tab", "open", "exit"] {
            assert!(labels(&set).contains(&label), "{label}");
        }
    }

    #[test]
    fn locked_mode_hints() {
        let set = hints(
            ClientShellMode::Terminal,
            true,
            &Keybinds::default(),
            PREFIX,
        );
        assert_eq!(set.badge.label, "LOCKED");
        assert_eq!(set.badge.accent, BadgeColor::Locked);
        assert_eq!(key_for(&set, "unlock"), "ctrl+g");
        assert!(labels(&set).contains(&"keys pass through"));
    }

    #[test]
    fn locked_unbound_unlock_key_is_honest() {
        let mut keybinds = Keybinds::default();
        keybinds.modal.entry.locked = ActionKeybinds::default();
        let set = hints(ClientShellMode::Terminal, true, &keybinds, PREFIX);
        assert_eq!(key_for(&set, "unlock"), "(unbound)");
    }

    #[test]
    fn prefix_hints_come_from_the_live_keymap() {
        let set = set_for(ClientShellMode::Prefix);
        assert_eq!(set.badge.label, "PREFIX");
        assert_eq!(key_for(&set, "send prefix"), "ctrl+b");
        assert_eq!(key_for(&set, "workspace nav"), "w");
        assert_eq!(key_for(&set, "keybinds"), "?");
    }

    #[test]
    fn prefix_hints_show_the_primary_of_several_prefixes() {
        let prefixes = [
            (KeyCode::Char('b'), KeyModifiers::CONTROL),
            (KeyCode::Char('s'), KeyModifiers::CONTROL),
        ];
        let set = hints(
            ClientShellMode::Prefix,
            false,
            &Keybinds::default(),
            &prefixes,
        );
        assert_eq!(key_for(&set, "send prefix"), "ctrl+b");
    }

    #[test]
    fn all_modes_produce_nonempty_hints() {
        for mode in [
            ClientShellMode::Terminal,
            ClientShellMode::Prefix,
            ClientShellMode::Navigate,
            ClientShellMode::Resize,
            ClientShellMode::Copy,
            ClientShellMode::Modal(ModalMode::Pane),
            ClientShellMode::Modal(ModalMode::Tab),
            ClientShellMode::Modal(ModalMode::Move),
            ClientShellMode::Modal(ModalMode::Session),
        ] {
            assert!(!set_for(mode).hints.is_empty(), "{mode:?}");
        }
    }

    #[test]
    fn a_remapped_mode_key_shows_in_its_hints() {
        let config: crate::config::Config = toml::from_str("[keys.pane]\nstack = \"q\"\n").unwrap();
        let set = hints(
            ClientShellMode::Modal(ModalMode::Pane),
            false,
            &config.keybinds(),
            PREFIX,
        );
        assert_eq!(key_for(&set, "stack"), "q");
    }

    #[test]
    fn compact_selects_top_four_by_priority() {
        let set = set_for(ClientShellMode::Modal(ModalMode::Session));
        let text = text(&render(&set, HintBarStyleConfig::Compact, 200, true));
        assert!(text.contains("SESSION"));
        for hint in set.hints.iter().filter(|hint| hint.priority < 4) {
            assert!(text.contains(hint.label), "compact missing {}", hint.label);
        }
        assert!(!text.contains("exit"), "compact keeps only four: {text}");
    }

    #[test]
    fn truncation_appends_ellipsis() {
        let set = set_for(ClientShellMode::Modal(ModalMode::Session));
        let text = text(&render(&set, HintBarStyleConfig::Full, 30, true));
        assert!(text.contains("SESSION"));
        assert!(text.contains('\u{2026}'));
    }

    #[test]
    fn badge_never_dropped_at_tiny_width() {
        let set = set_for(ClientShellMode::Modal(ModalMode::Session));
        assert!(text(&render(&set, HintBarStyleConfig::Full, 5, true)).contains("SESSION"));
    }

    #[test]
    fn display_column_width_accounting() {
        let set = HintSet {
            badge: badge(),
            hints: vec![hint("\u{4e16}", "x")],
            alt_hints: Vec::new(),
        };
        let text = text(&render(&set, HintBarStyleConfig::Full, 7, true));
        assert!(text.contains('\u{2026}') || !text.contains('\u{4e16}'));
    }

    #[test]
    fn sanitize_strips_bidi_and_control() {
        assert_eq!(sanitize_key("a\u{202e}b\x01c").as_ref(), "abc");
    }

    #[test]
    fn build_hint_line_sanitizes_key_strings() {
        let set = HintSet {
            badge: badge(),
            hints: vec![hint("a\u{202e}b\x01c", "x")],
            alt_hints: Vec::new(),
        };
        let text = text(&render(&set, HintBarStyleConfig::Full, 200, true));
        assert!(!text.contains('\u{202e}') && !text.contains('\x01'));
        assert!(text.contains("abc"));
    }

    #[test]
    fn alt_section_sanitizes_bidi_control_chars() {
        let set = HintSet {
            badge: badge(),
            hints: vec![hint("x", "left")],
            alt_hints: vec![hint("alt+\u{202e}h", "FOC")],
        };
        let text = text(&render(&set, HintBarStyleConfig::Full, 200, true));
        assert!(!text.contains('\u{202e}'));
        assert!(text.contains("Alt +"), "{text}");
        assert!(text.contains("<h>"), "{text}");
    }

    #[test]
    fn terminal_mode_has_alt_hints() {
        let set = normal();
        let alt: Vec<_> = set.alt_hints.iter().map(|hint| hint.label).collect();
        assert_eq!(alt, vec!["FOCUS", "SPLIT", "CLOSE", "RESIZE", "MOV TAB"]);
        assert_eq!(key_for(&set, "FOCUS"), "alt+h/alt+j/alt+k/alt+l");
        assert_eq!(key_for(&set, "SPLIT"), "alt+n");
        assert_eq!(key_for(&set, "MOV TAB"), "alt+i/alt+o");
    }

    #[test]
    fn alt_hint_many_binding_selects_alt_alternative() {
        assert_eq!(key_for(&normal(), "CLOSE"), "alt+x");
    }

    #[test]
    fn alt_hint_dropped_when_no_alt_alternative() {
        let keybinds = Keybinds {
            focus_pane_left: ActionKeybinds::prefix("h"),
            focus_pane_down: ActionKeybinds::prefix("j"),
            focus_pane_up: ActionKeybinds::prefix("k"),
            focus_pane_right: ActionKeybinds::prefix("l"),
            ..Keybinds::default()
        };
        let set = hints(ClientShellMode::Terminal, false, &keybinds, PREFIX);
        assert!(set.alt_hints.iter().all(|hint| hint.label != "FOCUS"));
    }

    #[test]
    fn non_terminal_modes_have_no_alt_hints() {
        for mode in [
            ClientShellMode::Prefix,
            ClientShellMode::Navigate,
            ClientShellMode::Resize,
            ClientShellMode::Modal(ModalMode::Pane),
            ClientShellMode::Modal(ModalMode::Tab),
            ClientShellMode::Modal(ModalMode::Move),
            ClientShellMode::Modal(ModalMode::Session),
        ] {
            assert!(set_for(mode).alt_hints.is_empty(), "{mode:?}");
        }
        let locked = hints(
            ClientShellMode::Terminal,
            true,
            &Keybinds::default(),
            PREFIX,
        );
        assert!(locked.alt_hints.is_empty());
    }

    #[test]
    fn degradation_tier1_full_labels_both_sections() {
        let text = text(&render(&normal(), HintBarStyleConfig::Full, 200, true));
        for needle in ["NORMAL", "PANE", "FOCUS", "SPLIT"] {
            assert!(text.contains(needle), "{needle}: {text}");
        }
    }

    #[test]
    fn degradation_tier2_alt_section_dropped() {
        let set = normal();
        let left: Vec<&Hint> = set.hints.iter().collect();
        let right: Vec<&Hint> = set.alt_hints.iter().collect();
        let left_full = compute_section_width(&left, detect_section_prefix(&left), true);
        let right_full = compute_section_width(&right, detect_section_prefix(&right), true);
        let badge_width = format!(" {} ", set.badge.label).width();
        let width = (badge_width + left_full + MIN_SECTION_GAP + right_full - 1) as u16;
        let text = text(&render(&set, HintBarStyleConfig::Full, width, true));
        assert!(text.contains("NORMAL"));
        assert!(!text.contains("FOCUS"), "{text}");
        assert!(text.contains("<p>") && text.contains("PANE"), "{text}");
    }

    #[test]
    fn degradation_tier3_ellipsis_on_left() {
        let text = text(&render(&normal(), HintBarStyleConfig::Full, 30, true));
        assert!(text.contains("NORMAL"));
        assert!(text.contains('\u{2026}'));
        assert!(!text.contains("FOCUS"));
        assert!(
            text.contains("ctrl+"),
            "modifier stays visible in tier 3: {text}"
        );
    }

    #[test]
    fn compact_style_uses_top_four_keys_at_full_uppercase() {
        let text = text(&render(&normal(), HintBarStyleConfig::Compact, 200, true));
        for needle in ["NORMAL", "PANE", "TAB", "RESIZE", "MOVE"] {
            assert!(text.contains(needle), "{needle}: {text}");
        }
        assert!(
            !text.contains("SESSION") && !text.contains("LOCK"),
            "{text}"
        );
    }

    #[test]
    fn no_lowercase_short_label_ever_renders() {
        let set = normal();
        for style in [HintBarStyleConfig::Full, HintBarStyleConfig::Compact] {
            for width in 0u16..=240 {
                let text = text(&render(&set, style, width, true));
                for short in ["rsz", "mov", "ses", "lck", "foc", "spl", "cls"] {
                    assert!(!text.contains(short), "width {width}: {text}");
                }
            }
        }
    }

    #[test]
    fn no_overlap_at_any_width() {
        for mode in [
            ClientShellMode::Terminal,
            ClientShellMode::Modal(ModalMode::Session),
            ClientShellMode::Resize,
        ] {
            let set = set_for(mode);
            let badge_width = format!(" {} ", set.badge.label).width() as u16;
            for style in [HintBarStyleConfig::Full, HintBarStyleConfig::Compact] {
                for powerline in [true, false] {
                    for width in badge_width..=200 {
                        let line = render(&set, style, width, powerline);
                        let total: usize = line.spans.iter().map(|span| span.content.width()).sum();
                        assert!(total <= usize::from(width), "{mode:?} width {width}");
                    }
                }
            }
        }
    }

    #[test]
    fn no_overlap_with_wide_remapped_alt_labels() {
        let mut set = normal();
        set.alt_hints = vec![
            hint("alt+shift+pageup/alt+shift+pagedown", "FOCUS"),
            hint("ctrl+alt+backspace", "CLOSE"),
        ];
        let badge_width = format!(" {} ", set.badge.label).width() as u16;
        for width in badge_width..=200 {
            let line = render(&set, HintBarStyleConfig::Full, width, true);
            let total: usize = line.spans.iter().map(|span| span.content.width()).sum();
            assert!(total <= usize::from(width), "width {width}");
        }
    }

    #[test]
    fn two_sections_keep_minimum_gap() {
        let line = render(&normal(), HintBarStyleConfig::Full, 200, true);
        let max_blank = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .filter(|content| content.chars().all(|ch| ch == ' '))
            .map(UnicodeWidthStr::width)
            .max()
            .unwrap_or(0);
        assert!(max_blank >= MIN_SECTION_GAP);
    }

    #[test]
    fn ribbon_uses_bracketed_key_format() {
        let text = text(&render(&normal(), HintBarStyleConfig::Full, 200, true));
        assert!(text.contains("<p>"), "{text}");
        assert!(!text.contains("ctrl+p"), "{text}");
    }

    #[test]
    fn ribbon_bracket_fg_distinct_from_key_fg() {
        let palette = Palette::catppuccin();
        let line = render(&normal(), HintBarStyleConfig::Full, 200, true);
        let spans = &line.spans;
        let mut pairs = 0;
        let mut i = 0;
        while i + 2 < spans.len() {
            if spans[i].content.as_ref() == "<" && spans[i + 2].content.as_ref() == ">" {
                assert_eq!(spans[i].style.fg, Some(palette.overlay1));
                assert_eq!(spans[i + 2].style.fg, Some(palette.overlay1));
                assert_eq!(spans[i + 1].style.fg, Some(palette.accent));
                assert!(spans[i + 1].style.add_modifier.contains(Modifier::BOLD));
                assert!(!spans[i].style.add_modifier.contains(Modifier::BOLD));
                pairs += 1;
                i += 3;
            } else {
                i += 1;
            }
        }
        assert!(pairs >= 4, "found {pairs}");
        assert_ne!(palette.overlay1, palette.accent);
    }

    #[test]
    fn ribbon_single_prefix_per_section() {
        let text = text(&render(&normal(), HintBarStyleConfig::Full, 200, true));
        assert_eq!(text.matches("Ctrl +").count(), 1, "{text}");
        assert_eq!(text.matches("Alt +").count(), 1, "{text}");
    }

    #[test]
    fn ribbon_powerline_arrows_follow_the_setting() {
        let on = text(&render(&normal(), HintBarStyleConfig::Full, 200, true));
        assert!(on.contains(POWERLINE_ARROW));
        let off = text(&render(&normal(), HintBarStyleConfig::Full, 200, false));
        assert!(!off.contains(POWERLINE_ARROW));
    }

    #[test]
    fn ribbon_arrows_visible_with_low_color_palette() {
        let line = build_hint_line(
            &normal(),
            HintBarStyleConfig::Full,
            &Palette::terminal(),
            200,
            true,
        );
        for span in line
            .spans
            .iter()
            .filter(|span| span.content.contains(POWERLINE_ARROW))
        {
            assert_ne!(span.style.fg, span.style.bg);
        }
    }

    #[test]
    fn ribbon_tiles_produce_back_to_back_wedge_separators() {
        let palette = Palette::catppuccin();
        let line = render(&normal(), HintBarStyleConfig::Full, 200, true);
        let arrows: Vec<_> = line
            .spans
            .iter()
            .filter(|span| span.content.as_ref() == POWERLINE_ARROW)
            .collect();
        assert!(arrows.len() >= 4);
        for pair in arrows.chunks(2) {
            assert_eq!(pair[0].style.fg, Some(palette.panel_bg));
            assert_eq!(pair[0].style.bg, Some(palette.surface0));
            assert_eq!(pair[1].style.fg, Some(palette.surface0));
            assert_eq!(pair[1].style.bg, Some(palette.panel_bg));
        }
    }

    #[test]
    fn ribbon_prefix_is_plain_text_on_every_palette() {
        for palette in [
            Palette::catppuccin(),
            Palette::catppuccin_latte(),
            Palette::tokyo_night(),
            Palette::tokyo_night_day(),
            Palette::dracula(),
            Palette::nord(),
            Palette::gruvbox(),
            Palette::gruvbox_light(),
            Palette::solarized(),
            Palette::solarized_light(),
            Palette::terminal(),
        ] {
            let line = build_hint_line(&normal(), HintBarStyleConfig::Full, &palette, 200, true);
            for needle in [" Ctrl + ", " Alt + "] {
                let span = line
                    .spans
                    .iter()
                    .find(|span| span.content.as_ref() == needle)
                    .unwrap_or_else(|| panic!("missing {needle:?}"));
                assert_eq!(span.style.bg, Some(palette.panel_bg));
                assert_ne!(span.style.fg, span.style.bg);
            }
        }
    }

    #[test]
    fn reserve_row_takes_the_bottom_row_only_when_panes_keep_one() {
        let panes = Rect::new(3, 1, 50, 10);
        assert_eq!(
            reserve_row(HintBarStyleConfig::Full, panes),
            (Rect::new(3, 1, 50, 9), Rect::new(3, 10, 50, 1))
        );
        assert_eq!(
            reserve_row(HintBarStyleConfig::Off, panes),
            (panes, Rect::default())
        );
        let short = Rect::new(0, 0, 50, 1);
        assert_eq!(
            reserve_row(HintBarStyleConfig::Compact, short),
            (short, Rect::default())
        );
    }
}
