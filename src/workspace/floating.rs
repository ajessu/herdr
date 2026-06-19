// Some methods will gain callers in steps 3-5 (input/render).
#![allow(dead_code)]

use ratatui::layout::Rect;

use crate::layout::PaneId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderEdge {
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatingGeom {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl FloatingGeom {
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.width, self.height)
    }

    pub fn inner_rect(&self) -> Rect {
        if self.width <= 2 || self.height <= 2 {
            return Rect::new(self.x, self.y, 0, 0);
        }
        Rect::new(
            self.x.saturating_add(1),
            self.y.saturating_add(1),
            self.width - 2,
            self.height - 2,
        )
    }

    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.x
            && x < self.x.saturating_add(self.width)
            && y >= self.y
            && y < self.y.saturating_add(self.height)
    }

    pub fn border_hit(&self, x: u16, y: u16) -> Option<BorderEdge> {
        if !self.contains(x, y) {
            return None;
        }
        let on_left = x == self.x;
        let on_right = x == self.x.saturating_add(self.width.saturating_sub(1));
        let on_top = y == self.y;
        let on_bottom = y == self.y.saturating_add(self.height.saturating_sub(1));

        match (on_top, on_bottom, on_left, on_right) {
            (true, _, true, _) => Some(BorderEdge::TopLeft),
            (true, _, _, true) => Some(BorderEdge::TopRight),
            (_, true, true, _) => Some(BorderEdge::BottomLeft),
            (_, true, _, true) => Some(BorderEdge::BottomRight),
            (true, _, _, _) => Some(BorderEdge::Top),
            (_, true, _, _) => Some(BorderEdge::Bottom),
            (_, _, true, _) => Some(BorderEdge::Left),
            (_, _, _, true) => Some(BorderEdge::Right),
            _ => None,
        }
    }

    pub fn clamp_within(&mut self, bounds: Rect) {
        let right = self.x.saturating_add(self.width);
        let bottom = self.y.saturating_add(self.height);
        let bounds_right = bounds.x.saturating_add(bounds.width);
        let bounds_bottom = bounds.y.saturating_add(bounds.height);

        let already_in_bounds = self.x >= bounds.x
            && self.y >= bounds.y
            && right <= bounds_right
            && bottom <= bounds_bottom;

        if already_in_bounds {
            return;
        }

        let w = self.width.min(bounds.width);
        let h = self.height.min(bounds.height);
        self.width = w;
        self.height = h;

        if self.x < bounds.x {
            self.x = bounds.x;
        } else if self.x.saturating_add(w) > bounds_right {
            self.x = bounds_right.saturating_sub(w);
        }

        if self.y < bounds.y {
            self.y = bounds.y;
        } else if self.y.saturating_add(h) > bounds_bottom {
            self.y = bounds_bottom.saturating_sub(h);
        }
    }
}

#[derive(Debug, Clone)]
pub struct FloatingPane {
    pub pane_id: PaneId,
    pub geom: FloatingGeom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleResult {
    Shown,
    NeedSpawn,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    Tiled(PaneId),
    Floating(PaneId),
}

impl FocusTarget {
    pub fn pane_id(&self) -> PaneId {
        match self {
            Self::Tiled(id) | Self::Floating(id) => *id,
        }
    }

    pub fn is_floating(&self) -> bool {
        matches!(self, Self::Floating(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneLayer {
    Tiled,
    Floating,
}

/// Per-tab collection of floating panes.
///
/// `panes` is ordered by z-index: the last element renders on top. `visible`
/// toggles the whole layer at once; `focus` is the floating pane that receives
/// input while the layer is focused.
///
/// Contract between `visible` and `focus`:
/// - Focus is only *acted on* while the layer is visible — [`is_focused`] is
///   `visible && focus.is_some()`, and input routing keys off that.
/// - `focus` may be `Some` while hidden: [`add_pane`] records the new pane as
///   focused regardless of visibility so the pane is already focused when the
///   layer is next shown. The only invariant is that `focus` always points at a
///   contained pane or is `None` (enforced by `assert_invariants_for_test`).
///
/// [`is_focused`]: FloatingLayer::is_focused
/// [`add_pane`]: FloatingLayer::add_pane
#[derive(Debug)]
pub struct FloatingLayer {
    panes: Vec<FloatingPane>,
    visible: bool,
    focus: Option<PaneId>,
}

impl Default for FloatingLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl FloatingLayer {
    pub fn new() -> Self {
        Self {
            panes: Vec::new(),
            visible: false,
            focus: None,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn is_focused(&self) -> bool {
        self.visible && self.focus.is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.panes.is_empty()
    }

    pub fn count(&self) -> usize {
        self.panes.len()
    }

    pub fn toggle_visible(&mut self) -> ToggleResult {
        if self.visible {
            self.visible = false;
            self.focus = None;
            ToggleResult::Hidden
        } else if self.panes.is_empty() {
            self.visible = true;
            ToggleResult::NeedSpawn
        } else {
            self.visible = true;
            if self.focus.is_none() {
                self.focus = self.panes.last().map(|fp| fp.pane_id);
            }
            ToggleResult::Shown
        }
    }

    pub fn show(&mut self) {
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.focus = None;
    }

    /// Add a new floating pane on top of the z-order and focus it.
    ///
    /// A `pane_id` already present is left untouched: its geom, z-order, and the
    /// current focus are all unchanged. Callers that want "ensure present, then
    /// focus and raise" must call [`focus_pane`] and [`bring_to_front`].
    ///
    /// [`focus_pane`]: FloatingLayer::focus_pane
    /// [`bring_to_front`]: FloatingLayer::bring_to_front
    pub fn add_pane(&mut self, pane_id: PaneId, geom: FloatingGeom) {
        if self.contains(pane_id) {
            return;
        }
        self.panes.push(FloatingPane { pane_id, geom });
        self.focus = Some(pane_id);
    }

    pub fn remove_pane(&mut self, pane_id: PaneId) -> Option<FloatingPane> {
        let idx = self.panes.iter().position(|fp| fp.pane_id == pane_id)?;
        let removed = self.panes.remove(idx);
        if self.focus == Some(pane_id) {
            self.focus = if self.panes.is_empty() {
                None
            } else {
                let next_idx = idx.min(self.panes.len() - 1);
                Some(self.panes[next_idx].pane_id)
            };
        }
        Some(removed)
    }

    pub fn focused_pane_id(&self) -> Option<PaneId> {
        self.focus
    }

    pub fn contains(&self, pane_id: PaneId) -> bool {
        self.panes.iter().any(|fp| fp.pane_id == pane_id)
    }

    /// Raise a pane to the top of the z-order. Does not change focus; raising
    /// and focusing are independent (a click-to-raise handler that should also
    /// focus must call [`focus_pane`] as well).
    ///
    /// [`focus_pane`]: FloatingLayer::focus_pane
    pub fn bring_to_front(&mut self, pane_id: PaneId) {
        let Some(idx) = self.panes.iter().position(|fp| fp.pane_id == pane_id) else {
            return;
        };
        let fp = self.panes.remove(idx);
        self.panes.push(fp);
    }

    /// Focus a pane. Does not change the z-order; see [`bring_to_front`].
    ///
    /// [`bring_to_front`]: FloatingLayer::bring_to_front
    pub fn focus_pane(&mut self, pane_id: PaneId) {
        if self.contains(pane_id) {
            self.focus = Some(pane_id);
        }
    }

    pub fn cycle_focus(&mut self, reverse: bool) {
        if self.panes.is_empty() {
            return;
        }
        let Some(current) = self.focus else {
            self.focus = self.panes.last().map(|fp| fp.pane_id);
            return;
        };
        let Some(idx) = self.panes.iter().position(|fp| fp.pane_id == current) else {
            self.focus = self.panes.last().map(|fp| fp.pane_id);
            return;
        };
        let next_idx = if reverse {
            if idx == 0 {
                self.panes.len() - 1
            } else {
                idx - 1
            }
        } else {
            (idx + 1) % self.panes.len()
        };
        self.focus = Some(self.panes[next_idx].pane_id);
    }

    pub fn unfocus(&mut self) {
        self.focus = None;
    }

    pub fn move_focused(&mut self, dx: i16, dy: i16, bounds: Rect) {
        let Some(focus_id) = self.focus else {
            return;
        };
        let Some(fp) = self.panes.iter_mut().find(|fp| fp.pane_id == focus_id) else {
            return;
        };
        fp.geom.x = (fp.geom.x as i32 + dx as i32).clamp(0, u16::MAX as i32) as u16;
        fp.geom.y = (fp.geom.y as i32 + dy as i32).clamp(0, u16::MAX as i32) as u16;
        fp.geom.clamp_within(bounds);
    }

    pub fn resize_focused(&mut self, dw: i16, dh: i16, bounds: Rect) {
        let Some(focus_id) = self.focus else {
            return;
        };
        let Some(fp) = self.panes.iter_mut().find(|fp| fp.pane_id == focus_id) else {
            return;
        };
        let new_w = (fp.geom.width as i32 + dw as i32).clamp(3, u16::MAX as i32) as u16;
        let new_h = (fp.geom.height as i32 + dh as i32).clamp(3, u16::MAX as i32) as u16;
        fp.geom.width = new_w.min(bounds.width);
        fp.geom.height = new_h.min(bounds.height);
        fp.geom.clamp_within(bounds);
    }

    pub fn pane_at(&self, x: u16, y: u16) -> Option<PaneId> {
        self.panes
            .iter()
            .rev()
            .find(|fp| fp.geom.contains(x, y))
            .map(|fp| fp.pane_id)
    }

    pub fn clamp_all_within(&mut self, bounds: Rect) {
        for fp in &mut self.panes {
            fp.geom.clamp_within(bounds);
        }
    }

    /// Iterate panes in z-order, bottom first and top last. Render must draw in
    /// this order so the topmost pane overdraws the rest; hit-testing wants the
    /// reverse, which [`pane_at`] handles.
    ///
    /// [`pane_at`]: FloatingLayer::pane_at
    pub fn iter(&self) -> impl Iterator<Item = &FloatingPane> {
        self.panes.iter()
    }

    /// Pane IDs in z-order, bottom first and top last (see [`iter`]).
    ///
    /// [`iter`]: FloatingLayer::iter
    pub fn pane_ids(&self) -> impl Iterator<Item = PaneId> + '_ {
        self.panes.iter().map(|fp| fp.pane_id)
    }

    pub fn next_geom(&self, bounds: Rect) -> FloatingGeom {
        let base_w = bounds.width / 2;
        let base_h = bounds.height / 2;
        let base_x = bounds.x.saturating_add(bounds.width / 4);
        let base_y = bounds.y.saturating_add(bounds.height / 4);

        let offset = (self.panes.len() % 8) as u16;
        let x = base_x
            .saturating_add(offset.saturating_mul(2))
            .min(bounds.x.saturating_add(bounds.width).saturating_sub(base_w));
        let y = base_y.saturating_add(offset).min(
            bounds
                .y
                .saturating_add(bounds.height)
                .saturating_sub(base_h),
        );

        FloatingGeom {
            x,
            y,
            width: base_w,
            height: base_h,
        }
    }

    pub fn geom_for(&self, pane_id: PaneId) -> Option<&FloatingGeom> {
        self.panes
            .iter()
            .find(|fp| fp.pane_id == pane_id)
            .map(|fp| &fp.geom)
    }

    pub fn geom_for_mut(&mut self, pane_id: PaneId) -> Option<&mut FloatingGeom> {
        self.panes
            .iter_mut()
            .find(|fp| fp.pane_id == pane_id)
            .map(|fp| &mut fp.geom)
    }
}

#[cfg(test)]
impl FloatingLayer {
    pub fn assert_invariants_for_test(&self) {
        if let Some(focus) = self.focus {
            assert!(
                self.contains(focus),
                "focus {:?} not in floating layer",
                focus
            );
        }
        let mut seen = std::collections::HashSet::new();
        for fp in &self.panes {
            assert!(
                seen.insert(fp.pane_id),
                "duplicate pane {:?} in floating layer",
                fp.pane_id
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geom(x: u16, y: u16, w: u16, h: u16) -> FloatingGeom {
        FloatingGeom {
            x,
            y,
            width: w,
            height: h,
        }
    }

    fn bounds() -> Rect {
        Rect::new(0, 0, 100, 50)
    }

    #[test]
    fn floating_geom_rect_and_inner_rect() {
        let g = geom(5, 10, 20, 15);
        assert_eq!(g.rect(), Rect::new(5, 10, 20, 15));
        assert_eq!(g.inner_rect(), Rect::new(6, 11, 18, 13));
    }

    #[test]
    fn floating_geom_inner_rect_degenerate() {
        let g = geom(0, 0, 2, 2);
        assert_eq!(g.inner_rect(), Rect::new(0, 0, 0, 0));

        let g2 = geom(0, 0, 1, 1);
        assert_eq!(g2.inner_rect(), Rect::new(0, 0, 0, 0));
    }

    #[test]
    fn floating_geom_contains() {
        let g = geom(10, 10, 20, 10);
        assert!(g.contains(10, 10));
        assert!(g.contains(29, 19));
        assert!(g.contains(15, 15));
        assert!(!g.contains(9, 10));
        assert!(!g.contains(30, 10));
        assert!(!g.contains(10, 20));
    }

    #[test]
    fn floating_geom_border_hit_edges() {
        let g = geom(10, 10, 20, 10);
        assert_eq!(g.border_hit(10, 10), Some(BorderEdge::TopLeft));
        assert_eq!(g.border_hit(29, 10), Some(BorderEdge::TopRight));
        assert_eq!(g.border_hit(10, 19), Some(BorderEdge::BottomLeft));
        assert_eq!(g.border_hit(29, 19), Some(BorderEdge::BottomRight));
        assert_eq!(g.border_hit(15, 10), Some(BorderEdge::Top));
        assert_eq!(g.border_hit(15, 19), Some(BorderEdge::Bottom));
        assert_eq!(g.border_hit(10, 15), Some(BorderEdge::Left));
        assert_eq!(g.border_hit(29, 15), Some(BorderEdge::Right));
        assert_eq!(g.border_hit(15, 15), None);
        assert_eq!(g.border_hit(5, 5), None);
    }

    #[test]
    fn floating_geom_border_hit_single_cell_resolves_to_corner() {
        // A 1x1 pane is simultaneously on all four edges; match-arm order
        // makes corners win, so the single cell deterministically reports TopLeft.
        let g = geom(10, 10, 1, 1);
        assert_eq!(g.border_hit(10, 10), Some(BorderEdge::TopLeft));
        assert_eq!(g.border_hit(11, 10), None);
    }

    #[test]
    fn floating_geom_clamp_within_no_op_when_in_bounds() {
        let mut g = geom(10, 10, 20, 10);
        let b = Rect::new(0, 0, 100, 50);
        g.clamp_within(b);
        assert_eq!(g, geom(10, 10, 20, 10));
    }

    #[test]
    fn floating_geom_clamp_within_moves_when_out_of_bounds() {
        let mut g = geom(90, 45, 20, 10);
        let b = Rect::new(0, 0, 100, 50);
        g.clamp_within(b);
        assert_eq!(g.x, 80);
        assert_eq!(g.y, 40);
    }

    #[test]
    fn floating_geom_clamp_within_shrinks_when_larger_than_bounds() {
        let mut g = geom(0, 0, 200, 100);
        let b = Rect::new(0, 0, 50, 25);
        g.clamp_within(b);
        assert_eq!(g.width, 50);
        assert_eq!(g.height, 25);
        assert_eq!(g.x, 0);
        assert_eq!(g.y, 0);
    }

    #[test]
    fn floating_geom_clamp_within_moves_left_when_below_origin() {
        let mut g = geom(0, 0, 20, 10);
        let b = Rect::new(5, 5, 100, 50);
        g.clamp_within(b);
        assert_eq!(g.x, 5);
        assert_eq!(g.y, 5);
    }

    #[test]
    fn floating_layer_add_remove_basic() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        let id2 = PaneId::alloc();

        layer.add_pane(id1, geom(0, 0, 20, 10));
        layer.add_pane(id2, geom(5, 5, 20, 10));

        assert_eq!(layer.count(), 2);
        assert!(layer.contains(id1));
        assert!(layer.contains(id2));
        assert_eq!(layer.focused_pane_id(), Some(id2));

        let removed = layer.remove_pane(id2);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().pane_id, id2);
        assert_eq!(layer.count(), 1);
        assert!(!layer.contains(id2));
        assert_eq!(layer.focused_pane_id(), Some(id1));
        layer.assert_invariants_for_test();
    }

    #[test]
    fn floating_layer_add_duplicate_no_op() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        layer.add_pane(id1, geom(0, 0, 20, 10));
        layer.add_pane(id1, geom(5, 5, 30, 15));
        assert_eq!(layer.count(), 1);
        layer.assert_invariants_for_test();
    }

    #[test]
    fn floating_layer_remove_unknown_no_op() {
        let mut layer = FloatingLayer::new();
        let unknown = PaneId::alloc();
        assert!(layer.remove_pane(unknown).is_none());
    }

    #[test]
    fn floating_layer_bring_to_front() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        let id2 = PaneId::alloc();
        let id3 = PaneId::alloc();

        layer.add_pane(id1, geom(0, 0, 10, 10));
        layer.add_pane(id2, geom(0, 0, 10, 10));
        layer.add_pane(id3, geom(0, 0, 10, 10));

        let ids: Vec<_> = layer.pane_ids().collect();
        assert_eq!(ids, vec![id1, id2, id3]);

        layer.bring_to_front(id1);
        let ids: Vec<_> = layer.pane_ids().collect();
        assert_eq!(ids, vec![id2, id3, id1]);
        layer.assert_invariants_for_test();
    }

    #[test]
    fn floating_layer_bring_to_front_unknown_no_op() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        layer.add_pane(id1, geom(0, 0, 10, 10));
        layer.bring_to_front(PaneId::alloc());
        let ids: Vec<_> = layer.pane_ids().collect();
        assert_eq!(ids, vec![id1]);
    }

    #[test]
    fn floating_layer_toggle_visible_hidden_to_shown() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        layer.add_pane(id1, geom(0, 0, 10, 10));
        layer.hide();

        let result = layer.toggle_visible();
        assert_eq!(result, ToggleResult::Shown);
        assert!(layer.is_visible());
        assert_eq!(layer.focused_pane_id(), Some(id1));
    }

    #[test]
    fn add_pane_while_hidden_records_focus_without_becoming_focused() {
        // Per the visible/focus contract: add_pane records focus regardless of
        // visibility, but the layer is not "focused" until shown. The focus
        // invariant still holds (focus points at a contained pane).
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        layer.add_pane(id1, geom(0, 0, 10, 10));
        assert!(!layer.is_visible());
        assert!(!layer.is_focused());
        assert_eq!(layer.focused_pane_id(), Some(id1));
        layer.assert_invariants_for_test();

        // Showing the layer then makes that recorded focus active.
        assert_eq!(layer.toggle_visible(), ToggleResult::Shown);
        assert!(layer.is_focused());
        assert_eq!(layer.focused_pane_id(), Some(id1));
    }

    #[test]
    fn floating_layer_toggle_visible_shown_to_hidden() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        layer.add_pane(id1, geom(0, 0, 10, 10));
        layer.show();
        layer.focus_pane(id1);

        let result = layer.toggle_visible();
        assert_eq!(result, ToggleResult::Hidden);
        assert!(!layer.is_visible());
        assert_eq!(layer.focused_pane_id(), None);
    }

    #[test]
    fn floating_layer_toggle_visible_empty_layer() {
        let mut layer = FloatingLayer::new();
        let result = layer.toggle_visible();
        assert_eq!(result, ToggleResult::NeedSpawn);
        assert!(layer.is_visible());
    }

    #[test]
    fn floating_layer_focus_cycle_forward() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        let id2 = PaneId::alloc();
        let id3 = PaneId::alloc();

        layer.add_pane(id1, geom(0, 0, 10, 10));
        layer.add_pane(id2, geom(10, 0, 10, 10));
        layer.add_pane(id3, geom(20, 0, 10, 10));
        layer.focus_pane(id1);

        layer.cycle_focus(false);
        assert_eq!(layer.focused_pane_id(), Some(id2));

        layer.cycle_focus(false);
        assert_eq!(layer.focused_pane_id(), Some(id3));

        layer.cycle_focus(false);
        assert_eq!(layer.focused_pane_id(), Some(id1));
        layer.assert_invariants_for_test();
    }

    #[test]
    fn floating_layer_focus_cycle_reverse() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        let id2 = PaneId::alloc();
        let id3 = PaneId::alloc();

        layer.add_pane(id1, geom(0, 0, 10, 10));
        layer.add_pane(id2, geom(10, 0, 10, 10));
        layer.add_pane(id3, geom(20, 0, 10, 10));
        layer.focus_pane(id1);

        layer.cycle_focus(true);
        assert_eq!(layer.focused_pane_id(), Some(id3));

        layer.cycle_focus(true);
        assert_eq!(layer.focused_pane_id(), Some(id2));
        layer.assert_invariants_for_test();
    }

    #[test]
    fn floating_layer_focus_cycle_empty_no_op() {
        let mut layer = FloatingLayer::new();
        layer.cycle_focus(false);
        assert_eq!(layer.focused_pane_id(), None);
    }

    #[test]
    fn floating_layer_focus_cycle_no_focus_selects_top() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        let id2 = PaneId::alloc();
        layer.add_pane(id1, geom(0, 0, 10, 10));
        layer.add_pane(id2, geom(10, 0, 10, 10));
        layer.unfocus();

        layer.cycle_focus(false);
        assert_eq!(layer.focused_pane_id(), Some(id2));
        layer.assert_invariants_for_test();
    }

    #[test]
    fn floating_layer_focus_pane_unknown_no_op() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        layer.add_pane(id1, geom(0, 0, 10, 10));
        layer.focus_pane(PaneId::alloc());
        assert_eq!(layer.focused_pane_id(), Some(id1));
    }

    #[test]
    fn floating_layer_pane_at_returns_topmost() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        let id2 = PaneId::alloc();
        layer.add_pane(id1, geom(0, 0, 20, 20));
        layer.add_pane(id2, geom(5, 5, 20, 20));

        assert_eq!(layer.pane_at(10, 10), Some(id2));
        assert_eq!(layer.pane_at(2, 2), Some(id1));
        assert_eq!(layer.pane_at(50, 50), None);
    }

    #[test]
    fn floating_layer_pane_at_respects_z_after_bring_to_front() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        let id2 = PaneId::alloc();
        layer.add_pane(id1, geom(0, 0, 20, 20));
        layer.add_pane(id2, geom(0, 0, 20, 20));

        assert_eq!(layer.pane_at(5, 5), Some(id2));
        layer.bring_to_front(id1);
        assert_eq!(layer.pane_at(5, 5), Some(id1));
    }

    #[test]
    fn floating_layer_clamp_all_within() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        let id2 = PaneId::alloc();
        layer.add_pane(id1, geom(5, 5, 10, 10));
        layer.add_pane(id2, geom(95, 45, 10, 10));

        layer.clamp_all_within(Rect::new(0, 0, 100, 50));

        let g1 = layer.geom_for(id1).unwrap();
        assert_eq!(*g1, geom(5, 5, 10, 10));

        let g2 = layer.geom_for(id2).unwrap();
        assert_eq!(g2.x, 90);
        assert_eq!(g2.y, 40);
    }

    #[test]
    fn floating_layer_next_geom_cascading() {
        let layer = FloatingLayer::new();
        let b = bounds();
        let g0 = layer.next_geom(b);
        assert_eq!(g0.x, 25);
        assert_eq!(g0.y, 12);
        assert_eq!(g0.width, 50);
        assert_eq!(g0.height, 25);

        let mut layer = FloatingLayer::new();
        layer.add_pane(PaneId::alloc(), geom(0, 0, 10, 10));
        let g1 = layer.next_geom(b);
        assert_eq!(g1.x, 27);
        assert_eq!(g1.y, 13);
    }

    #[test]
    fn floating_layer_next_geom_modulo_wrap() {
        let mut layer = FloatingLayer::new();
        let b = bounds();
        for _ in 0..8 {
            layer.add_pane(PaneId::alloc(), geom(0, 0, 10, 10));
        }
        let g8 = layer.next_geom(b);
        let g0 = FloatingLayer::new().next_geom(b);
        assert_eq!(g8.x, g0.x);
        assert_eq!(g8.y, g0.y);
    }

    #[test]
    fn floating_layer_move_focused() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        layer.add_pane(id1, geom(10, 10, 20, 10));

        layer.move_focused(5, 3, bounds());
        let g = layer.geom_for(id1).unwrap();
        assert_eq!(g.x, 15);
        assert_eq!(g.y, 13);
        layer.assert_invariants_for_test();
    }

    #[test]
    fn floating_layer_move_focused_clamps() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        layer.add_pane(id1, geom(80, 40, 20, 10));

        layer.move_focused(50, 50, bounds());
        let g = layer.geom_for(id1).unwrap();
        assert!(g.x + g.width <= 100);
        assert!(g.y + g.height <= 50);
    }

    #[test]
    fn floating_layer_move_focused_large_delta_does_not_wrap() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        layer.add_pane(id1, geom(60000, 0, 20, 10));
        // A large positive delta overflows u16 with a bare `as u16` cast and
        // wraps x back to a small value; the clamp must instead saturate x to
        // the right, keeping it monotonically increasing.
        layer.move_focused(i16::MAX, 0, Rect::new(0, 0, u16::MAX, 50));
        let g = layer.geom_for(id1).unwrap();
        assert!(g.x >= 60000);
    }

    #[test]
    fn floating_layer_resize_focused_large_delta_does_not_wrap() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        layer.add_pane(id1, geom(0, 0, 60000, 40));
        layer.resize_focused(i16::MAX, 0, Rect::new(0, 0, u16::MAX, 50));
        let g = layer.geom_for(id1).unwrap();
        assert!(g.width >= 60000);
    }

    #[test]
    fn floating_layer_move_focused_no_focus_no_op() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        layer.add_pane(id1, geom(10, 10, 20, 10));
        layer.unfocus();
        layer.move_focused(5, 5, bounds());
        let g = layer.geom_for(id1).unwrap();
        assert_eq!(g.x, 10);
    }

    #[test]
    fn floating_layer_resize_focused() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        layer.add_pane(id1, geom(10, 10, 20, 10));

        layer.resize_focused(5, 3, bounds());
        let g = layer.geom_for(id1).unwrap();
        assert_eq!(g.width, 25);
        assert_eq!(g.height, 13);
        layer.assert_invariants_for_test();
    }

    #[test]
    fn floating_layer_resize_focused_minimum_size() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        layer.add_pane(id1, geom(10, 10, 5, 5));

        layer.resize_focused(-100, -100, bounds());
        let g = layer.geom_for(id1).unwrap();
        assert_eq!(g.width, 3);
        assert_eq!(g.height, 3);
    }

    #[test]
    fn floating_layer_remove_focused_advances_to_next() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        let id2 = PaneId::alloc();
        let id3 = PaneId::alloc();
        layer.add_pane(id1, geom(0, 0, 10, 10));
        layer.add_pane(id2, geom(10, 0, 10, 10));
        layer.add_pane(id3, geom(20, 0, 10, 10));
        layer.focus_pane(id2);

        layer.remove_pane(id2);
        assert_eq!(layer.focused_pane_id(), Some(id3));
        layer.assert_invariants_for_test();
    }

    #[test]
    fn floating_layer_remove_last_clears_focus() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        layer.add_pane(id1, geom(0, 0, 10, 10));
        layer.remove_pane(id1);
        assert_eq!(layer.focused_pane_id(), None);
        layer.assert_invariants_for_test();
    }

    #[test]
    fn floating_layer_remove_tail_wraps_focus() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        let id2 = PaneId::alloc();
        let id3 = PaneId::alloc();
        layer.add_pane(id1, geom(0, 0, 10, 10));
        layer.add_pane(id2, geom(10, 0, 10, 10));
        layer.add_pane(id3, geom(20, 0, 10, 10));
        layer.focus_pane(id3);

        layer.remove_pane(id3);
        assert_eq!(layer.focused_pane_id(), Some(id2));
        layer.assert_invariants_for_test();
    }

    #[test]
    fn focus_target_accessors() {
        let id = PaneId::alloc();
        let tiled = FocusTarget::Tiled(id);
        let floating = FocusTarget::Floating(id);

        assert_eq!(tiled.pane_id(), id);
        assert_eq!(floating.pane_id(), id);
        assert!(!tiled.is_floating());
        assert!(floating.is_floating());
    }

    #[test]
    fn removing_focused_head_advances_to_survivor() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        let id2 = PaneId::alloc();
        layer.add_pane(id1, geom(0, 0, 10, 10));
        layer.add_pane(id2, geom(10, 0, 10, 10));
        layer.focus_pane(id1);
        layer.remove_pane(id1);
        layer.assert_invariants_for_test();
        assert_eq!(layer.focused_pane_id(), Some(id2));
    }

    #[test]
    fn adversarial_empty_but_visible_layer() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        layer.add_pane(id1, geom(0, 0, 10, 10));
        layer.show();
        layer.remove_pane(id1);
        assert!(layer.is_visible());
        assert!(!layer.is_focused());
        assert_eq!(layer.focused_pane_id(), None);
        layer.assert_invariants_for_test();
    }

    #[test]
    fn cycle_after_removing_focused_pane_stays_consistent() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        let id2 = PaneId::alloc();
        layer.add_pane(id1, geom(0, 0, 10, 10));
        layer.add_pane(id2, geom(10, 0, 10, 10));
        layer.focus_pane(id1);
        layer.remove_pane(id1);
        assert_eq!(layer.focused_pane_id(), Some(id2));
        layer.cycle_focus(false);
        assert_eq!(layer.focused_pane_id(), Some(id2));
        layer.assert_invariants_for_test();
    }

    #[test]
    fn adversarial_all_methods_no_op_on_unknown_pane_id() {
        let mut layer = FloatingLayer::new();
        let id1 = PaneId::alloc();
        layer.add_pane(id1, geom(0, 0, 10, 10));
        let unknown = PaneId::alloc();

        assert!(!layer.contains(unknown));
        assert!(layer.remove_pane(unknown).is_none());
        layer.bring_to_front(unknown);
        layer.focus_pane(unknown);
        assert_eq!(layer.focused_pane_id(), Some(id1));
        assert_eq!(layer.geom_for(unknown), None);
        assert_eq!(layer.geom_for_mut(unknown).map(|_| ()), None);
        layer.assert_invariants_for_test();
    }
}
