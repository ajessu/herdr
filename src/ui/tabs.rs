use std::borrow::Cow;

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use unicode_width::UnicodeWidthStr;

use super::widgets::panel_contrast_fg;
use crate::app::AppState;
use crate::config::TabStatusMode;

const MIN_TAB_WIDTH: u16 = 8;
const NEW_TAB_WIDTH: u16 = 3;
/// Width of a collapsed `←+N` / `+N→` overflow indicator. Four cells keeps a
/// touch-adequate hit zone (NFR5) — never a 1-cell target — and fits the widest
/// label (`←+9+` / `+9+→`) without clipping.
const OVERFLOW_INDICATOR_WIDTH: u16 = 4;

/// The Powerline "right arrow" separator glyph (U+E0B0). Rendered only when
/// `SeparatorStyle::Powerline` is selected; the `AlternatingBg` path emits zero
/// Powerline codepoints so a font-tofu terminal degrades cleanly.
const POWERLINE_ARROW: &str = "\u{e0b0}";

/// Selects how adjacent tabs are visually separated. Chosen by an explicit
/// painter parameter (from `ui.tabs.powerline`), never an environment probe, so
/// both paths are deterministically unit-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SeparatorStyle {
    /// Powerline arrow glyphs between tabs (default, `ui.tabs.powerline = true`).
    Powerline,
    /// Alternating tab backgrounds as the separator; no Powerline codepoints.
    AlternatingBg,
}

/// A run of hidden tabs collapsed behind one overflow indicator. `jump_to` is a
/// TAB INDEX (not a vec position) — the nearest hidden tab on that side — and is
/// range-asserted against the live tab count before use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HiddenGroup {
    pub count: usize,
    pub jump_to: usize,
}

/// Overflow indicators for the centered-active fill: the hidden tabs to each
/// side of the visible window. Either side is `None` when nothing is hidden
/// there. The rects are the clickable hit zones (mouse chrome) or marker
/// positions (non-mouse).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TabBarOverflow {
    pub left: Option<HiddenGroup>,
    pub right: Option<HiddenGroup>,
    pub left_hit_area: Rect,
    pub right_hit_area: Rect,
}

// ---------------------------------------------------------------------------
// TabChrome — structured per-tab label model
// ---------------------------------------------------------------------------

/// Strip control and bidi-override characters from a user-writable display
/// name before it reaches the buffer. Tab/workspace `custom_name` is persisted,
/// re-encoded over the web transport, and rendered in the full-width readable
/// active tab beside inverted Powerline separators, which raises the spoof
/// value of an embedded RTL override or control sequence. Borrows when the
/// input is already clean. Mirrors `hint_bar::sanitize_key`.
pub(crate) fn sanitize_display_name(s: &str) -> Cow<'_, str> {
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

/// Truncate `s` to at most `budget` display columns, appending `…` (1 column)
/// when anything was dropped. Accumulates per-grapheme display width so wide
/// (CJK) glyphs are never split across the truncation boundary.
fn truncate_to_width(s: &str, budget: u16) -> String {
    if budget == 0 {
        return String::new();
    }
    // Reserve one column for the trailing ellipsis.
    let content_budget = usize::from(budget.saturating_sub(1));
    let mut out = String::new();
    let mut used = 0usize;
    for ch in s.chars() {
        let ch_w = ch.to_string().width();
        if used + ch_w > content_budget {
            break;
        }
        out.push(ch);
        used += ch_w;
    }
    out.push('…');
    out
}

#[derive(Debug, Clone)]
pub(crate) struct TabStatusDot {
    pub glyph: &'static str,
    pub style: Style,
}

#[derive(Debug, Clone)]
pub(crate) struct TabChrome {
    pub status: Option<TabStatusDot>,
    pub name: String,
    pub zoomed: bool,
}

impl TabChrome {
    pub fn display_width(&self, mode: TabStatusMode) -> u16 {
        let status_w: u16 = if matches!(mode, TabStatusMode::Off) {
            0
        } else {
            2
        };
        // Unicode display width (not char count): CJK/wide glyphs occupy two
        // columns and combining/ZWJ sequences fewer, so char count would
        // mis-size the tab and break the position→index round-trip. The
        // sanitized name is what actually reaches the buffer, so measure it.
        let name_w = u16::try_from(sanitize_display_name(&self.name).width()).unwrap_or(u16::MAX);
        let mod_w: u16 = if self.zoomed { 2 } else { 0 };
        status_w.saturating_add(name_w).saturating_add(mod_w)
    }

    /// Render the tab label into spans padded to `rect_width`. The name is
    /// sanitized (control/bidi stripped) at this render chokepoint. The active
    /// tab is always laid out at its natural width, so truncation only happens
    /// for a single over-wide tab that alone exceeds the bar — in that case the
    /// name is shortened by Unicode display width with a trailing `…`.
    pub fn to_spans(&self, mode: TabStatusMode, rect_width: u16) -> Vec<Span<'static>> {
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(6);

        // Leading space
        spans.push(Span::raw(" "));

        // Status slot (only when mode != Off)
        if !matches!(mode, TabStatusMode::Off) {
            if let Some(ref dot) = self.status {
                spans.push(Span::styled(dot.glyph, dot.style));
                spans.push(Span::raw(" "));
            } else {
                spans.push(Span::raw("  "));
            }
        }

        let status_w: u16 = if matches!(mode, TabStatusMode::Off) {
            0
        } else {
            2
        };
        let mod_w: u16 = if self.zoomed { 2 } else { 0 };
        // 1 col for the leading space, plus the status slot and zoom suffix.
        let name_budget = rect_width
            .saturating_sub(1)
            .saturating_sub(status_w)
            .saturating_sub(mod_w);
        let name = sanitize_display_name(&self.name);
        let name_cols = u16::try_from(name.width()).unwrap_or(u16::MAX);
        if name_budget > 0 && name_cols > name_budget {
            spans.push(Span::raw(truncate_to_width(&name, name_budget)));
        } else {
            spans.push(Span::raw(name.into_owned()));
        }

        // Zoom modifier
        if self.zoomed {
            spans.push(Span::raw(" Z"));
        }

        // Trailing pad to fill rect_width
        let content_width = spans.iter().fold(0u16, |acc, s| {
            acc.saturating_add(u16::try_from(s.content.width()).unwrap_or(u16::MAX))
        });
        let pad = rect_width.saturating_sub(content_width);
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad as usize)));
        }

        spans
    }
}

// ---------------------------------------------------------------------------
// Chrome builders
// ---------------------------------------------------------------------------

fn tab_status_dot(
    state: crate::detect::AgentState,
    seen: bool,
    mode: TabStatusMode,
    tick: u32,
    palette: &crate::app::state::Palette,
) -> Option<TabStatusDot> {
    use crate::detect::AgentState;

    let visible = match mode {
        TabStatusMode::Off => false,
        TabStatusMode::Attention => matches!(
            (state, seen),
            (AgentState::Blocked, _) | (AgentState::Idle, false)
        ),
        TabStatusMode::All => !matches!(state, AgentState::Unknown),
    };
    if !visible {
        return None;
    }
    let (glyph, style) = super::status::agent_icon(state, seen, tick, palette);
    Some(TabStatusDot { glyph, style })
}

pub(crate) fn build_tab_chromes(
    ws: &crate::workspace::Workspace,
    terminals: &std::collections::HashMap<
        crate::terminal::TerminalId,
        crate::terminal::TerminalState,
    >,
    show_tab_status: TabStatusMode,
    spinner_tick: u32,
    palette: &crate::app::state::Palette,
) -> Vec<TabChrome> {
    use crate::detect::AgentState;

    let mut chromes: Vec<TabChrome> = Vec::with_capacity(ws.tabs.len());
    let mut attention_count = 0usize;
    let mut dot_count = 0usize;

    for tab_idx in 0..ws.tabs.len() {
        let (chrome, source) = build_tab_chrome(
            ws,
            tab_idx,
            terminals,
            show_tab_status,
            spinner_tick,
            palette,
        );
        if chrome.status.is_some() {
            dot_count += 1;
        }
        if let Some((state, seen)) = source {
            if matches!(state, AgentState::Blocked)
                || matches!((state, seen), (AgentState::Idle, false))
            {
                attention_count += 1;
            }
        }
        chromes.push(chrome);
    }

    if !matches!(show_tab_status, TabStatusMode::Off) {
        tracing::debug!(
            visible_tabs = chromes.len(),
            dot_count,
            attention_count,
            "tab chrome built"
        );
    }

    chromes
}

fn build_tab_chrome(
    ws: &crate::workspace::Workspace,
    tab_idx: usize,
    terminals: &std::collections::HashMap<
        crate::terminal::TerminalId,
        crate::terminal::TerminalState,
    >,
    show_tab_status: TabStatusMode,
    spinner_tick: u32,
    palette: &crate::app::state::Palette,
) -> (TabChrome, Option<(crate::detect::AgentState, bool)>) {
    let name = ws
        .tab_display_name(tab_idx)
        .unwrap_or_else(|| (tab_idx + 1).to_string());
    let Some(tab) = ws.tabs.get(tab_idx) else {
        return (
            TabChrome {
                status: None,
                name,
                zoomed: false,
            },
            None,
        );
    };
    let zoomed = tab.zoomed;

    let (status, source) = if matches!(show_tab_status, TabStatusMode::Off) {
        (None, None)
    } else {
        let (state, seen) = crate::app::actions::tab_aggregate_state(tab, terminals);
        let dot = tab_status_dot(state, seen, show_tab_status, spinner_tick, palette);
        let glyph = dot.as_ref().map(|d| d.glyph).unwrap_or("none");
        tracing::trace!(
            tab_idx,
            %name,
            ?state,
            seen,
            ?show_tab_status,
            glyph,
            "tab chrome"
        );
        (dot, Some((state, seen)))
    };

    (
        TabChrome {
            status,
            name,
            zoomed,
        },
        source,
    )
}

// ---------------------------------------------------------------------------
// Shared helper for both call sites (compute_view_internal & refresh_tab_bar_view)
// ---------------------------------------------------------------------------

pub(crate) fn build_tab_bar_inputs(
    ws: &crate::workspace::Workspace,
    terminals: &std::collections::HashMap<
        crate::terminal::TerminalId,
        crate::terminal::TerminalState,
    >,
    show_tab_status: TabStatusMode,
    spinner_tick: u32,
    palette: &crate::app::state::Palette,
) -> (Vec<TabChrome>, usize, TabStatusMode) {
    let chromes = build_tab_chromes(ws, terminals, show_tab_status, spinner_tick, palette);
    (chromes, ws.active_tab, show_tab_status)
}

// ---------------------------------------------------------------------------
// TabBarView
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub(crate) struct TabBarView {
    /// Dense per-tab hit rects, length == tab count. Hidden (off-window) tabs
    /// keep `width == 0`; visible tabs round-trip through `tab_at`.
    pub tab_hit_areas: Vec<Rect>,
    pub tab_chrome: Vec<TabChrome>,
    pub tab_status_mode: TabStatusMode,
    /// Collapsed hidden-tab indicators on each overflowing side, with their
    /// clickable/marker rects. `None`/default when nothing overflows.
    pub overflow: TabBarOverflow,
    pub new_tab_hit_area: Rect,
}

fn tab_width(chrome: &TabChrome, mode: TabStatusMode) -> u16 {
    chrome
        .display_width(mode)
        .saturating_add(4)
        .max(MIN_TAB_WIDTH)
}

/// Trailing x just past the last visible (width>0) tab, for placing the
/// new-tab button. Falls back to `fallback_x` when nothing is visible.
fn trailing_tab_controls_x(tab_hit_areas: &[Rect], fallback_x: u16) -> u16 {
    tab_hit_areas
        .iter()
        .rev()
        .find(|rect| rect.width > 0)
        .map(|rect| rect.x + rect.width)
        .unwrap_or(fallback_x)
}

/// Stateless centered-active batch fill (zellij model).
///
/// Returns the dense per-tab rect vec (len == `chromes.len()`, hidden tabs
/// `width == 0`) plus the first/last visible tab indices. Computed purely from
/// `(active_tab, area)` — no scroll state is carried across frames.
///
/// Algorithm: place the active tab first at its natural width, then grow the
/// visible window one whole tab at a time, alternately preferring the lighter
/// (narrower-so-far) side, until the next tab on a side would not fit the
/// remaining width. `reserve_left`/`reserve_right` reserve columns for the
/// overflow indicators that may sit on each edge; the caller passes the
/// indicator width when mouse/marker chrome is shown. A single active tab wider
/// than the whole bar is clipped to the bar so the row is never blank.
fn centered_active_fill(
    chromes: &[TabChrome],
    active_tab: usize,
    mode: TabStatusMode,
    area: Rect,
    reserve_left: u16,
    reserve_right: u16,
) -> Vec<Rect> {
    let n = chromes.len();
    let mut rects = vec![Rect::default(); n];
    if n == 0 || area.width == 0 || area.height == 0 {
        return rects;
    }
    let active = active_tab.min(n - 1);

    // Total columns the window [lo, hi] would occupy: tab widths + inter-tab
    // gaps + an indicator reservation on each side that still hides tabs (a
    // fully-shown side reclaims its reserved columns).
    let footprint = |lo: usize, hi: usize| -> u16 {
        let mut total: u16 = 0;
        for chrome in &chromes[lo..=hi] {
            total = total.saturating_add(tab_width(chrome, mode));
        }
        let gaps = u16::try_from(hi - lo).unwrap_or(u16::MAX);
        total = total.saturating_add(gaps);
        if lo > 0 {
            total = total.saturating_add(reserve_left);
        }
        if hi + 1 < n {
            total = total.saturating_add(reserve_right);
        }
        total
    };

    // Window of visible tab indices [lo, hi], grown around the active tab.
    let mut lo = active;
    let mut hi = active;

    loop {
        let can_extend_left = lo > 0;
        let can_extend_right = hi + 1 < n;
        if !can_extend_left && !can_extend_right {
            break;
        }

        // Prefer the lighter side (fewer tabs added so far) for balance; on a
        // tie, extend left first (matches zellij's bias toward earlier tabs).
        let left_count = active - lo;
        let right_count = hi - active;
        let prefer_left = can_extend_left && (left_count <= right_count || !can_extend_right);

        let mut progressed = false;
        if prefer_left {
            if footprint(lo - 1, hi) <= area.width {
                lo -= 1;
                progressed = true;
            } else if can_extend_right && footprint(lo, hi + 1) <= area.width {
                hi += 1;
                progressed = true;
            }
        } else if can_extend_right {
            if footprint(lo, hi + 1) <= area.width {
                hi += 1;
                progressed = true;
            } else if can_extend_left && footprint(lo - 1, hi) <= area.width {
                lo -= 1;
                progressed = true;
            }
        }

        if !progressed {
            break;
        }
    }

    // Lay the window [lo, hi] left-to-right starting at area.x (after the left
    // indicator reservation when present), with 1-col gaps. A single active tab
    // wider than the bar is clipped to the remaining width so the row is never
    // blank.
    let left_gutter = if lo > 0 { reserve_left } else { 0 };
    let mut x = area.x.saturating_add(left_gutter);
    let right_limit = area.x.saturating_add(area.width);
    for idx in lo..=hi {
        let desired = tab_width(&chromes[idx], mode);
        let remaining = right_limit.saturating_sub(x);
        if remaining == 0 {
            break;
        }
        let width = desired.min(remaining);
        rects[idx] = Rect::new(x, area.y, width, 1);
        x = x.saturating_add(width).saturating_add(1);
    }

    rects
}

/// First and last visible (width>0) tab indices in a dense hit-area vec.
fn visible_bounds(rects: &[Rect]) -> Option<(usize, usize)> {
    let first = rects.iter().position(|r| r.width > 0)?;
    let last = rects.iter().rposition(|r| r.width > 0)?;
    Some((first, last))
}

pub(crate) fn compute_tab_bar_view(
    chromes: Vec<TabChrome>,
    active_tab: usize,
    mode: TabStatusMode,
    area: Rect,
    mouse_chrome: bool,
) -> TabBarView {
    if area.width == 0 || area.height == 0 {
        return TabBarView::default();
    }

    let area_right = area.x + area.width;
    // Reserve the new-tab button column only under mouse chrome.
    let new_tab_reserve = if mouse_chrome { NEW_TAB_WIDTH } else { 0 };
    let tabs_area = Rect::new(
        area.x,
        area.y,
        area.width.saturating_sub(new_tab_reserve),
        area.height,
    );

    // First pass: no indicator reservation, to learn whether anything overflows.
    let probe = centered_active_fill(&chromes, active_tab, mode, tabs_area, 0, 0);
    let tab_count = chromes.len();
    let probe_overflow = probe.iter().any(|r| r.width == 0) && tab_count > 0;

    let (tab_hit_areas, overflow) = if !probe_overflow {
        // Everything fits — no indicators.
        (probe, TabBarOverflow::default())
    } else {
        // Reserve indicator columns on each side that overflows. We don't yet
        // know which sides overflow, so reserve on both and let the fill's
        // "last hidden tab removes the indicator" logic reclaim a side that
        // ends up fully shown.
        let rects = centered_active_fill(
            &chromes,
            active_tab,
            mode,
            tabs_area,
            OVERFLOW_INDICATOR_WIDTH,
            OVERFLOW_INDICATOR_WIDTH,
        );
        let (first, last) = visible_bounds(&rects).unwrap_or((active_tab, active_tab));

        let left_hidden = first;
        let right_hidden = tab_count.saturating_sub(last + 1);

        let left = (left_hidden > 0).then(|| HiddenGroup {
            count: left_hidden,
            // Nearest hidden tab on the left is just before the first visible.
            jump_to: first.saturating_sub(1),
        });
        let right = (right_hidden > 0).then(|| HiddenGroup {
            count: right_hidden,
            // Nearest hidden tab on the right is just after the last visible.
            jump_to: last + 1,
        });

        // Indicator rects sit in the columns the fill reserved for them. The
        // left indicator hugs area.x; the right indicator sits at the right edge
        // of the tabs area (before any new-tab button), where the fill reserved
        // OVERFLOW_INDICATOR_WIDTH columns.
        let left_hit_area = if left.is_some() {
            Rect::new(area.x, area.y, OVERFLOW_INDICATOR_WIDTH.min(area.width), 1)
        } else {
            Rect::default()
        };
        let right_hit_area = if right.is_some() {
            let tabs_right = tabs_area.x + tabs_area.width;
            let rx = tabs_right
                .saturating_sub(OVERFLOW_INDICATOR_WIDTH)
                .max(area.x);
            Rect::new(rx, area.y, tabs_right.saturating_sub(rx), 1)
        } else {
            Rect::default()
        };

        (
            rects,
            TabBarOverflow {
                left,
                right,
                left_hit_area,
                right_hit_area,
            },
        )
    };

    let new_tab_hit_area = if mouse_chrome {
        // Place the new-tab button just past the rightmost chrome: either the
        // last visible tab, or the right overflow indicator when present.
        let trailing = trailing_tab_controls_x(&tab_hit_areas, tabs_area.x).max(
            overflow
                .right_hit_area
                .x
                .saturating_add(overflow.right_hit_area.width),
        );
        let new_tab_x = trailing.min(area_right.saturating_sub(NEW_TAB_WIDTH).max(area.x));
        Rect::new(
            new_tab_x,
            area.y,
            area_right.saturating_sub(new_tab_x).min(NEW_TAB_WIDTH),
            1,
        )
    } else {
        Rect::default()
    };

    tracing::debug!(
        area_width = area.width,
        tab_count,
        ?mode,
        active_tab,
        left_hidden = overflow.left.map(|g| g.count).unwrap_or(0),
        right_hidden = overflow.right.map(|g| g.count).unwrap_or(0),
        visible_count = tab_hit_areas.iter().filter(|r| r.width > 0).count(),
        "tab bar overflow"
    );

    TabBarView {
        tab_hit_areas,
        tab_chrome: chromes,
        tab_status_mode: mode,
        overflow,
        new_tab_hit_area,
    }
}

fn tab_drop_indicator_x(
    app: &AppState,
    ws: &crate::workspace::Workspace,
    insert_idx: usize,
) -> Option<u16> {
    let mut visible_tabs = app
        .view
        .tab_hit_areas
        .iter()
        .enumerate()
        .filter(|(_, rect)| rect.width > 0);
    let first_visible = visible_tabs.clone().next()?;
    let last_visible = visible_tabs.next_back().unwrap_or(first_visible);
    let overflow = &app.view.tab_overflow;

    if insert_idx == 0 {
        // When the left edge is clipped, anchor on the left overflow indicator
        // (just past it) rather than the first visible tab, which is not tab 0.
        return Some(if first_visible.0 == 0 {
            first_visible.1.x
        } else {
            overflow.left_hit_area.x + overflow.left_hit_area.width
        });
    }

    if let Some((_, rect)) = app
        .view
        .tab_hit_areas
        .iter()
        .enumerate()
        .find(|(idx, rect)| *idx == insert_idx && rect.width > 0)
    {
        return Some(rect.x.saturating_sub(1));
    }

    if insert_idx >= ws.tabs.len() {
        // When the right edge is clipped, anchor just before the right overflow
        // indicator rather than after the last visible tab.
        return Some(if last_visible.0 + 1 >= ws.tabs.len() {
            last_visible.1.x + last_visible.1.width
        } else {
            overflow.right_hit_area.x.saturating_sub(1)
        });
    }

    None
}

/// Background color for a visible tab, used both to paint the tab and to color
/// the Powerline arrow transition. Inactive tabs alternate `surface0`/`surface1`
/// by tab index (zellij-style banding); the active tab uses the accent.
fn tab_bg(p: &crate::app::state::Palette, idx: usize, active: bool) -> ratatui::style::Color {
    if active {
        p.accent
    } else if idx.is_multiple_of(2) {
        p.surface0
    } else {
        p.surface1
    }
}

pub(super) fn render_tab_bar(app: &AppState, frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let Some(active_ws_idx) = app.active else {
        return;
    };
    let Some(ws) = app.workspaces.get(active_ws_idx) else {
        return;
    };

    let p = &app.palette;
    let separator = if app.tabs_powerline {
        SeparatorStyle::Powerline
    } else {
        SeparatorStyle::AlternatingBg
    };

    frame.render_widget(
        Paragraph::new(" ".repeat(area.width as usize)).style(Style::default().bg(p.panel_bg)),
        area,
    );

    let overflow = &app.view.tab_overflow;

    // Overflow indicators. Under mouse chrome they are clickable `← +N` / `+N →`
    // affordances; without mouse chrome they are non-clickable `← N` / `N →`
    // markers. Counts cap at `9+`.
    let fmt_count = |n: usize| -> String {
        if n > 9 {
            "9+".to_string()
        } else {
            n.to_string()
        }
    };
    if let Some(group) = overflow.left {
        if overflow.left_hit_area.width > 0 {
            let label = if app.mouse_capture {
                format!("←+{}", fmt_count(group.count))
            } else {
                format!("←{} ", fmt_count(group.count))
            };
            frame.render_widget(
                Paragraph::new(label).style(Style::default().fg(p.overlay1).bg(p.surface0)),
                overflow.left_hit_area,
            );
        }
    }
    if let Some(group) = overflow.right {
        if overflow.right_hit_area.width > 0 {
            let label = if app.mouse_capture {
                format!("+{}→", fmt_count(group.count))
            } else {
                format!(" {}→", fmt_count(group.count))
            };
            frame.render_widget(
                Paragraph::new(label).style(Style::default().fg(p.overlay1).bg(p.surface0)),
                overflow.right_hit_area,
            );
        }
    }

    // Track the previous visible tab's rect + bg to paint a Powerline arrow in
    // the inter-tab gap (fg = left tab bg, bg = right tab bg).
    let mut prev: Option<(Rect, ratatui::style::Color)> = None;

    for (idx, tab) in ws.tabs.iter().enumerate() {
        let Some(rect) = app.view.tab_hit_areas.get(idx).copied() else {
            break;
        };
        if rect.width == 0 {
            continue;
        }
        let active = idx == ws.active_tab;
        let bg = tab_bg(p, idx, active);
        let style = if active {
            let base = Style::default().fg(panel_contrast_fg(p)).bg(bg);
            if tab.is_auto_named() {
                base.add_modifier(Modifier::DIM)
            } else {
                base.add_modifier(Modifier::BOLD)
            }
        } else if tab.is_auto_named() {
            Style::default()
                .fg(p.overlay0)
                .bg(bg)
                .add_modifier(Modifier::DIM)
        } else {
            Style::default().fg(p.overlay1).bg(bg)
        };

        // Powerline arrow in the gap between the previous visible tab and this
        // one. Only emitted under `SeparatorStyle::Powerline`; `AlternatingBg`
        // relies on the banded backgrounds alone and emits no Powerline glyph.
        if separator == SeparatorStyle::Powerline {
            if let Some((prev_rect, prev_bg)) = prev {
                let gap_x = prev_rect.x + prev_rect.width;
                if gap_x < rect.x && gap_x < area.x + area.width {
                    frame.buffer_mut()[(gap_x, area.y)]
                        .set_symbol(POWERLINE_ARROW)
                        .set_style(Style::default().fg(prev_bg).bg(bg));
                }
            }
        }

        let spans = if let Some(chrome) = app.view.tab_chrome.get(idx) {
            chrome.to_spans(app.view.tab_status_mode, rect.width)
        } else {
            vec![Span::raw(" ".repeat(rect.width as usize))]
        };
        frame.render_widget(Paragraph::new(Line::from(spans)).style(style), rect);
        prev = Some((rect, bg));
    }

    if let Some(crate::app::state::DragState {
        target:
            crate::app::state::DragTarget::TabReorder {
                ws_idx,
                insert_idx: Some(insert_idx),
                ..
            },
    }) = &app.drag
    {
        if *ws_idx == active_ws_idx {
            if let Some(x) = tab_drop_indicator_x(app, ws, *insert_idx) {
                frame.buffer_mut()[(x.min(area.x + area.width.saturating_sub(1)), area.y)]
                    .set_symbol("│")
                    .set_style(Style::default().fg(p.accent));
            }
        }
    }

    if app.mouse_capture && app.view.new_tab_hit_area.width > 0 {
        frame.render_widget(
            Paragraph::new(" + ").style(Style::default().fg(p.overlay1)),
            app.view.new_tab_hit_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AppState;
    use crate::workspace::Workspace;
    use ratatui::{backend::TestBackend, Terminal};

    fn buffer_row_text(buffer: &ratatui::buffer::Buffer, area: Rect, row: u16) -> String {
        (area.x..area.x + area.width)
            .map(|x| buffer[(x, row)].symbol())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    fn make_ws_with_tabs(names: &[&str]) -> crate::workspace::Workspace {
        assert!(
            !names.is_empty(),
            "make_ws_with_tabs requires at least one tab"
        );
        let mut ws = Workspace::test_new("test");
        ws.tabs[0].set_custom_name(names[0].to_string());
        for &name in &names[1..] {
            ws.test_add_tab(Some(name));
        }
        ws
    }

    fn chromes_from_ws(ws: &crate::workspace::Workspace) -> Vec<TabChrome> {
        (0..ws.tabs.len())
            .map(|i| {
                let name = ws
                    .tab_display_name(i)
                    .unwrap_or_else(|| (i + 1).to_string());
                let zoomed = ws.tabs.get(i).is_some_and(|tab| tab.zoomed);
                TabChrome {
                    status: None,
                    name,
                    zoomed,
                }
            })
            .collect()
    }

    fn chromes_from_names(names: &[&str]) -> Vec<TabChrome> {
        names
            .iter()
            .map(|&name| TabChrome {
                status: None,
                name: name.to_string(),
                zoomed: false,
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // Stateless centered-active fill: visibility, overflow, jump targets
    // -----------------------------------------------------------------------

    #[test]
    fn all_tabs_visible_when_they_fit() {
        let chromes = chromes_from_names(&["ab", "cd", "ef"]);
        let view =
            compute_tab_bar_view(chromes, 0, TabStatusMode::Off, Rect::new(0, 0, 60, 1), true);
        assert!(view.tab_hit_areas.iter().all(|r| r.width > 0));
        assert_eq!(view.overflow, TabBarOverflow::default());
    }

    #[test]
    fn active_tab_visible_at_full_width_with_many_tabs() {
        // 10 tabs, narrow bar, active in the middle: active must be visible at
        // its full natural width, with overflow indicators on both sides.
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_from_names(&refs);
        let active = 5;
        let view = compute_tab_bar_view(
            chromes.clone(),
            active,
            TabStatusMode::Off,
            Rect::new(0, 0, 40, 1),
            true,
        );
        let natural = tab_width(&chromes[active], TabStatusMode::Off);
        assert!(view.tab_hit_areas[active].width > 0);
        assert_eq!(
            view.tab_hit_areas[active].width, natural,
            "active tab must render at full natural width"
        );
        assert!(view.overflow.left.is_some());
        assert!(view.overflow.right.is_some());
    }

    #[test]
    fn overflow_jump_targets_point_at_hidden_tabs_in_range() {
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_from_names(&refs);
        let view =
            compute_tab_bar_view(chromes, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
        let n = view.tab_hit_areas.len();
        let left = view.overflow.left.expect("left overflow");
        let right = view.overflow.right.expect("right overflow");
        assert!(left.jump_to < n);
        assert!(right.jump_to < n);
        // Jump targets are hidden (width 0) — the nearest hidden tab each side.
        assert_eq!(view.tab_hit_areas[left.jump_to].width, 0);
        assert_eq!(view.tab_hit_areas[right.jump_to].width, 0);
        // Counts add up: left_hidden + visible + right_hidden == tab count.
        let visible = view.tab_hit_areas.iter().filter(|r| r.width > 0).count();
        assert_eq!(left.count + visible + right.count, n);
    }

    #[test]
    fn active_first_tab_only_overflows_right() {
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_from_names(&refs);
        let view =
            compute_tab_bar_view(chromes, 0, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
        assert!(view.tab_hit_areas[0].width > 0);
        assert!(view.overflow.left.is_none(), "no hidden tabs to the left");
        assert!(view.overflow.right.is_some());
    }

    #[test]
    fn active_last_tab_only_overflows_left() {
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_from_names(&refs);
        let last = 9;
        let view = compute_tab_bar_view(
            chromes,
            last,
            TabStatusMode::Off,
            Rect::new(0, 0, 40, 1),
            true,
        );
        assert!(view.tab_hit_areas[last].width > 0);
        assert!(view.overflow.left.is_some());
        assert!(view.overflow.right.is_none(), "no hidden tabs to the right");
    }

    #[test]
    fn indicator_hit_zone_is_touch_adequate() {
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_from_names(&refs);
        let view =
            compute_tab_bar_view(chromes, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
        assert!(
            view.overflow.left_hit_area.width >= 3,
            "left indicator must be a touch-adequate hit zone, not 1 cell"
        );
        assert!(
            view.overflow.right_hit_area.width >= 1,
            "right indicator clipped only by the bar edge"
        );
    }

    #[test]
    fn dense_hit_area_len_equals_tab_count() {
        let names: Vec<String> = (0..12).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        for active in [0, 6, 11] {
            for width in 20..60 {
                let chromes = chromes_from_names(&refs);
                let view = compute_tab_bar_view(
                    chromes,
                    active,
                    TabStatusMode::Off,
                    Rect::new(0, 0, width, 1),
                    true,
                );
                assert_eq!(view.tab_hit_areas.len(), refs.len());
                assert!(
                    view.tab_hit_areas[active].width > 0,
                    "active={active} width={width}: active tab hidden"
                );
            }
        }
    }

    #[test]
    fn compute_tab_bar_view_returns_default_for_zero_width_area() {
        let chromes = chromes_from_names(&["aa", "bb"]);
        let view =
            compute_tab_bar_view(chromes, 0, TabStatusMode::Off, Rect::new(0, 0, 0, 1), true);
        assert!(view.tab_hit_areas.is_empty());
        assert_eq!(view.overflow, TabBarOverflow::default());
        assert_eq!(view.new_tab_hit_area.width, 0);
    }

    #[test]
    fn non_mouse_branch_uses_centered_fill_and_no_clickable_chrome() {
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_from_names(&refs);
        let view = compute_tab_bar_view(
            chromes,
            5,
            TabStatusMode::Off,
            Rect::new(0, 0, 40, 1),
            false,
        );
        // Active tab still placed via the centered fill.
        assert!(view.tab_hit_areas[5].width > 0);
        // Overflow groups still computed (for the non-clickable marker render).
        assert!(view.overflow.left.is_some());
        assert!(view.overflow.right.is_some());
        // No new-tab button without mouse chrome.
        assert_eq!(view.new_tab_hit_area.width, 0);
    }

    #[test]
    fn position_index_round_trip_for_visible_tabs() {
        // Every visible rect's center maps back to its own tab index via the
        // same predicate `tab_at` uses.
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_from_names(&refs);
        let view =
            compute_tab_bar_view(chromes, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
        for (idx, rect) in view.tab_hit_areas.iter().enumerate() {
            if rect.width == 0 {
                continue;
            }
            let center = rect.x + rect.width / 2;
            let found = view
                .tab_hit_areas
                .iter()
                .enumerate()
                .find(|(_, r)| r.width > 0 && center >= r.x && center < r.x + r.width)
                .map(|(i, _)| i);
            assert_eq!(found, Some(idx), "tab {idx} center did not round-trip");
        }
    }

    #[test]
    fn round_trip_with_adversarial_unicode_names() {
        // CJK (wide), combining, and ZWJ emoji names: the fill uses Unicode
        // display width, so visible rects must still round-trip cleanly.
        let chromes = chromes_from_names(&[
            "你好世界",
            "e\u{0301}ditor",
            "👨\u{200d}💻",
            "tab-d",
            "넓은탭이름",
            "f",
            "g",
            "h",
        ]);
        for active in [0, 3, 7] {
            let view = compute_tab_bar_view(
                chromes.clone(),
                active,
                TabStatusMode::Off,
                Rect::new(0, 0, 30, 1),
                true,
            );
            assert_eq!(view.tab_hit_areas.len(), 8);
            assert!(view.tab_hit_areas[active].width > 0);
            for (idx, rect) in view.tab_hit_areas.iter().enumerate() {
                if rect.width == 0 {
                    continue;
                }
                let center = rect.x + rect.width / 2;
                let found = view
                    .tab_hit_areas
                    .iter()
                    .enumerate()
                    .find(|(_, r)| r.width > 0 && center >= r.x && center < r.x + r.width)
                    .map(|(i, _)| i);
                assert_eq!(found, Some(idx), "active={active}: tab {idx} round-trip");
            }
        }
    }

    #[test]
    fn width_accounting_at_overflow_boundary() {
        // Sweep widths around the fit/overflow boundary; the active tab is never
        // hidden and the visible window is always a contiguous run.
        let names: Vec<String> = (0..6).map(|i| format!("name{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        for width in 12..70 {
            let chromes = chromes_from_names(&refs);
            let view = compute_tab_bar_view(
                chromes,
                3,
                TabStatusMode::Off,
                Rect::new(0, 0, width, 1),
                true,
            );
            assert!(
                view.tab_hit_areas[3].width > 0,
                "width={width}: active hidden"
            );
            // Visible indices form a contiguous range.
            let visible: Vec<usize> = view
                .tab_hit_areas
                .iter()
                .enumerate()
                .filter(|(_, r)| r.width > 0)
                .map(|(i, _)| i)
                .collect();
            if let (Some(&first), Some(&last)) = (visible.first(), visible.last()) {
                assert_eq!(
                    visible.len(),
                    last - first + 1,
                    "width={width}: visible window not contiguous: {visible:?}"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // TabChrome: width + spans + Unicode + sanitization
    // -----------------------------------------------------------------------

    #[test]
    fn zoom_marker_counts_toward_tab_width() {
        let chrome = TabChrome {
            status: None,
            name: "abcdefgh".into(),
            zoomed: true,
        };
        assert_eq!(tab_width(&chrome, TabStatusMode::Off), 14);
    }

    #[test]
    fn display_width_uses_unicode_display_columns() {
        // CJK glyphs are 2 columns each.
        let c = TabChrome {
            status: None,
            name: "你好".into(),
            zoomed: false,
        };
        assert_eq!(c.display_width(TabStatusMode::Off), 4);

        // Combining mark contributes 0 columns: "a" + U+0300 = 1 column.
        let c = TabChrome {
            status: None,
            name: "a\u{0300}".into(),
            zoomed: false,
        };
        assert_eq!(c.display_width(TabStatusMode::Off), 1);
    }

    #[test]
    fn display_width_invariants() {
        let c = TabChrome {
            status: None,
            name: "hello".into(),
            zoomed: false,
        };
        assert_eq!(c.display_width(TabStatusMode::Off), 5);
        let c = TabChrome {
            status: None,
            name: "hello".into(),
            zoomed: true,
        };
        assert_eq!(c.display_width(TabStatusMode::Off), 7);
        let c = TabChrome {
            status: None,
            name: "hello".into(),
            zoomed: false,
        };
        assert_eq!(c.display_width(TabStatusMode::All), 7);
        let c = TabChrome {
            status: Some(TabStatusDot {
                glyph: "●",
                style: Style::default(),
            }),
            name: "hello".into(),
            zoomed: true,
        };
        assert_eq!(c.display_width(TabStatusMode::All), 9);
    }

    #[test]
    fn to_spans_ordering_and_padding() {
        let c = TabChrome {
            status: Some(TabStatusDot {
                glyph: "●",
                style: Style::default().fg(ratatui::style::Color::Red),
            }),
            name: "test".into(),
            zoomed: true,
        };
        let spans = c.to_spans(TabStatusMode::All, 15);
        assert_eq!(spans[0].content.as_ref(), " ");
        assert_eq!(spans[1].content.as_ref(), "●");
        assert_eq!(spans[2].content.as_ref(), " ");
        assert_eq!(spans[3].content.as_ref(), "test");
        assert_eq!(spans[4].content.as_ref(), " Z");
        assert_eq!(spans[5].content.len(), 6);

        let c = TabChrome {
            status: None,
            name: "abc".into(),
            zoomed: false,
        };
        let spans = c.to_spans(TabStatusMode::All, 10);
        assert_eq!(spans[0].content.as_ref(), " ");
        assert_eq!(spans[1].content.as_ref(), "  ");
        assert!(spans[1].style.fg.is_none(), "empty slot must not set fg");
        assert_eq!(spans[2].content.as_ref(), "abc");
        assert_eq!(spans[3].content.len(), 4);

        let c = TabChrome {
            status: None,
            name: "xyz".into(),
            zoomed: false,
        };
        let spans = c.to_spans(TabStatusMode::Off, 8);
        assert_eq!(spans[0].content.as_ref(), " ");
        assert_eq!(spans[1].content.as_ref(), "xyz");
        assert_eq!(spans[2].content.len(), 4);
    }

    #[test]
    fn to_spans_truncates_over_wide_name_by_display_width() {
        let c = TabChrome {
            status: None,
            name: "longername".into(),
            zoomed: false,
        };
        // mode=Off, no zoom: name_budget = rect_width - 1 = 4 → "lon…".
        let spans = c.to_spans(TabStatusMode::Off, 5);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains('…'), "expected ellipsis truncation: {text:?}");
        assert!(text.contains("lon"), "kept a prefix of the name: {text:?}");
    }

    #[test]
    fn to_spans_no_truncation_when_name_fits() {
        let c = TabChrome {
            status: None,
            name: "abc".into(),
            zoomed: false,
        };
        let spans = c.to_spans(TabStatusMode::Off, 8);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("abc"));
        assert!(!text.contains('…'));
    }

    #[test]
    fn sanitize_display_name_strips_bidi_and_control() {
        assert_eq!(sanitize_display_name("a\u{202e}b\x01c"), "abc");
        // Clean input borrows.
        assert!(matches!(
            sanitize_display_name("clean"),
            Cow::Borrowed("clean")
        ));
    }

    #[test]
    fn to_spans_sanitizes_name_at_render_chokepoint() {
        let c = TabChrome {
            status: None,
            name: "ev\u{202e}il".into(),
            zoomed: false,
        };
        let spans = c.to_spans(TabStatusMode::Off, 12);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            !text.contains('\u{202e}'),
            "bidi override must be stripped: {text:?}"
        );
        assert!(text.contains("evil"));
    }

    // -----------------------------------------------------------------------
    // Render: separators, coloring, indicators
    // -----------------------------------------------------------------------

    fn render_to_buffer(app: &AppState, area: Rect) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(area.x + area.width, area.y + area.height.max(1));
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_tab_bar(app, frame, area))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn app_with_tab_bar(names: &[&str], active: usize, area: Rect, mouse_chrome: bool) -> AppState {
        let mut app = AppState::test_new();
        let mut ws = make_ws_with_tabs(names);
        ws.active_tab = active;
        app.workspaces = vec![ws];
        app.active = Some(0);
        app.mouse_capture = mouse_chrome;
        app.view.tab_bar_rect = area;
        let chromes = chromes_from_ws(&app.workspaces[0]);
        let view = compute_tab_bar_view(chromes, active, TabStatusMode::Off, area, mouse_chrome);
        app.view.tab_hit_areas = view.tab_hit_areas;
        app.view.tab_chrome = view.tab_chrome;
        app.view.tab_status_mode = view.tab_status_mode;
        app.view.tab_overflow = view.overflow;
        app.view.new_tab_hit_area = view.new_tab_hit_area;
        app
    }

    #[test]
    fn render_active_tab_paints_accent_bg() {
        let mut app = app_with_tab_bar(&["ab", "cd", "ef"], 0, Rect::new(0, 0, 40, 1), false);
        app.show_tab_status = TabStatusMode::All;
        let active_rect = app.view.tab_hit_areas[0];
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        for x in active_rect.x..active_rect.x + active_rect.width {
            assert_eq!(
                buffer[(x, 0)].bg,
                app.palette.accent,
                "active tab cell at x={x} should have accent bg"
            );
        }
    }

    #[test]
    fn render_inactive_tabs_alternate_background() {
        // Banding: even-index inactive tabs use surface0, odd use surface1.
        let app = app_with_tab_bar(&["aa", "bb", "cc"], 0, Rect::new(0, 0, 60, 1), false);
        let p = &app.palette;
        let r1 = app.view.tab_hit_areas[1];
        let r2 = app.view.tab_hit_areas[2];
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        assert_eq!(buffer[(r1.x, 0)].bg, p.surface1, "tab 1 should be surface1");
        assert_eq!(buffer[(r2.x, 0)].bg, p.surface0, "tab 2 should be surface0");
    }

    #[test]
    fn render_powerline_emits_arrow_when_enabled() {
        let mut app = app_with_tab_bar(&["aa", "bb", "cc"], 0, Rect::new(0, 0, 60, 1), false);
        app.tabs_powerline = true;
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        let row = buffer_row_text(&buffer, app.view.tab_bar_rect, 0);
        assert!(
            row.contains(POWERLINE_ARROW),
            "Powerline arrow expected when powerline on: {row:?}"
        );
    }

    #[test]
    fn render_alternating_bg_emits_no_powerline_codepoints() {
        let mut app = app_with_tab_bar(&["aa", "bb", "cc"], 0, Rect::new(0, 0, 60, 1), false);
        app.tabs_powerline = false;
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        let area = app.view.tab_bar_rect;
        for x in area.x..area.x + area.width {
            let sym = buffer[(x, 0)].symbol();
            assert!(
                !sym.contains(POWERLINE_ARROW),
                "AlternatingBg path must emit zero Powerline codepoints, found at x={x}"
            );
            // Only single-cell glyphs in the OFF path.
            assert!(sym.width() <= 1, "OFF path emitted a wide glyph {sym:?}");
        }
    }

    #[test]
    fn render_mouse_indicators_show_plus_counts() {
        let names: Vec<String> = (0..10).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let app = app_with_tab_bar(&refs, 5, Rect::new(0, 0, 40, 1), true);
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        let row = buffer_row_text(&buffer, app.view.tab_bar_rect, 0);
        assert!(
            row.contains('+'),
            "mouse indicators show +N counts: {row:?}"
        );
        assert!(row.contains('←') || row.contains('→'), "arrows: {row:?}");
    }

    #[test]
    fn render_non_mouse_marker_is_present_without_plus() {
        let names: Vec<String> = (0..10).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let app = app_with_tab_bar(&refs, 5, Rect::new(0, 0, 40, 1), false);
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        let row = buffer_row_text(&buffer, app.view.tab_bar_rect, 0);
        // Non-mouse markers use the arrow + count, but no clickable "+".
        assert!(
            row.contains('←') || row.contains('→'),
            "marker arrow: {row:?}"
        );
    }

    #[test]
    fn render_count_caps_at_nine_plus() {
        let names: Vec<String> = (0..15).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        // Active near the end so >9 hidden on the left.
        let app = app_with_tab_bar(&refs, 14, Rect::new(0, 0, 30, 1), true);
        assert!(app.view.tab_overflow.left.unwrap().count > 9);
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        let row = buffer_row_text(&buffer, app.view.tab_bar_rect, 0);
        assert!(row.contains("9+"), "expected 9+ cap: {row:?}");
    }

    // -----------------------------------------------------------------------
    // Drop-indicator re-coupling
    // -----------------------------------------------------------------------

    #[test]
    fn drop_indicator_x_at_start_returns_first_tab_x() {
        let app = app_with_tab_bar(&["ab", "cd", "ef"], 0, Rect::new(0, 0, 50, 1), true);
        assert_eq!(
            tab_drop_indicator_x(&app, &app.workspaces[0], 0),
            Some(app.view.tab_hit_areas[0].x)
        );
    }

    #[test]
    fn drop_indicator_x_between_tabs_is_one_before_target() {
        let app = app_with_tab_bar(&["ab", "cd", "ef"], 0, Rect::new(0, 0, 50, 1), true);
        let r1 = app.view.tab_hit_areas[1];
        assert_eq!(
            tab_drop_indicator_x(&app, &app.workspaces[0], 1),
            Some(r1.x.saturating_sub(1))
        );
    }

    #[test]
    fn drop_indicator_x_at_start_uses_left_indicator_when_left_clipped() {
        let names: Vec<String> = (0..10).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let app = app_with_tab_bar(&refs, 5, Rect::new(0, 0, 40, 1), true);
        assert!(app.view.tab_hit_areas[0].width == 0, "left edge clipped");
        let overflow = &app.view.tab_overflow;
        let expected = overflow.left_hit_area.x + overflow.left_hit_area.width;
        assert_eq!(
            tab_drop_indicator_x(&app, &app.workspaces[0], 0),
            Some(expected)
        );
    }

    #[test]
    fn drop_indicator_x_at_end_uses_right_indicator_when_right_clipped() {
        let names: Vec<String> = (0..10).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let app = app_with_tab_bar(&refs, 5, Rect::new(0, 0, 40, 1), true);
        let last = refs.len() - 1;
        assert!(
            app.view.tab_hit_areas[last].width == 0,
            "right edge clipped"
        );
        let expected = app.view.tab_overflow.right_hit_area.x.saturating_sub(1);
        assert_eq!(
            tab_drop_indicator_x(&app, &app.workspaces[0], refs.len()),
            Some(expected)
        );
    }

    #[test]
    fn truncate_to_width_respects_wide_glyphs() {
        // "你好世界" is 8 columns; budget 5 → 4 columns of content + ellipsis.
        let out = truncate_to_width("你好世界", 5);
        assert!(out.ends_with('…'));
        assert!(
            out.width() <= 5,
            "must not exceed budget: {out:?} ({})",
            out.width()
        );
    }
}
