use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::widgets::panel_contrast_fg;
use crate::app::AppState;
use crate::config::TabStatusMode;

const MIN_TAB_WIDTH: u16 = 8;
const NEW_TAB_WIDTH: u16 = 3;
const TAB_SCROLL_BUTTON_WIDTH: u16 = 3;

// ---------------------------------------------------------------------------
// TabChrome — structured per-tab label model
// ---------------------------------------------------------------------------

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
        let name_w = u16::try_from(self.name.chars().count()).unwrap_or(u16::MAX);
        let mod_w: u16 = if self.zoomed { 2 } else { 0 };
        status_w.saturating_add(name_w).saturating_add(mod_w)
    }

    /// When `truncate` is set (compression mode), the name is shortened with a
    /// trailing `…` so the whole label fits `rect_width`. The budget is derived
    /// here from `rect_width` and the same status/zoom column reservations
    /// `display_width` uses, so the truncation predicate lives in one place.
    pub fn to_spans(&self, mode: TabStatusMode, rect_width: u16, truncate: bool) -> Vec<Span<'_>> {
        let mut spans: Vec<Span> = Vec::with_capacity(6);

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
        let name_cols = u16::try_from(self.name.chars().count()).unwrap_or(u16::MAX);
        if truncate && name_budget > 0 && name_cols > name_budget {
            let truncated: String = self.name.chars().take((name_budget - 1) as usize).collect();
            spans.push(Span::raw(format!("{truncated}…")));
        } else {
            spans.push(Span::raw(self.name.as_str()));
        }

        // Zoom modifier
        if self.zoomed {
            spans.push(Span::raw(" Z"));
        }

        // Trailing pad to fill rect_width
        let content_width = spans.iter().fold(0u16, |acc, s| {
            acc.saturating_add(u16::try_from(s.content.chars().count()).unwrap_or(u16::MAX))
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
    pub scroll: usize,
    pub tab_hit_areas: Vec<Rect>,
    pub tab_chrome: Vec<TabChrome>,
    pub tab_status_mode: TabStatusMode,
    pub compressed_width: Option<u16>,
    pub scroll_left_hit_area: Rect,
    pub scroll_right_hit_area: Rect,
    pub new_tab_hit_area: Rect,
}

fn tab_width(chrome: &TabChrome, mode: TabStatusMode) -> u16 {
    chrome
        .display_width(mode)
        .saturating_add(4)
        .max(MIN_TAB_WIDTH)
}

fn layout_tab_hit_areas(
    chromes: &[TabChrome],
    mode: TabStatusMode,
    area: Rect,
    scroll: usize,
) -> Vec<Rect> {
    let mut rects = vec![Rect::default(); chromes.len()];
    if area.width == 0 || area.height == 0 {
        return rects;
    }

    let mut x = area.x;
    let right = area.x.saturating_add(area.width);
    let mut placed_any = false;
    for (idx, rect) in rects.iter_mut().enumerate().skip(scroll) {
        if x >= right {
            break;
        }
        let desired = tab_width(&chromes[idx], mode);
        let remaining = right.saturating_sub(x);
        // No slivers: a tab that can't reach MIN_TAB_WIDTH is hidden (width 0)
        // and reachable via the scroll buttons (which show hidden-tab counts) —
        // unless nothing has been placed yet, in which case the first tab at this
        // scroll offset is rendered clipped (>= 1 col) so the row is never blank.
        // This break (not continue) keeps hidden tabs as a single trailing
        // zero-width run, which the right-edge hidden-count indicator relies on.
        if remaining < MIN_TAB_WIDTH && placed_any {
            break;
        }
        let width = desired.min(remaining).max(1);
        *rect = Rect::new(x, area.y, width, 1);
        placed_any = true;
        x = x.saturating_add(width.saturating_add(1));
    }
    rects
}

/// Compute a single uniform tab width that lets all tabs fit `available_width`,
/// or `None` if they already fit naturally or cannot fit even compressed.
///
/// Uniform (every tab the same width) rather than proportional-to-natural: it
/// keeps click targets stable as names change in this mouse-first TUI and keeps
/// the apportionment arithmetic exact, avoiding the rounding drift a weighted
/// split would introduce. See the design's Acknowledged Tradeoffs.
fn compress_tab_widths(
    chromes: &[TabChrome],
    mode: TabStatusMode,
    available_width: u16,
) -> Option<u16> {
    let n = u16::try_from(chromes.len()).unwrap_or(u16::MAX);
    if n <= 1 {
        return None;
    }
    let total_natural: u16 = chromes
        .iter()
        .map(|c| tab_width(c, mode))
        .fold(0u16, |acc, w| acc.saturating_add(w));
    let gaps = n.saturating_sub(1);
    let total_with_gaps = total_natural.saturating_add(gaps);
    if total_with_gaps <= available_width {
        return None;
    }
    let space_for_tabs = available_width.saturating_sub(gaps);
    let compressed_width = space_for_tabs / n;
    if compressed_width < MIN_TAB_WIDTH {
        return None;
    }
    // Guard against integer-division rounding: re-check the exact fit with
    // saturating arithmetic so absurd tab counts can't wrap the product.
    if compressed_width.saturating_mul(n).saturating_add(gaps) > available_width {
        return None;
    }
    Some(compressed_width)
}

/// Lay out `count` tabs at a uniform `uniform_width` with one-column gaps.
///
/// Precondition: the caller (only `compute_tab_bar_view` via `compress_tab_widths`)
/// must have proven `count * uniform_width + (count - 1) <= area.width`, so every
/// tab fits at full width with no clipping or hidden tab. The debug assertion
/// below pins that contract: under compression no rect may collapse below
/// `uniform_width`, which keeps both the no-sliver and active-tab-visible
/// invariants trivially true.
fn layout_tab_hit_areas_compressed(count: usize, uniform_width: u16, area: Rect) -> Vec<Rect> {
    let mut rects = vec![Rect::default(); count];
    if area.width == 0 || area.height == 0 {
        return rects;
    }
    let mut x = area.x;
    let right = area.x.saturating_add(area.width);
    for rect in rects.iter_mut() {
        if x >= right {
            break;
        }
        let remaining = right.saturating_sub(x);
        let width = uniform_width.min(remaining);
        *rect = Rect::new(x, area.y, width, 1);
        x = x.saturating_add(width.saturating_add(1));
    }
    debug_assert!(
        rects.iter().all(|r| r.width == uniform_width),
        "compressed layout clipped a tab: precondition count*width+gaps <= area.width violated"
    );
    rects
}

fn centered_tab_scroll(
    chromes: &[TabChrome],
    active_tab: usize,
    mode: TabStatusMode,
    area: Rect,
) -> usize {
    let mut best_scroll = active_tab;
    let mut best_distance = u16::MAX;
    let viewport_center = area.x.saturating_mul(2).saturating_add(area.width);

    for scroll in 0..=active_tab {
        let rects = layout_tab_hit_areas(chromes, mode, area, scroll);
        let Some(active_rect) = rects.get(active_tab).copied() else {
            continue;
        };
        if active_rect.width == 0 {
            continue;
        }

        let active_center = active_rect
            .x
            .saturating_mul(2)
            .saturating_add(active_rect.width);
        let distance = active_center.abs_diff(viewport_center);
        if distance <= best_distance {
            best_distance = distance;
            best_scroll = scroll;
        }
    }

    best_scroll
}

fn trailing_tab_controls_x(tab_hit_areas: &[Rect], fallback_x: u16) -> u16 {
    tab_hit_areas
        .iter()
        .rev()
        .find(|rect| rect.width > 0)
        .map(|rect| rect.x + rect.width)
        .unwrap_or(fallback_x)
}

fn max_tab_scroll(chromes: &[TabChrome], mode: TabStatusMode, area: Rect) -> usize {
    (0..chromes.len())
        .find(|&scroll| {
            layout_tab_hit_areas(chromes, mode, area, scroll)
                .last()
                .is_some_and(|rect| rect.width > 0)
        })
        .unwrap_or(0)
}

pub(crate) fn compute_tab_bar_view(
    chromes: Vec<TabChrome>,
    active_tab: usize,
    mode: TabStatusMode,
    area: Rect,
    current_scroll: usize,
    follow_active: bool,
    mouse_chrome: bool,
) -> TabBarView {
    if area.width == 0 || area.height == 0 {
        return TabBarView::default();
    }

    if !mouse_chrome {
        if let Some(cw) = compress_tab_widths(&chromes, mode, area.width) {
            tracing::debug!(
                area_width = area.width,
                tab_count = chromes.len(),
                ?mode,
                compressed_width = cw,
                "tab bar compressed"
            );
            let tab_hit_areas = layout_tab_hit_areas_compressed(chromes.len(), cw, area);
            return TabBarView {
                scroll: 0,
                tab_hit_areas,
                tab_chrome: chromes,
                tab_status_mode: mode,
                compressed_width: Some(cw),
                scroll_left_hit_area: Rect::default(),
                scroll_right_hit_area: Rect::default(),
                new_tab_hit_area: Rect::default(),
            };
        }
        let max_scroll = max_tab_scroll(&chromes, mode, area);
        let scroll = if follow_active {
            centered_tab_scroll(&chromes, active_tab, mode, area).min(max_scroll)
        } else {
            current_scroll.min(max_scroll)
        };
        let tab_hit_areas = layout_tab_hit_areas(&chromes, mode, area, scroll);
        return TabBarView {
            scroll,
            tab_hit_areas,
            tab_chrome: chromes,
            tab_status_mode: mode,
            compressed_width: None,
            scroll_left_hit_area: Rect::default(),
            scroll_right_hit_area: Rect::default(),
            new_tab_hit_area: Rect::default(),
        };
    }

    let area_right = area.x + area.width;
    let all_tabs_area = Rect::new(
        area.x,
        area.y,
        area.width.saturating_sub(NEW_TAB_WIDTH),
        area.height,
    );
    let all_tabs = layout_tab_hit_areas(&chromes, mode, all_tabs_area, 0);
    let overflow = all_tabs.iter().any(|rect| rect.width == 0);
    if !overflow {
        let new_tab_x = trailing_tab_controls_x(&all_tabs, area.x);
        let new_tab_hit_area = Rect::new(
            new_tab_x,
            area.y,
            area_right.saturating_sub(new_tab_x).min(NEW_TAB_WIDTH),
            1,
        );
        return TabBarView {
            scroll: 0,
            tab_hit_areas: all_tabs,
            tab_chrome: chromes,
            tab_status_mode: mode,
            compressed_width: None,
            scroll_left_hit_area: Rect::default(),
            scroll_right_hit_area: Rect::default(),
            new_tab_hit_area,
        };
    }

    if let Some(cw) = compress_tab_widths(&chromes, mode, all_tabs_area.width) {
        tracing::debug!(
            tab_area_width = all_tabs_area.width,
            tab_count = chromes.len(),
            ?mode,
            compressed_width = cw,
            "tab bar compressed"
        );
        let tab_hit_areas = layout_tab_hit_areas_compressed(chromes.len(), cw, all_tabs_area);
        let new_tab_x = trailing_tab_controls_x(&tab_hit_areas, area.x);
        let new_tab_hit_area = Rect::new(
            new_tab_x,
            area.y,
            area_right.saturating_sub(new_tab_x).min(NEW_TAB_WIDTH),
            1,
        );
        return TabBarView {
            scroll: 0,
            tab_hit_areas,
            tab_chrome: chromes,
            tab_status_mode: mode,
            compressed_width: Some(cw),
            scroll_left_hit_area: Rect::default(),
            scroll_right_hit_area: Rect::default(),
            new_tab_hit_area,
        };
    }

    let left_hit_area = Rect::new(area.x, area.y, TAB_SCROLL_BUTTON_WIDTH.min(area.width), 1);
    let tab_area_x = left_hit_area.x + left_hit_area.width;
    let reserved_trailing_width = NEW_TAB_WIDTH.saturating_add(TAB_SCROLL_BUTTON_WIDTH);
    let tab_area_right = area_right.saturating_sub(reserved_trailing_width);
    let tab_area = Rect::new(
        tab_area_x,
        area.y,
        tab_area_right.saturating_sub(tab_area_x),
        area.height,
    );

    let max_scroll = max_tab_scroll(&chromes, mode, tab_area);
    let scroll = if follow_active {
        centered_tab_scroll(&chromes, active_tab, mode, tab_area).min(max_scroll)
    } else {
        current_scroll.min(max_scroll)
    };
    let tab_hit_areas = layout_tab_hit_areas(&chromes, mode, tab_area, scroll);
    let hidden_count = tab_hit_areas.iter().filter(|r| r.width == 0).count();
    tracing::debug!(
        tab_area_width = tab_area.width,
        tab_count = chromes.len(),
        ?mode,
        active_tab,
        max_scroll,
        scroll,
        hidden_count,
        "tab bar overflow"
    );
    let trailing_x = trailing_tab_controls_x(&tab_hit_areas, tab_area_x).min(tab_area_right);
    let right_hit_area = Rect::new(
        trailing_x,
        area.y,
        area_right
            .saturating_sub(trailing_x)
            .min(TAB_SCROLL_BUTTON_WIDTH),
        1,
    );
    let new_tab_x = right_hit_area.x + right_hit_area.width;
    let new_tab_hit_area = Rect::new(
        new_tab_x,
        area.y,
        area_right.saturating_sub(new_tab_x).min(NEW_TAB_WIDTH),
        1,
    );

    TabBarView {
        scroll,
        tab_hit_areas,
        tab_chrome: chromes,
        tab_status_mode: mode,
        compressed_width: None,
        scroll_left_hit_area: left_hit_area,
        scroll_right_hit_area: right_hit_area,
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

    if insert_idx == 0 {
        return Some(if first_visible.0 == 0 {
            first_visible.1.x
        } else {
            app.view.tab_scroll_left_hit_area.x + app.view.tab_scroll_left_hit_area.width
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
        return Some(if last_visible.0 + 1 >= ws.tabs.len() {
            last_visible.1.x + last_visible.1.width
        } else {
            app.view.tab_scroll_right_hit_area.x.saturating_sub(1)
        });
    }

    None
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

    frame.render_widget(
        Paragraph::new(" ".repeat(area.width as usize)).style(Style::default().bg(p.panel_bg)),
        area,
    );

    let first_visible_idx = app
        .view
        .tab_hit_areas
        .iter()
        .enumerate()
        .find(|(_, rect)| rect.width > 0)
        .map(|(idx, _)| idx);
    let last_visible_idx = app
        .view
        .tab_hit_areas
        .iter()
        .enumerate()
        .rev()
        .find(|(_, rect)| rect.width > 0)
        .map(|(idx, _)| idx);
    let can_scroll_left = app.view.tab_scroll_left_hit_area.width > 0 && app.tab_scroll > 0;
    let can_scroll_right = app.view.tab_scroll_right_hit_area.width > 0
        && last_visible_idx.is_some_and(|idx| idx + 1 < ws.tabs.len());

    if app.mouse_capture && app.view.tab_scroll_left_hit_area.width > 0 {
        // Chevron-first so a rect clipped below 3 cols still shows the affordance.
        // The hidden count is shown only when that direction can actually scroll;
        // a disabled button is a neutral dim chevron, not "‹0".
        let left_hidden = first_visible_idx.unwrap_or(0);
        let left_label = if !can_scroll_left {
            " ‹ ".to_string()
        } else if left_hidden > 9 {
            "‹9+".to_string()
        } else {
            format!("‹{left_hidden} ")
        };
        let style = if can_scroll_left {
            Style::default().fg(p.overlay1).bg(p.surface0)
        } else {
            Style::default()
                .fg(p.overlay0)
                .bg(p.surface0)
                .add_modifier(Modifier::DIM)
        };
        frame.render_widget(
            Paragraph::new(left_label).style(style),
            app.view.tab_scroll_left_hit_area,
        );
    }

    if app.mouse_capture && app.view.tab_scroll_right_hit_area.width > 0 {
        let right_hidden = app
            .view
            .tab_hit_areas
            .iter()
            .rev()
            .take_while(|rect| rect.width == 0)
            .count();
        let right_label = if !can_scroll_right {
            " › ".to_string()
        } else if right_hidden > 9 {
            "9+›".to_string()
        } else {
            format!(" {right_hidden}›")
        };
        let style = if can_scroll_right {
            Style::default().fg(p.overlay1).bg(p.surface0)
        } else {
            Style::default()
                .fg(p.overlay0)
                .bg(p.surface0)
                .add_modifier(Modifier::DIM)
        };
        frame.render_widget(
            Paragraph::new(right_label).style(style),
            app.view.tab_scroll_right_hit_area,
        );
    }

    for (idx, tab) in ws.tabs.iter().enumerate() {
        let Some(rect) = app.view.tab_hit_areas.get(idx).copied() else {
            break;
        };
        if rect.width == 0 {
            continue;
        }
        let active = idx == ws.active_tab;
        let style = if active {
            let base = Style::default().fg(panel_contrast_fg(p)).bg(p.accent);
            if tab.is_auto_named() {
                base.add_modifier(Modifier::DIM)
            } else {
                base.add_modifier(Modifier::BOLD)
            }
        } else if tab.is_auto_named() {
            Style::default()
                .fg(p.overlay0)
                .bg(p.surface0)
                .add_modifier(Modifier::DIM)
        } else {
            Style::default().fg(p.overlay1).bg(p.surface0)
        };

        let truncate = app.view.tab_compressed_width.is_some();
        let spans = if let Some(chrome) = app.view.tab_chrome.get(idx) {
            chrome.to_spans(app.view.tab_status_mode, rect.width, truncate)
        } else {
            vec![Span::raw(" ".repeat(rect.width as usize))]
        };
        frame.render_widget(Paragraph::new(Line::from(spans)).style(style), rect);
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

    #[test]
    fn tab_bar_marks_zoomed_tabs_without_renaming_them() {
        let mut app = AppState::test_new();
        let mut ws = Workspace::test_new("test");
        ws.tabs[0].zoomed = true;
        let custom_tab = ws.test_add_tab(Some("test"));
        ws.tabs[custom_tab].zoomed = true;

        app.workspaces = vec![ws];
        app.active = Some(0);
        app.view.tab_bar_rect = Rect::new(0, 0, 30, 1);
        let chromes = chromes_from_ws(&app.workspaces[0]);
        let view = compute_tab_bar_view(
            chromes,
            app.workspaces[0].active_tab,
            TabStatusMode::Off,
            app.view.tab_bar_rect,
            0,
            true,
            false,
        );
        app.view.tab_hit_areas = view.tab_hit_areas;
        app.view.tab_chrome = view.tab_chrome;
        app.view.tab_status_mode = view.tab_status_mode;

        let backend = TestBackend::new(30, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_tab_bar(&app, frame, app.view.tab_bar_rect))
            .unwrap();

        let row = buffer_row_text(terminal.backend().buffer(), app.view.tab_bar_rect, 0);
        assert!(row.contains(" 1 Z"), "tab row: {row:?}");
        assert!(row.contains(" test Z"), "tab row: {row:?}");
        assert_eq!(app.workspaces[0].tab_display_name(0).as_deref(), Some("1"));
        assert_eq!(
            app.workspaces[0].tab_display_name(custom_tab).as_deref(),
            Some("test")
        );
    }

    #[test]
    fn zoom_marker_counts_toward_tab_width() {
        let chrome = TabChrome {
            status: None,
            name: "abcdefgh".into(),
            zoomed: true,
        };
        assert_eq!(tab_width(&chrome, TabStatusMode::Off), 14);
    }

    // Characterization tests pinning the current tab bar layout behavior.
    // gap=1 col between tabs, MIN_TAB_WIDTH=8, padding=4 cols around the label,
    // zoom suffix " Z", no status-dot column (TabStatusMode::Off baseline).

    #[test]
    fn overflow_detected_when_tabs_exceed_area() {
        let ws = make_ws_with_tabs(&["ab", "cd", "ef"]);
        let chromes = chromes_from_ws(&ws);
        let area = Rect::new(0, 0, 30, 1);
        let view = compute_tab_bar_view(
            chromes.clone(),
            ws.active_tab,
            TabStatusMode::Off,
            area,
            0,
            true,
            true,
        );
        assert_eq!(view.scroll_left_hit_area.width, 0);
        assert_eq!(view.scroll_right_hit_area.width, 0);

        let narrow_area = Rect::new(0, 0, 21, 1);
        let view_narrow = compute_tab_bar_view(
            chromes,
            ws.active_tab,
            TabStatusMode::Off,
            narrow_area,
            0,
            true,
            true,
        );
        assert!(view_narrow.scroll_left_hit_area.width > 0);
        assert!(view_narrow.scroll_right_hit_area.width > 0);
    }

    #[test]
    fn no_overflow_non_mouse_mode_all_tabs_visible() {
        let ws = make_ws_with_tabs(&["ab", "cd", "ef"]);
        let chromes = chromes_from_ws(&ws);
        let area = Rect::new(0, 0, 26, 1);
        let view = compute_tab_bar_view(
            chromes,
            ws.active_tab,
            TabStatusMode::Off,
            area,
            0,
            true,
            false,
        );
        assert_eq!(view.tab_hit_areas.len(), 3);
        assert!(view.tab_hit_areas.iter().all(|r| r.width > 0));
    }

    #[test]
    fn overflow_in_non_mouse_mode_hides_last_tab() {
        // Previously the last tab got a 2-col sliver; now it is hidden (width 0)
        // because remaining (2) < MIN_TAB_WIDTH (8). The existing overflow
        // detection (any(width == 0)) fires correctly for this case.
        let ws = make_ws_with_tabs(&["ab", "cd", "ef"]);
        let chromes = chromes_from_ws(&ws);
        let area = Rect::new(0, 0, 20, 1);
        let view = compute_tab_bar_view(
            chromes,
            ws.active_tab,
            TabStatusMode::Off,
            area,
            0,
            true,
            false,
        );
        assert_eq!(view.tab_hit_areas[0].width, 8);
        assert_eq!(view.tab_hit_areas[1].width, 8);
        assert_eq!(view.tab_hit_areas[2].width, 0);
    }

    #[test]
    fn centered_scroll_centers_active_tab_in_viewport() {
        let mut ws = make_ws_with_tabs(&["aa", "bb", "cc", "dd", "ee"]);
        ws.active_tab = 2;
        let chromes = chromes_from_ws(&ws);

        let area = Rect::new(0, 0, 25, 1);
        assert_eq!(
            centered_tab_scroll(&chromes, ws.active_tab, TabStatusMode::Off, area),
            1
        );

        let view = compute_tab_bar_view(
            chromes,
            ws.active_tab,
            TabStatusMode::Off,
            area,
            0,
            true,
            false,
        );
        assert_eq!(view.scroll, 1);
    }

    #[test]
    fn centered_scroll_first_tab_stays_at_zero() {
        let mut ws = make_ws_with_tabs(&["aa", "bb", "cc", "dd", "ee"]);
        ws.active_tab = 0;
        let chromes = chromes_from_ws(&ws);
        let area = Rect::new(0, 0, 25, 1);
        let scroll = centered_tab_scroll(&chromes, ws.active_tab, TabStatusMode::Off, area);
        assert_eq!(scroll, 0);
    }

    #[test]
    fn centered_scroll_last_tab_scrolls_to_show_it() {
        // With the no-sliver fix, the last tab's former 7-col sliver at scroll=2
        // is now width 0, so max_tab_scroll becomes 3 and the resolved scroll is 3.
        let mut ws = make_ws_with_tabs(&["aa", "bb", "cc", "dd", "ee"]);
        ws.active_tab = 4;
        let chromes = chromes_from_ws(&ws);
        let area = Rect::new(0, 0, 25, 1);
        assert_eq!(
            centered_tab_scroll(&chromes, ws.active_tab, TabStatusMode::Off, area),
            3
        );

        let view = compute_tab_bar_view(
            chromes,
            ws.active_tab,
            TabStatusMode::Off,
            area,
            0,
            true,
            false,
        );
        assert_eq!(view.scroll, 3);
        assert!(
            view.tab_hit_areas[4].width > 0,
            "active/last tab must be visible"
        );
    }

    #[test]
    fn compute_tab_bar_view_returns_default_for_zero_width_area() {
        let ws = make_ws_with_tabs(&["aa", "bb"]);
        let chromes = chromes_from_ws(&ws);
        let view = compute_tab_bar_view(
            chromes,
            ws.active_tab,
            TabStatusMode::Off,
            Rect::new(0, 0, 0, 1),
            0,
            true,
            true,
        );
        assert_eq!(view.scroll, 0);
        assert!(view.tab_hit_areas.is_empty());
        assert_eq!(view.scroll_left_hit_area.width, 0);
        assert_eq!(view.scroll_right_hit_area.width, 0);
        assert_eq!(view.new_tab_hit_area.width, 0);
    }

    #[test]
    fn layout_positions_tabs_sequentially_with_gap() {
        let ws = make_ws_with_tabs(&["ab", "cd", "ef"]);
        let chromes = chromes_from_ws(&ws);
        let area = Rect::new(5, 3, 50, 1);
        let rects = layout_tab_hit_areas(&chromes, TabStatusMode::Off, area, 0);

        assert_eq!(rects[0], Rect::new(5, 3, 8, 1));
        assert_eq!(rects[1], Rect::new(14, 3, 8, 1));
        assert_eq!(rects[2], Rect::new(23, 3, 8, 1));
    }

    #[test]
    fn layout_hides_last_tab_below_min_width() {
        // Previously the last tab got a 2-col sliver (remaining=2 < MIN_TAB_WIDTH=8).
        // Now it is hidden (width 0) so overflow detection fires correctly.
        let ws = make_ws_with_tabs(&["ab", "cd", "ef"]);
        let chromes = chromes_from_ws(&ws);
        let area = Rect::new(0, 0, 20, 1);
        let rects = layout_tab_hit_areas(&chromes, TabStatusMode::Off, area, 0);

        assert_eq!(rects[0].width, 8);
        assert_eq!(rects[0].x, 0);
        assert_eq!(rects[1].width, 8);
        assert_eq!(rects[1].x, 9);
        assert_eq!(rects[2].width, 0);
    }

    #[test]
    fn layout_no_left_clipping_scrolled_tabs_are_zeroed() {
        let ws = make_ws_with_tabs(&["ab", "cd", "ef", "gh"]);
        let chromes = chromes_from_ws(&ws);
        let area = Rect::new(0, 0, 50, 1);
        let rects = layout_tab_hit_areas(&chromes, TabStatusMode::Off, area, 2);

        assert_eq!(rects[0].width, 0);
        assert_eq!(rects[1].width, 0);
        assert_eq!(rects[2].x, 0);
        assert_eq!(rects[2].width, 8);
        assert_eq!(rects[3].x, 9);
        assert_eq!(rects[3].width, 8);
    }

    #[test]
    fn layout_hides_non_first_tab_below_min_width() {
        // Previously the second tab got a 1-col sliver (remaining=1 < MIN_TAB_WIDTH=8).
        // Now non-first tabs below the threshold are hidden (width 0).
        let ws = make_ws_with_tabs(&["ab", "cd", "ef"]);
        let chromes = chromes_from_ws(&ws);
        let area = Rect::new(0, 0, 10, 1);
        let rects = layout_tab_hit_areas(&chromes, TabStatusMode::Off, area, 0);

        assert_eq!(rects[0].width, 8);
        assert_eq!(rects[1].width, 0);
        assert_eq!(rects[2].width, 0);
    }

    #[test]
    fn layout_with_nonzero_area_x_offset() {
        let ws = make_ws_with_tabs(&["ab", "cd"]);
        let chromes = chromes_from_ws(&ws);
        let area = Rect::new(10, 0, 20, 1);
        let rects = layout_tab_hit_areas(&chromes, TabStatusMode::Off, area, 0);

        assert_eq!(rects[0], Rect::new(10, 0, 8, 1));
        assert_eq!(rects[1], Rect::new(19, 0, 8, 1));
    }

    fn app_with_tab_bar(names: &[&str]) -> (AppState, TabBarView) {
        app_with_tab_bar_in(names, Rect::new(0, 0, 50, 1), false, 0)
    }

    fn app_with_tab_bar_in(
        names: &[&str],
        area: Rect,
        mouse_chrome: bool,
        active_tab: usize,
    ) -> (AppState, TabBarView) {
        let mut app = AppState::test_new();
        let mut ws = make_ws_with_tabs(names);
        ws.active_tab = active_tab;
        app.workspaces = vec![ws];
        app.active = Some(0);

        let chromes = chromes_from_ws(&app.workspaces[0]);
        let view = compute_tab_bar_view(
            chromes,
            active_tab,
            TabStatusMode::Off,
            area,
            0,
            true,
            mouse_chrome,
        );
        app.view.tab_hit_areas = view.tab_hit_areas.clone();
        app.view.tab_chrome = view.tab_chrome.clone();
        app.view.tab_status_mode = view.tab_status_mode;
        app.view.tab_compressed_width = view.compressed_width;
        app.view.tab_scroll_left_hit_area = view.scroll_left_hit_area;
        app.view.tab_scroll_right_hit_area = view.scroll_right_hit_area;
        (app, view)
    }

    #[test]
    fn drop_indicator_x_at_start_returns_first_tab_x() {
        let (app, _) = app_with_tab_bar(&["ab", "cd", "ef"]);
        let x = tab_drop_indicator_x(&app, &app.workspaces[0], 0);
        assert_eq!(x, Some(0));
    }

    #[test]
    fn drop_indicator_x_between_tabs_is_one_before_target() {
        let (app, _) = app_with_tab_bar(&["ab", "cd", "ef"]);
        assert_eq!(tab_drop_indicator_x(&app, &app.workspaces[0], 1), Some(8));
        assert_eq!(tab_drop_indicator_x(&app, &app.workspaces[0], 2), Some(17));
    }

    #[test]
    fn drop_indicator_x_at_end_returns_after_last_tab() {
        let (app, _) = app_with_tab_bar(&["ab", "cd", "ef"]);
        let tab_count = app.workspaces[0].tabs.len();
        assert_eq!(
            tab_drop_indicator_x(&app, &app.workspaces[0], tab_count),
            Some(26),
        );
    }

    #[test]
    fn drop_indicator_x_at_known_widths() {
        let (app, view) = app_with_tab_bar(&["hello", "world"]);
        assert_eq!(view.tab_hit_areas[0], Rect::new(0, 0, 9, 1));
        assert_eq!(view.tab_hit_areas[1], Rect::new(10, 0, 9, 1));

        assert_eq!(tab_drop_indicator_x(&app, &app.workspaces[0], 0), Some(0));
        assert_eq!(tab_drop_indicator_x(&app, &app.workspaces[0], 1), Some(9));
        assert_eq!(tab_drop_indicator_x(&app, &app.workspaces[0], 2), Some(19));
    }

    #[test]
    fn drop_indicator_x_at_start_uses_left_scroll_button_when_left_clipped() {
        let area = Rect::new(0, 0, 25, 1);
        let (app, view) = app_with_tab_bar_in(&["aa", "bb", "cc", "dd", "ee"], area, true, 2);
        assert_eq!(view.tab_hit_areas[0].width, 0);
        assert_eq!(tab_drop_indicator_x(&app, &app.workspaces[0], 0), Some(3));
    }

    #[test]
    fn drop_indicator_x_at_end_uses_right_scroll_button_when_right_clipped() {
        // With the no-sliver fix, the trailing tab is hidden instead of slivered,
        // collapsing trailing_x and moving the right scroll button left.
        let area = Rect::new(0, 0, 25, 1);
        let (app, view) = app_with_tab_bar_in(&["aa", "bb", "cc", "dd", "ee"], area, true, 2);
        assert_eq!(view.tab_hit_areas[4].width, 0);
        assert_eq!(view.scroll_right_hit_area.x.saturating_sub(1), 10);
        assert_eq!(tab_drop_indicator_x(&app, &app.workspaces[0], 5), Some(10));
    }

    // --- New unit tests for TabChrome ---

    #[test]
    fn tab_status_dot_truth_table() {
        use crate::app::state::Palette;
        use crate::detect::AgentState;

        let p = Palette::catppuccin();
        let tick = 0u32;

        // Off mode → always None
        assert!(tab_status_dot(AgentState::Blocked, false, TabStatusMode::Off, tick, &p).is_none());
        assert!(tab_status_dot(AgentState::Working, false, TabStatusMode::Off, tick, &p).is_none());
        assert!(tab_status_dot(AgentState::Idle, false, TabStatusMode::Off, tick, &p).is_none());
        assert!(tab_status_dot(AgentState::Idle, true, TabStatusMode::Off, tick, &p).is_none());
        assert!(tab_status_dot(AgentState::Unknown, false, TabStatusMode::Off, tick, &p).is_none());

        // Attention mode → only Blocked and Idle+unseen
        assert!(tab_status_dot(
            AgentState::Blocked,
            false,
            TabStatusMode::Attention,
            tick,
            &p
        )
        .is_some());
        assert!(tab_status_dot(
            AgentState::Blocked,
            true,
            TabStatusMode::Attention,
            tick,
            &p
        )
        .is_some());
        assert!(
            tab_status_dot(AgentState::Idle, false, TabStatusMode::Attention, tick, &p).is_some()
        );
        assert!(tab_status_dot(
            AgentState::Working,
            false,
            TabStatusMode::Attention,
            tick,
            &p
        )
        .is_none());
        assert!(
            tab_status_dot(AgentState::Idle, true, TabStatusMode::Attention, tick, &p).is_none()
        );
        assert!(tab_status_dot(
            AgentState::Unknown,
            false,
            TabStatusMode::Attention,
            tick,
            &p
        )
        .is_none());

        // All mode → everything except Unknown
        assert!(tab_status_dot(AgentState::Blocked, false, TabStatusMode::All, tick, &p).is_some());
        assert!(tab_status_dot(AgentState::Working, false, TabStatusMode::All, tick, &p).is_some());
        assert!(tab_status_dot(AgentState::Idle, false, TabStatusMode::All, tick, &p).is_some());
        assert!(tab_status_dot(AgentState::Idle, true, TabStatusMode::All, tick, &p).is_some());
        assert!(tab_status_dot(AgentState::Unknown, false, TabStatusMode::All, tick, &p).is_none());
    }

    #[test]
    fn display_width_invariants() {
        // mode == Off, no zoom, no dot: name.len()
        let c = TabChrome {
            status: None,
            name: "hello".into(),
            zoomed: false,
        };
        assert_eq!(c.display_width(TabStatusMode::Off), 5);

        // mode == Off, zoomed: name.len() + 2
        let c = TabChrome {
            status: None,
            name: "hello".into(),
            zoomed: true,
        };
        assert_eq!(c.display_width(TabStatusMode::Off), 7);

        // mode == All, no dot, no zoom: name.len() + 2 (slot reservation)
        let c = TabChrome {
            status: None,
            name: "hello".into(),
            zoomed: false,
        };
        assert_eq!(c.display_width(TabStatusMode::All), 7);

        // mode == All, with dot, with zoom: name.len() + 4
        let c = TabChrome {
            status: Some(TabStatusDot {
                glyph: "●",
                style: Style::default(),
            }),
            name: "hello".into(),
            zoomed: true,
        };
        assert_eq!(c.display_width(TabStatusMode::All), 9);

        // Width is independent of which dot variant
        let c1 = TabChrome {
            status: Some(TabStatusDot {
                glyph: "●",
                style: Style::default().fg(ratatui::style::Color::Red),
            }),
            name: "test".into(),
            zoomed: false,
        };
        let c2 = TabChrome {
            status: Some(TabStatusDot {
                glyph: "○",
                style: Style::default().fg(ratatui::style::Color::Green),
            }),
            name: "test".into(),
            zoomed: false,
        };
        assert_eq!(
            c1.display_width(TabStatusMode::All),
            c2.display_width(TabStatusMode::All)
        );
    }

    #[test]
    fn to_spans_ordering_and_padding() {
        // With status slot (mode=All), with dot
        let c = TabChrome {
            status: Some(TabStatusDot {
                glyph: "●",
                style: Style::default().fg(ratatui::style::Color::Red),
            }),
            name: "test".into(),
            zoomed: true,
        };
        let spans = c.to_spans(TabStatusMode::All, 15, false);
        assert_eq!(spans[0].content.as_ref(), " ");
        assert_eq!(spans[1].content.as_ref(), "●");
        assert_eq!(spans[2].content.as_ref(), " ");
        assert_eq!(spans[3].content.as_ref(), "test");
        assert_eq!(spans[4].content.as_ref(), " Z");
        assert_eq!(spans[5].content.len(), 6);

        // Reserved-but-empty status slot (mode=All, no dot) must not override fg
        let c = TabChrome {
            status: None,
            name: "abc".into(),
            zoomed: false,
        };
        let spans = c.to_spans(TabStatusMode::All, 10, false);
        assert_eq!(spans[0].content.as_ref(), " ");
        assert_eq!(spans[1].content.as_ref(), "  ");
        assert!(spans[1].style.fg.is_none(), "empty slot must not set fg");
        assert_eq!(spans[2].content.as_ref(), "abc");
        assert_eq!(spans[3].content.len(), 4);

        // No status slot (mode=Off)
        let c = TabChrome {
            status: None,
            name: "xyz".into(),
            zoomed: false,
        };
        let spans = c.to_spans(TabStatusMode::Off, 8, false);
        assert_eq!(spans[0].content.as_ref(), " ");
        assert_eq!(spans[1].content.as_ref(), "xyz");
        assert_eq!(spans[2].content.len(), 4);
    }

    #[test]
    fn width_increases_by_two_when_mode_switches_off_to_all() {
        // Names long enough that both modes clear MIN_TAB_WIDTH, so the status
        // slot's two columns are observable in the laid-out width. (Short names
        // sit under the floor in both modes — see
        // short_tab_width_stays_at_floor_across_modes.)
        let ws = make_ws_with_tabs(&["alpha-one", "beta-two", "gamma-three"]);
        let chromes = chromes_from_ws(&ws);
        let area = Rect::new(0, 0, 120, 1);

        let view_off = compute_tab_bar_view(
            chromes.clone(),
            ws.active_tab,
            TabStatusMode::Off,
            area,
            0,
            true,
            false,
        );
        let view_all = compute_tab_bar_view(
            chromes,
            ws.active_tab,
            TabStatusMode::All,
            area,
            0,
            true,
            false,
        );

        for i in 0..3 {
            assert_eq!(
                view_all.tab_hit_areas[i].width,
                view_off.tab_hit_areas[i].width + 2,
                "tab {i} width should increase by 2"
            );
        }
    }

    #[test]
    fn short_tab_width_stays_at_floor_across_modes() {
        // Short names fit within MIN_TAB_WIDTH in both modes, so the status slot
        // is absorbed by the floor and the laid-out width does not change.
        let ws = make_ws_with_tabs(&["ab", "cd", "ef"]);
        let chromes = chromes_from_ws(&ws);
        let area = Rect::new(0, 0, 80, 1);

        let view_off = compute_tab_bar_view(
            chromes.clone(),
            ws.active_tab,
            TabStatusMode::Off,
            area,
            0,
            true,
            false,
        );
        let view_all = compute_tab_bar_view(
            chromes,
            ws.active_tab,
            TabStatusMode::All,
            area,
            0,
            true,
            false,
        );

        for i in 0..3 {
            assert_eq!(
                view_off.tab_hit_areas[i].width, MIN_TAB_WIDTH,
                "tab {i} should sit at the floor in Off mode"
            );
            assert_eq!(
                view_all.tab_hit_areas[i].width, MIN_TAB_WIDTH,
                "tab {i} should stay at the floor in All mode"
            );
        }
    }

    // --- Render snapshot tests ---

    fn render_to_buffer(app: &AppState, area: Rect) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(area.x + area.width, area.y + area.height.max(1));
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_tab_bar(app, frame, area))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn render_snapshot_active_tab_bg_paints_full_rect() {
        let mut app = AppState::test_new();
        let ws = make_ws_with_tabs(&["ab", "cd", "ef"]);
        app.workspaces = vec![ws];
        app.active = Some(0);
        app.show_tab_status = TabStatusMode::All;

        let area = Rect::new(0, 0, 40, 1);
        app.view.tab_bar_rect = area;
        let chromes = chromes_from_ws(&app.workspaces[0]);
        let view = compute_tab_bar_view(
            chromes,
            app.workspaces[0].active_tab,
            TabStatusMode::All,
            area,
            0,
            true,
            false,
        );
        let active_rect = view.tab_hit_areas[0];
        app.view.tab_hit_areas = view.tab_hit_areas;
        app.view.tab_chrome = view.tab_chrome;
        app.view.tab_status_mode = view.tab_status_mode;

        let buffer = render_to_buffer(&app, area);

        // Active-tab accent bg must cover every cell of the active tab's rect.
        for x in active_rect.x..active_rect.x + active_rect.width {
            let cell = &buffer[(x, active_rect.y)];
            assert_eq!(
                cell.bg, app.palette.accent,
                "cell at x={x} should have accent bg, got {:?}",
                cell.bg
            );
        }
    }

    #[test]
    fn render_snapshot_glyphs_for_mixed_states() {
        // Build chromes manually with the glyphs `agent_icon` produces (the same
        // icons the sidebar/agents panel uses). This pins the render path: a
        // TabChrome with a given dot.glyph produces that glyph in the buffer at
        // the expected column with the expected fg color.
        let p = crate::app::state::Palette::catppuccin();
        let working_glyph = crate::ui::spinner_frame(0);
        let chromes = vec![
            TabChrome {
                status: Some(TabStatusDot {
                    glyph: "◉",
                    style: Style::default().fg(p.red),
                }),
                name: "a".into(),
                zoomed: false,
            },
            TabChrome {
                status: Some(TabStatusDot {
                    glyph: working_glyph,
                    style: Style::default().fg(p.yellow),
                }),
                name: "b".into(),
                zoomed: false,
            },
            TabChrome {
                status: Some(TabStatusDot {
                    glyph: "●",
                    style: Style::default().fg(p.teal),
                }),
                name: "c".into(),
                zoomed: false,
            },
            TabChrome {
                status: Some(TabStatusDot {
                    glyph: "✓",
                    style: Style::default().fg(p.green),
                }),
                name: "d".into(),
                zoomed: false,
            },
        ];

        let mut app = AppState::test_new();
        let mut ws = make_ws_with_tabs(&["a", "b", "c", "d"]);
        ws.active_tab = 0;
        app.workspaces = vec![ws];
        app.active = Some(0);

        let area = Rect::new(0, 0, 60, 1);
        app.view.tab_bar_rect = area;
        let view = compute_tab_bar_view(chromes, 0, TabStatusMode::All, area, 0, true, false);
        app.view.tab_hit_areas = view.tab_hit_areas.clone();
        app.view.tab_chrome = view.tab_chrome;
        app.view.tab_status_mode = view.tab_status_mode;

        let buffer = render_to_buffer(&app, area);

        let expected = [
            (p.red, "◉"),
            (p.yellow, working_glyph),
            (p.teal, "●"),
            (p.green, "✓"),
        ];
        for (idx, (color, glyph)) in expected.iter().enumerate() {
            let rect = view.tab_hit_areas[idx];
            // Leading space + dot → dot is at rect.x + 1.
            let cell = &buffer[(rect.x + 1, rect.y)];
            assert_eq!(cell.symbol(), *glyph, "tab {idx} glyph");
            assert_eq!(cell.fg, *color, "tab {idx} fg");
        }
    }

    #[test]
    fn render_uses_view_mode_not_app_show_tab_status() {
        // Pins the mode-drift fix: render reads app.view.tab_status_mode, not
        // app.show_tab_status. Set the two fields to different values and
        // verify rendering uses the view-stored mode.
        let mut app = AppState::test_new();
        let ws = make_ws_with_tabs(&["a", "b"]);
        app.workspaces = vec![ws];
        app.active = Some(0);
        app.show_tab_status = TabStatusMode::All;

        let area = Rect::new(0, 0, 30, 1);
        app.view.tab_bar_rect = area;
        let chromes = chromes_from_ws(&app.workspaces[0]);
        // Compute layout with Off — narrower tabs, no status slot.
        let view = compute_tab_bar_view(
            chromes,
            app.workspaces[0].active_tab,
            TabStatusMode::Off,
            area,
            0,
            true,
            false,
        );
        let off_tab_width = view.tab_hit_areas[0].width;
        app.view.tab_hit_areas = view.tab_hit_areas;
        app.view.tab_chrome = view.tab_chrome;
        app.view.tab_status_mode = view.tab_status_mode;
        assert_eq!(app.view.tab_status_mode, TabStatusMode::Off);

        let buffer = render_to_buffer(&app, area);
        // First non-space char after the leading space should be the tab name 'a'
        // (mode=Off → no status slot column). If render incorrectly read
        // app.show_tab_status (=All), it would render two extra spaces or a
        // styled empty slot before the name.
        let cell = &buffer[(1, 0)];
        assert_eq!(
            cell.symbol(),
            "a",
            "expected name at col 1 when view mode is Off (tab width {off_tab_width})"
        );
    }

    // --- No-sliver invariant tests ---

    #[test]
    fn no_sliver_invariant_direct_layout() {
        let ws = make_ws_with_tabs(&["alpha", "alpha", "alpha", "alpha", "alpha"]);
        let chromes = chromes_from_ws(&ws);
        for width in 10..60 {
            let area = Rect::new(0, 0, width, 1);
            let rects = layout_tab_hit_areas(&chromes, TabStatusMode::Off, area, 0);
            let mut placed_first = false;
            for r in &rects {
                if r.width == 0 {
                    continue;
                }
                if !placed_first {
                    placed_first = true;
                    continue;
                }
                assert!(
                    r.width >= MIN_TAB_WIDTH,
                    "width={width}: non-first tab has sliver width {}",
                    r.width
                );
            }
        }
    }

    #[test]
    fn no_sliver_invariant_via_compute_tab_bar_view() {
        let names = &["alpha", "alpha", "alpha", "alpha", "alpha"];
        for active in [0, 2, 4] {
            for width in 30..55 {
                let mut ws = make_ws_with_tabs(names);
                ws.active_tab = active;
                let chromes = chromes_from_ws(&ws);
                let area = Rect::new(0, 0, width, 1);
                let view =
                    compute_tab_bar_view(chromes, active, TabStatusMode::Off, area, 0, true, true);
                for (idx, r) in view.tab_hit_areas.iter().enumerate() {
                    if r.width == 0 {
                        continue;
                    }
                    let is_first_visible =
                        view.tab_hit_areas[..idx].iter().all(|prev| prev.width == 0);
                    if is_first_visible {
                        continue;
                    }
                    assert!(
                        r.width >= MIN_TAB_WIDTH,
                        "active={active} width={width} tab {idx} has sliver width {}",
                        r.width
                    );
                }
            }
        }
    }

    #[test]
    fn active_tab_never_hidden_at_resolved_scroll() {
        let names = &["alpha", "alpha", "alpha", "alpha", "alpha"];
        for active in [0, 2, 4] {
            for width in 25..55 {
                let mut ws = make_ws_with_tabs(names);
                ws.active_tab = active;
                let chromes = chromes_from_ws(&ws);
                let area = Rect::new(0, 0, width, 1);
                let view =
                    compute_tab_bar_view(chromes, active, TabStatusMode::Off, area, 0, true, true);
                assert!(
                    view.tab_hit_areas[active].width > 0,
                    "active={active} width={width}: active tab hidden"
                );
            }
        }
    }

    #[test]
    fn active_tab_never_hidden_when_max_scroll_clamps_centering() {
        // Adversarial: the .min(max_scroll) clamp in compute_tab_bar_view could in
        // principle pull the resolved scroll below the centered value and hide the
        // active tab, since max_tab_scroll is computed from the LAST tab's
        // visibility, not the active tab's. The design's Edit 3 proof argues this
        // cannot happen because visible tabs form a contiguous fully-visible suffix.
        // Pin that empirically with many non-uniform tabs, the active tab far right,
        // across narrow widths where max_scroll resolves small.
        let names = &[
            "one", "two", "three", "fourfour", "five", "six", "seven", "eight",
        ];
        for active in [5, 6, 7] {
            for width in 18..60 {
                let mut ws = make_ws_with_tabs(names);
                ws.active_tab = active;
                let chromes = chromes_from_ws(&ws);
                let area = Rect::new(0, 0, width, 1);
                let view =
                    compute_tab_bar_view(chromes, active, TabStatusMode::Off, area, 0, true, true);
                assert!(
                    view.tab_hit_areas[active].width > 0,
                    "active={active} width={width}: active tab hidden by max_scroll clamp"
                );
            }
        }
    }

    #[test]
    fn narrow_terminal_fallback_first_tab_rendered() {
        let ws = make_ws_with_tabs(&["alpha", "bravo"]);
        let chromes = chromes_from_ws(&ws);
        for width in 1..MIN_TAB_WIDTH {
            let area = Rect::new(0, 0, width, 1);
            let rects = layout_tab_hit_areas(&chromes, TabStatusMode::Off, area, 0);
            assert!(
                rects[0].width >= 1,
                "width={width}: first tab must render (got width {})",
                rects[0].width
            );
            assert_eq!(
                rects[1].width, 0,
                "width={width}: second tab must be hidden"
            );
        }
    }

    #[test]
    fn truncation_preserved_above_min_width() {
        let ws = make_ws_with_tabs(&["longername"]);
        let chromes = chromes_from_ws(&ws);
        let desired = tab_width(&chromes[0], TabStatusMode::Off);
        let remaining = desired - 2;
        assert!(remaining >= MIN_TAB_WIDTH);
        let area = Rect::new(0, 0, remaining, 1);
        let rects = layout_tab_hit_areas(&chromes, TabStatusMode::Off, area, 0);
        assert_eq!(rects[0].width, remaining);
    }

    #[test]
    fn compression_activates_before_scroll_for_flagship_scenario() {
        let ws = make_ws_with_tabs(&["alpha", "alpha", "alpha", "alpha", "alpha"]);
        let chromes = chromes_from_ws(&ws);
        let area = Rect::new(0, 0, 50, 1);
        let view = compute_tab_bar_view(chromes, 0, TabStatusMode::Off, area, 0, true, true);
        assert_eq!(
            view.scroll_left_hit_area.width, 0,
            "compression should prevent scroll buttons"
        );
        assert!(view.compressed_width.is_some(), "compression must activate");
        assert!(
            view.tab_hit_areas.iter().all(|r| r.width > 0),
            "all tabs visible under compression"
        );
    }

    #[test]
    fn scroll_activates_when_compression_insufficient() {
        let ws = make_ws_with_tabs(&["alpha", "alpha", "alpha", "alpha", "alpha", "alpha"]);
        let chromes = chromes_from_ws(&ws);
        // 6 tabs: compression needs (w-5)/6 >= 8 → w >= 53. all_tabs_area = area-3.
        // area=40 → all_tabs_area=37 → (37-5)/6 = 5 < 8 → compression fails.
        let area = Rect::new(0, 0, 40, 1);
        let view = compute_tab_bar_view(chromes, 0, TabStatusMode::Off, area, 0, true, true);
        assert!(
            view.scroll_left_hit_area.width > 0,
            "scroll buttons must appear when compression insufficient"
        );
        assert!(
            view.compressed_width.is_none(),
            "compression must not activate"
        );
    }

    #[test]
    fn tab_status_mode_boundary_triggers_compression() {
        let ws = make_ws_with_tabs(&["alpha", "alpha", "alpha"]);
        let chromes_off: Vec<TabChrome> = chromes_from_ws(&ws);
        let chromes_all: Vec<TabChrome> = (0..ws.tabs.len())
            .map(|i| {
                let name = ws
                    .tab_display_name(i)
                    .unwrap_or_else(|| (i + 1).to_string());
                TabChrome {
                    status: Some(TabStatusDot {
                        glyph: "●",
                        style: Style::default(),
                    }),
                    name,
                    zoomed: false,
                }
            })
            .collect();

        let width_off = tab_width(&chromes_off[0], TabStatusMode::Off);
        let width_all = tab_width(&chromes_all[0], TabStatusMode::All);
        assert_eq!(width_all, width_off + 2);

        let area_width = width_off * 3 + 2 + NEW_TAB_WIDTH;
        let area = Rect::new(0, 0, area_width, 1);

        let view_off =
            compute_tab_bar_view(chromes_off, 0, TabStatusMode::Off, area, 0, true, true);
        assert_eq!(
            view_off.scroll_left_hit_area.width, 0,
            "Off mode: no overflow"
        );
        assert!(
            view_off.compressed_width.is_none(),
            "Off mode: no compression needed"
        );

        let view_all = compute_tab_bar_view(
            chromes_all.clone(),
            0,
            TabStatusMode::All,
            area,
            0,
            true,
            true,
        );
        assert!(
            view_all.compressed_width.is_some(),
            "All mode: compression triggered by +2 status columns"
        );
        assert_eq!(
            view_all.scroll_left_hit_area.width, 0,
            "All mode: compression prevents scroll"
        );

        let view_attention = compute_tab_bar_view(
            chromes_all,
            0,
            TabStatusMode::Attention,
            area,
            0,
            true,
            true,
        );
        assert!(
            view_attention.compressed_width.is_some(),
            "Attention mode: compression triggered by +2 status columns"
        );
    }

    #[test]
    fn new_tab_button_reachable_across_overflow_transition() {
        let ws = make_ws_with_tabs(&["ab", "cd", "ef"]);
        let chromes = chromes_from_ws(&ws);

        let no_overflow_area = Rect::new(0, 0, 30, 1);
        let view = compute_tab_bar_view(
            chromes.clone(),
            0,
            TabStatusMode::Off,
            no_overflow_area,
            0,
            true,
            true,
        );
        assert_eq!(view.new_tab_hit_area.width, NEW_TAB_WIDTH);

        let overflow_area = Rect::new(0, 0, 21, 1);
        let view =
            compute_tab_bar_view(chromes, 0, TabStatusMode::Off, overflow_area, 0, true, true);
        assert_eq!(view.new_tab_hit_area.width, NEW_TAB_WIDTH);
    }

    #[test]
    fn happy_path_unchanged_when_tabs_fit() {
        let ws = make_ws_with_tabs(&["ab", "cd", "ef"]);
        let chromes = chromes_from_ws(&ws);
        let area = Rect::new(5, 3, 50, 1);
        let rects = layout_tab_hit_areas(&chromes, TabStatusMode::Off, area, 0);
        assert_eq!(rects[0], Rect::new(5, 3, 8, 1));
        assert_eq!(rects[1], Rect::new(14, 3, 8, 1));
        assert_eq!(rects[2], Rect::new(23, 3, 8, 1));
    }

    // --- Compression tests ---

    #[test]
    fn compress_tab_widths_returns_none_when_tabs_fit_naturally() {
        let ws = make_ws_with_tabs(&["ab", "cd"]);
        let chromes = chromes_from_ws(&ws);
        assert!(compress_tab_widths(&chromes, TabStatusMode::Off, 30).is_none());
    }

    #[test]
    fn compress_tab_widths_returns_none_for_single_tab() {
        let ws = make_ws_with_tabs(&["a_long_tab_name"]);
        let chromes = chromes_from_ws(&ws);
        assert!(compress_tab_widths(&chromes, TabStatusMode::Off, 5).is_none());
    }

    #[test]
    fn compress_tab_widths_returns_none_when_result_below_min() {
        let ws = make_ws_with_tabs(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
        let chromes = chromes_from_ws(&ws);
        // 10 tabs need (w-9)/10 >= 8, so w >= 89. Test with w=50.
        assert!(compress_tab_widths(&chromes, TabStatusMode::Off, 50).is_none());
    }

    #[test]
    fn compress_tab_widths_succeeds_at_boundary() {
        let ws = make_ws_with_tabs(&["alpha", "bravo", "charlie"]);
        let chromes = chromes_from_ws(&ws);
        // 3 tabs: needs (w-2)/3 >= 8 → w >= 26
        assert!(compress_tab_widths(&chromes, TabStatusMode::Off, 25).is_none());
        let cw = compress_tab_widths(&chromes, TabStatusMode::Off, 26);
        assert_eq!(cw, Some(8));
    }

    #[test]
    fn compress_tab_widths_integer_rounding_check() {
        // 5-char names → width 9; "charlie" is 7 chars → width 11.
        // Natural with gaps = 9+9+11 + 2 = 31 > 26 → overflow.
        // Available=26: (26-2)/3 = 8 ≥ 8. Check: 3*8+2 = 26 ≤ 26 → OK.
        let ws = make_ws_with_tabs(&["alpha", "bravo", "charlie"]);
        let chromes = chromes_from_ws(&ws);
        let cw = compress_tab_widths(&chromes, TabStatusMode::Off, 26);
        assert_eq!(cw, Some(8));
        let n = 3u16;
        assert!(n * 8 + (n - 1) <= 26);
    }

    #[test]
    fn compression_uses_scroll_zero() {
        // 5-char names → width 9; "charlie" is 7 chars → width 11.
        // Natural with gaps = 9+9+11+9 + 3 = 41. all_tabs_area = 40-3 = 37 < 41
        // → overflow. Compression: (37-3)/4 = 8 ≥ 8 → compresses.
        let ws = make_ws_with_tabs(&["alpha", "bravo", "charlie", "delta"]);
        let chromes = chromes_from_ws(&ws);
        let area = Rect::new(0, 0, 40, 1);
        let view = compute_tab_bar_view(chromes, 0, TabStatusMode::Off, area, 5, false, true);
        assert_eq!(view.scroll, 0, "compression must use scroll=0");
        assert!(view.compressed_width.is_some());
    }

    #[test]
    fn compressed_tabs_all_visible() {
        // 5-char names → width 9; "charli" is 6 chars → width 10.
        // Natural with gaps = 9+9+10+9+9 + 4 = 50. all_tabs_area = 48-3 = 45 < 50
        // → overflow. Compression: (45-4)/5 = 8 ≥ 8. 5*8+4 = 44 ≤ 45 → OK.
        let ws = make_ws_with_tabs(&["alpha", "bravo", "charli", "delta", "echos"]);
        let chromes = chromes_from_ws(&ws);
        let area = Rect::new(0, 0, 48, 1);
        let view = compute_tab_bar_view(chromes, 2, TabStatusMode::Off, area, 0, true, true);
        assert!(view.compressed_width.is_some(), "compression must activate");
        assert!(
            view.tab_hit_areas.iter().all(|r| r.width > 0),
            "all tabs must be visible under compression"
        );
    }

    #[test]
    fn active_tab_never_hidden_under_compression() {
        let names = &["alpha", "bravo", "charlie", "delta"];
        for active in 0..4 {
            let mut ws = make_ws_with_tabs(names);
            ws.active_tab = active;
            let chromes = chromes_from_ws(&ws);
            let area = Rect::new(0, 0, 40, 1);
            let view =
                compute_tab_bar_view(chromes, active, TabStatusMode::Off, area, 0, true, true);
            // Assert compression actually fires (no vacuous pass) and that the
            // real contract — every tab visible, active included — holds.
            assert!(
                view.compressed_width.is_some(),
                "active={active}: expected compression at width 40"
            );
            assert!(
                view.tab_hit_areas.iter().all(|r| r.width > 0),
                "active={active}: all tabs (incl. active) must be visible under compression"
            );
        }
    }

    #[test]
    fn no_sliver_invariant_under_compression() {
        let ws = make_ws_with_tabs(&["alpha", "bravo", "charlie", "delta", "echo"]);
        let chromes = chromes_from_ws(&ws);
        for width in 44..55 {
            let area = Rect::new(0, 0, width, 1);
            let view =
                compute_tab_bar_view(chromes.clone(), 0, TabStatusMode::Off, area, 0, true, true);
            for (idx, r) in view.tab_hit_areas.iter().enumerate() {
                if r.width == 0 {
                    continue;
                }
                if idx == 0 {
                    continue;
                }
                assert!(
                    r.width >= MIN_TAB_WIDTH,
                    "width={width} tab {idx}: sliver width {}",
                    r.width
                );
            }
        }
    }

    #[test]
    fn to_spans_truncates_to_fit_rect_width() {
        let c = TabChrome {
            status: None,
            name: "longername".into(),
            zoomed: false,
        };
        // mode=Off, no zoom: name_budget = rect_width - 1 = 4 → "lon…".
        let spans = c.to_spans(TabStatusMode::Off, 5, true);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains("lon…"),
            "expected truncated name, got: {text:?}"
        );
    }

    #[test]
    fn to_spans_no_truncation_when_name_fits() {
        let c = TabChrome {
            status: None,
            name: "abc".into(),
            zoomed: false,
        };
        // name_budget = 8 - 1 = 7 ≥ 3 chars → no truncation even with truncate=true.
        let spans = c.to_spans(TabStatusMode::Off, 8, true);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains("abc"),
            "name should not be truncated: {text:?}"
        );
        assert!(!text.contains("…"), "no ellipsis expected: {text:?}");
    }

    #[test]
    fn to_spans_does_not_truncate_when_flag_false() {
        let c = TabChrome {
            status: None,
            name: "longername".into(),
            zoomed: false,
        };
        // Even with a tight rect, truncate=false leaves the name intact (scroll
        // mode clips via the rect rather than inserting an ellipsis).
        let spans = c.to_spans(TabStatusMode::Off, 5, false);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains("longername"),
            "name must stay intact when truncate=false: {text:?}"
        );
        assert!(
            !text.contains("…"),
            "no ellipsis when truncate=false: {text:?}"
        );
    }

    #[test]
    fn layout_tab_hit_areas_compressed_uniform_widths() {
        let rects = layout_tab_hit_areas_compressed(4, 10, Rect::new(0, 0, 50, 1));
        assert_eq!(rects.len(), 4);
        for (i, r) in rects.iter().enumerate() {
            assert_eq!(r.width, 10, "tab {i} should have uniform width");
        }
        // Check gaps (1 col between tabs)
        assert_eq!(rects[0].x, 0);
        assert_eq!(rects[1].x, 11);
        assert_eq!(rects[2].x, 22);
        assert_eq!(rects[3].x, 33);
    }

    #[test]
    fn compression_non_mouse_mode() {
        // 5-char names → width 9; "charlie" is 7 chars → width 11.
        // Non-mouse mode uses the full area (no new-tab button reserved).
        // Natural with gaps = 9+9+11+9 + 3 = 41 > 35 → overflow.
        // Compression: (35-3)/4 = 8 ≥ 8. Check: 4*8+3 = 35 ≤ 35 → OK.
        let ws = make_ws_with_tabs(&["alpha", "bravo", "charlie", "delta"]);
        let chromes = chromes_from_ws(&ws);
        let area = Rect::new(0, 0, 35, 1);
        let view = compute_tab_bar_view(chromes, 0, TabStatusMode::Off, area, 0, true, false);
        assert!(view.compressed_width.is_some());
        assert_eq!(view.scroll, 0);
        assert!(view.tab_hit_areas.iter().all(|r| r.width > 0));
    }

    // --- Hidden count indicator tests ---

    #[test]
    fn hidden_count_left_equals_scroll() {
        let ws = make_ws_with_tabs(&["a", "b", "c", "d", "e", "f"]);
        let chromes = chromes_from_ws(&ws);
        // Force scroll mode: 6 tabs, narrow area. all_tabs_area = 30-3 = 27.
        // Compression: (27-5)/6 = 3 < 8 → fails → scroll.
        let area = Rect::new(0, 0, 30, 1);
        let view = compute_tab_bar_view(chromes, 3, TabStatusMode::Off, area, 0, true, true);
        assert!(view.scroll_left_hit_area.width > 0);
        let left_hidden = view
            .tab_hit_areas
            .iter()
            .take_while(|r| r.width == 0)
            .count();
        assert_eq!(left_hidden, view.scroll);
    }

    fn app_in_scroll_mode(names: &[&str], active: usize, area: Rect) -> AppState {
        let mut app = AppState::test_new();
        let mut ws = make_ws_with_tabs(names);
        ws.active_tab = active;
        app.workspaces = vec![ws];
        app.active = Some(0);
        app.mouse_capture = true;
        app.view.tab_bar_rect = area;
        let chromes = chromes_from_ws(&app.workspaces[0]);
        let view = compute_tab_bar_view(chromes, active, TabStatusMode::Off, area, 0, true, true);
        app.tab_scroll = view.scroll;
        app.view.tab_hit_areas = view.tab_hit_areas;
        app.view.tab_chrome = view.tab_chrome;
        app.view.tab_status_mode = view.tab_status_mode;
        app.view.tab_compressed_width = view.compressed_width;
        app.view.tab_scroll_left_hit_area = view.scroll_left_hit_area;
        app.view.tab_scroll_right_hit_area = view.scroll_right_hit_area;
        app.view.new_tab_hit_area = view.new_tab_hit_area;
        app
    }

    #[test]
    fn render_hidden_count_indicators_show_counts() {
        // 6 tabs in a narrow bar force scroll mode with hidden tabs on both edges.
        let area = Rect::new(0, 0, 30, 1);
        let app = app_in_scroll_mode(&["a", "b", "c", "d", "e", "f"], 3, area);
        assert!(
            app.view.tab_scroll_left_hit_area.width > 0,
            "must be in scroll mode"
        );

        let buffer = render_to_buffer(&app, area);
        let row = buffer_row_text(&buffer, area, 0);

        // Left indicator shows count of left-hidden tabs prefixed with ‹.
        let left_hidden = app.tab_scroll;
        assert!(
            row.contains(&format!("‹{left_hidden}")),
            "left indicator missing in row: {row:?}"
        );
        // Right indicator ends with › and shows the right-hidden count.
        let right_hidden = app
            .view
            .tab_hit_areas
            .iter()
            .rev()
            .take_while(|r| r.width == 0)
            .count();
        assert!(
            row.contains(&format!("{right_hidden}›")),
            "right indicator missing in row: {row:?}"
        );
    }

    #[test]
    fn render_hidden_count_caps_at_nine_plus() {
        // 12 tabs scrolled so more than 9 are hidden to the left.
        let names: Vec<&str> = vec!["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"];
        let area = Rect::new(0, 0, 30, 1);
        let app = app_in_scroll_mode(&names, 11, area);
        assert!(
            app.view.tab_scroll_left_hit_area.width > 0,
            "must be in scroll mode"
        );
        assert!(
            app.tab_scroll > 9,
            "need >9 tabs hidden left, got {}",
            app.tab_scroll
        );

        let buffer = render_to_buffer(&app, area);
        let row = buffer_row_text(&buffer, area, 0);
        assert!(row.contains("‹9+"), "expected 9+ cap in row: {row:?}");
    }

    #[test]
    fn render_disabled_left_indicator_is_neutral_chevron_not_zero() {
        // Active tab 0: scrolled fully left, so the left button is disabled.
        // It must show a neutral "‹", never "‹0".
        let names: Vec<&str> = vec!["a", "b", "c", "d", "e", "f"];
        let area = Rect::new(0, 0, 30, 1);
        let app = app_in_scroll_mode(&names, 0, area);
        assert!(
            app.view.tab_scroll_left_hit_area.width > 0,
            "must be in scroll mode"
        );
        assert_eq!(app.tab_scroll, 0, "left must be at the start (disabled)");

        let buffer = render_to_buffer(&app, area);
        let row = buffer_row_text(&buffer, area, 0);
        assert!(
            !row.contains("‹0"),
            "disabled left button must not show a literal zero count: {row:?}"
        );
        assert!(row.contains('‹'), "left chevron must still render: {row:?}");
    }
}
