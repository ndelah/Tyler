//! Command-line behaviour that does not need macOS.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tyler-tiler"))
}

#[test]
fn readable_shortcuts_resolve_fn_and_app_names() {
    let absent =
        std::env::temp_dir().join(format!("tyler-default-absent-{}.toml", std::process::id()));
    let output = bin().arg("shortcuts").arg(absent).output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.lines()
            .any(|line| line.starts_with("FN + B ") && line.contains("Open Safari")),
        "{text}"
    );
    assert!(text.contains("Quit Tyler and restore windows"));
    for (chord, title) in [
        ("FN + K", "Show keybindings"),
        ("FN + F", "Open Finder"),
        ("FN + C", "Open Safari"),
        ("FN + BRACKETLEFT", "Next window"),
        ("FN + BRACKETRIGHT", "Previous window"),
        ("FN + APOSTROPHE", "Focus next monitor"),
        ("FN + BACKSLASH", "Focus previous monitor"),
        ("FN + EQUAL", "Expand window width (100 pt)"),
        ("ALT + FN + MINUS", "Shrink window width a little (25 pt)"),
        (
            "CTRL + SHIFT + FN + EQUAL",
            "Expand window height a lot (300 pt)",
        ),
    ] {
        assert!(
            text.lines()
                .any(|line| line.starts_with(&format!("{chord} ")) && line.ends_with(title)),
            "Missing {chord}: {text}"
        );
    }
}

#[test]
fn check_config_accepts_builtin_defaults() {
    let output = bin().arg("check-config").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("bindings"), "{text}");
}

#[test]
fn bindings_include_macos_terminal_chord() {
    let absent =
        std::env::temp_dir().join(format!("tyler-bindings-absent-{}.toml", std::process::id()));
    let output = bin().arg("bindings").arg(absent).output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("SUPER + RETURN"), "{text}");
    assert!(text.contains("Launch(\"terminal\")"), "{text}");
    assert!(
        text.contains("workspace 1") || text.contains("Index(1)"),
        "{text}"
    );
}

#[test]
fn check_config_rejects_unknown_dispatcher() {
    let path = std::env::temp_dir().join(format!("tyler-bad-{}.toml", std::process::id()));
    std::fs::write(&path, "[bindings]\n\"SUPER + Z\" = \"frobnicate\"\n").unwrap();
    let output = bin().arg("check-config").arg(&path).output().unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("frobnicate"));
}

#[test]
fn write_config_round_trips() {
    let path = std::env::temp_dir().join(format!("tyler-write-{}-tyler.toml", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let output = bin().arg("write-config").arg(&path).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let again = bin().arg("write-config").arg(&path).output().unwrap();
    assert!(!again.status.success(), "must refuse to overwrite");
    let check = bin().arg("check-config").arg(&path).output().unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(check.status.success());
}

#[test]
fn settings_round_trip_validates_and_preserves_other_options() {
    use std::io::Write;
    use std::process::Stdio;
    let dir = std::env::temp_dir().join(format!("tyler-settings-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tyler.toml");
    let original =
        "[general]\ngaps_out = 23\nsuper = 'cmd'\n[bindings]\n'SUPER + B' = 'launch browser'\n";
    std::fs::write(&path, original).unwrap();
    let export = || {
        let output = bin().arg("settings-json").arg(&path).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let save = |settings: &serde_json::Value| {
        let mut child = bin()
            .arg("save-settings")
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(settings).unwrap())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let mut settings = export();
    let browser = settings["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["action"] == "launch browser")
        .unwrap();
    assert_eq!(browser["title"], "Open Safari");
    assert_eq!(browser["group"], "Launch apps & commands");
    let focus = settings["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["action"] == "movefocus l")
        .unwrap();
    assert_eq!(focus["title"], "Focus window left");
    assert_eq!(focus["group"], "Focus & navigation");
    settings["tiling_enabled"] = false.into();
    settings["navigator"]["enabled"] = true.into();
    settings["navigator"]["labels"]["77"] =
        serde_json::json!({"name": "Development", "icon": "sf:terminal"});
    settings["border"]["width"] = 6.into();
    settings["border"]["color"] = "#123ABC".into();
    settings["layout"]["gaps_in"] = 8.into();
    settings["layout"]["gaps_out"] = 32.into();
    settings["layout"]["split_ratio"] = 0.65.into();
    settings["layout"]["force_split"] = 1.into();
    settings["animations"]["enabled"] = true.into();
    settings["animations"]["duration_ms"] = 220.into();
    settings["apps"]["browser"] = "/Applications/Google Chrome.app".into();
    let rows = settings["bindings"].as_array_mut().unwrap();
    let browser = rows
        .iter_mut()
        .find(|r| r["action"] == "launch browser")
        .unwrap();
    browser["chord"] = "CTRL + ALT + SHIFT + B".into();
    rows.push(serde_json::json!({"chord": "FN + A", "action": "launch /Applications/Visual Studio Code.app"}));
    rows.push(
        serde_json::json!({"chord": "FN + SHIFT + A", "action": "exec printf 'hello\\nworld'"}),
    );
    let output = save(&settings);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config = tyler_tiler::config::Config::load(&path).unwrap();
    assert!(!config.tiling_enabled);
    assert!(config.navigator.enabled);
    assert_eq!(config.navigator.labels["77"].name, "Development");
    assert_eq!(config.navigator.labels["77"].icon, "sf:terminal");
    assert_eq!(config.general.border_size, 6.0);
    assert_eq!(config.general.border_color, "#123ABC");
    assert_eq!(config.general.gaps_in, 8.0);
    assert_eq!(config.general.gaps_out, 32.0);
    assert_eq!(config.dwindle.split_ratio, 0.65);
    assert_eq!(
        config.dwindle.force_split,
        tyler_tiler::layout::ForceSplit::First
    );
    assert!(config.animations.enabled);
    assert_eq!(config.animations.duration_ms, 220);
    assert_eq!(config.general.super_mods.to_string(), "CMD");
    assert_eq!(
        config.resolve_app("browser"),
        "/Applications/Google Chrome.app"
    );
    let old = tyler_tiler::keys::parse_chord("SUPER + B", config.general.super_mods).unwrap();
    assert!(
        config.action_for(old).is_none(),
        "moving a default must remove the old chord"
    );
    assert!(config.bindings.iter().any(|b| b.spec == "FN + A"));
    assert!(
        !save(&settings).status.success(),
        "stale settings cannot overwrite changes"
    );

    let saved = std::fs::read_to_string(&path).unwrap();
    let mut duplicate = export();
    let rows = duplicate["bindings"].as_array_mut().unwrap();
    rows.push(rows[0].clone());
    assert!(!save(&duplicate).status.success());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
    let mut invalid = export();
    invalid["bindings"][0]["action"] = "not-an-action".into();
    assert!(!save(&invalid).status.success());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);

    for (section, key, value) in [
        ("border", "width", serde_json::json!(-1)),
        ("border", "width", serde_json::json!(21)),
        ("border", "color", serde_json::json!("green")),
        ("layout", "gaps_in", serde_json::json!(-1)),
        ("layout", "split_ratio", serde_json::json!(1.5)),
        ("layout", "force_split", serde_json::json!(3)),
        ("animations", "duration_ms", serde_json::json!(401)),
    ] {
        let mut invalid = export();
        invalid[section][key] = value;
        assert!(!save(&invalid).status.success(), "accepted invalid {key}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
    }

    std::fs::remove_file(&path).unwrap();
    let missing = export();
    assert!(missing["source"].is_null());
    assert!(save(&missing).status.success());
    std::fs::remove_dir_all(&dir).unwrap();
}
