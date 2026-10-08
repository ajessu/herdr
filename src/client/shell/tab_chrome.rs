//! Fork: zellij-style tab bar (`ui.tabs.style = "zellij"`).
//!
//! Tabs are tiles that abut, each with its own Powerline wedges (or
//! alternating backgrounds when `ui.tabs.powerline` is off). The visible window
//! grows outward from the active tab; tabs that do not fit collapse into
//! `← +N` / `+N →` tiles that count hidden blocked, working and finished
//! agents, and a click on a tile focuses the most urgent hidden tab.
//!
//! Layout and painting are ported from the fork's pre-v0.9.0 `src/ui/tabs.rs`
//! (minus its wheel-to-pan browse mode). Upstream's `render_tab_bar` calls in
//! through one branch; drag-reorder, the drop marker and the right-side status
//! keep running on upstream code over the hit rects this module records.

use std::borrow::Cow;

use ratatui::style::Color;
use ratatui::text::{Line, Span};

use super::overflow::{self, ListWindow, OverflowSide};
use super::*;
use crate::api::schema::AgentStatus;
use crate::config::TabStatusModeConfig;

/// Each tab owns a left and a right wedge column; adjacent tabs abut.
const TAB_SEPARATOR_OVERHEAD: u16 = 2;
/// Base width of an overflow tile: 2 wedges plus zellij's ` ← +N ` interior.
/// Also a touch-adequate click target.
const OVERFLOW_INDICATOR_WIDTH: u16 = 8;
const POWERLINE_ARROW: &str = "\u{e0b0}";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeparatorStyle {
    Powerline,
    AlternatingBg,
}

/// Strip control and bidi-override characters from a user-writable tab name.
fn sanitize_display_name(s: &str) -> Cow<'_, str> {
    if s.chars().any(|c| c.is_control() || is_bidi_override(c)) {
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

#[derive(Debug, Clone)]
struct TabStatusDot {
    glyph: &'static str,
    style: Style,
}

/// One tab's label model.
#[derive(Debug, Clone)]
struct TabChrome {
    status: Option<TabStatusDot>,
    name: String,
    zoomed: bool,
    custom_label: bool,
    /// Classifies a hidden tab for the overflow badges; `None` when
    /// `show_tab_status` is off, so no badges appear.
    agent_status: Option<AgentStatus>,
}

impl TabChrome {
    /// Columns of the status dot plus its separating space, or 0. Shared by
    /// sizing and filling so the two can never disagree.
    fn status_cols(&self) -> u16 {
        self.status
            .as_ref()
            .map(|dot| {
                u16::try_from(dot.glyph.width())
                    .unwrap_or(u16::MAX)
                    .saturating_add(1)
            })
            .unwrap_or(0)
    }

    fn display_width(&self) -> u16 {
        let name_w = u16::try_from(sanitize_display_name(&self.name).width()).unwrap_or(u16::MAX);
        let zoom_w: u16 = if self.zoomed { 2 } else { 0 };
        self.status_cols()
            .saturating_add(name_w)
            .saturating_add(zoom_w)
    }

    /// ` [dot ]name[ Z]` padded to `rect_width`. Never truncated: an over-wide
    /// name is clipped at the rect, with no ellipsis (zellij).
    fn to_spans(&self, rect_width: u16) -> Vec<Span<'static>> {
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(6);
        if let Some(ref dot) = self.status {
            spans.push(Span::styled(dot.glyph, dot.style));
            spans.push(Span::raw(" "));
        }
        spans.push(Span::raw(sanitize_display_name(&self.name).into_owned()));
        if self.zoomed {
            spans.push(Span::raw(" Z"));
        }
        let content_width = spans.iter().fold(0u16, |acc, s| {
            acc.saturating_add(u16::try_from(s.content.width()).unwrap_or(u16::MAX))
        });
        let right = rect_width.saturating_sub(content_width.saturating_add(1));
        let mut out = Vec::with_capacity(spans.len() + 2);
        out.push(Span::raw(" "));
        out.extend(spans);
        out.push(Span::raw(" ".repeat(usize::from(right))));
        out
    }
}

fn tab_status_dot(
    status: AgentStatus,
    mode: TabStatusModeConfig,
    config: &ClientShellConfig,
) -> Option<TabStatusDot> {
    let visible = match mode {
        TabStatusModeConfig::Off => false,
        TabStatusModeConfig::Attention => {
            matches!(status, AgentStatus::Blocked | AgentStatus::Done)
        }
        TabStatusModeConfig::All => status != AgentStatus::Unknown,
    };
    visible.then(|| TabStatusDot {
        glyph: status_icon(status, config.status_indicators),
        style: Style::default().fg(status_color(status, &config.palette)),
    })
}

fn build_chromes(tabs: &[&ClientShellTab], config: &ClientShellConfig) -> Vec<TabChrome> {
    tabs.iter()
        .map(|tab| TabChrome {
            status: tab_status_dot(tab.agent_status, config.tab_status, config),
            name: tab.label.clone(),
            zoomed: tab.zoomed,
            custom_label: tab.custom_label,
            agent_status: (config.tab_status != TabStatusModeConfig::Off)
                .then_some(tab.agent_status),
        })
        .collect()
}

/// Full count, or `many` past 9999 (zellij's `left_more_message`).
fn indicator_count_text(count: usize) -> String {
    if count > 9999 {
        "many".to_string()
    } else {
        count.to_string()
    }
}

/// Columns of `indicator_count_text`, without allocating (runs in the
/// reserve-convergence loop).
fn indicator_count_cols(count: usize) -> u16 {
    if count > 9999 {
        return 4;
    }
    let mut n = count;
    let mut digits = 1u16;
    while n >= 10 {
        n /= 10;
        digits += 1;
    }
    digits
}

/// Columns an overflow tile needs: 2 wedges plus ` ← +N ` (5 + count), plus the
/// badge segments under mouse chrome; never below the base width.
fn tab_indicator_width(count: usize, side: OverflowSide, mouse_chrome: bool) -> u16 {
    let badge_w = if mouse_chrome {
        overflow::badge_attention_width(side)
    } else {
        0
    };
    5u16.saturating_add(indicator_count_cols(count))
        .saturating_add(badge_w)
        .saturating_add(TAB_SEPARATOR_OVERHEAD)
        .max(OVERFLOW_INDICATOR_WIDTH)
}

/// ` name ` interior plus the 2 wedge columns. No minimum width (zellij).
fn tab_width(chrome: &TabChrome) -> u16 {
    chrome
        .display_width()
        .saturating_add(2)
        .saturating_add(TAB_SEPARATOR_OVERHEAD)
}

/// Columns the window `[lo, hi]` takes: its tabs plus a reserve on each side
/// that still hides tabs.
fn window_footprint(
    chromes: &[TabChrome],
    lo: usize,
    hi: usize,
    reserve_left: u16,
    reserve_right: u16,
) -> u16 {
    let mut total: u16 = chromes[lo..=hi]
        .iter()
        .fold(0, |acc, chrome| acc.saturating_add(tab_width(chrome)));
    if lo > 0 {
        total = total.saturating_add(reserve_left);
    }
    if hi + 1 < chromes.len() {
        total = total.saturating_add(reserve_right);
    }
    total
}

/// Place `[lo, hi]` left to right after the left reserve. A single tab wider
/// than the space left is clipped, so the row is never blank. Dense result:
/// one rect per tab, hidden tabs zero-width.
fn lay_window(
    chromes: &[TabChrome],
    lo: usize,
    hi: usize,
    area: Rect,
    reserve_left: u16,
) -> Vec<Rect> {
    let n = chromes.len();
    let mut rects = vec![Rect::default(); n];
    if n == 0 || area.width == 0 || area.height == 0 {
        return rects;
    }
    // A converged left reserve can exceed a tiny bar; keep a column for the
    // window so the active tab is never pushed off the row.
    let gutter = if lo > 0 { reserve_left } else { 0 }.min(area.width.saturating_sub(1));
    let mut x = area.x.saturating_add(gutter);
    let right_limit = area.right();
    for idx in lo..=hi.min(n - 1) {
        let remaining = right_limit.saturating_sub(x);
        if remaining == 0 {
            break;
        }
        let width = tab_width(&chromes[idx]).min(remaining);
        rects[idx] = Rect::new(x, area.y, width, 1);
        x = x.saturating_add(width);
    }
    rects
}

/// zellij's centered fill: start from the active tab and add whole tabs on the
/// side with less accumulated width (ties and a full right side prefer left)
/// until neither side fits.
fn centered_active_fill(
    chromes: &[TabChrome],
    active_tab: usize,
    area: Rect,
    reserve_left: u16,
    reserve_right: u16,
) -> Vec<Rect> {
    let n = chromes.len();
    if n == 0 || area.width == 0 || area.height == 0 {
        return vec![Rect::default(); n];
    }
    let active = active_tab.min(n - 1);
    let footprint =
        |lo: usize, hi: usize| window_footprint(chromes, lo, hi, reserve_left, reserve_right);
    let (mut lo, mut hi) = (active, active);
    let (mut total_left, mut total_right) = (0u16, 0u16);
    loop {
        let left_fits = lo > 0 && footprint(lo - 1, hi) <= area.width;
        let right_fits = hi + 1 < n && footprint(lo, hi + 1) <= area.width;
        if (total_left <= total_right || !right_fits) && left_fits {
            lo -= 1;
            total_left = total_left.saturating_add(tab_width(&chromes[lo]));
        } else if right_fits {
            hi += 1;
            total_right = total_right.saturating_add(tab_width(&chromes[hi]));
        } else {
            break;
        }
    }
    lay_window(chromes, lo, hi, area, reserve_left)
}

fn visible_bounds(rects: &[Rect]) -> Option<(usize, usize)> {
    let first = rects.iter().position(|r| r.width > 0)?;
    let last = rects.iter().rposition(|r| r.width > 0)?;
    Some((first, last))
}

/// Hidden tabs on one side of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HiddenGroup {
    count: usize,
    side: OverflowSide,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TabBarOverflow {
    left: Option<HiddenGroup>,
    right: Option<HiddenGroup>,
    left_hit_area: Rect,
    right_hit_area: Rect,
}

/// Dense tab rects plus overflow tiles. Reserves start at the base tile width
/// on both sides, then converge (at most 4 passes) to the width each side's
/// tile actually needs, so the reserved width always equals the painted and
/// clickable width.
fn build_overflow_layout(
    chromes: &[TabChrome],
    tabs_area: Rect,
    mouse_chrome: bool,
    active_tab: usize,
) -> (Vec<Rect>, TabBarOverflow) {
    let tab_count = chromes.len();
    let fill = |l: u16, r: u16| centered_active_fill(chromes, active_tab, tabs_area, l, r);
    let status_of = |i: usize| chromes.get(i).and_then(|chrome| chrome.agent_status);
    let groups_for = |rects: &[Rect]| -> (Option<HiddenGroup>, Option<HiddenGroup>) {
        let (first, last) = visible_bounds(rects).unwrap_or((active_tab, active_tab));
        let window = ListWindow {
            first,
            count: last.saturating_sub(first) + 1,
            hidden_above: first,
            hidden_below: tab_count.saturating_sub(last + 1),
        };
        let left = (window.hidden_above > 0).then(|| HiddenGroup {
            count: window.hidden_above,
            side: overflow::side_above(window, status_of),
        });
        let right = (window.hidden_below > 0).then(|| HiddenGroup {
            count: window.hidden_below,
            side: overflow::side_below(window, tab_count, status_of),
        });
        (left, right)
    };

    let mut rects = fill(OVERFLOW_INDICATOR_WIDTH, OVERFLOW_INDICATOR_WIDTH);
    let (mut left, mut right) = groups_for(&rects);
    let want = |group: &Option<HiddenGroup>| {
        group
            .map(|g| tab_indicator_width(g.count, g.side, mouse_chrome))
            .unwrap_or(0)
    };
    let (mut left_w, mut right_w) = (OVERFLOW_INDICATOR_WIDTH, OVERFLOW_INDICATOR_WIDTH);
    for _ in 0..4 {
        let (need_left, need_right) = (want(&left), want(&right));
        if need_left == left_w && need_right == right_w {
            break;
        }
        left_w = need_left;
        right_w = need_right;
        rects = fill(
            left_w.max(OVERFLOW_INDICATOR_WIDTH),
            right_w.max(OVERFLOW_INDICATOR_WIDTH),
        );
        (left, right) = groups_for(&rects);
    }

    // The left tile hugs the bar's left edge; the right tile abuts the last
    // visible tab and clips at the edge. At pathological widths the always-
    // visible active tab can eat a tile's reserve, so both clamp.
    let (first, last) = visible_bounds(&rects).unwrap_or((active_tab, active_tab));
    let first_x = rects.get(first).map(|r| r.x).unwrap_or(tabs_area.x);
    let last_right = rects.get(last).map(|r| r.right()).unwrap_or(tabs_area.x);
    let left_hit_area = if left.is_some() {
        let width = left_w.min(first_x.saturating_sub(tabs_area.x));
        Rect::new(tabs_area.x, tabs_area.y, width, 1)
    } else {
        Rect::default()
    };
    let right_hit_area = if right.is_some() {
        let x = last_right.min(tabs_area.right());
        Rect::new(x, tabs_area.y, right_w.min(tabs_area.right() - x), 1)
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
}

/// The computed bar: dense tab rects, overflow tiles, and the `+` button.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TabBarView {
    tab_rects: Vec<Rect>,
    overflow: TabBarOverflow,
    new_tab: Rect,
}

fn compute_tab_bar_view(
    chromes: &[TabChrome],
    active_tab: usize,
    area: Rect,
    mouse_chrome: bool,
) -> TabBarView {
    if area.width == 0 || area.height == 0 {
        return TabBarView::default();
    }
    let new_tab_reserve = if mouse_chrome { NEW_TAB_WIDTH } else { 0 };
    let tabs_area = Rect {
        width: area.width.saturating_sub(new_tab_reserve),
        ..area
    };
    let probe = centered_active_fill(chromes, active_tab, tabs_area, 0, 0);
    let (tab_rects, overflow) = if probe.iter().any(|r| r.width == 0) {
        build_overflow_layout(chromes, tabs_area, mouse_chrome, active_tab)
    } else {
        (probe, TabBarOverflow::default())
    };
    let new_tab = if mouse_chrome {
        let trailing = tab_rects
            .iter()
            .rev()
            .find(|r| r.width > 0)
            .map(|r| r.right())
            .unwrap_or(tabs_area.x)
            .max(overflow.right_hit_area.right());
        let x = trailing.min(area.right().saturating_sub(NEW_TAB_WIDTH).max(area.x));
        Rect::new(
            x,
            area.y,
            area.right().saturating_sub(x).min(NEW_TAB_WIDTH),
            1,
        )
    } else {
        Rect::default()
    };
    TabBarView {
        tab_rects,
        overflow,
        new_tab,
    }
}

/// Inactive tabs alternate surface0/surface1 without wedges; with wedges they
/// share surface0 (surface_dim when surface0 equals the panel, so the wedges
/// stay visible). The active tab is the accent.
fn tab_bg(p: &Palette, idx: usize, active: bool, separator: SeparatorStyle) -> Color {
    if active {
        p.accent
    } else if separator == SeparatorStyle::AlternatingBg {
        if idx.is_multiple_of(2) {
            p.surface0
        } else {
            p.surface1
        }
    } else if p.surface0 == p.panel_bg {
        p.surface_dim
    } else {
        p.surface0
    }
}

/// Paint one tile: wedges at both edges with the interior between them, or the
/// interior across the whole rect when wedges are off or do not fit.
fn paint_tile(
    buffer: &mut Buffer,
    rect: Rect,
    interior: Vec<Span<'static>>,
    interior_style: Style,
    separator: SeparatorStyle,
    panel_bg: Color,
) {
    let tile_bg = interior_style.bg.unwrap_or(panel_bg);
    if rect.width >= TAB_SEPARATOR_OVERHEAD {
        let inner = Rect::new(rect.x + 1, rect.y, rect.width - 2, 1);
        if separator == SeparatorStyle::Powerline {
            if let Some(cell) = buffer.cell_mut((rect.x, rect.y)) {
                cell.set_symbol(POWERLINE_ARROW)
                    .set_style(Style::default().fg(panel_bg).bg(tile_bg));
            }
            if let Some(cell) = buffer.cell_mut((rect.right() - 1, rect.y)) {
                cell.set_symbol(POWERLINE_ARROW)
                    .set_style(Style::default().fg(tile_bg).bg(panel_bg));
            }
        } else {
            buffer.set_stringn(
                rect.x,
                rect.y,
                " ".repeat(usize::from(rect.width)),
                usize::from(rect.width),
                interior_style,
            );
        }
        if inner.width > 0 {
            buffer.set_style(inner, interior_style);
            buffer.set_line(
                inner.x,
                inner.y,
                &Line::from(interior).style(interior_style),
                inner.width,
            );
        }
    } else {
        buffer.set_style(rect, interior_style);
        buffer.set_line(
            rect.x,
            rect.y,
            &Line::from(interior).style(interior_style),
            rect.width,
        );
    }
}

/// Paint the zellij-style bar into `area` (upstream's content area, already
/// filled with the panel background) and record its hits: one `hits.tabs`
/// entry per visible tab in order, the overflow tiles in the scroll-arrow
/// slots with their jump targets, and the `+` button.
pub(super) fn render_tabs(
    buffer: &mut Buffer,
    area: Rect,
    tabs: &[&ClientShellTab],
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    let p = &config.palette;
    let separator = if config.powerline {
        SeparatorStyle::Powerline
    } else {
        SeparatorStyle::AlternatingBg
    };
    let chromes = build_chromes(tabs, config);
    let active = tabs.iter().position(|tab| tab.focused).unwrap_or(0);
    let mouse_chrome = config.mouse_capture;
    let view = compute_tab_bar_view(&chromes, active, area, mouse_chrome);

    let tile_bg = tab_bg(p, 1, false, SeparatorStyle::Powerline);
    let tile_text = Style::default()
        .fg(p.overlay1)
        .bg(tile_bg)
        .add_modifier(Modifier::BOLD);
    let on_tile = |spans: Vec<Span<'static>>| -> Vec<Span<'static>> {
        spans
            .into_iter()
            .map(|span| Span::styled(span.content, span.style.bg(tile_bg)))
            .collect()
    };
    for (group, rect, left) in [
        (view.overflow.left, view.overflow.left_hit_area, true),
        (view.overflow.right, view.overflow.right_hit_area, false),
    ] {
        let Some(group) = group else { continue };
        if rect.width == 0 {
            continue;
        }
        let count = indicator_count_text(group.count);
        let mut interior = vec![Span::styled(
            if left {
                format!(" ← +{count}")
            } else {
                format!(" +{count}")
            },
            tile_text,
        )];
        if mouse_chrome {
            interior.extend(on_tile(overflow::bucket_segment_spans(group.side, p)));
        }
        interior.push(Span::styled(if left { " " } else { " → " }, tile_text));
        paint_tile(buffer, rect, interior, tile_text, separator, p.panel_bg);
    }

    for (idx, (tab, chrome)) in tabs.iter().zip(&chromes).enumerate() {
        let Some(rect) = view.tab_rects.get(idx).copied().filter(|r| r.width > 0) else {
            continue;
        };
        let active = idx == active;
        let fg = if active {
            panel_contrast_fg(p)
        } else if chrome.custom_label {
            p.text
        } else {
            p.overlay1
        };
        let style = Style::default()
            .fg(fg)
            .bg(tab_bg(p, idx, active, separator))
            .add_modifier(Modifier::BOLD);
        let interior_width = if rect.width >= TAB_SEPARATOR_OVERHEAD {
            rect.width - TAB_SEPARATOR_OVERHEAD
        } else {
            rect.width
        };
        paint_tile(
            buffer,
            rect,
            chrome.to_spans(interior_width),
            style,
            separator,
            p.panel_bg,
        );
        hits.tabs.push((rect, tab.tab_id.clone()));
    }

    let jump = |group: Option<HiddenGroup>| {
        group
            .and_then(|g| overflow::resolve_jump(g.side))
            .and_then(|index| tabs.get(index))
            .map(|tab| tab.tab_id.clone())
    };
    hits.tab_scroll_left = view.overflow.left_hit_area;
    hits.tab_scroll_right = view.overflow.right_hit_area;
    hits.tab_overflow_targets = [jump(view.overflow.left), jump(view.overflow.right)];

    if mouse_chrome && view.new_tab.width > 0 {
        hits.new_tab = view.new_tab;
        super::render::put_text(
            buffer,
            view.new_tab.x,
            view.new_tab.y,
            view.new_tab.width,
            " + ",
            Style::default().fg(p.overlay1).bg(p.panel_bg),
        );
    }
}

/// Two clicks on the same tab within this window open Rename.
const TAB_DOUBLE_CLICK_WINDOW: std::time::Duration = std::time::Duration::from_millis(350);

impl ClientShellState {
    fn zellij_tabs(&self) -> bool {
        self.config.tab_style == crate::config::TabStyleConfig::Zellij
    }

    /// A click on an overflow tile focuses the tab it resolved to.
    pub(super) fn tab_overflow_jump(
        &mut self,
        point: (u16, u16),
        outcome: &mut ClientShellInput,
    ) -> bool {
        if !self.zellij_tabs() {
            return false;
        }
        let tiles = [self.hits.tab_scroll_left, self.hits.tab_scroll_right];
        let Some(tab_id) = tiles
            .iter()
            .zip(&self.hits.tab_overflow_targets)
            .find(|(rect, _)| super::contains(**rect, point))
            .and_then(|(_, target)| target.clone())
        else {
            return false;
        };
        self.push_endpoint_method(
            crate::api::schema::Method::TabFocus(crate::api::schema::TabTarget { tab_id }),
            outcome,
        );
        true
    }

    /// Middle-click on a tab closes it, through the same confirmation as the
    /// tab menu's Close.
    pub(super) fn tab_middle_click(
        &mut self,
        point: (u16, u16),
        outcome: &mut ClientShellInput,
    ) -> bool {
        if !self.zellij_tabs() {
            return false;
        }
        let Some(tab_id) = self
            .hits
            .tabs
            .iter()
            .find(|(rect, _)| super::contains(*rect, point))
            .map(|(_, tab_id)| tab_id.clone())
        else {
            return false;
        };
        self.request_tab_close(tab_id, outcome);
        true
    }

    /// Record a tab click; the second click on the same tab within the window
    /// opens Rename for it.
    pub(super) fn note_tab_click(&mut self, tab_id: &str) {
        if !self.zellij_tabs() {
            return;
        }
        let now = std::time::Instant::now();
        let double = self.last_tab_click.take().is_some_and(|(last, at)| {
            last == tab_id && now.duration_since(at) <= TAB_DOUBLE_CLICK_WINDOW
        });
        if !double {
            self.last_tab_click = Some((tab_id.to_owned(), now));
            return;
        }
        let Some(tab) = self
            .snapshot
            .as_deref()
            .and_then(|snapshot| snapshot.tabs.iter().find(|tab| tab.tab_id == tab_id))
        else {
            return;
        };
        self.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
            title: "rename tab",
            input: TextEditor::new(&tab.label, false),
            target: ClientRenameTarget::Tab {
                tab_id: tab_id.to_owned(),
                auto_name: !tab.custom_label,
                original_name: tab.label.clone(),
            },
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chrome(name: &str) -> TabChrome {
        TabChrome {
            status: None,
            name: name.to_owned(),
            zoomed: false,
            custom_label: true,
            agent_status: Some(AgentStatus::Idle),
        }
    }

    fn named(count: usize) -> Vec<TabChrome> {
        (0..count).map(|i| chrome(&format!("tab{i}"))).collect()
    }

    fn bar(width: u16) -> Rect {
        Rect::new(0, 0, width, 1)
    }

    fn visible(rects: &[Rect]) -> Vec<usize> {
        rects
            .iter()
            .enumerate()
            .filter(|(_, r)| r.width > 0)
            .map(|(i, _)| i)
            .collect()
    }

    fn text(spans: &[Span<'_>]) -> String {
        spans.iter().map(|span| span.content.as_ref()).collect()
    }

    #[test]
    fn all_tabs_visible_when_they_fit() {
        let view = compute_tab_bar_view(&named(3), 0, bar(60), true);
        assert_eq!(visible(&view.tab_rects), vec![0, 1, 2]);
        assert_eq!(view.overflow, TabBarOverflow::default());
        // Tabs abut and the `+` button follows the last one.
        assert_eq!(view.tab_rects[1].x, view.tab_rects[0].right());
        assert_eq!(view.new_tab.x, view.tab_rects[2].right());
    }

    #[test]
    fn active_tab_is_visible_at_full_width_with_tiles_on_both_sides() {
        let chromes = named(10);
        let view = compute_tab_bar_view(&chromes, 5, bar(40), true);
        assert_eq!(view.tab_rects[5].width, tab_width(&chromes[5]));
        let left = view.overflow.left.expect("left tile");
        let right = view.overflow.right.expect("right tile");
        let shown = visible(&view.tab_rects);
        assert_eq!(left.count + shown.len() + right.count, 10);
        assert_eq!(view.overflow.left_hit_area.x, 0);
        assert_eq!(
            view.overflow.right_hit_area.x,
            view.tab_rects[*shown.last().unwrap()].right()
        );
    }

    #[test]
    fn only_one_side_overflows_at_the_ends() {
        let chromes = named(10);
        let first = compute_tab_bar_view(&chromes, 0, bar(40), true);
        assert!(first.overflow.left.is_none() && first.overflow.right.is_some());
        let last = compute_tab_bar_view(&chromes, 9, bar(40), true);
        assert!(last.overflow.left.is_some() && last.overflow.right.is_none());
    }

    #[test]
    fn tiles_jump_to_the_most_urgent_hidden_tab_and_count_only_hidden_ones() {
        let mut chromes = named(12);
        chromes[1].agent_status = Some(AgentStatus::Blocked);
        chromes[2].agent_status = Some(AgentStatus::Done);
        chromes[6].agent_status = Some(AgentStatus::Blocked); // visible: active
        chromes[10].agent_status = Some(AgentStatus::Working);
        let view = compute_tab_bar_view(&chromes, 6, bar(50), true);
        let left = view.overflow.left.unwrap().side;
        let right = view.overflow.right.unwrap().side;
        assert_eq!((left.hidden_blocked, left.hidden_done), (1, 1));
        assert_eq!(overflow::resolve_jump(left), Some(1));
        assert_eq!(
            right.hidden_blocked, 0,
            "the visible blocked tab is not counted"
        );
        assert_eq!(overflow::resolve_jump(right), Some(10));
    }

    #[test]
    fn badges_widen_the_tile_and_the_reserve_matches() {
        let mut chromes = named(12);
        for chrome in &mut chromes[..4] {
            chrome.agent_status = Some(AgentStatus::Blocked);
        }
        let view = compute_tab_bar_view(&chromes, 8, bar(60), true);
        let left = view.overflow.left.unwrap();
        let want = tab_indicator_width(left.count, left.side, true);
        assert!(want > OVERFLOW_INDICATOR_WIDTH);
        assert_eq!(view.overflow.left_hit_area.width, want);
        let first_visible = visible(&view.tab_rects)[0];
        assert_eq!(
            view.tab_rects[first_visible].x,
            view.overflow.left_hit_area.right()
        );
    }

    #[test]
    fn layout_invariants_hold_across_widths_and_active_tabs() {
        let mut chromes: Vec<TabChrome> = (0..15)
            .map(|i| chrome(&"x".repeat(1 + (i * 7) % 11)))
            .collect();
        chromes[3].agent_status = Some(AgentStatus::Blocked);
        chromes[11].agent_status = Some(AgentStatus::Working);
        for width in 12u16..=120 {
            for active in [0, 3, 7, 14] {
                let area = bar(width);
                let view = compute_tab_bar_view(&chromes, active, area, true);
                assert_eq!(view.tab_rects.len(), chromes.len());
                assert!(view.tab_rects[active].width > 0, "active hidden at {width}");
                let shown = visible(&view.tab_rects);
                assert!(
                    shown.windows(2).all(|w| w[1] == w[0] + 1),
                    "contiguous window"
                );
                for rect in view.tab_rects.iter().filter(|r| r.width > 0) {
                    assert!(rect.right() <= area.right());
                    for tile in [view.overflow.left_hit_area, view.overflow.right_hit_area] {
                        assert!(
                            tile.width == 0 || tile.right() <= rect.x || tile.x >= rect.right(),
                            "tile overlaps a tab at width {width}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn widths_follow_display_columns_dots_and_zoom() {
        let mut tab = chrome("abc");
        assert_eq!(tab_width(&tab), 7);
        tab.zoomed = true;
        assert_eq!(tab_width(&tab), 9);
        tab.status = Some(TabStatusDot {
            glyph: "●",
            style: Style::default(),
        });
        assert_eq!(tab_width(&tab), 11);
        assert_eq!(tab_width(&chrome("你好")), 8);
        assert_eq!(
            tab_width(&chrome("a\u{202e}b")),
            6,
            "bidi stripped before measuring"
        );
    }

    #[test]
    fn to_spans_orders_dot_name_zoom_and_never_truncates() {
        let mut tab = chrome("name");
        tab.zoomed = true;
        tab.status = Some(TabStatusDot {
            glyph: "●",
            style: Style::default(),
        });
        assert_eq!(text(&tab.to_spans(12)), " ● name Z   ");
        assert_eq!(text(&chrome("longername").to_spans(5)), " longername");
        assert_eq!(text(&chrome("a\u{202e}b\x01c").to_spans(5)), " abc ");
    }

    #[test]
    fn status_dots_follow_the_mode() {
        let mut config = ClientShellConfig::from_config(&crate::config::Config::default());
        let shown = |config: &ClientShellConfig, status| {
            tab_status_dot(status, config.tab_status, config).is_some()
        };
        assert!(!shown(&config, AgentStatus::Blocked));
        config.tab_status = TabStatusModeConfig::Attention;
        assert!(shown(&config, AgentStatus::Blocked) && shown(&config, AgentStatus::Done));
        assert!(!shown(&config, AgentStatus::Working) && !shown(&config, AgentStatus::Idle));
        config.tab_status = TabStatusModeConfig::All;
        assert!(shown(&config, AgentStatus::Working) && shown(&config, AgentStatus::Idle));
        assert!(!shown(&config, AgentStatus::Unknown));
        let dot = tab_status_dot(AgentStatus::Done, config.tab_status, &config).unwrap();
        assert_eq!(dot.style.fg, Some(config.palette.teal));
    }

    #[test]
    fn indicator_text_counts_in_full() {
        assert_eq!(indicator_count_text(7), "7");
        assert_eq!(indicator_count_text(123), "123");
        assert_eq!(indicator_count_text(10_000), "many");
        for count in [0, 9, 10, 99, 100, 9999, 10_000] {
            assert_eq!(
                usize::from(indicator_count_cols(count)),
                indicator_count_text(count).width()
            );
        }
    }

    fn paint(
        chromes: Vec<TabChrome>,
        active: usize,
        width: u16,
        powerline: bool,
    ) -> (Buffer, ShellHitMap) {
        let mut config = ClientShellConfig::from_config(&crate::config::Config::default());
        config.powerline = powerline;
        config.tab_status = TabStatusModeConfig::Attention;
        config.palette = Palette::catppuccin();
        let tabs: Vec<ClientShellTab> = chromes
            .iter()
            .enumerate()
            .map(|(i, chrome)| ClientShellTab {
                tab_id: format!("t{i}"),
                workspace_id: "w".into(),
                number: i + 1,
                label: chrome.name.clone(),
                custom_label: chrome.custom_label,
                zoomed: chrome.zoomed,
                focused: i == active,
                agent_status: chrome.agent_status.unwrap_or(AgentStatus::Unknown),
            })
            .collect();
        let refs: Vec<&ClientShellTab> = tabs.iter().collect();
        let area = bar(width);
        let mut buffer = Buffer::empty(area);
        let mut hits = ShellHitMap::default();
        render_tabs(&mut buffer, area, &refs, &config, &mut hits);
        (buffer, hits)
    }

    fn row(buffer: &Buffer) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, 0)].symbol().to_owned())
            .collect()
    }

    #[test]
    fn painted_tabs_use_back_to_back_wedges_and_bold_labels() {
        let p = Palette::catppuccin();
        let (buffer, hits) = paint(named(3), 1, 60, true);
        assert_eq!(hits.tabs.len(), 3);
        let second = hits.tabs[1].0;
        let left = &buffer[(second.x, 0)];
        let right = &buffer[(second.right() - 1, 0)];
        assert_eq!(left.symbol(), POWERLINE_ARROW);
        assert_eq!((left.fg, left.bg), (p.panel_bg, p.accent));
        assert_eq!((right.fg, right.bg), (p.accent, p.panel_bg));
        let label = &buffer[(second.x + 2, 0)];
        assert_eq!(label.bg, p.accent);
        assert!(label.modifier.contains(Modifier::BOLD));
        let inactive = &buffer[(hits.tabs[0].0.x + 2, 0)];
        assert_eq!(inactive.bg, p.surface0);
        assert!(inactive.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn without_powerline_inactive_tabs_alternate_and_no_wedges_are_drawn() {
        let p = Palette::catppuccin();
        let (buffer, hits) = paint(named(3), 0, 60, false);
        assert!(!row(&buffer).contains(POWERLINE_ARROW));
        assert_eq!(buffer[(hits.tabs[1].0.x, 0)].bg, p.surface1);
        assert_eq!(buffer[(hits.tabs[2].0.x, 0)].bg, p.surface0);
    }

    #[test]
    fn overflow_tiles_paint_their_counts_and_badges() {
        let mut chromes = named(12);
        chromes[1].agent_status = Some(AgentStatus::Blocked);
        let (buffer, hits) = paint(chromes, 8, 60, true);
        let row = row(&buffer);
        assert!(row.contains("← +"), "{row}");
        assert!(row.contains("◉¹"), "{row}");
        assert!(row.contains(" → "), "{row}");
        assert!(row.contains(" + "), "{row}");
        assert_eq!(hits.tab_overflow_targets[0].as_deref(), Some("t1"));
        assert!(hits.tab_scroll_left.width > 0 && hits.tab_scroll_right.width > 0);
    }

    #[test]
    fn wedges_stay_visible_on_the_terminal_palette() {
        let p = Palette::terminal();
        assert_eq!(
            tab_bg(&p, 0, false, SeparatorStyle::Powerline),
            p.surface_dim
        );
        assert_ne!(tab_bg(&p, 0, false, SeparatorStyle::Powerline), p.panel_bg);
    }
}
