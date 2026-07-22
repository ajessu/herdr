# TUI Sidebar / Resize Best Practices

## Resize
- tmux/zellij: entire border is a drag target (any point); zellij adds corner bidirectional + hover hints at bottom of active pane for discoverability.
- lazygit: no free drag — discrete screen modes (normal/half/full via `+`/`_`), config `sidePanelWidth`, `expandFocusedSidePanel` accordion, `portraitMode: auto` stacks on narrow terminals.
- neovim: drag split borders with `mouse=a`; no special handle glyph.
- helix: no resize at all (equal splits).
- btop/zenith (ratatui): `e` expand / `m` minimize active section; height 0 removes section.

## Minimize / Collapse patterns
- Toggle key (VS Code Ctrl+B; herdr toggle button).
- Click-to-collapse icon (`«`/`»`).
- Auto-collapse below width threshold (lazygit portrait mode).
- Accordion expand-focused.
- Collapsed remnant shows icons only (VS Code activity bar; Windows CompactOverlay 48px icon strip).

## Multiple affordances
- No existing TUI puts distinct handle glyphs at top/mid/bottom of one border (GUI/web pattern). Would be novel; nice-to-have for herdr touch.

## Touch / mobile (herdr runs in web terminal on phone)
- Terminal cells ~7×15px; far below 44pt touch target → need wider hit zones (2–3 cells) and visible affordances.
- Patterns: expanded hit zone, center grab glyph (`⋮`/`┃`), double-tap reset (herdr has it), snap points, drag-below-min → collapse, long-press to enter resize.
- crossterm events: Down/Drag/Up/Moved already used. Clear drag state on focus/tab change (zellij stale-latch bug #5251).
- WAI-ARIA splitter pattern: `role=separator`, arrows move, Enter toggles collapse.

## Recommendations applied to herdr
1. Minimized rail = first-class interactive surface: widen to ~6–8 cols, icon-only rows rendered button-like (bg/spacing), click-to-switch + status-jump.
2. Prominent expand/minimize toggle (accent glyph, larger hit area).
3. Thicker scrollbars (fuller glyph; widen track where width allows).
4. (nice-to-have) center grab glyph on the resize divider + wider hit zone.
