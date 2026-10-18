//! Presentation shared by the native menu-bar controls and Settings. Numbers use the same
//! filtered Mission Control ordering as WorkspaceTarget::Index.
use crate::{
    action::{Action, WorkspaceTarget},
    config::Config,
    spaces::Snapshot,
};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Desktop {
    pub id: String,
    pub number: usize,
    pub name: String,
    pub icon: String,
    pub shortcut: String,
    pub selected: bool,
}

pub fn desktops(snapshot: &Snapshot, config: &Config) -> Vec<Desktop> {
    snapshot
        .desktops()
        .into_iter()
        .enumerate()
        .map(|(index, space)| {
            let number = index + 1;
            let id = space.id.to_string();
            let label = config
                .navigator
                .labels
                .get(&id)
                .cloned()
                .unwrap_or_default();
            let shortcut = config
                .bindings
                .iter()
                .filter(|binding| {
                    binding.action == Action::Workspace(WorkspaceTarget::Index(number))
                })
                .map(|binding| binding.chord.to_string())
                .collect::<Vec<_>>()
                .join(" / ");
            Desktop {
                id,
                number,
                name: label.name,
                icon: label.icon,
                shortcut,
                selected: space.visible,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn module_switches_release_only_their_own_shortcuts() {
        let mut config = Config::builtin();
        let desktop = Action::Workspace(WorkspaceTarget::Index(1));
        let tile = Action::ToggleFloat;
        for tiling in [false, true] {
            for navigator in [false, true] {
                config.tiling_enabled = tiling;
                config.navigator.enabled = navigator;
                assert_eq!(config.permits(&desktop), tiling || navigator);
                assert_eq!(config.permits(&tile), tiling);
            }
        }
    }

    #[test]
    fn validates_persistent_labels_and_preserves_default_shortcuts() {
        assert!(Config::from_layers(
            crate::config::DEFAULT_CONFIG,
            Some("[navigator.labels.bad]\nname = 'Work'")
        )
        .is_err());
        assert!(Config::from_layers(
            crate::config::DEFAULT_CONFIG,
            Some(&format!(
                "[navigator.labels.'77']\nname = '{}'",
                "x".repeat(49)
            ))
        )
        .is_err());
        let config = Config::from_layers(crate::config::DEFAULT_CONFIG, Some("[general]\ntiling_enabled = false\n[navigator]\nenabled = false\n[navigator.labels.'77']\nname = 'Work'\nicon = '💻'")).unwrap();
        assert!(!config.tiling_enabled && !config.navigator.enabled);
        assert_eq!(config.navigator.labels["77"].icon, "💻");
    }

    #[test]
    fn identities_follow_native_ids_while_numbers_and_shortcuts_follow_order() {
        let spaces = serde_json::from_str(r#"[
            {"id":77,"index":5,"display":1,"windows":[],"is-visible":false,"is-native-fullscreen":false,"type":"float"},
            {"id":44,"index":2,"display":1,"windows":[],"is-visible":true,"is-native-fullscreen":false,"type":"float"},
            {"id":55,"index":3,"display":1,"windows":[],"is-visible":false,"is-native-fullscreen":true,"type":"float"}
        ]"#).unwrap();
        let mut snapshot = Snapshot { spaces, display: 1 };
        let mut config = Config::builtin();
        config.navigator.labels.insert(
            "77".into(),
            crate::config::DesktopLabel {
                name: "Work".into(),
                icon: "sf:hammer".into(),
            },
        );
        let rows = desktops(&snapshot, &config);
        assert_eq!(
            (
                rows[1].number,
                rows[1].name.as_str(),
                rows[1].shortcut.as_str()
            ),
            (2, "Work", "FN + 2")
        );
        assert!(rows[0].selected);
        snapshot.spaces[0].index = 1;
        let rows = desktops(&snapshot, &config);
        assert_eq!(
            (rows[0].id.as_str(), rows[0].number, rows[0].name.as_str()),
            ("77", 1, "Work")
        );
        assert_eq!(rows[0].shortcut, "FN + 1");
    }
}

/// Pick the closest safe gap to the requested screen alignment. A busy bar
/// contracts the viewport; the scroll view retains access to every desktop.
pub fn menu_bar_frame(
    bar: crate::geometry::Rect,
    occupied: &[crate::geometry::Rect],
    desired_width: f64,
    placement: crate::config::NavigatorPlacement,
) -> Option<crate::geometry::Rect> {
    use crate::{config::NavigatorPlacement, geometry::Rect};
    let mut gaps = vec![(bar.x + 6.0, bar.max_x() - 6.0)];
    for item in occupied {
        if item.y >= bar.max_y() || item.max_y() <= bar.y || item.w <= 0.0 {
            continue;
        }
        let left = item.x - 6.0;
        let right = item.max_x() + 6.0;
        gaps = gaps
            .into_iter()
            .flat_map(|(a, b)| {
                if right <= a || left >= b {
                    vec![(a, b)]
                } else {
                    vec![(a, left.min(b)), (right.max(a), b)]
                }
            })
            .filter(|(a, b)| b - a >= 48.0)
            .collect();
    }
    let target = match placement {
        NavigatorPlacement::Left => bar.x,
        NavigatorPlacement::Center => bar.center().0,
        NavigatorPlacement::Right => bar.max_x(),
    };
    // Prefer a gap that fits the row. Otherwise use the largest safe viewport.
    let fits = gaps.iter().any(|(a, b)| b - a >= desired_width);
    gaps.into_iter()
        .filter(|(a, b)| !fits || b - a >= desired_width)
        .map(|(a, b)| {
            let width = desired_width.min(b - a);
            let x = match placement {
                NavigatorPlacement::Left => a,
                NavigatorPlacement::Center => (target - width / 2.0).clamp(a, b - width),
                NavigatorPlacement::Right => b - width,
            };
            Rect::new(x, bar.y, width, bar.h)
        })
        .min_by(|a, b| {
            let score = |r: &Rect| {
                if !fits {
                    -r.w * bar.w + (r.center().0 - target).abs()
                } else {
                    (r.center().0 - target).abs()
                }
            };
            score(a).total_cmp(&score(b))
        })
}

#[cfg(test)]
mod placement_tests {
    use super::*;
    use crate::{config::NavigatorPlacement::*, geometry::Rect};
    #[test]
    fn aligns_without_covering_menus_status_items_or_notch() {
        let bar = Rect::new(0.0, 0.0, 1440.0, 32.0);
        let occupied = [
            Rect::new(0.0, 0.0, 300.0, 32.0),
            Rect::new(680.0, 0.0, 80.0, 32.0),
            Rect::new(1200.0, 0.0, 240.0, 32.0),
        ];
        for placement in [Left, Center, Right] {
            let r = menu_bar_frame(bar, &occupied, 200.0, placement).unwrap();
            assert!(occupied
                .iter()
                .all(|o| r.max_x() <= o.x || r.x >= o.max_x()));
            assert_eq!(r.y, 0.0);
            assert_eq!(r.h, 32.0);
        }
        assert_eq!(
            menu_bar_frame(bar, &occupied, 200.0, Left).unwrap().x,
            306.0
        );
        assert_eq!(
            menu_bar_frame(bar, &occupied, 200.0, Right)
                .unwrap()
                .max_x(),
            1194.0
        );
        assert!(menu_bar_frame(bar, &[bar], 200.0, Center).is_none());
        assert!(menu_bar_frame(bar, &occupied, 1000.0, Center).unwrap().w < 1000.0);
    }
}
