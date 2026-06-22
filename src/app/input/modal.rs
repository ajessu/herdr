use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Direction, Rect};

use crate::{
    app::state::{
        AppState, ContextMenuKind, ContextMenuState, MenuListState, Mode, NavigatorStateFilter,
    },
    config::mode_binding_matches,
    input::TerminalKey,
    layout::NavDirection,
};

use super::navigate::NavigateAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ModalAction {
    Continue,
    Save,
    Clear,
    Cancel,
    Confirm,
    Apply,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ModalKeyBinding {
    Enter,
    Esc,
    CtrlC,
}

impl ModalKeyBinding {
    fn matches(self, key: &KeyEvent) -> bool {
        match self {
            Self::Enter => key.code == KeyCode::Enter,
            Self::Esc => key.code == KeyCode::Esc,
            Self::CtrlC => {
                key.code == KeyCode::Char('c')
                    && key.modifiers == crossterm::event::KeyModifiers::CONTROL
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ModalActionSpec<A> {
    pub action: A,
    pub bindings: &'static [ModalKeyBinding],
}

pub(super) fn modal_action_from_key<A: Copy>(
    key: &KeyEvent,
    specs: &[ModalActionSpec<A>],
) -> Option<A> {
    specs
        .iter()
        .find(|spec| spec.bindings.iter().any(|binding| binding.matches(key)))
        .map(|spec| spec.action)
}

pub(super) fn modal_action_from_buttons<A: Copy>(
    col: u16,
    row: u16,
    buttons: &[(Rect, A)],
) -> Option<A> {
    buttons.iter().find_map(|(rect, action)| {
        (col >= rect.x && col < rect.x + rect.width && row >= rect.y && row < rect.y + rect.height)
            .then_some(*action)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GlobalMenuAction {
    Detach,
    WhatsNew,
    Keybinds,
    ReloadConfig,
    Settings,
}

pub(super) fn global_menu_actions(state: &AppState) -> Vec<GlobalMenuAction> {
    let mut actions = vec![
        GlobalMenuAction::Settings,
        GlobalMenuAction::Keybinds,
        GlobalMenuAction::ReloadConfig,
    ];
    if state.update_available.is_some() || state.latest_release_notes_available {
        actions.push(GlobalMenuAction::WhatsNew);
    }
    actions.push(GlobalMenuAction::Detach);
    actions
}

pub(super) fn open_global_menu(state: &mut AppState) {
    state.global_menu = MenuListState::new(0);
    state.mode = Mode::GlobalMenu;
}

pub(super) fn open_keybind_help(state: &mut AppState) {
    state.keybind_help.scroll = 0;
    state.mode = Mode::KeybindHelp;
}

fn open_update_release_notes(state: &mut AppState) {
    let Some(notes) = crate::release_notes::load_latest() else {
        return;
    };

    state.release_notes = Some(crate::app::state::ReleaseNotesState {
        version: notes.version,
        body: notes.body,
        scroll: 0,
        preview: notes.preview,
    });
    state.mode = Mode::ReleaseNotes;
}

pub(super) fn request_detach(state: &mut AppState) {
    if state.mode.is_sticky() {
        state.mode = Mode::normal_mode(state.active.is_some());
    }
    if state.detach_exits {
        state.should_quit = true;
    } else {
        state.detach_requested = true;
    }
}

pub(super) fn apply_global_menu_action(state: &mut AppState, action: GlobalMenuAction) {
    match action {
        GlobalMenuAction::Detach => {
            leave_modal(state);
            request_detach(state);
        }
        GlobalMenuAction::WhatsNew => open_update_release_notes(state),
        GlobalMenuAction::Keybinds => open_keybind_help(state),
        GlobalMenuAction::ReloadConfig => {
            state.request_reload_config = true;
            leave_modal(state);
        }
        GlobalMenuAction::Settings => super::settings::open_settings(state),
    }
}

pub(crate) fn handle_global_menu_key(state: &mut AppState, key: KeyEvent) {
    let actions = global_menu_actions(state);
    match key.code {
        KeyCode::Esc => leave_modal(state),
        KeyCode::Up | KeyCode::Char('k') => state.global_menu.move_prev(),
        KeyCode::Down | KeyCode::Char('j') => state.global_menu.move_next(actions.len()),
        KeyCode::Enter => {
            if let Some(action) = actions.get(state.global_menu.highlighted).copied() {
                apply_global_menu_action(state, action);
            }
        }
        _ => {}
    }
}

pub(crate) fn handle_navigator_key(
    state: &mut AppState,
    terminal_runtimes: &crate::terminal::TerminalRuntimeRegistry,
    key: KeyEvent,
) {
    if state.navigator.search_focused {
        match key.code {
            KeyCode::Esc => {
                if state.navigator.query.is_empty() {
                    state.navigator.search_focused = false;
                    leave_modal(state);
                } else {
                    state.navigator.query.clear();
                    state.navigator.state_filter = None;
                    state.navigator.search_focused = false;
                    state.clamp_navigator_selection_from(terminal_runtimes);
                }
            }
            KeyCode::Enter => {
                state.accept_navigator_selection_from(terminal_runtimes);
            }
            KeyCode::Backspace => {
                state.navigator.state_filter = None;
                state.navigator.query.pop();
                state.clamp_navigator_selection_from(terminal_runtimes);
            }
            KeyCode::Up => state.move_navigator_selection_from(terminal_runtimes, -1),
            KeyCode::Down => state.move_navigator_selection_from(terminal_runtimes, 1),
            KeyCode::Char('n') if key.modifiers == KeyModifiers::CONTROL => {
                state.move_navigator_selection_from(terminal_runtimes, 1)
            }
            KeyCode::Char('p') if key.modifiers == KeyModifiers::CONTROL => {
                state.move_navigator_selection_from(terminal_runtimes, -1)
            }
            KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                state.navigator.query.clear();
                state.navigator.state_filter = None;
                state.clamp_navigator_selection_from(terminal_runtimes);
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                insert_navigator_search_text(state, terminal_runtimes, &c.to_string());
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc => {
            if state.navigator.query.is_empty() && state.navigator.state_filter.is_none() {
                leave_modal(state);
            } else {
                state.navigator.query.clear();
                state.navigator.state_filter = None;
                state.clamp_navigator_selection_from(terminal_runtimes);
            }
        }
        KeyCode::Enter => {
            state.accept_navigator_selection_from(terminal_runtimes);
        }
        KeyCode::Char('/') => {
            state.navigator.query.clear();
            state.navigator.state_filter = None;
            state.navigator.search_focused = true;
            state.clamp_navigator_selection_from(terminal_runtimes);
        }
        KeyCode::Backspace if state.navigator.state_filter.is_some() => {
            state.navigator.state_filter = None;
            state.clamp_navigator_selection_from(terminal_runtimes);
        }
        KeyCode::Char('a') if key.modifiers.is_empty() => {
            state.navigator.query.clear();
            state.navigator.state_filter = None;
            state.clamp_navigator_selection_from(terminal_runtimes);
        }
        KeyCode::Char('b') if key.modifiers.is_empty() => {
            state.navigator.query.clear();
            state.navigator.state_filter = Some(NavigatorStateFilter::Blocked);
            state.clamp_navigator_selection_from(terminal_runtimes);
        }
        KeyCode::Char('w') if key.modifiers.is_empty() => {
            state.navigator.query.clear();
            state.navigator.state_filter = Some(NavigatorStateFilter::Working);
            state.clamp_navigator_selection_from(terminal_runtimes);
        }
        KeyCode::Char('i') if key.modifiers.is_empty() => {
            state.navigator.query.clear();
            state.navigator.state_filter = Some(NavigatorStateFilter::Idle);
            state.clamp_navigator_selection_from(terminal_runtimes);
        }
        KeyCode::Char('d') if key.modifiers.is_empty() => {
            state.navigator.query.clear();
            state.navigator.state_filter = Some(NavigatorStateFilter::Done);
            state.clamp_navigator_selection_from(terminal_runtimes);
        }
        KeyCode::Char('j') | KeyCode::Down => {
            state.move_navigator_selection_from(terminal_runtimes, 1)
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.move_navigator_selection_from(terminal_runtimes, -1)
        }
        KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => state
            .move_navigator_selection_from(
                terminal_runtimes,
                (state.navigator_body_rect().height / 2).max(1) as isize,
            ),
        KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => state
            .move_navigator_selection_from(
                terminal_runtimes,
                -((state.navigator_body_rect().height / 2).max(1) as isize),
            ),
        KeyCode::Char(' ') => state.toggle_selected_navigator_workspace_from(terminal_runtimes),
        KeyCode::Home => {
            state.navigator.selected = 0;
            state.ensure_navigator_selection_visible_from(terminal_runtimes);
        }
        KeyCode::End | KeyCode::Char('G') => {
            state.navigator.selected = state
                .navigator_rows_from(terminal_runtimes)
                .len()
                .saturating_sub(1);
            state.ensure_navigator_selection_visible_from(terminal_runtimes);
        }
        _ => {}
    }
}

pub(crate) fn insert_navigator_search_text(
    state: &mut AppState,
    terminal_runtimes: &crate::terminal::TerminalRuntimeRegistry,
    text: &str,
) {
    if !state.navigator.search_focused {
        return;
    }
    state.navigator.state_filter = None;
    state.navigator.query.push_str(text);
    state.clamp_navigator_selection_from(terminal_runtimes);
}

pub(crate) fn handle_keybind_help_key(state: &mut AppState, key: KeyEvent) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => state.scroll_keybind_help(-1),
        KeyCode::Down | KeyCode::Char('j') => state.scroll_keybind_help(1),
        KeyCode::PageUp => state.scroll_keybind_help(-8),
        KeyCode::PageDown => state.scroll_keybind_help(8),
        KeyCode::Home => state.keybind_help.scroll = 0,
        KeyCode::End => state.keybind_help.scroll = state.keybind_help_max_scroll(),
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') => leave_modal(state),
        _ => {}
    }
}

pub(super) fn open_rename_workspace(
    state: &mut AppState,
    terminal_runtimes: &crate::terminal::TerminalRuntimeRegistry,
    ws_idx: usize,
) {
    state.selected = ws_idx;
    state.rename_pane_target = None;
    state.name_input =
        state.workspaces[ws_idx].display_name_from(&state.terminals, terminal_runtimes);
    state.name_input_replace_on_type = false;
    state.mode = Mode::RenameWorkspace;
}

pub(super) fn open_rename_active_tab(state: &mut AppState, replace_on_type: bool) {
    state.creating_new_tab = false;
    state.requested_new_tab_name = None;
    state.rename_pane_target = None;
    if let Some(ws) = state.active.and_then(|i| state.workspaces.get(i)) {
        if let Some(name) = ws.active_tab_display_name() {
            state.name_input = name;
            state.name_input_replace_on_type = replace_on_type;
            state.mode = Mode::RenameTab;
        }
    }
}

pub(super) fn open_rename_pane(state: &mut AppState, pane_id: crate::layout::PaneId) {
    let Some(ws) = state.active.and_then(|i| state.workspaces.get(i)) else {
        return;
    };
    let Some(pane) = ws.pane_state(pane_id) else {
        return;
    };
    let terminal = state.terminals.get(&pane.attached_terminal_id);
    state.creating_new_tab = false;
    state.requested_new_tab_name = None;
    state.rename_pane_target = Some(pane_id);
    state.name_input = terminal
        .and_then(|t| t.manual_label.clone())
        .unwrap_or_default();
    state.name_input_replace_on_type = terminal.and_then(|t| t.manual_label.as_ref()).is_none();
    state.mode = Mode::RenamePane;
}

fn next_new_tab_default_name(state: &AppState) -> String {
    state
        .active
        .and_then(|i| state.workspaces.get(i))
        .map(|ws| (ws.tabs.len() + 1).to_string())
        .unwrap_or_else(|| "1".to_string())
}

pub(super) fn open_new_tab_dialog(state: &mut AppState) {
    state.creating_new_tab = true;
    state.requested_new_tab_name = None;
    state.rename_pane_target = None;
    state.name_input = next_new_tab_default_name(state);
    state.name_input_replace_on_type = true;
    state.mode = Mode::RenameTab;
}

pub(super) fn leave_modal(state: &mut AppState) {
    if state.active.is_some() {
        state.mode = Mode::Terminal;
    } else {
        state.mode = Mode::Navigate;
    }
}

pub(super) const ONBOARDING_WELCOME_ACTIONS: &[ModalActionSpec<ModalAction>] = &[ModalActionSpec {
    action: ModalAction::Continue,
    bindings: &[ModalKeyBinding::Enter],
}];

pub(super) const RELEASE_NOTES_ACTIONS: &[ModalActionSpec<ModalAction>] = &[ModalActionSpec {
    action: ModalAction::Close,
    bindings: &[ModalKeyBinding::Enter, ModalKeyBinding::Esc],
}];

pub(super) const RENAME_ACTIONS: &[ModalActionSpec<ModalAction>] = &[
    ModalActionSpec {
        action: ModalAction::Save,
        bindings: &[ModalKeyBinding::Enter],
    },
    ModalActionSpec {
        action: ModalAction::Clear,
        bindings: &[ModalKeyBinding::CtrlC],
    },
    ModalActionSpec {
        action: ModalAction::Cancel,
        bindings: &[ModalKeyBinding::Esc],
    },
];

pub(super) const CONFIRM_CLOSE_ACTIONS: &[ModalActionSpec<ModalAction>] = &[
    ModalActionSpec {
        action: ModalAction::Confirm,
        bindings: &[ModalKeyBinding::Enter],
    },
    ModalActionSpec {
        action: ModalAction::Cancel,
        bindings: &[ModalKeyBinding::Esc],
    },
];

pub(super) const SETTINGS_ACTIONS: &[ModalActionSpec<ModalAction>] = &[
    ModalActionSpec {
        action: ModalAction::Apply,
        bindings: &[ModalKeyBinding::Enter],
    },
    ModalActionSpec {
        action: ModalAction::Close,
        bindings: &[ModalKeyBinding::Esc],
    },
];

pub(super) fn apply_rename_action(state: &mut AppState, action: ModalAction) {
    match action {
        ModalAction::Save => {
            let new_name = if state.name_input.trim().is_empty() {
                state.name_input.clone()
            } else {
                state.name_input.trim().to_string()
            };
            match state.mode {
                Mode::RenameWorkspace if !state.workspaces.is_empty() && !new_name.is_empty() => {
                    let workspace_id = state.workspaces[state.selected].id.clone();
                    state.workspaces[state.selected].set_custom_name(new_name);
                    crate::logging::workspace_renamed(&workspace_id);
                    state.mark_session_dirty();
                }
                Mode::RenameTab if state.creating_new_tab => {
                    state.request_new_tab = true;
                    let default_name = next_new_tab_default_name(state);
                    state.requested_new_tab_name =
                        if new_name.is_empty() || new_name == default_name {
                            None
                        } else {
                            Some(new_name)
                        };
                }
                Mode::RenameTab => {
                    if let Some(ws_idx) = state.active {
                        if let Some(ws) = state.workspaces.get_mut(ws_idx) {
                            let workspace_id = ws.id.clone();
                            let active_tab = ws.active_tab;
                            let keep_auto_name = ws
                                .tabs
                                .get(active_tab)
                                .is_some_and(|tab| tab.is_auto_named())
                                && ws
                                    .tab_display_name(active_tab)
                                    .is_some_and(|name| new_name == name);
                            if let Some(tab) = ws.active_tab_mut() {
                                if !new_name.is_empty() && !keep_auto_name {
                                    tab.set_custom_name(new_name);
                                    let tab_id = ws
                                        .public_tab_number(active_tab)
                                        .map(|number| {
                                            crate::workspace::public_tab_id_for_number(
                                                &workspace_id,
                                                number,
                                            )
                                        })
                                        .unwrap_or_else(|| workspace_id.clone());
                                    crate::logging::tab_renamed(&workspace_id, &tab_id);
                                    state.mark_session_dirty();
                                }
                            }
                        }
                    }
                }
                Mode::RenamePane => {
                    if let (Some(ws_idx), Some(pane_id)) = (state.active, state.rename_pane_target)
                    {
                        if let Some(ws) = state.workspaces.get(ws_idx) {
                            if let Some(pane) = ws.pane_state(pane_id) {
                                let terminal_id = pane.attached_terminal_id.clone();
                                if let Some(terminal) = state.terminals.get_mut(&terminal_id) {
                                    terminal.set_manual_label(new_name);
                                    state.mark_session_dirty();
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
            state.creating_new_tab = false;
            state.rename_pane_target = None;
            state.name_input.clear();
            state.name_input_replace_on_type = false;
            leave_modal(state);
        }
        ModalAction::Clear => {
            state.name_input.clear();
            state.name_input_replace_on_type = false;
        }
        ModalAction::Cancel => {
            state.creating_new_tab = false;
            state.requested_new_tab_name = None;
            state.rename_pane_target = None;
            state.name_input.clear();
            state.name_input_replace_on_type = false;
            leave_modal(state);
        }
        _ => {}
    }
}

fn clear_rename_input(state: &mut AppState) {
    state.name_input.clear();
    state.name_input_replace_on_type = false;
}

pub(crate) fn insert_rename_input_text(state: &mut AppState, text: &str) {
    if state.name_input_replace_on_type {
        clear_rename_input(state);
    }
    state.name_input.push_str(text);
}

fn delete_rename_input_char(state: &mut AppState) {
    if state.name_input_replace_on_type {
        clear_rename_input(state);
    } else {
        state.name_input.pop();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenameWordDeleteClass {
    Word,
    Separator,
}

fn rename_word_delete_class(ch: char) -> RenameWordDeleteClass {
    if ch.is_alphanumeric() || ch == '_' {
        RenameWordDeleteClass::Word
    } else {
        RenameWordDeleteClass::Separator
    }
}

fn delete_rename_input_word(state: &mut AppState) {
    if state.name_input_replace_on_type {
        clear_rename_input(state);
        return;
    }

    while state
        .name_input
        .chars()
        .last()
        .is_some_and(char::is_whitespace)
    {
        state.name_input.pop();
    }

    let Some(class) = state
        .name_input
        .chars()
        .last()
        .map(rename_word_delete_class)
    else {
        return;
    };

    while state
        .name_input
        .chars()
        .last()
        .is_some_and(|ch| !ch.is_whitespace() && rename_word_delete_class(ch) == class)
    {
        state.name_input.pop();
    }
}

pub(crate) fn handle_rename_key(state: &mut AppState, key: KeyEvent) {
    if let Some(action) = modal_action_from_key(&key, RENAME_ACTIONS) {
        apply_rename_action(state, action);
        return;
    }

    match key.code {
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            clear_rename_input(state);
        }
        KeyCode::Backspace if key.modifiers.contains(KeyModifiers::SUPER) => {
            clear_rename_input(state);
        }
        KeyCode::Backspace
            if key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::ALT) =>
        {
            delete_rename_input_word(state);
        }
        KeyCode::Char('h' | 'w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            delete_rename_input_word(state);
        }
        KeyCode::Backspace => delete_rename_input_char(state),
        KeyCode::Char(c) if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
            insert_rename_input_text(state, &c.to_string());
        }
        _ => {}
    }
}

#[cfg(test)]
pub(crate) fn handle_resize_key(state: &mut AppState, raw_key: TerminalKey) {
    let key = raw_key.as_key_event();
    if key.code == KeyCode::Esc
        || key.code == KeyCode::Enter
        || state.keybinds.resize_mode.matches_prefix_key(raw_key)
        || state.keybinds.resize_mode.matches_direct_key(raw_key)
    {
        if state.active.is_some() {
            state.mode = Mode::Terminal;
        } else {
            state.mode = Mode::Navigate;
        }
        return;
    }

    match key.code {
        KeyCode::Char('h') | KeyCode::Left => {
            state.resize_pane(NavDirection::Left);
        }
        KeyCode::Char('l') | KeyCode::Right => {
            state.resize_pane(NavDirection::Right);
        }
        KeyCode::Char('j') | KeyCode::Down => {
            state.resize_pane(NavDirection::Down);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.resize_pane(NavDirection::Up);
        }
        _ => {}
    }
}

pub(super) fn open_confirm_close(state: &mut AppState) {
    state.mode = Mode::ConfirmClose;
}

pub(super) fn confirm_close_accept(state: &mut AppState) {
    state.close_selected_workspace();
    if state.workspaces.is_empty() {
        state.mode = Mode::Navigate;
    } else {
        state.mode = Mode::Terminal;
    }
}

pub(super) fn confirm_close_cancel(state: &mut AppState) {
    state.mode = Mode::Navigate;
}

pub(crate) fn handle_confirm_close_key(state: &mut AppState, key: KeyEvent) {
    match modal_action_from_key(&key, CONFIRM_CLOSE_ACTIONS) {
        Some(ModalAction::Confirm) => confirm_close_accept(state),
        Some(ModalAction::Cancel) => confirm_close_cancel(state),
        _ => {}
    }
}

pub(super) fn apply_context_menu_action(
    state: &mut AppState,
    terminal_runtimes: &mut crate::terminal::TerminalRuntimeRegistry,
    menu: ContextMenuState,
    idx: usize,
) {
    let item = menu.items().get(idx).copied();
    match (menu.kind, item) {
        (ContextMenuKind::GitWorkspace { ws_idx, .. }, Some("New worktree")) => {
            state.request_new_linked_worktree = Some(ws_idx);
            leave_modal(state);
        }
        (ContextMenuKind::GitWorkspace { ws_idx, .. }, Some("Delete worktree checkout...")) => {
            state.request_remove_linked_worktree = Some(ws_idx);
            leave_modal(state);
        }
        (ContextMenuKind::GitWorkspace { ws_idx, .. }, Some("Open worktree...")) => {
            state.request_open_existing_worktree = Some(ws_idx);
            leave_modal(state);
        }
        (
            ContextMenuKind::GitWorkspace {
                ws_idx, collapsed, ..
            },
            Some("Collapse" | "Expand"),
        ) => {
            if let Some(key) = state
                .workspaces
                .get(ws_idx)
                .and_then(|ws| ws.worktree_space())
                .map(|space| space.key.clone())
            {
                if collapsed {
                    state.collapsed_space_keys.remove(&key);
                } else {
                    state.collapsed_space_keys.insert(key);
                }
                state.mark_session_dirty();
            }
            leave_modal(state);
        }
        (
            ContextMenuKind::Workspace { ws_idx } | ContextMenuKind::GitWorkspace { ws_idx, .. },
            Some("Rename"),
        ) => {
            open_rename_workspace(state, terminal_runtimes, ws_idx);
        }
        (
            ContextMenuKind::Workspace { ws_idx } | ContextMenuKind::GitWorkspace { ws_idx, .. },
            Some("Close" | "Close group"),
        ) => {
            state.selected = ws_idx;
            if state.confirm_close {
                open_confirm_close(state);
            } else {
                state.close_selected_workspace();
                state.mode = Mode::Navigate;
            }
        }
        (ContextMenuKind::Tab { ws_idx, tab_idx }, Some("New tab")) => {
            state.selected = ws_idx;
            state.active = Some(ws_idx);
            state.switch_tab(tab_idx);
            open_new_tab_dialog(state);
        }
        (ContextMenuKind::Tab { ws_idx, tab_idx }, Some("Rename")) => {
            state.selected = ws_idx;
            state.active = Some(ws_idx);
            state.switch_tab(tab_idx);
            open_rename_active_tab(state, false);
        }
        (ContextMenuKind::Tab { ws_idx, tab_idx }, Some("Close")) => {
            state.selected = ws_idx;
            state.active = Some(ws_idx);
            state.switch_tab(tab_idx);
            if !state.close_tab() {
                state.mode = if state.active.is_some() {
                    Mode::Terminal
                } else {
                    Mode::Navigate
                };
            }
        }
        (ContextMenuKind::Pane { pane_id, .. }, Some("Rename pane")) => {
            open_rename_pane(state, pane_id);
        }
        (
            ContextMenuKind::Pane {
                ws_idx, pane_id, ..
            },
            Some("Clear pane name"),
        ) => {
            if let Some(ws) = state.workspaces.get(ws_idx) {
                if let Some(pane) = ws.pane_state(pane_id) {
                    let terminal_id = pane.attached_terminal_id.clone();
                    if let Some(terminal) = state.terminals.get_mut(&terminal_id) {
                        terminal.clear_manual_label();
                        state.mark_session_dirty();
                    }
                }
            }
            state.mode = Mode::Terminal;
        }
        (
            ContextMenuKind::Pane {
                ws_idx,
                tab_idx,
                pane_id,
                source_pane_id,
                ..
            },
            Some("Swap with focused pane"),
        ) => {
            if let Some(source_pane_id) = source_pane_id {
                state.selected = ws_idx;
                state.active = Some(ws_idx);
                state.switch_tab(tab_idx);
                if let Some(tab) = state
                    .workspaces
                    .get_mut(ws_idx)
                    .and_then(|ws| ws.tabs.get_mut(tab_idx))
                {
                    if tab.layout.swap_panes(source_pane_id, pane_id) {
                        tab.layout.focus_pane(source_pane_id);
                        state.mark_session_dirty();
                    }
                }
            }
            state.mode = Mode::Terminal;
        }
        (
            ContextMenuKind::Pane {
                ws_idx,
                tab_idx,
                pane_id,
                ..
            },
            Some("Split right"),
        ) => {
            state.selected = ws_idx;
            state.active = Some(ws_idx);
            state.switch_tab(tab_idx);
            state.focus_pane_in_workspace(ws_idx, pane_id);
            state.split_pane(terminal_runtimes, Direction::Horizontal);
            state.mode = Mode::Terminal;
        }
        (
            ContextMenuKind::Pane {
                ws_idx,
                tab_idx,
                pane_id,
                ..
            },
            Some("Split down"),
        ) => {
            state.selected = ws_idx;
            state.active = Some(ws_idx);
            state.switch_tab(tab_idx);
            state.focus_pane_in_workspace(ws_idx, pane_id);
            state.split_pane(terminal_runtimes, Direction::Vertical);
            state.mode = Mode::Terminal;
        }
        (
            ContextMenuKind::Pane {
                ws_idx,
                tab_idx,
                pane_id,
                ..
            },
            Some("Zoom"),
        ) => {
            state.selected = ws_idx;
            state.active = Some(ws_idx);
            state.switch_tab(tab_idx);
            state.focus_pane_in_workspace(ws_idx, pane_id);
            state.toggle_zoom();
            state.mode = Mode::Terminal;
        }
        (
            ContextMenuKind::Pane {
                ws_idx,
                tab_idx,
                pane_id,
                ..
            },
            Some("Close pane"),
        ) => {
            state.selected = ws_idx;
            state.active = Some(ws_idx);
            state.switch_tab(tab_idx);
            state.focus_pane_in_workspace(ws_idx, pane_id);
            if !state.close_pane() {
                state.mode = if state.active.is_some() {
                    Mode::Terminal
                } else {
                    Mode::Navigate
                };
            }
        }
        _ => leave_modal(state),
    }
}

pub(crate) fn handle_context_menu_key(
    state: &mut AppState,
    terminal_runtimes: &mut crate::terminal::TerminalRuntimeRegistry,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => {
            state.context_menu = None;
            leave_modal(state);
        }
        KeyCode::Up => {
            if let Some(menu) = &mut state.context_menu {
                menu.list.move_prev();
            }
        }
        KeyCode::Down => {
            if let Some(menu) = &mut state.context_menu {
                menu.list.move_next(menu.items().len());
            }
        }
        KeyCode::Enter => {
            if let Some(menu) = state.context_menu.take() {
                let idx = menu.list.highlighted;
                apply_context_menu_action(state, terminal_runtimes, menu, idx);
            }
        }
        _ => {}
    }
}

impl AppState {
    pub(super) fn global_menu_item_at(&self, col: u16, row: u16) -> Option<GlobalMenuAction> {
        let rect = self.global_menu_rect();
        if col <= rect.x
            || col >= rect.x + rect.width.saturating_sub(1)
            || row <= rect.y
            || row >= rect.y + rect.height.saturating_sub(1)
        {
            return None;
        }
        let idx = (row - rect.y - 1) as usize;
        global_menu_actions(self).get(idx).copied()
    }
}

// ---------------------------------------------------------------------------
// ModeAction and pure per-mode resolvers
// ---------------------------------------------------------------------------

/// A resolved, mode-agnostic action returned by per-mode key resolvers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModeAction {
    /// Dispatch through the existing runtime-aware NavigateAction path.
    Navigate(NavigateAction),
    /// Sidebar movement (workspace selection up/down, pane focus left/right).
    SidebarNavigate(NavDirection),
    /// Confirm sidebar selection (switch to `state.selected` workspace).
    SidebarConfirm,
    /// Switch to a different mode.
    #[allow(dead_code)]
    EnterMode(Mode),
    /// Return to Normal (Terminal or Navigate depending on workspace state).
    ExitToNormal,
    /// Unrecognized key — deliberate swallow, no PTY forward.
    None,
}

/// Resolve a key press in Pane mode to a `ModeAction`.
pub(crate) fn pane_mode_action(state: &AppState, key: TerminalKey) -> ModeAction {
    let ke = key.as_key_event();
    if ke.code == KeyCode::Esc || ke.code == KeyCode::Enter {
        return ModeAction::ExitToNormal;
    }
    if let Some(combo) = state.keybinds.mode_entry.pane {
        if crate::config::terminal_key_matches_combo(key, combo) {
            return ModeAction::ExitToNormal;
        }
    }

    let b = &state.keybinds.mode_pane;
    if mode_binding_matches(&b.focus_left, key) {
        return ModeAction::Navigate(NavigateAction::FocusPaneLeft);
    }
    if mode_binding_matches(&b.focus_down, key) {
        return ModeAction::Navigate(NavigateAction::FocusPaneDown);
    }
    if mode_binding_matches(&b.focus_up, key) {
        return ModeAction::Navigate(NavigateAction::FocusPaneUp);
    }
    if mode_binding_matches(&b.focus_right, key) {
        return ModeAction::Navigate(NavigateAction::FocusPaneRight);
    }
    if mode_binding_matches(&b.new_pane, key) {
        return ModeAction::Navigate(NavigateAction::SplitAuto);
    }
    if mode_binding_matches(&b.split_down, key) {
        return ModeAction::Navigate(NavigateAction::SplitHorizontal);
    }
    if mode_binding_matches(&b.split_right, key) {
        return ModeAction::Navigate(NavigateAction::SplitVertical);
    }
    if mode_binding_matches(&b.stack, key) {
        return ModeAction::Navigate(NavigateAction::StackPane);
    }
    if mode_binding_matches(&b.close, key) {
        return ModeAction::Navigate(NavigateAction::ClosePane);
    }
    if mode_binding_matches(&b.zoom, key) {
        return ModeAction::Navigate(NavigateAction::Zoom);
    }
    if mode_binding_matches(&b.toggle_float, key) {
        return ModeAction::Navigate(NavigateAction::ToggleFloating);
    }
    if mode_binding_matches(&b.rename, key) {
        return ModeAction::Navigate(NavigateAction::RenamePane);
    }
    if mode_binding_matches(&b.cycle, key) {
        return ModeAction::Navigate(NavigateAction::CyclePaneNext);
    }
    ModeAction::None
}

/// Resolve a key press in Tab mode to a `ModeAction`.
pub(crate) fn tab_mode_action(state: &AppState, key: TerminalKey) -> ModeAction {
    let ke = key.as_key_event();
    if ke.code == KeyCode::Esc || ke.code == KeyCode::Enter {
        return ModeAction::ExitToNormal;
    }
    if let Some(combo) = state.keybinds.mode_entry.tab {
        if crate::config::terminal_key_matches_combo(key, combo) {
            return ModeAction::ExitToNormal;
        }
    }

    // 1-9 jump to tab N
    if ke.modifiers.is_empty() {
        if let KeyCode::Char(c @ '1'..='9') = ke.code {
            let idx = (c as usize) - ('1' as usize);
            return ModeAction::Navigate(NavigateAction::SwitchTab(idx));
        }
    }

    let b = &state.keybinds.mode_tab;
    if mode_binding_matches(&b.previous, key) {
        return ModeAction::Navigate(NavigateAction::PreviousTab);
    }
    if mode_binding_matches(&b.next, key) {
        return ModeAction::Navigate(NavigateAction::NextTab);
    }
    if mode_binding_matches(&b.new, key) {
        return ModeAction::Navigate(NavigateAction::NewTab);
    }
    if mode_binding_matches(&b.close, key) {
        return ModeAction::Navigate(NavigateAction::CloseTab);
    }
    if mode_binding_matches(&b.rename, key) {
        return ModeAction::Navigate(NavigateAction::RenameTab);
    }
    if mode_binding_matches(&b.break_to_tab, key) {
        return ModeAction::Navigate(NavigateAction::BreakPaneToTab);
    }
    if mode_binding_matches(&b.toggle, key) {
        return ModeAction::Navigate(NavigateAction::LastPane);
    }
    ModeAction::None
}

/// Resolve a key press in Resize mode to a `ModeAction`.
pub(crate) fn resize_mode_action(state: &AppState, key: TerminalKey) -> ModeAction {
    let ke = key.as_key_event();
    if ke.code == KeyCode::Esc || ke.code == KeyCode::Enter {
        return ModeAction::ExitToNormal;
    }
    if let Some(combo) = state.keybinds.mode_entry.resize {
        if crate::config::terminal_key_matches_combo(key, combo) {
            return ModeAction::ExitToNormal;
        }
    }

    let b = &state.keybinds.mode_resize;
    if mode_binding_matches(&b.increase_left, key) {
        return ModeAction::Navigate(NavigateAction::ResizeIncrease(NavDirection::Left));
    }
    if mode_binding_matches(&b.increase_down, key) {
        return ModeAction::Navigate(NavigateAction::ResizeIncrease(NavDirection::Down));
    }
    if mode_binding_matches(&b.increase_up, key) {
        return ModeAction::Navigate(NavigateAction::ResizeIncrease(NavDirection::Up));
    }
    if mode_binding_matches(&b.increase_right, key) {
        return ModeAction::Navigate(NavigateAction::ResizeIncrease(NavDirection::Right));
    }
    if mode_binding_matches(&b.decrease_left, key) {
        return ModeAction::Navigate(NavigateAction::ResizeDecrease(NavDirection::Left));
    }
    if mode_binding_matches(&b.decrease_down, key) {
        return ModeAction::Navigate(NavigateAction::ResizeDecrease(NavDirection::Down));
    }
    if mode_binding_matches(&b.decrease_up, key) {
        return ModeAction::Navigate(NavigateAction::ResizeDecrease(NavDirection::Up));
    }
    if mode_binding_matches(&b.decrease_right, key) {
        return ModeAction::Navigate(NavigateAction::ResizeDecrease(NavDirection::Right));
    }
    if mode_binding_matches(&b.increase, key) {
        return ModeAction::Navigate(NavigateAction::ResizeGrow);
    }
    if mode_binding_matches(&b.decrease, key) {
        return ModeAction::Navigate(NavigateAction::ResizeShrink);
    }
    ModeAction::None
}

/// Resolve a key press in Move mode to a `ModeAction`.
pub(crate) fn move_mode_action(state: &AppState, key: TerminalKey) -> ModeAction {
    let ke = key.as_key_event();
    if ke.code == KeyCode::Esc || ke.code == KeyCode::Enter {
        return ModeAction::ExitToNormal;
    }
    if let Some(combo) = state.keybinds.mode_entry.move_ {
        if crate::config::terminal_key_matches_combo(key, combo) {
            return ModeAction::ExitToNormal;
        }
    }

    let b = &state.keybinds.mode_move;
    if mode_binding_matches(&b.move_left, key) {
        return ModeAction::Navigate(NavigateAction::SwapPaneLeft);
    }
    if mode_binding_matches(&b.move_down, key) {
        return ModeAction::Navigate(NavigateAction::SwapPaneDown);
    }
    if mode_binding_matches(&b.move_up, key) {
        return ModeAction::Navigate(NavigateAction::SwapPaneUp);
    }
    if mode_binding_matches(&b.move_right, key) {
        return ModeAction::Navigate(NavigateAction::SwapPaneRight);
    }
    if mode_binding_matches(&b.cycle_forward, key) {
        return ModeAction::Navigate(NavigateAction::CyclePaneNext);
    }
    if mode_binding_matches(&b.cycle_backward, key) {
        return ModeAction::Navigate(NavigateAction::CyclePanePrevious);
    }
    ModeAction::None
}

/// Resolve a key press in Session/Navigate mode to a `ModeAction`.
/// Shared between `Mode::Session` and `Mode::Navigate`.
pub(crate) fn session_mode_action(state: &AppState, key: TerminalKey) -> ModeAction {
    let ke = key.as_key_event();
    if ke.code == KeyCode::Esc {
        return ModeAction::ExitToNormal;
    }
    if let Some(combo) = state.keybinds.mode_entry.session {
        if crate::config::terminal_key_matches_combo(key, combo) {
            return ModeAction::ExitToNormal;
        }
    }

    // Enter confirms selection (the executor interprets per-mode: Navigate
    // confirms+exits, Session confirms+stays).
    if ke.code == KeyCode::Enter {
        return ModeAction::SidebarConfirm;
    }

    // 1-9 jump to tab N
    if ke.modifiers.is_empty() {
        if let KeyCode::Char(c @ '1'..='9') = ke.code {
            let idx = (c as usize) - ('1' as usize);
            return ModeAction::Navigate(NavigateAction::SwitchTab(idx));
        }
    }

    let b = &state.keybinds.mode_session;
    // Sidebar navigation (pure — no runtime needed)
    if mode_binding_matches(&b.workspace_up, key) {
        return ModeAction::SidebarNavigate(NavDirection::Up);
    }
    if mode_binding_matches(&b.workspace_down, key) {
        return ModeAction::SidebarNavigate(NavDirection::Down);
    }
    if mode_binding_matches(&b.focus_left, key) {
        return ModeAction::SidebarNavigate(NavDirection::Left);
    }
    if mode_binding_matches(&b.focus_right, key) {
        return ModeAction::SidebarNavigate(NavDirection::Right);
    }
    if mode_binding_matches(&b.cycle, key) {
        return ModeAction::Navigate(NavigateAction::CyclePaneNext);
    }
    // Runtime-touching actions resolve to Navigate(NavigateAction) for the executor
    if mode_binding_matches(&b.goto, key) {
        return ModeAction::Navigate(NavigateAction::OpenNavigator);
    }
    if mode_binding_matches(&b.workspace_picker, key) {
        return ModeAction::Navigate(NavigateAction::WorkspacePicker);
    }
    if mode_binding_matches(&b.new_workspace, key) {
        return ModeAction::Navigate(NavigateAction::NewWorkspace);
    }
    if mode_binding_matches(&b.new_worktree, key) {
        return ModeAction::Navigate(NavigateAction::NewWorktree);
    }
    if mode_binding_matches(&b.rename_workspace, key) {
        return ModeAction::Navigate(NavigateAction::RenameWorkspace);
    }
    if mode_binding_matches(&b.close_workspace, key) {
        return ModeAction::Navigate(NavigateAction::CloseWorkspace);
    }
    if mode_binding_matches(&b.settings, key) {
        return ModeAction::Navigate(NavigateAction::Settings);
    }
    if mode_binding_matches(&b.help, key) {
        return ModeAction::Navigate(NavigateAction::Help);
    }
    if mode_binding_matches(&b.detach, key) {
        return ModeAction::Navigate(NavigateAction::Detach);
    }
    if mode_binding_matches(&b.previous_agent, key) {
        return ModeAction::Navigate(NavigateAction::PreviousAgent);
    }
    if mode_binding_matches(&b.next_agent, key) {
        return ModeAction::Navigate(NavigateAction::NextAgent);
    }
    ModeAction::None
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::layout::Rect;

    use super::super::{capture_snapshot, state_with_workspaces};
    use super::*;

    fn config_env_lock() -> &'static std::sync::Mutex<()> {
        crate::config::test_config_env_lock()
    }

    fn temp_config_path(name: &str) -> std::path::PathBuf {
        let unique = format!(
            "herdr-modal-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(unique).join("config.toml")
    }

    #[test]
    fn custom_resize_key_exits_resize_mode() {
        let mut state = state_with_workspaces(&["test"]);
        state.mode = Mode::Resize;
        state.keybinds.resize_mode = crate::config::ActionKeybinds::prefix("g");

        handle_resize_key(
            &mut state,
            TerminalKey::new(KeyCode::Char('g'), KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::Terminal);
    }

    #[test]
    fn direct_resize_key_exits_resize_mode() {
        let mut state = state_with_workspaces(&["test"]);
        state.mode = Mode::Resize;
        state.keybinds.resize_mode = crate::config::ActionKeybinds::direct("ctrl+alt+r");

        handle_resize_key(
            &mut state,
            TerminalKey::new(
                KeyCode::Char('r'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
        );

        assert_eq!(state.mode, Mode::Terminal);
    }

    #[test]
    fn resize_key_exit_matches_enhanced_shifted_punctuation() {
        let mut state = state_with_workspaces(&["test"]);
        state.mode = Mode::Resize;
        state.keybinds.resize_mode = crate::config::ActionKeybinds::prefix("?");

        handle_resize_key(
            &mut state,
            TerminalKey::new(KeyCode::Char('/'), KeyModifiers::SHIFT)
                .with_shifted_codepoint('?' as u32),
        );

        assert_eq!(state.mode, Mode::Terminal);
    }

    #[test]
    fn detach_requests_client_detach_in_persistence_mode() {
        let mut state = state_with_workspaces(&["test"]);
        state.detach_exits = false;

        request_detach(&mut state);

        assert!(state.detach_requested);
        assert!(!state.should_quit);
    }

    #[test]
    fn detach_exits_in_no_session_mode() {
        let mut state = state_with_workspaces(&["test"]);
        state.detach_exits = true;

        request_detach(&mut state);

        assert!(state.should_quit);
        assert!(!state.detach_requested);
    }

    #[test]
    fn global_menu_whats_new_opens_saved_release_notes() {
        let _guard = config_env_lock().lock().unwrap();
        let path = temp_config_path("whats-new-saved-release-notes");
        std::env::set_var(crate::config::CONFIG_PATH_ENV_VAR, &path);
        crate::release_notes::save_pending(env!("CARGO_PKG_VERSION"), "### Changed\n- Menu")
            .unwrap();

        let mut state = state_with_workspaces(&["test"]);
        state.latest_release_notes_available = true;

        assert!(global_menu_actions(&state).contains(&GlobalMenuAction::WhatsNew));

        apply_global_menu_action(&mut state, GlobalMenuAction::WhatsNew);

        assert_eq!(state.mode, Mode::ReleaseNotes);
        assert_eq!(
            state
                .release_notes
                .as_ref()
                .map(|notes| notes.body.as_str()),
            Some("### Changed\n- Menu")
        );

        std::env::remove_var(crate::config::CONFIG_PATH_ENV_VAR);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn rename_modal_keyboard_and_mouse_share_actions() {
        let mut state = state_with_workspaces(&["test"]);
        state.mode = Mode::RenameWorkspace;
        state.name_input = "hello".into();

        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );
        assert!(state.name_input.is_empty());

        state.name_input = "renamed".into();
        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );
        assert_eq!(state.mode, Mode::Terminal);
        assert_eq!(state.workspaces[0].display_name(), "renamed");
        let snapshot = capture_snapshot(&state);
        assert_eq!(
            snapshot.workspaces[0].custom_name.as_deref(),
            Some("renamed")
        );

        state.view.sidebar_rect = Rect::new(0, 0, 26, 20);
        state.view.terminal_area = Rect::new(26, 0, 80, 20);
        state.mode = Mode::RenameWorkspace;
        state.name_input = "mouse".into();
        let inner = state.rename_modal_inner().unwrap();
        let (save, _, _) = crate::ui::rename_button_rects(inner);
        let action = modal_action_from_buttons(save.x, save.y, &[(save, ModalAction::Save)]);
        assert_eq!(action, Some(ModalAction::Save));
    }

    #[test]
    fn tab_rename_updates_captured_snapshot() {
        let mut state = state_with_workspaces(&["test"]);
        state.mode = Mode::RenameTab;
        state.name_input = "logs".into();

        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        let snapshot = capture_snapshot(&state);
        assert_eq!(
            snapshot.workspaces[0].tabs[0].custom_name.as_deref(),
            Some("logs")
        );
    }

    #[test]
    fn rename_cancel_returns_to_terminal_when_workspace_is_active() {
        let mut state = state_with_workspaces(&["test"]);
        state.mode = Mode::RenameTab;
        state.name_input = "test".into();

        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::Terminal);
        assert!(state.name_input.is_empty());
    }

    #[test]
    fn rename_modal_replaces_prefilled_text_on_first_type() {
        let mut state = state_with_workspaces(&["test"]);
        state.mode = Mode::RenameTab;
        state.name_input = "2".into();
        state.name_input_replace_on_type = true;

        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::empty()),
        );
        assert_eq!(state.name_input, "n");
        assert!(!state.name_input_replace_on_type);

        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::empty()),
        );
        assert_eq!(state.name_input, "ne");
    }

    #[test]
    fn rename_modal_replaces_prefilled_text_on_paste() {
        let mut state = state_with_workspaces(&["test"]);
        state.mode = Mode::RenameTab;
        state.name_input = "2".into();
        state.name_input_replace_on_type = true;

        insert_rename_input_text(&mut state, "feature/logs");

        assert_eq!(state.name_input, "feature/logs");
        assert!(!state.name_input_replace_on_type);

        insert_rename_input_text(&mut state, "-copy");

        assert_eq!(state.name_input, "feature/logs-copy");
    }

    #[test]
    fn rename_modal_handles_line_editing_shortcuts() {
        let mut state = state_with_workspaces(&["test"]);
        state.mode = Mode::RenameWorkspace;
        state.name_input = "website zero".into();

        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()),
        );
        assert_eq!(state.name_input, "website zer");

        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL),
        );
        assert_eq!(state.name_input, "website ");

        state.name_input = "website-zero".into();
        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT),
        );
        assert_eq!(state.name_input, "website-");

        state.name_input = "website-zero".into();
        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.name_input, "website-");

        state.name_input = "website-zero".into();
        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.name_input, "website-");

        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::SUPER),
        );
        assert!(state.name_input.is_empty());

        state.name_input = "website zero".into();
        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        );
        assert!(state.name_input.is_empty());
    }

    #[test]
    fn rename_modal_does_not_insert_modified_shortcut_chars() {
        let mut state = state_with_workspaces(&["test"]);
        state.mode = Mode::RenameWorkspace;
        state.name_input = "website".into();

        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.name_input, "website");

        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::SHIFT),
        );
        assert_eq!(state.name_input, "websiteZ");
    }

    #[test]
    fn navigator_search_accepts_pasted_text_when_focused() {
        let mut state = state_with_workspaces(&["alpha", "beta"]);
        let terminal_runtimes = crate::terminal::TerminalRuntimeRegistry::new();
        state.mode = Mode::Navigator;
        state.navigator.search_focused = true;
        state.navigator.state_filter = Some(NavigatorStateFilter::Working);

        insert_navigator_search_text(&mut state, &terminal_runtimes, "beta");

        assert_eq!(state.navigator.query, "beta");
        assert_eq!(state.navigator.state_filter, None);
    }

    #[test]
    fn navigator_search_ignores_paste_when_search_is_not_focused() {
        let mut state = state_with_workspaces(&["alpha", "beta"]);
        let terminal_runtimes = crate::terminal::TerminalRuntimeRegistry::new();
        state.mode = Mode::Navigator;
        state.navigator.search_focused = false;

        insert_navigator_search_text(&mut state, &terminal_runtimes, "beta");

        assert!(state.navigator.query.is_empty());
    }

    #[test]
    fn open_rename_active_tab_can_prefill_default_new_tab_name() {
        let mut state = state_with_workspaces(&["test"]);
        state.workspaces[0].test_add_tab(None);
        state.workspaces[0].switch_tab(1);

        open_rename_active_tab(&mut state, true);

        assert_eq!(state.mode, Mode::RenameTab);
        assert_eq!(state.name_input, "2");
        assert!(state.name_input_replace_on_type);
    }

    #[test]
    fn cancel_new_tab_dialog_leaves_workspace_unchanged() {
        let mut state = state_with_workspaces(&["test"]);
        open_new_tab_dialog(&mut state);

        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::Terminal);
        assert!(!state.creating_new_tab);
        assert!(!state.request_new_tab);
        assert!(state.requested_new_tab_name.is_none());
        assert_eq!(state.workspaces[0].tabs.len(), 1);
    }

    #[test]
    fn saving_new_tab_dialog_requests_creation_with_name() {
        let mut state = state_with_workspaces(&["test"]);
        open_new_tab_dialog(&mut state);
        state.name_input = "logs".into();
        state.name_input_replace_on_type = false;

        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::Terminal);
        assert!(!state.creating_new_tab);
        assert!(state.request_new_tab);
        assert_eq!(state.requested_new_tab_name.as_deref(), Some("logs"));
    }

    #[test]
    fn saving_new_tab_dialog_with_default_name_keeps_tab_auto_named() {
        let mut state = state_with_workspaces(&["test"]);
        open_new_tab_dialog(&mut state);

        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::Terminal);
        assert!(!state.creating_new_tab);
        assert!(state.request_new_tab);
        assert!(state.requested_new_tab_name.is_none());
    }

    #[test]
    fn closing_first_auto_tab_compacts_remaining_auto_tab_label_and_next_prompt() {
        let mut state = state_with_workspaces(&["test"]);
        open_new_tab_dialog(&mut state);
        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        state.workspaces[0].test_add_tab(state.requested_new_tab_name.as_deref());
        state.request_new_tab = false;
        state.requested_new_tab_name = None;

        state.workspaces[0].close_tab(0);
        state.workspaces[0].switch_tab(0);

        assert_eq!(
            state.workspaces[0].tab_display_name(0).as_deref(),
            Some("1")
        );
        assert!(state.workspaces[0].tabs[0].custom_name.is_none());

        open_new_tab_dialog(&mut state);
        assert_eq!(state.name_input, "2");
    }

    #[test]
    fn renaming_auto_tab_to_its_default_number_keeps_it_auto_named() {
        let mut state = state_with_workspaces(&["test"]);
        state.workspaces[0].test_add_tab(None);
        state.workspaces[0].switch_tab(1);

        open_rename_active_tab(&mut state, false);
        handle_rename_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::Terminal);
        assert!(state.workspaces[0].tabs[1].custom_name.is_none());
        assert_eq!(
            state.workspaces[0].tab_display_name(1).as_deref(),
            Some("2")
        );
    }

    #[test]
    fn confirm_close_keyboard_actions_are_direct_not_focused() {
        let mut state = state_with_workspaces(&["a", "b"]);
        state.mode = Mode::ConfirmClose;
        state.selected = 1;

        handle_confirm_close_key(
            &mut state,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()),
        );
        assert_eq!(state.mode, Mode::Navigate);
        assert_eq!(state.workspaces.len(), 2);

        state.mode = Mode::ConfirmClose;
        handle_confirm_close_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );
        assert_eq!(state.workspaces.len(), 1);
    }

    #[test]
    fn confirm_close_for_linked_worktree_closes_workspace_only() {
        let mut state = state_with_workspaces(&["main", "issue"]);
        state.mode = Mode::ConfirmClose;
        state.selected = 1;
        state.workspaces[1].worktree_space = Some(crate::workspace::WorktreeSpaceMembership {
            key: "repo-key".into(),
            label: "herdr".into(),
            repo_root: "/repo/herdr".into(),
            checkout_path: "/repo/herdr-issue".into(),
            is_linked_worktree: true,
        });

        handle_confirm_close_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(state.request_remove_linked_worktree, None);
        assert_eq!(state.workspaces.len(), 1);
        assert_eq!(state.workspaces[0].display_name(), "main");
        assert_eq!(state.mode, Mode::Terminal);
    }

    #[test]
    fn context_menu_close_group_opens_group_close_confirmation() {
        let mut state = state_with_workspaces(&["main", "issue"]);
        state.active = Some(0);
        state.selected = 1;
        state.workspaces[0].worktree_space = Some(crate::workspace::WorktreeSpaceMembership {
            key: "repo-key".into(),
            label: "herdr".into(),
            repo_root: "/repo/herdr".into(),
            checkout_path: "/repo/herdr".into(),
            is_linked_worktree: false,
        });
        state.workspaces[1].worktree_space = Some(crate::workspace::WorktreeSpaceMembership {
            key: "repo-key".into(),
            label: "herdr".into(),
            repo_root: "/repo/herdr".into(),
            checkout_path: "/repo/herdr-issue".into(),
            is_linked_worktree: true,
        });
        let menu = ContextMenuState {
            kind: ContextMenuKind::GitWorkspace {
                ws_idx: 0,
                is_linked_worktree: false,
                has_worktree_children: true,
                collapsed: false,
            },
            x: 0,
            y: 0,
            list: MenuListState::new(0),
        };
        let mut terminal_runtimes = crate::terminal::TerminalRuntimeRegistry::new();

        apply_context_menu_action(&mut state, &mut terminal_runtimes, menu, 1);

        assert_eq!(state.selected, 0);
        assert_eq!(state.mode, Mode::ConfirmClose);

        confirm_close_accept(&mut state);

        assert!(state.workspaces.is_empty());
        assert_eq!(state.mode, Mode::Navigate);
    }

    #[test]
    fn context_menu_close_pane_last_parent_group_pane_keeps_confirmation_mode() {
        let mut state = state_with_workspaces(&["main", "issue"]);
        state.active = Some(0);
        state.selected = 1;
        state.workspaces[0].worktree_space = Some(crate::workspace::WorktreeSpaceMembership {
            key: "repo-key".into(),
            label: "herdr".into(),
            repo_root: "/repo/herdr".into(),
            checkout_path: "/repo/herdr".into(),
            is_linked_worktree: false,
        });
        state.workspaces[1].worktree_space = Some(crate::workspace::WorktreeSpaceMembership {
            key: "repo-key".into(),
            label: "herdr".into(),
            repo_root: "/repo/herdr".into(),
            checkout_path: "/repo/herdr-issue".into(),
            is_linked_worktree: true,
        });
        let pane_id = state.workspaces[0].tabs[0].root_pane;
        let menu = ContextMenuState {
            kind: ContextMenuKind::Pane {
                ws_idx: 0,
                tab_idx: 0,
                pane_id,
                source_pane_id: None,
                has_manual_label: false,
            },
            x: 0,
            y: 0,
            list: MenuListState::new(0),
        };
        let idx = menu
            .items()
            .iter()
            .position(|item| *item == "Close pane")
            .expect("close pane item");
        let mut terminal_runtimes = crate::terminal::TerminalRuntimeRegistry::new();

        apply_context_menu_action(&mut state, &mut terminal_runtimes, menu, idx);

        assert_eq!(state.selected, 0);
        assert_eq!(state.mode, Mode::ConfirmClose);
        assert_eq!(state.workspaces.len(), 2);
    }

    // -----------------------------------------------------------------------
    // ModeAction resolver tests
    // -----------------------------------------------------------------------

    fn key(code: KeyCode) -> TerminalKey {
        TerminalKey::new(code, KeyModifiers::empty())
    }

    fn key_mod(code: KeyCode, mods: KeyModifiers) -> TerminalKey {
        TerminalKey::new(code, mods)
    }

    // -- Pane mode resolver --

    #[test]
    fn pane_mode_esc_exits() {
        let state = AppState::test_new();
        assert_eq!(
            pane_mode_action(&state, key(KeyCode::Esc)),
            ModeAction::ExitToNormal
        );
    }

    #[test]
    fn pane_mode_enter_exits() {
        let state = AppState::test_new();
        assert_eq!(
            pane_mode_action(&state, key(KeyCode::Enter)),
            ModeAction::ExitToNormal
        );
    }

    #[test]
    fn pane_mode_self_entry_key_exits() {
        let state = AppState::test_new();
        // Default entry key for pane is Ctrl+p
        let k = key_mod(KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert_eq!(pane_mode_action(&state, k), ModeAction::ExitToNormal);
    }

    #[test]
    fn pane_mode_h_focuses_left() {
        let state = AppState::test_new();
        assert_eq!(
            pane_mode_action(&state, key(KeyCode::Char('h'))),
            ModeAction::Navigate(NavigateAction::FocusPaneLeft)
        );
    }

    #[test]
    fn pane_mode_j_focuses_down() {
        let state = AppState::test_new();
        assert_eq!(
            pane_mode_action(&state, key(KeyCode::Char('j'))),
            ModeAction::Navigate(NavigateAction::FocusPaneDown)
        );
    }

    #[test]
    fn pane_mode_left_arrow_focuses_left() {
        let state = AppState::test_new();
        assert_eq!(
            pane_mode_action(&state, key(KeyCode::Left)),
            ModeAction::Navigate(NavigateAction::FocusPaneLeft)
        );
    }

    #[test]
    fn pane_mode_n_splits_auto() {
        let state = AppState::test_new();
        assert_eq!(
            pane_mode_action(&state, key(KeyCode::Char('n'))),
            ModeAction::Navigate(NavigateAction::SplitAuto)
        );
    }

    #[test]
    fn pane_mode_x_closes() {
        let state = AppState::test_new();
        assert_eq!(
            pane_mode_action(&state, key(KeyCode::Char('x'))),
            ModeAction::Navigate(NavigateAction::ClosePane)
        );
    }

    #[test]
    fn pane_mode_f_zooms() {
        let state = AppState::test_new();
        assert_eq!(
            pane_mode_action(&state, key(KeyCode::Char('f'))),
            ModeAction::Navigate(NavigateAction::Zoom)
        );
    }

    #[test]
    fn pane_mode_z_zooms() {
        let state = AppState::test_new();
        assert_eq!(
            pane_mode_action(&state, key(KeyCode::Char('z'))),
            ModeAction::Navigate(NavigateAction::Zoom)
        );
    }

    #[test]
    fn pane_mode_unrecognized_returns_none() {
        let state = AppState::test_new();
        assert_eq!(
            pane_mode_action(&state, key(KeyCode::Char('q'))),
            ModeAction::None
        );
    }

    // -- Tab mode resolver --

    #[test]
    fn tab_mode_esc_exits() {
        let state = AppState::test_new();
        assert_eq!(
            tab_mode_action(&state, key(KeyCode::Esc)),
            ModeAction::ExitToNormal
        );
    }

    #[test]
    fn tab_mode_enter_exits() {
        let state = AppState::test_new();
        assert_eq!(
            tab_mode_action(&state, key(KeyCode::Enter)),
            ModeAction::ExitToNormal
        );
    }

    #[test]
    fn tab_mode_self_entry_key_exits() {
        let state = AppState::test_new();
        let k = key_mod(KeyCode::Char('t'), KeyModifiers::CONTROL);
        assert_eq!(tab_mode_action(&state, k), ModeAction::ExitToNormal);
    }

    #[test]
    fn tab_mode_h_previous_tab() {
        let state = AppState::test_new();
        assert_eq!(
            tab_mode_action(&state, key(KeyCode::Char('h'))),
            ModeAction::Navigate(NavigateAction::PreviousTab)
        );
    }

    #[test]
    fn tab_mode_l_next_tab() {
        let state = AppState::test_new();
        assert_eq!(
            tab_mode_action(&state, key(KeyCode::Char('l'))),
            ModeAction::Navigate(NavigateAction::NextTab)
        );
    }

    #[test]
    fn tab_mode_digit_switches_tab() {
        let state = AppState::test_new();
        assert_eq!(
            tab_mode_action(&state, key(KeyCode::Char('3'))),
            ModeAction::Navigate(NavigateAction::SwitchTab(2))
        );
    }

    #[test]
    fn tab_mode_n_new_tab() {
        let state = AppState::test_new();
        assert_eq!(
            tab_mode_action(&state, key(KeyCode::Char('n'))),
            ModeAction::Navigate(NavigateAction::NewTab)
        );
    }

    #[test]
    fn tab_mode_unrecognized_returns_none() {
        let state = AppState::test_new();
        assert_eq!(
            tab_mode_action(&state, key(KeyCode::Char('q'))),
            ModeAction::None
        );
    }

    // -- Resize mode resolver --

    #[test]
    fn resize_mode_esc_exits() {
        let state = AppState::test_new();
        assert_eq!(
            resize_mode_action(&state, key(KeyCode::Esc)),
            ModeAction::ExitToNormal
        );
    }

    #[test]
    fn resize_mode_enter_exits() {
        let state = AppState::test_new();
        assert_eq!(
            resize_mode_action(&state, key(KeyCode::Enter)),
            ModeAction::ExitToNormal
        );
    }

    #[test]
    fn resize_mode_self_entry_key_exits() {
        let state = AppState::test_new();
        let k = key_mod(KeyCode::Char('n'), KeyModifiers::CONTROL);
        assert_eq!(resize_mode_action(&state, k), ModeAction::ExitToNormal);
    }

    #[test]
    fn resize_mode_h_increases_left() {
        let state = AppState::test_new();
        assert_eq!(
            resize_mode_action(&state, key(KeyCode::Char('h'))),
            ModeAction::Navigate(NavigateAction::ResizeIncrease(NavDirection::Left))
        );
    }

    #[test]
    fn resize_mode_j_increases_down() {
        let state = AppState::test_new();
        assert_eq!(
            resize_mode_action(&state, key(KeyCode::Char('j'))),
            ModeAction::Navigate(NavigateAction::ResizeIncrease(NavDirection::Down))
        );
    }

    #[test]
    fn resize_mode_shift_h_decreases_left() {
        let state = AppState::test_new();
        // 'H' is Shift+h; parse_key_combo("H") produces (Char('h'), SHIFT)
        let k = key_mod(KeyCode::Char('h'), KeyModifiers::SHIFT);
        assert_eq!(
            resize_mode_action(&state, k),
            ModeAction::Navigate(NavigateAction::ResizeDecrease(NavDirection::Left))
        );
    }

    #[test]
    fn resize_mode_shift_l_decreases_right() {
        let state = AppState::test_new();
        let k = key_mod(KeyCode::Char('l'), KeyModifiers::SHIFT);
        assert_eq!(
            resize_mode_action(&state, k),
            ModeAction::Navigate(NavigateAction::ResizeDecrease(NavDirection::Right))
        );
    }

    #[test]
    fn resize_mode_plus_increases() {
        let state = AppState::test_new();
        assert_eq!(
            resize_mode_action(&state, key(KeyCode::Char('+'))),
            ModeAction::Navigate(NavigateAction::ResizeGrow)
        );
    }

    #[test]
    fn resize_mode_equals_increases() {
        let state = AppState::test_new();
        assert_eq!(
            resize_mode_action(&state, key(KeyCode::Char('='))),
            ModeAction::Navigate(NavigateAction::ResizeGrow)
        );
    }

    #[test]
    fn resize_mode_minus_decreases() {
        let state = AppState::test_new();
        assert_eq!(
            resize_mode_action(&state, key(KeyCode::Char('-'))),
            ModeAction::Navigate(NavigateAction::ResizeShrink)
        );
    }

    #[test]
    fn resize_mode_unrecognized_returns_none() {
        let state = AppState::test_new();
        assert_eq!(
            resize_mode_action(&state, key(KeyCode::Char('q'))),
            ModeAction::None
        );
    }

    // -- Move mode resolver --

    #[test]
    fn move_mode_esc_exits() {
        let state = AppState::test_new();
        assert_eq!(
            move_mode_action(&state, key(KeyCode::Esc)),
            ModeAction::ExitToNormal
        );
    }

    #[test]
    fn move_mode_enter_exits() {
        let state = AppState::test_new();
        assert_eq!(
            move_mode_action(&state, key(KeyCode::Enter)),
            ModeAction::ExitToNormal
        );
    }

    #[test]
    fn move_mode_self_entry_key_exits() {
        let state = AppState::test_new();
        let k = key_mod(KeyCode::Char('h'), KeyModifiers::CONTROL);
        assert_eq!(move_mode_action(&state, k), ModeAction::ExitToNormal);
    }

    #[test]
    fn move_mode_h_swaps_left() {
        let state = AppState::test_new();
        assert_eq!(
            move_mode_action(&state, key(KeyCode::Char('h'))),
            ModeAction::Navigate(NavigateAction::SwapPaneLeft)
        );
    }

    #[test]
    fn move_mode_j_swaps_down() {
        let state = AppState::test_new();
        assert_eq!(
            move_mode_action(&state, key(KeyCode::Char('j'))),
            ModeAction::Navigate(NavigateAction::SwapPaneDown)
        );
    }

    #[test]
    fn move_mode_n_cycles_forward() {
        let state = AppState::test_new();
        assert_eq!(
            move_mode_action(&state, key(KeyCode::Char('n'))),
            ModeAction::Navigate(NavigateAction::CyclePaneNext)
        );
    }

    #[test]
    fn move_mode_p_cycles_backward() {
        let state = AppState::test_new();
        assert_eq!(
            move_mode_action(&state, key(KeyCode::Char('p'))),
            ModeAction::Navigate(NavigateAction::CyclePanePrevious)
        );
    }

    #[test]
    fn move_mode_unrecognized_returns_none() {
        let state = AppState::test_new();
        assert_eq!(
            move_mode_action(&state, key(KeyCode::Char('q'))),
            ModeAction::None
        );
    }

    // -- Session mode resolver --

    #[test]
    fn session_mode_esc_exits() {
        let state = AppState::test_new();
        assert_eq!(
            session_mode_action(&state, key(KeyCode::Esc)),
            ModeAction::ExitToNormal
        );
    }

    #[test]
    fn session_mode_self_entry_key_exits() {
        let state = AppState::test_new();
        let k = key_mod(KeyCode::Char('o'), KeyModifiers::CONTROL);
        assert_eq!(session_mode_action(&state, k), ModeAction::ExitToNormal);
    }

    #[test]
    fn session_mode_enter_confirms() {
        let state = AppState::test_new();
        assert_eq!(
            session_mode_action(&state, key(KeyCode::Enter)),
            ModeAction::SidebarConfirm
        );
    }

    #[test]
    fn session_mode_k_sidebar_up() {
        let state = AppState::test_new();
        assert_eq!(
            session_mode_action(&state, key(KeyCode::Char('k'))),
            ModeAction::SidebarNavigate(NavDirection::Up)
        );
    }

    #[test]
    fn session_mode_j_sidebar_down() {
        let state = AppState::test_new();
        assert_eq!(
            session_mode_action(&state, key(KeyCode::Char('j'))),
            ModeAction::SidebarNavigate(NavDirection::Down)
        );
    }

    #[test]
    fn session_mode_h_sidebar_left() {
        let state = AppState::test_new();
        assert_eq!(
            session_mode_action(&state, key(KeyCode::Char('h'))),
            ModeAction::SidebarNavigate(NavDirection::Left)
        );
    }

    #[test]
    fn session_mode_g_opens_navigator() {
        let state = AppState::test_new();
        assert_eq!(
            session_mode_action(&state, key(KeyCode::Char('g'))),
            ModeAction::Navigate(NavigateAction::OpenNavigator)
        );
    }

    #[test]
    fn session_mode_digit_switches_tab() {
        let state = AppState::test_new();
        assert_eq!(
            session_mode_action(&state, key(KeyCode::Char('5'))),
            ModeAction::Navigate(NavigateAction::SwitchTab(4))
        );
    }

    #[test]
    fn session_mode_x_closes_workspace() {
        let state = AppState::test_new();
        assert_eq!(
            session_mode_action(&state, key(KeyCode::Char('x'))),
            ModeAction::Navigate(NavigateAction::CloseWorkspace)
        );
    }

    #[test]
    fn session_mode_unrecognized_returns_none() {
        let state = AppState::test_new();
        assert_eq!(
            session_mode_action(&state, key(KeyCode::Char('z'))),
            ModeAction::None
        );
    }

    // -- Output contract tests: verify the exact set of variants each resolver can emit --

    #[test]
    fn pane_resolver_emits_only_expected_variants() {
        let state = AppState::test_new();
        let all_keys: Vec<TerminalKey> = "hjklndsxfwcp"
            .chars()
            .map(|c| key(KeyCode::Char(c)))
            .chain([
                key(KeyCode::Left),
                key(KeyCode::Right),
                key(KeyCode::Up),
                key(KeyCode::Down),
            ])
            .chain([key(KeyCode::Char('z'))])
            .collect();
        for k in all_keys {
            let action = pane_mode_action(&state, k);
            match action {
                ModeAction::Navigate(nav) => {
                    assert!(
                        matches!(
                            nav,
                            NavigateAction::FocusPaneLeft
                                | NavigateAction::FocusPaneDown
                                | NavigateAction::FocusPaneUp
                                | NavigateAction::FocusPaneRight
                                | NavigateAction::SplitAuto
                                | NavigateAction::SplitHorizontal
                                | NavigateAction::SplitVertical
                                | NavigateAction::StackPane
                                | NavigateAction::ClosePane
                                | NavigateAction::Zoom
                                | NavigateAction::ToggleFloating
                                | NavigateAction::RenamePane
                                | NavigateAction::CyclePaneNext
                        ),
                        "unexpected Navigate variant: {nav:?}"
                    );
                }
                ModeAction::ExitToNormal | ModeAction::None => {}
                other => panic!("unexpected ModeAction from pane resolver: {other:?}"),
            }
        }
    }

    #[test]
    fn resize_resolver_emits_only_expected_variants() {
        let state = AppState::test_new();
        let all_keys: Vec<TerminalKey> = "hjkl"
            .chars()
            .map(|c| key(KeyCode::Char(c)))
            .chain(
                "hjkl"
                    .chars()
                    .map(|c| key_mod(KeyCode::Char(c), KeyModifiers::SHIFT)),
            )
            .chain([
                key(KeyCode::Char('+')),
                key(KeyCode::Char('=')),
                key(KeyCode::Char('-')),
            ])
            .chain([
                key(KeyCode::Left),
                key(KeyCode::Right),
                key(KeyCode::Up),
                key(KeyCode::Down),
            ])
            .collect();
        for k in all_keys {
            let action = resize_mode_action(&state, k);
            match action {
                ModeAction::Navigate(nav) => {
                    assert!(
                        matches!(
                            nav,
                            NavigateAction::ResizeIncrease(_)
                                | NavigateAction::ResizeDecrease(_)
                                | NavigateAction::ResizeGrow
                                | NavigateAction::ResizeShrink
                        ),
                        "unexpected Navigate variant: {nav:?}"
                    );
                }
                ModeAction::ExitToNormal | ModeAction::None => {}
                other => panic!("unexpected ModeAction from resize resolver: {other:?}"),
            }
        }
    }
}
