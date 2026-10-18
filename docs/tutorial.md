# Tyler user guide

[Website & demo](https://tyler.delahousse.xyz/) · [Overview](../README.md) · [Report a problem](https://github.com/ndelah/Tyler/issues)

Tyler shows your native Mission Control desktops and optionally arranges windows on your main display. It runs in the menu bar, so you can close Terminal after installing it.

[Install](#1-install-or-update) · [Permissions](#3-grant-macos-permissions) · [Shortcuts](#5-complete-default-shortcut-reference) · [Customize](#7-customize-and-reload) · [Troubleshooting](#8-limitations-and-recovery)

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

### Native desktop backend (required)

From the checkout used to build Tyler, run:

```sh
sh scripts/setup-native-spaces.sh
```

For the installer checkout, use
`sh ~/.local/share/tyler/scripts/setup-native-spaces.sh` instead.
This installs yabai through Homebrew if needed, creates a float-only yabai
configuration when none exists, and starts its login service. Existing yabai
configuration is preserved. Enable **yabai** separately in **System Settings →
Privacy & Security → Accessibility**; if missing, add `/usr/local/bin/yabai`
(Intel) or `/opt/homebrew/bin/yabai` (Apple Silicon). Restart its service after
granting access: `yabai --restart-service`.

Yabai must use `layout float` so Tyler alone controls window placement. If you
already configure yabai, set `yabai -m config layout float` in your `yabairc`
and remove conflicting per-Space layout overrides. For an already running
service, `yabai -m config layout float` applies the setting immediately. Tyler
checks existing desktop layouts on startup and pauses if it detects competing
layouts later. It does not change an existing yabai configuration for you.

Create additional desktops with the **+** button in Mission Control. The first
ten regular desktops on the primary display map to Fn + 1…0; native fullscreen
Spaces are excluded. A number with no corresponding desktop reports an error
and leaves the current desktop alone. Disable **Automatically rearrange Spaces
based on most recent use** in Desktop & Dock → Mission Control if you want
stable desktop numbers. Reordering desktops keeps each layout attached to its
native Space ID.

Run `tyler-tiler check-spaces`, or inspect **Settings → Desktops**, to
check the native connection before tiling. There is no simulated-workspace
fallback. This integration uses yabai's queries, Space focus and window-to-Space
commands; it does not install its scripting addition or change SIP.
See [yabai's command reference](https://github.com/asmvik/yabai/blob/master/doc/yabai.asciidoc).

Quit the old Tyler version normally before starting this build so it restores
any windows it minimized. A previous force-quit may require restoring those
windows from the Dock once.

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

1. Open **Tyler.app**. Its menu bar icon provides **Settings** and **Quit Tyler**.
   There is no Dock icon or terminal window to leave open.
2. Open **System Settings → Privacy & Security → Accessibility** and enable
   **Tyler** and **yabai**. If Tyler is missing, click **+**, press Command-Shift-G,
   and enter `~/Applications/Tyler.app` (your personal Applications folder).
3. Tyler checks for the grant every two seconds and starts its worker when allowed.
   If macOS requires a relaunch, quit and reopen Tyler.app. Both features disabled
   in Settings leave the app idle without requesting Accessibility on startup.
4. If startup fails, read `~/Library/Logs/Tyler.log`, check permissions and run
   `~/.local/bin/tyler-tiler check-spaces`, then quit and reopen the app.
5. Press **Fn + K** to view current shortcuts. Closing that window leaves Tyler
   running. **Quit Tyler** restores explicit scratchpad windows before exiting.

Reopen Tyler.app from Finder to bring up Settings if its menu bar icon is hidden
by a busy menu bar. Updating an ad-hoc signed build can require macOS to grant
Accessibility again; check the installed app's permission if startup stops.

Tyler uses an active (filtering) keyboard tap, which requires **Accessibility**.
It does not require a separate Input Monitoring entry. See
[Apple's explanation of event-tap permissions](https://developer.apple.com/videos/play/wwdc2019/701/).

The bundle identifier is `com.ndelah.tyler`. Local builds are ad-hoc signed by default;
replacing a build can require renewed authorization. For the optional CLI,
macOS may attribute access to its hosting terminal instead: enable that terminal
under Accessibility, then fully quit/reopen it. Do not disable SIP, use sudo to
run Tyler, or edit the TCC database.

If you have an Apple signing certificate, keep using the same identity for
updates: `TYLER_SIGN_IDENTITY="your certificate name or SHA-1" sh scripts/build-app.sh`.
The installer also honors this environment variable. Switching from ad-hoc
signing to a certificate can require removing and re-adding Tyler in
Accessibility once. A development signature does not notarize the app.

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
SUPER + 2 selects your second Mission Control desktop (create it first); SUPER + 1 returns. SUPER + Shift + 2 moves
the focused window to workspace 2 and follows it.

**Stop safely:** choose **Quit Tyler** from its menu bar. In CLI mode, use
SUPER + Escape or Control-C. Tyler restores windows it minimized before exiting.
Command-W closes the focused window and may trigger its app's save prompt.
Command-Q retains its usual meaning: quit the foreground application.

### How the Omarchy-style layout works

Tyler uses Omarchy's default **dwindle** layout, with 10-point outer gaps and
10 points between neighbors. Each new window splits the **focused tile**, not
the whole display:

```text
One window          Second window       Third, while B is focused
┌─────────────┐     ┌──────┬──────┐      ┌──────┬──────┐
│             │     │      │      │      │      │  B   │
│      A      │  →  │  A   │  B   │   →  │  A   ├──────┤
│             │     │      │      │      │      │  C   │
└─────────────┘     └──────┴──────┘      └──────┴──────┘
```

1. Open a normal window. It fills the usable display, respecting the menu bar
   and Dock. Open a **new window** in another app; the original tile splits.
   Fn + B activates Safari, so it only adds a tile if Safari creates a window.
2. Click A or B, or use **Fn + arrows**, to select the tile that should split
   when you open the next window. Closing a window expands its sibling.
3. **Fn + Shift + arrows** swaps the selected tile with its neighbor.
   **Fn + J** flips its split between side-by-side and stacked.
4. **Fn + T** toggles floating.
5. **Fn + Shift + F** stretches and activates the selected window to fill the display
   without Apple’s fullscreen animation or a new Space. Repeat to restore its
   tile or floating frame. Other windows stay behind it; switching windows
   (Command-Tab, or Alt-Tab with a window switcher) brings them forward without
   shrinking the stretched window.
   **Fn + 2** switches workspace; **Fn + Shift + 2** moves the selected window
   there and follows it. **Fn + 1** returns.

Use **Tyler → Retile Windows** if an app changes its own position. If an app
repeatedly refuses a requested tile size, Tyler adjusts splits or reorganizes
the tiles into columns to honor its minimum size. If the layout still cannot fit,
it keeps the window floating with its top-left corner inside the display. Move it to a less
crowded workspace and press Fn + T to try tiling it again.

This follows Omarchy's default dwindle model and keyboard operations. macOS
still draws title bars, enforces each app's minimum window size, and manages
native Spaces. Tyler uses those same desktops; switching leaves windows open
and lets macOS control visibility. Only the explicit scratchpad uses minimization. Hyprland's
scrolling/master layouts, window groups, borders, pointer warping, and
modifier-drag behavior are not implemented. The Fn shortcuts remain adapted
to avoid the common macOS shortcuts listed below.

References: [Omarchy layout settings](https://github.com/basecamp/omarchy/blob/master/default/hypr/looknfeel.lua),
[Omarchy tiling bindings](https://github.com/basecamp/omarchy/blob/master/default/hypr/bindings/tiling.lua).

## 5. Complete default shortcut reference

Every chord below includes Fn/Globe. The notation matches
the configuration file. `code:10` through `code:19` mean the physical number-row
keys 1 through 0; 0 selects workspace 10. Letter and punctuation names refer to US
physical key positions, so the printed character can differ on AZERTY or other
layouts. Shift does not change which physical key is matched.

| Chord in configuration | Result |
| --- | --- |
| `SUPER + RETURN` | Open or activate Terminal |
| `SUPER + B` | Open or activate Safari |
| `SUPER + C` | Open or activate the configured browser (Safari by default) |
| `SUPER + O` | Open or activate Finder |
| `SUPER + F` | Open or activate Finder |
| `SUPER + K` | Show Tyler keybindings |
| `SUPER + T` | Toggle floating / tiled |
| `SUPER + J` | Flip the parent split axis |
| `SUPER + SHIFT + F` | Stretch and focus the active window to fill the display; repeat to restore |
| `SUPER + TAB` | Next occupied workspace |
| `SUPER + SHIFT + TAB` | Previous occupied workspace |
| `SUPER + GRAVE` | Return to the last workspace |
| `SUPER + BRACKETLEFT` | Next window |
| `SUPER + BRACKETRIGHT` | Previous window |
| `SUPER + APOSTROPHE` | Focus next monitor (`'`), wrapping in display arrangement order |
| `SUPER + BACKSLASH` | Focus previous monitor (`\`), wrapping in display arrangement order |
| `SUPER + V` | Show/hide scratchpad |
| `SUPER + SHIFT + V` | Send focused window to hidden scratchpad |
| `SUPER + R` | Reload configuration |
| `SUPER + ESCAPE` | Stop tiling and restore windows minimized by Tyler |
| `SUPER + LEFT` | Focus window left |
| `SUPER + SHIFT + LEFT` | Swap with window left |
| `SUPER + RIGHT` | Focus window right |
| `SUPER + SHIFT + RIGHT` | Swap with window right |
| `SUPER + UP` | Focus window above |
| `SUPER + SHIFT + UP` | Swap with window above |
| `SUPER + DOWN` | Focus window below |
| `SUPER + SHIFT + DOWN` | Swap with window below |
| `SUPER + MINUS` | Shrink window width by 100 points |
| `SUPER + EQUAL` | Expand window width by 100 points |
| `SUPER + SHIFT + MINUS` | Shrink window height by 100 points |
| `SUPER + SHIFT + EQUAL` | Expand window height by 100 points |
| `SUPER + ALT + MINUS` | Shrink window width a little (25 points) |
| `SUPER + ALT + EQUAL` | Expand window width a little (25 points) |
| `SUPER + SHIFT + ALT + MINUS` | Shrink window height a little (25 points) |
| `SUPER + SHIFT + ALT + EQUAL` | Expand window height a little (25 points) |
| `SUPER + CTRL + MINUS` | Shrink window width a lot (300 points) |
| `SUPER + CTRL + EQUAL` | Expand window width a lot (300 points) |
| `SUPER + CTRL + SHIFT + MINUS` | Shrink window height a lot (300 points) |
| `SUPER + CTRL + SHIFT + EQUAL` | Expand window height a lot (300 points) |
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

Directional focus and swaps require a window in that direction. Flipping a split
requires a suitable tiled neighbour. Scratchpad toggle needs a window sent there
first; the scratchpad is local to its native desktop, and switching desktops hides it again. Launch shortcuts activate
an app and do not necessarily create a new window if it is already running.
Monitor focus uses yabai and works across connected displays; automatic tiling
and Tyler's window cycle still apply to the main display. With one monitor,
monitor focus leaves focus unchanged. Fn + K opens the shortcuts window in Tyler.app.

Resize bindings follow [Omarchy's minus/equal modifier pattern](https://github.com/basecamp/omarchy/blob/master/default/hypr/bindings/tiling.lua):
Fn selects resize, Shift selects height, Option selects a 25-point step, and
Control selects a 300-point step. Without Option or Control the step is 100 points.
Minus always shrinks the chosen dimension and equal always expands it. The equal
key needs no Shift for horizontal resizing. Hold a chord to repeat it.
For tiled windows the shared split moves, resizing the neighbor too; a matching
split must exist. Layout limits and app minimum sizes can reduce the actual step.
Floating windows resize directly, with a 100-point minimum width and height.

## 6. Preserve Mac shortcuts and check personal conflicts

Standard Command-Q/W/F/T/S, Command-Tab, Spotlight, screenshots, and
Control-arrow Mission Control/Spaces remain available. Tyler avoids the common
macOS Fn + A/D/E/H/M/N/Q/S/W letter shortcuts. Fn + F opens Finder instead of
Apple’s fullscreen, and Fn + C opens the configured browser instead of Control Center.
Fn + Shift + F stretches the active window immediately; Fn + K shows keybindings.
Fn + O remains a Finder alias, and Fn + V toggles the scratchpad.
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
lists system combinations. If your keyboard does not emit Fn, use
`default_bindings = false` with a custom binding table and a suitable Super
modifier. A Super that includes Shift, Option, or Control overlaps the default
resize step modifiers and is rejected as a collision.

## 7. Customize and reload

### Desktop navigator and feature switches

Open **Settings → Desktops**. **Enable automatic tiling** and **Show desktop
navigator** are independent; press **Save Changes** to apply. With tiling off,
desktop navigation stays available and tiling shortcuts pass through to apps.
With both off, Tyler stays available for Settings and Quit.

The navigator appears immediately below the existing macOS menu bar on the main
display, clear of the notch and existing status icons. Desktop numbers match
Mission Control order and Tyler's configured shortcuts: Fn + 1…9 and Fn + 0 for
desktop 10 by default. Click a desktop to switch; the current one is highlighted.
Long rows scroll horizontally, and native fullscreen Spaces hide the strip.

Give each desktop a name or an emoji / SF Symbol (`sf:briefcase`, `sf:house`,
`sf:terminal`). An icon replaces its visible name; the tooltip retains the name
and actual shortcut. **Refresh desktops** updates the editor without discarding
unsaved labels. Names are saved against native Space IDs and survive Tyler restarts
and desktop reordering. Deleted/recreated desktops or new macOS Space IDs need
naming again. This does not change Mission Control's own desktop titles.

When tiling is enabled, Tyler reserves the strip's height in its own layout.
Navigator-only mode leaves freely positioned windows unchanged. The existing
yabai backend is still required; the navigator itself is implemented in Rust
using native AppKit controls, with no SketchyBar dependency.

### Configuration file

Edit `~/.config/tyler/tyler.toml` (or the file selected by `TYLER_CONFIG`, then
`XDG_CONFIG_HOME`). Files are layered over the defaults. Save to reload, or press
SUPER + R. Invalid hot reloads keep the previous working configuration; read
`~/Library/Logs/Tyler.log` for the error (or terminal output in CLI mode). Always validate before restarting:

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
"SUPER + B" = "unbind" # leave browser launching to the app or Dock
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

- Workspaces are **native macOS Mission Control desktops** controlled through
  yabai. Switching desktops does not minimize windows. Desktop membership is
  owned by macOS and survives Tyler restarts; Tyler's split ratios and floating
  overrides are currently held only for the running session.
- Only the primary display is tiled. Fullscreen Spaces, windows assigned to
  multiple desktops, and other displays are left alone. Tiling pauses while a
  native fullscreen Space is active on the primary display. Use Tyler's maximize
  shortcut to keep a window within a normal desktop.
- Yabai must be running, have Accessibility access, and use `layout float` on
  Tyler's display. A failed connection pauses tiling and native shortcuts;
  Read `~/Library/Logs/Tyler.log` and run `tyler-tiler check-spaces` for setup failures.
  Quit remains available. Restart yabai, then quit and reopen Tyler after startup failures.
- Some apps reject Accessibility moves/resizes or enforce minimum sizes. Tyler
  learns stable size rejections together and adapts splits before retrying.
  Failed AX requests are retried separately; windows that still cannot fit or
  accept moves become floating. Dialogs
  and configured floating apps keep their normal placement within the display.
- Secure Input in a terminal or password field can prevent global hotkeys from
  arriving. Leave the secure field or use **Quit Tyler** from its menu.
- Only explicit scratchpad windows can remain minimized after a crash or
  force-quit. Restore them through the Dock. Native desktop windows stay open.
  Prefer **Quit Tyler** so it restores the scratchpad automatically.
- This is a source-built app bundle with an optional CLI,
  not a Developer ID notarized or App Store release. “macOS-friendly” here describes shortcut and permission behavior;
  it is not a certification claim. The backend currently uses the undocumented
  `_AXUIElementGetWindow` symbol to match windows; see [design notes](design.md).

### Verify on your Mac

For the optional window animation prototype, display-clock diagnostics and
the smoothness acceptance check, see [motion setup and validation](motion.md).

After installation, use two disposable, resizable windows to try focus, swap,
float, split, both fullscreen modes, width/height resize, workspace moves,
workspace cycling, scratchpad, launchers, reload, close, and safe quit using the
table and menu. Confirm Fn + K opens the shortcut guide and the icon stays when its window closes. Try ordinary Command-F/T/S, Command-Tab, Spotlight, and screenshots too.
For workspace cycling, first create two native desktops in Mission Control and put windows in both. For resize, try both
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
