# Tyler on macOS: installation and everyday use

Tyler automatically arranges windows in a splitting layout on your main display.
It runs in a terminal, uses macOS Accessibility APIs, and leaves SIP enabled.
There is no menu bar icon. These instructions describe the macOS defaults in
`config/default.toml`; the original Omarchy shortcuts are now an optional preset.

## 1. Install or update

Run in Terminal (or your preferred terminal application):

```sh
curl -fsSL https://raw.githubusercontent.com/ndelah/Tyler/main/scripts/install.sh | sh
```

The script installs Rust if needed, builds in `~/.local/share/tyler`, and installs
`~/.local/bin/tyler-tiler`. It does not build from your current directory.
If you prefer to inspect the installer first, download and read it before running it.
To build the checkout you already cloned instead:

```sh
cd ~/Workspace/Tyler
git pull --ff-only
cargo build --release
./target/release/tyler-tiler check-config
```

Use the same executable path consistently when granting permissions and running
Tyler. The examples below use the installer path. With a source build, substitute
`~/Workspace/Tyler/target/release/tyler-tiler`.

You can run the installed binary by its full path without changing your PATH.
Optionally add this line once to `~/.zshrc`, then open a new terminal:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

## 2. Migrate an existing configuration

An existing configuration overrides the new defaults. In particular,
`super = "cmd"` keeps the old conflicting Command shortcuts. Stop Tyler with
Control-C before migrating. To use the new defaults while preserving your file:

```sh
mv ~/.config/tyler/tyler.toml ~/.config/tyler/tyler.toml.backup
~/.local/bin/tyler-tiler write-config
```

Only run `mv` if the file exists; choose a different backup name if that name is
already taken. If you use `TYLER_CONFIG` or `XDG_CONFIG_HOME`, back up that file
instead. Reapply desired gaps, floating rules, and app names manually, leaving
`super = "ctrl+alt+cmd"`. Do not copy the entire old binding table back.

If no configuration exists, run `~/.local/bin/tyler-tiler write-config` now.
This also creates the directory needed for reliable configuration watching.

## 3. Grant macOS permissions

1. Run `~/.local/bin/tyler-tiler`. If Accessibility is missing, Tyler prints
   guidance and exits; macOS may also show a prompt.
2. Open **System Settings → Privacy & Security → Accessibility**.
3. Enable the application macOS associates with the request. For a command-line
   program this can be its hosting terminal: **Terminal**, **iTerm**, **Ghostty**,
   **Warp**, or an editor hosting an integrated terminal. Tyler need not appear
   as a separate application.
4. If the terminal is missing, click **+** and select it. For Apple Terminal,
   press **Command-Shift-G** in the file picker, enter
   `/System/Applications/Utilities/Terminal.app`, and choose **Open**.
5. Fully quit and reopen the terminal application, then run Tyler again.
6. If Tyler reports that the event tap cannot start, check **Privacy & Security →
   Input Monitoring** for that same application; enable it if requested and
   fully quit/reopen the application again. If macOS lists `tyler-tiler` itself,
   authorize that entry as well. Exact attribution can vary by launch method.

Granting permission to a terminal also permits other programs launched through
that terminal to use that access. A CLI executable is not an `.app` bundle.
Switching terminals, changing the executable location, or rebuilding can require
renewed authorization. If an old entry stops working, remove that specific entry,
add it again, and restart the host. Do not disable SIP, run Tyler with `sudo`, or
edit the TCC permissions database.

Apple's instructions: [Accessibility access](https://support.apple.com/guide/mac-help/allow-accessibility-apps-to-access-your-mac-mh43185/mac)
and [Input Monitoring](https://support.apple.com/guide/mac-help/control-access-to-input-monitoring-on-mac-mchl4cedafb6/mac).

## 4. First session

```sh
~/.local/bin/tyler-tiler check-config
~/.local/bin/tyler-tiler bindings
~/.local/bin/tyler-tiler
```

Keep that terminal open. Run only one instance of Tyler. Open two ordinary,
resizable windows on the main display; they should tile automatically.

Throughout this tutorial, **SUPER means hold Control + Option + Command**
(**⌃⌥⌘**) together, then press the final key. For example, SUPER + Left means
⌃⌥⌘←. Add Shift only where shown. Either side's modifier keys work.

Try SUPER + Left/Right to change focus, SUPER + Shift + Left/Right to swap,
SUPER + T to float the focused window, and the same shortcut to tile it again.
SUPER + 2 selects an empty workspace; SUPER + 1 returns. SUPER + Shift + 2 moves
the focused window to workspace 2 and follows it.

**Stop safely:** press SUPER + Escape from any app, or Control-C in Tyler's
terminal. Tyler restores parked windows to the screen before exiting.
SUPER + W closes the focused window and may trigger its app's save prompt.
Command-Q retains its usual meaning: quit the foreground application.

## 5. Complete default shortcut reference

Every chord below includes Control + Option + Command. The notation matches
the configuration file. `code:10` through `code:19` mean the physical number-row
keys 1 through 0; 0 selects workspace 10. `code:20` and `code:21` are the physical
US minus and equals positions. Letter and punctuation names also refer to US
physical key positions, so the printed character can differ on AZERTY or other
layouts. Shift does not change which physical key is matched.

| Chord in configuration | Result |
| --- | --- |
| `SUPER + RETURN` | Open or activate Terminal |
| `SUPER + B` | Open or activate Safari |
| `SUPER + E` | Open or activate Finder |
| `SUPER + W` | Close focused window |
| `SUPER + T` | Toggle floating / tiled |
| `SUPER + J` | Flip the parent split axis |
| `SUPER + F` | Toggle filling the display (Tyler layout, not a native fullscreen Space) |
| `SUPER + M` | Toggle filling usable area, respecting Dock/menu bar and outer gaps |
| `SUPER + TAB` | Next occupied workspace |
| `SUPER + SHIFT + TAB` | Previous occupied workspace |
| `SUPER + GRAVE` | Return to the last workspace |
| `SUPER + BRACKETRIGHT` | Next window |
| `SUPER + BRACKETLEFT` | Previous window |
| `SUPER + S` | Show/hide scratchpad |
| `SUPER + SHIFT + S` | Send focused window to hidden scratchpad |
| `SUPER + R` | Reload configuration |
| `SUPER + ESCAPE` | Quit Tyler and restore parked windows |
| `SUPER + LEFT` | Focus window left |
| `SUPER + SHIFT + LEFT` | Swap with window left |
| `SUPER + RIGHT` | Focus window right |
| `SUPER + SHIFT + RIGHT` | Swap with window right |
| `SUPER + UP` | Focus window above |
| `SUPER + SHIFT + UP` | Swap with window above |
| `SUPER + DOWN` | Focus window below |
| `SUPER + SHIFT + DOWN` | Swap with window below |
| `SUPER + code:10` | Switch to workspace 1 |
| `SUPER + SHIFT + code:10` | Move focused window to workspace 1 and follow |
| `SUPER + code:11` | Switch to workspace 2 |
| `SUPER + SHIFT + code:11` | Move focused window to workspace 2 and follow |
| `SUPER + code:12` | Switch to workspace 3 |
| `SUPER + SHIFT + code:12` | Move focused window to workspace 3 and follow |
| `SUPER + code:13` | Switch to workspace 4 |
| `SUPER + SHIFT + code:13` | Move focused window to workspace 4 and follow |
| `SUPER + code:14` | Switch to workspace 5 |
| `SUPER + SHIFT + code:14` | Move focused window to workspace 5 and follow |
| `SUPER + code:15` | Switch to workspace 6 |
| `SUPER + SHIFT + code:15` | Move focused window to workspace 6 and follow |
| `SUPER + code:16` | Switch to workspace 7 |
| `SUPER + SHIFT + code:16` | Move focused window to workspace 7 and follow |
| `SUPER + code:17` | Switch to workspace 8 |
| `SUPER + SHIFT + code:17` | Move focused window to workspace 8 and follow |
| `SUPER + code:18` | Switch to workspace 9 |
| `SUPER + SHIFT + code:18` | Move focused window to workspace 9 and follow |
| `SUPER + code:19` | Switch to workspace 10 |
| `SUPER + SHIFT + code:19` | Move focused window to workspace 10 and follow |
| `SUPER + code:20` | Decrease width by 100 points (tiled: adjust split) |
| `SUPER + code:21` | Increase width by 100 points (tiled: adjust split) |
| `SUPER + SHIFT + code:20` | Decrease height by 100 points (tiled: adjust split) |
| `SUPER + SHIFT + code:21` | Increase height by 100 points (tiled: adjust split) |
| `SUPER + COMMA` | Decrease width by 25 points (tiled: adjust split) |
| `SUPER + PERIOD` | Increase width by 25 points (tiled: adjust split) |
| `SUPER + SHIFT + COMMA` | Decrease height by 25 points (tiled: adjust split) |
| `SUPER + SHIFT + PERIOD` | Increase height by 25 points (tiled: adjust split) |

Directional focus and swaps require a window in that direction. Split/resize
actions require a suitable tiled neighbour; application minimum sizes and split
limits can prevent further resizing. Scratchpad toggle needs a window sent there
first; switching workspaces hides the scratchpad again. Launch shortcuts activate
an app and do not necessarily create a new window if it is already running.

## 6. Preserve Mac shortcuts and check personal conflicts

The defaults leave Command-Q/W/F/T/S, Command-Tab, Command-grave, Spotlight,
screenshots, Control-arrow Mission Control/Spaces, and ordinary editing/navigation
shortcuts untouched. Tyler matches exact modifier combinations; adding the three
SUPER modifiers creates separate chords. It does not bind the Control-Option
VoiceOver prefix by itself. The default launchers use built-in macOS apps.

No global shortcut scheme can guarantee compatibility with every app, custom
macOS setting, keyboard remapper, or accessibility configuration. In particular,
check Rectangle/Magnet, Raycast/Alfred, BetterTouchTool, Karabiner, and VoiceOver
if you use them. Three modifiers plus an arrow can already be assigned by another
window manager. Review **System Settings → Keyboard → Keyboard Shortcuts** and
the other utility's shortcut settings. Stop the other tiler while trying Tyler.
If a chord overlaps an accessibility workflow, unbind or remap it in Tyler.

Apple's [Mac keyboard shortcut reference](https://support.apple.com/en-us/102650)
lists the standard combinations used as the compatibility baseline.

## 7. Customize and reload

Edit `~/.config/tyler/tyler.toml` (or the file selected by `TYLER_CONFIG`, then
`XDG_CONFIG_HOME`). Files are layered over the defaults. Save to reload, or press
SUPER + R. Invalid hot reloads keep the previous working configuration; read
Tyler's terminal output for the error. Always validate before restarting:

```sh
~/.local/bin/tyler-tiler check-config
~/.local/bin/tyler-tiler bindings
```

Example overrides (merge into existing sections; TOML cannot repeat a section):

```toml
[general]
super = "ctrl+alt+cmd"
gaps_in = 5
gaps_out = 10

[apps]
terminal = "Terminal"
browser = "Safari"
file_manager = "Finder"

[bindings]
"SUPER + W" = "unbind" # leave window closing exclusively to the app
"SUPER + SHIFT + B" = "webapp https://chatgpt.com"
"SUPER + SHIFT + D" = "movetoworkspacesilent 2" # send to 2 without following
```

To define everything yourself, set `default_bindings = false` in `[general]`.
Keep an `exit` binding or use Control-C. Modifier combinations that collapse
distinct defaults (for example, adding Shift to SUPER) are rejected as duplicates.
Avoid changing SUPER to Command alone: it intercepts normal application shortcuts.

The original optional `config/omarchy.toml` is available for users deliberately
choosing the old behavior. Copy it to your config path only after backing up your
current configuration; it captures Command-Q/W/F/T/S, Command-Tab, and more.
It also expects optional apps such as Ghostty and Google Chrome. Its shortcuts
are not the macOS defaults documented above.

## 8. Limitations and recovery

- Workspaces are Tyler's virtual groups, **not native macOS Spaces**. Inactive
  windows are parked at the main display's corner; they can still appear in
  Mission Control, the Dock, and Command-Tab. Activating one switches to its
  Tyler workspace. These groups are not persisted across restarts.
- Only the main display is tiled. A neighbouring monitor can expose parked
  windows. Begin with one display and avoid native fullscreen while using Tyler.
- Some apps reject Accessibility moves/resizes or enforce minimum sizes. Dialogs
  and configured floating apps are not expected to tile like regular windows.
- Secure Input in a terminal or password field can prevent global hotkeys from
  arriving. Leave the secure field or use Control-C in Tyler's terminal to stop.
- After a crash or force-quit, windows can remain parked. Activate them through
  the Dock/Command-Tab and drag them back. Prefer Tyler's exit shortcut or
  Control-C over force-quitting.
- This is a source-built CLI, not a signed/notarized app bundle or an App Store
  release. “macOS-friendly” here describes shortcut and permission behavior;
  it is not a certification claim. The backend currently uses the undocumented
  `_AXUIElementGetWindow` symbol to match windows; see [design notes](design.md).

### Verify on your Mac

After installation, use two disposable, resizable windows to try focus, swap,
float, split, both fullscreen modes, width/height resize, workspace moves,
workspace cycling, scratchpad, launchers, reload, close, and safe quit using the
table. Try ordinary Command-F/T/S, Command-Tab, Spotlight, and screenshots too.
For workspace cycling, first put windows in two workspaces. For resize, try both
split orientations. A hardware test is needed because automated parsing and
layout tests cannot establish TCC authorization, event delivery, or another
application's Accessibility behavior.

For diagnostic output, stop Tyler first and start it with:

```sh
RUST_LOG=debug ~/.local/bin/tyler-tiler
```

When reporting a problem, include macOS version, keyboard layout, launch terminal,
the output of `bindings`, and whether the action fails for all apps or one app.

## Automated verification

On an Intel Mac, `cargo test` passed 62 library tests and 4 CLI tests.
Native CoreGraphics events were constructed locally for every default binding
and passed into the callback to check dispatch, autorepeat, and modifier-first
release. No events were posted and no global event tap was installed by these
tests. Common Mac shortcuts pass through; all 53 default bindings are documented
and parse without collisions. Layout, workspace, scratchpad, and quit behavior
are covered at the command-model level. Live permissions and cross-app window
control remain interactive checks, as described above.
