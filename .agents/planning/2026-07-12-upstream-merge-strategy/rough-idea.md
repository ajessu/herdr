# Rough Idea

We are going to test merging or rebasing main with upstream. Evaluate and test how that
looks like on this branch (`merge`, currently at fork `main` = 3e93d5f).

Evaluate:

- the strategy (merge vs rebase),
- whether there are any main-branch fork features that fit into the new upstream
  plugin system,
- areas to simplify.

Keeping up with upstream long term is the goal.

## Context

- Fork: `ajessu/herdr` (`origin`), branch `main`.
- Upstream: `ogulcancelik/herdr` (`upstream`), branch `master`.
- Merge-base: 4cf9f8e (2026-06-15). Fork is 80 commits ahead, 209 commits behind.
- Fork footprint: 96 files, +26.5k/-1.9k vs merge-base.
- Upstream footprint: 358 files, +71k/-11.7k vs merge-base. Includes a new plugin
  system + marketplace, a large "runtime authority" refactor series, Windows support
  work, and two releases (v0.7.2, v0.7.3).
- A test merge on this branch conflicts in 32 files (~94 hunks).
