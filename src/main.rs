//! tyler-tiler command line.
//!
//! `check-config`, `bindings` and `write-config` run on any host. With no
//! arguments the window manager starts, which is macOS-only.

mod settings;
mod shortcuts;

use std::path::PathBuf;
use std::process::ExitCode;

use tyler_tiler::config::{self, Config};

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("-h") | Some("--help") | Some("help") => {
            print_help();
            ExitCode::SUCCESS
        }
        Some("settings-json") => settings::run(resolve(args.next()), false),
        Some("save-settings") => settings::run(resolve(args.next()), true),
        Some("check-config") => check_config(args.next()),
        Some("check-motion") => {
            #[cfg(target_os = "macos")]
            match tyler_tiler::platform::macos::check_motion() {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::from(1)
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                eprintln!("The display clock requires macOS");
                ExitCode::from(1)
            }
        }
        Some("check-navigator") => {
            #[cfg(target_os = "macos")]
            match tyler_tiler::platform::macos::check_navigator(args.next().as_deref()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::from(1)
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                eprintln!("Native controls require macOS");
                ExitCode::from(1)
            }
        }
        Some("check-spaces") => {
            #[cfg(target_os = "macos")]
            match tyler_tiler::platform::macos::check_spaces() {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::from(1)
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                eprintln!("Native Spaces require macOS");
                ExitCode::from(1)
            }
        }
        Some("bindings") => print_bindings(args.next()),
        Some("shortcuts") => print_shortcuts(args.next(), false),
        Some("shortcuts-tsv") => print_shortcuts(args.next(), true),
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
  tyler-tiler check-spaces        check native Mission Control connection (macOS)
  tyler-tiler check-navigator [PNG] check native controls offscreen; optional synthetic preview
  tyler-tiler check-motion        probe the display clock without moving windows (macOS)
  tyler-tiler check-config [FILE] validate a config, or the built-in default
  tyler-tiler bindings [FILE]     print the resolved keybindings
  tyler-tiler shortcuts [FILE]    print readable shortcuts with physical modifiers
  tyler-tiler shortcuts-tsv [FILE] print grouped shortcuts for the native guide
  tyler-tiler settings-json [FILE] export editable settings as JSON
  tyler-tiler save-settings [FILE] validate and save settings JSON from stdin
  tyler-tiler write-config [FILE] write the default config (refuses to overwrite)

Config: $TYLER_CONFIG, else $XDG_CONFIG_HOME/tyler/tyler.toml,
        else ~/.config/tyler/tyler.toml
A user file is layered on top of the built-in macOS defaults."
    );
}

fn print_shortcuts(path: Option<String>, structured: bool) -> ExitCode {
    let config = match load(&resolve(path)) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let mut bindings = config.bindings.clone();
    bindings.sort_by_key(|b| b.chord.to_string());
    for binding in bindings {
        let title = shortcuts::title(&binding.action, &config);
        if structured {
            let group = shortcuts::group(&binding.action);
            // Keep every binding on one record, including custom shell commands.
            let title = title.replace(['\t', '\n', '\r'], " ");
            println!("{group}\t{}\t{title}", binding.chord);
        } else {
            println!("{:<30} {title}", binding.chord.to_string());
        }
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
                println!("{:<44} {:?}", binding.spec, binding.action);
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
