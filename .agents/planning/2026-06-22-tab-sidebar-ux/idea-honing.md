# Requirements Clarification

Mode: interactive. Answers below are from the user unless marked (assumption).

## Q1: What is the full scope of this change?
**A:** Four areas, in priority order:
1. **Tab overflow** — replace one-by-one scroll with zellij's batch-jump indicators.
2. **Bottom/hint bar** — add Alt shortcuts on the right (Ctrl-style on left), improve visual painting to match zellij.
3. **Sidebar minimize** (PRIORITY) — make the expand/minimize button more prominent; make the minimized rail wider and genuinely interactive (icons-only, button-like, click-to-switch + status-jump). Resize handle prominence is secondary.
4. **Scrollbars** — make both pane scrollback and sidebar scrollbars thicker.

## Q2: Tab overflow — copy zellij exactly?
**A:** Yes. Zellij keeps the active tab visible and shows collapsed counts on each side
(`← +3` / `+3 →`) that are clickable to jump to the next batch — not one-at-a-time scroll.
Also adopt zellij's visual painting (separators, active highlight).

## Q3: Bottom bar layout?
**A:** Ctrl shortcuts (mode entries) on the LEFT, Alt quick-shortcuts on the RIGHT.
herdr already has baseline Alt binds: Alt+h/j/k/l focus, Alt+n split, Alt+x close,
Alt+=/Alt+- resize, Alt+i/o move tab. These should be surfaced on the right.

## Q4: Minimized sidebar interactions?
**A:** Click-to-switch workspace + status-driven jump to the agent/pane needing attention.
NOT "click anywhere to expand" — the user wants to *interact with* the minimized rail, not
just expand it. The expand button itself should be separately more prominent. The rail should
work the same as expanded but with text removed: icons only, with more prominence/spacing so
each is a button-like target, not just a tiny circle.

## Q5: Minimized width?
**A:** Current 4 cols (`num + space + dot + separator`) is too cramped. Widen by a few cells so
the icon-only view is actionable. Status icons alone should convey enough to navigate + act.

## Q6: Which scrollbar thicker?
**A:** Both (pane scrollback + sidebar lists). Investigate configurability — herdr's scrollbar
is 1 cell using partial-block glyphs; "thicker" = fuller glyph and/or wider track.

## Six Dimensions

### Goals & Success Criteria
- Tab overflow navigable in batches (jump to next hidden group in one action), active tab always visible.
- Bottom bar shows Ctrl (left) + Alt (right) shortcuts with zellij-quality painting.
- Minimized sidebar is a usable interactive rail: wider, icon-only, button-like, click-to-switch + status-jump.
- Expand/minimize button is visually prominent.
- Both scrollbars visibly thicker.
- Success = side-by-side with zellij, the tab/bar UX reads as equivalent; minimized rail is usable on mobile.

### Functional Requirements
- FR1: Tab bar overflow uses centered-active layout with clickable `← +N` / `+N →` collapsed indicators that jump to the nearest hidden tab.
- FR2: Tab painting adopts zellij-style separators + active/inactive/alternate coloring (within herdr's palette).
- FR3: Hint bar renders a right-aligned Alt-shortcut section in addition to the existing left mode-entry section.
- FR4: Hint bar degrades responsively (full → short → drop right section → ellipsis) under width pressure.
- FR5: Minimized sidebar rail widened (target ~6–8 cols) with icon-only rows rendered as button-like cells (spacing/background), preserving click-to-switch and adding status-jump.
- FR6: Expand/minimize toggle rendered prominently (clear glyph + accent treatment, larger hit area).
- FR7: Both pane and sidebar scrollbars rendered thicker (fuller glyph; widen track to 2 cols where layout budget allows).

### Non-Functional Requirements
- NFR1: `render()` stays pure; geometry/hit-rects computed in `compute_view`/layout helpers (per CLAUDE.md).
- NFR2: No new deps. Use existing ratatui + buffer glyph approach.
- NFR3: Mouse hit-targets must match rendered rects exactly (click maps to correct tab/workspace/agent).
- NFR4: Graceful fallback when terminal lacks Powerline/Nerd glyphs (alternating bg like zellij).
- NFR5: Touch-friendly — wider hit zones for divider/toggle; works in mobile/web view.

### Scope (Out) / Non-Goals
- Not redesigning the agent panel content model.
- Not adding new keybinds (Alt binds already exist); only surfacing them.
- Not a configurable-theme overhaul; reuse current palette.
- Resize-handle multi-position glyphs are nice-to-have, not required (minimize is the priority).
- Not changing scrollback storage or scroll math — only thickness/rendering.

### Acceptance Criteria
- AC1: With more tabs than fit, the active tab is always visible and `+N` indicators appear on the overflow side(s); clicking one reveals the next batch.
- AC2: Bottom bar shows Alt shortcuts right-aligned; they truncate before the left section under width pressure.
- AC3: Minimized sidebar is wider, icon rows look button-like, clicking a workspace switches to it, clicking an attention status jumps to that agent/pane.
- AC4: Expand button is visually distinct and easy to hit.
- AC5: Both scrollbars are visibly thicker than today.
- AC6: `just check` passes; new layout/hit-rect logic has unit tests.

### Risks, Assumptions & Dependencies
- (assumption) Powerline arrow glyph availability varies; provide fallback. Detect via existing capability flags or default to safe glyph.
- (assumption) Widening minimized rail to ~6–8 cols won't crowd narrow terminals — clamp relative to total width.
- Risk: widening scrollbar track steals a content column; gate on available width and keep 1-col fallback.
- Risk: stale drag latch on focus/tab switch (zellij issue #5251) — clear drag state defensively.
- Dependency: existing mouse hit-test plumbing (`on_sidebar_*`, tab click mapping) must be updated in lockstep with render changes.
