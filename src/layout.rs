//! Dwindle (binary space partitioning) layout tree, modelled on Hyprland's
//! dwindle layout with Omarchy's settings (`preserve_split = true`,
//! `force_split = 2`).
//!
//! Each workspace owns one [`Tree`]. Leaves hold windows; internal nodes hold
//! a split axis and ratio. Inserting a window splits the target leaf along its
//! longer side and puts the new window second (right/bottom). Removing a window
//! promotes its sibling. The tree is pure data and fully unit tested.

use crate::geometry::{find_neighbor, Axis, Direction, Rect};
use crate::WindowId;
use std::collections::HashMap;

pub const MIN_RATIO: f64 = 0.1;
pub const MAX_RATIO: f64 = 0.9;

type NodeId = usize;

#[derive(Debug, Clone)]
enum NodeKind {
    Leaf(WindowId),
    Split {
        axis: Axis,
        ratio: f64,
        children: [NodeId; 2],
    },
}

#[derive(Debug, Clone)]
struct Node {
    parent: Option<NodeId>,
    kind: NodeKind,
}

/// Where a new window lands relative to the split target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ForceSplit {
    /// Hyprland `force_split = 0`: in Hyprland this follows the cursor; with no
    /// cursor information we treat it like `Second`.
    #[default]
    Auto,
    /// `force_split = 1`: new window goes left/top.
    First,
    /// `force_split = 2` (Omarchy default): new window goes right/bottom.
    Second,
}

impl ForceSplit {
    pub fn from_hyprland(value: u8) -> ForceSplit {
        match value {
            1 => ForceSplit::First,
            2 => ForceSplit::Second,
            _ => ForceSplit::Auto,
        }
    }
}

/// Gap settings applied when computing frames (Hyprland `gaps_in`/`gaps_out`).
/// `gaps_in` is applied per window side, so two neighbours are `2 * gaps_in`
/// apart, matching Hyprland.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Gaps {
    pub inner: f64,
    pub outer: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Tree {
    nodes: Vec<Option<Node>>,
    free: Vec<NodeId>,
    root: Option<NodeId>,
    /// Most recently inserted window; split target when nothing is focused.
    last_inserted: Option<WindowId>,
}

impl Tree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    pub fn len(&self) -> usize {
        self.windows().len()
    }

    pub fn contains(&self, window: WindowId) -> bool {
        self.find_leaf(window).is_some()
    }

    /// Windows in depth-first (left-to-right / top-to-bottom) order.
    pub fn windows(&self) -> Vec<WindowId> {
        let mut out = Vec::new();
        if let Some(root) = self.root {
            self.collect(root, &mut out);
        }
        out
    }

    fn collect(&self, id: NodeId, out: &mut Vec<WindowId>) {
        match &self.node(id).kind {
            NodeKind::Leaf(w) => out.push(*w),
            NodeKind::Split { children, .. } => {
                self.collect(children[0], out);
                self.collect(children[1], out);
            }
        }
    }

    /// Inserts `window` by splitting `target` (or the last inserted window).
    ///
    /// `area` is the workspace area used to determine the target's current
    /// aspect ratio, and therefore the split axis.
    pub fn insert(
        &mut self,
        window: WindowId,
        target: Option<WindowId>,
        area: Rect,
        force: ForceSplit,
        ratio: f64,
    ) {
        if self.contains(window) {
            return;
        }
        let Some(root) = self.root else {
            self.root = Some(self.alloc(Node {
                parent: None,
                kind: NodeKind::Leaf(window),
            }));
            self.last_inserted = Some(window);
            return;
        };

        let target_leaf = target
            .and_then(|t| self.find_leaf(t))
            .or_else(|| self.last_inserted.and_then(|t| self.find_leaf(t)))
            .unwrap_or_else(|| self.last_leaf(root));

        let target_rect = self.node_rect(target_leaf, area);
        let axis = Axis::for_rect(&target_rect);
        let parent = self.node(target_leaf).parent;

        let new_leaf = self.alloc(Node {
            parent: None,
            kind: NodeKind::Leaf(window),
        });
        let children = match force {
            ForceSplit::First => [new_leaf, target_leaf],
            ForceSplit::Second | ForceSplit::Auto => [target_leaf, new_leaf],
        };
        let split = self.alloc(Node {
            parent,
            kind: NodeKind::Split {
                axis,
                ratio: ratio.clamp(MIN_RATIO, MAX_RATIO),
                children,
            },
        });
        self.node_mut(target_leaf).parent = Some(split);
        self.node_mut(new_leaf).parent = Some(split);
        self.replace_child(parent, target_leaf, split);
        self.last_inserted = Some(window);
    }

    /// Removes `window`; its sibling takes over the parent's space.
    /// Returns `false` if the window was not in the tree.
    pub fn remove(&mut self, window: WindowId) -> bool {
        let Some(leaf) = self.find_leaf(window) else {
            return false;
        };
        if self.last_inserted == Some(window) {
            self.last_inserted = None;
        }
        let parent = self.node(leaf).parent;
        self.dealloc(leaf);
        let Some(parent) = parent else {
            self.root = None;
            return true;
        };
        let NodeKind::Split { children, .. } = self.node(parent).kind else {
            unreachable!("parent of a leaf is always a split");
        };
        let sibling = if children[0] == leaf {
            children[1]
        } else {
            children[0]
        };
        let grandparent = self.node(parent).parent;
        self.node_mut(sibling).parent = grandparent;
        self.replace_child(grandparent, parent, sibling);
        self.dealloc(parent);
        true
    }

    /// Swaps the positions of two windows in the tree.
    pub fn swap(&mut self, a: WindowId, b: WindowId) -> bool {
        let (Some(la), Some(lb)) = (self.find_leaf(a), self.find_leaf(b)) else {
            return false;
        };
        self.node_mut(la).kind = NodeKind::Leaf(b);
        self.node_mut(lb).kind = NodeKind::Leaf(a);
        true
    }

    /// Flips the split axis of the window's parent (Hyprland `togglesplit`).
    pub fn toggle_split(&mut self, window: WindowId) -> bool {
        let Some(parent) = self.find_leaf(window).and_then(|l| self.node(l).parent) else {
            return false;
        };
        if let NodeKind::Split { axis, .. } = &mut self.node_mut(parent).kind {
            *axis = axis.flipped();
        }
        true
    }

    /// Grows (positive) or shrinks (negative) `window` by `dx`/`dy` points,
    /// adjusting the nearest ancestor split on each axis
    /// (Hyprland `resizeactive dx dy`).
    pub fn resize(&mut self, window: WindowId, dx: f64, dy: f64, area: Rect) -> bool {
        let Some(leaf) = self.find_leaf(window) else {
            return false;
        };
        let mut changed = false;
        for (axis, delta) in [(Axis::Horizontal, dx), (Axis::Vertical, dy)] {
            if delta == 0.0 {
                continue;
            }
            if let Some((split, child_is_first)) = self.ancestor_split(leaf, axis) {
                let rect = self.node_rect(split, area);
                let extent = match axis {
                    Axis::Horizontal => rect.w,
                    Axis::Vertical => rect.h,
                };
                if extent <= 0.0 {
                    continue;
                }
                let signed = if child_is_first { delta } else { -delta };
                if let NodeKind::Split { ratio, .. } = &mut self.node_mut(split).kind {
                    *ratio = (*ratio + signed / extent).clamp(MIN_RATIO, MAX_RATIO);
                    changed = true;
                }
            }
        }
        changed
    }

    /// Computes the frame for every window inside `area` with Hyprland gap
    /// semantics: `gaps.outer` against the area edges, `gaps.inner` on each
    /// side facing another window.
    pub fn layout(&self, area: Rect, gaps: Gaps) -> Vec<(WindowId, Rect)> {
        let mut out = Vec::new();
        let Some(root) = self.root else {
            return out;
        };
        let inner_area = area.inset(gaps.outer);
        let mut raw = Vec::new();
        self.layout_node(root, inner_area, &mut raw);
        for (w, r) in raw {
            let left = if (r.x - inner_area.x).abs() < 0.5 {
                0.0
            } else {
                gaps.inner
            };
            let top = if (r.y - inner_area.y).abs() < 0.5 {
                0.0
            } else {
                gaps.inner
            };
            let right = if (r.max_x() - inner_area.max_x()).abs() < 0.5 {
                0.0
            } else {
                gaps.inner
            };
            let bottom = if (r.max_y() - inner_area.max_y()).abs() < 0.5 {
                0.0
            } else {
                gaps.inner
            };
            out.push((w, r.inset_sides(left, top, right, bottom)));
        }
        out
    }

    /// Respect minimum sizes learned from AX readback. Preserve the split
    /// unless changing its axis is necessary to fit both children.
    pub fn constrain(
        &mut self,
        area: Rect,
        gaps: Gaps,
        minimums: &HashMap<WindowId, (f64, f64)>,
    ) -> bool {
        let Some(root) = self.root else { return true };
        let mut candidate = self.clone();
        let area = area.inset(gaps.outer);
        if candidate.constrain_node(root, area, gaps.inner, minimums) {
            *self = candidate;
            return true;
        }
        // A nested subtree may need to rotate before its parent can fit.
        // Search the smallest number of axis changes first, with a fixed
        // budget so crowded workspaces cannot stall the event loop.
        let splits: Vec<_> = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(id, node)| {
                matches!(node.as_ref().map(|n| &n.kind), Some(NodeKind::Split { .. })).then_some(id)
            })
            .take(12)
            .collect();
        let mut masks: Vec<usize> = (1..(1 << splits.len())).collect();
        masks.sort_by_key(|mask| mask.count_ones());
        for mask in masks {
            let mut candidate = self.clone();
            for (bit, id) in splits.iter().enumerate() {
                if mask & (1 << bit) != 0 {
                    if let NodeKind::Split { axis, .. } = &mut candidate.node_mut(*id).kind {
                        *axis = axis.flipped();
                    }
                }
            }
            if candidate.constrain_node(root, area, gaps.inner, minimums) {
                *self = candidate;
                return true;
            }
        }
        self.regroup_to_fit(area, gaps.inner, minimums)
    }

    /// macOS minimum sizes can make the existing dwindle topology impossible
    /// even though all windows fit in another BSP arrangement. Regroup into
    /// columns only after preserving that topology has failed.
    fn regroup_to_fit(
        &mut self,
        area: Rect,
        gap: f64,
        minimums: &HashMap<WindowId, (f64, f64)>,
    ) -> bool {
        let mut windows = self.windows();
        let size = |id: &WindowId| {
            let (w, h) = minimums.get(id).copied().unwrap_or((100.0, 100.0));
            (w + 2.0 * gap, h + 2.0 * gap)
        };
        windows.sort_by(|a, b| size(b).1.total_cmp(&size(a).1));
        for count in 1..=windows.len() {
            let mut columns: Vec<Vec<WindowId>> = vec![Vec::new(); count];
            let mut heights = vec![0.0_f64; count];
            let mut widths = vec![0.0_f64; count];
            let mut fits = true;
            for id in &windows {
                let (w, h) = size(id);
                let target = (0..count)
                    .filter(|i| heights[*i] + h <= area.h + 0.5)
                    .min_by(|a, b| heights[*a].total_cmp(&heights[*b]));
                let Some(i) = target else {
                    fits = false;
                    break;
                };
                columns[i].push(*id);
                heights[i] += h;
                widths[i] = widths[i].max(w);
            }
            if !fits || widths.iter().sum::<f64>() > area.w + 0.5 {
                continue;
            }
            let mut candidate = Tree::new();
            let mut roots = Vec::new();
            for column in columns.into_iter().filter(|c| !c.is_empty()) {
                let leaves = column
                    .into_iter()
                    .map(|id| {
                        candidate.alloc(Node {
                            parent: None,
                            kind: NodeKind::Leaf(id),
                        })
                    })
                    .collect();
                roots.push(candidate.join(leaves, Axis::Vertical));
            }
            candidate.root = Some(candidate.join(roots, Axis::Horizontal));
            candidate.last_inserted = self.last_inserted;
            if candidate.constrain_node(candidate.root.unwrap(), area, gap, minimums) {
                *self = candidate;
                return true;
            }
        }
        false
    }

    fn join(&mut self, mut nodes: Vec<NodeId>, axis: Axis) -> NodeId {
        let count = nodes.len();
        let first = nodes.remove(0);
        if nodes.is_empty() {
            return first;
        }
        let second = self.join(nodes, axis);
        let parent = self.alloc(Node {
            parent: None,
            kind: NodeKind::Split {
                axis,
                ratio: 1.0 / count as f64,
                children: [first, second],
            },
        });
        self.node_mut(first).parent = Some(parent);
        self.node_mut(second).parent = Some(parent);
        parent
    }

    fn minimum(
        &self,
        id: NodeId,
        gap: f64,
        minimums: &HashMap<WindowId, (f64, f64)>,
    ) -> (f64, f64) {
        match self.node(id).kind {
            NodeKind::Leaf(w) => {
                let (w, h) = minimums.get(&w).copied().unwrap_or((100.0, 100.0));
                (w + 2.0 * gap, h + 2.0 * gap)
            }
            NodeKind::Split { axis, children, .. } => {
                let a = self.minimum(children[0], gap, minimums);
                let b = self.minimum(children[1], gap, minimums);
                match axis {
                    Axis::Horizontal => (a.0 + b.0, a.1.max(b.1)),
                    Axis::Vertical => (a.0.max(b.0), a.1 + b.1),
                }
            }
        }
    }

    fn constrain_node(
        &mut self,
        id: NodeId,
        area: Rect,
        gap: f64,
        minimums: &HashMap<WindowId, (f64, f64)>,
    ) -> bool {
        let NodeKind::Split {
            axis,
            ratio,
            children,
        } = self.node(id).kind
        else {
            let (w, h) = self.minimum(id, gap, minimums);
            return w <= area.w + 0.5 && h <= area.h + 0.5;
        };
        let a = self.minimum(children[0], gap, minimums);
        let b = self.minimum(children[1], gap, minimums);
        for axis in [axis, axis.flipped()] {
            let (extent, first, second, fits) = match axis {
                Axis::Horizontal => (
                    area.w,
                    a.0,
                    b.0,
                    a.0 + b.0 <= area.w + 0.5 && a.1.max(b.1) <= area.h + 0.5,
                ),
                Axis::Vertical => (
                    area.h,
                    a.1,
                    b.1,
                    a.1 + b.1 <= area.h + 0.5 && a.0.max(b.0) <= area.w + 0.5,
                ),
            };
            if !fits {
                continue;
            }
            let lower = first / extent;
            let ratio = ratio.clamp(lower, (1.0 - second / extent).max(lower));
            self.node_mut(id).kind = NodeKind::Split {
                axis,
                ratio,
                children,
            };
            let (first, second) = area.split(axis, ratio);
            return self.constrain_node(children[0], first, gap, minimums)
                && self.constrain_node(children[1], second, gap, minimums);
        }
        false
    }

    /// Window adjacent to `window` in `dir`, based on the computed layout.
    pub fn neighbor(&self, window: WindowId, dir: Direction, area: Rect) -> Option<WindowId> {
        let frames = self.layout(area, Gaps::default());
        let from = frames.iter().find(|(w, _)| *w == window)?.1;
        let others: Vec<_> = frames.into_iter().filter(|(w, _)| *w != window).collect();
        find_neighbor(&from, dir, &others)
    }

    // --- internals -------------------------------------------------------

    fn layout_node(&self, id: NodeId, rect: Rect, out: &mut Vec<(WindowId, Rect)>) {
        match &self.node(id).kind {
            NodeKind::Leaf(w) => out.push((*w, rect)),
            NodeKind::Split {
                axis,
                ratio,
                children,
            } => {
                let (a, b) = rect.split(*axis, *ratio);
                self.layout_node(children[0], a, out);
                self.layout_node(children[1], b, out);
            }
        }
    }

    /// Rect a node currently occupies within `area` (gapless).
    fn node_rect(&self, id: NodeId, area: Rect) -> Rect {
        let mut path = vec![id];
        let mut cur = id;
        while let Some(p) = self.node(cur).parent {
            path.push(p);
            cur = p;
        }
        let mut rect = area;
        for pair in path.windows(2).rev() {
            let (child, parent) = (pair[0], pair[1]);
            if let NodeKind::Split {
                axis,
                ratio,
                children,
            } = &self.node(parent).kind
            {
                let (a, b) = rect.split(*axis, *ratio);
                rect = if children[0] == child { a } else { b };
            }
        }
        rect
    }

    fn ancestor_split(&self, leaf: NodeId, axis: Axis) -> Option<(NodeId, bool)> {
        let mut child = leaf;
        while let Some(parent) = self.node(child).parent {
            if let NodeKind::Split {
                axis: a, children, ..
            } = &self.node(parent).kind
            {
                if *a == axis {
                    return Some((parent, children[0] == child));
                }
            }
            child = parent;
        }
        None
    }

    fn last_leaf(&self, mut id: NodeId) -> NodeId {
        while let NodeKind::Split { children, .. } = &self.node(id).kind {
            id = children[1];
        }
        id
    }

    fn find_leaf(&self, window: WindowId) -> Option<NodeId> {
        self.nodes
            .iter()
            .position(|n| matches!(n, Some(Node { kind: NodeKind::Leaf(w), .. }) if *w == window))
    }

    fn replace_child(&mut self, parent: Option<NodeId>, old: NodeId, new: NodeId) {
        match parent {
            None => self.root = Some(new),
            Some(p) => {
                if let NodeKind::Split { children, .. } = &mut self.node_mut(p).kind {
                    for c in children.iter_mut() {
                        if *c == old {
                            *c = new;
                        }
                    }
                }
            }
        }
    }

    fn alloc(&mut self, node: Node) -> NodeId {
        if let Some(id) = self.free.pop() {
            self.nodes[id] = Some(node);
            id
        } else {
            self.nodes.push(Some(node));
            self.nodes.len() - 1
        }
    }

    fn dealloc(&mut self, id: NodeId) {
        self.nodes[id] = None;
        self.free.push(id);
    }

    fn node(&self, id: NodeId) -> &Node {
        self.nodes[id].as_ref().expect("dangling node id")
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node {
        self.nodes[id].as_mut().expect("dangling node id")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect::new(0.0, 0.0, 1600.0, 1000.0);
    const NO_GAPS: Gaps = Gaps {
        inner: 0.0,
        outer: 0.0,
    };

    fn frame(tree: &Tree, w: WindowId) -> Rect {
        tree.layout(SCREEN, NO_GAPS)
            .into_iter()
            .find(|(id, _)| *id == w)
            .unwrap()
            .1
    }

    fn insert(tree: &mut Tree, w: WindowId, target: Option<WindowId>) {
        tree.insert(w, target, SCREEN, ForceSplit::Second, 0.5);
    }

    #[test]
    fn single_window_fills_area() {
        let mut t = Tree::new();
        insert(&mut t, 1, None);
        assert!(frame(&t, 1).approx_eq(&SCREEN));
    }

    #[test]
    fn minimum_width_grows_tile_or_changes_split_instead_of_overflowing() {
        let mut t = Tree::new();
        insert(&mut t, 1, None);
        insert(&mut t, 2, Some(1));
        let mut minimums = HashMap::from([(2, (1000.0, 400.0))]);
        assert!(t.constrain(SCREEN, NO_GAPS, &minimums));
        assert_eq!(frame(&t, 2).w, 1000.0);
        assert_eq!(frame(&t, 1).w, 600.0);
        minimums.insert(1, (1000.0, 400.0));
        assert!(t.constrain(SCREEN, NO_GAPS, &minimums));
        assert_eq!(frame(&t, 1).w, SCREEN.w);
        assert_eq!(frame(&t, 2).w, SCREEN.w);
        assert_eq!(frame(&t, 1).max_y(), frame(&t, 2).y);
        let before = t.layout(SCREEN, NO_GAPS);
        minimums.insert(1, (2000.0, 1500.0));
        assert!(!t.constrain(SCREEN, NO_GAPS, &minimums));
        assert_eq!(before, t.layout(SCREEN, NO_GAPS));
    }

    #[test]
    fn nested_minimums_can_rotate_subtrees_to_keep_apps_tiled() {
        let mut t = Tree::new();
        insert(&mut t, 1, None);
        insert(&mut t, 2, Some(1));
        insert(&mut t, 3, Some(2));
        insert(&mut t, 4, Some(3));
        let minimums = HashMap::from([
            (1, (600.0, 600.0)),
            (2, (500.0, 500.0)),
            (3, (480.0, 600.0)),
            (4, (484.0, 344.0)),
        ]);
        assert!(t.constrain(SCREEN, NO_GAPS, &minimums));
        let frames = t.layout(SCREEN, NO_GAPS);
        for (id, r) in &frames {
            let min = minimums[id];
            assert!(r.w >= min.0 - 0.5 && r.h >= min.1 - 0.5);
            assert!(r.contained_in(SCREEN).approx_eq(r));
            for (other, s) in &frames {
                if id == other {
                    continue;
                }
                let overlap = (r.max_x().min(s.max_x()) - r.x.max(s.x)).max(0.0)
                    * (r.max_y().min(s.max_y()) - r.y.max(s.y)).max(0.0);
                assert!(overlap < 0.5);
            }
        }
    }

    #[test]
    fn real_mac_app_minimums_regroup_without_floating_or_overlap() {
        let area = Rect::new(0.0, 30.0, 1792.0, 1090.0);
        let gaps = Gaps {
            inner: 5.0,
            outer: 10.0,
        };
        let minimums = HashMap::from([
            (1, (300.0, 200.0)),
            (2, (500.0, 375.0)),
            (3, (500.0, 520.0)),
            (4, (484.0, 344.0)),
            (5, (480.0, 600.0)),
        ]);
        let mut t = Tree::new();
        for id in 1..=5 {
            t.insert(id, None, area, ForceSplit::Second, 0.5);
        }
        assert!(t.constrain(area, gaps, &minimums));
        let frames = t.layout(area, gaps);
        for (id, r) in &frames {
            let min = minimums[id];
            assert!(r.w >= min.0 - 0.5 && r.h >= min.1 - 0.5, "{id}: {r:?}");
            assert!(r.contained_in(area).approx_eq(r));
            for (other, s) in &frames {
                if id == other {
                    continue;
                }
                let overlap = (r.max_x().min(s.max_x()) - r.x.max(s.x)).max(0.0)
                    * (r.max_y().min(s.max_y()) - r.y.max(s.y)).max(0.0);
                assert!(overlap < 0.5);
            }
        }
    }

    #[test]
    fn dwindle_spiral_splits_longer_side() {
        let mut t = Tree::new();
        insert(&mut t, 1, None);
        insert(&mut t, 2, Some(1)); // landscape -> side by side
        assert!(frame(&t, 1).approx_eq(&Rect::new(0.0, 0.0, 800.0, 1000.0)));
        assert!(frame(&t, 2).approx_eq(&Rect::new(800.0, 0.0, 800.0, 1000.0)));
        insert(&mut t, 3, Some(2)); // 800x1000 portrait -> stacked
        assert!(frame(&t, 2).approx_eq(&Rect::new(800.0, 0.0, 800.0, 500.0)));
        assert!(frame(&t, 3).approx_eq(&Rect::new(800.0, 500.0, 800.0, 500.0)));
        insert(&mut t, 4, Some(3)); // 800x500 landscape -> side by side
        assert!(frame(&t, 4).approx_eq(&Rect::new(1200.0, 500.0, 400.0, 500.0)));
        assert_eq!(t.windows(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn force_split_first_places_new_window_left() {
        let mut t = Tree::new();
        t.insert(1, None, SCREEN, ForceSplit::First, 0.5);
        t.insert(2, Some(1), SCREEN, ForceSplit::First, 0.5);
        assert!(frame(&t, 2).approx_eq(&Rect::new(0.0, 0.0, 800.0, 1000.0)));
    }

    #[test]
    fn insert_without_target_uses_last_inserted() {
        let mut t = Tree::new();
        insert(&mut t, 1, None);
        insert(&mut t, 2, None);
        insert(&mut t, 3, None);
        assert!(frame(&t, 3).approx_eq(&Rect::new(800.0, 500.0, 800.0, 500.0)));
    }

    #[test]
    fn duplicate_insert_is_ignored() {
        let mut t = Tree::new();
        insert(&mut t, 1, None);
        insert(&mut t, 1, None);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn remove_promotes_sibling() {
        let mut t = Tree::new();
        insert(&mut t, 1, None);
        insert(&mut t, 2, Some(1));
        insert(&mut t, 3, Some(2));
        assert!(t.remove(2));
        assert!(frame(&t, 3).approx_eq(&Rect::new(800.0, 0.0, 800.0, 1000.0)));
        assert!(t.remove(1));
        assert!(frame(&t, 3).approx_eq(&SCREEN));
        assert!(t.remove(3));
        assert!(t.is_empty());
        assert!(!t.remove(3));
    }

    #[test]
    fn node_slots_are_reused() {
        let mut t = Tree::new();
        for round in 0..10 {
            insert(&mut t, 1, None);
            insert(&mut t, 2, None);
            t.remove(1);
            t.remove(2);
            assert!(t.is_empty(), "round {round}");
        }
        assert!(t.nodes.len() <= 3);
    }

    #[test]
    fn swap_exchanges_frames() {
        let mut t = Tree::new();
        insert(&mut t, 1, None);
        insert(&mut t, 2, Some(1));
        let (f1, f2) = (frame(&t, 1), frame(&t, 2));
        assert!(t.swap(1, 2));
        assert!(frame(&t, 1).approx_eq(&f2));
        assert!(frame(&t, 2).approx_eq(&f1));
        assert!(!t.swap(1, 99));
    }

    #[test]
    fn toggle_split_flips_parent_axis() {
        let mut t = Tree::new();
        insert(&mut t, 1, None);
        insert(&mut t, 2, Some(1));
        assert!(t.toggle_split(2));
        assert!(frame(&t, 1).approx_eq(&Rect::new(0.0, 0.0, 1600.0, 500.0)));
        assert!(frame(&t, 2).approx_eq(&Rect::new(0.0, 500.0, 1600.0, 500.0)));
        let mut single = Tree::new();
        insert(&mut single, 1, None);
        assert!(!single.toggle_split(1));
    }

    #[test]
    fn resize_adjusts_nearest_matching_split() {
        let mut t = Tree::new();
        insert(&mut t, 1, None);
        insert(&mut t, 2, Some(1));
        // Grow left window by 160 points: ratio 0.5 -> 0.6.
        assert!(t.resize(1, 160.0, 0.0, SCREEN));
        assert!(frame(&t, 1).approx_eq(&Rect::new(0.0, 0.0, 960.0, 1000.0)));
        // Growing the right window shrinks the left one.
        assert!(t.resize(2, 160.0, 0.0, SCREEN));
        assert!(frame(&t, 1).approx_eq(&Rect::new(0.0, 0.0, 800.0, 1000.0)));
        // No vertical split exists yet.
        assert!(!t.resize(1, 0.0, 100.0, SCREEN));
    }

    #[test]
    fn resize_is_clamped() {
        let mut t = Tree::new();
        insert(&mut t, 1, None);
        insert(&mut t, 2, Some(1));
        t.resize(1, 10_000.0, 0.0, SCREEN);
        assert!(frame(&t, 1).approx_eq(&Rect::new(0.0, 0.0, 1600.0 * MAX_RATIO, 1000.0)));
    }

    #[test]
    fn preserve_split_keeps_axis_after_neighbour_removed() {
        let mut t = Tree::new();
        insert(&mut t, 1, None);
        insert(&mut t, 2, Some(1));
        insert(&mut t, 3, Some(2)); // stacked split under 2
        t.remove(1); // [2/3] now fills the whole landscape screen
        assert!(frame(&t, 2).approx_eq(&Rect::new(0.0, 0.0, 1600.0, 500.0)));
    }

    #[test]
    fn gaps_match_hyprland_semantics() {
        let mut t = Tree::new();
        insert(&mut t, 1, None);
        insert(&mut t, 2, Some(1));
        let gaps = Gaps {
            inner: 5.0,
            outer: 10.0,
        };
        let frames = t.layout(SCREEN, gaps);
        let f1 = frames[0].1;
        let f2 = frames[1].1;
        assert!(f1.approx_eq(&Rect::new(10.0, 10.0, 790.0 - 5.0, 980.0)));
        assert!(f2.approx_eq(&Rect::new(800.0 + 5.0, 10.0, 785.0, 980.0)));
        assert!(
            (f2.x - f1.max_x() - 10.0).abs() < 0.01,
            "neighbours are 2*gaps_in apart"
        );
    }

    #[test]
    fn neighbor_lookup_uses_geometry() {
        let mut t = Tree::new();
        insert(&mut t, 1, None);
        insert(&mut t, 2, Some(1));
        insert(&mut t, 3, Some(2));
        assert_eq!(t.neighbor(1, Direction::Right, SCREEN), Some(2));
        assert_eq!(t.neighbor(3, Direction::Up, SCREEN), Some(2));
        assert_eq!(t.neighbor(3, Direction::Left, SCREEN), Some(1));
        assert_eq!(t.neighbor(1, Direction::Left, SCREEN), None);
    }
}
