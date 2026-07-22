# Idea Honing — upstream merge/rebase strategy

Mode: auto. All answers below are assumptions unless marked otherwise.

## Q1. Goals & success criteria — what does "done" look like?

**A (assumption):** Fork `main` contains all of upstream/master (through 3661d99),
compiles, and passes `just test` in the container (scripts/test-host.sh). All fork
features that are kept still work (web client, modal keybinds, tab bar, sidebar,
floating/stacked panes). A written, repeatable process exists for future syncs so
the fork never again drifts 200+ commits. Success metric: next upstream sync takes
under a day, and the standing fork diff shrinks rather than grows.

## Q2. Functional requirements — what must the strategy cover?

**A (assumption):**
1. A one-time catch-up integration of the 209 upstream commits.
2. A per-feature disposition for each of the ~8 fork feature areas: keep-as-patch,
   convert-to-plugin, PR-upstream, or drop (upstream equivalent exists).
3. A recurring sync cadence and mechanics (merge vs rebase, rerere, branch naming).
4. Handling of upstream's new architecture guardrail (runtime-authority routing)
   so kept fork features don't violate it and future syncs don't re-conflict.

## Q3. Non-functional requirements?

**A (assumption):** Web access constraint: the user is often connected via the
fork-only web client; the integration must not require dropping web access on the
running host (rebuild only, swap via swap-restart.sh, ask before any restart —
see herdr-web-access-do-not-restart memory). Build must go through
scripts/test-host.sh (host glibc too old for direct cargo). History on `origin/main`
is shared across ~25 local worktrees/branches; a strategy that rewrites `main`
history (rebase + force-push) invalidates all of them, so worktree impact must be
weighed.

## Q4. Scope (out) / non-goals?

**A (assumption):** Out of scope: actually landing PRs upstream (that is follow-up
work per the external-contributor guardrail: discussion → accepted issue →
approved PR); implementing plugin conversions of fork features (follow-up);
Windows validation; translating fork docs to ja/zh-cn. This design covers the
strategy, the tested integration approach, and the disposition table — not each
feature's re-implementation.

## Q5. Acceptance criteria?

**A (assumption):**
- Empirical merge and rebase test data recorded (conflict counts, worst areas).
- A chosen strategy with rationale (merge vs rebase, one-time vs recurring).
- A disposition table for every fork feature area with evidence.
- A step-ordered integration plan (what to drop first, what to merge, how to verify).
- A documented recurring sync process (cadence, commands, rerere, verification).

## Q6. Risks, assumptions & dependencies?

**A (assumption):**
- Risk: upstream's runtime-authority refactor moved the exact code paths the fork's
  modal-keybind and tab-bar work sits on; semantic conflicts will outnumber textual
  ones. Mitigation: post-merge compile+test is the real gate, not conflict count.
- Risk: rebasing 80 commits replays conflicts per-commit; may be strictly worse
  than one merge. Mitigation: measure both empirically (done in research).
- Risk: force-pushing rewritten history breaks the ~25 local worktrees and any
  remote clones. Mitigation: prefer merge, or if rebase, plan worktree migration.
- Assumption: upstream `master` is the long-term integration target; fork `main`
  remains the fork's default branch.
- Assumption: the user wants to keep the web client even though upstream now has
  a mobile switcher (verify in research whether upstream's is web-based).
- Dependency: container build via scripts/test-host.sh; upstream pinned rust
  toolchain 1.96.1 (b137c7b) may require container image update.

## Research-driven updates (auto mode, PDD 1d)

- Resolved Q6 assumption: upstream's mobile switcher is a narrow-width TUI mode,
  NOT a web client — the fork web client is not superseded. Keeping it is
  justified; long-term it should migrate onto upstream's observe/control session
  streams.
- New fact: fork never bumped PROTOCOL_VERSION (still 14) despite adding web
  bridge messages; upstream is at 16. Post-merge the protocol version needs
  review per the repo convention.
- New fact: fork 98c810a is a literal cherry-pick of upstream 5449025 — one
  guaranteed-duplicate commit, which also biases the strategy toward `merge`
  (merge handles cherry-picks gracefully; rebase replays them as conflicts).

## User decisions (interactive, 2026-07-12 — supersede earlier assumptions)

- **Web client: DROP.** The user's phone access is a standalone web terminal
  product, not the fork's web client ("that is not at all how I am accessing
  this"). Preserve the code on a reference branch, then hard-drop from `main`.
  This invalidates the Q3 web-access NFR and removes `--features web` from all
  gates. The herdr-tunnel script (tied to the fork web server) drops with it.
- **Sidebar: partially backtracked already, on `main`.** Commit a1804a8
  (landed from another wave) reverted the toggle chip and close button to
  upstream defaults. KEPT: overflow badges, scrollbar glyphs, agent status
  dots, 7-col rail, responsive width ratio. The real merge must start from
  a1804a8, not 3e93d5f.
- **Tab management + zellij shortcuts: doubling down.** These are the fork's
  core identity now.
- **Floating + stacked panes: keep both.**
- **Upstream push list:** tab management pack, floating/stacked panes RFC,
  small fixes (env scrub, ANSI perf, recursion check). Modal keybinds are NOT
  pushed upstream — "too much copying zellij for them to accept"; the user
  hopes a plugin mechanism can eventually host modal keybinds, but until
  upstream plugins can own input dispatch this stays a fork patch.

## Ref

None provided.
