# Herdr Current Implementation — Map

## Tab Bar — `src/ui/tabs.rs`
Constants: `MIN_TAB_WIDTH=8`, `NEW_TAB_WIDTH=3`, `TAB_SCROLL_BUTTON_WIDTH=3`.

`compute_tab_bar_view` (≈434–600) is a 3-phase waterfall:
1. Natural fit (1-col gaps, `+` after last tab).
2. Uniform compression `compress_tab_widths` (319–349): single width for ALL tabs, only if `>= MIN_TAB_WIDTH`; truncate names with `…`; scroll always 0.
3. Scroll mode: reserve left(3)+right(3)+`+`(3); `centered_tab_scroll` (382–413).

Overflow indicators (682–735): `‹N ` left / ` N›` right (one-by-one), `9+` cap, dimmed when disabled. **No separator chars** between tabs — 1-col gaps. Active style = `fg(panel_contrast_fg).bg(accent)` BOLD. No-sliver invariant (296–303): tabs `< MIN_TAB_WIDTH` hidden except first at offset.

## Hint Bar — `src/ui/hint_bar.rs`
Single row. Left: colored mode badge + key hints. Right: only an "update ready" badge (627–646). `hints(mode, kb)` (518) builds per-mode `HintSet`. `build_hint_line` (533–606): badge + hints joined by spaces, Compact picks top-4 by priority + short labels, Full = all. Truncates with `…`. **No Alt/right-aligned shortcut section today.**

## Sidebar — `src/ui/sidebar.rs` + `src/app/input/sidebar.rs`
Collapsed width = `COLLAPSED_WIDTH=4` (`src/ui.rs:98`, "num + space + dot + separator").
- Collapsed render (637–747): per-workspace row `N ●`, divider, pane detail rows `N ●`, toggle `»`. Selected/active row gets bg fill. **Already renders icons; just cramped + minimal interactivity.**
- Expanded render (780–1138): spaces list + agents panel, section divider, right `│` border, toggle `«`.
- Resize: right column `sidebar.x+width-1` is drag target (`on_sidebar_divider`), clamps `[18,36]`, double-click resets to 26 (`mod.rs` 490–508), responsive ratio otherwise.
- Toggle rects: `collapsed_sidebar_toggle_rect` (1140–1148) center-bottom; `expanded_sidebar_toggle_rect` (1150–1160) bottom-right.

## Scrollbar — `src/ui/scrollbar.rs`
1-cell wide. Track glyph `▕` (1/8 block). Thumb `▐` (focused) / `▕` (unfocused) — half/eighth block. `render_scrollbar` writes a single column at `track.x`. Rects: `pane_scrollbar_rect`, `workspace_list_scrollbar_rect`, `agent_panel_scrollbar_rect`, `release_notes_scrollbar_rect`. "Thicker" achievable by: (a) fuller glyph `█`/`▉`, and/or (b) widening track Rect to 2 cols (steals 1 content col — gate on width).

## Existing Alt baseline binds — `src/app/input/dispatch_baseline.rs`
Alt+h/j/k/l = focus pane L/D/U/R; Alt+n = split auto; Alt+x = close pane; Alt+= / Alt+- = resize grow/shrink; Alt+i/o = move tab left/right. These are the right-side quick shortcuts to surface.

## Mouse plumbing
`src/app/input/mouse.rs`, `src/app/input/sidebar.rs`: `on_sidebar_divider`, `on_sidebar_section_divider`, tab click X→index mapping, `DragTarget` enum in `state.rs`. Any new clickable rect (batch indicators, minimized rows, prominent toggle) must register a matching hit-test.
