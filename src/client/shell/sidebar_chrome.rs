//! Fork: zellij-style sidebar (`ui.sidebar.style = "zellij"`).
//!
//! - A 7-column collapsed rail: `[marker] [number] [status]` rows, an accent
//!   `●` marker on items that need attention, and lists windowed around the
//!   selected workspace and focused agent. Hidden items collapse into a
//!   reserved `+N ◉² ●¹` badge row.
//! - The same badges on the expanded spaces and agents lists, drawn over the
//!   first or last row while items are scrolled out of view.
//! - Clicking a badge focuses the most urgent hidden item (blocked, then
//!   working, then finished-unseen, then the nearest).
//! - Agent labels that stay bright while an agent is pending and dim once it
//!   has settled, heavier list scrollbars, and an underlined drop row.
//!
//! "Needs attention" is Blocked, Working or Done. The client's projected
//! `Done` already means "finished and not yet seen by this client", and
//! workspace statuses are already rolled up from their agents.
//!
//! Ported from the fork's pre-v0.9.0 `src/ui/sidebar.rs` and
//! `src/app/input/sidebar.rs`. Upstream's sidebar renderers call in through
//! one early return (the rail) and a few gated hooks (badges, styles).

use ratatui::layout::Alignment;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::overflow::{self, ListWindow, OverflowSide};
use super::render::{put_text, sidebar::collapsed_sidebar_sections};
use super::*;
use crate::api::schema::AgentStatus;
use crate::config::SidebarStyleConfig;

/// Collapsed rail width under the zellij style: 6 content columns plus the
/// separator (upstream's rail is 4).
const RAIL_WIDTH: u16 = 7;
const UPSTREAM_RAIL_WIDTH: u16 = 4;
const ATTENTION_MARKER: &str = "●";

/// Where a click on an overflow badge goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SidebarJump {
    Workspace(String),
    /// `index` is the agent's position in the agents list, so the expanded
    /// panel can scroll to it.
    Pane {
        pane_id: String,
        index: usize,
    },
}

pub(super) fn zellij(config: &ClientShellConfig) -> bool {
    config.sidebar_style == SidebarStyleConfig::Zellij
}

/// Width of the `compact` collapsed sidebar.
pub(super) fn compact_rail_width(style: SidebarStyleConfig) -> u16 {
    match style {
        SidebarStyleConfig::Upstream => UPSTREAM_RAIL_WIDTH,
        SidebarStyleConfig::Zellij => RAIL_WIDTH,
    }
}

pub(super) fn needs_attention(status: AgentStatus) -> bool {
    matches!(
        status,
        AgentStatus::Blocked | AgentStatus::Working | AgentStatus::Done
    )
}

/// Bright while the agent is pending (working, blocked, finished-unseen), dim
/// once it has settled. Also usable as a patch over a bold base style.
pub(super) fn pending_style(status: AgentStatus, palette: &Palette) -> Style {
    if needs_attention(status) {
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(palette.overlay0)
            .remove_modifier(Modifier::BOLD)
    }
}

/// The mobile agent item's style patch: the pending style under the zellij
/// sidebar style (except for stale machines), else nothing.
pub(super) fn mobile_agent_patch(
    status: AgentStatus,
    stale: bool,
    config: &ClientShellConfig,
) -> Style {
    if zellij(config) && !stale {
        pending_style(status, &config.palette)
    } else {
        Style::default()
    }
}

/// `(track, thumb)` glyphs for the sidebar list scrollbars.
pub(super) fn scrollbar_glyphs(config: &ClientShellConfig) -> (&'static str, &'static str) {
    if zellij(config) {
        ("┃", "█")
    } else {
        ("▕", "▕")
    }
}

/// The workspace drag marker: underline the existing row instead of writing a
/// `─` over it, which would hide a neighboring one-row card.
pub(super) fn render_drop_row(buffer: &mut Buffer, rect: Rect, palette: &Palette) {
    buffer.set_style(
        rect,
        Style::default()
            .fg(palette.accent)
            .add_modifier(Modifier::UNDERLINED),
    );
}

fn badge_line(side: OverflowSide, palette: &Palette) -> Line<'static> {
    Line::from(overflow::badge_spans(side, palette))
}

fn list_window(start: usize, drawn: usize, total: usize) -> ListWindow {
    ListWindow {
        first: start,
        count: drawn,
        hidden_above: start,
        hidden_below: total.saturating_sub(start + drawn),
    }
}

fn push_jump(
    hits: &mut ShellHitMap,
    rect: Rect,
    side: OverflowSide,
    target: impl Fn(usize) -> Option<SidebarJump>,
) {
    if let Some(jump) = overflow::resolve_jump(side).and_then(target) {
        hits.sidebar_overflow.push((rect, jump));
    }
}

/// Paint `[marker] [number] [status]` into a rail row whose background is
/// already set.
fn paint_rail_row(
    buffer: &mut Buffer,
    rect: Rect,
    number: usize,
    status: AgentStatus,
    number_style: Style,
    config: &ClientShellConfig,
) {
    let palette = &config.palette;
    if needs_attention(status) {
        put_text(
            buffer,
            rect.x,
            rect.y,
            rect.width.min(1),
            ATTENTION_MARKER,
            Style::default().fg(palette.accent),
        );
    }
    let number = number.to_string();
    let digits = number.len() as u16;
    put_text(
        buffer,
        rect.x.saturating_add(2),
        rect.y,
        rect.width.saturating_sub(2),
        &number,
        number_style,
    );
    let icon_offset = 3 + digits;
    put_text(
        buffer,
        rect.x.saturating_add(icon_offset),
        rect.y,
        rect.width.saturating_sub(icon_offset),
        status_icon(status, config.status_indicators),
        Style::default().fg(status_color(status, palette)),
    );
}

/// Badges for a rail section: the window reserves the section's first and/or
/// last row, and the whole row is the click target.
fn render_rail_badges(
    buffer: &mut Buffer,
    area: Rect,
    window: ListWindow,
    total: usize,
    status_of: impl Fn(usize) -> Option<AgentStatus>,
    target: impl Fn(usize) -> Option<SidebarJump>,
    palette: &Palette,
    hits: &mut ShellHitMap,
) {
    if area.is_empty() {
        return;
    }
    let above = overflow::side_above(window, &status_of);
    let below = overflow::side_below(window, total, &status_of);
    for (side, y) in [(above, area.y), (below, area.bottom().saturating_sub(1))] {
        if side.is_empty() {
            continue;
        }
        let rect = Rect::new(area.x, y, area.width, 1);
        Paragraph::new(badge_line(side, palette))
            .alignment(Alignment::Right)
            .render(rect, buffer);
        push_jump(hits, rect, side, &target);
    }
}

/// The collapsed rail. Replaces upstream's `render_collapsed_sidebar` body.
pub(super) fn render_rail(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    selected_workspace_id: Option<&str>,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    super::render::render_sidebar_background(buffer, area, palette);
    let (workspace_area, divider_y, detail_area) = collapsed_sidebar_sections(area);

    let workspaces = &snapshot.workspaces;
    let anchor = selected_workspace_id
        .and_then(|id| workspaces.iter().position(|ws| ws.workspace_id == id))
        .or_else(|| workspaces.iter().position(|ws| ws.focused))
        .unwrap_or(0);
    let window =
        overflow::anchored_window(workspaces.len(), usize::from(workspace_area.height), anchor);
    let top = u16::from(window.hidden_above > 0);
    for (offset, index) in (window.first..window.first + window.count).enumerate() {
        let workspace = &workspaces[index];
        let rect = Rect::new(
            workspace_area.x,
            workspace_area.y + top + offset as u16,
            workspace_area.width,
            1,
        );
        let selected = selected_workspace_id == Some(workspace.workspace_id.as_str());
        let background = if selected {
            Some(
                if workspace.focused && palette.selection_bg == ratatui::style::Color::Reset {
                    palette.active_row_bg
                } else {
                    palette.selection_bg
                },
            )
        } else if workspace.focused {
            Some(palette.active_row_bg)
        } else {
            None
        };
        if let Some(background) = background {
            buffer.set_style(rect, Style::default().bg(background));
        }
        let number_style = match background {
            Some(background) if selected => Style::default()
                .fg(palette.text)
                .bg(background)
                .add_modifier(Modifier::BOLD),
            Some(background) => Style::default().fg(palette.text).bg(background),
            None => Style::default().fg(palette.overlay0),
        };
        paint_rail_row(
            buffer,
            rect,
            index + 1,
            workspace.agent_status,
            number_style,
            config,
        );
        hits.workspaces.push(WorkspaceHit {
            rect,
            endpoint_id: ClientEndpointId::Local,
            workspace_id: workspace.workspace_id.clone(),
            indented: false,
            group_toggle: None,
        });
    }
    render_rail_badges(
        buffer,
        workspace_area,
        window,
        workspaces.len(),
        |index| workspaces.get(index).map(|ws| ws.agent_status),
        |index| {
            workspaces
                .get(index)
                .map(|ws| SidebarJump::Workspace(ws.workspace_id.clone()))
        },
        palette,
        hits,
    );

    if let Some(divider_y) = divider_y {
        put_text(
            buffer,
            workspace_area.x,
            divider_y,
            workspace_area.width,
            &"─".repeat(workspace_area.width as usize),
            Style::default().fg(if snapshot.agent_view_label.is_some() {
                palette.accent
            } else {
                palette.surface_dim
            }),
        );
    }

    let detail_content = Rect::new(
        detail_area.x,
        detail_area.y,
        detail_area.width,
        detail_area.height.saturating_sub(1),
    );
    let agents = super::agent_sidebar::ordered_agent_pane_ids(snapshot, config.agent_panel_sort)
        .into_iter()
        .filter_map(|pane_id| {
            snapshot
                .agents
                .iter()
                .find(|agent| agent.pane_id == pane_id)
        })
        .collect::<Vec<_>>();
    let anchor = agents.iter().position(|agent| agent.focused).unwrap_or(0);
    let window =
        overflow::anchored_window(agents.len(), usize::from(detail_content.height), anchor);
    let top = u16::from(window.hidden_above > 0);
    for (offset, index) in (window.first..window.first + window.count).enumerate() {
        let agent = agents[index];
        let rect = Rect::new(
            detail_content.x,
            detail_content.y + top + offset as u16,
            detail_content.width,
            1,
        );
        let attention = needs_attention(agent.agent_status);
        let background = if agent.focused {
            Some(palette.active_row_bg)
        } else if attention {
            Some(palette.surface_dim)
        } else {
            None
        };
        if let Some(background) = background {
            buffer.set_style(rect, Style::default().bg(background));
        }
        let number_style = match background {
            Some(background) => Style::default().fg(palette.text).bg(background),
            None => Style::default().fg(palette.overlay0),
        };
        paint_rail_row(
            buffer,
            rect,
            index + 1,
            agent.agent_status,
            number_style,
            config,
        );
        // Only rows that need attention are click targets on the rail.
        if attention {
            hits.agents.push((rect, agent.pane_id.clone()));
        }
    }
    render_rail_badges(
        buffer,
        detail_content,
        window,
        agents.len(),
        |index| agents.get(index).map(|agent| agent.agent_status),
        |index| {
            agents.get(index).map(|agent| SidebarJump::Pane {
                pane_id: agent.pane_id.clone(),
                index,
            })
        },
        palette,
        hits,
    );

    hits.sidebar_toggle = if area.is_empty() || workspace_area.width == 0 {
        Rect::default()
    } else {
        Rect::new(
            workspace_area.x + workspace_area.width / 2,
            area.bottom().saturating_sub(1),
            1,
            1,
        )
    };
    put_text(
        buffer,
        hits.sidebar_toggle.x,
        hits.sidebar_toggle.y,
        hits.sidebar_toggle.width,
        "»",
        if super::global_menu::global_menu_attention(snapshot) {
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.overlay0)
        },
    );
}

/// Badges over a scrolled expanded list. `body` is the list body, minus the
/// scrollbar column when one is drawn; the badge is right-aligned on its first
/// (hidden above) or last (hidden below) row, and only its own cells are a
/// click target, so the rest of that row still acts as the row underneath.
fn render_list_badges(
    buffer: &mut Buffer,
    body: Rect,
    window: ListWindow,
    total: usize,
    status_of: impl Fn(usize) -> Option<AgentStatus>,
    target: impl Fn(usize) -> Option<SidebarJump>,
    palette: &Palette,
    hits: &mut ShellHitMap,
) {
    if body.is_empty() {
        return;
    }
    let above = overflow::side_above(window, &status_of);
    let below = overflow::side_below(window, total, &status_of);
    for (side, y) in [(above, body.y), (below, body.bottom().saturating_sub(1))] {
        if side.is_empty() {
            continue;
        }
        let mut line = badge_line(side, palette);
        line.spans.insert(0, Span::raw(" "));
        let width = u16::try_from(line.width())
            .unwrap_or(u16::MAX)
            .min(body.width);
        let rect = Rect::new(body.right() - width, y, width, 1);
        Paragraph::new(line)
            .alignment(Alignment::Right)
            .render(rect, buffer);
        push_jump(hits, rect, side, &target);
    }
}

/// The list body without the scrollbar column, from the rects the upstream
/// renderer recorded.
fn content_body(body: Rect, scrollbar: Rect) -> Rect {
    if scrollbar.is_empty() {
        body
    } else {
        Rect::new(body.x, body.y, body.width.saturating_sub(1), body.height)
    }
}

/// Spaces-list badges. Called by upstream's `render_sidebar` after the list
/// and its scrollbar are drawn; `start` is the normalized scroll offset.
pub(super) fn workspace_list_badges(
    buffer: &mut Buffer,
    snapshot: &ClientShellSnapshot,
    entries: &[WorkspaceEntry],
    start: usize,
    collapsed_groups: &HashSet<String>,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    let body = content_body(hits.workspace_body, hits.workspace_scrollbar);
    let drawn = hits
        .workspaces
        .iter()
        .filter(|hit| super::contains(body, (hit.rect.x, hit.rect.y)))
        .count();
    let workspace = |index: usize| {
        entries
            .get(index)
            .and_then(|entry| snapshot.workspaces.get(entry.index))
    };
    render_list_badges(
        buffer,
        body,
        list_window(start, drawn, entries.len()),
        entries.len(),
        |index| {
            workspace(index).map(|ws| {
                super::render::sidebar::displayed_workspace_status(snapshot, ws, collapsed_groups)
            })
        },
        |index| workspace(index).map(|ws| SidebarJump::Workspace(ws.workspace_id.clone())),
        &config.palette,
        hits,
    );
}

/// Agents-panel badges. Called by upstream's `render_agent_panel` after the
/// list is drawn; `start` is the normalized scroll offset.
pub(super) fn agent_list_badges(
    buffer: &mut Buffer,
    rows: &[super::agent_sidebar::AgentRow],
    start: usize,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    let body = content_body(hits.agent_body, hits.agent_scrollbar);
    let drawn = hits
        .agents
        .iter()
        .filter(|(rect, _)| super::contains(body, (rect.x, rect.y)))
        .count();
    render_list_badges(
        buffer,
        body,
        list_window(start, drawn, rows.len()),
        rows.len(),
        |index| rows.get(index).map(|row| row.status),
        |index| {
            rows.get(index).map(|row| SidebarJump::Pane {
                pane_id: row.pane_id.clone(),
                index,
            })
        },
        &config.palette,
        hits,
    );
}

impl ClientShellState {
    /// A click on a sidebar overflow badge focuses its target.
    pub(super) fn sidebar_overflow_jump(
        &mut self,
        point: (u16, u16),
        outcome: &mut ClientShellInput,
    ) -> bool {
        let Some(jump) = self
            .hits
            .sidebar_overflow
            .iter()
            .find(|(rect, _)| super::contains(*rect, point))
            .map(|(_, jump)| jump.clone())
        else {
            return false;
        };
        let method = match jump {
            SidebarJump::Workspace(workspace_id) => {
                crate::api::schema::Method::WorkspaceFocus(crate::api::schema::WorkspaceTarget {
                    workspace_id,
                })
            }
            SidebarJump::Pane { pane_id, index } => {
                self.agent_scroll = index;
                outcome.repaint = true;
                crate::api::schema::Method::PaneFocus(crate::api::schema::PaneTarget { pane_id })
            }
        };
        self.push_endpoint_method(method, outcome);
        true
    }
}
