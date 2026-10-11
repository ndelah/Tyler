# tyler-tiler

Automatic tiling for macOS, with [Omarchy](https://github.com/basecamp/omarchy)'s Hyprland keybindings. Windows tile as they open. SIP stays on: this uses the Accessibility API and a global hotkey tap, not a private Spaces API.

## Install

On a Mac, this one command installs Rust if `cargo` is missing, clones or updates `~/.local/share/tyler`, builds there, and copies the binary to `~/.local/bin/tyler-tiler`. It does not use your current directory.

```sh
curl -fsSL https://raw.githubusercontent.com/ndelah/Tyler/main/scripts/install.sh | sh
```

`~/.local/bin` is not on the default macOS `PATH`. The script prints the line to add. Until you do that, start it with `~/.local/bin/tyler-tiler`.

## Permissions

Leave SIP enabled. Do not run `csrutil disable`.

1. In a terminal, run `tyler-tiler`.
2. macOS asks for **Accessibility**. Turn tyler-tiler on under System Settings → Privacy & Security → Accessibility.
3. Also turn it on under **Input Monitoring** (the hotkey tap needs it).
4. Run `tyler-tiler` again.

The grant is tied to that binary. Replacing it with a newly built copy makes macOS ask again.

## Start and quit

`tyler-tiler` runs in the foreground in that terminal. It tiles the main display only.

Quit with **Ctrl-C**. That moves windows that were parked off-screen back onto the display. Closing the terminal does the same. There is no menu-bar icon.

## Config

It runs with built-in Omarchy-style defaults and no config file. `SUPER` is the Command key.

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
