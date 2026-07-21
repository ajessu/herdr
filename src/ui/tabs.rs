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

const NEW_TAB_WIDTH: u16 = 3;
/// Each tab owns its own left + right separator columns (zellij convention).
/// Adjacent tabs abut directly: the right separator of T<sub>N</sub> sits next
/// to the left separator of T<sub>N+1</sub>, producing two back-to-back wedges
/// that blend against `panel_bg` from both sides.
const TAB_SEPARATOR_OVERHEAD: u16 = 2;
/// Base width of a collapsed ` ← +N ` / ` +N → ` overflow indicator, rendered
/// as a proper zellij tile (left arrow + interior + right arrow). Eight cells
/// = 2 separator cols + 6 interior cols: zellij's single-digit ` ← +N `
/// interior. The tile grows past this base with the count (full count, `many`
/// past 9999 — zellij `left_more_message`/`right_more_message`) and the
/// mouse-chrome badge segments; `tab_indicator_width` computes the demanded
/// width per side. Also a touch-adequate hit zone (NFR5) — never a 1-cell
/// target. Under `AlternatingBg` the 2 separator cols become interior padding.
const OVERFLOW_INDICATOR_WIDTH: u16 = 8;

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
    /// Columns the inline status dot occupies: the dot's measured display
    /// width plus one separating space, or 0 when this chrome has no dot.
    /// Single source of truth shared by `display_width` (sizing) and
    /// `to_spans` (fill) — if the two derived this independently a tab could
    /// be sized for a slot the fill doesn't emit, or vice versa.
    fn status_cols(&self) -> u16 {
        self.status
            .as_ref()
            .map(|dot| {
                u16::try_from(dot.glyph.width())
                    .unwrap_or(u16::MAX)
                    .saturating_add(1)
            })
            .unwrap_or(0)
    }

    pub fn display_width(&self) -> u16 {
        // Unicode display width (not char count): CJK/wide glyphs occupy two
        // columns and combining/ZWJ sequences fewer, so char count would
        // mis-size the tab and break the position→index round-trip. The
        // sanitized name is what actually reaches the buffer, so measure it.
        let name_w = u16::try_from(sanitize_display_name(&self.name).width()).unwrap_or(u16::MAX);
        let mod_w: u16 = if self.zoomed { 2 } else { 0 };
        self.status_cols()
            .saturating_add(name_w)
            .saturating_add(mod_w)
    }

    /// Render the tab label into spans padded to `rect_width` (the tab's
    /// interior, excluding the 2 separator cols). The name is sanitized
    /// (control/bidi stripped) at this render chokepoint. Interior layout is
    /// zellij's ` name ` — one space each side — with the status dot inline
    /// (` ● name `) only when present. The name is never truncated: a single
    /// over-wide active tab renders at natural width and the render path clips
    /// it at the bar edge with no ellipsis, matching zellij.
    pub fn to_spans(&self, rect_width: u16) -> Vec<Span<'static>> {
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(6);

        // Leading space
        spans.push(Span::raw(" "));

        // Inline status dot (only when present): dot + separating space,
        // exactly the `status_cols()` columns `display_width` sized for.
        if let Some(ref dot) = self.status {
            spans.push(Span::styled(dot.glyph, dot.style));
            spans.push(Span::raw(" "));
        }

        let name = sanitize_display_name(&self.name);
        spans.push(Span::raw(name.into_owned()));

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
    tab_scroll: Option<&crate::app::state::TabScroll>,
) -> (Vec<TabChrome>, usize, TabStatusMode, Option<usize>) {
    let chromes = build_tab_chromes(ws, terminals, show_tab_status, spinner_tick, palette);
    // Anchor resolution: the browse offset applies only while the active tab
    // is still the one browsing was entered on, identified by workspace id +
    // per-workspace tab number (unique among live tabs, asserted by
    // `Workspace::assert_invariants_for_test`; never reassigned during a live
    // session). Any activation path — click, keyboard, indicator jump, tab
    // API, agent focus, workspace switch, or a direct `active_tab` write —
    // changes that identity, so browse mode exits here by construction on the
    // next compute; the call sites write the resolved `None` back.
    let offset = tab_scroll.and_then(|scroll| {
        let anchored = scroll.anchor_workspace_id == ws.id
            && ws.tabs.get(ws.active_tab).map(|tab| tab.number) == Some(scroll.anchor_tab_number);
        if !anchored {
            // Exit cause: the active tab's identity no longer matches the
            // anchor (any activation path). Logged here, at the resolution
            // site, because `reconcile_tab_scroll` only sees the `None` result
            // and cannot tell this apart from the no-overflow / fallback exits.
            // Edge-triggered: the next frame's `tab_scroll` is already cleared.
            tracing::debug!(
                reason = "anchor_mismatch",
                anchor_tab_number = scroll.anchor_tab_number,
                active_tab = ws.active_tab,
                "tab-bar browse mode exiting"
            );
        }
        anchored.then_some(scroll.first_visible)
    });
    (chromes, ws.active_tab, show_tab_status, offset)
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
    /// Diagnostics only: dot presence is baked into each `TabChrome.status`
    /// at chrome-build time, so rendering no longer consults the mode.
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

/// Uncapped hidden-count text for the zellij-format overflow indicator: the
/// full count, or `many` past 9999 — matching zellij's `left_more_message` /
/// `right_more_message`. Tab-bar-local on purpose: the sidebar's badge keeps
/// the capped `overflow::count_label` (`9+`), which this does NOT replace.
fn indicator_count_text(count: usize) -> String {
    if count > 9999 {
        "many".to_string()
    } else {
        count.to_string()
    }
}

/// Display columns of `indicator_count_text(count)`, computed arithmetically
/// so the sizing path (which runs inside the per-frame reserve-convergence
/// loop) allocates nothing. Exact because the count digits are width-1 ASCII
/// and `many` is 4 columns; pinned to the text by a lockstep test.
fn indicator_count_cols(count: usize) -> u16 {
    if count > 9999 {
        return 4;
    }
    let mut n = count;
    let mut digits = 1u16;
    while n >= 10 {
        n /= 10;
        digits += 1;
    }
    digits
}

/// Columns an overflow indicator tile needs for `count` hidden tabs whose
/// disaggregated badge breakdown is `side`. The tile owns 2 separator cols
/// (left + right Powerline arrow) plus the zellij ` ← +N ` / ` +N → `
/// interior: leading space + arrow + space + `+` + count text + trailing
/// space = 5 columns plus the count, sized from the UNCAPPED count on every
/// path so the reserve always matches the render. Badge segments (mouse
/// chrome only) sit between the count and the trailing space and widen the
/// interior further. Count columns come from the allocation-free
/// `indicator_count_cols` since this runs in the per-frame sizing loop.
fn tab_indicator_width(
    count: usize,
    side: super::overflow::OverflowSide,
    mouse_chrome: bool,
) -> u16 {
    let count_w = indicator_count_cols(count);
    let badge_w = if mouse_chrome {
        super::overflow::badge_attention_width(side)
    } else {
        0
    };
    let interior = 5u16.saturating_add(count_w).saturating_add(badge_w);
    interior
        .saturating_add(TAB_SEPARATOR_OVERHEAD)
        .max(OVERFLOW_INDICATOR_WIDTH)
}

fn tab_width(chrome: &TabChrome) -> u16 {
    // Interior (` name ` — one space each side, zellij tab.rs:59) + 2
    // separator cols owned by the tab (left arrow + right arrow under
    // Powerline, equivalent padding under AlternatingBg). Adjacent tabs abut,
    // so there is no extra inter-tab gap. No minimum-width floor: zellij has
    // none, and the tightest real tab (1-col name) is still 5 cols and
    // clickable.
    chrome
        .display_width()
        .saturating_add(2)
        .saturating_add(TAB_SEPARATOR_OVERHEAD)
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

/// Total columns the window `[lo, hi]` occupies: each tab's width (which
/// already includes its own separator columns) plus an indicator reservation
/// on each side that still hides tabs — a fully-shown side reclaims its
/// reserved columns. Adjacent tabs abut directly, so there is no inter-tab gap.
/// Shared by the centered fill, the scrolled fill, and the `max_scroll` walk so
/// all three measure a window identically.
fn window_footprint(
    chromes: &[TabChrome],
    lo: usize,
    hi: usize,
    reserve_left: u16,
    reserve_right: u16,
) -> u16 {
    let n = chromes.len();
    let mut total: u16 = 0;
    for chrome in &chromes[lo..=hi] {
        total = total.saturating_add(tab_width(chrome));
    }
    if lo > 0 {
        total = total.saturating_add(reserve_left);
    }
    if hi + 1 < n {
        total = total.saturating_add(reserve_right);
    }
    total
}

/// Lay the visible window `[lo, hi]` left-to-right starting at `area.x` (after
/// the left indicator reservation when `lo > 0`). Adjacent tabs abut directly
/// (each carries its own separator cols, no inter-tab gap). A single tab wider
/// than the remaining width is clipped to the bar so the row is never blank.
/// Returns a dense rect vec (len == `chromes.len()`, off-window tabs `width 0`).
///
/// The single placement routine shared by `centered_active_fill` and
/// `scrolled_fill`: the two fills differ only in how they choose `[lo, hi]`.
fn lay_window(
    chromes: &[TabChrome],
    lo: usize,
    hi: usize,
    area: Rect,
    reserve_left: u16,
) -> Vec<Rect> {
    let n = chromes.len();
    let mut rects = vec![Rect::default(); n];
    if n == 0 || area.width == 0 || area.height == 0 {
        return rects;
    }
    let left_gutter = if lo > 0 { reserve_left } else { 0 };
    let mut x = area.x.saturating_add(left_gutter);
    let right_limit = area.x.saturating_add(area.width);
    for idx in lo..=hi.min(n - 1) {
        let desired = tab_width(&chromes[idx]);
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
    area: Rect,
    reserve_left: u16,
    reserve_right: u16,
) -> Vec<Rect> {
    let n = chromes.len();
    if n == 0 || area.width == 0 || area.height == 0 {
        return vec![Rect::default(); n];
    }
    let active = active_tab.min(n - 1);

    let footprint =
        |lo: usize, hi: usize| window_footprint(chromes, lo, hi, reserve_left, reserve_right);

    // Window of visible tab indices [lo, hi], grown around the active tab.
    let mut lo = active;
    let mut hi = active;

    // Accumulated width of the tabs added to each side so far. zellij balances
    // the centered-active fill by ACCUMULATED WIDTH (`total_left`/`total_right`
    // in `populate_tabs_in_tab_line`), not by tab count — for tabs of unequal
    // width, count-balance would off-center the active tab. Match zellij: add
    // to whichever side has less width so far (ties + a non-fitting other side
    // both bias left, mirroring zellij's `total_left <= total_right || !right_fits`).
    let mut total_left: u16 = 0;
    let mut total_right: u16 = 0;

    loop {
        let can_extend_left = lo > 0;
        let can_extend_right = hi + 1 < n;
        if !can_extend_left && !can_extend_right {
            break;
        }

        let left_fits = can_extend_left && footprint(lo - 1, hi) <= area.width;
        let right_fits = can_extend_right && footprint(lo, hi + 1) <= area.width;

        // zellij: `if (total_left <= total_right || !right_fits) && left_fits {
        // add left } else if right_fits { add right }`. Balance by accumulated
        // width; a tie or a non-fitting right side both prefer left.
        let mut progressed = false;
        if (total_left <= total_right || !right_fits) && left_fits {
            lo -= 1;
            total_left = total_left.saturating_add(tab_width(&chromes[lo]));
            progressed = true;
        } else if right_fits {
            hi += 1;
            total_right = total_right.saturating_add(tab_width(&chromes[hi]));
            progressed = true;
        }

        if !progressed {
            break;
        }
    }

    // Lay the window [lo, hi] via the shared placement routine. `footprint`
    // already reserved the right indicator's columns when growing the window,
    // so a fully-fit window never reaches into them; `lay_window`'s clip only
    // bounds a single over-wide tab so the row is never blank.
    lay_window(chromes, lo, hi, area, reserve_left)
}

/// Left-packed scrolled fill: place tabs starting at `offset` and pack right
/// until the bar is full. Differs from `centered_active_fill` only in window
/// selection — the visible run begins at `offset` (clamped into range) and
/// grows rightward. `reserve_left`/`reserve_right` reserve indicator columns on
/// a side that still hides tabs, matching the centered fill's convention. The
/// window end `hi` is the last tab that fits after reserving the right
/// indicator when tabs remain past it. A single tab at `offset` wider than the
/// bar is clipped by `lay_window` so the row is never blank.
fn scrolled_fill(
    chromes: &[TabChrome],
    offset: usize,
    area: Rect,
    reserve_left: u16,
    reserve_right: u16,
) -> Vec<Rect> {
    let n = chromes.len();
    if n == 0 || area.width == 0 || area.height == 0 {
        return vec![Rect::default(); n];
    }
    let lo = offset.min(n - 1);

    // Choose the window end `hi` as the largest tab index for which the
    // contiguous window `[lo, hi]` fits the usable width. `window_footprint`
    // accounts the left reserve (present whenever lo > 0) and the right reserve
    // (present only while tabs remain hidden past hi), so the fit test sees the
    // same columns the render reserves. The footprint is NOT monotonic in `hi`
    // at the final tab: reaching `n-1` drops the right reserve, so a full window
    // to the end can fit even when an intermediate prefix (which still pays the
    // right reserve) does not. Scanning all candidates and taking the max that
    // fits — rather than breaking on the first miss — is what lets the fill
    // reach the last tab exactly at `max_scroll`, matching `max_scroll_offset`.
    let mut hi = lo;
    for candidate in (lo + 1)..n {
        if window_footprint(chromes, lo, candidate, reserve_left, reserve_right) <= area.width {
            hi = candidate;
        }
    }

    lay_window(chromes, lo, hi, area, reserve_left)
}

/// Column-containment lookup over a dense hit-area vec: the index of the
/// visible (width>0) rect containing `col`, if any. The single hit predicate
/// shared by the production mouse mapping (`AppState::tab_at`) and the
/// hit-test round-trip tests, so both exercise identical containment logic.
/// Row containment stays with the caller (all tab rects share the bar's row).
pub(crate) fn hit_index(rects: &[Rect], col: u16) -> Option<usize> {
    rects
        .iter()
        .position(|area| area.width > 0 && col >= area.x && col < area.x + area.width)
}

/// First and last visible (width>0) tab indices in a dense hit-area vec.
fn visible_bounds(rects: &[Rect]) -> Option<(usize, usize)> {
    let first = rects.iter().position(|r| r.width > 0)?;
    let last = rects.iter().rposition(|r| r.width > 0)?;
    Some((first, last))
}

/// Build the dense hit rects and overflow indicators for an overflowing strip,
/// given a `fill(reserve_left, reserve_right) -> rects` closure. This is the
/// badge-aware reserve-convergence loop plus indicator-rect placement, factored
/// out so the centered and scrolled fills share it verbatim: the two differ
/// only in how `fill` chooses the visible window. `fallback_index` is the
/// visible-bounds default for a degenerate (nothing-visible) fill.
fn build_overflow_layout(
    chromes: &[TabChrome],
    tabs_area: Rect,
    area: Rect,
    mouse_chrome: bool,
    fallback_index: usize,
    fill: impl Fn(u16, u16) -> Vec<Rect>,
) -> (Vec<Rect>, TabBarOverflow) {
    let tab_count = chromes.len();
    // Hidden groups reuse the shared `overflow::side_above`/`side_below` so the
    // tab bar and the sidebar surfaces classify hidden items into the same
    // Working/Blocked/Done-unseen buckets with one nearest-to-edge walk over
    // only the hidden ranges [0..first) and (last..n). A tab with no
    // agent_state (TabStatusMode::Off) classifies as Unknown, which
    // `AttnBucket::classify` treats as non-badge-worthy.
    let state_of = |i: usize| {
        chromes
            .get(i)
            .and_then(|c| c.agent_state)
            .unwrap_or((crate::detect::AgentState::Unknown, true))
    };
    let groups_for = |rects: &[Rect]| -> (Option<HiddenGroup>, Option<HiddenGroup>) {
        let (first, last) = visible_bounds(rects).unwrap_or((fallback_index, fallback_index));
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
    let mut rects = fill(OVERFLOW_INDICATOR_WIDTH, OVERFLOW_INDICATOR_WIDTH);
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
        rects = fill(
            left_w.max(OVERFLOW_INDICATOR_WIDTH),
            right_w.max(OVERFLOW_INDICATOR_WIDTH),
        );
        let (l2, r2) = groups_for(&rects);
        left = l2;
        right = r2;
    }

    // Indicator rects occupy the columns the final fill reserved for them.
    // The left indicator hugs area.x; the right abuts the last visible tab
    // directly (zellij left-packs: tabs, then the collapsed indicator, then
    // bar-bg fill to the edge — no dead gap, and the tile stays compact at
    // its demanded width instead of stretching to the edge). At
    // pathologically narrow widths the active tab (always visible) can
    // consume more than the reserve left for an indicator, so clamp: the
    // left indicator's right edge must not pass the first visible tab, and
    // the right indicator clips at the tabs-area edge. The indicator
    // content clips if the clamp shrinks it.
    let (vis_first, vis_last) = visible_bounds(&rects).unwrap_or((fallback_index, fallback_index));
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
        let rx = last_visible_right.min(tabs_right);
        let rw = right_w.min(tabs_right.saturating_sub(rx));
        Rect::new(rx, area.y, rw, 1)
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
}

/// The maximum scroll position — the last tab flush-right, with no scrolling
/// past it. That position is reached at the *smallest* `offset` whose
/// left-packed window `[offset, n-1]` fits the usable width; scrolling further
/// (a larger offset) would only push the last tab off the right, so this
/// smallest fitting offset is the clamp ceiling. Computed with the badge-aware
/// left-indicator reserve the hidden-left range demands (and no right reserve,
/// since the last tab being shown means
/// nothing is hidden on the right). Derived from the same
/// `window_footprint`/`tab_indicator_width` machinery the scrolled fill uses,
/// so the clamp and the fill agree at the boundary even when a badge widens the
/// indicator. Returns `n-1` when even a single tab cannot fit (the fill clips
/// it; the row is still non-blank).
fn max_scroll_offset(
    chromes: &[TabChrome],
    usable_width: u16,
    mouse_chrome: bool,
    state_of: &impl Fn(usize) -> (crate::detect::AgentState, bool),
) -> usize {
    let n = chromes.len();
    if n == 0 {
        return 0;
    }
    for offset in 0..n {
        let reserve_left = if offset > 0 {
            let window = super::overflow::ListWindow {
                first: offset,
                count: n - offset,
                hidden_above: offset,
                hidden_below: 0,
            };
            let side = super::overflow::side_above(window, state_of);
            tab_indicator_width(offset, side, mouse_chrome).max(OVERFLOW_INDICATOR_WIDTH)
        } else {
            0
        };
        if window_footprint(chromes, offset, n - 1, reserve_left, 0) <= usable_width {
            return offset;
        }
    }
    n - 1
}

/// Defensive consistency predicate for a scrolled fill result, release-safe and
/// testable in isolation (fed hand-crafted bad windows). A consistent window
/// has at least one visible tab, packs within the usable area, and — when
/// nothing is hidden on the right — shows the final tab (its dense rect is
/// non-zero-width). A `false` result routes the compute path back to the
/// centered fill rather than tripping a `debug_assert!`.
fn window_is_consistent(rects: &[Rect], area: Rect, hidden_right_count: usize) -> bool {
    let mut any_visible = false;
    let right_limit = area.x.saturating_add(area.width);
    for r in rects.iter().filter(|r| r.width > 0) {
        any_visible = true;
        if r.x.saturating_add(r.width) > right_limit {
            return false;
        }
    }
    if !any_visible {
        return false;
    }
    if hidden_right_count == 0 {
        if let Some(last) = rects.last() {
            if last.width == 0 {
                return false;
            }
        }
    }
    true
}

pub(crate) fn compute_tab_bar_view(
    chromes: Vec<TabChrome>,
    active_tab: usize,
    mode: TabStatusMode,
    area: Rect,
    mouse_chrome: bool,
    scroll_offset: Option<usize>,
) -> (TabBarView, Option<usize>) {
    if area.width == 0 || area.height == 0 {
        return (TabBarView::default(), None);
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

    // Whether the strip overflows is a property of the strip and bar, not the
    // scroll offset: probe with the centered fill and no reserve.
    let probe = centered_active_fill(&chromes, active_tab, tabs_area, 0, 0);
    let tab_count = chromes.len();
    let probe_overflow = probe.iter().any(|r| r.width == 0) && tab_count > 0;

    // Shared badge classifier for the scrolled clamp (same mapping the layout
    // builder uses internally).
    let state_of = |i: usize| {
        chromes
            .get(i)
            .and_then(|c| c.agent_state)
            .unwrap_or((crate::detect::AgentState::Unknown, true))
    };

    let (tab_hit_areas, overflow, resolved_offset) = if !probe_overflow {
        // Everything fits — no indicators, and any requested offset resolves to
        // no-offset (centered layout).
        if scroll_offset.is_some() {
            // Exit cause: the bar no longer overflows (resize/close), so a
            // pending browse offset collapses. Logged at the resolution site
            // for the same reason as the anchor-mismatch case above.
            tracing::debug!(reason = "no_overflow", "tab-bar browse mode exiting");
        }
        (probe, TabBarOverflow::default(), None)
    } else {
        match scroll_offset {
            None => {
                let (rects, overflow) = build_overflow_layout(
                    &chromes,
                    tabs_area,
                    area,
                    mouse_chrome,
                    active_tab,
                    |l, r| centered_active_fill(&chromes, active_tab, tabs_area, l, r),
                );
                (rects, overflow, None)
            }
            Some(requested) => {
                // Clamp to the largest offset keeping the last tab fully
                // visible, derived from the badge-aware converged reserves.
                let max_scroll =
                    max_scroll_offset(&chromes, tabs_area.width, mouse_chrome, &state_of);
                let offset = requested.min(max_scroll);
                let (rects, overflow) = build_overflow_layout(
                    &chromes,
                    tabs_area,
                    area,
                    mouse_chrome,
                    offset,
                    |l, r| scrolled_fill(&chromes, offset, tabs_area, l, r),
                );
                let hidden_right = overflow.right.map(|g| g.count).unwrap_or(0);
                if window_is_consistent(&rects, tabs_area, hidden_right) {
                    (rects, overflow, Some(offset))
                } else {
                    // Inconsistent scrolled window. Normal offsets can't reach
                    // here — the clamp keeps the last tab visible — but a
                    // pathologically narrow bar can: when a badge-widened
                    // hidden-left reserve is itself as wide as the usable area,
                    // even the clamped last-tab window packs to nothing, so this
                    // branch is a genuine (if rare) runtime path, not dead code.
                    // Discard the offset, re-run the centered fill, and emit one
                    // structured warn. Returning `None` clears the offset; when
                    // a caller writes that back (the wheel-browse seam), the next
                    // frame resolves to `None` and does not re-enter this path,
                    // so the warn is edge-triggered by the exit rather than
                    // re-emitted per frame.
                    let visible_count = rects.iter().filter(|r| r.width > 0).count();
                    let packed_width = rects
                        .iter()
                        .filter(|r| r.width > 0)
                        .map(|r| r.x.saturating_add(r.width))
                        .max()
                        .unwrap_or(tabs_area.x)
                        .saturating_sub(tabs_area.x);
                    tracing::warn!(
                        offset,
                        max_scroll,
                        packed_width,
                        usable_width = tabs_area.width,
                        visible_count,
                        "scrolled tab window inconsistent; falling back to centered layout"
                    );
                    let (rects, overflow) = build_overflow_layout(
                        &chromes,
                        tabs_area,
                        area,
                        mouse_chrome,
                        active_tab,
                        |l, r| centered_active_fill(&chromes, active_tab, tabs_area, l, r),
                    );
                    (rects, overflow, None)
                }
            }
        }
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

    (
        TabBarView {
            tab_hit_areas,
            tab_chrome: chromes,
            tab_status_mode: mode,
            overflow,
            new_tab_hit_area,
        },
        resolved_offset,
    )
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
    // + right arrow), matching a regular inactive tab. The interior is
    // zellij's exact format — ` ← +N ` / ` +N → `, full count (`many` past
    // 9999), bold, unselected-ribbon styling. Under mouse chrome the tile is
    // clickable and up to three disaggregated Blocked/Working/Done-unseen
    // badge segments follow the count inside the same interior.
    let interior_text_style = Style::default()
        .fg(p.overlay1)
        .bg(indicator_tile_bg)
        .add_modifier(Modifier::BOLD);
    // Re-tint the shared bucket segments (which use transparent bg) onto the
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
            interior.push(Span::styled(
                format!(" ← +{}", indicator_count_text(group.count)),
                interior_text_style,
            ));
            if app.mouse_capture {
                interior.extend(on_tile(super::overflow::bucket_segment_spans(
                    group.side, p,
                )));
            }
            interior.push(Span::styled(" ", interior_text_style));
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
            interior.push(Span::styled(
                format!(" +{}", indicator_count_text(group.count)),
                interior_text_style,
            ));
            if app.mouse_capture {
                interior.extend(on_tile(super::overflow::bucket_segment_spans(
                    group.side, p,
                )));
            }
            interior.push(Span::styled(" → ", interior_text_style));
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
        // zellij `render_tab` paints EVERY tab label bold (active and inactive
        // alike) and distinguishes them only by color — the selected ribbon's
        // base/background vs the unselected ribbon's. herdr mirrors that: the
        // active tab uses `panel_contrast_fg` on the accent bg, inactive tabs
        // use the readable `text` color on the surface bg, and both are BOLD.
        // Auto-named tabs keep a herdr-only "unnamed" hint, but expressed as a
        // dimmer color (`overlay1`) rather than by dropping bold or dimming the
        // focused tab — zellij never renders a non-bold or dimmed tab.
        let fg = if active {
            panel_contrast_fg(p)
        } else if tab.is_auto_named() {
            p.overlay1
        } else {
            p.text
        };
        let interior_style = Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD);

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
                    chrome.to_spans(interior_w)
                } else {
                    vec![Span::raw(" ".repeat(interior_w as usize))]
                };
                frame.render_widget(
                    Paragraph::new(Line::from(spans)).style(interior_style),
                    interior_rect,
                );
            }
        } else if rect.width >= TAB_SEPARATOR_OVERHEAD {
            // AlternatingBg path: the 2 separator cols become one tab-bg
            // padding column on each side, and the label interior sits between
            // them — the same symmetric peel as the Powerline branch, so the
            // interior renders ` name ` under both separator styles.
            frame.render_widget(
                Paragraph::new(" ".repeat(rect.width as usize)).style(interior_style),
                rect,
            );
            let interior_w = rect.width - 2;
            if interior_w > 0 {
                let interior_rect = Rect::new(rect.x + 1, area.y, interior_w, 1);
                let spans = if let Some(chrome) = app.view.tab_chrome.get(idx) {
                    chrome.to_spans(interior_w)
                } else {
                    vec![Span::raw(" ".repeat(interior_w as usize))]
                };
                frame.render_widget(
                    Paragraph::new(Line::from(spans)).style(interior_style),
                    interior_rect,
                );
            }
        } else {
            // Rect too narrow for any separator columns: fill what remains.
            let spans = if let Some(chrome) = app.view.tab_chrome.get(idx) {
                chrome.to_spans(rect.width)
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

    /// No-offset (centered) wrapper: the vast majority of tab-bar tests predate
    /// the scroll offset and only care about the centered layout, so they call
    /// this thin adapter that passes `scroll_offset = None` and returns just the
    /// view. Scroll-specific tests call `compute_tab_bar_view` directly.
    fn cbv(
        chromes: Vec<TabChrome>,
        active_tab: usize,
        mode: TabStatusMode,
        area: Rect,
        mouse_chrome: bool,
    ) -> TabBarView {
        compute_tab_bar_view(chromes, active_tab, mode, area, mouse_chrome, None).0
    }

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
        let view = cbv(chromes, 0, TabStatusMode::Off, Rect::new(0, 0, 60, 1), true);
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
        let view = cbv(
            chromes.clone(),
            active,
            TabStatusMode::Off,
            Rect::new(0, 0, 40, 1),
            true,
        );
        let natural = tab_width(&chromes[active]);
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
        let view = cbv(chromes, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
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
        let view = cbv(chromes, 0, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
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
        let view = cbv(
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
        let view = cbv(chromes, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
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
        let view = cbv(chromes, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
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
        let view = cbv(chromes, 6, TabStatusMode::Off, Rect::new(0, 0, 36, 1), true);
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
        let view = cbv(chromes, 7, TabStatusMode::Off, Rect::new(0, 0, 36, 1), true);
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
        let view_with = cbv(with, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
        let without = chromes_from_names(&refs);
        let view_without = cbv(without, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
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
                    let view = cbv(
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
        let view = cbv(chromes, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
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
        let view = cbv(chromes, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
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
                let view = cbv(
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
        let view = cbv(chromes, 0, TabStatusMode::Off, Rect::new(0, 0, 0, 1), true);
        assert!(view.tab_hit_areas.is_empty());
        assert_eq!(view.overflow, TabBarOverflow::default());
        assert_eq!(view.new_tab_hit_area.width, 0);
    }

    #[test]
    fn non_mouse_branch_uses_centered_fill_and_no_clickable_chrome() {
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_from_names(&refs);
        let view = cbv(
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
        let view = cbv(chromes, 5, TabStatusMode::Off, Rect::new(0, 0, 40, 1), true);
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
            let view = cbv(
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
            let view = cbv(
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
    // Golden characterization of the no-offset (centered) layout (T15)
    // -----------------------------------------------------------------------

    /// Absolute path to the committed golden snapshot. The snapshot is captured
    /// from the PRE-change centered layout so the offset refactor is pinned to
    /// reproduce it byte-for-byte in no-offset mode. Set `HERDR_BLESS_GOLDEN=1`
    /// to (re)write it — only ever done against the pre-change code.
    fn golden_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui/tab_bar_centered_golden.txt")
    }

    /// The fixture matrix the golden pins: (label, tab count, active, area
    /// width, mouse_chrome, status mode, attention indices). Varied over tab
    /// count, active position, bar width, mouse chrome, and attention badges so
    /// the snapshot exercises the fit, both-side-overflow, edge-overflow, and
    /// badge-widened paths.
    struct GoldenFixture {
        label: &'static str,
        names: Vec<String>,
        active: usize,
        width: u16,
        mouse: bool,
        mode: TabStatusMode,
        attention: Vec<usize>,
    }

    fn golden_fixtures() -> Vec<GoldenFixture> {
        let n = |count: usize| (0..count).map(|i| format!("tab{i}")).collect::<Vec<_>>();
        let wide = || {
            vec![
                "你好世界".to_string(),
                "editor".to_string(),
                "👨\u{200d}💻".to_string(),
                "tab-d".to_string(),
                "넓은탭이름".to_string(),
                "f".to_string(),
                "g".to_string(),
                "h".to_string(),
            ]
        };
        let mut fx = Vec::new();
        let mut push =
            |label, names: Vec<String>, active, width, mouse, mode, attention: &[usize]| {
                fx.push(GoldenFixture {
                    label,
                    names,
                    active,
                    width,
                    mouse,
                    mode,
                    attention: attention.to_vec(),
                });
            };
        push("fit_all", n(3), 0, 60, true, TabStatusMode::Off, &[]);
        push(
            "fit_all_nomouse",
            n(3),
            1,
            60,
            false,
            TabStatusMode::Off,
            &[],
        );
        push(
            "both_overflow_mid",
            n(10),
            5,
            40,
            true,
            TabStatusMode::Off,
            &[],
        );
        push(
            "both_overflow_nomouse",
            n(10),
            5,
            40,
            false,
            TabStatusMode::Off,
            &[],
        );
        push("first_active", n(10), 0, 40, true, TabStatusMode::Off, &[]);
        push("last_active", n(10), 9, 40, true, TabStatusMode::Off, &[]);
        push("narrow", n(12), 6, 28, true, TabStatusMode::Off, &[]);
        push(
            "badge_left",
            n(10),
            5,
            40,
            true,
            TabStatusMode::All,
            &[0, 1, 2],
        );
        push(
            "badge_both",
            n(12),
            6,
            34,
            true,
            TabStatusMode::All,
            &[0, 1, 10, 11],
        );
        push("wide_unicode", wide(), 3, 30, true, TabStatusMode::Off, &[]);
        push(
            "wide_unicode_last",
            wide(),
            7,
            30,
            true,
            TabStatusMode::Off,
            &[],
        );
        push("single_tab", n(1), 0, 40, true, TabStatusMode::Off, &[]);
        fx
    }

    /// Deterministic one-line-per-field serialization of a `TabBarView`'s
    /// geometry: dense hit rects, both overflow groups (count + jump target +
    /// hit rect), and the new-tab rect. Stable across runs, so it can be
    /// diffed against the committed golden.
    fn serialize_view(label: &str, view: &TabBarView) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let rect = |r: &Rect| format!("{},{},{},{}", r.x, r.y, r.width, r.height);
        let group = |g: &Option<HiddenGroup>| match g {
            Some(g) => format!("count={} jump={}", g.count, g.jump_to),
            None => "none".to_string(),
        };
        let _ = writeln!(s, "== {label} ==");
        for (i, r) in view.tab_hit_areas.iter().enumerate() {
            let _ = writeln!(s, "  tab[{i}]={}", rect(r));
        }
        let _ = writeln!(s, "  left={}", group(&view.overflow.left));
        let _ = writeln!(s, "  right={}", group(&view.overflow.right));
        let _ = writeln!(s, "  left_hit={}", rect(&view.overflow.left_hit_area));
        let _ = writeln!(s, "  right_hit={}", rect(&view.overflow.right_hit_area));
        let _ = writeln!(s, "  new_tab={}", rect(&view.new_tab_hit_area));
        s
    }

    fn compute_golden_snapshot() -> String {
        let mut out = String::new();
        for fx in golden_fixtures() {
            let refs: Vec<&str> = fx.names.iter().map(String::as_str).collect();
            let chromes = chromes_with_attention(&refs, &fx.attention);
            let view = cbv(
                chromes,
                fx.active,
                fx.mode,
                Rect::new(0, 0, fx.width, 1),
                fx.mouse,
            );
            out.push_str(&serialize_view(fx.label, &view));
        }
        out
    }

    #[test]
    fn no_offset_layout_matches_pre_change_golden() {
        let snapshot = compute_golden_snapshot();
        let path = golden_path();
        // Only (re)write under an explicit bless. A missing golden must FAIL,
        // not self-heal: auto-writing when the file is absent would regenerate
        // it from the current code and make the assertion tautological,
        // defeating the characterization guarantee on any tree where the
        // committed golden is missing.
        if std::env::var("HERDR_BLESS_GOLDEN").is_ok() {
            std::fs::write(&path, &snapshot).expect("write golden snapshot");
        }
        let golden = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "golden snapshot missing or unreadable at {path:?} ({e}); \
                 re-bless against pre-change code with HERDR_BLESS_GOLDEN=1"
            )
        });
        assert_eq!(
            snapshot, golden,
            "no-offset layout drifted from the pre-change golden at {path:?}; \
             re-bless only against pre-change code with HERDR_BLESS_GOLDEN=1"
        );
    }

    // -----------------------------------------------------------------------
    // Scrolled-window geometry (offset-aware layout: T1-T7, T6b, T19)
    // -----------------------------------------------------------------------

    /// The badge classifier `max_scroll_offset` expects (no attention anywhere).
    fn no_attention_state(_i: usize) -> (crate::detect::AgentState, bool) {
        (crate::detect::AgentState::Unknown, true)
    }

    #[test]
    fn t1_scrolled_fill_left_packs_from_offset() {
        // T1: with an offset, the window starts exactly at `offset` and packs
        // rightward with abutting widths; nothing left of the offset is visible.
        let names: Vec<String> = (0..12).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let chromes = chromes_from_names(&refs);
        let (view, resolved) = compute_tab_bar_view(
            chromes,
            0,
            TabStatusMode::Off,
            Rect::new(0, 0, 40, 1),
            true,
            Some(3),
        );
        assert_eq!(resolved, Some(3), "mid-range offset is unchanged");
        // Tabs before the offset are hidden.
        for i in 0..3 {
            assert_eq!(
                view.tab_hit_areas[i].width, 0,
                "tab {i} before offset hidden"
            );
        }
        // Offset tab is the first visible one.
        let (first, last) = visible_bounds(&view.tab_hit_areas).expect("some visible");
        assert_eq!(first, 3, "window starts at the offset");
        // Visible run is contiguous and abuts.
        for i in first..last {
            let a = view.tab_hit_areas[i];
            let b = view.tab_hit_areas[i + 1];
            assert!(a.width > 0 && b.width > 0, "contiguous visible run");
            assert_eq!(b.x, a.x + a.width, "tabs {i},{} abut", i + 1);
        }
        // Left indicator counts exactly the hidden-left tabs (0..offset).
        assert_eq!(view.overflow.left.expect("left overflow").count, 3);
    }

    #[test]
    fn t2_clamp_and_non_overflow_resolution() {
        let names: Vec<String> = (0..12).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let area = Rect::new(0, 0, 40, 1);
        // Offset past max clamps; last tab fully visible at the clamped offset.
        let (view, resolved) = compute_tab_bar_view(
            chromes_from_names(&refs),
            0,
            TabStatusMode::Off,
            area,
            true,
            Some(9999),
        );
        let last = refs.len() - 1;
        let max = resolved.expect("overflowing strip resolves to an offset");
        let expected_max = max_scroll_offset(
            &chromes_from_names(&refs),
            area.width.saturating_sub(NEW_TAB_WIDTH),
            true,
            &no_attention_state,
        );
        assert_eq!(max, expected_max, "clamped to max_scroll");
        assert!(
            view.tab_hit_areas[last].width > 0,
            "last tab fully visible at max_scroll"
        );
        assert!(view.overflow.right.is_none(), "nothing hidden right at max");

        // A non-overflowing strip resolves any offset to no-offset (centered).
        let (view, resolved) = compute_tab_bar_view(
            chromes_from_names(&["ab", "cd", "ef"]),
            0,
            TabStatusMode::Off,
            Rect::new(0, 0, 60, 1),
            true,
            Some(2),
        );
        assert_eq!(
            resolved, None,
            "non-overflowing strip resolves to no-offset"
        );
        assert!(view.tab_hit_areas.iter().all(|r| r.width > 0));
        assert_eq!(view.overflow, TabBarOverflow::default());
    }

    #[test]
    fn t3_overflow_counts_at_offsets_0_mid_max() {
        let names: Vec<String> = (0..12).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let area = Rect::new(0, 0, 40, 1);
        let n = refs.len();
        let max = max_scroll_offset(
            &chromes_from_names(&refs),
            area.width.saturating_sub(NEW_TAB_WIDTH),
            true,
            &no_attention_state,
        );
        for offset in [0usize, 3, max] {
            let (view, resolved) = compute_tab_bar_view(
                chromes_from_names(&refs),
                0,
                TabStatusMode::Off,
                area,
                true,
                Some(offset),
            );
            let o = resolved.expect("overflowing");
            let visible = view.tab_hit_areas.iter().filter(|r| r.width > 0).count();
            let left = view.overflow.left.map(|g| g.count).unwrap_or(0);
            let right = view.overflow.right.map(|g| g.count).unwrap_or(0);
            assert_eq!(left, o, "hidden-left count == offset at offset {o}");
            assert_eq!(left + visible + right, n, "counts sum to tab count");
            if o == 0 {
                assert!(view.overflow.left.is_none(), "no left indicator at 0");
            }
            if o == max {
                assert!(view.overflow.right.is_none(), "no right indicator at max");
            }
        }
    }

    #[test]
    fn t4_clamp_stays_consistent_with_badge_widened_indicators() {
        // T4: attention badges on hidden-left tabs widen the left indicator; the
        // clamp must derive max_scroll from the same badge-aware reserve, so the
        // last tab stays fully visible and indicators never overlap at the
        // boundary.
        let names: Vec<String> = (0..12).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        for width in [30u16, 36, 40, 46] {
            let area = Rect::new(0, 0, width, 1);
            let chromes = chromes_with_attention(&refs, &[0, 1, 2, 3]);
            let (view, resolved) =
                compute_tab_bar_view(chromes, 0, TabStatusMode::All, area, true, Some(9999));
            let last = refs.len() - 1;
            assert!(
                view.tab_hit_areas[last].width > 0,
                "width={width}: last tab visible at clamped max"
            );
            assert!(resolved.is_some(), "width={width}: resolves");
            for ind in [view.overflow.left_hit_area, view.overflow.right_hit_area] {
                if ind.width == 0 {
                    continue;
                }
                for rect in view.tab_hit_areas.iter().filter(|r| r.width > 0) {
                    let (ind_lo, ind_hi) = (ind.x, ind.x + ind.width);
                    let (lo, hi) = (rect.x, rect.x + rect.width);
                    assert!(
                        ind_hi <= lo || hi <= ind_lo,
                        "width={width}: indicator overlaps a visible tab at the boundary"
                    );
                }
            }
        }
    }

    #[test]
    fn t5_dense_hit_areas_and_unicode_in_scrolled_mode() {
        // T5: dense hit-area invariant holds in scrolled mode, including
        // adversarial unicode names.
        let chromes_names = &[
            "你好世界",
            "e\u{0301}ditor",
            "👨\u{200d}💻",
            "tab-d",
            "넓은탭이름",
            "f",
            "g",
            "h",
        ];
        for offset in [0usize, 2, 4, 7, 99] {
            for width in [20u16, 30, 40] {
                let chromes = chromes_from_names(chromes_names);
                let (view, _) = compute_tab_bar_view(
                    chromes,
                    0,
                    TabStatusMode::Off,
                    Rect::new(0, 0, width, 1),
                    true,
                    Some(offset),
                );
                assert_eq!(
                    view.tab_hit_areas.len(),
                    chromes_names.len(),
                    "dense hit areas in scrolled mode (offset={offset} width={width})"
                );
            }
        }
    }

    #[test]
    fn t6_adversarial_inputs_no_panic() {
        // T6: offset far past tab count, single over-wide tab, one-tab strip,
        // empty strip: no panic, non-empty window for any non-empty strip.
        // Offset far past the tab count.
        let (view, resolved) = compute_tab_bar_view(
            chromes_from_names(&["t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7", "t8", "t9"]),
            0,
            TabStatusMode::Off,
            Rect::new(0, 0, 30, 1),
            true,
            Some(usize::MAX),
        );
        assert!(resolved.map(|o| o < 10).unwrap_or(true));
        assert!(view.tab_hit_areas.iter().any(|r| r.width > 0));

        // Single tab far wider than the bar: clipped, still non-empty.
        let (view, _) = compute_tab_bar_view(
            chromes_from_names(&["a-very-very-wide-single-tab-name-that-exceeds-the-bar"]),
            0,
            TabStatusMode::Off,
            Rect::new(0, 0, 10, 1),
            true,
            Some(0),
        );
        assert_eq!(view.tab_hit_areas.len(), 1);

        // One-tab strip with a large offset.
        let (view, resolved) = compute_tab_bar_view(
            chromes_from_names(&["only"]),
            0,
            TabStatusMode::Off,
            Rect::new(0, 0, 40, 1),
            true,
            Some(50),
        );
        assert_eq!(view.tab_hit_areas.len(), 1);
        assert_eq!(resolved, None, "single fitting tab resolves to no-offset");

        // Empty strip.
        let (view, resolved) = compute_tab_bar_view(
            Vec::new(),
            0,
            TabStatusMode::Off,
            Rect::new(0, 0, 40, 1),
            true,
            Some(3),
        );
        assert!(view.tab_hit_areas.is_empty());
        assert_eq!(resolved, None);
    }

    #[test]
    fn t6b_window_is_consistent_rejects_bad_windows() {
        // T6b: the free predicate rejects hand-crafted bad windows.
        let area = Rect::new(0, 0, 40, 1);
        // Empty window (nothing visible).
        assert!(
            !window_is_consistent(&[Rect::default(), Rect::default()], area, 0),
            "empty window is inconsistent"
        );
        // Packed width exceeds the usable area.
        let overrun = vec![Rect::new(0, 0, 20, 1), Rect::new(20, 0, 30, 1)];
        assert!(
            !window_is_consistent(&overrun, area, 0),
            "over-wide packing is inconsistent"
        );
        // Clipped last tab with nothing hidden on the right.
        let clipped_last = vec![Rect::new(0, 0, 10, 1), Rect::default()];
        assert!(
            !window_is_consistent(&clipped_last, area, 0),
            "clipped last tab with nothing hidden right is inconsistent"
        );
        // A well-formed window with something hidden right is consistent even
        // when the last dense rect is zero-width (it is legitimately hidden).
        let ok = vec![
            Rect::new(0, 0, 10, 1),
            Rect::new(10, 0, 10, 1),
            Rect::default(),
        ];
        assert!(
            window_is_consistent(&ok, area, 1),
            "hidden-right last tab is fine"
        );
    }

    #[test]
    fn scrolled_fallback_recenters_on_inconsistent_window() {
        // AC6 (compute-path arm): drive `compute_tab_bar_view` itself onto the
        // defensive fallback, not just the `window_is_consistent` predicate. A
        // pathologically narrow bar with many attention-badged hidden-left tabs
        // widens the left indicator until its reserve is as wide as the usable
        // area, so even the clamped last-tab window packs to nothing and the
        // consistency check fails. The compute path must then discard the offset
        // (resolve to `None`), fall back to the centered fill, and keep the
        // dense hit-area invariant — no panic, no all-zero row surfaced.
        //
        // Search a small width/tab-count grid for a configuration that actually
        // trips the fallback (the exact trigger depends on badge-width math), so
        // the test stays valid if indicator sizing shifts. `found` guards that
        // we truly exercised the branch rather than passing vacuously.
        let mut found = false;
        'search: for count in [12usize, 16, 20] {
            let names: Vec<String> = (0..count).map(|i| format!("t{i}")).collect();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            let attention: Vec<usize> = (0..count.saturating_sub(1)).collect();
            for width in 6u16..=16 {
                let area = Rect::new(0, 0, width, 1);
                // Request the far-right offset so the clamp lands at n-1, the
                // window with the widest possible hidden-left reserve.
                let chromes = chromes_with_attention(&refs, &attention);
                let (view, resolved) = compute_tab_bar_view(
                    chromes,
                    0,
                    TabStatusMode::All,
                    area,
                    true,
                    Some(usize::MAX),
                );
                // Invariants that must hold on EVERY outcome (fallback or not):
                // dense hit areas, and no rect escaping the bar.
                assert_eq!(
                    view.tab_hit_areas.len(),
                    count,
                    "dense hit areas (w={width})"
                );
                for r in view.tab_hit_areas.iter().filter(|r| r.width > 0) {
                    assert!(
                        r.x + r.width <= area.x + area.width,
                        "rect escapes bar (w={width})"
                    );
                }
                // The fallback re-centers, so the active tab (0) is placed by the
                // centered fill and is visible; the offset resolves to None.
                if resolved.is_none() && view.tab_hit_areas[0].width > 0 {
                    found = true;
                    break 'search;
                }
            }
        }
        assert!(
            found,
            "expected some narrow-bar/badged config to trip the compute-path fallback"
        );
    }

    #[test]
    fn t7_active_tab_outside_window_has_zero_width_and_no_active_styling() {
        // T7: an active tab outside the scrolled window renders zero-width and
        // no active (accent) styling appears anywhere in the buffer.
        let names: Vec<String> = (0..12).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let area = Rect::new(0, 0, 40, 1);
        let mut app = AppState::test_new();
        let mut ws = make_ws_with_tabs(&refs);
        ws.active_tab = 0; // active is far left; scroll away from it
        app.workspaces = vec![ws];
        app.active = Some(0);
        app.mouse_capture = true;
        app.view.tab_bar_rect = area;
        let chromes = chromes_from_ws(&app.workspaces[0]);
        let (view, resolved) =
            compute_tab_bar_view(chromes, 0, TabStatusMode::Off, area, true, Some(6));
        assert!(resolved.is_some());
        assert_eq!(
            view.tab_hit_areas[0].width, 0,
            "active tab outside the window is zero-width"
        );
        app.view.tab_hit_areas = view.tab_hit_areas;
        app.view.tab_chrome = view.tab_chrome;
        app.view.tab_status_mode = view.tab_status_mode;
        app.view.tab_overflow = view.overflow;
        app.view.new_tab_hit_area = view.new_tab_hit_area;
        let buffer = render_to_buffer(&app, area);
        for x in area.x..area.x + area.width {
            assert_ne!(
                buffer[(x, 0)].bg,
                app.palette.accent,
                "no active-accent styling at x={x} when active tab is off-window"
            );
        }
    }

    #[test]
    fn t19_property_sweep_offsets_within_area_and_non_overlapping() {
        // T19: sweep (tab count × name widths × offset × mouse_chrome). All hit
        // areas lie within the area and never overlap; the last tab is fully
        // visible exactly when the clamped offset equals max_scroll.
        let widths_sets: &[&[&str]] = &[
            &["t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7"],
            &["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta"],
        ];
        for names in widths_sets {
            for mouse in [false, true] {
                for area_w in [24u16, 32, 44] {
                    let area = Rect::new(0, 0, area_w, 1);
                    let usable = if mouse {
                        area_w.saturating_sub(NEW_TAB_WIDTH)
                    } else {
                        area_w
                    };
                    let max = max_scroll_offset(
                        &chromes_from_names(names),
                        usable,
                        mouse,
                        &no_attention_state,
                    );
                    for offset in 0..=names.len() + 1 {
                        let (view, resolved) = compute_tab_bar_view(
                            chromes_from_names(names),
                            0,
                            TabStatusMode::Off,
                            area,
                            mouse,
                            Some(offset),
                        );
                        // All rects inside the bar.
                        for r in view.tab_hit_areas.iter().filter(|r| r.width > 0) {
                            assert!(
                                r.x >= area.x && r.x + r.width <= area.x + area.width,
                                "rect outside area (names={names:?} mouse={mouse} w={area_w} off={offset})"
                            );
                        }
                        // Non-overlapping visible rects.
                        let mut vis: Vec<Rect> = view
                            .tab_hit_areas
                            .iter()
                            .copied()
                            .filter(|r| r.width > 0)
                            .collect();
                        vis.sort_by_key(|r| r.x);
                        for pair in vis.windows(2) {
                            assert!(
                                pair[0].x + pair[0].width <= pair[1].x,
                                "overlap (names={names:?} mouse={mouse} w={area_w} off={offset})"
                            );
                        }
                        // Last tab fully visible iff clamped offset == max.
                        if let Some(o) = resolved {
                            let last = names.len() - 1;
                            let last_visible =
                                view.tab_hit_areas[last].width > 0 && view.overflow.right.is_none();
                            assert_eq!(
                                last_visible,
                                o == max,
                                "last-visible mismatch (names={names:?} mouse={mouse} w={area_w} off={o} max={max})"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn scrolled_mid_offset_badge_indicator_no_overlap() {
        // The badge-aware reserve-convergence loop is now shared with the
        // scrolled fill. Exercise a mid-range offset whose hidden-left range
        // carries attention badges (widening the left indicator) and assert the
        // indicators never overlap a visible tab — the scrolled analog of
        // `indicator_hit_areas_never_overlap_visible_tabs`, at an offset short
        // of max_scroll so both indicators are present.
        let names: Vec<String> = (0..14).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        for width in [30u16, 36, 42, 48] {
            for offset in [2usize, 3, 4] {
                let area = Rect::new(0, 0, width, 1);
                // Attention on hidden-left tabs (indices < offset) widens the
                // left indicator past its plain-count width.
                let chromes = chromes_with_attention(&refs, &[0, 1, 2]);
                let (view, resolved) =
                    compute_tab_bar_view(chromes, 0, TabStatusMode::All, area, true, Some(offset));
                // Only meaningful while an offset actually resolved (overflow).
                if resolved.is_none() {
                    continue;
                }
                for ind in [view.overflow.left_hit_area, view.overflow.right_hit_area] {
                    if ind.width == 0 {
                        continue;
                    }
                    let (ind_lo, ind_hi) = (ind.x, ind.x + ind.width);
                    for rect in view.tab_hit_areas.iter().filter(|r| r.width > 0) {
                        let (lo, hi) = (rect.x, rect.x + rect.width);
                        assert!(
                            ind_hi <= lo || hi <= ind_lo,
                            "width={width} offset={offset}: scrolled indicator overlaps a visible tab"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn build_tab_bar_inputs_resolves_no_scroll_offset() {
        // With no browse state, the seam resolves to no offset (resting
        // centered fill) — the behavior-neutral path.
        let mut app = AppState::test_new();
        app.workspaces = vec![make_ws_with_tabs(&["a", "b", "c"])];
        app.active = Some(0);
        let (_chromes, _active, _mode, offset) = crate::ui::build_tab_bar_inputs(
            &app.workspaces[0],
            &app.terminals,
            TabStatusMode::Off,
            0,
            &app.palette,
            None,
        );
        assert_eq!(offset, None, "resting layout resolves to no scroll offset");
    }

    #[test]
    fn build_tab_bar_inputs_resolves_browse_offset_when_anchor_matches() {
        // A browse state whose anchor matches the active tab resolves to its
        // `first_visible` offset; a mismatched anchor (wrong workspace id or
        // wrong tab number) resolves to `None`, which the compute path uses to
        // exit browse mode.
        let mut app = AppState::test_new();
        let ws = make_ws_with_tabs(&["a", "b", "c"]);
        let ws_id = ws.id.clone();
        let active_number = ws.tabs[ws.active_tab].number;
        app.workspaces = vec![ws];
        app.active = Some(0);

        let matching = crate::app::state::TabScroll {
            first_visible: 1,
            anchor_workspace_id: ws_id.clone(),
            anchor_tab_number: active_number,
        };
        let (.., offset) = crate::ui::build_tab_bar_inputs(
            &app.workspaces[0],
            &app.terminals,
            TabStatusMode::Off,
            0,
            &app.palette,
            Some(&matching),
        );
        assert_eq!(
            offset,
            Some(1),
            "matching anchor resolves the browse offset"
        );

        let wrong_number = crate::app::state::TabScroll {
            first_visible: 1,
            anchor_workspace_id: ws_id,
            anchor_tab_number: active_number + 999,
        };
        let (.., offset) = crate::ui::build_tab_bar_inputs(
            &app.workspaces[0],
            &app.terminals,
            TabStatusMode::Off,
            0,
            &app.palette,
            Some(&wrong_number),
        );
        assert_eq!(offset, None, "mismatched anchor exits browse mode");

        let wrong_ws = crate::app::state::TabScroll {
            first_visible: 1,
            anchor_workspace_id: "some-other-workspace".to_string(),
            anchor_tab_number: active_number,
        };
        let (.., offset) = crate::ui::build_tab_bar_inputs(
            &app.workspaces[0],
            &app.terminals,
            TabStatusMode::Off,
            0,
            &app.palette,
            Some(&wrong_ws),
        );
        assert_eq!(
            offset, None,
            "anchor for another workspace exits browse mode"
        );
    }

    // -----------------------------------------------------------------------
    // TabChrome: width + spans + Unicode + sanitization
    // -----------------------------------------------------------------------

    #[test]
    fn zoom_marker_counts_toward_tab_width() {
        // tab_width = display_width (8 name + 2 zoom) + 2 interior padding
        //           (one space each side, zellij) + 2 separator overhead
        //           (zellij left+right arrow cols).
        let chrome = TabChrome {
            status: None,
            name: "abcdefgh".into(),
            zoomed: true,
            is_attention: false,
            agent_state: None,
        };
        assert_eq!(tab_width(&chrome), 14);
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
        assert_eq!(c.display_width(), 4);

        // Combining mark contributes 0 columns: "a" + U+0300 = 1 column.
        let c = TabChrome {
            status: None,
            name: "a\u{0300}".into(),
            zoomed: false,
            is_attention: false,
            agent_state: None,
        };
        assert_eq!(c.display_width(), 1);
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
        assert_eq!(c.display_width(), 5);
        let c = TabChrome {
            status: None,
            name: "hello".into(),
            zoomed: true,
            is_attention: false,
            agent_state: None,
        };
        assert_eq!(c.display_width(), 7);
        // A dot-less chrome has NO status reserve — width is name-only
        // regardless of the display mode (the dot Option already encodes
        // whether anything renders).
        let c = TabChrome {
            status: None,
            name: "hello".into(),
            zoomed: false,
            is_attention: false,
            agent_state: None,
        };
        assert_eq!(c.display_width(), 5);
        // A dotted chrome adds the dot's width + 1 separating space: 5 name
        // + 2 zoom + 2 dot = 9.
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
        assert_eq!(c.display_width(), 9);
    }

    #[test]
    fn dot_adds_exactly_its_width_plus_one() {
        // AC3: a dotted tab is exactly dot.width() + 1 cols wider than its
        // dot-less form — a 1-col glyph adds 2, a synthetic 2-col glyph adds 3.
        let base = TabChrome {
            status: None,
            name: "hello".into(),
            zoomed: false,
            is_attention: false,
            agent_state: None,
        };
        let narrow_dot = TabChrome {
            status: Some(TabStatusDot {
                glyph: "●",
                style: Style::default(),
            }),
            ..base.clone()
        };
        assert_eq!("●".width(), 1);
        assert_eq!(tab_width(&narrow_dot), tab_width(&base) + 2);
        // Synthetic wide dot: all real dots are 1 col today (the glyph set is
        // width-uniform), so the 2-col branch is exercised synthetically.
        let wide_dot = TabChrome {
            status: Some(TabStatusDot {
                glyph: "字",
                style: Style::default(),
            }),
            ..base.clone()
        };
        assert_eq!("字".width(), 2);
        assert_eq!(tab_width(&wide_dot), tab_width(&base) + 3);
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
        let spans = c.to_spans(15);
        assert_eq!(spans[0].content.as_ref(), " ");
        assert_eq!(spans[1].content.as_ref(), "●");
        assert_eq!(spans[2].content.as_ref(), " ");
        assert_eq!(spans[3].content.as_ref(), "test");
        assert_eq!(spans[4].content.as_ref(), " Z");
        assert_eq!(spans[5].content.len(), 6);

        // Dot-less chrome: symmetric ` name ` at the natural interior width —
        // no phantom two-space status slot.
        let c = TabChrome {
            status: None,
            name: "abc".into(),
            zoomed: false,
            is_attention: false,
            agent_state: None,
        };
        let spans = c.to_spans(5);
        assert_eq!(spans[0].content.as_ref(), " ");
        assert_eq!(spans[1].content.as_ref(), "abc");
        assert_eq!(spans[2].content.as_ref(), " ");
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, " abc ", "interior is symmetric ` name `");

        let c = TabChrome {
            status: None,
            name: "xyz".into(),
            zoomed: false,
            is_attention: false,
            agent_state: None,
        };
        let spans = c.to_spans(8);
        assert_eq!(spans[0].content.as_ref(), " ");
        assert_eq!(spans[1].content.as_ref(), "xyz");
        assert_eq!(spans[2].content.len(), 4);
    }

    #[test]
    fn to_spans_never_truncates_or_adds_ellipsis() {
        // C5/AC6: zellij never renders an ellipsis. An over-wide name is
        // emitted at natural width — the render path's fixed interior rect
        // clips it at the bar edge. No `…` appears in any span, and the
        // emitted content is the natural ` name` prefix.
        let c = TabChrome {
            status: None,
            name: "longername".into(),
            zoomed: false,
            is_attention: false,
            agent_state: None,
        };
        let spans = c.to_spans(5);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(!text.contains('…'), "no ellipsis ever: {text:?}");
        assert_eq!(text, " longername", "natural name, clipped by the rect");
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
        let spans = c.to_spans(8);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("abc"));
        assert!(!text.contains('…'));
    }

    #[test]
    fn dotless_main_tab_is_eight_columns_like_zellij() {
        // AC1: ` main ` between two arrow cells = 8 cols, zellij's per-tab
        // column count for the same tab.
        let chrome = TabChrome {
            status: None,
            name: "main".into(),
            zoomed: false,
            is_attention: false,
            agent_state: None,
        };
        assert_eq!(tab_width(&chrome), 8);
        // And the emitted interior is exactly ` main `.
        let spans = chrome.to_spans(6);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, " main ");
    }

    #[test]
    fn no_phantom_status_reserve_when_no_dot() {
        // AC2: a dot-less chrome has the same width whether tab status display
        // is on or off — the dot Option is the only signal, so "all" mode with
        // no dots present costs zero columns.
        let dotless = TabChrome {
            status: None,
            name: "server".into(),
            zoomed: false,
            is_attention: false,
            agent_state: Some((crate::detect::AgentState::Unknown, true)),
        };
        // Width is a pure function of the chrome; both modes see the same
        // chrome when no dot was built.
        assert_eq!(tab_width(&dotless), 6 + 2 + 2);
        // Interior symmetric under Powerline (peeled interior)…
        let text: String = dotless
            .to_spans(8)
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(text, " server ");
    }

    #[test]
    fn alternating_bg_interior_is_symmetric() {
        // AC2 (AlternatingBg branch): with powerline off the label still
        // renders symmetric ` name ` — one padding column each side from the
        // separator overhead, then the one-space interior padding.
        let mut app = app_with_tab_bar(&["abc", "def"], 0, Rect::new(0, 0, 40, 1), false);
        app.tabs_powerline = false;
        let rect = app.view.tab_hit_areas[1];
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        let row: String = (rect.x..rect.x + rect.width)
            .map(|x| buffer[(x, 0)].symbol())
            .collect();
        assert_eq!(
            row, "  def  ",
            "AlternatingBg tab cell: 1 separator-pad + ` def ` interior + 1 \
             separator-pad — symmetric"
        );
        // The name starts at col 2: separator pad + one interior space —
        // symmetric with the Powerline branch, no 1-left/3-right asymmetry.
        assert_eq!(buffer[(rect.x + 2, 0)].symbol(), "d");
    }

    #[test]
    fn denser_than_before_for_short_names() {
        // AC5: at a fixed bar width and tab set, the new sizing shows at least
        // as many tabs as the old (old width: name + 2 status + 4 pad + 2
        // arrows, min 10), and strictly more for ≤8-char names.
        let names: Vec<String> = (0..12).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let old_tab_width = |chrome: &TabChrome| -> u16 {
            // The pre-change formula under show_tab_status="all".
            let name_w = u16::try_from(chrome.name.width()).unwrap_or(u16::MAX);
            (name_w + 2 + 4 + 2).max(10)
        };
        for width in [30u16, 40, 50, 60] {
            let chromes = chromes_from_names(&refs);
            let old_fit: u16 = {
                // Old capacity under the same chrome budget: bar minus the
                // ` + ` button (3) minus the old fixed indicator (6), divided
                // by the old per-tab width (uniform 2-char names → 10).
                let per = old_tab_width(&chromes[0]);
                width.saturating_sub(NEW_TAB_WIDTH + 6) / per
            };
            let view = cbv(
                chromes,
                0,
                TabStatusMode::All,
                Rect::new(0, 0, width, 1),
                true,
            );
            let visible = view.tab_hit_areas.iter().filter(|r| r.width > 0).count();
            assert!(
                visible as u16 >= old_fit,
                "width={width}: {visible} visible < old capacity {old_fit}"
            );
            assert!(
                visible as u16 > old_fit,
                "width={width}: strictly denser for 2-char names"
            );
        }
    }

    #[test]
    fn indicator_interior_matches_zellij_format() {
        // AC7: interiors read exactly ` ← +N ` / ` +N → ` — full count,
        // ` +many ` past 9999, no `9+`, no `←N ` short form. Swept over the
        // non-mouse and mouse paths (no attention, so no badge segments).
        assert_eq!(indicator_count_text(7), "7");
        assert_eq!(indicator_count_text(42), "42");
        assert_eq!(indicator_count_text(9999), "9999");
        assert_eq!(indicator_count_text(10000), "many");

        for mouse in [false, true] {
            let names: Vec<String> = (0..15).map(|i| format!("t{i}")).collect();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            // Active last → >9 hidden on the left only.
            let app = app_with_tab_bar(&refs, 14, Rect::new(0, 0, 30, 1), mouse);
            let count = app.view.tab_overflow.left.expect("left overflow").count;
            assert!(count > 9);
            let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
            let row = buffer_row_text(&buffer, app.view.tab_bar_rect, 0);
            assert!(
                row.contains(&format!("← +{count}")),
                "mouse={mouse}: zellij left format with full count: {row:?}"
            );
            assert!(!row.contains("9+"), "mouse={mouse}: no 9+ cap: {row:?}");
        }
    }

    #[test]
    fn indicator_count_cols_matches_count_text_width() {
        // The allocation-free sizing arithmetic must agree with the rendered
        // text's measured width for every count regime.
        for count in [
            0usize, 1, 5, 9, 10, 42, 99, 100, 999, 1000, 9999, 10000, 123456,
        ] {
            assert_eq!(
                indicator_count_cols(count) as usize,
                indicator_count_text(count).width(),
                "count={count}"
            );
        }
    }

    #[test]
    fn indicator_reserved_width_equals_rendered_width_on_all_paths() {
        // AC8: `tab_indicator_width` must size from the same uncapped count
        // the render emits, on all three paths — non-mouse, mouse with zero
        // attention, mouse with attention — at ≥100 and `many` counts.
        use super::super::overflow::OverflowSide;
        let no_attention = OverflowSide::default();
        let with_attention = OverflowSide {
            hidden: 120,
            hidden_blocked: 2,
            blocked_jump_to: Some(1),
            ..OverflowSide::default()
        };
        for count in [1usize, 9, 42, 120, 10000] {
            let text = indicator_count_text(count);
            // ` ← +N ` interior = 5 + count columns; +2 separator cols.
            let base_demand =
                5 + u16::try_from(text.width()).unwrap_or(u16::MAX) + TAB_SEPARATOR_OVERHEAD;
            // Non-mouse and mouse-no-attention agree (no badge segments).
            for (mouse, side) in [(false, no_attention), (true, no_attention)] {
                assert_eq!(
                    tab_indicator_width(count, side, mouse),
                    base_demand.max(OVERFLOW_INDICATOR_WIDTH),
                    "count={count} mouse={mouse}"
                );
            }
            // Mouse + attention adds exactly the badge columns.
            let badge_w = super::super::overflow::badge_attention_width(with_attention);
            assert_eq!(
                tab_indicator_width(count, with_attention, true),
                (base_demand + badge_w).max(OVERFLOW_INDICATOR_WIDTH),
                "count={count} with attention"
            );
        }
    }

    #[test]
    fn indicator_no_overlap_at_large_counts_all_paths() {
        // AC8 extension of `indicator_hit_areas_never_overlap_visible_tabs`:
        // a ≥100 hidden count must not overrun the adjacent tab on the
        // non-mouse path or the mouse-no-attention path (previously only
        // mouse+attention was swept).
        let names: Vec<String> = (0..120).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        for mouse in [false, true] {
            for active in [5usize, 115] {
                for width in [28u16, 34, 40, 46] {
                    let chromes = chromes_from_names(&refs);
                    let view = cbv(
                        chromes,
                        active,
                        TabStatusMode::Off,
                        Rect::new(0, 0, width, 1),
                        mouse,
                    );
                    let left = view.overflow.left.map(|g| g.count).unwrap_or(0);
                    let right = view.overflow.right.map(|g| g.count).unwrap_or(0);
                    assert!(left >= 100 || right >= 100, "large count on one side");
                    for ind in [view.overflow.left_hit_area, view.overflow.right_hit_area] {
                        if ind.width == 0 {
                            continue;
                        }
                        for rect in view.tab_hit_areas.iter().filter(|r| r.width > 0) {
                            assert!(
                                ind.x + ind.width <= rect.x || rect.x + rect.width <= ind.x,
                                "mouse={mouse} active={active} width={width}: \
                                 indicator {ind:?} overlaps tab {rect:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn right_indicator_abuts_last_visible_tab_compactly() {
        // AC10 / C7: the right indicator's x equals the last visible tab's
        // right edge AND its width is the demanded indicator width — a compact
        // tile, not stretched across the former gap — and the ` + ` button
        // abuts the indicator.
        let names: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        for width in [30u16, 36, 42, 48, 54] {
            let chromes = chromes_from_names(&refs);
            let view = cbv(
                chromes.clone(),
                0,
                TabStatusMode::Off,
                Rect::new(0, 0, width, 1),
                true,
            );
            let Some(right) = view.overflow.right else {
                continue;
            };
            let last_right = view
                .tab_hit_areas
                .iter()
                .filter(|r| r.width > 0)
                .map(|r| r.x + r.width)
                .max()
                .expect("visible tabs");
            let ind = view.overflow.right_hit_area;
            assert_eq!(ind.x, last_right, "width={width}: indicator abuts last tab");
            let demanded = tab_indicator_width(right.count, right.side, true);
            assert_eq!(
                ind.width, demanded,
                "width={width}: compact tile at demanded width, not stretched"
            );
            // ` + ` button abuts the indicator.
            assert_eq!(
                view.new_tab_hit_area.x,
                ind.x + ind.width,
                "width={width}: + button follows the indicator"
            );
        }
    }

    #[test]
    fn new_tab_button_abuts_last_tab_when_nothing_overflows() {
        // AC10 mirror: no overflow → the ` + ` button abuts the last tab.
        let view = cbv(
            chromes_from_names(&["ab", "cd"]),
            0,
            TabStatusMode::Off,
            Rect::new(0, 0, 60, 1),
            true,
        );
        assert!(view.overflow.right.is_none());
        let last_right = view
            .tab_hit_areas
            .iter()
            .filter(|r| r.width > 0)
            .map(|r| r.x + r.width)
            .max()
            .expect("visible tabs");
        assert_eq!(view.new_tab_hit_area.x, last_right);
    }

    #[test]
    fn status_cols_lockstep_between_sizing_and_fill() {
        // The single-source guard: for the same chrome, the columns
        // `display_width` sizes for the status slot equal the columns
        // `to_spans` actually emits for it. Measured as the delta each
        // function shows between the dotted and dot-less form of the same
        // chrome, swept over dot widths.
        for glyph in ["●", "◉", "✓", "○", "⠋", "字"] {
            let dotless = TabChrome {
                status: None,
                name: "name".into(),
                zoomed: false,
                is_attention: false,
                agent_state: None,
            };
            let dotted = TabChrome {
                status: Some(TabStatusDot {
                    glyph,
                    style: Style::default(),
                }),
                ..dotless.clone()
            };
            let sizing_delta = dotted.display_width() - dotless.display_width();
            assert_eq!(sizing_delta, dotted.status_cols(), "glyph={glyph:?}");

            let emitted = |c: &TabChrome| -> u16 {
                let wide = 40; // room so padding never masks the slot
                c.to_spans(wide)
                    .iter()
                    .take_while(|s| s.content.as_ref() != "name")
                    .fold(0u16, |acc, s| {
                        acc + u16::try_from(s.content.width()).unwrap_or(u16::MAX)
                    })
            };
            let fill_delta = emitted(&dotted) - emitted(&dotless);
            assert_eq!(
                fill_delta, sizing_delta,
                "glyph={glyph:?}: sizing and fill disagree on status columns"
            );
        }
    }

    #[test]
    fn budget_consistency_across_separator_styles() {
        // For every (dot, zoom, name, rect_width) — including the clip regime
        // where rect_width < natural content — no fitting name is shortened
        // and the emitted content equals rect_width when it fits. Both render
        // conventions pass the interior (rect minus the 2 separator cols) to
        // to_spans, so one sweep covers Powerline and AlternatingBg.
        let dots = [None, Some("●"), Some("字")];
        let names = ["a", "name", "longer-name", "你好世界"];
        for dot in dots {
            for zoomed in [false, true] {
                for name in names {
                    let chrome = TabChrome {
                        status: dot.map(|glyph| TabStatusDot {
                            glyph,
                            style: Style::default(),
                        }),
                        name: name.into(),
                        zoomed,
                        is_attention: false,
                        agent_state: None,
                    };
                    let natural = chrome.display_width() + 2; // ` content `
                    for rect_width in 0..(natural + 6) {
                        let spans = chrome.to_spans(rect_width);
                        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
                        // The full sanitized name always survives — never
                        // truncated, never an ellipsis.
                        assert!(
                            text.contains(name),
                            "name shortened: dot={dot:?} zoomed={zoomed} \
                             name={name:?} rect_width={rect_width} text={text:?}"
                        );
                        assert!(!text.contains('…'), "ellipsis at {rect_width}");
                        let content_w = u16::try_from(text.width()).unwrap_or(u16::MAX);
                        if rect_width >= natural {
                            // Fits: padded to exactly rect_width, symmetric
                            // ` content ` (leading + trailing space).
                            assert_eq!(
                                content_w, rect_width,
                                "dot={dot:?} zoomed={zoomed} name={name:?}"
                            );
                            assert!(text.starts_with(' ') && text.ends_with(' '));
                        } else {
                            // Clip regime: leading space + full content, no
                            // trailing pad (content already meets or exceeds
                            // the rect); ratatui clips at the drawn edge.
                            assert_eq!(
                                content_w,
                                chrome.display_width() + 1,
                                "dot={dot:?} zoomed={zoomed} name={name:?} \
                                 rect_width={rect_width}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn agent_icon_glyphs_have_uniform_width_and_stable_tab_width() {
        use crate::detect::AgentState;
        // Every glyph agent_icon can emit — all states × seen × a full spinner
        // cycle — must occupy the same display columns, or the bar would
        // reflow as the spinner animates and hit areas would move mid-click.
        let palette = crate::app::state::Palette::catppuccin();
        let states = [
            AgentState::Blocked,
            AgentState::Working,
            AgentState::Idle,
            AgentState::Unknown,
        ];
        let mut widths = std::collections::BTreeSet::new();
        for state in states {
            for seen in [false, true] {
                for tick in 0..100 {
                    let (glyph, _) = super::super::status::agent_icon(state, seen, tick, &palette);
                    widths.insert(glyph.width());
                }
            }
        }
        assert_eq!(
            widths.len(),
            1,
            "agent_icon widths must be uniform: {widths:?}"
        );

        // And a dotted tab's width is invariant across spinner ticks.
        let width_at = |tick: u32| -> u16 {
            let (glyph, style) =
                super::super::status::agent_icon(AgentState::Working, false, tick, &palette);
            let chrome = TabChrome {
                status: Some(TabStatusDot { glyph, style }),
                name: "worker".into(),
                zoomed: false,
                is_attention: true,
                agent_state: Some((AgentState::Working, false)),
            };
            tab_width(&chrome)
        };
        let w0 = width_at(0);
        for tick in 1..100 {
            assert_eq!(width_at(tick), w0, "tab width moved at tick {tick}");
        }
    }

    #[test]
    fn hit_index_round_trip_center_and_boundaries() {
        // AC4: the production containment predicate (`tab_at` delegates to
        // `hit_index`) round-trips the center AND both boundary columns of
        // every visible rect — the tighter widths raise edge-adjacency risk a
        // center-only test cannot see. Includes adversarial-unicode names.
        let name_sets: [&[&str]; 2] = [
            &[
                "tab0", "tab1", "tab2", "tab3", "tab4", "tab5", "tab6", "tab7",
            ],
            &[
                "你好世界",
                "e\u{0301}ditor",
                "👨\u{200d}💻",
                "tab-d",
                "넓은탭이름",
                "f",
                "g",
                "h",
            ],
        ];
        for names in name_sets {
            for active in [0, 3, 7] {
                for width in [24u16, 30, 40, 60] {
                    let chromes = chromes_from_names(names);
                    let view = cbv(
                        chromes,
                        active,
                        TabStatusMode::Off,
                        Rect::new(0, 0, width, 1),
                        true,
                    );
                    for (idx, rect) in view.tab_hit_areas.iter().enumerate() {
                        if rect.width == 0 {
                            continue;
                        }
                        for col in [rect.x, rect.x + rect.width / 2, rect.x + rect.width - 1] {
                            assert_eq!(
                                hit_index(&view.tab_hit_areas, col),
                                Some(idx),
                                "active={active} width={width} tab={idx} col={col}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn narrow_and_degenerate_names_never_underflow() {
        // 1-char and empty names against narrow bars: fill converges, visible
        // window stays contiguous, no u16 underflow at the right-arrow column
        // (rendering exercises `rect.x + rect.width - 1`).
        let name_sets: [&[&str]; 3] = [&["a", "b", "c", "d", "e", "f"], &["a"], &["x", "y"]];
        for names in name_sets {
            for active in 0..names.len() {
                for width in 1u16..30 {
                    let chromes = chromes_from_names(names);
                    let view = cbv(
                        chromes,
                        active,
                        TabStatusMode::Off,
                        Rect::new(0, 0, width, 1),
                        true,
                    );
                    assert_eq!(view.tab_hit_areas.len(), names.len());
                    let visible: Vec<usize> = view
                        .tab_hit_areas
                        .iter()
                        .enumerate()
                        .filter(|(_, r)| r.width > 0)
                        .map(|(i, _)| i)
                        .collect();
                    if let (Some(&first), Some(&last)) = (visible.first(), visible.last()) {
                        assert_eq!(visible.len(), last - first + 1, "contiguous window");
                    }
                }
            }
        }
        // Render the tightest case end-to-end to prove no arithmetic panic.
        let app = app_with_tab_bar(&["a", "b", "c"], 1, Rect::new(0, 0, 7, 1), true);
        let _ = render_to_buffer(&app, app.view.tab_bar_rect);
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
        let spans = c.to_spans(12);
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
        let view = cbv(chromes, active, TabStatusMode::Off, area, mouse_chrome);
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
    fn render_all_tab_labels_are_bold_like_zellij() {
        // zellij `render_tab` paints every tab label bold — active AND inactive
        // alike — distinguishing them only by color. herdr matches: the name
        // glyph of both the active and an inactive tab must carry BOLD.
        use ratatui::style::Modifier;
        let app = app_with_tab_bar(&["ab", "cd", "ef"], 0, Rect::new(0, 0, 60, 1), false);
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        for (idx, first_ch) in [(0usize, 'a'), (1usize, 'c')] {
            let rect = app.view.tab_hit_areas[idx];
            let cell = (rect.x..rect.x + rect.width)
                .map(|x| &buffer[(x, 0)])
                .find(|c| c.symbol() == first_ch.to_string())
                .unwrap_or_else(|| panic!("tab {idx} name glyph not found"));
            assert!(
                cell.modifier.contains(Modifier::BOLD),
                "tab {idx} label must be bold (zellij bolds every tab)"
            );
            assert!(
                !cell.modifier.contains(Modifier::DIM),
                "tab {idx} label must not be dimmed (zellij has no dim tabs)"
            );
        }
    }

    #[test]
    fn render_inactive_tab_fg_uses_text_not_overlay() {
        // zellij inactive fg = ribbon_unselected.base (the readable text color),
        // mapped in herdr to `palette.text` — NOT the muted `overlay1` that made
        // inactive labels low-contrast before round 3.
        let app = app_with_tab_bar(&["ab", "cd", "ef"], 0, Rect::new(0, 0, 60, 1), false);
        let p = &app.palette;
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        let rect = app.view.tab_hit_areas[1]; // inactive, non-auto-named
        let cell = (rect.x..rect.x + rect.width)
            .map(|x| &buffer[(x, 0)])
            .find(|c| c.symbol() == "c")
            .expect("tab 1 name glyph not found");
        assert_eq!(cell.fg, p.text, "inactive tab fg should be palette.text");
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
    fn render_non_mouse_marker_uses_zellij_format() {
        let names: Vec<String> = (0..10).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let app = app_with_tab_bar(&refs, 5, Rect::new(0, 0, 40, 1), false);
        let left = app.view.tab_overflow.left.expect("left overflow").count;
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        let row = buffer_row_text(&buffer, app.view.tab_bar_rect, 0);
        // Non-mouse markers carry the same zellij ` ← +N ` interior.
        assert!(
            row.contains(&format!("← +{left}")),
            "zellij marker format: {row:?}"
        );
    }

    #[test]
    fn render_full_count_past_nine() {
        // Zellij format: the full hidden count renders, never a `9+` cap.
        let names: Vec<String> = (0..15).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        // Active near the end so >9 hidden on the left.
        let app = app_with_tab_bar(&refs, 14, Rect::new(0, 0, 30, 1), true);
        let count = app.view.tab_overflow.left.unwrap().count;
        assert!(count > 9);
        let buffer = render_to_buffer(&app, app.view.tab_bar_rect);
        let row = buffer_row_text(&buffer, app.view.tab_bar_rect, 0);
        assert!(
            row.contains(&format!("+{count}")),
            "full count, no cap: {row:?}"
        );
        assert!(!row.contains("9+"), "no 9+ cap: {row:?}");
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
}
