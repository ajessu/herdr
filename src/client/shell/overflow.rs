//! Fork: attention-aware overflow helpers shared by the zellij-style tab bar
//! and sidebar: visible windows over a list, hidden-item counts per agent
//! state, the jump target a click resolves to, and the `+N ◉³ ◐ ●` badges.
//!
//! Ported from the fork's pre-v0.9.0 `src/ui/overflow.rs`. States come from the
//! client's projected `AgentStatus`, where `Done` already means "finished and
//! not yet seen by this client".

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};
use unicode_width::UnicodeWidthStr;

use crate::api::schema::AgentStatus;
use crate::app::state::Palette;

/// The three states an overflow badge surfaces. `Blocked` outranks `Working`
/// outranks `Done` for jump resolution (blocked is the only state that won't
/// progress without the user).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AttnBucket {
    Blocked,
    Working,
    Done,
}

impl AttnBucket {
    pub(super) fn classify(status: AgentStatus) -> Option<Self> {
        match status {
            AgentStatus::Blocked => Some(Self::Blocked),
            AgentStatus::Working => Some(Self::Working),
            AgentStatus::Done => Some(Self::Done),
            _ => None,
        }
    }

    /// A distinct glyph per bucket, not one dot in three colors.
    fn glyph(self) -> &'static str {
        match self {
            Self::Blocked => "◉",
            Self::Working => "◐",
            Self::Done => "●",
        }
    }

    fn color(self, p: &Palette) -> Color {
        match self {
            Self::Blocked => p.red,
            Self::Working => p.yellow,
            Self::Done => p.teal,
        }
    }
}

/// Render and jump-priority order: most urgent first.
const BUCKET_ORDER: [AttnBucket; 3] = [AttnBucket::Blocked, AttnBucket::Working, AttnBucket::Done];

/// Hidden items on one side of a window, with per-bucket counts and the
/// nearest-to-edge jump target of each, from one walk over the hidden range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct OverflowSide {
    pub(super) hidden: usize,
    pub(super) hidden_working: usize,
    pub(super) hidden_blocked: usize,
    pub(super) hidden_done: usize,
    pub(super) jump_to: usize,
    pub(super) working_jump_to: Option<usize>,
    pub(super) blocked_jump_to: Option<usize>,
    pub(super) done_jump_to: Option<usize>,
}

impl OverflowSide {
    pub(super) fn is_empty(&self) -> bool {
        self.hidden == 0
    }

    fn bucket_count(&self, bucket: AttnBucket) -> usize {
        match bucket {
            AttnBucket::Blocked => self.hidden_blocked,
            AttnBucket::Working => self.hidden_working,
            AttnBucket::Done => self.hidden_done,
        }
    }

    fn bucket_jump(&self, bucket: AttnBucket) -> Option<usize> {
        match bucket {
            AttnBucket::Blocked => self.blocked_jump_to,
            AttnBucket::Working => self.working_jump_to,
            AttnBucket::Done => self.done_jump_to,
        }
    }
}

/// A visible window `first..first+count` over a list, with the hidden counts on
/// either side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct ListWindow {
    pub(super) first: usize,
    pub(super) count: usize,
    pub(super) hidden_above: usize,
    pub(super) hidden_below: usize,
}

impl ListWindow {
    fn last(&self) -> Option<usize> {
        (self.count > 0).then(|| self.first + self.count - 1)
    }
}

/// Place a window of `vis` rows that keeps `anchor` visible, as close to the
/// top as possible. Returns `(first, count)`.
fn place_anchored(total: usize, vis: usize, anchor: usize) -> (usize, usize) {
    if vis == 0 || total == 0 {
        return (0, 0);
    }
    let vis = vis.min(total);
    let anchor = anchor.min(total - 1);
    let first = (anchor + 1).saturating_sub(vis).min(total - vis);
    (first, vis)
}

/// A window of at most `height` rows over `total` items that keeps `anchor`
/// visible and reserves one row on each side that has hidden items, for its
/// badge. Content and badges never overlap: `count + badges <= height`, and
/// `count >= 1` whenever `height >= 1`. When there is no room for both badges
/// the top one is dropped first, and a dropped side reports 0 hidden.
pub(super) fn anchored_window(total: usize, height: usize, anchor: usize) -> ListWindow {
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

/// Accumulate bucket counts and jump targets over `range`. `keep_latest` keeps
/// the highest index (the side above/left, closest to the visible edge);
/// otherwise the lowest wins.
fn accumulate_buckets(
    side: &mut OverflowSide,
    range: std::ops::Range<usize>,
    keep_latest: bool,
    status_of: impl Fn(usize) -> Option<AgentStatus>,
) {
    for i in range {
        let Some(bucket) = status_of(i).and_then(AttnBucket::classify) else {
            continue;
        };
        let (count, jump) = match bucket {
            AttnBucket::Blocked => (&mut side.hidden_blocked, &mut side.blocked_jump_to),
            AttnBucket::Working => (&mut side.hidden_working, &mut side.working_jump_to),
            AttnBucket::Done => (&mut side.hidden_done, &mut side.done_jump_to),
        };
        *count += 1;
        if keep_latest || jump.is_none() {
            *jump = Some(i);
        }
    }
}

/// The hidden items before the window. `status_of` is only queried over
/// `0..window.first`.
pub(super) fn side_above(
    window: ListWindow,
    status_of: impl Fn(usize) -> Option<AgentStatus>,
) -> OverflowSide {
    if window.hidden_above == 0 {
        return OverflowSide::default();
    }
    let mut side = OverflowSide {
        hidden: window.hidden_above,
        jump_to: window.first.saturating_sub(1),
        ..OverflowSide::default()
    };
    accumulate_buckets(&mut side, 0..window.first, true, status_of);
    side
}

/// The hidden items after the window. `status_of` is only queried over
/// `last+1..total`.
pub(super) fn side_below(
    window: ListWindow,
    total: usize,
    status_of: impl Fn(usize) -> Option<AgentStatus>,
) -> OverflowSide {
    if window.hidden_below == 0 {
        return OverflowSide::default();
    }
    let lo = window.last().map(|last| last + 1).unwrap_or(window.first);
    let mut side = OverflowSide {
        hidden: window.hidden_below,
        jump_to: lo,
        ..OverflowSide::default()
    };
    accumulate_buckets(&mut side, lo..total, false, status_of);
    side
}

/// Where a click on the overflow indicator goes: the nearest hidden item of the
/// most urgent bucket present, else the nearest hidden item.
pub(super) fn resolve_jump(side: OverflowSide) -> Option<usize> {
    if side.is_empty() {
        return None;
    }
    BUCKET_ORDER
        .into_iter()
        .find_map(|bucket| side.bucket_jump(bucket))
        .or(Some(side.jump_to))
}

/// Superscript count, capped at `⁹⁺`.
pub(super) fn attention_superscript(n: usize) -> String {
    const SUP: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    if n > 9 {
        "⁹⁺".to_string()
    } else {
        SUP[n].to_string()
    }
}

/// Hidden-item count, capped at `9+`.
pub(super) fn count_label(n: usize) -> String {
    if n > 9 {
        "9+".to_string()
    } else {
        n.to_string()
    }
}

/// The full overflow badge: `+N` then the bucket segments.
pub(super) fn badge_spans(side: OverflowSide, p: &Palette) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        format!("+{}", count_label(side.hidden)),
        Style::default().fg(p.overlay0),
    )];
    spans.extend(bucket_segment_spans(side, p));
    spans
}

/// ` <glyph><count>` per non-empty bucket, in urgency order.
pub(super) fn bucket_segment_spans(side: OverflowSide, p: &Palette) -> Vec<Span<'static>> {
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

/// Display columns of `bucket_segment_spans` (the glyphs and superscripts are
/// East-Asian-ambiguous, so measure rather than count).
pub(super) fn badge_attention_width(side: OverflowSide) -> u16 {
    let mut width: u16 = 0;
    for bucket in BUCKET_ORDER {
        let count = side.bucket_count(bucket);
        if count == 0 {
            continue;
        }
        let segment = format!(" {}{}", bucket.glyph(), attention_superscript(count));
        width = width.saturating_add(u16::try_from(segment.width()).unwrap_or(u16::MAX));
    }
    width
}

#[cfg(test)]
mod tests {
    use super::*;

    fn statuses(list: &[AgentStatus]) -> impl Fn(usize) -> Option<AgentStatus> + '_ {
        move |i| list.get(i).copied()
    }

    #[test]
    fn anchored_window_all_visible_when_fits() {
        let w = anchored_window(3, 5, 0);
        assert_eq!(
            (w.first, w.count, w.hidden_above, w.hidden_below),
            (0, 3, 0, 0)
        );
    }

    #[test]
    fn anchored_window_keeps_anchor_visible_and_reserves_indicators() {
        let w = anchored_window(10, 6, 5);
        assert_eq!(w.count, 4);
        assert!(w.first <= 5 && 5 < w.first + w.count);
        assert!(w.hidden_above > 0 && w.hidden_below > 0);
        assert_eq!(w.hidden_above + w.count + w.hidden_below, 10);
    }

    #[test]
    fn anchored_window_top_only_reserves_one_row() {
        let w = anchored_window(10, 6, 9);
        assert_eq!(w.hidden_below, 0);
        assert!(w.hidden_above > 0);
        assert_eq!(w.count, 5);
        assert_eq!(w.last(), Some(9));
    }

    #[test]
    fn anchored_window_bottom_only_reserves_one_row() {
        let w = anchored_window(10, 6, 0);
        assert_eq!(w.hidden_above, 0);
        assert!(w.hidden_below > 0);
        assert_eq!((w.first, w.count), (0, 5));
    }

    #[test]
    fn anchored_window_zero_height_hides_everything() {
        let w = anchored_window(4, 0, 0);
        assert_eq!((w.count, w.hidden_below), (0, 4));
    }

    #[test]
    fn anchored_window_never_overlaps_content_and_indicators() {
        for total in [0usize, 1, 2, 5, 10] {
            for height in 1usize..=12 {
                for anchor in 0..total.max(1) {
                    let w = anchored_window(total, height, anchor);
                    let shown = usize::from(w.hidden_above > 0) + usize::from(w.hidden_below > 0);
                    assert!(
                        w.count + shown <= height,
                        "total {total} height {height} anchor {anchor}: {w:?}"
                    );
                    if total > 0 {
                        assert!(
                            w.first <= anchor && anchor < w.first + w.count,
                            "anchor hidden: total {total} height {height} anchor {anchor}: {w:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn anchored_window_tiny_height_keeps_anchor_over_indicators() {
        let w = anchored_window(5, 1, 2);
        assert_eq!(
            (w.first, w.count, w.hidden_above, w.hidden_below),
            (2, 1, 0, 0)
        );
    }

    #[test]
    fn anchored_window_two_rows_shows_one_indicator_only() {
        let w = anchored_window(5, 2, 2);
        assert_eq!(w.count, 1);
        assert!(usize::from(w.hidden_above > 0) + usize::from(w.hidden_below > 0) <= 1);
    }

    #[test]
    fn count_label_caps_at_nine_plus() {
        assert_eq!(count_label(3), "3");
        assert_eq!(count_label(9), "9");
        for n in [10, 100, 10_000] {
            assert_eq!(count_label(n), "9+");
        }
    }

    #[test]
    fn badge_spans_lead_with_a_capped_count_then_the_bucket_segments() {
        let p = Palette::catppuccin();
        let side = OverflowSide {
            hidden: 12,
            hidden_working: 2,
            ..OverflowSide::default()
        };
        let spans = badge_spans(side, &p);
        assert_eq!(spans[0].content, "+9+");
        assert_eq!(spans[0].style.fg, Some(p.overlay0));
        assert_eq!(spans[1..], bucket_segment_spans(side, &p)[..]);
    }

    #[test]
    fn classify_keeps_only_attention_states() {
        assert_eq!(
            AttnBucket::classify(AgentStatus::Blocked),
            Some(AttnBucket::Blocked)
        );
        assert_eq!(
            AttnBucket::classify(AgentStatus::Working),
            Some(AttnBucket::Working)
        );
        assert_eq!(
            AttnBucket::classify(AgentStatus::Done),
            Some(AttnBucket::Done)
        );
        assert_eq!(AttnBucket::classify(AgentStatus::Idle), None);
        assert_eq!(AttnBucket::classify(AgentStatus::Unknown), None);
    }

    #[test]
    fn sides_count_each_bucket_and_keep_the_nearest_jump() {
        use AgentStatus::*;
        let list = [
            Blocked, Working, Done, Idle, Idle, Idle, Done, Blocked, Working, Idle,
        ];
        let window = ListWindow {
            first: 4,
            count: 2,
            hidden_above: 4,
            hidden_below: 4,
        };
        let above = side_above(window, statuses(&list));
        assert_eq!(
            (
                above.hidden,
                above.hidden_blocked,
                above.hidden_working,
                above.hidden_done
            ),
            (4, 1, 1, 1)
        );
        assert_eq!(above.jump_to, 3);
        assert_eq!(
            above.done_jump_to,
            Some(2),
            "left side keeps the highest index"
        );
        let below = side_below(window, list.len(), statuses(&list));
        assert_eq!(below.jump_to, 6);
        assert_eq!(
            below.done_jump_to,
            Some(6),
            "right side keeps the lowest index"
        );
        assert_eq!(below.blocked_jump_to, Some(7));
    }

    #[test]
    fn resolve_jump_prefers_blocked_then_working_then_done_then_nearest() {
        let mut side = OverflowSide {
            hidden: 5,
            jump_to: 4,
            done_jump_to: Some(3),
            working_jump_to: Some(2),
            blocked_jump_to: Some(1),
            ..OverflowSide::default()
        };
        assert_eq!(resolve_jump(side), Some(1));
        side.blocked_jump_to = None;
        assert_eq!(resolve_jump(side), Some(2));
        side.working_jump_to = None;
        assert_eq!(resolve_jump(side), Some(3));
        side.done_jump_to = None;
        assert_eq!(resolve_jump(side), Some(4));
        assert_eq!(resolve_jump(OverflowSide::default()), None);
    }

    #[test]
    fn badges_use_distinct_glyphs_colors_and_capped_superscripts() {
        let p = Palette::catppuccin();
        let side = OverflowSide {
            hidden: 20,
            hidden_blocked: 12,
            hidden_done: 1,
            ..OverflowSide::default()
        };
        let spans = bucket_segment_spans(side, &p);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, " ◉⁹⁺ ●¹");
        assert_eq!(spans[1].style.fg, Some(p.red));
        assert_eq!(spans[3].style.fg, Some(p.teal));
        assert_eq!(usize::from(badge_attention_width(side)), text.width());
        assert_eq!(badge_attention_width(OverflowSide::default()), 0);
    }
}
