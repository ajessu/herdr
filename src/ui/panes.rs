use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use super::scrollbar::{render_pane_scrollbar, should_show_scrollbar};
use super::widgets::panel_contrast_fg;
use crate::app::state::{FloatingPaneInfo, Palette};
use crate::app::{AppState, Mode};
use crate::layout::PaneInfo;
use crate::terminal::{TerminalRuntime, TerminalRuntimeRegistry};

pub(crate) fn pane_is_scrolled_back(rt: &TerminalRuntime) -> bool {
    rt.scroll_metrics()
        .is_some_and(|metrics| metrics.offset_from_bottom > 0)
}

fn truncate_label(text: &str, max_width: usize) -> String {
    let len = text.chars().count();
    if len <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }
    let prefix: String = text.chars().take(max_width.saturating_sub(1)).collect();
    format!("{prefix}…")
}

fn pane_border_title(label: &str, pane_width: u16) -> Option<String> {
    let label = label.trim();
    if label.is_empty() || pane_width <= 4 {
        return None;
    }
    let max_label_width = pane_width.saturating_sub(4) as usize;
    Some(format!(" {} ", truncate_label(label, max_label_width)))
}

fn stable_terminal_inner_rect(pane_inner: Rect) -> Rect {
    if pane_inner.width <= 4 {
        return pane_inner;
    }

    Rect::new(
        pane_inner.x,
        pane_inner.y,
        pane_inner.width.saturating_sub(1),
        pane_inner.height,
    )
}

fn pane_inner_rect(area: Rect, framed: bool) -> Rect {
    if framed {
        Block::default().borders(Borders::ALL).inner(area)
    } else {
        area
    }
}

/// Stack-aware inner rect used by both PTY-resize loops.
///
/// A collapsed stack member has a height-1 outer rect; running it through
/// `Block::inner` would subtract the top *and* bottom border rows and saturate
/// to height 0, starving the runtime. Instead we bypass the border inset and
/// hand the runtime the full 1-row rect, which it clamps to its 2-row minimum
/// (`PaneRuntime::resize`, R13). Expanded members and non-stacked panes keep the
/// normal bordered-inner path; single-pane mode uses the full area.
fn pane_inner_for(info: &PaneInfo, area: Rect, multi_pane: bool) -> Rect {
    if info.stack.as_ref().is_some_and(|member| member.collapsed) {
        return info.rect;
    }
    if multi_pane {
        Block::default().borders(Borders::ALL).inner(info.rect)
    } else {
        area
    }
}

fn runtime_for_tab_pane<'a>(
    terminal_runtimes: &'a TerminalRuntimeRegistry,
    tab: &'a crate::workspace::Tab,
    pane_id: crate::layout::PaneId,
) -> Option<(&'a crate::terminal::TerminalId, &'a TerminalRuntime)> {
    let terminal_id = tab.terminal_id(pane_id)?;
    #[cfg(test)]
    if let Some(runtime) = tab.runtimes.get(&pane_id) {
        return Some((terminal_id, runtime));
    }
    terminal_runtimes
        .get(terminal_id)
        .map(|runtime| (terminal_id, runtime))
}

fn stable_scrollbar_gutter(rt: &TerminalRuntime, pane_inner: Rect) -> (Rect, Option<Rect>) {
    let inner_rect = stable_terminal_inner_rect(pane_inner);
    if inner_rect == pane_inner {
        return (inner_rect, None);
    }
    let gutter = Rect::new(
        pane_inner.x + pane_inner.width.saturating_sub(1),
        pane_inner.y,
        1,
        pane_inner.height,
    );
    let scrollbar_rect = rt
        .scroll_metrics()
        .filter(|metrics| should_show_scrollbar(*metrics))
        .map(|_| gutter);

    (inner_rect, scrollbar_rect)
}

/// Resize every visible runtime in a tab to the geometry it would receive if the tab were selected.
pub(super) fn resize_tab_panes(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    tab: &crate::workspace::Tab,
    area: Rect,
    cell_size: crate::kitty_graphics::HostCellSize,
) {
    let multi_pane = tab.layout.pane_count() > 1;

    if tab.zoomed {
        let focused_id = tab.layout.focused();
        if let Some((terminal_id, rt)) = runtime_for_tab_pane(terminal_runtimes, tab, focused_id) {
            let pane_inner = pane_inner_rect(area, multi_pane);
            let inner_rect = stable_terminal_inner_rect(pane_inner);
            if !app.direct_attach_resize_locks.contains(terminal_id) {
                rt.resize(
                    inner_rect.height,
                    inner_rect.width,
                    cell_size.width_px,
                    cell_size.height_px,
                );
            }
        }
        return;
    }

    for info in tab.layout.panes(area) {
        let pane_inner = pane_inner_for(&info, area, multi_pane);

        if let Some((terminal_id, rt)) = runtime_for_tab_pane(terminal_runtimes, tab, info.id) {
            let inner_rect = stable_terminal_inner_rect(pane_inner);
            if !app.direct_attach_resize_locks.contains(terminal_id) {
                rt.resize(
                    inner_rect.height,
                    inner_rect.width,
                    cell_size.width_px,
                    cell_size.height_px,
                );
            }
        }
    }
}

/// Compute pane layout info and optionally resize pane runtimes to match.
pub(super) fn compute_pane_infos(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    area: Rect,
    resize_panes: bool,
    cell_size: crate::kitty_graphics::HostCellSize,
) -> Vec<PaneInfo> {
    let Some(ws_idx) = app.active else {
        return Vec::new();
    };
    let Some(ws) = app.workspaces.get(ws_idx) else {
        return Vec::new();
    };

    let multi_pane = ws.layout.pane_count() > 1;

    if ws.zoomed {
        let focused_id = ws.layout.focused();
        let pane_inner = pane_inner_rect(area, multi_pane);
        let mut inner_rect = pane_inner;
        let mut scrollbar_rect = None;
        if let Some(rt) = app.runtime_for_pane_in_workspace(terminal_runtimes, ws_idx, focused_id) {
            (inner_rect, scrollbar_rect) = stable_scrollbar_gutter(rt, pane_inner);
            if resize_panes
                && ws.terminal_id(focused_id).is_some_and(|terminal_id| {
                    !app.direct_attach_resize_locks.contains(terminal_id)
                })
            {
                rt.resize(
                    inner_rect.height,
                    inner_rect.width,
                    cell_size.width_px,
                    cell_size.height_px,
                );
            }
        }
        return vec![PaneInfo {
            id: focused_id,
            rect: area,
            inner_rect,
            scrollbar_rect,
            is_focused: true,
            stack: None,
        }];
    }

    let mut pane_infos = ws.layout.panes(area);

    for info in &mut pane_infos {
        // `Block::inner` subtracts a symmetric 1-row/1-col inset regardless of
        // which border set draws it, so the thick-vs-plain choice does not change
        // the inner rect. `pane_inner_for` covers the collapsed-bypass and
        // single-pane cases; this loop just consumes its result.
        let pane_inner = pane_inner_for(info, area, multi_pane);

        let mut inner_rect = pane_inner;
        let mut scrollbar_rect = None;
        if let Some(rt) = app.runtime_for_pane_in_workspace(terminal_runtimes, ws_idx, info.id) {
            (inner_rect, scrollbar_rect) = stable_scrollbar_gutter(rt, pane_inner);
            if resize_panes
                && ws.terminal_id(info.id).is_some_and(|terminal_id| {
                    !app.direct_attach_resize_locks.contains(terminal_id)
                })
            {
                rt.resize(
                    inner_rect.height,
                    inner_rect.width,
                    cell_size.width_px,
                    cell_size.height_px,
                );
            }
        }

        info.inner_rect = inner_rect;
        info.scrollbar_rect = scrollbar_rect;
    }

    pane_infos
}

pub(super) fn render_panes(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    frame: &mut Frame,
    area: Rect,
) {
    let Some(ws_idx) = app.active else {
        render_empty(app, frame, area);
        return;
    };
    let Some(ws) = app.workspaces.get(ws_idx) else {
        render_empty(app, frame, area);
        return;
    };

    let multi_pane = ws.layout.pane_count() > 1;
    let terminal_active = app.mode == Mode::Terminal;

    for info in &app.view.pane_infos {
        // A collapsed stack member draws as a single title row, not a bordered
        // pane with terminal content (R2). The expanded member and non-stacked
        // panes fall through to the normal path below.
        if info.stack.as_ref().is_some_and(|member| member.collapsed) {
            render_collapsed_stack_member(app, ws, frame, info);
            continue;
        }

        if let Some(rt) = app.runtime_for_pane_in_workspace(terminal_runtimes, ws_idx, info.id) {
            if multi_pane {
                let (border_style, border_set) = if info.is_focused && terminal_active {
                    (
                        Style::default().fg(app.palette.accent),
                        ratatui::symbols::border::THICK,
                    )
                } else if info.is_focused {
                    (
                        Style::default().fg(app.palette.accent),
                        ratatui::symbols::border::PLAIN,
                    )
                } else {
                    (
                        Style::default().fg(app.palette.overlay0),
                        ratatui::symbols::border::PLAIN,
                    )
                };

                let mut block = Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .border_set(border_set);
                if let Some(title) = ws
                    .pane_state(info.id)
                    .and_then(|pane| app.terminals.get(&pane.attached_terminal_id))
                    .and_then(|terminal| {
                        terminal.border_label(app.show_agent_labels_on_pane_borders)
                    })
                    .and_then(|label| pane_border_title(&label, info.rect.width))
                {
                    block = block.title(Line::from(Span::styled(title, border_style)));
                }
                frame.render_widget(block, info.rect);
            }

            let show_cursor = info.is_focused
                && terminal_active
                && !pane_is_scrolled_back(rt)
                && app.pane_exposes_host_cursor(ws_idx, info.id);
            rt.render(frame, info.inner_rect, show_cursor);
            render_pane_scrollbar(app, frame, info, rt);

            let should_dim = !info.is_focused && multi_pane && !terminal_active;
            if should_dim {
                let inner = info.inner_rect;
                let buf = frame.buffer_mut();
                for y in inner.y..inner.y + inner.height {
                    for x in inner.x..inner.x + inner.width {
                        let cell = &mut buf[(x, y)];
                        cell.set_style(cell.style().add_modifier(Modifier::DIM));
                    }
                }
            }

            render_selection_highlight(
                &app.selection,
                frame,
                info.id,
                info.inner_rect,
                rt.scroll_metrics(),
                &app.palette,
                app.host_terminal_theme,
            );
            render_copy_mode_cursor(app, frame, info);
        }
    }
}

/// Draw a collapsed stack member as a single title row (R2): a leading frame
/// glyph, the agent status dot, and the pane title. No border block, no terminal
/// content, no scrollbar. Reuses the existing title/status-dot language.
fn render_collapsed_stack_member(
    app: &AppState,
    ws: &crate::workspace::Workspace,
    frame: &mut Frame,
    info: &PaneInfo,
) {
    if info.rect.width == 0 || info.rect.height == 0 {
        return;
    }

    let member = match &info.stack {
        Some(member) => member,
        None => return,
    };

    let border_color = if info.is_focused {
        app.palette.accent
    } else {
        app.palette.overlay0
    };
    let text_style = Style::default().fg(panel_contrast_fg(&app.palette));

    // A vertical-rule lead-in marks the row as part of the stack frame. The
    // `[position/count]` hint is read from the `StackMember` marker, so render
    // never re-reads the layout tree (render stays pure).
    let lead_glyph = "│";

    let terminal = ws
        .pane_state(info.id)
        .and_then(|pane| app.terminals.get(&pane.attached_terminal_id));

    let (dot_glyph, dot_style) = terminal
        .map(|terminal| {
            let seen = ws.pane_state(info.id).map(|pane| pane.seen).unwrap_or(true);
            super::status::agent_icon(terminal.state, seen, app.spinner_tick, &app.palette)
        })
        .unwrap_or(("·", text_style));

    let label = terminal
        .and_then(|terminal| terminal.border_label(app.show_agent_labels_on_pane_borders))
        .unwrap_or_else(|| format!("pane {}", info.id.raw()));

    let position_hint = format!("{}/{}", member.position + 1, member.count);
    // Reserve room for: lead glyph + space + dot + space + " [n/m]".
    let reserved = 4 + position_hint.len() + 3;
    let label_width = (info.rect.width as usize).saturating_sub(reserved);
    let label = truncate_label(label.trim(), label_width);

    let line = Line::from(vec![
        Span::styled(lead_glyph, Style::default().fg(border_color)),
        Span::raw(" "),
        Span::styled(dot_glyph, dot_style),
        Span::raw(" "),
        Span::styled(label, text_style),
        Span::raw(" "),
        Span::styled(
            format!("[{position_hint}]"),
            Style::default().fg(app.palette.overlay0),
        ),
    ]);

    frame.render_widget(Paragraph::new(line), info.rect);
}

fn render_copy_mode_cursor(app: &AppState, frame: &mut Frame, info: &PaneInfo) {
    if app.mode != Mode::Copy {
        return;
    }
    let Some(copy_mode) = app.copy_mode else {
        return;
    };
    if copy_mode.pane_id != info.id
        || copy_mode.cursor_row >= info.inner_rect.height
        || copy_mode.cursor_col >= info.inner_rect.width
    {
        return;
    }

    let x = info.inner_rect.x + copy_mode.cursor_col;
    let y = info.inner_rect.y + copy_mode.cursor_row;
    let cell = &mut frame.buffer_mut()[(x, y)];
    cell.set_style(
        Style::default()
            .fg(panel_contrast_fg(&app.palette))
            .bg(app.palette.accent)
            .add_modifier(Modifier::BOLD),
    );
}

fn render_selection_highlight(
    selection: &Option<crate::selection::Selection>,
    frame: &mut Frame,
    pane_id: crate::layout::PaneId,
    inner: Rect,
    scroll_metrics: Option<crate::pane::ScrollMetrics>,
    p: &Palette,
    host_theme: crate::terminal_theme::TerminalTheme,
) {
    if let Some(sel) = selection {
        if sel.is_visible() && sel.pane_id == pane_id {
            let buf = frame.buffer_mut();
            let style = automatic_selection_style(p, host_theme);
            for y in 0..inner.height {
                for x in 0..inner.width {
                    if sel.contains(y, x, scroll_metrics) {
                        let cell = &mut buf[(inner.x + x, inner.y + y)];
                        cell.set_style(style);
                    }
                }
            }
        }
    }
}

type Rgb = (u8, u8, u8);

fn automatic_selection_style(
    p: &Palette,
    host_theme: crate::terminal_theme::TerminalTheme,
) -> Style {
    let bg = automatic_selection_bg(p, host_theme);
    Style::reset().fg(selection_fg_for_bg(bg, p)).bg(bg)
}

fn automatic_selection_bg(p: &Palette, host_theme: crate::terminal_theme::TerminalTheme) -> Color {
    let Some(background) = host_theme.background.map(terminal_theme_to_rgb) else {
        return selection_palette_background(p);
    };

    let target = if relative_luminance(background) < 0.5 {
        (255, 255, 255)
    } else {
        (0, 0, 0)
    };
    let selected = mix_rgb(background, target, 0.28);
    Color::Rgb(selected.0, selected.1, selected.2)
}

fn selection_palette_background(p: &Palette) -> Color {
    if p.panel_bg == Color::Reset {
        p.surface_dim
    } else {
        p.panel_bg
    }
}

fn terminal_theme_to_rgb(color: crate::terminal_theme::RgbColor) -> Rgb {
    (color.r, color.g, color.b)
}

fn selection_fg_for_bg(bg: Color, p: &Palette) -> Color {
    color_to_rgb(bg)
        .map(|bg| {
            if relative_luminance(bg) < 0.5 {
                Color::White
            } else {
                Color::Black
            }
        })
        .unwrap_or_else(|| panel_contrast_fg(p))
}

fn mix_rgb(base: Rgb, target: Rgb, amount: f32) -> Rgb {
    fn channel(base: u8, target: u8, amount: f32) -> u8 {
        (f32::from(base) + (f32::from(target) - f32::from(base)) * amount).round() as u8
    }
    (
        channel(base.0, target.0, amount),
        channel(base.1, target.1, amount),
        channel(base.2, target.2, amount),
    )
}

fn relative_luminance(color: Rgb) -> f32 {
    fn channel(value: u8) -> f32 {
        let value = f32::from(value) / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(color.0) + 0.7152 * channel(color.1) + 0.0722 * channel(color.2)
}

fn color_to_rgb(color: Color) -> Option<Rgb> {
    match color {
        Color::Reset => None,
        Color::Black => Some((0, 0, 0)),
        Color::Red => Some((128, 0, 0)),
        Color::Green => Some((0, 128, 0)),
        Color::Yellow => Some((128, 128, 0)),
        Color::Blue => Some((0, 0, 128)),
        Color::Magenta => Some((128, 0, 128)),
        Color::Cyan => Some((0, 128, 128)),
        Color::Gray => Some((192, 192, 192)),
        Color::DarkGray => Some((128, 128, 128)),
        Color::LightRed => Some((255, 0, 0)),
        Color::LightGreen => Some((0, 255, 0)),
        Color::LightYellow => Some((255, 255, 0)),
        Color::LightBlue => Some((0, 0, 255)),
        Color::LightMagenta => Some((255, 0, 255)),
        Color::LightCyan => Some((0, 255, 255)),
        Color::White => Some((255, 255, 255)),
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Indexed(_) => None,
    }
}

pub(super) fn compute_floating_pane_infos(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    resize_panes: bool,
    cell_size: crate::kitty_graphics::HostCellSize,
) -> Vec<FloatingPaneInfo> {
    let Some(ws_idx) = app.active else {
        return Vec::new();
    };
    let Some(ws) = app.workspaces.get(ws_idx) else {
        return Vec::new();
    };

    if !ws.floating.is_visible() {
        return Vec::new();
    }

    let focused_id = ws.floating.focused_pane_id();

    ws.floating
        .iter()
        .enumerate()
        .map(|(z_idx, fp)| {
            let rect = fp.geom.rect();
            let inner_rect = fp.geom.inner_rect();

            if resize_panes {
                if let Some(rt) =
                    app.runtime_for_pane_in_workspace(terminal_runtimes, ws_idx, fp.pane_id)
                {
                    if ws
                        .terminal_id(fp.pane_id)
                        .is_some_and(|tid| !app.direct_attach_resize_locks.contains(tid))
                    {
                        rt.resize(
                            inner_rect.height,
                            inner_rect.width,
                            cell_size.width_px,
                            cell_size.height_px,
                        );
                    }
                }
            }

            FloatingPaneInfo {
                pane_id: fp.pane_id,
                rect,
                inner_rect,
                is_focused: focused_id == Some(fp.pane_id),
                z_index: z_idx,
            }
        })
        .collect()
}

pub(super) fn render_floating_panes(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    frame: &mut Frame,
) {
    let Some(ws_idx) = app.active else {
        return;
    };
    let Some(ws) = app.workspaces.get(ws_idx) else {
        return;
    };

    if !ws.floating.is_visible() {
        return;
    }

    let terminal_active = app.mode == Mode::Terminal;

    for info in &app.view.floating_pane_infos {
        frame.render_widget(Clear, info.rect);

        let (border_style, border_set) = if info.is_focused && terminal_active {
            (
                Style::default().fg(app.palette.accent),
                ratatui::symbols::border::THICK,
            )
        } else if info.is_focused {
            (
                Style::default().fg(app.palette.accent),
                ratatui::symbols::border::PLAIN,
            )
        } else {
            (
                Style::default().fg(app.palette.overlay0),
                ratatui::symbols::border::PLAIN,
            )
        };

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .border_set(border_set);

        if let Some(title) = ws
            .pane_state(info.pane_id)
            .and_then(|pane| app.terminals.get(&pane.attached_terminal_id))
            .and_then(|terminal| terminal.border_label(app.show_agent_labels_on_pane_borders))
            .and_then(|label| pane_border_title(&label, info.rect.width))
        {
            block = block.title(Line::from(Span::styled(title, border_style)));
        }

        frame.render_widget(block, info.rect);

        if let Some((_, rt)) =
            runtime_for_tab_pane(terminal_runtimes, ws.active_tab().unwrap(), info.pane_id)
        {
            let show_cursor = info.is_focused
                && terminal_active
                && !pane_is_scrolled_back(rt)
                && app.pane_exposes_host_cursor(ws_idx, info.pane_id);
            rt.render(frame, info.inner_rect, show_cursor);
        }
    }
}

fn render_empty(app: &AppState, frame: &mut Frame, area: Rect) {
    let p = &app.palette;
    let lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "  No workspaces yet",
            Style::default().fg(p.overlay0),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  A workspace is one project context.",
            Style::default().fg(p.overlay1),
        )),
        Line::from(Span::styled(
            "  Its root pane (top-left) sets the default repo or folder name.",
            Style::default().fg(p.overlay1),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Press ", Style::default().fg(p.overlay0)),
            Span::styled(
                app.keybinds
                    .new_workspace
                    .label()
                    .unwrap_or_else(|| "unset".to_string()),
                Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to create one", Style::default().fg(p.overlay0)),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(p.surface_dim)),
        ),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::PaneId;
    use crate::selection::Selection;
    use crate::terminal::TerminalRuntime;
    use crate::workspace::Workspace;

    #[test]
    fn pane_border_title_trims_and_truncates() {
        assert_eq!(
            pane_border_title(" claude ", 20).as_deref(),
            Some(" claude ")
        );
        assert_eq!(pane_border_title("", 20), None);
        assert_eq!(pane_border_title("abcdef", 8).as_deref(), Some(" abc… "));
        assert_eq!(pane_border_title("abcdef", 4), None);
    }

    #[tokio::test]
    async fn pane_scrollbar_gutter_is_reserved_before_scrollback_exists() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(40, 8, 1024, b"ready\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(10, 3, 40, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let info = &infos[0];

        assert_eq!(info.rect, area);
        assert_eq!(info.scrollbar_rect, None);
        assert_eq!(info.inner_rect, Rect::new(10, 3, 39, 8));
    }

    #[tokio::test]
    async fn zoomed_pane_scrollbar_gutter_is_reserved_before_scrollback_exists() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        workspace.zoomed = true;
        let root_pane = workspace.tabs[0].root_pane;
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(40, 8, 1024, b"ready\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(10, 3, 40, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let info = &infos[0];

        assert_eq!(info.rect, area);
        assert_eq!(info.scrollbar_rect, None);
        assert_eq!(info.inner_rect, Rect::new(10, 3, 39, 8));
    }

    #[tokio::test]
    async fn zoomed_multi_pane_keeps_border_space() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let focused_pane = workspace.test_split(ratatui::layout::Direction::Horizontal);
        workspace.zoomed = true;
        workspace.tabs[0].runtimes.insert(
            focused_pane,
            TerminalRuntime::test_with_scrollback_bytes(40, 8, 1024, b"ready\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(10, 3, 40, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let info = &infos[0];

        assert_eq!(info.id, focused_pane);
        assert_eq!(info.rect, area);
        assert_eq!(info.scrollbar_rect, None);
        assert_eq!(info.inner_rect, Rect::new(11, 4, 37, 6));
    }

    #[tokio::test]
    async fn tiny_pane_does_not_reserve_scrollbar_gutter() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(4, 8, 1024, b"ready\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(10, 3, 4, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let info = &infos[0];

        assert_eq!(info.rect, area);
        assert_eq!(info.scrollbar_rect, None);
        assert_eq!(info.inner_rect, area);
    }

    #[tokio::test]
    async fn pane_scrollbar_reserves_last_column_from_terminal_area() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(
                40,
                8,
                1024,
                b"one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
            ),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(10, 3, 40, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let info = &infos[0];

        assert_eq!(info.rect, area);
        assert_eq!(info.scrollbar_rect, Some(Rect::new(49, 3, 1, 8)));
        assert_eq!(info.inner_rect, Rect::new(10, 3, 39, 8));
    }

    #[test]
    fn selection_highlight_uses_one_uniform_style() {
        let palette = Palette::catppuccin();
        let host_theme = crate::terminal_theme::TerminalTheme {
            foreground: None,
            background: Some(crate::terminal_theme::RgbColor {
                r: 12,
                g: 14,
                b: 16,
            }),
        };
        let expected_style = automatic_selection_style(&palette, host_theme);
        let selection = Some(Selection::range(PaneId::from_raw(1), 0, 0, 2, None));
        let backend = ratatui::backend::TestBackend::new(4, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                let buf = frame.buffer_mut();
                buf[(0, 0)].set_style(
                    Style::default()
                        .fg(Color::Rgb(10, 220, 120))
                        .bg(Color::Black),
                );
                buf[(1, 0)].set_style(
                    Style::default()
                        .fg(Color::Rgb(220, 180, 40))
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                );
                buf[(2, 0)].set_style(Style::default().fg(Color::Blue).bg(Color::Reset));
                render_selection_highlight(
                    &selection,
                    frame,
                    PaneId::from_raw(1),
                    Rect::new(0, 0, 4, 1),
                    None,
                    &palette,
                    host_theme,
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let first = buffer[(0, 0)].style();
        let second = buffer[(1, 0)].style();
        let third = buffer[(2, 0)].style();

        assert_eq!(first.fg, expected_style.fg);
        assert_eq!(second.fg, expected_style.fg);
        assert_eq!(third.fg, expected_style.fg);
        assert_eq!(first.bg, expected_style.bg);
        assert_eq!(second.bg, expected_style.bg);
        assert_eq!(third.bg, expected_style.bg);
        assert_eq!(first.add_modifier, expected_style.add_modifier);
        assert_eq!(second.add_modifier, expected_style.add_modifier);
        assert_eq!(third.add_modifier, expected_style.add_modifier);
        assert!(!second.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn automatic_selection_background_uses_host_background() {
        let bg = automatic_selection_bg(
            &Palette::terminal(),
            crate::terminal_theme::TerminalTheme {
                foreground: Some(crate::terminal_theme::RgbColor {
                    r: 230,
                    g: 230,
                    b: 230,
                }),
                background: Some(crate::terminal_theme::RgbColor {
                    r: 12,
                    g: 14,
                    b: 16,
                }),
            },
        );

        let Color::Rgb(r, g, b) = bg else {
            panic!("selection background should resolve to rgb");
        };
        assert!(relative_luminance((r, g, b)) > relative_luminance((12, 14, 16)));
    }

    fn app_with_stack(count: usize, expanded: usize) -> (AppState, Vec<PaneId>) {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let members = workspace.test_set_stack(count, expanded);
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.ensure_test_terminals();
        for id in &members {
            app.workspaces[0].insert_test_runtime(
                *id,
                TerminalRuntime::test_with_scrollback_bytes(80, 24, 1024, b"hi\n"),
            );
        }
        (app, members)
    }

    #[tokio::test]
    async fn stacked_panes_geometry_collapsed_rows_expanded_remainder() {
        // 4-member stack, expanded at index 1, area 80x24 → 3 collapsed rows of
        // height 1, expanded member fills 24 - 3 = 21 rows (R2/R3).
        let (app, members) = app_with_stack(4, 1);
        let area = Rect::new(0, 0, 80, 24);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );

        assert_eq!(infos.len(), 4);
        assert_eq!(infos[0].id, members[0]);
        assert_eq!(infos[0].rect.height, 1);
        assert_eq!(infos[1].id, members[1]);
        assert_eq!(infos[1].rect.height, 21);
        assert_eq!(infos[2].rect.height, 1);
        assert_eq!(infos[3].rect.height, 1);

        // Collapsed members bypass the border inset: their inner rect keeps the
        // full 1-row height instead of saturating to 0 (R13).
        assert_eq!(infos[0].inner_rect.height, 1);
        assert_eq!(infos[2].inner_rect.height, 1);
        // Expanded member uses the normal bordered inner path.
        assert_eq!(infos[1].inner_rect.height, 19);
    }

    #[tokio::test]
    async fn stacked_panes_render_collapsed_members_as_single_title_rows() {
        let (mut app, _members) = app_with_stack(4, 1);
        app.mode = Mode::Terminal;
        let area = Rect::new(0, 0, 80, 24);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        app.view.pane_infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );

        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_panes(&app, &terminal_runtimes, frame, area))
            .unwrap();
        let buffer = terminal.backend().buffer();

        // Row 0 is collapsed member 0: a single title row beginning with the
        // stack lead glyph, not a box-drawing corner of a full bordered block.
        let row0_first = buffer[(0, 0)].symbol();
        assert_eq!(row0_first, "│");
        // The expanded member (rows 1..22) draws a full border: its top-left is
        // a corner glyph, distinct from the collapsed lead glyph.
        let expanded_top_left = buffer[(0, 1)].symbol();
        assert!(
            expanded_top_left == "┌" || expanded_top_left == "┏",
            "expected a border corner for the expanded member, got {expanded_top_left:?}"
        );
    }

    #[tokio::test]
    async fn stacked_panes_zoom_shows_single_pane_unzoom_restores_stack() {
        let (mut app, members) = app_with_stack(3, 1);
        let area = Rect::new(0, 0, 80, 24);
        let terminal_runtimes = TerminalRuntimeRegistry::new();

        // Zoomed: a single full-area PaneInfo for the expanded/focused member.
        app.workspaces[0].zoomed = true;
        let zoomed = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        assert_eq!(zoomed.len(), 1);
        assert_eq!(zoomed[0].id, members[1]);
        assert_eq!(zoomed[0].rect, area);

        // Un-zoomed: full stack geometry restored.
        app.workspaces[0].zoomed = false;
        let restored = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        assert_eq!(restored.len(), 3);
        assert_eq!(restored[0].rect.height, 1);
        assert_eq!(restored[1].rect.height, 22);
        assert_eq!(restored[2].rect.height, 1);
    }

    #[test]
    fn compute_floating_pane_infos_produces_entries_when_visible() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let float_id = workspace.test_add_floating_pane();
        workspace.floating.show();
        workspace.floating.focus_pane(float_id);
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_floating_pane_infos(
            &app,
            &terminal_runtimes,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );

        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].pane_id, float_id);
        assert!(infos[0].is_focused);
        assert_eq!(infos[0].z_index, 0);
    }

    #[test]
    fn compute_floating_pane_infos_empty_when_hidden() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        workspace.test_add_floating_pane();
        // Layer remains hidden (not shown)
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_floating_pane_infos(
            &app,
            &terminal_runtimes,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );

        assert!(infos.is_empty());
    }

    #[tokio::test]
    async fn floating_panes_render_above_tiled_in_frame_buffer() {
        use crate::workspace::floating::FloatingGeom;

        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(20, 10, 256, b"TILED\n"),
        );
        // Manually add a floating pane with explicit geometry that fits in 20x10
        let float_id = PaneId::alloc();
        let geom = FloatingGeom {
            x: 3,
            y: 2,
            width: 12,
            height: 6,
        };
        let tab = &mut workspace.tabs[0];
        tab.floating.add_pane(float_id, geom);
        tab.panes.insert(
            float_id,
            crate::pane::PaneState::new(crate::terminal::TerminalId::alloc()),
        );
        tab.runtimes.insert(
            float_id,
            TerminalRuntime::test_with_scrollback_bytes(10, 4, 256, b"FLOAT\n"),
        );
        workspace.floating.show();
        workspace.floating.focus_pane(float_id);
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.mode = Mode::Terminal;

        let area = Rect::new(0, 0, 20, 10);
        let terminal_runtimes = TerminalRuntimeRegistry::new();

        let pane_infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let floating_infos = compute_floating_pane_infos(
            &app,
            &terminal_runtimes,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        app.view.pane_infos = pane_infos;
        app.view.floating_pane_infos = floating_infos;
        app.view.terminal_area = area;

        let backend = ratatui::backend::TestBackend::new(20, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_panes(&app, &terminal_runtimes, frame, area);
                render_floating_panes(&app, &terminal_runtimes, frame);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let fp_rect = app.view.floating_pane_infos[0].rect;

        // Top-left corner of floating pane should be a thick border char (focused)
        let top_left = &buf[(fp_rect.x, fp_rect.y)];
        assert_eq!(
            top_left.symbol(),
            "┏",
            "focused floating pane should have thick border"
        );

        // Cell inside floating pane's inner area should have floating terminal content,
        // not tiled content
        let inner = app.view.floating_pane_infos[0].inner_rect;
        let inner_cell = &buf[(inner.x, inner.y)];
        assert_eq!(
            inner_cell.symbol(),
            "F",
            "floating terminal content should render"
        );
    }

    #[tokio::test]
    async fn floating_pane_focused_gets_thick_border() {
        use crate::workspace::floating::FloatingGeom;

        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let float_id = PaneId::alloc();
        let geom = FloatingGeom {
            x: 2,
            y: 1,
            width: 10,
            height: 5,
        };
        let tab = &mut workspace.tabs[0];
        tab.floating.add_pane(float_id, geom);
        tab.panes.insert(
            float_id,
            crate::pane::PaneState::new(crate::terminal::TerminalId::alloc()),
        );
        tab.runtimes.insert(
            float_id,
            TerminalRuntime::test_with_scrollback_bytes(8, 3, 256, b"hi\n"),
        );
        workspace.floating.show();
        workspace.floating.focus_pane(float_id);
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.mode = Mode::Terminal;

        let terminal_runtimes = TerminalRuntimeRegistry::new();

        let floating_infos = compute_floating_pane_infos(
            &app,
            &terminal_runtimes,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        app.view.floating_pane_infos = floating_infos;

        let backend = ratatui::backend::TestBackend::new(20, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_floating_panes(&app, &terminal_runtimes, frame);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let fp_rect = app.view.floating_pane_infos[0].rect;
        let top_left = &buf[(fp_rect.x, fp_rect.y)];
        assert_eq!(top_left.symbol(), "┏");
    }

    #[tokio::test]
    async fn floating_pane_unfocused_gets_plain_border() {
        use crate::workspace::floating::FloatingGeom;

        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let float_id = PaneId::alloc();
        let geom = FloatingGeom {
            x: 2,
            y: 1,
            width: 10,
            height: 5,
        };
        let tab = &mut workspace.tabs[0];
        tab.floating.add_pane(float_id, geom);
        tab.panes.insert(
            float_id,
            crate::pane::PaneState::new(crate::terminal::TerminalId::alloc()),
        );
        tab.runtimes.insert(
            float_id,
            TerminalRuntime::test_with_scrollback_bytes(8, 3, 256, b"hi\n"),
        );
        workspace.floating.show();
        workspace.floating.unfocus();
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.mode = Mode::Terminal;

        let terminal_runtimes = TerminalRuntimeRegistry::new();

        let floating_infos = compute_floating_pane_infos(
            &app,
            &terminal_runtimes,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        app.view.floating_pane_infos = floating_infos;

        let backend = ratatui::backend::TestBackend::new(20, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_floating_panes(&app, &terminal_runtimes, frame);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let fp_rect = app.view.floating_pane_infos[0].rect;
        let top_left = &buf[(fp_rect.x, fp_rect.y)];
        assert_eq!(top_left.symbol(), "┌");
    }
}
