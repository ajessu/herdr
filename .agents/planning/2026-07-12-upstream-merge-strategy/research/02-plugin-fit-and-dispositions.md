# Research: upstream plugin capability surface and fork feature dispositions

Date: 2026-07-12. Sources: `git show upstream/master:` for docs/next plugins.mdx,
src/api/schema/plugins.rs, src/app/api/plugins/*, tests/fixtures/plugin-smoke/.

## Upstream plugin system: what plugins can and cannot do

A plugin is a directory with `herdr-plugin.toml` plus argv commands. No SDK, no
sandbox, no WASM. "The entire Herdr CLI is the plugin API" — plugins call back via
`HERDR_BIN_PATH` / raw socket (`HERDR_SOCKET_PATH`).

Manifest surface (`src/api/schema/plugins.rs`):

- `[[build]]` — install-time argv commands (GitHub installs only).
- `[[actions]]` — id/title/contexts (`global|workspace|tab|pane|selection`) + argv;
  invokable via CLI and bindable via `[[keys.command]] type = "plugin_action"`.
- `[[events]]` — hook on fixed `EventKind` set: workspace.*, worktree.*, tab.*,
  pane.* (created/closed/focused/moved/output_changed/exited/agent_detected/
  agent_status_changed), layout.updated.
- `[[panes]]` — plugin-owned terminal panes, placement `overlay|split|tab|zoomed`.
- `[[link_handlers]]` — regex over Ctrl-clicked URLs → plugin action.

Runtime env: socket path, bin path, plugin id/root/config/state dirs, invocation
context JSON (workspace/tab/pane/worktree/agent/selection/clicked URL).

**Cannot** (explicit in docs, "not part of plugin v1"): alter Herdr rendering (tab
bar, sidebar, hint bar, borders, status dots), add keybind modes or dispatch
semantics, intercept input/output, add layout node types. **Can**: read pane
content (`pane.read`, `terminal session observe` NDJSON stream — built "for
third-party bridges"), write input (`terminal session control --takeover`), drive
layout via pane move/swap/resize/zoom + layout export/apply, run long-lived
daemons, open own sockets/web servers.

## Disposition table for fork features

| Feature | Verdict | Evidence |
|---|---|---|
| Web: per-pane browser terminal | **PLUGIN-FIT** | upstream observe/control streams built for third-party bridges (bffc4a8, 0fa6440); plugin daemon + xterm.js works with no core patch |
| Web: full-app multiplexed client (tabs/sidebar in browser), trust-proxy, touch scroll | **KEEP-AS-FORK-PATCH** | fork bridge attaches as first-class rendering client via TerminalFrame; upstream exposes no attach-as-client stream; touches headless.rs, client_transport.rs, main.rs. RFC upstream at best (~4.7k lines) |
| Modal keybinds + Locked mode + hint bar | **KEEP-AS-FORK-PATCH** | plugins can't add modes/dispatch; upstream has only fixed copy/resize pseudo-modes and a different keybind philosophy (088922d); hint_bar.rs (1821 lines) is core UI |
| Tab bar zellij styling (TabChrome, overflow tiles, centered batching) | **KEEP-AS-FORK-PATCH** | rendering not plugin-reachable; styling opinion upstream unlikely to take |
| Tab middle-click close, ctx-menu move, status dots | **UPSTREAM-CANDIDATE** | upstream has tab context menu (state.rs:1178) but forwards middle-click to pane; status dots absent upstream; small opt-in additions |
| Tab-refresh fixes (ad46cf0, e713a0a) | **DROP** | superseded by upstream 010afe5, 974a481, 2a1a8d6, bc764c8 — verify at merge |
| Agent panel priority sort (98c810a) | **DROP** | same commit as upstream 5449025 ("refs #318"); resolves at merge |
| Sidebar attention badges (53c44dc, c1a0adc) | **UPSTREAM-CANDIDATE** | aligns with upstream's agents-first investment (db1ef28, 14d8e93) |
| Minimized 7-col rail, width ratio, close button | **KEEP-AS-FORK-PATCH** | compare vs upstream f54d8e8/552aa8c/0cd0b1a at merge; different UX intent |
| Floating panes | **UPSTREAM-CANDIDATE** (not plugin-expressible) | needs core z-order/input/layout; upstream has zero floating support; zellij-parity feature |
| Stacked panes | **UPSTREAM-CANDIDATE** (not plugin-expressible) | new layout node type + persistence schema must live in core; upstream has no Stack |
| rebuild/test/swap scripts | keep — personal dev tooling | encode fork's own build container/host; not features |
| herdr-tunnel.sh | **PLUGIN-FIT** | matches upstream plugin cookbook shape (workspace-context action running a script); depends on fork web server existing |
| ANSI encoder cursor tracking + SGR caching (c77066d) | **UPSTREAM-CANDIDATE** | pure perf, refs upstream #623; benefits upstream's own remote client and observe streams |
| allow_nested recursion check + default flip (3e93d5f) | **UPSTREAM-CANDIDATE** | refs upstream #148; recursion check stands alone even if default flip is rejected |
| Env scrub shared module (0019e4d) | **UPSTREAM-CANDIDATE** | bug fix — upstream remote client leaks HERDR_ENV=1, trips nested guard when tunneling |
| break_pane_to_tab / alt+key nav | **DROP into config/plugin or small PR** | expressible today: keybound plugin_action calling `herdr` pane move with PaneMoveDestination::NewTab (upstream panes.rs:65); native PR avoids per-keystroke spawn latency |
| Scrollbar fuller glyphs (4f1f272) | small UPSTREAM-CANDIDATE or keep | cosmetic |

## Net divergence effect

If DROP + PLUGIN-FIT + UPSTREAM-CANDIDATE items eventually land, the standing
fork diff reduces to three core patches: (1) full-app web client, (2) modal
keybinds + hint bar, (3) zellij tab-bar/sidebar styling — plus floating/stacked
panes while their upstream PRs are pending. That is the long-term maintenance
surface.
