# Research: upstream-delta refresh (POC base 3661d99 → current tip bc064e9)

Date: 2026-07-21. Prep for the "rebase and retry with latest of both" re-run,
authored while `sidebar-items` is still in flight on the fork side. This file
does NOT supersede research/03 or 04; it records the **84 new upstream commits**
(`3661d99..bc064e9`, incl. the `v0.7.4` release) that landed *after* the POC's
analysis, so step-3 merges against ground truth instead of the stale POC base.

Measurement commands (re-run at merge time; upstream advances daily):

```
git fetch upstream --prune
git rev-list --count 3661d99..upstream/master        # was 84 on 2026-07-21
git log --pretty=format: --name-only 3661d99..upstream/master | sort | uniq -c | sort -rn
```

## Base/target coordinates at this refresh

- Fork side: `origin/main` = 4d0ac81 (a1804a8 + 6 commits: tab geometry,
  agent `--status` filter, tab/workspace labels). `sidebar-items` pending — the
  re-run waits on it landing on `origin/main` per the user.
- Upstream: `upstream/master` = bc064e9 (POC base was 3661d99).
- The `merge` branch still sits on a1804a8 with the uncommitted step-1 web-drop.
  Nothing from step-1 is committed yet. The re-run rebases the web-drop onto the
  post-sidebar-items `origin/main` first, then merges upstream.

## PLAN-STALE items the refresh corrects (highest priority)

These three change acceptance criteria already written in implementation/plan.md
step-3. Fix the plan text at merge time; do NOT execute the stale numbers.

1. **PROTOCOL_VERSION: plan says "bump 16→17" — now WRONG.**
   - Upstream is already at **17** (was 16 at POC base). Fork `origin/main` = 14.
   - Correct action: bump the fork past upstream → **18**, and update
     `tests/support/mod.rs` `CURRENT_PROTOCOL` to match. The POC's "16→17"
     acceptance line and its `CURRENT_PROTOCOL = 16` note are both obsolete.
   - Re-verify at merge time: upstream may bump again before the re-run.

2. **SNAPSHOT_VERSION collision is now LIVE (design predicted it; here it is).**
   - Fork `origin/main` = **4** (Stack layout). Upstream still = **3**.
   - Upstream's next breaking snapshot bump will mint a *semantically different*
     "version 4". Per the design's defuse: advance the fork's SNAPSHOT_VERSION
     past upstream's counter line (→ at least 5, or upstream+1 whichever higher
     at merge time) and add the snapshot-schema fingerprint test (step-3 / step-5).

3. **NEW collision the POC never saw — DECIDED: DROP fork floating, adopt upstream popup.**
   - Upstream `2c7c8be` ("feat: add floating popup panes", refs #1125, shipped in
     **v0.7.4**) adds a *native* session-modal popup: new `src/app/popup.rs` (309),
     `src/popup_size.rs` (252), plus edits to `src/pane.rs` (+118), `ui/panes.rs`
     (+64), `app/state.rs`, `app/mod.rs` (+187), `input/terminal.rs` (+274),
     `config/keybinds.rs` (+76 — `type = "popup"` custom-command keybind),
     `server/headless.rs` (+131), `app/api/plugins/*`.
   - **Disposition (user-decided 2026-07-21): DROP the fork floating layer
     entirely, take upstream's popup as-is.** Same treatment as the web client.
     No runtime-authority re-target of the fork feature; no keep-fork rationale.
     The capability trade (fork = multi-pane draggable/resizable window layer;
     upstream = single session-modal popup) is accepted and not to be relitigated.
   - **Removal scope (~19 files touch "floating" on `origin/main`):** delete
     `src/workspace/floating.rs` (1001 lines) and its `pub mod floating` in
     `src/workspace.rs`; strip floating from `app/actions.rs`, `app/mod.rs`,
     `app/state.rs`, `app/input/{mod,modal,mouse,navigate,terminal}.rs`,
     `ui.rs`, `ui/panes.rs`, `ui/keybind_help.rs`, `workspace/tab.rs`; retire the
     11 fork floating keybinds (`toggle_floating`, `new_floating_pane`,
     `close_floating_pane`, `move_floating_{left,down,up,right}`,
     `resize_floating_{grow,shrink}`, `cycle_floating_{next,previous}`) + the
     modal `toggle_float` binding from `config/keybinds.rs`, `config/model.rs`,
     `config/io.rs`.
   - **No snapshot migration needed (verified):** the fork floating layer is NOT
     persisted — `persist/restore.rs:706` always rebuilds a fresh
     `FloatingLayer::new()`, and `persist/snapshot.rs` has zero floating fields.
     Upstream's `popup_pane` is likewise non-persisted (`"intentionally outside
     workspace layouts"`). So the drop retires no `SNAPSHOT_VERSION` field.
   - **Execution timing:** treat like the web-client drop — a clean removal pass
     BEFORE the upstream merge shrinks the conflict surface (the fork floating
     edits in `app/input/*`, `state.rs`, `ui/panes.rs` are exactly upstream's
     `2c7c8be` hot files). Do the removal on the rebased `merge` branch, then
     merge upstream so the popup arrives with no counterpart to reconcile.
   - FORK.md: no KEEP entry; record as a DROP disposition (fork floating removed,
     superseded by upstream popup `2c7c8be` / v0.7.4).

## New upstream FEATURES since POC (collision + port triage)

Ranked by new merge surface. "collision" = touches a fork hot file.

- **`d30ab1b` plugin-driven agent views + startup hooks** (28 src files) —
  touches `ui/sidebar.rs`, `app/input/navigate.rs`, plugin API. Collides with
  fork sidebar rail + the pending sidebar-items work. Port/reconcile.
- **`3f80947` + `e0758c3` first-class agent-automation CLI / hardened named
  agent workflows** (28 + 29 src files) — touches `persist/snapshot.rs`,
  `protocol/wire.rs` (drove the 16→17 bump), `cli/*`, `app/state.rs`. Large,
  mostly additive; watch the snapshot + wire surface.
- **`a26a654` navigator tree glyphs / workspace grouping / search auto-select**
  (5 files) — `app/input/modal.rs`, navigator. Reconcile with fork modal dispatch.
- **`e298e45` optional workspace name prompt** (11 files) — `app/input/modal.rs`,
  `mouse.rs`, `navigate.rs`, `config/model.rs`. Touches modal seam.
- **`9c9490d` configure collapsed sidebar startup state**, **`5cfe5e5`
  configurable sidebar metadata tokens**, **`b16465a` sidebar token styles**,
  **`5b91dae` sidebar entry gaps** — all `config/model.rs` + `ui/sidebar.rs`.
  Direct overlap with the fork's sidebar rail rewrite AND the in-flight
  sidebar-items branch. Reconcile after sidebar-items lands.

## New upstream CORRECTNESS/PERF fixes touching fork hot files

Candidate mandatory ports (confirm each still applies post-rebase):

- **`02a6e87` perf: batch contiguous terminal diff writes** — `render_ansi.rs`.
  Stacks on the POC's existing render_ansi ports (2b99ced/b7015f1). The fork's
  CursorTracker+SGR cache path must absorb the batching or lose the perf win.
- **`de94cf8` discover canonical mise remote installs** + **`616d1dd`
  (windows) suppress noninteractive process windows** — `remote/unix.rs`.
  Stack on the POC's 26db26e rebase.
- **`f43a6d4` preserve selection when auto-copy disabled (#1496)**,
  **`4c9a3f3` forward horizontal wheel events (#1402)** — `app/input/mouse.rs`.
  Reconcile with fork floating/mouse routing.
- **`75ed6ab` reap detached custom command children (#1384)** — `ui/sidebar.rs`
  + `input/navigate.rs`. Process-lifecycle correctness; include in the security
  review pass (child-reaping = spawn-boundary).
- **`3a8490f` show renamed single tabs in agents sidebar** — `ui/sidebar.rs`.

## Security-review-pass additions (step-3 focused review)

New incoming diffs touching spawn/socket/plugin-exec boundaries beyond the POC set:
`75ed6ab` (child reaping), `3f80947`/`e0758c3` (agent CLI spawn surface),
`d30ab1b` (plugin startup hooks = plugin-execution boundary),
`1955406` (link plugins without a running server), `69d07db` (global plugin state),
`6382bd4` (reject oversized text pastes — input boundary).

## What did NOT change (still valid from POC)

- Web-drop is conflict-free vs the 6 new origin commits EXCEPT one file:
  `src/api/schema/tests.rs` — origin appended agent-label tests exactly where the
  web-drop removes the old web tests. Trivial both-modified reconcile.
- rerere recordings (32) from the POC (`../herdr-merge-poc`, merge 639aef7) remain
  reachable from this worktree's common dir. They replay the *pre-existing* 32
  conflicts; the NEW upstream commits above will surface conflicts rerere has no
  recording for — expect fresh manual resolution concentrated in the floating-pane,
  sidebar, and agent-CLI surfaces.
- The runtime-authority re-target playbook (research/03) is unchanged and now
  applies to more files (upstream's floating popup + agent-CLI series extend it).

## Bottom line for the re-run

The retry is NOT a replay of the POC. Three plan-stale corrections (protocol 18
not 17, live snapshot collision, defuse now mandatory) plus one major new
collision (upstream native floating panes vs fork floating) plus a second sidebar
rework colliding with the in-flight sidebar-items branch. Wait for sidebar-items
to land, rebase the web-drop onto the new `origin/main`, then merge `upstream/master`
at its then-current tip (re-measure — bc064e9 will itself be stale by merge time).
