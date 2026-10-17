# tyler-tiler

Automatic tiling for macOS, with an icon in the menu bar, a shortcuts window, and safe Quit. Windows tile as they open. SIP stays on: this uses the Accessibility API and a global hotkey tap, not a private Spaces API.

## Install

On a Mac, this command installs Rust if needed, builds Tyler.app, and installs it in `~/Applications`. It also installs the optional CLI in `~/.local/bin`. It does not use your current directory. Xcode Command Line Tools (`xcode-select --install`) are required for Swift and signing tools.

```sh
curl -fsSL https://raw.githubusercontent.com/ndelah/Tyler/main/scripts/install.sh | sh
```

Open **Tyler.app** from `~/Applications`. Use its menu bar icon to **View Shortcuts**, open permission settings, view logs, retry startup, or **Quit Tyler**. No terminal stays open. Quit Tyler before running the installer again to update it.

## Tutorial and permissions

Follow the **[macOS tutorial](docs/tutorial.md)** for installation, terminal-app
permissions, migration, every shortcut, customization, and troubleshooting.

Enable **Tyler** in System Settings → Privacy & Security → Accessibility.
The icon remains available while permission is missing. After enabling it,
Tyler retries startup automatically. If the worker cannot start, use **View Log**,
check Input Monitoring for Tyler, then use **Retry Starting Tyler** or reopen it.
When using the optional CLI, permission may belong to your terminal app instead.

## Shortcuts

`SUPER` now means **Fn/Globe**, wherever your keyboard exposes that key to macOS.
Use Fn + B for Safari, Fn + arrows to focus, add Shift to swap, Fn + 1…0 for
workspaces, and Fn + T to float. Fn + G fills the display, Fn + U maximizes,
Fn + O opens Finder, Fn + X closes a window, and Fn + V toggles the scratchpad.
Common macOS Fn-letter shortcuts are left available; Fn + arrows deliberately
controls Tyler instead of Home/End/Page Up/Page Down while Tyler is running.
The [complete shortcut table](docs/tutorial.md#5-complete-default-shortcut-reference)
lists every default. Terminal, Safari, and Finder are the built-in app launchers.

**Upgrading:** an existing `super = "cmd"` or `"ctrl+alt+cmd"` or old binding table overrides the new
defaults. Follow the [migration steps](docs/tutorial.md#2-migrate-an-existing-configuration).
The conflicting original shortcuts remain an opt-in [legacy preset](config/omarchy.toml).
Custom shortcuts in macOS or other utilities may still overlap; check them before use.

## Start and quit

Open Tyler.app. Only the main display is tiled. Closing the shortcuts window leaves the menu bar app running.

Choose **Quit Tyler** from the menu bar to stop the worker and restore parked windows. The optional `tyler-tiler` CLI still runs in the foreground and stops with Control-C. Run one mode at a time; a second worker is rejected.

## Config

It runs with built-in macOS defaults and no config file. `SUPER` is Fn/Globe.

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
