## Run 2026-07-12 2026-07-12-upstream-merge-strategy
## Round 1 — delta=code/structural
reviewer=aspect-reviewer outcome=fired
reviewer=devils-advocate outcome=fired
reviewer=adversarial-validator outcome=fired
reviewer=failure-mode-adversary outcome=fired
dispositions=18 total: 15 ACCEPT, 3 ACKNOWLEDGE (11 supply-chain, 12 keybind-base, 16 rebase-onto-new-branch), 0 REJECT
note=validator DISPUTED plugin-framing resolved by git check (scaffolding is pre-merge-base shared history; only marketplace is upstream-new) — corrected in Overview
note=findings 6,7 accepted as factual corrections (web-client exit path; mandatory protocol bump)
note=USER SCOPE DECISIONS mid-loop (before round 2): web client DROP (archive branch), sidebar partially backtracked already (a1804a8 on main), keep floating+stacked, upstream-push = tab pack + panes RFC + small fixes, modal keybinds NOT PR'd. Design doc revised accordingly (code/structural delta) — round 2 re-runs all four reviewers against the revised doc.
## Round 2 — delta=code/structural
reviewer=aspect-reviewer outcome=fired
reviewer=devils-advocate outcome=fired
reviewer=adversarial-validator outcome=fired
reviewer=failure-mode-adversary outcome=fired
note=all four spawned with cumulative exclusion clause (5 acknowledged/resolved items enumerated); POC status at spawn: build clean, 3204/3217 tests, 13 failures being fixed/parked
dispositions=15 consolidated: 12 ACCEPT (R2-1 alert direction, R2-2 parked-test ratchet, R2-3 remote/unix.rs factual fix, R2-4 protocol determinate 16->17, R2-5 rerere staleness on a1804a8 files, R2-6 post-drop cargo check gate, R2-7 ungated-method equivalence tests, R2-8 research/04 tested-pending honesty, R2-9 trust-boundary rerere review set + scrub spawn-site audit, R2-10 stale-web/smoke/diff3/tag/backup batch, R2-11 modal-keybind permanent labeling, R2-12 hot-file tax + wave-branch step), 2 ACKNOWLEDGE (R2-13 full dedup, R2-14 archive revival cost), 1 REJECT (R2-15 container 1.96.1 — empirically confirmed in POC logs)
note=max_rounds=2 reached — loop complete; remaining ACCEPT edits applied in this final revision pass, no round 3
