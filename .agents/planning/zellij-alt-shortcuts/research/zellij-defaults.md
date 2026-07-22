# Research: Zellij default Alt bindings (authoritative)

Sources:
- Default config (binding authority): `github.com/zellij-org/zellij/blob/main/zellij-utils/assets/config/default.kdl`
- New-pane / resize logic: `zellij-server/src/panes/tiled_panes/tiled_pane_grid.rs`
- Action semantics: `zellij.dev/documentation/cli-actions`

Version note: current defaults (post-0.41 "unlock-first/classic" preset). Alt set stable across recent releases; `Alt p` / `Alt Shift p` pane-group bindings are ~0.42+.

## Default Alt bindings (`shared_except "locked"`)

```kdl
bind "Alt f" { ToggleFloatingPanes; }
bind "Alt n" { NewPane; }
bind "Alt i" { MoveTab "Left"; }
bind "Alt o" { MoveTab "Right"; }
bind "Alt h" "Alt Left"  { MoveFocusOrTab "Left"; }
bind "Alt l" "Alt Right" { MoveFocusOrTab "Right"; }
bind "Alt j" "Alt Down"  { MoveFocus "Down"; }
bind "Alt k" "Alt Up"    { MoveFocus "Up"; }
bind "Alt =" "Alt +" { Resize "Increase"; }
bind "Alt -"         { Resize "Decrease"; }
bind "Alt [" { PreviousSwapLayout; }
bind "Alt ]" { NextSwapLayout; }
bind "Alt p"       { TogglePaneInGroup; }      # newer
bind "Alt Shift p" { ToggleGroupMarking; }     # newer
```

## Semantics

- **MoveFocusOrTab** (Alt+h/l): move focus that direction; **at screen edge, switch to adjacent tab**. Horizontal only.
- **MoveFocus** (Alt+j/k): move focus only, no edge fallback (tabs are horizontal).
- **NewPane** (Alt+n): no direction arg → auto. Picks the largest splittable pane and splits along its longer **cell-corrected** dimension.
- **Resize Increase/Decrease** (Alt+=/Alt+-): step = `RESIZE_PERCENT = 5.0` (5% per press). Increase grows the focused pane into reducible neighbors (with boundary inversion); Decrease shrinks it. Not a single-edge resize; no mode.
- **PreviousSwapLayout / NextSwapLayout** (Alt+[/]): cycle swap layouts — **NOT tab navigation**.
- **MoveTab Left/Right** (Alt+i/o): reorder the current tab.
- **ToggleFloatingPanes** (Alt+f), pane-group toggles (Alt+p): floating/group layers.

## NOT bound by default in Zellij
- No `Alt x` (close pane), no `Alt z` (zoom).
- No default **Alt** key for prev/next tab focus — tab nav is in Tab mode (`Ctrl t` then h/l). Cross-mode tab reach is only via Alt+h/l edge fallback.

## New-pane auto-direction algorithm (exact, from tiled_pane_grid.rs)

1. Cell aspect correction: `DEFAULT_CURSOR_HEIGHT_WIDTH_RATIO = 4` (cells ~4:1 tall vs wide for weighting).
2. `find_room_for_new_pane`: pick splittable pane with largest weighted size
   `pane_size = rows * ratio * cols`.
3. Direction for chosen pane — split along longer cell-corrected dim:
   ```rust
   let direction = if pane.rows() * ratio > pane.cols()
        && pane.rows() > pane.min_height() * 2 {
       Some(SplitDirection::Horizontal)   // taller → stack top/bottom
   } else if pane.cols() > pane.min_width() * 2 {
       Some(SplitDirection::Vertical)     // wider → side by side
   } else { None };                       // no room → stack
   ```
4. `split()` halves the chosen dimension.

So Zellij compares `rows * 4` vs `cols`. The rough idea's `width > height * 1.5` is a simplified herdr approximation of the same idea (cell aspect correction). Both pick "split along the longer visual dimension."
