# Building and debugging Tyler

[Overview](../README.md) · [User guide](tutorial.md) · [Technical design](design.md) · [Website](https://tyler.delahousse.xyz/)

## Build from source

On a Mac, install Xcode Command Line Tools (`xcode-select --install`) and [Rust](https://www.rust-lang.org/tools/install). Then:

```sh
git clone https://github.com/ndelah/Tyler.git
cd Tyler
sh scripts/build-app.sh
open target/Tyler.app
```

Grant Accessibility access to this copy of `target/Tyler.app`. Keep using the same app path for builds and permissions. Replacing an ad-hoc signed build may require renewed access.

The app uses bundle identifier `com.ndelah.tyler`. The build script compiles the Rust worker and Swift menu bar launcher, then signs the bundle locally.

## Use the CLI

The installer also provides `~/.local/bin/tyler-tiler`. Run it by its full path, or add `~/.local/bin` to your shell's PATH.

Quit Tyler.app before starting the CLI:

```sh
~/.local/bin/tyler-tiler
```

It runs in the foreground. **Control-C** stops tiling and restores parked windows. macOS may attribute Accessibility permission to your terminal app; grant access to that app and restart the terminal if needed.

Useful commands:

```sh
~/.local/bin/tyler-tiler write-config
~/.local/bin/tyler-tiler check-config
~/.local/bin/tyler-tiler bindings
```

For a source build, replace `~/.local/bin/tyler-tiler` with `./target/release/tyler-tiler`.

## Debug logging

Choose **View Log…** in the menu bar app. For more detailed CLI output, quit the app first, then run:

```sh
RUST_LOG=debug ~/.local/bin/tyler-tiler
```

When reporting a problem, include the macOS version, keyboard layout, affected app, active bindings, and relevant log output.

## Validation

Run from your checkout:

```sh
cargo test
```

On Linux, `cargo check --target aarch64-apple-darwin` can typecheck the macOS backend once the Rust target is installed. Linking it requires a Mac.

Automated tests cover configuration, layout, workspace commands, and default binding handling using constructed keyboard events. They do not establish live Accessibility authorization, global keyboard delivery, or how another app responds to window moves.

For a manual check on a Mac, use two disposable windows to try focus, swap, float, split, resize, workspace moves, launchers, configuration reload, and safe quit. Check that the shortcut window opens and that closing it leaves the menu bar app running. Verify ordinary Mac shortcuts still work.

The backend uses the undocumented `_AXUIElementGetWindow` symbol to match windows. See the [design notes](design.md) for implementation details and limitations.
