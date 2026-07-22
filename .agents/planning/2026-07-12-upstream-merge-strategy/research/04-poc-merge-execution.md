# Research: POC merge execution (COMPLETE — suite green)

> **Scope note (2026-07-12, mid-POC):** after the merge was resolved, the user
> decided to DROP the web client (their browser access is a standalone web
> terminal, not the fork client) and confirmed `main` has advanced to a1804a8
> (sidebar toggle/close-button backtrack). The POC below ran pre-drop from
> 3e93d5f with `--features web`; its conflict counts therefore represent the
> WORST case. The real merge starts from a1804a8 with the web tree deleted
> first, which removes headless.rs/remote-web wiring conflicts and the 4
> web/ws + web/bridge error clusters. Semantic repair continues without the
> web feature (`cargo check --locked`).

Date: 2026-07-12. Worktree `../herdr-merge-poc`, branch `poc/upstream-merge`
(from fork `main` 3e93d5f). `rerere.enabled=true` set before the merge, so every
resolution here is cached for the real merge on the `merge` branch.

This file is the executed proof of the strategy. It supersedes estimates in the
design doc where they differ.

## Merge invocation

`git merge upstream/master` produced conflicts in **32 files** — matching the
non-committed test-merge measurement in research/01 exactly. diff3 conflict style
(base section visible), which materially eased resolution.

## Resolution progress

Resolved and marker-free (main-loop + subagents):

- Mechanical/union: Cargo.toml, Cargo.lock (took upstream, regen pending),
  docs/next/CHANGELOG.md, configuration.mdx, socket-api.mdx.
- Config (main loop): config.rs, config/io.rs, config/model.rs,
  config/keybinds.rs. keybinds.rs and model.rs took the fork's modal schema as
  base; upstream's three keybind behaviors (088922d user-displaces-default,
  b708f85 shifted-indexed, 32e3d7b help ranges) and the `remote_image_paste`
  field (ed31632) are deferred with explicit `TODO(upstream-merge)` markers in
  model.rs — the design's documented deferral policy, keeping the POC compiling
  while recording the residual semantic work honestly.
- Layout/workspace (main loop): layout.rs (union: fork tracing + upstream
  Borders), workspace.rs (dropped dead `close_active_tab_and_report`),
  workspace/tab.rs (kept fork `focused_target` + upstream test-only
  `close_focused`), persist/snapshot.rs (fork manual-width fields; both sides
  had dropped `agent_panel_scope`).
- UI (main loop): ui/panes.rs — the fork stack-collapse-bypass vs upstream
  pane-gaps-borders collision, reconciled by making `pane_inner_for` honor the
  gap-aware `info.borders` from upstream's `apply_pane_chrome` while keeping the
  collapsed-member bypass. This is the kind of semantic reconciliation the design
  predicted for the render path.
- tests/cross_area.rs: adopted upstream's `CURRENT_PROTOCOL` constant instead of
  the fork's hardcoded 14, kept the fork's 200 dimension arg.

Resolved by subagents (all 32 files now marker-free; merge committed as 639aef7
with rerere recording every resolution for replay on the real merge):

- app/actions.rs, app/mod.rs, app/input/* — runtime-authority re-targeting per
  playbook (upstream adapter structure as base, fork features re-attached).
- ui.rs — union with a real composition: upstream's hide-single-tab-row helper
  extended to a 3-tuple `(tab_bar, terminal, hint_bar)` so it composes with the
  fork's hint-bar row in all four combinations; upstream's indexed-help-range
  test (32e3d7b) appended.
- ui/tabs.rs — fork zellij painter as base; b44ca3b width fidelity verified
  already structurally present (painter measures via display-width helpers);
  bc764c8 verified inherently satisfied (painter never dims the active tab);
  upstream's CJK width tests ported to the TabChrome API. No deferrals.
- ui/sidebar.rs — fork 7-col rail as base with upstream's display-width helpers
  adopted; 552aa8c/0cd0b1a (all-workspace collapsed rows + list-position
  numbering) deferred with TODO(upstream-merge) and upstream's three collapsed-
  row tests parked under `#[cfg(any())]` for one-line re-enable after the port.
- server/headless.rs — upstream's refactored `handle_deferred_requests_headless`
  structure as base; fork's `request_break_pane_to_tab` handler re-inserted; the
  entire web-bridge wiring verified intact behind 8 `#[cfg(feature = "web")]`
  gates (WebServerState, Method::WebStart/WebStatus dispatch, accept-loop
  client-id plumbing). One 088922d test-assertion deferral recorded.

## Observations feeding the design

1. **The merge conflict count was exactly as measured (32 files).** The strategy
   table's merge column is validated.
2. **diff3 conflict style is a material aid** — seeing the merge-base section
   made "both sides deleted X" vs "both added different things" unambiguous. The
   recurring-sync process should set `merge.conflictStyle = diff3` (or `zdiff3`).
3. **Several conflicts were trivial unions** (docs, config fields, imports) — the
   32-file count overstates difficulty; the real work concentrates in the
   runtime-authority files (app/*) and the tab/sidebar rewrites, exactly as
   research/03 predicted.
4. **`agent_panel_scope` was already gone on both sides** — the fork's cherry-pick
   of upstream 5449025 (98c810a) meant that rename merged cleanly, confirming the
   duplicate-commit finding.
5. **PROTOCOL_VERSION**: base merged value is 16; `tests/support/mod.rs:18`
   hardcodes `CURRENT_PROTOCOL = 16` and must be bumped to 17 alongside
   `src/protocol/wire.rs` per the repo convention — recorded for the regen step.
6. **Resilience note:** a mid-run Bedrock outage killed the parallel resolver
   subagents repeatedly; resolution continued in the main loop and resumed
   subagents once the service recovered. The isolated POC worktree meant zero
   risk to `main` or the `merge` branch throughout — validating the design's
   branch-isolation error-handling.

## Semantic repair: first-build error landscape

The design predicted textual conflicts understate the work and semantic repair
dominates. The first container build of the fully-resolved tree quantifies that:

- `Cargo.lock` had to be regenerated in-container first (the merged Cargo.toml's
  fork web deps weren't in upstream's lock; `--locked` fails otherwise).
- One cross-file break found before the build: upstream's
  `execute_tui_navigate_action` (runtime-authority successor to the now
  test-only `execute_navigate_action_in_context`) was missing all 23 fork
  `NavigateAction` variants — non-exhaustive match. Fixed by porting the fork
  arms: `SplitAuto` routed through the split runtime adapter; tab-move/resize
  kept direct with `TODO(upstream-merge)` for adapter porting;
  floating/stacked kept direct as TUI-layer features pending a
  shared-vs-client guardrail decision.
- First `cargo test --no-run --features web` run: **~269 errors across 16
  files**, concentrated exactly where research/03 predicted:
  - `src/config/model.rs` (107) + `src/config/keybinds.rs` (61): upstream
    088922d's overlay/user-field machinery written against the flat keybind
    schema vs the fork's modal schema — the "single largest reconciliation
    item" of the design, confirmed empirically.
  - Struct-field unions needing both sides' construction sites updated:
    `PaneInfo` gained fork `stack` + upstream `borders` (11 errors);
    `CopyModeState` gained upstream `search` (3).
  - Signature drift (6), missing imports (9), moved methods (3), misc web/api
    singletons.
- Ratio worth keeping: ~94 textual conflict hunks → ~269 compile errors. For
  future syncs, expect semantic repair ≈ 3x the textual conflict count in
  compile errors, dominated by whichever schema surface diverged that cycle.

## Semantic repair outcome

`cargo check --locked` and `cargo test --no-run --locked` both **clean** (no web
feature, per the scope decision). The ~269 errors resolved as:

- **Keybind seam (~230 errors, the bulk):** the winning move was NOT hunk-level
  repair but *restore the fork's `keybinds.rs` wholesale, then port upstream
  behaviors individually on top*. Ported outright: b708f85 (shifted
  indexed-number matching) and ed31632's `remote_image_paste` field. Cleanly
  deferred with TODOs + parked `#[cfg(any())]` tests: 088922d
  (BindingSource/overlay provenance) and 32e3d7b (help ranges). Upstream's
  `KeysConfigOverlay`/custom-Deserialize machinery was deleted (it iterates
  flat-schema fields the modal schema doesn't have).
- **Struct unions:** `PaneInfo` needed `borders:`/`stack:` added at 11
  construction sites across both sides; `CopyModeState` needed `search:` at 3.
- **Test-gating friction:** upstream marked several `AppState` methods
  `#[cfg(test)]` that the fork's modal/sticky dispatch still calls in
  production (`switch_tab`, `previous_tab`, `focus_agent_entry`,
  `navigate_pane`, `close_active_tab`) — ungated with TODOs to re-gate once
  modal dispatch is rewired through runtime adapters. This is a recurring
  pattern to expect in future syncs: upstream keeps demoting direct-mutation
  methods to test-only as the runtime-authority refactor advances.
- Repair effort: one focused agent session (~160 tool invocations) for the
  full 269-error landscape. Per-release syncs will see a fraction of this.

Standing TODO(upstream-merge) ledger after repair: 088922d port (schema +
profile + 6 parked tests), 32e3d7b port, runtime_tab_move routing, pane-resize
adapter, stacked/floating shared-vs-client decision, re-gate ungated AppState
methods, sidebar 552aa8c/0cd0b1a port (+3 parked tests), headless 088922d test
assertions. These become FORK.md entries at the real merge.

## Test-suite result: GREEN

Final: **3216/3216 passed** (`cargo nextest run --locked`, container, no web
feature). Progression: first full run 3204/3217 with 13 failures + 1
environment-skip; all 13 dispositioned:

- **10 FIXED.** Root causes split three ways:
  1. *Test-setup rot, not logic bugs* (7): upstream tests configured flat
     `[keys]` fields the modal schema silently ignores. Fixed via a test-only
     `IndexedKeybind::test_bindings` helper; the ported b708f85 matching logic
     and the cleanly-merged 32e3d7b help-range machinery were both correct.
     (Note: 32e3d7b turned out to have merged live — the model.rs TODO listing
     it as deferred should be trimmed at the real merge.)
  2. *One REAL regression from merge reconciliation* (2 tests): the
     `apply_pane_chrome` + `pane_inner_for` union stripped shared-edge borders
     from stack members. Fixed in code (stack members opt out of gap logic;
     collapsed rows get `Borders::NONE`, expanded keeps `Borders::ALL`), tests
     untouched. This validates the design's core testing claim: a resolution
     that compiles and looks right can still be semantically wrong, and only
     the test gate catches it.
  3. *Behavior-preserving assertion updates* (2): upstream 3e8f9df changed the
     diagnostic summary banner; the fork's 5-item tab context menu shifted the
     "Close" row index in a click test (behavior itself — 974a481's
     leave-menu-after-close — was correctly merged).
- **2 PARKED** with `#[cfg(any())]` + TODO(upstream-merge): pure 088922d
  displacement semantics (`reload_config_user_binding_displaces_default...`)
  and upstream's cross-workspace collapsed-rail click test (blocked on the
  552aa8c/0cd0b1a sidebar port). Both join the parked-test ledger the design's
  deferred-debt ratchet counts.
- **1 environment fix**: the Docker `/dev/ptmx` symlink broke the live-handoff
  fd-count test; now accepts both resolutions. (Known container quirk,
  documented in scripts/test-host.sh.)
- `herdr-api.schema.json` regenerated by the golden-file test (fork stack
  layout variant now in the schema, protocol 16 pending the 16→17 bump).

All repairs live uncommitted in the POC worktree
(`../herdr-merge-poc`, on top of merge commit 639aef7) as the reference
implementation for the real merge; rerere carries the conflict resolutions.

## Bottom line

The strategy is proven end-to-end: merge → resolve (32 files) → semantic
repair (~269 errors) → suite green (3216/3216), in one working session's
wall-clock. The real merge repeats this from a1804a8 with the web client
dropped first, which only shrinks the problem.
