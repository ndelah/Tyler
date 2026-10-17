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
    let output = bin().arg("bindings").output().unwrap();
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
