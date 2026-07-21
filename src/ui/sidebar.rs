use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::scrollbar::{render_scrollbar, should_show_scrollbar, SCROLLBAR_THUMB};
use super::status::{agent_icon, state_dot, state_label, state_label_color};
use crate::app::state::{AgentPanelSort, Palette};
use crate::app::{AppState, Mode};
use crate::detect::AgentState;
use crate::terminal::TerminalRuntimeRegistry;

const WORKSPACE_SECTION_HEADER_ROWS: u16 = 2;
const AGENT_PANEL_HEADER_ROWS: u16 = 3;

pub(crate) struct AgentPanelEntry {
    pub ws_idx: usize,
    pub tab_idx: usize,
    pub pane_id: crate::layout::PaneId,
    pub primary_label: String,
    pub primary_tab_label: Option<String>,
    pub agent_label: Option<String>,
    pub state: AgentState,
    pub seen: bool,
    pub last_agent_state_change_seq: Option<u64>,
    pub custom_status: Option<String>,
    pub state_labels: std::collections::HashMap<String, String>,
}

fn sidebar_section_heights(total_h: u16, split_ratio: f32) -> (u16, u16) {
    if total_h == 0 {
        return (0, 0);
    }

    if total_h < 6 {
        let ws_h = total_h.div_ceil(2);
        return (ws_h, total_h.saturating_sub(ws_h));
    }

    let ratio = split_ratio.clamp(0.1, 0.9);
    let ws_h = ((total_h as f32) * ratio).round() as u16;
    let ws_h = ws_h.clamp(3, total_h.saturating_sub(3));
    let detail_h = total_h.saturating_sub(ws_h);
    (ws_h, detail_h)
}

pub(crate) struct ExpandedSidebarLayout {
    pub spaces: Rect,
    pub detail: Rect,
}

pub(crate) fn compute_expanded_sidebar_layout(
    area: Rect,
    split_ratio: f32,
) -> ExpandedSidebarLayout {
    let content = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    if content.width == 0 || content.height == 0 {
        return ExpandedSidebarLayout {
            spaces: Rect::default(),
            detail: Rect::default(),
        };
    }

    let (ws_h, detail_h) = sidebar_section_heights(content.height, split_ratio);
    let spaces = Rect::new(content.x, content.y, content.width, ws_h);
    let detail = Rect::new(content.x, content.y + ws_h, content.width, detail_h);
    ExpandedSidebarLayout { spaces, detail }
}

pub(crate) fn expanded_sidebar_sections(area: Rect, split_ratio: f32) -> (Rect, Rect) {
    let layout = compute_expanded_sidebar_layout(area, split_ratio);
    (layout.spaces, layout.detail)
}

pub(crate) fn sidebar_section_divider_rect(area: Rect, split_ratio: f32) -> Rect {
    let content = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    if content.width == 0 || content.height < 6 {
        return Rect::default();
    }

    let (ws_h, _) = sidebar_section_heights(content.height, split_ratio);
    Rect::new(content.x, content.y + ws_h, content.width, 1)
}

fn agent_panel_sort_label(sort: AgentPanelSort) -> &'static str {
    match sort {
        AgentPanelSort::Spaces => "grouped",
        AgentPanelSort::Priority => "priority",
    }
}

pub(crate) fn agent_panel_toggle_rect(area: Rect, sort: AgentPanelSort) -> Rect {
    if area.width == 0 || area.height < 2 {
        return Rect::default();
    }

    let label = agent_panel_sort_label(sort);
    let width = label.chars().count() as u16;
    Rect::new(
        area.x + area.width.saturating_sub(width),
        area.y + 1,
        width,
        1,
    )
}

pub(crate) fn agent_panel_entries(app: &AppState) -> Vec<AgentPanelEntry> {
    agent_panel_entries_with_runtimes(app, None)
}

pub(crate) fn agent_panel_entries_from(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
) -> Vec<AgentPanelEntry> {
    agent_panel_entries_with_runtimes(app, Some(terminal_runtimes))
}

fn agent_panel_entries_with_runtimes(
    app: &AppState,
    terminal_runtimes: Option<&TerminalRuntimeRegistry>,
) -> Vec<AgentPanelEntry> {
    let empty_runtimes;
    let terminal_runtimes = match terminal_runtimes {
        Some(terminal_runtimes) => terminal_runtimes,
        None => {
            empty_runtimes = TerminalRuntimeRegistry::new();
            &empty_runtimes
        }
    };

    let mut entries: Vec<_> = app
        .workspaces
        .iter()
        .enumerate()
        .flat_map(|(ws_idx, ws)| {
            let multi_tab = ws.tabs.len() > 1;
            let workspace_label = ws.display_name_from(&app.terminals, terminal_runtimes);
            ws.pane_details(&app.terminals)
                .into_iter()
                .map(move |detail| AgentPanelEntry {
                    ws_idx,
                    tab_idx: detail.tab_idx,
                    pane_id: detail.pane_id,
                    primary_label: workspace_label.clone(),
                    primary_tab_label: multi_tab.then_some(detail.tab_label),
                    agent_label: Some(detail.agent_label),
                    state: detail.state,
                    seen: detail.seen,
                    last_agent_state_change_seq: detail.last_agent_state_change_seq,
                    custom_status: detail.custom_status,
                    state_labels: detail.state_labels,
                })
        })
        .collect();

    if matches!(app.agent_panel_sort, AgentPanelSort::Priority) {
        entries.sort_by_key(|entry| {
            (
                std::cmp::Reverse(workspace_attention_priority(entry.state, entry.seen)),
                std::cmp::Reverse(entry.last_agent_state_change_seq),
            )
        });
    }

    entries
}

pub(super) fn agent_panel_status_key(state: AgentState, seen: bool) -> &'static str {
    match (state, seen) {
        (AgentState::Idle, false) => "done",
        (AgentState::Idle, true) => "idle",
        (AgentState::Working, _) => "working",
        (AgentState::Blocked, _) => "blocked",
        (AgentState::Unknown, _) => "unknown",
    }
}

fn truncate_text(text: &str, max_width: usize) -> String {
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

const AGENT_LABEL_SEPARATOR: &str = " · ";

fn format_agent_panel_primary_label(entry: &AgentPanelEntry, max_width: usize) -> String {
    let Some(tab_label) = entry.primary_tab_label.as_deref() else {
        return truncate_text(&entry.primary_label, max_width);
    };

    let separator_width = AGENT_LABEL_SEPARATOR.chars().count();
    if max_width <= separator_width + 2 {
        return truncate_text(
            &format!(
                "{}{}{}",
                entry.primary_label, AGENT_LABEL_SEPARATOR, tab_label
            ),
            max_width,
        );
    }

    let available = max_width.saturating_sub(separator_width);
    let min_tab = 4.min(available.saturating_sub(1)).max(1);
    let preferred_workspace = ((available * 2) / 3).max(1);
    let mut workspace_budget = preferred_workspace
        .min(available.saturating_sub(min_tab))
        .max(1);
    let mut tab_budget = available.saturating_sub(workspace_budget);

    let workspace_len = entry.primary_label.chars().count();
    let tab_len = tab_label.chars().count();

    if workspace_len < workspace_budget {
        let spare = workspace_budget - workspace_len;
        workspace_budget = workspace_len;
        tab_budget = (tab_budget + spare).min(available.saturating_sub(workspace_budget));
    }
    if tab_len < tab_budget {
        let spare = tab_budget - tab_len;
        tab_budget = tab_len;
        workspace_budget = (workspace_budget + spare).min(available.saturating_sub(tab_budget));
    }

    format!(
        "{}{}{}",
        truncate_text(&entry.primary_label, workspace_budget),
        AGENT_LABEL_SEPARATOR,
        truncate_text(tab_label, tab_budget)
    )
}

/// FR2 degradation for the single-line space item, applied in priority order:
/// ahead/behind counts drop first, then the branch truncates (char-safe, at
/// least 4 visible chars), then the branch drops, then the name truncates.
/// `counts_width` includes its leading separator space; pass 0 when there are
/// no counts. Returns `(name, branch, show_counts)`.
pub(crate) fn space_line_parts(
    name: &str,
    branch: Option<&str>,
    counts_width: usize,
    available: usize,
) -> (String, Option<String>, bool) {
    const MIN_BRANCH_CHARS: usize = 4;
    let name_len = name.chars().count();
    if let Some(branch) = branch {
        let branch_len = branch.chars().count();
        if counts_width > 0 && name_len + 1 + branch_len + counts_width <= available {
            return (name.to_string(), Some(branch.to_string()), true);
        }
        if name_len + 1 + branch_len <= available {
            return (name.to_string(), Some(branch.to_string()), false);
        }
        let branch_budget = available.saturating_sub(name_len + 1);
        if branch_budget >= MIN_BRANCH_CHARS {
            return (
                name.to_string(),
                Some(truncate_text(branch, branch_budget)),
                false,
            );
        }
    }
    if name_len <= available {
        return (name.to_string(), None, false);
    }
    (truncate_text(name, available), None, false)
}

fn workspace_attention_priority(state: AgentState, seen: bool) -> u8 {
    match (state, seen) {
        (AgentState::Blocked, _) => 4,
        (AgentState::Idle, false) => 3,
        (AgentState::Working, _) => 2,
        (AgentState::Idle, true) => 1,
        (AgentState::Unknown, _) => 0,
    }
}

fn space_aggregate_state(app: &AppState, key: &str) -> (AgentState, bool) {
    app.workspaces
        .iter()
        .filter(|ws| ws.worktree_space().is_some_and(|space| space.key == key))
        .map(|ws| ws.aggregate_state(&app.terminals))
        .max_by_key(|(state, seen)| workspace_attention_priority(*state, *seen))
        .unwrap_or((AgentState::Unknown, true))
}

pub(crate) fn workspace_parent_group_state(
    app: &AppState,
    ws_idx: usize,
) -> Option<(String, bool)> {
    let space = app.workspaces.get(ws_idx)?.worktree_space()?;
    if space.is_linked_worktree {
        return None;
    }
    let member_count = app
        .workspaces
        .iter()
        .filter(|ws| {
            ws.worktree_space()
                .is_some_and(|member| member.key == space.key)
        })
        .count();
    (member_count >= 2).then(|| {
        (
            space.key.clone(),
            app.collapsed_space_keys.contains(&space.key),
        )
    })
}

fn grouped_child_display_label(label: &str, branch: Option<&str>, has_custom_name: bool) -> String {
    if has_custom_name {
        return label.to_string();
    }
    let Some(branch) = branch else {
        return label.to_string();
    };
    branch
        .strip_prefix("worktree/")
        .unwrap_or(branch)
        .to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkspaceListEntry {
    Workspace { ws_idx: usize, indented: bool },
}

pub(crate) fn normalized_workspace_scroll(app: &AppState, area: Rect, requested: usize) -> usize {
    let ws_area = workspace_list_rect(area, app.sidebar_section_split);
    let body = workspace_list_body_rect(ws_area, false);
    if body.height == 0 {
        return requested;
    }

    let entry_count = workspace_list_entries(app).len();
    if entry_count == 0 {
        0
    } else {
        requested.min(entry_count.saturating_sub(1))
    }
}

pub(crate) fn workspace_list_entries(app: &AppState) -> Vec<WorkspaceListEntry> {
    let mut members_by_key = std::collections::HashMap::<String, Vec<usize>>::new();
    for (ws_idx, ws) in app.workspaces.iter().enumerate() {
        if let Some(space) = ws.worktree_space() {
            members_by_key
                .entry(space.key.clone())
                .or_default()
                .push(ws_idx);
        }
    }
    let grouped_keys = members_by_key
        .iter()
        .filter(|(_, members)| {
            members.len() >= 2
                && members.iter().any(|idx| {
                    app.workspaces
                        .get(*idx)
                        .and_then(|ws| ws.worktree_space())
                        .is_some_and(|space| !space.is_linked_worktree)
                })
        })
        .map(|(key, _)| key.clone())
        .collect::<std::collections::HashSet<_>>();

    let visible_group_idx = if matches!(app.mode, Mode::Navigate) {
        Some(app.selected)
    } else {
        app.active
    };
    let active_group = visible_group_idx.and_then(|idx| {
        app.workspaces
            .get(idx)
            .and_then(|ws| ws.worktree_space())
            .map(|space| space.key.clone())
    });

    let mut emitted_groups = std::collections::HashSet::<String>::new();
    let mut entries = Vec::new();
    for (ws_idx, ws) in app.workspaces.iter().enumerate() {
        let Some(space) = ws
            .worktree_space()
            .filter(|space| grouped_keys.contains(&space.key))
        else {
            entries.push(WorkspaceListEntry::Workspace {
                ws_idx,
                indented: false,
            });
            continue;
        };

        if !emitted_groups.insert(space.key.clone()) {
            continue;
        }

        let Some(members) = members_by_key.get(&space.key) else {
            continue;
        };
        let Some(parent_idx) = members.iter().copied().find(|idx| {
            app.workspaces
                .get(*idx)
                .and_then(|member| member.worktree_space())
                .is_some_and(|member_space| !member_space.is_linked_worktree)
        }) else {
            entries.push(WorkspaceListEntry::Workspace {
                ws_idx,
                indented: false,
            });
            continue;
        };
        let collapsed = app.collapsed_space_keys.contains(&space.key);
        entries.push(WorkspaceListEntry::Workspace {
            ws_idx: parent_idx,
            indented: false,
        });

        if collapsed {
            if let Some(active_idx) = visible_group_idx
                .filter(|idx| *idx != parent_idx)
                .filter(|_| active_group.as_deref() == Some(space.key.as_str()))
            {
                entries.push(WorkspaceListEntry::Workspace {
                    ws_idx: active_idx,
                    indented: true,
                });
            }
        } else {
            for member_idx in members {
                if *member_idx == parent_idx {
                    continue;
                }
                entries.push(WorkspaceListEntry::Workspace {
                    ws_idx: *member_idx,
                    indented: true,
                });
            }
        }
    }
    entries
}

pub(crate) fn workspace_list_rect(area: Rect, split_ratio: f32) -> Rect {
    let (ws_area, _) = expanded_sidebar_sections(area, split_ratio);
    ws_area
}

pub(crate) fn workspace_list_body_rect(area: Rect, has_scrollbar: bool) -> Rect {
    if area.width == 0 || area.height <= WORKSPACE_SECTION_HEADER_ROWS {
        return Rect::default();
    }

    let body_y = area.y.saturating_add(WORKSPACE_SECTION_HEADER_ROWS);
    let footer_y = area.y + area.height.saturating_sub(1);
    let body_height = footer_y.saturating_sub(body_y);
    let body_width = area.width.saturating_sub(u16::from(has_scrollbar));
    Rect::new(area.x, body_y, body_width, body_height)
}

fn workspace_list_visible_count(app: &AppState, area: Rect, scroll: usize) -> usize {
    let body = workspace_list_body_rect(area, false);
    if body.width == 0 || body.height == 0 {
        return 0;
    }

    let entries = workspace_list_entries(app);
    entries
        .len()
        .saturating_sub(scroll)
        .min(body.height as usize)
}

pub(crate) fn workspace_list_scroll_metrics(
    app: &AppState,
    area: Rect,
) -> crate::pane::ScrollMetrics {
    let entries = workspace_list_entries(app);
    let total_rows = entries.len();
    let scroll = app.workspace_scroll.min(total_rows.saturating_sub(1));
    let viewport_rows = workspace_list_visible_count(app, area, scroll);
    let max_offset_from_bottom = total_rows.saturating_sub(viewport_rows);
    let offset_from_bottom = total_rows
        .saturating_sub(scroll)
        .saturating_sub(viewport_rows);

    crate::pane::ScrollMetrics {
        offset_from_bottom,
        max_offset_from_bottom,
        viewport_rows,
    }
}

pub(crate) fn workspace_list_scrollbar_rect(app: &AppState, area: Rect) -> Option<Rect> {
    let metrics = workspace_list_scroll_metrics(app, area);
    let body = workspace_list_body_rect(area, true);
    (should_show_scrollbar(metrics) && body.width > 0 && body.height > 0).then_some(Rect::new(
        area.x + area.width.saturating_sub(1),
        body.y,
        1,
        body.height,
    ))
}

pub(crate) fn agent_panel_body_rect(area: Rect, has_scrollbar: bool) -> Rect {
    if area.width == 0 || area.height <= AGENT_PANEL_HEADER_ROWS {
        return Rect::default();
    }

    let body_y = area.y.saturating_add(AGENT_PANEL_HEADER_ROWS);
    let body_height = (area.y + area.height).saturating_sub(body_y);
    let body_width = area.width.saturating_sub(u16::from(has_scrollbar));
    Rect::new(area.x, body_y, body_width, body_height)
}

/// Rows consumed by one agent panel item (2 content lines, no spacer).
pub(crate) const AGENT_PANEL_ITEM_ROWS: u16 = 2;

/// Single source of truth for agent panel item geometry (FR7): yields
/// `(entry_index, row_y)` for each item visible in `body` after `scroll`.
/// An item is visible only when both of its rows fit inside the body; an odd
/// body height leaves the trailing row unused.
pub(crate) fn agent_panel_item_rows(
    body: Rect,
    scroll: usize,
    entry_count: usize,
) -> impl Iterator<Item = (usize, u16)> {
    let capacity = if body.width == 0 {
        0
    } else {
        (body.height / AGENT_PANEL_ITEM_ROWS) as usize
    };
    let visible = entry_count.saturating_sub(scroll).min(capacity);
    (0..visible).map(move |i| (scroll + i, body.y + (i as u16) * AGENT_PANEL_ITEM_ROWS))
}

fn agent_panel_visible_count(area: Rect) -> usize {
    let body = agent_panel_body_rect(area, false);
    agent_panel_item_rows(body, 0, usize::MAX).count()
}

pub(crate) fn agent_panel_scroll_metrics(app: &AppState, area: Rect) -> crate::pane::ScrollMetrics {
    let viewport_rows = agent_panel_visible_count(area);
    let total_rows = agent_panel_entries(app).len();
    let max_offset_from_bottom = total_rows.saturating_sub(viewport_rows);
    let offset_from_bottom = total_rows
        .saturating_sub(app.agent_panel_scroll)
        .saturating_sub(viewport_rows);

    crate::pane::ScrollMetrics {
        offset_from_bottom,
        max_offset_from_bottom,
        viewport_rows,
    }
}

pub(crate) fn agent_panel_scrollbar_rect(app: &AppState, area: Rect) -> Option<Rect> {
    let metrics = agent_panel_scroll_metrics(app, area);
    let body = agent_panel_body_rect(area, true);
    (should_show_scrollbar(metrics) && body.width > 0 && body.height > 0).then_some(Rect::new(
        area.x + area.width.saturating_sub(1),
        body.y,
        1,
        body.height,
    ))
}

pub(crate) fn compute_workspace_list_areas(
    app: &AppState,
    area: Rect,
) -> (Vec<crate::app::state::WorkspaceCardArea>, Vec<()>) {
    let ws_area = workspace_list_rect(area, app.sidebar_section_split);
    if ws_area == Rect::default() {
        return (Vec::new(), Vec::new());
    }

    let metrics = workspace_list_scroll_metrics(app, ws_area);
    let body = workspace_list_body_rect(ws_area, should_show_scrollbar(metrics));
    if body.width == 0 || body.height == 0 {
        return (Vec::new(), Vec::new());
    }

    let scroll = app.workspace_scroll;
    let mut row_y = body.y;
    let body_bottom = body.y + body.height;
    let mut cards = Vec::new();
    let headers = Vec::new();

    let entries = workspace_list_entries(app);
    for entry in entries.iter().skip(scroll) {
        match entry {
            WorkspaceListEntry::Workspace { ws_idx, indented } => {
                if row_y.saturating_add(1) > body_bottom {
                    break;
                }
                cards.push(crate::app::state::WorkspaceCardArea {
                    ws_idx: *ws_idx,
                    rect: Rect::new(body.x, row_y, body.width, 1),
                    indented: *indented,
                });
                row_y = row_y.saturating_add(1);
            }
        }
    }

    (cards, headers)
}

pub(crate) fn compute_workspace_card_areas(
    app: &AppState,
    area: Rect,
) -> Vec<crate::app::state::WorkspaceCardArea> {
    compute_workspace_list_areas(app, area).0
}

/// Auto-scale sidebar width based on workspace identity + agent summary.
pub(crate) fn collapsed_sidebar_sections(area: Rect) -> (Rect, Option<u16>, Rect) {
    let content = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    if content.width == 0 || content.height == 0 {
        return (Rect::default(), None, Rect::default());
    }

    if content.height < 7 {
        return (content, None, Rect::default());
    }

    let total_h = content.height as usize;
    let ws_h = total_h.div_ceil(2);
    let detail_h = total_h.saturating_sub(ws_h + 1);
    if ws_h == 0 || detail_h == 0 {
        return (content, None, Rect::default());
    }

    let divider_y = content.y + ws_h as u16;
    let ws_area = Rect::new(content.x, content.y, content.width, ws_h as u16);
    let detail_area = Rect::new(content.x, divider_y + 1, content.width, detail_h as u16);
    (ws_area, Some(divider_y), detail_area)
}

pub(crate) struct CollapsedRailLayout {
    pub ws_area: Rect,
    pub divider_y: Option<u16>,
    pub detail_area: Rect,
    // Kept on the struct so the rail layout is single-sourced; read by the
    // layout-bounds tests. render_sidebar_toggle resolves the same rect.
    #[allow(dead_code)]
    pub toggle: Rect,
}

pub(crate) fn compute_collapsed_rail_layout(area: Rect) -> CollapsedRailLayout {
    let (ws_area, divider_y, detail_area) = collapsed_sidebar_sections(area);
    let toggle = collapsed_sidebar_toggle_rect(area);
    CollapsedRailLayout {
        ws_area,
        divider_y,
        detail_area,
        toggle,
    }
}

/// "Any non-idle state" predicate — Blocked, Working, or Idle-unseen — shared
/// with `tabs::chrome_is_attention` so the rail and the tab bar agree on which
/// items are badge-worthy. Idle-seen and Unknown are excluded.
pub(crate) fn is_attention_state(state: AgentState, seen: bool) -> bool {
    matches!(state, AgentState::Blocked | AgentState::Working)
        || (matches!(state, AgentState::Idle) && !seen)
}

/// Which workspace anchors the collapsed rail's window — the selected one while
/// navigating, else the active one. Always kept visible by the anchored window.
fn collapsed_ws_anchor(app: &AppState) -> usize {
    if matches!(app.mode, Mode::Navigate) {
        app.selected
    } else {
        app.active.unwrap_or(app.selected)
    }
}

/// Stateless visible window over the collapsed rail's workspace section.
pub(crate) fn collapsed_ws_window(app: &AppState, ws_area: Rect) -> super::overflow::ListWindow {
    super::overflow::anchored_window(
        app.workspaces.len(),
        ws_area.height as usize,
        collapsed_ws_anchor(app),
    )
}

/// The workspace index a collapsed detail section is showing panes for.
fn collapsed_detail_ws_idx(app: &AppState) -> Option<usize> {
    if matches!(app.mode, Mode::Navigate) {
        Some(app.selected)
    } else {
        app.active
    }
}

/// Pane details for the collapsed detail section, with the focused-pane anchor
/// index (so the focused pane stays visible in the window).
fn collapsed_detail_details(
    app: &AppState,
) -> Option<(usize, Vec<crate::workspace::PaneDetail>, usize)> {
    let ws_idx = collapsed_detail_ws_idx(app)?;
    let ws = app.workspaces.get(ws_idx)?;
    let details = ws.pane_details(&app.terminals);
    if details.is_empty() {
        return None;
    }
    let anchor = ws
        .active_tab()
        .map(|tab| tab.focused_pane_id())
        .and_then(|pid| details.iter().position(|d| d.pane_id == pid))
        .unwrap_or(0);
    Some((ws_idx, details, anchor))
}

/// The collapsed detail section's window mapping for the input layer: the
/// workspace index it shows, its pane details, and the anchored visible window.
/// Mirrors what the rail renders so a clicked row resolves to the same pane.
pub(crate) fn collapsed_detail_window(
    app: &AppState,
    detail_area: Rect,
) -> Option<(
    usize,
    Vec<crate::workspace::PaneDetail>,
    super::overflow::ListWindow,
)> {
    let content = collapsed_detail_content_area(detail_area);
    if content == Rect::default() {
        return None;
    }
    let (ws_idx, details, anchor) = collapsed_detail_details(app)?;
    let win = super::overflow::anchored_window(details.len(), content.height as usize, anchor);
    Some((ws_idx, details, win))
}

/// The detail-content area (detail_area minus its trailing toggle row) for the
/// collapsed rail, matching the render carve-out.
pub(crate) fn collapsed_detail_content_area(detail_area: Rect) -> Rect {
    Rect::new(
        detail_area.x,
        detail_area.y,
        detail_area.width,
        detail_area.height.saturating_sub(1),
    )
}

/// Place an overflow badge on a single row, right-anchored within `width` so the
/// `+N ●ᵏ` reads at the row's end. Hit zone is the whole row (touch-adequate,
/// NFR5). Returns a zero rect when the side is empty.
fn place_row_badge(
    x: u16,
    y: u16,
    width: u16,
    side: super::overflow::OverflowSide,
) -> super::overflow::OverflowBadgeRect {
    if side.is_empty() || width == 0 {
        return super::overflow::OverflowBadgeRect::default();
    }
    super::overflow::OverflowBadgeRect {
        rect: Rect::new(x, y, width, 1),
        side,
    }
}

/// Compute all four sidebar overflow badge rects (collapsed rail + expanded
/// surfaces) for the current frame. Pure: reads `AppState`, mutates nothing.
/// Render and the mouse layer both consume these rects so draw-rect == hit-rect.
pub(crate) fn compute_sidebar_overflow(
    app: &AppState,
    sidebar_area: Rect,
) -> crate::app::state::SidebarOverflowRects {
    use super::overflow::{scrolled_window, side_above, side_below};
    let mut rects = crate::app::state::SidebarOverflowRects::default();
    if sidebar_area == Rect::default() {
        return rects;
    }

    if app.sidebar_collapsed {
        let layout = compute_collapsed_rail_layout(sidebar_area);
        // Workspace section.
        if layout.ws_area != Rect::default() {
            let ws_area = layout.ws_area;
            let win = collapsed_ws_window(app, ws_area);
            let state_of = |i: usize| {
                app.workspaces
                    .get(i)
                    .map(|ws| ws.aggregate_state(&app.terminals))
                    .unwrap_or((AgentState::Idle, true))
            };
            let above = side_above(win, state_of);
            let below = side_below(win, app.workspaces.len(), state_of);
            rects.collapsed_ws_above = place_row_badge(ws_area.x, ws_area.y, ws_area.width, above);
            rects.collapsed_ws_below = place_row_badge(
                ws_area.x,
                ws_area.y + ws_area.height.saturating_sub(1),
                ws_area.width,
                below,
            );
        }
        // Detail section.
        let content = collapsed_detail_content_area(layout.detail_area);
        if content != Rect::default() {
            if let Some((_, details, anchor)) = collapsed_detail_details(app) {
                let win = super::overflow::anchored_window(
                    details.len(),
                    content.height as usize,
                    anchor,
                );
                let state_of = |i: usize| {
                    details
                        .get(i)
                        .map(|d| (d.state, d.seen))
                        .unwrap_or((AgentState::Idle, true))
                };
                let above = side_above(win, state_of);
                let below = side_below(win, details.len(), state_of);
                rects.collapsed_detail_above =
                    place_row_badge(content.x, content.y, content.width, above);
                rects.collapsed_detail_below = place_row_badge(
                    content.x,
                    content.y + content.height.saturating_sub(1),
                    content.width,
                    below,
                );
            }
        }
        return rects;
    }

    // Expanded surfaces reuse their existing top-anchored scroll offsets.
    let (ws_area, detail_area) = expanded_sidebar_sections(sidebar_area, app.sidebar_section_split);

    // Expanded workspace list.
    let ws_metrics = workspace_list_scroll_metrics(app, ws_area);
    let ws_body = workspace_list_body_rect(ws_area, should_show_scrollbar(ws_metrics));
    if ws_body != Rect::default() {
        let entries = workspace_list_entries(app);
        let win = scrolled_window(
            entries.len(),
            ws_metrics.viewport_rows,
            app.workspace_scroll,
        );
        let state_of = |i: usize| entry_state(app, &entries, i);
        let above = side_above(win, state_of);
        let below = side_below(win, entries.len(), state_of);
        rects.expanded_ws_above = place_row_badge(ws_body.x, ws_body.y, ws_body.width, above);
        rects.expanded_ws_below = place_row_badge(
            ws_body.x,
            ws_body.y + ws_body.height.saturating_sub(1),
            ws_body.width,
            below,
        );
    }

    // Expanded agent panel.
    let ag_metrics = agent_panel_scroll_metrics(app, detail_area);
    let ag_body = agent_panel_body_rect(detail_area, should_show_scrollbar(ag_metrics));
    if ag_body != Rect::default() {
        let entries = agent_panel_entries(app);
        let win = scrolled_window(
            entries.len(),
            ag_metrics.viewport_rows,
            app.agent_panel_scroll,
        );
        let state_of = |i: usize| {
            entries
                .get(i)
                .map(|e| (e.state, e.seen))
                .unwrap_or((AgentState::Idle, true))
        };
        let above = side_above(win, state_of);
        let below = side_below(win, entries.len(), state_of);
        rects.expanded_agents_above = place_row_badge(ag_body.x, ag_body.y, ag_body.width, above);
        rects.expanded_agents_below = place_row_badge(
            ag_body.x,
            ag_body.y + ag_body.height.saturating_sub(1),
            ag_body.width,
            below,
        );
    }

    rects
}

/// Render an overflow badge onto its pre-computed rect: a dim `+N` count plus an
/// accent `●ᵏ` attention badge, right-aligned within the row. No-op for an
/// inactive (zero-area / empty) rect.
fn render_overflow_badge(
    frame: &mut Frame,
    badge: super::overflow::OverflowBadgeRect,
    p: &crate::app::state::Palette,
) {
    if !badge.is_active() {
        return;
    }
    let spans = super::overflow::badge_spans(badge.side, p);
    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Right),
        badge.rect,
    );
}

/// Attention predicate for a workspace-list entry by index (resolves the
/// underlying workspace's aggregate state).
/// The `(state, seen)` tuple for a workspace-list entry, used to classify it
/// into the overflow badge buckets. Idle-seen for an entry that has no
/// workspace (group header / out of range), so it never counts as badge-worthy.
fn entry_state(app: &AppState, entries: &[WorkspaceListEntry], idx: usize) -> (AgentState, bool) {
    match entries.get(idx) {
        Some(WorkspaceListEntry::Workspace { ws_idx, .. }) => app
            .workspaces
            .get(*ws_idx)
            .map(|ws| ws.aggregate_state(&app.terminals))
            .unwrap_or((AgentState::Idle, true)),
        _ => (AgentState::Idle, true),
    }
}

/// Collapsed sidebar: workspace glance on top, compact agent list below.
/// Renders button-like rows with full-row background on active/selected, a
/// leading 1-col attention marker, the row number, and a trailing state icon.
pub(super) fn render_sidebar_collapsed(app: &AppState, frame: &mut Frame, area: Rect) {
    let is_navigating = matches!(app.mode, Mode::Navigate);

    let p = &app.palette;
    let sep_style = if is_navigating {
        Style::default().fg(p.accent)
    } else {
        Style::default().fg(p.surface_dim)
    };
    let sep_x = area.x + area.width.saturating_sub(1);
    let buf = frame.buffer_mut();
    for y in area.y..area.y + area.height {
        buf[(sep_x, y)].set_symbol("│");
        buf[(sep_x, y)].set_style(sep_style);
    }

    let layout = compute_collapsed_rail_layout(area);
    if layout.ws_area == Rect::default() {
        render_sidebar_toggle(app, frame, area, true, p);
        return;
    }

    let content_w = layout.ws_area.width;

    // Stateless anchored window: the selected/active workspace stays visible and
    // overflow rows count hidden spaces (FR8). Indicator rows are drawn from the
    // pre-computed `sidebar_overflow` rects so render-rect == hit-rect.
    let ws_win = collapsed_ws_window(app, layout.ws_area);
    let ws_above = app.view.sidebar_overflow.collapsed_ws_above;
    let ws_below = app.view.sidebar_overflow.collapsed_ws_below;
    let ws_first_row = layout.ws_area.y + u16::from(ws_above.is_active());

    for slot in 0..ws_win.count {
        let visible_idx = ws_win.first + slot;
        let Some(ws) = app.workspaces.get(visible_idx) else {
            break;
        };
        let y = ws_first_row + slot as u16;
        if y >= layout.ws_area.y + layout.ws_area.height {
            break;
        }
        let (agg_state, agg_seen) = ws.aggregate_state(&app.terminals);
        let has_attention = is_attention_state(agg_state, agg_seen);
        let (dot, dot_style) = state_dot(agg_state, agg_seen, p);
        let is_selected = visible_idx == app.selected && is_navigating;
        let is_active = Some(visible_idx) == app.active;

        let row_bg = if is_selected {
            Some(p.surface0)
        } else if is_active {
            Some(p.surface_dim)
        } else {
            None
        };

        if let Some(bg) = row_bg {
            let buf = frame.buffer_mut();
            for x in layout.ws_area.x..layout.ws_area.x + content_w {
                buf[(x, y)].set_style(Style::default().bg(bg));
            }
        }

        let num_style = match row_bg {
            Some(bg) if is_selected => Style::default()
                .fg(p.text)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
            Some(bg) => Style::default().fg(p.text).bg(bg),
            None => Style::default().fg(p.overlay0),
        };

        let marker = if has_attention { "●" } else { " " };
        let marker_style = if has_attention {
            Style::default().fg(p.accent)
        } else {
            Style::default()
        };

        // marker | pad | num | pad | dot, left-aligned; Paragraph clips to content_w.
        let spans = vec![
            Span::styled(marker, marker_style),
            Span::styled(" ", Style::default()),
            Span::styled(format!("{}", visible_idx + 1), num_style),
            Span::styled(" ", Style::default()),
            Span::styled(dot, dot_style),
        ];

        frame.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect::new(layout.ws_area.x, y, content_w, 1),
        );
    }

    render_overflow_badge(frame, ws_above, p);
    render_overflow_badge(frame, ws_below, p);

    if let Some(divider_y) = layout.divider_y {
        let buf = frame.buffer_mut();
        for x in layout.ws_area.x..layout.ws_area.x + content_w {
            buf[(x, divider_y)].set_symbol("─");
            buf[(x, divider_y)].set_style(Style::default().fg(p.surface_dim));
        }
    }

    let detail_content_area = collapsed_detail_content_area(layout.detail_area);
    let detail_above = app.view.sidebar_overflow.collapsed_detail_above;
    let detail_below = app.view.sidebar_overflow.collapsed_detail_below;
    if detail_content_area != Rect::default() {
        if let Some((ws_idx, details, anchor)) = collapsed_detail_details(app) {
            if let Some(ws) = app.workspaces.get(ws_idx) {
                let win = super::overflow::anchored_window(
                    details.len(),
                    detail_content_area.height as usize,
                    anchor,
                );
                let first_row = detail_content_area.y + u16::from(detail_above.is_active());
                for slot in 0..win.count {
                    let detail_idx = win.first + slot;
                    let Some(detail) = details.get(detail_idx) else {
                        break;
                    };
                    let y = first_row + slot as u16;
                    if y >= detail_content_area.y + detail_content_area.height {
                        break;
                    }
                    let pane_num = ws
                        .public_pane_number(detail.pane_id)
                        .unwrap_or(detail_idx + 1);
                    let has_attention = is_attention_state(detail.state, detail.seen);

                    let row_bg = if has_attention {
                        Some(p.surface_dim)
                    } else {
                        None
                    };

                    if let Some(bg) = row_bg {
                        let buf = frame.buffer_mut();
                        for x in detail_content_area.x..detail_content_area.x + content_w {
                            buf[(x, y)].set_style(Style::default().bg(bg));
                        }
                    }

                    let pane_style = if has_attention {
                        Style::default().fg(p.text)
                    } else {
                        Style::default().fg(p.overlay0)
                    };
                    let (icon, icon_style) =
                        agent_icon(detail.state, detail.seen, app.spinner_tick, p);

                    let marker = if has_attention { "●" } else { " " };
                    let marker_style = if has_attention {
                        Style::default().fg(p.accent)
                    } else {
                        Style::default()
                    };

                    let spans = vec![
                        Span::styled(marker, marker_style),
                        Span::styled(" ", Style::default()),
                        Span::styled(format!("{pane_num}"), pane_style),
                        Span::styled(" ", Style::default()),
                        Span::styled(icon, icon_style),
                    ];

                    frame.render_widget(
                        Paragraph::new(Line::from(spans)),
                        Rect::new(detail_content_area.x, y, content_w, 1),
                    );
                }

                render_overflow_badge(frame, detail_above, p);
                render_overflow_badge(frame, detail_below, p);
            }
        }
    }

    render_sidebar_toggle(app, frame, area, true, p);
}

/// Row carrying the edge-based drop indicator for `insert_idx` (FR4). The
/// insertion boundary sits at the returned row's bottom edge, so the render
/// overlays an underline on that row instead of drawing into a gap row:
/// insert-before-k underlines the row above card k (the previous card, or a
/// section header row for the top slot) and insert-after-last underlines the
/// last card's own row. Geometry never changes during drag.
pub(crate) fn workspace_drop_indicator_row(
    cards: &[crate::app::state::WorkspaceCardArea],
    area: Rect,
    insert_idx: usize,
) -> Option<u16> {
    if area.height == 0 {
        return None;
    }
    let list_bottom = area.y + area.height.saturating_sub(1);

    let first = cards.first()?;
    if insert_idx == first.ws_idx {
        return first.rect.y.checked_sub(1).filter(|y| *y < list_bottom);
    }

    if let Some(row) = cards
        .last()
        .filter(|card| insert_idx == card.ws_idx.saturating_add(1))
        .map(|card| card.rect.y)
        .filter(|y| *y < list_bottom)
    {
        return Some(row);
    }

    if let Some(card) = cards.iter().find(|card| card.ws_idx == insert_idx) {
        return card.rect.y.checked_sub(1).filter(|y| *y < list_bottom);
    }

    None
}

pub(super) fn render_sidebar(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    frame: &mut Frame,
    area: Rect,
) {
    let p = &app.palette;
    let is_navigating = matches!(app.mode, Mode::Navigate);
    let sep_style = if is_navigating {
        Style::default().fg(p.accent)
    } else {
        Style::default().fg(p.surface_dim)
    };

    let sep_x = area.x + area.width.saturating_sub(1);
    let buf = frame.buffer_mut();
    for y in area.y..area.y + area.height {
        buf[(sep_x, y)].set_symbol("│");
        buf[(sep_x, y)].set_style(sep_style);
    }

    let layout = compute_expanded_sidebar_layout(area, app.sidebar_section_split);

    render_workspace_list(app, terminal_runtimes, frame, layout.spaces, is_navigating);
    render_agent_detail(app, terminal_runtimes, frame, layout.detail);

    // Attention-aware overflow badges (FR8) overlaid on the first/last body row
    // of each expanded section, from the pre-computed rects (render == hit-test).
    let ov = &app.view.sidebar_overflow;
    render_overflow_badge(frame, ov.expanded_ws_above, p);
    render_overflow_badge(frame, ov.expanded_ws_below, p);
    render_overflow_badge(frame, ov.expanded_agents_above, p);
    render_overflow_badge(frame, ov.expanded_agents_below, p);

    render_sidebar_toggle(app, frame, area, false, p);
}

fn render_workspace_list(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    frame: &mut Frame,
    area: Rect,
    is_navigating: bool,
) {
    let p = &app.palette;
    let dragged_ws_idx = match app.drag.as_ref().map(|drag| &drag.target) {
        Some(crate::app::state::DragTarget::WorkspaceReorder { source_ws_idx, .. }) => {
            Some(*source_ws_idx)
        }
        _ => None,
    };
    let insertion_row = match app.drag.as_ref().map(|drag| &drag.target) {
        Some(crate::app::state::DragTarget::WorkspaceReorder {
            insert_idx: Some(insert_idx),
            ..
        }) => workspace_drop_indicator_row(&app.view.workspace_card_areas, area, *insert_idx),
        _ => None,
    };

    let list_bottom = area.y + area.height.saturating_sub(1);
    if area.height > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                " spaces",
                Style::default().fg(p.overlay0).add_modifier(Modifier::BOLD),
            )])),
            Rect::new(area.x, area.y, area.width, 1),
        );
    }

    let metrics = workspace_list_scroll_metrics(app, area);
    let scrollbar_rect = workspace_list_scrollbar_rect(app, area);
    let cards = &app.view.workspace_card_areas;

    for card in cards {
        let i = card.ws_idx;
        let ws = &app.workspaces[i];
        let row_y = card.rect.y;
        let row_height = card.rect.height;
        let selected = i == app.selected && is_navigating;
        let is_active = Some(i) == app.active;
        let is_dragged = dragged_ws_idx == Some(i);
        let highlighted = selected || is_active || is_dragged;
        let (agg_state, agg_seen) = ws.aggregate_state(&app.terminals);

        if highlighted {
            let bg = if selected {
                p.surface0
            } else if is_dragged {
                p.surface1
            } else {
                p.surface_dim
            };
            let buf = frame.buffer_mut();
            for y in row_y..row_y + row_height {
                if y >= list_bottom {
                    break;
                }
                for x in card.rect.x..card.rect.x + card.rect.width {
                    buf[(x, y)].set_style(Style::default().bg(bg));
                }
            }
        }

        let name_style = if selected || is_active || is_dragged {
            Style::default().fg(p.text).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.subtext0)
        };

        let (icon, icon_style) = state_dot(agg_state, agg_seen, p);
        let label = ws.display_name_from(&app.terminals, terminal_runtimes);
        let mut line1 = Vec::new();
        let mut show_workspace_icon = true;
        if card.indented {
            line1.push(Span::styled("   ", Style::default()));
        } else if let Some((key, collapsed)) = workspace_parent_group_state(app, i) {
            let icon = if collapsed { "▸" } else { "▾" };
            let (state_icon, state_style) = if collapsed {
                let (state, seen) = space_aggregate_state(app, &key);
                state_dot(state, seen, p)
            } else {
                (icon, Style::default().fg(p.accent))
            };
            line1.push(Span::styled(icon, Style::default().fg(p.accent)));
            if collapsed {
                line1.push(Span::styled(" ", Style::default()));
                line1.push(Span::styled(state_icon, state_style));
                show_workspace_icon = false;
            }
            line1.push(Span::styled(" ", Style::default()));
        } else {
            line1.push(Span::styled(" ", Style::default()));
        }
        if show_workspace_icon {
            line1.push(Span::styled(icon, icon_style));
            line1.push(Span::styled(" ", Style::default()));
        }
        if card.indented {
            let display_label = grouped_child_display_label(
                &label,
                ws.branch().as_deref(),
                ws.custom_name.is_some(),
            );
            line1.push(Span::styled(display_label, name_style));
        } else {
            let upstream_parts = ws.git_ahead_behind().and_then(|(ahead, behind)| {
                let mut parts = Vec::new();
                if ahead > 0 {
                    parts.push((format!("↑{}", ahead), p.green));
                }
                if behind > 0 {
                    parts.push((format!("↓{}", behind), p.red));
                }
                (!parts.is_empty()).then_some(parts)
            });
            let counts_width = upstream_parts
                .as_ref()
                .map(|parts| {
                    parts
                        .iter()
                        .map(|(label, _)| label.chars().count())
                        .sum::<usize>()
                        + parts.len()
                })
                .unwrap_or(0);
            let prefix_width: usize = line1.iter().map(|span| span.content.chars().count()).sum();
            let available = (card.rect.width as usize).saturating_sub(prefix_width);
            let (name, branch_display, show_counts) =
                space_line_parts(&label, ws.branch().as_deref(), counts_width, available);
            line1.push(Span::styled(name, name_style));
            if let Some(branch_display) = branch_display {
                let branch_color = if selected || is_active {
                    p.mauve
                } else {
                    p.overlay0
                };
                line1.push(Span::styled(" ", Style::default()));
                line1.push(Span::styled(
                    branch_display,
                    Style::default().fg(branch_color),
                ));
            }
            if show_counts {
                if let Some(parts) = upstream_parts {
                    for (label, color) in parts {
                        line1.push(Span::styled(" ", Style::default()));
                        line1.push(Span::styled(label, Style::default().fg(color)));
                    }
                }
            }
        }

        frame.render_widget(
            Paragraph::new(Line::from(line1)),
            Rect::new(card.rect.x, row_y, card.rect.width, 1),
        );
    }

    // Edge-based drop indicator (FR4): underline the row whose bottom edge is
    // the insertion boundary instead of drawing a `─` line into a gap row, so
    // list geometry never changes during drag.
    if let Some(y) = insertion_row.filter(|y| *y < list_bottom) {
        let indicator_right = scrollbar_rect
            .map(|rect| rect.x)
            .unwrap_or(area.x + area.width);
        let buf = frame.buffer_mut();
        for x in area.x..indicator_right {
            buf[(x, y)].set_style(
                Style::default()
                    .fg(p.accent)
                    .add_modifier(Modifier::UNDERLINED),
            );
        }
    }

    if let Some(track) = scrollbar_rect {
        render_scrollbar(
            frame,
            metrics,
            track,
            p.surface_dim,
            p.overlay0,
            SCROLLBAR_THUMB,
        );
    }

    if app.mouse_capture && list_bottom > area.y {
        let new_rect = app.sidebar_new_button_rect();
        frame.render_widget(
            Paragraph::new(Span::styled(" new", Style::default().fg(p.overlay0))),
            new_rect,
        );

        let menu_rect = app.global_launcher_rect();
        let menu_line = if app.global_menu_attention_badge_visible() {
            Line::from(vec![
                Span::styled(
                    "● ",
                    Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled("menu", Style::default().fg(p.overlay0)),
            ])
        } else {
            Line::from(vec![Span::styled("menu", Style::default().fg(p.overlay0))])
        };
        frame.render_widget(
            Paragraph::new(menu_line).alignment(Alignment::Right),
            menu_rect,
        );
    }
}

/// Agent item line 1: leading space, state icon, then the primary label.
/// Pure span producer — no Frame/AppState.
pub(crate) fn agent_panel_name_spans(
    detail: &AgentPanelEntry,
    is_active: bool,
    spinner_tick: u32,
    max_label_width: usize,
    p: &Palette,
) -> Vec<Span<'static>> {
    let (icon, icon_style) = agent_icon(detail.state, detail.seen, spinner_tick, p);
    let name_style = if is_active {
        Style::default().fg(p.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(p.subtext0).add_modifier(Modifier::BOLD)
    };
    let primary_label = format_agent_panel_primary_label(detail, max_label_width);
    vec![
        Span::styled(" ", Style::default()),
        Span::styled(icon, icon_style),
        Span::styled(" ", Style::default()),
        Span::styled(primary_label, name_style),
    ]
}

/// Agent item line 2 (FR6): `state_label · agent_label · custom_status` with
/// the panel's dim styling and `state_labels` overrides. Pure span producer.
pub(crate) fn agent_panel_status_spans(
    detail: &AgentPanelEntry,
    is_active: bool,
    p: &Palette,
) -> Vec<Span<'static>> {
    let label_color = state_label_color(detail.state, detail.seen, p);
    let label = detail
        .state_labels
        .get(agent_panel_status_key(detail.state, detail.seen))
        .map(String::as_str)
        .unwrap_or_else(|| state_label(detail.state, detail.seen));
    let status_style = if is_active {
        Style::default().fg(label_color)
    } else {
        Style::default().fg(label_color).add_modifier(Modifier::DIM)
    };
    let agent_style = Style::default().fg(p.overlay0).add_modifier(Modifier::DIM);

    let mut spans = vec![
        Span::styled("   ", Style::default()),
        Span::styled(label.to_string(), status_style),
    ];
    if let Some(agent_label) = &detail.agent_label {
        spans.push(Span::styled(AGENT_LABEL_SEPARATOR, agent_style));
        spans.push(Span::styled(agent_label.clone(), agent_style));
    }
    if let Some(custom_status) = &detail.custom_status {
        spans.push(Span::styled(AGENT_LABEL_SEPARATOR, agent_style));
        spans.push(Span::styled(custom_status.clone(), agent_style));
    }
    spans
}

fn render_agent_detail(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    frame: &mut Frame,
    area: Rect,
) {
    let p = &app.palette;

    if area.height < 3 {
        return;
    }

    let sep_line = "─".repeat(area.width as usize);
    frame.render_widget(
        Paragraph::new(Span::styled(&sep_line, Style::default().fg(p.surface_dim))),
        Rect::new(area.x, area.y, area.width, 1),
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            " agents",
            Style::default().fg(p.overlay0).add_modifier(Modifier::BOLD),
        )])),
        Rect::new(area.x, area.y + 1, area.width, 1),
    );
    let toggle_rect = agent_panel_toggle_rect(area, app.agent_panel_sort);
    if toggle_rect != Rect::default() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                agent_panel_sort_label(app.agent_panel_sort),
                Style::default().fg(p.overlay0).add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Right),
            toggle_rect,
        );
    }

    let details = agent_panel_entries_from(app, terminal_runtimes);
    let metrics = agent_panel_scroll_metrics(app, area);
    let scrollbar_rect = agent_panel_scrollbar_rect(app, area);
    let body = agent_panel_body_rect(area, should_show_scrollbar(metrics));
    if body == Rect::default() {
        return;
    }

    for (entry_idx, row_y) in agent_panel_item_rows(body, app.agent_panel_scroll, details.len()) {
        let detail = &details[entry_idx];

        // Check if this agent entry corresponds to the active session
        let is_active = app.is_active_pane(detail.ws_idx, detail.tab_idx, detail.pane_id);
        let row_style = if is_active {
            Style::default().bg(p.surface_dim)
        } else {
            Style::default()
        };

        let name_line = Line::from(agent_panel_name_spans(
            detail,
            is_active,
            app.spinner_tick,
            body.width.saturating_sub(3) as usize,
            p,
        ));
        frame.render_widget(
            Paragraph::new(name_line).style(row_style),
            Rect::new(body.x, row_y, body.width, 1),
        );

        let status_line = Line::from(agent_panel_status_spans(detail, is_active, p));
        frame.render_widget(
            Paragraph::new(status_line).style(row_style),
            Rect::new(body.x, row_y + 1, body.width, 1),
        );
    }

    if let Some(track) = scrollbar_rect {
        render_scrollbar(
            frame,
            metrics,
            track,
            p.surface_dim,
            p.overlay0,
            SCROLLBAR_THUMB,
        );
    }
}

pub(crate) fn collapsed_sidebar_toggle_rect(area: Rect) -> Rect {
    let bottom_y = area.y + area.height.saturating_sub(1);
    let content_w = area.width.saturating_sub(1);
    if content_w == 0 || area.height == 0 {
        return Rect::default();
    }
    let x = area.x + content_w / 2;
    Rect::new(x, bottom_y, 1, 1)
}

pub(crate) fn expanded_sidebar_toggle_rect(area: Rect) -> Rect {
    if area.width <= 1 || area.height == 0 {
        return Rect::default();
    }
    Rect::new(
        area.x + area.width.saturating_sub(2),
        area.y + area.height.saturating_sub(1),
        1,
        1,
    )
}

fn render_sidebar_toggle(
    app: &AppState,
    frame: &mut Frame,
    area: Rect,
    collapsed: bool,
    p: &Palette,
) {
    let toggle_area = if collapsed {
        collapsed_sidebar_toggle_rect(area)
    } else {
        expanded_sidebar_toggle_rect(area)
    };
    if toggle_area == Rect::default() {
        return;
    }
    let icon = if collapsed { "»" } else { "«" };
    let icon_style = if collapsed && app.global_menu_attention_badge_visible() {
        Style::default().fg(p.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(p.overlay0)
    };
    frame.render_widget(Paragraph::new(Span::styled(icon, icon_style)), toggle_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{detect::Agent, workspace::Workspace};
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn render_sidebar_toggle_draws_expanded_collapse_icon() {
        let app = crate::app::state::AppState::test_new();
        let area = Rect::new(0, 0, 26, 20);
        let mut terminal =
            Terminal::new(TestBackend::new(26, 20)).expect("test terminal should initialize");

        terminal
            .draw(|frame| render_sidebar_toggle(&app, frame, area, false, &app.palette))
            .expect("sidebar toggle should render");

        let toggle = expanded_sidebar_toggle_rect(area);
        assert_eq!(
            terminal.backend().buffer()[(toggle.x, toggle.y)].symbol(),
            "«"
        );
    }

    #[test]
    fn expanded_sidebar_toggle_sits_inside_sidebar_content() {
        let area = Rect::new(0, 0, 26, 20);
        let toggle = expanded_sidebar_toggle_rect(area);

        assert_eq!(toggle.x, area.x + area.width - 2);
        assert_eq!(toggle.y, area.y + area.height - 1);
    }

    #[test]
    fn all_workspaces_agent_panel_entries_use_workspace_and_optional_tab_labels() {
        let mut app = crate::app::state::AppState::test_new();
        let first = Workspace::test_new("one");
        let first_pane = first.tabs[0].root_pane;
        let mut second = Workspace::test_new("two");
        let second_tab = second.test_add_tab(Some("logs"));
        let second_pane = second.tabs[second_tab].root_pane;

        app.workspaces = vec![first, second];
        app.ensure_test_terminals();
        let first_terminal_id = app.workspaces[0].tabs[0].panes[&first_pane]
            .attached_terminal_id
            .clone();
        app.terminals
            .get_mut(&first_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Pi);
        let second_terminal_id = app.workspaces[1].tabs[second_tab].panes[&second_pane]
            .attached_terminal_id
            .clone();
        app.terminals
            .get_mut(&second_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Claude);
        app.active = Some(0);
        app.selected = 0;

        let entries = agent_panel_entries(&app);
        assert_eq!(entries[0].primary_label, "one");
        assert!(entries[0].primary_tab_label.is_none());
        assert_eq!(entries[0].agent_label.as_deref(), Some("pi"));
        assert_eq!(entries[1].primary_label, "two");
        assert_eq!(entries[1].primary_tab_label.as_deref(), Some("logs"));
        assert_eq!(entries[1].agent_label.as_deref(), Some("claude"));
    }

    #[test]
    fn priority_agent_panel_sort_uses_attention_then_space_order() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![
            Workspace::test_new("one"),
            Workspace::test_new("two"),
            Workspace::test_new("three"),
            Workspace::test_new("four"),
        ];
        app.ensure_test_terminals();
        app.active = Some(0);
        app.selected = 0;
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Priority;

        let set_state = |app: &mut crate::app::state::AppState, ws_idx: usize, state| {
            let pane = app.workspaces[ws_idx].tabs[0].root_pane;
            let terminal_id = app.workspaces[ws_idx].tabs[0].panes[&pane]
                .attached_terminal_id
                .clone();
            let terminal = app.terminals.get_mut(&terminal_id).unwrap();
            terminal.detected_agent = Some(Agent::Claude);
            terminal.state = state;
        };
        set_state(&mut app, 0, AgentState::Working);
        set_state(&mut app, 1, AgentState::Idle);
        set_state(&mut app, 2, AgentState::Working);
        set_state(&mut app, 3, AgentState::Blocked);

        let done_pane = app.workspaces[1].tabs[0].root_pane;
        app.workspaces[1].tabs[0]
            .panes
            .get_mut(&done_pane)
            .unwrap()
            .seen = false;

        let labels: Vec<String> = agent_panel_entries(&app)
            .into_iter()
            .map(|entry| entry.primary_label)
            .collect();

        assert_eq!(labels, ["four", "two", "one", "three"]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn all_workspaces_agent_panel_entries_use_live_root_runtime_cwd_for_workspace_label() {
        let unique = format!(
            "herdr-agent-panel-runtime-cwd-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let stale_cwd = root.join("issue-264-nix-support");
        let live_cwd = root.join("herdr");
        std::fs::create_dir_all(stale_cwd.join(".git")).unwrap();
        std::fs::create_dir_all(live_cwd.join(".git")).unwrap();

        let mut app = crate::app::state::AppState::test_new();
        let mut workspace = Workspace::test_new("stale-name");
        workspace.custom_name = None;
        workspace.identity_cwd = stale_cwd.clone();
        let pane = workspace.tabs[0].root_pane;

        app.workspaces = vec![workspace];
        app.ensure_test_terminals();
        let terminal_id = app.workspaces[0].tabs[0].panes[&pane]
            .attached_terminal_id
            .clone();
        let terminal = app.terminals.get_mut(&terminal_id).unwrap();
        terminal.cwd = stale_cwd;
        terminal.detected_agent = Some(Agent::Pi);
        app.active = Some(0);
        app.selected = 0;

        let (events, _) = tokio::sync::mpsc::channel(4);
        let runtime = crate::terminal::TerminalRuntime::spawn(
            pane,
            24,
            80,
            live_cwd.clone(),
            0,
            crate::terminal_theme::TerminalTheme::default(),
            crate::pane::PaneShellConfig::new("/bin/sh", crate::config::ShellModeConfig::NonLogin),
            &crate::pane::PaneLaunchEnv::default(),
            events,
            std::sync::Arc::new(tokio::sync::Notify::new()),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while runtime.cwd() != Some(live_cwd.clone()) && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let mut runtime_registry = TerminalRuntimeRegistry::new();
        runtime_registry.insert(terminal_id, runtime);
        let entries = agent_panel_entries_from(&app, &runtime_registry);
        let primary_label = entries[0].primary_label.clone();

        for (_, runtime) in runtime_registry.drain() {
            runtime.shutdown();
        }
        let _ = std::fs::remove_dir_all(root);

        assert_eq!(primary_label, "herdr");
    }

    #[test]
    fn all_workspaces_agent_panel_entries_prefer_agent_names_for_agent_identity() {
        let mut app = crate::app::state::AppState::test_new();
        let workspace = Workspace::test_new("bridge");
        let first_pane = workspace.tabs[0].root_pane;

        app.workspaces = vec![workspace];
        app.ensure_test_terminals();
        let first_terminal_id = app.workspaces[0].tabs[0].panes[&first_pane]
            .attached_terminal_id
            .clone();
        app.terminals
            .get_mut(&first_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Pi);
        app.terminals
            .get_mut(&first_terminal_id)
            .unwrap()
            .set_agent_name("planner".into());
        app.active = Some(0);
        app.selected = 0;

        let entries = agent_panel_entries(&app);
        assert_eq!(entries[0].primary_label, "bridge");
        assert_eq!(entries[0].agent_label.as_deref(), Some("planner"));
    }

    fn panel_entry(primary_label: &str, primary_tab_label: Option<&str>) -> AgentPanelEntry {
        AgentPanelEntry {
            ws_idx: 0,
            tab_idx: 0,
            pane_id: crate::layout::PaneId::from_raw(1),
            primary_label: primary_label.into(),
            primary_tab_label: primary_tab_label.map(str::to_string),
            agent_label: Some("claude".into()),
            state: AgentState::Idle,
            seen: true,
            last_agent_state_change_seq: None,
            custom_status: None,
            state_labels: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn all_workspaces_primary_label_truncates_workspace_and_tab() {
        let entry = panel_entry("agent-browser", Some("test-escalation"));

        let label = format_agent_panel_primary_label(&entry, 18);

        assert_eq!(label, "agent-bro… · test…");
    }

    fn spans_text(spans: &[Span<'_>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn agent_status_spans_render_state_agent_and_custom_status() {
        let p = Palette::catppuccin();
        let mut entry = panel_entry("herdr", None);
        entry.custom_status = Some("compiling".into());

        let spans = agent_panel_status_spans(&entry, false, &p);

        assert_eq!(spans_text(&spans), "   idle · claude · compiling");
        // Non-active rows keep the dim styling on every non-indent span.
        assert!(spans[1..]
            .iter()
            .all(|s| s.style.add_modifier.contains(Modifier::DIM)));
    }

    #[test]
    fn agent_status_spans_omit_missing_segments() {
        let p = Palette::catppuccin();
        let mut entry = panel_entry("herdr", None);
        entry.agent_label = None;

        assert_eq!(
            spans_text(&agent_panel_status_spans(&entry, false, &p)),
            "   idle"
        );
    }

    #[test]
    fn agent_status_spans_honor_state_labels_override() {
        let p = Palette::catppuccin();
        let mut entry = panel_entry("herdr", None);
        entry.state_labels.insert("idle".into(), "resting".into());

        assert_eq!(
            spans_text(&agent_panel_status_spans(&entry, true, &p)),
            "   resting · claude"
        );
    }

    #[test]
    fn agent_name_spans_lead_with_icon_then_primary_label() {
        let p = Palette::catppuccin();
        let entry = panel_entry("herdr", Some("logs"));

        let spans = agent_panel_name_spans(&entry, false, 0, 40, &p);

        assert_eq!(spans_text(&spans), " ✓ herdr · logs");
    }

    #[test]
    fn agent_panel_item_rows_maps_entries_and_agrees_with_consumers() {
        // Table: (body height, scroll, entry_count, expected (entry, row_y)
        // pairs relative to body.y). Covers scrolled and partially-clipped
        // viewports (odd heights leave the trailing row unused) per AC5.
        type Case = (u16, usize, usize, Vec<(usize, u16)>);
        let cases: Vec<Case> = vec![
            // Everything fits.
            (6, 0, 2, vec![(0, 0), (1, 2)]),
            // Odd height: trailing row unused, third item clipped out.
            (7, 0, 4, vec![(0, 0), (1, 2), (2, 4)]),
            // Scrolled: indices offset, rows re-anchored at body top.
            (6, 1, 4, vec![(1, 0), (2, 2), (3, 4)]),
            // Scrolled past most entries: only the remainder shows.
            (6, 3, 4, vec![(3, 0)]),
            // Scroll beyond the list: nothing.
            (6, 5, 4, vec![]),
            // More entries than fit.
            (4, 0, 10, vec![(0, 0), (1, 2)]),
            // Tiny heights: no items.
            (1, 0, 3, vec![]),
            (0, 0, 3, vec![]),
        ];

        for (height, scroll, entry_count, expected) in cases {
            let body = Rect::new(0, 13, 24, height);
            let items: Vec<(usize, u16)> = agent_panel_item_rows(body, scroll, entry_count)
                .map(|(idx, row_y)| (idx, row_y - body.y))
                .collect();
            assert_eq!(
                items, expected,
                "height={height} scroll={scroll} entries={entry_count}"
            );

            // Hit-test agreement: each item owns exactly its two rows.
            for (idx, rel_row) in &items {
                for offset in 0..AGENT_PANEL_ITEM_ROWS {
                    let row = body.y + rel_row + offset;
                    let hit = agent_panel_item_rows(body, scroll, entry_count)
                        .find(|(_, row_y)| row == *row_y || row == row_y + 1)
                        .map(|(i, _)| i);
                    assert_eq!(hit, Some(*idx), "row {row} must hit entry {idx}");
                }
            }
        }

        // Zero width: no items regardless of height.
        assert_eq!(
            agent_panel_item_rows(Rect::new(0, 0, 0, 10), 0, 5).count(),
            0
        );
    }

    #[test]
    fn agent_panel_visible_count_matches_item_rows_capacity() {
        // The metric and the shared helper must agree on how many items fit.
        for height in 0u16..12 {
            let area = Rect::new(0, 0, 24, height + AGENT_PANEL_HEADER_ROWS);
            let body = agent_panel_body_rect(area, false);
            assert_eq!(
                agent_panel_visible_count(area),
                agent_panel_item_rows(body, 0, usize::MAX).count(),
                "height={height}"
            );
        }
    }

    #[test]
    fn agent_panel_fits_eight_items_at_reference_geometry() {
        // Design success check: 40-row sidebar at the default 0.5 split gives
        // a 20-row detail section; 17 body rows fit 8 agents.
        let area = Rect::new(0, 0, 26, 40);
        let (_, detail_area) = expanded_sidebar_sections(area, 0.5);
        assert_eq!(detail_area.height, 20);
        assert_eq!(agent_panel_visible_count(detail_area), 8);
    }

    #[test]
    fn expanded_sidebar_sections_handle_tiny_heights() {
        let (ws_area, detail_area) = expanded_sidebar_sections(Rect::new(0, 0, 20, 5), 0.9);

        assert_eq!(ws_area, Rect::new(0, 0, 19, 3));
        assert_eq!(detail_area, Rect::new(0, 3, 19, 2));
    }

    #[test]
    fn sidebar_section_divider_is_hidden_for_tiny_heights() {
        let divider = sidebar_section_divider_rect(Rect::new(0, 0, 20, 5), 0.5);

        assert_eq!(divider, Rect::default());
    }

    #[test]
    fn grouped_child_label_keeps_custom_workspace_name() {
        assert_eq!(
            grouped_child_display_label("renamed issue", Some("worktree/issue-137"), true),
            "renamed issue"
        );
    }

    #[test]
    fn grouped_child_label_uses_short_branch_for_auto_named_workspace() {
        assert_eq!(
            grouped_child_display_label("herdr-issue", Some("worktree/issue-137"), false),
            "issue-137"
        );
    }

    fn workspace_with_worktree_space(
        name: &str,
        key: Option<&str>,
        checkout_key: &str,
    ) -> crate::workspace::Workspace {
        let mut ws = crate::workspace::Workspace::test_new(name);
        if let Some(key) = key {
            ws.worktree_space = Some(crate::workspace::WorktreeSpaceMembership {
                key: key.into(),
                label: "herdr".into(),
                repo_root: std::path::PathBuf::from("/repo/herdr"),
                checkout_path: std::path::PathBuf::from(checkout_key),
                is_linked_worktree: name != "main",
            });
        }
        ws
    }

    fn workspace_with_git_space(name: &str, key: &str) -> crate::workspace::Workspace {
        let mut ws = crate::workspace::Workspace::test_new(name);
        ws.cached_git_space = Some(crate::workspace::GitSpaceMetadata {
            key: key.into(),
            checkout_key: format!("/repo/{name}"),
            label: "herdr".into(),
            repo_root: std::path::PathBuf::from(format!("/repo/{name}")),
            is_linked_worktree: false,
        });
        ws
    }

    #[test]
    fn parent_workspace_row_stays_clickable_when_grouped() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
        ];

        let (cards, headers) = compute_workspace_list_areas(&app, Rect::new(0, 0, 30, 20));

        assert!(headers.is_empty());
        assert_eq!(cards[0].ws_idx, 0);
        assert!(!cards[0].indented);
        assert_eq!(cards[0].rect.height, 1);
        assert_eq!(cards[1].ws_idx, 1);
        assert!(cards[1].indented);
        // No gap row: the child is vertically adjacent to the parent.
        assert_eq!(cards[1].rect.y, cards[0].rect.y + cards[0].rect.height);
    }

    #[test]
    fn space_line_parts_degrade_in_fr2_priority_order() {
        // name "workspace-name" (14) + " " + branch "feature/long-branch" (19)
        // + " ↑2 ↓13" (7 incl. leading space) = 40 chars full line.
        let name = "workspace-name";
        let branch = "feature/long-branch";
        let counts_width = 7;
        let full = 14 + 1 + 19 + counts_width;

        // Descending width sweep asserting the degradation ORDER: counts
        // dropped first, then branch truncated (min 4 visible chars), then
        // branch dropped, then name truncated. Expected branch truncations go
        // through the same char-safe helper the implementation must use.
        type Case = (usize, (String, Option<String>, bool));
        let cases: Vec<Case> = vec![
            // Everything fits: counts shown.
            (full, (name.into(), Some(branch.into()), true)),
            // One short: counts dropped first, branch intact.
            (full - 1, (name.into(), Some(branch.into()), false)),
            (34, (name.into(), Some(branch.into()), false)),
            // Branch truncates char-safely (budget = width - name - 1 space).
            (33, (name.into(), Some(truncate_text(branch, 18)), false)),
            (20, (name.into(), Some(truncate_text(branch, 5)), false)),
            // Minimum 4 visible chars before the branch drops entirely.
            (19, (name.into(), Some(truncate_text(branch, 4)), false)),
            (18, (name.into(), None, false)),
            // Name survives untruncated as long as it fits.
            (14, (name.into(), None, false)),
            // Finally the name truncates.
            (13, (truncate_text(name, 13), None, false)),
            (1, ("…".into(), None, false)),
        ];
        for (width, expected) in cases {
            let got = space_line_parts(name, Some(branch), counts_width, width);
            assert_eq!(got, expected, "width {width}");
        }
    }

    #[test]
    fn space_line_parts_branchless_has_no_counts_or_branch() {
        let got = space_line_parts("notes", None, 0, 30);
        assert_eq!(got, ("notes".into(), None, false));
    }

    #[test]
    fn space_line_parts_multibyte_branch_never_panics() {
        // Regression: the old byte-slice truncation panicked on multibyte
        // branch names; the char-safe path must survive every width.
        let name = "ws";
        let branch = "функция/дизайн-веток";
        let full = name.chars().count() + 1 + branch.chars().count() + 7;
        for width in 0..=full + 2 {
            let (n, b, _) = space_line_parts(name, Some(branch), 7, width);
            assert!(n.chars().count() <= width.max(1));
            if let Some(b) = b {
                assert!(!b.is_empty());
            }
        }
    }

    #[test]
    fn workspace_cards_are_adjacent_one_row_each() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![
            Workspace::test_new("a"),
            Workspace::test_new("b"),
            Workspace::test_new("c"),
        ];

        let (cards, _) = compute_workspace_list_areas(&app, Rect::new(0, 0, 30, 20));

        assert_eq!(cards.len(), 3);
        for pair in cards.windows(2) {
            assert_eq!(pair[0].rect.height, 1);
            assert_eq!(pair[1].rect.y, pair[0].rect.y + 1, "no gap row");
        }
    }

    #[test]
    fn spaces_body_fits_17_items_at_reference_geometry() {
        // Design success check: 40-row sidebar at the default 0.5 split gives
        // a 20-row spaces section (2 header + 17 body + 1 footer) => 17 items.
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = (0..20)
            .map(|i| Workspace::test_new(&format!("ws{i}")))
            .collect();

        let sidebar = Rect::new(0, 0, 30, 40);
        let ws_area = workspace_list_rect(sidebar, app.sidebar_section_split);
        let metrics = workspace_list_scroll_metrics(&app, ws_area);

        assert_eq!(metrics.viewport_rows, 17);
    }

    #[test]
    fn spaces_body_height_one_still_shows_an_item() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("a")];

        // 4 rows = 2 header + 1 body + 1 footer.
        let metrics = workspace_list_scroll_metrics(&app, Rect::new(0, 0, 30, 4));

        assert_eq!(metrics.viewport_rows, 1);
    }

    #[test]
    fn linked_only_worktree_members_do_not_form_parentless_group() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
            workspace_with_worktree_space("review", Some("repo-key"), "/repo/herdr-review"),
        ];

        let entries = workspace_list_entries(&app);

        assert_eq!(
            entries,
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: false
                },
            ]
        );
    }

    #[test]
    fn compact_space_group_scroll_offset_can_start_inside_group() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("one", Some("repo-key"), "/repo/herdr-one"),
            workspace_with_worktree_space("two", Some("repo-key"), "/repo/herdr-two"),
        ];
        let area = Rect::new(0, 0, 30, 20);
        app.workspace_scroll = normalized_workspace_scroll(&app, area, 2);

        let (cards, headers) = compute_workspace_list_areas(&app, area);

        assert!(headers.is_empty());
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].ws_idx, 2);
    }

    #[test]
    fn workspace_scroll_metrics_count_display_entries_not_raw_workspaces() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
            Workspace::test_new("notes"),
        ];
        app.collapsed_space_keys.insert("repo-key".into());
        app.active = None;
        app.mode = Mode::Terminal;

        // 4 rows = 2 header + 1 body + 1 footer: a 1-row body clips the second
        // display entry (collapsed group + "notes" = 2 entries from 3 raw
        // workspaces).
        let ws_area = Rect::new(0, 0, 30, 4);
        let metrics = workspace_list_scroll_metrics(&app, ws_area);

        assert_eq!(metrics.viewport_rows, 1);
        assert_eq!(metrics.max_offset_from_bottom, 1);
        assert_eq!(metrics.offset_from_bottom, 1);
    }

    #[test]
    fn workspace_scroll_offset_applies_to_group_children() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
            Workspace::test_new("notes"),
        ];
        app.collapsed_space_keys.insert("repo-key".into());
        app.active = None;
        app.mode = Mode::Terminal;
        app.workspace_scroll = 1;

        let (cards, headers) = compute_workspace_list_areas(&app, Rect::new(0, 0, 30, 12));

        assert!(headers.is_empty());
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].ws_idx, 2);
    }

    #[test]
    fn workspace_list_entries_group_multiple_workspaces_in_same_git_space() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
        ];

        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: true,
                },
            ]
        );
    }

    #[test]
    fn workspace_list_entries_group_non_contiguous_explicit_members() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_git_space("normal", "other-key"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
        ];

        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 2,
                    indented: true,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: false,
                },
            ]
        );
    }

    #[test]
    fn workspace_list_entries_do_not_group_normal_git_workspaces() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_git_space("one", "repo-key"),
            workspace_with_git_space("two", "repo-key"),
        ];

        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: false,
                },
            ]
        );
    }

    #[test]
    fn workspace_list_entries_do_not_auto_attach_normal_git_workspace_to_group() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_git_space("scratch", "repo-key"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
        ];

        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 2,
                    indented: true,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: false,
                },
            ]
        );
    }

    #[test]
    fn workspace_list_entries_leave_single_git_and_non_git_workspaces_flat() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_git_space("one", "repo-key"),
            workspace_with_worktree_space("notes", None, "/notes"),
        ];

        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: false,
                },
            ]
        );
    }

    #[test]
    fn collapsed_group_hides_inactive_children_but_keeps_active_visible() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
        ];
        app.active = Some(1);
        app.mode = Mode::Terminal;
        app.collapsed_space_keys.insert("repo-key".into());

        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: true,
                },
            ]
        );

        app.active = None;
        app.mode = Mode::Terminal;
        assert_eq!(
            workspace_list_entries(&app),
            vec![WorkspaceListEntry::Workspace {
                ws_idx: 0,
                indented: false,
            }]
        );
    }

    #[test]
    fn collapsed_group_keeps_selected_child_visible_in_navigate_mode() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
        ];
        app.mode = Mode::Navigate;
        app.selected = 1;
        app.active = Some(1);
        app.collapsed_space_keys.insert("repo-key".into());

        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: true,
                },
            ]
        );
    }

    #[test]
    fn collapsed_toggle_rect_is_single_cell_centered() {
        let area = Rect::new(0, 0, 7, 20);
        let toggle = collapsed_sidebar_toggle_rect(area);
        assert_eq!(toggle.width, 1);
        assert_eq!(toggle.y, area.y + area.height - 1);
        assert_eq!(toggle.x, area.x + (area.width - 1) / 2);
    }

    #[test]
    fn collapsed_toggle_rect_degrades_at_narrow_width() {
        let area = Rect::new(0, 0, 3, 10);
        let toggle = collapsed_sidebar_toggle_rect(area);
        assert!(toggle.width >= 1);
        assert!(toggle.width <= area.width.saturating_sub(1));
    }

    #[test]
    fn collapsed_rail_layout_sections_no_overlap() {
        let area = Rect::new(0, 0, 7, 20);
        let layout = compute_collapsed_rail_layout(area);

        assert!(layout.ws_area.height > 0);
        if let Some(div_y) = layout.divider_y {
            assert!(div_y >= layout.ws_area.y + layout.ws_area.height);
            assert!(layout.detail_area.y > div_y);
        }
        assert!(layout.ws_area.x + layout.ws_area.width <= area.x + area.width);
        assert!(layout.ws_area.y + layout.ws_area.height <= area.y + area.height);
        if layout.detail_area != Rect::default() {
            assert!(layout.detail_area.y + layout.detail_area.height <= area.y + area.height);
        }
    }

    #[test]
    fn collapsed_rail_layout_height_below_7_has_no_detail_area() {
        let area = Rect::new(0, 0, 7, 6);
        let layout = compute_collapsed_rail_layout(area);

        assert_eq!(layout.detail_area, Rect::default());
        assert!(layout.divider_y.is_none());
        assert!(layout.ws_area.height > 0);
    }

    #[test]
    fn is_attention_state_blocked_is_attention() {
        assert!(is_attention_state(AgentState::Blocked, true));
        assert!(is_attention_state(AgentState::Blocked, false));
    }

    #[test]
    fn is_attention_state_idle_unseen_is_attention() {
        assert!(is_attention_state(AgentState::Idle, false));
    }

    #[test]
    fn is_attention_state_idle_seen_is_not_attention() {
        assert!(!is_attention_state(AgentState::Idle, true));
    }

    #[test]
    fn is_attention_state_working_is_attention() {
        // zellij-fidelity round 2: Working agents now count as badge-worthy
        // (the badge surfaces all three non-idle states).
        assert!(is_attention_state(AgentState::Working, true));
        assert!(is_attention_state(AgentState::Working, false));
    }

    #[test]
    fn is_attention_state_unknown_is_not_attention() {
        assert!(!is_attention_state(AgentState::Unknown, true));
        assert!(!is_attention_state(AgentState::Unknown, false));
    }

    #[test]
    fn collapsed_rail_layout_at_exact_height_floor_has_detail_area() {
        let area = Rect::new(0, 0, 7, 7);
        let layout = compute_collapsed_rail_layout(area);

        assert!(layout.ws_area.height > 0);
        assert!(layout.divider_y.is_some());
        assert!(layout.detail_area.height > 0);
    }

    #[test]
    fn collapsed_rail_renders_button_like_rows_at_width_7() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        app.ensure_test_terminals();
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;
        app.sidebar_collapsed = true;

        let area = Rect::new(0, 0, 7, 20);
        let mut terminal =
            Terminal::new(TestBackend::new(7, 20)).expect("test terminal should initialize");

        terminal
            .draw(|frame| render_sidebar_collapsed(&app, frame, area))
            .expect("collapsed sidebar should render");

        let layout = compute_collapsed_rail_layout(area);
        let buf = terminal.backend().buffer();
        let active_row = layout.ws_area.y;
        let active_bg = buf[(layout.ws_area.x, active_row)].style().bg;
        assert_eq!(active_bg, Some(app.palette.surface_dim));
    }

    #[test]
    fn collapsed_rail_renders_attention_overflow_badge() {
        // Many short workspaces so some are hidden; a hidden one is Blocked, so
        // the bottom badge must paint a `●` attention glyph (FR8).
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = (0..12)
            .map(|i| Workspace::test_new(&format!("ws{i}")))
            .collect();
        app.ensure_test_terminals();
        // Force workspace 9 (hidden below) Blocked.
        let pane = app.workspaces[9].tabs[0].root_pane;
        let tid = app.workspaces[9].tabs[0].panes[&pane]
            .attached_terminal_id
            .clone();
        app.terminals.get_mut(&tid).unwrap().state = crate::detect::AgentState::Blocked;
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;
        app.sidebar_collapsed = true;

        crate::ui::compute_view(&mut app, Rect::new(0, 0, 80, 16));
        // compute_view recomputes sidebar_rect; render with the real rect.
        let sidebar_rect = app.view.sidebar_rect;
        let mut terminal = Terminal::new(TestBackend::new(
            sidebar_rect.width.max(7),
            sidebar_rect.height.max(16),
        ))
        .expect("test terminal should initialize");
        terminal
            .draw(|frame| render_sidebar_collapsed(&app, frame, sidebar_rect))
            .expect("collapsed sidebar should render");

        let below = app.view.sidebar_overflow.collapsed_ws_below;
        assert!(below.is_active(), "spaces hidden below");
        assert!(below.side.hidden_blocked >= 1, "blocked space counted");
        let buf = terminal.backend().buffer();
        let row_text: String = (below.rect.x..below.rect.x + below.rect.width)
            .map(|x| buf[(x, below.rect.y)].symbol())
            .collect();
        assert!(
            row_text.contains('◉'),
            "blocked badge glyph drawn: {row_text:?}"
        );
        assert!(row_text.contains('+'), "hidden count drawn: {row_text:?}");
    }

    #[test]
    fn collapsed_toggle_renders_overlay0_without_badge() {
        let app = crate::app::state::AppState::test_new();
        let area = Rect::new(0, 0, 7, 20);
        let mut terminal =
            Terminal::new(TestBackend::new(7, 20)).expect("test terminal should initialize");

        terminal
            .draw(|frame| render_sidebar_toggle(&app, frame, area, true, &app.palette))
            .expect("sidebar toggle should render");

        let toggle = collapsed_sidebar_toggle_rect(area);
        let cell = &terminal.backend().buffer()[(toggle.x, toggle.y)];
        assert_eq!(cell.symbol(), "»");
        assert_eq!(cell.style().fg, Some(app.palette.overlay0));
        // No chip background (upstream draws no surface_dim fill).
        assert_ne!(cell.style().bg, Some(app.palette.surface_dim));
        // No pending badge → not bold (bold is reserved for the attention signal).
        assert!(!cell.style().add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn collapsed_toggle_bolds_when_attention_badge_pending() {
        let mut app = crate::app::state::AppState::test_new();
        app.update_available = Some("9.9.9".into());
        let area = Rect::new(0, 0, 7, 20);
        let mut terminal =
            Terminal::new(TestBackend::new(7, 20)).expect("test terminal should initialize");

        terminal
            .draw(|frame| render_sidebar_toggle(&app, frame, area, true, &app.palette))
            .expect("sidebar toggle should render");

        let toggle = collapsed_sidebar_toggle_rect(area);
        let cell = &terminal.backend().buffer()[(toggle.x + toggle.width / 2, toggle.y)];
        assert_eq!(cell.style().fg, Some(app.palette.accent));
        assert!(cell.style().add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn collapsed_rail_layout_rects_in_bounds_at_mobile_threshold() {
        let area = Rect::new(0, 0, 7, 10);
        let layout = compute_collapsed_rail_layout(area);
        let content_w = area.width.saturating_sub(1);

        assert!(layout.ws_area.x + layout.ws_area.width <= area.x + content_w);
        assert!(layout.ws_area.y + layout.ws_area.height <= area.y + area.height);
        if layout.detail_area != Rect::default() {
            assert!(layout.detail_area.x + layout.detail_area.width <= area.x + content_w);
            assert!(layout.detail_area.y + layout.detail_area.height <= area.y + area.height);
        }
        if layout.toggle != Rect::default() {
            assert!(layout.toggle.x + layout.toggle.width <= area.x + content_w);
            assert!(layout.toggle.y + layout.toggle.height <= area.y + area.height);
        }
    }

    #[test]
    fn expanded_toggle_stays_overlay0_when_attention_badge_pending() {
        let mut app = crate::app::state::AppState::test_new();
        app.update_available = Some("9.9.9".into());
        assert!(app.global_menu_attention_badge_visible());
        let area = Rect::new(0, 0, 26, 20);
        let mut terminal =
            Terminal::new(TestBackend::new(26, 20)).expect("test terminal should initialize");

        terminal
            .draw(|frame| render_sidebar_toggle(&app, frame, area, false, &app.palette))
            .expect("sidebar toggle should render");

        let toggle = expanded_sidebar_toggle_rect(area);
        let cell = &terminal.backend().buffer()[(toggle.x, toggle.y)];
        assert_eq!(cell.symbol(), "«");
        assert_eq!(cell.style().fg, Some(app.palette.overlay0));
        assert!(!cell.style().add_modifier.contains(Modifier::BOLD));
    }
}
