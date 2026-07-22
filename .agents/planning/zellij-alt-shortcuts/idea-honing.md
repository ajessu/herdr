# Requirements Clarification

Directive from user: **"Research Zellij. Do the same. No decisions to be made by us."**
Zellij is the authority. Mirror its default behavior; do not invent herdr-specific semantics.

## Zellij default Alt bindings (authoritative — from `zellij-utils/assets/config/default.kdl`, `main`)

Active in `shared_except "locked"` (i.e. normal/shared mode):

| Key(s)            | Zellij action          | Behavior |
|-------------------|------------------------|----------|
| `Alt h` / `Alt ←` | `MoveFocusOrTab "Left"` | Focus pane left; **if at screen edge, switch to previous tab** |
| `Alt l` / `Alt →` | `MoveFocusOrTab "Right"`| Focus pane right; **if at edge, switch to next tab** |
| `Alt j` / `Alt ↓` | `MoveFocus "Down"`      | Focus pane down only (no edge fallback) |
| `Alt k` / `Alt ↑` | `MoveFocus "Up"`        | Focus pane up only (no edge fallback) |
| `Alt n`           | `NewPane`              | New pane, auto direction (largest pane, split along longer cell-corrected dim, 4:1 ratio) |
| `Alt =` / `Alt +` | `Resize "Increase"`    | Grow focused pane 5% into reducible neighbors |
| `Alt -`           | `Resize "Decrease"`    | Shrink focused pane 5% |
| `Alt [`           | `PreviousSwapLayout`   | Cycle swap layouts (NOT tab nav) |
| `Alt ]`           | `NextSwapLayout`       | Cycle swap layouts (NOT tab nav) |
| `Alt i`           | `MoveTab "Left"`       | Reorder current tab left |
| `Alt o`           | `MoveTab "Right"`      | Reorder current tab right |
| `Alt f`           | `ToggleFloatingPanes`  | Show/hide floating pane layer |
| `Alt p`           | `TogglePaneInGroup`    | (newer pane-group feature) |
| `Alt Shift p`     | `ToggleGroupMarking`   | (newer pane-group feature) |

**Source:** github.com/zellij-org/zellij `default.kdl`; resize step `RESIZE_PERCENT = 5.0` and new-pane algorithm in `zellij-server/src/panes/tiled_panes/tiled_pane_grid.rs`; action docs zellij.dev/documentation/cli-actions.

## Where Zellij defaults DIVERGE from the rough-idea table

The rough idea (derived from herdr's master parity design) does **not** match Zellij defaults:

1. **Alt+h/l** — rough idea = pure focus. Zellij = `MoveFocusOrTab` (focus, **with edge fallback to switch tabs**). Alt+j/k are pure focus in both.
2. **Alt+[ / Alt+]** — rough idea = previous/next **tab**. Zellij = previous/next **swap layout**. Zellij has **no default Alt key for tab switching** — tab nav lives in Tab mode (`Ctrl t` then h/l). Swap layouts are a *later herdr wave*, not this one.
3. **Alt+x (close pane)** — **unbound in Zellij.**
4. **Alt+z (toggle zoom)** — **unbound in Zellij.**
5. **Alt+i / Alt+o (MoveTab reorder)** — bound in Zellij, **absent from rough idea.**
6. **Alt+f (floating panes), Alt+p (pane group)** — bound in Zellij; herdr has **no floating-pane or pane-group system** → cannot mirror this wave.

## herdr capability constraints (what can actually be mirrored now)

- ✅ Pane focus L/D/U/R — herdr has `FocusPaneLeft/Down/Up/Right`.
- ✅ New pane — herdr has split; needs a new `split_auto` action (no auto-direction split exists today).
- ⚠️ `MoveFocusOrTab` edge fallback — herdr focus actions do **not** currently fall back to tab switch at the edge. Mirroring Zellij requires either adding that behavior or accepting pure focus.
- ⚠️ Direct `Resize "Increase/Decrease"` — herdr resize is a **mode** (`prefix+r` → resize mode), not a fixed-step direct action. Mirroring Zellij's 5% direct resize requires a new direct resize action.
- ❌ Swap layouts (`Alt [ ]`) — not implemented in herdr (later wave).
- ❌ Floating panes (`Alt f`), pane groups (`Alt p`) — not implemented in herdr.
- ✅ Move/reorder tab (`Alt i/o`) — needs verification herdr has a tab-reorder action.

## RESOLVED decisions (user, 2026-06-19)

1. **Alt+h/j/k/l** → **plain directional pane focus**. No MoveFocusOrTab edge-fallback (diverges from Zellij; simpler, matches rough idea). Maps to existing `FocusPaneLeft/Down/Up/Right`.
2. **Alt+= / Alt+-** → **direct fixed-step resize, faithful to Zellij** (REVISED after critique — supersedes the earlier "enter resize mode" answer). Alt+= grows the focused pane, Alt+- shrinks it, by 5% (no mode entry). herdr already has the primitive: `AppState::resize_pane(NavDirection)` → `resize_focused(dir, 0.05, area)`, and 0.05 == Zellij's `RESIZE_PERCENT = 5.0`. So `resize_mode` (`prefix+r`) is left unchanged; two new direct actions `ResizeGrow`/`ResizeShrink` are added.
3. **Binding set** = **Zellij subset + herdr extras**:
   - Zellij-faithful subset herdr can do: `Alt+h/j/k/l` (focus), `Alt+n` (new pane / split_auto), `Alt+=`/`Alt+-` (resize mode), `Alt+i`/`Alt+o` (move tab).
   - herdr extras (not in Zellij defaults but herdr supports): `Alt+x` (close pane), `Alt+z` (zoom).
   - **Skipped** (not implemented in herdr): `Alt+[`/`Alt+]` swap layouts, `Alt+f` floating panes, `Alt+p` pane groups.

## Final binding set for this wave

| Shortcut | herdr action | New action? | Trigger style |
|----------|--------------|-------------|---------------|
| `Alt+h/j/k/l` | FocusPaneLeft/Down/Up/Right | no (exists) | Direct |
| `Alt+n` | SplitAuto | **yes** | Direct |
| `Alt+x` | ClosePane | no | Direct |
| `Alt+z` | Zoom | no | Direct |
| `Alt+=` | ResizeGrow (new) → `resize_pane(NavDirection::Right)` | **yes** | Direct |
| `Alt+-` | ResizeShrink (new) → `resize_pane(NavDirection::Left)` | **yes** | Direct |
| `Alt+i` / `Alt+o` | MoveTabLeft / MoveTabRight | **yes** (capability exists, no keybind action today) | Direct |

Existing-action Alt bindings added as `BindingConfig::Many([<existing prefix>, <new alt>])` so prefix bindings still work. Existing single-string user configs still parse as `One`.

### New actions required
- `SplitAuto`: pick split direction by focused-pane aspect ratio. Mirror Zellij's "split along longer cell-corrected dimension." Mapping (CODE is authority — earlier prose in this doc was inverted): **wide focused pane (`width > height * 1.5`) → `Direction::Horizontal` = side-by-side (two columns); else → `Direction::Vertical` = stacked (two rows); square → stacked.** Verified against `src/layout.rs` `split_rect`: `Direction::Horizontal` splits along width (side-by-side), `Direction::Vertical` splits along height (stacked).
- `ResizeGrow` / `ResizeShrink`: direct fixed-step resize. Reuse the `0.05` step via `AppState::resize_pane(NavDirection)` (`src/app/actions.rs:1617` → `resize_focused(dir, 0.05, area)`, `src/layout.rs:210`). `resize_focused` semantics: Right/Down → grow, Left/Up → shrink. **IMPORTANT:** `resize_focused`'s internal `.or_else` flips only the edge, NOT the axis (`nearest_resize_split` filters by fixed `target_dir`, `src/layout.rs:359`), so a horizontal-only `Right`/`Left` resize is a **silent no-op on a stacked layout**. Therefore a new helper `AppState::resize_focused_pane(grow)` tries the horizontal axis, then falls back to the vertical axis if the ratios did not change — requires `AppState::resize_pane` to return its change-detection `bool`. No mode entry.
- `MoveTabLeft` / `MoveTabRight`: wire existing `AppState::move_tab` (currently mouse-drag only) to a keybind action.

### Resize note (REVISED)
`resize_mode` (`prefix+r`) is left UNCHANGED. The Alt resize keys are direct, faithful to Zellij (Alt+= grow, Alt+- shrink, 5% step). `resize_focused` already uses `delta = 0.05` == Zellij's `RESIZE_PERCENT`.

## Verified capability facts
- `AppState::move_tab` exists (`src/app/actions.rs:1241`), only wired to mouse drag today → Alt+i/o needs a new action, same effort class as split_auto.
- `alt+=`, `alt+-` parse correctly (`parse_key_combo`). `alt+[`/`alt+]` have a legacy-encoding ambiguity (CSI collision) but are NOT bound this wave, so the risk is deferred with swap layouts.
