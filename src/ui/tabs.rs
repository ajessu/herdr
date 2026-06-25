use std::borrow::Cow;

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use unicode_width::UnicodeWidthStr;

use super::widgets::panel_contrast_fg;
use crate::app::AppState;
use crate::config::TabStatusMode;

const MIN_TAB_WIDTH: u16 = 10;
const NEW_TAB_WIDTH: u16 = 3;
/// Each tab owns its own left + right separator columns (zellij convention).
/// Adjacent tabs abut directly: the right separator of T<sub>N</sub> sits next
/// to the left separator of T<sub>N+1</sub>, producing two back-to-back wedges
/// that blend against `panel_bg` from both sides.
const TAB_SEPARATOR_OVERHEAD: u16 = 2;
/// Width of a collapsed `←+N` / `+N→` overflow indicator, rendered as a proper
/// zellij tile (left arrow + interior + right arrow). Six cells = 2 separator
/// cols + 4 interior cols, keeping a touch-adequate hit zone (NFR5) — never a
/// 1-cell target — and fitting the widest label (`←+9+` / `+9+→`) without
/// clipping. Under `AlternatingBg` the 2 separator cols become interior
/// padding.
const OVERFLOW_INDICATOR_WIDTH: u16 = 6;
/// Interior width of an overflow indicator = total minus the 2 separator cols.
const OVERFLOW_INDICATOR_INTERIOR: u16 = OVERFLOW_INDICATOR_WIDTH - TAB_SEPARATOR_OVERHEAD;

/// The Powerline "right arrow" separator glyph (U+E0B0). Rendered only when
/// `SeparatorStyle::Powerline` is selected; the `AlternatingBg` path emits zero
/// Powerline codepoints so a font-tofu terminal degrades cleanly.
const POWERLINE_ARROW: &str = "\u{e0b0}";

/// Selects how adjacent tabs are visually separated. Chosen by an explicit
/// painter parameter (from `ui.tabs.powerline`), never an environment probe, so
/// both paths are deterministically unit-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SeparatorStyle {
    /// Powerline arrow glyphs between tabs (default, `ui.tabs.powerline = true`).
    Powerline,
    /// Alternating tab backgrounds as the separator; no Powerline codepoints.
    AlternatingBg,
}

/// A run of hidden tabs collapsed behind one overflow indicator. `count` and
/// `jump_to` are the total hidden count and the nearest hidden tab INDEX on
/// that side. `side` carries the disaggregated Working/Blocked/Done-unseen
/// counts + per-bucket jump targets (shared with the sidebar surfaces via
/// `overflow::OverflowSide`), all computed on one walk over the hidden range.
/// The TAB INDEX targets are range-asserted against the live tab count before
/// use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HiddenGroup {
    pub count: usize,
    pub jump_to: usize,
    pub side: super::overflow::OverflowSide,
}

/// Overflow indicators for the centered-active fill: the hidden tabs to each
/// side of the visible window. Either side is `None` when nothing is hidden
/// there. The rects are the clickable hit zones (mouse chrome) or marker
/// positions (non-mouse).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TabBarOverflow {
    pub left: Option<HiddenGroup>,
    pub right: Option<HiddenGroup>,
    pub left_hit_area: Rect,
    pub right_hit_area: Rect,
}

// ---------------------------------------------------------------------------
// TabChrome — structured per-tab label model
// ---------------------------------------------------------------------------

/// Strip control and bidi-override characters from a user-writable display
/// name before it reaches the buffer. Tab/workspace `custom_name` is persisted,
/// re-encoded over the web transport, and rendered in the full-width readable
/// active tab beside inverted Powerline separators, which raises the spoof
/// value of an embedded RTL override or control sequence. Borrows when the
/// input is already clean. Mirrors `hint_bar::sanitize_key`.
pub(crate) fn sanitize_display_name(s: &str) -> Cow<'_, str> {
    let needs_sanitize = s.chars().any(|c| c.is_control() || is_bidi_override(c));
    if needs_sanitize {
        Cow::Owned(
            s.chars()
                .filter(|c| !c.is_control() && !is_bidi_override(*c))
                .collect(),
        )
    } else {
        Cow::Borrowed(s)
    }
}

fn is_bidi_override(c: char) -> bool {
    matches!(
        c,
        '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}'
    )
}

/// Truncate `s` to at most `budget` display columns, appending `…` (1 column)
/// when anything was dropped. Accumulates per-grapheme display width so wide
/// (CJK) glyphs are never split across the truncation boundary.
fn truncate_to_width(s: &str, budget: u16) -> String {
    if budget == 0 {
        return String::new();
    }
    // Reserve one column for the trailing ellipsis.
    let content_budget = usize::from(budget.saturating_sub(1));
    let mut out = String::new();
    let mut used = 0usize;
    for ch in s.chars() {
        let ch_w = ch.to_string().width();
        if used + ch_w > content_budget {
            break;
        }
        out.push(ch);
        used += ch_w;
    }
    out.push('…');
    out
}

#[derive(Debug, Clone)]
pub(crate) struct TabStatusDot {
    pub glyph: &'static str,
    pub style: Style,
}

#[derive(Debug, Clone)]
pub(crate) struct TabChrome {
    pub status: Option<TabStatusDot>,
    pub name: String,
    pub zoomed: bool,
    /// Whether this tab's pane is in an attention state (Blocked or
    /// Idle-unseen). Derived from the same `(state, seen)` tuple the status dot
    /// uses; `false` under `TabStatusMode::Off`. Does not affect tab width or
    /// span content — the attention badge renders on the overflow indicator, not
    /// the tab cell. Consumed by `compute_tab_bar_view` to count hidden-attention
    /// tabs on the same fill walk.
    pub is_attention: bool,
    /// The tab's `(state, seen)` source tuple (None under `TabStatusMode::Off`).
    /// Carried so `compute_tab_bar_view` can classify hidden tabs into the
    /// three overflow badge buckets on its single fill walk.
    pub agent_state: Option<(crate::detect::AgentState, bool)>,
}

impl TabChrome {
    pub fn display_width(&self, mode: TabStatusMode) -> u16 {
        let status_w: u16 = if matches!(mode, TabStatusMode::Off) {
            0
        } else {
            2
        };
        // Unicode display width (not char count): CJK/wide glyphs occupy two
        // columns and combining/ZWJ sequences fewer, so char count would
        // mis-size the tab and break the position→index round-trip. The
        // sanitized name is what actually reaches the buffer, so measure it.
        let name_w = u16::try_from(sanitize_display_name(&self.name).width()).unwrap_or(u16::MAX);
        let mod_w: u16 = if self.zoomed { 2 } else { 0 };
        status_w.saturating_add(name_w).saturating_add(mod_w)
    }

    /// Render the tab label into spans padded to `rect_width`. The name is
    /// sanitized (control/bidi stripped) at this render chokepoint. The active
    /// tab is always laid out at its natural width, so truncation only happens
    /// for a single over-wide tab that alone exceeds the bar — in that case the
    /// name is shortened by Unicode display width with a trailing `…`.
    pub fn to_spans(&self, mode: TabStatusMode, rect_width: u16) -> Vec<Span<'static>> {
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(6);

        // Leading space
        spans.push(Span::raw(" "));

        // Status slot (only when mode != Off)
        if !matches!(mode, TabStatusMode::Off) {
            if let Some(ref dot) = self.status {
                spans.push(Span::styled(dot.glyph, dot.style));
                spans.push(Span::raw(" "));
            } else {
                spans.push(Span::raw("  "));
            }
        }

        let status_w: u16 = if matches!(mode, TabStatusMode::Off) {
            0
        } else {
            2
        };
        let mod_w: u16 = if self.zoomed { 2 } else { 0 };
        // 1 col for the leading space, plus the status slot and zoom suffix.
        let name_budget = rect_width
            .saturating_sub(1)
            .saturating_sub(status_w)
            .saturating_sub(mod_w);
        let name = sanitize_display_name(&self.name);
        let name_cols = u16::try_from(name.width()).unwrap_or(u16::MAX);
        if name_budget > 0 && name_cols > name_budget {
            spans.push(Span::raw(truncate_to_width(&name, name_budget)));
        } else {
            spans.push(Span::raw(name.into_owned()));
        }

        // Zoom modifier
        if self.zoomed {
            spans.push(Span::raw(" Z"));
        }

        // Trailing pad to fill rect_width
        let content_width = spans.iter().fold(0u16, |acc, s| {
            acc.saturating_add(u16::try_from(s.content.width()).unwrap_or(u16::MAX))
        });
        let pad = rect_width.saturating_sub(content_width);
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad as usize)));
        }

        spans
    }
}

// ---------------------------------------------------------------------------
// Chrome builders
// ---------------------------------------------------------------------------

fn tab_status_dot(
    state: crate::detect::AgentState,
    seen: bool,
    mode: TabStatusMode,
    tick: u32,
    palette: &crate::app::state::Palette,
) -> Option<TabStatusDot> {
    use crate::detect::AgentState;

    let visible = match mode {
        TabStatusMode::Off => false,
        TabStatusMode::Attention => matches!(
            (state, seen),
            (AgentState::Blocked, _) | (AgentState::Idle, false)
        ),
        TabStatusMode::All => !matches!(state, AgentState::Unknown),
    };
    if !visible {
        return None;
    }
    let (glyph, style) = super::status::agent_icon(state, seen, tick, palette);
    Some(TabStatusDot { glyph, style })
}

pub(crate) fn build_tab_chromes(
    ws: &crate::workspace::Workspace,
    terminals: &std::collections::HashMap<
        crate::terminal::TerminalId,
        crate::terminal::TerminalState,
    >,
    show_tab_status: TabStatusMode,
    spinner_tick: u32,
    palette: &crate::app::state::Palette,
) -> Vec<TabChrome> {
    let mut chromes: Vec<TabChrome> = Vec::with_capacity(ws.tabs.len());
    let mut attention_count = 0usize;
    let mut dot_count = 0usize;

    for tab_idx in 0..ws.tabs.len() {
        let (chrome, _source) = build_tab_chrome(
            ws,
            tab_idx,
            terminals,
            show_tab_status,
            spinner_tick,
            palette,
        );
        if chrome.status.is_some() {
            dot_count += 1;
        }
        if chrome.is_attention {
            attention_count += 1;
        }
        chromes.push(chrome);
    }

    if !matches!(show_tab_status, TabStatusMode::Off) {
        tracing::debug!(
            visible_tabs = chromes.len(),
            dot_count,
            attention_count,
            "tab chrome built"
        );
    }

    chromes
}

fn build_tab_chrome(
    ws: &crate::workspace::Workspace,
    tab_idx: usize,
    terminals: &std::collections::HashMap<
        crate::terminal::TerminalId,
        crate::terminal::TerminalState,
    >,
    show_tab_status: TabStatusMode,
    spinner_tick: u32,
    palette: &crate::app::state::Palette,
) -> (TabChrome, Option<(crate::detect::AgentState, bool)>) {
    let name = ws
        .tab_display_name(tab_idx)
        .unwrap_or_else(|| (tab_idx + 1).to_string());
    let Some(tab) = ws.tabs.get(tab_idx) else {
        return (
            TabChrome {
                status: None,
                name,
                zoomed: false,
                is_attention: false,
                agent_state: None,
            },
            None,
        );
    };
    let zoomed = tab.zoomed;

    let (status, source) = if matches!(show_tab_status, TabStatusMode::Off) {
        (None, None)
    } else {
        let (state, seen) = crate::app::actions::tab_aggregate_state(tab, terminals);
        let dot = tab_status_dot(state, seen, show_tab_status, spinner_tick, palette);
        let glyph = dot.as_ref().map(|d| d.glyph).unwrap_or("none");
        tracing::trace!(
            tab_idx,
            %name,
            ?state,
            seen,
            ?show_tab_status,
            glyph,
            "tab chrome"
        );
        (dot, Some((state, seen)))
    };

    // Attention iff Blocked or Idle-unseen — the same predicate the status dot
    // and the sidebar rail use. `None` source (TabStatusMode::Off) is never
    // attention.
    let is_attention = source
        .map(|(state, seen)| chrome_is_attention(state, seen))
        .unwrap_or(false);

    (
        TabChrome {
            status,
            name,
            zoomed,
            is_attention,
            agent_state: source,
        },
        source,
    )
}

// ---------------------------------------------------------------------------
// Shared helper for both call sites (compute_view_internal & refresh_tab_bar_view)
// ---------------------------------------------------------------------------

pub(crate) fn build_tab_bar_inputs(
    ws: &crate::workspace::Workspace,
    terminals: &std::collections::HashMap<
        crate::terminal::TerminalId,
        crate::terminal::TerminalState,
    >,
    show_tab_status: TabStatusMode,
    spinner_tick: u32,
    palette: &crate::app::state::Palette,
) -> (Vec<TabChrome>, usize, TabStatusMode) {
    let chromes = build_tab_chromes(ws, terminals, show_tab_status, spinner_tick, palette);
    (chromes, ws.active_tab, show_tab_status)
}

// ---------------------------------------------------------------------------
// TabBarView
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub(crate) struct TabBarView {
    /// Dense per-tab hit rects, length == tab count. Hidden (off-window) tabs
    /// keep `width == 0`; visible tabs round-trip through `tab_at`.
    pub tab_hit_areas: Vec<Rect>,
    pub tab_chrome: Vec<TabChrome>,
    pub tab_status_mode: TabStatusMode,
    /// Collapsed hidden-tab indicators on each overflowing side, with their
    /// clickable/marker rects. `None`/default when nothing overflows.
    pub overflow: TabBarOverflow,
    pub new_tab_hit_area: Rect,
}

/// Canonical "any non-idle state" predicate for a tab's `(state, seen)`:
/// Blocked (awaiting input), Working (in flight), or Idle-unseen (done, not
/// yet looked at). Mirrors `sidebar::is_attention_state` so the tab bar and
/// the sidebar rail agree on which tabs are badge-worthy.
fn chrome_is_attention(state: crate::detect::AgentState, seen: bool) -> bool {
    use crate::detect::AgentState;
    matches!(state, AgentState::Blocked | AgentState::Working)
        || matches!((state, seen), (AgentState::Idle, false))
}

/// Columns an overflow indicator tile needs for `count` hidden tabs whose
/// disaggregated badge breakdown is `side`. The tile owns 2 separator cols
/// (left + right Powerline arrow) plus its interior. Without any badge it is
/// the base `OVERFLOW_INDICATOR_WIDTH` (touch-adequate, NFR5); with badges the
/// interior grows to fit `←+N` plus each non-zero bucket segment. Badges only
/// show under mouse chrome (non-mouse markers are count-only).
fn tab_indicator_width(
    count: usize,
    side: super::overflow::OverflowSide,
    mouse_chrome: bool,
) -> u16 {
    if !mouse_chrome || side.hidden_attention() == 0 {
        return OVERFLOW_INDICATOR_WIDTH;
    }
    // Interior = directional-arrow(1) + "+"(1) + count text + badge columns
    // (`badge_attention_width` already includes a leading space per non-zero
    // bucket segment); plus the 2 separator cols for the tile's Powerline
    // wedges. Count text is measured by display width to match the rest of the
    // tab-bar sizing.
    let count_w = u16::try_from(super::overflow::count_label(count).width()).unwrap_or(u16::MAX);
    let badge_w = super::overflow::badge_attention_width(side);
    let interior = (2 + count_w + badge_w).max(OVERFLOW_INDICATOR_INTERIOR);
    interior
        .saturating_add(TAB_SEPARATOR_OVERHEAD)
        .max(OVERFLOW_INDICATOR_WIDTH)
}

fn tab_width(chrome: &TabChrome, mode: TabStatusMode) -> u16 {
    // Interior (text + " name " padding) + 2 separator cols owned by the tab
    // (left arrow + right arrow under Powerline, equivalent padding under
    // AlternatingBg). Adjacent tabs abut, so there is no extra inter-tab gap.
    chrome
        .display_width(mode)
        .saturating_add(4)
        .saturating_add(TAB_SEPARATOR_OVERHEAD)
        .max(MIN_TAB_WIDTH)
}

/// Trailing x just past the last visible (width>0) tab, for placing the
/// new-tab button. Falls back to `fallback_x` when nothing is visible.
fn trailing_tab_controls_x(tab_hit_areas: &[Rect], fallback_x: u16) -> u16 {
    tab_hit_areas
        .iter()
        .rev()
        .find(|rect| rect.width > 0)
        .map(|rect| rect.x + rect.width)
        .unwrap_or(fallback_x)
}

/// Stateless centered-active batch fill (zellij model).
///
/// Returns the dense per-tab rect vec (len == `chromes.len()`, hidden tabs
/// `width == 0`) plus the first/last visible tab indices. Computed purely from
/// `(active_tab, area)` — no scroll state is carried across frames.
///
/// Algorithm: place the active tab first at its natural width, then grow the
/// visible window one whole tab at a time, alternately preferring the lighter
/// (narrower-so-far) side, until the next tab on a side would not fit the
/// remaining width. `reserve_left`/`reserve_right` reserve columns for the
/// overflow indicators that may sit on each edge; the caller passes the
/// indicator width when mouse/marker chrome is shown. A single active tab wider
/// than the whole bar is clipped to the bar so the row is never blank.
fn centered_active_fill(
    chromes: &[TabChrome],
    active_tab: usize,
    mode: TabStatusMode,
    area: Rect,
    reserve_left: u16,
    reserve_right: u16,
) -> Vec<Rect> {
    let n = chromes.len();
    let mut rects = vec![Rect::default(); n];
    if n == 0 || area.width == 0 || area.height == 0 {
        return rects;
    }
    let active = active_tab.min(n - 1);

    // Total columns the window [lo, hi] would occupy: tab widths (each tab
    // already includes its own separator columns) + an indicator reservation
    // on each side that still hides tabs (a fully-shown side reclaims its
    // reserved columns). Adjacent tabs abut directly, so there is no inter-
    // tab gap.
    let footprint = |lo: usize, hi: usize| -> u16 {
        let mut total: u16 = 0;
        for chrome in &chromes[lo..=hi] {
            total = total.saturating_add(tab_width(chrome, mode));
        }
        if lo > 0 {
            total = total.saturating_add(reserve_left);
        }
        if hi + 1 < n {
            total = total.saturating_add(reserve_right);
        }
        total
    };

    // Window of visible tab indices [lo, hi], grown around the active tab.
    let mut lo = active;
    let mut hi = active;

    loop {
        let can_extend_left = lo > 0;
        let can_extend_right = hi + 1 < n;
        if !can_extend_left && !can_extend_right {
            break;
        }

        // Prefer the lighter side (fewer tabs added so far) for balance; on a
        // tie, extend left first (matches zellij's bias toward earlier tabs).
        let left_count = active - lo;
        let right_count = hi - active;
        let prefer_left = can_extend_left && (left_count <= right_count || !can_extend_right);

        let mut progressed = false;
        if prefer_left {
            if footprint(lo - 1, hi) <= area.width {
                lo -= 1;
                progressed = true;
            } else if can_extend_right && footprint(lo, hi + 1) <= area.width {
                hi += 1;
                progressed = true;
            }
        } else if can_extend_right {
            if footprint(lo, hi + 1) <= area.width {
                hi += 1;
                progressed = true;
            } else if can_extend_left && footprint(lo - 1, hi) <= area.width {
                lo -= 1;
                progressed = true;
            }
        }

        if !progressed {
            break;
        }
    }

    // Lay the window [lo, hi] left-to-right starting at area.x (after the
    // left indicator reservation when present). Adjacent tabs abut directly
    // (each tab carries its own separator cols, no inter-tab gap). A single
    // active tab wider than the bar is clipped to the remaining width so the
    // row is never blank. `footprint` already reserved the right indicator's
    // columns when growing the window, so a fully-fit window never reaches
    // into them; the clip below only bounds a single over-wide tab.
    let left_gutter = if lo > 0 { reserve_left } else { 0 };
    let mut x = area.x.saturating_add(left_gutter);
    let right_limit = area.x.saturating_add(area.width);
    for idx in lo..=hi {
        let desired = tab_width(&chromes[idx], mode);
        let remaining = right_limit.saturating_sub(x);
        if remaining == 0 {
            break;
        }
        let width = desired.min(remaining);
        rects[idx] = Rect::new(x, area.y, width, 1);
        x = x.saturating_add(width);
    }

    rects
}

/// First and last visible (width>0) tab indices in a dense hit-area vec.
fn visible_bounds(rects: &[Rect]) -> Option<(usize, usize)> {
    let first = rects.iter().position(|r| r.width > 0)?;
    let last = rects.iter().rposition(|r| r.width > 0)?;
    Some((first, last))
}

pub(crate) fn compute_tab_bar_view(
    chromes: Vec<TabChrome>,
    active_tab: usize,
    mode: TabStatusMode,
    area: Rect,
    mouse_chrome: bool,
) -> TabBarView {
    if area.width == 0 || area.height == 0 {
        return TabBarView::default();
    }

    let area_right = area.x + area.width;
    // Reserve the new-tab button column only under mouse chrome.
    let new_tab_reserve = if mouse_chrome { NEW_TAB_WIDTH } else { 0 };
    let tabs_area = Rect::new(
        area.x,
        area.y,
        area.width.saturating_sub(new_tab_reserve),
        area.height,
    );

    // First pass: no indicator reservation, to learn whether anything overflows.
    let probe = centered_active_fill(&chromes, active_tab, mode, tabs_area, 0, 0);
    let tab_count = chromes.len();
    let probe_overflow = probe.iter().any(|r| r.width == 0) && tab_count > 0;

    let (tab_hit_areas, overflow) = if !probe_overflow {
        // Everything fits — no indicators.
        (probe, TabBarOverflow::default())
    } else {
        // Build the hidden groups for a given fill result, reusing the shared
        // `overflow::side_above`/`side_below` so the tab bar and the sidebar
        // surfaces classify hidden items into the same Working/Blocked/Done-
        // unseen buckets with the same single-walk, nearest-to-edge semantics.
        // The walk covers only the hidden ranges [0..first) and (last..n) — no
        // second full-tab scan. A tab with no agent_state (TabStatusMode::Off)
        // classifies as Unknown, which `AttnBucket::classify` treats as
        // non-badge-worthy.
        let state_of = |i: usize| {
            chromes
                .get(i)
                .and_then(|c| c.agent_state)
                .unwrap_or((crate::detect::AgentState::Unknown, true))
        };
        let groups_for = |rects: &[Rect]| -> (Option<HiddenGroup>, Option<HiddenGroup>) {
            let (first, last) = visible_bounds(rects).unwrap_or((active_tab, active_tab));
            let left_hidden = first;
            let right_hidden = tab_count.saturating_sub(last + 1);

            let window = super::overflow::ListWindow {
                first,
                count: last.saturating_sub(first) + 1,
                hidden_above: left_hidden,
                hidden_below: right_hidden,
            };
            let left = (left_hidden > 0).then(|| {
                let side = super::overflow::side_above(window, state_of);
                HiddenGroup {
                    count: left_hidden,
                    jump_to: first.saturating_sub(1),
                    side,
                }
            });
            let right = (right_hidden > 0).then(|| {
                let side = super::overflow::side_below(window, tab_count, state_of);
                HiddenGroup {
                    count: right_hidden,
                    jump_to: last + 1,
                    side,
                }
            });
            (left, right)
        };

        // First reserve the base indicator width on each side. We don't yet know
        // which sides overflow, so reserve on both; the fill's "last hidden tab
        // removes the indicator" logic reclaims a side that ends up fully shown.
        let mut rects = centered_active_fill(
            &chromes,
            active_tab,
            mode,
            tabs_area,
            OVERFLOW_INDICATOR_WIDTH,
            OVERFLOW_INDICATOR_WIDTH,
        );
        let (mut left, mut right) = groups_for(&rects);

        // An attention badge widens the indicator past the base reservation. The
        // fill's per-side reserve must equal the rendered/hit-tested width or a
        // widened indicator overlaps the adjacent visible tab (and a click there
        // mis-navigates). Iterate reserve = badge-aware-width until the reserve
        // fed to the fill matches the width the resulting groups demand — so
        // reserved-width == hit-width by construction. Bounded: widening only
        // hides more tabs (monotonic), so this converges in a few passes; cap at
        // 4 as a backstop. `left_w`/`right_w` always hold the reserves that
        // produced the final `rects`/`left`/`right`.
        let want_w = |g: &Option<HiddenGroup>| {
            g.map(|g| tab_indicator_width(g.count, g.side, mouse_chrome))
                .unwrap_or(0)
        };
        let mut left_w = OVERFLOW_INDICATOR_WIDTH;
        let mut right_w = OVERFLOW_INDICATOR_WIDTH;
        for _ in 0..4 {
            let need_left = want_w(&left).max(if left.is_some() {
                OVERFLOW_INDICATOR_WIDTH
            } else {
                0
            });
            let need_right = want_w(&right).max(if right.is_some() {
                OVERFLOW_INDICATOR_WIDTH
            } else {
                0
            });
            if need_left == left_w && need_right == right_w {
                break;
            }
            left_w = need_left;
            right_w = need_right;
            rects = centered_active_fill(
                &chromes,
                active_tab,
                mode,
                tabs_area,
                left_w.max(OVERFLOW_INDICATOR_WIDTH),
                right_w.max(OVERFLOW_INDICATOR_WIDTH),
            );
            let (l2, r2) = groups_for(&rects);
            left = l2;
            right = r2;
        }

        // Indicator rects occupy the columns the final fill reserved for them.
        // The left indicator hugs area.x; the right sits at the right edge of
        // the tabs area (before any new-tab button). At pathologically narrow
        // widths the active tab (always visible) can consume more than the
        // reserve left for an indicator, so clamp each indicator away from the
        // visible tabs: the left indicator's right edge must not pass the first
        // visible tab, and the right indicator's left edge must not precede the
        // last visible tab. The indicator content clips if the clamp shrinks it.
        let (vis_first, vis_last) = visible_bounds(&rects).unwrap_or((active_tab, active_tab));
        let first_visible_x = rects.get(vis_first).map(|r| r.x).unwrap_or(area.x);
        let last_visible_right = rects.get(vis_last).map(|r| r.x + r.width).unwrap_or(area.x);

        let left_hit_area = if left.is_some() {
            let lw = left_w
                .min(area.width)
                .min(first_visible_x.saturating_sub(area.x));
            Rect::new(area.x, area.y, lw, 1)
        } else {
            Rect::default()
        };
        let right_hit_area = if right.is_some() {
            let tabs_right = tabs_area.x + tabs_area.width;
            let rx = tabs_right
                .saturating_sub(right_w)
                .max(area.x)
                .max(last_visible_right);
            Rect::new(rx, area.y, tabs_right.saturating_sub(rx), 1)
        } else {
            Rect::default()
        };

        (
            rects,
            TabBarOverflow {
                left,
                right,
                left_hit_area,
                right_hit_area,
            },
        )
    };

    let new_tab_hit_area = if mouse_chrome {
        // Place the new-tab button just past the rightmost chrome: either the
        // last visible tab, or the right overflow indicator when present.
        let trailing = trailing_tab_controls_x(&tab_hit_areas, tabs_area.x).max(
            overflow
                .right_hit_area
                .x
                .saturating_add(overflow.right_hit_area.width),
        );
        let new_tab_x = trailing.min(area_right.saturating_sub(NEW_TAB_WIDTH).max(area.x));
        Rect::new(
            new_tab_x,
            area.y,
            area_right.saturating_sub(new_tab_x).min(NEW_TAB_WIDTH),
            1,
        )
    } else {
        Rect::default()
    };

    tracing::debug!(
        area_width = area.width,
        tab_count,
        ?mode,
        active_tab,
        left_hidden = overflow.left.map(|g| g.count).unwrap_or(0),
        right_hidden = overflow.right.map(|g| g.count).unwrap_or(0),
        visible_count = tab_hit_areas.iter().filter(|r| r.width > 0).count(),
        "tab bar overflow"
    );

    TabBarView {
        tab_hit_areas,
        tab_chrome: chromes,
        tab_status_mode: mode,
        overflow,
        new_tab_hit_area,
    }
}

fn tab_drop_indicator_x(
    app: &AppState,
    ws: &crate::workspace::Workspace,
    insert_idx: usize,
) -> Option<u16> {
    let mut visible_tabs = app
        .view
        .tab_hit_areas
        .iter()
        .enumerate()
        .filter(|(_, rect)| rect.width > 0);
    let first_visible = visible_tabs.clone().next()?;
    let last_visible = visible_tabs.next_back().unwrap_or(first_visible);
    let overflow = &app.view.tab_overflow;

    if insert_idx == 0 {
        // When the left edge is clipped, anchor on the left overflow indicator
        // (just past it) rather than the first visible tab, which is not tab 0.
        return Some(if first_visible.0 == 0 {
            first_visible.1.x
        } else {
            overflow.left_hit_area.x + overflow.left_hit_area.width
        });
    }

    if let Some((_, rect)) = app
        .view
        .tab_hit_areas
        .iter()
        .enumerate()
        .find(|(idx, rect)| *idx == insert_idx && rect.width > 0)
    {
        return Some(rect.x.saturating_sub(1));
    }

    if insert_idx >= ws.tabs.len() {
        // When the right edge is clipped, anchor just before the right overflow
        // indicator rather than after the last visible tab.
        return Some(if last_visible.0 + 1 >= ws.tabs.len() {
            last_visible.1.x + last_visible.1.width
        } else {
            overflow.right_hit_area.x.saturating_sub(1)
        });
    }

    None
}

/// Background color for a visible tab, used both to paint the tab and to color
/// the Powerline arrow transition. Under `SeparatorStyle::Powerline` (default),
/// all inactive tabs share a uniform `surface0` background — the arrow glyph
/// provides visual separation. Under `AlternatingBg`, inactive tabs alternate
/// `surface0`/`surface1` by index (the only separation without arrows). The
/// active tab always uses the accent.
fn tab_bg(
    p: &crate::app::state::Palette,
    idx: usize,
    active: bool,
    separator: SeparatorStyle,
) -> ratatui::style::Color {
    // Under Powerline, the inactive tab bg must differ from panel_bg or the
    // per-tile arrows would collapse to fg=bg. Fall back to `surface_dim` when
    // `surface0 == panel_bg` (low-color palettes where both default to Reset).
    let inactive_bg = if separator == SeparatorStyle::Powerline && p.surface0 == p.panel_bg {
        p.surface_dim
    } else {
        p.surface0
    };
    if active {
        p.accent
    } else if separator == SeparatorStyle::AlternatingBg {
        if idx.is_multiple_of(2) {
            p.surface0
        } else {
            p.surface1
        }
    } else {
        inactive_bg
    }
}

/// Render an overflow indicator as a proper zellij tile: a left Powerline
/// arrow (panel→tile), the interior spans on `tile_bg`, and a right Powerline
/// arrow (tile→panel) — identical chrome to a regular inactive tab. Under
/// `AlternatingBg` (or when the rect is too narrow for arrows) the interior
/// fills the whole rect and no Powerline codepoint is emitted.
#[allow(clippy::too_many_arguments)]
fn render_overflow_tile(
    frame: &mut Frame,
    area: Rect,
    rect: Rect,
    interior: Vec<Span<'static>>,
    tile_bg: ratatui::style::Color,
    separator: SeparatorStyle,
    panel_bg: ratatui::style::Color,
) {
    let area_right = area.x + area.width;
    if separator == SeparatorStyle::Powerline && rect.width >= TAB_SEPARATOR_OVERHEAD {
        // Left arrow: panel_bg → tile_bg.
        if rect.x < area_right {
            frame.buffer_mut()[(rect.x, area.y)]
                .set_symbol(POWERLINE_ARROW)
                .set_style(Style::default().fg(panel_bg).bg(tile_bg));
        }
        // Right arrow: tile_bg → panel_bg.
        let right_x = rect.x + rect.width - 1;
        if right_x < area_right {
            frame.buffer_mut()[(right_x, area.y)]
                .set_symbol(POWERLINE_ARROW)
                .set_style(Style::default().fg(tile_bg).bg(panel_bg));
        }
        // Interior between the two arrows.
        let interior_w = rect.width.saturating_sub(2);
        if interior_w > 0 {
            let interior_rect = Rect::new(rect.x + 1, area.y, interior_w, 1);
            frame.render_widget(
                Paragraph::new(Line::from(interior)).style(Style::default().bg(tile_bg)),
                interior_rect,
            );
        }
    } else {
        frame.render_widget(
            Paragraph::new(Line::from(interior)).style(Style::default().bg(tile_bg)),
            rect,
        );
    }
}

pub(super) fn render_tab_bar(app: &AppState, frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let Some(active_ws_idx) = app.active else {
        return;
    };
    let Some(ws) = app.workspaces.get(active_ws_idx) else {
        return;
    };

    let p = &app.palette;
    let separator = if app.tabs_powerline {
        SeparatorStyle::Powerline
    } else {
        SeparatorStyle::AlternatingBg
    };

    frame.render_widget(
        Paragraph::new(" ".repeat(area.width as usize)).style(Style::default().bg(p.panel_bg)),
        area,
    );

    let overflow = &app.view.tab_overflow;

    // Inactive-tile bg for overflow indicators. Falls back to surface_dim when
    // surface0 == panel_bg so the Powerline arrows stay visible (same rule as
    // `tab_bg`).
    let indicator_tile_bg = if separator == SeparatorStyle::Powerline && p.surface0 == p.panel_bg {
        p.surface_dim
    } else {
        p.surface0
    };

    // Overflow indicators render as proper zellij tiles (left arrow + interior
    // + right arrow), matching a regular inactive tab. Under mouse chrome the
    // interior is a clickable `←+N` affordance plus up to three disaggregated
    // Blocked/Working/Done-unseen badge segments; without mouse chrome it is a
    // non-clickable `← N` / `N →` marker (count only). Counts cap at `9+`.
    let fmt_count = |n: usize| -> String {
        if n > 9 {
            "9+".to_string()
        } else {
            n.to_string()
        }
    };
    let interior_text_style = Style::default().fg(p.overlay1).bg(indicator_tile_bg);
    // Re-tint the shared `badge_spans` (which uses transparent bg) onto the
    // tile bg so the badge sits on the indicator tile, not the panel.
    let on_tile = |spans: Vec<Span<'static>>| -> Vec<Span<'static>> {
        spans
            .into_iter()
            .map(|s| {
                let style = s.style.bg(indicator_tile_bg);
                Span::styled(s.content, style)
            })
            .collect()
    };
    if let Some(group) = overflow.left {
        if overflow.left_hit_area.width > 0 {
            let mut interior: Vec<Span<'static>> = Vec::new();
            if app.mouse_capture {
                // `←` + the shared badge (`+N` plus bucket segments), re-tinted.
                interior.push(Span::styled("←", interior_text_style));
                interior.extend(on_tile(super::overflow::badge_spans(group.side, p)));
            } else {
                interior.push(Span::styled(
                    format!("←{} ", fmt_count(group.count)),
                    interior_text_style,
                ));
            }
            render_overflow_tile(
                frame,
                area,
                overflow.left_hit_area,
                interior,
                indicator_tile_bg,
                separator,
                p.panel_bg,
            );
        }
    }
    if let Some(group) = overflow.right {
        if overflow.right_hit_area.width > 0 {
            let mut interior: Vec<Span<'static>> = Vec::new();
            if app.mouse_capture {
                // The shared badge then a trailing `→`.
                interior.extend(on_tile(super::overflow::badge_spans(group.side, p)));
                interior.push(Span::styled("→", interior_text_style));
            } else {
                interior.push(Span::styled(
                    format!(" {}→", fmt_count(group.count)),
                    interior_text_style,
                ));
            }
            render_overflow_tile(
                frame,
                area,
                overflow.right_hit_area,
                interior,
                indicator_tile_bg,
                separator,
                p.panel_bg,
            );
        }
    }

    // Each visible tab owns its own left + right Powerline arrows. Both arrows
    // blend against `panel_bg` (NOT against the adjacent tab's bg) — this is
    // the zellij convention: adjacent tabs each paint a wedge back-to-back,
    // giving the signature "two facing arrows" look instead of a single
    // transition glyph. Under `SeparatorStyle::AlternatingBg`, the two
    // separator cols become extra tab-bg padding so the banding still
    // visually separates adjacent tabs without any Powerline codepoint.
    let panel_bg = p.panel_bg;

    for (idx, tab) in ws.tabs.iter().enumerate() {
        let Some(rect) = app.view.tab_hit_areas.get(idx).copied() else {
            break;
        };
        if rect.width == 0 {
            continue;
        }
        let active = idx == ws.active_tab;
        let bg = tab_bg(p, idx, active, separator);
        let interior_style = if active {
            let base = Style::default().fg(panel_contrast_fg(p)).bg(bg);
            if tab.is_auto_named() {
                base.add_modifier(Modifier::DIM)
            } else {
                base.add_modifier(Modifier::BOLD)
            }
        } else if tab.is_auto_named() {
            Style::default()
                .fg(p.overlay0)
                .bg(bg)
                .add_modifier(Modifier::DIM)
        } else {
            Style::default().fg(p.overlay1).bg(bg)
        };

        let area_right = area.x + area.width;
        if separator == SeparatorStyle::Powerline && rect.width >= TAB_SEPARATOR_OVERHEAD {
            // Left arrow: fg = panel_bg, bg = tab bg (panel→tab wedge)
            if rect.x < area_right {
                frame.buffer_mut()[(rect.x, area.y)]
                    .set_symbol(POWERLINE_ARROW)
                    .set_style(Style::default().fg(panel_bg).bg(bg));
            }
            // Right arrow: fg = tab bg, bg = panel_bg (tab→panel wedge)
            let right_x = rect.x + rect.width - 1;
            if right_x < area_right {
                frame.buffer_mut()[(right_x, area.y)]
                    .set_symbol(POWERLINE_ARROW)
                    .set_style(Style::default().fg(bg).bg(panel_bg));
            }
            // Interior: between the two arrows.
            let interior_x = rect.x + 1;
            let interior_w = rect.width.saturating_sub(2);
            if interior_w > 0 {
                let interior_rect = Rect::new(interior_x, area.y, interior_w, 1);
                let spans = if let Some(chrome) = app.view.tab_chrome.get(idx) {
                    chrome.to_spans(app.view.tab_status_mode, interior_w)
                } else {
                    vec![Span::raw(" ".repeat(interior_w as usize))]
                };
                frame.render_widget(
                    Paragraph::new(Line::from(spans)).style(interior_style),
                    interior_rect,
                );
            }
        } else {
            // AlternatingBg path (or rect too narrow for arrows): full-width
            // interior with tab bg. The 2 separator cols become extra padding.
            let spans = if let Some(chrome) = app.view.tab_chrome.get(idx) {
                chrome.to_spans(app.view.tab_status_mode, rect.width)
            } else {
                vec![Span::raw(" ".repeat(rect.width as usize))]
            };
            frame.render_widget(
                Paragraph::new(Line::from(spans)).style(interior_style),
                rect,
            );
        }
    }

    if let Some(crate::app::state::DragState {
        target:
            crate::app::state::DragTarget::TabReorder {
                ws_idx,
                insert_idx: Some(insert_idx),
                ..
            },
    }) = &app.drag
    {
        if *ws_idx == active_ws_idx {
            if let Some(x) = tab_drop_indicator_x(app, ws, *insert_idx) {
                frame.buffer_mut()[(x.min(area.x + area.width.saturating_sub(1)), area.y)]
                    .set_symbol("│")
                    .set_style(Style::default().fg(p.accent));
            }
        }
    }

    if app.mouse_capture && app.view.new_tab_hit_area.width > 0 {
        frame.render_widget(
            Paragraph::new(" + ").style(Style::default().fg(p.overlay1)),
            app.view.new_tab_hit_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AppState;
    use crate::workspace::Workspace;
    use ratatui::{backend::TestBackend, Terminal};

    fn buffer_row_text(buffer: &ratatui::buffer::Buffer, area: Rect, row: u16) -> String {
        (area.x..area.x + area.width)
            .map(|x| buffer[(x, row)].symbol())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    fn make_ws_with_tabs(names: &[&str]) -> crate::workspace::Workspace {
        assert!(
            !names.is_empty(),
            "make_ws_with_tabs requires at least one tab"
        );
        let mut ws = Workspace::test_new("test");
        ws.tabs[0].set_custom_name(names[0].to_string());
        for &name in &names[1..] {
            ws.test_add_tab(Some(name));
        }
        ws
    }

    fn chromes_from_ws(ws: &crate::workspace::Workspace) -> Vec<TabChrome> {
        (0..ws.tabs.len())
            .map(|i| {
                let name = ws
                    .tab_display_name(i)
                    .unwrap_or_else(|| (i + 1).to_string());
                let zoomed = ws.tabs.get(i).is_some_and(|tab| tab.zoomed);
                TabChrome {
                    status: None,
                    name,
                    zoomed,
                    is_attention: false,
                    agent_state: None,
                }
            })
            .collect()
    }

    fn chromes_from_names(names: &[&str]) -> Vec<TabChrome> {
        names
            .iter()
            .map(|&name| TabChrome {
                status: None,
                name: name.to_string(),
                zoomed: false,
                is_attention: false,
                agent_state: None,
            })
            .collect()
    }

    /// Build chromes with explicit attention flags (indices in `attention` are
    /// Blocked-and-seen so they count as badge-worthy). Used by the FR8
    /// hidden-attention tests.
    fn chromes_with_attention(names: &[&str], attention: &[usize]) -> Vec<TabChrome> {
        names
            .iter()
            .enumerate()
            .map(|(i, &name)| {
                let attn = attention.contains(&i);
                TabChrome {
                    status: None,
                    name: name.to_string(),
                    zoomed: false,
                    is_attention: attn,
                    agent_state: attn.then_some((crate::detect::AgentState::Blocked, true)),
                }
            })
            .collect()
    }

    /// Build chromes with explicit per-index agent states. Used by the
    /// disaggregated-bucket tests.
    fn chromes_with_states(
        names: &[&str],
        states: &[(usize, crate::detect::AgentState, bool)],
    ) -> Vec<TabChrome> {
        names
            .iter()
            .enumerate()
            .map(|(i, &name)| {
                let st = states
                    .iter()
                    .find(|(idx, ..)| *idx == i)
                    .map(|(_, s, seen)| (*s, *seen));
                TabChrome {
                    status: None,
                    name: name.to_string(),
                    zoomed: false,
                    is_attention: st.is_some_and(|(s, seen)| chrome_is_attention(s, seen)),
                    agent_state: st,
                }
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // Stateless centered-active fill: visibility, overflow, jump targets
    // -----------------------------------------------------------------------

    #[test]
    fn all_tabs_visible_when_they_fit() {
        let chromes = chromes_from_names(&["ab", "cd", "ef"]);
        let view =
            compute_tab_bar_view(chromes, 0, TabStatusMode::Off, Rect::new(0, 0, 60, 1), true);
        assert!(view.tab_hit_areas.iter().all(|r| r.width > 0));
        assert_eq!(view.overflow, TabBarOverflow::default());
    }

    #[test]
    fn active_tab_visible_at_full_width_with_many_tabs() {
        // 10 tabs, narrow bar, active in the middle: active must be visible at
        // its full natural width, with overflow indicators on both sides.
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_from_names(&refs);
        let active = 5;
        let view = compute_tab_bar_view(
            chromes.clone(),
            active,
            TabStatusMode::Off,
            Rect::new(0, 0, 40, 1),
            true,
        );
        let natural = tab_width(&chromes[active], TabStatusMode::Off);
        assert!(view.tab_hit_areas[active].width > 0);
        assert_eq!(
            view.tab_hit_areas[active].width, natural,
            "active tab must render at full natural width"
        );
        assert!(view.overflow.left.is_some());
        assert!(view.overflow.right.is_some());
    }

    #[test]
    fn overflow_jump_targets_point_at_hidden_tabs_in_range() {
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_from_names(&refs);
        let view =
            compute_tab_bar_view(chromes, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
        let n = view.tab_hit_areas.len();
        let left = view.overflow.left.expect("left overflow");
        let right = view.overflow.right.expect("right overflow");
        assert!(left.jump_to < n);
        assert!(right.jump_to < n);
        // Jump targets are hidden (width 0) — the nearest hidden tab each side.
        assert_eq!(view.tab_hit_areas[left.jump_to].width, 0);
        assert_eq!(view.tab_hit_areas[right.jump_to].width, 0);
        // Counts add up: left_hidden + visible + right_hidden == tab count.
        let visible = view.tab_hit_areas.iter().filter(|r| r.width > 0).count();
        assert_eq!(left.count + visible + right.count, n);
    }

    #[test]
    fn active_first_tab_only_overflows_right() {
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_from_names(&refs);
        let view =
            compute_tab_bar_view(chromes, 0, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
        assert!(view.tab_hit_areas[0].width > 0);
        assert!(view.overflow.left.is_none(), "no hidden tabs to the left");
        assert!(view.overflow.right.is_some());
    }

    #[test]
    fn active_last_tab_only_overflows_left() {
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_from_names(&refs);
        let last = 9;
        let view = compute_tab_bar_view(
            chromes,
            last,
            TabStatusMode::Off,
            Rect::new(0, 0, 40, 1),
            true,
        );
        assert!(view.tab_hit_areas[last].width > 0);
        assert!(view.overflow.left.is_some());
        assert!(view.overflow.right.is_none(), "no hidden tabs to the right");
    }

    // -----------------------------------------------------------------------
    // FR8: attention-aware tab overflow
    // -----------------------------------------------------------------------

    #[test]
    fn build_tab_chromes_sets_is_attention_from_state() {
        use crate::detect::AgentState;
        // zellij-fidelity round 2: the predicate is "any non-idle state".
        // Blocked, Working, and Idle-unseen are attention; Idle-seen and
        // Unknown are not.
        assert!(chrome_is_attention(AgentState::Blocked, true));
        assert!(chrome_is_attention(AgentState::Working, false));
        assert!(chrome_is_attention(AgentState::Working, true));
        assert!(chrome_is_attention(AgentState::Idle, false));
        assert!(!chrome_is_attention(AgentState::Idle, true));
        assert!(!chrome_is_attention(AgentState::Unknown, false));
    }

    #[test]
    fn hidden_group_reports_attention_count_and_nearest_each_side() {
        // 10 tabs, active 5. Mark attention tabs on each hidden side.
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        // Left hidden range includes 1 (attention); right hidden includes 8 (attention).
        let chromes = chromes_with_attention(&refs, &[1, 8]);
        let view =
            compute_tab_bar_view(chromes, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
        let left = view.overflow.left.expect("left overflow");
        let right = view.overflow.right.expect("right overflow");

        // Left: attention tab 1 is hidden and counted (chromes_with_attention
        // marks them Blocked); nearest-to-edge is the rightmost blocked index
        // in the hidden left range.
        assert!(left.side.hidden_attention() >= 1);
        assert_eq!(left.side.blocked_jump_to, Some(1));
        // The attention jump target is actually hidden.
        assert_eq!(
            view.tab_hit_areas[left.side.blocked_jump_to.unwrap()].width,
            0
        );

        assert!(right.side.hidden_attention() >= 1);
        assert_eq!(right.side.blocked_jump_to, Some(8));
        assert_eq!(
            view.tab_hit_areas[right.side.blocked_jump_to.unwrap()].width,
            0
        );
    }

    #[test]
    fn hidden_group_no_attention_means_none() {
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        // No attention anywhere.
        let chromes = chromes_with_attention(&refs, &[]);
        let view =
            compute_tab_bar_view(chromes, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
        let left = view.overflow.left.expect("left overflow");
        let right = view.overflow.right.expect("right overflow");
        assert_eq!(left.side.hidden_attention(), 0);
        assert_eq!(left.side.blocked_jump_to, None);
        assert_eq!(right.side.hidden_attention(), 0);
        assert_eq!(right.side.blocked_jump_to, None);
    }

    #[test]
    fn tab_overflow_counts_working_blocked_done_unseen_in_one_walk() {
        use crate::detect::AgentState::*;
        // 12 tabs, active 6. Left hidden range [0..6): blocked@1, working@2,
        // idle-unseen@4. Right hidden range (last..12): blocked@9, working@10.
        let names: Vec<String> = (0..12).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_with_states(
            &refs,
            &[
                (1, Blocked, true),
                (2, Working, false),
                (4, Idle, false),
                (9, Blocked, true),
                (10, Working, false),
            ],
        );
        let view =
            compute_tab_bar_view(chromes, 6, TabStatusMode::Off, Rect::new(0, 0, 36, 1), true);
        let left = view.overflow.left.expect("left overflow").side;
        let right = view.overflow.right.expect("right overflow").side;
        assert_eq!(left.hidden_blocked, 1, "left blocked");
        assert_eq!(left.hidden_working, 1, "left working");
        assert_eq!(left.hidden_done_unseen, 1, "left done-unseen");
        // ABOVE keeps highest index per bucket.
        assert_eq!(left.blocked_jump_to, Some(1));
        assert_eq!(left.working_jump_to, Some(2));
        assert_eq!(left.done_unseen_jump_to, Some(4));
        assert_eq!(right.hidden_blocked, 1, "right blocked");
        assert_eq!(right.hidden_working, 1, "right working");
        // BELOW keeps lowest index per bucket.
        assert_eq!(right.blocked_jump_to, Some(9));
        assert_eq!(right.working_jump_to, Some(10));
    }

    #[test]
    fn tab_overflow_click_priority_blocked_over_working() {
        use crate::detect::AgentState::*;
        // Left hidden range has a working tab (index 1) and a blocked tab
        // (index 3); resolve_jump must prefer the blocked one.
        let names: Vec<String> = (0..12).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_with_states(&refs, &[(1, Working, false), (3, Blocked, true)]);
        let view =
            compute_tab_bar_view(chromes, 7, TabStatusMode::Off, Rect::new(0, 0, 36, 1), true);
        let left = view.overflow.left.expect("left overflow");
        let mut side = left.side;
        side.jump_to = left.jump_to;
        assert_eq!(
            crate::ui::overflow::resolve_jump(side),
            Some(3),
            "blocked outranks working for the jump target"
        );
    }

    #[test]
    fn attention_badge_widens_indicator_hit_zone() {
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        // Several attention tabs hidden left so the badge shows a count.
        let with = chromes_with_attention(&refs, &[0, 1, 2]);
        let view_with =
            compute_tab_bar_view(with, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
        let without = chromes_from_names(&refs);
        let view_without =
            compute_tab_bar_view(without, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
        assert!(
            view_with.overflow.left_hit_area.width > view_without.overflow.left_hit_area.width,
            "attention badge must widen the indicator past the plain-count width"
        );
        // Still touch-adequate.
        assert!(view_with.overflow.left_hit_area.width >= OVERFLOW_INDICATOR_WIDTH);
    }

    #[test]
    fn indicator_hit_areas_never_overlap_visible_tabs() {
        // Gate-3 regression: the badge-aware indicator reserve must equal the
        // rendered/hit width, or a widened indicator overlaps the adjacent tab
        // and a click there mis-navigates. Sweep attention layouts (including
        // asymmetric ones where only one side widens) and bar widths, asserting
        // each indicator hit-area is disjoint from every visible tab rect.
        let names: Vec<String> = (0..12).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let attention_sets: &[&[usize]] = &[
            &[0, 1, 2],        // left-heavy
            &[9, 10, 11],      // right-heavy
            &[0, 1, 2, 9, 10], // both sides
            &[1],              // single left
            &[10],             // single right
        ];
        for set in attention_sets {
            for active in [4usize, 5, 6] {
                for width in [28u16, 34, 40, 46] {
                    let chromes = chromes_with_attention(&refs, set);
                    let view = compute_tab_bar_view(
                        chromes,
                        active,
                        TabStatusMode::Off,
                        Rect::new(0, 0, width, 1),
                        true,
                    );
                    for ind in [view.overflow.left_hit_area, view.overflow.right_hit_area] {
                        if ind.width == 0 {
                            continue;
                        }
                        let ind_lo = ind.x;
                        let ind_hi = ind.x + ind.width;
                        for rect in view.tab_hit_areas.iter().filter(|r| r.width > 0) {
                            let lo = rect.x;
                            let hi = rect.x + rect.width;
                            assert!(
                                ind_hi <= lo || hi <= ind_lo,
                                "indicator [{ind_lo},{ind_hi}) overlaps tab [{lo},{hi}) \
                                 set={set:?} active={active} width={width}",
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn attention_count_walks_only_hidden_ranges() {
        // An attention tab inside the VISIBLE window must not be counted by the
        // hidden-attention tally (the walk is bounded to hidden ranges).
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_with_attention(&refs, &[5]); // active tab itself
        let view =
            compute_tab_bar_view(chromes, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
        let left = view
            .overflow
            .left
            .map(|g| g.side.hidden_attention())
            .unwrap_or(0);
        let right = view
            .overflow
            .right
            .map(|g| g.side.hidden_attention())
            .unwrap_or(0);
        assert_eq!(left + right, 0, "visible attention tab is not a hidden one");
    }

    #[test]
    fn indicator_hit_zone_is_touch_adequate() {
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_from_names(&refs);
        let view =
            compute_tab_bar_view(chromes, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
        assert!(
            view.overflow.left_hit_area.width >= 3,
            "left indicator must be a touch-adequate hit zone, not 1 cell"
        );
        assert!(
            view.overflow.right_hit_area.width >= 1,
            "right indicator clipped only by the bar edge"
        );
    }

    #[test]
    fn dense_hit_area_len_equals_tab_count() {
        let names: Vec<String> = (0..12).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        for active in [0, 6, 11] {
            for width in 20..60 {
                let chromes = chromes_from_names(&refs);
                let view = compute_tab_bar_view(
                    chromes,
                    active,
                    TabStatusMode::Off,
                    Rect::new(0, 0, width, 1),
                    true,
                );
                assert_eq!(view.tab_hit_areas.len(), refs.len());
                assert!(
                    view.tab_hit_areas[active].width > 0,
                    "active={active} width={width}: active tab hidden"
                );
            }
        }
    }

    #[test]
    fn compute_tab_bar_view_returns_default_for_zero_width_area() {
        let chromes = chromes_from_names(&["aa", "bb"]);
        let view =
            compute_tab_bar_view(chromes, 0, TabStatusMode::Off, Rect::new(0, 0, 0, 1), true);
        assert!(view.tab_hit_areas.is_empty());
        assert_eq!(view.overflow, TabBarOverflow::default());
        assert_eq!(view.new_tab_hit_area.width, 0);
    }

    #[test]
    fn non_mouse_branch_uses_centered_fill_and_no_clickable_chrome() {
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_from_names(&refs);
        let view = compute_tab_bar_view(
            chromes,
            5,
            TabStatusMode::Off,
            Rect::new(0, 0, 40, 1),
            false,
        );
        // Active tab still placed via the centered fill.
        assert!(view.tab_hit_areas[5].width > 0);
        // Overflow groups still computed (for the non-clickable marker render).
        assert!(view.overflow.left.is_some());
        assert!(view.overflow.right.is_some());
        // No new-tab button without mouse chrome.
        assert_eq!(view.new_tab_hit_area.width, 0);
    }

    #[test]
    fn position_index_round_trip_for_visible_tabs() {
        // Every visible rect's center maps back to its own tab index via the
        // same predicate `tab_at` uses.
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_from_names(&refs);
        let view =
            compute_tab_bar_view(chromes, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
        for (idx, rect) in view.tab_hit_areas.iter().enumerate() {
            if rect.width == 0 {
                continue;
            }
            let center = rect.x + rect.width / 2;
            let found = view
                .tab_hit_areas
                .iter()
                .enumerate()
                .find(|(_, r)| r.width > 0 && center >= r.x && center < r.x + r.width)
                .map(|(i, _)| i);
            assert_eq!(found, Some(idx), "tab {idx} center did not round-trip");
        }
    }

    #[test]
    fn round_trip_with_adversarial_unicode_names() {
        // CJK (wide), combining, and ZWJ emoji names: the fill uses Unicode
        // display width, so visible rects must still round-trip cleanly.
        let chromes = chromes_from_names(&[
            "你好世界",
            "e\u{0301}ditor",
            "👨\u{200d}💻",
            "tab-d",
            "넓은탭이름",
            "f",
            "g",
            "h",
        ]);
        for active in [0, 3, 7] {
            let view = compute_tab_bar_view(
                chromes.clone(),
                active,
                TabStatusMode::Off,
                Rect::new(0, 0, 30, 1),
                true,
            );
            assert_eq!(view.tab_hit_areas.len(), 8);
            assert!(view.tab_hit_areas[active].width > 0);
            for (idx, rect) in view.tab_hit_areas.iter().enumerate() {
                if rect.width == 0 {
                    continue;
                }
                let center = rect.x + rect.width / 2;
                let found = view
                    .tab_hit_areas
                    .iter()
                    .enumerate()
                    .find(|(_, r)| r.width > 0 && center >= r.x && center < r.x + r.width)
                    .map(|(i, _)| i);
                assert_eq!(found, Some(idx), "active={active}: tab {idx} round-trip");
            }
        }
    }

    #[test]
    fn width_accounting_at_overflow_boundary() {
        // Sweep widths around the fit/overflow boundary; the active tab is never
        // hidden and the visible window is always a contiguous run.
        let names: Vec<String> = (0..6).map(|i| format!("name{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        for width in 12..70 {
            let chromes = chromes_from_names(&refs);
            let view = compute_tab_bar_view(
                chromes,
                3,
                TabStatusMode::Off,
                Rect::new(0, 0, width, 1),
                true,
            );
            assert!(
                view.tab_hit_areas[3].width > 0,
                "width={width}: active hidden"
            );
            // Visible indices form a contiguous range.
            let visible: Vec<usize> = view
                .tab_hit_areas
                .iter()
                .enumerate()
                .filter(|(_, r)| r.width > 0)
                .map(|(i, _)| i)
                .collect();
            if let (Some(&first), Some(&last)) = (visible.first(), visible.last()) {
                assert_eq!(
                    visible.len(),
                    last - first + 1,
                    "width={width}: visible window not contiguous: {visible:?}"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // TabChrome: width + spans + Unicode + sanitization
    // -----------------------------------------------------------------------

    #[test]
    fn zoom_marker_counts_toward_tab_width() {
        // tab_width = display_width (8 name + 2 zoom) + 4 interior padding
        //           + 2 separator overhead (zellij left+right arrow cols).
        let chrome = TabChrome {
            status: None,
            name: "abcdefgh".into(),
            zoomed: true,
            is_attention: false,
            agent_state: None,
        };
        assert_eq!(tab_width(&chrome, TabStatusMode::Off), 16);
    }

    #[test]
    fn display_width_uses_unicode_display_columns() {
        // CJK glyphs are 2 columns each.
        let c = TabChrome {
            status: None,
            name: "你好".into(),
            zoomed: false,
            is_attention: false,
            agent_state: None,
        };
        assert_eq!(c.display_width(TabStatusMode::Off), 4);

        // Combining mark contributes 0 columns: "a" + U+0300 = 1 column.
        let c = TabChrome {
            status: None,
            name: "a\u{0300}".into(),
            zoomed: false,
            is_attention: false,
            agent_state: None,
        };
        assert_eq!(c.display_width(TabStatusMode::Off), 1);
    }

    #[test]
    fn display_width_invariants() {
        let c = TabChrome {
            status: None,
            name: "hello".into(),
            zoomed: false,
            is_attention: false,
            agent_state: None,
        };
        assert_eq!(c.display_width(TabStatusMode::Off), 5);
        let c = TabChrome {
            status: None,
            name: "hello".into(),
            zoomed: true,
            is_attention: false,
            agent_state: None,
        };
        assert_eq!(c.display_width(TabStatusMode::Off), 7);
        let c = TabChrome {
            status: None,
            name: "hello".into(),
            zoomed: false,
            is_attention: false,
            agent_state: None,
        };
        assert_eq!(c.display_width(TabStatusMode::All), 7);
        let c = TabChrome {
            status: Some(TabStatusDot {
                glyph: "●",
                style: Style::default(),
            }),
            name: "hello".into(),
            zoomed: true,
            is_attention: false,
            agent_state: None,
        };
        assert_eq!(c.display_width(TabStatusMode::All), 9);
    }

    #[test]
    fn to_spans_ordering_and_padding() {
        let c = TabChrome {
            status: Some(TabStatusDot {
                glyph: "●",
                style: Style::default().fg(ratatui::style::Color::Red),
            }),
            name: "test".into(),
            zoomed: true,
            is_attention: false,
            agent_state: None,
        };
        let spans = c.to_spans(TabStatusMode::All, 15);
        assert_eq!(spans[0].content.as_ref(), " ");
        assert_eq!(spans[1].content.as_ref(), "●");
        assert_eq!(spans[2].content.as_ref(), " ");
        assert_eq!(spans[3].content.as_ref(), "test");
        assert_eq!(spans[4].content.as_ref(), " Z");
        assert_eq!(spans[5].content.len(), 6);

        let c = TabChrome {
            status: None,
            name: "abc".into(),
            zoomed: false,
            is_attention: false,
            agent_state: None,
        };
        let spans = c.to_spans(TabStatusMode::All, 10);
        assert_eq!(spans[0].content.as_ref(), " ");
        assert_eq!(spans[1].content.as_ref(), "  ");
        assert!(spans[1].style.fg.is_none(), "empty slot must not set fg");
        assert_eq!(spans[2].content.as_ref(), "abc");
        assert_eq!(spans[3].content.len(), 4);

        let c = TabChrome {
            status: None,
            name: "xyz".into(),
            zoomed: false,
            is_attention: false,
            agent_state: None,
        };
        let spans = c.to_spans(TabStatusMode::Off, 8);
        assert_eq!(spans[0].content.as_ref(), " ");
        assert_eq!(spans[1].content.as_ref(), "xyz");
        assert_eq!(spans[2].content.len(), 4);
    }

    #[test]
    fn to_spans_truncates_over_wide_name_by_display_width() {
        let c = TabChrome {
            status: None,
            name: "longername".into(),
            zoomed: false,
            is_attention: false,
            agent_state: None,
        };
        // mode=Off, no zoom: name_budget = rect_width - 1 = 4 → "lon…".
        let spans = c.to_spans(TabStatusMode::Off, 5);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains('…'), "expected ellipsis truncation: {text:?}");
        assert!(text.contains("lon"), "kept a prefix of the name: {text:?}");
    }

    #[test]
    fn to_spans_no_truncation_when_name_fits() {
        let c = TabChrome {
            status: None,
            name: "abc".into(),
            zoomed: false,
            is_attention: false,
            agent_state: None,
        };
        let spans = c.to_spans(TabStatusMode::Off, 8);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("abc"));
        assert!(!text.contains('…'));
    }

    #[test]
    fn sanitize_display_name_strips_bidi_and_control() {
        assert_eq!(sanitize_display_name("a\u{202e}b\x01c"), "abc");
        // Clean input borrows.
        assert!(matches!(
            sanitize_display_name("clean"),
            Cow::Borrowed("clean")
        ));
    }

    #[test]
    fn to_spans_sanitizes_name_at_render_chokepoint() {
        let c = TabChrome {
            status: None,
            name: "ev\u{202e}il".into(),
            zoomed: false,
            is_attention: false,
            agent_state: None,
        };
        let spans = c.to_spans(TabStatusMode::Off, 12);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            !text.contains('\u{202e}'),
            "bidi override must be stripped: {text:?}"
        );
        assert!(text.contains("evil"));
    }

    // -----------------------------------------------------------------------
    // Render: separators, coloring, indicators
    // -----------------------------------------------------------------------

    fn render_to_buffer(app: &AppState, area: Rect) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(area.x + area.width, area.y + area.height.max(1));
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_tab_bar(app, frame, area))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn app_with_tab_bar(names: &[&str], active: usize, area: Rect, mouse_chrome: bool) -> AppState {
        let mut app = AppState::test_new();
        let mut ws = make_ws_with_tabs(names);
        ws.active_tab = active;
        app.workspaces = vec![ws];
        app.active = Some(0);
        app.mouse_capture = mouse_chrome;
        app.view.tab_bar_rect = area;
        let chromes = chromes_from_ws(&app.workspaces[0]);
        let view = compute_tab_bar_view(chromes, active, TabStatusMode::Off, area, mouse_chrome);
        app.view.tab_hit_areas = view.tab_hit_areas;
        app.view.tab_chrome = view.tab_chrome;
        app.view.tab_status_mode = view.tab_status_mode;
        app.view.tab_overflow = view.overflow;
        app.view.new_tab_hit_area = view.new_tab_hit_area;
        app
    }

    #[test]
    fn render_active_tab_paints_accent_bg() {
        // Active tab's interior (between its own left and right arrows) is
        // painted accent. Under Powerline, the rightmost column carries the
        // right-arrow glyph whose bg is panel_bg (the wedge transitioning back
        // to the outer panel), so iterate strictly over the interior.
        let mut app = app_with_tab_bar(&["ab", "cd", "ef"], 0, Rect::new(0, 0, 40, 1), false);
        app.show_tab_status = TabStatusMode::All;
        let active_rect = app.view.tab_hit_areas[0];
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        // Left arrow cell at rect.x: bg = accent (panel→accent wedge).
        assert_eq!(buffer[(active_rect.x, 0)].bg, app.palette.accent);
        // Interior columns: bg = accent.
        for x in (active_rect.x + 1)..(active_rect.x + active_rect.width - 1) {
            assert_eq!(
                buffer[(x, 0)].bg,
                app.palette.accent,
                "active tab interior cell at x={x} should have accent bg"
            );
        }
    }

    #[test]
    fn render_inactive_tabs_alternate_background() {
        // AlternatingBg (powerline OFF): even-index inactive tabs use surface0,
        // odd use surface1, providing visual separation without arrows. The
        // 2 separator cols at each tab edge become extra tab-bg padding.
        let mut app = app_with_tab_bar(&["aa", "bb", "cc"], 0, Rect::new(0, 0, 60, 1), false);
        app.tabs_powerline = false;
        let p = &app.palette;
        let r1 = app.view.tab_hit_areas[1];
        let r2 = app.view.tab_hit_areas[2];
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        // Interior bg (any column strictly inside the rect) is the tab bg.
        assert_eq!(
            buffer[(r1.x + 1, 0)].bg,
            p.surface1,
            "tab 1 interior should be surface1"
        );
        assert_eq!(
            buffer[(r2.x + 1, 0)].bg,
            p.surface0,
            "tab 2 interior should be surface0"
        );
    }

    #[test]
    fn render_inactive_tabs_uniform_bg_when_powerline_on() {
        // Powerline ON: all inactive tabs share the same surface0 background;
        // each tab owns its left+right arrow blending against panel_bg.
        let mut app = app_with_tab_bar(&["aa", "bb", "cc"], 0, Rect::new(0, 0, 60, 1), false);
        app.tabs_powerline = true;
        let p = &app.palette;
        let r1 = app.view.tab_hit_areas[1];
        let r2 = app.view.tab_hit_areas[2];
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        // Interior columns are the tab bg (surface0 for both inactive tabs).
        assert_eq!(
            buffer[(r1.x + 1, 0)].bg,
            p.surface0,
            "tab 1 interior should be uniform surface0"
        );
        assert_eq!(
            buffer[(r2.x + 1, 0)].bg,
            p.surface0,
            "tab 2 interior should be uniform surface0"
        );
    }

    #[test]
    fn render_powerline_arrow_two_wedge_separator_between_tabs() {
        // Zellij convention: each tab carries its own left+right arrow, both
        // blending against panel_bg. Two adjacent tabs produce back-to-back
        // wedges: the right arrow of tab N (fg=tab_bg, bg=panel_bg) abuts the
        // left arrow of tab N+1 (fg=panel_bg, bg=tab_bg).
        let mut app = app_with_tab_bar(&["aa", "bb", "cc"], 0, Rect::new(0, 0, 60, 1), false);
        app.tabs_powerline = true;
        let p = &app.palette;
        let r1 = app.view.tab_hit_areas[1];
        let r2 = app.view.tab_hit_areas[2];
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);

        // Tab 1 right arrow at the last column of r1.
        let r1_right_x = r1.x + r1.width - 1;
        let r1_right = &buffer[(r1_right_x, 0)];
        assert_eq!(r1_right.symbol(), POWERLINE_ARROW);
        assert_eq!(r1_right.fg, p.surface0, "tab 1 right arrow fg = tab_bg");
        assert_eq!(r1_right.bg, p.panel_bg, "tab 1 right arrow bg = panel_bg");

        // Tab 2 left arrow at the first column of r2 — abuts r1's right arrow.
        assert_eq!(r2.x, r1.x + r1.width, "tabs must abut directly");
        let r2_left = &buffer[(r2.x, 0)];
        assert_eq!(r2_left.symbol(), POWERLINE_ARROW);
        assert_eq!(r2_left.fg, p.panel_bg, "tab 2 left arrow fg = panel_bg");
        assert_eq!(r2_left.bg, p.surface0, "tab 2 left arrow bg = tab_bg");
    }

    #[test]
    fn render_powerline_emits_arrow_when_enabled() {
        let mut app = app_with_tab_bar(&["aa", "bb", "cc"], 0, Rect::new(0, 0, 60, 1), false);
        app.tabs_powerline = true;
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        let row = buffer_row_text(&buffer, app.view.tab_bar_rect, 0);
        assert!(
            row.contains(POWERLINE_ARROW),
            "Powerline arrow expected when powerline on: {row:?}"
        );
    }

    #[test]
    fn overflow_indicator_renders_as_tile_with_arrows() {
        // The overflow indicator is a proper zellij tile: a left arrow
        // (panel→tile), interior on tile_bg, and a right arrow (tile→panel) —
        // identical chrome to a regular inactive tab.
        let names: Vec<String> = (0..14).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut app = app_with_tab_bar(&refs, 7, Rect::new(0, 0, 40, 1), true);
        app.tabs_powerline = true;
        let p = &app.palette;
        let tile_bg = p.surface0;
        let left = app.view.tab_overflow.left_hit_area;
        let right = app.view.tab_overflow.right_hit_area;
        assert!(
            left.width >= TAB_SEPARATOR_OVERHEAD,
            "left indicator present"
        );
        assert!(
            right.width >= TAB_SEPARATOR_OVERHEAD,
            "right indicator present"
        );
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);

        // Left indicator: left col = panel→tile arrow, right col = tile→panel.
        let l0 = &buffer[(left.x, 0)];
        assert_eq!(l0.symbol(), POWERLINE_ARROW);
        assert_eq!(l0.fg, p.panel_bg, "left indicator left arrow fg = panel_bg");
        assert_eq!(l0.bg, tile_bg, "left indicator left arrow bg = tile_bg");
        let l_last = &buffer[(left.x + left.width - 1, 0)];
        assert_eq!(l_last.symbol(), POWERLINE_ARROW);
        assert_eq!(
            l_last.fg, tile_bg,
            "left indicator right arrow fg = tile_bg"
        );
        assert_eq!(
            l_last.bg, p.panel_bg,
            "left indicator right arrow bg = panel_bg"
        );

        // Right indicator: same wedge chrome.
        let r0 = &buffer[(right.x, 0)];
        assert_eq!(r0.symbol(), POWERLINE_ARROW);
        assert_eq!(
            r0.fg, p.panel_bg,
            "right indicator left arrow fg = panel_bg"
        );
        assert_eq!(r0.bg, tile_bg, "right indicator left arrow bg = tile_bg");
        let r_last = &buffer[(right.x + right.width - 1, 0)];
        assert_eq!(r_last.symbol(), POWERLINE_ARROW);
        assert_eq!(
            r_last.fg, tile_bg,
            "right indicator right arrow fg = tile_bg"
        );
        assert_eq!(
            r_last.bg, p.panel_bg,
            "right indicator right arrow bg = panel_bg"
        );
    }

    #[test]
    fn overflow_indicator_alternating_bg_path_skips_arrows() {
        // Under AlternatingBg the indicator interior fills the whole rect and
        // emits no Powerline codepoint.
        let names: Vec<String> = (0..14).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut app = app_with_tab_bar(&refs, 7, Rect::new(0, 0, 40, 1), true);
        app.tabs_powerline = false;
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        for ind in [
            app.view.tab_overflow.left_hit_area,
            app.view.tab_overflow.right_hit_area,
        ] {
            for x in ind.x..ind.x + ind.width {
                assert!(
                    !buffer[(x, 0)].symbol().contains(POWERLINE_ARROW),
                    "AlternatingBg overflow indicator must emit no Powerline glyph at x={x}"
                );
            }
        }
    }

    #[test]
    fn render_alternating_bg_emits_no_powerline_codepoints() {
        let mut app = app_with_tab_bar(&["aa", "bb", "cc"], 0, Rect::new(0, 0, 60, 1), false);
        app.tabs_powerline = false;
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        let area = app.view.tab_bar_rect;
        for x in area.x..area.x + area.width {
            let sym = buffer[(x, 0)].symbol();
            assert!(
                !sym.contains(POWERLINE_ARROW),
                "AlternatingBg path must emit zero Powerline codepoints, found at x={x}"
            );
            // Only single-cell glyphs in the OFF path.
            assert!(sym.width() <= 1, "OFF path emitted a wide glyph {sym:?}");
        }
    }

    #[test]
    fn render_mouse_indicators_show_plus_counts() {
        let names: Vec<String> = (0..10).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let app = app_with_tab_bar(&refs, 5, Rect::new(0, 0, 40, 1), true);
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        let row = buffer_row_text(&buffer, app.view.tab_bar_rect, 0);
        assert!(
            row.contains('+'),
            "mouse indicators show +N counts: {row:?}"
        );
        assert!(row.contains('←') || row.contains('→'), "arrows: {row:?}");
    }

    #[test]
    fn render_non_mouse_marker_is_present_without_plus() {
        let names: Vec<String> = (0..10).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let app = app_with_tab_bar(&refs, 5, Rect::new(0, 0, 40, 1), false);
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        let row = buffer_row_text(&buffer, app.view.tab_bar_rect, 0);
        // Non-mouse markers use the arrow + count, but no clickable "+".
        assert!(
            row.contains('←') || row.contains('→'),
            "marker arrow: {row:?}"
        );
    }

    #[test]
    fn render_count_caps_at_nine_plus() {
        let names: Vec<String> = (0..15).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        // Active near the end so >9 hidden on the left.
        let app = app_with_tab_bar(&refs, 14, Rect::new(0, 0, 30, 1), true);
        assert!(app.view.tab_overflow.left.unwrap().count > 9);
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        let row = buffer_row_text(&buffer, app.view.tab_bar_rect, 0);
        assert!(row.contains("9+"), "expected 9+ cap: {row:?}");
    }

    // -----------------------------------------------------------------------
    // Drop-indicator re-coupling
    // -----------------------------------------------------------------------

    #[test]
    fn drop_indicator_x_at_start_returns_first_tab_x() {
        let app = app_with_tab_bar(&["ab", "cd", "ef"], 0, Rect::new(0, 0, 50, 1), true);
        assert_eq!(
            tab_drop_indicator_x(&app, &app.workspaces[0], 0),
            Some(app.view.tab_hit_areas[0].x)
        );
    }

    #[test]
    fn drop_indicator_x_between_tabs_is_one_before_target() {
        let app = app_with_tab_bar(&["ab", "cd", "ef"], 0, Rect::new(0, 0, 50, 1), true);
        let r1 = app.view.tab_hit_areas[1];
        assert_eq!(
            tab_drop_indicator_x(&app, &app.workspaces[0], 1),
            Some(r1.x.saturating_sub(1))
        );
    }

    #[test]
    fn drop_indicator_x_at_start_uses_left_indicator_when_left_clipped() {
        let names: Vec<String> = (0..10).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let app = app_with_tab_bar(&refs, 5, Rect::new(0, 0, 40, 1), true);
        assert!(app.view.tab_hit_areas[0].width == 0, "left edge clipped");
        let overflow = &app.view.tab_overflow;
        let expected = overflow.left_hit_area.x + overflow.left_hit_area.width;
        assert_eq!(
            tab_drop_indicator_x(&app, &app.workspaces[0], 0),
            Some(expected)
        );
    }

    #[test]
    fn drop_indicator_x_at_end_uses_right_indicator_when_right_clipped() {
        let names: Vec<String> = (0..10).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let app = app_with_tab_bar(&refs, 5, Rect::new(0, 0, 40, 1), true);
        let last = refs.len() - 1;
        assert!(
            app.view.tab_hit_areas[last].width == 0,
            "right edge clipped"
        );
        let expected = app.view.tab_overflow.right_hit_area.x.saturating_sub(1);
        assert_eq!(
            tab_drop_indicator_x(&app, &app.workspaces[0], refs.len()),
            Some(expected)
        );
    }

    #[test]
    fn truncate_to_width_respects_wide_glyphs() {
        // "你好世界" is 8 columns; budget 5 → 4 columns of content + ellipsis.
        let out = truncate_to_width("你好世界", 5);
        assert!(out.ends_with('…'));
        assert!(
            out.width() <= 5,
            "must not exceed budget: {out:?} ({})",
            out.width()
        );
    }
}
