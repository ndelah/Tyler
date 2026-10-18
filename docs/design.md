# Desktop navigator and independent modules (current)

The Rust worker owns a native AppKit nonactivating panel containing a segmented
control and horizontal scroll view. It runs AppKit's event loop, sharing the existing
CFRunLoop sources, keyboard tap, configuration watcher, and native desktop snapshot.
The panel occupies a 36-point strip below the macOS menu bar on the main display;
screen safe-area geometry keeps it outside the camera housing. The current desktop
stays selected and visible; fullscreen Spaces hide the strip. Standard system fonts,
materials, selection, and image rendering follow macOS appearance.

`src/navigator.rs` derives numbers and shortcut hints from the same filtered Space
order and resolved bindings used by Tyler. Labels in `[navigator.labels]` use native
Space IDs, preserving identities on reorder and app restart, but not when macOS
recreates IDs. The strip does not rename Mission Control desktops. Icons may be
emoji or `sf:` symbol names. Settings persists these through the existing validated,
atomic Rust configuration bridge; SwiftUI is retained for the existing Settings shell.
No SketchyBar source or runtime is used.

`general.tiling_enabled` and `navigator.enabled` are independent. Disabling tiling
cancels frame writes, restores explicit scratchpads, clears tracked layout state,
and releases tiling shortcuts. Desktop shortcuts use native focus requests directly
in navigator-only mode. Disabling the navigator removes the strip and releases its
reserved space in Tyler's layout. Both disabled leave an idle Settings host. The
existing yabai IPC backend remains responsible for querying and switching desktops.

Validation includes stable identity/numbering tests, independent shortcut capture,
settings round trips, and `check-navigator [PNG]`, an offscreen native-control test
using synthetic desktops. A preview does not establish live desktop switching,
notched-hardware behavior, or VoiceOver interaction.

# Native Mission Control integration (current)

Tyler now retains one layout per native Space ID, discovered through yabai's
local Unix socket. Desktop shortcuts emit native focus/move requests. The core
commits active desktop and window membership only from subsequent OS snapshots;
failed operations never pretend a window moved. Mission Control reordering
changes shortcut numbers without exchanging layouts. Trackpad/native desktop
switches reconcile through the same snapshots.

The backend uses public NSWorkspace notifications for desktop/app changes and
AXObserver notifications for window lifecycle, focus and geometry. A coalescing
CFRunLoop source drains shortcuts immediately after the event-tap callback
returns. A one-second reconciliation remains for apps that omit notifications;
pending native commands are confirmed on the 50 ms maintenance timer. Queries
use IPC directly, without spawning shell processes. Other displays,
fullscreen Spaces and windows assigned to multiple desktops are excluded.
Only the current normal desktop receives frame/focus commands. Native desktop
visibility never produces minimize/restore commands. The explicit, desktop-local
scratchpad still uses minimization and is restored on normal quit.

Yabai runs separately with `layout float`; Tyler owns tiling and input shortcuts.
The connection is required, with a bounded IPC timeout and explicit startup /
runtime failure reporting. SIP remains enabled; this integration does not load
yabai's optional scripting addition. `scripts/setup-native-spaces.sh` preserves
existing configuration; `check-spaces` diagnoses connectivity and layout conflicts.

Layout commands are coalesced per window before AX writes. Rejected sizes are
learned together after two stable observations, rather than repeatedly forcing
an impossible tile. Transport failures are retried separately and never become
minimum-size constraints. Frame writes pause briefly after an observed desktop
change; this is a notification/settling heuristic, not a public signal that a
Mission Control animation has finished.

The optional animation prototype samples cubic ease-out curves from a CoreVideo
display clock. Its callback only wakes the main loop; AX runs on the main thread.
Intermediate samples write only changed geometry, with final readback and
constraint validation. New targets replace old ones, desktop changes cancel
them, Reduce Motion disables them, and slow AX batches trigger immediate tiling.
This does not turn Tyler into a compositor or guarantee refresh-rate presentation.
See [motion validation](motion.md) for the opt-in configuration and limits.

The remainder of this file is historical and describes the former implementation.

> Historical design notes from the original Omarchy port. The binding map,
> defaults, environment, and validation counts below describe that initial tree.
> Current macOS defaults and operational instructions are in the
> [tutorial](tutorial.md) and [default configuration](../config/default.toml).
> The old mapping is opt-in via [omarchy.toml](../config/omarchy.toml).

# tyler-tiler — design and roadmap

A small, fast automatic tiling window manager for macOS that behaves like
i3/Hyprland and ships Omarchy's defaults. Milestone 1 (this tree) is the
layout engine, the config, the binding table, and the macOS Accessibility
backend. The backend typechecks and has never been executed.

Omarchy no longer uses `bindings.conf`. As of `basecamp/omarchy`
`bef30c1c50a88364a19648fd6d4e9d5fb99dc9b9` (11 Oct 2026) the live defaults are
Lua:

- `default/hypr/bindings/tiling.lua` — window, workspace, resize, scratchpad
- `default/hypr/bindings/applications.lua` — launchers and web apps
- `default/hypr/looknfeel.lua` — gaps, borders, dwindle
- `default/hypr/bindings/utilities.lua` — menus, screenshots, notifications
  (not ported; there is no Omarchy shell on macOS)

## Architecture

```
key chord ── CGEventTap ──┐
                          ▼
AX poll (windows, focus) ──►  Wm  ── commands ──► AXUIElement / open(1)
                          ▲    │
               TOML reload┘    └─ dwindle Tree per workspace
```

`Wm` is platform-independent. It owns ten workspaces, one dwindle tree each,
floating state, fullscreen, and a scratchpad. It never touches the OS. It
returns commands (`SetFrame`, `Focus`, `Close`, `LaunchApp`, `OpenWebApp`,
`Exec`). The macOS backend executes them.

The event-tap callback only enqueues a chord. Accessibility calls inside the
tap deadlock the event stream, so a CFRunLoop timer (20 Hz) drains chords and,
every fourth tick, reconciles the window list.

### Dwindle

Each workspace is a binary tree. Leaves are windows; internal nodes store an
axis and a ratio. A new window splits the target leaf on its longer side
(Hyprland dwindle) and lands second (right/bottom), which is Omarchy's
`force_split = 2`. Removing a window promotes its sibling, and the axis stays
on the surviving node (`preserve_split = true`). Ratios clamp to 0.1–0.9.

Gaps match Hyprland: `gaps_out` against the usable screen edges, `gaps_in` on
each side that faces another window, so neighbours are `2 * gaps_in` apart.
Omarchy's numbers are `gaps_in = 5`, `gaps_out = 10`, `border_size = 2`.
Borders are stored and not drawn: macOS has no compositor hook for them.

### Virtual workspaces

Same model as AeroSpace. Ten workspaces per main display. Windows on the
inactive ones are moved to the bottom-right corner of the display (top-left of
the window one point inside the corner, so a zero-size rejection is less
likely) and moved back when the workspace is shown. This does not use the
private Spaces SPI (`CGS*` / SkyLight), so SIP stays on.

Tradeoffs against real Spaces:

- Hidden windows keep running. They can still show up in Mission Control, the
  Dock, and Cmd-Tab.
- A second monitor may show the parking corner if it extends past the main
  display's bottom-right. Milestone 1 only tiles the main display and ignores
  windows whose centre is elsewhere.
- Apps that refuse AX moves (many games, some Electron builds, anything that
  draws its own window) will not tile. Electron is asked to expose its tree
  via `AXManualAccessibility` before we read it.
- Native fullscreen still creates a real Space and will fight the layout.
- Quitting via the `exit` binding, SIGINT, or SIGTERM moves parked windows
  back on screen. A crash leaves them in the corner until the user drags them
  back. Cmd-Tab onto a parked window is treated like Hyprland
  `focus_on_activate`: we switch to that window's workspace.
- There is no per-space wallpaper, menu bar, or window list.

### Permissions, and what we deliberately don't do

Required, and sufficient: the Accessibility TCC grant, plus Input Monitoring
for the event tap on recent macOS. Both are System Settings checkboxes.

Not used: SIP disable, `csrutil`, code injection, the SkyLight framework, the
private Spaces API, Dock injection. `_AXUIElementGetWindow` is not in the
public headers, but it is a long-stable symbol in ApplicationServices (the
same one AeroSpace and yabai call) and does not require a SIP change. Linking
it is a normal undefined symbol; `cargo check` does not link.

## Crate choices

Platform-independent:

| Crate | Why |
| --- | --- |
| `serde` + `toml` 1.0 | Config. 1.0 (not 1.1) so the parser stays on Rust 1.85. Unknown keys are errors. |
| `notify` | Hot reload. The parent directory is watched so atomic-save editors are seen. 150 ms debounce. A bad file keeps the previous config. |
| `log` + `env_logger` | `RUST_LOG`, default `info`. |

macOS only (`cfg(target_os = "macos")`):

| Crate | Why |
| --- | --- |
| `accessibility-sys` 0.2 | Raw `AXUIElement` (copy/set attribute, perform action, trust check). No safe wrapper exists that is worth the dependency. |
| `core-foundation` 0.10 / `core-graphics` 0.25 | CF types, `CGWindowListCopyWindowInfo`, `CGEventTap`, display bounds. |
| `objc2` 0.6 + `objc2-app-kit` (`NSScreen` only) | `visibleFrame`, converted from AppKit's bottom-left origin to AX's top-left origin, so tiles clear the menu bar and the Dock. |
| `libc` | SIGINT/SIGTERM set a flag the run-loop timer observes. |

`objc2` is not used for Accessibility. `accessibility-sys` already matches the
C API and is smaller. A higher-level crate such as `tauri` or `winit` would
own a window we don't want.

## Config

`config/default.toml` is compiled in. `~/.config/tyler/tyler.toml` (or
`$TYLER_CONFIG`) is layered on top.

```toml
[general]
super = "cmd"          # or "ctrl+alt", "ctrl+cmd", ...
gaps_in = 5
gaps_out = 10
default_bindings = true

[dwindle]
preserve_split = true
force_split = 2        # 2 = new window on the right/bottom (Omarchy)
split_ratio = 0.5

[apps]
terminal = "Ghostty"   # `open -a` name
browser = "Google Chrome"

[bindings]
"SUPER + H" = "movefocus l"
"SUPER + W" = "unbind"
```

Dispatchers follow Hyprland names: `movefocus l`, `swapwindow d`,
`workspace 3`, `workspace e+1`, `workspace previous`, `movetoworkspace 2`,
`movetoworkspacesilent 2`, `resizeactive -100 0`, `fullscreen`,
`fullscreen maximized`, `togglefloating`, `togglesplit`, `killactive`,
`togglespecialworkspace`, `cyclenext`, `exec …`, `launch <app-key>`,
`webapp https://…`.

Chords use Hyprland's `code:N` X11 keycodes for the number row (`code:10` is
1, `code:19` is 0, `code:20` is minus, `code:21` is equal) so a non-US layout
still resizes with the keys left of Backspace. Those map to macOS virtual key
codes (`kVK_*`), which are also physical.

## Omarchy binding map

`SUPER` is Command by default. 85 bindings. Arrows, not hjkl: in current
Omarchy, `SUPER + J` toggles the split, and focus is on the arrow keys. hjkl
focus is a one-line user override (`SUPER + H` = `movefocus l`, and so on);
it is not the default, because H/J/K/L are already J=split, K=keybinding
help, L=cycle layout.

| Omarchy chord | Omarchy action | tyler-tiler |
| --- | --- | --- |
| SUPER + arrows | focus l/r/u/d | `movefocus` |
| SUPER + SHIFT + arrows | swap l/r/u/d | `swapwindow` |
| SUPER + 1…0 | workspace 1…10 | `workspace N` (`code:10`–`code:19`) |
| SUPER + SHIFT + 1…0 | move window, follow | `movetoworkspace` |
| SUPER + SHIFT + ALT + 1…0 | move window, stay | `movetoworkspacesilent` |
| SUPER + TAB / SHIFT + TAB | next / previous occupied workspace | `workspace e+1` / `e-1` |
| SUPER + CTRL + TAB | previous workspace | `workspace previous` |
| SUPER + W, SUPER + Q | close | `killactive` (presses AX close) |
| SUPER + T | float | `togglefloating` |
| SUPER + F | fullscreen | covers the whole display |
| SUPER + ALT + F | maximized | fills the usable area, keeps outer gaps |
| SUPER + J | togglesplit | flips the parent axis |
| SUPER + minus / equal | resize ±100 | `resizeactive`, plus the SHIFT/ALT/CTRL step variants (25 and 300) |
| SUPER + S, SUPER + grave | scratchpad | show/hide floating scratchpad |
| SUPER + ALT + S, SUPER + SHIFT + grave | move to scratchpad | hidden until toggled; hides again on workspace change (`hide_special_on_workspace_change`) |
| ALT + TAB / ALT + SHIFT + TAB | cycle windows | `cyclenext` / `cyclenext prev` |
| SUPER + RETURN | terminal | `open -a Ghostty` |
| SUPER + SHIFT + RETURN, SUPER + SHIFT + B | browser | `open -a "Google Chrome"` |
| SUPER + SHIFT + ALT + B | private browser | Chrome `--incognito` |
| SUPER + SHIFT + F | files | Finder (Omarchy: Nautilus) |
| SUPER + SHIFT + N | editor | Ghostty running `nvim` |
| SUPER + ALT + RETURN | tmux | Ghostty running `tmux new-session -A -s Work` |
| SUPER + SHIFT + M / G / O / SLASH | music, chat, notes, passwords | Spotify, Signal, Obsidian, 1Password |
| SUPER + SHIFT + letter web apps | ChatGPT, Grok, HEY, YouTube, WhatsApp, Photos, Maps, X | Chrome `--app=<url>` |

Look and feel that is real in milestone 1: gaps 5/10, dwindle, preserve_split,
force_split 2, float rules for System Settings, Calculator, Archive Utility,
1Password. Not real yet: the 2 px gradient border, rounding, blur (Omarchy
has blur and shadow off anyway), animations, dimming.

Explicitly not bound, because the Omarchy feature has no macOS counterpart
yet:

- `SUPER + mouse` drag/resize, scroll to change workspace
- `SUPER + G` groups, `SUPER + P` pseudo, `SUPER + L` cycle layout (only
  dwindle exists)
- `SUPER + CTRL + F` tiled fullscreen, `SUPER + O` pop-out
- moving a workspace to another monitor
- `SUPER + SPACE` and the rest of `utilities.lua` (menus, screenshots, night
  light, notifications)

## Milestones

1. **Done in this tree.** Dwindle engine with unit tests. TOML config, layered
   defaults, hot reload, 85 Omarchy bindings. `Wm` command core with virtual
   workspaces, float, two fullscreen modes, scratchpad, directional focus and
   swap. macOS module: AX observe/move/focus/close, CGEventTap, NSScreen
   visible frame, `open -a` launchers. CLI: `check-config`, `bindings`,
   `write-config`.
2. **Bring-up on a real Mac.** Confirm the TCC prompt, the event tap, AX
   moves on AppKit and on Chrome after `AXManualAccessibility`, menu-bar
   clearance, and that parked windows are actually invisible. Fix whatever
   the typecheck could not tell us (see below).
3. **Input and displays.** `SUPER + drag` / `SUPER + right-drag`. Multi-monitor
   workspaces. Replace the 200 ms AX poll with `AXObserver` notifications.
   Left vs right Command, if we want Super to stop eating app shortcuts.
4. **Hyprland leftovers.** Groups, pseudo, layout toggle, per-app rules by
   title and subrole, a real launcher on `SUPER + SPACE` (Spotlight or
   Raycast).
5. **Chrome.** Optional menu-bar item (quit, reload, workspace number).
   Optional border overlays. No animation in the first cut: moving windows
   through AX is already a round trip, and faking Hyprland's curves on top
   would fight AppKit's own animation.

## What was verified, and what was not

On this Linux VM:

- `cargo test`: 56 library tests and 4 CLI tests, all passing. Layout,
  neighbour search, config layering, chord parsing, the `Wm` command core.
- `cargo check --target aarch64-apple-darwin`: the macOS backend typechecks
  with the `aarch64-apple-darwin` standard library. It was not linked and not
  run. There is no Mac here.

Untested on hardware, in the order it is likely to hurt:

1. Whether `CGEventTap` at session level actually arrives, and whether Input
   Monitoring is required in addition to Accessibility on the OS version you
   run.
2. Whether `AXPosition` / `AXSize` are honoured by AppKit, Chrome, Electron,
   and SwiftUI, and whether setting size before position is the right order
   for each.
3. Whether `_AXUIElementGetWindow` still returns the `CGWindowID` on the
   current OS. If it does not, we cannot match AX elements to CG windows.
4. The AppKit-to-AX coordinate conversion for `visibleFrame`, including a
   non-zero menu bar and a Dock on the side rather than the bottom.
5. Whether parking a window at the display corner hides it, or whether
   macOS clamps it back on screen (the whole workspace model depends on this).
6. Focus-follows-click racing the 500 ms grace period after a binding.
7. `NSScreen` on the main thread before `NSApplication` is started.
   `MainThreadMarker` may be `None` until AppKit is initialized, in which
   case we fall back to full `CGDisplay` bounds and tiles go under the menu
   bar. Milestone 2 should start AppKit if that happens.
8. SIGINT delivered during an AX call.

## Open questions

- **What is Super?** Command matches the physical key where Super sits, and
  Omarchy never combines Super with itself, so the chords stay unique. It
  also swallows Cmd-Q, Cmd-W, Cmd-F, Cmd-T, Cmd-1…9. `general.super` can be
  `ctrl+alt` today. Right Command only (leaving left Command for apps) needs
  the device-dependent flag bits, which `core-graphics` 0.25 does not expose.
  Worth doing in milestone 3 if Cmd turns out to be too aggressive.
- **Terminal.** Omarchy's menu offers Alacritty, Foot, Ghostty, and Kitty,
  with `xdg-terminal-exec` as the indirection. Ghostty is the default here
  because it is native on macOS. Override `[apps] terminal`.
- **Launcher.** `SUPER + SPACE` is the Omarchy menu. macOS already uses that
  chord for Spotlight, and we don't bind it. A later milestone can bind a
  launcher the user names.
- **Menu bar icon.** Deferred to milestone 5. A status item needs a running
  `NSApplication`, which interacts with the CFRunLoop and with
  `MainThreadMarker`, and none of that is testable here.
- **Distribution.** No GitHub credentials were available, so there is no
  repo and no pull request. The tree is the tarball next to this note.
  Codesign and a stable bundle id will matter for the TCC grant: an ad-hoc
  signature changes between builds and macOS will re-prompt.
