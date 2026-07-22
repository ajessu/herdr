# Research: fork/upstream overlap, duplicates, and structural collisions

Date: 2026-07-12. Method: `git show upstream/master:<path>`, patch-id comparison,
per-commit stat inspection. Fork = `main` (3e93d5f), upstream = `upstream/master`
(3661d99), merge-base 4cf9f8e.

## Exact and near duplicates

1. **Agent panel priority sort — confirmed cherry-pick.** Fork 98c810a is a
   cherry-pick of upstream 5449025 (same author, subject, "refs #318", identical
   21-file +247/-234 stat; patch-ids differ only from context drift around
   fork-only `show_tab_status`/`hint_bar` fields). Merges nearly clean; the shared
   -22 in `src/persist/snapshot.rs` exists on both sides.
2. **Context-menu-after-close.** Fork 4514046 overlaps upstream 974a481 for the
   close-tab case — reconcile at merge.
3. **Env scrubber.** Fork's shared `src/env.rs::scrub_herdr_runtime_env`
   duplicates upstream's private copy in `src/cli/plugin.rs:1438`. Fork version is
   the better factoring (upstream's remote client still leaks `HERDR_ENV=1`);
   unify on the fork module at merge and check upstream 9c45342 (preserve explicit
   session sockets during handoff) is not broken by the scrub.

## Overlapping-but-different (manual reconciliation required)

4. **Keybinds.** Fork replaced the flat schema with a modal `[keys.*]` schema
   (78cf5c1 +2021/-831). Upstream kept patching the flat schema: 088922d
   (user-displaces-default `BindingSource` machinery), b708f85 (shifted indexed
   keys), 32e3d7b (help ranges). None of those semantics exist in the fork's modal
   schema. `src/config/keybinds.rs` conflicts wholesale; the fork must
   re-implement upstream's three behaviors inside the modal schema.
   `src/ui/keybind_help.rs` conflicts (fork 70143e3 vs upstream 32e3d7b).
5. **Tab bar.** Fork rewrote `src/ui/tabs.rs` (9cd7d0a net -1692 to a stateless
   zellij painter). Upstream bc764c8 and the tabs.rs part of b44ca3b must be
   re-applied by hand onto the fork painter. Upstream's new `src/ui/text.rs`
   (`display_width_u16`, `truncate_end`) is the canonical width helper going
   forward — fork tab/sidebar code should adopt it.
6. **Collapsed sidebar — direct feature collision.** Fork's 7-col minimized rail
   (a225c95) vs upstream's hidden-collapsed mode + row alignment/numbering
   (f54d8e8, 552aa8c, 0cd0b1a) rework the same geometry and hit-testing in
   `src/ui/sidebar.rs` + `src/app/input/sidebar.rs`. Mutually incompatible
   layouts; pick one per surface at merge.
7. **render_ansi.** Fork c77066d (CursorTracker + SGR cache, signature change)
   vs upstream 2b99ced (cursor-hide inside sync output ordering), d1471e6
   (windows host cursor), b7015f1 (undercurl SGR). Same functions, incompatible
   line edits. The fork's SGR cache must learn undercurl or styles leak; the
   ?25l/?2026h ordering fix must be ported into the cached path.
8. **remote/unix.rs.** Upstream 26db26e rewrote ~1041 lines around the fork's
   `build_client_command` edits — hard conflicts.
9. **Web vs mobile switcher — NOT duplicates.** Upstream's mobile switcher
   (db1ef28, 14d8e93) is a narrow-width TUI mode, not a web client. But upstream's
   observe/control session streams (bffc4a8, 0fa6440) are the sanctioned bridge
   primitives; the fork's bespoke `src/web/bridge.rs` piggybacking on
   `src/server/headless.rs` should eventually be rebuilt on them (upstream
   rewrote headless.rs in 0bab015/04682e5 — conflicts guaranteed).

## Structural collision: runtime-authority refactor

Upstream series (1a4e94e, 97f7822, 9a91a2a, 0bab015, 89fe897, edd08e8, c97e098,
ce6c0dc, 1c606d4, 04682e5) added `src/app/runtime_mutations.rs` (typed `runtime_*`
adapters dispatching `Method::*` through `dispatch_api_request`) and rewrote
`src/app/input/{navigate,modal,mouse}.rs`, `src/app/mod.rs`, `src/app/actions.rs`,
`src/server/headless.rs`, `src/cli/*`. New guardrail in upstream AGENTS.md:
"Do not add new shared behavior that only works through the private TUI client
socket."

Fork code that mutates AppState/Workspace directly (the pattern the refactor
eliminated) and must be re-targeted at `runtime_*` adapters post-merge:

- Floating panes: focus routing/actions in actions.rs, input/mouse.rs.
- Modal executor / ModeAction (faa2578, d6b4afa, 0721d51): input/mod.rs and
  input/modal.rs were both rewritten upstream.
- Alt shortcuts (70143e3): MoveTabLeft/Right, ResizeGrow/Shrink must become
  runtime API calls.
- Tab mutations (ad46cf0, 4514046): tab create/move/rename/close now route
  through `Method::Tab*` adapters. ad46cf0's manual "refresh after mutation" is
  likely obsolete — upstream 15cab96 emits layout-update events.

Guardrail-compliant fork work (pure TUI presentation, low structural risk):
hint bar, overflow badges, tab painting, sidebar rail visuals.

At-risk under the guardrail: floating/stacked pane placement + persistence
(snapshot is now API-visible via upstream 9150ed6 session snapshot API), web
bridge session semantics.

Upstream 12cce7d split `src/integration/mod.rs` into ~10 files; fork made no
integration edits → merges clean.

## Protocol / persistence

- `PROTOCOL_VERSION`: fork still 14 (never bumped — pre-existing gap given web
  bridge messages); upstream is 16 (8b74fb8, 2bc1724). No divergent bump; merged
  result takes 16, then bump to 17 if fork web/stack surface is wire-visible.
- Integration versions: fork made no changes; upstream's split
  `src/integration/version.rs` + bumped asset versions win cleanly.
- `src/persist/snapshot.rs`: fork +189/-28 (stack/floating/sidebar-ratio);
  upstream's only change is the shared -22. Low textual conflict, but fork's
  persisted fields become API-visible through upstream's snapshot API → schema
  regen required (`herdr-api.schema.json`, 25aeaa4).
- `src/api/schema/*`: both sides added surface (fork: web server config, stack
  layout; upstream: mutation methods, events, pane scroll state, generated
  schema). Conflicts in server.rs, response.rs, tests.rs; mandatory schema regen
  after merge.

## Merge-risk file ranking

Guaranteed heavy conflicts: src/config/keybinds.rs, src/config/model.rs,
src/app/input/*, src/app/mod.rs, src/app/actions.rs, src/app/state.rs,
src/ui/tabs.rs, src/ui/sidebar.rs, src/ui.rs, src/server/headless.rs,
src/remote/unix.rs, src/protocol/render_ansi.rs.

Drop-at-merge: fork 98c810a (cherry-pick). Reconcile: 4514046 vs 974a481, env
scrubbers. Re-architect post-merge: modal executor, alt shortcuts, tab mutations
→ runtime adapters; ad46cf0 refresh → layout events; web bridge → evaluate
observe/control streams.
