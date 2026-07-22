## Develop: step-3
Date: 2026-06-23

### Implementation
- Files changed: src/app/actions.rs, src/app/runtime.rs
- Tests added/modified: 4 new tests (switch_tab_clears_drag_state, switch_workspace_clears_drag_state, switch_workspace_tab_clears_drag_state, outer_focus_lost_clears_drag_state)

### Review Pipeline
- Round 1:
  - Gate 1: 4 reviewers (auto-cr-reviewer, gpu-reviewer, autosde, aspect-reviewer). autosde: 0 findings. Others: 1 major (extract helper — fixed), 2 minor (test variant, ratio assertion), several nits. Fixed major: extracted `clear_gesture_state()` helper. Fixed: test uses PaneSplit variant.
  - Gate 2: 2 cold reviewers + reconcile. Devils-advocate: close paths not clearing (over_flagged, out of scope), unconditional true return (over_flagged). Adversarial-validator: all 7 requirements CONFIRMED, 1 INSUFFICIENT (build pass — verified separately). 0 VALID items.
  - Gate 3: 1 significant (close_pane/close_tab not clearing — same as Gate 1, out of scope for step-3), 3 minor. 0 critical.
- Internal loops: 1 (Gate 1 major → fix → amend)
- Round 2: not needed

### Circuit Breaker
- Issues tracked: none (no repeated failures)

### Review Log
HUMANIZE_SKIP phase=develop gate=rmslop id=- reason="humanizer error: patterns.md not found"
DROP phase=develop gate=2 id=da-3 reason="selection not cleared on focus loss — intentional policy (not_a_bug, minor)"
DROP phase=develop gate=2 id=da-5 reason="hint_bar findings — not part of committed diff (not_a_bug, minor)"
DEMOTE phase=develop gate=2 id=da-1 reason="close_tab/close_pane not clearing — out of scope for step-3 (over_flagged, major)"

### Commit
- Branch: task/step-3
- Message: fix: clear stale drag state on tab switch, workspace switch, and focus loss
