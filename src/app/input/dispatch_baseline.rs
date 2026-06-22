//! Characterization tests capturing dispatch behavior as a regression oracle.

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
    use ratatui::layout::Rect;

    use super::super::{app_for_mouse_test, mouse, state_with_workspaces};
    use crate::app::input::modal::handle_resize_key;
    use crate::app::input::navigate::{
        handle_navigate_key, terminal_direct_navigation_action, NavigateAction,
    };
    use crate::app::state::{AppState, Mode};
    use crate::app::App;
    use crate::config::Config;
    use crate::input::TerminalKey;
    use crate::terminal::TerminalRuntimeRegistry;
    use crate::workspace::Workspace;

    // -----------------------------------------------------------------------
    // Terminal mode baseline — direct keybinds
    // -----------------------------------------------------------------------

    #[test]
    fn baseline_terminal_alt_h_resolves_to_focus_pane_left() {
        let state = state_with_workspaces(&["test"]);
        let action = terminal_direct_navigation_action(
            &state,
            TerminalKey::new(KeyCode::Char('h'), KeyModifiers::ALT),
        );
        assert_eq!(action, Some(NavigateAction::FocusPaneLeft));
    }

    #[test]
    fn baseline_terminal_alt_j_resolves_to_focus_pane_down() {
        let state = state_with_workspaces(&["test"]);
        let action = terminal_direct_navigation_action(
            &state,
            TerminalKey::new(KeyCode::Char('j'), KeyModifiers::ALT),
        );
        assert_eq!(action, Some(NavigateAction::FocusPaneDown));
    }

    #[test]
    fn baseline_terminal_alt_k_resolves_to_focus_pane_up() {
        let state = state_with_workspaces(&["test"]);
        let action = terminal_direct_navigation_action(
            &state,
            TerminalKey::new(KeyCode::Char('k'), KeyModifiers::ALT),
        );
        assert_eq!(action, Some(NavigateAction::FocusPaneUp));
    }

    #[test]
    fn baseline_terminal_alt_l_resolves_to_focus_pane_right() {
        let state = state_with_workspaces(&["test"]);
        let action = terminal_direct_navigation_action(
            &state,
            TerminalKey::new(KeyCode::Char('l'), KeyModifiers::ALT),
        );
        assert_eq!(action, Some(NavigateAction::FocusPaneRight));
    }

    #[test]
    fn baseline_terminal_alt_n_resolves_to_split_auto() {
        let state = state_with_workspaces(&["test"]);
        let action = terminal_direct_navigation_action(
            &state,
            TerminalKey::new(KeyCode::Char('n'), KeyModifiers::ALT),
        );
        assert_eq!(action, Some(NavigateAction::SplitAuto));
    }

    #[test]
    fn baseline_terminal_alt_x_resolves_to_close_pane() {
        let state = state_with_workspaces(&["test"]);
        let action = terminal_direct_navigation_action(
            &state,
            TerminalKey::new(KeyCode::Char('x'), KeyModifiers::ALT),
        );
        assert_eq!(action, Some(NavigateAction::ClosePane));
    }

    #[test]
    fn baseline_terminal_alt_equals_resolves_to_resize_grow() {
        let state = state_with_workspaces(&["test"]);
        let action = terminal_direct_navigation_action(
            &state,
            TerminalKey::new(KeyCode::Char('='), KeyModifiers::ALT),
        );
        assert_eq!(action, Some(NavigateAction::ResizeGrow));
    }

    #[test]
    fn baseline_terminal_alt_minus_resolves_to_resize_shrink() {
        let state = state_with_workspaces(&["test"]);
        let action = terminal_direct_navigation_action(
            &state,
            TerminalKey::new(KeyCode::Char('-'), KeyModifiers::ALT),
        );
        assert_eq!(action, Some(NavigateAction::ResizeShrink));
    }

    #[test]
    fn baseline_terminal_alt_i_resolves_to_move_tab_left() {
        let state = state_with_workspaces(&["test"]);
        let action = terminal_direct_navigation_action(
            &state,
            TerminalKey::new(KeyCode::Char('i'), KeyModifiers::ALT),
        );
        assert_eq!(action, Some(NavigateAction::MoveTabLeft));
    }

    #[test]
    fn baseline_terminal_alt_o_resolves_to_move_tab_right() {
        let state = state_with_workspaces(&["test"]);
        let action = terminal_direct_navigation_action(
            &state,
            TerminalKey::new(KeyCode::Char('o'), KeyModifiers::ALT),
        );
        assert_eq!(action, Some(NavigateAction::MoveTabRight));
    }

    #[test]
    fn baseline_terminal_ctrl_b_enters_prefix_mode() {
        let mut state = AppState::test_new();
        state.workspaces = vec![Workspace::test_new("test")];
        state.active = Some(0);
        state.mode = Mode::Terminal;

        let key = TerminalKey::new(KeyCode::Char('b'), KeyModifiers::CONTROL);
        assert!(state.is_prefix_key(key));
    }

    #[test]
    fn baseline_terminal_unbound_key_does_not_match_action() {
        let state = state_with_workspaces(&["test"]);
        let action = terminal_direct_navigation_action(
            &state,
            TerminalKey::new(KeyCode::Char('q'), KeyModifiers::empty()),
        );
        assert_eq!(action, None);
    }

    // -----------------------------------------------------------------------
    // Prefix mode baseline (one-shot)
    // -----------------------------------------------------------------------

    #[test]
    fn baseline_prefix_action_returns_to_normal_after_action() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = Some(0);
        state.mode = Mode::Prefix;

        let mut terminal_runtimes = TerminalRuntimeRegistry::new();
        crate::app::input::navigate::execute_navigate_action_in_context(
            &mut state,
            &mut terminal_runtimes,
            NavigateAction::NewWorkspace,
            crate::app::input::navigate::ActionContext::Prefix,
        );

        assert!(state.request_new_workspace);
        assert_eq!(state.mode, Mode::Terminal);
    }

    #[test]
    fn baseline_prefix_action_returns_to_navigate_when_no_workspace() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = None;
        state.mode = Mode::Prefix;

        let mut terminal_runtimes = TerminalRuntimeRegistry::new();
        crate::app::input::navigate::execute_navigate_action_in_context(
            &mut state,
            &mut terminal_runtimes,
            NavigateAction::NewWorkspace,
            crate::app::input::navigate::ActionContext::Prefix,
        );

        assert!(state.request_new_workspace);
        assert_eq!(state.mode, Mode::Navigate);
    }

    #[test]
    fn baseline_prefix_new_workspace_one_shot() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = Some(0);
        state.mode = Mode::Prefix;

        let mut terminal_runtimes = TerminalRuntimeRegistry::new();
        crate::app::input::navigate::execute_navigate_action_in_context(
            &mut state,
            &mut terminal_runtimes,
            NavigateAction::NewWorkspace,
            crate::app::input::navigate::ActionContext::Prefix,
        );

        assert_eq!(state.mode, Mode::Terminal);
    }

    // -----------------------------------------------------------------------
    // Navigate mode baseline
    // -----------------------------------------------------------------------

    #[test]
    fn baseline_navigate_esc_leaves_to_terminal() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = Some(0);
        state.mode = Mode::Navigate;

        handle_navigate_key(
            &mut state,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::Terminal);
    }

    #[test]
    fn baseline_navigate_esc_stays_navigate_without_active() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = None;
        state.mode = Mode::Navigate;

        handle_navigate_key(
            &mut state,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::Navigate);
    }

    #[test]
    fn baseline_navigate_enter_switches_workspace() {
        let mut state = state_with_workspaces(&["ws1", "ws2"]);
        state.active = Some(0);
        state.selected = 1;
        state.mode = Mode::Navigate;

        handle_navigate_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(state.active, Some(1));
        assert_eq!(state.mode, Mode::Terminal);
    }

    #[test]
    fn baseline_navigate_digit_switches_indexed_workspace() {
        let mut state = state_with_workspaces(&["ws1", "ws2", "ws3"]);
        state.active = Some(0);
        state.mode = Mode::Navigate;

        handle_navigate_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('2'), KeyModifiers::empty()),
        );

        assert_eq!(state.active, Some(1));
        assert_eq!(state.mode, Mode::Terminal);
    }

    #[test]
    fn baseline_navigate_tab_cycles_pane_stays_navigate() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = Some(0);
        state.mode = Mode::Navigate;

        handle_navigate_key(
            &mut state,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::Navigate);
    }

    #[test]
    fn baseline_navigate_arrows_stay_in_navigate() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = Some(0);
        state.mode = Mode::Navigate;

        handle_navigate_key(
            &mut state,
            KeyEvent::new(KeyCode::Left, KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::Navigate);
    }

    #[test]
    fn baseline_navigate_c_opens_new_tab_dialog() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = Some(0);
        state.mode = Mode::Navigate;

        handle_navigate_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::RenameTab);
        assert!(state.creating_new_tab);
    }

    #[test]
    fn baseline_navigate_g_opens_navigator() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = Some(0);
        state.mode = Mode::Navigate;

        handle_navigate_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::Navigator);
    }

    #[test]
    fn baseline_navigate_prefix_key_leaves_to_terminal() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = Some(0);
        state.mode = Mode::Navigate;

        handle_navigate_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
        );

        assert_eq!(state.mode, Mode::Terminal);
    }

    // -----------------------------------------------------------------------
    // Resize mode baseline
    // -----------------------------------------------------------------------

    #[test]
    fn baseline_resize_esc_exits_to_terminal() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = Some(0);
        state.mode = Mode::Resize;

        handle_resize_key(
            &mut state,
            TerminalKey::new(KeyCode::Esc, KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::Terminal);
    }

    #[test]
    fn baseline_resize_enter_exits_to_terminal() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = Some(0);
        state.mode = Mode::Resize;

        handle_resize_key(
            &mut state,
            TerminalKey::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::Terminal);
    }

    #[test]
    fn baseline_resize_exits_to_navigate_without_active() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = None;
        state.mode = Mode::Resize;

        handle_resize_key(
            &mut state,
            TerminalKey::new(KeyCode::Esc, KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::Navigate);
    }

    #[test]
    fn baseline_resize_hjkl_stays_in_resize() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = Some(0);
        state.mode = Mode::Resize;

        for c in ['h', 'j', 'k', 'l'] {
            handle_resize_key(
                &mut state,
                TerminalKey::new(KeyCode::Char(c), KeyModifiers::empty()),
            );
            assert_eq!(state.mode, Mode::Resize);
        }
    }

    #[test]
    fn baseline_resize_arrows_stay_in_resize() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = Some(0);
        state.mode = Mode::Resize;

        for code in [KeyCode::Left, KeyCode::Right, KeyCode::Up, KeyCode::Down] {
            handle_resize_key(&mut state, TerminalKey::new(code, KeyModifiers::empty()));
            assert_eq!(state.mode, Mode::Resize);
        }
    }

    // -----------------------------------------------------------------------
    // Copy mode baseline
    // -----------------------------------------------------------------------

    #[test]
    fn baseline_copy_mode_q_exits_to_terminal() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = Some(0);
        state.mode = Mode::Copy;
        state.copy_mode = Some(crate::app::state::CopyModeState {
            pane_id: state.workspaces[0].tabs[0].root_pane,
            cursor_row: 0,
            cursor_col: 0,
            entry_offset_from_bottom: 0,
            selection: None,
        });

        let runtimes = TerminalRuntimeRegistry::new();
        state.handle_copy_mode_key(
            &runtimes,
            TerminalKey::new(KeyCode::Char('q'), KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::Terminal);
    }

    #[test]
    fn baseline_copy_mode_esc_exits_to_terminal() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = Some(0);
        state.mode = Mode::Copy;
        state.copy_mode = Some(crate::app::state::CopyModeState {
            pane_id: state.workspaces[0].tabs[0].root_pane,
            cursor_row: 0,
            cursor_col: 0,
            entry_offset_from_bottom: 0,
            selection: None,
        });

        let runtimes = TerminalRuntimeRegistry::new();
        state.handle_copy_mode_key(
            &runtimes,
            TerminalKey::new(KeyCode::Esc, KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::Terminal);
    }

    #[test]
    fn baseline_copy_mode_movement_stays_in_copy() {
        let mut state = state_with_workspaces(&["test"]);
        state.active = Some(0);
        let pane_id = state.workspaces[0].tabs[0].root_pane;
        let pane_infos = state.workspaces[0].tabs[0]
            .layout
            .panes(Rect::new(26, 2, 80, 18));
        state.view.pane_infos = pane_infos;
        state.mode = Mode::Copy;
        state.copy_mode = Some(crate::app::state::CopyModeState {
            pane_id,
            cursor_row: 5,
            cursor_col: 5,
            entry_offset_from_bottom: 0,
            selection: None,
        });

        let runtimes = TerminalRuntimeRegistry::new();
        for c in ['h', 'j', 'k', 'l'] {
            state.handle_copy_mode_key(
                &runtimes,
                TerminalKey::new(KeyCode::Char(c), KeyModifiers::empty()),
            );
            assert_eq!(state.mode, Mode::Copy);
        }
    }

    // -----------------------------------------------------------------------
    // Mouse sidebar-click → Navigate baseline
    // -----------------------------------------------------------------------

    #[test]
    fn baseline_mobile_sidebar_click_enters_navigate_from_terminal() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;

        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 44, 20));

        let switch = app.state.view.mobile_menu_hit_area;
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            switch.x + 1,
            switch.y + 1,
        ));

        assert_eq!(app.state.mode, Mode::Navigate);
    }

    #[test]
    fn baseline_mobile_sidebar_click_enters_navigate_from_resize() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![Workspace::test_new("one")];
        app.state.active = Some(0);
        app.state.mode = Mode::Resize;

        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 44, 20));

        let switch = app.state.view.mobile_menu_hit_area;
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            switch.x + 1,
            switch.y + 1,
        ));

        assert_eq!(app.state.mode, Mode::Navigate);
    }

    // -----------------------------------------------------------------------
    // Initial mode computation baseline
    // -----------------------------------------------------------------------

    #[test]
    fn baseline_initial_mode_depends_on_onboarding_and_default_mode() {
        let config = Config::default();
        let app = App::new(
            &config,
            true,
            None,
            tokio::sync::mpsc::unbounded_channel().1,
            crate::api::EventHub::default(),
        );
        // With default config and no workspaces, initial mode is either
        // Onboarding (first run) or Navigate (after onboarding dismissed).
        // The exact mode depends on whether onboarding config file exists.
        assert!(matches!(
            app.state.mode,
            Mode::Onboarding | Mode::Navigate | Mode::Locked
        ));
    }
}
