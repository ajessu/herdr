//! Fork: under `ui.sidebar.style = "zellij"` the phone switcher's agent and
//! space items come from the desktop sidebar row engine, so `[ui.sidebar.agents]`
//! and `[ui.sidebar.spaces]` rows, the divider, the highlight and `row_gap`
//! apply on the phone too.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::mobile::{mobile_endpoint_state, MobileItem};
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

/// Pushes one space item, preceded by a `row_gap` item when it is a top-level
/// entry that follows another space, as on desktop.
pub(super) fn push_workspace_item(
    items: &mut Vec<MobileItem>,
    endpoint: super::aggregate_navigation::CachedEndpointSnapshot<'_>,
    entry: &WorkspaceEntry,
    workspace: &ClientShellWorkspace,
    focused: bool,
    background: Color,
    config: &ClientShellConfig,
    width: u16,
) {
    let palette = &config.palette;
    let follows_space = matches!(
        items.last().and_then(|item| item.target.as_ref()),
        Some(ClientMobileTarget::Workspace { .. })
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
                (false, false, _) => "    ",
                (true, true, false) => "  ├─ ",
                (true, true, true) => "  └─ ",
                (true, false, false) => "  │    ",
                (true, false, true) => "       ",
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

/// Dims a stale machine's item, as the desktop agents panel does, and appends
/// the machine's state to its last line. Callers fit the rows to leave room.
fn mark_stale(lines: &mut [Line<'static>], suffix: String, config: &ClientShellConfig) {
    let dim = Style::default()
        .fg(config.palette.overlay0)
        .add_modifier(Modifier::DIM);
    for line in lines.iter_mut() {
        for span in &mut line.spans {
            span.style = span.style.patch(dim);
        }
    }
    if let Some(last) = lines.last_mut() {
        last.spans.push(Span::styled(suffix, dim));
    }
}
