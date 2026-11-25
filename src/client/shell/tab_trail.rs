//! Fork: back/forward buttons on the zellij-style sidebar's `spaces` header.
//!
//! A browser-style trail of visited tabs, observed from snapshot focus changes
//! so every source counts. Client memory only.

use super::render::{put_right_text, put_text};
use super::sidebar_chrome::SidebarJump;
use super::*;

const LIMIT: usize = 50;
/// Nerd Font `md-chevron_left` and `md-chevron_right`.
pub(super) const BACK: &str = "\u{f0141}";
pub(super) const FORWARD: &str = "\u{f0142}";
const BUTTON_WIDTH: u16 = 3;
/// " spaces" plus both buttons.
const MIN_WIDTH: u16 = 13;

#[derive(Debug, Default)]
pub(super) struct TabTrail {
    tabs: Vec<String>,
    cursor: usize,
    /// The step being waited for. The cursor moves only when its focus lands.
    pending: Option<usize>,
}

impl TabTrail {
    pub(super) fn observe(&mut self, from: Option<&str>, to: Option<&str>) {
        let (Some(from), Some(to)) = (from, to) else {
            return;
        };
        if from == to {
            return;
        }
        if let Some(step) = self.pending.take() {
            if self.tabs.get(step).is_some_and(|tab| tab == to) {
                self.cursor = step;
                return;
            }
        }
        if self.tabs.is_empty() {
            self.tabs.push(from.to_owned());
        }
        self.tabs.truncate(self.cursor + 1);
        self.tabs.push(to.to_owned());
        if self.tabs.len() > LIMIT {
            self.tabs.remove(0);
        }
        self.cursor = self.tabs.len() - 1;
    }

    /// Where one step in a direction leads: the nearest open tab other than
    /// the current one.
    fn target(
        &self,
        back: bool,
        current: Option<&str>,
        is_open: impl Fn(&str) -> bool,
    ) -> Option<usize> {
        let usable = |index: &usize| {
            let tab = self.tabs[*index].as_str();
            Some(tab) != current && is_open(tab)
        };
        if back {
            (0..self.cursor).rev().find(usable)
        } else {
            (self.cursor + 1..self.tabs.len()).find(usable)
        }
    }

    pub(super) fn step(
        &mut self,
        back: bool,
        current: Option<&str>,
        is_open: impl Fn(&str) -> bool,
    ) -> Option<String> {
        let target = self.target(back, current, is_open)?;
        self.pending = Some(target);
        Some(self.tabs[target].clone())
    }
}

/// Reserves both buttons in the last cells of the `spaces` header.
/// `paint_tab_trail` draws them once the frame's hit map is known.
pub(super) fn place_buttons(config: &ClientShellConfig, hits: &mut ShellHitMap) {
    let body = hits.workspace_body;
    if !config.mouse_capture || body.width < MIN_WIDTH || body.y < WORKSPACE_HEADER_ROWS {
        return;
    }
    let y = body.y - WORKSPACE_HEADER_ROWS;
    for (from_right, jump) in [(2, SidebarJump::Back), (1, SidebarJump::Forward)] {
        let x = body.right() - BUTTON_WIDTH * from_right;
        hits.sidebar_overflow
            .push((Rect::new(x, y, BUTTON_WIDTH, 1), jump));
    }
}

impl ClientShellState {
    fn tab_trail_target(&self, back: bool) -> Option<&str> {
        let snapshot = self.snapshot.as_deref()?;
        let target = self
            .tab_trail
            .target(back, snapshot.focused_tab_id.as_deref(), |tab| {
                snapshot.tabs.iter().any(|open| open.tab_id == tab)
            })?;
        Some(&self.tab_trail.tabs[target])
    }

    pub(super) fn tab_trail_step(&mut self, back: bool) -> Option<crate::api::schema::Method> {
        let snapshot = self.snapshot.as_deref()?;
        let tab_id = self
            .tab_trail
            .step(back, snapshot.focused_tab_id.as_deref(), |tab| {
                snapshot.tabs.iter().any(|open| open.tab_id == tab)
            })?;
        Some(crate::api::schema::Method::TabFocus(
            crate::api::schema::TabTarget { tab_id },
        ))
    }

    /// Tracks which button the pointer is over.
    pub(super) fn update_tab_trail_hover(
        &mut self,
        point: (u16, u16),
        outcome: &mut ClientShellInput,
    ) {
        let hover = self
            .hits
            .sidebar_overflow
            .iter()
            .filter(|_| self.overlay.is_none() && self.chrome_drag.is_none())
            .find(|(rect, _)| super::contains(*rect, point))
            .and_then(|(_, jump)| match jump {
                SidebarJump::Back => Some(true),
                SidebarJump::Forward => Some(false),
                _ => None,
            });
        if self.tab_trail_hover != hover {
            self.tab_trail_hover = hover;
            outcome.repaint = true;
        }
    }

    /// Bright with somewhere to go, dim without. Hovering a usable button
    /// lights it up and names its tab on the blank row below.
    pub(super) fn paint_tab_trail(&self, buffer: &mut Buffer) {
        let palette = &self.config.palette;
        for (rect, jump) in &self.hits.sidebar_overflow {
            let (back, glyph) = match jump {
                SidebarJump::Back => (true, BACK),
                SidebarJump::Forward => (false, FORWARD),
                _ => continue,
            };
            let target = self.tab_trail_target(back);
            let hovered = self.tab_trail_hover == Some(back);
            let style = match target {
                Some(_) if hovered => Style::default()
                    .fg(palette.accent)
                    .bg(palette.surface1)
                    .add_modifier(Modifier::BOLD),
                Some(_) => Style::default()
                    .fg(palette.text)
                    .add_modifier(Modifier::BOLD),
                None => Style::default().fg(palette.surface1),
            };
            put_text(
                buffer,
                rect.x,
                rect.y,
                rect.width,
                &format!(" {glyph} "),
                style,
            );
            if let Some(label) = target
                .filter(|_| hovered)
                .and_then(|tab| self.tab_name(tab))
            {
                let left = self.hits.workspace_body.x + 1;
                let row = Rect::new(left, rect.y + 1, self.hits.workspace_body.right() - left, 1);
                put_right_text(
                    buffer,
                    row,
                    row.y,
                    &label,
                    Style::default().fg(palette.accent),
                );
            }
        }
    }

    /// `tab · space`.
    fn tab_name(&self, tab_id: &str) -> Option<String> {
        let snapshot = self.snapshot.as_deref()?;
        let tab = snapshot.tabs.iter().find(|tab| tab.tab_id == tab_id)?;
        let workspace = snapshot
            .workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == tab.workspace_id)?;
        Some(format!("{} · {}", tab.label, workspace.label))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(_: &str) -> bool {
        true
    }

    fn trail(tabs: &[&str]) -> TabTrail {
        let mut trail = TabTrail::default();
        for pair in tabs.windows(2) {
            trail.observe(Some(pair[0]), Some(pair[1]));
        }
        trail
    }

    /// Steps and lands.
    fn go(trail: &mut TabTrail, back: bool, from: &str) -> Option<String> {
        let to = trail.step(back, Some(from), open)?;
        trail.observe(Some(from), Some(&to));
        Some(to)
    }

    #[test]
    fn walks_back_and_forward() {
        let mut t = trail(&["a", "b", "c"]);
        assert_eq!(go(&mut t, true, "c").as_deref(), Some("b"));
        assert_eq!(go(&mut t, true, "b").as_deref(), Some("a"));
        assert_eq!(go(&mut t, true, "a"), None);
        assert_eq!(go(&mut t, false, "a").as_deref(), Some("b"));
        assert_eq!(go(&mut t, false, "b").as_deref(), Some("c"));
        assert_eq!(go(&mut t, false, "c"), None);
    }

    #[test]
    fn empty_trail_goes_nowhere() {
        let mut t = TabTrail::default();
        assert_eq!(t.step(true, Some("a"), open), None);
        assert_eq!(t.step(false, Some("a"), open), None);
    }

    #[test]
    fn a_new_tab_drops_the_forward_steps() {
        let mut t = trail(&["a", "b", "c"]);
        go(&mut t, true, "c");
        t.observe(Some("b"), Some("d"));
        assert_eq!(t.step(false, Some("d"), open), None);
        assert_eq!(go(&mut t, true, "d").as_deref(), Some("b"));
    }

    #[test]
    fn closed_tabs_and_the_current_tab_are_skipped() {
        let mut t = trail(&["x", "a", "b", "a"]);
        // b closed: the next entry back is a, which is already current.
        assert_eq!(
            t.step(true, Some("a"), |tab| tab != "b").as_deref(),
            Some("x")
        );
    }

    #[test]
    fn a_step_that_never_lands_does_not_move_the_cursor() {
        let mut t = trail(&["a", "b", "c"]);
        assert_eq!(t.step(true, Some("c"), open).as_deref(), Some("b"));
        assert_eq!(t.step(true, Some("c"), open).as_deref(), Some("b"));
        t.observe(Some("c"), Some("d"));
        assert_eq!(go(&mut t, true, "d").as_deref(), Some("c"));
    }

    #[test]
    fn the_trail_is_capped() {
        let names: Vec<String> = (0..LIMIT + 10).map(|n| n.to_string()).collect();
        let t = trail(&names.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(t.tabs.len(), LIMIT);
        assert_eq!(t.cursor, LIMIT - 1);
        assert_eq!(t.tabs.last(), names.last());
    }

    #[test]
    #[ignore = "timing only"]
    fn timing() {
        let names: Vec<String> = (0..7).map(|n| format!("w1:t{n}")).collect();
        let mut t = TabTrail::default();
        let start = std::time::Instant::now();
        for n in 0..1_000_000usize {
            t.observe(Some(&names[n % 7]), Some(&names[(n + 1) % 7]));
        }
        println!("switch: {:?}", start.elapsed() / 1_000_000);
        let start = std::time::Instant::now();
        for _ in 0..1_000_000 {
            t.observe(
                std::hint::black_box(Some("w1:t1")),
                std::hint::black_box(Some("w1:t1")),
            );
        }
        println!("no change: {:?}", start.elapsed() / 1_000_000);
    }
}
