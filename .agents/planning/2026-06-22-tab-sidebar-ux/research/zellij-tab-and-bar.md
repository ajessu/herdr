# Zellij Tab Bar & Status Bar — Reference

Source: zellij `default-plugins/{compact-bar,tab-bar,status-bar}`.

## Tab Overflow (the behavior to copy)

Zellij does **not** scroll one-by-one. Algorithm (`TabLinePopulator::populate_tabs`):

1. Active tab always rendered first (guaranteed visible).
2. Tabs split into `before_active` / `after_active`.
3. Loop adds tabs from each side, width-balanced: if `total_left <= total_right` add left, else add right; stop when `total_size > cols`.
4. When no more fit, insert **collapsed indicators** at the boundaries.

Indicator format:
- Left: `" <- +N "` (e.g. `← +3`)
- Right: `" +N -> "` (e.g. `+3 →`)
- Overflow fallback: `" <- +many "` / `" +many -> "`

Each indicator carries a `tab_index` of the nearest hidden tab → **clickable**, jumps there, which re-centers and reveals the next batch. Smart width accounting: if showing the last hidden tab on a side would remove the indicator entirely, subtract the indicator width from the projected total.

```rust
enum TabAction { AddLeft, AddRight, Finish }
struct CollapsedIndicators { left: LinePart, right: LinePart }
```

`LinePart { part: String /*ANSI*/, len: usize /*display width*/, tab_index: Option<usize> }`.
Click mapping: accumulate `LinePart.len` left→right until the click X is reached.

## Visual Painting

- Separator: `ARROW_SEPARATOR` = U+E0B0 (Powerline right triangle ``). If `capabilities.arrow_fonts` is false → separator = "" and tabs differentiated by **alternating background colors** instead.
- Separator painted with inverted fg/bg on each side for the angled Powerline transition.
- Active bg = `ribbon_selected.background`, fg = `ribbon_selected.base`.
- Inactive bg = `ribbon_unselected.background`; alternate bg (no arrow fonts) = `ribbon_unselected.emphasis_1`.
- Tab text: bold, single-space padding `" Name "`, suffixes `(FULLSCREEN)`, `(SYNC)`, `[!]` (bell).
- Collapsed indicator bg uses `emphasis_0` to distinguish from real tabs.

## Status / Bottom Bar Layout

Single-line mode (`one_line_ui`, rows==1):
- **Left**: mode key indicators with shared modifier prefix, e.g. ` Ctrl + <a> <b> <c>`.
- **Right**: secondary info (New Pane / Change Focus / Resize / Floating), **right-aligned** by prepending padding spaces: `remaining = max_len - secondary_info.len - 1`.

Keybind formats: `<a> Foobar`, `Ctrl + <a> Foobar`, grouped `<a|b|c>`, directions `<hjkl>`. Entries joined by ` / `.

Responsive degradation (3 tiers): full labels → short labels → ` ... ` ellipsis.

## Takeaways for herdr
- Replace herdr's `‹N` / `N›` one-by-one scroll with centered-active + clickable `+N` batch indicators.
- Add a right-aligned Alt-shortcut section to the hint bar (manual padding arithmetic like zellij).
- Optionally adopt Powerline separators with alternating-bg fallback.
