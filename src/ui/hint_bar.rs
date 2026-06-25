use std::borrow::Cow;

use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
    Frame,
};
use unicode_width::UnicodeWidthStr;

use super::widgets::panel_contrast_fg;
use crate::app::state::{Mode, Palette};
use crate::app::AppState;
use crate::config::{format_key_combo, ActionKeybinds, HintBarStyle, Keybinds, ModeBinding};

/// Minimum blank columns kept between the left and right hint sections so they
/// stay visually distinct and never touch.
const MIN_SECTION_GAP: usize = 2;

pub struct Hint {
    pub key: Cow<'static, str>,
    pub label: &'static str,
    // No longer consumed since zellij-fidelity round 2 (labels always render
    // full uppercase, no lowercase short fallback tier). Kept on the struct so
    // the many `hints()` constructor sites don't churn this round; removed in a
    // separate cleanup pass.
    #[allow(dead_code)]
    pub short: &'static str,
    pub priority: u8,
}

#[derive(Clone, Copy)]
enum BadgeColor {
    Accent,
    Mauve,
    Locked,
}

pub struct Badge {
    pub label: &'static str,
    accent: BadgeColor,
}

pub struct HintSet {
    pub badge: Badge,
    pub hints: Vec<Hint>,
    pub alt_hints: Vec<Hint>,
}

fn mode_binding_label(binding: &ModeBinding) -> Option<Cow<'static, str>> {
    binding
        .first()
        .map(|&combo| Cow::Owned(format_key_combo(combo)))
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

fn alt_binding_label(bindings: &ActionKeybinds) -> Option<Cow<'static, str>> {
    bindings.alt_direct_label().map(Cow::Owned)
}

fn resolve_prefix_rhs(bindings: &ActionKeybinds) -> Option<Cow<'static, str>> {
    bindings.prefix_rhs_label().map(Cow::Owned)
}

fn terminal_alt_hints(kb: &Keybinds) -> Vec<Hint> {
    let mut alt_hints = Vec::new();

    let focus_labels: Vec<String> = [
        &kb.focus_pane_left,
        &kb.focus_pane_down,
        &kb.focus_pane_up,
        &kb.focus_pane_right,
    ]
    .iter()
    .filter_map(|b| b.alt_direct_label())
    .collect();
    if !focus_labels.is_empty() {
        let combined = focus_labels.join("/");
        alt_hints.push(Hint {
            key: Cow::Owned(combined),
            label: "FOCUS",
            short: "foc",
            priority: 0,
        });
    }

    if let Some(key) = alt_binding_label(&kb.split_auto) {
        alt_hints.push(Hint {
            key,
            label: "SPLIT",
            short: "spl",
            priority: 1,
        });
    }

    if let Some(key) = alt_binding_label(&kb.close_pane) {
        alt_hints.push(Hint {
            key,
            label: "CLOSE",
            short: "cls",
            priority: 2,
        });
    }

    let resize_labels: Vec<String> = [&kb.resize_grow, &kb.resize_shrink]
        .iter()
        .filter_map(|b| b.alt_direct_label())
        .collect();
    if !resize_labels.is_empty() {
        let combined = resize_labels.join("/");
        alt_hints.push(Hint {
            key: Cow::Owned(combined),
            label: "RESIZE",
            short: "rsz",
            priority: 3,
        });
    }

    let move_labels: Vec<String> = [&kb.move_tab_left, &kb.move_tab_right]
        .iter()
        .filter_map(|b| b.alt_direct_label())
        .collect();
    if !move_labels.is_empty() {
        let combined = move_labels.join("/");
        alt_hints.push(Hint {
            key: Cow::Owned(combined),
            label: "MOV TAB",
            short: "mov",
            priority: 4,
        });
    }

    alt_hints
}

fn terminal_hints(kb: &Keybinds) -> HintSet {
    let mut hints = Vec::new();
    let entry = &kb.mode_entry;

    if let Some(key) = entry.pane.map(format_key_combo).map(Cow::Owned) {
        hints.push(Hint {
            key,
            label: "PANE",
            short: "pane",
            priority: 0,
        });
    }
    if let Some(key) = entry.tab.map(format_key_combo).map(Cow::Owned) {
        hints.push(Hint {
            key,
            label: "TAB",
            short: "tab",
            priority: 1,
        });
    }
    if let Some(key) = entry.resize.map(format_key_combo).map(Cow::Owned) {
        hints.push(Hint {
            key,
            label: "RESIZE",
            short: "rsz",
            priority: 2,
        });
    }
    if let Some(key) = entry.move_.map(format_key_combo).map(Cow::Owned) {
        hints.push(Hint {
            key,
            label: "MOVE",
            short: "mov",
            priority: 3,
        });
    }
    if let Some(key) = entry.session.map(format_key_combo).map(Cow::Owned) {
        hints.push(Hint {
            key,
            label: "SESSION",
            short: "ses",
            priority: 4,
        });
    }
    if let Some(key) = entry.locked.map(format_key_combo).map(Cow::Owned) {
        hints.push(Hint {
            key,
            label: "LOCK",
            short: "lck",
            priority: 5,
        });
    }

    let alt_hints = terminal_alt_hints(kb);

    HintSet {
        badge: Badge {
            label: "NORMAL",
            accent: BadgeColor::Accent,
        },
        hints,
        alt_hints,
    }
}

fn session_hints(kb: &Keybinds) -> HintSet {
    let b = &kb.mode_session;
    let mut hints = Vec::new();

    if let (Some(up), Some(down)) = (
        mode_binding_label(&b.workspace_up),
        mode_binding_label(&b.workspace_down),
    ) {
        hints.push(Hint {
            key: Cow::Owned(format!("{up}/{down}")),
            label: "ws",
            short: "ws",
            priority: 0,
        });
    }
    if let Some(key) = mode_binding_label(&b.goto) {
        hints.push(Hint {
            key,
            label: "navigator",
            short: "goto",
            priority: 1,
        });
    }
    if let Some(key) = mode_binding_label(&b.new_workspace) {
        hints.push(Hint {
            key,
            label: "new ws",
            short: "new",
            priority: 2,
        });
    }
    if let Some(key) = mode_binding_label(&b.close_workspace) {
        hints.push(Hint {
            key,
            label: "close",
            short: "cls",
            priority: 3,
        });
    }
    if let Some(key) = mode_binding_label(&b.detach) {
        hints.push(Hint {
            key,
            label: "detach",
            short: "det",
            priority: 4,
        });
    }
    if let Some(key) = mode_binding_label(&b.help) {
        hints.push(Hint {
            key,
            label: "keybinds",
            short: "keys",
            priority: 5,
        });
    }
    hints.push(Hint {
        key: Cow::Borrowed("esc"),
        label: "exit",
        short: "exit",
        priority: 6,
    });

    HintSet {
        badge: Badge {
            label: "SESSION",
            accent: BadgeColor::Mauve,
        },
        hints,
        alt_hints: Vec::new(),
    }
}

fn pane_hints(kb: &Keybinds) -> HintSet {
    let b = &kb.mode_pane;
    let mut hints = Vec::new();

    hints.push(Hint {
        key: Cow::Borrowed("h/j/k/l"),
        label: "focus",
        short: "foc",
        priority: 0,
    });
    if let Some(key) = mode_binding_label(&b.new_pane) {
        hints.push(Hint {
            key,
            label: "new",
            short: "new",
            priority: 1,
        });
    }
    if let Some(key) = mode_binding_label(&b.close) {
        hints.push(Hint {
            key,
            label: "close",
            short: "cls",
            priority: 2,
        });
    }
    if let Some(key) = mode_binding_label(&b.zoom) {
        hints.push(Hint {
            key,
            label: "zoom",
            short: "zm",
            priority: 3,
        });
    }
    if let Some(key) = mode_binding_label(&b.split_down) {
        hints.push(Hint {
            key,
            label: "split\u{2500}",
            short: "\u{2500}",
            priority: 4,
        });
    }
    if let Some(key) = mode_binding_label(&b.split_right) {
        hints.push(Hint {
            key,
            label: "split\u{2502}",
            short: "\u{2502}",
            priority: 5,
        });
    }
    hints.push(Hint {
        key: Cow::Borrowed("esc"),
        label: "exit",
        short: "exit",
        priority: 6,
    });

    HintSet {
        badge: Badge {
            label: "PANE",
            accent: BadgeColor::Mauve,
        },
        hints,
        alt_hints: Vec::new(),
    }
}

fn tab_hints(kb: &Keybinds) -> HintSet {
    let b = &kb.mode_tab;
    let mut hints = Vec::new();

    hints.push(Hint {
        key: Cow::Borrowed("h/l"),
        label: "prev/next",
        short: "nav",
        priority: 0,
    });
    if let Some(key) = mode_binding_label(&b.new) {
        hints.push(Hint {
            key,
            label: "new",
            short: "new",
            priority: 1,
        });
    }
    if let Some(key) = mode_binding_label(&b.close) {
        hints.push(Hint {
            key,
            label: "close",
            short: "cls",
            priority: 2,
        });
    }
    if let Some(key) = mode_binding_label(&b.rename) {
        hints.push(Hint {
            key,
            label: "rename",
            short: "ren",
            priority: 3,
        });
    }
    if let Some(key) = mode_binding_label(&b.break_to_tab) {
        hints.push(Hint {
            key,
            label: "break",
            short: "brk",
            priority: 4,
        });
    }
    hints.push(Hint {
        key: Cow::Borrowed("1-9"),
        label: "goto",
        short: "go",
        priority: 5,
    });
    hints.push(Hint {
        key: Cow::Borrowed("esc"),
        label: "exit",
        short: "exit",
        priority: 6,
    });

    HintSet {
        badge: Badge {
            label: "TAB",
            accent: BadgeColor::Mauve,
        },
        hints,
        alt_hints: Vec::new(),
    }
}

fn resize_hints(kb: &Keybinds) -> HintSet {
    let b = &kb.mode_resize;
    let mut hints = vec![
        Hint {
            key: Cow::Borrowed("h/j/k/l"),
            label: "increase",
            short: "+",
            priority: 0,
        },
        Hint {
            key: Cow::Borrowed("H/J/K/L"),
            label: "decrease",
            short: "-",
            priority: 1,
        },
    ];

    if let (Some(grow), Some(shrink)) = (
        mode_binding_label(&b.increase),
        mode_binding_label(&b.decrease),
    ) {
        hints.push(Hint {
            key: Cow::Owned(format!("{grow}/{shrink}")),
            label: "grow/shrink",
            short: "sz",
            priority: 2,
        });
    }
    hints.push(Hint {
        key: Cow::Borrowed("esc"),
        label: "exit",
        short: "exit",
        priority: 3,
    });

    HintSet {
        badge: Badge {
            label: "RESIZE",
            accent: BadgeColor::Mauve,
        },
        hints,
        alt_hints: Vec::new(),
    }
}

fn move_hints(kb: &Keybinds) -> HintSet {
    let b = &kb.mode_move;
    let mut hints = Vec::new();

    hints.push(Hint {
        key: Cow::Borrowed("h/j/k/l"),
        label: "swap",
        short: "swp",
        priority: 0,
    });
    if let Some(key) = mode_binding_label(&b.cycle_forward) {
        hints.push(Hint {
            key,
            label: "cycle",
            short: "cyc",
            priority: 1,
        });
    }
    hints.push(Hint {
        key: Cow::Borrowed("esc"),
        label: "exit",
        short: "exit",
        priority: 2,
    });

    HintSet {
        badge: Badge {
            label: "MOVE",
            accent: BadgeColor::Mauve,
        },
        hints,
        alt_hints: Vec::new(),
    }
}

fn locked_hints(kb: &Keybinds) -> HintSet {
    let mut hints = Vec::new();

    let unlock = match kb.mode_entry.locked {
        Some(combo) => Cow::Owned(format_key_combo(combo)),
        None => Cow::Borrowed("(unbound)"),
    };
    hints.push(Hint {
        key: unlock,
        label: "unlock",
        short: "unlock",
        priority: 0,
    });
    hints.push(Hint {
        key: Cow::Borrowed("*"),
        label: "keys pass through",
        short: "pass",
        priority: 1,
    });

    HintSet {
        badge: Badge {
            label: "LOCKED",
            accent: BadgeColor::Locked,
        },
        hints,
        alt_hints: Vec::new(),
    }
}

fn prefix_hints(kb: &Keybinds) -> HintSet {
    let mut hints = Vec::new();

    hints.push(Hint {
        key: Cow::Borrowed("esc"),
        label: "cancel",
        short: "esc",
        priority: 0,
    });

    if let Some(key) = resolve_prefix_rhs(&kb.workspace_picker) {
        hints.push(Hint {
            key,
            label: "workspace nav",
            short: "ws",
            priority: 1,
        });
    }
    if let Some(key) = resolve_prefix_rhs(&kb.help) {
        hints.push(Hint {
            key,
            label: "keybinds",
            short: "keys",
            priority: 2,
        });
    }

    HintSet {
        badge: Badge {
            label: "PREFIX",
            accent: BadgeColor::Accent,
        },
        hints,
        alt_hints: Vec::new(),
    }
}

fn copy_hints() -> HintSet {
    HintSet {
        badge: Badge {
            label: "COPY",
            accent: BadgeColor::Accent,
        },
        hints: vec![
            Hint {
                key: Cow::Borrowed("h/j/k/l w/b/e { }"),
                label: "move",
                short: "mov",
                priority: 0,
            },
            Hint {
                key: Cow::Borrowed("v/space"),
                label: "select",
                short: "sel",
                priority: 1,
            },
            Hint {
                key: Cow::Borrowed("y/enter"),
                label: "copy",
                short: "cp",
                priority: 2,
            },
            Hint {
                key: Cow::Borrowed("q/esc"),
                label: "exit",
                short: "exit",
                priority: 3,
            },
        ],
        alt_hints: Vec::new(),
    }
}

pub fn hints(mode: Mode, kb: &Keybinds) -> HintSet {
    match mode {
        Mode::Terminal => terminal_hints(kb),
        Mode::Pane => pane_hints(kb),
        Mode::Tab => tab_hints(kb),
        Mode::Resize => resize_hints(kb),
        Mode::Move => move_hints(kb),
        Mode::Session | Mode::Navigate => session_hints(kb),
        Mode::Locked => locked_hints(kb),
        Mode::Prefix => prefix_hints(kb),
        Mode::Copy => copy_hints(),
        _ => terminal_hints(kb),
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

/// Shared color bundle for hint-bar tile rendering. The `key_fg`/`label_fg`/
/// `tile_bg` triple styles the interior of each `<key> LABEL` tile; the
/// `arrow_fg_outside`/`arrow_bg_outside` are always the panel background — each
/// tile owns its own left+right arrow that blends against that outer bg (zellij
/// convention).
#[derive(Clone, Copy)]
struct TileColors {
    tile_bg: ratatui::style::Color,
    key_fg: ratatui::style::Color,
    label_fg: ratatui::style::Color,
    /// Outer bg that the per-tile arrows blend against (`panel_bg`).
    outer_bg: ratatui::style::Color,
}

/// The Powerline "right arrow" used between hint-bar tiles (same glyph as
/// `tabs::POWERLINE_ARROW`). Each tile carries its own left and right arrow,
/// both blending against `outer_bg`.
const POWERLINE_ARROW: &str = "\u{e0b0}";

/// Width of a hint section: the modifier prefix text (when present) + each
/// tile's full footprint (2 arrows + ` <key> LABEL `). Tiles abut directly —
/// there is no inter-tile gap. Labels are ALWAYS the full uppercase
/// `hint.label`; the legacy lowercase `hint.short` is never consumed
/// (zellij-fidelity round 2 dropped the short-label tier).
fn compute_section_width(hints: &[&Hint], prefix: Option<&str>, powerline: bool) -> usize {
    if hints.is_empty() && prefix.is_none() {
        return 0;
    }
    let arrow_w: usize = if powerline { 2 } else { 0 };
    let mut total = 0usize;

    if let Some(pfx) = prefix {
        // Plain-text prefix ` Ctrl + ` (no colored bg). One leading space, the
        // text itself, and one trailing space before the first tile's arrow.
        total += 1 + pfx.width() + 1;
    }

    for hint in hints {
        let key = sanitize_key(&hint.key);
        let bare_key = if let Some(pfx) = prefix {
            strip_key_modifier(&key, pfx)
        } else {
            key.into_owned()
        };
        // Tile = [left arrow][ <key> label ][right arrow]
        //       = arrow_w + (1 + 1 + bare_key + 1 + 1 + label + 1) = arrow_w + 5 + key + label
        total += arrow_w + 5 + bare_key.width() + hint.label.width();
    }

    total
}

/// Append a single `<key> LABEL` tile (including its left+right arrows when
/// powerline is on) to `spans`.
fn push_tile(
    spans: &mut Vec<Span<'static>>,
    bare_key: &str,
    label: &str,
    colors: &TileColors,
    powerline: bool,
) {
    if powerline {
        // Left arrow: outer_bg → tile_bg
        spans.push(Span::styled(
            POWERLINE_ARROW,
            Style::default().fg(colors.outer_bg).bg(colors.tile_bg),
        ));
    }
    // Interior: " <key> label "
    // Split the bracketed key into three spans so only the key body gets the
    // accent highlight — the `<` and `>` brackets render in the dim label
    // color, matching zellij's `default-plugins/status-bar/src/first_line.rs`
    // (`char_left_separator` / `char_right_separator` styled with the
    // ribbon's `base` color, distinct from `emphasis_0` on the key shortcut).
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
        // Right arrow: tile_bg → outer_bg
        spans.push(Span::styled(
            POWERLINE_ARROW,
            Style::default().fg(colors.tile_bg).bg(colors.outer_bg),
        ));
    }
}

/// Append a section (optional plain-text modifier prefix + sequence of tiles)
/// to `spans`. The prefix renders as bold plain text on `outer_bg` (zellij's
/// `superkey` convention — NOT a colored ribbon segment); each tile abuts the
/// next directly so adjacent right+left arrows produce zellij's back-to-back
/// wedge separators. Labels are always full uppercase `hint.label`.
fn emit_section(
    spans: &mut Vec<Span<'static>>,
    hints: &[&Hint],
    prefix: Option<&str>,
    prefix_fg: ratatui::style::Color,
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

pub fn build_hint_line(
    hint_set: &HintSet,
    style: HintBarStyle,
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

    // `HintBarStyle::Compact` truncates the left section to the top-4 keys
    // by priority. Labels are ALWAYS full uppercase — Compact does not
    // switch to lowercase short forms (those were a herdr invention; zellij
    // never lowercases hint labels).
    let left_hints: Vec<&Hint> = if style == HintBarStyle::Compact {
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

    // Detect section prefixes
    let left_prefix = detect_section_prefix(&left_hints);
    let right_prefix = if has_right {
        detect_section_prefix(&right_hints)
    } else {
        None
    };

    // Each tile carries its own left+right Powerline arrows blending against
    // panel_bg (zellij convention). The `Ctrl +`/`Alt +` prefix renders as
    // plain bold text on panel_bg (zellij's `superkey_prefix`), not a colored
    // ribbon. Tiles abut directly so adjacent right+left arrows produce zellij's
    // signature back-to-back wedge separator. When `surface0 == panel_bg` (low-
    // color palettes where both default to `Reset`), fall back to `surface_dim`
    // so the per-tile arrows stay visible.
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
    // Prefix `Ctrl +`/`Alt +` foregrounds. Both must differ from `panel_bg`
    // so the text stays visible — on the low-color terminal palette where
    // `text == panel_bg == Reset`, fall back to `overlay1` (the bold fg used
    // by tile labels) so the prefix reads.
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

    // FR4 degradation (zellij-fidelity round 2: short-label tier removed):
    //   Tier 1: both sections at full uppercase labels.
    //   Tier 2: drop Alt section, left section at full uppercase labels.
    //   Tier 3: progressive ellipsis on left section (flat fallback).
    if has_right {
        let left_full = compute_section_width(&left_hints, left_prefix, powerline);
        let right_full = compute_section_width(&right_hints, right_prefix, powerline);
        let gap = MIN_SECTION_GAP;

        // Tier 1: both sections fit at full uppercase
        if left_full + gap + right_full <= remaining {
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

    // Tier 2 (or no right section): left only at full uppercase.
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

    // Tier 3: progressive ellipsis on left section. No prefix renders here,
    // so keep the FULL sanitized key (`<ctrl+p>`) rather than the stripped
    // `<p>` — otherwise the modifier the prefix would have shown is silently
    // lost. Tiles still use their own arrows so the visual pattern stays
    // consistent with the wider tiers.
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

pub fn hint_bar_active(app: &AppState) -> bool {
    app.hint_bar != HintBarStyle::Off && app.view.hint_bar_rect.height > 0
}

pub fn render_hint_bar(app: &AppState, frame: &mut Frame, area: Rect) {
    if area.height == 0 || app.hint_bar == HintBarStyle::Off {
        return;
    }

    let hint_set = hints(app.mode, &app.keybinds);
    let line = build_hint_line(
        &hint_set,
        app.hint_bar,
        &app.palette,
        area.width,
        app.tabs_powerline,
    );

    frame.render_widget(Clear, area);
    let buf = frame.buffer_mut();
    for x in area.x..area.x + area.width {
        buf[(x, area.y)].set_style(Style::default().bg(app.palette.panel_bg));
    }
    frame.render_widget(Paragraph::new(line), area);

    if matches!(app.mode, Mode::Navigate | Mode::Session) && app.update_available.is_some() {
        let status = Line::from(vec![Span::styled(
            " update ready",
            Style::default()
                .fg(app.palette.accent)
                .add_modifier(Modifier::BOLD),
        )]);
        let badge_width = 13u16.min(area.width);
        let status_area = Rect::new(
            area.x + area.width.saturating_sub(badge_width),
            area.y,
            badge_width,
            area.height,
        );
        frame.render_widget(Clear, status_area);
        frame.render_widget(
            Paragraph::new(status).alignment(Alignment::Right),
            status_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AppState;
    use crate::config::HintBarStyle;

    fn default_keybinds() -> Keybinds {
        let app = AppState::test_new();
        app.keybinds.clone()
    }

    #[test]
    fn normal_mode_shows_mode_entry_keys() {
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        assert_eq!(set.badge.label, "NORMAL");
        let labels: Vec<&str> = set.hints.iter().map(|h| h.label).collect();
        assert!(labels.contains(&"PANE"));
        assert!(labels.contains(&"TAB"));
        assert!(labels.contains(&"RESIZE"));
        assert!(labels.contains(&"MOVE"));
        assert!(labels.contains(&"SESSION"));
        assert!(labels.contains(&"LOCK"));
    }

    #[test]
    fn session_and_navigate_share_badge() {
        let kb = default_keybinds();
        let session_set = hints(Mode::Session, &kb);
        let navigate_set = hints(Mode::Navigate, &kb);
        assert_eq!(session_set.badge.label, "SESSION");
        assert_eq!(navigate_set.badge.label, "SESSION");
        assert_eq!(session_set.hints.len(), navigate_set.hints.len());
    }

    #[test]
    fn pane_mode_hints() {
        let kb = default_keybinds();
        let set = hints(Mode::Pane, &kb);
        assert_eq!(set.badge.label, "PANE");
        let labels: Vec<&str> = set.hints.iter().map(|h| h.label).collect();
        assert!(labels.contains(&"focus"));
        assert!(labels.contains(&"new"));
        assert!(labels.contains(&"close"));
        assert!(labels.contains(&"zoom"));
        assert!(labels.contains(&"exit"));
    }

    #[test]
    fn tab_mode_hints() {
        let kb = default_keybinds();
        let set = hints(Mode::Tab, &kb);
        assert_eq!(set.badge.label, "TAB");
        let labels: Vec<&str> = set.hints.iter().map(|h| h.label).collect();
        assert!(labels.contains(&"prev/next"));
        assert!(labels.contains(&"new"));
        assert!(labels.contains(&"close"));
        assert!(labels.contains(&"rename"));
        assert!(labels.contains(&"exit"));
    }

    #[test]
    fn resize_mode_hints() {
        let kb = default_keybinds();
        let set = hints(Mode::Resize, &kb);
        assert_eq!(set.badge.label, "RESIZE");
        let labels: Vec<&str> = set.hints.iter().map(|h| h.label).collect();
        assert!(labels.contains(&"increase"));
        assert!(labels.contains(&"decrease"));
        assert!(labels.contains(&"exit"));
    }

    #[test]
    fn move_mode_hints() {
        let kb = default_keybinds();
        let set = hints(Mode::Move, &kb);
        assert_eq!(set.badge.label, "MOVE");
        let labels: Vec<&str> = set.hints.iter().map(|h| h.label).collect();
        assert!(labels.contains(&"swap"));
        assert!(labels.contains(&"exit"));
    }

    #[test]
    fn locked_mode_hints() {
        let kb = default_keybinds();
        let set = hints(Mode::Locked, &kb);
        assert_eq!(set.badge.label, "LOCKED");
        let labels: Vec<&str> = set.hints.iter().map(|h| h.label).collect();
        assert!(labels.contains(&"unlock"));
        assert!(labels.contains(&"keys pass through"));
    }

    #[test]
    fn session_hints_contain_expected_actions() {
        let kb = default_keybinds();
        let set = hints(Mode::Session, &kb);
        assert_eq!(set.badge.label, "SESSION");
        assert!(!set.hints.is_empty());
        let labels: Vec<&str> = set.hints.iter().map(|h| h.label).collect();
        assert!(labels.contains(&"navigator"));
        assert!(labels.contains(&"detach"));
        assert!(labels.contains(&"keybinds"));
        assert!(labels.contains(&"exit"));
    }

    #[test]
    fn fallback_modes_return_normal_hints() {
        let kb = default_keybinds();
        let terminal_set = hints(Mode::Terminal, &kb);
        let settings_set = hints(Mode::Settings, &kb);
        assert_eq!(terminal_set.badge.label, "NORMAL");
        assert_eq!(settings_set.badge.label, "NORMAL");
        assert_eq!(terminal_set.hints.len(), settings_set.hints.len());
    }

    #[test]
    fn all_modes_produce_nonempty_hints() {
        let kb = default_keybinds();
        let modes = [
            Mode::Terminal,
            Mode::Navigate,
            Mode::Prefix,
            Mode::Resize,
            Mode::Copy,
            Mode::Pane,
            Mode::Tab,
            Mode::Move,
            Mode::Session,
            Mode::Locked,
            Mode::Onboarding,
            Mode::ReleaseNotes,
            Mode::ProductAnnouncement,
            Mode::RenameWorkspace,
            Mode::RenameTab,
            Mode::RenamePane,
            Mode::NewLinkedWorktree,
            Mode::OpenExistingWorktree,
            Mode::ConfirmRemoveWorktree,
            Mode::ConfirmClose,
            Mode::ContextMenu,
            Mode::Settings,
            Mode::GlobalMenu,
            Mode::KeybindHelp,
            Mode::Navigator,
        ];
        for mode in modes {
            let set = hints(mode, &kb);
            assert!(
                !set.hints.is_empty(),
                "Mode {:?} produced empty hints",
                mode
            );
        }
    }

    #[test]
    fn compact_selects_top_four_by_priority() {
        // Compact mode truncates to the top-4 keys by priority, then renders
        // them at full uppercase (no lowercase short fallback).
        let kb = default_keybinds();
        let set = hints(Mode::Session, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Compact, &palette, 200, true);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("SESSION"));
        let top_labels: Vec<&str> = set
            .hints
            .iter()
            .filter(|h| h.priority < 4)
            .map(|h| h.label)
            .collect();
        for label in top_labels {
            assert!(text.contains(label), "compact missing full label: {label}");
        }
    }

    #[test]
    fn truncation_appends_ellipsis() {
        let kb = default_keybinds();
        let set = hints(Mode::Session, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 30, true);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("SESSION"));
        assert!(text.contains('\u{2026}'));
    }

    #[test]
    fn badge_never_dropped_at_tiny_width() {
        let kb = default_keybinds();
        let set = hints(Mode::Session, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 5, true);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("SESSION"));
    }

    #[test]
    fn display_column_width_accounting() {
        let set = HintSet {
            badge: Badge {
                label: "T",
                accent: BadgeColor::Accent,
            },
            hints: vec![Hint {
                key: Cow::Borrowed("\u{4e16}"),
                label: "x",
                short: "x",
                priority: 0,
            }],
            alt_hints: Vec::new(),
        };
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 7, true);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains('\u{2026}') || !text.contains('\u{4e16}'),
            "wide char should be truncated or ellipsized"
        );
    }

    #[test]
    fn sanitize_strips_bidi_and_control() {
        let result = sanitize_key("a\u{202e}b\x01c");
        assert_eq!(result.as_ref(), "abc");
    }

    #[test]
    fn build_hint_line_sanitizes_key_strings() {
        let set = HintSet {
            badge: Badge {
                label: "T",
                accent: BadgeColor::Accent,
            },
            hints: vec![Hint {
                key: Cow::Borrowed("a\u{202e}b\x01c"),
                label: "x",
                short: "x",
                priority: 0,
            }],
            alt_hints: Vec::new(),
        };
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 200, true);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            !text.contains('\u{202e}'),
            "bidi override leaked into hint bar"
        );
        assert!(!text.contains('\x01'), "control char leaked into hint bar");
        assert!(text.contains("abc"));
    }

    #[test]
    fn locked_unbound_unlock_key_is_honest() {
        let mut kb = default_keybinds();
        kb.mode_entry.locked = None;
        let set = hints(Mode::Locked, &kb);
        let unlock = set.hints.iter().find(|h| h.label == "unlock").unwrap();
        assert_eq!(unlock.key.as_ref(), "(unbound)");
    }

    // --- Alt-shortcut section tests ---

    #[test]
    fn terminal_mode_has_alt_hints() {
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        assert!(!set.alt_hints.is_empty());
        let labels: Vec<&str> = set.alt_hints.iter().map(|h| h.label).collect();
        assert!(labels.contains(&"FOCUS"));
        assert!(labels.contains(&"SPLIT"));
        assert!(labels.contains(&"CLOSE"));
        assert!(labels.contains(&"RESIZE"));
        assert!(labels.contains(&"MOV TAB"));
    }

    #[test]
    fn alt_hints_use_alt_modifier_labels() {
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let focus = set.alt_hints.iter().find(|h| h.label == "FOCUS").unwrap();
        assert!(
            focus.key.contains("alt+"),
            "FOCUS hint should contain 'alt+', got: {}",
            focus.key
        );
        let split = set.alt_hints.iter().find(|h| h.label == "SPLIT").unwrap();
        assert_eq!(split.key.as_ref(), "alt+n");
    }

    #[test]
    fn alt_hint_many_binding_selects_alt_alternative() {
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let close = set.alt_hints.iter().find(|h| h.label == "CLOSE").unwrap();
        assert_eq!(close.key.as_ref(), "alt+x");
    }

    #[test]
    fn alt_hint_dropped_when_no_alt_alternative() {
        let mut kb = default_keybinds();
        kb.focus_pane_left = ActionKeybinds::prefix("h");
        kb.focus_pane_down = ActionKeybinds::prefix("j");
        kb.focus_pane_up = ActionKeybinds::prefix("k");
        kb.focus_pane_right = ActionKeybinds::prefix("l");
        let set = hints(Mode::Terminal, &kb);
        let focus = set.alt_hints.iter().find(|h| h.label == "FOCUS");
        assert!(focus.is_none());
    }

    #[test]
    fn non_terminal_modes_have_no_alt_hints() {
        let kb = default_keybinds();
        for mode in [
            Mode::Pane,
            Mode::Tab,
            Mode::Session,
            Mode::Locked,
            Mode::Copy,
            Mode::Resize,
            Mode::Move,
            Mode::Prefix,
        ] {
            let set = hints(mode, &kb);
            assert!(
                set.alt_hints.is_empty(),
                "Mode {:?} should not have alt hints",
                mode
            );
        }
    }

    #[test]
    fn degradation_tier1_full_labels_both_sections() {
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 200, true);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("NORMAL"));
        assert!(text.contains("PANE"));
        assert!(text.contains("FOCUS"));
        assert!(text.contains("SPLIT"));
    }

    #[test]
    fn degradation_tier2_alt_section_dropped() {
        // FR4 tier 2: when the width is just shy of fitting both sections at
        // full uppercase, drop the Alt section and render the left section
        // alone at full uppercase. (The legacy lowercase short-label tier
        // was removed in zellij-fidelity round 2.)
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let left_hints: Vec<&Hint> = set.hints.iter().collect();
        let right_hints: Vec<&Hint> = set.alt_hints.iter().collect();
        let left_prefix = detect_section_prefix(&left_hints);
        let right_prefix = detect_section_prefix(&right_hints);
        let left_full = compute_section_width(&left_hints, left_prefix, true);
        let right_full = compute_section_width(&right_hints, right_prefix, true);
        let badge_width = format!(" {} ", set.badge.label).width();
        let gap = MIN_SECTION_GAP;
        let fits_both_full = badge_width + left_full + gap + right_full;
        // One column shy of fitting both sections: exercises tier 2 (drop Alt).
        let width = fits_both_full.saturating_sub(1) as u16;
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, width, true);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("NORMAL"));
        // Alt section labels and short forms are both absent.
        assert!(
            !text.contains("FOCUS") && !text.contains("foc"),
            "Alt section should be dropped, got: {text}"
        );
        // Left section keys render at full uppercase, brackets present.
        assert!(text.contains("<p>"), "left key expected: {text}");
        assert!(text.contains("PANE"), "left full label expected: {text}");
    }

    #[test]
    fn degradation_tier3_ellipsis_on_left() {
        // FR4 tier 3: very narrow widths render an ellipsized prefix of the
        // left section. No prefix tile fits, so each key carries its full
        // sanitized form (`<ctrl+p>`) — the modifier stays discoverable.
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 30, true);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("NORMAL"));
        assert!(text.contains('\u{2026}'));
        assert!(!text.contains("FOCUS") && !text.contains("foc"));
        assert!(
            text.contains("ctrl+"),
            "Ctrl modifier must remain discoverable in tier 3: {text}"
        );
    }

    #[test]
    fn compact_style_uses_top_four_keys_at_full_uppercase() {
        // Compact mode truncates to top-4 keys by priority and renders at
        // full uppercase — never lowercase short forms (zellij never
        // lowercases hint labels).
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Compact, &palette, 200, true);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("NORMAL"));
        // Top-4 left labels (priorities 0-3): PANE, TAB, RESIZE, MOVE.
        // Compact does NOT show SESSION (priority 4) or LOCK (priority 5).
        assert!(text.contains("PANE"));
        assert!(text.contains("TAB"));
        assert!(text.contains("RESIZE"));
        assert!(text.contains("MOVE"));
        assert!(
            !text.contains("SESSION"),
            "compact dropped SESSION (priority 4): {text}"
        );
        assert!(
            !text.contains("LOCK"),
            "compact dropped LOCK (priority 5): {text}"
        );
        // No legacy lowercase short forms anywhere.
        for short in [
            "pane", "tab", "rsz", "mov", "foc", "spl", "cls", "ses", "lck",
        ] {
            assert!(
                !text.contains(short),
                "compact must NOT render lowercase short form {short:?}: {text}"
            );
        }
    }

    #[test]
    fn no_lowercase_short_label_ever_renders() {
        // Render the default terminal-mode hint set at every width from 0 up
        // to a comfortably wide bar; assert NO span text equals any of the
        // legacy lowercase short labels at ANY width. Both Full and Compact
        // styles must comply (the short tier is gone).
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let lowercase_shorts = [
            "pane", "tab", "rsz", "mov", "ses", "lck", "foc", "spl", "cls",
        ];
        for style in [HintBarStyle::Full, HintBarStyle::Compact] {
            for w in 0u16..=240 {
                let line = build_hint_line(&set, style, &palette, w, true);
                let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
                for short in lowercase_shorts {
                    assert!(
                        !text.contains(short),
                        "width={w} style={style:?} rendered short label {short:?}: {text}"
                    );
                }
            }
        }
    }

    #[test]
    fn alt_section_sanitizes_bidi_control_chars() {
        let set = HintSet {
            badge: Badge {
                label: "T",
                accent: BadgeColor::Accent,
            },
            hints: vec![Hint {
                key: Cow::Borrowed("x"),
                label: "left",
                short: "l",
                priority: 0,
            }],
            alt_hints: vec![Hint {
                key: Cow::Borrowed("alt+\u{202e}h"),
                label: "FOC",
                short: "f",
                priority: 0,
            }],
        };
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 200, true);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            !text.contains('\u{202e}'),
            "bidi char should be stripped from alt hint"
        );
        // The "Alt +" prefix is detected and shown as a ribbon; the key is
        // stripped to just "h" (bidi removed) and rendered bracketed.
        assert!(text.contains("Alt +"), "Alt prefix ribbon expected: {text}");
        assert!(text.contains("<h>"), "bracketed key expected: {text}");
    }

    #[test]
    fn no_overlap_at_any_width() {
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let badge_width = format!(" {} ", set.badge.label).width() as u16;
        for style in [HintBarStyle::Full, HintBarStyle::Compact] {
            for w in badge_width..=200 {
                let line = build_hint_line(&set, style, &palette, w, true);
                let total: usize = line.spans.iter().map(|s| s.content.width()).sum();
                assert!(
                    total <= w as usize,
                    "overlap at width {w} ({style:?}): rendered {total} cols"
                );
            }
        }
    }

    #[test]
    fn no_overlap_with_wide_remapped_alt_labels() {
        // Alt labels are user-customizable; a remap to long combos must not
        // overflow or overlap — only shift the degradation thresholds.
        let mut set = hints(Mode::Terminal, &Default::default());
        set.alt_hints = vec![
            Hint {
                key: Cow::Borrowed("alt+shift+pageup/alt+shift+pagedown"),
                label: "FOCUS",
                short: "foc",
                priority: 0,
            },
            Hint {
                key: Cow::Borrowed("ctrl+alt+backspace"),
                label: "CLOSE",
                short: "cls",
                priority: 1,
            },
        ];
        let palette = Palette::catppuccin();
        let badge_width = format!(" {} ", set.badge.label).width() as u16;
        for w in badge_width..=200 {
            let line = build_hint_line(&set, HintBarStyle::Full, &palette, w, true);
            let total: usize = line.spans.iter().map(|s| s.content.width()).sum();
            assert!(
                total <= w as usize,
                "overlap at width {w} with wide alt labels: rendered {total} cols"
            );
        }
    }

    #[test]
    fn two_sections_keep_minimum_gap() {
        // When both sections render, the blank run between them must be at least
        // MIN_SECTION_GAP so they stay visually distinct (FR4 "never overlap").
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        // A wide terminal renders both sections (tier 1 or 2).
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 200, true);
        // The single padding span is the inter-section blank run.
        let max_blank = line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .filter(|c| c.chars().all(|ch| ch == ' '))
            .map(|c| c.width())
            .max()
            .unwrap_or(0);
        assert!(
            max_blank >= MIN_SECTION_GAP,
            "inter-section gap {max_blank} < MIN_SECTION_GAP {MIN_SECTION_GAP}"
        );
    }

    // --- Ribbon segment tests ---

    #[test]
    fn ribbon_uses_bracketed_key_format() {
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 200, true);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        // Keys are bracketed <x>, not bare ctrl+x
        assert!(text.contains("<p>"), "bracketed key expected: {text}");
        assert!(
            !text.contains("ctrl+p"),
            "bare ctrl+p should not appear in ribbon mode: {text}"
        );
    }

    #[test]
    fn ribbon_bracket_fg_distinct_from_key_fg() {
        // The `<` and `>` brackets render in the dim label color; only the
        // key body inside carries the accent highlight + bold. Verifies the
        // three-span split in `push_tile` (zellij-fidelity round 2 change 3).
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 200, true);

        // Walk spans, find each `<` and `>` and the span immediately between
        // them; assert the bracket fg matches label_fg and the body fg matches
        // key_fg + bold.
        let spans = &line.spans;
        let mut found_pairs = 0;
        let mut i = 0;
        while i + 2 < spans.len() {
            if spans[i].content.as_ref() == "<" && spans[i + 2].content.as_ref() == ">" {
                let open = &spans[i];
                let body = &spans[i + 1];
                let close = &spans[i + 2];
                assert_eq!(open.style.fg, Some(palette.overlay1), "`<` fg = label_fg");
                assert_eq!(close.style.fg, Some(palette.overlay1), "`>` fg = label_fg");
                assert_eq!(body.style.fg, Some(palette.accent), "key body fg = accent");
                assert!(
                    body.style.add_modifier.contains(Modifier::BOLD),
                    "key body must be bold"
                );
                assert!(
                    !open.style.add_modifier.contains(Modifier::BOLD),
                    "`<` must not be bold"
                );
                found_pairs += 1;
                i += 3;
            } else {
                i += 1;
            }
        }
        assert!(
            found_pairs >= 4,
            "expected several bracketed key tiles, found {found_pairs}"
        );
    }

    #[test]
    fn bracket_fg_distinct_from_key_fg_on_default_palette() {
        // Visual contrast invariant: the dim bracket fg and the accent key fg
        // are different colors on the default palette. If a palette change
        // makes them identical the bracket-vs-letter distinction collapses
        // and this test will catch it.
        let p = Palette::catppuccin();
        assert_ne!(
            p.overlay1, p.accent,
            "default palette overlay1 (bracket fg) must differ from accent (key fg)"
        );
    }

    #[test]
    fn ribbon_single_ctrl_prefix_per_section() {
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 200, true);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        // Exactly one "Ctrl +" prefix ribbon for the left section
        assert_eq!(
            text.matches("Ctrl +").count(),
            1,
            "single Ctrl + prefix expected: {text}"
        );
    }

    #[test]
    fn ribbon_single_alt_prefix_per_section() {
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 200, true);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        // Exactly one "Alt +" prefix ribbon for the right section
        assert_eq!(
            text.matches("Alt +").count(),
            1,
            "single Alt + prefix expected: {text}"
        );
    }

    #[test]
    fn ribbon_powerline_arrows_present_when_on() {
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 200, true);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        let arrow_count = text.matches(POWERLINE_ARROW).count();
        assert!(
            arrow_count > 0,
            "powerline arrows expected in ribbon: {text}"
        );
    }

    #[test]
    fn ribbon_arrows_visible_with_low_color_palette() {
        // In the terminal() 16-color palette surface0 == panel_bg == Reset, so a
        // naive `fg = from_bg` arrow would be invisible against panel_bg. Every
        // powerline arrow span must have fg != bg.
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::terminal();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 200, true);
        for span in &line.spans {
            if span.content.as_ref().contains(POWERLINE_ARROW) {
                assert_ne!(
                    span.style.fg, span.style.bg,
                    "powerline arrow must be visible (fg != bg) in low-color palette"
                );
            }
        }
    }

    #[test]
    fn ribbon_tiles_produce_back_to_back_wedge_separators() {
        // Zellij convention: each tile owns its own left+right Powerline arrow,
        // both blending against panel_bg. Two adjacent tiles produce a
        // back-to-back wedge pattern: `[right arrow: tile_bg→panel_bg]
        // [left arrow: panel_bg→tile_bg]`. The arrow spans alternate fg/bg
        // direction across consecutive arrow spans.
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 200, true);

        // Collect arrow spans in render order.
        let arrows: Vec<_> = line
            .spans
            .iter()
            .filter(|s| s.content.as_ref() == POWERLINE_ARROW)
            .collect();
        assert!(arrows.len() >= 4, "expected multiple tile arrows");

        // Each tile contributes 2 arrows. Walk pairs and assert the back-to-back
        // pattern: arrow[i] (left arrow of a tile, panel→tile) followed by
        // arrow[i+1] (right arrow of the same tile, tile→panel).
        let mut i = 0;
        while i + 1 < arrows.len() {
            let left = arrows[i];
            let right = arrows[i + 1];
            assert_eq!(
                left.style.fg,
                Some(palette.panel_bg),
                "left arrow fg = panel_bg"
            );
            assert_eq!(
                left.style.bg,
                Some(palette.surface0),
                "left arrow bg = tile_bg"
            );
            assert_eq!(
                right.style.fg,
                Some(palette.surface0),
                "right arrow fg = tile_bg"
            );
            assert_eq!(
                right.style.bg,
                Some(palette.panel_bg),
                "right arrow bg = panel_bg"
            );
            i += 2;
        }
    }

    #[test]
    fn ribbon_prefix_is_plain_text_not_colored_ribbon() {
        // Zellij's superkey (Ctrl+/Alt+) renders as plain bold text on
        // panel_bg, NOT as a colored ribbon segment. Verified across every
        // built-in palette so a future palette change cannot silently
        // reintroduce a colored prefix ribbon.
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
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
            let line = build_hint_line(&set, HintBarStyle::Full, &palette, 200, true);
            for needle in [" Ctrl + ", " Alt + "] {
                let span = line
                    .spans
                    .iter()
                    .find(|s| s.content.as_ref() == needle)
                    .unwrap_or_else(|| panic!("missing {needle:?} prefix span"));
                assert_eq!(
                    span.style.bg,
                    Some(palette.panel_bg),
                    "{needle:?} prefix must sit on panel_bg (zellij superkey convention)"
                );
                assert_ne!(
                    span.style.fg, span.style.bg,
                    "{needle:?} prefix fg/bg collision"
                );
            }
        }
    }

    #[test]
    fn ribbon_no_powerline_codepoints_when_off() {
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 200, false);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            !text.contains(POWERLINE_ARROW),
            "no powerline codepoints when powerline off: {text}"
        );
    }

    #[test]
    fn ribbon_no_overlap_at_any_width_powerline_off() {
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let badge_width = format!(" {} ", set.badge.label).width() as u16;
        for w in badge_width..=200 {
            let line = build_hint_line(&set, HintBarStyle::Full, &palette, w, false);
            let total: usize = line.spans.iter().map(|s| s.content.width()).sum();
            assert!(
                total <= w as usize,
                "overlap at width {w} (powerline off): rendered {total} cols"
            );
        }
    }
}
