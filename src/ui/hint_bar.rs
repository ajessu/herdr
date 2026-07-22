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

pub struct Hint {
    pub key: Cow<'static, str>,
    pub label: &'static str,
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
    // Sanitization happens at the build_hint_line chokepoint for all hint sources.
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
        // Directional resize keys are hardcoded like the other sticky modes; only
        // the non-directional grow/shrink keys are surfaced from the live config.
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

    // mode_entry.locked is None only when config validation dropped the key; the
    // reachability invariant (keybinds.rs) prevents starting locked without it, but
    // be honest rather than advertise a key that won't work.
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

fn compute_section_width(hints: &[&Hint], use_short: bool) -> usize {
    hints.iter().fold(0usize, |acc, hint| {
        let label = if use_short { hint.short } else { hint.label };
        let key_w = hint.key.width();
        // " key label" → separator(1) + key_w + space(1) + label_w
        acc + 1 + key_w + 1 + label.width()
    })
}

fn build_section_spans(
    hints: &[&Hint],
    use_short: bool,
    key_style: Style,
    dim_style: Style,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for hint in hints {
        let label = if use_short { hint.short } else { hint.label };
        let key_str = sanitize_key(&hint.key);
        spans.push(Span::raw(String::from(" ")));
        spans.push(Span::styled(key_str.into_owned(), key_style));
        spans.push(Span::styled(format!(" {label}"), dim_style));
    }
    spans
}

pub fn build_hint_line(
    hint_set: &HintSet,
    style: HintBarStyle,
    palette: &Palette,
    width: u16,
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
    let key_style = Style::default()
        .fg(palette.accent)
        .add_modifier(Modifier::BOLD);
    let dim_style = Style::default().fg(palette.overlay0);

    let badge_text = format!(" {} ", hint_set.badge.label);
    let badge_width = badge_text.width();

    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled(badge_text, badge_style));

    if badge_width >= width {
        return Line::from(spans);
    }

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

    // FR4 degradation: full labels → short labels → drop right → ellipsis on left
    let remaining = width.saturating_sub(badge_width);

    if has_right {
        let left_full = compute_section_width(&left_hints, false);
        let right_full = compute_section_width(&right_hints, false);
        let gap = 2; // minimum gap between sections

        // Tier 1: both full labels
        if left_full + gap + right_full <= remaining {
            let pad = remaining - left_full - right_full;
            spans.extend(build_section_spans(
                &left_hints,
                false,
                key_style,
                dim_style,
            ));
            spans.push(Span::raw(" ".repeat(pad)));
            spans.extend(build_section_spans(
                &right_hints,
                false,
                key_style,
                dim_style,
            ));
            return Line::from(spans);
        }

        // Tier 2: both short labels
        let left_short = compute_section_width(&left_hints, true);
        let right_short = compute_section_width(&right_hints, true);
        if left_short + gap + right_short <= remaining {
            let pad = remaining - left_short - right_short;
            spans.extend(build_section_spans(&left_hints, true, key_style, dim_style));
            spans.push(Span::raw(" ".repeat(pad)));
            spans.extend(build_section_spans(
                &right_hints,
                true,
                key_style,
                dim_style,
            ));
            return Line::from(spans);
        }
    }

    // Tier 3 (or no right section): left only with truncation/ellipsis
    let use_short = style == HintBarStyle::Compact;
    let mut used = badge_width;
    for hint in &left_hints {
        let label = if use_short { hint.short } else { hint.label };
        let key_str = sanitize_key(&hint.key);
        let entry = format!(" {key_str} {label}");
        let entry_width = entry.width();

        let ellipsis_width = 2; // " …"
        if used + entry_width + ellipsis_width > width && used + entry_width > width {
            if used + ellipsis_width <= width {
                spans.push(Span::styled(" \u{2026}", dim_style));
            }
            return Line::from(spans);
        }

        spans.push(Span::raw(String::from(" ")));
        let key_width = key_str.width();
        spans.push(Span::styled(key_str.into_owned(), key_style));
        let label_text = format!(" {label}");
        used += 1 + key_width + label_text.width();
        spans.push(Span::styled(label_text, dim_style));
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
    let line = build_hint_line(&hint_set, app.hint_bar, &app.palette, area.width);

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
        let kb = default_keybinds();
        let set = hints(Mode::Session, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Compact, &palette, 200);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("SESSION"));
        let short_labels: Vec<&str> = set
            .hints
            .iter()
            .filter(|h| h.priority < 4)
            .map(|h| h.short)
            .collect();
        for label in short_labels {
            assert!(text.contains(label), "compact missing short label: {label}");
        }
    }

    #[test]
    fn truncation_appends_ellipsis() {
        let kb = default_keybinds();
        let set = hints(Mode::Session, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 30);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("SESSION"));
        assert!(text.contains('\u{2026}'));
    }

    #[test]
    fn badge_never_dropped_at_tiny_width() {
        let kb = default_keybinds();
        let set = hints(Mode::Session, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 5);
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
                key: Cow::Borrowed("\u{4e16}"), // CJK char, 2 display columns
                label: "x",
                short: "x",
                priority: 0,
            }],
            alt_hints: Vec::new(),
        };
        let palette = Palette::catppuccin();
        // Badge " T " = 3 cols, then " 世 x" = 1 + 2 + 1 + 1 = 5 cols; total = 8
        // At width 7, the hint shouldn't fit (badge=3, entry needs 5, total 8 > 7)
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 7);
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
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 200);
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
        assert!(
            !set.alt_hints.is_empty(),
            "Terminal mode should have alt hints"
        );
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
        assert_eq!(
            close.key.as_ref(),
            "alt+x",
            "close_pane Many binding should show the alt alternative"
        );
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
        assert!(
            focus.is_none(),
            "FOCUS should be dropped when no alt alternative exists"
        );
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
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 200);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("NORMAL"));
        assert!(text.contains("PANE"));
        assert!(
            text.contains("FOCUS"),
            "right section should be present at wide width"
        );
        assert!(text.contains("SPLIT"));
    }

    #[test]
    fn degradation_tier2_short_labels_both_sections() {
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let left_full = compute_section_width(&set.hints.iter().collect::<Vec<_>>(), false);
        let right_full = compute_section_width(&set.alt_hints.iter().collect::<Vec<_>>(), false);
        let left_short = compute_section_width(&set.hints.iter().collect::<Vec<_>>(), true);
        let right_short = compute_section_width(&set.alt_hints.iter().collect::<Vec<_>>(), true);
        let badge_width = format!(" {} ", set.badge.label).width();
        let gap = 2;
        // Width that fits short but not full
        let too_small_for_full = badge_width + left_full + gap + right_full;
        let fits_short = badge_width + left_short + gap + right_short;
        let width = too_small_for_full.saturating_sub(1).max(fits_short) as u16;
        if (width as usize) < fits_short {
            return; // can't exercise this tier with default binds
        }
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, width);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("NORMAL"));
        // Should contain short labels
        assert!(
            text.contains("foc") || text.contains("spl"),
            "short labels should appear in tier 2, got: {text}"
        );
    }

    #[test]
    fn degradation_tier3_alt_section_dropped() {
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let left_short = compute_section_width(&set.hints.iter().collect::<Vec<_>>(), true);
        let right_short = compute_section_width(&set.alt_hints.iter().collect::<Vec<_>>(), true);
        let badge_width = format!(" {} ", set.badge.label).width();
        let gap = 2;
        // Width too small for short labels of both, but enough for left only
        let fits_both_short = badge_width + left_short + gap + right_short;
        let width = (fits_both_short.saturating_sub(1)) as u16;
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, width);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("NORMAL"));
        assert!(
            !text.contains("FOCUS") && !text.contains("foc"),
            "Alt section should be dropped at narrow width, got: {text}"
        );
        assert!(text.contains("PANE") || text.contains("pane"));
    }

    #[test]
    fn degradation_tier4_ellipsis_on_left() {
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 30);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("NORMAL"));
        assert!(
            text.contains('\u{2026}'),
            "should have ellipsis at very narrow"
        );
        assert!(
            !text.contains("FOCUS") && !text.contains("foc"),
            "Alt section must not appear"
        );
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
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 200);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            !text.contains('\u{202e}'),
            "bidi char should be stripped from alt hint"
        );
        assert!(text.contains("alt+h"), "sanitized key should remain");
    }

    #[test]
    fn no_overlap_at_any_width() {
        let kb = default_keybinds();
        let set = hints(Mode::Terminal, &kb);
        let palette = Palette::catppuccin();
        let badge_width = format!(" {} ", set.badge.label).width() as u16;
        for w in badge_width..=200 {
            let line = build_hint_line(&set, HintBarStyle::Full, &palette, w);
            let total: usize = line.spans.iter().map(|s| s.content.width()).sum();
            assert!(
                total <= w as usize,
                "overlap at width {w}: rendered {total} cols"
            );
        }
    }
}
