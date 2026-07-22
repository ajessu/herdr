# Task: Add right-aligned Alt-shortcut section to the bottom hint bar

## Description
Extend the hint bar to display a right-aligned Alt-shortcut section alongside the existing left mode-entry section. This surfaces the Alt quick-shortcuts (focus, split, close, resize, move tab) that already exist in herdr's baseline bindings, matching zellij's left/right hint bar split with responsive degradation under width pressure.

## Background
The hint bar (`src/ui/hint_bar.rs`) currently renders only mode-entry shortcuts (Ctrl-style) left-aligned in a single section. Herdr already has Alt-modifier bindings for common actions (e.g. `alt+h/j/k/l` focus, `alt+n` split, `alt+x` close, `alt+=/−` resize, `alt+i/o` move tab) dispatched via the `ActionKeybinds` system, but they are invisible to users. The design (FR3/FR4) calls for a right-aligned section that surfaces these Alt binds, sourced live from the configured keybinds so user remaps stay correct.

## Technical Requirements
1. Add `alt_binding_label` helper in `hint_bar.rs` that extracts the Alt-modifier Direct alternative from an `ActionKeybinds`.
2. Extend `HintSet` with `alt_hints: Vec<Hint>`.
3. In `build_hint_line`, compute right section and enforce FR4 degradation tiers.
4. Sanitize `alt_hints` key strings through `sanitize_key` explicitly.
5. No input/dispatch change.

## Acceptance Criteria
- Alt section renders in Terminal mode with correct Alt-modifier labels
- Many-binding Alt-alternative resolution works
- Drop-on-remap when Alt alternative removed
- 4-tier degradation: full labels → short labels → drop Alt section → ellipsis on left
- No overlap enforcement (left_used + right_width <= width)
- Sanitization of bidi/control chars in Alt bind
- `just check` passes
