# Zellij Feature Research

## Stacked Panes
- Accordion-style: all panes except one show only title line (1 row)
- Navigating with Alt+direction expands target, collapses previous
- Layout syntax: `pane stacked=true { pane; pane expanded=true; pane }`
- Stacked resize (0.42.0): auto-stack neighbors when resizing active pane
- Killer feature for many-pane workflows

## Alt Shortcuts (Shared Mode)
- Work without entering any mode — zero overhead
- Core set: Alt+hjkl (navigate), Alt+n (new pane), Alt+f (float toggle), Alt+[] (swap layout), Alt+=/-  (resize), Alt+io (move tab)
- #1 ergonomic win over tmux prefix model
- Available in all modes except Locked

## Status Bar
- Implemented as WASM plugin (tab-bar, status-bar, compact-bar)
- Shows: current mode name + contextual shortcuts for that mode
- Dynamically updates as mode changes
- This is what makes Zellij learnable — the bar teaches you
- Compact bar: combines tabs + hints in single row

## Tab Features
- Sync mode: send keystrokes to ALL panes in tab
- Break pane to tab: move focused pane into its own new tab
- Break pane left/right: move pane to adjacent tab
- Move tab position: Alt+i/o reorders
- Toggle tab: switch to last-used tab

## Swap Layouts / Auto-Layout
- Predefined arrangements that auto-apply based on pane count
- `swap_tiled_layout` with constraints: max_panes, min_panes, exact_panes
- Cycle with Alt+[/]
- As panes are added/removed, layout adapts automatically
- Transformative for agent workflows

## Pinned Floating Panes (0.42.0)
- Always-on-top floating pane even when not focused
- Toggle pin: Ctrl+p+i
- Visual indicator differentiates from normal floating

## Community Most-Loved Features (ordered)
1. Alt shortcuts without mode entry
2. Floating panes
3. Stacked panes
4. Discoverable UI (status bar teaching)
5. Swap layouts / auto-layout
6. Session resurrection
7. Unlock-first preset (no key collisions)
8. WASM plugin system
9. Compact bar
10. Pinned floating panes
