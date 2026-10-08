//! Fork: client side of the modal layer: the sticky-mode router, locked mode,
//! and dispatch for the fork's direct actions.
//!
//! Upstream calls in at four points: the top of the Terminal arm of
//! `route_key_press` (locked mode and entry keys), the `Modal` arm and
//! `route_resize_key` (sticky keys), and `record_binding` (direct actions).

use crate::config::StickyMode;
use crate::input::{KeybindAction, KeybindMatch, ModeEntry};

use super::*;

/// The sticky modes other than Resize, which keeps upstream's own variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModalMode {
    Pane,
    Tab,
    Move,
    Session,
}

impl ModalMode {
    pub(super) fn sticky(self) -> StickyMode {
        match self {
            ModalMode::Pane => StickyMode::Pane,
            ModalMode::Tab => StickyMode::Tab,
            ModalMode::Move => StickyMode::Move,
            ModalMode::Session => StickyMode::Session,
        }
    }

    fn label(self) -> &'static str {
        match self {
            ModalMode::Pane => " PANE ",
            ModalMode::Tab => " TAB ",
            ModalMode::Move => " MOVE ",
            ModalMode::Session => " SESSION ",
        }
    }
}

fn shell_mode(mode: StickyMode) -> ClientShellMode {
    match mode {
        StickyMode::Pane => ClientShellMode::Modal(ModalMode::Pane),
        StickyMode::Tab => ClientShellMode::Modal(ModalMode::Tab),
        StickyMode::Resize => ClientShellMode::Resize,
        StickyMode::Move => ClientShellMode::Modal(ModalMode::Move),
        StickyMode::Session => ClientShellMode::Modal(ModalMode::Session),
    }
}

/// Actions that end a sticky mode even though they open no overlay right away:
/// closes, detach, and the worktree actions, whose overlays open later.
fn ends_sticky_mode(action: KeybindAction) -> bool {
    matches!(
        action,
        KeybindAction::Detach
            | KeybindAction::ClosePane
            | KeybindAction::CloseTab
            | KeybindAction::CloseWorkspace
            | KeybindAction::NewWorktree
            | KeybindAction::OpenWorktree
            | KeybindAction::RemoveWorktree
    )
}

impl ClientShellState {
    /// Navigate and Session mode both move upstream's workspace cursor
    /// (`navigate_workspace_id`), so its gating applies to both.
    pub(super) fn navigates_workspaces(&self) -> bool {
        matches!(
            self.mode,
            ClientShellMode::Navigate | ClientShellMode::Modal(ModalMode::Session)
        )
    }

    /// Terminal-mode hook. `Some(target)` means the key was handled here:
    /// locked mode forwards everything except the unlock key, and mode-entry
    /// keys open their mode. `None` lets upstream routing continue.
    pub(super) fn route_modal_terminal_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) -> Option<Option<ClientInputTarget>> {
        // A stale Session cursor would retarget workspace actions.
        if self.navigate_workspace_id.is_some() {
            self.navigate_workspace_id = None;
        }
        let keybinds = &self.config.keybinds.keybinds;
        if self.modal_locked {
            let unlock = &keybinds.modal.entry.locked;
            // Fail open: a keymap without an unlock key cannot keep the client locked.
            if unlock.bindings.is_empty() {
                self.modal_locked = false;
            } else if unlock.matches_direct_key(key) {
                self.modal_locked = false;
                outcome.repaint = true;
                return Some(None);
            } else {
                return Some(self.focused_pane_id().map(ClientInputTarget::Pane));
            }
        }
        let entry = crate::input::resolve_mode_entry(keybinds, key)?;
        self.enter_modal(entry, outcome);
        Some(None)
    }

    fn enter_modal(&mut self, entry: ModeEntry, outcome: &mut ClientShellInput) {
        let leaving_session = self.mode == ClientShellMode::Modal(ModalMode::Session);
        match entry {
            ModeEntry::Locked => {
                self.mode = self.copy_or_terminal_mode();
                self.modal_locked = true;
            }
            ModeEntry::Sticky(mode) => self.mode = shell_mode(mode),
        }
        if self.mode == ClientShellMode::Modal(ModalMode::Session) {
            if !leaving_session {
                self.navigate_workspace_id = self.focused_navigation_target();
            }
        } else {
            self.navigate_workspace_id = None;
        }
        outcome.repaint = true;
    }

    fn leave_sticky(&mut self, outcome: &mut ClientShellInput) {
        if self.mode == ClientShellMode::Modal(ModalMode::Session) {
            self.navigate_workspace_id = None;
        }
        self.mode = self.copy_or_terminal_mode();
        outcome.repaint = true;
    }

    /// One key while a sticky mode is active. Unbound keys are swallowed.
    pub(super) fn route_sticky_key(
        &mut self,
        mode: StickyMode,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) {
        let keybinds = &self.config.keybinds.keybinds;
        let (code, modifiers) = crate::config::normalize_key_combo((key.code, key.modifiers));
        let unmodified = modifiers.is_empty();

        let session_enter = mode == StickyMode::Session && code == KeyCode::Enter && unmodified;
        if mode == StickyMode::Session {
            self.pending_workspace_highlight = None;
        }
        let upstream_resize_exit = mode == StickyMode::Resize
            && (keybinds.resize_mode.matches_prefix_key(key)
                || keybinds.resize_mode.matches_direct_key(key));
        if code == KeyCode::Esc
            || (code == KeyCode::Enter && !session_enter)
            || upstream_resize_exit
        {
            self.leave_sticky(outcome);
            return;
        }
        if let Some(entry) = crate::input::resolve_mode_entry(keybinds, key) {
            if entry == ModeEntry::Sticky(mode) {
                self.leave_sticky(outcome);
            } else {
                self.enter_modal(entry, outcome);
            }
            return;
        }
        if self.config.keybinds.matches_prefix(key) {
            if mode == StickyMode::Session {
                self.navigate_workspace_id = None;
            }
            self.mode = ClientShellMode::Prefix;
            outcome.repaint = true;
            return;
        }
        if let Some(binding) = crate::input::resolve_direct_binding(keybinds, key) {
            self.leave_sticky(outcome);
            self.record_binding(binding, outcome);
            return;
        }
        if matches!(mode, StickyMode::Tab | StickyMode::Session) && unmodified {
            if let KeyCode::Char(digit @ '1'..='9') = code {
                let index = usize::from(digit as u8 - b'1');
                let binding = KeybindMatch::Action(KeybindAction::SwitchTab(index));
                if self.session_preview_blocked(mode, outcome) {
                    return;
                }
                if self.indexed_navigation_target_exists(&binding) {
                    self.record_binding(binding, outcome);
                }
                return;
            }
        }
        if session_enter {
            // Upstream's Navigate accept (cross-machine aware) drops to the
            // terminal; Session mode stays active on the focused workspace.
            let target = self.navigate_workspace_id.clone();
            self.accept_navigate_workspace(outcome);
            if self.mode != ClientShellMode::Modal(ModalMode::Session) {
                self.mode = ClientShellMode::Modal(ModalMode::Session);
                self.navigate_workspace_id = target;
            }
            return;
        }
        let action = crate::input::resolve_sticky_action(keybinds, mode, key).or_else(|| {
            // Upstream's resize mode ignored modifiers on the arrow keys.
            let arrow = matches!(
                code,
                KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
            );
            (mode == StickyMode::Resize && arrow && !unmodified).then(|| {
                let bare =
                    crate::input::TerminalKey::new(code, crossterm::event::KeyModifiers::empty());
                crate::input::resolve_sticky_action(keybinds, mode, &bare)
            })?
        });
        match action {
            Some(crate::config::ModalAction::SessionWorkspaceUp) => {
                self.move_navigate_workspace(-1);
                outcome.repaint = true;
            }
            Some(crate::config::ModalAction::SessionWorkspaceDown) => {
                self.move_navigate_workspace(1);
                outcome.repaint = true;
            }
            Some(crate::config::ModalAction::Action(action)) => {
                if self.session_preview_blocked(mode, outcome) {
                    return;
                }
                self.run_sticky_action(action, outcome)
            }
            None => {}
        }
    }

    /// Session mode can preview another machine's workspace; like upstream's
    /// Navigate mode, workspace and pane actions wait until Enter confirms it.
    fn session_preview_blocked(
        &mut self,
        mode: StickyMode,
        outcome: &mut ClientShellInput,
    ) -> bool {
        if mode != StickyMode::Session || !self.workspace_preview_action_blocked() {
            return false;
        }
        self.push_endpoint_notice(
            ClientEndpointNoticeKind::Rejected,
            "navigate_endpoint_inactive",
            "Confirm workspace first",
            "Select an available workspace and press Enter before using workspace or pane actions",
        );
        outcome.repaint = true;
        true
    }

    /// Run a table action and decide where the mode lands: an action that set
    /// its own mode keeps it, closes and newly opened overlays drop to the
    /// terminal, and everything else stays in the sticky mode.
    fn run_sticky_action(&mut self, action: KeybindAction, outcome: &mut ClientShellInput) {
        let mode_before = self.mode;
        let had_overlay = self.overlay.is_some();
        self.record_binding(KeybindMatch::Action(action), outcome);
        outcome.repaint = true;
        if self.mode != mode_before {
            return;
        }
        if ends_sticky_mode(action) || (!had_overlay && self.overlay.is_some()) {
            self.leave_sticky(outcome);
        }
    }

    /// Dispatch an action that only the fork defines. Returns false for every
    /// upstream action so `record_binding` carries on unchanged.
    pub(super) fn record_modal_keybind_action(
        &mut self,
        action: KeybindAction,
        outcome: &mut ClientShellInput,
    ) -> bool {
        use crate::api::schema::{Method, PaneDirection, PaneResizeParams, PaneTarget};

        match action {
            KeybindAction::SplitAuto => {
                let split = if self.focused_pane_is_wide() {
                    KeybindAction::SplitVertical
                } else {
                    KeybindAction::SplitHorizontal
                };
                self.record_binding(KeybindMatch::Action(split), outcome);
            }
            KeybindAction::MoveTabLeft | KeybindAction::MoveTabRight => {
                let focused_tab = self
                    .snapshot
                    .as_deref()
                    .and_then(|snapshot| snapshot.focused_tab_id.clone());
                if let Some(method) = focused_tab.and_then(|tab_id| {
                    self.non_wrapping_tab_move(&tab_id, action == KeybindAction::MoveTabRight)
                }) {
                    self.push_endpoint_method(method, outcome);
                }
            }
            KeybindAction::ResizeGrow | KeybindAction::ResizeShrink => {
                let Some(pane_id) = self.focused_pane_id() else {
                    return true;
                };
                // The fork's grow/shrink: try the horizontal border first and
                // fall back to the vertical one when nothing moved.
                let (direction, fallback) = if action == KeybindAction::ResizeGrow {
                    (PaneDirection::Right, PaneDirection::Down)
                } else {
                    (PaneDirection::Left, PaneDirection::Up)
                };
                self.push_endpoint_method_with_kind(
                    Method::PaneResize(PaneResizeParams {
                        pane_id: Some(pane_id.clone()),
                        direction,
                        amount: None,
                    }),
                    PendingEndpointKind::ModalResizeFallback { pane_id, fallback },
                    outcome,
                );
            }
            KeybindAction::StackPane | KeybindAction::UnstackPane => {
                let Some(pane_id) = self.focused_pane_id() else {
                    return true;
                };
                let target = PaneTarget { pane_id };
                let method = if action == KeybindAction::StackPane {
                    Method::PaneStack(target)
                } else {
                    Method::PaneUnstack(target)
                };
                self.push_endpoint_method(method, outcome);
            }
            _ => return false,
        }
        true
    }

    /// Finish a grow/shrink: when the first direction moved no border, resize
    /// along the fallback axis instead.
    pub(super) fn complete_modal_resize(
        &mut self,
        pane_id: String,
        fallback: crate::api::schema::PaneDirection,
        result: Result<crate::api::schema::ResponseResult, ClientShellEndpointError>,
    ) -> Vec<ClientShellAction> {
        let unchanged = matches!(
            result,
            Ok(crate::api::schema::ResponseResult::PaneResize { ref resize }) if !resize.changed
        );
        let pane_exists = self
            .snapshot
            .as_deref()
            .is_some_and(|snapshot| snapshot.panes.iter().any(|pane| pane.pane_id == pane_id));
        if !unchanged || !pane_exists {
            return Vec::new();
        }
        let mut outcome = ClientShellInput::default();
        self.push_endpoint_method(
            crate::api::schema::Method::PaneResize(crate::api::schema::PaneResizeParams {
                pane_id: Some(pane_id),
                direction: fallback,
                amount: None,
            }),
            &mut outcome,
        );
        outcome.actions
    }

    /// The fork splits along the longer side: right when the pane is more than
    /// 1.5x as wide as it is tall (in cells), otherwise down.
    fn focused_pane_is_wide(&self) -> bool {
        let Some(focused) = self.focused_pane_id() else {
            return false;
        };
        self.pane_surface
            .as_ref()
            .and_then(|surface| surface.panes.iter().find(|pane| pane.pane_id == focused))
            .is_some_and(|pane| f32::from(pane.rect.width) > f32::from(pane.rect.height) * 1.5)
    }

    /// Like upstream's MoveTabPrevious/MoveTabNext, but stops at the ends.
    pub(super) fn non_wrapping_tab_move(
        &self,
        tab_id: &str,
        right: bool,
    ) -> Option<crate::api::schema::Method> {
        let snapshot = self.snapshot.as_deref()?;
        let workspace_id = &snapshot
            .tabs
            .iter()
            .find(|tab| tab.tab_id == tab_id)?
            .workspace_id;
        let tab_id = tab_id.to_owned();
        let tabs: Vec<_> = snapshot
            .tabs
            .iter()
            .filter(|tab| &tab.workspace_id == workspace_id)
            .collect();
        let source = tabs.iter().position(|tab| tab.tab_id == tab_id)?;
        let insert_index = if right {
            (source + 1 < tabs.len()).then_some(source + 2)?
        } else {
            source.checked_sub(1)?
        };
        Some(crate::api::schema::Method::TabMove(
            crate::api::schema::TabMoveParams {
                tab_id,
                insert_index,
            },
        ))
    }
}

/// Mode-bar content for a sticky mode: the badge, then each table entry. Runs
/// of entries whose descriptions share a first word ("focus left", "focus
/// down", ...) collapse into one "h/j/k/l focus" segment.
pub(super) fn mode_bar_segments(
    mode: ModalMode,
    keybinds: &crate::config::Keybinds,
    key: Style,
    base: Style,
    badge: Style,
) -> Vec<(String, Style)> {
    let mut segments = vec![
        (mode.label().to_owned(), badge),
        ("  ".to_owned(), base),
        ("esc".to_owned(), key),
        (" done  ".to_owned(), base),
    ];
    let mut groups: Vec<(Vec<String>, &str)> = Vec::new();
    for entry in &keybinds.modal.mode(mode.sticky()).entries {
        let Some(first) = entry.bindings.bindings.first() else {
            continue;
        };
        let word = entry
            .description
            .split_whitespace()
            .next()
            .unwrap_or(entry.description);
        match groups.last_mut() {
            Some((keys, last)) if *last == word => keys.push(first.label.clone()),
            _ => groups.push((vec![first.label.clone()], word)),
        }
    }
    for (keys, word) in groups {
        let description = if keys.len() == 1 {
            keybinds
                .modal
                .mode(mode.sticky())
                .entries
                .iter()
                .find(|entry| {
                    entry
                        .bindings
                        .bindings
                        .first()
                        .is_some_and(|binding| binding.label == keys[0])
                })
                .map_or(word, |entry| entry.description)
        } else {
            word
        };
        segments.push((keys.join("/"), key));
        segments.push((format!(" {description}  "), base));
    }
    segments
}
