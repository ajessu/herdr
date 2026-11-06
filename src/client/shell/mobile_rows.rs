//! Fork: under `ui.sidebar.style = "zellij"` the phone switcher's agent and
//! space items come from the desktop sidebar row engine, so `[ui.sidebar.agents]`
//! and `[ui.sidebar.spaces]` rows, the divider, the highlight and `row_gap`
//! apply on the phone too. Each space on the active machine lists its tabs
//! beneath it, and `+N` badges count the agents and spaces scrolled out of view.

use crate::api::schema::AgentStatus;
use ratatui::{
    buffer::Buffer,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::mobile::{mobile_endpoint_state, MobileItem};
use super::overflow::{self, OverflowSide};
use super::*;

/// Agent items in the aggregate agents order, each followed by its gap item
/// (blank lines, with the divider rule on the middle one).
pub(super) fn agent_items(
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &ClientShellConfig,
    width: u16,
) -> Vec<MobileItem> {
    let palette = &config.palette;
    let rows = super::endpoint_agents::agent_rows(endpoints, active_endpoint_id, config);
    let marks = rows
        .iter()
        .map(|row| row.agent.divider_mark)
        .collect::<Vec<_>>();
    let gaps = super::agent_sidebar::agent_list_gaps(config.agents.row_gap, &marks);
    let mut items = Vec::new();
    for (row, (gap, divider)) in rows.into_iter().zip(gaps) {
        let stale = row.stale.then(|| stale_suffix(row.status));
        let mut lines = super::agent_sidebar::agent_row_lines(
            &row.agent,
            config,
            width.saturating_sub(stale.as_deref().map_or(0, super::render::display_width)),
        );
        if let Some(suffix) = stale {
            mark_stale(&mut lines, suffix, config);
        }
        items.push(MobileItem {
            lines,
            background: super::agent_sidebar::agent_row_background(&row.agent, palette.surface_dim)
                .unwrap_or(palette.panel_bg),
            target: Some(ClientMobileTarget::Agent {
                endpoint_id: row.endpoint_id,
                pane_id: row.agent.pane_id,
            }),
        });
        if gap > 0 {
            let mut lines = vec![Line::default(); usize::from(gap)];
            if divider {
                lines[usize::from((gap - 1) / 2)] =
                    super::sidebar_chrome::divider_rule_line(config, width);
            }
            items.push(gap_item(lines, config));
        }
    }
    items
}

/// Indices of the active machine's spaces directly followed by a worktree child.
/// Empty elsewhere, since only spaces with nested tabs draw that line.
pub(super) fn worktree_parents(
    endpoint: super::aggregate_navigation::CachedEndpointSnapshot<'_>,
    active_endpoint_id: &ClientEndpointId,
    config: &ClientShellConfig,
) -> HashSet<usize> {
    if endpoint.endpoint_id != active_endpoint_id || !super::sidebar_chrome::zellij(config) {
        return HashSet::new();
    }
    super::render::workspace_entries(endpoint.snapshot, &HashSet::new())
        .windows(2)
        .filter(|pair| !pair[0].indented && pair[1].indented)
        .map(|pair| pair[0].index)
        .collect()
}

/// Pushes one space item and, on the active machine, the rows nested under it.
pub(super) fn push_space_items(
    items: &mut Vec<MobileItem>,
    endpoint: super::aggregate_navigation::CachedEndpointSnapshot<'_>,
    active_endpoint_id: &ClientEndpointId,
    entry: &WorkspaceEntry,
    workspace: &ClientShellWorkspace,
    has_worktree_children: bool,
    background: Color,
    config: &ClientShellConfig,
    width: u16,
) {
    let active = endpoint.endpoint_id == active_endpoint_id;
    push_workspace_item(
        items,
        endpoint,
        entry,
        workspace,
        active && workspace.focused,
        has_worktree_children,
        background,
        config,
        width,
    );
    if active {
        push_tab_items(
            items,
            endpoint,
            entry,
            &workspace.workspace_id,
            has_worktree_children,
            config,
            width,
        );
    }
}

/// Pushes one space item, preceded by a `row_gap` item when it is a top-level
/// entry that follows another space, as on desktop.
fn push_workspace_item(
    items: &mut Vec<MobileItem>,
    endpoint: super::aggregate_navigation::CachedEndpointSnapshot<'_>,
    entry: &WorkspaceEntry,
    workspace: &ClientShellWorkspace,
    focused: bool,
    draws_worktree_line: bool,
    background: Color,
    config: &ClientShellConfig,
    width: u16,
) {
    let palette = &config.palette;
    let follows_space = matches!(
        items.last().and_then(|item| item.target.as_ref()),
        Some(
            ClientMobileTarget::Workspace { .. }
                | ClientMobileTarget::Tab { .. }
                | ClientMobileTarget::NewTab
        )
    );
    if follows_space && !entry.indented && config.spaces.row_gap > 0 {
        items.push(gap_item(
            vec![Line::default(); usize::from(config.spaces.row_gap)],
            config,
        ));
    }
    let mut rows = super::render::sidebar::workspace_rows(
        workspace,
        workspace.agent_status,
        entry.indented,
        &config.spaces,
    );
    let stale = endpoint.stale().then(|| stale_suffix(endpoint.status));
    let width = width.saturating_sub(stale.as_deref().map_or(0, super::render::display_width));
    let mut labelled = false;
    // The widest connector is 7 cells; the label gets at most half of the rest.
    let short_machine =
        crate::ui::truncate_end(endpoint.label, usize::from(width.saturating_sub(7) / 2));
    for token in rows.iter_mut().flatten() {
        if let crate::ui::ResolvedTokenKind::Workspace(name) = &mut token.kind {
            *name = format!("{short_machine} · {name}");
            labelled = true;
        }
    }
    if rows.is_empty() {
        rows.push(Vec::new());
    }
    let workspace_style = if focused {
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.subtext0)
    };
    let status_style = Style::default().fg(status_color(workspace.agent_status, palette));
    let mut lines = rows
        .iter()
        .enumerate()
        .map(|(index, tokens)| {
            let connector = match (entry.indented, index == 0, entry.last_child) {
                (false, true, _) => "  ",
                (false, false, _) if draws_worktree_line => "  │ ",
                (false, false, _) => "    ",
                (true, true, false) => "  ├─ ",
                (true, true, true) => "  └─ ",
                (true, false, last_child) => worktree_continuation(last_child),
            };
            let room = usize::from(width.saturating_sub(super::render::display_width(connector)));
            // Without a workspace token the machine label leads the row: the
            // tokens keep at least half the room and the label takes what they
            // leave, or is left out when that is 3 cells or fewer.
            let machine = (index == 0 && !labelled).then_some(endpoint.label);
            let token_room = machine.map_or(room, |machine| {
                room.saturating_sub(
                    (usize::from(super::render::display_width(machine)) + 3).min(room / 2),
                )
            });
            let token_spans = crate::ui::resolved_token_spans(
                tokens,
                (
                    status_icon(workspace.agent_status, config.status_indicators),
                    status_style,
                ),
                status_style,
                workspace_style,
                Style::default().fg(if focused {
                    palette.mauve
                } else {
                    palette.overlay0
                }),
                Style::default().fg(palette.overlay1),
                palette,
                token_room,
            );
            let mut prefix = connector.to_owned();
            if let Some(machine) = machine {
                let used = token_spans.iter().map(Span::width).sum::<usize>();
                if used == 0 {
                    prefix.push_str(&crate::ui::truncate_end(machine, room));
                } else if room.saturating_sub(used) > 3 {
                    prefix.push_str(&crate::ui::truncate_end(machine, room - used - 3));
                    prefix.push_str(" · ");
                }
            }
            let mut spans = vec![Span::styled(prefix, Style::default().fg(palette.overlay0))];
            spans.extend(token_spans);
            Line::from(spans)
        })
        .collect::<Vec<_>>();
    if let Some(suffix) = stale {
        mark_stale(&mut lines, suffix, config);
    }
    items.push(MobileItem {
        lines,
        background,
        target: Some(ClientMobileTarget::Workspace {
            endpoint_id: endpoint.endpoint_id.clone(),
            workspace_id: workspace.workspace_id.clone(),
        }),
    });
}

/// Pushes a space's nested rows: its tabs in snapshot order, led by "+ new tab"
/// under the current space, which is where that action creates the tab. The
/// focused tab's branch points at it.
fn push_tab_items(
    items: &mut Vec<MobileItem>,
    endpoint: super::aggregate_navigation::CachedEndpointSnapshot<'_>,
    entry: &WorkspaceEntry,
    workspace_id: &str,
    has_worktree_children: bool,
    config: &ClientShellConfig,
    width: u16,
) {
    let palette = &config.palette;
    let indent = match (entry.indented, has_worktree_children) {
        (false, true) => "  │ ",
        (false, false) => "    ",
        (true, _) => worktree_continuation(entry.last_child),
    };
    let tabs = endpoint
        .snapshot
        .tabs
        .iter()
        .filter(|tab| tab.workspace_id == workspace_id)
        .collect::<Vec<_>>();
    let current_space = endpoint.snapshot.focused_workspace_id.as_deref() == Some(workspace_id);
    let rows = tabs.len() + usize::from(current_space);
    let prefix = |row: usize, focused: bool| {
        let branch = match (row + 1 == rows, focused) {
            (false, false) => "├─ ",
            (false, true) => "├▸ ",
            (true, false) => "└─ ",
            (true, true) => "└▸ ",
        };
        let color = if focused {
            palette.accent
        } else {
            palette.overlay0
        };
        vec![
            Span::styled(indent, Style::default().fg(palette.overlay0)),
            Span::styled(branch, Style::default().fg(color)),
        ]
    };
    let item = |spans: Vec<Span<'static>>, background, target| {
        let mut lines = vec![Line::from(spans)];
        if endpoint.stale() {
            dim_lines(&mut lines, config);
        }
        MobileItem {
            lines,
            background,
            target: Some(target),
        }
    };
    if current_space {
        let mut spans = prefix(0, false);
        spans.push(Span::styled(
            "+ new tab",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ));
        items.push(item(spans, palette.panel_bg, ClientMobileTarget::NewTab));
    }
    let prefix_width = usize::from(super::render::display_width(indent)) + 3;
    for (index, tab) in tabs.iter().enumerate() {
        let mut spans = prefix(index + usize::from(current_space), tab.focused);
        let mut room = usize::from(width).saturating_sub(prefix_width);
        if let Some(dot) =
            super::tab_chrome::tab_status_dot(tab.agent_status, config.tab_status, config)
        {
            room = room.saturating_sub(usize::from(super::render::display_width(dot.glyph)) + 1);
            spans.push(Span::styled(dot.glyph, dot.style));
            spans.push(Span::raw(" "));
        }
        let name = super::tab_chrome::sanitize_display_name(&tab.label);
        let label = if tab.custom_label {
            format!("{} · {name}", index + 1)
        } else {
            format!("tab {name}")
        };
        let (background, label_style) = if tab.focused {
            (
                palette.surface_dim,
                Style::default()
                    .fg(palette.text)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            (palette.panel_bg, Style::default().fg(palette.subtext0))
        };
        spans.push(Span::styled(
            crate::ui::truncate_end(&label, room),
            label_style,
        ));
        items.push(item(
            spans,
            background,
            ClientMobileTarget::Tab {
                endpoint_id: endpoint.endpoint_id.clone(),
                tab_id: tab.tab_id.clone(),
            },
        ));
    }
}

/// The header's tab text under the zellij style, led by the space's active tab's
/// dot per `ui.show_tab_status`, and that dot for `paint_header_tab_dot`.
pub(super) fn header_tab_text(
    text: String,
    snapshot: &ClientShellSnapshot,
    workspace: &ClientShellWorkspace,
    config: &ClientShellConfig,
) -> (String, Option<super::tab_chrome::TabStatusDot>) {
    let dot = super::sidebar_chrome::zellij(config)
        .then(|| {
            // The same tab the text names: the active one, else the space's first.
            let mut tabs = snapshot
                .tabs
                .iter()
                .filter(|tab| tab.workspace_id == workspace.workspace_id);
            tabs.clone()
                .find(|tab| tab.tab_id == workspace.active_tab_id)
                .or_else(|| tabs.next())
        })
        .flatten()
        .and_then(|tab| {
            super::tab_chrome::tab_status_dot(tab.agent_status, config.tab_status, config)
        });
    match dot {
        Some(dot) => (format!("{} {text}", dot.glyph), Some(dot)),
        None => (text, None),
    }
}

/// Recolors the dot `header_tab_text` wrote at the start of the tab text.
pub(super) fn paint_header_tab_dot(
    buffer: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    dot: Option<super::tab_chrome::TabStatusDot>,
    background: Color,
) {
    if let Some(dot) = dot {
        super::render::put_text(buffer, x, y, width, dot.glyph, dot.style.bg(background));
    }
}

/// Splits the last row off an overflowing switcher's list viewport, for the badges
/// of what is scrolled below. The row stays reserved at every scroll position, so
/// the rows above never shift. It is empty when no row is reserved.
pub(super) fn split_badge_row(
    viewport: Rect,
    total_rows: usize,
    config: &ClientShellConfig,
) -> (Rect, Rect) {
    if !super::sidebar_chrome::zellij(config)
        || viewport.height < 2
        || viewport.width < 2
        || total_rows <= usize::from(viewport.height)
    {
        return (viewport, Rect::default());
    }
    let height = viewport.height - 1;
    (
        Rect { height, ..viewport },
        Rect {
            y: viewport.y + height,
            height: 1,
            ..viewport
        },
    )
}

/// One agent or space in the switcher's scrolled document.
struct ListedItem<'a> {
    rows: std::ops::Range<usize>,
    status: Option<AgentStatus>,
    target: &'a ClientMobileTarget,
}

/// A list's hidden items on each edge of the visible document rows. An item with
/// a line still visible is not hidden.
fn hidden_sides(
    list: &[ListedItem<'_>],
    visible: &std::ops::Range<usize>,
) -> (OverflowSide, OverflowSide) {
    let first = list
        .iter()
        .take_while(|item| item.rows.end <= visible.start)
        .count();
    let drawn = list[first..]
        .iter()
        .take_while(|item| item.rows.start < visible.end)
        .count();
    let window = super::sidebar_chrome::list_window(first, drawn, list.len());
    let status_of = |index: usize| list.get(index).and_then(|item| item.status);
    (
        overflow::side_above(window, status_of),
        overflow::side_below(window, list.len(), status_of),
    )
}

/// The snapshot of an online machine. Any other machine's cached statuses are not
/// live and a tap may not reach it, so its items get no badge.
fn online_snapshot<'a>(
    endpoints: &'a [ClientShellEndpoint],
    endpoint_id: &ClientEndpointId,
) -> Option<&'a ClientShellSnapshot> {
    endpoints
        .iter()
        .find(|endpoint| &endpoint.endpoint_id == endpoint_id)
        .filter(|endpoint| endpoint.status == ClientEndpointStatus::Online)
        .and_then(|endpoint| endpoint.snapshot.as_deref())
}

/// The desktop `+N` badges for the switcher's two lists. Both share one scrolled
/// document, so a badge names its list. Those for items above sit on the rule
/// over the list, unless the active machine's connection banner has that row, and
/// those for items below on `badge_row`, so no row's tap area is covered. A tap
/// acts on the hidden item's own target, as tapping its row does.
pub(super) fn render_switcher_badges(
    buffer: &mut Buffer,
    rule_row: Rect,
    badge_row: Rect,
    items: &[MobileItem],
    visible: std::ops::Range<usize>,
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    palette: &Palette,
    hits: &mut ShellHitMap,
) {
    if badge_row.is_empty() {
        return;
    }
    super::render::put_text(
        buffer,
        badge_row.x,
        badge_row.y,
        badge_row.width,
        &"─".repeat(usize::from(badge_row.width)),
        Style::default()
            .fg(palette.surface_dim)
            .bg(palette.panel_bg),
    );
    let (mut agents, mut spaces) = (Vec::new(), Vec::new());
    let mut row = 0usize;
    for item in items {
        let rows = row..row + item.lines.len();
        row = rows.end;
        match item.target.as_ref() {
            Some(
                target @ ClientMobileTarget::Agent {
                    endpoint_id,
                    pane_id,
                },
            ) => {
                if let Some(snapshot) = online_snapshot(endpoints, endpoint_id) {
                    let agent = snapshot.agents.iter().find(|a| &a.pane_id == pane_id);
                    agents.push(ListedItem {
                        rows,
                        status: agent.map(|agent| agent.agent_status),
                        target,
                    });
                }
            }
            Some(
                target @ ClientMobileTarget::Workspace {
                    endpoint_id,
                    workspace_id,
                },
            ) => {
                if let Some(snapshot) = online_snapshot(endpoints, endpoint_id) {
                    let space = snapshot
                        .workspaces
                        .iter()
                        .find(|ws| &ws.workspace_id == workspace_id);
                    spaces.push(ListedItem {
                        rows,
                        status: space.map(|space| space.agent_status),
                        target,
                    });
                }
            }
            _ => {}
        }
    }
    let active_online = endpoints
        .iter()
        .find(|endpoint| &endpoint.endpoint_id == active_endpoint_id)
        .is_none_or(|endpoint| endpoint.status == ClientEndpointStatus::Online);
    let rule_row = if active_online {
        rule_row
    } else {
        Rect::default()
    };
    let (agents_above, agents_below) = hidden_sides(&agents, &visible);
    let (spaces_above, spaces_below) = hidden_sides(&spaces, &visible);
    for (row, sides) in [
        (
            rule_row,
            [
                (&spaces, "spaces", spaces_above),
                (&agents, "agents", agents_above),
            ],
        ),
        (
            badge_row,
            [
                (&spaces, "spaces", spaces_below),
                (&agents, "agents", agents_below),
            ],
        ),
    ] {
        if row.is_empty() {
            continue;
        }
        let mut right = row.right();
        for (list, label, side) in sides {
            if side.is_empty() {
                continue;
            }
            let mut spans = vec![Span::styled(
                format!(" {label} "),
                Style::default().fg(palette.overlay0),
            )];
            spans.extend(overflow::badge_spans(side, palette));
            let line = Line::from(spans);
            let room = right - row.x;
            // A clipped badge still shows its list and how many are hidden.
            let minimum = format!(" {label} +{}", overflow::count_label(side.hidden));
            if room < super::render::display_width(&minimum) {
                continue;
            }
            let width = u16::try_from(line.width()).unwrap_or(u16::MAX).min(room);
            let rect = Rect::new(right - width, row.y, width, 1);
            Paragraph::new(line).render(rect, buffer);
            if let Some(item) = overflow::resolve_jump(side).and_then(|index| list.get(index)) {
                hits.mobile_targets.push((rect, item.target.clone()));
            }
            right = rect.x;
        }
    }
}

/// What continues the worktree column on rows below a worktree child's first.
fn worktree_continuation(last_child: bool) -> &'static str {
    if last_child {
        "       "
    } else {
        "  │    "
    }
}

fn gap_item(lines: Vec<Line<'static>>, config: &ClientShellConfig) -> MobileItem {
    MobileItem {
        lines,
        background: config.palette.panel_bg,
        target: None,
    }
}

fn stale_suffix(status: ClientEndpointStatus) -> String {
    format!(" · {}", mobile_endpoint_state(status))
}

fn stale_style(config: &ClientShellConfig) -> Style {
    Style::default()
        .fg(config.palette.overlay0)
        .add_modifier(Modifier::DIM)
}

fn dim_lines(lines: &mut [Line<'static>], config: &ClientShellConfig) {
    for line in lines.iter_mut() {
        for span in &mut line.spans {
            span.style = span.style.patch(stale_style(config));
        }
    }
}

/// Dims a stale machine's item, as the desktop agents panel does, and appends
/// the machine's state to its last line. Callers fit the rows to leave room.
fn mark_stale(lines: &mut [Line<'static>], suffix: String, config: &ClientShellConfig) {
    dim_lines(lines, config);
    if let Some(last) = lines.last_mut() {
        last.spans.push(Span::styled(suffix, stale_style(config)));
    }
}
