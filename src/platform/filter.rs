//! Decides which macOS windows the window manager should take over.
//!
//! Pure data so it can be tested on Linux; the Accessibility scan in
//! `platform::macos` fills in the fields.

/// Owner names that are never tiled, even when they report a layer-0 window.
const IGNORED_OWNERS: &[&str] = &[
    "Window Server",
    "Dock",
    "SystemUIServer",
    "Control Center",
    "Notification Center",
    "Spotlight",
    "Wallpaper",
    "tyler-tiler",
];

/// A window as reported by `CGWindowListCopyWindowInfo` before Accessibility
/// enrichment.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowCandidate {
    pub layer: i64,
    pub owner: String,
    pub alpha: f64,
    pub minimized: bool,
}

impl WindowCandidate {
    /// Layer 0 is the normal application-window layer. Minimized windows are
    /// left alone so un-minimizing restores the user's placement.
    pub fn should_manage(&self) -> bool {
        self.layer == 0
            && self.alpha > 0.0
            && !self.minimized
            && !self.owner.is_empty()
            && !IGNORED_OWNERS
                .iter()
                .any(|name| self.owner.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(owner: &str, layer: i64) -> WindowCandidate {
        WindowCandidate {
            layer,
            owner: owner.into(),
            alpha: 1.0,
            minimized: false,
        }
    }

    #[test]
    fn manages_ordinary_app_windows() {
        assert!(candidate("Ghostty", 0).should_manage());
    }

    #[test]
    fn skips_system_layers_and_minimized() {
        assert!(!candidate("Ghostty", 25).should_manage());
        assert!(!candidate("Dock", 0).should_manage());
        assert!(!candidate("Window Server", 0).should_manage());
        let mut minimized = candidate("Ghostty", 0);
        minimized.minimized = true;
        assert!(!minimized.should_manage());
        let mut invisible = candidate("Ghostty", 0);
        invisible.alpha = 0.0;
        assert!(!invisible.should_manage());
    }
}
