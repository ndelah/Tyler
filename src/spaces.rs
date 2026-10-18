//! Mission Control snapshots. Native IDs survive desktop reordering; numbers do not.

use serde::Deserialize;

use crate::WindowId;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Space {
    pub id: u64,
    pub index: usize,
    pub display: usize,
    pub windows: Vec<WindowId>,
    #[serde(rename = "is-visible")]
    pub visible: bool,
    #[serde(rename = "is-native-fullscreen")]
    pub fullscreen: bool,
    #[serde(rename = "type")]
    pub layout: String,
}

#[derive(Debug, Deserialize)]
pub struct Display {
    pub id: u32,
    pub index: usize,
}

/// Cycle in yabai's display arrangement order, wrapping at either end.
pub fn adjacent_display(displays: &[Display], current: usize, forward: bool) -> Option<usize> {
    let mut order: Vec<_> = displays.iter().map(|d| d.index).collect();
    order.sort_unstable();
    order.dedup();
    let position = order.iter().position(|&index| index == current)?;
    let offset = if forward { 1 } else { order.len() - 1 };
    Some(order[(position + offset) % order.len()])
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    /// All Spaces, including other displays and fullscreen Spaces.
    pub spaces: Vec<Space>,
    pub display: usize,
}

/// Native transitions may finish after the command's IPC reply.
#[derive(Debug, Clone, Copy)]
pub struct Transition {
    pub space: u64,
    pub window: Option<WindowId>,
    pub follow: bool,
}

impl Transition {
    pub fn confirmed(&self, snapshot: &Snapshot) -> bool {
        self.window
            .is_none_or(|id| snapshot.desktop_for_window(id) == Some(self.space))
            && (!self.follow || snapshot.active() == Some(self.space))
    }
}

impl Snapshot {
    pub fn desktops(&self) -> Vec<&Space> {
        let mut spaces: Vec<_> = self
            .spaces
            .iter()
            .filter(|s| s.display == self.display && !s.fullscreen)
            .collect();
        spaces.sort_by_key(|s| s.index);
        spaces
    }

    pub fn active(&self) -> Option<u64> {
        self.desktops()
            .into_iter()
            .find(|s| s.visible)
            .map(|s| s.id)
    }

    /// Ambiguous/sticky windows belong to multiple Spaces and are left alone.
    pub fn desktop_for_window(&self, id: WindowId) -> Option<u64> {
        let mut matches = self.spaces.iter().filter(|s| s.windows.contains(&id));
        let space = matches.next()?;
        (matches.next().is_none() && space.display == self.display && !space.fullscreen)
            .then_some(space.id)
    }

    pub fn layout_conflict(&self) -> bool {
        self.desktops().iter().any(|s| s.layout != "float")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_cycle_wraps_in_arrangement_order_and_handles_disconnection() {
        let displays = [
            Display { id: 30, index: 3 },
            Display { id: 10, index: 1 },
            Display { id: 70, index: 7 },
        ];
        assert_eq!(adjacent_display(&displays, 3, true), Some(7));
        assert_eq!(adjacent_display(&displays, 3, false), Some(1));
        assert_eq!(adjacent_display(&displays, 7, true), Some(1));
        assert_eq!(adjacent_display(&displays, 1, false), Some(7));
        assert_eq!(adjacent_display(&displays, 2, true), None);
        assert_eq!(adjacent_display(&[], 1, false), None);
        assert_eq!(adjacent_display(&displays[..1], 3, true), Some(3));
        assert_eq!(adjacent_display(&displays[..1], 3, false), Some(3));
    }

    #[test]
    fn native_desktops_exclude_other_displays_fullscreen_and_sticky_windows() {
        let spaces = serde_json::from_str(r#"[
          {"id":101,"index":1,"display":2,"windows":[1],"is-visible":true,"is-native-fullscreen":false,"type":"float"},
          {"id":204,"index":4,"display":1,"windows":[4,9],"is-visible":false,"is-native-fullscreen":false,"type":"float"},
          {"id":202,"index":2,"display":1,"windows":[2,9],"is-visible":true,"is-native-fullscreen":false,"type":"float"},
          {"id":203,"index":3,"display":1,"windows":[3],"is-visible":false,"is-native-fullscreen":true,"type":"bsp"}
        ]"#).unwrap();
        let snapshot = Snapshot { spaces, display: 1 };
        assert_eq!(
            snapshot.desktops().iter().map(|s| s.id).collect::<Vec<_>>(),
            [202, 204]
        );
        assert_eq!(snapshot.active(), Some(202));
        assert_eq!(snapshot.desktop_for_window(2), Some(202));
        for id in [1, 3, 9] {
            assert_eq!(snapshot.desktop_for_window(id), None);
        }
        assert!(!snapshot.layout_conflict());
        let moved = Transition {
            space: 204,
            window: Some(4),
            follow: false,
        };
        assert!(moved.confirmed(&snapshot));
        assert!(!Transition {
            follow: true,
            ..moved
        }
        .confirmed(&snapshot));
        assert!(!Transition {
            window: Some(2),
            ..moved
        }
        .confirmed(&snapshot));
        let mut full = snapshot.clone();
        full.spaces[2].visible = false;
        full.spaces[3].visible = true;
        assert_eq!(full.active(), None);
    }
}
