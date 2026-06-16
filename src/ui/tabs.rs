use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    widgets::Paragraph,
    Frame,
};

use super::widgets::panel_contrast_fg;
use crate::app::AppState;

const MIN_TAB_WIDTH: u16 = 8;
const NEW_TAB_WIDTH: u16 = 3;
const TAB_SCROLL_BUTTON_WIDTH: u16 = 3;

#[derive(Debug, Clone, Default)]
pub(crate) struct TabBarView {
    pub scroll: usize,
    pub tab_hit_areas: Vec<Rect>,
    pub scroll_left_hit_area: Rect,
    pub scroll_right_hit_area: Rect,
    pub new_tab_hit_area: Rect,
}

fn tab_width(ws: &crate::workspace::Workspace, tab_idx: usize) -> u16 {
    (tab_chrome_label(ws, tab_idx).chars().count() as u16 + 4).max(MIN_TAB_WIDTH)
}

fn tab_chrome_label(ws: &crate::workspace::Workspace, tab_idx: usize) -> String {
    let name = ws
        .tab_display_name(tab_idx)
        .unwrap_or_else(|| (tab_idx + 1).to_string());
    if ws.tabs.get(tab_idx).is_some_and(|tab| tab.zoomed) {
        format!("{name} Z")
    } else {
        name
    }
}

fn layout_tab_hit_areas(ws: &crate::workspace::Workspace, area: Rect, scroll: usize) -> Vec<Rect> {
    let mut rects = vec![Rect::default(); ws.tabs.len()];
    if area.width == 0 || area.height == 0 {
        return rects;
    }

    let mut x = area.x;
    let right = area.x + area.width;
    for (idx, rect) in rects.iter_mut().enumerate().skip(scroll) {
        if x >= right {
            break;
        }
        let desired = tab_width(ws, idx);
        let remaining = right.saturating_sub(x);
        let width = desired.min(remaining).max(1);
        *rect = Rect::new(x, area.y, width, 1);
        x = x.saturating_add(width + 1);
    }
    rects
}

fn centered_tab_scroll(ws: &crate::workspace::Workspace, area: Rect) -> usize {
    let mut best_scroll = ws.active_tab;
    let mut best_distance = u16::MAX;
    let viewport_center = area.x.saturating_mul(2).saturating_add(area.width);

    for scroll in 0..=ws.active_tab {
        let rects = layout_tab_hit_areas(ws, area, scroll);
        let Some(active_rect) = rects.get(ws.active_tab).copied() else {
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

fn max_tab_scroll(ws: &crate::workspace::Workspace, area: Rect) -> usize {
    (0..ws.tabs.len())
        .find(|&scroll| {
            layout_tab_hit_areas(ws, area, scroll)
                .last()
                .is_some_and(|rect| rect.width > 0)
        })
        .unwrap_or(0)
}

pub(crate) fn compute_tab_bar_view(
    ws: &crate::workspace::Workspace,
    area: Rect,
    current_scroll: usize,
    follow_active: bool,
    mouse_chrome: bool,
) -> TabBarView {
    if area.width == 0 || area.height == 0 {
        return TabBarView::default();
    }

    if !mouse_chrome {
        let max_scroll = max_tab_scroll(ws, area);
        let scroll = if follow_active {
            centered_tab_scroll(ws, area).min(max_scroll)
        } else {
            current_scroll.min(max_scroll)
        };
        return TabBarView {
            scroll,
            tab_hit_areas: layout_tab_hit_areas(ws, area, scroll),
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
    let all_tabs = layout_tab_hit_areas(ws, all_tabs_area, 0);
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

    let max_scroll = max_tab_scroll(ws, tab_area);
    let scroll = if follow_active {
        centered_tab_scroll(ws, tab_area).min(max_scroll)
    } else {
        current_scroll.min(max_scroll)
    };
    let tab_hit_areas = layout_tab_hit_areas(ws, tab_area, scroll);
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
        let style = if can_scroll_left {
            Style::default().fg(p.overlay1).bg(p.surface0)
        } else {
            Style::default()
                .fg(p.overlay0)
                .bg(p.surface0)
                .add_modifier(Modifier::DIM)
        };
        frame.render_widget(
            Paragraph::new(" < ").style(style),
            app.view.tab_scroll_left_hit_area,
        );
    }

    if app.mouse_capture && app.view.tab_scroll_right_hit_area.width > 0 {
        let style = if can_scroll_right {
            Style::default().fg(p.overlay1).bg(p.surface0)
        } else {
            Style::default()
                .fg(p.overlay0)
                .bg(p.surface0)
                .add_modifier(Modifier::DIM)
        };
        frame.render_widget(
            Paragraph::new(" > ").style(style),
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
        let width = rect.width as usize;
        let name = tab_chrome_label(ws, idx);
        let text = format!(" {:width$}", name, width = width.saturating_sub(1));
        frame.render_widget(Paragraph::new(text).style(style), rect);
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

    if first_visible_idx.is_some_and(|idx| idx > 0) {
        let x = if app.mouse_capture && app.view.tab_scroll_left_hit_area.width > 0 {
            app.view.tab_scroll_left_hit_area.x + app.view.tab_scroll_left_hit_area.width
        } else {
            area.x
        };
        if x < area.x + area.width {
            frame.buffer_mut()[(x, area.y)]
                .set_symbol("…")
                .set_style(Style::default().fg(p.overlay0));
        }
    }
    if last_visible_idx.is_some_and(|idx| idx + 1 < ws.tabs.len()) {
        let x = if app.mouse_capture && app.view.tab_scroll_right_hit_area.width > 0 {
            app.view.tab_scroll_right_hit_area.x.saturating_sub(1)
        } else {
            area.x + area.width.saturating_sub(1)
        };
        if x >= area.x && x < area.x + area.width {
            frame.buffer_mut()[(x, area.y)]
                .set_symbol("…")
                .set_style(Style::default().fg(p.overlay0));
        }
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
        let view = compute_tab_bar_view(&app.workspaces[0], app.view.tab_bar_rect, 0, true, false);
        app.view.tab_hit_areas = view.tab_hit_areas;

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
        let mut ws = Workspace::test_new("test");
        ws.tabs[0].set_custom_name("abcdefgh".into());
        ws.tabs[0].zoomed = true;

        assert_eq!(tab_width(&ws, 0), 14);
    }

    // Characterization tests pinning the current tab bar layout behavior.
    // The TabChrome restructure must keep these passing. Pinned invariants:
    // gap=1 col between tabs, MIN_TAB_WIDTH=8, padding=4 cols around the label,
    // zoom suffix " Z", no status-dot column (TabStatusMode::Off baseline).
    // When step-3 introduces TabStatusMode, calls must default to Off to preserve
    // the literal expectations below.

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

    #[test]
    fn overflow_detected_when_tabs_exceed_area() {
        // 3 tabs named "ab" each → tab_width = (2+4).max(8) = 8.
        // Total with gaps: 8 + 1 + 8 + 1 + 8 = 26 cols.
        // mouse_chrome=true reserves NEW_TAB_WIDTH(3): all_tabs_area = area.width - 3.
        // At area=30, all_tabs_area=27, all three tabs fit.
        let ws = make_ws_with_tabs(&["ab", "cd", "ef"]);
        let area = Rect::new(0, 0, 30, 1);
        let view = compute_tab_bar_view(&ws, area, 0, true, true);
        assert_eq!(view.scroll_left_hit_area.width, 0);
        assert_eq!(view.scroll_right_hit_area.width, 0);

        // At area=21, all_tabs_area=18. tab2 starts at x=18, x>=right triggers break,
        // tab2 keeps default Rect (width=0). Overflow detected.
        let narrow_area = Rect::new(0, 0, 21, 1);
        let view_narrow = compute_tab_bar_view(&ws, narrow_area, 0, true, true);
        assert!(view_narrow.scroll_left_hit_area.width > 0);
        assert!(view_narrow.scroll_right_hit_area.width > 0);
    }

    #[test]
    fn no_overflow_non_mouse_mode_all_tabs_visible() {
        let ws = make_ws_with_tabs(&["ab", "cd", "ef"]);
        // 3 tabs × 8 width + 2 gaps = 26.
        let area = Rect::new(0, 0, 26, 1);
        let view = compute_tab_bar_view(&ws, area, 0, true, false);
        assert_eq!(view.tab_hit_areas.len(), 3);
        assert!(view.tab_hit_areas.iter().all(|r| r.width > 0));
    }

    #[test]
    fn overflow_in_non_mouse_mode_clips_last_tab() {
        let ws = make_ws_with_tabs(&["ab", "cd", "ef"]);
        // x advances: tab0@0 (w=8) → 9, tab1@9 (w=8) → 18, tab2@18 remaining=2 → w=2.
        let area = Rect::new(0, 0, 20, 1);
        let view = compute_tab_bar_view(&ws, area, 0, true, false);
        assert_eq!(view.tab_hit_areas[0].width, 8);
        assert_eq!(view.tab_hit_areas[1].width, 8);
        assert_eq!(view.tab_hit_areas[2].width, 2);
    }

    #[test]
    fn centered_scroll_centers_active_tab_in_viewport() {
        // 5 tabs × 8 width + 4 gaps = 44. Area=25. Active=middle.
        // scroll=1 places tab2 at x=9, w=8: center=26, distance=1 from viewport_center=25.
        // scroll=0 distance=18, scroll=2 distance=17. Best is scroll=1.
        let mut ws = make_ws_with_tabs(&["aa", "bb", "cc", "dd", "ee"]);
        ws.active_tab = 2;

        let area = Rect::new(0, 0, 25, 1);
        assert_eq!(centered_tab_scroll(&ws, area), 1);

        // Same scroll must surface on the integrated TabBarView.
        let view = compute_tab_bar_view(&ws, area, 0, true, false);
        assert_eq!(view.scroll, 1);
    }

    #[test]
    fn centered_scroll_first_tab_stays_at_zero() {
        let mut ws = make_ws_with_tabs(&["aa", "bb", "cc", "dd", "ee"]);
        ws.active_tab = 0;
        let area = Rect::new(0, 0, 25, 1);
        let scroll = centered_tab_scroll(&ws, area);
        assert_eq!(scroll, 0);
    }

    #[test]
    fn centered_scroll_last_tab_scrolls_to_show_it() {
        // scroll=3 places tab4 at x=9, w=8: distance=1. scroll<3 leaves tab4 farther right.
        let mut ws = make_ws_with_tabs(&["aa", "bb", "cc", "dd", "ee"]);
        ws.active_tab = 4;
        let area = Rect::new(0, 0, 25, 1);
        assert_eq!(centered_tab_scroll(&ws, area), 3);

        // compute_tab_bar_view clamps the centered scroll by max_tab_scroll.
        // max_tab_scroll for this layout is 2 (smallest scroll where tab4 stays visible),
        // so view.scroll should be min(3, 2) = 2 even though centered_tab_scroll wants 3.
        let view = compute_tab_bar_view(&ws, area, 0, true, false);
        assert_eq!(view.scroll, 2);
    }

    #[test]
    fn compute_tab_bar_view_returns_default_for_zero_width_area() {
        let ws = make_ws_with_tabs(&["aa", "bb"]);
        let view = compute_tab_bar_view(&ws, Rect::new(0, 0, 0, 1), 0, true, true);
        assert_eq!(view.scroll, 0);
        assert!(view.tab_hit_areas.is_empty());
        assert_eq!(view.scroll_left_hit_area.width, 0);
        assert_eq!(view.scroll_right_hit_area.width, 0);
        assert_eq!(view.new_tab_hit_area.width, 0);
    }

    #[test]
    fn layout_positions_tabs_sequentially_with_gap() {
        let ws = make_ws_with_tabs(&["ab", "cd", "ef"]);
        let area = Rect::new(5, 3, 50, 1);
        let rects = layout_tab_hit_areas(&ws, area, 0);

        // Each tab is width 8 with a 1-column gap between them.
        assert_eq!(rects[0], Rect::new(5, 3, 8, 1));
        assert_eq!(rects[1], Rect::new(14, 3, 8, 1));
        assert_eq!(rects[2], Rect::new(23, 3, 8, 1));
    }

    #[test]
    fn layout_clips_last_tab_on_right_edge() {
        let ws = make_ws_with_tabs(&["ab", "cd", "ef"]);
        // right=20. tab0@0(w=8), tab1@9(w=8), tab2@18 remaining=2 → w=2.
        let area = Rect::new(0, 0, 20, 1);
        let rects = layout_tab_hit_areas(&ws, area, 0);

        assert_eq!(rects[0].width, 8);
        assert_eq!(rects[0].x, 0);
        assert_eq!(rects[1].width, 8);
        assert_eq!(rects[1].x, 9);
        assert_eq!(rects[2].width, 2);
        assert_eq!(rects[2].x, 18);
    }

    #[test]
    fn layout_no_left_clipping_scrolled_tabs_are_zeroed() {
        let ws = make_ws_with_tabs(&["ab", "cd", "ef", "gh"]);
        let area = Rect::new(0, 0, 50, 1);
        let rects = layout_tab_hit_areas(&ws, area, 2);

        // Tabs before the scroll point get default Rect (width=0). The visible
        // tabs start at area.x — there is no left-clipping path to test.
        assert_eq!(rects[0].width, 0);
        assert_eq!(rects[1].width, 0);
        assert_eq!(rects[2].x, 0);
        assert_eq!(rects[2].width, 8);
        assert_eq!(rects[3].x, 9);
        assert_eq!(rects[3].width, 8);
    }

    #[test]
    fn layout_clipped_tab_has_at_least_one_column() {
        let ws = make_ws_with_tabs(&["ab", "cd", "ef"]);
        // area=10: tab0(0..8), tab1@9 remaining=1 → w=1 (.max(1) floor).
        let area = Rect::new(0, 0, 10, 1);
        let rects = layout_tab_hit_areas(&ws, area, 0);

        assert_eq!(rects[0].width, 8);
        assert_eq!(rects[1].x, 9);
        assert_eq!(rects[1].width, 1);
        assert_eq!(rects[2].width, 0);
    }

    #[test]
    fn layout_with_nonzero_area_x_offset() {
        let ws = make_ws_with_tabs(&["ab", "cd"]);
        // right = 10 + 20 = 30. tab0@10 w=8, tab1@19 w=8.
        let area = Rect::new(10, 0, 20, 1);
        let rects = layout_tab_hit_areas(&ws, area, 0);

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

        let view = compute_tab_bar_view(&app.workspaces[0], area, 0, true, mouse_chrome);
        app.view.tab_hit_areas = view.tab_hit_areas.clone();
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
        // 3 tabs × 8 width + 1-col gaps. tab1.x=9, tab2.x=18.
        let (app, _) = app_with_tab_bar(&["ab", "cd", "ef"]);
        assert_eq!(tab_drop_indicator_x(&app, &app.workspaces[0], 1), Some(8));
        assert_eq!(tab_drop_indicator_x(&app, &app.workspaces[0], 2), Some(17));
    }

    #[test]
    fn drop_indicator_x_at_end_returns_after_last_tab() {
        // tab2.x=18, tab2.width=8 → after = 26.
        let (app, _) = app_with_tab_bar(&["ab", "cd", "ef"]);
        let tab_count = app.workspaces[0].tabs.len();
        assert_eq!(
            tab_drop_indicator_x(&app, &app.workspaces[0], tab_count),
            Some(26),
        );
    }

    #[test]
    fn drop_indicator_x_at_known_widths() {
        // "hello" and "world" are both 5 chars → tab_width = (5+4).max(8) = 9.
        let (app, view) = app_with_tab_bar(&["hello", "world"]);
        assert_eq!(view.tab_hit_areas[0], Rect::new(0, 0, 9, 1));
        assert_eq!(view.tab_hit_areas[1], Rect::new(10, 0, 9, 1));

        assert_eq!(tab_drop_indicator_x(&app, &app.workspaces[0], 0), Some(0));
        assert_eq!(tab_drop_indicator_x(&app, &app.workspaces[0], 1), Some(9));
        assert_eq!(tab_drop_indicator_x(&app, &app.workspaces[0], 2), Some(19));
    }

    // Scroll-clipped variants: when mouse_chrome=true overflow forces some tabs offscreen,
    // tab_drop_indicator_x routes insert_idx==0 and insert_idx==tabs.len() through the
    // scroll button hit areas instead of the visible-tab edges.

    #[test]
    fn drop_indicator_x_at_start_uses_left_scroll_button_when_left_clipped() {
        // 5 tabs × 8 + 4 gaps = 44 cols don't fit in width=25; centering tab 2 hides tab 0.
        // Then first_visible.0 != 0 → tab_drop_indicator_x routes through left scroll button.
        let area = Rect::new(0, 0, 25, 1);
        let (app, view) = app_with_tab_bar_in(&["aa", "bb", "cc", "dd", "ee"], area, true, 2);
        assert_eq!(view.tab_hit_areas[0].width, 0);
        // TAB_SCROLL_BUTTON_WIDTH=3 at area.x=0 → button right edge = 3.
        assert_eq!(tab_drop_indicator_x(&app, &app.workspaces[0], 0), Some(3));
    }

    #[test]
    fn drop_indicator_x_at_end_uses_right_scroll_button_when_right_clipped() {
        // Same overflow setup; tab 4 also offscreen → routes through right scroll button.
        let area = Rect::new(0, 0, 25, 1);
        let (app, view) = app_with_tab_bar_in(&["aa", "bb", "cc", "dd", "ee"], area, true, 2);
        assert_eq!(view.tab_hit_areas[4].width, 0);
        // reserved_trailing = NEW_TAB_WIDTH(3) + TAB_SCROLL_BUTTON_WIDTH(3) = 6
        // → tab_area_right = 25 - 6 = 19 → right button x=19, tab_drop returns x-1=18.
        assert_eq!(view.scroll_right_hit_area.x.saturating_sub(1), 18);
        assert_eq!(tab_drop_indicator_x(&app, &app.workspaces[0], 5), Some(18));
    }
}
