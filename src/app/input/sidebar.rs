use ratatui::layout::Rect;

use crate::app::state::{AppState, ViewLayout};

use super::ScrollbarClickTarget;

/// True when `(col, row)` lands inside an active overflow badge's rect.
fn badge_hit(badge: crate::ui::OverflowBadgeRect, col: u16, row: u16) -> bool {
    badge.is_active()
        && col >= badge.rect.x
        && col < badge.rect.x + badge.rect.width
        && row >= badge.rect.y
        && row < badge.rect.y + badge.rect.height
}

impl AppState {
    pub(super) fn workspace_list_rect(&self) -> Rect {
        let sidebar = self.view.sidebar_rect;
        if self.sidebar_collapsed || sidebar.width <= 1 || sidebar.height == 0 {
            return Rect::default();
        }
        crate::ui::workspace_list_rect(sidebar, self.sidebar_section_split)
    }

    pub(super) fn agent_panel_rect(&self) -> Rect {
        let sidebar = self.view.sidebar_rect;
        if self.sidebar_collapsed || sidebar.width <= 1 || sidebar.height == 0 {
            return Rect::default();
        }
        let (_, detail_area) =
            crate::ui::expanded_sidebar_sections(sidebar, self.sidebar_section_split);
        detail_area
    }

    pub(super) fn workspace_list_scrollbar_target_at(
        &self,
        col: u16,
        row: u16,
    ) -> Option<ScrollbarClickTarget> {
        let area = self.workspace_list_rect();
        let metrics = crate::ui::workspace_list_scroll_metrics(self, area);
        let track = crate::ui::workspace_list_scrollbar_rect(self, area)?;
        if col < track.x
            || col >= track.x + track.width
            || row < track.y
            || row >= track.y + track.height
        {
            return None;
        }
        if let Some(grab_row_offset) = crate::ui::scrollbar_thumb_grab_offset(metrics, track, row) {
            Some(ScrollbarClickTarget::Thumb { grab_row_offset })
        } else {
            Some(ScrollbarClickTarget::Track {
                offset_from_bottom: crate::ui::scrollbar_offset_from_row(metrics, track, row),
            })
        }
    }

    pub(super) fn workspace_list_offset_for_drag_row(
        &self,
        row: u16,
        grab_row_offset: u16,
    ) -> Option<usize> {
        let area = self.workspace_list_rect();
        let metrics = crate::ui::workspace_list_scroll_metrics(self, area);
        let track = crate::ui::workspace_list_scrollbar_rect(self, area)?;
        Some(crate::ui::scrollbar_offset_from_drag_row(
            metrics,
            track,
            row,
            grab_row_offset,
        ))
    }

    pub(super) fn set_workspace_list_offset_from_bottom(&mut self, offset_from_bottom: usize) {
        let area = self.workspace_list_rect();
        let metrics = crate::ui::workspace_list_scroll_metrics(self, area);
        self.workspace_scroll = metrics
            .max_offset_from_bottom
            .saturating_sub(offset_from_bottom);
        self.workspace_scroll = crate::ui::normalized_workspace_scroll(
            self,
            self.view.sidebar_rect,
            self.workspace_scroll,
        );
    }

    pub(super) fn scroll_workspace_list(&mut self, delta: i16) {
        if delta.is_negative() {
            self.workspace_scroll = self
                .workspace_scroll
                .saturating_sub(delta.unsigned_abs() as usize);
            self.workspace_scroll = crate::ui::normalized_workspace_scroll(
                self,
                self.view.sidebar_rect,
                self.workspace_scroll,
            );
            return;
        }

        let area = self.workspace_list_rect();
        let metrics = crate::ui::workspace_list_scroll_metrics(self, area);
        self.workspace_scroll = self
            .workspace_scroll
            .saturating_add(delta as usize)
            .min(metrics.max_offset_from_bottom);
        self.workspace_scroll = crate::ui::normalized_workspace_scroll(
            self,
            self.view.sidebar_rect,
            self.workspace_scroll,
        );
    }

    pub(super) fn agent_panel_scrollbar_target_at(
        &self,
        col: u16,
        row: u16,
    ) -> Option<ScrollbarClickTarget> {
        let area = self.agent_panel_rect();
        let metrics = crate::ui::agent_panel_scroll_metrics(self, area);
        let track = crate::ui::agent_panel_scrollbar_rect(self, area)?;
        if col < track.x
            || col >= track.x + track.width
            || row < track.y
            || row >= track.y + track.height
        {
            return None;
        }
        if let Some(grab_row_offset) = crate::ui::scrollbar_thumb_grab_offset(metrics, track, row) {
            Some(ScrollbarClickTarget::Thumb { grab_row_offset })
        } else {
            Some(ScrollbarClickTarget::Track {
                offset_from_bottom: crate::ui::scrollbar_offset_from_row(metrics, track, row),
            })
        }
    }

    pub(super) fn agent_panel_offset_for_drag_row(
        &self,
        row: u16,
        grab_row_offset: u16,
    ) -> Option<usize> {
        let area = self.agent_panel_rect();
        let metrics = crate::ui::agent_panel_scroll_metrics(self, area);
        let track = crate::ui::agent_panel_scrollbar_rect(self, area)?;
        Some(crate::ui::scrollbar_offset_from_drag_row(
            metrics,
            track,
            row,
            grab_row_offset,
        ))
    }

    pub(super) fn set_agent_panel_offset_from_bottom(&mut self, offset_from_bottom: usize) {
        let area = self.agent_panel_rect();
        let metrics = crate::ui::agent_panel_scroll_metrics(self, area);
        self.agent_panel_scroll = metrics
            .max_offset_from_bottom
            .saturating_sub(offset_from_bottom);
    }

    pub(super) fn scroll_agent_panel(&mut self, delta: i16) {
        let area = self.agent_panel_rect();
        let max_scroll = crate::ui::agent_panel_scroll_metrics(self, area).max_offset_from_bottom;
        if delta.is_negative() {
            self.agent_panel_scroll = self
                .agent_panel_scroll
                .saturating_sub(delta.unsigned_abs() as usize);
        } else {
            self.agent_panel_scroll = self
                .agent_panel_scroll
                .saturating_add(delta as usize)
                .min(max_scroll);
        }
    }

    pub(crate) fn sidebar_footer_rect(&self) -> Rect {
        let ws_area = self.workspace_list_rect();
        if ws_area == Rect::default() {
            return Rect::default();
        }
        let y = ws_area.y + ws_area.height.saturating_sub(1);
        Rect::new(ws_area.x, y, ws_area.width, 1)
    }

    pub(crate) fn sidebar_new_button_rect(&self) -> Rect {
        let footer = self.sidebar_footer_rect();
        let width = 5u16.min(footer.width.max(1));
        Rect::new(footer.x, footer.y, width, footer.height)
    }

    pub(crate) fn global_launcher_rect(&self) -> Rect {
        if self.view.layout == ViewLayout::Mobile {
            return self.view.mobile_menu_hit_area;
        }

        let footer = self.sidebar_footer_rect();
        let width = if self.global_menu_attention_badge_visible() {
            8
        } else {
            6
        }
        .min(footer.width.max(1));
        let x = footer.x + footer.width.saturating_sub(width);
        Rect::new(x, footer.y, width, footer.height)
    }

    pub(crate) fn global_menu_labels(&self) -> Vec<&'static str> {
        let mut labels = vec!["settings", "keybinds", "reload config"];
        if self.update_available.is_some() {
            labels.push("update ready");
        } else if self.latest_release_notes_available {
            labels.push("what's new");
        }
        labels.push("detach");
        labels
    }

    pub(crate) fn global_menu_rect(&self) -> Rect {
        let screen = self.screen_rect();
        let launcher = self.global_launcher_rect();
        let labels = self.global_menu_labels();
        let content_width = labels
            .iter()
            .map(|label| {
                let badge_width = if self.global_menu_item_has_badge(label) {
                    2
                } else {
                    0
                };
                label.chars().count() as u16 + badge_width
            })
            .max()
            .unwrap_or(8)
            .saturating_add(2);
        let menu_w = content_width.saturating_add(2).min(screen.width.max(1));
        let menu_h = (labels.len() as u16 + 2).min(screen.height.max(1));
        let max_x = screen.x + screen.width.saturating_sub(menu_w);
        let desired_x = launcher.x + launcher.width.saturating_sub(menu_w);
        let x = desired_x.min(max_x);
        let y = launcher.y.saturating_sub(menu_h);
        Rect::new(x, y, menu_w, menu_h)
    }

    pub(super) fn on_sidebar_divider(&self, col: u16, row: u16) -> bool {
        if self.sidebar_collapsed {
            return false;
        }
        let sidebar = self.view.sidebar_rect;
        let toggle = crate::ui::expanded_sidebar_toggle_rect(sidebar);
        let on_toggle = toggle.width > 0
            && col >= toggle.x
            && col < toggle.x + toggle.width
            && row >= toggle.y
            && row < toggle.y + toggle.height;
        sidebar.width > 0
            && !on_toggle
            && col == sidebar.x + sidebar.width.saturating_sub(1)
            && row >= sidebar.y
            && row < sidebar.y + sidebar.height
    }

    pub(super) fn on_sidebar_toggle(&self, col: u16, row: u16) -> bool {
        let hit = |rect: Rect| -> bool {
            rect.width > 0
                && col >= rect.x
                && col < rect.x + rect.width
                && row >= rect.y
                && row < rect.y + rect.height
        };
        if self.sidebar_collapsed {
            return hit(crate::ui::collapsed_sidebar_toggle_rect(
                self.view.sidebar_rect,
            ));
        }
        hit(crate::ui::expanded_sidebar_toggle_rect(
            self.view.sidebar_rect,
        ))
    }

    pub(super) fn toggle_sidebar_chrome(&mut self) {
        self.drag = None;
        self.sidebar_collapsed = !self.sidebar_collapsed;
    }

    pub(super) fn set_manual_sidebar_width(&mut self, divider_col: u16) {
        let sidebar = self.view.sidebar_rect;
        let width = divider_col.saturating_sub(sidebar.x).saturating_add(1);
        self.sidebar_width = width.clamp(self.sidebar_min_width, self.sidebar_max_width);
        self.sidebar_width_source = crate::app::state::SidebarWidthSource::Manual;
        self.mark_session_dirty();
    }

    pub(super) fn on_sidebar_section_divider(&self, col: u16, row: u16) -> bool {
        if self.sidebar_collapsed {
            return false;
        }
        let rect = crate::ui::sidebar_section_divider_rect(
            self.view.sidebar_rect,
            self.sidebar_section_split,
        );
        rect.width > 0
            && col >= rect.x
            && col < rect.x + rect.width
            && row >= rect.y
            && row < rect.y + rect.height
    }

    pub(super) fn set_sidebar_section_split(&mut self, row: u16) {
        let sidebar = self.view.sidebar_rect;
        let content_height = sidebar.height;
        if content_height < 6 {
            return;
        }
        let relative_y = row.saturating_sub(sidebar.y);
        let ratio = (relative_y as f32) / (content_height as f32);
        self.sidebar_section_split = ratio.clamp(0.1, 0.9);
        self.mark_session_dirty();
    }

    pub(super) fn workspace_at_row(&self, row: u16) -> Option<usize> {
        let footer = self.sidebar_footer_rect();
        if footer == Rect::default() {
            return None;
        }

        let cards = if self.view.workspace_card_areas.is_empty() {
            crate::ui::compute_workspace_card_areas(self, self.view.sidebar_rect)
        } else {
            self.view.workspace_card_areas.clone()
        };

        cards.iter().find_map(|card| {
            (row >= card.rect.y && row < card.rect.y + card.rect.height).then_some(card.ws_idx)
        })
    }

    pub(super) fn collapsed_workspace_at_row(&self, row: u16) -> Option<usize> {
        if !self.sidebar_collapsed {
            return None;
        }

        let (ws_area, _, _) = crate::ui::collapsed_sidebar_sections(self.view.sidebar_rect);
        if ws_area == Rect::default() || row < ws_area.y || row >= ws_area.y + ws_area.height {
            return None;
        }

        // Rows are placed through the same anchored window the rail renders, with
        // a reserved top-indicator row when spaces are hidden above. Map the
        // clicked row back to the workspace index (skipping indicator rows).
        let win = crate::ui::collapsed_ws_window(self, ws_area);
        let above_active = self.view.sidebar_overflow.collapsed_ws_above.is_active();
        let first_row = ws_area.y + u16::from(above_active);
        if row < first_row {
            return None; // top indicator row, not a workspace
        }
        let slot = (row - first_row) as usize;
        if slot >= win.count {
            return None; // bottom indicator row or empty space
        }
        let idx = win.first + slot;
        (idx < self.workspaces.len()).then_some(idx)
    }

    pub(super) fn collapsed_agent_detail_target_at(
        &self,
        row: u16,
    ) -> Option<(usize, usize, crate::layout::PaneId)> {
        if !self.sidebar_collapsed {
            return None;
        }

        let (_, _, detail_area) = crate::ui::collapsed_sidebar_sections(self.view.sidebar_rect);
        let detail_content_area = crate::ui::collapsed_detail_content_area(detail_area);
        if detail_content_area == Rect::default()
            || row < detail_content_area.y
            || row >= detail_content_area.y + detail_content_area.height
        {
            return None;
        }

        // Map the clicked row through the same anchored window the rail renders,
        // skipping a reserved top-indicator row when details are hidden above.
        let (ws_idx, details, win) = crate::ui::collapsed_detail_window(self, detail_area)?;
        let above_active = self
            .view
            .sidebar_overflow
            .collapsed_detail_above
            .is_active();
        let first_row = detail_content_area.y + u16::from(above_active);
        if row < first_row {
            return None; // top indicator row
        }
        let slot = (row - first_row) as usize;
        if slot >= win.count {
            return None; // bottom indicator row or empty space
        }
        let detail = details.get(win.first + slot)?;
        Some((ws_idx, detail.tab_idx, detail.pane_id))
    }

    /// Handle a click on a collapsed-rail overflow badge (FR8). Resolves the
    /// nearest hidden attention item (fallback: nearest hidden) and jumps:
    /// workspace badges switch workspace, detail badges focus the pane. Both
    /// route through drag-clearing chokepoints (`switch_workspace` /
    /// `focus_pane_in_workspace` → `switch_workspace_tab`). Returns true when a
    /// badge was hit (whether or not the jump resolved to a valid target).
    pub(super) fn on_collapsed_overflow_badge(&mut self, col: u16, row: u16) -> bool {
        let ov = self.view.sidebar_overflow;

        // Workspace section badges → switch_workspace.
        for badge in [ov.collapsed_ws_above, ov.collapsed_ws_below] {
            if badge_hit(badge, col, row) {
                if let Some(target) = crate::ui::resolve_overflow_jump(badge.side) {
                    if target < self.workspaces.len() {
                        self.switch_workspace(target);
                    } else {
                        tracing::warn!(target, "collapsed ws overflow jump out of range");
                    }
                }
                return true;
            }
        }

        // Detail section badges → focus the resolved pane.
        for badge in [ov.collapsed_detail_above, ov.collapsed_detail_below] {
            if badge_hit(badge, col, row) {
                self.jump_to_collapsed_detail_index(crate::ui::resolve_overflow_jump(badge.side));
                return true;
            }
        }

        false
    }

    /// Handle a click on an expanded-surface overflow badge (FR8). Workspace-list
    /// badges switch workspace; agent-panel badges focus the resolved pane.
    pub(super) fn on_expanded_overflow_badge(&mut self, col: u16, row: u16) -> bool {
        let ov = self.view.sidebar_overflow;

        for badge in [ov.expanded_ws_above, ov.expanded_ws_below] {
            if badge_hit(badge, col, row) {
                if let Some(entry_idx) = crate::ui::resolve_overflow_jump(badge.side) {
                    self.jump_to_workspace_list_entry(entry_idx);
                }
                return true;
            }
        }

        for badge in [ov.expanded_agents_above, ov.expanded_agents_below] {
            if badge_hit(badge, col, row) {
                if let Some(entry_idx) = crate::ui::resolve_overflow_jump(badge.side) {
                    self.jump_to_agent_panel_entry(entry_idx);
                }
                return true;
            }
        }

        false
    }

    /// Focus the pane at `detail_idx` within the collapsed rail's current detail
    /// workspace. Range-asserted; out-of-range is a logged no-op.
    fn jump_to_collapsed_detail_index(&mut self, detail_idx: Option<usize>) {
        let Some(detail_idx) = detail_idx else {
            return;
        };
        let detail_area = crate::ui::collapsed_sidebar_sections(self.view.sidebar_rect).2;
        // collapsed_detail_window resolves the same ws + details the rail renders.
        let Some((ws_idx, details, _win)) = crate::ui::collapsed_detail_window(self, detail_area)
        else {
            return;
        };
        if let Some(detail) = details.get(detail_idx) {
            let pane_id = detail.pane_id;
            self.focus_pane_in_workspace(ws_idx, pane_id);
        } else {
            tracing::warn!(detail_idx, "collapsed detail overflow jump out of range");
        }
    }

    /// Switch to the workspace behind workspace-list entry `entry_idx`. The
    /// expanded list's `switch_workspace` auto-scrolls the target into view.
    fn jump_to_workspace_list_entry(&mut self, entry_idx: usize) {
        let entries = crate::ui::workspace_list_entries(self);
        match entries.get(entry_idx) {
            Some(crate::ui::WorkspaceListEntry::Workspace { ws_idx, .. }) => {
                let ws_idx = *ws_idx;
                if ws_idx < self.workspaces.len() {
                    self.switch_workspace(ws_idx);
                } else {
                    tracing::warn!(ws_idx, "expanded ws overflow jump out of range");
                }
            }
            None => tracing::warn!(entry_idx, "expanded ws overflow entry out of range"),
        }
    }

    /// Focus the pane behind agent-panel entry `entry_idx`. Routes through
    /// `focus_agent_entry`, which range-asserts, clears drag (via
    /// `focus_pane_in_workspace` → `switch_workspace_tab`), and scrolls the
    /// panel so the focused entry is visible — the badge jump must advance the
    /// window, not just focus.
    fn jump_to_agent_panel_entry(&mut self, entry_idx: usize) {
        if !self.focus_agent_entry(entry_idx) {
            tracing::warn!(entry_idx, "expanded agent overflow jump did not resolve");
        }
    }

    /// Integer-row drop-slot resolution (FR4): a pointer row over card *k*
    /// resolves to insert-before-*k*, and any row strictly below the last card
    /// resolves to insert-after-last, so every index 0..=len is reachable over
    /// adjacent 1-row cards. Rows over a group-interior card (no slot inside a
    /// compact worktree-space group) resolve to the slot below the group.
    pub(super) fn workspace_drop_index_at_row(&self, row: u16) -> Option<usize> {
        let area = self.workspace_list_rect();
        let footer = self.sidebar_footer_rect();
        if area == Rect::default() || row < area.y || row >= footer.y {
            return None;
        }

        let cards = if self.view.workspace_card_areas.is_empty() {
            crate::ui::compute_workspace_card_areas(self, self.view.sidebar_rect)
        } else {
            self.view.workspace_card_areas.clone()
        };
        if cards.is_empty() {
            return Some(0);
        }

        let inside_group_gap = |idx: usize| {
            let card_group = self
                .workspaces
                .get(cards[idx].ws_idx)
                .and_then(|ws| ws.worktree_space())
                .map(|space| space.key.as_str());
            let previous_group = idx.checked_sub(1).and_then(|prev_idx| {
                self.workspaces
                    .get(cards[prev_idx].ws_idx)
                    .and_then(|ws| ws.worktree_space())
                    .map(|space| space.key.as_str())
            });
            card_group.is_some() && card_group == previous_group
        };
        let after_last = cards.last().map(|card| card.ws_idx + 1).unwrap_or(0);

        let Some(display_idx) = cards
            .iter()
            .position(|card| row >= card.rect.y && row < card.rect.y + card.rect.height)
        else {
            // Header rows above the first card resolve to the top slot; rows
            // strictly below the last card resolve to insert-after-last.
            let first = cards.first()?;
            if row < first.rect.y {
                return Some(first.ws_idx);
            }
            return Some(after_last);
        };

        // A group-interior card has no slot at its top edge; resolve to the
        // first slot below the group segment.
        match (display_idx..cards.len()).find(|idx| !inside_group_gap(*idx)) {
            Some(idx) => Some(cards[idx].ws_idx),
            None => Some(after_last),
        }
    }

    pub(super) fn on_agent_panel_sort_toggle(&self, col: u16, row: u16) -> bool {
        if self.sidebar_collapsed {
            return false;
        }

        let (_, detail_area) = crate::ui::expanded_sidebar_sections(
            self.view.sidebar_rect,
            self.sidebar_section_split,
        );
        let rect = crate::ui::agent_panel_toggle_rect(detail_area, self.agent_panel_sort);
        rect.width > 0
            && col >= rect.x
            && col < rect.x + rect.width
            && row >= rect.y
            && row < rect.y + rect.height
    }

    pub(super) fn agent_detail_target_at(
        &self,
        row: u16,
    ) -> Option<(usize, usize, crate::layout::PaneId)> {
        if self.sidebar_collapsed {
            return None;
        }

        let detail_area = self.agent_panel_rect();
        let metrics = crate::ui::agent_panel_scroll_metrics(self, detail_area);
        let body = crate::ui::agent_panel_body_rect(
            detail_area,
            crate::ui::should_show_scrollbar(metrics),
        );
        if body.height < 2 || row < body.y || row >= body.y + body.height {
            return None;
        }

        let entries = crate::ui::agent_panel_entries(self);
        crate::ui::agent_panel_item_rows(body, self.agent_panel_scroll, entries.len())
            .find(|(_, row_y)| row == *row_y || row == row_y + 1)
            .map(|(entry_idx, _)| {
                let detail = &entries[entry_idx];
                (detail.ws_idx, detail.tab_idx, detail.pane_id)
            })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::layout::Rect;

    use super::super::{app_for_mouse_test, capture_snapshot, mouse, unique_temp_path, App};
    use crate::{
        app::state::{AgentPanelSort, DragState, DragTarget, Mode},
        detect::Agent,
        workspace::Workspace,
    };

    #[test]
    fn clicking_launcher_opens_global_menu() {
        let mut app = app_for_mouse_test();
        let rect = app.state.global_launcher_rect();

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            rect.x + rect.width.saturating_sub(1),
            rect.y,
        ));

        assert_eq!(app.state.mode, Mode::GlobalMenu);
    }

    #[test]
    fn hovering_global_menu_updates_highlight() {
        let mut app = app_for_mouse_test();
        let launcher = app.state.global_launcher_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            launcher.x,
            launcher.y,
        ));

        let menu = app.state.global_menu_rect();
        app.handle_mouse(mouse(MouseEventKind::Moved, menu.x + 2, menu.y + 2));

        assert_eq!(app.state.global_menu.highlighted, 1);
    }

    #[test]
    fn clicking_keybinds_menu_item_opens_help() {
        let mut app = app_for_mouse_test();
        let launcher = app.state.global_launcher_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            launcher.x,
            launcher.y,
        ));

        let menu = app.state.global_menu_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            menu.x + 2,
            menu.y + 2,
        ));

        assert_eq!(app.state.mode, Mode::KeybindHelp);
    }

    #[test]
    fn clicking_settings_menu_item_opens_settings() {
        let mut app = app_for_mouse_test();
        let launcher = app.state.global_launcher_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            launcher.x,
            launcher.y,
        ));

        let menu = app.state.global_menu_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            menu.x + 2,
            menu.y + 1,
        ));

        assert_eq!(app.state.mode, Mode::Settings);
    }

    #[test]
    fn clicking_reload_config_menu_item_requests_reload() {
        let mut app = app_for_mouse_test();
        let launcher = app.state.global_launcher_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            launcher.x,
            launcher.y,
        ));

        let menu = app.state.global_menu_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            menu.x + 2,
            menu.y + 3,
        ));

        assert!(app.state.request_reload_config);
        assert_eq!(app.state.mode, Mode::Navigate);
    }

    #[test]
    fn update_pending_menu_surfaces_update_ready_entry() {
        let mut app = app_for_mouse_test();
        app.state.update_available = Some("0.3.2".into());
        app.state.latest_release_notes_available = true;

        let launcher = app.state.global_launcher_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            launcher.x,
            launcher.y,
        ));

        assert_eq!(
            app.state.global_menu_labels(),
            vec![
                "settings",
                "keybinds",
                "reload config",
                "update ready",
                "detach"
            ]
        );
        assert!(!app.state.should_quit);
    }

    #[test]
    fn persistence_mode_menu_surfaces_detach_action() {
        let mut app = app_for_mouse_test();
        app.state.detach_exits = false;

        let launcher = app.state.global_launcher_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            launcher.x,
            launcher.y,
        ));

        assert_eq!(
            app.state.global_menu_labels(),
            vec!["settings", "keybinds", "reload config", "detach"]
        );

        let menu = app.state.global_menu_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            menu.x + 2,
            menu.y + 4,
        ));

        assert!(app.state.detach_requested);
        assert!(!app.state.should_quit);
        assert_ne!(app.state.mode, Mode::GlobalMenu);
    }

    #[test]
    fn whats_new_remains_in_menu_for_latest_installed_release_notes() {
        let mut app = app_for_mouse_test();
        app.state.latest_release_notes_available = true;

        assert_eq!(
            app.state.global_menu_labels(),
            vec![
                "settings",
                "keybinds",
                "reload config",
                "what's new",
                "detach"
            ]
        );
    }

    #[test]
    fn clicking_agent_detail_row_switches_to_correct_tab_and_pane() {
        let mut app = app_for_mouse_test();
        let mut ws = Workspace::test_new("test");
        ws.tabs[0].set_custom_name("main".into());
        let first_pane = ws.tabs[0].root_pane;
        let first_tab = ws.test_add_tab(Some("logs"));
        let second_pane = ws.tabs[first_tab].root_pane;
        app.state.workspaces = vec![ws];
        app.state.ensure_test_terminals();
        let first_terminal_id = app.state.workspaces[0].tabs[0].panes[&first_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&first_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Pi);
        let second_terminal_id = app.state.workspaces[0].tabs[first_tab].panes[&second_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&second_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Claude);
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;

        // Second entry, second row of its 2-row item.
        let body = crate::ui::agent_panel_body_rect(app.state.agent_panel_rect(), false);
        let (_, row_y) = crate::ui::agent_panel_item_rows(body, 0, 2)
            .nth(1)
            .expect("second agent entry visible");
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, row_y + 1));

        assert_eq!(app.state.workspaces[0].active_tab, 1);
        assert_eq!(
            app.state.workspaces[0].tabs[1].layout.focused(),
            second_pane
        );
        assert_eq!(app.state.mode, Mode::Terminal);
        let snapshot = capture_snapshot(&app.state);
        assert_eq!(snapshot.workspaces[0].active_tab, first_tab);
        assert_eq!(
            snapshot.workspaces[0].tabs[first_tab].focused,
            Some(second_pane.raw())
        );
    }

    #[test]
    fn clicking_agent_panel_toggle_switches_sort() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![Workspace::test_new("test")];
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;
        app.state.agent_panel_scroll = 3;

        let (_, detail_area) = crate::ui::expanded_sidebar_sections(
            app.state.view.sidebar_rect,
            app.state.sidebar_section_split,
        );
        let toggle = crate::ui::agent_panel_toggle_rect(detail_area, app.state.agent_panel_sort);
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            toggle.x,
            toggle.y,
        ));

        assert_eq!(app.state.agent_panel_sort, AgentPanelSort::Priority);
        assert_eq!(app.state.agent_panel_scroll, 0);
    }

    #[test]
    fn clicking_all_workspaces_agent_row_switches_to_correct_workspace() {
        let mut app = app_for_mouse_test();
        let first = Workspace::test_new("one");
        let first_pane = first.tabs[0].root_pane;

        let second = Workspace::test_new("two");
        let second_pane = second.tabs[0].root_pane;

        app.state.workspaces = vec![first, second];
        app.state.ensure_test_terminals();
        let first_terminal_id = app.state.workspaces[0].tabs[0].panes[&first_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&first_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Pi);
        let second_terminal_id = app.state.workspaces[1].tabs[0].panes[&second_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&second_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Claude);
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;

        // Second workspace's entry, first row of its 2-row item.
        let body = crate::ui::agent_panel_body_rect(app.state.agent_panel_rect(), false);
        let (_, row_y) = crate::ui::agent_panel_item_rows(body, 0, 2)
            .nth(1)
            .expect("second agent entry visible");
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            body.x + 2,
            row_y,
        ));

        assert_eq!(app.state.active, Some(1));
        assert_eq!(app.state.selected, 1);
        assert_eq!(app.state.workspaces[1].active_tab, 0);
        assert_eq!(
            app.state.workspaces[1].tabs[0].layout.focused(),
            second_pane
        );
    }

    #[test]
    fn scrolling_agent_panel_with_wheel_updates_agent_panel_scroll() {
        let mut app = app_for_mouse_test();
        let mut ws = Workspace::test_new("test");
        let first_pane = ws.tabs[0].root_pane;

        let mut tabs = Vec::new();
        for (tab_name, agent) in [
            ("logs", Agent::Claude),
            ("review", Agent::Codex),
            ("ops", Agent::Gemini),
        ] {
            let tab_idx = ws.test_add_tab(Some(tab_name));
            let pane_id = ws.tabs[tab_idx].root_pane;
            tabs.push((tab_idx, pane_id, agent));
        }

        app.state.workspaces = vec![ws];
        app.state.ensure_test_terminals();
        let first_terminal_id = app.state.workspaces[0].tabs[0].panes[&first_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&first_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Pi);
        for (tab_idx, pane_id, agent) in tabs {
            let terminal_id = app.state.workspaces[0].tabs[tab_idx].panes[&pane_id]
                .attached_terminal_id
                .clone();
            app.state
                .terminals
                .get_mut(&terminal_id)
                .unwrap()
                .detected_agent = Some(agent);
        }
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;

        let detail_area = app.state.agent_panel_rect();
        assert!(crate::ui::should_show_scrollbar(
            crate::ui::agent_panel_scroll_metrics(&app.state, detail_area)
        ));

        app.handle_mouse(mouse(
            MouseEventKind::ScrollDown,
            detail_area.x + 1,
            detail_area.y + 4,
        ));

        assert_eq!(app.state.agent_panel_scroll, 1);
        assert_eq!(app.state.selected, 0);
    }

    #[test]
    fn clicking_scrolled_agent_detail_row_switches_to_correct_tab_and_pane() {
        let mut app = app_for_mouse_test();
        let mut ws = Workspace::test_new("test");
        let first_pane = ws.tabs[0].root_pane;
        let second_tab = ws.test_add_tab(Some("logs"));
        let second_pane = ws.tabs[second_tab].root_pane;
        let mut extra_tabs = Vec::new();
        for (tab_name, agent) in [("review", Agent::Codex), ("ops", Agent::Gemini)] {
            let tab_idx = ws.test_add_tab(Some(tab_name));
            let pane_id = ws.tabs[tab_idx].root_pane;
            extra_tabs.push((tab_idx, pane_id, agent));
        }

        app.state.workspaces = vec![ws];
        app.state.ensure_test_terminals();
        let first_terminal_id = app.state.workspaces[0].tabs[0].panes[&first_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&first_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Pi);
        let second_terminal_id = app.state.workspaces[0].tabs[second_tab].panes[&second_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&second_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Claude);
        for (tab_idx, pane_id, agent) in extra_tabs {
            let terminal_id = app.state.workspaces[0].tabs[tab_idx].panes[&pane_id]
                .attached_terminal_id
                .clone();
            app.state
                .terminals
                .get_mut(&terminal_id)
                .unwrap()
                .detected_agent = Some(agent);
        }
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;
        app.state.agent_panel_scroll = 1;

        let detail_area = app.state.agent_panel_rect();
        let body = crate::ui::agent_panel_body_rect(detail_area, true);
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            body.x + 1,
            body.y,
        ));

        assert_eq!(app.state.workspaces[0].active_tab, second_tab);
        assert_eq!(
            app.state.workspaces[0].tabs[second_tab].layout.focused(),
            second_pane
        );
        assert_eq!(app.state.mode, Mode::Terminal);
    }

    #[test]
    fn clicking_collapsed_agent_row_switches_to_correct_tab_and_pane() {
        let mut app = app_for_mouse_test();
        let mut ws = Workspace::test_new("test");
        let first_pane = ws.tabs[0].root_pane;
        let second_tab = ws.test_add_tab(Some("logs"));
        let second_pane = ws.tabs[second_tab].root_pane;
        app.state.workspaces = vec![ws];
        app.state.ensure_test_terminals();
        let first_terminal_id = app.state.workspaces[0].tabs[0].panes[&first_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&first_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Pi);
        let second_terminal_id = app.state.workspaces[0].tabs[second_tab].panes[&second_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&second_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Claude);
        app.state
            .terminals
            .get_mut(&second_terminal_id)
            .unwrap()
            .state = crate::detect::AgentState::Blocked;
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;
        app.state.sidebar_collapsed = true;
        app.state.view.sidebar_rect = Rect::new(0, 0, 7, 20);
        app.state.view.terminal_area = Rect::new(7, 0, 80, 20);

        let (_, _, detail_area) =
            crate::ui::collapsed_sidebar_sections(app.state.view.sidebar_rect);
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            detail_area.x,
            detail_area.y + 1,
        ));

        assert_eq!(app.state.workspaces[0].active_tab, 1);
        assert_eq!(
            app.state.workspaces[0].tabs[1].layout.focused(),
            second_pane
        );
        assert_eq!(app.state.mode, Mode::Terminal);
    }

    #[test]
    fn clicking_collapsed_non_attention_agent_row_does_not_switch() {
        let mut app = app_for_mouse_test();
        let mut ws = Workspace::test_new("test");
        let first_pane = ws.tabs[0].root_pane;
        let second_tab = ws.test_add_tab(Some("logs"));
        let second_pane = ws.tabs[second_tab].root_pane;
        app.state.workspaces = vec![ws];
        app.state.ensure_test_terminals();
        let first_terminal_id = app.state.workspaces[0].tabs[0].panes[&first_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&first_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Pi);
        let second_terminal_id = app.state.workspaces[0].tabs[second_tab].panes[&second_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&second_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Claude);
        // Unknown is the only genuinely non-attention state after zellij-
        // fidelity round 2 broadened the predicate to include Working. A row
        // in a non-attention state must not steal focus on click.
        app.state
            .terminals
            .get_mut(&second_terminal_id)
            .unwrap()
            .state = crate::detect::AgentState::Unknown;
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;
        app.state.sidebar_collapsed = true;
        app.state.view.sidebar_rect = Rect::new(0, 0, 7, 20);
        app.state.view.terminal_area = Rect::new(7, 0, 80, 20);

        let (_, _, detail_area) =
            crate::ui::collapsed_sidebar_sections(app.state.view.sidebar_rect);
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            detail_area.x,
            detail_area.y + 1,
        ));

        assert_eq!(app.state.workspaces[0].active_tab, 0);
        assert_eq!(app.state.workspaces[0].tabs[0].layout.focused(), first_pane);
    }

    #[test]
    fn clicking_collapsed_idle_unseen_agent_row_switches() {
        let mut app = app_for_mouse_test();
        let mut ws = Workspace::test_new("test");
        let first_pane = ws.tabs[0].root_pane;
        let second_tab = ws.test_add_tab(Some("logs"));
        let second_pane = ws.tabs[second_tab].root_pane;
        app.state.workspaces = vec![ws];
        app.state.ensure_test_terminals();
        let first_terminal_id = app.state.workspaces[0].tabs[0].panes[&first_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&first_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Pi);
        let second_terminal_id = app.state.workspaces[0].tabs[second_tab].panes[&second_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&second_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Claude);
        app.state
            .terminals
            .get_mut(&second_terminal_id)
            .unwrap()
            .state = crate::detect::AgentState::Idle;
        app.state.workspaces[0].tabs[second_tab]
            .panes
            .get_mut(&second_pane)
            .unwrap()
            .seen = false;
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;
        app.state.sidebar_collapsed = true;
        app.state.view.sidebar_rect = Rect::new(0, 0, 7, 20);
        app.state.view.terminal_area = Rect::new(7, 0, 80, 20);

        let (_, _, detail_area) =
            crate::ui::collapsed_sidebar_sections(app.state.view.sidebar_rect);
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            detail_area.x,
            detail_area.y + 1,
        ));

        assert_eq!(app.state.workspaces[0].active_tab, 1);
        assert_eq!(
            app.state.workspaces[0].tabs[1].layout.focused(),
            second_pane
        );
        assert_eq!(app.state.mode, Mode::Terminal);
    }

    #[test]
    fn clicking_collapsed_sidebar_toggle_expands_sidebar() {
        let mut app = app_for_mouse_test();
        app.state.sidebar_collapsed = true;
        app.state.view.sidebar_rect = Rect::new(0, 0, 7, 20);
        app.state.view.terminal_area = Rect::new(7, 0, 80, 20);

        let toggle = crate::ui::collapsed_sidebar_toggle_rect(app.state.view.sidebar_rect);
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            toggle.x,
            toggle.y,
        ));

        assert!(!app.state.sidebar_collapsed);
    }

    #[test]
    fn clicking_expanded_sidebar_toggle_collapses_sidebar() {
        let mut app = app_for_mouse_test();
        app.state.sidebar_collapsed = false;
        app.state.view.sidebar_rect = Rect::new(0, 0, 26, 20);
        app.state.view.terminal_area = Rect::new(26, 0, 80, 20);

        let toggle = crate::ui::expanded_sidebar_toggle_rect(app.state.view.sidebar_rect);
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            toggle.x,
            toggle.y,
        ));

        assert!(app.state.sidebar_collapsed);
        assert!(app.state.drag.is_none());
    }

    #[test]
    fn clicking_workspace_switches_on_mouse_up() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![Workspace::test_new("a"), Workspace::test_new("b")];
        app.state.active = Some(0);
        app.state.selected = 0;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));
        let target_row = app.state.view.workspace_card_areas[1].rect.y;

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            2,
            target_row,
        ));
        assert_eq!(app.state.active, Some(0));
        assert!(app.state.workspace_press.is_some());

        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, target_row));
        assert_eq!(app.state.active, Some(1));
        assert_eq!(app.state.selected, 1);
        assert!(app.state.workspace_press.is_none());
        let snapshot = capture_snapshot(&app.state);
        assert_eq!(snapshot.active, Some(1));
        assert_eq!(snapshot.selected, 1);
    }

    #[test]
    fn clicking_worktree_parent_row_focuses_workspace_without_toggling() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![Workspace::test_new("main"), Workspace::test_new("issue")];
        for (idx, checkout_path) in ["/repo/herdr", "/repo/herdr-issue"].into_iter().enumerate() {
            app.state.workspaces[idx].worktree_space =
                Some(crate::workspace::WorktreeSpaceMembership {
                    key: "repo-key".into(),
                    label: "herdr".into(),
                    repo_root: "/repo/herdr".into(),
                    checkout_path: checkout_path.into(),
                    is_linked_worktree: idx > 0,
                });
        }
        app.state.active = None;
        app.state.mode = Mode::Terminal;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));
        let parent = app.state.view.workspace_card_areas[0].rect;

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            parent.x + 2,
            parent.y,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            parent.x + 2,
            parent.y,
        ));

        assert_eq!(app.state.active, Some(0));
        assert!(!app.state.collapsed_space_keys.contains("repo-key"));
    }

    #[test]
    fn clicking_worktree_parent_chevron_toggles_group_only() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![Workspace::test_new("main"), Workspace::test_new("issue")];
        for (idx, checkout_path) in ["/repo/herdr", "/repo/herdr-issue"].into_iter().enumerate() {
            app.state.workspaces[idx].worktree_space =
                Some(crate::workspace::WorktreeSpaceMembership {
                    key: "repo-key".into(),
                    label: "herdr".into(),
                    repo_root: "/repo/herdr".into(),
                    checkout_path: checkout_path.into(),
                    is_linked_worktree: idx > 0,
                });
        }
        app.state.active = None;
        app.state.mode = Mode::Terminal;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));
        let parent = app.state.view.workspace_card_areas[0].rect;

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            parent.x,
            parent.y,
        ));

        assert_eq!(app.state.active, None);
        assert!(app.state.workspace_press.is_none());
        assert!(app.state.collapsed_space_keys.contains("repo-key"));

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            parent.x,
            parent.y,
        ));

        assert!(!app.state.collapsed_space_keys.contains("repo-key"));
    }

    #[test]
    fn wheel_workspace_selection_follows_grouped_visual_order_without_scrollbar() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            Workspace::test_new("main"),
            Workspace::test_new("normal"),
            Workspace::test_new("issue"),
        ];
        for (idx, checkout_path) in [(0, "/repo/herdr"), (2, "/repo/herdr-issue")] {
            app.state.workspaces[idx].worktree_space =
                Some(crate::workspace::WorktreeSpaceMembership {
                    key: "repo-key".into(),
                    label: "herdr".into(),
                    repo_root: "/repo/herdr".into(),
                    checkout_path: checkout_path.into(),
                    is_linked_worktree: idx != 0,
                });
        }
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Navigate;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 30));
        let list = app.state.workspace_list_rect();
        assert!(!crate::ui::should_show_scrollbar(
            crate::ui::workspace_list_scroll_metrics(&app.state, list)
        ));

        app.handle_mouse(mouse(MouseEventKind::ScrollDown, list.x + 1, list.y + 1));

        assert_eq!(app.state.selected, 2);
    }

    #[test]
    fn dragging_workspace_reorders_without_changing_identity() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            Workspace::test_new("a"),
            Workspace::test_new("b"),
            Workspace::test_new("c"),
        ];
        let active_id = app.state.workspaces[1].id.clone();
        let selected_id = app.state.workspaces[2].id.clone();
        app.state.active = Some(1);
        app.state.selected = 2;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));
        let source_row = app.state.view.workspace_card_areas[1].rect.y;
        let target_row = crate::ui::workspace_drop_indicator_row(
            &app.state.view.workspace_card_areas,
            app.state.workspace_list_rect(),
            0,
        )
        .unwrap();

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            2,
            source_row,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            2,
            target_row,
        ));
        assert!(matches!(
            app.state.drag.as_ref().map(|drag| &drag.target),
            Some(DragTarget::WorkspaceReorder {
                source_ws_idx: 1,
                insert_idx: Some(0),
            })
        ));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, target_row));

        let names: Vec<_> = app
            .state
            .workspaces
            .iter()
            .map(|ws| ws.display_name())
            .collect();
        assert_eq!(names, vec!["b", "a", "c"]);
        assert_eq!(app.state.active, Some(0));
        assert_eq!(app.state.selected, 2);
        assert_eq!(app.state.workspaces[0].id, active_id);
        assert_eq!(app.state.workspaces[2].id, selected_id);
        let snapshot = capture_snapshot(&app.state);
        let captured_names: Vec<_> = snapshot
            .workspaces
            .iter()
            .map(|ws| ws.custom_name.clone().unwrap())
            .collect();
        assert_eq!(captured_names, vec!["b", "a", "c"]);
    }

    #[test]
    fn clicking_overflow_indicator_jumps_to_hidden_tab_without_renaming() {
        let mut app = app_for_mouse_test();
        let mut ws = Workspace::test_new("test");
        // Enough tabs to overflow the tab bar at width 65 even after the sidebar
        // claims its minimum width, so the overflow indicators are guaranteed to
        // appear. Keep a margin so a future sidebar-width default change cannot
        // silently make these fit again.
        for name in ["logs", "review", "ops", "notes", "build", "deploy", "watch"] {
            ws.test_add_tab(Some(name));
        }
        app.state.workspaces = vec![ws];
        app.state.active = Some(0);
        app.state.selected = 0;
        // Desktop width (above the mobile threshold) but narrow enough that the
        // trailing tabs overflow behind the right indicator with active tab 0.
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 70, 20));

        let right = app.state.view.tab_overflow.right.expect("right overflow");
        let right_rect = app.state.view.tab_overflow.right_hit_area;
        assert!(right_rect.width > 0);
        let expected_tab = right.jump_to;

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            right_rect.x + 1,
            right_rect.y,
        ));

        // The indicator-jump activates the nearest hidden tab on that side.
        assert_eq!(app.state.workspaces[0].active_tab, expected_tab);
        // After re-centering, the jumped-to tab is now visible.
        assert!(app.state.view.tab_hit_areas[expected_tab].width > 0);
        // Tabs are not renamed by the jump (custom names preserved).
        assert!(app.state.workspaces[0].tabs[0].custom_name.is_none());
        assert_eq!(
            app.state.workspaces[0].tabs[1].custom_name.as_deref(),
            Some("logs")
        );
    }

    #[test]
    fn clicking_last_visible_tab_activates_it() {
        let mut app = app_for_mouse_test();
        let mut ws = Workspace::test_new("test");
        for name in [
            "one", "two", "three", "four", "five", "six", "seven", "eight",
        ] {
            ws.test_add_tab(Some(name));
        }
        // Active on the last tab so the centered fill keeps it visible.
        let last_idx = ws.tabs.len() - 1;
        ws.active_tab = last_idx;
        app.state.workspaces = vec![ws];
        app.state.active = Some(0);
        app.state.selected = 0;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 65, 20));

        let target = app.state.view.tab_hit_areas[last_idx];
        assert!(target.width > 0, "active/last tab should be visible");

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            target.x + 1,
            target.y,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            target.x + 1,
            target.y,
        ));

        assert_eq!(app.state.workspaces[0].active_tab, last_idx);
        assert!(app.state.view.tab_hit_areas[last_idx].width > 0);
    }

    #[test]
    fn dragging_tab_reorders_auto_and_custom_names_without_materializing_numbers() {
        let mut app = app_for_mouse_test();
        let mut ws = Workspace::test_new("test");
        ws.test_add_tab(Some("foo"));
        ws.test_add_tab(None);
        let moved_root = ws.tabs[0].root_pane;
        app.state.workspaces = vec![ws];
        app.state.active = Some(0);
        app.state.selected = 0;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));

        let source = app.state.view.tab_hit_areas[0];
        let last = app.state.view.tab_hit_areas[2];
        let drop_col = last.x + last.width;

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            source.x + 1,
            source.y,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            drop_col,
            source.y,
        ));
        assert!(matches!(
            app.state.drag.as_ref().map(|drag| &drag.target),
            Some(DragTarget::TabReorder {
                ws_idx: 0,
                source_tab_idx: 0,
                insert_idx: Some(3),
            })
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            drop_col,
            source.y,
        ));

        let labels: Vec<_> = app.state.workspaces[0]
            .tabs
            .iter()
            .enumerate()
            .map(|(tab_idx, _)| app.state.workspaces[0].tab_display_name(tab_idx).unwrap())
            .collect();
        assert_eq!(labels, vec!["foo", "2", "3"]);
        assert_eq!(
            app.state.workspaces[0].tabs[0].custom_name.as_deref(),
            Some("foo")
        );
        assert!(app.state.workspaces[0].tabs[1].custom_name.is_none());
        assert!(app.state.workspaces[0].tabs[2].custom_name.is_none());
        assert_eq!(app.state.workspaces[0].tabs[0].number, 2);
        assert_eq!(app.state.workspaces[0].tabs[1].number, 3);
        assert_eq!(app.state.workspaces[0].tabs[2].number, 1);
        assert_eq!(app.state.workspaces[0].tabs[2].root_pane, moved_root);
        assert_eq!(app.state.workspaces[0].active_tab, 2);
    }

    fn temp_git_repo(branch: &str) -> std::path::PathBuf {
        let repo = unique_temp_path("sidebar-drop-slot-repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(
            repo.join(".git/HEAD"),
            format!("ref: refs/heads/{branch}\n"),
        )
        .unwrap();
        repo
    }

    fn workspace_with_space(name: &str, key: &str) -> Workspace {
        let mut ws = Workspace::test_new(name);
        ws.worktree_space = Some(crate::workspace::WorktreeSpaceMembership {
            key: key.into(),
            label: "herdr".into(),
            repo_root: "/repo/herdr".into(),
            checkout_path: format!("/repo/{name}").into(),
            is_linked_worktree: name != "main",
        });
        ws
    }

    #[test]
    fn top_drop_slot_is_distinct_from_gap_below_first_workspace() {
        let mut app = app_for_mouse_test();
        let first_repo = temp_git_repo("main");
        let second_repo = temp_git_repo("main");

        let mut first = Workspace::test_new("a");
        let first_root = first.tabs[0].root_pane;
        first.identity_cwd = first_repo.clone();
        first.refresh_git_ahead_behind();

        let mut second = Workspace::test_new("b");
        let second_root = second.tabs[0].root_pane;
        second.identity_cwd = second_repo.clone();
        second.refresh_git_ahead_behind();

        app.state.workspaces = vec![first, second];
        app.state.ensure_test_terminals();
        let first_terminal_id = app.state.workspaces[0].tabs[0].panes[&first_root]
            .attached_terminal_id
            .clone();
        app.state.terminals.get_mut(&first_terminal_id).unwrap().cwd = first_repo.clone();
        let second_terminal_id = app.state.workspaces[1].tabs[0].panes[&second_root]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&second_terminal_id)
            .unwrap()
            .cwd = second_repo.clone();
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));

        // Integer-row slot resolution over adjacent 1-row cards: header rows
        // above the first card resolve to the top slot, a row over card k
        // resolves to insert-before-k, and rows below the last card resolve
        // to insert-after-last.
        assert_eq!(app.state.workspace_drop_index_at_row(0), Some(0));
        assert_eq!(app.state.workspace_drop_index_at_row(1), Some(0));
        assert_eq!(app.state.workspace_drop_index_at_row(2), Some(0));
        assert_eq!(app.state.workspace_drop_index_at_row(3), Some(1));
        assert_eq!(app.state.workspace_drop_index_at_row(4), Some(2));

        let _ = fs::remove_dir_all(first_repo);
        let _ = fs::remove_dir_all(second_repo);
    }

    #[test]
    fn drop_slot_reachability_covers_every_insertion_index() {
        // FR4 reachability: every insertion index 0..=len must be reachable
        // via integer pointer rows over adjacent 1-row cards (top, middle,
        // and end of list).
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            Workspace::test_new("a"),
            Workspace::test_new("b"),
            Workspace::test_new("c"),
            Workspace::test_new("d"),
        ];
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));

        let cards = app.state.view.workspace_card_areas.clone();
        assert_eq!(cards.len(), 4);

        // Row over card k → insert-before-k.
        let mut reachable = std::collections::BTreeSet::new();
        for card in &cards {
            let idx = app
                .state
                .workspace_drop_index_at_row(card.rect.y)
                .expect("row over a card resolves to a slot");
            assert_eq!(idx, card.ws_idx, "insert-before-k over card k");
            reachable.insert(idx);
        }
        // Row strictly below the last card → insert-after-last.
        let below = cards.last().unwrap().rect.y + 1;
        let idx = app
            .state
            .workspace_drop_index_at_row(below)
            .expect("row below the last card resolves to a slot");
        assert_eq!(idx, cards.len(), "insert-after-last below the last card");
        reachable.insert(idx);

        assert_eq!(
            reachable.into_iter().collect::<Vec<_>>(),
            (0..=cards.len()).collect::<Vec<_>>(),
            "every insertion index 0..=len is reachable"
        );
    }

    #[test]
    fn bottom_drop_slot_stays_below_last_workspace_not_footer() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            Workspace::test_new("a"),
            Workspace::test_new("b"),
            Workspace::test_new("c"),
        ];
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));

        let cards = &app.state.view.workspace_card_areas;
        let bottom_slot = crate::ui::workspace_drop_indicator_row(
            cards,
            app.state.workspace_list_rect(),
            cards.len(),
        )
        .unwrap();

        // Edge-based indicator: the end-of-list slot underlines the last
        // card's own row (its bottom edge is the insertion boundary).
        let last = cards.last().unwrap().rect;
        assert_eq!(bottom_slot, last.y);
        assert!(bottom_slot < app.state.sidebar_footer_rect().y.saturating_sub(1));
    }

    #[test]
    fn grouped_sidebar_drop_slots_do_not_land_inside_compact_group() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            workspace_with_space("main", "repo-key"),
            Workspace::test_new("normal"),
            workspace_with_space("issue", "repo-key"),
        ];
        app.state.active = Some(1);
        app.state.selected = 1;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 40));

        let cards = &app.state.view.workspace_card_areas;
        let order = cards.iter().map(|card| card.ws_idx).collect::<Vec<_>>();
        assert_eq!(order, vec![0, 2, 1]);
        let issue = cards.iter().find(|card| card.ws_idx == 2).unwrap();
        let normal = cards.iter().find(|card| card.ws_idx == 1).unwrap();

        assert_eq!(app.state.workspace_drop_index_at_row(issue.rect.y), Some(1));
        // Insert-after-last underlines the last card's own row.
        assert_eq!(
            crate::ui::workspace_drop_indicator_row(cards, app.state.workspace_list_rect(), 2),
            Some(normal.rect.y)
        );
    }

    #[test]
    fn dragging_worktree_space_member_does_not_reorder_workspaces() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            workspace_with_space("main", "repo-key"),
            Workspace::test_new("normal"),
            workspace_with_space("issue", "repo-key"),
        ];
        app.state.active = Some(0);
        app.state.selected = 0;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 40));

        let source = app
            .state
            .view
            .workspace_card_areas
            .iter()
            .find(|card| card.ws_idx == 2)
            .unwrap()
            .rect;
        let target_row = crate::ui::workspace_drop_indicator_row(
            &app.state.view.workspace_card_areas,
            app.state.workspace_list_rect(),
            0,
        )
        .unwrap();

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, source.y));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            2,
            target_row,
        ));
        assert!(app.state.drag.is_none());
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, target_row));

        let names = app
            .state
            .workspaces
            .iter()
            .map(|ws| ws.display_name())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["main", "normal", "issue"]);
    }

    #[test]
    fn dragging_sidebar_divider_sets_manual_width() {
        let mut app = app_for_mouse_test();

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 25, 5));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 30, 5));

        assert_eq!(app.state.sidebar_width, 31);
        let snapshot = capture_snapshot(&app.state);
        assert_eq!(snapshot.sidebar_width, Some(31));
    }

    #[test]
    fn manual_pin_uses_clicked_column_not_rendered_width() {
        // Multi-client safety on the input path: set_manual_sidebar_width derives
        // the pinned width from divider_col - sidebar.x + 1, reading only
        // sidebar.x (always 0, width-invariant). So an already-foreground client
        // dragging to a given column pins exactly that column regardless of which
        // client's responsive width last populated view.sidebar_rect.
        let mut app = app_for_mouse_test();
        app.state.sidebar_width_ratio = 0.18;
        app.state.sidebar_width_source = crate::app::state::SidebarWidthSource::ConfigDefault;

        // A different-width client rendered last, leaving a narrow rect width.
        app.state.view.sidebar_rect = ratatui::layout::Rect::new(0, 0, 18, 20);

        // The dragging client (already foreground) drops the divider on column 28.
        app.state.set_manual_sidebar_width(28);

        // Pinned width = 28 - 0 + 1 = 29, independent of the stale rect width 18.
        assert_eq!(app.state.sidebar_width, 29);
        assert_eq!(
            app.state.sidebar_width_source,
            crate::app::state::SidebarWidthSource::Manual
        );
    }

    #[test]
    fn dragging_sidebar_bottom_divider_still_sets_manual_width() {
        let mut app = app_for_mouse_test();
        let divider_col = app.state.view.sidebar_rect.x + app.state.view.sidebar_rect.width - 1;
        let bottom_row = app.state.view.sidebar_rect.y + app.state.view.sidebar_rect.height - 1;

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            divider_col,
            bottom_row,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            divider_col + 5,
            bottom_row,
        ));

        assert_eq!(app.state.sidebar_width, 31);
    }

    #[test]
    fn dragging_past_max_clamps_to_configured_max() {
        let mut app = app_for_mouse_test();
        app.state.sidebar_max_width = 30;

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 25, 5));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 50, 5));

        assert_eq!(app.state.sidebar_width, 30);
    }

    #[test]
    fn dragging_below_min_clamps_to_configured_min() {
        let mut app = app_for_mouse_test();
        app.state.sidebar_min_width = 22;

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 25, 5));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 5, 5));

        assert_eq!(app.state.sidebar_width, 22);
    }

    #[test]
    fn dragging_sidebar_section_divider_sets_split_ratio() {
        let mut app = app_for_mouse_test();
        let divider = crate::ui::sidebar_section_divider_rect(
            app.state.view.sidebar_rect,
            app.state.sidebar_section_split,
        );

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            divider.x + 1,
            divider.y,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            divider.x + 1,
            divider.y + 4,
        ));

        assert!(app.state.sidebar_section_split > 0.5);
        let snapshot = capture_snapshot(&app.state);
        assert_eq!(
            snapshot.sidebar_section_split,
            Some(app.state.sidebar_section_split)
        );
    }

    #[test]
    fn double_clicking_sidebar_divider_resets_default_width() {
        let mut app = app_for_mouse_test();
        app.state.default_sidebar_width = 26;
        app.state.sidebar_width = 30;
        app.state.sidebar_width_source = crate::app::state::SidebarWidthSource::Manual;

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 25, 5));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 25, 5));
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 25, 5));

        assert_eq!(app.state.sidebar_width, 26);
        assert_eq!(
            app.state.sidebar_width_source,
            crate::app::state::SidebarWidthSource::ConfigDefault,
        );
        assert!(app.state.drag.is_none());
        let snapshot = capture_snapshot(&app.state);
        // Non-manual capture emits None (responsive width does not persist)
        assert_eq!(snapshot.sidebar_width, None);
        assert_eq!(snapshot.sidebar_width_manual, None);

        // Responsive sizing re-activates: a layout pass now derives the width
        // from the ratio rather than honoring the prior manual pin.
        app.state.sidebar_width_ratio = 0.18;
        crate::ui::compute_view(&mut app.state, ratatui::layout::Rect::new(0, 0, 120, 20));
        assert_eq!(app.state.view.sidebar_rect.width, 22);
    }

    // -----------------------------------------------------------------------
    // FR8: attention-aware overflow badges — sidebar surfaces
    // -----------------------------------------------------------------------

    /// Build an app with `n` single-pane workspaces and test terminals. The
    /// workspaces at `blocked` are forced into Blocked (an attention state).
    fn app_with_attention_workspaces(n: usize, blocked: &[usize]) -> App {
        let mut app = app_for_mouse_test();
        app.state.workspaces = (0..n)
            .map(|i| Workspace::test_new(&format!("ws{i}")))
            .collect();
        app.state.ensure_test_terminals();
        for &i in blocked {
            let pane = app.state.workspaces[i].tabs[0].root_pane;
            let tid = app.state.workspaces[i].tabs[0].panes[&pane]
                .attached_terminal_id
                .clone();
            app.state.terminals.get_mut(&tid).unwrap().state = crate::detect::AgentState::Blocked;
        }
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;
        app
    }

    #[test]
    fn collapsed_rail_overflow_badge_counts_hidden_attention_spaces() {
        // Many workspaces, short rail: some are hidden. A hidden blocked space
        // makes the bottom badge carry an attention count.
        let mut app = app_with_attention_workspaces(12, &[9]);
        app.state.sidebar_collapsed = true;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 80, 16));

        let below = app.state.view.sidebar_overflow.collapsed_ws_below;
        assert!(below.is_active(), "spaces are hidden below");
        assert!(
            below.side.hidden_blocked >= 1,
            "hidden blocked space counted"
        );
        assert_eq!(below.side.blocked_jump_to, Some(9));
    }

    #[test]
    fn collapsed_rail_overflow_badge_click_jumps_to_hidden_attention_space() {
        let mut app = app_with_attention_workspaces(12, &[9]);
        app.state.sidebar_collapsed = true;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 80, 16));

        let below = app.state.view.sidebar_overflow.collapsed_ws_below;
        assert!(below.is_active());
        let rect = below.rect;
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            rect.x + 1,
            rect.y,
        ));
        // Jumped to the hidden attention workspace (index 9), not a neighbor.
        assert_eq!(app.state.active, Some(9));
    }

    #[test]
    fn collapsed_rail_overflow_badge_no_attention_falls_back_to_nearest_hidden() {
        // Hidden spaces but none in attention: badge still clickable, jumps to
        // the nearest hidden space (plain `+N`, no attention target).
        let mut app = app_with_attention_workspaces(12, &[]);
        app.state.sidebar_collapsed = true;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 80, 16));

        let below = app.state.view.sidebar_overflow.collapsed_ws_below;
        assert!(below.is_active());
        assert_eq!(below.side.hidden_attention(), 0);
        assert_eq!(below.side.blocked_jump_to, None);
        let rect = below.rect;
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            rect.x + 1,
            rect.y,
        ));
        // Fell back to the nearest hidden space (the one just below the window).
        assert_eq!(app.state.active, Some(below.side.jump_to));
    }

    #[test]
    fn no_overflow_badge_when_everything_fits() {
        let mut app = app_with_attention_workspaces(3, &[1]);
        app.state.sidebar_collapsed = true;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 80, 20));
        let ov = app.state.view.sidebar_overflow;
        assert!(!ov.collapsed_ws_above.is_active());
        assert!(!ov.collapsed_ws_below.is_active());
    }

    #[test]
    fn expanded_workspace_overflow_badge_click_jumps_and_scrolls() {
        // Tall list of workspaces in the expanded sidebar; a hidden blocked
        // workspace below the fold. Clicking the bottom badge switches to it.
        let mut app = app_with_attention_workspaces(30, &[25]);
        app.state.sidebar_collapsed = false;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 16));

        let below = app.state.view.sidebar_overflow.expanded_ws_below;
        assert!(
            below.is_active(),
            "workspaces hidden below the expanded list"
        );
        let rect = below.rect;
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            rect.x + rect.width.saturating_sub(1),
            rect.y,
        ));
        assert_eq!(app.state.active, Some(25));
        // After the switch, a fresh layout must REVEAL the target — resolving the
        // index alone is insufficient; the scroll-aware window has to advance so
        // the target is within the visible window, not still hidden.
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 16));
        assert_eq!(app.state.selected, 25);
        let ws_area = crate::ui::workspace_list_rect(
            app.state.view.sidebar_rect,
            app.state.sidebar_section_split,
        );
        let metrics = crate::ui::workspace_list_scroll_metrics(&app.state, ws_area);
        let scroll = app.state.workspace_scroll;
        assert!(
            scroll <= 25 && 25 < scroll + metrics.viewport_rows,
            "target 25 must be inside the visible window [{scroll}, {}) after the jump",
            scroll + metrics.viewport_rows,
        );
    }

    #[test]
    fn collapsed_rail_drag_latch_cleared_on_badge_jump() {
        // A stranded drag latch must not survive a badge-jump (step-3 chokepoint).
        let mut app = app_with_attention_workspaces(12, &[9]);
        app.state.sidebar_collapsed = true;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 80, 16));
        // Synthesize an active drag.
        app.state.drag = Some(DragState {
            target: DragTarget::WorkspaceListScrollbar { grab_row_offset: 0 },
        });
        let rect = app.state.view.sidebar_overflow.collapsed_ws_below.rect;
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            rect.x + 1,
            rect.y,
        ));
        assert!(app.state.drag.is_none(), "badge jump must clear drag latch");
    }

    /// Build a single workspace with `n` panes (each its own tab) and test
    /// terminals; the panes at `blocked` are forced Blocked. Returns the app and
    /// the blocked panes' `(tab_idx, pane_id)`.
    fn app_with_many_panes(
        n: usize,
        blocked: &[usize],
    ) -> (App, Vec<(usize, crate::layout::PaneId)>) {
        let mut app = app_for_mouse_test();
        let mut ws = Workspace::test_new("w");
        for i in 1..n {
            ws.test_add_tab(Some(&format!("t{i}")));
        }
        app.state.workspaces = vec![ws];
        app.state.ensure_test_terminals();
        // A pane only appears in pane_details / agent_panel_entries when its
        // terminal has a detected agent, so give every pane one.
        let tab_count = app.state.workspaces[0].tabs.len();
        for i in 0..tab_count {
            let pane = app.state.workspaces[0].tabs[i].root_pane;
            let tid = app.state.workspaces[0].tabs[i].panes[&pane]
                .attached_terminal_id
                .clone();
            app.state.terminals.get_mut(&tid).unwrap().detected_agent = Some(Agent::Claude);
        }
        let mut blocked_panes = Vec::new();
        for &i in blocked {
            let pane = app.state.workspaces[0].tabs[i].root_pane;
            let tid = app.state.workspaces[0].tabs[i].panes[&pane]
                .attached_terminal_id
                .clone();
            app.state.terminals.get_mut(&tid).unwrap().state = crate::detect::AgentState::Blocked;
            blocked_panes.push((i, pane));
        }
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;
        (app, blocked_panes)
    }

    #[test]
    fn expanded_agent_panel_overflow_badge_click_focuses_and_scrolls() {
        // Many panes so the agent panel overflows; a hidden Blocked pane below.
        // Clicking the bottom badge focuses it AND scrolls it into view.
        let (mut app, blocked) = app_with_many_panes(20, &[15]);
        app.state.sidebar_collapsed = false;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 16));

        let below = app.state.view.sidebar_overflow.expanded_agents_below;
        assert!(below.is_active(), "agents hidden below the panel");
        assert!(below.side.hidden_attention() >= 1);
        let rect = below.rect;
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            rect.x + rect.width.saturating_sub(1),
            rect.y,
        ));

        // Focused the hidden Blocked pane (tab 15), not a neighbor.
        let (tab_idx, pane_id) = blocked[0];
        assert_eq!(app.state.workspaces[0].active_tab, tab_idx);
        assert_eq!(
            app.state.workspaces[0].tabs[tab_idx].layout.focused(),
            pane_id
        );

        // The panel scroll advanced so the focused entry is within the window.
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 16));
        let detail_area = crate::ui::expanded_sidebar_sections(
            app.state.view.sidebar_rect,
            app.state.sidebar_section_split,
        )
        .1;
        let metrics = crate::ui::agent_panel_scroll_metrics(&app.state, detail_area);
        let scroll = app.state.agent_panel_scroll;
        // Entry index of the focused pane in the (Spaces-sorted) entry list is 15.
        assert!(
            scroll <= 15 && 15 < scroll + metrics.viewport_rows,
            "focused entry 15 must be inside the panel window [{scroll}, {}) after the jump",
            scroll + metrics.viewport_rows,
        );
    }

    #[test]
    fn expanded_agent_panel_badge_jump_clears_drag_latch() {
        let (mut app, _) = app_with_many_panes(20, &[15]);
        app.state.sidebar_collapsed = false;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 16));
        app.state.drag = Some(DragState {
            target: DragTarget::AgentPanelScrollbar { grab_row_offset: 0 },
        });
        let rect = app.state.view.sidebar_overflow.expanded_agents_below.rect;
        assert!(rect.width > 0, "agent panel badge present");
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            rect.x + rect.width.saturating_sub(1),
            rect.y,
        ));
        assert!(
            app.state.drag.is_none(),
            "agent-panel badge jump must clear drag latch via switch_workspace_tab"
        );
    }

    #[test]
    fn collapsed_detail_overflow_badge_click_focuses_hidden_attention_pane() {
        // Collapsed rail, one workspace with many panes; a hidden Blocked pane in
        // the detail section. Clicking the detail bottom badge focuses it.
        let (mut app, blocked) = app_with_many_panes(20, &[15]);
        app.state.sidebar_collapsed = true;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 80, 20));

        let below = app.state.view.sidebar_overflow.collapsed_detail_below;
        assert!(below.is_active(), "detail panes hidden below");
        assert!(below.side.hidden_attention() >= 1);
        let rect = below.rect;
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            rect.x + 1,
            rect.y,
        ));

        let (tab_idx, pane_id) = blocked[0];
        assert_eq!(app.state.workspaces[0].active_tab, tab_idx);
        assert_eq!(
            app.state.workspaces[0].tabs[tab_idx].layout.focused(),
            pane_id
        );
    }

    #[test]
    fn clicking_old_close_button_span_does_not_collapse_sidebar() {
        let mut app = app_for_mouse_test();
        app.state.sidebar_collapsed = false;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));

        let (_, detail) = crate::ui::expanded_sidebar_sections(
            app.state.view.sidebar_rect,
            app.state.sidebar_section_split,
        );
        // The old close button's leftmost column (its `btn.x` at full 3-cell
        // span). Requires detail width >= 4 so the coordinate is meaningful.
        assert!(detail.width >= 4, "fixture must give detail width >= 4");
        let col = detail.x + detail.width - 4;
        let row = detail.y + detail.height - 1;

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), col, row));

        // The close affordance is gone; a click there no longer collapses the
        // sidebar. Mode/focus inertness is deliberately NOT asserted (FR4): the
        // cell may legitimately route to an agent row or the scrollbar.
        assert!(!app.state.sidebar_collapsed);
    }

    #[test]
    fn toggle_click_preserves_invariants_on_adversarial_state() {
        let mut state = crate::app::state::AppState::test_with_adversarial_identity_state();
        state.sidebar_collapsed = false;
        crate::ui::compute_view(&mut state, Rect::new(0, 0, 106, 20));

        let toggle = crate::ui::expanded_sidebar_toggle_rect(state.view.sidebar_rect);
        if toggle != Rect::default() {
            state.toggle_sidebar_chrome();
            state.assert_invariants_for_test();
        }
    }
}
