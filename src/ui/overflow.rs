//! Shared attention-aware overflow indicator helpers (FR8).
//!
//! Five surfaces show "+N hidden, ●ᵏ need action" badges: the tab bar, the
//! collapsed rail's workspace + detail sections, and the expanded workspace
//! list + agent panel. The badge formatting and the list-windowing math live
//! here so each surface renders and hit-tests the same shape.
//!
//! Geometry is pure: these functions never mutate `AppState`. Each surface
//! computes its badge rects on the compute side and stores them for the mouse
//! layer to hit-test.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::Span,
};
use unicode_width::UnicodeWidthStr;

use crate::app::state::Palette;
use crate::detect::AgentState;

/// The three non-idle states an overflow badge surfaces, each rendered as its
/// own icon + count. `Blocked` outranks `Working` outranks `DoneUnseen` for
/// jump resolution (blocked is the only state that won't progress without the
/// user). Idle-seen and Unknown are not badge-worthy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttnBucket {
    /// Agent is blocked awaiting user input.
    Blocked,
    /// Agent is actively working.
    Working,
    /// Agent finished but the result has not been seen yet (`Idle`, `!seen`).
    DoneUnseen,
}

impl AttnBucket {
    /// Classify a `(state, seen)` tuple into a badge bucket, or `None` when the
    /// item is idle-seen / unknown (not badge-worthy).
    pub fn classify(state: AgentState, seen: bool) -> Option<Self> {
        match (state, seen) {
            (AgentState::Blocked, _) => Some(Self::Blocked),
            (AgentState::Working, _) => Some(Self::Working),
            (AgentState::Idle, false) => Some(Self::DoneUnseen),
            _ => None,
        }
    }

    /// A distinct badge glyph per bucket — the user wants three visually
    /// different icons, not one dot in three colors. Blocked is the filled
    /// target `◉`, Working the half-filled `◐`, DoneUnseen the solid `●`.
    fn glyph(self) -> &'static str {
        match self {
            Self::Blocked => "◉",
            Self::Working => "◐",
            Self::DoneUnseen => "●",
        }
    }

    /// The palette tone for this bucket, matching the per-tab status dots
    /// (`status::state_dot`): blocked red, working yellow, done-unseen teal.
    fn color(self, p: &Palette) -> ratatui::style::Color {
        match self {
            Self::Blocked => p.red,
            Self::Working => p.yellow,
            Self::DoneUnseen => p.teal,
        }
    }
}

/// Render order for the badge segments — Blocked first (most urgent), then
/// Working, then DoneUnseen. Also the jump-resolution priority order.
const BUCKET_ORDER: [AttnBucket; 3] = [
    AttnBucket::Blocked,
    AttnBucket::Working,
    AttnBucket::DoneUnseen,
];

/// A rendered overflow badge's clickable rect plus the resolved jump targets it
/// carries, stored on the compute side for the mouse layer to hit-test. `rect`
/// is `Rect::default()` (zero area) when the side is not shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct OverflowBadgeRect {
    pub rect: Rect,
    pub side: OverflowSide,
}

impl OverflowBadgeRect {
    pub fn is_active(&self) -> bool {
        self.rect.width > 0 && self.rect.height > 0 && !self.side.is_empty()
    }
}

/// Counts of hidden items on one side of a scrollable list, with the jump
/// targets a badge click resolves to. `jump_to` is the nearest hidden item
/// (any) toward the visible edge. Each non-idle bucket carries its own hidden
/// count and nearest-to-edge jump target, all computed in one walk over the
/// hidden range (no rescans).
///
/// The index space is surface-specific (workspace index, pane-entry index, tab
/// index); each surface range-asserts it before acting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct OverflowSide {
    pub hidden: usize,
    pub hidden_working: usize,
    pub hidden_blocked: usize,
    pub hidden_done_unseen: usize,
    pub jump_to: usize,
    pub working_jump_to: Option<usize>,
    pub blocked_jump_to: Option<usize>,
    pub done_unseen_jump_to: Option<usize>,
}

impl OverflowSide {
    pub fn is_empty(&self) -> bool {
        self.hidden == 0
    }

    /// Count for a given bucket.
    fn bucket_count(&self, bucket: AttnBucket) -> usize {
        match bucket {
            AttnBucket::Blocked => self.hidden_blocked,
            AttnBucket::Working => self.hidden_working,
            AttnBucket::DoneUnseen => self.hidden_done_unseen,
        }
    }

    /// Jump target for a given bucket.
    fn bucket_jump(&self, bucket: AttnBucket) -> Option<usize> {
        match bucket {
            AttnBucket::Blocked => self.blocked_jump_to,
            AttnBucket::Working => self.working_jump_to,
            AttnBucket::DoneUnseen => self.done_unseen_jump_to,
        }
    }

    /// Total badge-worthy (non-idle) hidden items across all three buckets.
    /// Production sizing now derives indicator width from the count and badge
    /// segments directly; the aggregate remains the assertion surface for the
    /// hidden-attention tests across the tab bar and sidebar.
    #[cfg(test)]
    pub fn hidden_attention(&self) -> usize {
        self.hidden_working + self.hidden_blocked + self.hidden_done_unseen
    }
}

/// A computed visible window over a scrollable list, with indicator rows
/// already reserved out of the available height. `first..first+count` are the
/// content rows actually drawn; `hidden_above`/`hidden_below` drive the top and
/// bottom overflow indicators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ListWindow {
    pub first: usize,
    pub count: usize,
    pub hidden_above: usize,
    pub hidden_below: usize,
}

impl ListWindow {
    pub fn last(&self) -> Option<usize> {
        (self.count > 0).then(|| self.first + self.count - 1)
    }
}

/// Place a window of `vis` rows over `total` items so `anchor` stays visible,
/// scrolling the minimum needed and never overscrolling past either end.
fn place_anchored(total: usize, vis: usize, anchor: usize) -> (usize, usize) {
    if vis == 0 || total == 0 {
        return (0, 0);
    }
    let vis = vis.min(total);
    let anchor = anchor.min(total - 1);
    // Pin anchor to the bottom of the window once it scrolls past the first
    // page, then clamp so we never show empty rows past the end.
    let first = (anchor + 1).saturating_sub(vis).min(total - vis);
    (first, vis)
}

/// Stateless anchored window for the collapsed rail's sections, reserving up to
/// two rows for top/bottom indicators. The active/selected workspace (or focused
/// pane) is the anchor and is always visible.
///
/// Invariant (no overlap): the returned window satisfies
/// `count + (hidden_above>0) + (hidden_below>0) <= height` and `count >= 1`
/// whenever `height >= 1` — the caller draws the top indicator at the first row,
/// the content rows next, and the bottom indicator at the last row, so those
/// regions must never share a cell. At tiny heights (1–2 rows) where content
/// plus both indicators cannot fit, an indicator is suppressed (its `hidden_*`
/// count is zeroed so no badge is drawn) — keeping the anchor visible wins over
/// signaling overflow on a rail too short to be useful.
pub(crate) fn anchored_window(total: usize, height: usize, anchor: usize) -> ListWindow {
    if height == 0 || total == 0 {
        return ListWindow {
            first: 0,
            count: 0,
            hidden_above: 0,
            hidden_below: total,
        };
    }
    if total <= height {
        return ListWindow {
            first: 0,
            count: total,
            hidden_above: 0,
            hidden_below: 0,
        };
    }

    // Overflow exists. Maximize content rows, shrinking only while the indicator
    // rows this window's position needs still fit alongside it. Content never
    // drops below one row (anchor stays visible).
    let mut content_h = height;
    let (first, vis, mut need_top, mut need_bottom) = loop {
        let (first, vis) = place_anchored(total, content_h, anchor);
        let last = first + vis - 1;
        let need_top = first > 0;
        let need_bottom = last + 1 < total;
        let indicators = usize::from(need_top) + usize::from(need_bottom);
        if vis + indicators <= height || content_h <= 1 {
            break (first, vis, need_top, need_bottom);
        }
        content_h -= 1;
    };

    // At the one-row floor both indicators may still not fit. Suppress them —
    // top first (keep the "more below" signal) — until the no-overlap invariant
    // holds. A suppressed side reports zero hidden so the caller draws no badge.
    let mut shown = usize::from(need_top) + usize::from(need_bottom);
    while vis + shown > height {
        if need_top {
            need_top = false;
        } else if need_bottom {
            need_bottom = false;
        } else {
            break;
        }
        shown = usize::from(need_top) + usize::from(need_bottom);
    }

    let last = first + vis - 1;
    ListWindow {
        first,
        count: vis,
        hidden_above: if need_top { first } else { 0 },
        hidden_below: if need_bottom { total - 1 - last } else { 0 },
    }
}

/// Scroll-offset window for the expanded surfaces, which already own a
/// top-anchored `scroll` (first visible index). Indicator rows are NOT reserved
/// here — the expanded lists keep their existing row budget and overlay the
/// badge on the first/last body row.
pub(crate) fn scrolled_window(total: usize, height: usize, scroll: usize) -> ListWindow {
    if height == 0 || total == 0 {
        return ListWindow {
            first: 0,
            count: 0,
            hidden_above: 0,
            hidden_below: total,
        };
    }
    let max_first = total.saturating_sub(1);
    let first = scroll.min(max_first);
    let count = height.min(total - first);
    let last = first + count - 1;
    ListWindow {
        first,
        count,
        hidden_above: first,
        hidden_below: total.saturating_sub(last + 1),
    }
}

/// Accumulate the three bucket counts + jump targets in a single forward walk
/// over `range`. `keep_latest` controls which match wins the jump target: for
/// the ABOVE side we keep the latest (highest index, closest to the visible
/// edge); for the BELOW side we keep the earliest (lowest index). `state_of`
/// is queried only over the hidden range — no full rescan.
fn accumulate_buckets(
    side: &mut OverflowSide,
    range: std::ops::Range<usize>,
    keep_latest: bool,
    state_of: impl Fn(usize) -> (AgentState, bool),
) {
    for i in range {
        let (state, seen) = state_of(i);
        let Some(bucket) = AttnBucket::classify(state, seen) else {
            continue;
        };
        let (count, jump) = match bucket {
            AttnBucket::Blocked => (&mut side.hidden_blocked, &mut side.blocked_jump_to),
            AttnBucket::Working => (&mut side.hidden_working, &mut side.working_jump_to),
            AttnBucket::DoneUnseen => (&mut side.hidden_done_unseen, &mut side.done_unseen_jump_to),
        };
        *count += 1;
        if keep_latest || jump.is_none() {
            *jump = Some(i);
        }
    }
}

/// Resolve the hidden items ABOVE a window into an `OverflowSide`. `state_of`
/// is queried only over the hidden range `[0..window.first)` — no full rescan.
/// `jump_to` is the nearest hidden item (just above the window); each bucket's
/// jump target is the closest item of that state to the visible edge (highest
/// index).
pub(crate) fn side_above(
    window: ListWindow,
    state_of: impl Fn(usize) -> (AgentState, bool),
) -> OverflowSide {
    let hidden = window.hidden_above;
    if hidden == 0 {
        return OverflowSide::default();
    }
    let mut side = OverflowSide {
        hidden,
        jump_to: window.first.saturating_sub(1),
        ..OverflowSide::default()
    };
    accumulate_buckets(&mut side, 0..window.first, true, state_of);
    side
}

/// Resolve the hidden items BELOW a window into an `OverflowSide`. `total` is the
/// full item count; `state_of` is queried only over `(last..total)`. `jump_to`
/// is the nearest hidden item (just below the window); each bucket's jump
/// target is the closest item of that state to the visible edge (lowest index).
pub(crate) fn side_below(
    window: ListWindow,
    total: usize,
    state_of: impl Fn(usize) -> (AgentState, bool),
) -> OverflowSide {
    let hidden = window.hidden_below;
    if hidden == 0 {
        return OverflowSide::default();
    }
    let lo = window.last().map(|l| l + 1).unwrap_or(window.first);
    let mut side = OverflowSide {
        hidden,
        jump_to: lo,
        ..OverflowSide::default()
    };
    accumulate_buckets(&mut side, lo..total, false, state_of);
    side
}

/// The jump target a badge click resolves to: the nearest hidden item in the
/// highest-priority non-idle bucket present (Blocked → Working → DoneUnseen),
/// else the nearest hidden item of any kind.
pub(crate) fn resolve_jump(side: OverflowSide) -> Option<usize> {
    if side.is_empty() {
        return None;
    }
    for bucket in BUCKET_ORDER {
        if let Some(target) = side.bucket_jump(bucket) {
            return Some(target);
        }
    }
    Some(side.jump_to)
}

/// Superscript rendering of an attention count, capped at `⁹⁺`.
pub(crate) fn attention_superscript(n: usize) -> String {
    const SUP: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    if n > 9 {
        "⁹⁺".to_string()
    } else {
        SUP[n].to_string()
    }
}

/// Plain hidden-count text, capped at `9+`. Used by the sidebar badges; the
/// tab bar's overflow indicator composes its own uncapped zellij-format count
/// and does not go through this cap.
pub(crate) fn count_label(n: usize) -> String {
    if n > 9 {
        "9+".to_string()
    } else {
        n.to_string()
    }
}

/// Styled spans for an overflow badge: always the dim `+N` count span, then one
/// ` <glyph><count>` segment per non-zero bucket in `BUCKET_ORDER` (Blocked,
/// Working, DoneUnseen). Each segment uses the bucket's distinct glyph + tone;
/// zero-count buckets are omitted entirely (no `◐⁰`).
pub(crate) fn badge_spans(side: OverflowSide, p: &Palette) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        format!("+{}", count_label(side.hidden)),
        Style::default().fg(p.overlay0),
    )];
    spans.extend(bucket_segment_spans(side, p));
    spans
}

/// Just the per-bucket badge segments (no leading `+N` count span): one
/// ` <glyph><count>` segment per non-zero bucket in `BUCKET_ORDER`. Split out
/// so the tab bar can compose its own uncapped count text (zellij's full-count
/// ` +N ` format) ahead of the segments, while `badge_spans` — and with it the
/// sidebar's capped `+9+` badge — stays byte-for-byte unchanged.
pub(crate) fn bucket_segment_spans(side: OverflowSide, p: &Palette) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for bucket in BUCKET_ORDER {
        let count = side.bucket_count(bucket);
        if count == 0 {
            continue;
        }
        spans.push(Span::styled(" ", Style::default()));
        spans.push(Span::styled(
            format!("{}{}", bucket.glyph(), attention_superscript(count)),
            Style::default()
                .fg(bucket.color(p))
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans
}

/// Columns the bucket-badge portion occupies after the `+N` count: for each
/// non-zero bucket, a leading space + the glyph + the superscript count.
/// Measured by Unicode display width (not char count) to stay consistent with
/// the rest of the tab-bar sizing — the glyphs and superscript digits are
/// East-Asian-ambiguous and may render two columns wide.
pub(crate) fn badge_attention_width(side: OverflowSide) -> u16 {
    let mut w: u16 = 0;
    for bucket in BUCKET_ORDER {
        let count = side.bucket_count(bucket);
        if count == 0 {
            continue;
        }
        let s = format!(" {}{}", bucket.glyph(), attention_superscript(count));
        w = w.saturating_add(u16::try_from(s.width()).unwrap_or(u16::MAX));
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchored_window_all_visible_when_fits() {
        let w = anchored_window(3, 5, 0);
        assert_eq!(w.first, 0);
        assert_eq!(w.count, 3);
        assert_eq!(w.hidden_above, 0);
        assert_eq!(w.hidden_below, 0);
    }

    #[test]
    fn anchored_window_keeps_anchor_visible_and_reserves_indicators() {
        // 10 items, 6 rows, anchor in the middle → both indicators reserved,
        // 4 content rows, anchor inside the window.
        let w = anchored_window(10, 6, 5);
        assert_eq!(w.count, 4, "6 rows minus top+bottom indicator rows");
        assert!(w.first <= 5 && 5 < w.first + w.count, "anchor visible");
        assert!(w.hidden_above > 0 && w.hidden_below > 0);
        assert_eq!(w.hidden_above + w.count + w.hidden_below, 10);
    }

    #[test]
    fn anchored_window_top_only_reserves_one_row() {
        // Anchor at the very end: nothing hidden below, only a top indicator.
        let w = anchored_window(10, 6, 9);
        assert_eq!(w.hidden_below, 0);
        assert!(w.hidden_above > 0);
        assert_eq!(w.count, 5, "6 rows minus a single top indicator row");
        assert_eq!(w.last(), Some(9));
    }

    #[test]
    fn anchored_window_bottom_only_reserves_one_row() {
        // Anchor at the start: nothing hidden above, only a bottom indicator.
        let w = anchored_window(10, 6, 0);
        assert_eq!(w.hidden_above, 0);
        assert!(w.hidden_below > 0);
        assert_eq!(w.first, 0);
        assert_eq!(w.count, 5, "6 rows minus a single bottom indicator row");
    }

    #[test]
    fn anchored_window_zero_height_hides_everything() {
        let w = anchored_window(4, 0, 0);
        assert_eq!(w.count, 0);
        assert_eq!(w.hidden_below, 4);
    }

    #[test]
    fn anchored_window_never_overlaps_content_and_indicators() {
        // No-overlap invariant: count + shown-indicators <= height, for every
        // height (including the tiny 1-2 row cases) and anchor position. The
        // caller draws the top indicator, content, and bottom indicator in
        // disjoint rows, so this must always hold or a badge collides with a row.
        for total in [0usize, 1, 2, 5, 10] {
            // height 0 draws nothing, so the no-overlap invariant only applies
            // once there is at least one row to place content/indicators in.
            for height in 1..=12usize {
                for anchor in 0..total.max(1) {
                    let w = anchored_window(total, height, anchor);
                    let shown = usize::from(w.hidden_above > 0) + usize::from(w.hidden_below > 0);
                    assert!(
                        w.count + shown <= height,
                        "overlap: total={total} height={height} anchor={anchor} \
                         -> count={} above={} below={}",
                        w.count,
                        w.hidden_above,
                        w.hidden_below,
                    );
                    // Anchor stays visible whenever any row fits.
                    if total > 0 {
                        assert!(
                            w.first <= anchor && anchor < w.first + w.count,
                            "anchor not visible: total={total} height={height} anchor={anchor}",
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn anchored_window_tiny_height_keeps_anchor_over_indicators() {
        // 5 items, only 1 row, interior anchor: content wins, no indicator row
        // is reserved (both suppressed) so nothing overlaps the single row.
        let w = anchored_window(5, 1, 2);
        assert_eq!(w.count, 1);
        assert!(w.first <= 2 && 2 < w.first + w.count, "anchor visible");
        assert_eq!(w.hidden_above, 0, "no room for a top indicator at height 1");
        assert_eq!(
            w.hidden_below, 0,
            "no room for a bottom indicator at height 1"
        );
    }

    #[test]
    fn anchored_window_two_rows_shows_one_indicator_only() {
        // 5 items, 2 rows, interior anchor: 1 content row + at most 1 indicator.
        let w = anchored_window(5, 2, 2);
        assert_eq!(w.count, 1);
        let shown = usize::from(w.hidden_above > 0) + usize::from(w.hidden_below > 0);
        assert!(
            shown <= 1,
            "only one indicator fits alongside one content row"
        );
        assert_eq!(w.count + shown, 2.min(w.count + shown));
    }

    #[test]
    fn scrolled_window_reports_hidden_both_sides() {
        let w = scrolled_window(10, 4, 3);
        assert_eq!(w.first, 3);
        assert_eq!(w.count, 4);
        assert_eq!(w.hidden_above, 3);
        assert_eq!(w.hidden_below, 3);
    }

    #[test]
    fn scrolled_window_clamps_scroll_past_end() {
        let w = scrolled_window(5, 4, 99);
        assert!(w.first < 5);
        assert_eq!(w.hidden_above + w.count + w.hidden_below, 5);
    }

    #[test]
    fn badge_attention_width_sums_nonzero_buckets() {
        // Single bucket: " " + glyph + superscript = 1 + 1 + 1 = 3.
        let one = OverflowSide {
            hidden: 2,
            hidden_blocked: 2,
            blocked_jump_to: Some(0),
            ..OverflowSide::default()
        };
        assert_eq!(badge_attention_width(one), 3);
        // All three non-zero: three " glyph^k" segments = 3 * 3 = 9.
        let three = OverflowSide {
            hidden: 3,
            hidden_blocked: 1,
            hidden_working: 1,
            hidden_done_unseen: 1,
            blocked_jump_to: Some(0),
            working_jump_to: Some(1),
            done_unseen_jump_to: Some(2),
            ..OverflowSide::default()
        };
        assert_eq!(badge_attention_width(three), 9);
        // No buckets: zero width.
        let none = OverflowSide {
            hidden: 4,
            jump_to: 0,
            ..OverflowSide::default()
        };
        assert_eq!(badge_attention_width(none), 0);
    }

    #[test]
    fn superscript_caps_at_nine_plus() {
        assert_eq!(attention_superscript(2), "²");
        assert_eq!(attention_superscript(42), "⁹⁺");
    }

    #[test]
    fn count_label_caps_at_nine_plus() {
        // The sidebar's badge cap must survive the tab bar going uncapped
        // (the tab bar composes its own full-count text and never calls this).
        assert_eq!(count_label(3), "3");
        assert_eq!(count_label(9), "9");
        assert_eq!(count_label(10), "9+");
        assert_eq!(count_label(100), "9+");
        assert_eq!(count_label(10000), "9+");
    }

    #[test]
    fn badge_spans_count_stays_capped_past_nine() {
        // Sidebar badge at a >9 hidden count still renders `+9+`.
        let p = Palette::catppuccin();
        let side = OverflowSide {
            hidden: 12,
            ..OverflowSide::default()
        };
        let spans = badge_spans(side, &p);
        assert_eq!(spans[0].content.as_ref(), "+9+");
    }

    #[test]
    fn bucket_segment_spans_match_badge_spans_tail() {
        // `badge_spans` == `+N` count span followed by `bucket_segment_spans`,
        // so the tab bar composing its own count over the shared segments
        // renders the identical bucket portion the sidebar shows.
        let p = Palette::catppuccin();
        let side = OverflowSide {
            hidden: 5,
            hidden_blocked: 2,
            hidden_working: 1,
            blocked_jump_to: Some(1),
            working_jump_to: Some(2),
            ..OverflowSide::default()
        };
        let full = badge_spans(side, &p);
        let segments = bucket_segment_spans(side, &p);
        assert_eq!(full.len(), segments.len() + 1);
        for (a, b) in full[1..].iter().zip(segments.iter()) {
            assert_eq!(a.content, b.content);
            assert_eq!(a.style, b.style);
        }
    }

    // --- Disaggregated-bucket tests (zellij-fidelity round 2, change 5) ---

    fn states(seq: &[(AgentState, bool)]) -> impl Fn(usize) -> (AgentState, bool) + '_ {
        move |i: usize| seq[i]
    }

    #[test]
    fn side_above_accumulates_three_counts_in_one_walk() {
        use AgentState::*;
        // indices 0..4 hidden: blocked, working, idle-unseen, blocked.
        let seq = [
            (Blocked, false),
            (Working, false),
            (Idle, false),
            (Blocked, false),
            (Idle, true), // visible window starts here (index 4)
        ];
        let window = ListWindow {
            first: 4,
            count: 1,
            hidden_above: 4,
            hidden_below: 0,
        };
        let side = side_above(window, states(&seq));
        assert_eq!(side.hidden, 4);
        assert_eq!(side.hidden_blocked, 2);
        assert_eq!(side.hidden_working, 1);
        assert_eq!(side.hidden_done_unseen, 1);
        // ABOVE keeps the highest index per bucket (closest to visible edge).
        assert_eq!(side.blocked_jump_to, Some(3));
        assert_eq!(side.working_jump_to, Some(1));
        assert_eq!(side.done_unseen_jump_to, Some(2));
    }

    #[test]
    fn side_below_accumulates_three_counts_in_one_walk() {
        use AgentState::*;
        // index 0 visible; 1..5 hidden: working, blocked, idle-unseen, working.
        let seq = [
            (Idle, true),
            (Working, false),
            (Blocked, false),
            (Idle, false),
            (Working, false),
        ];
        let window = ListWindow {
            first: 0,
            count: 1,
            hidden_above: 0,
            hidden_below: 4,
        };
        let side = side_below(window, seq.len(), states(&seq));
        assert_eq!(side.hidden, 4);
        assert_eq!(side.hidden_working, 2);
        assert_eq!(side.hidden_blocked, 1);
        assert_eq!(side.hidden_done_unseen, 1);
        // BELOW keeps the lowest index per bucket (closest to visible edge).
        assert_eq!(side.working_jump_to, Some(1));
        assert_eq!(side.blocked_jump_to, Some(2));
        assert_eq!(side.done_unseen_jump_to, Some(3));
    }

    #[test]
    fn badge_spans_omits_zero_count_segments() {
        let p = Palette::catppuccin();
        // Only working > 0.
        let side = OverflowSide {
            hidden: 3,
            hidden_working: 2,
            working_jump_to: Some(0),
            ..OverflowSide::default()
        };
        let text: String = badge_spans(side, &p)
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(text.contains('◐'), "working glyph present: {text}");
        assert!(!text.contains('◉'), "no blocked glyph: {text}");
        assert!(!text.contains('●'), "no done-unseen glyph: {text}");
    }

    #[test]
    fn badge_spans_renders_all_three_in_bucket_order() {
        let p = Palette::catppuccin();
        let side = OverflowSide {
            hidden: 3,
            hidden_blocked: 1,
            hidden_working: 2,
            hidden_done_unseen: 3,
            blocked_jump_to: Some(0),
            working_jump_to: Some(1),
            done_unseen_jump_to: Some(2),
            ..OverflowSide::default()
        };
        let spans = badge_spans(side, &p);
        // Glyph spans in order: ◉ (blocked), ◐ (working), ● (done-unseen).
        let glyphs: Vec<String> = spans
            .iter()
            .map(|s| s.content.to_string())
            .filter(|c| c.contains('◉') || c.contains('◐') || c.contains('●'))
            .collect();
        assert_eq!(glyphs.len(), 3, "all three bucket glyphs: {glyphs:?}");
        assert!(glyphs[0].starts_with('◉'), "blocked first: {glyphs:?}");
        assert!(glyphs[1].starts_with('◐'), "working second: {glyphs:?}");
        assert!(glyphs[2].starts_with('●'), "done-unseen third: {glyphs:?}");
    }

    #[test]
    fn resolve_jump_priority_blocked_first() {
        let side = OverflowSide {
            hidden: 3,
            hidden_working: 1,
            hidden_blocked: 1,
            working_jump_to: Some(1),
            blocked_jump_to: Some(2),
            jump_to: 0,
            ..OverflowSide::default()
        };
        assert_eq!(resolve_jump(side), Some(2), "blocked outranks working");
    }

    #[test]
    fn resolve_jump_done_unseen_then_fallback() {
        // Only done-unseen populated → resolves to done-unseen.
        let only_done = OverflowSide {
            hidden: 2,
            hidden_done_unseen: 1,
            done_unseen_jump_to: Some(1),
            jump_to: 0,
            ..OverflowSide::default()
        };
        assert_eq!(resolve_jump(only_done), Some(1));
        // No buckets → falls back to the nearest hidden item.
        let plain = OverflowSide {
            hidden: 2,
            jump_to: 0,
            ..OverflowSide::default()
        };
        assert_eq!(resolve_jump(plain), Some(0));
        // Empty side → None.
        assert_eq!(resolve_jump(OverflowSide::default()), None);
    }
}
