//! TOML configuration: an embedded Omarchy-equivalent default layered with
//! the user's `~/.config/tyler/tyler.toml`, compiled into a hotkey table, plus
//! a debounced file watcher for hot reload.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use serde::Deserialize;

use crate::action::Action;
use crate::keys::{parse_chord, Chord, Modifiers};
use crate::layout::{ForceSplit, Gaps};

pub const DEFAULT_CONFIG: &str = include_str!("../config/default.toml");
const UNBIND: &str = "unbind";
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, PartialEq)]
pub struct General {
    pub super_mods: Modifiers,
    pub gaps_in: f64,
    pub gaps_out: f64,
    pub border_size: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dwindle {
    pub preserve_split: bool,
    pub force_split: ForceSplit,
    pub split_ratio: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    /// Chord as written in the config (for display).
    pub spec: String,
    pub chord: Chord,
    pub action: Action,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub general: General,
    pub dwindle: Dwindle,
    pub apps: BTreeMap<String, String>,
    pub float_apps: Vec<String>,
    pub bindings: Vec<Binding>,
    keymap: HashMap<Chord, usize>,
}

impl Config {
    pub fn gaps(&self) -> Gaps {
        Gaps {
            inner: self.general.gaps_in,
            outer: self.general.gaps_out,
        }
    }

    pub fn action_for(&self, chord: Chord) -> Option<&Action> {
        self.keymap.get(&chord).map(|&i| &self.bindings[i].action)
    }

    /// Resolves an `[apps]` key to an application name; unknown keys are
    /// treated as literal app names.
    pub fn resolve_app<'a>(&'a self, key: &'a str) -> &'a str {
        self.apps.get(key).map(String::as_str).unwrap_or(key)
    }

    pub fn should_float(&self, app_name: &str) -> bool {
        self.float_apps
            .iter()
            .any(|a| a.eq_ignore_ascii_case(app_name))
    }

    /// The built-in Omarchy-equivalent configuration.
    pub fn builtin() -> Config {
        Config::from_layers(DEFAULT_CONFIG, None).expect("embedded default config is valid")
    }

    /// Loads defaults plus the user file at `path` (missing file = defaults).
    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Config::from_layers(DEFAULT_CONFIG, Some(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::builtin()),
            Err(e) => Err(ConfigError {
                errors: vec![format!("{}: {e}", path.display())],
            }),
        }
    }

    /// Parses `base` and layers `user` on top.
    pub fn from_layers(base: &str, user: Option<&str>) -> Result<Config, ConfigError> {
        let base: RawConfig = toml::from_str(base).map_err(ConfigError::toml)?;
        let user: RawConfig = match user {
            Some(text) => toml::from_str(text).map_err(ConfigError::toml)?,
            None => RawConfig::default(),
        };
        let mut errors = Vec::new();

        let super_spec = user
            .general
            .super_
            .or(base.general.super_)
            .unwrap_or_else(|| "cmd".into());
        let super_mods = Modifiers::parse_combo(&super_spec)
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| {
                errors.push(format!(
                    "general.super: invalid modifier combo `{super_spec}`"
                ));
                Modifiers::CMD
            });
        let pick = |u: Option<f64>, b: Option<f64>, d: f64| u.or(b).unwrap_or(d).max(0.0);
        let general = General {
            super_mods,
            gaps_in: pick(user.general.gaps_in, base.general.gaps_in, 5.0),
            gaps_out: pick(user.general.gaps_out, base.general.gaps_out, 10.0),
            border_size: pick(user.general.border_size, base.general.border_size, 2.0),
        };
        if let Some(layout) = user
            .general
            .layout
            .as_deref()
            .or(base.general.layout.as_deref())
        {
            if layout != "dwindle" {
                errors.push(format!(
                    "general.layout: only \"dwindle\" is supported, got `{layout}`"
                ));
            }
        }

        let split_ratio = user
            .dwindle
            .split_ratio
            .or(base.dwindle.split_ratio)
            .unwrap_or(0.5);
        if !(0.1..=0.9).contains(&split_ratio) {
            errors.push(format!(
                "dwindle.split_ratio must be within 0.1..=0.9, got {split_ratio}"
            ));
        }
        let dwindle = Dwindle {
            preserve_split: user
                .dwindle
                .preserve_split
                .or(base.dwindle.preserve_split)
                .unwrap_or(true),
            force_split: ForceSplit::from_hyprland(
                user.dwindle
                    .force_split
                    .or(base.dwindle.force_split)
                    .unwrap_or(2),
            ),
            split_ratio,
        };

        let mut apps = base.apps;
        apps.extend(user.apps);
        let float_apps = user.rules.float.or(base.rules.float).unwrap_or_default();

        let use_defaults = user
            .general
            .default_bindings
            .or(base.general.default_bindings)
            .unwrap_or(true);
        let mut bindings: Vec<Binding> = Vec::new();
        let mut keymap: HashMap<Chord, usize> = HashMap::new();
        let layers: Vec<(&str, &BTreeMap<String, String>)> = if use_defaults {
            vec![("default", &base.bindings), ("user", &user.bindings)]
        } else {
            vec![("user", &user.bindings)]
        };
        for (layer, table) in layers {
            let mut seen_in_layer: HashMap<Chord, &str> = HashMap::new();
            for (spec, action_spec) in table {
                let chord = match parse_chord(spec, super_mods) {
                    Ok(c) => c,
                    Err(e) => {
                        errors.push(format!("[bindings] \"{spec}\": {e}"));
                        continue;
                    }
                };
                if let Some(prev) = seen_in_layer.insert(chord, spec) {
                    errors.push(format!(
                        "[bindings] \"{spec}\" and \"{prev}\" both resolve to {chord} in the {layer} layer \
                         (check general.super)"
                    ));
                    continue;
                }
                let action_spec = action_spec.trim();
                if action_spec.is_empty() || action_spec.eq_ignore_ascii_case(UNBIND) {
                    if let Some(i) = keymap.remove(&chord) {
                        bindings[i].spec.clear();
                    }
                    continue;
                }
                let action = match Action::parse(action_spec) {
                    Ok(a) => a,
                    Err(e) => {
                        errors.push(format!("[bindings] \"{spec}\" = \"{action_spec}\": {e}"));
                        continue;
                    }
                };
                let binding = Binding {
                    spec: spec.clone(),
                    chord,
                    action,
                };
                match keymap.get(&chord) {
                    Some(&i) => bindings[i] = binding,
                    None => {
                        keymap.insert(chord, bindings.len());
                        bindings.push(binding);
                    }
                }
            }
        }
        // Drop unbound tombstones and rebuild indices.
        bindings.retain(|b| !b.spec.is_empty());
        let keymap = bindings
            .iter()
            .enumerate()
            .map(|(i, b)| (b.chord, i))
            .collect();

        if errors.is_empty() {
            Ok(Config {
                general,
                dwindle,
                apps,
                float_apps,
                bindings,
                keymap,
            })
        } else {
            Err(ConfigError { errors })
        }
    }
}

/// All problems found in a config file; the previous config stays active
/// when a hot reload fails.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigError {
    pub errors: Vec<String>,
}

impl ConfigError {
    fn toml(e: toml::de::Error) -> ConfigError {
        ConfigError {
            errors: vec![e.to_string()],
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, e) in self.errors.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{e}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    general: RawGeneral,
    #[serde(default)]
    dwindle: RawDwindle,
    #[serde(default)]
    apps: BTreeMap<String, String>,
    #[serde(default)]
    rules: RawRules,
    #[serde(default)]
    bindings: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGeneral {
    #[serde(rename = "super")]
    super_: Option<String>,
    gaps_in: Option<f64>,
    gaps_out: Option<f64>,
    border_size: Option<f64>,
    layout: Option<String>,
    default_bindings: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDwindle {
    preserve_split: Option<bool>,
    force_split: Option<u8>,
    split_ratio: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRules {
    float: Option<Vec<String>>,
}

/// `$TYLER_CONFIG`, else `$XDG_CONFIG_HOME/tyler/tyler.toml`, else
/// `~/.config/tyler/tyler.toml`.
pub fn default_path() -> PathBuf {
    if let Some(p) = std::env::var_os("TYLER_CONFIG") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("tyler").join("tyler.toml")
}

/// Watches `path` and calls `on_change` (debounced) whenever it is written,
/// created or replaced. The parent directory is watched so that editors that
/// save via rename are handled. Keep the returned watcher alive.
pub fn watch<F>(path: &Path, on_change: F) -> notify::Result<notify::RecommendedWatcher>
where
    F: Fn() + Send + 'static,
{
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let file_name = path.file_name().map(|n| n.to_os_string());
    let (tx, rx) = mpsc::channel::<()>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        if event.kind.is_access() {
            return;
        }
        let relevant = event
            .paths
            .iter()
            .any(|p| p.file_name().map(|n| n.to_os_string()) == file_name);
        if relevant {
            let _ = tx.send(());
        }
    })?;
    watcher.watch(&dir, RecursiveMode::NonRecursive)?;
    std::thread::Builder::new()
        .name("tyler-config-watch".into())
        .spawn(move || {
            while rx.recv().is_ok() {
                while rx.recv_timeout(RELOAD_DEBOUNCE).is_ok() {}
                on_change();
            }
        })
        .map_err(|e| notify::Error::generic(&e.to_string()))?;
    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::WorkspaceTarget;
    use crate::geometry::Direction;
    use crate::keys::keycode_for;

    fn chord(spec: &str, sup: Modifiers) -> Chord {
        parse_chord(spec, sup).unwrap()
    }

    #[test]
    fn builtin_matches_omarchy_looknfeel() {
        let c = Config::builtin();
        assert_eq!(c.general.gaps_in, 5.0);
        assert_eq!(c.general.gaps_out, 10.0);
        assert_eq!(c.general.border_size, 2.0);
        assert!(c.dwindle.preserve_split);
        assert_eq!(c.dwindle.force_split, ForceSplit::Second);
        assert_eq!(c.general.super_mods, Modifiers::CMD);
    }

    #[test]
    fn builtin_has_omarchy_core_bindings() {
        let c = Config::builtin();
        let s = Modifiers::CMD;
        assert_eq!(c.action_for(chord("SUPER + W", s)), Some(&Action::Close));
        assert_eq!(
            c.action_for(chord("SUPER + T", s)),
            Some(&Action::ToggleFloat)
        );
        assert_eq!(
            c.action_for(chord("SUPER + J", s)),
            Some(&Action::ToggleSplit)
        );
        assert_eq!(
            c.action_for(chord("SUPER + LEFT", s)),
            Some(&Action::Focus(Direction::Left))
        );
        assert_eq!(
            c.action_for(chord("SUPER + SHIFT + UP", s)),
            Some(&Action::Swap(Direction::Up))
        );
        assert_eq!(
            c.action_for(chord("SUPER + RETURN", s)),
            Some(&Action::Launch("terminal".into()))
        );
        for n in 1..=10usize {
            let key = format!("code:{}", n + 9);
            assert_eq!(
                c.action_for(chord(&format!("SUPER + {key}"), s)),
                Some(&Action::Workspace(WorkspaceTarget::Index(n)))
            );
            assert_eq!(
                c.action_for(chord(&format!("SUPER + SHIFT + {key}"), s)),
                Some(&Action::MoveToWorkspace {
                    workspace: n,
                    follow: true
                })
            );
            assert_eq!(
                c.action_for(chord(&format!("SUPER + SHIFT + ALT + {key}"), s)),
                Some(&Action::MoveToWorkspace {
                    workspace: n,
                    follow: false
                })
            );
        }
        assert_eq!(
            c.action_for(chord("SUPER + MINUS", s)),
            Some(&Action::Resize {
                dx: -100.0,
                dy: 0.0
            })
        );
        assert_eq!(c.resolve_app("terminal"), "Ghostty");
        assert_eq!(c.resolve_app("Safari"), "Safari");
    }

    #[test]
    fn user_layer_overrides_rebinds_and_unbinds() {
        let user = r#"
            [general]
            gaps_in = 0
            [apps]
            terminal = "Alacritty"
            [bindings]
            "SUPER+W" = "unbind"
            "SUPER + Q" = "exec say hello"
            "SUPER + H" = "movefocus l"
        "#;
        let c = Config::from_layers(DEFAULT_CONFIG, Some(user)).unwrap();
        let s = Modifiers::CMD;
        assert_eq!(c.general.gaps_in, 0.0);
        assert_eq!(c.general.gaps_out, 10.0);
        assert_eq!(c.resolve_app("terminal"), "Alacritty");
        assert_eq!(c.action_for(chord("SUPER + W", s)), None);
        assert_eq!(
            c.action_for(chord("SUPER + Q", s)),
            Some(&Action::Exec("say hello".into()))
        );
        assert_eq!(
            c.action_for(chord("SUPER + H", s)),
            Some(&Action::Focus(Direction::Left))
        );
        assert!(c
            .bindings
            .iter()
            .all(|b| c.action_for(b.chord) == Some(&b.action)));
    }

    #[test]
    fn default_bindings_can_be_disabled() {
        let user = "[general]\ndefault_bindings = false\n[bindings]\n\"CTRL + ALT + RETURN\" = \"launch terminal\"\n";
        let c = Config::from_layers(DEFAULT_CONFIG, Some(user)).unwrap();
        assert_eq!(c.bindings.len(), 1);
        assert_eq!(c.bindings[0].chord.keycode, keycode_for("RETURN").unwrap());
    }

    #[test]
    fn super_remap_is_detected_as_collision() {
        // SUPER = alt makes "SUPER + ALT + F" collide with "SUPER + F".
        let err =
            Config::from_layers(DEFAULT_CONFIG, Some("[general]\nsuper = \"alt\"\n")).unwrap_err();
        assert!(
            err.errors.iter().any(|e| e.contains("both resolve to")),
            "{err}"
        );
    }

    #[test]
    fn super_can_be_hyper_style_combo() {
        let user = "[general]\nsuper = \"ctrl+cmd\"\ndefault_bindings = false\n[bindings]\n\"SUPER + W\" = \"killactive\"\n";
        let c = Config::from_layers(DEFAULT_CONFIG, Some(user)).unwrap();
        assert_eq!(c.bindings[0].chord.mods, Modifiers::CTRL | Modifiers::CMD);
    }

    #[test]
    fn reports_all_errors() {
        let user = r#"
            [dwindle]
            split_ratio = 2.0
            [bindings]
            "SUPER + NOPE" = "killactive"
            "SUPER + Z" = "frobnicate"
        "#;
        let err = Config::from_layers(DEFAULT_CONFIG, Some(user)).unwrap_err();
        assert_eq!(err.errors.len(), 3, "{err}");
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(Config::from_layers(DEFAULT_CONFIG, Some("[general]\ngapz = 3\n")).is_err());
        assert!(Config::from_layers(DEFAULT_CONFIG, Some("not toml = = =")).is_err());
    }

    #[test]
    fn missing_file_yields_builtin() {
        let c = Config::load(Path::new("/nonexistent/tyler.toml")).unwrap();
        assert_eq!(c.bindings.len(), Config::builtin().bindings.len());
    }

    #[test]
    fn float_rules() {
        let c = Config::builtin();
        assert!(c.should_float("system settings"));
        assert!(!c.should_float("Ghostty"));
    }

    #[test]
    fn watcher_fires_on_write() {
        let dir = std::env::temp_dir().join(format!("tyler-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tyler.toml");
        std::fs::write(&path, "").unwrap();
        let (tx, rx) = mpsc::channel();
        let _w = watch(&path, move || {
            let _ = tx.send(());
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(&path, "[general]\ngaps_in = 1\n").unwrap();
        let fired = rx.recv_timeout(Duration::from_secs(5)).is_ok();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(fired, "watcher did not fire");
    }
}
