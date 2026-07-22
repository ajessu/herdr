# Upstream-rebase: live state + resume instructions

Last updated: 2026-07-22. Read this FIRST in a fresh session, then read
`cluster-replay-plan.md` (same dir) for the full 93-commit bucketing.

## Strategy (one line)

Upstream is the base of truth. Start from `upstream/master` (v0.7.5) and replay
only the fork's *surviving* feature clusters on top, as editable commits. Features
we cut (web, old floating panes) are OMITTED, never replayed. When a fork feature
collides with upstream's evolved architecture, REDEVELOP that feature commit
against upstream's current code — do not reconcile upstream into the fork.

The earlier "merge upstream into fork" approach was ABORTED. Do not resume it.

## Branch / worktree layout

- `merge` branch @ b685972 — UNTOUCHED safety anchor (two `chore: drop` commits on
  origin/main). Do NOT advance it until `upstream-rebase` is container-green.
- Work branch `upstream-rebase` in worktree `../herdr-worktrees/upstream-rebase`,
  based on `upstream/master` = ef85fa0 (v0.7.5).
- Reference: `../herdr-merge-poc` (branch poc/upstream-merge, merge 639aef7) — an
  OLD fully-resolved merge tree; consult for intent only, its base differs.
- rerere.enabled=true, merge.conflictStyle=zdiff3 set in the worktree.
- Run commands with `git -C /workplace/albjessu/waves/herdr-worktrees/upstream-rebase`.
  The Bash tool resets cwd each call, so always use absolute paths / `git -C`.

## Coordinates

- upstream/master = ef85fa0 (v0.7.5). PROTOCOL_VERSION=17, SNAPSHOT_VERSION=3 upstream.
- origin/main = 3b30f62 (fork tip). merge-base = 4cf9f8e. 93 fork commits since.
- Fork target versions once features land: PROTOCOL_VERSION=18, SNAPSHOT_VERSION=5,
  tests/support/mod.rs CURRENT_PROTOCOL=18.

## Build/test (container only — host glibc too old)

`scripts/test-host.sh` runs cargo nextest in a Debian container. Compile-check with
`scripts/test-host.sh --no-run`. Full run excludes the known /dev/ptmx test:
`scripts/test-host.sh -E 'not test(live_server_holds_one_pty_master_fd_per_pane)'`.
Runs are slow (~2-4 min compile); launch in background, don't block.

## Ordering (dependencies)

1. Cherry-pick low-risk standalone: allow_nested, env scrub, render_ansi, scrollbar, host scripts.  <-- IN PROGRESS
2. Redevelop modal keybinds (foundation for alt-shortcuts, hint bar, headless intercept).
3. Redevelop zellij tabs + TabStatusMode.
4. Redevelop sidebar rail, then port sidebar model-name/labels on top.
5. Redevelop stacked panes.
6. Responsive width, drag-clearing, break_pane_to_tab, statusline wrapper.
7. Version anchors + regen (Cargo.lock, herdr-api.schema.json, vendored patch index).
8. Container test gate → fast-forward `merge` → push.

## Recurring gotcha (IMPORTANT)

Cherry-picking a fork commit drags its DIFF CONTEXT, which references fork features
NOT yet on the upstream base (e.g. TabStatusMode, tabs.powerline, break_pane_to_tab
tests showed up inside the allow_nested cherry-pick). When resolving: keep ONLY the
content genuinely belonging to THIS commit's feature; drop context-dragged tests for
other clusters — they arrive with their own clusters later. HEAD (upstream) side of
such conflicts is usually empty; that's the signal.

## CURRENT PROGRESS — step 1, allow_nested (3e93d5f) DONE ✓

Committed as `75095f8` "feat: flip allow_nested default to true and add
same-server recursion check". Resolution recap (for reference):

- `src/config/model.rs`: kept only the 3 allow_nested tests; dropped
  context-dragged tab-status/powerline/break-pane tests. `allow_nested` field +
  default(true) at lines ~883/923.
- `src/main.rs`, `src/server/autodetect.rs`, `tests/auto_detect.rs`: auto-merged clean.
- `tests/cli_wrapper.rs`: modify/delete conflict — `git rm` (upstream moved these
  into tests/cli/ via 3f80947; surface.rs already has the nested-guard test).
- `tests/cli/surface.rs`: fixed stale `explicit_client_command_respects_nested_guard`
  to write `[experimental]\nallow_nested = false` into
  `$XDG_CONFIG_HOME/herdr-dev/config.toml` so the guard still fires under the flipped
  default. Uses harness `app_dir_name()`.

Compile-check: PASSED (container, 2m26s, exit 0).

## env scrub 0019e4d — DONE ✓ (commit a55782f)

Clean 3-way auto-merge (plugin.rs, main.rs, remote/unix.rs; env.rs new). No
conflicts. Verified: all env-var constants the new src/env.rs references
(`crate::HERDR_ENV_VAR`, `crate::integration::HERDR_{WORKSPACE,TAB,PANE}_ID_ENV_VAR`)
exist on the upstream base. Scrub is wired ONLY into the 2 intended spawn sites
(plugin build child @ plugin.rs:1324, remote client launcher @ unix.rs:1945);
handoff.rs is untouched, so upstream 9c45342 (preserve explicit session sockets
during live handoff) survives intact.

Test RUN surfaced two upstream-base drifts (both fixed, AMENDED into 06f71ff):
1. Drift test failed: upstream base now sets `HERDR_ARGV_CAPTURE` on subprocesses
   in `src/platform/windows.rs` — but only inside `#[cfg(test)] mod tests` (an
   argv-capture fixture, not a real server-identity leak). Added it to env.rs
   `exemptions` with a comment. (The drift test greps the live src tree and can't
   distinguish test code from production — expected behavior, exemption is correct.)
2. `unused import: Command` in plugin.rs:5 — the fork removed the inline
   scrub fn that used `Command`; trimmed the import to `{ExitStatus, Stdio}`.
Commit amended → 06f71ff. Re-run GREEN: all 3 tests pass
(scrub_removes_known_herdr_vars, build_client_command_strips_herdr_env_vars,
scrub_herdr_vars_drift_test), no unused-import warning. env-scrub cluster COMPLETE.

## render_ansi c77066d — DONE ✓ (commit 2df7847), tests running (bt5fanu13)

This was the anticipated ARCHITECTURE COLLISION, resolved:
- Related upstream commits 2b99ced (cursor-hide in sync), b7015f1 (undercurl SGR),
  02a6e87 (batch diff writes) are ALL already on the upstream base.
- KEY FINDING: upstream INDEPENDENTLY implemented the same diff-path CUP-skip
  optimization the fork's c77066d adds, but via a caller-side mechanism
  (`next_inline_col` + `write_cell(cursor_position: Option<(u16,u16)>)`). The fork
  used a `CursorTracker` object instead, AND extended it to the full-redraw path
  (`write_all_cells`) with SGR caching (upstream full-redraw had neither).
- RESOLUTION: adopted the fork's CursorTracker fully; dropped upstream's redundant
  `next_inline_col`/`cursor_position` path. Rationale: the merged caller
  (blit_frame_to_with_cursor_memory) already auto-merged to expect the fork's `u64`
  cups_skipped return + render_prof counter, and CursorTracker is a superset
  (tracks full position incl. wide-char advance, per-row reset). The struct,
  write_all_cells, and counter plumbing auto-merged clean; only write_cell + its
  diff call site collided. Removed the now-dangling `next_inline_col` decl+comment.
- DESIGN TRADEOFF (noted, intentional): upstream forced a CUP after any non-ASCII/
  wide cell (conservative vs unicode-width disagreement); the fork instead bounds
  that risk to a single row via reset_for_row(). This is the fork's tested choice.
- UNDERCURL LEAK CHECK (state-file concern): SATISFIED. Cache key is
  build_sgr(fg,bg,modifier); underline style (undercurl=3) is bit-PACKED into the
  `modifier` u16 (UNDERLINE_STYLE_MASK/SHIFT in wire.rs), so build_sgr fully
  captures undercurl variance — no style leak. No extra work needed.

TEST RUN #1 surfaced a failing UPSTREAM test
`full_redraw_skips_trailing_cells_covered_by_wide_graphemes`: c77066d had flipped
its assertion to `!contains("\x1b[1;3H")`, but that sequence is emitted by the
HOST-CURSOR PARK (bottom-right when frame.cursor=None), NOT a per-cell CUP. The
fork itself already caught+fixed this in follow-up commit 90a42b2 ("revert
wide-grapheme test assertion and apply rustfmt"). Applied that fix: assertion
back to `contains` with the corrected comment. Rest of 90a42b2 is pure rustfmt
of the tests c77066d added.

FMT: upstream enforces `cargo fmt --check` via `just check`. Ran cargo fmt; it
reflowed test/code in ALL THREE commits (main.rs+model.rs → allow_nested,
env.rs → env-scrub, render_ansi.rs → render_ansi). Folded each file group back
into its originating commit via `git commit --fixup` + `rebase -i --autosquash`.

## CURRENT HEAD (clean 3-commit replay on upstream ef85fa0):
- 5e907bc feat: flip allow_nested default to true ...
- eab0ab4 fix(remote): scrub all herdr env vars from client subprocess
- 4f09ba8 feat: add cursor tracking and SGR caching to ANSI frame encoder
(SHAs changed from the pre-autosquash 75095f8/06f71ff/1c1d64a.)

VALIDATED: full-crate `cargo fmt --check` clean (exit 0). render_ansi test module
50/50 PASS — including the fixed full_redraw_skips_trailing_cells_covered_by_wide_
graphemes and all fork cursor-tracking + sgr-caching tests. render_ansi COMPLETE.

## scrollbar glyphs 4f1f272 — DONE ✓ (commit 64fc70c). fmt clean, 16/16 tests PASS.

Touches 5 UI files, all exist on base. scrollbar.rs core + keybind_help/navigator/
release_notes auto-merged clean. Only sidebar.rs collided (1 import hunk): upstream
added a `use self::tokens::{...}` import while the fork added `SCROLLBAR_THUMB` to the
scrollbar import — both compatible, kept both. Verified SCROLLBAR_THUMB is defined
(scrollbar.rs:15 = "█") and used at 2 sites in merged sidebar.rs (no unused import).
NOTE: this sidebar.rs is UPSTREAM's sidebar; the fork's sidebar-rail redevelopment
is a later plan step and is independent of this glyph change.

## CURRENT HEAD (4-commit replay on upstream ef85fa0):
5e907bc allow_nested / eab0ab4 env-scrub / 4f09ba8 render_ansi / 64fc70c scrollbar

## host scripts — DEFERRED to end (user decision 2026-07-22)

Decision: skip the host-script cluster for now, keep using /tmp/test-rebase.sh,
and land host scripts LAST (right before the version-anchor step 7). Continue
with feature clusters first.

Host-script chain (chronological, for when we return to it). NOTE the state
file's original list MISSED the creating commit c646671 and the fix 9332ab1:
  c646671 (CREATE rebuild-host.sh) → dba63a8 → 1957eb0 → 8c03a84 → a73d5f1 →
  84e6644 → 50790a9 → df5a2ce (CREATE test-host.sh) → add0f35 → e7ca9a9 →
  ea70d91 → 2ef6486 → 62de5f0 → 9332ab1
When replaying: build rebuild-host.sh/test-host.sh WITHOUT --features web (drop
the web coupling from e7ca9a9 / 2ef6486); OMIT swap-restart.sh (62de5f0/9332ab1,
web-safe swap helper). This matches what /tmp/test-rebase.sh already proves works.
These are personal, non-shipped scripts — not upstream CI.

### NEXT: modal keybinds — RECON DONE, needs its own planned redevelopment.

RECON FINDINGS (do NOT attempt a mechanical cherry-pick):
- Cluster = 78cf5c1 (mode-structured keybind schema, 1554-line keybinds.rs
  rewrite + 526-line model.rs) / faa2578 (modal dispatch + Locked mode, new
  541-line dispatch_baseline.rs) / d6b4afa (per-mode resolvers, new 823-line
  modal.rs) / 0721d51 (sticky executor lifecycle) / aafbd0f (headless mode-entry
  intercept, new terminal.rs).
- BOTH SIDES diverged heavily on keybinds.rs since merge-base 4cf9f8e:
  base 1814 lines → upstream/master 2264 → fork-parent 2032. 58 upstream commits
  touched src/app/input/ since the base.
- Trial cherry-pick of just commit 1 (78cf5c1): 7 conflicted files
  (keybinds.rs, model.rs, io.rs, main.rs, navigate.rs, app/mod.rs, headless.rs),
  effectively no clean hunks. Aborted; worktree reset to 64fc70c (clean).
- KEY COLLISION: upstream ALREADY HAS src/app/input/modal.rs (2094 lines) — but
  it is a DIFFERENT subsystem. Upstream's modal.rs = dialog/overlay handling
  (ModalAction/ModalKeyBinding Enter/Esc/CtrlC, global menu, navigator, rename,
  confirm-close). The fork's d6b4afa modal.rs = zellij-style STICKY KEYBIND MODES
  (pane/tab/resize/move/session resolvers). The fork ALSO has the dialog code in
  its modal.rs — the two files share the dialog half almost verbatim (both
  extracted it from a common ancestor) and DIVERGE on the fork's added sticky-mode
  resolvers. upstream/master has NO dispatch_baseline.rs and NO sticky-mode system.
- CONCLUSION: this is a ground-up REDEVELOPMENT of the fork's modal keybind system
  on top of upstream's current (flat-keybind + dialog-modal) architecture, NOT a
  replay. It is the single biggest + highest-risk piece of the whole merge and is
  refactor-risk per CLAUDE.md (core input surface, keybind schema, dispatch,
  headless intercept). It deserves its own /design or /develop pass with
  characterization tests BEFORE editing — not inline autopilot.

## MODAL KEYBIND PROBE (2026-07-22) — "isolate as new file" strategy MEASURED

User idea: rename/isolate our modal code so it lands as a conflict-free new file
instead of fighting upstream's modal.rs. Prototyped it on throwaway branch
proto/sticky-modes-probe (deleted; worktree clean at 64fc70c).

WHAT I DID: carved the 5 pure per-mode resolvers (pane/tab/resize/move/session,
~282 lines from d6b4afa's modal.rs) into a NEW file src/app/input/sticky_modes.rs
+ inlined their scaffolding (ModeAction, 5 *ModeBindings structs, ModeBinding,
mode_binding_matches, ModeEntryKeys). Added minimal glue: 5 Mode variants, 6
Keybinds fields w/ Default, 8 stub NavigateAction variants, 1 module decl.
Saved the probe file at .agents/scratchpad/2026-07-12-upstream-merge-strategy/
sticky_modes-probe.rs.

RESULT — the resolvers are cleanly isolatable. Dependency audit vs upstream:
- 33/41 NavigateAction variants the resolvers need ALREADY exist on upstream.
- terminal_key_matches_combo + KeyCombo + NavDirection all exist, exact shapes.
- The resolver logic itself needed ZERO changes to reference upstream types.
Compile probe surfaced 17 errors, ALL trivial/mechanical, NOT architectural:
- crate::config::KeyCombo → is crate::config::keybinds::KeyCombo (import path).
- `module input is private` ×6 → confirms the *ModeBindings scaffolding structs
  belong in src/config/keybinds.rs (where the real port puts them), not in input.
- NavDirection needs a PartialEq derive (resolvers compare it) — 1-line.
- 8 stub NavigateActions need match arms in navigate.rs (they belong to OTHER
  clusters: stacked panes, resize magnitude, split-auto, break-pane, floating).

THE REAL GLUE SURFACE for the Mode enum = exactly 3 match sites:
  src/app/input/mod.rs:93 (key dispatch router)
  src/app/mod.rs:1670 (secondary dispatch)
  src/ui.rs:427 (overlay renderer)
Each needs arms for the 5 new modes — and THAT IS the feature (wire dispatch to
call the resolvers + render a mode indicator), not incidental glue. Small + local.

VERDICT: the "big isolated new file + thin glue" shape WORKS. ~80% of the
modal-keybind logic (resolvers) lands conflict-free in sticky_modes.rs. The
remaining work is: (a) port the keybind SCHEMA (78cf5c1, ~1229 lines in
keybinds.rs — parsing/validation/defaults for the per-mode tables) into
keybinds.rs, (b) add 5 Mode variants + arms at the 3 dispatch/render sites,
(c) a sticky executor that maps ModeAction→effects + mode lifecycle (0721d51),
(d) headless mode-entry intercept (aafbd0f). (a) and (c) are the bulk; both are
mostly additive, not conflict-prone. This is a tractable REDEVELOPMENT, best run
as its own /design→/develop pass with characterization tests. NOT a cherry-pick.

## DEPENDENCY DIRECTION (2026-07-22) — plan's "modal = foundation" is WRONG

Verified by code, not the plan note. The downstream clusters touch the same big
files (keybinds.rs, state.rs, input/mod.rs) but have ZERO dependency on the
sticky-mode machinery:
- Stacked panes 2bd3408 adds stack_pane/unstack_pane as FLAT ActionKeybinds
  (upstream's existing schema). 0 refs to Mode::Pane/mode_pane/ModeAction/sticky.
- Tab context menu 4514046 touches upstream's modal.rs (DIALOGS, for a context
  menu) — not the sticky resolvers. 0 sticky refs.
- Zellij tab clusters (bca5431/121ee40/15c8168/9cd7d0a): pure UI/geometry, no modal.

The REAL dependency runs the OTHER way: the sticky-mode resolvers call actions
introduced by other clusters (that's why the probe stubbed 8 NavigateActions):
  SplitAuto/ResizeGrow/ResizeShrink <- alt-shortcuts 70143e3
  StackPane <- stacked panes 2bd3408
  ToggleFloating <- floating f059619 (NOTE: floating is a DROP-by-omission cluster!)
  BreakPaneToTab <- break-pane 8b0200b
  ResizeIncrease/Decrease <- native to modal d6b4afa
CONCLUSION: modal keybinds should go LAST (after stacked panes, alt-shortcuts,
break-pane land their actions), so its resolvers reference real actions instead
of stubs. Doing it last also means fewer stubs to carry + it re-sorts naturally
last on any future re-run. Floating is dropped, so the modal resolver's
ToggleFloating branch must be dropped/guarded during the port.

## STACKED PANES cluster — IN FLIGHT (2026-07-22)

Cluster 4654b36/2bc7714/0845f6a/2bd3408/b065510. Landed so far on upstream-rebase:
- 516d81c (4654b36: Node::Stack + geometry) — clean auto-merge.
- b6fdcc1 (2bc7714: stack tree ops + invariant tests) — resolved workspace.rs:
  DROPPED floating tests/helpers (floating is omitted cluster; symbols don't exist
  on upstream), KEPT stack tests. layout.rs auto-resolved by rerere, verified:
  correctly merges upstream's Borders field (pane-gaps 4421c0f) + set_ratio_at->bool
  (runtime-authority 1a4e94e) WITH the fork's Stack additions.
- a20d948 (0845f6a: persistence restore) — clean auto-merge.
- 2bd3408 (wire rendering/resize/input/keybinds) — CHERRY-PICK IN PROGRESS,
  NOT yet committed. Resolved so far (all context-drag: kept ONLY stack_pane/
  unstack_pane, dropped break_pane_to_tab/split_auto/move_tab/resize_grow/floating
  which belong to other/omitted clusters):
    * actions.rs: added stack_focused_pane/unstack_focused_pane before the now
      #[cfg(test)] cycle_pane. DONE.
    * config/keybinds.rs: construction site — kept upstream fields + stack_pane/
      unstack_pane empty_action!(). DONE (struct fields added by non-conflict hunk).
    * config/model.rs: kept upstream close_pane/zoom one(...) + stack/unstack
      defaults (prefix+shift+s / prefix+shift+u). DONE.
    * ui/keybind_help.rs: kept only stack/unstack help entries (dropped split_auto).
    * navigate.rs (3 hunks): added StackPane/UnstackPane enum variants, binding
      map entries, and dispatch arms; dropped SplitAuto/MoveTab/Resize context. DONE.
    * ui/panes.rs (4 hunks): PARTIALLY RESOLVED — see below, the hard part.

### RESOLVED — full cluster landed. Validation running (b51e3lhaf tests, fmt PASS).

Commits: 516d81c / b6fdcc1 / a20d948 / 48ae101 / 4468799.
- ui/panes.rs 4-hunk merge DONE (user: one try, lean upstream). Rewrote
  `pane_inner_for(info, area, multi_pane)`: collapsed->info.rect; else multi_pane->
  `pane_inner_rect(info.rect, info.borders)` (upstream border-aware, honors pane-gaps);
  else area. Both PTY-resize + render loops iterate `apply_pane_chrome(...)` + call
  pane_inner_for. Render loop keeps upstream's `pane_infos` param iterator + grafts
  the fork's collapsed-member early-continue (render_collapsed_stack_member).
- b065510: dropped floating helpers in mouse.rs (upstream deleted them; floating
  omitted). CHANGELOG: kept upstream list, added stack line under ## Unreleased.
IF tests fail on the merge and not quick to fix: per user, OK to
`git reset --hard 64fc70c` and drop stacked panes for a later rebuild.
fmt --check: PASS (exit 0).

<details><summary>original OUTSTANDING notes (kept for history)</summary>

### ui/panes.rs 4-hunk SEMANTIC merge (the tricky part)

This is a genuine collision: upstream's pane-gaps/borders refactor (4421c0f) vs the
fork's stack-aware rendering. Must MERGE BOTH, not pick one:
- Hunk1 (55-129): upstream added apply_pane_chrome + pane_to_right/pane_below/
  shrink_for_one_cell_gap; fork added pane_inner_for (collapsed-stack inner bypass).
  Both are new fns → KEEP BOTH. But pane_inner_for hardcodes Block::borders(ALL);
  it must be adapted to honor per-pane info.borders (see below).
- Hunk2 (218-231): PTY-resize loop. upstream: `for info in apply_pane_chrome(...)`
  + `pane_inner_rect(info.rect, info.borders)`. fork: `for info in panes(area)` +
  `pane_inner_for(&info, area, multi_pane)`. MERGE: iterate apply_pane_chrome, inner =
  collapsed? info.rect : pane_inner_rect(info.rect, info.borders).
- Hunk3 (303-324): render loop, same shape. Merge same way.
- Hunk4 (369-383): render loop iterator + collapsed-member early-branch
  (render_collapsed_stack_member). Keep upstream's `for info in &app.view.pane_infos`
  + the fork's collapsed early-continue block.

PLAN: rewrite pane_inner_for to `pane_inner_for(info, area, multi_pane) -> Rect`:
  if collapsed → info.rect; else if multi_pane → pane_inner_rect(info.rect,
  info.borders); else area. Then both loops call it uniformly. This preserves
  upstream pane-gaps (border-aware inner) AND fork collapsed bypass. Needs a
  container compile + stacked_panes_live.rs test run to confirm.
Worktree currently mid-cherry-pick (2bd3408). To resume: finish ui/panes.rs,
`git add`, `git cherry-pick --continue`, then cherry-pick b065510 (public API,
+tests/stacked_panes_live.rs), then fmt + container test the cluster.
</details>

## STACKED PANES — ATTEMPTED, then DROPPED (2026-07-22, user pre-authorized).

Got 58/60 tests green. Dropped for a clean rebuild later (reset --hard 64fc70c).
WHY dropped: upstream rewrote pane-BORDER rendering into a unified line-grid
(`render_pane_borders`, from pane-gaps 4421c0f) that has no concept of stacks.
The fork drew borders per-pane inline. Two render tests failed:
- stacked_panes_geometry_collapsed_rows_expanded_remainder: expanded member inner
  height 20 vs 19 — apply_pane_chrome removed the expanded member's bottom border
  (saw the collapsed member below as a gap-neighbor). FIXED via pane_inner_for
  forcing Borders::ALL for expanded stack members.
- stacked_panes_render_collapsed_members_as_single_title_rows: buffer[(0,0)] drew
  `─` not the `│` stack lead glyph. NOT fixed — render_pane_borders draws the
  wrong glyphs for stack members; teaching upstream's line-grid renderer about
  stacks is open-ended surgery (each render fix uncovered the next divergence).
Per user ("one try, lean upstream, OK to drop, rebuild later"), stopped there.

WHAT WORKED (for the eventual rebuild — this recon is reusable):
- 4 of 5 commits auto-merged or resolved cleanly (Node::Stack geometry, tree ops,
  persistence, public API). Only the render/keybind WIRING commit (2bd3408) fought.
- Context-drag discipline: kept ONLY stack_pane/unstack_pane; dropped
  break_pane_to_tab/split_auto/move_tab/resize_grow/floating (other/omitted clusters).
- Wiring tail that MUST be redone on rebuild: (a) both NavigateAction exhaustive
  matches (navigate.rs execute_tui_navigate_action + execute_navigate_action_in_context)
  need StackPane/UnstackPane arms; (b) apply_action! lines for stack_pane/unstack_pane
  in the keybind resolver (~line 661) — without them keybinds.stack_pane.bindings is
  empty; (c) all layout::PaneInfo literals need BOTH `borders` (upstream) + `stack`
  (fork) fields; (d) render_collapsed_stack_member uses truncate_end not truncate_label;
  (e) render_panes test calls need the 5-arg signature (pane_infos, split_borders).
- THE HARD PART for rebuild: make render_pane_borders stack-aware (skip collapsed
  members — they draw via render_collapsed_stack_member; force full box on expanded).
  This is the crux the "lean upstream" pane_inner_for shortcut could not cover.
- fold-on-split (fold_new_pane_into_focused_stack + fold_into_stack +
  MIN_STACK_EXPANDED_ROWS) went dead-code under clippy -D warnings because its only
  caller (AppState::split_pane fold branch) is reachable only via the fork baseline
  dispatch that isn't replayed. Needs #[allow(dead_code)] until modal/dispatch lands,
  or wire it to upstream's split path.
Recommendation: rebuild stacked panes AFTER modal keybinds, in the same
render-system-aware design pass. It is not a cherry-pick.

## STATUS. 4 clusters landed + validated on upstream ef85fa0:
  5e907bc allow_nested · eab0ab4 env-scrub · 4f09ba8 render_ansi · 64fc70c scrollbar
  (worktree reset to 64fc70c; stacked panes fully removed, no residue)
  (allow_nested/env-scrub/render_ansi/scrollbar) — all fmt-clean + tests green.
Host scripts deferred to end. Modal keybinds is the next major effort and needs
a plan. Downstream clusters (zellij tabs, sidebar rail, hint bar, stacked panes,
alt shortcuts, break_pane_to_tab, responsive width, drag-clearing) DEPEND on the
modal foundation per the ordering, so they wait behind it.

## IMPORTANT: test runner note
`scripts/test-host.sh` is itself a not-yet-cherry-picked fork script AND hardcodes
`--features web` (no such feature on upstream base). Use the temp adapted runner
`/tmp/test-rebase.sh` (REPO=worktree, no --features web) until the host-scripts
cherry-picks land in step 1. Delete it once real test-host.sh is cherry-picked.

## step-1 remaining cherry-picks (after allow_nested lands)

- 0019e4d env scrub (src/env.rs). Verify upstream 9c45342 (preserve explicit session
  sockets during handoff) survives the scrub; scrub must cover all spawn sites.
- c77066d render_ansi (CursorTracker + SGR cache) — REDEVELOP-lite: apply fork cache
  on top of upstream's current render_ansi, absorbing 2b99ced (cursor-hide ordering),
  b7015f1 (undercurl SGR key), 02a6e87 (batch diff writes). Undercurl must be in the
  SGR cache key or styles leak.
- 4f1f272 scrollbar glyphs.
- host scripts (drop web-specific ones): df5a2ce test-host.sh, 50790a9, 1957eb0,
  dba63a8, 8c03a84, a73d5f1, 84e6644, add0f35, ea70d91 rebuild-host.sh fixes.
  OMIT web-specific: 62de5f0 swap-restart, 2ef6486 web-tests-default, e7ca9a9 --features web.

## DROP by omission (never cherry-pick these)

- Web (13): b741b12, a8f8f7f, cc7355e, f63f97b, 493c838, 11df0f4, 772b77f, 3950abc,
  9533b40, f1e3d62, 62de5f0, 2ef6486, e7ca9a9.
- Old floating (6): ad9d5bc, 8b162e3, f059619, f966f5b, d4d4174, ca9b685.
- Duplicate (1): 98c810a (= upstream 5449025).
- Tunnel script part of 48ea1f8 (keep only the allow_nested-adjacent docs if any;
  the flip itself is 3e93d5f).

## REDEVELOP clusters (see cluster-replay-plan.md for commit lists)

Modal keybinds (78cf5c1/faa2578/d6b4afa/0721d51/aafbd0f) — biggest; zellij tabs;
sidebar rail; hint bar; stacked panes (4654b36/2bc7714/0845f6a/2bd3408/b065510);
alt shortcuts (70143e3/8e6d48b/ba7e23c); break_pane_to_tab (8b0200b); responsive
width (526d31e); drag-clearing (2d1a0ab). Rebuild each on upstream's current
keybinds.rs/model.rs, tab_surface, sidebar, layout.rs (Borders) — do NOT port fork plumbing.
