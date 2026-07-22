## Develop: step-2
Date: 2026-06-19

### Implementation
- Files changed (docs-only):
  - `docs/next/website/src/content/docs/keyboard.mdx` — new "Alt shortcuts" section (binding table, Alt+x destructive caution, known limitations, "Diagnosing my Alt key does nothing" troubleshooting); updated "Learn these five first" and "The rest, by task" tables.
  - `docs/next/website/src/content/docs/configuration.mdx` — default keymap block updated to array form for the six dual-bound actions + five new Alt-only fields; added config-shape + upgrade-behavior explanation.
  - `docs/next/CHANGELOG.md` — Unreleased > Added entry for default Alt shortcuts.
- Tests added/modified: none (docs-only). Validation: `prepare-docs.mjs` (exit 0), `scripts.test_changelog` + full maintenance suite (53 tests, OK) run under Python 3.12 (sandbox default python3 is 3.7 and cannot run the scripts).
- Note: crate does not compile in this sandbox (vendored libghostty-vt zig build fails) — code-fact verification done by reading source + existing tests, not cargo.

### Review Pipeline
- Round 1:
  - Gate 1: 4 reviewers. gpu-reviewer APPROVED (0 issues). autosde 0 comments. auto-cr-reviewer 2 minor accuracy findings (FIXED: "silently rejected" → emits diagnostic; "longer dimension" → 1.5x threshold/"side-by-side for wide panes"). aspect-reviewer scored Clarity/Discoverability Strong, Completeness/Consistency Adequate, Conciseness Needs-Improvement; accepted verb-normalization (changelog "reorder"→"move"), rejected "remove dedicated table" (criterion requires it) and "trim 44-line keymap" (out of scope).
  - Gate 2: devils-advocate (no factual errors; completeness gaps) + adversarial-validator (every claim CONFIRMED truthful, both Gate 1 fixes accurate). FIXED 4 VALID items: vim/emacs collision naming (n/i/o/x) in silent-interception limitation; silent no-op note; terminal-mode-only expanded to Resize/copy/dialog; upgrade footgun for customized single-string configs; exact log string quoted in troubleshooting step 5.
  - Gate 3: blind-spot-assessor raised 1 Critical: "absent fields are unbound by default" misleading. Investigated against source — resolved a Gate2/Gate3 conflict: struct-level #[serde(default)] (model.rs:296) fills absent dual-bound fields from KeysConfig::default() (Alt-inclusive), while the 5 Alt-only fields carry field-level #[serde(default)] → unbound when absent (proven by test absent_new_action_field_is_unbound). FIXED: rewrote upgrade-behavior wording in both configuration.mdx and keyboard.mdx step 3 to state the per-action asymmetry and that upgraders get destructive Alt+x automatically unless they set a single string. Re-verified by adversarial-validator: CONFIRMED accurate.
- Internal loops: 1 (Gate 3 Critical → fix → targeted re-verify). 0 Gate-2-VALID loops.
- Round 2: not run — docs-only change, pipeline settled (Gate 3 produced no remaining Critical after fix; re-verify confirmed accuracy).

### ACKNOWLEDGED
- Auto-split phrasing "side-by-side for wide panes" — implementation threshold is w > h*1.5 (cell-aspect-corrected ~squareness approximation). Kept the visual description; over-precising into "1.5x in cells" would mislead. (Gate 1 auto-cr, Gate 3 #2)
- Resize axis-selection (horizontal-then-vertical) and single-pane no-op detail omitted from the resize line — covered generally in the silent-no-op limitation note; not inflating further. (Gate 2 devils, Gate 3 minor)
- Conciseness: same bindings appear in multiple tables across two files. Rejected aspect-reviewer's "remove dedicated table" because the acceptance criterion explicitly requires the table. (Gate 1 aspect)
- Test coverage: absent_new_action_field_is_unbound asserts 3/5 Alt-only fields; uniform by construction but only 3 sampled. Test-coverage observation, not a docs defect; belongs to step-1's test surface. (Gate 2 validator)

### REJECTED
- "Remove the dedicated Alt binding table" — contradicts acceptance criterion ("page or section covering the default Alt shortcut table").
- "Trim the 44-line default keymap block" — pre-existing content, out of step scope.
- rebuild-host.sh / .agents/ working-tree noise — pre-existing, not part of this step's commit.

### Circuit Breaker
- Issues tracked: none hit 2 strikes. The Gate 3 Critical was resolved on first fix + verification.

### Commit
- Branch: task/step-2-alt-shortcuts-docs
- Commit: ee2f9a7
- Message: docs: document alt shortcuts in unreleased docs
