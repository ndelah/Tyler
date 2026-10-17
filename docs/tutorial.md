# Tyler on macOS: installation and everyday use

Tyler automatically arranges windows in a splitting layout on your main display.
Tyler.app runs in the menu bar without a terminal, uses macOS Accessibility APIs,
and leaves SIP enabled. These instructions describe the macOS defaults in
`config/default.toml`; the original Omarchy shortcuts are now an optional preset.

## 1. Install or update

Run in Terminal (or your preferred terminal application):

```sh
curl -fsSL https://raw.githubusercontent.com/ndelah/Tyler/main/scripts/install.sh | sh
```

The script installs Rust if needed, builds in `~/.local/share/tyler`, and installs
`~/Applications/Tyler.app` plus the optional `~/.local/bin/tyler-tiler`.
Open Tyler.app from Finder. It requires Xcode Command Line Tools for the Swift
launcher and signing tools; install them with `xcode-select --install` if needed.
Quit Tyler before running the installer to update an existing app. It does not build from your current directory.
If you prefer to inspect the installer first, download and read it before running it.
To build the checkout you already cloned instead:

```sh
cd ~/Workspace/Tyler
git pull --ff-only
sh scripts/build-app.sh
open target/Tyler.app
```

Use the same executable path consistently when granting permissions and running
Tyler. The examples below use the installer path. For a source-built app, grant
permission to `target/Tyler.app` instead; for source CLI commands, substitute
`~/Workspace/Tyler/target/release/tyler-tiler`.

You can run the installed binary by its full path without changing your PATH.
Optionally add this line once to `~/.zshrc`, then open a new terminal:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

## 2. Migrate an existing configuration

An existing configuration overrides the new defaults. In particular,
`super = "cmd"` or `"ctrl+alt+cmd"` keeps the previous modifier. Quit Tyler
from its menu bar (or use Control-C for the CLI) before migrating. To use the new defaults while preserving your file:

```sh
mv ~/.config/tyler/tyler.toml ~/.config/tyler/tyler.toml.backup
~/.local/bin/tyler-tiler write-config
```

Only run `mv` if the file exists; choose a different backup name if that name is
already taken. If you use `TYLER_CONFIG` or `XDG_CONFIG_HOME`, back up that file
instead. Reapply desired gaps, floating rules, and app names manually, leaving
`super = "fn"`. Do not copy the entire old binding table back.

If no configuration exists, run `~/.local/bin/tyler-tiler write-config` now.
This also creates the directory needed for reliable configuration watching.

## 3. Grant macOS permissions

1. Open **Tyler.app**. A tiling icon appears in the menu bar, even before access
   is granted. There is no Dock icon and no terminal window to leave open.
2. Open its menu and choose **Open Accessibility Settings…**. Enable **Tyler**.
   If missing, use **+** and select `~/Applications/Tyler.app`.
3. Tyler checks for the grant every two seconds and starts the tiling worker
   when it is allowed. If macOS requires a relaunch, quit and reopen Tyler.app.
4. If the menu reports **Tiling could not start**, use **View Log…**. If the event
   tap could not start, choose **Open Input Monitoring Settings…**, authorize
   Tyler if requested, then **Retry Starting Tyler** or quit and reopen the app.
5. Use **View Shortcuts…** for the current bindings and resolved Super modifier.
   Use **Quit Tyler** to restore parked windows and exit. Closing only the
   shortcuts window keeps the menu bar app running.

The bundle identifier is `com.ndelah.tyler`. Local builds are ad-hoc signed;
replacing a build can require renewed authorization. For the optional CLI,
macOS may attribute access to its hosting terminal instead: enable that terminal
under Accessibility, then fully quit/reopen it. Do not disable SIP, use sudo to
run Tyler, or edit the TCC database.

Apple's instructions: [Accessibility access](https://support.apple.com/guide/mac-help/allow-accessibility-apps-to-access-your-mac-mh43185/mac)
and [Input Monitoring](https://support.apple.com/guide/mac-help/control-access-to-input-monitoring-on-mac-mchl4cedafb6/mac).

## 4. First session

```sh
~/.local/bin/tyler-tiler check-config
~/.local/bin/tyler-tiler bindings
open ~/Applications/Tyler.app
```

No terminal needs to remain open. Do not start the CLI alongside the app. Run only one instance of Tyler. Open two ordinary,
resizable windows on the main display; they should tile automatically.

Throughout this tutorial, **SUPER means hold Fn/Globe**, then press the final
key. For example, SUPER + B means Fn + B. Add Shift only where shown.
macOS does not expose a separate right-Fn flag; either Fn/Globe key works if the
keyboard reports it. Some external keyboards handle Fn entirely in firmware;
those keys cannot be used as a macOS modifier without keyboard-specific remapping.

Try SUPER + Left/Right to change focus, SUPER + Shift + Left/Right to swap,
SUPER + T to float the focused window, and the same shortcut to tile it again.
SUPER + 2 selects an empty workspace; SUPER + 1 returns. SUPER + Shift + 2 moves
the focused window to workspace 2 and follows it.

**Stop safely:** choose **Quit Tyler** from its menu bar. In CLI mode, use
SUPER + Escape or Control-C. Tyler restores parked windows to the screen before exiting.
SUPER + X closes the focused window and may trigger its app's save prompt.
Command-Q retains its usual meaning: quit the foreground application.

## 5. Complete default shortcut reference

Every chord below includes Fn/Globe. The notation matches
the configuration file. `code:10` through `code:19` mean the physical number-row
keys 1 through 0; 0 selects workspace 10. `code:20` and `code:21` are the physical
US minus and equals positions. Letter and punctuation names also refer to US
physical key positions, so the printed character can differ on AZERTY or other
layouts. Shift does not change which physical key is matched.

| Chord in configuration | Result |
| --- | --- |
| `SUPER + RETURN` | Open or activate Terminal |
| `SUPER + B` | Open or activate Safari |
| `SUPER + O` | Open or activate Finder |
| `SUPER + X` | Close focused window |
| `SUPER + T` | Toggle floating / tiled |
| `SUPER + J` | Flip the parent split axis |
| `SUPER + G` | Toggle filling the display (Tyler layout, not a native fullscreen Space) |
| `SUPER + U` | Toggle filling usable area, respecting Dock/menu bar and outer gaps |
| `SUPER + TAB` | Next occupied workspace |
| `SUPER + SHIFT + TAB` | Previous occupied workspace |
| `SUPER + GRAVE` | Return to the last workspace |
| `SUPER + BRACKETRIGHT` | Next window |
| `SUPER + BRACKETLEFT` | Previous window |
| `SUPER + V` | Show/hide scratchpad |
| `SUPER + SHIFT + V` | Send focused window to hidden scratchpad |
| `SUPER + R` | Reload configuration |
| `SUPER + ESCAPE` | Stop tiling and restore parked windows |
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

Standard Command-Q/W/F/T/S, Command-Tab, Spotlight, screenshots, and
Control-arrow Mission Control/Spaces remain available. Tyler avoids the common
macOS Fn + A/C/D/E/F/H/M/N/Q/S/W letter shortcuts. It uses Fn + G for its own
fullscreen, Fn + U for maximize, Fn + O for Finder, and Fn + V for scratchpad.
Fn + arrows intentionally replaces their usual Home/End/Page Up/Page Down
behavior while Tyler runs. Fn + Return opens Terminal instead of keypad Enter.
Fn + Escape stops tiling; on some keyboards that combination is handled in
firmware and cannot reach an app. Use the menu bar Quit in that case.

No global scheme guarantees compatibility with custom shortcuts or remappers.
Check other window managers, Raycast/Alfred, BetterTouchTool, Karabiner, and
VoiceOver if installed. For Fn pressed by itself, **System Settings → Keyboard →
Press Globe/Fn key to → Do Nothing** can prevent input-source/emoji activation.
This does not disable all system Fn combinations. A keyboard's own firmware Fn
key may not be visible to macOS at all.

Apple's [Mac keyboard shortcut reference](https://support.apple.com/en-us/102650)
lists system combinations. You can return to a multi-key Super with
`super = "ctrl+alt+cmd"` in the config if your keyboard does not emit Fn.

## 7. Customize and reload

Edit `~/.config/tyler/tyler.toml` (or the file selected by `TYLER_CONFIG`, then
`XDG_CONFIG_HOME`). Files are layered over the defaults. Save to reload, or press
SUPER + R. Invalid hot reloads keep the previous working configuration; read
**View Log** from the menu for the error (or terminal output in CLI mode). Always validate before restarting:

```sh
~/.local/bin/tyler-tiler check-config
~/.local/bin/tyler-tiler bindings
```

Example overrides (merge into existing sections; TOML cannot repeat a section):

```toml
[general]
super = "fn"
gaps_in = 5
gaps_out = 10

[apps]
terminal = "Terminal"
browser = "Safari"
file_manager = "Finder"

[bindings]
"SUPER + X" = "unbind" # leave window closing exclusively to the app
"SUPER + SHIFT + B" = "webapp https://chatgpt.com"
"SUPER + SHIFT + D" = "movetoworkspacesilent 2" # send to 2 without following
```

To define everything yourself, set `default_bindings = false` in `[general]`.
Keep an `exit` binding or use the menu bar Quit (Control-C for CLI mode). Modifier combinations that collapse
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
  arriving. Leave the secure field or use **Quit Tyler** from its menu.
- After a crash or force-quit, windows can remain parked. Activate them through
  the Dock/Command-Tab and drag them back. Prefer Tyler's exit shortcut or
  Control-C over force-quitting.
- This is a source-built, ad-hoc signed app bundle with an optional CLI,
  not a Developer ID notarized or App Store release. “macOS-friendly” here describes shortcut and permission behavior;
  it is not a certification claim. The backend currently uses the undocumented
  `_AXUIElementGetWindow` symbol to match windows; see [design notes](design.md).

### Verify on your Mac

After installation, use two disposable, resizable windows to try focus, swap,
float, split, both fullscreen modes, width/height resize, workspace moves,
workspace cycling, scratchpad, launchers, reload, close, and safe quit using the
table and menu. Confirm View Shortcuts opens and the icon stays when its window closes. Try ordinary Command-F/T/S, Command-Tab, Spotlight, and screenshots too.
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

## Verification scope

Automated tests cover the config, layout, workspace command model, and every
default binding through locally constructed CoreGraphics events. They cover Fn
modifier parsing, translated navigation keys, and modifier-first key release.
These tests do not post global keyboard input. The app bundle is compiled and
its signature checked on macOS. Live TCC authorization and cross-app behavior
still require the interactive checks above. The menu's active state is shown
only after the worker logs that it has installed its global event tap.
