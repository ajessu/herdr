# Research: herdr keybinding infrastructure (file:line map)

## Config model — `src/config/model.rs`
- `KeysConfig` struct: `model.rs:295-408`. All binding fields are `BindingConfig`.
  - `focus_pane_left/down/up/right`: lines 369-375, currently `BindingConfig::one("prefix+h/j/k/l")` (defaults 595-598).
  - `split_vertical` (595→`one("prefix+v")` @606), `split_horizontal` (`one("prefix+minus")` @607).
  - `close_pane` (`one("prefix+x")` @608), `zoom` (`one("prefix+z")` @609, alias "fullscreen").
  - `resize_mode` (`one("prefix+r")` @610).
  - `next_tab` (`one("prefix+n")` @588), `previous_tab` (`one("prefix+p")` @587).
  - NO `split_auto`, NO `move_tab_*` keybind fields exist.
- `impl Default for KeysConfig`: `model.rs:557-616`.

## `BindingConfig` enum — `src/config/keybinds.rs:18-46`
```rust
#[serde(untagged)]
pub enum BindingConfig { One(String), Many(Vec<String>) }
```
Constructors `one()`, `empty()`, `values() -> Vec<&str>`. `#[serde(untagged)]` → TOML accepts string OR array. `Many` round-trips to a TOML array automatically. Existing single-string user configs still parse as `One`.

## Resolution / triggers — `src/config/keybinds.rs`
- `Keybinds` struct: `keybinds.rs:261-310` (resolved `ActionKeybinds` per action).
- `BindingTrigger`: `keybinds.rs:89-109` — `Direct(KeyCombo)` (standalone, e.g. `alt+h`) vs `Prefix(KeyCombo)`. `KeyCombo = (KeyCode, KeyModifiers)`.
- `Config::validated_keybinds`: `keybinds.rs:376-559`, builds `Keybinds` via `action!` macro → `parse_action_bindings(_owned)` (583-636) → `parse_binding_string` (813-860). `prefix+...` → `Prefix` trigger; anything else → `Direct`. A `Many(["prefix+h","alt+h"])` yields one Prefix + one Direct trigger.
- Conflict detection: `BindingRegistry` (324-374); `reject_binding` (776-811) rejects prefix==configured-prefix, dup combos, and **unmodified printable** direct keys (`is_unmodified_printable` 1198-1201). `alt+h` is modified → passes. Field declaration order decides conflict winner.
- Label rendering: `ActionKeybinds::label()/labels()` (184-218) already join `Many` as `"prefix+h / alt+h"`. Used by `src/ui/keybind_help.rs`, `src/ui/menus.rs:112-141`.
- Parser `parse_key_combo`: `keybinds.rs:970-1033`. Handles `alt`/`option`/`meta`→ALT. Confirmed parses `alt+=`, `alt+-`, `alt+[`, `alt+]` (single-char arm 1019-1027); `minus`/`plus` named aliases exist.

## Config export / TOML — `src/config.rs:82-92`
`local_keybindings_profile_toml` serializes whole `KeysConfig` via `toml::to_string_pretty`. New field with `#[serde(default)]` round-trips with no extra work.

## Actions — `src/app/input/navigate.rs`
- `NavigateAction` enum: `navigate.rs:568-615`. Split variants `SplitVertical` (598), `SplitHorizontal` (599). `ClosePane` (600), `Zoom` (603), `FocusPaneLeft/Down/Up/Right` (590-593), `EnterResizeMode` (604). NO `SplitAuto`, NO `MoveTab*`.
- `action_for_key` table: `navigate.rs:664-726`, split entries 707-708. Maps `(&kb.field, NavigateAction::X)`.
- Dispatch match `execute_navigate_action_in_context`: `navigate.rs:753-953`. Split handling 897-904:
  ```rust
  NavigateAction::SplitVertical   => { state.split_pane(rt, Direction::Horizontal); leave_navigate_mode(state); }
  NavigateAction::SplitHorizontal => { state.split_pane(rt, Direction::Vertical);   leave_navigate_mode(state); }
  ```
  (Note direction inversion: SplitVertical→Direction::Horizontal.)
- Direct (Alt) key path: `src/app/input/terminal.rs:37-56` → `terminal_direct_navigation_action` → `action_for_key(.., Direct)` → `execute_navigate_action_in_context(.., ActionContext::Direct)`.

## Split + geometry
- `AppState::split_pane`: `src/app/input/mod.rs:477-526`. Calls `estimate_pane_size()` then `ws.split_focused(direction, ...)`.
- Focused pane geometry: `PaneInfo { rect: Rect (cells), inner_rect, is_focused }` at `src/layout.rs:31-41`. `Rect = {x,y,width,height}`.
- Lookups on `AppState`: `pane_info_by_id` (`src/app/input/mouse.rs:1289-1291`), `estimate_pane_size` (`src/app/state.rs:1501-1507`), `Workspace::focused_pane_id` (`src/workspace.rs:1114`).
- Pane infos stored in `state.view.pane_infos`, computed `src/ui/panes.rs:149`.
- So `split_auto` arm can: focused pane id → `pane_info_by_id` → read `rect.width/height` → choose Direction → `state.split_pane`.

## Tab reorder (for Alt+i/o)
- `Workspace::move_tab(source_idx, insert_idx)`: `src/workspace.rs:571`.
- `AppState::move_tab`: `src/app/actions.rs:1241`. **Currently only invoked from mouse drag** (`src/app/input/mouse.rs:798`). NO keybind action exists → Alt+i/o needs a new `MoveTabLeft/Right` action wired, same effort class as `split_auto`.

## Raw input / Alt decoding — `src/raw_input.rs` + `src/input/parse.rs`
- `raw_input.rs` = byte framing only; decoding in `src/input/parse.rs`.
- Two Alt paths: (1) Kitty `\x1b[<cp>;<mod>u`, ALT = mod bit `0b10` (`parse_kitty_key_sequence` parse.rs:13-42); (2) legacy ESC-prefixed `\x1b<char>` → Char+ALT (`parse_legacy_key_sequence` parse.rs:72-80).
- **Ambiguity:** legacy `Alt+[` = `\x1b[` collides with CSI introducer; framer gives host control replies precedence (`raw_input.rs:205-225`). Kitty form `\x1b[91;3u` is unambiguous. **This is the main correctness risk** — but it only affects `Alt+[`/`Alt+]`, which we are NOT binding this wave (swap layouts deferred). Still worth a parser test for the symbol/bracket keys we DO bind (`alt+=`, `alt+-`).
- Both paths land as `KeyModifiers::ALT`, so a parsed `(Char('h'), ALT)` matches regardless of wire encoding.

## Edit sites summary
1. `model.rs`: change `focus_pane_*`, `close_pane`, `zoom` defaults `One`→`Many` (add alt); add `split_auto`, `move_tab_left`, `move_tab_right` fields + defaults.
2. `keybinds.rs`: add `split_auto`, `move_tab_left`, `move_tab_right` to `Keybinds` struct + `action!` literal.
3. `navigate.rs`: add `SplitAuto`, `MoveTabLeft`, `MoveTabRight` to `NavigateAction`; add to `action_for_key` table; add dispatch arms.
4. `keybind_help.rs` (+ menus.rs): add help entries.
5. No raw_input/parser/serialization changes required for the bound keys.
