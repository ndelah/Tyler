//! Native settings bridge. Validate the complete candidate before atomic replacement.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, process::ExitCode};
use tyler_tiler::{
    config::{Config, DEFAULT_CONFIG},
    keys::parse_chord,
    layout::ForceSplit,
};

#[derive(Serialize, Deserialize)]
struct Settings {
    tiling_enabled: bool,
    navigator: tyler_tiler::config::Navigator,
    #[serde(skip_deserializing)]
    desktops: Vec<tyler_tiler::navigator::Desktop>,
    #[serde(skip_deserializing)]
    desktop_error: Option<String>,
    source: Option<String>,
    apps: BTreeMap<String, String>,
    bindings: Vec<Binding>,
    layout: LayoutSettings,
    animations: AnimationSettings,
    border: BorderSettings,
}
#[derive(Serialize, Deserialize)]
struct LayoutSettings {
    gaps_in: f64,
    gaps_out: f64,
    force_split: u8,
    split_ratio: f64,
}
#[derive(Serialize, Deserialize)]
struct AnimationSettings {
    enabled: bool,
    duration_ms: u64,
}
#[derive(Serialize, Deserialize)]
struct BorderSettings {
    width: f64,
    color: String,
}
#[derive(Serialize, Deserialize)]
struct Binding {
    chord: String,
    action: String,
    #[serde(skip_deserializing)]
    title: String,
    #[serde(skip_deserializing)]
    group: String,
}

fn source(path: &Path) -> Result<Option<String>, Box<dyn std::error::Error>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn snapshot(path: &Path) -> Result<Settings, Box<dyn std::error::Error>> {
    let source = source(path)?;
    let config = Config::from_layers(DEFAULT_CONFIG, source.as_deref())?;
    let base: toml::Value = toml::from_str(DEFAULT_CONFIG)?;
    let user: toml::Value = toml::from_str(source.as_deref().unwrap_or(""))?;
    let mut bindings = Vec::new();
    for binding in &config.bindings {
        let action = user
            .get("bindings")
            .and_then(|t| t.get(&binding.spec))
            .or_else(|| base.get("bindings").and_then(|t| t.get(&binding.spec)))
            .and_then(toml::Value::as_str)
            .ok_or("Missing binding source")?;
        bindings.push(Binding {
            chord: binding.chord.to_string(),
            action: action.into(),
            title: crate::shortcuts::title(&binding.action, &config),
            group: crate::shortcuts::group(&binding.action).into(),
        });
    }
    bindings.sort_by(|a, b| a.chord.cmp(&b.chord));
    #[cfg(target_os = "macos")]
    let (desktops, desktop_error) = match tyler_tiler::platform::macos::navigator_desktops(&config)
    {
        Ok(desktops) => (desktops, None),
        Err(error) => (vec![], Some(error)),
    };
    #[cfg(not(target_os = "macos"))]
    let (desktops, desktop_error) = (vec![], Some("Native desktops require macOS".into()));
    Ok(Settings {
        tiling_enabled: config.tiling_enabled,
        navigator: config.navigator.clone(),
        desktops,
        desktop_error,
        source,
        apps: config.apps,
        bindings,
        layout: LayoutSettings {
            gaps_in: config.general.gaps_in,
            gaps_out: config.general.gaps_out,
            force_split: match config.dwindle.force_split {
                ForceSplit::Auto => 0,
                ForceSplit::First => 1,
                ForceSplit::Second => 2,
            },
            split_ratio: config.dwindle.split_ratio,
        },
        border: BorderSettings {
            width: config.general.border_size,
            color: config.general.border_color,
        },
        animations: AnimationSettings {
            enabled: config.animations.enabled,
            duration_ms: config.animations.duration_ms,
        },
    })
}

fn save(path: &Path, settings: Settings) -> Result<(), Box<dyn std::error::Error>> {
    if source(path)? != settings.source {
        return Err(
            "Configuration changed since Settings opened. Reopen Settings before saving.".into(),
        );
    }
    if !settings.layout.gaps_in.is_finite()
        || settings.layout.gaps_in < 0.0
        || !settings.layout.gaps_out.is_finite()
        || settings.layout.gaps_out < 0.0
    {
        return Err("Window gaps must be finite, nonnegative numbers".into());
    }
    if settings.layout.force_split > 2 {
        return Err("Choose a valid new-window position".into());
    }
    let current = Config::from_layers(DEFAULT_CONFIG, settings.source.as_deref())?;
    let mut document: toml::Table = toml::from_str(settings.source.as_deref().unwrap_or(""))?;
    let mut bindings = toml::Table::new();
    let mut seen = std::collections::HashSet::new();
    for binding in settings.bindings {
        let chord = parse_chord(&binding.chord, current.general.super_mods)?;
        if chord.mods.is_empty() {
            return Err(format!("{} needs at least one modifier", binding.chord).into());
        }
        if !seen.insert(chord) {
            return Err(format!("{} is assigned more than once", binding.chord).into());
        }
        if binding.action.trim().is_empty() || binding.action.trim().eq_ignore_ascii_case("unbind")
        {
            return Err("Choose an action, or remove the shortcut.".into());
        }
        bindings.insert(binding.chord, toml::Value::String(binding.action));
    }
    if settings.apps.values().any(|app| app.trim().is_empty()) {
        return Err("Application names cannot be empty".into());
    }
    document.insert(
        "navigator".into(),
        toml::Value::try_from(&settings.navigator)?,
    );
    document
        .entry("general")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or("Invalid general settings")?
        .insert(
            "tiling_enabled".into(),
            toml::Value::Boolean(settings.tiling_enabled),
        );
    document.insert("bindings".into(), toml::Value::Table(bindings));
    document.insert(
        "apps".into(),
        toml::Value::Table(
            settings
                .apps
                .into_iter()
                .map(|(k, v)| (k, toml::Value::String(v)))
                .collect(),
        ),
    );
    document
        .entry("general")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or("Invalid general settings")?
        .insert("default_bindings".into(), toml::Value::Boolean(false));
    let general = document.get_mut("general").unwrap().as_table_mut().unwrap();
    general.insert(
        "gaps_in".into(),
        toml::Value::Float(settings.layout.gaps_in),
    );
    general.insert(
        "gaps_out".into(),
        toml::Value::Float(settings.layout.gaps_out),
    );
    general.insert(
        "border_size".into(),
        toml::Value::Float(settings.border.width),
    );
    general.insert(
        "border_color".into(),
        toml::Value::String(settings.border.color),
    );
    // Merge the exposed controls so unrelated configuration remains intact.
    let dwindle = document
        .entry("dwindle")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or("Invalid layout settings")?;
    dwindle.insert(
        "force_split".into(),
        toml::Value::Integer(settings.layout.force_split.into()),
    );
    dwindle.insert(
        "split_ratio".into(),
        toml::Value::Float(settings.layout.split_ratio),
    );
    document.insert(
        "animations".into(),
        toml::Value::try_from(settings.animations)?,
    );
    let text = toml::to_string_pretty(&document)?;
    Config::from_layers(DEFAULT_CONFIG, Some(&text))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".tyler-settings-{}.tmp", std::process::id()));
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        // Recheck after validation and writing, before replacing the saved file.
        if source(path)? != settings.source {
            return Err("Configuration changed while saving".into());
        }
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

pub fn run(path: std::path::PathBuf, saving: bool) -> ExitCode {
    let result = if saving {
        serde_json::from_reader(std::io::stdin())
            .map_err(|e| e.into())
            .and_then(|settings| save(&path, settings))
    } else {
        snapshot(&path).and_then(|settings| {
            println!("{}", serde_json::to_string(&settings)?);
            Ok(())
        })
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}
