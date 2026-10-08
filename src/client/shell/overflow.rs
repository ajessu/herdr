//! Fork: attention-aware overflow helpers shared by the zellij-style tab bar
//! (and, later, the sidebar): hidden-item counts per agent state, the jump
//! target a click resolves to, and the `◉³ ◐ ●` badge segments.
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
