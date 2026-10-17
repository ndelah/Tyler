# tyler-tiler

Automatic tiling for macOS, with Omarchy-inspired layouts and macOS-friendly shortcuts. Windows tile as they open. SIP stays on: this uses the Accessibility API and a global hotkey tap, not a private Spaces API.

## Install

On a Mac, this one command installs Rust if `cargo` is missing, clones or updates `~/.local/share/tyler`, builds there, and copies the binary to `~/.local/bin/tyler-tiler`. It does not use your current directory.

```sh
curl -fsSL https://raw.githubusercontent.com/ndelah/Tyler/main/scripts/install.sh | sh
```

`~/.local/bin` is not on the default macOS `PATH`. The script prints the line to add. Until you do that, start it with `~/.local/bin/tyler-tiler`.

## Tutorial and permissions

Follow the **[macOS tutorial](docs/tutorial.md)** for installation, terminal-app
permissions, migration, every shortcut, customization, and troubleshooting.

Tyler is a command-line program. macOS may attribute Accessibility permission to
your terminal app (Terminal, iTerm, etc.), so Tyler may not appear separately.
In System Settings → Privacy & Security → Accessibility, use **+** to add your
terminal if absent, enable it, then fully quit and reopen it. If the hotkey tap
cannot start, check Input Monitoring for the same app. Leave SIP enabled.

## Shortcuts

`SUPER` now means **Control + Option + Command** (⌃⌥⌘), preserving normal Mac
shortcuts. Use SUPER + arrows to focus, add Shift to swap, SUPER + 1…0 for
workspaces, SUPER + T to float, and SUPER + Escape to quit safely.
The [complete shortcut table](docs/tutorial.md#5-complete-default-shortcut-reference)
lists every default. Terminal, Safari, and Finder are the built-in app launchers.

**Upgrading:** an existing `super = "cmd"` or old binding table overrides the new
defaults. Follow the [migration steps](docs/tutorial.md#2-migrate-an-existing-configuration).
The conflicting original shortcuts remain an opt-in [legacy preset](config/omarchy.toml).
Custom shortcuts in macOS or other utilities may still overlap; check them before use.

## Start and quit

`tyler-tiler` runs in the foreground in that terminal. It tiles the main display only.

Quit with **Ctrl-C**. That moves windows that were parked off-screen back onto the display. Use SUPER + Escape to quit from another app; stop Tyler before closing its terminal. There is no menu-bar icon.

## Config

It runs with built-in macOS defaults and no config file. `SUPER` is Control + Option + Command.

To change anything, write a file and edit it. Saving reloads it.

```sh
tyler-tiler write-config
```

That creates `~/.config/tyler/tyler.toml` and will not overwrite an existing file. Override the path with `TYLER_CONFIG`. A key in your file replaces the default. Set a chord to `unbind` to drop it. Other useful commands: `tyler-tiler bindings` and `tyler-tiler check-config`.

## Building it yourself

Release builds are stripped, with link-time optimization. There is no GUI. From the fixed checkout:

```sh
cd ~/.local/share/tyler
cargo test
```

`cargo check --target aarch64-apple-darwin` typechecks the macOS backend on Linux. Linking that backend needs a Mac.

The layout, the binding table, and what is intentionally not implemented yet are in [docs/design.md](docs/design.md).
