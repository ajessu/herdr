# Task: Implement Alt shortcuts with config, dispatch, helpers, and tests

## Description
Add direct Alt+key keybindings that work without entering prefix mode, mirroring Zellij's "shared mode" ergonomics. This adds default Alt bindings alongside existing prefix bindings (not replacing them) and introduces five new actions: SplitAuto, MoveTabLeft, MoveTabRight, ResizeGrow, and ResizeShrink.

## Background
Herdr currently uses a tmux-style prefix model (ctrl+b then a key). Zellij provides direct Alt+key shortcuts that fire immediately in terminal mode. The infrastructure already exists: `BindingTrigger::Direct` for non-prefix bindings, `BindingConfig::Many` for multiple bindings per action, and an Alt-modifier parser that handles both Kitty and legacy ESC-prefixed encodings. This wave adds default Alt bindings alongside existing prefix bindings.

The design doc is at `.agents/planning/zellij-alt-shortcuts/design/detailed-design.md`.

## Technical Requirements

1. Change `KeysConfig` defaults for `focus_pane_left/down/up/right`, `close_pane`, and `zoom` from `BindingConfig::one("prefix+...")` to `BindingConfig::Many(vec!["prefix+...", "alt+..."])` in `src/config/model.rs`
2. Add five new `BindingConfig` fields to `KeysConfig`: `split_auto` (alt+n), `move_tab_left` (alt+i), `move_tab_right` (alt+o), `resize_grow` (alt+=), `resize_shrink` (alt+-) with `#[serde(default)]`
3. Add five corresponding `ActionKeybinds` fields to `Keybinds` struct and wire them in the `validated_keybinds` literal in `src/config/keybinds.rs`
4. Add five new variants to `NavigateAction` enum: `SplitAuto`, `MoveTabLeft`, `MoveTabRight`, `ResizeGrow`, `ResizeShrink`
5. Add entries in `action_for_key` table mapping the new keybinds to their actions
6. Implement dispatch arms in `execute_navigate_action_in_context` for all five new actions
7. Implement `AppState::auto_split_direction(&self) -> Direction` helper that returns `Direction::Horizontal` for wide panes (width > height * 1.5) and `Direction::Vertical` otherwise
8. Implement `AppState::focused_pane_rect(&self) -> Option<(u16, u16)>` helper returning `(width, height)` from the focused pane's `PaneInfo.rect`
9. Implement `AppState::resize_focused_pane(&mut self, grow: bool)` that tries horizontal axis then vertical axis as fallback
10. Change `AppState::resize_pane` return type from `()` to `bool` (whether ratios changed); gate `mark_session_dirty()` on the bool being true
11. Implement `AppState::move_active_tab_left(&mut self)` and `move_active_tab_right(&mut self)` with correct insert-before semantics (rightward: insert slot = target + 1)
12. Add help entries for the five new actions in `keybind_help_groups` in `src/ui/keybind_help.rs`
13. Leave `resize_mode` (`prefix+r`) unchanged

## Dependencies
- The codebase already has `BindingConfig::Many`, `BindingTrigger::Direct`, and Alt-key parsing infrastructure
- `Workspace::move_tab(source_idx, insert_idx)` at `src/workspace.rs:571` uses insert-before semantics
- `TileLayout::resize_focused` at `src/layout.rs:210` handles directional resize with 0.05 delta
- `AppState::split_pane(terminal_runtimes, direction)` at `src/app/input/mod.rs` handles the actual pane split
- `PaneInfo` struct at `src/layout.rs:31` provides `rect: Rect` with width/height
- `AppState::view.pane_infos` must be populated (non-empty) for geometry-dependent helpers to function

## Implementation Approach

1. **Config layer first**: Add new fields and change defaults in `model.rs`, then wire the resolved keybinds in `keybinds.rs`. This establishes the binding infrastructure.
2. **Actions and dispatch**: Add `NavigateAction` variants, `action_for_key` entries, and dispatch arms. The compiler's exhaustive match enforcement ensures nothing is missed.
3. **Helpers (pure logic)**: Implement `auto_split_direction`, `focused_pane_rect`, `resize_focused_pane`, and `move_active_tab_left/right`. These are testable without PTYs.
4. **Return type change**: Change `AppState::resize_pane` to return `bool`. Verify the two existing call sites (`handle_resize_key` in modal.rs, `capture_contract_tracks_resize_ratio_changes` in persist/snapshot.rs) compile with unused return value.
5. **Help screen**: Add entries for the new actions in the panes and tabs groups.
6. **Tests**: Focus on config resolution, pure helpers, tab reorder logic, and resize axis-fallback. Use `AppState::test_new()` and seed `view.pane_infos` for geometry tests.

Key pitfalls to avoid:
- `auto_split_direction` must use `(width, height)` tuple order (NOT the `(height, width)` of `estimate_pane_size`)
- `move_active_tab_right` must pass `target + 1` as insert slot (insert-before semantics)
- `resize_focused_pane` must try both axes; naive single-direction `resize_pane(Right)` silently no-ops on stacked layouts
- Direction enum inversion: `Direction::Horizontal` produces side-by-side columns (splits along width), `Direction::Vertical` produces stacked rows

## Acceptance Criteria

1. Alt+h/j/k/l focus the pane left/down/up/right in terminal mode
2. Alt+n splits the focused pane along its longer dimension (SplitAuto)
3. Alt+x closes the focused pane, Alt+z toggles zoom
4. Alt+=/- resize with axis fallback; stacked layout does not silently no-op
5. AppState::resize_pane returns bool; existing callers compile unchanged; mark_session_dirty gated
6. Alt+i/o move the active tab left/right with correct insert-before semantics
7. All existing prefix bindings unchanged
8. Single-string user configs still parse; absent new fields deserialize as unbound
9. Default config produces zero conflict diagnostics
10. TOML round-trip lossless for Many-valued bindings
11. Keybind help screen renders new Alt bindings alongside prefix bindings
12. resize_mode (prefix+r) unchanged
13. just check passes
