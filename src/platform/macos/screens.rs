//! Main-display geometry.
//!
//! Accessibility and `CGWindow` bounds use a top-left origin. AppKit's
//! `NSScreen` uses a bottom-left origin anchored at the main screen, so
//! `visibleFrame` (which excludes the menu bar and the Dock) is converted
//! before it reaches the layout engine. Other displays are ignored until
//! multi-monitor support lands; see the plan.

use core_graphics::display::CGDisplay;
use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;

use crate::geometry::Rect;

/// `(full display bounds, usable area)` in Accessibility coordinates.
///
/// Falls back to the full `CGDisplay` bounds when AppKit is unavailable, in
/// which case windows can extend under the menu bar and the Dock.
pub fn main_screen() -> (Rect, Rect) {
    if let Some(mtm) = MainThreadMarker::new() {
        if let Some(screen) = NSScreen::screens(mtm).firstObject() {
            let frame = screen.frame();
            let visible = screen.visibleFrame();
            let height = frame.size.height;
            let to_ax = |origin_x, origin_y, width, height_of_rect| {
                Rect::new(
                    origin_x,
                    height - origin_y - height_of_rect,
                    width,
                    height_of_rect,
                )
            };
            return (
                to_ax(
                    frame.origin.x,
                    frame.origin.y,
                    frame.size.width,
                    frame.size.height,
                ),
                to_ax(
                    visible.origin.x,
                    visible.origin.y,
                    visible.size.width,
                    visible.size.height,
                ),
            );
        }
    }
    let bounds = CGDisplay::main().bounds();
    let full = Rect::new(
        bounds.origin.x,
        bounds.origin.y,
        bounds.size.width,
        bounds.size.height,
    );
    log::warn!("NSScreen unavailable; tiling will include the menu bar and Dock");
    (full, full)
}
