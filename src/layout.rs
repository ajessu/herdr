//! BSP tree layout for tiling panes within a workspace.

use std::cmp::Reverse;

use ratatui::{
    layout::{Direction, Rect},
    widgets::Borders,
};
use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PaneId(u32);

/// Global atomic counter for unique PaneId generation across all workspaces.
static NEXT_PANE_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

impl PaneId {
    /// Allocate a globally unique PaneId.
    pub fn alloc() -> Self {
        Self(NEXT_PANE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }

    pub fn raw(self) -> u32 {
        self.0
    }

    /// Reconstruct from a saved u32 (persistence only).
    pub fn from_raw(id: u32) -> Self {
        Self(id)
    }
}

/// Snapshot of a pane's position and focus state after layout.
#[derive(Clone)]
pub struct PaneInfo {
    pub id: PaneId,
    /// Outer rect (including borders if present).
    pub rect: Rect,
    /// Inner rect (content area, excluding borders). Used for selection.
    pub inner_rect: Rect,
    /// Visible scrollbar lane, when scrollback is present. `inner_rect` may still
    /// exclude a stable hidden gutter when this is `None`.
    pub scrollbar_rect: Option<Rect>,
    /// Borders drawn around this pane after UI chrome is applied.
    pub borders: Borders,
    pub is_focused: bool,
    pub stack: Option<StackMember>,
}

/// Metadata for a pane that lives inside a `Node::Stack`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackMember {
    pub collapsed: bool,
    pub position: usize,
    pub count: usize,
}

/// Info about a split boundary, used for mouse drag resize.
#[derive(Clone)]
pub struct SplitBorder {
    /// Position of the divider line (x for horizontal split, y for vertical).
    pub pos: u16,
    /// Direction of the split that created this border.
    pub direction: Direction,
    /// Ratio assigned to the first child of this split.
    pub ratio: f32,
    /// Total area of the split node.
    pub area: Rect,
    /// Path from root to this split node (false=first, true=second).
    pub path: Vec<bool>,
}

/// Cardinal direction for pane navigation.
#[derive(Debug, Clone, Copy)]
pub enum NavDirection {
    Left,
    Right,
    Up,
    Down,
}

/// A node in the BSP tree. Public for serialization.
pub enum Node {
    Pane(PaneId),
    Split {
        direction: Direction,
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
    Stack {
        panes: Vec<PaneId>,
        expanded: usize,
    },
}

/// BSP tiling layout. Tracks a tree of splits and a focused pane.
pub struct TileLayout {
    root: Node,
    focus: PaneId,
    /// Pane focused before `focus`, used by `close_focused`. Only a real focus
    /// move writes it; tree edits go through the target-taking primitives
    /// (`split_pane`, `close_pane`, unfocused `insert_pane_near`) so internal
    /// focus excursions never corrupt it.
    prev_focus: Option<PaneId>,
}

impl TileLayout {
    /// Create a new layout with a single pane (globally unique ID).
    /// Returns (layout, root_pane_id) so the caller can create the pane.
    pub fn new() -> (Self, PaneId) {
        let root_id = PaneId::alloc();
        (
            Self {
                root: Node::Pane(root_id),
                focus: root_id,
                prev_focus: None,
            },
            root_id,
        )
    }

    /// Move focus, recording the pane being left. No-op when focus is unchanged.
    fn set_focus(&mut self, id: PaneId) {
        if id != self.focus {
            self.prev_focus = Some(self.focus);
            self.focus = id;
        }
    }

    pub fn focused(&self) -> PaneId {
        self.focus
    }

    pub fn pane_count(&self) -> usize {
        count_panes(&self.root)
    }

    /// Compute rects for all panes given the available area.
    pub fn panes(&self, area: Rect) -> Vec<PaneInfo> {
        let mut result = Vec::new();
        collect_panes(&self.root, area, self.focus, &mut result);
        result
    }

    /// Collect all split boundaries for mouse drag resize.
    pub fn splits(&self, area: Rect) -> Vec<SplitBorder> {
        let mut result = Vec::new();
        collect_splits(&self.root, area, &mut Vec::new(), &mut result);
        result
    }

    /// Split the focused pane. Returns the new pane's id. Production splits
    /// flow through `Tab` so a failed runtime spawn can roll back; this remains
    /// as the user-split shape for tests.
    #[cfg(test)]
    pub fn split_focused(&mut self, direction: Direction) -> PaneId {
        self.split_focused_with_ratio(direction, 0.5)
    }

    /// Split the focused pane with a custom first-child ratio.
    #[cfg(test)]
    pub fn split_focused_with_ratio(&mut self, direction: Direction, ratio: f32) -> PaneId {
        let new_id = self
            .split_pane(self.focus, direction, ratio)
            .expect("focused pane is in the layout");
        self.set_focus(new_id);
        new_id
    }

    /// Split `target` without moving focus. Returns the new pane's id, or None
    /// when `target` is not in the layout.
    pub fn split_pane(
        &mut self,
        target: PaneId,
        direction: Direction,
        ratio: f32,
    ) -> Option<PaneId> {
        let node = find_pane_mut(&mut self.root, target)?;
        let new_id = PaneId::alloc();
        split_found(node, target, direction, new_id, valid_split_ratio(ratio));
        Some(new_id)
    }

    /// Insert an existing pane id next to a target pane without allocating a new
    /// pane or spawning a terminal runtime. When `focus` is false, focus and its
    /// history are left untouched. Stack nodes are treated as opaque leaves:
    /// `split_found` wraps a stack holding the target as one unit, so a new pane
    /// always splits *beside* the stack, never between its members.
    pub fn insert_pane_near(
        &mut self,
        target: PaneId,
        moved: PaneId,
        direction: Direction,
        ratio: f32,
        focus: bool,
    ) -> bool {
        if target == moved {
            return false;
        }
        let ids = self.pane_ids();
        if ids.contains(&moved) {
            return false;
        }
        let Some(node) = find_pane_mut(&mut self.root, target) else {
            return false;
        };
        split_found(node, target, direction, moved, valid_split_ratio(ratio));
        if focus {
            self.set_focus(moved);
        }
        true
    }

    /// Close the focused pane, returning focus to the pane it came from when
    /// that pane is still open. Returns false if it's the last pane.
    pub fn close_focused(&mut self) -> bool {
        if self.pane_count() <= 1 {
            return false;
        }
        let target = self.focus;
        let was_in_stack = contains_in_stack(&self.root, target);
        let ids = self.pane_ids();

        // For non-stack closes, precompute flat-order neighbor as the fallback.
        // For stack closes, collect the target's stack-neighbors so we can find
        // the promoted member in the post-removal tree.
        let (flat_focus, stack_neighbors) = if !was_in_stack {
            let pos = ids.iter().position(|id| *id == target).unwrap();
            let nf = if pos + 1 < ids.len() {
                ids[pos + 1]
            } else {
                ids[pos - 1]
            };
            (Some(nf), Vec::new())
        } else {
            let neighbors = stack_member_neighbors(&self.root, target);
            (None, neighbors)
        };

        let placeholder = PaneId::from_raw(0);
        let old = std::mem::replace(&mut self.root, Node::Pane(placeholder));
        if let Some(new_root) = remove_pane(old, target) {
            self.root = new_root;
            let fallback = if was_in_stack {
                let promoted = find_promoted_after_close(&self.root, &stack_neighbors)
                    .unwrap_or_else(|| self.pane_ids()[0]);
                debug!(
                    target = target.raw(),
                    new_focus = promoted.raw(),
                    "close_focused: stack-promotion branch"
                );
                promoted
            } else {
                let nf = flat_focus.unwrap();
                debug!(
                    target = target.raw(),
                    new_focus = nf.raw(),
                    "close_focused: flat-order branch"
                );
                nf
            };
            self.focus = match self.prev_focus {
                Some(prev) if prev != target && ids.contains(&prev) => prev,
                _ => fallback,
            };
            self.prev_focus = None;
            true
        } else {
            false
        }
    }

    /// Close any pane. Focus and its history are left alone unless the closed
    /// pane is the focused one.
    pub fn close_pane(&mut self, id: PaneId) -> bool {
        if self.focus == id {
            return self.close_focused();
        }
        if self.pane_count() <= 1 || !self.pane_ids().contains(&id) {
            return false;
        }
        let placeholder = PaneId::from_raw(0);
        let old = std::mem::replace(&mut self.root, Node::Pane(placeholder));
        let Some(new_root) = remove_pane(old, id) else {
            return false;
        };
        self.root = new_root;
        if self.prev_focus == Some(id) {
            self.prev_focus = None;
        }
        true
    }

    pub fn focus_pane(&mut self, id: PaneId) {
        if self.pane_ids().contains(&id) {
            self.set_focus(id);
            expand_member(&mut self.root, id);
        }
    }

    /// Returns true if the focused pane is inside a `Node::Stack`.
    pub fn focused_in_stack(&self) -> bool {
        contains_in_stack(&self.root, self.focus)
    }

    /// True for a stack member other than the expanded one; it shows only its
    /// title row.
    pub fn is_collapsed_stack_member(&self, id: PaneId) -> bool {
        collapsed_in_stack(&self.root, id)
    }

    /// Swap two pane ids in the layout tree while preserving split shape and
    /// ratios. Returns true only when both panes exist and are different.
    pub fn swap_panes(&mut self, first: PaneId, second: PaneId) -> bool {
        if first == second {
            return false;
        }
        let ids = self.pane_ids();
        if !ids.contains(&first) || !ids.contains(&second) {
            return false;
        }
        swap_pane_ids(&mut self.root, first, second);
        expand_member(&mut self.root, self.focus);
        true
    }

    /// Set the ratio of a split node at the given path.
    pub fn set_ratio_at(&mut self, path: &[bool], ratio: f32) -> bool {
        set_ratio_at(&mut self.root, path, ratio.clamp(0.1, 0.9))
    }

    /// Adjust the nearest split in the given direction for the focused pane.
    /// `delta` is positive to grow, negative to shrink.
    pub fn resize_focused(&mut self, nav: NavDirection, delta: f32, area: Rect) {
        let panes = self.panes(area);
        let Some(focused) = panes.iter().find(|p| p.is_focused) else {
            return;
        };
        let focused_rect = focused.rect;
        let splits = self.splits(area);

        let target_dir = match nav {
            NavDirection::Left | NavDirection::Right => Direction::Horizontal,
            NavDirection::Up | NavDirection::Down => Direction::Vertical,
        };
        let grows = matches!(nav, NavDirection::Right | NavDirection::Down);

        let best = nearest_resize_split(&splits, target_dir, focused_rect, nav).or_else(|| {
            nearest_resize_split(&splits, target_dir, focused_rect, opposite_direction(nav))
        });

        if let Some(split) = best {
            let path = split.path.clone();
            let current_ratio = get_ratio_at(&self.root, &path).unwrap_or(0.5);
            let adj = if grows { delta } else { -delta };
            self.set_ratio_at(&path, current_ratio + adj);
        }
    }

    pub fn resize_pane(
        &mut self,
        pane_id: PaneId,
        nav: NavDirection,
        delta: f32,
        area: Rect,
    ) -> bool {
        if !self.pane_ids().contains(&pane_id) {
            return false;
        }
        let before = split_ratios(&self.root);
        let previous_focus = self.focus;
        self.focus = pane_id;
        self.resize_focused(nav, delta, area);
        self.focus = previous_focus;
        split_ratios(&self.root) != before
    }

    pub fn pane_ids(&self) -> Vec<PaneId> {
        let mut ids = Vec::new();
        collect_ids(&self.root, &mut ids);
        ids
    }

    /// Access the tree root for serialization.
    pub fn root(&self) -> &Node {
        &self.root
    }

    /// Reconstruct a layout from a saved tree. Reconciles stack `expanded`
    /// indices with the restored focus so the invariant holds immediately.
    pub fn from_saved(mut root: Node, focus: PaneId) -> Self {
        expand_member(&mut root, focus);
        Self {
            root,
            focus,
            prev_focus: None,
        }
    }

    /// Stack the focused pane with its adjacent sibling subtree. Returns false
    /// if not possible (lone root, sibling is a multi-pane Split subtree).
    pub fn stack_focused(&mut self) -> bool {
        let placeholder = PaneId::from_raw(0);
        let old = std::mem::replace(&mut self.root, Node::Pane(placeholder));
        let (new_root, success) = stack_at_focus(old, self.focus);
        self.root = new_root;
        if success {
            expand_member(&mut self.root, self.focus);
        }
        success
    }

    /// Remove the focused member from its stack and re-place as a sibling split.
    /// Returns false if the focused pane is not in a stack.
    pub fn unstack_focused(&mut self, direction: Direction, ratio: f32) -> bool {
        if !self.focused_in_stack() {
            return false;
        }
        let placeholder = PaneId::from_raw(0);
        let old = std::mem::replace(&mut self.root, Node::Pane(placeholder));
        let (new_root, success) = unstack_at_focus(old, self.focus, direction, ratio);
        self.root = new_root;
        success
    }

    /// Fork: stack `pane_id` with its adjacent sibling without moving focus.
    pub fn stack_pane(&mut self, pane_id: PaneId) -> bool {
        self.with_focus_on(pane_id, Self::stack_focused)
    }

    /// Fork: take `pane_id` out of its stack without moving focus.
    pub fn unstack_pane(&mut self, pane_id: PaneId, direction: Direction, ratio: f32) -> bool {
        self.with_focus_on(pane_id, |layout| layout.unstack_focused(direction, ratio))
    }

    /// Run a focus-relative edit against `pane_id`, then restore focus and
    /// re-expand it so the expanded stack member still matches focus.
    fn with_focus_on(&mut self, pane_id: PaneId, edit: impl FnOnce(&mut Self) -> bool) -> bool {
        if !self.pane_ids().contains(&pane_id) {
            return false;
        }
        let previous_focus = self.focus;
        self.focus = pane_id;
        let changed = edit(self);
        self.focus = previous_focus;
        expand_member(&mut self.root, self.focus);
        changed
    }

    /// Try to fold `new_id` into the stack that contains `stack_member`.
    /// `stack_member` is the original focused pane (the expanded member before
    /// the split that created `new_id`). After a successful fold, focus and
    /// expanded move to `new_id`. Returns false when `stack_member` is not in a
    /// stack, or the parent split containing {stack, Pane(new_id)} is not found,
    /// or capacity is exceeded. Caller then leaves the normal split.
    #[allow(dead_code)]
    pub fn fold_new_pane_into_focused_stack(
        &mut self,
        new_id: PaneId,
        stack_member: PaneId,
        area: Rect,
    ) -> bool {
        let panes = self.panes(area);
        let neighbors = stack_member_neighbors(&self.root, stack_member);
        let new_rect = panes.iter().find(|p| p.id == new_id).map(|p| p.rect);

        // Compute the bounding rect of all current stack members + new_id
        let stack_ids: Vec<PaneId> = std::iter::once(stack_member)
            .chain(neighbors.iter().copied())
            .collect();
        let stack_rects_union = stack_ids
            .iter()
            .filter_map(|id| panes.iter().find(|p| p.id == *id).map(|p| p.rect))
            .reduce(|acc, r| acc.union(r));

        if let (Some(sr), Some(nr)) = (stack_rects_union, new_rect) {
            let union = sr.union(nr);
            let current_members = stack_ids.len();
            let new_member_count = (current_members + 1) as u16;
            let collapsed_rows = new_member_count.saturating_sub(1);
            if union.height.saturating_sub(collapsed_rows) < MIN_STACK_EXPANDED_ROWS {
                return false;
            }
        } else {
            return false;
        }

        let placeholder = PaneId::from_raw(0);
        let old = std::mem::replace(&mut self.root, Node::Pane(placeholder));
        let (new_root, success) = fold_into_stack(old, stack_member, new_id);
        self.root = new_root;
        if success {
            self.focus = new_id;
            expand_member(&mut self.root, new_id);
        }
        success
    }

    /// Replace the minimal subtree containing all `member_ids` with a
    /// `Node::Stack`. Used by layout-apply to construct a stack from freshly
    /// split panes. On success, sets focus to `member_ids[expanded]` and
    /// reconciles the expanded index. Returns false (leaving the tree
    /// unchanged) if the panes don't form a contiguous subtree or any id is
    /// missing.
    pub fn replace_subtree_with_stack(&mut self, member_ids: &[PaneId], expanded: usize) -> bool {
        if member_ids.len() < 2 {
            return false;
        }
        let ids = self.pane_ids();
        if !member_ids.iter().all(|id| ids.contains(id)) {
            return false;
        }
        let expanded_idx = expanded.min(member_ids.len() - 1);
        let placeholder = PaneId::from_raw(0);
        let old = std::mem::replace(&mut self.root, Node::Pane(placeholder));
        let (new_root, success) = replace_subtree_as_stack(old, member_ids, expanded_idx);
        self.root = new_root;
        if success {
            self.focus = member_ids[expanded_idx];
            expand_member(&mut self.root, self.focus);
        }
        success
    }
}

/// Recursively find and replace the minimal subtree containing all `ids`
/// with a `Node::Stack { panes: ids, expanded }`. Always returns a valid
/// tree: on a non-match the original `node` is returned unchanged with
/// `false`, so a failed match never destroys the layout.
fn replace_subtree_as_stack(node: Node, ids: &[PaneId], expanded: usize) -> (Node, bool) {
    if ids.len() < 2 {
        return (node, false);
    }
    match node {
        Node::Pane(_) | Node::Stack { .. } => (node, false),
        Node::Split {
            direction,
            ratio,
            first,
            second,
        } => {
            let mut first_ids_buf = Vec::new();
            collect_ids(&first, &mut first_ids_buf);
            let first_match = first_ids_buf.iter().filter(|id| ids.contains(id)).count();
            let mut second_ids_buf = Vec::new();
            collect_ids(&second, &mut second_ids_buf);
            let second_match = second_ids_buf.iter().filter(|id| ids.contains(id)).count();

            let all_in_first = first_match == ids.len() && second_match == 0;
            let all_in_second = second_match == ids.len() && first_match == 0;
            let spans_both = first_match > 0 && second_match > 0;

            if all_in_first {
                let (new_first, success) = replace_subtree_as_stack(*first, ids, expanded);
                (
                    Node::Split {
                        direction,
                        ratio,
                        first: Box::new(new_first),
                        second,
                    },
                    success,
                )
            } else if all_in_second {
                let (new_second, success) = replace_subtree_as_stack(*second, ids, expanded);
                (
                    Node::Split {
                        direction,
                        ratio,
                        first,
                        second: Box::new(new_second),
                    },
                    success,
                )
            } else if spans_both
                && first_match == first_ids_buf.len()
                && second_match == second_ids_buf.len()
            {
                (
                    Node::Stack {
                        panes: ids.to_vec(),
                        expanded,
                    },
                    true,
                )
            } else {
                (
                    Node::Split {
                        direction,
                        ratio,
                        first,
                        second,
                    },
                    false,
                )
            }
        }
    }
}

// --- Directional pane navigation ---

/// Find the nearest pane in the given direction from `focused`.
pub fn find_in_direction(
    focused: &PaneInfo,
    direction: NavDirection,
    panes: &[PaneInfo],
) -> Option<PaneId> {
    let fr = focused.rect;

    panes
        .iter()
        .enumerate()
        .filter(|(_, p)| p.id != focused.id)
        .filter(|(_, p)| {
            let r = p.rect;
            match direction {
                NavDirection::Left => {
                    r.x + r.width <= fr.x && ranges_overlap(r.y, r.height, fr.y, fr.height)
                }
                NavDirection::Right => {
                    r.x >= fr.x + fr.width && ranges_overlap(r.y, r.height, fr.y, fr.height)
                }
                NavDirection::Up => {
                    r.y + r.height <= fr.y && ranges_overlap(r.x, r.width, fr.x, fr.width)
                }
                NavDirection::Down => {
                    r.y >= fr.y + fr.height && ranges_overlap(r.x, r.width, fr.x, fr.width)
                }
            }
        })
        .min_by_key(|(index, p)| {
            let r = p.rect;
            let edge_distance = match direction {
                NavDirection::Left => fr.x.saturating_sub(r.x + r.width),
                NavDirection::Right => r.x.saturating_sub(fr.x + fr.width),
                NavDirection::Up => fr.y.saturating_sub(r.y + r.height),
                NavDirection::Down => r.y.saturating_sub(fr.y + fr.height),
            };
            let overlap = match direction {
                NavDirection::Left | NavDirection::Right => {
                    range_overlap_amount(r.y, r.height, fr.y, fr.height)
                }
                NavDirection::Up | NavDirection::Down => {
                    range_overlap_amount(r.x, r.width, fr.x, fr.width)
                }
            };
            let center_distance = match direction {
                NavDirection::Left | NavDirection::Right => {
                    range_center_distance(r.y, r.height, fr.y, fr.height)
                }
                NavDirection::Up | NavDirection::Down => {
                    range_center_distance(r.x, r.width, fr.x, fr.width)
                }
            };
            (edge_distance, Reverse(overlap), center_distance, *index)
        })
        .map(|(_, p)| p.id)
}

fn ranges_overlap(a_start: u16, a_len: u16, b_start: u16, b_len: u16) -> bool {
    a_start < b_start + b_len && a_start + a_len > b_start
}

fn split_on_requested_edge(split: &SplitBorder, focused: Rect, nav: NavDirection) -> bool {
    split_edge_distance(split, focused, nav) <= 1
}

fn split_area_overlaps_focused_pane(split: &SplitBorder, focused: Rect, nav: NavDirection) -> bool {
    match nav {
        NavDirection::Left | NavDirection::Right => {
            ranges_overlap(split.area.y, split.area.height, focused.y, focused.height)
        }
        NavDirection::Up | NavDirection::Down => {
            ranges_overlap(split.area.x, split.area.width, focused.x, focused.width)
        }
    }
}

fn nearest_resize_split(
    splits: &[SplitBorder],
    target_dir: Direction,
    focused: Rect,
    nav: NavDirection,
) -> Option<&SplitBorder> {
    splits
        .iter()
        .filter(|s| s.direction == target_dir)
        .filter(|s| split_area_overlaps_focused_pane(s, focused, nav))
        .filter(|s| split_on_requested_edge(s, focused, nav))
        .min_by_key(|s| split_edge_distance(s, focused, nav))
}

fn opposite_direction(nav: NavDirection) -> NavDirection {
    match nav {
        NavDirection::Left => NavDirection::Right,
        NavDirection::Right => NavDirection::Left,
        NavDirection::Up => NavDirection::Down,
        NavDirection::Down => NavDirection::Up,
    }
}

fn split_edge_distance(split: &SplitBorder, focused: Rect, nav: NavDirection) -> u32 {
    match nav {
        NavDirection::Left => (split.pos as i32 - focused.x as i32).unsigned_abs(),
        NavDirection::Right => {
            (split.pos as i32 - (focused.x + focused.width) as i32).unsigned_abs()
        }
        NavDirection::Up => (split.pos as i32 - focused.y as i32).unsigned_abs(),
        NavDirection::Down => {
            (split.pos as i32 - (focused.y + focused.height) as i32).unsigned_abs()
        }
    }
}

fn range_overlap_amount(a_start: u16, a_len: u16, b_start: u16, b_len: u16) -> u16 {
    let a_end = a_start.saturating_add(a_len);
    let b_end = b_start.saturating_add(b_len);
    a_end.min(b_end).saturating_sub(a_start.max(b_start))
}

fn range_center_distance(a_start: u16, a_len: u16, b_start: u16, b_len: u16) -> u16 {
    let a_center = a_start.saturating_mul(2).saturating_add(a_len);
    let b_center = b_start.saturating_mul(2).saturating_add(b_len);
    a_center.abs_diff(b_center)
}

// --- Stack constants ---

/// Minimum height (in rows) the expanded member must retain for a stack to accept
/// another member. Mirrors Zellij's MIN_TERMINAL_HEIGHT = 5.
// Only split-into-stack folding (fold_new_pane_into_focused_stack) reads this,
// and nothing calls that yet: v0.9.0 splits through the pane.split endpoint.
#[allow(dead_code)]
pub const MIN_STACK_EXPANDED_ROWS: u16 = 5;

// --- Stack geometry ---

/// Compute rects for each member of a stack occupying `area`.
/// Non-expanded members get 1 row each; the expanded member gets the remainder.
/// Total function: saturating arithmetic, never panics on any input.
fn stack_rects(area: Rect, members: usize, expanded: usize) -> Vec<Rect> {
    if members == 0 {
        return Vec::new();
    }
    let expanded = expanded.min(members.saturating_sub(1));
    let collapsed_rows = u16::try_from(members - 1).unwrap_or(u16::MAX);
    let expanded_height = area.height.saturating_sub(collapsed_rows);

    let mut rects = Vec::with_capacity(members);
    let mut y = area.y;
    for i in 0..members {
        if i == expanded {
            let h = expanded_height.min(area.y.saturating_add(area.height).saturating_sub(y));
            rects.push(Rect::new(area.x, y, area.width, h));
            y = y.saturating_add(h);
        } else {
            let h = 1u16.min(area.y.saturating_add(area.height).saturating_sub(y));
            rects.push(Rect::new(area.x, y, area.width, h));
            y = y.saturating_add(h);
        }
    }
    rects
}

// --- Tree operations ---

fn count_panes(node: &Node) -> usize {
    match node {
        Node::Pane(_) => 1,
        Node::Split { first, second, .. } => count_panes(first) + count_panes(second),
        Node::Stack { panes, .. } => panes.len(),
    }
}

fn collect_panes(node: &Node, area: Rect, focus: PaneId, result: &mut Vec<PaneInfo>) {
    match node {
        Node::Pane(id) => {
            result.push(PaneInfo {
                id: *id,
                rect: area,
                inner_rect: area,
                scrollbar_rect: None,
                borders: Borders::NONE,
                is_focused: *id == focus,
                stack: None,
            });
        }
        Node::Split {
            direction,
            ratio,
            first,
            second,
        } => {
            let (a, b) = split_rect(area, *direction, *ratio);
            collect_panes(first, a, focus, result);
            collect_panes(second, b, focus, result);
        }
        Node::Stack { panes, expanded } => {
            let rects = stack_rects(area, panes.len(), *expanded);
            let count = panes.len();
            for (i, id) in panes.iter().enumerate() {
                let rect = rects.get(i).copied().unwrap_or(Rect::default());
                result.push(PaneInfo {
                    id: *id,
                    rect,
                    inner_rect: rect,
                    scrollbar_rect: None,
                    borders: Borders::NONE,
                    is_focused: *id == focus,
                    stack: Some(StackMember {
                        collapsed: i != *expanded,
                        position: i,
                        count,
                    }),
                });
            }
        }
    }
}

fn collect_splits(node: &Node, area: Rect, path: &mut Vec<bool>, result: &mut Vec<SplitBorder>) {
    if let Node::Split {
        direction,
        ratio,
        first,
        second,
    } = node
    {
        let (a, b) = split_rect(area, *direction, *ratio);
        let pos = match direction {
            Direction::Horizontal => a.x + a.width,
            Direction::Vertical => a.y + a.height,
        };
        result.push(SplitBorder {
            pos,
            direction: *direction,
            ratio: *ratio,
            area,
            path: path.clone(),
        });
        path.push(false);
        collect_splits(first, a, path, result);
        path.pop();
        path.push(true);
        collect_splits(second, b, path, result);
        path.pop();
    }
}

fn collect_ids(node: &Node, ids: &mut Vec<PaneId>) {
    match node {
        Node::Pane(id) => ids.push(*id),
        Node::Split { first, second, .. } => {
            collect_ids(first, ids);
            collect_ids(second, ids);
        }
        Node::Stack { panes, .. } => ids.extend(panes),
    }
}

fn split_ratios(node: &Node) -> Vec<(Vec<bool>, f32)> {
    fn collect(node: &Node, path: &mut Vec<bool>, out: &mut Vec<(Vec<bool>, f32)>) {
        match node {
            Node::Pane(_) | Node::Stack { .. } => {}
            Node::Split {
                ratio,
                first,
                second,
                ..
            } => {
                out.push((path.clone(), *ratio));
                path.push(false);
                collect(first, path, out);
                path.pop();
                path.push(true);
                collect(second, path, out);
                path.pop();
            }
        }
    }

    let mut out = Vec::new();
    collect(node, &mut Vec::new(), &mut out);
    out
}

fn swap_pane_ids(node: &mut Node, first: PaneId, second: PaneId) {
    match node {
        Node::Pane(id) if *id == first => *id = second,
        Node::Pane(id) if *id == second => *id = first,
        Node::Pane(_) => {}
        Node::Split {
            first: first_child,
            second: second_child,
            ..
        } => {
            swap_pane_ids(first_child, first, second);
            swap_pane_ids(second_child, first, second);
        }
        Node::Stack { panes, .. } => {
            for id in panes.iter_mut() {
                if *id == first {
                    *id = second;
                } else if *id == second {
                    *id = first;
                }
            }
        }
    }
}

fn find_pane_mut(node: &mut Node, target: PaneId) -> Option<&mut Node> {
    match node {
        Node::Pane(id) if *id == target => Some(node),
        Node::Pane(_) => None,
        Node::Split { first, second, .. } => {
            find_pane_mut(first, target).or_else(|| find_pane_mut(second, target))
        }
        // A stack holding the target is returned whole: splits treat it as one unit.
        Node::Stack { panes, .. } if panes.contains(&target) => Some(node),
        Node::Stack { .. } => None,
    }
}

/// Split the node `find_pane_mut` returned. A stack is wrapped as a unit;
/// `split_node` would rebuild it as the bare target pane and drop the other
/// members.
fn split_found(node: &mut Node, target: PaneId, direction: Direction, new_id: PaneId, ratio: f32) {
    *node = if matches!(node, Node::Stack { .. }) {
        let stack = std::mem::replace(node, Node::Pane(target));
        Node::Split {
            direction,
            ratio,
            first: Box::new(stack),
            second: Box::new(Node::Pane(new_id)),
        }
    } else {
        split_node(target, direction, new_id, ratio)
    };
}

fn split_node(target: PaneId, direction: Direction, new_id: PaneId, split_ratio: f32) -> Node {
    Node::Split {
        direction,
        ratio: split_ratio,
        first: Box::new(Node::Pane(target)),
        second: Box::new(Node::Pane(new_id)),
    }
}

fn valid_split_ratio(ratio: f32) -> f32 {
    if ratio.is_finite() {
        ratio.clamp(0.1, 0.9)
    } else {
        0.5
    }
}

fn remove_pane(node: Node, target: PaneId) -> Option<Node> {
    match node {
        Node::Pane(id) if id == target => None,
        Node::Pane(_) => Some(node),
        Node::Split {
            direction,
            ratio,
            first,
            second,
        } => match (remove_pane(*first, target), remove_pane(*second, target)) {
            (None, Some(s)) => Some(s),
            (Some(f), None) => Some(f),
            (Some(f), Some(s)) => Some(Node::Split {
                direction,
                ratio,
                first: Box::new(f),
                second: Box::new(s),
            }),
            (None, None) => None,
        },
        Node::Stack {
            mut panes,
            expanded,
        } => {
            let Some(pos) = panes.iter().position(|id| *id == target) else {
                return Some(Node::Stack { panes, expanded });
            };
            panes.remove(pos);
            if panes.is_empty() {
                return None;
            }
            if panes.len() == 1 {
                return Some(Node::Pane(panes[0]));
            }
            let new_expanded = if pos < panes.len() {
                pos
            } else {
                panes.len() - 1
            };
            Some(Node::Stack {
                panes,
                expanded: new_expanded,
            })
        }
    }
}

fn set_ratio_at(node: &mut Node, path: &[bool], new_ratio: f32) -> bool {
    if let Node::Split {
        ratio,
        first,
        second,
        ..
    } = node
    {
        if path.is_empty() {
            *ratio = new_ratio;
            true
        } else if path[0] {
            set_ratio_at(second, &path[1..], new_ratio)
        } else {
            set_ratio_at(first, &path[1..], new_ratio)
        }
    } else {
        false
    }
}

fn get_ratio_at(node: &Node, path: &[bool]) -> Option<f32> {
    if let Node::Split {
        ratio,
        first,
        second,
        ..
    } = node
    {
        if path.is_empty() {
            Some(*ratio)
        } else if path[0] {
            get_ratio_at(second, &path[1..])
        } else {
            get_ratio_at(first, &path[1..])
        }
    } else {
        None
    }
}

fn split_rect(area: Rect, direction: Direction, ratio: f32) -> (Rect, Rect) {
    match direction {
        Direction::Horizontal => {
            let first_w = ((area.width as f32) * ratio).round() as u16;
            let second_w = area.width.saturating_sub(first_w);
            (
                Rect::new(area.x, area.y, first_w, area.height),
                Rect::new(area.x + first_w, area.y, second_w, area.height),
            )
        }
        Direction::Vertical => {
            let first_h = ((area.height as f32) * ratio).round() as u16;
            let second_h = area.height.saturating_sub(first_h);
            (
                Rect::new(area.x, area.y, area.width, first_h),
                Rect::new(area.x, area.y + first_h, area.width, second_h),
            )
        }
    }
}

// --- Stack helpers ---

/// Walk the tree and set the `expanded` index of any `Stack` containing `id`
/// to that member's position. Logs when `expanded` actually changes.
fn expand_member(node: &mut Node, id: PaneId) {
    match node {
        Node::Pane(_) => {}
        Node::Split { first, second, .. } => {
            expand_member(first, id);
            expand_member(second, id);
        }
        Node::Stack { panes, expanded } => {
            if let Some(pos) = panes.iter().position(|p| *p == id) {
                if *expanded != pos {
                    debug!(
                        pane = id.raw(),
                        prev_expanded = *expanded,
                        new_expanded = pos,
                        stack_size = panes.len(),
                        "expand_member: expanded index changed"
                    );
                    *expanded = pos;
                }
            }
        }
    }
}

/// Returns the other members of the stack that contains `id`.
fn stack_member_neighbors(node: &Node, id: PaneId) -> Vec<PaneId> {
    match node {
        Node::Pane(_) => Vec::new(),
        Node::Split { first, second, .. } => {
            let r = stack_member_neighbors(first, id);
            if !r.is_empty() {
                return r;
            }
            stack_member_neighbors(second, id)
        }
        Node::Stack { panes, .. } => {
            if panes.contains(&id) {
                panes.iter().copied().filter(|p| *p != id).collect()
            } else {
                Vec::new()
            }
        }
    }
}

/// Returns true if `id` lives inside a `Node::Stack` somewhere in the tree.
fn collapsed_in_stack(node: &Node, id: PaneId) -> bool {
    match node {
        Node::Pane(_) => false,
        Node::Split { first, second, .. } => {
            collapsed_in_stack(first, id) || collapsed_in_stack(second, id)
        }
        Node::Stack { panes, expanded } => panes.contains(&id) && panes.get(*expanded) != Some(&id),
    }
}

fn contains_in_stack(node: &Node, id: PaneId) -> bool {
    match node {
        Node::Pane(_) => false,
        Node::Split { first, second, .. } => {
            contains_in_stack(first, id) || contains_in_stack(second, id)
        }
        Node::Stack { panes, .. } => panes.contains(&id),
    }
}

/// Find the promoted pane id after a stack member was removed. Searches for:
/// 1. A Stack node containing one of the `neighbors` → return its expanded member
/// 2. A Pane node matching a neighbor (stack collapsed to single pane) → return it
fn find_promoted_after_close(node: &Node, neighbors: &[PaneId]) -> Option<PaneId> {
    match node {
        Node::Pane(id) => {
            if neighbors.contains(id) {
                Some(*id)
            } else {
                None
            }
        }
        Node::Split { first, second, .. } => find_promoted_after_close(first, neighbors)
            .or_else(|| find_promoted_after_close(second, neighbors)),
        Node::Stack { panes, expanded } => {
            if neighbors.iter().any(|n| panes.contains(n)) {
                panes.get(*expanded).copied()
            } else {
                None
            }
        }
    }
}

// --- Stack operations ---

/// Returns true if `id` is a direct leaf of this node (Pane match) or a
/// direct member of this node (Stack containing id). Does not recurse into
/// Split children.
fn node_directly_contains(node: &Node, id: PaneId) -> bool {
    match node {
        Node::Pane(p) => *p == id,
        Node::Stack { panes, .. } => panes.contains(&id),
        Node::Split { .. } => false,
    }
}

/// Merge the focused pane with its sibling into a stack. Returns the new tree
/// and whether the operation succeeded.
fn stack_at_focus(node: Node, focus: PaneId) -> (Node, bool) {
    match node {
        Node::Pane(_) => (node, false),
        Node::Stack { .. } => (node, false),
        Node::Split {
            direction,
            ratio,
            first,
            second,
        } => {
            let first_has_focus = node_directly_contains(&first, focus);
            let second_has_focus = node_directly_contains(&second, focus);

            if first_has_focus {
                match (*first, *second) {
                    (Node::Pane(a), Node::Pane(b)) => {
                        debug!(
                            focused = focus.raw(),
                            stack_size = 2,
                            "stack_focused: created 2-member stack"
                        );
                        (
                            Node::Stack {
                                panes: vec![a, b],
                                expanded: 0,
                            },
                            true,
                        )
                    }
                    (Node::Pane(a), Node::Stack { mut panes, .. }) => {
                        panes.insert(0, a);
                        debug!(
                            focused = focus.raw(),
                            stack_size = panes.len(),
                            "stack_focused: joined pane into adjacent stack"
                        );
                        (Node::Stack { panes, expanded: 0 }, true)
                    }
                    (first_node, second_node) => (
                        Node::Split {
                            direction,
                            ratio,
                            first: Box::new(first_node),
                            second: Box::new(second_node),
                        },
                        false,
                    ),
                }
            } else if second_has_focus {
                match (*first, *second) {
                    (Node::Pane(a), Node::Pane(b)) => {
                        debug!(
                            focused = focus.raw(),
                            stack_size = 2,
                            "stack_focused: created 2-member stack"
                        );
                        (
                            Node::Stack {
                                panes: vec![a, b],
                                expanded: 1,
                            },
                            true,
                        )
                    }
                    (Node::Stack { mut panes, .. }, Node::Pane(b)) => {
                        let expanded = panes.len();
                        panes.push(b);
                        debug!(
                            focused = focus.raw(),
                            stack_size = panes.len(),
                            "stack_focused: joined pane into adjacent stack"
                        );
                        (Node::Stack { panes, expanded }, true)
                    }
                    (first_node, second_node) => (
                        Node::Split {
                            direction,
                            ratio,
                            first: Box::new(first_node),
                            second: Box::new(second_node),
                        },
                        false,
                    ),
                }
            } else {
                let (new_first, first_ok) = stack_at_focus(*first, focus);
                if first_ok {
                    return (
                        Node::Split {
                            direction,
                            ratio,
                            first: Box::new(new_first),
                            second,
                        },
                        true,
                    );
                }
                let (new_second, second_ok) = stack_at_focus(*second, focus);
                (
                    Node::Split {
                        direction,
                        ratio,
                        first: Box::new(new_first),
                        second: Box::new(new_second),
                    },
                    second_ok,
                )
            }
        }
    }
}

/// Remove the focused member from its stack and wrap the residual + unstacked
/// pane in a new Split at the stack's tree position. Returns (new_tree, success).
fn unstack_at_focus(
    node: Node,
    focus: PaneId,
    split_direction: Direction,
    split_ratio: f32,
) -> (Node, bool) {
    match node {
        Node::Pane(_) => (node, false),
        Node::Stack {
            mut panes,
            expanded,
        } => {
            let Some(pos) = panes.iter().position(|p| *p == focus) else {
                return (Node::Stack { panes, expanded }, false);
            };
            panes.remove(pos);
            let residual = if panes.len() == 1 {
                Node::Pane(panes[0])
            } else {
                let new_expanded = if pos < panes.len() {
                    pos
                } else {
                    panes.len() - 1
                };
                Node::Stack {
                    panes,
                    expanded: new_expanded,
                }
            };
            debug!(
                focused = focus.raw(),
                "unstack_focused: removed member from stack"
            );
            (
                Node::Split {
                    direction: split_direction,
                    ratio: split_ratio,
                    first: Box::new(residual),
                    second: Box::new(Node::Pane(focus)),
                },
                true,
            )
        }
        Node::Split {
            direction,
            ratio,
            first,
            second,
        } => {
            let (new_first, first_ok) =
                unstack_at_focus(*first, focus, split_direction, split_ratio);
            if first_ok {
                return (
                    Node::Split {
                        direction,
                        ratio,
                        first: Box::new(new_first),
                        second,
                    },
                    true,
                );
            }
            let (new_second, second_ok) =
                unstack_at_focus(*second, focus, split_direction, split_ratio);
            (
                Node::Split {
                    direction,
                    ratio,
                    first: Box::new(new_first),
                    second: Box::new(new_second),
                },
                second_ok,
            )
        }
    }
}

/// Find the Split whose children are {node containing focus} and {Pane(new_id)},
/// then merge new_id into the stack (or create a new 2-member stack). Returns
/// (new_tree, success).
#[allow(dead_code)]
fn fold_into_stack(node: Node, focus: PaneId, new_id: PaneId) -> (Node, bool) {
    match node {
        Node::Pane(_) | Node::Stack { .. } => (node, false),
        Node::Split {
            direction,
            ratio,
            first,
            second,
        } => {
            // Check if this split has {focus-container} + {Pane(new_id)} as direct children
            let first_has_focus = node_directly_contains(&first, focus);
            let second_is_new = matches!(&*second, Node::Pane(id) if *id == new_id);
            let second_has_focus = node_directly_contains(&second, focus);
            let first_is_new = matches!(&*first, Node::Pane(id) if *id == new_id);

            if first_has_focus && second_is_new {
                match *first {
                    Node::Pane(f) => {
                        debug!(
                            new_pane = new_id.raw(),
                            stack_size = 2,
                            "fold_new_pane: created 2-member stack"
                        );
                        (
                            Node::Stack {
                                panes: vec![f, new_id],
                                expanded: 1,
                            },
                            true,
                        )
                    }
                    Node::Stack { mut panes, .. } => {
                        let expanded = panes.len();
                        panes.push(new_id);
                        debug!(
                            new_pane = new_id.raw(),
                            stack_size = panes.len(),
                            "fold_new_pane: added to existing stack"
                        );
                        (Node::Stack { panes, expanded }, true)
                    }
                    _ => unreachable!(),
                }
            } else if second_has_focus && first_is_new {
                match *second {
                    Node::Pane(f) => {
                        debug!(
                            new_pane = new_id.raw(),
                            stack_size = 2,
                            "fold_new_pane: created 2-member stack"
                        );
                        (
                            Node::Stack {
                                panes: vec![new_id, f],
                                expanded: 0,
                            },
                            true,
                        )
                    }
                    Node::Stack { mut panes, .. } => {
                        panes.insert(0, new_id);
                        debug!(
                            new_pane = new_id.raw(),
                            stack_size = panes.len(),
                            "fold_new_pane: added to existing stack"
                        );
                        (Node::Stack { panes, expanded: 0 }, true)
                    }
                    _ => unreachable!(),
                }
            } else {
                // Recurse
                let (new_first, first_ok) = fold_into_stack(*first, focus, new_id);
                if first_ok {
                    return (
                        Node::Split {
                            direction,
                            ratio,
                            first: Box::new(new_first),
                            second,
                        },
                        true,
                    );
                }
                let (new_second, second_ok) = fold_into_stack(*second, focus, new_id);
                (
                    Node::Split {
                        direction,
                        ratio,
                        first: Box::new(new_first),
                        second: Box::new(new_second),
                    },
                    second_ok,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_paths_preserve_preorder_geometry_and_resize_targets() {
        let ids = [
            PaneId::alloc(),
            PaneId::alloc(),
            PaneId::alloc(),
            PaneId::alloc(),
        ];
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Horizontal,
                ratio: 0.5,
                first: Box::new(split_node(ids[0], Direction::Vertical, ids[1], 0.5)),
                second: Box::new(split_node(ids[2], Direction::Vertical, ids[3], 0.25)),
            },
            ids[0],
        );
        let area = Rect::new(3, 7, 120, 80);
        let splits = layout.splits(area);
        assert_eq!(splits.len(), 3);
        for (split, (path, direction, pos, rect)) in splits.iter().zip([
            (vec![], Direction::Horizontal, 63, area),
            (
                vec![false],
                Direction::Vertical,
                47,
                Rect::new(3, 7, 60, 80),
            ),
            (
                vec![true],
                Direction::Vertical,
                27,
                Rect::new(63, 7, 60, 80),
            ),
        ]) {
            assert_eq!(
                (split.path.clone(), split.direction, split.pos, split.area),
                (path, direction, pos, rect)
            );
        }
        assert!(layout.set_ratio_at(&splits[2].path, 0.5));
        let resized = layout.splits(area);
        assert_eq!(resized[1].pos, splits[1].pos);
        assert_eq!(resized[2].pos, 47);
    }

    #[test]
    fn rejected_splits_and_insertions_preserve_layout_and_focus_history() {
        let (mut layout, root) = TileLayout::new();
        let second = layout.split_focused(Direction::Horizontal);
        let absent = PaneId::alloc();
        let before = pane_rects(&layout);
        let splits = split_snapshot(&layout);
        assert!(layout
            .split_pane(absent, Direction::Vertical, 0.5)
            .is_none());
        for (target, moved) in [(absent, PaneId::alloc()), (root, second), (root, root)] {
            assert!(!layout.insert_pane_near(target, moved, Direction::Vertical, 0.3, true));
        }
        assert_eq!(pane_rects(&layout), before);
        assert_eq!(split_snapshot(&layout), splits);
        assert_eq!(layout.focused(), second);
        assert_eq!(layout.prev_focus, Some(root));
        assert!(layout.close_focused());
        assert_eq!(layout.focused(), root);
    }

    #[test]
    fn splitting_a_deep_leaf_preserves_other_branches_and_focus() {
        let (mut layout, root) = TileLayout::new();
        let right = layout.split_pane(root, Direction::Horizontal, 0.6).unwrap();
        let bottom_left = layout.split_pane(root, Direction::Vertical, 0.4).unwrap();
        let right_rect = pane_rect(&layout, right);
        let focus = layout.focused();
        let new = layout
            .split_pane(bottom_left, Direction::Horizontal, f32::NAN)
            .unwrap();
        assert_eq!(layout.pane_ids(), [root, bottom_left, new, right]);
        assert_eq!(pane_rect(&layout, right), right_rect);
        assert_eq!(layout.focused(), focus);
        assert_eq!(
            split_snapshot(&layout),
            [
                (Direction::Horizontal, 0.6),
                (Direction::Vertical, 0.4),
                (Direction::Horizontal, 0.5)
            ]
        );
    }

    #[test]
    #[ignore = "manual BSP allocation and traversal scaling profile"]
    fn bsp_layout_profile() {
        use std::hint::black_box;
        use std::time::{Duration, Instant};
        fn build(count: usize, balanced: bool) -> TileLayout {
            let (mut layout, root) = TileLayout::new();
            let mut leaves = std::collections::VecDeque::from([root]);
            for _ in 1..count {
                let target = leaves.pop_front().unwrap();
                let new = layout
                    .split_pane(target, Direction::Horizontal, 0.5)
                    .unwrap();
                if balanced {
                    leaves.push_back(target);
                }
                leaves.push_back(new);
            }
            layout
        }
        fn measure(mut operation: impl FnMut()) -> f64 {
            for _ in 0..32 {
                operation();
            }
            let mut samples = Vec::new();
            for _ in 0..7 {
                let start = Instant::now();
                let mut iterations = 0;
                while start.elapsed() < Duration::from_millis(20) {
                    operation();
                    iterations += 1;
                }
                samples.push(start.elapsed().as_secs_f64() * 1e6 / f64::from(iterations));
            }
            samples.sort_by(f64::total_cmp);
            samples[3]
        }
        for count in [1, 15, 128, 512] {
            for balanced in [true, false] {
                let layout = build(count, balanced);
                let splits = measure(|| {
                    black_box(layout.splits(Rect::new(0, 0, 120, 40)));
                });
                let build = measure(|| {
                    black_box(build(count, balanced));
                });
                println!("bsp panes={count} balanced={balanced} splits_us={splits:.3} build_us={build:.3}");
            }
        }
    }

    fn pane(id: u32) -> PaneId {
        PaneId::from_raw(id)
    }

    fn sample_layout() -> TileLayout {
        TileLayout::from_saved(
            Node::Split {
                direction: Direction::Horizontal,
                ratio: 0.3,
                first: Box::new(Node::Pane(pane(1))),
                second: Box::new(Node::Split {
                    direction: Direction::Vertical,
                    ratio: 0.6,
                    first: Box::new(Node::Pane(pane(2))),
                    second: Box::new(Node::Split {
                        direction: Direction::Horizontal,
                        ratio: 0.4,
                        first: Box::new(Node::Pane(pane(3))),
                        second: Box::new(Node::Pane(pane(4))),
                    }),
                }),
            },
            pane(2),
        )
    }

    fn pane_rects(layout: &TileLayout) -> Vec<(PaneId, Rect)> {
        layout
            .panes(Rect::new(0, 0, 100, 40))
            .into_iter()
            .map(|info| (info.id, info.rect))
            .collect()
    }

    fn pane_rect(layout: &TileLayout, pane_id: PaneId) -> Rect {
        pane_rects(layout)
            .into_iter()
            .find_map(|(id, rect)| (id == pane_id).then_some(rect))
            .expect("pane should exist")
    }

    fn split_snapshot(layout: &TileLayout) -> Vec<(Direction, f32)> {
        fn collect(node: &Node, out: &mut Vec<(Direction, f32)>) {
            match node {
                Node::Pane(_) | Node::Stack { .. } => {}
                Node::Split {
                    direction,
                    ratio,
                    first,
                    second,
                } => {
                    out.push((*direction, *ratio));
                    collect(first, out);
                    collect(second, out);
                }
            }
        }

        let mut out = Vec::new();
        collect(layout.root(), &mut out);
        out
    }

    #[test]
    fn swap_panes_exchanges_leaf_ids_without_changing_cells() {
        let mut layout = sample_layout();
        let before_rects = pane_rects(&layout);
        let before_splits = split_snapshot(&layout);

        assert!(layout.swap_panes(pane(2), pane(4)));

        assert_eq!(layout.pane_count(), 4);
        assert_eq!(split_snapshot(&layout), before_splits);
        assert_eq!(layout.focused(), pane(2));

        let after_rects = pane_rects(&layout);
        assert_eq!(after_rects[0], before_rects[0]);
        assert_eq!(after_rects[1], (pane(4), before_rects[1].1));
        assert_eq!(after_rects[2], before_rects[2]);
        assert_eq!(after_rects[3], (pane(2), before_rects[3].1));
    }

    #[test]
    fn swap_panes_is_noop_for_same_or_missing_pane() {
        let mut layout = sample_layout();
        let before_rects = pane_rects(&layout);
        let before_splits = split_snapshot(&layout);
        let before_focus = layout.focused();

        assert!(!layout.swap_panes(pane(2), pane(2)));
        assert!(!layout.swap_panes(pane(2), pane(99)));
        assert!(!layout.swap_panes(pane(99), pane(2)));

        assert_eq!(pane_rects(&layout), before_rects);
        assert_eq!(split_snapshot(&layout), before_splits);
        assert_eq!(layout.focused(), before_focus);
    }

    #[test]
    fn insert_existing_pane_near_target_preserves_existing_ids_and_focuses_moved_pane() {
        let (mut layout, root) = TileLayout::new();
        let moved = pane(99);

        assert!(layout.insert_pane_near(root, moved, Direction::Horizontal, 0.25, true));

        assert_eq!(layout.pane_count(), 2);
        assert_eq!(layout.pane_ids(), vec![root, moved]);
        assert_eq!(layout.focused(), moved);
        let splits = split_snapshot(&layout);
        assert_eq!(splits, vec![(Direction::Horizontal, 0.25)]);
        assert_eq!(pane_rect(&layout, root), Rect::new(0, 0, 25, 40));
        assert_eq!(pane_rect(&layout, moved), Rect::new(25, 0, 75, 40));
    }

    #[test]
    fn split_focused_with_ratio_sets_new_split_ratio() {
        let (mut layout, root) = TileLayout::new();
        layout.focus_pane(root);

        layout.split_focused_with_ratio(Direction::Horizontal, 0.333);

        let splits = split_snapshot(&layout);
        assert_eq!(splits.len(), 1);
        assert_eq!(splits[0].0, Direction::Horizontal);
        assert!((splits[0].1 - 0.333).abs() < f32::EPSILON);
    }

    #[test]
    fn resize_pane_preserves_focus_and_reports_change() {
        let mut layout = sample_layout();
        let original_focus = layout.focused();

        assert!(layout.resize_pane(pane(1), NavDirection::Right, 0.05, Rect::new(0, 0, 100, 40),));

        assert_eq!(layout.focused(), original_focus);
        let split = split_snapshot(&layout)[0];
        assert_eq!(split.0, Direction::Horizontal);
        assert!((split.1 - 0.35).abs() < f32::EPSILON);
    }

    #[test]
    fn resize_second_child_toward_split_decreases_ratio() {
        let (mut layout, root) = TileLayout::new();
        let right = layout.split_focused(Direction::Horizontal);
        layout.focus_pane(root);

        assert!(layout.resize_pane(right, NavDirection::Left, 0.05, Rect::new(0, 0, 100, 40),));

        let split = split_snapshot(&layout)[0];
        assert_eq!(split.0, Direction::Horizontal);
        assert!((split.1 - 0.45).abs() < f32::EPSILON);
        assert_eq!(layout.focused(), root);
    }

    #[test]
    fn resize_outer_edges_shrink_focused_pane() {
        let (mut horizontal, left) = TileLayout::new();
        horizontal.split_focused(Direction::Horizontal);

        assert!(horizontal.resize_pane(left, NavDirection::Left, 0.05, Rect::new(0, 0, 100, 40),));
        let split = split_snapshot(&horizontal)[0];
        assert_eq!(split.0, Direction::Horizontal);
        assert!((split.1 - 0.45).abs() < f32::EPSILON);

        let (mut horizontal, _left) = TileLayout::new();
        let right = horizontal.split_focused(Direction::Horizontal);

        assert!(horizontal.resize_pane(right, NavDirection::Right, 0.05, Rect::new(0, 0, 100, 40),));
        let split = split_snapshot(&horizontal)[0];
        assert_eq!(split.0, Direction::Horizontal);
        assert!((split.1 - 0.55).abs() < f32::EPSILON);

        let (mut vertical, top) = TileLayout::new();
        vertical.split_focused(Direction::Vertical);

        assert!(vertical.resize_pane(top, NavDirection::Up, 0.05, Rect::new(0, 0, 100, 40),));
        let split = split_snapshot(&vertical)[0];
        assert_eq!(split.0, Direction::Vertical);
        assert!((split.1 - 0.45).abs() < f32::EPSILON);

        let (mut vertical, _top) = TileLayout::new();
        let bottom = vertical.split_focused(Direction::Vertical);

        assert!(vertical.resize_pane(bottom, NavDirection::Down, 0.05, Rect::new(0, 0, 100, 40),));
        let split = split_snapshot(&vertical)[0];
        assert_eq!(split.0, Direction::Vertical);
        assert!((split.1 - 0.55).abs() < f32::EPSILON);
    }

    #[test]
    fn resize_outer_edge_falls_back_to_horizontal_ancestor_split() {
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Horizontal,
                ratio: 0.6,
                first: Box::new(Node::Split {
                    direction: Direction::Vertical,
                    ratio: 0.5,
                    first: Box::new(Node::Pane(pane(1))),
                    second: Box::new(Node::Pane(pane(2))),
                }),
                second: Box::new(Node::Pane(pane(3))),
            },
            pane(1),
        );
        let before = pane_rect(&layout, pane(1));

        assert!(layout.resize_pane(pane(1), NavDirection::Left, 0.05, Rect::new(0, 0, 100, 40),));

        let after = pane_rect(&layout, pane(1));
        assert_eq!(after.height, before.height);
        assert!(after.width < before.width);
        let splits = split_snapshot(&layout);
        assert_eq!(splits[0].0, Direction::Horizontal);
        assert!((splits[0].1 - 0.55).abs() < f32::EPSILON);
        assert_eq!(splits[1], (Direction::Vertical, 0.5));
    }

    #[test]
    fn resize_outer_edge_falls_back_to_vertical_ancestor_split() {
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.6,
                first: Box::new(Node::Split {
                    direction: Direction::Horizontal,
                    ratio: 0.5,
                    first: Box::new(Node::Pane(pane(1))),
                    second: Box::new(Node::Pane(pane(2))),
                }),
                second: Box::new(Node::Pane(pane(3))),
            },
            pane(1),
        );
        let before = pane_rect(&layout, pane(1));

        assert!(layout.resize_pane(pane(1), NavDirection::Up, 0.05, Rect::new(0, 0, 100, 40),));

        let after = pane_rect(&layout, pane(1));
        assert_eq!(after.width, before.width);
        assert!(after.height < before.height);
        let splits = split_snapshot(&layout);
        assert_eq!(splits[0].0, Direction::Vertical);
        assert!((splits[0].1 - 0.55).abs() < f32::EPSILON);
        assert_eq!(splits[1], (Direction::Horizontal, 0.5));
    }

    #[test]
    fn resize_uses_split_in_same_branch_when_borders_share_coordinate() {
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(Node::Split {
                    direction: Direction::Horizontal,
                    ratio: 0.5,
                    first: Box::new(Node::Pane(pane(1))),
                    second: Box::new(Node::Pane(pane(2))),
                }),
                second: Box::new(Node::Split {
                    direction: Direction::Horizontal,
                    ratio: 0.5,
                    first: Box::new(Node::Pane(pane(3))),
                    second: Box::new(Node::Pane(pane(4))),
                }),
            },
            pane(3),
        );

        assert!(layout.resize_pane(pane(3), NavDirection::Right, 0.05, Rect::new(0, 0, 100, 40),));

        let splits = split_snapshot(&layout);
        assert_eq!(splits[0], (Direction::Vertical, 0.5));
        assert_eq!(splits[1], (Direction::Horizontal, 0.5));
        assert_eq!(splits[2].0, Direction::Horizontal);
        assert!((splits[2].1 - 0.55).abs() < f32::EPSILON);
    }

    #[test]
    fn find_in_direction_tiebreaks_by_larger_overlap_before_layout_order() {
        let focused = PaneInfo {
            id: pane(1),
            rect: Rect::new(10, 10, 10, 10),
            inner_rect: Rect::new(10, 10, 10, 10),
            scrollbar_rect: None,
            borders: Borders::NONE,
            is_focused: true,
            stack: None,
        };
        let small_overlap_first = PaneInfo {
            id: pane(2),
            rect: Rect::new(0, 10, 10, 2),
            inner_rect: Rect::new(0, 10, 10, 2),
            scrollbar_rect: None,
            borders: Borders::NONE,
            is_focused: false,
            stack: None,
        };
        let larger_overlap_second = PaneInfo {
            id: pane(3),
            rect: Rect::new(0, 10, 10, 8),
            inner_rect: Rect::new(0, 10, 10, 8),
            scrollbar_rect: None,
            borders: Borders::NONE,
            is_focused: false,
            stack: None,
        };
        let panes = vec![focused.clone(), small_overlap_first, larger_overlap_second];

        assert_eq!(
            find_in_direction(&focused, NavDirection::Left, &panes),
            Some(pane(3))
        );
    }

    #[test]
    fn close_focused_returns_to_the_pane_focus_came_from() {
        let mut layout = sample_layout();
        layout.focus_pane(pane(4));

        assert!(layout.close_focused());

        assert_eq!(layout.focused(), pane(2));
    }

    #[test]
    fn close_focused_returns_to_the_pane_that_opened_a_split() {
        // Allocated ids only: sample_layout() uses from_raw and shares the id
        // space with the allocator.
        let (mut layout, first) = TileLayout::new();
        let second = layout.split_focused(Direction::Horizontal);
        let third = layout.split_focused(Direction::Vertical);
        assert_eq!(layout.pane_ids().len(), 3);

        layout.focus_pane(first);
        let opened = layout.split_focused(Direction::Horizontal);
        assert_eq!(layout.focused(), opened);

        assert!(layout.close_focused());

        assert_eq!(layout.focused(), first);
        assert!(layout.pane_ids().contains(&second));
        assert!(layout.pane_ids().contains(&third));
    }

    #[test]
    fn closing_a_background_pane_keeps_the_focused_pane_history() {
        let mut layout = sample_layout();
        layout.focus_pane(pane(4));

        assert!(layout.close_pane(pane(1)));
        assert_eq!(layout.focused(), pane(4));

        assert!(layout.close_focused());
        assert_eq!(layout.focused(), pane(2));
    }

    #[test]
    fn closing_the_remembered_pane_drops_the_focus_history() {
        let mut layout = sample_layout();
        layout.focus_pane(pane(4));

        assert!(layout.close_pane(pane(2)));

        assert!(layout.close_focused());
        assert_eq!(layout.focused(), pane(3));
    }

    #[test]
    fn close_focused_uses_tree_order_without_focus_history() {
        let mut layout = sample_layout();

        assert!(layout.close_focused());

        assert_eq!(layout.focused(), pane(3));
    }

    #[test]
    fn close_focused_does_not_reuse_history_after_it_is_consumed() {
        let mut layout = sample_layout();
        layout.focus_pane(pane(4));

        assert!(layout.close_focused());
        assert_eq!(layout.focused(), pane(2));

        assert!(layout.close_focused());
        assert_eq!(layout.focused(), pane(3));
    }

    #[test]
    fn resize_does_not_disturb_the_close_focus_target() {
        let mut layout = sample_layout();
        layout.focus_pane(pane(4));
        layout.resize_pane(pane(1), NavDirection::Right, 0.05, Rect::new(0, 0, 100, 40));

        assert!(layout.close_focused());

        assert_eq!(layout.focused(), pane(2));
    }

    #[test]
    fn split_pane_leaves_focus_and_history_untouched() {
        let mut layout = sample_layout();
        layout.focus_pane(pane(4));

        let new_id = layout
            .split_pane(pane(1), Direction::Horizontal, 0.5)
            .expect("target exists");

        assert!(layout.pane_ids().contains(&new_id));
        assert_eq!(layout.focused(), pane(4));
        assert!(layout.close_focused());
        assert_eq!(layout.focused(), pane(2));
    }

    #[test]
    fn split_pane_missing_target_changes_nothing() {
        let mut layout = sample_layout();
        let ids = layout.pane_ids();

        assert_eq!(
            layout.split_pane(pane(99), Direction::Horizontal, 0.5),
            None
        );

        assert_eq!(layout.pane_ids(), ids);
    }

    #[test]
    fn insert_pane_near_unfocused_keeps_focus_and_history() {
        let mut layout = sample_layout();
        layout.focus_pane(pane(4));

        assert!(layout.insert_pane_near(pane(1), pane(9), Direction::Horizontal, 0.5, false));

        assert_eq!(layout.focused(), pane(4));
        assert!(layout.close_focused());
        assert_eq!(layout.focused(), pane(2));
    }

    #[test]
    fn failed_split_rollback_preserves_focus_history() {
        let mut layout = sample_layout();
        layout.focus_pane(pane(4));

        let new_id = layout
            .split_pane(layout.focused(), Direction::Horizontal, 0.5)
            .expect("target exists");
        assert!(layout.close_pane(new_id));

        assert_eq!(layout.focused(), pane(4));
        assert!(layout.close_focused());
        assert_eq!(layout.focused(), pane(2));
    }

    // --- Characterization tests: lock pre-stack split-layout geometry ---

    #[test]
    fn characterization_sample_layout_pane_rects() {
        let layout = sample_layout();
        let rects = pane_rects(&layout);
        assert_eq!(rects.len(), 4);
        assert_eq!(rects[0], (pane(1), Rect::new(0, 0, 30, 40)));
        assert_eq!(rects[1], (pane(2), Rect::new(30, 0, 70, 24)));
        assert_eq!(rects[2], (pane(3), Rect::new(30, 24, 28, 16)));
        assert_eq!(rects[3], (pane(4), Rect::new(58, 24, 42, 16)));
    }

    #[test]
    fn characterization_sample_layout_navigation() {
        let layout = sample_layout();
        let panes = layout.panes(Rect::new(0, 0, 100, 40));
        let focused = panes.iter().find(|p| p.id == pane(2)).unwrap();

        assert_eq!(
            find_in_direction(focused, NavDirection::Left, &panes),
            Some(pane(1))
        );
        // pane(4) has more horizontal overlap with pane(2) than pane(3)
        assert_eq!(
            find_in_direction(focused, NavDirection::Down, &panes),
            Some(pane(4))
        );
        assert_eq!(find_in_direction(focused, NavDirection::Up, &panes), None);

        let p3 = panes.iter().find(|p| p.id == pane(3)).unwrap();
        assert_eq!(
            find_in_direction(p3, NavDirection::Right, &panes),
            Some(pane(4))
        );
        assert_eq!(
            find_in_direction(p3, NavDirection::Up, &panes),
            Some(pane(2))
        );
        assert_eq!(
            find_in_direction(p3, NavDirection::Left, &panes),
            Some(pane(1))
        );
    }

    #[test]
    fn characterization_vertical_split_rects() {
        let layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane(1))),
                second: Box::new(Node::Pane(pane(2))),
            },
            pane(1),
        );
        let rects = pane_rects(&layout);
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0], (pane(1), Rect::new(0, 0, 100, 20)));
        assert_eq!(rects[1], (pane(2), Rect::new(0, 20, 100, 20)));
    }

    #[test]
    fn characterization_horizontal_split_rects() {
        let layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Horizontal,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane(1))),
                second: Box::new(Node::Pane(pane(2))),
            },
            pane(1),
        );
        let rects = pane_rects(&layout);
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0], (pane(1), Rect::new(0, 0, 50, 40)));
        assert_eq!(rects[1], (pane(2), Rect::new(50, 0, 50, 40)));
    }

    // --- stack_rects unit tests ---

    #[test]
    fn stack_rects_four_members_expanded_at_each_index() {
        let area = Rect::new(0, 0, 80, 24);
        for exp in 0..4 {
            let rects = stack_rects(area, 4, exp);
            assert_eq!(rects.len(), 4);
            let mut y = 0u16;
            for (i, r) in rects.iter().enumerate() {
                assert_eq!(r.x, 0);
                assert_eq!(r.width, 80);
                assert_eq!(r.y, y);
                if i == exp {
                    assert_eq!(r.height, 21); // 24 - 3 collapsed rows
                } else {
                    assert_eq!(r.height, 1);
                }
                y += r.height;
            }
            assert_eq!(y, 24);
        }
    }

    #[test]
    fn stack_rects_degenerate_area_smaller_than_members() {
        let area = Rect::new(5, 10, 80, 3);
        let rects = stack_rects(area, 5, 2);
        assert_eq!(rects.len(), 5);
        for r in &rects {
            assert_eq!(r.x, 5);
            assert_eq!(r.width, 80);
            assert!(r.y >= 10);
            assert!(r.y + r.height <= 13);
        }
    }

    #[test]
    fn stack_rects_zero_members() {
        let rects = stack_rects(Rect::new(0, 0, 80, 24), 0, 0);
        assert!(rects.is_empty());
    }

    #[test]
    fn stack_rects_expanded_out_of_range_clamped() {
        let rects = stack_rects(Rect::new(0, 0, 80, 24), 3, 99);
        assert_eq!(rects.len(), 3);
        assert_eq!(rects[2].height, 22); // expanded at clamped index 2
        assert_eq!(rects[0].height, 1);
        assert_eq!(rects[1].height, 1);
    }

    #[test]
    fn stack_rects_two_members() {
        let area = Rect::new(0, 0, 80, 10);
        let rects = stack_rects(area, 2, 0);
        assert_eq!(rects[0], Rect::new(0, 0, 80, 9));
        assert_eq!(rects[1], Rect::new(0, 9, 80, 1));

        let rects = stack_rects(area, 2, 1);
        assert_eq!(rects[0], Rect::new(0, 0, 80, 1));
        assert_eq!(rects[1], Rect::new(0, 1, 80, 9));
    }

    // --- collect_panes Stack tests ---

    #[test]
    fn collect_panes_stack_emits_correct_pane_infos() {
        let layout = TileLayout::from_saved(
            Node::Stack {
                panes: vec![pane(1), pane(2), pane(3)],
                expanded: 1,
            },
            pane(2),
        );
        let infos = layout.panes(Rect::new(0, 0, 80, 24));
        assert_eq!(infos.len(), 3);

        assert_eq!(infos[0].id, pane(1));
        assert_eq!(infos[0].rect, Rect::new(0, 0, 80, 1));
        assert!(!infos[0].is_focused);
        assert_eq!(
            infos[0].stack,
            Some(StackMember {
                collapsed: true,
                position: 0,
                count: 3
            })
        );

        assert_eq!(infos[1].id, pane(2));
        assert_eq!(infos[1].rect, Rect::new(0, 1, 80, 22));
        assert!(infos[1].is_focused);
        assert_eq!(
            infos[1].stack,
            Some(StackMember {
                collapsed: false,
                position: 1,
                count: 3
            })
        );

        assert_eq!(infos[2].id, pane(3));
        assert_eq!(infos[2].rect, Rect::new(0, 23, 80, 1));
        assert!(!infos[2].is_focused);
        assert_eq!(
            infos[2].stack,
            Some(StackMember {
                collapsed: true,
                position: 2,
                count: 3
            })
        );
    }

    #[test]
    fn swap_pane_ids_in_stack_preserves_order_and_length() {
        // step-2's expanded/focus reconciliation depends on swap being in-place
        // with no Vec reorder; lock that precondition here.
        let mut layout = TileLayout::from_saved(
            Node::Stack {
                panes: vec![pane(1), pane(2), pane(3)],
                expanded: 1,
            },
            pane(2),
        );
        assert!(layout.swap_panes(pane(1), pane(3)));
        match layout.root() {
            Node::Stack { panes, expanded } => {
                assert_eq!(*panes, vec![pane(3), pane(2), pane(1)]);
                assert_eq!(*expanded, 1);
            }
            _ => panic!("expected Stack"),
        }
    }

    #[test]
    fn count_and_ids_include_stack_members() {
        let layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Horizontal,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane(1))),
                second: Box::new(Node::Stack {
                    panes: vec![pane(2), pane(3), pane(4)],
                    expanded: 0,
                }),
            },
            pane(1),
        );
        assert_eq!(layout.pane_count(), 4);
        assert_eq!(layout.pane_ids(), vec![pane(1), pane(2), pane(3), pane(4)]);
    }

    #[test]
    fn remove_pane_from_stack_promotes_and_collapses() {
        let root = Node::Stack {
            panes: vec![pane(1), pane(2), pane(3)],
            expanded: 1,
        };
        let result = remove_pane(root, pane(2)).unwrap();
        match &result {
            Node::Stack { panes, expanded } => {
                assert_eq!(*panes, vec![pane(1), pane(3)]);
                assert_eq!(*expanded, 1); // promotes below (index 1 after removal)
            }
            _ => panic!("expected Stack"),
        }

        // Removing another collapses to Pane
        let result = remove_pane(result, pane(1)).unwrap();
        match result {
            Node::Pane(id) => assert_eq!(id, pane(3)),
            _ => panic!("expected Pane"),
        }
    }

    #[test]
    fn remove_last_member_from_stack_promotes_above() {
        let root = Node::Stack {
            panes: vec![pane(1), pane(2), pane(3)],
            expanded: 2,
        };
        let result = remove_pane(root, pane(3)).unwrap();
        match result {
            Node::Stack { panes, expanded } => {
                assert_eq!(panes, vec![pane(1), pane(2)]);
                assert_eq!(expanded, 1); // now-last member
            }
            _ => panic!("expected Stack"),
        }
    }

    // --- Step-2: focus auto-expand tests ---

    #[test]
    fn focus_pane_auto_expands_in_stack() {
        let mut layout = TileLayout::from_saved(
            Node::Stack {
                panes: vec![pane(1), pane(2), pane(3)],
                expanded: 1,
            },
            pane(2),
        );
        layout.focus_pane(pane(3));
        assert_eq!(layout.focused(), pane(3));
        match layout.root() {
            Node::Stack { expanded, .. } => assert_eq!(*expanded, 2),
            _ => panic!("expected Stack"),
        }

        layout.focus_pane(pane(1));
        assert_eq!(layout.focused(), pane(1));
        match layout.root() {
            Node::Stack { expanded, .. } => assert_eq!(*expanded, 0),
            _ => panic!("expected Stack"),
        }
    }

    #[test]
    fn focused_in_stack_returns_correct_boolean() {
        let layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Horizontal,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane(1))),
                second: Box::new(Node::Stack {
                    panes: vec![pane(2), pane(3)],
                    expanded: 0,
                }),
            },
            pane(2),
        );
        assert!(layout.focused_in_stack());

        let layout2 = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Horizontal,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane(1))),
                second: Box::new(Node::Stack {
                    panes: vec![pane(2), pane(3)],
                    expanded: 0,
                }),
            },
            pane(1),
        );
        assert!(!layout2.focused_in_stack());
    }

    // --- Step-2: close_focused stack-aware promotion tests ---

    #[test]
    fn close_focused_promotes_below_in_stack() {
        let mut layout = TileLayout::from_saved(
            Node::Stack {
                panes: vec![pane(1), pane(2), pane(3)],
                expanded: 1,
            },
            pane(2),
        );
        assert!(layout.close_focused());
        assert_eq!(layout.focused(), pane(3));
        match layout.root() {
            Node::Stack { panes, expanded } => {
                assert_eq!(*panes, vec![pane(1), pane(3)]);
                assert_eq!(*expanded, 1);
            }
            _ => panic!("expected Stack"),
        }
    }

    #[test]
    fn close_focused_promotes_above_when_bottom() {
        let mut layout = TileLayout::from_saved(
            Node::Stack {
                panes: vec![pane(1), pane(2), pane(3)],
                expanded: 2,
            },
            pane(3),
        );
        assert!(layout.close_focused());
        assert_eq!(layout.focused(), pane(2));
        match layout.root() {
            Node::Stack { panes, expanded } => {
                assert_eq!(*panes, vec![pane(1), pane(2)]);
                assert_eq!(*expanded, 1);
            }
            _ => panic!("expected Stack"),
        }
    }

    #[test]
    fn close_focused_non_stack_preserves_flat_order() {
        let mut layout = sample_layout();
        layout.focus_pane(pane(2));
        assert!(layout.close_focused());
        // pane(2) was at position 1 in [1,2,3,4], so new focus should be ids[2] = pane(3)
        assert_eq!(layout.focused(), pane(3));
    }

    // --- Step-2: swap_panes expanded reconciliation ---

    #[test]
    fn swap_panes_reconciles_expanded_when_focus_moves() {
        let mut layout = TileLayout::from_saved(
            Node::Stack {
                panes: vec![pane(1), pane(2), pane(3)],
                expanded: 1,
            },
            pane(2),
        );
        // Swap pane(1) and pane(2): [2, 1, 3], focus still on pane(2) which is now at index 0
        assert!(layout.swap_panes(pane(1), pane(2)));
        match layout.root() {
            Node::Stack { panes, expanded } => {
                assert_eq!(*panes, vec![pane(2), pane(1), pane(3)]);
                assert_eq!(*expanded, 0); // reconciled to follow focus
            }
            _ => panic!("expected Stack"),
        }
    }

    // --- Step-2: stack_focused tests ---

    #[test]
    fn stack_focused_two_panes_become_stack() {
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane(1))),
                second: Box::new(Node::Pane(pane(2))),
            },
            pane(1),
        );
        assert!(layout.stack_focused());
        match layout.root() {
            Node::Stack { panes, expanded } => {
                assert_eq!(*panes, vec![pane(1), pane(2)]);
                assert_eq!(*expanded, 0);
            }
            _ => panic!("expected Stack"),
        }
        assert_eq!(layout.focused(), pane(1));
    }

    #[test]
    fn only_non_expanded_stack_members_are_collapsed() {
        let layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(Node::Stack {
                    panes: vec![pane(1), pane(2)],
                    expanded: 0,
                }),
                second: Box::new(Node::Pane(pane(3))),
            },
            pane(1),
        );
        assert!(!layout.is_collapsed_stack_member(pane(1)));
        assert!(layout.is_collapsed_stack_member(pane(2)));
        assert!(!layout.is_collapsed_stack_member(pane(3)));
    }

    #[test]
    fn splitting_a_stack_member_wraps_the_whole_stack() {
        let stacked = || Node::Stack {
            panes: vec![pane(1), pane(2), pane(3)],
            expanded: 0,
        };
        let assert_wrapped = |layout: &TileLayout, new: PaneId| match layout.root() {
            Node::Split { first, second, .. } => {
                assert!(
                    matches!(first.as_ref(), Node::Stack { panes, .. } if *panes == vec![pane(1), pane(2), pane(3)]),
                    "every member stays in the stack"
                );
                assert!(matches!(second.as_ref(), Node::Pane(id) if *id == new));
            }
            _ => panic!("expected the stack wrapped in a split"),
        };

        let mut layout = TileLayout::from_saved(stacked(), pane(1));
        let new = layout
            .split_pane(pane(2), Direction::Vertical, 0.5)
            .expect("member is in the layout");
        assert_wrapped(&layout, new);
        assert_eq!(layout.focused(), pane(1));

        let mut layout = TileLayout::from_saved(stacked(), pane(1));
        assert!(layout.insert_pane_near(pane(3), pane(9), Direction::Horizontal, 0.5, false));
        assert_wrapped(&layout, pane(9));
    }

    #[test]
    fn stack_focused_second_pane_becomes_stack() {
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane(1))),
                second: Box::new(Node::Pane(pane(2))),
            },
            pane(2),
        );
        assert!(layout.stack_focused());
        match layout.root() {
            Node::Stack { panes, expanded } => {
                assert_eq!(*panes, vec![pane(1), pane(2)]);
                assert_eq!(*expanded, 1);
            }
            _ => panic!("expected Stack"),
        }
        assert_eq!(layout.focused(), pane(2));
    }

    #[test]
    fn stack_focused_pane_joins_adjacent_stack() {
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane(1))),
                second: Box::new(Node::Stack {
                    panes: vec![pane(2), pane(3)],
                    expanded: 0,
                }),
            },
            pane(1),
        );
        assert!(layout.stack_focused());
        match layout.root() {
            Node::Stack { panes, expanded } => {
                assert_eq!(*panes, vec![pane(1), pane(2), pane(3)]);
                assert_eq!(*expanded, 0);
            }
            _ => panic!("expected Stack"),
        }
    }

    #[test]
    fn stack_focused_pane_joins_stack_from_second_position() {
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(Node::Stack {
                    panes: vec![pane(1), pane(2)],
                    expanded: 0,
                }),
                second: Box::new(Node::Pane(pane(3))),
            },
            pane(3),
        );
        assert!(layout.stack_focused());
        match layout.root() {
            Node::Stack { panes, expanded } => {
                assert_eq!(*panes, vec![pane(1), pane(2), pane(3)]);
                assert_eq!(*expanded, 2);
            }
            _ => panic!("expected Stack"),
        }
    }

    #[test]
    fn stack_focused_sibling_split_is_noop() {
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane(1))),
                second: Box::new(Node::Split {
                    direction: Direction::Horizontal,
                    ratio: 0.5,
                    first: Box::new(Node::Pane(pane(2))),
                    second: Box::new(Node::Pane(pane(3))),
                }),
            },
            pane(1),
        );
        assert!(!layout.stack_focused());
    }

    #[test]
    fn stack_focused_lone_root_is_noop() {
        let mut layout = TileLayout::from_saved(Node::Pane(pane(1)), pane(1));
        assert!(!layout.stack_focused());
    }

    // --- Step-2: unstack_focused tests ---

    #[test]
    fn unstack_focused_creates_sibling_split() {
        let mut layout = TileLayout::from_saved(
            Node::Stack {
                panes: vec![pane(1), pane(2), pane(3)],
                expanded: 1,
            },
            pane(2),
        );
        assert!(layout.unstack_focused(Direction::Vertical, 0.5));
        assert_eq!(layout.focused(), pane(2));
        match layout.root() {
            Node::Split { first, second, .. } => {
                match first.as_ref() {
                    Node::Stack { panes, expanded } => {
                        assert_eq!(*panes, vec![pane(1), pane(3)]);
                        assert!(*expanded < panes.len());
                    }
                    _ => panic!("expected Stack as first child"),
                }
                match second.as_ref() {
                    Node::Pane(id) => assert_eq!(*id, pane(2)),
                    _ => panic!("expected Pane(2) as second child"),
                }
            }
            _ => panic!("expected Split"),
        }
    }

    #[test]
    fn unstack_focused_two_member_stack_collapses_to_pane() {
        let mut layout = TileLayout::from_saved(
            Node::Stack {
                panes: vec![pane(1), pane(2)],
                expanded: 0,
            },
            pane(1),
        );
        assert!(layout.unstack_focused(Direction::Vertical, 0.5));
        assert_eq!(layout.focused(), pane(1));
        match layout.root() {
            Node::Split { first, second, .. } => {
                match first.as_ref() {
                    Node::Pane(id) => assert_eq!(*id, pane(2)),
                    _ => panic!("expected Pane(2) as residual"),
                }
                match second.as_ref() {
                    Node::Pane(id) => assert_eq!(*id, pane(1)),
                    _ => panic!("expected Pane(1) as unstacked"),
                }
            }
            _ => panic!("expected Split"),
        }
    }

    #[test]
    fn unstack_focused_not_in_stack_is_noop() {
        let mut layout = TileLayout::from_saved(Node::Pane(pane(1)), pane(1));
        assert!(!layout.unstack_focused(Direction::Vertical, 0.5));
    }

    // --- Step-2: fold_new_pane_into_focused_stack tests ---

    /// The stack holding focus must show focus as its expanded member.
    fn assert_focus_is_expanded(layout: &TileLayout) {
        fn check(node: &Node, focus: PaneId) {
            match node {
                Node::Pane(_) => {}
                Node::Split { first, second, .. } => {
                    check(first, focus);
                    check(second, focus);
                }
                Node::Stack { panes, expanded } => {
                    if panes.contains(&focus) {
                        assert_eq!(panes[*expanded], focus, "expanded member is not focus");
                    }
                }
            }
        }
        check(layout.root(), layout.focused());
    }

    #[test]
    fn stack_pane_targets_a_non_focused_pane_and_keeps_focus_expanded() {
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane(1))),
                second: Box::new(Node::Pane(pane(2))),
            },
            pane(1),
        );
        assert!(layout.stack_pane(pane(2)));
        assert_eq!(layout.focused(), pane(1));
        assert!(
            matches!(layout.root(), Node::Stack { panes, .. } if *panes == vec![pane(1), pane(2)])
        );
        assert_focus_is_expanded(&layout);
    }

    #[test]
    fn unstack_pane_targets_a_non_focused_member_and_keeps_focus_expanded() {
        let mut layout = TileLayout::from_saved(
            Node::Stack {
                panes: vec![pane(1), pane(2), pane(3)],
                expanded: 0,
            },
            pane(1),
        );
        assert!(layout.unstack_pane(pane(3), Direction::Vertical, 0.5));
        assert_eq!(layout.focused(), pane(1));
        assert_focus_is_expanded(&layout);
        assert!(!layout.stack_pane(pane(99)));
        assert!(!layout.unstack_pane(pane(3), Direction::Vertical, 0.5));
    }

    #[test]
    fn fold_new_pane_into_stack_with_capacity() {
        // Setup: stack with 2 members adjacent to a pane (simulates post-split state)
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(Node::Stack {
                    panes: vec![pane(1), pane(2)],
                    expanded: 0,
                }),
                second: Box::new(Node::Pane(pane(3))),
            },
            pane(3), // focus is on new_id (post-split state)
        );
        let area = Rect::new(0, 0, 80, 30);
        // stack_member = pane(1), which was the old focus before split
        assert!(layout.fold_new_pane_into_focused_stack(pane(3), pane(1), area));
        assert_eq!(layout.focused(), pane(3));
        match layout.root() {
            Node::Stack { panes, expanded } => {
                assert_eq!(*panes, vec![pane(1), pane(2), pane(3)]);
                assert_eq!(*expanded, 2);
            }
            _ => panic!("expected Stack"),
        }
    }

    #[test]
    fn fold_new_pane_no_capacity_returns_false() {
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(Node::Stack {
                    panes: vec![pane(1), pane(2), pane(3), pane(4)],
                    expanded: 0,
                }),
                second: Box::new(Node::Pane(pane(5))),
            },
            pane(5), // focus on new_id (post-split)
        );
        // area height 8: after fold would be 5 members -> 4 collapsed rows
        // expanded = 8 - 4 = 4 < MIN_STACK_EXPANDED_ROWS(5)
        let area = Rect::new(0, 0, 80, 8);
        assert!(!layout.fold_new_pane_into_focused_stack(pane(5), pane(1), area));
        // Tree unchanged, focus stays on pane(5)
        assert_eq!(layout.focused(), pane(5));
    }

    // --- Step-5: replace_subtree_with_stack (layout.apply construction) ---

    #[test]
    fn replace_subtree_with_stack_right_leaning_chain() {
        // Shape produced by layout.apply: Split(p1, Split(p2, p3)).
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane(1))),
                second: Box::new(Node::Split {
                    direction: Direction::Vertical,
                    ratio: 0.5,
                    first: Box::new(Node::Pane(pane(2))),
                    second: Box::new(Node::Pane(pane(3))),
                }),
            },
            pane(1),
        );
        assert!(layout.replace_subtree_with_stack(&[pane(1), pane(2), pane(3)], 1));
        match layout.root() {
            Node::Stack { panes, expanded } => {
                assert_eq!(panes, &[pane(1), pane(2), pane(3)]);
                assert_eq!(*expanded, 1);
            }
            _ => panic!("expected stack root"),
        }
        assert_eq!(layout.focused(), pane(2));
    }

    #[test]
    fn replace_subtree_with_stack_left_leaning_chain() {
        // Split(Split(p1, p2), p3) — exercises the all_in_first descent.
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(Node::Split {
                    direction: Direction::Vertical,
                    ratio: 0.5,
                    first: Box::new(Node::Pane(pane(1))),
                    second: Box::new(Node::Pane(pane(2))),
                }),
                second: Box::new(Node::Pane(pane(3))),
            },
            pane(1),
        );
        assert!(layout.replace_subtree_with_stack(&[pane(1), pane(2), pane(3)], 0));
        match layout.root() {
            Node::Stack { panes, expanded } => {
                assert_eq!(panes, &[pane(1), pane(2), pane(3)]);
                assert_eq!(*expanded, 0);
            }
            _ => panic!("expected stack root"),
        }
    }

    #[test]
    fn replace_subtree_with_stack_nested_subtree_preserves_siblings() {
        // Stack only the right side; the outer split and pane(9) must survive.
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Horizontal,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane(9))),
                second: Box::new(Node::Split {
                    direction: Direction::Vertical,
                    ratio: 0.5,
                    first: Box::new(Node::Pane(pane(1))),
                    second: Box::new(Node::Pane(pane(2))),
                }),
            },
            pane(9),
        );
        assert!(layout.replace_subtree_with_stack(&[pane(1), pane(2)], 0));
        match layout.root() {
            Node::Split { first, second, .. } => {
                assert!(matches!(**first, Node::Pane(p) if p == pane(9)));
                assert!(matches!(**second, Node::Stack { .. }));
            }
            _ => panic!("expected split root"),
        }
    }

    #[test]
    fn replace_subtree_with_stack_non_contiguous_leaves_tree_intact() {
        // ids [p1, p3] are interleaved with non-member p2, so they do not form
        // an exact subtree. Must return false AND leave the tree unchanged
        // (no Stack node, all original panes still present, focus unmoved).
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane(1))),
                second: Box::new(Node::Split {
                    direction: Direction::Vertical,
                    ratio: 0.5,
                    first: Box::new(Node::Pane(pane(2))),
                    second: Box::new(Node::Pane(pane(3))),
                }),
            },
            pane(1),
        );
        assert!(!layout.replace_subtree_with_stack(&[pane(1), pane(3)], 0));
        assert!(
            matches!(layout.root(), Node::Split { .. }),
            "tree must remain a split on failure, not be clobbered"
        );
        let mut ids = layout.pane_ids();
        ids.sort_by_key(|id| id.raw());
        assert_eq!(ids, vec![pane(1), pane(2), pane(3)]);
        assert_eq!(layout.focused(), pane(1));
    }

    #[test]
    fn replace_subtree_with_stack_missing_id_returns_false() {
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane(1))),
                second: Box::new(Node::Pane(pane(2))),
            },
            pane(1),
        );
        assert!(!layout.replace_subtree_with_stack(&[pane(1), pane(99)], 0));
        assert!(matches!(layout.root(), Node::Split { .. }));
        let mut ids = layout.pane_ids();
        ids.sort_by_key(|id| id.raw());
        assert_eq!(ids, vec![pane(1), pane(2)]);
    }

    // --- Step-2: directional navigation characterization ---

    #[test]
    fn directional_nav_in_4_member_stack_up_down() {
        // Stack within a vertical split: pane(5) above, stack below
        let layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.3,
                first: Box::new(Node::Pane(pane(5))),
                second: Box::new(Node::Stack {
                    panes: vec![pane(1), pane(2), pane(3), pane(4)],
                    expanded: 1,
                }),
            },
            pane(2),
        );
        let area = Rect::new(0, 0, 80, 40);
        let panes = layout.panes(area);

        // From expanded (pane 2): up should land on pane(1), down on pane(3)
        let p2 = panes.iter().find(|p| p.id == pane(2)).unwrap();
        assert_eq!(
            find_in_direction(p2, NavDirection::Up, &panes),
            Some(pane(1))
        );
        assert_eq!(
            find_in_direction(p2, NavDirection::Down, &panes),
            Some(pane(3))
        );

        // From top of stack (pane 1 when expanded): up should exit to pane(5)
        let mut layout2 = layout;
        layout2.focus_pane(pane(1));
        let panes2 = layout2.panes(area);
        let p1 = panes2.iter().find(|p| p.id == pane(1)).unwrap();
        assert_eq!(
            find_in_direction(p1, NavDirection::Up, &panes2),
            Some(pane(5))
        );

        // From bottom of stack (pane 4 when expanded): down should be None (no pane below)
        let layout3 = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.3,
                first: Box::new(Node::Pane(pane(5))),
                second: Box::new(Node::Stack {
                    panes: vec![pane(1), pane(2), pane(3), pane(4)],
                    expanded: 3,
                }),
            },
            pane(4),
        );
        let panes3 = layout3.panes(area);
        let p4 = panes3.iter().find(|p| p.id == pane(4)).unwrap();
        assert_eq!(find_in_direction(p4, NavDirection::Down, &panes3), None);
    }

    #[test]
    fn directional_nav_stack_no_wrap_isolated() {
        // Stack as root, no surrounding panes
        let layout = TileLayout::from_saved(
            Node::Stack {
                panes: vec![pane(1), pane(2), pane(3), pane(4)],
                expanded: 0,
            },
            pane(1),
        );
        let area = Rect::new(0, 0, 80, 40);
        let panes = layout.panes(area);
        let p1 = panes.iter().find(|p| p.id == pane(1)).unwrap();
        assert_eq!(find_in_direction(p1, NavDirection::Up, &panes), None);

        let layout2 = TileLayout::from_saved(
            Node::Stack {
                panes: vec![pane(1), pane(2), pane(3), pane(4)],
                expanded: 3,
            },
            pane(4),
        );
        let panes2 = layout2.panes(area);
        let p4 = panes2.iter().find(|p| p.id == pane(4)).unwrap();
        assert_eq!(find_in_direction(p4, NavDirection::Down, &panes2), None);
    }

    #[test]
    fn directional_nav_left_right_enters_stack_on_expanded() {
        let layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Horizontal,
                ratio: 0.3,
                first: Box::new(Node::Pane(pane(5))),
                second: Box::new(Node::Stack {
                    panes: vec![pane(1), pane(2), pane(3), pane(4)],
                    expanded: 1,
                }),
            },
            pane(5),
        );
        let area = Rect::new(0, 0, 80, 40);
        let panes = layout.panes(area);
        let p5 = panes.iter().find(|p| p.id == pane(5)).unwrap();
        // Right from pane(5) should land on pane(2) — the expanded member with the largest rect
        assert_eq!(
            find_in_direction(p5, NavDirection::Right, &panes),
            Some(pane(2))
        );
    }

    // --- Step-2: in-stack resize characterization ---

    #[test]
    fn resize_in_stack_is_noop() {
        let mut layout = TileLayout::from_saved(
            Node::Stack {
                panes: vec![pane(1), pane(2), pane(3)],
                expanded: 1,
            },
            pane(2),
        );
        let area = Rect::new(0, 0, 80, 40);
        // Resize should not panic and should not change anything (no split borders in a stack)
        layout.resize_focused(NavDirection::Up, 0.05, area);
        layout.resize_focused(NavDirection::Down, 0.05, area);
        match layout.root() {
            Node::Stack { panes, expanded } => {
                assert_eq!(*panes, vec![pane(1), pane(2), pane(3)]);
                assert_eq!(*expanded, 1);
            }
            _ => panic!("expected Stack"),
        }
    }

    #[test]
    fn resize_expanded_member_changes_containing_split_ratio() {
        // Split { Vertical, Stack{[1,2,3], expanded:2}, Pane(4) }: the stack is
        // the top child and its expanded member sits at the bottom of the stack,
        // so its lower edge is adjacent to the containing split border. Resizing
        // the focused expanded member (pane 3) downward grows the stack area by
        // changing the containing split's ratio (R10 = Zellij §8, structurally);
        // the gained height flows to the expanded member because the collapsed
        // members stay pinned at 1 row. Members do not reorder.
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(Node::Stack {
                    panes: vec![pane(1), pane(2), pane(3)],
                    expanded: 2,
                }),
                second: Box::new(Node::Pane(pane(4))),
            },
            pane(3),
        );
        let area = Rect::new(0, 0, 80, 40);

        layout.resize_focused(NavDirection::Down, 0.05, area);

        let splits = split_snapshot(&layout);
        assert_eq!(splits.len(), 1);
        assert_eq!(splits[0].0, Direction::Vertical);
        assert!(
            (splits[0].1 - 0.55).abs() < f32::EPSILON,
            "expected ratio to grow to 0.55, got {}",
            splits[0].1
        );

        // The stack survived unchanged: same members, same order, same expanded.
        match layout.root() {
            Node::Split { first, .. } => match first.as_ref() {
                Node::Stack { panes, expanded } => {
                    assert_eq!(*panes, vec![pane(1), pane(2), pane(3)]);
                    assert_eq!(*expanded, 2);
                }
                _ => panic!("expected Stack as first child"),
            },
            _ => panic!("expected Split root"),
        }
    }
}
