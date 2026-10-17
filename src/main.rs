//! tyler-tiler command line.
//!
//! `check-config`, `bindings` and `write-config` run on any host. With no
//! arguments the window manager starts, which is macOS-only.

use std::path::PathBuf;
use std::process::ExitCode;

use tyler_tiler::action::{Action, FullscreenMode, WorkspaceTarget};
use tyler_tiler::config::{self, Config};

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("-h") | Some("--help") | Some("help") => {
            print_help();
            ExitCode::SUCCESS
        }
        Some("check-config") => check_config(args.next()),
        Some("bindings") => print_bindings(args.next()),
        Some("shortcuts") => print_shortcuts(args.next()),
        Some("write-config") => write_config(args.next()),
        Some(other) => {
            eprintln!("unknown command `{other}`\n");
            print_help();
            ExitCode::from(2)
        }
        None => run_wm(),
    }
}

fn print_help() {
    println!(
        "\
tyler-tiler — automatic tiling for macOS, with macOS-friendly tiling shortcuts

Usage:
  tyler-tiler                     run the window manager (macOS)
  tyler-tiler check-config [FILE] validate a config, or the built-in default
  tyler-tiler bindings [FILE]     print the resolved keybindings
  tyler-tiler shortcuts [FILE]    print readable shortcuts with physical modifiers
  tyler-tiler write-config [FILE] write the default config (refuses to overwrite)

Config: $TYLER_CONFIG, else $XDG_CONFIG_HOME/tyler/tyler.toml,
        else ~/.config/tyler/tyler.toml
A user file is layered on top of the built-in macOS defaults."
    );
}

fn print_shortcuts(path: Option<String>) -> ExitCode {
    let config = match load(&resolve(path)) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let mut bindings = config.bindings.clone();
    bindings.sort_by_key(|b| b.chord.to_string());
    for binding in bindings {
        let title = match &binding.action {
            Action::Launch(app) => format!("Open {}", config.resolve_app(app)),
            Action::Close => "Close focused window".into(),
            Action::ToggleFloat => "Toggle floating / tiled".into(),
            Action::ToggleSplit => "Flip split direction".into(),
            Action::Fullscreen(FullscreenMode::Fullscreen) => "Fill display".into(),
            Action::Fullscreen(FullscreenMode::Maximized) => "Maximize in usable area".into(),
            Action::Focus(dir) | Action::Swap(dir) => {
                use tyler_tiler::geometry::Direction;
                let direction = match dir {
                    Direction::Left => "left",
                    Direction::Right => "right",
                    Direction::Up => "above",
                    Direction::Down => "below",
                };
                format!(
                    "{} window {direction}",
                    if matches!(binding.action, Action::Focus(_)) {
                        "Focus"
                    } else {
                        "Swap with"
                    }
                )
            }
            Action::Workspace(WorkspaceTarget::Index(n)) => format!("Switch to workspace {n}"),
            Action::Workspace(WorkspaceTarget::Previous) => "Return to last workspace".into(),
            Action::Workspace(WorkspaceTarget::Relative(n)) => format!(
                "{} occupied workspace",
                if *n > 0 { "Next" } else { "Previous" }
            ),
            Action::MoveToWorkspace { workspace, follow } => format!(
                "Move to workspace {workspace}{}",
                if *follow {
                    " and follow"
                } else {
                    " without following"
                }
            ),
            Action::Resize { dx, dy } => format!("Resize: width {dx:+}, height {dy:+} points"),
            Action::CycleNext { forward } => {
                format!("{} window", if *forward { "Next" } else { "Previous" })
            }
            Action::ToggleScratchpad => "Show / hide scratchpad".into(),
            Action::MoveToScratchpad => "Send window to scratchpad".into(),
            Action::Reload => "Reload configuration".into(),
            Action::Quit => "Quit Tyler and restore windows".into(),
            Action::Exec(cmd) => format!("Run {cmd}"),
            Action::WebApp(url) => format!("Open {url}"),
        };
        println!("{:<30} {title}", binding.chord.to_string());
    }
    ExitCode::SUCCESS
}

fn resolve(path: Option<String>) -> PathBuf {
    path.map(PathBuf::from).unwrap_or_else(config::default_path)
}

fn load(path: &std::path::Path) -> Result<Config, ExitCode> {
    match Config::load(path) {
        Ok(config) => Ok(config),
        Err(error) => {
            eprintln!("{error}");
            Err(ExitCode::from(1))
        }
    }
}

fn check_config(path: Option<String>) -> ExitCode {
    let path = resolve(path);
    match load(&path) {
        Ok(config) => {
            let source = if path.exists() {
                path.display().to_string()
            } else {
                format!("built-in defaults ({} not found)", path.display())
            };
            println!(
                "ok: {} bindings, super={} ({source})",
                config.bindings.len(),
                config.general.super_mods
            );
            ExitCode::SUCCESS
        }
        Err(code) => code,
    }
}

fn print_bindings(path: Option<String>) -> ExitCode {
    match load(&resolve(path)) {
        Ok(config) => {
            let mut bindings = config.bindings;
            bindings.sort_by(|a, b| a.spec.cmp(&b.spec));
            for binding in bindings {
                println!("{:<44} {}", binding.spec, format!("{:?}", binding.action));
            }
            ExitCode::SUCCESS
        }
        Err(code) => code,
    }
}

fn write_config(path: Option<String>) -> ExitCode {
    let path = resolve(path);
    if path.exists() {
        eprintln!("refusing to overwrite {}", path.display());
        return ExitCode::from(1);
    }
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            eprintln!("{}: {error}", parent.display());
            return ExitCode::from(1);
        }
    }
    if let Err(error) = std::fs::write(&path, config::DEFAULT_CONFIG) {
        eprintln!("{}: {error}", path.display());
        return ExitCode::from(1);
    }
    println!("wrote {}", path.display());
    ExitCode::SUCCESS
}

fn run_wm() -> ExitCode {
    #[cfg(target_os = "macos")]
    {
        tyler_tiler::platform::macos::run();
    }
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("tyler-tiler manages windows only on macOS. Try `tyler-tiler check-config` or `tyler-tiler bindings`.");
        ExitCode::from(1)
    }
}
