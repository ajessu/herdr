# Cluster-replay plan: rebase fork features onto fresh upstream base

Date: 2026-07-22. **Supersedes the merge-into-fork strategy** (aborted mid-merge).

## Strategy

Upstream is the base of truth. Start from `upstream/master` (ef85fa0, v0.7.5) and
replay only the fork's *surviving* feature clusters on top. Upstream's real commits
always sit underneath; we only ever edit our own feature commits. When a feature
collides with upstream's evolved architecture, we **redevelop** that feature commit
against upstream's current code rather than reconciling upstream into the fork.

Key efficiency: features we cut (web, old floating panes) are **omitted, never
replayed** — no "add then delete" churn. They simply aren't in the port list.

## Coordinates

- Base: `upstream/master` = ef85fa0 (v0.7.5).
- Fork tip: `origin/main` = 3b30f62. Merge-base = 4cf9f8e. 93 fork commits since.
- Work branch: `upstream-rebase` in worktree `../herdr-worktrees/upstream-rebase`.
- `merge` branch stays at b685972 (untouched) until `upstream-rebase` is
  container-green, then `merge` fast-forwards / resets to it and pushes.

## DROP by omission (never replayed) — ~20 commits

**Web client (13):** b741b12, a8f8f7f, cc7355e, f63f97b, 493c838, 11df0f4,
772b77f, 3950abc, 9533b40, f1e3d62, and web-specific scripts 62de5f0
(swap-restart), 2ef6486 (web tests default), e7ca9a9 (build --features web).
Rationale: standalone web terminal replaces it; upstream never had it.

**Old floating panes (6):** ad9d5bc, 8b162e3, f059619, f966f5b, d4d4174, ca9b685.
Rationale: superseded by upstream's native popup panes (2c7c8be, v0.7.4).

**Duplicate (1):** 98c810a "sort agent panel by priority" = upstream 5449025.

**Tunnel script:** the `scripts/herdr-tunnel` part of 48ea1f8. (Keep the
`allow_nested` flip from that commit; drop the tunnel script.)

## CHERRY-PICK clusters (clean or near-clean on upstream base)

Port these mostly as-is, edit only where upstream's surrounding code moved:

- **env scrub** 0019e4d (`src/env.rs` scrub_herdr_runtime_env) — check upstream
  9c45342 (preserve explicit session sockets) still holds.
- **render_ansi** c77066d (CursorTracker + SGR cache) — must absorb upstream
  2b99ced (cursor-hide ordering), b7015f1 (undercurl SGR), 02a6e87 (batch diff
  writes). Redevelop-lite: apply the fork's cache on top of upstream's current fns.
- **allow_nested flip** 3e93d5f (default true + same-server recursion guard) — minus tunnel.
- **scrollbar** 4f1f272 (fuller glyphs).
- **sidebar model-name + labels** 2e3cde6, 12e33f6, 1084b3f, c49e167, acc7cf5,
  516a881, 4d0ac81 — depends on sidebar rail cluster landing first.
- **statusline wrapper** 9056c02, 3b30f62 (claude integration assets).
- **host scripts** df5a2ce (test-host.sh), rebuild-host.sh fixes (minus web bits):
  50790a9, 1957eb0, dba63a8, 8c03a84, a73d5f1, 84e6644, add0f35, ea70d91.

## REDEVELOP clusters (collide with upstream architecture)

Upstream's runtime-authority refactor + evolved keybind/tab/sidebar code is the
base; rebuild the fork feature on top of it. Do NOT port the fork's plumbing.

- **Modal keybinds** 78cf5c1, faa2578, d6b4afa, 0721d51, aafbd0f: the fork's
  modal `[keys.*]` schema + mode-entry/sticky-executor/ModeAction. Biggest item.
  Upstream kept evolving the flat schema (BindingSource provenance 088922d,
  shifted-index b708f85, help-ranges 32e3d7b, floating-popup keybinds 2c7c8be).
  Redevelop the modal schema against upstream's current keybinds.rs/model.rs.
- **Alt shortcuts** 70143e3, 8e6d48b, ba7e23c: MoveTab/Resize direct alt keys +
  hint-bar section. Re-target at upstream's runtime_* adapters.
- **Zellij tabs** 70d5bf1 (TabChrome), a198e6d, bf1b0bc (TabStatusMode), dbe24fb,
  52dc68b, 9ab71c7, af4691b, 9cd7d0a (stateless painter), 12ff1b7, c1a0adc,
  15c8168, ce72c5e, 4514046, cf2465a, ad46cf0, bca5431, 83e0a82, 7932d8e,
  121ee40, a14623b, 90a42b2, 90968e2: the fork's tab-bar rewrite. Redevelop
  against upstream's tab_surface module + display-width helpers.
- **Sidebar rail** a225c95 (7-col rail), 53c44dc (overflow badges), 307942b,
  a1804a8 (toggle backtrack): vs upstream's sidebar token styles / collapsed
  startup / entry gaps (9c9490d, 5cfe5e5, b16465a, 5b91dae, d30ab1b). Redevelop.
- **Hint bar** 68876cd, 90ddd64, 87fdb3d, 06eefcb: mode-driven hint bar. Rebuild
  on upstream's current ui.rs row composition.
- **Stacked panes** 4654b36, 2bc7714, 0845f6a, 2bd3408, b065510: Node::Stack +
  geometry + persistence + public API. Check upstream layout.rs (Borders model)
  and popup panes don't already occupy this space; redevelop the stack tree ops.
- **break_pane_to_tab** 8b0200b: keybind + request handler. Re-target at runtime.
- **responsive sidebar width** 526d31e (sidebar_width_ratio): re-add config field
  + compute-view logic on upstream's sidebar geometry.
- **drag-state clearing** 2d1a0ab: clear stale drag on tab/ws switch + focus loss.
- **headless mode-entry intercept** aafbd0f: intercept mode-entry keys in headless
  terminal path (pairs with modal keybinds).

## Version anchors (set once features land)

- PROTOCOL_VERSION: upstream is 17; fork adds stack API surface → bump to 18.
- SNAPSHOT_VERSION: upstream is 3; fork stack layout → set to 5 (past upstream).
- CURRENT_PROTOCOL in tests/support/mod.rs → 18.

## Verification

Container `just test` (scripts/test-host.sh) green on `upstream-rebase` before
`merge` advances. Excludes the known /dev/ptmx live-handoff fd test.

## Ordering (dependencies)

1. Cherry-pick low-risk standalone: env scrub, render_ansi, scrollbar, host scripts, allow_nested.
2. Redevelop modal keybinds (foundation for alt-shortcuts, hint bar, headless intercept).
3. Redevelop zellij tabs + TabStatusMode.
4. Redevelop sidebar rail, then port sidebar model-name/labels on top.
5. Redevelop stacked panes.
6. Responsive width, drag-clearing, break_pane_to_tab, statusline wrapper.
7. Version anchors + regen (Cargo.lock, herdr-api.schema.json).
8. Container test gate.
