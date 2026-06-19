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
use crate::config::{HintBarStyle, Keybinds};

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
}

pub struct Badge {
    pub label: &'static str,
    accent: BadgeColor,
}

pub struct HintSet {
    pub badge: Badge,
    pub hints: Vec<Hint>,
}

fn sanitize_key(s: &str) -> Cow<'static, str> {
    let needs_sanitize = s.chars().any(|c| c.is_control() || is_bidi_override(c));
    if needs_sanitize {
        Cow::Owned(
            s.chars()
                .filter(|c| !c.is_control() && !is_bidi_override(*c))
                .collect(),
        )
    } else {
        Cow::Owned(s.to_string())
    }
}

fn is_bidi_override(c: char) -> bool {
    matches!(
        c,
        '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}'
    )
}

fn resolve_label(bindings: &crate::config::ActionKeybinds) -> Option<Cow<'static, str>> {
    bindings.label().map(|s| sanitize_key(&s))
}

fn resolve_prefix_rhs(bindings: &crate::config::ActionKeybinds) -> Option<Cow<'static, str>> {
    bindings.prefix_rhs_label().map(|s| sanitize_key(&s))
}

fn terminal_hints(kb: &Keybinds) -> HintSet {
    let mut hints = Vec::new();

    if let Some(key) = resolve_label(&kb.focus_pane_left) {
        hints.push(Hint {
            key,
            label: "navigate",
            short: "nav",
            priority: 0,
        });
    }
    if let Some(key) = resolve_label(&kb.split_vertical) {
        hints.push(Hint {
            key,
            label: "split",
            short: "split",
            priority: 1,
        });
    }
    if let Some(key) = resolve_label(&kb.zoom) {
        hints.push(Hint {
            key,
            label: "zoom",
            short: "zoom",
            priority: 2,
        });
    }
    if let Some(key) = resolve_label(&kb.resize_mode) {
        hints.push(Hint {
            key,
            label: "resize",
            short: "rsz",
            priority: 3,
        });
    }

    HintSet {
        badge: Badge {
            label: "TERMINAL",
            accent: BadgeColor::Accent,
        },
        hints,
    }
}

fn navigate_hints(kb: &Keybinds) -> HintSet {
    let mut hints = Vec::new();

    hints.push(Hint {
        key: Cow::Borrowed("esc"),
        label: "back",
        short: "back",
        priority: 0,
    });

    if let Some(up) = resolve_label(&kb.navigate.workspace_up) {
        if let Some(down) = resolve_label(&kb.navigate.workspace_down) {
            hints.push(Hint {
                key: Cow::Owned(format!("{up} / {down}")),
                label: "ws",
                short: "ws",
                priority: 1,
            });
        }
    }

    hints.push(Hint {
        key: Cow::Borrowed("\u{21e5}"),
        label: "pane",
        short: "pane",
        priority: 2,
    });

    if let Some(key) = resolve_prefix_rhs(&kb.goto) {
        hints.push(Hint {
            key,
            label: "navigator",
            short: "goto",
            priority: 3,
        });
    }
    if let Some(key) = resolve_prefix_rhs(&kb.new_tab) {
        hints.push(Hint {
            key,
            label: "new tab",
            short: "tab",
            priority: 4,
        });
    }
    if let Some(key) = resolve_prefix_rhs(&kb.split_vertical) {
        hints.push(Hint {
            key,
            label: "split\u{2502}",
            short: "\u{2502}",
            priority: 5,
        });
    }
    if let Some(key) = resolve_prefix_rhs(&kb.split_horizontal) {
        hints.push(Hint {
            key,
            label: "split\u{2500}",
            short: "\u{2500}",
            priority: 6,
        });
    }
    if let Some(key) = resolve_prefix_rhs(&kb.close_pane) {
        hints.push(Hint {
            key,
            label: "close",
            short: "cls",
            priority: 7,
        });
    }
    if let Some(key) = resolve_prefix_rhs(&kb.zoom) {
        hints.push(Hint {
            key,
            label: "zoom",
            short: "zm",
            priority: 8,
        });
    }
    if let Some(key) = resolve_prefix_rhs(&kb.resize_mode) {
        hints.push(Hint {
            key,
            label: "resize",
            short: "rsz",
            priority: 9,
        });
    }
    if let Some(key) = resolve_prefix_rhs(&kb.help) {
        hints.push(Hint {
            key,
            label: "keybinds",
            short: "keys",
            priority: 10,
        });
    }
    if let Some(key) = resolve_prefix_rhs(&kb.settings) {
        hints.push(Hint {
            key,
            label: "settings",
            short: "set",
            priority: 11,
        });
    }
    if let Some(key) = resolve_prefix_rhs(&kb.detach) {
        hints.push(Hint {
            key,
            label: "detach",
            short: "det",
            priority: 12,
        });
    }

    HintSet {
        badge: Badge {
            label: "NAVIGATE",
            accent: BadgeColor::Accent,
        },
        hints,
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
    }
}

fn resize_hints() -> HintSet {
    HintSet {
        badge: Badge {
            label: "RESIZE",
            accent: BadgeColor::Mauve,
        },
        hints: vec![
            Hint {
                key: Cow::Borrowed("h/l"),
                label: "width",
                short: "w",
                priority: 0,
            },
            Hint {
                key: Cow::Borrowed("j/k"),
                label: "height",
                short: "h",
                priority: 1,
            },
            Hint {
                key: Cow::Borrowed("esc"),
                label: "done",
                short: "done",
                priority: 2,
            },
        ],
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
    }
}

pub fn hints(mode: Mode, kb: &Keybinds) -> HintSet {
    match mode {
        Mode::Navigate => navigate_hints(kb),
        Mode::Prefix => prefix_hints(kb),
        Mode::Resize => resize_hints(),
        Mode::Copy => copy_hints(),
        _ => terminal_hints(kb),
    }
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

    let mut used = badge_width;

    let selected_hints: Vec<&Hint> = if style == HintBarStyle::Compact {
        let mut sorted: Vec<&Hint> = hint_set.hints.iter().collect();
        sorted.sort_by_key(|h| h.priority);
        sorted.truncate(4);
        sorted
    } else {
        hint_set.hints.iter().collect()
    };

    for hint in &selected_hints {
        let label = if style == HintBarStyle::Compact {
            hint.short
        } else {
            hint.label
        };
        let separator = " ";
        let key_str: &str = &hint.key;
        let entry = format!("{separator}{key_str} {label}");
        let entry_width = entry.width();

        let ellipsis_width = 2; // " …"
        if used + entry_width + ellipsis_width > width && used + entry_width > width {
            if used + ellipsis_width <= width {
                spans.push(Span::styled(" \u{2026}", dim_style));
            }
            return Line::from(spans);
        }

        used += separator.width();
        spans.push(Span::raw(String::from(separator)));
        spans.push(Span::styled(String::from(key_str), key_style));
        let label_text = format!(" {label}");
        used += key_str.width() + label_text.width();
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

    if app.mode == Mode::Navigate && app.update_available.is_some() {
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
    fn navigate_hints_contain_expected_actions() {
        let kb = default_keybinds();
        let set = hints(Mode::Navigate, &kb);
        assert_eq!(set.badge.label, "NAVIGATE");
        assert!(!set.hints.is_empty());
        let labels: Vec<&str> = set.hints.iter().map(|h| h.label).collect();
        assert!(labels.contains(&"back"));
        assert!(labels.contains(&"zoom"));
        assert!(labels.contains(&"keybinds"));
        assert!(labels.contains(&"settings"));
        assert!(labels.contains(&"detach"));
    }

    #[test]
    fn unbound_action_is_omitted() {
        let mut kb = default_keybinds();
        kb.zoom = crate::config::ActionKeybinds { bindings: vec![] };
        let set = hints(Mode::Navigate, &kb);
        let labels: Vec<&str> = set.hints.iter().map(|h| h.label).collect();
        assert!(!labels.contains(&"zoom"));
    }

    #[test]
    fn fallback_modes_return_terminal_hints() {
        let kb = default_keybinds();
        let terminal_set = hints(Mode::Terminal, &kb);
        let settings_set = hints(Mode::Settings, &kb);
        assert_eq!(terminal_set.badge.label, "TERMINAL");
        assert_eq!(settings_set.badge.label, "TERMINAL");
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
        let set = hints(Mode::Navigate, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Compact, &palette, 200);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("NAVIGATE"));
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
        let set = hints(Mode::Navigate, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 30);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("NAVIGATE"));
        assert!(text.contains('\u{2026}'));
    }

    #[test]
    fn badge_never_dropped_at_tiny_width() {
        let kb = default_keybinds();
        let set = hints(Mode::Navigate, &kb);
        let palette = Palette::catppuccin();
        let line = build_hint_line(&set, HintBarStyle::Full, &palette, 5);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("NAVIGATE"));
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
}
