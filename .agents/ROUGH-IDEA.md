# Wave: Alt Shortcuts (Zellij Feature 1)

## Goal
Add direct `Alt+key` keybindings that work without entering prefix mode, mirroring Zellij's "shared mode" ergonomics. This is the #1 ergonomic win over the tmux-style prefix model.

## Scope (this wave only)
Feature 1 from the master parity design. Self-contained, LOW effort (~1-2h), no architectural change — the infrastructure already exists (`BindingTrigger::Direct`, `BindingConfig::Many`, Alt modifier parser).

## What to build
Add Alt default bindings *alongside* existing prefix bindings (as `BindingConfig::Many`), so both work:

| Shortcut | Action | Prefix equivalent |
|----------|--------|-------------------|
| `Alt+h/j/k/l` | Focus pane left/down/up/right | `prefix+h/j/k/l` |
| `Alt+n` | New pane (split auto-direction) | `prefix+v` / `prefix+minus` |
| `Alt+x` | Close pane | `prefix+x` |
| `Alt+z` | Toggle zoom | `prefix+z` |
| `Alt+[` / `Alt+]` | Previous / next tab | `prefix+p` / `prefix+n` |
| `Alt+=` / `Alt+-` | Resize increase / decrease | resize mode |

Note: `Alt+{` / `Alt+}` are reserved for swap-layout cycling (a later wave) — do NOT bind them here.

## New action needed
- `split_auto`: picks split direction by aspect ratio. Algorithm: if focused pane `width > height * 1.5` → split Vertical (side-by-side); else → split Horizontal (stacked). Square defaults to Vertical.

## Key files
- `src/config/model.rs` — KeysConfig defaults (change `One` → `Many` for nav/close/zoom; add `split_auto`)
- `src/config/keybinds.rs` — Keybinds struct (add `split_auto` field; verify Many resolution in conflict detection + help rendering + config export)
- Input dispatch — wire `split_auto`

## Known constraints / risks
- Alt conflicts with terminal apps (vim) and nested tmux/screen. Mitigation: all configurable; users can drop Alt bindings. Document as known limitation, not a blocker.
- Verify Kitty keyboard protocol vs legacy ESC-prefixed Alt encoding (`src/raw_input.rs`) handles both reliably — this is the main correctness risk worth a test.
- Default TOML now emits arrays for these bindings; existing single-string user configs must still parse (they become `One`).

## Reference
Master design: `.agents/planning/2026-06-18-zellij-feature-parity/design/detailed-design.md` (Feature 1).

## Suggested next step
Small, well-understood change. Run `/roadmap` to decompose, or `/design <this file>` if you want a critique pass on the keybinding choices first.
