//! Presentation shared by the native strip and Settings. Numbers use the same
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
