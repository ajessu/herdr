//! Fork: modal layer on the client shell.

use super::*;
use crate::api::schema::{Method, PaneDirection, ResponseResult, SplitDirection};

fn shell() -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state
}

fn with_tabs(count: usize, focused: usize) -> ClientShellSnapshot {
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

fn only_method(outcome: &ClientShellInput) -> &Method {
    let [ClientShellAction::Endpoint { request, .. }] = &outcome.actions[..] else {
        panic!("expected one endpoint request, got {:?}", outcome.actions);
    };
    &request.method
}

fn request_id(outcome: &ClientShellInput) -> String {
    let [ClientShellAction::Endpoint { request, .. }] = &outcome.actions[..] else {
        panic!("expected one endpoint request");
    };
    request.id.clone()
}

fn resize_result(changed: bool) -> ResponseResult {
    let rect = crate::api::schema::PaneLayoutRect {
        x: 0,
        y: 0,
        width: 4,
        height: 2,
    };
    ResponseResult::PaneResize {
        resize: crate::api::schema::PaneResizeResult {
            changed,
            reason: None,
            pane_id: "pane_1".into(),
            focused_pane_id: "pane_1".into(),
            layout: crate::api::schema::PaneLayoutSnapshot {
                workspace_id: "ws_1".into(),
                tab_id: "tab_1".into(),
                zoomed: false,
                area: rect,
                focused_pane_id: "pane_1".into(),
                panes: Vec::new(),
                splits: Vec::new(),
            },
        },
    }
}

#[test]
fn split_auto_splits_along_the_focused_panes_longer_side() {
    let mut state = shell();
    let wide = state.handle_input_bytes(b"\x1b[110;3u");
    assert!(matches!(
        only_method(&wide),
        Method::PaneSplit(params)
            if params.direction == SplitDirection::Right
                && params.target_pane_id.as_deref() == Some("pane_1")
    ));

    let mut tall = surface();
    tall.panes[0].rect.height = 4;
    let mut state = shell();
    state.set_pane_surface(tall);
    let tall = state.handle_input_bytes(b"\x1b[110;3u");
    assert!(matches!(
        only_method(&tall),
        Method::PaneSplit(params) if params.direction == SplitDirection::Down
    ));
}

#[test]
fn move_tab_left_and_right_stop_at_the_ends() {
    let mut state = shell();
    state.set_snapshot(Box::new(with_tabs(3, 0)));
    assert!(state.handle_input_bytes(b"\x1b[105;3u").actions.is_empty());
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"\x1b[111;3u")),
        Method::TabMove(params) if params.tab_id == "tab_1" && params.insert_index == 2
    ));

    let mut state = shell();
    state.set_snapshot(Box::new(with_tabs(3, 2)));
    assert!(state.handle_input_bytes(b"\x1b[111;3u").actions.is_empty());
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"\x1b[105;3u")),
        Method::TabMove(params) if params.tab_id == "tab_3" && params.insert_index == 1
    ));
}

#[test]
fn resize_grow_falls_back_to_the_vertical_border_when_nothing_moved() {
    let mut state = shell();
    let grow = state.handle_input_bytes(b"\x1b[61;3u");
    assert!(matches!(
        only_method(&grow),
        Method::PaneResize(params) if params.direction == PaneDirection::Right
    ));

    let (_, actions) =
        state.handle_endpoint_result("boot-1", &request_id(&grow), Ok(resize_result(false)));
    let [ClientShellAction::Endpoint { request, .. }] = &actions[..] else {
        panic!("expected the fallback resize");
    };
    assert!(matches!(
        &request.method,
        Method::PaneResize(params)
            if params.direction == PaneDirection::Down
                && params.pane_id.as_deref() == Some("pane_1")
    ));

    let shrink = state.handle_input_bytes(b"\x1b[45;3u");
    assert!(matches!(
        only_method(&shrink),
        Method::PaneResize(params) if params.direction == PaneDirection::Left
    ));
    let (_, actions) =
        state.handle_endpoint_result("boot-1", &request_id(&shrink), Ok(resize_result(true)));
    assert!(
        actions.is_empty(),
        "a resize that moved a border needs no fallback"
    );
}

#[test]
fn stack_shortcut_uses_the_pane_stack_method() {
    let mut state = shell();
    assert!(state.handle_input_bytes(&[0x02]).actions.is_empty());
    let stack = state.handle_input_bytes(b"S");
    assert!(matches!(
        only_method(&stack),
        Method::PaneStack(target) if target.pane_id == "pane_1"
    ));
    assert!(state.handle_input_bytes(&[0x02]).actions.is_empty());
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"U")),
        Method::PaneUnstack(target) if target.pane_id == "pane_1"
    ));
}

#[test]
fn stack_on_a_server_without_the_method_shows_a_notice() {
    let mut state = shell();
    state.set_endpoint_methods(Some(vec!["pane.focus".into()]));
    state.handle_input_bytes(&[0x02]);
    let stack = state.handle_input_bytes(b"S");
    assert!(stack.actions.is_empty());
    let notice = state
        .visible_endpoint_notice
        .as_ref()
        .expect("unsupported action notice");
    assert_eq!(notice.key.kind, ClientEndpointNoticeKind::Unsupported);
    assert_eq!(notice.key.code, "pane.stack");
    assert!(state.endpoint_error.is_none());
}

const CTRL_B: &[u8] = &[0x02];
const CTRL_G: &[u8] = &[0x07];
const CTRL_H: &[u8] = &[0x08];
const CTRL_N: &[u8] = &[0x0e];
const CTRL_O: &[u8] = &[0x0f];
const CTRL_P: &[u8] = &[0x10];
const CTRL_T: &[u8] = &[0x14];
const ESC: &[u8] = b"\x1b";
const ENTER: &[u8] = b"\r";

fn pane_mode() -> ClientShellMode {
    ClientShellMode::Modal(super::super::modal::ModalMode::Pane)
}

fn modal(mode: super::super::modal::ModalMode) -> ClientShellMode {
    ClientShellMode::Modal(mode)
}

fn with_workspaces(count: usize) -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    let template = snapshot.workspaces[0].clone();
    snapshot.workspaces = (0..count)
        .map(|index| ClientShellWorkspace {
            workspace_id: format!("ws_{}", index + 1),
            number: index + 1,
            label: format!("space-{}", index + 1),
            focused: index == 0,
            ..template.clone()
        })
        .collect();
    snapshot
}

fn frame_text(state: &mut ClientShellState) -> String {
    let frame = state.compose(106, 20).expect("frame");
    frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn mode_entry_keys_switch_mode_instead_of_reaching_the_pane() {
    use super::super::modal::ModalMode;
    for (bytes, mode) in [
        (CTRL_P, modal(ModalMode::Pane)),
        (CTRL_T, modal(ModalMode::Tab)),
        (CTRL_N, ClientShellMode::Resize),
        (CTRL_H, modal(ModalMode::Move)),
        (CTRL_O, modal(ModalMode::Session)),
    ] {
        let mut state = shell();
        let entered = state.handle_input_bytes(bytes);
        assert!(entered.requests.is_empty(), "{bytes:?} reached the pane");
        assert!(entered.actions.is_empty());
        assert_eq!(state.mode, mode);
    }
}

#[test]
fn locked_mode_forwards_every_key_but_the_unlock_key() {
    let mut state = shell();
    assert!(state.handle_input_bytes(CTRL_G).requests.is_empty());
    assert!(state.modal_locked);
    assert_eq!(state.mode, ClientShellMode::Terminal);

    for bytes in [b"a".as_slice(), CTRL_B, CTRL_P, b"\x1b[104;3u"] {
        let forwarded = state.handle_input_bytes(bytes);
        assert_eq!(
            forwarded.requests.len(),
            1,
            "{bytes:?} should reach the pane"
        );
        assert!(forwarded.actions.is_empty());
        assert_eq!(state.mode, ClientShellMode::Terminal);
    }

    assert!(state.handle_input_bytes(CTRL_G).requests.is_empty());
    assert!(!state.modal_locked);
    assert!(state.handle_input_bytes(CTRL_P).requests.is_empty());
    assert_eq!(state.mode, pane_mode());
}

#[test]
fn default_mode_locked_starts_the_client_locked() {
    let config = toml::from_str::<Config>("[keys]\ndefault_mode = \"locked\"\n").unwrap();
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    assert!(state.modal_locked);
    assert_eq!(state.handle_input_bytes(CTRL_P).requests.len(), 1);
}

#[test]
fn another_entry_key_switches_modes_directly() {
    use super::super::modal::ModalMode;
    let mut state = shell();
    state.handle_input_bytes(CTRL_P);
    state.handle_input_bytes(CTRL_T);
    assert_eq!(state.mode, modal(ModalMode::Tab));
    state.handle_input_bytes(CTRL_N);
    assert_eq!(state.mode, ClientShellMode::Resize);
    state.handle_input_bytes(CTRL_O);
    assert_eq!(state.mode, modal(ModalMode::Session));
    state.handle_input_bytes(CTRL_G);
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(state.modal_locked);
    assert!(state.navigate_workspace_id.is_none());
}

#[test]
fn own_entry_key_esc_and_enter_leave_each_sticky_mode() {
    for entry in [CTRL_P, CTRL_T, CTRL_N, CTRL_H, CTRL_O] {
        for exit in [entry, ESC, ENTER] {
            if entry == CTRL_O && exit == ENTER {
                continue; // Enter confirms the selection in Session mode.
            }
            let mut state = shell();
            state.handle_input_bytes(entry);
            assert_ne!(state.mode, ClientShellMode::Terminal);
            let left = state.handle_input_bytes(exit);
            assert!(left.requests.is_empty());
            assert_eq!(
                state.mode,
                ClientShellMode::Terminal,
                "{entry:?} then {exit:?}"
            );
            assert!(state.navigate_workspace_id.is_none());
        }
    }
}

#[test]
fn prefix_key_from_a_sticky_mode_enters_prefix() {
    let mut state = shell();
    state.handle_input_bytes(CTRL_P);
    assert!(state.handle_input_bytes(CTRL_B).requests.is_empty());
    assert_eq!(state.mode, ClientShellMode::Prefix);
}

#[test]
fn every_configured_prefix_enters_prefix_from_a_sticky_mode() {
    let config: Config = toml::from_str("[keys]\nprefix = [\"ctrl+b\", \"ctrl+s\"]\n").unwrap();
    for bytes in [CTRL_B, &[0x13]] {
        let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
        state.set_snapshot(Box::new(snapshot()));
        state.set_pane_surface(surface());
        state.handle_input_bytes(CTRL_P);
        assert_eq!(state.mode, pane_mode());
        assert!(state.handle_input_bytes(bytes).requests.is_empty());
        assert_eq!(state.mode, ClientShellMode::Prefix, "{bytes:?}");
    }
}

#[test]
fn unbound_keys_in_a_sticky_mode_are_swallowed() {
    let mut state = shell();
    state.handle_input_bytes(CTRL_P);
    for bytes in [b"q".as_slice(), b"7", b"\x1b[121;3u"] {
        let swallowed = state.handle_input_bytes(bytes);
        assert!(swallowed.requests.is_empty(), "{bytes:?}");
        assert!(swallowed.actions.is_empty(), "{bytes:?}");
        assert_eq!(state.mode, pane_mode());
    }
}

#[test]
fn pane_mode_actions_use_the_endpoint_api_and_stay_in_the_mode() {
    let mut state = shell();
    state.handle_input_bytes(CTRL_P);
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"n")),
        Method::PaneSplit(params) if params.direction == SplitDirection::Right
    ));
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"d")),
        Method::PaneSplit(params) if params.direction == SplitDirection::Down
    ));
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"r")),
        Method::PaneSplit(params) if params.direction == SplitDirection::Right
    ));
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"s")),
        Method::PaneStack(target) if target.pane_id == "pane_1"
    ));
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"h")),
        Method::PaneFocusDirection(params) if params.direction == PaneDirection::Left
    ));
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"z")),
        Method::PaneZoom(_)
    ));
    assert_eq!(state.mode, pane_mode());
}

#[test]
fn tab_mode_digits_switch_tabs_and_navigation_stays() {
    use super::super::modal::ModalMode;
    let mut state = shell();
    state.set_snapshot(Box::new(with_tabs(3, 0)));
    state.handle_input_bytes(CTRL_T);
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"2")),
        Method::TabFocus(target) if target.tab_id == "tab_2"
    ));
    assert!(state.handle_input_bytes(b"9").actions.is_empty());
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"l")),
        Method::TabFocus(target) if target.tab_id == "tab_2"
    ));
    assert_eq!(state.mode, modal(ModalMode::Tab));
    // prompt_new_tab_name is on by default: the name prompt is an overlay,
    // so the mode drops to the terminal behind it.
    state.handle_input_bytes(b"n");
    assert!(matches!(state.overlay, Some(ClientShellOverlay::Rename(_))));
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn resize_mode_is_table_driven_and_keeps_upstream_exits() {
    let mut state = shell();
    state.handle_input_bytes(CTRL_N);
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"l")),
        Method::PaneResize(params) if params.direction == PaneDirection::Right
    ));
    // decrease_left shrinks from the left: the border moves right.
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"H")),
        Method::PaneResize(params) if params.direction == PaneDirection::Right
    ));
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"+")),
        Method::PaneResize(params) if params.direction == PaneDirection::Right
    ));
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"\x1b[1;2D")),
        Method::PaneResize(params) if params.direction == PaneDirection::Left
    ));
    assert_eq!(state.mode, ClientShellMode::Resize);
    // `r` is resize_mode's prefix key; upstream's resize mode exits on it.
    assert!(state.handle_input_bytes(b"r").actions.is_empty());
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn move_mode_swaps_and_cycles() {
    use super::super::modal::ModalMode;
    let mut state = shell();
    state.handle_input_bytes(CTRL_H);
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"l")),
        Method::PaneSwap(params) if params.direction == Some(PaneDirection::Right)
    ));
    assert_eq!(state.mode, modal(ModalMode::Move));
}

#[test]
fn session_mode_moves_the_selection_and_enter_focuses_it_without_leaving() {
    use super::super::modal::ModalMode;
    let mut state = shell();
    state.set_snapshot(Box::new(with_workspaces(3)));
    state.handle_input_bytes(CTRL_O);
    assert_eq!(
        state
            .navigate_workspace_id
            .as_ref()
            .map(|target| target.workspace_id.as_str()),
        Some("ws_1")
    );

    let moved = state.handle_input_bytes(b"j");
    assert!(moved.actions.is_empty() && moved.requests.is_empty());
    assert_eq!(
        state
            .navigate_workspace_id
            .as_ref()
            .map(|target| target.workspace_id.as_str()),
        Some("ws_2")
    );
    let text = frame_text(&mut state);
    assert!(text.contains("SESSION"), "frame: {text:?}");

    assert!(matches!(
        only_method(&state.handle_input_bytes(ENTER)),
        Method::WorkspaceFocus(target) if target.workspace_id == "ws_2"
    ));
    assert_eq!(state.mode, modal(ModalMode::Session));
    assert_eq!(
        state
            .navigate_workspace_id
            .as_ref()
            .map(|target| target.workspace_id.as_str()),
        Some("ws_2")
    );

    state.handle_input_bytes(ESC);
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(state.navigate_workspace_id.is_none());
}

#[test]
fn session_actions_wait_while_the_selection_is_not_confirmable() {
    use super::super::modal::ModalMode;
    let mut state = shell();
    state.set_snapshot(Box::new(with_workspaces(2)));
    state.handle_input_bytes(CTRL_O);
    // Like upstream's Navigate mode: a selection that is not an available
    // workspace on the active machine blocks workspace and pane actions.
    if let Some(target) = state.navigate_workspace_id.as_mut() {
        target.workspace_id = "ws_gone".into();
    }
    let rename = state.handle_input_bytes(b"r");
    assert!(rename.actions.is_empty(), "no action is sent");
    assert!(state.overlay.is_none(), "rename does not open");
    let digit = state.handle_input_bytes(b"1");
    assert!(digit.actions.is_empty(), "no tab switch is sent");
    assert_eq!(state.mode, modal(ModalMode::Session));
}

#[test]
fn session_workspace_actions_target_the_selected_workspace() {
    let mut state = shell();
    state.set_snapshot(Box::new(with_workspaces(2)));
    state.handle_input_bytes(CTRL_O);
    state.handle_input_bytes(b"j");
    state.handle_input_bytes(b"r");
    let Some(ClientShellOverlay::Rename(rename)) = &state.overlay else {
        panic!("expected the rename overlay");
    };
    assert!(matches!(
        &rename.target,
        ClientRenameTarget::Workspace { workspace_id } if workspace_id == "ws_2"
    ));
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(state.navigate_workspace_id.is_none());
}

#[test]
fn an_overlay_opened_from_a_sticky_mode_lands_in_the_terminal() {
    let mut state = shell();
    state.handle_input_bytes(CTRL_P);
    state.handle_input_bytes(b"c");
    assert!(matches!(state.overlay, Some(ClientShellOverlay::Rename(_))));
    assert_eq!(state.mode, ClientShellMode::Terminal);
    state.handle_input_bytes(ESC);
    assert!(state.overlay.is_none());
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn closing_from_a_sticky_mode_drops_to_the_terminal() {
    let mut state = shell();
    state.handle_input_bytes(CTRL_P);
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"x")),
        Method::PaneClose(target) if target.pane_id == "pane_1"
    ));
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn a_direct_shortcut_in_a_sticky_mode_runs_and_drops_to_the_terminal() {
    let mut state = shell();
    state.handle_input_bytes(CTRL_P);
    assert!(matches!(
        only_method(&state.handle_input_bytes(b"\x1b[104;3u")),
        Method::PaneFocusDirection(params) if params.direction == PaneDirection::Left
    ));
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn navigate_mode_does_not_intercept_mode_entry_keys() {
    let mut state = shell();
    state.handle_input_bytes(CTRL_B);
    state.handle_input_bytes(b"w");
    assert_eq!(state.mode, ClientShellMode::Navigate);
    state.handle_input_bytes(CTRL_P);
    assert!(!matches!(state.mode, ClientShellMode::Modal(_)));
}

#[test]
fn held_sticky_actions_repeat_but_the_lock_key_does_not() {
    let mut state = shell();
    state.handle_input_bytes(CTRL_P);
    assert_eq!(state.handle_input_bytes(b"\x1b[104;1:1u").actions.len(), 1);
    assert_eq!(
        state.handle_input_bytes(b"\x1b[104;1:2u").actions.len(),
        1,
        "a held focus key keeps acting"
    );
    state.handle_input_bytes(b"\x1b[104;1:3u");
    state.handle_input_bytes(ESC);

    state.handle_input_bytes(b"\x1b[103;5:1u");
    assert!(state.modal_locked);
    let repeat = state.handle_input_bytes(b"\x1b[103;5:2u");
    assert!(state.modal_locked, "a held lock key must not toggle back");
    assert!(repeat.requests.is_empty());
}

#[test]
fn a_keymap_change_resets_a_sticky_mode() {
    let mut state = shell();
    state.set_snapshot(Box::new(with_workspaces(2)));
    state.handle_input_bytes(CTRL_O);
    state.handle_input_bytes(b"j");
    let mut changed = with_workspaces(2);
    changed.revision += 1;
    changed.commands.push(crate::protocol::ClientShellCommand {
        command_id: "cmd_new".into(),
        binding_label: "prefix+y".into(),
        binding_labels: vec!["prefix+y".into()],
        action: crate::protocol::ClientShellCommandAction::Shell,
        description: None,
    });
    state.set_snapshot(Box::new(changed));
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(state.navigate_workspace_id.is_none());
}

#[test]
fn a_sticky_mode_renders_its_mode_bar() {
    let mut state = shell();
    state.handle_input_bytes(CTRL_P);
    let text = frame_text(&mut state);
    assert!(text.contains("PANE"), "frame: {text:?}");
    assert!(text.contains("h/j/k/l"), "frame: {text:?}");
}

#[test]
fn an_async_worktree_action_from_session_drops_to_the_terminal() {
    let mut state = shell();
    state.set_snapshot(Box::new(with_workspaces(2)));
    state.handle_input_bytes(CTRL_O);
    state.handle_input_bytes(b"j");
    // N (new worktree) asks the server first; its overlay opens later.
    let worktree = state.handle_input_bytes(b"N");
    assert!(matches!(
        only_method(&worktree),
        Method::WorktreeList(params) if params.workspace_id.as_deref() == Some("ws_2")
    ));
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(state.navigate_workspace_id.is_none());
}
