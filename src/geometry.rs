//! Platform-independent geometry: rectangles, directions and directional
//! neighbour search used by focus/swap commands.

/// Axis-aligned rectangle in screen points, origin at top-left (the same
/// convention as the macOS Accessibility API's global coordinates).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

const EPSILON: f64 = 0.5;

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.w
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.h
    }

    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// Keep a window inside the usable display, including after an app refuses
    /// to shrink. Oversized windows retain an accessible top-left corner.
    pub fn contained_in(self, area: Rect) -> Rect {
        let w = self.w.min(area.w).max(1.0);
        let h = self.h.min(area.h).max(1.0);
        Rect::new(
            self.x.clamp(area.x, area.max_x() - w),
            self.y.clamp(area.y, area.max_y() - h),
            w,
            h,
        )
    }

    /// Shrinks the rect by `amount` on every side, never below zero size.
    pub fn inset(&self, amount: f64) -> Rect {
        self.inset_sides(amount, amount, amount, amount)
    }

    pub fn inset_sides(&self, left: f64, top: f64, right: f64, bottom: f64) -> Rect {
        Rect {
            x: self.x + left,
            y: self.y + top,
            w: (self.w - left - right).max(0.0),
            h: (self.h - top - bottom).max(0.0),
        }
    }

    /// Splits along `axis`, giving the first part `ratio` of the extent.
    pub fn split(&self, axis: Axis, ratio: f64) -> (Rect, Rect) {
        match axis {
            Axis::Horizontal => {
                let first_w = self.w * ratio;
                (
                    Rect::new(self.x, self.y, first_w, self.h),
                    Rect::new(self.x + first_w, self.y, self.w - first_w, self.h),
                )
            }
            Axis::Vertical => {
                let first_h = self.h * ratio;
                (
                    Rect::new(self.x, self.y, self.w, first_h),
                    Rect::new(self.x, self.y + first_h, self.w, self.h - first_h),
                )
            }
        }
    }

    pub fn approx_eq(&self, other: &Rect) -> bool {
        (self.x - other.x).abs() < EPSILON
            && (self.y - other.y).abs() < EPSILON
            && (self.w - other.w).abs() < EPSILON
            && (self.h - other.h).abs() < EPSILON
    }
}

/// Split axis. `Horizontal` places children side by side (left | right),
/// `Vertical` stacks them (top / bottom).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    pub fn flipped(self) -> Axis {
        match self {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        }
    }

    /// Hyprland dwindle rule: split the longer side.
    pub fn for_rect(rect: &Rect) -> Axis {
        if rect.w >= rect.h {
            Axis::Horizontal
        } else {
            Axis::Vertical
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    /// Parses Hyprland direction arguments (`l`, `r`, `u`, `d`, or full words).
    pub fn parse(s: &str) -> Option<Direction> {
        match s.to_ascii_lowercase().as_str() {
            "l" | "left" => Some(Direction::Left),
            "r" | "right" => Some(Direction::Right),
            "u" | "t" | "up" | "top" => Some(Direction::Up),
            "d" | "b" | "down" | "bottom" => Some(Direction::Down),
            _ => None,
        }
    }
}

/// Finds the rect adjacent to `from` in `dir`.
///
/// Candidates must lie entirely on the requested side and overlap `from` on
/// the perpendicular axis; the closest edge wins, ties broken by the largest
/// overlap and then by centre distance. Returns the candidate's key.
pub fn find_neighbor<K: Copy>(from: &Rect, dir: Direction, candidates: &[(K, Rect)]) -> Option<K> {
    let (fcx, fcy) = from.center();
    candidates
        .iter()
        .filter_map(|(key, r)| {
            let (gap, overlap) = match dir {
                Direction::Left => (
                    from.x - r.max_x(),
                    overlap_1d(from.y, from.max_y(), r.y, r.max_y()),
                ),
                Direction::Right => (
                    r.x - from.max_x(),
                    overlap_1d(from.y, from.max_y(), r.y, r.max_y()),
                ),
                Direction::Up => (
                    from.y - r.max_y(),
                    overlap_1d(from.x, from.max_x(), r.x, r.max_x()),
                ),
                Direction::Down => (
                    r.y - from.max_y(),
                    overlap_1d(from.x, from.max_x(), r.x, r.max_x()),
                ),
            };
            if gap < -EPSILON || overlap <= 0.0 {
                return None;
            }
            let (cx, cy) = r.center();
            let center_dist = (cx - fcx).hypot(cy - fcy);
            Some((*key, gap.max(0.0), overlap, center_dist))
        })
        .min_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal))
                .then(a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal))
        })
        .map(|c| c.0)
}

fn overlap_1d(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_resize_still_keeps_whole_window_inside_display() {
        let usable = Rect::new(0.0, 30.0, 1792.0, 1090.0);
        let codex = Rect::new(901.0, 30.0, 1792.0, 1090.0);
        assert_eq!(codex.contained_in(usable), usable);
        let oversized = Rect::new(1791.0, 1119.0, 2000.0, 1400.0);
        assert_eq!(oversized.contained_in(usable), usable);
        let offset_display = Rect::new(-1600.0, 100.0, 1600.0, 900.0);
        assert_eq!(
            Rect::new(20.0, -20.0, 800.0, 500.0).contained_in(offset_display),
            Rect::new(-800.0, 100.0, 800.0, 500.0)
        );
    }

    #[test]
    fn split_horizontal_and_vertical() {
        let r = Rect::new(0.0, 0.0, 100.0, 50.0);
        let (a, b) = r.split(Axis::Horizontal, 0.5);
        assert!(a.approx_eq(&Rect::new(0.0, 0.0, 50.0, 50.0)));
        assert!(b.approx_eq(&Rect::new(50.0, 0.0, 50.0, 50.0)));
        let (a, b) = r.split(Axis::Vertical, 0.25);
        assert!(a.approx_eq(&Rect::new(0.0, 0.0, 100.0, 12.5)));
        assert!(b.approx_eq(&Rect::new(0.0, 12.5, 100.0, 37.5)));
    }

    #[test]
    fn axis_follows_aspect_ratio() {
        assert_eq!(
            Axis::for_rect(&Rect::new(0.0, 0.0, 200.0, 100.0)),
            Axis::Horizontal
        );
        assert_eq!(
            Axis::for_rect(&Rect::new(0.0, 0.0, 100.0, 200.0)),
            Axis::Vertical
        );
    }

    #[test]
    fn neighbor_prefers_closest_overlapping() {
        // [1][2]
        //    [3]
        let rects = [
            (1, Rect::new(0.0, 0.0, 50.0, 100.0)),
            (2, Rect::new(50.0, 0.0, 50.0, 50.0)),
            (3, Rect::new(50.0, 50.0, 50.0, 50.0)),
        ];
        assert_eq!(
            find_neighbor(&rects[0].1, Direction::Right, &rects[1..]),
            Some(2)
        );
        assert_eq!(
            find_neighbor(&rects[2].1, Direction::Left, &rects[..1]),
            Some(1)
        );
        assert_eq!(find_neighbor(&rects[1].1, Direction::Down, &rects), Some(3));
        assert_eq!(find_neighbor(&rects[1].1, Direction::Up, &rects), None);
    }

    #[test]
    fn direction_parse() {
        assert_eq!(Direction::parse("l"), Some(Direction::Left));
        assert_eq!(Direction::parse("DOWN"), Some(Direction::Down));
        assert_eq!(Direction::parse("x"), None);
    }
}
