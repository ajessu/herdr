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

    // row_gap goes between top-level spaces only, and between agents.
    assert_eq!(child.y, parent.bottom());
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
