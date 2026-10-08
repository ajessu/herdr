//! Fork: zellij-style tab bar and tab context-menu moves.

use super::*;
use crate::api::schema::Method;

fn three_tabs(focused: usize) -> ClientShellSnapshot {
    tabs_snapshot(3, focused)
}

fn tabs_snapshot(count: usize, focused: usize) -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    let template = snapshot.tabs[0].clone();
    snapshot.tabs = (0..count)
        .map(|index| ClientShellTab {
            tab_id: format!("tab_{}", index + 1),
            number: index + 1,
            label: format!("{}", index + 1),
            focused: index == focused,
            ..template.clone()
        })
        .collect();
    snapshot.focused_tab_id = Some(format!("tab_{}", focused + 1));
    snapshot.workspaces[0].active_tab_id = format!("tab_{}", focused + 1);
    snapshot.panes[0].tab_id = format!("tab_{}", focused + 1);
    snapshot
}

fn menu_action(
    state: &mut ClientShellState,
    tab_id: &str,
    action: ClientContextMenuAction,
) -> Vec<Method> {
    state.open_tab_context_menu(tab_id.to_owned(), 10, 0);
    let index = match state.overlay.as_ref() {
        Some(ClientShellOverlay::ContextMenu(menu)) => menu
            .items()
            .iter()
            .position(|item| item.action == action)
            .expect("menu item"),
        _ => panic!("tab context menu"),
    };
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(index, &mut outcome);
    outcome
        .actions
        .into_iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some(request.method),
            _ => None,
        })
        .collect()
}

#[test]
fn tab_menu_lists_move_items_after_upstreams() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(three_tabs(0)));
    state.open_tab_context_menu("tab_1".into(), 10, 0);
    let Some(ClientShellOverlay::ContextMenu(menu)) = state.overlay.as_ref() else {
        panic!("tab context menu");
    };
    let labels: Vec<_> = menu.items().iter().map(|item| item.label).collect();
    assert_eq!(
        labels,
        vec!["New tab", "Rename", "Close", "Move left", "Move right"]
    );
}

#[test]
fn tab_menu_moves_the_clicked_tab_without_wrapping() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(three_tabs(0)));

    let methods = menu_action(&mut state, "tab_2", ClientContextMenuAction::MoveTabRight);
    assert!(matches!(&methods[0], Method::TabFocus(target) if target.tab_id == "tab_2"));
    assert!(matches!(
        &methods[1],
        Method::TabMove(params) if params.tab_id == "tab_2" && params.insert_index == 3
    ));

    let methods = menu_action(&mut state, "tab_2", ClientContextMenuAction::MoveTabLeft);
    assert!(matches!(
        &methods[1],
        Method::TabMove(params) if params.tab_id == "tab_2" && params.insert_index == 0
    ));

    let first = menu_action(&mut state, "tab_1", ClientContextMenuAction::MoveTabLeft);
    assert_eq!(first.len(), 1, "moving the first tab left only focuses it");
    let last = menu_action(&mut state, "tab_3", ClientContextMenuAction::MoveTabRight);
    assert_eq!(last.len(), 1, "moving the last tab right only focuses it");
}

fn zellij_shell(snapshot: ClientShellSnapshot) -> ClientShellState {
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.tab_style = crate::config::TabStyleConfig::Zellij;
    config.tab_status = crate::config::TabStatusModeConfig::Attention;
    let mut state = ClientShellState::new(config);
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
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

fn top_row(state: &mut ClientShellState, cols: u16) -> String {
    let frame = state.compose(cols, 20).expect("frame");
    frame.cells[..usize::from(frame.width)]
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect()
}

#[test]
fn zellij_tiles_replace_scroll_arrows_and_jump_to_the_urgent_hidden_tab() {
    let mut projected = tabs_snapshot(12, 0);
    projected.tabs[9].agent_status = crate::api::schema::AgentStatus::Blocked;
    let mut state = zellij_shell(projected);
    let row = top_row(&mut state, 80);
    assert!(row.contains(" → "), "{row}");
    assert!(row.contains("◉¹"), "{row}");
    assert!(!row.contains(" > "), "no upstream scroll arrow: {row}");
    let tile = state.hits.tab_scroll_right;
    assert!(tile.width > 0);

    let click = mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        tile.x + 1,
        tile.y,
    );
    assert!(matches!(
        methods(&click)[..],
        [Method::TabFocus(target)] if target.tab_id == "tab_10"
    ));
    assert_eq!(state.tab_scroll, 0, "the tile jumps instead of scrolling");
}

#[test]
fn zellij_tabs_keep_upstream_wheel_drag_and_new_tab_behavior() {
    let mut state = zellij_shell(three_tabs(0));
    top_row(&mut state, 106);
    let first = state.hits.tabs[0].0;
    let third = state.hits.tabs[2].0;

    let wheel = mouse(&mut state, MouseEventKind::ScrollDown, first.x + 1, first.y);
    assert!(matches!(
        methods(&wheel)[..],
        [Method::TabFocus(target)] if target.tab_id == "tab_2"
    ));

    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        first.x + 1,
        first.y,
    );
    mouse(
        &mut state,
        MouseEventKind::Drag(MouseButton::Left),
        third.right() - 1,
        third.y,
    );
    let drop = mouse(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        third.right() - 1,
        third.y,
    );
    assert!(matches!(
        methods(&drop)[..],
        [Method::TabMove(params)] if params.tab_id == "tab_1" && params.insert_index == 3
    ));

    top_row(&mut state, 106);
    let plus = state.hits.new_tab;
    assert!(plus.width > 0 && plus.x == state.hits.tabs[2].0.right());
}

#[test]
fn middle_click_on_a_workspaces_last_tab_asks_first() {
    let mut state = zellij_shell(tabs_snapshot(1, 0));
    top_row(&mut state, 106);
    let only = state.hits.tabs[0].0;
    let close = mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Middle),
        only.x + 1,
        only.y,
    );
    assert!(
        methods(&close).is_empty(),
        "nothing closes before confirming"
    );
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::ConfirmClose(_))
    ));
}

#[test]
fn middle_click_closes_and_double_click_renames_a_zellij_tab() {
    let mut state = zellij_shell(three_tabs(0));
    top_row(&mut state, 106);
    let second = state.hits.tabs[1].0;
    let close = mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Middle),
        second.x + 1,
        second.y,
    );
    assert!(matches!(
        methods(&close)[..],
        [Method::TabClose(target)] if target.tab_id == "tab_2"
    ));

    for _ in 0..2 {
        mouse(
            &mut state,
            MouseEventKind::Down(MouseButton::Left),
            second.x + 1,
            second.y,
        );
        mouse(
            &mut state,
            MouseEventKind::Up(MouseButton::Left),
            second.x + 1,
            second.y,
        );
    }
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::Rename(ClientRenameOverlay {
            target: ClientRenameTarget::Tab { ref tab_id, .. },
            ..
        })) if tab_id == "tab_2"
    ));
}

#[test]
fn upstream_style_ignores_the_zellij_mouse_extras() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(three_tabs(0)));
    state.set_pane_surface(surface());
    top_row(&mut state, 106);
    let second = state.hits.tabs[1].0;
    let middle = mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Middle),
        second.x + 1,
        second.y,
    );
    assert!(methods(&middle).is_empty());
    for _ in 0..2 {
        mouse(
            &mut state,
            MouseEventKind::Down(MouseButton::Left),
            second.x + 1,
            second.y,
        );
        mouse(
            &mut state,
            MouseEventKind::Up(MouseButton::Left),
            second.x + 1,
            second.y,
        );
    }
    assert!(state.overlay.is_none());
}

#[test]
fn tab_dots_follow_show_tab_status_and_the_client_projection() {
    let mut projected = three_tabs(0);
    projected.tabs[1].agent_status = crate::api::schema::AgentStatus::Done;
    projected.tabs[2].agent_status = crate::api::schema::AgentStatus::Working;
    let mut state = zellij_shell(projected.clone());
    let row = top_row(&mut state, 106);
    assert_eq!(
        row.matches('●').count(),
        1,
        "attention shows only the done tab: {row}"
    );

    state.config.tab_status = crate::config::TabStatusModeConfig::All;
    let row = top_row(&mut state, 106);
    assert!(
        row.matches('●').count() >= 2,
        "all shows working too: {row}"
    );

    state.config.tab_status = crate::config::TabStatusModeConfig::Off;
    let row = top_row(&mut state, 106);
    assert!(!row.contains('●'), "{row}");
}
