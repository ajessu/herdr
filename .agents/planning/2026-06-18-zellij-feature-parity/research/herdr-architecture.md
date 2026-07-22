# Herdr Architecture Analysis

## Layout System (BSP Tree)
- File: `src/layout.rs`
- Binary Space Partition tree: `Node::Pane(PaneId)` or `Node::Split { direction, ratio, first, second }`
- No stacked/stack concept exists — would need new `Node` variant
- Operations: split_focused, close_focused, insert_pane_near, swap_panes, resize_focused, find_in_direction

## Tab Management
- File: `src/workspace/tab.rs`, `src/workspace.rs`
- Full support: create, close, rename, switch, reorder (move_tab)
- Tab bar rendering via `TabChrome` in `src/ui/tabs.rs`
- Stable public tab numbers (never recycled)
- Zoom: `Tab.zoomed` boolean for single-pane fullscreen

## Input System
- Files: `src/app/input/mod.rs`, `src/config/keybinds.rs`
- Mode system: Terminal, Prefix, Navigate, Copy, Resize, Rename*, Onboarding, Settings, etc.
- Prefix key model (default Ctrl+B) — tmux-compatible
- Direct bindings already supported (`BindingTrigger::Direct`)
- Alt modifier fully supported in keybind parser
- Safety: unmodified printable chars rejected as direct bindings
- Indexed bindings: `"prefix+1..9"` range syntax

## Status/UI
- No persistent bottom status bar
- Tab bar (1 row) + sidebar (left panel) are primary status surfaces
- Toast notifications for transient messages
- Navigate/prefix overlay shows shortcuts contextually
- Agent state dots in tab bar and sidebar

## Configuration
- TOML at `~/.config/herdr/config.toml`
- Full keybinding configuration via `KeysConfig`
- Custom commands via `[[keys.command]]`
- Theme, terminal, session, UI, advanced, experimental sections

## Key Observations for Feature Porting
1. Direct bindings exist → Alt shortcuts trivial to add as defaults
2. BSP tree is clean and extensible → Stack node variant is architecturally sound
3. No bottom bar → needs new UI surface in `src/ui.rs`
4. Mode system already has navigate/resize → contextual hints have data source
5. Mouse support is already rich → stacked pane title clicks feasible
