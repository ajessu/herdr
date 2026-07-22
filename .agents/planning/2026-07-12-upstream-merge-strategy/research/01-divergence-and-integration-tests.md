# Research: divergence facts and empirical merge/rebase tests

Date: 2026-07-12. All measurements taken on branch `merge` (= fork `main`, 3e93d5f)
with `upstream/master` fetched at 3661d99.

## Divergence

- Merge-base: `4cf9f8e` (2026-06-15, "docs: update preview manifest").
- Fork ahead: **80 commits** (`upstream/master..main`).
- Fork behind: **209 commits** (`main..upstream/master`).
- Fork diff footprint: 96 files, +26,537 / -1,864.
- Upstream diff footprint: 358 files, +71,092 / -11,722. Includes releases v0.7.1,
  v0.7.2, v0.7.3.
- Files modified on BOTH sides: 60 (see conflict list below — 32 actually conflict).

### Fork commit clusters (80 commits)

| Area | Commits (approx) | Notable |
|---|---|---|
| Web client/server (`--features web`) | ~14 | b741b12, a8f8f7f, cc7355e, 9533b40 (trust-proxy), 493c838, 11df0f4, f1e3d62 (touch scroll) |
| Modal keybind system | ~7 | 78cf5c1 (schema), faa2578 (dispatch), d6b4afa, 0721d51, 90ddd64/68876cd (hint bar) |
| Tab bar (zellij-faithful) | ~15 | 70d5bf1 (TabChrome), bf1b0bc (status dots), 9cd7d0a (batching), 121ee40 |
| Sidebar | ~6 | a225c95 (rail), 53c44dc (badges), 526d31e (ratio), 307942b, 98c810a (priority sort) |
| Floating panes | ~6 | ad9d5bc..ca9b685 |
| Stacked panes | ~5 | 4654b36..b065510 |
| Personal scripts | ~12 | rebuild-host.sh, test-host.sh, swap-restart.sh, tunnel |
| Misc | ~8 | c77066d (ANSI encoder), 0019e4d (env scrub), 3e93d5f (allow_nested), 70143e3 (alt+key) |

### Upstream commit clusters (209 commits)

| Area | Notable commits |
|---|---|
| **Plugin system + marketplace** | 2eeea9a (marketplace), bf75226 (blocklist), d74ba8c (lifecycle events), e1bbc25; new `src/app/api/plugins/`, `src/cli/plugin.rs`, `src/persist/plugin_registry.rs`, marketplace worker |
| **Runtime-authority refactor series** | 1a4e94e, 97f7822, 9a91a2a, 0bab015, 89fe897, edd08e8, c97e098, ce6c0dc, 1c606d4, 04682e5 — routes TUI/CLI/headless mutations through typed runtime adapters. New CLAUDE.md "Runtime/client boundary guardrail". |
| **Session/API surface** | 9150ed6 (session snapshot API), 5d9212a (snapshot via CLI), bffc4a8/0fa6440 (terminal session observe/control streams), 25aeaa4 (socket protocol schema), 6703413 (api schema CLI) |
| Windows support | ~30 commits (conpty, named pipes, clipboard, installer) |
| Mobile switcher | db1ef28 (agents-first mobile switcher), 14d8e93 (worktrees tree) |
| UI | f54d8e8 (hidden collapsed sidebar), 2a1a8d6 (hide single-tab row), 4421c0f (pane gaps), 36b4001 (host theme sync), 4617456 (copy-on-select) |
| Agents | 3b8aeee (maki), d0e3334 (mastracode), many detection fixes |
| Overlap with fork | 5449025 "sort agent panel by priority" (same feature as fork 98c810a) |
| Toolchain | b137c7b pins rust 1.96.1 |

## Empirical test 1: single merge

`git merge --no-commit --no-ff upstream/master` on this branch:

- **32 conflicted files, ~94 conflict hunks** (aborted after measurement).
- Worst files: `src/app/mod.rs` (13 hunks), `src/config/model.rs` (9),
  `src/app/actions.rs` (7), `src/app/input/mod.rs` (5), `src/ui/sidebar.rs` (5).
- Conflicts concentrate exactly where fork features meet the upstream
  runtime-authority refactor: `src/app/`, `src/config/`, `src/ui/`.
- `Cargo.lock` conflict is regenerable; docs conflicts are trivial.

Full conflict list: Cargo.lock, Cargo.toml, docs/next/CHANGELOG.md,
docs/next/.../configuration.mdx, docs/next/.../socket-api.mdx, src/app/actions.rs,
src/app/api/{layouts,panes,tabs}.rs, src/app/creation.rs,
src/app/input/{mod,modal,mouse,sidebar,terminal}.rs, src/app/mod.rs,
src/app/state.rs, src/config.rs, src/config/{io,keybinds,model}.rs, src/layout.rs,
src/main.rs, src/persist/snapshot.rs, src/server/headless.rs, src/ui.rs,
src/ui/{panes,sidebar,tabs}.rs, src/workspace.rs, src/workspace/tab.rs,
tests/cross_area.rs.

## Empirical test 2: full rebase replay (isolated worktree)

`git -c rerere.enabled=false rebase upstream/master` from detached fork main;
conflicts mechanically resolved fork-side to keep the replay moving (throwaway).

- **80/80 commits replayed; 16 stops (20%) with conflicts.**
- Cumulative: 31 conflicted files, **42 hunks** total across all stops.
- Median stop: 1 file, 1–2 hunks. Max stop: 5 files (FloatingLayer integration).
- Stops by area: tab bar 5, keybind modes 3, stacked panes 3, floating panes 2,
  sidebar 2, web 1.
- No repeated tarpit file (each file conflicts ≤2 times), no delete/modify
  conflicts, no commits became empty.
- Caveat: fork-side mechanical resolution slightly undercounts; realistic range
  ~16–20 stops.

## Reading

- The merge concentrates all pain into one sitting: 94 mixed-context hunks.
- The rebase spreads it into 16 small, single-commit-context stops (42 hunks) —
  individually easier, but rewrites history.
- **History-rewrite cost is high here:** fork `main` is the base of ~25 local
  branches/worktrees (waves) and is pushed to `origin/main`. A rebase +
  force-push orphans all of them.
- Textual conflicts are not the real risk. Upstream's runtime-authority refactor
  moved the mutation paths the fork's tab bar, keybind modes, and sidebar code
  hook into; the expensive part is post-integration semantic repair + compile +
  `just test`, identical under both strategies.

## Toolchain / build constraints

- Upstream pins rust 1.96.1 (ci: b137c7b, `rust-toolchain.toml`); container image
  used by `scripts/test-host.sh` must provide it.
- Host glibc 2.26 → all builds/tests go through `scripts/test-host.sh`.
- User frequently connected over the fork-only web client; the integration must
  not require stopping the running server (rebuild + swap-restart.sh only, ask
  before restart).
