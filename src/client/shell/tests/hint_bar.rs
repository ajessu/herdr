//! Fork: contextual hint bar.

use super::*;
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
fn no_hint_row_on_mobile_or_when_panes_would_vanish() {
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

#[test]
fn copy_mode_keeps_its_search_prompt_in_the_hint_row() {
    let mut state = hint_shell(HintBarStyleConfig::Full);
    state.mode = ClientShellMode::Copy;
    state.copy_mode = Some(ClientCopyModeState {
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
            query: "needle".into(),
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
    });
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
