//! PROBE: isolated sticky-mode key resolvers carved out of fork commit d6b4afa.
//! This file exists to measure how much of the modal-keybind cluster can land as
//! a conflict-free new module against upstream. The `*ModeBindings` structs and
//! `ModeBinding`/`mode_binding_matches` are inlined here as scaffolding; in the
//! real port they belong in src/config/keybinds.rs.

use crossterm::event::KeyCode;

use crate::app::state::{AppState, Mode};
use crate::config::KeyCombo;
use crate::input::TerminalKey;
use crate::layout::NavDirection;

use super::navigate::NavigateAction;

// --- scaffolding that really belongs in config/keybinds.rs -----------------

/// A single logical binding: any of these combos triggers the action.
pub type ModeBinding = Vec<KeyCombo>;

pub fn mode_binding_matches(binding: &ModeBinding, key: TerminalKey) -> bool {
    binding
        .iter()
        .any(|&combo| crate::config::terminal_key_matches_combo(key, combo))
}

#[derive(Debug, Clone, Default)]
pub struct PaneModeBindings {
    pub focus_left: ModeBinding,
    pub focus_down: ModeBinding,
    pub focus_up: ModeBinding,
    pub focus_right: ModeBinding,
    pub new_pane: ModeBinding,
    pub split_down: ModeBinding,
    pub split_right: ModeBinding,
    pub stack: ModeBinding,
    pub close: ModeBinding,
    pub zoom: ModeBinding,
    pub toggle_float: ModeBinding,
    pub rename: ModeBinding,
    pub cycle: ModeBinding,
}

#[derive(Debug, Clone, Default)]
pub struct TabModeBindings {
    pub previous: ModeBinding,
    pub next: ModeBinding,
    pub new: ModeBinding,
    pub close: ModeBinding,
    pub rename: ModeBinding,
    pub break_to_tab: ModeBinding,
    pub toggle: ModeBinding,
}

#[derive(Debug, Clone, Default)]
pub struct ResizeModeBindings {
    pub increase_left: ModeBinding,
    pub increase_down: ModeBinding,
    pub increase_up: ModeBinding,
    pub increase_right: ModeBinding,
    pub decrease_left: ModeBinding,
    pub decrease_down: ModeBinding,
    pub decrease_up: ModeBinding,
    pub decrease_right: ModeBinding,
    pub increase: ModeBinding,
    pub decrease: ModeBinding,
}

#[derive(Debug, Clone, Default)]
pub struct MoveModeBindings {
    pub move_left: ModeBinding,
    pub move_down: ModeBinding,
    pub move_up: ModeBinding,
    pub move_right: ModeBinding,
    pub cycle_forward: ModeBinding,
    pub cycle_backward: ModeBinding,
}

#[derive(Debug, Clone, Default)]
pub struct SessionModeBindings {
    pub workspace_up: ModeBinding,
    pub workspace_down: ModeBinding,
    pub focus_left: ModeBinding,
    pub focus_right: ModeBinding,
    pub cycle: ModeBinding,
    pub goto: ModeBinding,
    pub workspace_picker: ModeBinding,
    pub new_workspace: ModeBinding,
    pub new_worktree: ModeBinding,
    pub rename_workspace: ModeBinding,
    pub close_workspace: ModeBinding,
    pub settings: ModeBinding,
    pub help: ModeBinding,
    pub detach: ModeBinding,
    pub previous_agent: ModeBinding,
    pub next_agent: ModeBinding,
}

#[derive(Debug, Clone, Default)]
pub struct ModeEntryKeys {
    pub pane: Option<KeyCombo>,
    pub tab: Option<KeyCombo>,
    pub resize: Option<KeyCombo>,
    pub move_: Option<KeyCombo>,
    pub session: Option<KeyCombo>,
    pub locked: Option<KeyCombo>,
    pub tmux: Option<KeyCombo>,
}

// ---------------------------------------------------------------------------
// ModeAction and pure per-mode resolvers  (verbatim from d6b4afa)
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

    if ke.code == KeyCode::Enter {
        return ModeAction::SidebarConfirm;
    }

    if ke.modifiers.is_empty() {
        if let KeyCode::Char(c @ '1'..='9') = ke.code {
            let idx = (c as usize) - ('1' as usize);
            return ModeAction::Navigate(NavigateAction::SwitchTab(idx));
        }
    }

    let b = &state.keybinds.mode_session;
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
