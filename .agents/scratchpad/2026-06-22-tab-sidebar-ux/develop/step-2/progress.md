## Develop: step-2
Date: 2026-06-23

### Implementation
- Files changed:
  - `src/config/keybinds.rs` — added `ActionKeybinds::alt_direct_label()` (returns the first Direct binding whose combo carries ALT; label is already normalized via `format_key_combo` at parse time) + 3 unit tests.
  - `src/ui/hint_bar.rs` — added `alt_hints: Vec<Hint>` to `HintSet`; `alt_binding_label` helper; `terminal_alt_hints` builder (grouped FOCUS/SPLIT/CLOSE/RESIZE/MOV TAB, Terminal mode only); `compute_section_width`/`build_section_spans` helpers; reworked `build_hint_line` into the FR4 4-tier degradation; `MIN_SECTION_GAP` const.
- Tests added/modified: 16 new hint_bar tests (alt-hint presence, Alt-modifier resolution, Many-binding alt alternative, drop-on-remap, 4 distinct degradation tiers, Compact interaction, bidi sanitization, no-overlap sweep across Full+Compact, wide-remapped-label no-overlap, min-gap invariant) + 3 keybinds tests for `alt_direct_label`. Updated the two existing `HintSet` literal test constructors for the new field.

### Review Pipeline
- Round 1:
  - Gate 1: 4 reviewers. auto-cr-reviewer (minor+2 nits), gpu-reviewer (APPROVED, 3 nits), autosde (0 comments), aspect-reviewer (all Strong). Fixed: Compact-mode label consistency (force_short skips tier 1), width-accounting sanitize consistency, test exhaustiveness (added Resize/Move/Prefix), removed dead-code test guard, added Compact+alt and direct alt_direct_label tests.
  - Gate 2 (cold path): devils-advocate + adversarial-validator run cold; reviewer-reconciler classified. adversarial-validator: CONFIRMED (no DISPUTED). Reconciler: [0] not_a_bug (bundled-commit premise false — drag-state files not in commit), [1] not_a_bug (raw-label premise false — label normalized at parse), [2]-[10] minor real_bugs → ACKNOWLEDGED. 0 VALID. narrative_drift: none. Applied 2 zero-risk cleanups: unified force_short/use_short predicate, MIN_SECTION_GAP const.
  - Gate 3: blind-spot-assessor. 0 Critical, 0 bsa-concurrency. Significant/Minor → ACKNOWLEDGED. Resolved author-question #4 (Tier-3 `used` accounting byte-for-byte unchanged). Added 2 test reinforcements of the never-overlap/never-touch AC (wide-remapped-label sweep, min-gap invariant).
- Internal loops: 0 (Gate 2 VALID: 0, Gate 3 Critical: 0)

### Circuit Breaker
- Issues tracked: none (no issue hit 2 strikes)

### Review Log
DEMOTE phase=develop gate=2 id=0 reason="major not_a_bug floored to ACKNOWLEDGED — claimed bundled drag-state change is not in this commit (commit touches only keybinds.rs + hint_bar.rs)"
DEMOTE phase=develop gate=2 id=1 reason="major not_a_bug floored to ACKNOWLEDGED — claimed raw unnormalized label is false; ResolvedBinding.label is normalized via format_key_combo at parse time"

### Commit
- Branch: task/step-2-hint-bar-alt
- Message: feat(hint-bar): add right-aligned Alt-shortcut section

### ACKNOWLEDGED tradeoffs
- `alt_direct_label` returns the first Alt alternative only (find_map); a second configured Alt binding is not surfaced. Acceptable for a hint bar; the section advertises one representative key.
- `mods.contains(ALT)` would also surface `ctrl+alt+x`-style combos under the Alt section; default binds are pure-Alt so this is theoretical for non-default configs.
- Grouped directional labels (alt+h/alt+j/...) can read as opaque under heavy partial remaps; the grouped-display format was a user-confirmed decision.
- `priority` on alt_hints is currently unused (Alt section is intentionally atomic — whole-section drop, not per-entry).
- Three explanatory comments were removed in resize_hints/locked_hints/a test during the refactor; behavior unchanged.
