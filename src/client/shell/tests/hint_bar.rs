//! Fork: contextual hint bar.

use super::*;
use crate::client::shell::modal::ModalMode;
use crate::config::HintBarStyleConfig;

fn hint_shell(style: HintBarStyleConfig) -> ClientShellState {
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.hint_bar = style;
    let mut state = ClientShellState::new(config);
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state
}

#[test]
fn hint_bar_is_off_by_default_and_layout_is_unchanged() {
    let state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let layout = state.layout(106, 30);
    assert_eq!(layout.hint_bar, Rect::default());
    assert_eq!(layout.pane_surface.bottom(), 30);
}

#[test]
fn hint_row_takes_the_bottom_row_of_the_pane_region() {
    let off = hint_shell(HintBarStyleConfig::Off).layout(106, 30);
    for style in [HintBarStyleConfig::Full, HintBarStyleConfig::Compact] {
        let state = hint_shell(style);
        let layout = state.layout(106, 30);
        assert_eq!(
            layout.hint_bar,
            Rect::new(off.pane_surface.x, 29, off.pane_surface.width, 1)
        );
        assert_eq!(layout.pane_surface.height, off.pane_surface.height - 1);
        assert_eq!(layout.pane_surface.y, off.pane_surface.y);
        assert_eq!(
            state.surface_size(106, 30),
            ClientSurfaceSize {
                cols: layout.pane_surface.width,
                rows: layout.pane_surface.height,
            }
        );
    }
}

#[test]
fn hint_row_sits_above_a_bottom_tab_bar() {
    let mut state = hint_shell(HintBarStyleConfig::Full);
    state.config.tab_bar_position = crate::config::TabBarPositionConfig::Bottom;
    let layout = state.layout(106, 30);
    assert_eq!(layout.tab_bar.y, 29);
    assert_eq!(layout.hint_bar.y, 28);
    assert_eq!(layout.pane_surface.y, 0);
    assert_eq!(layout.pane_surface.bottom(), 28);
}

#[test]
fn no_reserved_hint_row_on_mobile_or_when_panes_would_vanish() {
    let state = hint_shell(HintBarStyleConfig::Full);
    let mobile = state.layout(44, 20);
    assert_eq!(mobile.hint_bar, Rect::default());
    assert_eq!(mobile.pane_surface, Rect::new(0, 2, 44, 18));

    let tiny = state.layout(106, 2);
    assert_eq!(tiny.hint_bar, Rect::default());
    assert_eq!(tiny.pane_surface.height, 1);
}

#[test]
fn hint_row_geometry_never_depends_on_the_mode() {
    let mut state = hint_shell(HintBarStyleConfig::Full);
    let terminal = state.layout(106, 30);
    for bytes in [&[0x10][..], &[0x1b], &[0x07], &[0x07], &[0x02]] {
        state.handle_input_bytes(bytes);
        assert_eq!(state.layout(106, 30), terminal, "mode {:?}", state.mode);
    }
}

#[test]
fn live_reload_toggles_the_hint_row() {
    let mut state = hint_shell(HintBarStyleConfig::Off);
    let before = state.surface_size(106, 30);
    let mut next = Config::default();
    next.ui.hint_bar = HintBarStyleConfig::Full;
    next.ui.tabs.powerline = false;
    state.config.apply_live_config(&next, &[], &[]);
    assert_eq!(state.config.hint_bar, HintBarStyleConfig::Full);
    assert!(!state.config.powerline);
    assert_eq!(state.surface_size(106, 30).rows, before.rows - 1);
}

fn rows(state: &mut ClientShellState, cols: u16, rows: u16) -> Vec<String> {
    let frame = state.compose(cols, rows).expect("frame");
    frame
        .cells
        .chunks(usize::from(frame.width))
        .map(|row| row.iter().map(|cell| cell.symbol.as_str()).collect())
        .collect()
}

#[test]
fn the_hint_row_follows_the_mode_and_nothing_covers_the_panes() {
    let mut state = hint_shell(HintBarStyleConfig::Full);
    let hint_y = usize::from(state.layout(200, 30).hint_bar.y);
    for (bytes, badge, expect) in [
        (&[][..], "NORMAL", "PANE"),
        (&[0x10][..], "PANE", "focus"),
        (&[0x1b][..], "NORMAL", "SESSION"),
        (&[0x07][..], "LOCKED", "unlock"),
        (&[0x07][..], "NORMAL", "TAB"),
        (&[0x02][..], "PREFIX", "workspace nav"),
        (&[0x1b][..], "NORMAL", "LOCK"),
        (&[0x0f][..], "SESSION", "navigator"),
    ] {
        if !bytes.is_empty() {
            state.handle_input_bytes(bytes);
        }
        let screen = rows(&mut state, 200, 30);
        let row = &screen[hint_y];
        assert!(
            row.contains(&format!(" {badge} ")) && row.contains(expect),
            "mode {:?}: {row:?}",
            state.mode
        );
        for (y, other) in screen.iter().enumerate() {
            if y != hint_y {
                assert!(
                    !other.contains(&format!(" {badge} ")),
                    "badge drawn over row {y}: {other:?}"
                );
            }
        }
    }
}

fn copy_mode_with_search_prompt(query: &str) -> ClientCopyModeState {
    ClientCopyModeState {
        pane_id: "pane_1".into(),
        content_revision: 0,
        geometry: (80, 24),
        cursor: crate::api::schema::PaneTextPoint { row: 0, col: 0 },
        offset_from_bottom: 0,
        max_offset_from_bottom: 0,
        entry_offset_from_bottom: 0,
        selection: None,
        search_prompt: Some(ClientCopySearchPrompt {
            direction: crate::api::schema::PaneCopySearchDirection::Forward,
            query: query.into(),
        }),
        search_query: String::new(),
        search_direction: None,
        search_matches: Vec::new(),
        search_total: 0,
        search_current: None,
        search_current_global: None,
        search_generation: 0,
        copy_after_search: false,
        alternate_screen_active: false,
    }
}

#[test]
fn copy_mode_keeps_its_search_prompt_in_the_hint_row() {
    let mut state = hint_shell(HintBarStyleConfig::Full);
    state.mode = ClientShellMode::Copy;
    state.copy_mode = Some(copy_mode_with_search_prompt("needle"));
    let hint_y = usize::from(state.layout(106, 30).hint_bar.y);
    let screen = rows(&mut state, 106, 30);
    assert!(screen[hint_y].contains("COPY"), "{:?}", screen[hint_y]);
    assert!(screen[hint_y].contains("needle"), "{:?}", screen[hint_y]);
}

#[test]
fn session_shows_update_ready_at_the_right_edge() {
    let mut state = hint_shell(HintBarStyleConfig::Full);
    let mut endpoint_snapshot = snapshot();
    endpoint_snapshot.update_available = Some("0.9.1".into());
    state.set_snapshot(Box::new(endpoint_snapshot));
    state.handle_input_bytes(&[0x0f]);
    let hint_y = usize::from(state.layout(106, 30).hint_bar.y);
    let screen = rows(&mut state, 106, 30);
    assert!(
        screen[hint_y].trim_end().ends_with("update ready"),
        "{:?}",
        screen[hint_y]
    );
}

const PHONE_COLS: u16 = 60;
const PHONE_ROWS: u16 = 30;
const PHONE_STATUS_COLS: usize = 49;
const HEADER_STATUS_ROW: usize = 1;

fn phone_rows(state: &mut ClientShellState) -> Vec<String> {
    rows(state, PHONE_COLS, PHONE_ROWS)
}

/// The status cells, and everything from the gap before the switch button on.
fn split_header_row(row: &str) -> (String, String) {
    let cells: Vec<char> = row.chars().collect();
    (
        cells[..PHONE_STATUS_COLS].iter().collect(),
        cells[PHONE_STATUS_COLS..].iter().collect(),
    )
}

/// A full last pane line, for telling a mode bar painted over it from the pane.
fn full_height_surface() -> PaneSurfaceFrame {
    let lines: Vec<String> = (0..usize::from(PHONE_ROWS) - 2)
        .map(|row| format!("pane line {row:02}"))
        .collect();
    let mut surface = surface();
    surface.frame =
        FrameData::from_ratatui_buffer_with_hyperlinks(&Buffer::with_lines(lines), None, &[]);
    surface
}

fn phone_shell(style: HintBarStyleConfig) -> ClientShellState {
    let mut state = hint_shell(style);
    state.set_pane_surface(full_height_surface());
    state
}

const LAST_PANE_LINE: &str = "pane line 27";

#[test]
fn phone_header_shows_each_mode_left_of_the_switch_button() {
    let switch = Rect::new(50, 0, 10, 2);
    for style in [HintBarStyleConfig::Full, HintBarStyleConfig::Compact] {
        for (bytes, badge) in [
            (&[0x10][..], "PANE"),
            (&[0x14][..], "TAB"),
            (&[0x0e][..], "RESIZE"),
            (&[0x08][..], "MOVE"),
            (&[0x0f][..], "SESSION"),
            (&[0x07][..], "LOCKED"),
            (&[0x02][..], "PREFIX"),
        ] {
            let mut state = phone_shell(style);
            state.handle_input_bytes(bytes);
            let screen = phone_rows(&mut state);
            let (status, button) = split_header_row(&screen[HEADER_STATUS_ROW]);
            assert!(
                status.contains(&format!(" {badge} ")),
                "{badge}: {status:?}"
            );
            if badge != "LOCKED" {
                assert!(status.contains("<esc>"), "{badge}: {status:?}");
            }
            assert!(button.contains("switch"), "{badge}: {button:?}");
            assert_eq!(state.hits.mobile_switch, switch, "{badge}");
            assert!(
                screen[29].contains(LAST_PANE_LINE),
                "{badge}: {:?}",
                screen[29]
            );
        }
    }
}

#[test]
fn phone_header_replaces_the_whole_agent_summary() {
    let mut projected = snapshot();
    projected.agents = [
        AgentStatus::Blocked,
        AgentStatus::Done,
        AgentStatus::Working,
        AgentStatus::Idle,
    ]
    .into_iter()
    .map(|agent_status| ClientShellAgent {
        pane_id: "pane_1".into(),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        name: None,
        display_agent: None,
        agent: Some("claude".into()),
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status,
        state_change_seq: 0,
        state_labels: Vec::new(),
        tokens: Vec::new(),
        focused: false,
    })
    .collect();
    let mut state = phone_shell(HintBarStyleConfig::Full);
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(full_height_surface());

    let screen = phone_rows(&mut state);
    assert!(
        screen[HEADER_STATUS_ROW].contains("idle"),
        "{:?}",
        screen[1]
    );

    state.handle_input_bytes(&[0x07]);
    let screen = phone_rows(&mut state);
    assert!(
        screen[HEADER_STATUS_ROW].contains("LOCKED"),
        "{:?}",
        screen[1]
    );
    assert!(
        !screen[HEADER_STATUS_ROW].contains("idle"),
        "{:?}",
        screen[1]
    );
}

#[test]
fn phone_header_shows_the_copy_search_prompt() {
    let mut state = phone_shell(HintBarStyleConfig::Full);
    state.mode = ClientShellMode::Copy;
    state.copy_mode = Some(copy_mode_with_search_prompt("needle"));
    let screen = phone_rows(&mut state);
    let (status, button) = split_header_row(&screen[HEADER_STATUS_ROW]);
    assert!(
        status.contains("COPY") && status.contains("needle"),
        "{status:?}"
    );
    assert!(button.contains("switch"), "{button:?}");
    assert!(screen[29].contains(LAST_PANE_LINE), "{:?}", screen[29]);
}

#[test]
fn phone_header_cuts_an_endpoint_error_at_the_switch_button() {
    let mut state = phone_shell(HintBarStyleConfig::Full);
    state.endpoint_error = Some(format!("boom {}", "x".repeat(80)));
    let screen = phone_rows(&mut state);
    let (status, button) = split_header_row(&screen[HEADER_STATUS_ROW]);
    assert!(
        status.contains("ERROR") && status.contains("boom"),
        "{status:?}"
    );
    assert!(
        button.contains("switch") && !button.contains('x'),
        "{button:?}"
    );
    assert!(screen[29].contains(LAST_PANE_LINE), "{:?}", screen[29]);
}

#[test]
fn phone_header_keeps_the_agent_summary_outside_a_mode() {
    let mut on = phone_shell(HintBarStyleConfig::Full);
    let mut off = phone_shell(HintBarStyleConfig::Off);
    assert_eq!(phone_rows(&mut on), phone_rows(&mut off));
}

#[test]
fn phone_switcher_covers_the_header_so_it_keeps_todays_path() {
    for error in [None, Some("boom")] {
        let mut on = phone_shell(HintBarStyleConfig::Full);
        let mut off = phone_shell(HintBarStyleConfig::Off);
        for state in [&mut on, &mut off] {
            state.mode = ClientShellMode::Navigate;
            state.endpoint_error = error.map(str::to_owned);
        }
        let screen = phone_rows(&mut on);
        assert_eq!(screen, phone_rows(&mut off), "error {error:?}");
        if error.is_some() {
            assert!(screen[29].contains("ERROR"), "{:?}", screen[29]);
        }
    }
}

#[test]
fn phone_overlay_keeps_todays_path_because_it_takes_the_keys() {
    for mode in [
        ClientShellMode::Navigate,
        ClientShellMode::Modal(ModalMode::Tab),
    ] {
        let mut on = phone_shell(HintBarStyleConfig::Full);
        let mut off = phone_shell(HintBarStyleConfig::Off);
        for state in [&mut on, &mut off] {
            state.mode = mode;
            state.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
                title: "new workspace",
                input: TextEditor::new("", true),
                target: ClientRenameTarget::Pane {
                    pane_id: "pane_1".into(),
                },
            }));
        }
        assert_eq!(phone_rows(&mut on), phone_rows(&mut off), "{mode:?}");
    }
}

#[test]
fn phone_with_the_hint_bar_off_keeps_the_mode_bar_over_the_panes() {
    let mut state = phone_shell(HintBarStyleConfig::Off);
    let plain = phone_rows(&mut state);
    state.handle_input_bytes(&[0x14]);
    let screen = phone_rows(&mut state);
    assert_eq!(screen[HEADER_STATUS_ROW], plain[HEADER_STATUS_ROW]);
    assert!(screen[29].contains(" TAB "), "{:?}", screen[29]);
    assert!(!screen[29].contains(LAST_PANE_LINE), "{:?}", screen[29]);

    let mut locked = phone_shell(HintBarStyleConfig::Off);
    locked.handle_input_bytes(&[0x07]);
    assert_eq!(phone_rows(&mut locked), plain);
}

#[test]
fn phone_too_narrow_for_the_row_falls_back_to_the_mode_bar() {
    for (cols, in_header) in [(11, false), (22, false), (23, true)] {
        let mut state = phone_shell(HintBarStyleConfig::Full);
        state.handle_input_bytes(&[0x14]);
        let screen = rows(&mut state, cols, PHONE_ROWS);
        let row = if in_header { HEADER_STATUS_ROW } else { 29 };
        assert!(screen[row].contains("TAB"), "{cols}: {:?}", screen[row]);
    }
}

#[test]
fn phone_with_one_row_has_no_status_row_and_still_composes() {
    let mut state = phone_shell(HintBarStyleConfig::Full);
    state.handle_input_bytes(&[0x14]);
    assert_eq!(rows(&mut state, PHONE_COLS, 1).len(), 1);
}

#[test]
fn phone_update_notice_never_covers_the_mode_or_its_exit_key() {
    for (cols, notice) in [(60, true), (48, true), (47, false), (24, false)] {
        let mut state = phone_shell(HintBarStyleConfig::Full);
        let mut projected = snapshot();
        projected.update_available = Some("0.9.9".into());
        state.set_snapshot(Box::new(projected));
        state.set_pane_surface(full_height_surface());
        state.handle_input_bytes(&[0x0f]);
        let row = rows(&mut state, cols, PHONE_ROWS).swap_remove(HEADER_STATUS_ROW);
        assert!(row.contains(" SESSION "), "{cols}: {row:?}");
        assert_eq!(row.contains("update ready"), notice, "{cols}: {row:?}");
        if cols >= 47 {
            assert!(row.contains("<esc>"), "{cols}: {row:?}");
        }
    }
}
