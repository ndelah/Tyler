# tyler-tiler

A small automatic tiling window manager for macOS. Windows tile in a Hyprland
**dwindle** (binary-space-partition) layout as they open, and the default
keybindings follow [Omarchy](https://github.com/basecamp/omarchy).

It does not disable SIP. On macOS it uses the Accessibility API to move
windows and a `CGEventTap` for global hotkeys. Workspaces are virtual: windows
on inactive workspaces are parked just off the main display, the same idea as
[AeroSpace](https://github.com/nikitabobko/AeroSpace), not the private Spaces API.

The layout engine, config parser and binding table are platform-independent
and tested on Linux. The macOS backend typechecks
(`cargo check --target aarch64-apple-darwin`) and has not been run on a Mac.
See [docs/design.md](docs/design.md).

## Build

Rust 1.85 or newer.

```sh
cargo test
cargo check --target aarch64-apple-darwin   # macOS backend; needs the target installed
cargo build --release                        # on a Mac, produces the WM
```

## Commands

```sh
tyler-tiler check-config [FILE]   # validate; missing file means the built-in defaults
tyler-tiler bindings [FILE]       # print the resolved chords
tyler-tiler write-config [FILE]   # write the default config; refuses to overwrite
tyler-tiler                       # run the window manager (macOS only)
```

Config is `$TYLER_CONFIG`, else `$XDG_CONFIG_HOME/tyler/tyler.toml`, else
`~/.config/tyler/tyler.toml`. A user file is layered on the built-in defaults:
set a key to override it, bind a chord to `unbind` to drop it, or set
`default_bindings = false` to start from an empty table. Saving the file
reloads it.

`SUPER` defaults to the Command key (`general.super = "cmd"`). Any modifier
combo works, for example `"ctrl+alt"`.

## Permission

System Settings → Privacy & Security → Accessibility, and Input Monitoring.
Both are required for the event tap. No SIP change.

## Layout of the source

| Path | What |
| --- | --- |
| `src/layout.rs` | Dwindle tree. Pure, unit tested. |
| `src/wm.rs` | Workspaces, focus, float, fullscreen, scratchpad. Returns commands. |
| `src/config.rs` | TOML layering, binding table, hot reload. |
| `src/keys.rs` | Hyprland chord syntax → macOS virtual key codes. |
| `src/platform/macos/` | Accessibility, event tap, `NSScreen`. Not executed in CI here. |
| `config/default.toml` | Omarchy-equivalent defaults, compiled into the binary. |
