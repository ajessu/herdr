//! Fork: zellij-style sidebar (rail, overflow badges, pending styling).

use super::*;
use crate::api::schema::Method;
use crate::client::shell::sidebar_chrome::SidebarJump;

fn workspaces_snapshot(count: usize, focused: usize) -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    let template = snapshot.workspaces[0].clone();
    snapshot.workspaces = (0..count)
        .map(|index| ClientShellWorkspace {
            workspace_id: format!("ws_{}", index + 1),
            number: index + 1,
            label: format!("space-{}", index + 1),
            focused: index == focused,
            ..template.clone()
        })
        .collect();
    snapshot.focused_workspace_id = Some(format!("ws_{}", focused + 1));
    snapshot
}

fn agent(index: usize, status: AgentStatus, focused: bool) -> ClientShellAgent {
    ClientShellAgent {
        pane_id: format!("pane_{}", index + 1),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        name: Some("pi".into()),
        display_agent: None,
        agent: Some("pi".into()),
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: status,
        state_change_seq: 1,
        state_labels: vec![("working".into(), "thinking".into())],
        tokens: Vec::new(),
        focused,
    }
}

fn agents_snapshot(statuses: &[AgentStatus]) -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    snapshot.agents = statuses
        .iter()
        .enumerate()
        .map(|(index, status)| agent(index, *status, index == 0))
        .collect();
    snapshot
}

fn shell(
    snapshot: ClientShellSnapshot,
    style: crate::config::SidebarStyleConfig,
) -> ClientShellState {
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.sidebar_style = style;
    let mut state = ClientShellState::new(config);
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state
}

fn zellij(snapshot: ClientShellSnapshot) -> ClientShellState {
    shell(snapshot, crate::config::SidebarStyleConfig::Zellij)
}

fn rail(snapshot: ClientShellSnapshot) -> ClientShellState {
    let mut state = zellij(snapshot);
    state.sidebar_collapsed = true;
    state
}

fn mouse(
    state: &mut ClientShellState,
    kind: MouseEventKind,
    column: u16,
    row: u16,
) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::empty(),
    })])
}

fn click(state: &mut ClientShellState, rect: Rect) -> ClientShellInput {
    mouse(
        state,
        MouseEventKind::Down(MouseButton::Left),
        rect.x,
        rect.y,
    )
}

fn methods(outcome: &ClientShellInput) -> Vec<&Method> {
    outcome
        .actions
        .iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some(&request.method),
            _ => None,
        })
        .collect()
}

fn row_text(buffer: &Buffer, y: u16, x: u16, width: u16) -> String {
    (x..x + width)
        .map(|x| buffer[(x, y)].symbol().to_owned())
        .collect()
}

fn compose(state: &mut ClientShellState, cols: u16, rows: u16) -> Buffer {
    state
        .compose(cols, rows)
        .expect("frame")
        .to_ratatui_buffer()
        .expect("buffer")
}

fn badge(state: &ClientShellState, jump: &SidebarJump) -> Rect {
    state
        .hits
        .sidebar_overflow
        .iter()
        .find(|(_, target)| target == jump)
        .map(|(rect, _)| *rect)
        .unwrap_or_else(|| panic!("no badge for {jump:?}"))
}

#[test]
fn upstream_style_keeps_the_four_column_rail() {
    let mut state = shell(
        workspaces_snapshot(3, 0),
        crate::config::SidebarStyleConfig::Upstream,
    );
    state.sidebar_collapsed = true;
    assert_eq!(state.layout(106, 20).sidebar.width, 4);
    state.sidebar_collapsed = false;
    let mut zellij = rail(workspaces_snapshot(3, 0));
    assert_eq!(zellij.layout(106, 20).sidebar.width, 7);
    zellij.sidebar_collapsed = false;
    assert_eq!(
        zellij.layout(106, 20).sidebar.width,
        state.layout(106, 20).sidebar.width,
        "the expanded width is unchanged"
    );
}

#[test]
fn rail_rows_show_marker_number_and_status() {
    let mut projected = workspaces_snapshot(10, 0);
    projected.workspaces[1].agent_status = AgentStatus::Blocked;
    let mut state = rail(projected);
    let buffer = compose(&mut state, 106, 24);
    let palette = &state.config.palette;

    assert_eq!(buffer[(0, 0)].symbol(), " ", "settled: no marker");
    assert_eq!(buffer[(2, 0)].symbol(), "1");
    assert_eq!(buffer[(4, 0)].symbol(), "○");
    assert_eq!(buffer[(2, 0)].bg, palette.active_row_bg, "focused row");

    assert_eq!(buffer[(0, 1)].symbol(), "●", "attention marker");
    assert_eq!(buffer[(0, 1)].fg, palette.accent);
    assert_eq!(buffer[(4, 1)].fg, palette.red);
    assert_eq!(buffer[(2, 1)].fg, palette.overlay0);

    assert_eq!(row_text(&buffer, 9, 2, 2), "10");
    assert_eq!(
        buffer[(5, 9)].symbol(),
        "○",
        "two digits push the icon right"
    );
    assert_eq!(buffer[(6, 9)].symbol(), "│", "separator stays at column 7");
    assert!(state.hits.sidebar_overflow.is_empty(), "everything fits");
}

#[test]
fn rail_badge_counts_hidden_attention_and_jumps_to_it() {
    let mut projected = workspaces_snapshot(12, 0);
    projected.workspaces[8].agent_status = AgentStatus::Blocked;
    let mut state = rail(projected);
    let buffer = compose(&mut state, 106, 16);
    let rect = badge(&state, &SidebarJump::Workspace("ws_9".into()));
    assert_eq!(rect.width, 6, "the reserved row is the click target");
    let text = row_text(&buffer, rect.y, rect.x, rect.width);
    assert!(text.contains('+') && text.contains('◉'), "{text}");
    assert!(
        state.hits.workspaces.iter().all(|hit| hit.rect.y != rect.y),
        "content never shares the badge row"
    );

    let outcome = click(&mut state, rect);
    assert!(matches!(
        methods(&outcome)[..],
        [Method::WorkspaceFocus(target)] if target.workspace_id == "ws_9"
    ));
}

#[test]
fn rail_badge_without_attention_jumps_to_the_nearest_hidden_workspace() {
    let mut state = rail(workspaces_snapshot(12, 0));
    compose(&mut state, 106, 16);
    let last_visible = state.hits.workspaces.len();
    let target = format!("ws_{}", last_visible + 1);
    let rect = badge(&state, &SidebarJump::Workspace(target.clone()));
    let outcome = click(&mut state, rect);
    assert!(matches!(
        methods(&outcome)[..],
        [Method::WorkspaceFocus(focus)] if focus.workspace_id == target
    ));
}

#[test]
fn rail_window_follows_the_focused_workspace() {
    let mut state = rail(workspaces_snapshot(12, 11));
    compose(&mut state, 106, 16);
    assert!(state
        .hits
        .workspaces
        .iter()
        .any(|hit| hit.workspace_id == "ws_12"));
    assert!(
        state
            .hits
            .sidebar_overflow
            .iter()
            .any(|(rect, _)| rect.y == 0),
        "hidden above: badge on the first row"
    );
}

#[test]
fn rail_agent_rows_are_clickable_only_when_they_need_attention() {
    let mut state = rail(agents_snapshot(&[
        AgentStatus::Idle,
        AgentStatus::Idle,
        AgentStatus::Blocked,
    ]));
    let buffer = compose(&mut state, 106, 20);
    let palette = state.config.palette.clone();
    assert_eq!(state.hits.agents.len(), 1);
    let (blocked, pane_id) = state.hits.agents[0].clone();
    assert_eq!(pane_id, "pane_3");
    assert_eq!(buffer[(blocked.x + 2, blocked.y)].bg, palette.surface_dim);
    assert_eq!(buffer[(blocked.x, blocked.y)].symbol(), "●");
    assert_eq!(
        buffer[(blocked.x + 2, blocked.y - 2)].bg,
        palette.active_row_bg,
        "focused agent row"
    );

    let idle = Rect::new(blocked.x + 2, blocked.y - 1, 1, 1);
    assert!(methods(&click(&mut state, idle)).is_empty());
    let outcome = click(&mut state, blocked);
    assert!(matches!(
        methods(&outcome)[..],
        [Method::PaneFocus(target)] if target.pane_id == "pane_3"
    ));
}

#[test]
fn rail_agent_badge_jumps_to_the_hidden_blocked_pane() {
    let mut statuses = vec![AgentStatus::Idle; 20];
    statuses[14] = AgentStatus::Blocked;
    let mut state = rail(agents_snapshot(&statuses));
    compose(&mut state, 106, 16);
    let rect = badge(
        &state,
        &SidebarJump::Pane {
            pane_id: "pane_15".into(),
            index: 14,
        },
    );
    let outcome = click(&mut state, rect);
    assert!(matches!(
        methods(&outcome)[..],
        [Method::PaneFocus(target)] if target.pane_id == "pane_15"
    ));
}

#[test]
fn expanded_spaces_badge_jumps_and_the_target_scrolls_into_view() {
    let mut projected = workspaces_snapshot(30, 0);
    projected.workspaces[24].agent_status = AgentStatus::Blocked;
    let mut state = zellij(projected.clone());
    let buffer = compose(&mut state, 106, 16);
    let rect = badge(&state, &SidebarJump::Workspace("ws_25".into()));
    let body = state.hits.workspace_body;
    assert_eq!(rect.y, body.bottom() - 1);
    assert!(rect.x > body.x + 2, "badge sits right-aligned: {rect:?}");
    assert!(
        rect.right() <= state.hits.workspace_scrollbar.x,
        "badge stays off the scrollbar"
    );
    let text = row_text(&buffer, rect.y, rect.x, rect.width);
    assert!(text.contains("+9+") && text.contains("◉¹"), "{text}");

    let outcome = click(&mut state, rect);
    assert!(matches!(
        methods(&outcome)[..],
        [Method::WorkspaceFocus(target)] if target.workspace_id == "ws_25"
    ));

    for (index, workspace) in projected.workspaces.iter_mut().enumerate() {
        workspace.focused = index == 24;
    }
    projected.focused_workspace_id = Some("ws_25".into());
    projected.revision += 1;
    let mut updated_surface = surface();
    updated_surface.projection_revision = projected.revision;
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(updated_surface);
    compose(&mut state, 106, 16);
    assert!(state
        .hits
        .workspaces
        .iter()
        .any(|hit| hit.workspace_id == "ws_25"));
}

#[test]
fn expanded_badge_only_claims_its_own_cells() {
    let mut config = Config::default();
    config.ui.sidebar.style = crate::config::SidebarStyleConfig::Zellij;
    config.ui.sidebar.spaces.rows = vec![vec![
        crate::config::SpaceSidebarToken::StateIcon,
        crate::config::SpaceSidebarToken::Workspace,
    ]];
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(workspaces_snapshot(30, 0)));
    state.set_pane_surface(surface());
    compose(&mut state, 106, 16);
    let (rect, _) = state.hits.sidebar_overflow[0].clone();
    let row_hit = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.rect.y <= rect.y && rect.y < hit.rect.bottom())
        .map(|hit| (hit.rect, hit.workspace_id.clone()))
        .expect("a row under the badge");
    let point = Rect::new(row_hit.0.x + 1, rect.y, 1, 1);
    assert!(methods(&click(&mut state, point)).is_empty(), "a row press");
    let release = mouse(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        point.x,
        point.y,
    );
    assert!(matches!(
        methods(&release)[..],
        [Method::WorkspaceFocus(target)] if target.workspace_id == row_hit.1
    ));
}

#[test]
fn expanded_agents_badge_focuses_and_scrolls_the_panel() {
    let mut statuses = vec![AgentStatus::Idle; 20];
    statuses[14] = AgentStatus::Blocked;
    let mut state = zellij(agents_snapshot(&statuses));
    compose(&mut state, 106, 20);
    let jump = SidebarJump::Pane {
        pane_id: "pane_15".into(),
        index: 14,
    };
    let rect = badge(&state, &jump);
    assert!(rect.y >= state.hits.agent_body.y);
    let outcome = click(&mut state, rect);
    assert!(matches!(
        methods(&outcome)[..],
        [Method::PaneFocus(target)] if target.pane_id == "pane_15"
    ));
    compose(&mut state, 106, 20);
    assert!(state
        .hits
        .agents
        .iter()
        .any(|(_, pane_id)| pane_id == "pane_15"));
}

#[test]
fn agent_panel_keeps_pending_labels_bright_and_dims_settled_ones() {
    let mut projected = agents_snapshot(&[AgentStatus::Working, AgentStatus::Idle]);
    projected.agents[0].focused = false;
    let mut state = zellij(projected.clone());
    let buffer = compose(&mut state, 106, 24);
    let palette = state.config.palette.clone();
    let label_cell = |pane_id: &str| {
        let rect = state
            .hits
            .agents
            .iter()
            .find(|(_, id)| id == pane_id)
            .map(|(rect, _)| *rect)
            .expect("agent row");
        let x = (rect.x..rect.right())
            .find(|x| buffer[(*x, rect.y)].symbol() == "c")
            .expect("workspace label");
        buffer[(x, rect.y)].clone()
    };
    let working = label_cell("pane_1");
    assert_eq!(working.fg, palette.text);
    assert!(working.modifier.contains(Modifier::BOLD));
    let idle = label_cell("pane_2");
    assert_eq!(idle.fg, palette.overlay0);
    assert!(!idle.modifier.contains(Modifier::BOLD));

    let mut upstream = shell(projected, crate::config::SidebarStyleConfig::Upstream);
    let buffer = compose(&mut upstream, 106, 24);
    let rect = upstream.hits.agents[1].0;
    let x = (rect.x..rect.right())
        .find(|x| buffer[(*x, rect.y)].symbol() == "c")
        .expect("workspace label");
    assert_eq!(
        buffer[(x, rect.y)].fg,
        palette.subtext0,
        "upstream unchanged"
    );
}

#[test]
fn mobile_agent_detail_drops_the_status_word() {
    for (style, expect_word) in [
        (crate::config::SidebarStyleConfig::Upstream, true),
        (crate::config::SidebarStyleConfig::Zellij, false),
    ] {
        let mut state = shell(agents_snapshot(&[AgentStatus::Working]), style);
        state.compose(44, 20).expect("mobile header");
        let switch = state.hits.mobile_switch;
        click(&mut state, switch);
        let frame = state.compose(44, 20).expect("mobile switcher");
        let text = frame
            .cells
            .iter()
            .map(|cell| cell.symbol.as_str())
            .collect::<String>();
        assert_eq!(text.contains("thinking"), expect_word, "{style:?}: {text}");
    }
}

#[test]
fn sidebar_scrollbars_use_heavier_glyphs() {
    for (style, track, thumb) in [
        (crate::config::SidebarStyleConfig::Upstream, "▕", "▕"),
        (crate::config::SidebarStyleConfig::Zellij, "┃", "█"),
    ] {
        let mut state = shell(workspaces_snapshot(30, 0), style);
        let buffer = compose(&mut state, 106, 16);
        let bar = state.hits.workspace_scrollbar;
        let glyphs: Vec<_> = (bar.y..bar.bottom())
            .map(|y| buffer[(bar.x, y)].symbol().to_owned())
            .collect();
        assert!(
            glyphs.iter().any(|glyph| glyph == track),
            "{style:?}: {glyphs:?}"
        );
        assert!(
            glyphs.iter().any(|glyph| glyph == thumb),
            "{style:?}: {glyphs:?}"
        );
    }
}

#[test]
fn workspace_drag_underlines_the_drop_row_without_hiding_it() {
    let mut projected = workspaces_snapshot(3, 0);
    for workspace in &mut projected.workspaces {
        workspace.branch = None;
    }
    let mut config = Config::default();
    config.ui.sidebar.style = crate::config::SidebarStyleConfig::Zellij;
    config.ui.sidebar.spaces.rows = vec![vec![crate::config::SpaceSidebarToken::Workspace]];
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    compose(&mut state, 106, 24);
    let first = state.hits.workspaces[0].rect;
    let second = state.hits.workspaces[1].rect;
    let third = state.hits.workspaces[2].rect;
    assert_eq!(second.y, first.y + 1, "one-row cards abut");

    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        third.x + 2,
        third.y,
    );
    mouse(
        &mut state,
        MouseEventKind::Drag(MouseButton::Left),
        first.x + 2,
        second.y,
    );
    let buffer = compose(&mut state, 106, 24);
    let marked = (first.y..=third.y)
        .find(|y| {
            buffer[(first.x + 1, *y)]
                .modifier
                .contains(Modifier::UNDERLINED)
        })
        .expect("an underlined drop row");
    let text = row_text(&buffer, marked, first.x, first.width);
    assert!(text.contains("space-"), "the row keeps its text: {text}");
    assert!(!text.contains('─'));
}
