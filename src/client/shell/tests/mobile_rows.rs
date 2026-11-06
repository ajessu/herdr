//! Fork: phone switcher rows through the sidebar row engine.

use super::*;

fn phone_agent(
    index: usize,
    agent: &str,
    tokens: &[(&str, &str)],
    focused: bool,
) -> ClientShellAgent {
    ClientShellAgent {
        pane_id: format!("pane_{index}"),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        name: Some(agent.into()),
        display_agent: None,
        agent: Some(agent.into()),
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: AgentStatus::Idle,
        state_change_seq: index as u64,
        state_labels: Vec::new(),
        tokens: tokens
            .iter()
            .map(|(name, value)| ((*name).into(), (*value).into()))
            .collect(),
        focused,
    }
}

fn phone_switcher(config_toml: &str, projected: ClientShellSnapshot) -> ClientShellState {
    let config: Config = toml::from_str(config_toml).expect("phone config");
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    state.mode = ClientShellMode::Navigate;
    state
}

fn mobile_target_rect(
    state: &ClientShellState,
    matches: impl Fn(&ClientMobileTarget) -> bool,
) -> Rect {
    state
        .hits
        .mobile_targets
        .iter()
        .find_map(|(rect, target)| matches(target).then_some(*rect))
        .expect("mobile target")
}

fn agent_rect(state: &ClientShellState, pane: &str) -> Rect {
    mobile_target_rect(
        state,
        |target| matches!(target, ClientMobileTarget::Agent { pane_id, .. } if pane_id == pane),
    )
}

fn workspace_rect(state: &ClientShellState, id: &str) -> Rect {
    mobile_target_rect(
        state,
        |target| matches!(target, ClientMobileTarget::Workspace { workspace_id, .. } if workspace_id == id),
    )
}

const PHONE_AGENT_ROWS: &str = r##"
[ui]
agent_panel_sort = "spaces"

[ui.sidebar]
style = "zellij"

[ui.sidebar.agents]
divider_token = "$agent_mark"
divider_label = "PINNED ↑"
highlight_token = "$agent_mark_age"
highlight_equals = ""
highlight_bg = "#252832"
rows = [["state_icon", "$agent_mark_age", "agent", "$pr"]]

[ui.sidebar.agents.rows_by_agent]
claude = [["state_icon", "agent"], ["$pr"]]
"##;

fn phone_agents_snapshot() -> ClientShellSnapshot {
    let mut projected = snapshot();
    projected.agents = vec![
        phone_agent(
            1,
            "pi",
            &[("agent_mark", "1"), ("agent_mark_age", ""), ("pr", "#41")],
            true,
        ),
        phone_agent(
            2,
            "pi",
            &[("agent_mark", "1"), ("agent_mark_age", "3h"), ("pr", "#42")],
            false,
        ),
        phone_agent(3, "claude", &[("agent_mark_age", ""), ("pr", "#43")], false),
    ];
    for index in 2..=3 {
        let mut pane = projected.panes[0].clone();
        pane.pane_id = format!("pane_{index}");
        pane.focused = false;
        projected.panes.push(pane);
    }
    projected
}

#[test]
fn zellij_phone_agent_rows_use_the_sidebar_row_engine() {
    let mut state = phone_switcher(PHONE_AGENT_ROWS, phone_agents_snapshot());
    let frame = state.compose(60, 40).expect("phone switcher");
    let rows = frame_rows(&frame);
    let first = agent_rect(&state, "pane_1");
    let second = agent_rect(&state, "pane_2");
    let claude = agent_rect(&state, "pane_3");

    let item_text = |rect: Rect| {
        (rect.y..rect.bottom())
            .map(|y| rows[usize::from(y)].clone())
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(
        item_text(first).contains("pi · #41"),
        "{}",
        item_text(first)
    );
    assert!(
        item_text(second).contains("3h · pi · #42"),
        "{}",
        item_text(second)
    );
    assert_eq!((first.height, second.height), (1, 1));
    // rows_by_agent: claude gets its own two-row layout.
    assert_eq!(claude.height, 2);
    assert!(rows[usize::from(claude.y)].contains("claude"));
    assert!(rows[usize::from(claude.y + 1)].contains("#43"));

    // One rule line, with its label, between the pinned agents and the rest.
    assert_eq!(second.bottom() + 1, claude.y);
    let rule = &rows[usize::from(second.bottom())];
    assert!(rule.contains("── PINNED ↑ ──"), "{rule:?}");
    assert_eq!(first.bottom(), second.y, "no rule between pinned agents");
    assert_eq!(
        rows.iter().filter(|row| row.contains("PINNED")).count(),
        1,
        "one divider"
    );

    // Highlight on matching items; the focused agent keeps its own background.
    let bg_at = |rect: Rect| frame.cells[usize::from(rect.y) * 60 + usize::from(rect.x) + 2].bg;
    let highlight = crate::protocol::color_to_u32(ratatui::style::Color::Rgb(0x25, 0x28, 0x32));
    assert_eq!(bg_at(claude), highlight);
    assert_eq!(
        bg_at(first),
        crate::protocol::color_to_u32(state.config.palette.surface_dim)
    );
    assert_ne!(bg_at(second), highlight);

    // The rule line selects nothing; the item after it selects its agent.
    let click = |column: u16, row: u16| {
        RawInputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::empty(),
        })
    };
    let missed = state.handle_raw_events(vec![click(claude.x, second.bottom())]);
    assert!(missed.actions.is_empty());
    assert_eq!(state.mode, ClientShellMode::Navigate);
    let picked = state.handle_raw_events(vec![click(claude.x, claude.y + 1)]);
    assert!(picked.actions.iter().any(|action| matches!(
        action,
        ClientShellAction::Endpoint { request, .. }
            if matches!(
                &request.method,
                crate::api::schema::Method::PaneFocus(params) if params.pane_id == "pane_3"
            )
    )));
}

#[test]
fn zellij_phone_space_rows_follow_space_tokens_gaps_and_worktree_connectors() {
    let mut projected = snapshot();
    projected.workspaces[0].git_ahead_behind = Some((2, 1));
    projected.workspaces[0].worktree = Some(ClientShellWorktree {
        key: "repo".into(),
        label: "repo".into(),
        is_linked_worktree: false,
    });
    let mut child = projected.workspaces[0].clone();
    child.workspace_id = "ws_child".into();
    child.number = 2;
    child.label = "feature".into();
    child.branch = Some("worktree/feature".into());
    child.git_ahead_behind = None;
    child.focused = false;
    child.worktree = Some(ClientShellWorktree {
        key: "repo".into(),
        label: "repo".into(),
        is_linked_worktree: true,
    });
    let mut other = child.clone();
    other.workspace_id = "ws_other".into();
    other.number = 3;
    other.label = "other".into();
    other.custom_label = true;
    other.branch = Some("dev".into());
    other.worktree = None;
    projected.workspaces.extend([child, other]);
    projected.agents = vec![
        phone_agent(1, "pi", &[], true),
        phone_agent(2, "pi", &[], false),
    ];

    let mut state = phone_switcher(
        r#"
[ui]
agent_panel_sort = "spaces"

[ui.sidebar]
style = "zellij"

[ui.sidebar.agents]
rows = [["state_icon", "agent"]]
row_gap = 1

[ui.sidebar.spaces]
rows = [["state_icon", "workspace", "branch", "git_status"]]
row_gap = 1
"#,
        projected,
    );
    let frame = state.compose(60, 40).expect("phone switcher");
    let rows = frame_rows(&frame);
    let machine = state.endpoints[0].label.clone();
    let parent = workspace_rect(&state, "ws_1");
    let child = workspace_rect(&state, "ws_child");
    let other = workspace_rect(&state, "ws_other");

    let parent_row = &rows[usize::from(parent.y)];
    assert!(
        parent_row.contains(&format!("{machine} · client-shell · main ↑2 ↓1")),
        "{parent_row:?}"
    );
    let child_row = &rows[usize::from(child.y)];
    assert!(
        child_row.contains(&format!("└─ ○ {machine} · feature")),
        "{child_row:?}"
    );
    assert!(rows[usize::from(other.y)].contains(&format!("{machine} · other · dev")));
    assert_eq!((parent.height, child.height, other.height), (1, 1, 1));

    // row_gap goes between top-level spaces only, and between agents. The
    // current space's nested rows sit between it and its worktree child.
    assert_eq!(child.y, tab_rect(&state, "tab_1").bottom());
    assert_eq!(other.y, child.bottom() + 1);
    assert_eq!(
        agent_rect(&state, "pane_2").y,
        agent_rect(&state, "pane_1").bottom() + 1
    );
}

#[test]
fn zellij_phone_rows_keep_stale_machine_context() {
    // Space rows without a workspace token still name the machine.
    let mut state = phone_switcher(
        "[ui.sidebar]\nstyle = \"zellij\"\n\n[ui.sidebar.spaces]\nrows = [[\"state_icon\", \"branch\"]]\n",
        snapshot(),
    );
    let profile = crate::client::endpoint::SavedSshEndpoint {
        id: crate::client::endpoint::ProfileId::parse("0123456789abcdef0123456789abcdef").unwrap(),
        label: "Build-host-with-a-really-long-machine-name-for-phones".into(),
        target: "build".into(),
        session: "agents".into(),
        enabled: true,
    };
    let endpoint_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.workspaces[0].workspace_id = "ws_remote".into();
    let long_name = "a-very-long-agent-name-".repeat(4);
    remote.agents = vec![phone_agent(1, &long_name, &[], false)];
    remote.agents[0].workspace_id = "ws_remote".into();
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Online);
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote));
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Reconnecting);

    let frame = state.compose(60, 40).expect("phone switcher");
    let rows = frame_rows(&frame);
    let space = workspace_rect(&state, "ws_remote");
    let space_row = &rows[usize::from(space.y)];
    assert!(space_row.contains("Build-host-"), "{space_row:?}");
    assert!(
        space_row.trim_end().ends_with("… · ○ main · reconnecting"),
        "a long machine label still leaves room for its state: {space_row:?}"
    );
    let agent = mobile_target_rect(
        &state,
        |target| matches!(target, ClientMobileTarget::Agent { endpoint_id: id, .. } if id == &endpoint_id),
    );
    // A full-width row still keeps room for the machine state.
    let agent_row = &rows[usize::from(agent.bottom() - 1)];
    assert!(
        agent_row.trim_end().ends_with("… · reconnecting"),
        "{agent_row:?}"
    );
    let overlay0 = crate::protocol::color_to_u32(state.config.palette.overlay0);
    for rect in [space, agent] {
        let cell = &frame.cells[usize::from(rect.y) * 60 + usize::from(rect.x) + 4];
        assert_eq!(cell.fg, overlay0, "stale items are dimmed");
    }
}

#[test]
fn zellij_phone_agents_section_counts_only_drawn_rows() {
    let mut projected = snapshot();
    projected.agent_view_label = Some("blocked".into());
    projected.agent_order = vec!["pane_1".into()];
    let mut orphan = phone_agent(1, "pi", &[], false);
    orphan.workspace_id = "ws_missing".into();
    projected.agents = vec![orphan];
    let mut state = phone_switcher("[ui.sidebar]\nstyle = \"zellij\"\n", projected);
    let frame = state.compose(60, 40).expect("phone switcher");
    let text = frame_rows(&frame).join("\n");
    assert!(text.contains("agents · blocked"), "{text}");
    assert!(text.contains("no matching agents"), "{text}");
    assert!(!state
        .hits
        .mobile_targets
        .iter()
        .any(|(_, target)| matches!(target, ClientMobileTarget::Agent { .. })));
}

#[test]
fn zellij_phone_space_name_survives_a_long_machine_label() {
    let mut state = phone_switcher("[ui.sidebar]\nstyle = \"zellij\"\n", snapshot());
    let profile = crate::client::endpoint::SavedSshEndpoint {
        id: crate::client::endpoint::ProfileId::parse("0123456789abcdef0123456789abcdef").unwrap(),
        label: "Build-host-with-a-long-name-xx".into(),
        target: "build".into(),
        session: "agents".into(),
        enabled: true,
    };
    let endpoint_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.workspaces[0].workspace_id = "ws_remote".into();
    remote.workspaces[0].label = "feature".into();
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Online);
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote));
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Reconnecting);

    let frame = state.compose(40, 40).expect("narrow phone switcher");
    let rows = frame_rows(&frame);
    let space = workspace_rect(&state, "ws_remote");
    let row = &rows[usize::from(space.y)];
    assert!(row.contains("Build-h… · feature"), "{row:?}");
    assert!(
        row.contains("· feat"),
        "the space name stays visible: {row:?}"
    );
}

fn phone_tab(tab_id: &str, workspace_id: &str, label: &str, status: AgentStatus) -> ClientShellTab {
    ClientShellTab {
        tab_id: tab_id.into(),
        workspace_id: workspace_id.into(),
        number: 1,
        label: label.into(),
        custom_label: true,
        zoomed: false,
        focused: false,
        agent_status: status,
    }
}

/// `ws_1` (focused, parent of `ws_child`) and the plain `ws_other`, with tabs on each.
fn phone_tabs_snapshot() -> ClientShellSnapshot {
    let mut projected = snapshot();
    projected.workspaces[0].worktree = Some(ClientShellWorktree {
        key: "repo".into(),
        label: "repo".into(),
        is_linked_worktree: false,
    });
    let mut child = projected.workspaces[0].clone();
    child.workspace_id = "ws_child".into();
    child.active_tab_id = "tab_c".into();
    child.number = 2;
    child.label = "feature".into();
    child.focused = false;
    child.worktree = Some(ClientShellWorktree {
        key: "repo".into(),
        label: "repo".into(),
        is_linked_worktree: true,
    });
    let mut other = child.clone();
    other.workspace_id = "ws_other".into();
    other.active_tab_id = "tab_o2".into();
    other.number = 3;
    other.label = "other".into();
    other.worktree = None;
    projected.workspaces.extend([child, other]);
    projected.tabs.extend([
        phone_tab("tab_logs", "ws_1", "logs", AgentStatus::Blocked),
        phone_tab("tab_c", "ws_child", "c-one", AgentStatus::Working),
        phone_tab("tab_o1", "ws_other", "o-one", AgentStatus::Done),
        phone_tab("tab_o2", "ws_other", "o-two", AgentStatus::Idle),
    ]);
    projected
}

fn tabs_config(show_tab_status: &str) -> String {
    format!(
        "[ui]\nshow_tab_status = \"{show_tab_status}\"\n\n[ui.sidebar]\nstyle = \"zellij\"\n\n\
         [ui.sidebar.spaces]\nrows = [[\"state_icon\", \"workspace\"]]\n"
    )
}

fn tab_rect(state: &ClientShellState, id: &str) -> Rect {
    mobile_target_rect(
        state,
        |target| matches!(target, ClientMobileTarget::Tab { tab_id, .. } if tab_id == id),
    )
}

fn new_tab_rect(state: &ClientShellState) -> Rect {
    mobile_target_rect(state, |target| matches!(target, ClientMobileTarget::NewTab))
}

fn tap(rect: Rect) -> RawInputEvent {
    RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x,
        row: rect.y,
        modifiers: KeyModifiers::empty(),
    })
}

#[test]
fn zellij_phone_nests_tabs_under_each_space_in_tab_order() {
    let mut state = phone_switcher(&tabs_config("all"), phone_tabs_snapshot());
    let icon = |status| status_icon(status, state.config.status_indicators);
    let (idle, blocked, working, done) = (
        icon(AgentStatus::Idle),
        icon(AgentStatus::Blocked),
        icon(AgentStatus::Working),
        icon(AgentStatus::Done),
    );
    let frame = state.compose(60, 40).expect("phone switcher");
    let rows = frame_rows(&frame);
    let row_of = |rect: Rect| rows[usize::from(rect.y)].clone();

    let parent = workspace_rect(&state, "ws_1");
    let new_tab = new_tab_rect(&state);
    let first = tab_rect(&state, "tab_1");
    let logs = tab_rect(&state, "tab_logs");
    let child = workspace_rect(&state, "ws_child");
    let child_tab = tab_rect(&state, "tab_c");
    let other = workspace_rect(&state, "ws_other");
    let other_one = tab_rect(&state, "tab_o1");
    let other_two = tab_rect(&state, "tab_o2");

    let order = [
        parent, new_tab, first, logs, child, child_tab, other, other_one, other_two,
    ];
    for pair in order.windows(2) {
        assert_eq!(pair[0].bottom(), pair[1].y, "{order:?}");
    }

    // The parent's tabs keep the worktree line open down to its child, and the
    // last child's tabs sit one level below the child's name.
    for (rect, expected) in [
        (new_tab, "  │ ├─ + new tab".to_owned()),
        (first, format!("  │ ├▸ {idle} tab 1")),
        (logs, format!("  │ └─ {blocked} 2 · logs")),
        (child_tab, format!("       └─ {working} 1 · c-one")),
        (other_one, format!("    ├─ {done} 1 · o-one")),
        (other_two, format!("    └─ {idle} 2 · o-two")),
    ] {
        let row = row_of(rect);
        assert!(row.contains(&expected), "{expected:?} in {row:?}");
    }
    assert!(
        !rows.iter().any(|row| row.trim() == "tabs"),
        "no separate tabs section: {rows:#?}"
    );
    assert!(rows.iter().any(|row| row.contains("menu")));

    let cell = |rect: Rect, column: u16| {
        &frame.cells[usize::from(rect.y) * 60 + usize::from(rect.x + column)]
    };
    // Only the focused tab carries the marker, the focus background and bold.
    let bold = ratatui::style::Modifier::BOLD.bits();
    assert_ne!(cell(first, 9).modifier & bold, 0);
    assert_eq!(cell(logs, 9).modifier & bold, 0);
    assert_eq!(rows.iter().filter(|row| row.contains('▸')).count(), 1);
    assert_eq!(
        cell(logs, 7).fg,
        crate::protocol::color_to_u32(state.config.palette.red),
        "the dot keeps its status color"
    );
    let surface_dim = crate::protocol::color_to_u32(state.config.palette.surface_dim);
    assert_eq!(cell(first, 8).bg, surface_dim);
    assert_ne!(cell(logs, 8).bg, surface_dim);
    assert_ne!(cell(other_two, 8).bg, surface_dim);
}

#[test]
fn zellij_phone_tab_dots_follow_show_tab_status() {
    let icon = |status| {
        status_icon(
            status,
            ClientShellConfig::from_config(&Config::default()).status_indicators,
        )
    };
    let row = |mode: &str, id: &str| {
        let mut state = phone_switcher(&tabs_config(mode), phone_tabs_snapshot());
        let frame = state.compose(60, 40).expect("phone switcher");
        frame_rows(&frame)[usize::from(tab_rect(&state, id).y)].clone()
    };
    let blocked = icon(AgentStatus::Blocked);
    let idle = icon(AgentStatus::Idle);
    assert!(row("off", "tab_logs").contains("└─ 2 · logs"));
    assert!(row("attention", "tab_logs").contains(&format!("└─ {blocked} 2 · logs")));
    assert!(row("attention", "tab_o2").contains("└─ 2 · o-two"));
    let done = icon(AgentStatus::Done);
    assert!(row("attention", "tab_o1").contains(&format!("├─ {done} 1 · o-one")));
    assert!(row("all", "tab_o2").contains(&format!("└─ {idle} 2 · o-two")));
}

#[test]
fn zellij_phone_tab_tap_switches_space_and_tab_in_one_tap() {
    let mut state = phone_switcher(&tabs_config("off"), phone_tabs_snapshot());
    state.compose(60, 40).expect("phone switcher");
    let picked = state.handle_raw_events(vec![tap(tab_rect(&state, "tab_o2"))]);
    assert!(picked.actions.iter().any(|action| matches!(
        action,
        ClientShellAction::Endpoint { request, .. }
            if matches!(
                &request.method,
                crate::api::schema::Method::TabFocus(params) if params.tab_id == "tab_o2"
            )
    )));
    assert_eq!(state.mode, ClientShellMode::Terminal);

    state.mode = ClientShellMode::Navigate;
    state.compose(60, 40).expect("phone switcher");
    let space = state.handle_raw_events(vec![tap(workspace_rect(&state, "ws_other"))]);
    assert!(space.actions.iter().any(|action| matches!(
        action,
        ClientShellAction::Endpoint { request, .. }
            if matches!(
                &request.method,
                crate::api::schema::Method::WorkspaceFocus(params)
                    if params.workspace_id == "ws_other"
            )
    )));
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn zellij_phone_new_tab_sits_under_the_current_space_only() {
    let mut state = phone_switcher(&tabs_config("off"), phone_tabs_snapshot());
    state.compose(60, 40).expect("phone switcher");
    assert_eq!(
        state
            .hits
            .mobile_targets
            .iter()
            .filter(|(_, target)| matches!(target, ClientMobileTarget::NewTab))
            .count(),
        1
    );
    assert_eq!(
        workspace_rect(&state, "ws_1").bottom(),
        new_tab_rect(&state).y
    );
    state.handle_raw_events(vec![tap(new_tab_rect(&state))]);
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::Rename(ClientRenameOverlay {
            target: ClientRenameTarget::NewTab { .. },
            ..
        }))
    ));
}

#[test]
fn zellij_phone_space_gap_follows_the_last_nested_tab() {
    let config = format!("{}row_gap = 1\n", tabs_config("off"));
    let mut state = phone_switcher(&config, phone_tabs_snapshot());
    state.compose(60, 40).expect("phone switcher");
    assert_eq!(
        workspace_rect(&state, "ws_other").y,
        tab_rect(&state, "tab_c").bottom() + 1,
        "gap between top-level spaces, after the previous space's tabs"
    );
    assert_eq!(
        tab_rect(&state, "tab_logs").bottom(),
        workspace_rect(&state, "ws_child").y,
        "no gap before a worktree child"
    );
    assert_eq!(
        workspace_rect(&state, "ws_other").bottom(),
        tab_rect(&state, "tab_o1").y,
        "no gap between a space and its tabs"
    );
}

#[test]
fn zellij_phone_tab_rows_fit_next_to_worktree_connectors_at_60_columns() {
    let mut projected = phone_tabs_snapshot();
    // A middle child, so its tabs draw the deepest prefix.
    let mut sibling = projected.workspaces[1].clone();
    sibling.workspace_id = "ws_sibling".into();
    sibling.active_tab_id = "tab_s".into();
    sibling.label = "sibling".into();
    projected.workspaces.insert(2, sibling);
    let long = "a-long-tab-label-".repeat(6);
    projected
        .tabs
        .push(phone_tab("tab_c2", "ws_child", &long, AgentStatus::Blocked));
    projected
        .tabs
        .push(phone_tab("tab_s", "ws_sibling", "s-one", AgentStatus::Idle));

    let mut state = phone_switcher(&tabs_config("all"), projected);
    let frame = state.compose(60, 40).expect("phone switcher");
    let rows = frame_rows(&frame);
    let long_row = &rows[usize::from(tab_rect(&state, "tab_c2").y)];
    let blocked = status_icon(AgentStatus::Blocked, state.config.status_indicators);
    assert!(
        long_row.contains(&format!("  │    └─ {blocked} 2 · a-long-tab")),
        "{long_row:?}"
    );
    let long_rect = tab_rect(&state, "tab_c2");
    let last_cell = usize::from(long_rect.y) * 60 + usize::from(long_rect.right() - 1);
    assert_eq!(
        frame.cells[last_cell].symbol, "…",
        "the label fills the content width and stops there: {long_row:?}"
    );
    let sibling_row = &rows[usize::from(tab_rect(&state, "tab_s").y)];
    assert!(sibling_row.contains("       └─"), "{sibling_row:?}");
}

#[test]
fn zellij_phone_other_machines_keep_spaces_without_nested_tabs() {
    let config = format!("{}row_gap = 1\n", tabs_config("all"));
    let mut state = phone_switcher(&config, phone_tabs_snapshot());
    let profile = crate::client::endpoint::SavedSshEndpoint {
        id: crate::client::endpoint::ProfileId::parse("0123456789abcdef0123456789abcdef").unwrap(),
        label: "build".into(),
        target: "build".into(),
        session: "agents".into(),
        enabled: true,
    };
    let endpoint_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.workspaces[0].workspace_id = "ws_remote".into();
    remote.tabs[0].tab_id = "tab_remote".into();
    remote.tabs[0].workspace_id = "ws_remote".into();
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Online);
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote));

    // With a second machine known, a local tab tap goes through activation.
    state.compose(60, 60).expect("phone switcher");
    let picked = state.handle_raw_events(vec![tap(tab_rect(&state, "tab_o2"))]);
    assert!(picked.actions.iter().any(|action| matches!(
        action,
        ClientShellAction::ActivateEndpoint {
            endpoint_id: ClientEndpointId::Local,
            target: Some(ClientEndpointFocusTarget::Tab(tab_id)),
        } if tab_id == "tab_o2"
    )));
    assert_eq!(state.mode, ClientShellMode::Terminal);

    state.mode = ClientShellMode::Navigate;
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Reconnecting);
    let frame = state.compose(60, 60).expect("phone switcher");
    let rows = frame_rows(&frame);
    let space = workspace_rect(&state, "ws_remote");
    let space_row = &rows[usize::from(space.y)];
    assert!(space_row.contains("build · client-shell"), "{space_row:?}");
    assert!(space_row.contains("reconnecting"), "{space_row:?}");
    assert!(
        !state.hits.mobile_targets.iter().any(
            |(_, target)| matches!(target, ClientMobileTarget::Tab { tab_id, .. } if tab_id == "tab_remote")
        ),
        "tabs nest on the active machine only"
    );
    assert_eq!(
        space.y,
        tab_rect(&state, "tab_o2").bottom() + 1,
        "row_gap separates the machine's first space from the last tab before it"
    );
}

#[test]
fn zellij_phone_nests_only_the_active_machines_tabs_when_ids_collide() {
    let mut state = phone_switcher(&tabs_config("off"), phone_tabs_snapshot());
    let profile = crate::client::endpoint::SavedSshEndpoint {
        id: crate::client::endpoint::ProfileId::parse("0123456789abcdef0123456789abcdef").unwrap(),
        label: "build".into(),
        target: "build".into(),
        session: "agents".into(),
        enabled: true,
    };
    let remote_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote
        .tabs
        .push(phone_tab("tab_remote", "ws_1", "remote", AgentStatus::Idle));
    state.set_endpoint_status(&remote_id, ClientEndpointStatus::Online);
    state.set_endpoint_snapshot(&remote_id, Box::new(remote));
    assert!(state.activate_endpoint_projection(&remote_id));
    let mut remote_surface = surface();
    remote_surface.boot_id = "remote-boot".into();
    state.set_pane_surface(remote_surface);
    state.mode = ClientShellMode::Navigate;
    state.compose(60, 60).expect("phone switcher");

    let targets = &state.hits.mobile_targets;
    let tabs = targets
        .iter()
        .filter_map(|(rect, target)| match target {
            ClientMobileTarget::Tab {
                endpoint_id,
                tab_id,
            } => Some((rect.y, endpoint_id, tab_id.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tabs.iter().map(|(.., tab_id)| *tab_id).collect::<Vec<_>>(),
        ["tab_1", "tab_remote"]
    );
    assert!(tabs
        .iter()
        .all(|(_, endpoint_id, _)| **endpoint_id == remote_id));
    let remote_space = targets
        .iter()
        .find_map(|(rect, target)| {
            matches!(
                target,
                ClientMobileTarget::Workspace { endpoint_id, .. } if *endpoint_id == remote_id
            )
            .then_some(*rect)
        })
        .expect("remote space");
    assert_eq!(new_tab_rect(&state).y, remote_space.bottom());
    assert_eq!(tabs[0].0, new_tab_rect(&state).bottom());
}

#[test]
fn zellij_phone_active_stale_machine_dims_its_tab_rows() {
    let mut state = phone_switcher(&tabs_config("all"), phone_tabs_snapshot());
    state.set_endpoint_status(&ClientEndpointId::Local, ClientEndpointStatus::Reconnecting);
    let frame = state.compose(60, 40).expect("phone switcher");
    let rect = tab_rect(&state, "tab_logs");
    let overlay0 = crate::protocol::color_to_u32(state.config.palette.overlay0);
    // The 7-cell prefix, then the dot, a blank, and the label.
    for column in [7, 9] {
        let cell = &frame.cells[usize::from(rect.y) * 60 + usize::from(rect.x + column)];
        assert_eq!(cell.fg, overlay0, "column {column}: {:?}", cell.symbol);
    }
}

#[test]
fn zellij_phone_reveal_scrolls_past_nested_tabs_to_the_selected_space() {
    let mut state = phone_switcher(&tabs_config("off"), phone_tabs_snapshot());
    state.navigate_workspace_id = state.navigation_target(&ClientEndpointId::Local, "ws_other");
    state.reveal_mobile_workspace = true;
    state.compose(60, 8).expect("short phone switcher");
    assert!(state.mobile_switcher_scroll > 0);
    // Not just a badge's target: the row itself is in the list, above the reserved row.
    let row = workspace_rect(&state, "ws_other");
    assert!(row.y > 2 && row.bottom() <= 7, "{row:?}");
}

#[test]
fn zellij_phone_space_and_nested_rows_agree_on_the_cached_snapshot() {
    let mut state = phone_switcher(&tabs_config("off"), phone_tabs_snapshot());
    // A newer cached snapshot moved focus before it was applied. The agent rows
    // and keyboard navigation read it, and the space rows and their tabs follow.
    let cached = state.endpoints[0].snapshot.as_deref_mut().expect("cached");
    cached.focused_workspace_id = Some("ws_other".into());
    for workspace in &mut cached.workspaces {
        workspace.focused = workspace.workspace_id == "ws_other";
    }
    for tab in &mut cached.tabs {
        tab.focused = tab.tab_id == "tab_o1";
    }
    let frame = state.compose(60, 40).expect("phone switcher");
    let rows = frame_rows(&frame);

    let current = workspace_rect(&state, "ws_other");
    assert_eq!(current.bottom(), new_tab_rect(&state).y);
    assert!(rows[usize::from(tab_rect(&state, "tab_o1").y)].contains('▸'));
    assert!(!rows[usize::from(tab_rect(&state, "tab_1").y)].contains('▸'));
    let bg = |rect: Rect| frame.cells[usize::from(rect.y) * 60 + usize::from(rect.x) + 2].bg;
    let surface_dim = crate::protocol::color_to_u32(state.config.palette.surface_dim);
    assert_eq!(bg(current), surface_dim, "the highlight agrees");
    assert_ne!(bg(workspace_rect(&state, "ws_1")), surface_dim);
}

#[test]
fn zellij_phone_switch_button_selects_the_space_above_its_focused_tab() {
    let config: Config = toml::from_str(&tabs_config("off")).expect("config");
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(phone_tabs_snapshot()));
    state.set_pane_surface(surface());
    state.compose(60, 40).expect("phone header");
    state.handle_raw_events(vec![tap(state.hits.mobile_switch)]);
    assert_eq!(state.mode, ClientShellMode::Navigate);
    let frame = state.compose(60, 40).expect("phone switcher");

    let bg = |rect: Rect| frame.cells[usize::from(rect.y) * 60 + usize::from(rect.x) + 2].bg;
    let palette = &state.config.palette;
    assert_eq!(
        bg(workspace_rect(&state, "ws_1")),
        crate::protocol::color_to_u32(palette.surface0),
        "the selected space"
    );
    assert_eq!(
        bg(tab_rect(&state, "tab_1")),
        crate::protocol::color_to_u32(palette.surface_dim),
        "its focused tab"
    );
}

#[test]
fn zellij_phone_two_row_parent_keeps_its_worktree_line_down_to_its_tabs() {
    let mut state = phone_switcher("[ui.sidebar]\nstyle = \"zellij\"\n", phone_tabs_snapshot());
    let frame = state.compose(60, 40).expect("phone switcher");
    let rows = frame_rows(&frame);
    let parent = workspace_rect(&state, "ws_1");
    assert_eq!(parent.height, 2, "default space rows");
    let second = &rows[usize::from(parent.y + 1)];
    assert!(second.contains("  │ main"), "{second:?}");
    let plain = workspace_rect(&state, "ws_other");
    let second = &rows[usize::from(plain.y + 1)];
    assert!(!second.contains('│'), "{second:?}");
}

#[test]
fn zellij_phone_tab_rows_strip_control_and_bidi_characters() {
    let mut projected = phone_tabs_snapshot();
    projected.tabs[1].label = "lo\u{202e}gs\u{7}".into();
    let mut state = phone_switcher(&tabs_config("off"), projected);
    let frame = state.compose(60, 40).expect("phone switcher");
    let row = &frame_rows(&frame)[usize::from(tab_rect(&state, "tab_logs").y)];
    assert!(row.contains("└─ 2 · logs"), "{row:?}");
}

#[test]
fn phone_header_tab_text_carries_the_focused_tab_dot() {
    let header = |sidebar: &str, mode: &str, status: AgentStatus, width: u16| {
        let mut projected = snapshot();
        projected.tabs[0].agent_status = status;
        projected.workspaces[0].label = "a-space-name-long-enough-to-meet-the-tab-text".into();
        let config: Config = toml::from_str(&format!(
            "[ui]\nshow_tab_status = \"{mode}\"\n\n[ui.sidebar]\nstyle = \"{sidebar}\"\n"
        ))
        .expect("config");
        let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
        state.set_snapshot(Box::new(projected));
        state.set_pane_surface(surface());
        let frame = state.compose(width, 20).expect("mobile header");
        (
            frame.cells[..usize::from(width)].to_vec(),
            crate::protocol::color_to_u32(state.config.palette.red),
            status_icon(AgentStatus::Blocked, state.config.status_indicators),
        )
    };
    fn text(cells: &[crate::protocol::CellData]) -> String {
        cells.iter().map(|cell| cell.symbol.as_str()).collect()
    }
    for width in [44, 60] {
        for mode in ["all", "attention"] {
            let (cells, red, blocked) = header("zellij", mode, AgentStatus::Blocked, width);
            let dot = cells
                .iter()
                .position(|cell| cell.symbol == blocked)
                .expect("dot in the header");
            assert_eq!(cells[dot].fg, red);
            assert_eq!(text(&cells[dot + 2..dot + 7]), "tab 1");
            let ellipsis = cells
                .iter()
                .position(|cell| cell.symbol == "…")
                .expect("space name cut to fit");
            assert!(ellipsis < dot, "{}", text(&cells));
        }
        let off = text(&header("zellij", "off", AgentStatus::Blocked, width).0);
        assert_eq!(
            text(&header("zellij", "attention", AgentStatus::Idle, width).0),
            off,
            "attention shows no dot for an idle tab"
        );
        assert_eq!(
            text(&header("upstream", "all", AgentStatus::Blocked, width).0),
            off,
            "the header stays as it is outside the zellij style"
        );
    }
    let off = text(&header("zellij", "off", AgentStatus::Blocked, 44).0);
    assert!(
        off.starts_with(" ○ a-space-name-long-enou…  tab 1 │"),
        "{off:?}"
    );
}

#[test]
fn phone_header_dot_is_the_active_tabs_not_the_first() {
    let mut projected = snapshot();
    projected.tabs[0].agent_status = AgentStatus::Done;
    projected
        .tabs
        .push(phone_tab("tab_logs", "ws_1", "logs", AgentStatus::Blocked));
    projected.workspaces[0].active_tab_id = "tab_logs".into();
    let config: Config = toml::from_str(&tabs_config("all")).expect("config");
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    let frame = state.compose(60, 20).expect("mobile header");
    let blocked = status_icon(AgentStatus::Blocked, state.config.status_indicators);
    let top = &frame_rows(&frame)[0];
    assert!(
        top.contains(&format!("{blocked} tab logs · 2/2")),
        "{top:?}"
    );
}

#[test]
fn upstream_style_keeps_the_tabs_section_and_flat_spaces() {
    let config = "[ui]\nshow_tab_status = \"all\"\n";
    let mut state = phone_switcher(config, phone_tabs_snapshot());
    let frame = state.compose(60, 40).expect("phone switcher");
    let rows = frame_rows(&frame);
    assert!(rows.iter().any(|row| row.trim() == "tabs"), "{rows:#?}");
    let new_tab = new_tab_rect(&state);
    assert!(new_tab.y > workspace_rect(&state, "ws_other").y);
    let logs = &rows[usize::from(tab_rect(&state, "tab_logs").y)];
    assert!(logs.contains("  2 · logs"), "{logs:?}");
    assert!(
        !state.hits.mobile_targets.iter().any(
            |(_, target)| matches!(target, ClientMobileTarget::Tab { tab_id, .. } if tab_id == "tab_o1")
        ),
        "only the current space's tabs are listed"
    );
}

const BADGE_CONFIG: &str = r#"
[ui]
agent_panel_sort = "spaces"

[ui.sidebar]
style = "zellij"

[ui.sidebar.agents]
rows = [["state_icon", "agent"]]

[ui.sidebar.spaces]
rows = [["state_icon", "workspace"]]
"#;

const BADGE_CONFIG_TWO_ROW_AGENTS: &str = r#"
[ui]
agent_panel_sort = "spaces"

[ui.sidebar]
style = "zellij"

[ui.sidebar.agents]
rows = [["state_icon"], ["agent"]]
"#;

/// The rule between the switcher's header and its list.
const BADGE_RULE_ROW: u16 = 2;

/// Agents `pane_1..` with `statuses` and spaces `ws_1..` with `space_statuses`.
/// The agents live in a leading `ws_agents` space, so the client's status rollup
/// lands there and leaves the numbered spaces' statuses alone.
fn badge_snapshot(statuses: &[AgentStatus], space_statuses: &[AgentStatus]) -> ClientShellSnapshot {
    let mut projected = snapshot();
    let pane = projected.panes[0].clone();
    projected.panes.clear();
    projected.agents.clear();
    for (index, status) in statuses.iter().enumerate() {
        let number = index + 1;
        let mut agent = phone_agent(number, "pi", &[], false);
        agent.agent_status = *status;
        agent.workspace_id = "ws_agents".into();
        agent.tab_id = "tab_agents".into();
        projected.agents.push(agent);
        let mut pane = pane.clone();
        pane.pane_id = format!("pane_{number}");
        pane.focused = false;
        projected.panes.push(pane);
    }
    let space = projected.workspaces[0].clone();
    projected.workspaces.clear();
    for (index, status) in space_statuses.iter().enumerate() {
        let number = index + 1;
        let mut space = space.clone();
        space.workspace_id = format!("ws_{number}");
        space.number = number;
        space.label = format!("space-{number}");
        space.focused = false;
        space.agent_status = *status;
        projected.workspaces.push(space);
    }
    if !statuses.is_empty() {
        let mut home = space;
        home.workspace_id = "ws_agents".into();
        home.label = "agents-home".into();
        home.focused = false;
        projected.workspaces.insert(0, home);
    }
    projected.focused_workspace_id = None;
    projected.tabs.clear();
    projected
}

/// A switcher where `Done` agents went from working to finished while the client
/// watched, so they project as finished-unseen instead of idle.
fn badge_switcher(config: &str, projected: ClientShellSnapshot) -> ClientShellState {
    let mut working = projected.clone();
    for agent in &mut working.agents {
        if agent.agent_status == AgentStatus::Done {
            agent.agent_status = AgentStatus::Working;
        }
    }
    let mut state = phone_switcher(config, working);
    let mut finished = projected;
    for agent in &mut finished.agents {
        if agent.agent_status == AgentStatus::Done {
            agent.state_change_seq += 1_000;
        }
    }
    state.set_snapshot(Box::new(finished));
    state
}

/// The badge hits on one row: only a badge can target an item from a rule row.
fn badges_on(state: &ClientShellState, row: u16) -> Vec<(Rect, ClientMobileTarget)> {
    state
        .hits
        .mobile_targets
        .iter()
        .filter(|(rect, _)| rect.y == row)
        .cloned()
        .collect()
}

fn row_after(frame: &FrameData, row: u16, marker: &str) -> Option<String> {
    frame_rows(frame)[usize::from(row)]
        .split_once(marker)
        .map(|(_, rest)| format!("{marker}{rest}").trim_end().to_owned())
}

#[test]
fn zellij_phone_top_badge_counts_items_wholly_above_the_viewport() {
    use AgentStatus::{Blocked, Done, Idle, Working};
    let mut state = badge_switcher(
        BADGE_CONFIG,
        badge_snapshot(&[Blocked, Working, Done, Idle, Idle, Idle, Idle], &[Idle]),
    );
    // One section row, then one row per agent: scroll 4 hides agents 1 to 3.
    state.mobile_switcher_scroll = 4;
    let frame = state.compose(60, 12).expect("phone switcher");
    assert_eq!(
        row_after(&frame, BADGE_RULE_ROW, "agents").as_deref(),
        Some("agents +3 ◉¹ ◐¹ ●¹")
    );
    assert!(row_after(&frame, BADGE_RULE_ROW, "spaces").is_none());
}

#[test]
fn zellij_phone_bottom_badge_sits_on_a_reserved_rule_row() {
    use AgentStatus::{Blocked, Idle, Working};
    let mut state = badge_switcher(
        BADGE_CONFIG,
        badge_snapshot(&[Idle; 9], &[Blocked, Working, Idle]),
    );
    let frame = state.compose(60, 12).expect("phone switcher");
    // Rows 3..11 show the section row and agents 1 to 7; row 11 is reserved.
    let reserved = &frame_rows(&frame)[11];
    assert!(reserved.starts_with('─'), "{reserved:?}");
    assert_eq!(
        row_after(&frame, 11, "agents").as_deref(),
        Some("agents +2 spaces +4 ◉¹ ◐¹")
    );
    assert_eq!(agent_rect(&state, "pane_7").y, 10);

    // Section, 9 agents, section, new workspace, 4 spaces, section, then the menu.
    let menu = crate::client::shell::global_menu::global_menu_items(
        state.snapshot.as_deref().expect("snapshot"),
    )
    .len();
    assert_eq!(state.hits.mobile_max_scroll, 17 + menu - 8);
    state.mobile_switcher_scroll = usize::MAX;
    let frame = state.compose(60, 12).expect("scrolled to the end");
    let rows = frame_rows(&frame);
    assert!(rows[11].starts_with('─'), "{:?}", rows[11]);
    assert_eq!(rows[11], "─".repeat(60), "nothing is below");
    let last_menu = state
        .hits
        .mobile_targets
        .iter()
        .filter(|(_, target)| matches!(target, ClientMobileTarget::Menu(_)))
        .map(|(rect, _)| rect.bottom())
        .max();
    assert_eq!(last_menu, Some(11), "the last row sits just above the rule");
}

#[test]
fn zellij_phone_badge_counts_an_item_only_when_no_line_is_visible() {
    use AgentStatus::{Blocked, Idle};
    let mut state = badge_switcher(
        BADGE_CONFIG_TWO_ROW_AGENTS,
        badge_snapshot(&[Blocked, Idle, Idle, Idle, Idle, Idle, Idle], &[Idle]),
    );
    // The section row is row 0, so pane_1 spans rows 1 and 2.
    state.mobile_switcher_scroll = 2;
    let frame = state.compose(60, 12).expect("half of pane_1 visible");
    assert!(row_after(&frame, BADGE_RULE_ROW, "agents").is_none());
    assert_eq!(agent_rect(&state, "pane_1").height, 1);

    state.mobile_switcher_scroll = 3;
    let frame = state.compose(60, 12).expect("pane_1 scrolled off");
    assert_eq!(
        row_after(&frame, BADGE_RULE_ROW, "agents").as_deref(),
        Some("agents +1 ◉¹")
    );
}

#[test]
fn zellij_phone_two_badges_share_an_edge_at_60_columns() {
    use AgentStatus::{Blocked, Done, Working};
    let mut state = badge_switcher(
        BADGE_CONFIG,
        badge_snapshot(&[Blocked; 20], &[Done, Working, Blocked]),
    );
    let frame = state.compose(60, 12).expect("phone switcher");
    assert_eq!(
        row_after(&frame, 11, "agents").as_deref(),
        Some("agents +9+ ◉⁹⁺ spaces +4 ◉² ◐¹ ●¹")
    );
    let badges = badges_on(&state, 11);
    assert_eq!(badges.len(), 2);
    let spaces = badges
        .iter()
        .find(|(_, target)| matches!(target, ClientMobileTarget::Workspace { .. }))
        .expect("spaces badge")
        .0;
    let agents = badges
        .iter()
        .find(|(_, target)| matches!(target, ClientMobileTarget::Agent { .. }))
        .expect("agents badge")
        .0;
    assert_eq!(spaces.right(), 60, "spaces is right-aligned");
    assert_eq!(agents.right(), spaces.x, "agents sits to its left");
    assert_eq!(
        usize::from(spaces.width),
        " spaces +4 ◉² ◐¹ ●¹".chars().count(),
        "only the badge's own cells are a target"
    );
}

#[test]
fn zellij_phone_narrow_badges_clip_or_drop() {
    use AgentStatus::{Blocked, Done, Working};
    let mut state = badge_switcher(
        BADGE_CONFIG,
        badge_snapshot(&[Blocked; 20], &[Done, Working, Blocked]),
    );
    // Spaces takes 19 columns; agents needs 11 for its label and count.
    state.compose(29, 12).expect("no room for the count");
    let badges = badges_on(&state, 11);
    assert_eq!(badges.len(), 1, "agents needs its count: {badges:?}");
    assert!(matches!(badges[0].1, ClientMobileTarget::Workspace { .. }));

    state.compose(32, 12).expect("clipped");
    let badges = badges_on(&state, 11);
    assert_eq!(badges.len(), 2);
    let (spaces, agents) = (badges[0].0, badges[1].0);
    assert!(matches!(badges[1].1, ClientMobileTarget::Agent { .. }));
    assert_eq!(spaces.right(), 32);
    assert_eq!(
        (agents.x, agents.right()),
        (0, spaces.x),
        "agents takes what spaces leaves"
    );

    state.compose(9, 12).expect("too narrow");
    assert!(badges_on(&state, 11).is_empty());
}

#[test]
fn zellij_phone_agents_badge_tap_focuses_the_most_urgent_hidden_agent() {
    use AgentStatus::{Blocked, Done, Idle, Working};
    let hidden_below = |statuses: [AgentStatus; 4], expected: &str| {
        let mut all = vec![Idle; 7];
        all.extend(statuses);
        let mut state = badge_switcher(BADGE_CONFIG, badge_snapshot(&all, &[Idle]));
        state.compose(60, 12).expect("phone switcher");
        let (rect, _) = badges_on(&state, 11)
            .into_iter()
            .find(|(_, target)| matches!(target, ClientMobileTarget::Agent { .. }))
            .expect("agents badge");
        let picked = state.handle_raw_events(vec![tap(rect)]);
        assert!(
            picked.actions.iter().any(|action| matches!(
                action,
                ClientShellAction::Endpoint { request, .. }
                    if matches!(
                        &request.method,
                        crate::api::schema::Method::PaneFocus(params) if params.pane_id == expected
                    )
            )),
            "{statuses:?}: {:?}",
            picked.actions
        );
        assert_eq!(state.mode, ClientShellMode::Terminal);
    };
    hidden_below([Idle, Done, Working, Blocked], "pane_11");
    hidden_below([Idle, Done, Working, Idle], "pane_10");
    hidden_below([Idle, Done, Idle, Idle], "pane_9");
    hidden_below([Idle, Idle, Idle, Idle], "pane_8");
}

#[test]
fn zellij_phone_spaces_badge_tap_acts_like_that_row_across_machines() {
    use AgentStatus::{Blocked, Idle};
    let mut remote = badge_snapshot(&[], &[Blocked]);
    remote.workspaces[0].workspace_id = "ws_remote".into();
    let (mut state, endpoint_id) = two_machine_badges(
        badge_snapshot(&[Idle; 9], &[Idle]),
        remote,
        ClientEndpointStatus::Online,
    );

    let frame = state.compose(60, 12).expect("phone switcher");
    assert!(row_after(&frame, 11, "spaces").is_some_and(|badge| badge.ends_with("+3 ◉¹")));
    let (rect, _) = badges_on(&state, 11)
        .into_iter()
        .find(|(_, target)| matches!(target, ClientMobileTarget::Workspace { .. }))
        .expect("spaces badge");
    let picked = state.handle_raw_events(vec![tap(rect)]);
    assert!(picked.actions.iter().any(|action| matches!(
        action,
        ClientShellAction::ActivateEndpoint {
            endpoint_id: picked,
            target: Some(ClientEndpointFocusTarget::Workspace(workspace_id)),
        } if picked == &endpoint_id && workspace_id == "ws_remote"
    )));
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn zellij_phone_no_badge_or_reserved_row_when_nothing_overflows() {
    use AgentStatus::Blocked;
    let mut state = badge_switcher(BADGE_CONFIG, badge_snapshot(&[Blocked; 2], &[Blocked]));
    let frame = state.compose(60, 40).expect("phone switcher");
    let rows = frame_rows(&frame);
    assert_eq!(state.hits.mobile_max_scroll, 0);
    assert!(!rows[39].contains('─'), "{:?}", rows[39]);
    assert!(badges_on(&state, BADGE_RULE_ROW).is_empty());
    assert!(
        !rows
            .iter()
            .any(|row| row.contains("agents +") || row.contains("spaces +")),
        "{rows:#?}"
    );
}

#[test]
fn zellij_phone_overflow_past_the_lists_keeps_the_rule_without_a_badge() {
    use AgentStatus::Blocked;
    let mut state = badge_switcher(BADGE_CONFIG, badge_snapshot(&[Blocked], &[Blocked]));
    // Agents and spaces are all visible; only the menu runs past the rule.
    let frame = state.compose(60, 11).expect("phone switcher");
    assert!(state.hits.mobile_max_scroll > 0);
    let rows = frame_rows(&frame);
    assert_eq!(rows[10].trim_end(), "─".repeat(60), "{rows:#?}");
    workspace_rect(&state, "ws_1");
    assert!(badges_on(&state, 10).is_empty());
}

#[test]
fn upstream_style_has_no_badges_and_no_reserved_row() {
    use AgentStatus::Blocked;
    let config = "[ui]\nagent_panel_sort = \"spaces\"\n";
    let mut state = phone_switcher(config, badge_snapshot(&[Blocked; 12], &[Blocked; 4]));
    let frame = state.compose(60, 12).expect("phone switcher");
    let rows = frame_rows(&frame);
    assert!(
        !rows
            .iter()
            .any(|row| row.contains("agents +") || row.contains("spaces +")),
        "{rows:#?}"
    );
    assert!(!rows[11].contains('─'), "{:?}", rows[11]);
    assert!(state
        .hits
        .mobile_targets
        .iter()
        .any(|(rect, _)| rect.bottom() == 12));
}

#[test]
fn zellij_phone_badges_never_overlap_a_tap_area_at_any_scroll() {
    use AgentStatus::{Blocked, Done, Idle, Working};
    let two_row_agents = badge_switcher(
        BADGE_CONFIG_TWO_ROW_AGENTS,
        badge_snapshot(
            &[
                Blocked, Working, Done, Idle, Idle, Idle, Blocked, Working, Done,
            ],
            &[Blocked, Working, Done, Idle, Idle, Idle, Blocked],
        ),
    );
    let nested_tabs = phone_switcher(
        &format!("{}row_gap = 1\n", tabs_config("all")),
        phone_tabs_snapshot(),
    );
    for mut state in [two_row_agents, nested_tabs] {
        state.compose(60, 14).expect("phone switcher");
        let max_scroll = state.hits.mobile_max_scroll;
        assert!(max_scroll > 0);
        for scroll in 0..=max_scroll {
            state.mobile_switcher_scroll = scroll;
            state.compose(60, 14).expect("phone switcher");
            let hits = &state.hits.mobile_targets;
            for (index, (rect, _)) in hits.iter().enumerate() {
                for (other, _) in &hits[index + 1..] {
                    assert!(
                        rect.intersection(*other).is_empty(),
                        "scroll {scroll}: {rect:?} overlaps {other:?}"
                    );
                }
                let list_row = rect.y > BADGE_RULE_ROW && rect.bottom() <= 13;
                // A list row is 59 columns wide; only a badge is narrower on these rows.
                let badge = rect.height == 1
                    && rect.width < 59
                    && (rect.y == BADGE_RULE_ROW || rect.y == 13);
                assert!(list_row || badge, "scroll {scroll}: {rect:?}");
            }
        }
    }
}

/// The `+N` a `list` badge shows on `row`, if it has one.
fn hidden_on(frame: &FrameData, row: u16, list: &str) -> usize {
    row_after(frame, row, &format!("{list} +"))
        .and_then(|badge| {
            badge
                .trim_start_matches(&format!("{list} +"))
                .split(' ')
                .next()
                .and_then(|count| count.parse().ok())
        })
        .unwrap_or(0)
}

#[test]
fn zellij_phone_spaces_badges_ignore_nested_tabs_and_gaps() {
    let config = format!("{}row_gap = 1\n", tabs_config("all"));
    let mut state = phone_switcher(&config, phone_tabs_snapshot());
    state.compose(60, 10).expect("phone switcher");
    for scroll in 0..=state.hits.mobile_max_scroll {
        state.mobile_switcher_scroll = scroll;
        let frame = state.compose(60, 10).expect("phone switcher");
        let list_rows = (BADGE_RULE_ROW + 1)..9;
        let drawn = state
            .hits
            .mobile_targets
            .iter()
            .filter(|(rect, target)| {
                list_rows.contains(&rect.y)
                    && matches!(target, ClientMobileTarget::Workspace { .. })
            })
            .count();
        assert_eq!(
            hidden_on(&frame, BADGE_RULE_ROW, "spaces") + drawn + hidden_on(&frame, 9, "spaces"),
            3,
            "scroll {scroll}: {:#?}",
            frame_rows(&frame)
        );
    }
}

#[test]
fn zellij_phone_top_badges_share_the_rule_and_tap_to_the_nearest_urgent_item() {
    use AgentStatus::{Blocked, Idle};
    let mut state = badge_switcher(
        BADGE_CONFIG,
        badge_snapshot(
            &[Idle, Blocked, Idle, Blocked, Idle],
            &[Blocked, Idle, Idle],
        ),
    );
    // Past the 5 agents and ws_agents, ws_1 and ws_2: the first row is ws_3.
    state.mobile_switcher_scroll = 1 + 5 + 2 + 3;
    let frame = state.compose(60, 8).expect("phone switcher");
    assert_eq!(
        row_after(&frame, BADGE_RULE_ROW, "agents").as_deref(),
        Some("agents +5 ◉² spaces +3 ◉²")
    );
    let badges = badges_on(&state, BADGE_RULE_ROW);
    assert_eq!(badges.len(), 2);
    assert_eq!(badges[0].0.right(), 60);
    assert_eq!(badges[1].0.right(), badges[0].0.x);
    let spaces = state.handle_raw_events(vec![tap(badges[0].0)]);
    assert!(spaces.actions.iter().any(|action| matches!(
        action,
        ClientShellAction::Endpoint { request, .. }
            if matches!(
                &request.method,
                crate::api::schema::Method::WorkspaceFocus(params)
                    if params.workspace_id == "ws_1"
            )
    )));

    state.mode = ClientShellMode::Navigate;
    state.compose(60, 8).expect("phone switcher");
    let (rect, _) = badges_on(&state, BADGE_RULE_ROW)[1].clone();
    let agents = state.handle_raw_events(vec![tap(rect)]);
    assert!(agents.actions.iter().any(|action| matches!(
        action,
        ClientShellAction::Endpoint { request, .. }
            if matches!(
                &request.method,
                crate::api::schema::Method::PaneFocus(params) if params.pane_id == "pane_4"
            )
    )));
}

/// A switcher with `local` locally and `remote` on an SSH machine in `status`.
fn two_machine_badges(
    local: ClientShellSnapshot,
    mut remote: ClientShellSnapshot,
    status: ClientEndpointStatus,
) -> (ClientShellState, ClientEndpointId) {
    let mut state = badge_switcher(BADGE_CONFIG, local);
    let profile = crate::client::endpoint::SavedSshEndpoint {
        id: crate::client::endpoint::ProfileId::parse("0123456789abcdef0123456789abcdef").unwrap(),
        label: "build".into(),
        target: "build".into(),
        session: "agents".into(),
        enabled: true,
    };
    let endpoint_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    remote.boot_id = "remote-boot".into();
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Online);
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote));
    state.set_endpoint_status(&endpoint_id, status);
    (state, endpoint_id)
}

#[test]
fn zellij_phone_badges_skip_a_machine_that_is_not_online() {
    use AgentStatus::{Blocked, Idle, Working};
    let mut local = vec![Idle; 7];
    local.extend([Idle, Working]);
    let (mut state, _) = two_machine_badges(
        badge_snapshot(&local, &[Idle]),
        badge_snapshot(&[Blocked, Blocked], &[Blocked]),
        ClientEndpointStatus::Reconnecting,
    );
    let frame = state.compose(60, 14).expect("phone switcher");
    let (rect, target) = badges_on(&state, 13)
        .into_iter()
        .find(|(_, target)| matches!(target, ClientMobileTarget::Agent { .. }))
        .expect("agents badge");
    assert!(
        row_after(&frame, 13, "agents").is_some_and(|badge| badge.starts_with("agents +5 ◐¹")),
        "{:?}",
        frame_rows(&frame)[13]
    );
    assert!(matches!(
        target,
        ClientMobileTarget::Agent { endpoint_id: ClientEndpointId::Local, pane_id } if pane_id == "pane_9"
    ));
    let picked = state.handle_raw_events(vec![tap(rect)]);
    assert!(!picked.actions.is_empty());
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn zellij_phone_offline_machine_adds_nothing_to_the_badges() {
    use AgentStatus::{Blocked, Idle};
    let (mut state, _) = two_machine_badges(
        badge_snapshot(&[Idle], &[Idle]),
        badge_snapshot(&[Blocked; 12], &[]),
        ClientEndpointStatus::Reconnecting,
    );
    let frame = state.compose(60, 12).expect("phone switcher");
    assert!(
        row_after(&frame, 11, "agents").is_none(),
        "{:#?}",
        frame_rows(&frame)
    );
    assert_eq!(hidden_on(&frame, 11, "spaces"), 2, "the local spaces only");
    assert!(frame_rows(&frame)[11].starts_with('─'));
}

#[test]
fn zellij_phone_badges_skip_every_machine_that_is_not_online() {
    use AgentStatus::{Blocked, Working};
    let (mut state, endpoint_id) = two_machine_badges(
        badge_snapshot(&[Blocked; 12], &[Blocked]),
        badge_snapshot(&[Working; 12], &[Working]),
        ClientEndpointStatus::Online,
    );
    state.mark_endpoint_disconnected(&ClientEndpointId::Local);
    let frame = state.compose(60, 14).expect("local down");
    let badges = badges_on(&state, 13);
    assert!(!badges.is_empty(), "the online machine keeps its badges");
    assert!(
        badges.iter().all(|(_, target)| matches!(
            target,
            ClientMobileTarget::Agent { endpoint_id: id, .. }
                | ClientMobileTarget::Workspace { endpoint_id: id, .. } if id == &endpoint_id
        )),
        "{badges:?}"
    );
    assert!(
        !frame_rows(&frame)[13].contains('◉'),
        "{:?}",
        frame_rows(&frame)[13]
    );

    let (mut state, endpoint_id) = two_machine_badges(
        badge_snapshot(&[Blocked; 2], &[Blocked]),
        badge_snapshot(&[Working; 12], &[Working]),
        ClientEndpointStatus::Online,
    );
    state.active_endpoint_id = endpoint_id.clone();
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Reconnecting);
    state.compose(60, 14).expect("active remote down");
    assert!(state.hits.mobile_max_scroll > 0);
    let badges = badges_on(&state, 13);
    assert!(!badges.is_empty(), "the online machine keeps its badges");
    assert!(badges.iter().all(|(_, target)| !matches!(
        target,
        ClientMobileTarget::Agent { endpoint_id: id, .. }
            | ClientMobileTarget::Workspace { endpoint_id: id, .. } if id == &endpoint_id
    )));
}

#[test]
fn zellij_phone_error_bar_drops_the_badge_targets_it_covers() {
    use AgentStatus::Idle;
    let mut state = badge_switcher(BADGE_CONFIG, badge_snapshot(&[Idle; 9], &[Idle]));
    state.compose(60, 12).expect("phone switcher");
    assert!(!badges_on(&state, 11).is_empty());

    state.set_endpoint_error("boom");
    let frame = state.compose(60, 12).expect("error bar");
    assert!(frame_rows(&frame)[11].contains("boom"));
    assert!(badges_on(&state, 11).is_empty());
}

#[test]
fn upstream_style_keeps_the_target_under_the_error_bar() {
    let config = "[ui]\nagent_panel_sort = \"spaces\"\n";
    let mut state = phone_switcher(config, badge_snapshot(&[AgentStatus::Idle; 12], &[]));
    state.set_endpoint_error("boom");
    state.compose(60, 12).expect("error bar");
    assert!(state
        .hits
        .mobile_targets
        .iter()
        .any(|(rect, _)| rect.bottom() == 12));
}

#[test]
fn zellij_phone_notice_takes_the_tap_before_the_badge_under_it() {
    use AgentStatus::Idle;
    let mut state = badge_switcher(BADGE_CONFIG, badge_snapshot(&[Idle; 9], &[Idle]));
    state.compose(60, 12).expect("phone switcher");
    state.receive_endpoint_unavailable("build is not ready".to_owned());
    state.compose(60, 12).expect("notice up");
    let (rect, _) = badges_on(&state, 11)[0].clone();
    let dismissed = state.handle_raw_events(vec![tap(rect)]);
    assert!(state.visible_endpoint_notice.is_none());
    assert!(dismissed.actions.is_empty(), "{:?}", dismissed.actions);
    assert_eq!(state.mode, ClientShellMode::Navigate);
}

#[test]
fn zellij_phone_one_row_list_draws_no_badges() {
    use AgentStatus::Blocked;
    let mut state = badge_switcher(BADGE_CONFIG, badge_snapshot(&[Blocked; 9], &[Blocked]));
    state.mobile_switcher_scroll = 3;
    state.compose(60, 4).expect("one list row");
    assert!(badges_on(&state, BADGE_RULE_ROW).is_empty());
    state.compose(60, 5).expect("two list rows");
    assert!(!badges_on(&state, BADGE_RULE_ROW).is_empty());
}

#[test]
fn zellij_phone_connection_banner_keeps_the_rule_row_free_of_taps() {
    use AgentStatus::Idle;
    let mut state = badge_switcher(BADGE_CONFIG, badge_snapshot(&[Idle; 9], &[Idle]));
    state.mobile_switcher_scroll = 4;
    state.compose(60, 12).expect("phone switcher");
    assert!(!badges_on(&state, BADGE_RULE_ROW).is_empty());

    state.mark_endpoint_disconnected(&ClientEndpointId::Local);
    let frame = state.compose(60, 12).expect("banner up");
    assert!(frame_rows(&frame)[usize::from(BADGE_RULE_ROW)].contains("reconnecting"));
    assert!(badges_on(&state, BADGE_RULE_ROW).is_empty());
}

#[test]
fn zellij_phone_one_column_reserves_no_badge_row() {
    let mut state = phone_switcher(BADGE_CONFIG, badge_snapshot(&[AgentStatus::Idle; 9], &[]));
    state.compose(1, 12).expect("one column");
    let one_column = state.hits.mobile_max_scroll;
    assert!(badges_on(&state, 11).is_empty());
    state.compose(2, 12).expect("two columns");
    assert_eq!(state.hits.mobile_max_scroll, one_column + 1);
}
