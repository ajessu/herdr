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
/// (any) toward the visible edge; `attention_jump_to` is the nearest hidden
/// item in an attention state, or `None` when none on that side is.
///
/// The index space is surface-specific (workspace index, pane-entry index, tab
/// index); each surface range-asserts it before acting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct OverflowSide {
    pub hidden: usize,
    pub hidden_attention: usize,
    pub jump_to: usize,
    pub attention_jump_to: Option<usize>,
}

impl OverflowSide {
    pub fn is_empty(&self) -> bool {
        self.hidden == 0
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

/// Resolve the hidden items ABOVE a window into an `OverflowSide`. `is_attention`
/// is queried only over the hidden range `[0..window.first)` — no full rescan.
/// `jump_to` is the nearest hidden item (just above the window); the attention
/// target is the closest attention item to the visible edge (highest index).
pub(crate) fn side_above(window: ListWindow, is_attention: impl Fn(usize) -> bool) -> OverflowSide {
    let hidden = window.hidden_above;
    if hidden == 0 {
        return OverflowSide::default();
    }
    let mut hidden_attention = 0;
    let mut attention_jump_to = None;
    for i in 0..window.first {
        if is_attention(i) {
            hidden_attention += 1;
            // Keep the highest index (closest to the visible edge).
            attention_jump_to = Some(i);
        }
    }
    OverflowSide {
        hidden,
        hidden_attention,
        jump_to: window.first.saturating_sub(1),
        attention_jump_to,
    }
}

/// Resolve the hidden items BELOW a window into an `OverflowSide`. `total` is the
/// full item count; `is_attention` is queried only over `(last..total)`.
/// `jump_to` is the nearest hidden item (just below the window); the attention
/// target is the closest attention item to the visible edge (lowest index).
pub(crate) fn side_below(
    window: ListWindow,
    total: usize,
    is_attention: impl Fn(usize) -> bool,
) -> OverflowSide {
    let hidden = window.hidden_below;
    if hidden == 0 {
        return OverflowSide::default();
    }
    let lo = window.last().map(|l| l + 1).unwrap_or(window.first);
    let mut hidden_attention = 0;
    let mut attention_jump_to = None;
    for i in lo..total {
        if is_attention(i) {
            hidden_attention += 1;
            if attention_jump_to.is_none() {
                // First found is the lowest index (closest to the visible edge).
                attention_jump_to = Some(i);
            }
        }
    }
    OverflowSide {
        hidden,
        hidden_attention,
        jump_to: lo,
        attention_jump_to,
    }
}

/// The jump target a badge click resolves to: the nearest hidden attention item
/// when one exists on that side, else the nearest hidden item.
pub(crate) fn resolve_jump(side: OverflowSide) -> Option<usize> {
    if side.is_empty() {
        None
    } else {
        Some(side.attention_jump_to.unwrap_or(side.jump_to))
    }
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

/// Plain hidden-count text, capped at `9+` (matches the tab overflow cap).
pub(crate) fn count_label(n: usize) -> String {
    if n > 9 {
        "9+".to_string()
    } else {
        n.to_string()
    }
}

/// Styled spans for an overflow badge: always the dim `+N` count span, plus an
/// accent `●ᵏ` attention span when `hidden_attention > 0`.
pub(crate) fn badge_spans(
    hidden: usize,
    hidden_attention: usize,
    p: &Palette,
) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        format!("+{}", count_label(hidden)),
        Style::default().fg(p.overlay0),
    )];
    if hidden_attention > 0 {
        spans.push(Span::styled(" ", Style::default()));
        spans.push(Span::styled(
            format!("●{}", attention_superscript(hidden_attention)),
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
        ));
    }
    spans
}

/// Columns the `●ᵏ` attention portion alone occupies (no leading space): the
/// dot plus the superscript count glyph(s). Measured by Unicode display width
/// (not char count) to stay consistent with the rest of the tab-bar sizing —
/// `●` and the superscript digits are East-Asian-ambiguous and may render two
/// columns wide.
pub(crate) fn badge_attention_width(hidden_attention: usize) -> u16 {
    let s = format!("●{}", attention_superscript(hidden_attention));
    u16::try_from(s.width()).unwrap_or(u16::MAX)
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
    fn badge_attention_width_counts_dot_plus_superscript() {
        assert_eq!(badge_attention_width(2), 2); // "●²"
        assert_eq!(badge_attention_width(42), 3); // "●⁹⁺"
    }

    #[test]
    fn superscript_caps_at_nine_plus() {
        assert_eq!(attention_superscript(2), "²");
        assert_eq!(attention_superscript(42), "⁹⁺");
    }
}
