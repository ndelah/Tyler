<p align="center">
  <a href="https://tyler.delahousse.xyz/">
    <img src="docs/assets/tyler-logo.png" alt="Tyler logo" width="160" height="160">
  </a>
</p>

<h1 align="center">Tyler</h1>
<p align="center"><strong>A place for every window.</strong></p>
<p align="center">A native desktop navigator and automatic window tiling for macOS.</p>
<p align="center">
  <a href="https://tyler.delahousse.xyz/">Website & interactive demo</a> ·
  <a href="docs/tutorial.md">User guide</a> ·
  <a href="https://github.com/ndelah/Tyler/issues">Report a problem</a>
</p>

Tyler shows your Mission Control desktops and can arrange windows as you open them. Enable desktop navigation and tiling independently in Settings. New windows split the focused tile; close one, and its neighbor takes the space. Use keyboard shortcuts to focus, swap, resize, or let a window float.

It lives in your menu bar, with a shortcut guide and a Quit button. No terminal needs to stay open.

## Get started

You need a Mac and **Xcode Command Line Tools**. If you haven't installed the tools, run `xcode-select --install` in Terminal and finish that installation first.

**1. Install Tyler.** Paste this into Terminal:

```sh
curl -fsSL https://raw.githubusercontent.com/ndelah/Tyler/main/scripts/install.sh | sh
```

The installer builds Tyler from source, installs Rust if needed, and places **Tyler.app** in your home Applications folder. You can [read the installer](scripts/install.sh) first.

**2. Set up native desktops.** Install and configure the existing yabai backend:

```sh
sh ~/.local/share/tyler/scripts/setup-native-spaces.sh
```

Open **Tyler.app** from `~/Applications`, then enable **Tyler** and **yabai** in **System Settings → Privacy & Security → Accessibility**. SIP stays enabled. Create extra desktops in Mission Control.

**3. Make it yours.** Open **Settings → Desktops** to name desktops, assign emoji or Apple symbols, and toggle tiling or the navigator. Press **Fn + K** for the shortcut guide. Reopen Tyler.app from Finder to access Settings if its menu bar icon is crowded out.

To update, choose **Quit Tyler**, then run the install command again. For setup help, see the [user guide](docs/tutorial.md).

## A few shortcuts to start

**Fn** is the Globe key on Apple keyboards. Hold it while pressing the other keys.

| Shortcut | What it does |
| --- | --- |
| Fn + arrow | Focus a window in that direction |
| Fn + Shift + arrow | Swap with a neighboring window |
| Fn + T | Switch between tiled and floating |
| Fn + J | Change a split from side-by-side to stacked, or back |
| Fn + Shift + F | Fill the display; press again to restore |
| Fn + 1…0 | Switch to Mission Control desktop 1…10 |
| Fn + Shift + 1…0 | Move a window to that workspace and follow it |
| Fn + C | Open your browser (Safari by default) |
| Fn + F | Open Finder |
| Fn + Return | Open Terminal |

[See all shortcuts →](docs/tutorial.md#5-complete-default-shortcut-reference)

## Make it yours

Use **Settings** to edit shortcuts, choose apps, adjust gaps and the active-window border, or name your desktops. **Save Changes** applies settings live. You can also edit `~/.config/tyler/tyler.toml`; saving reloads it. Tyler uses built-in defaults when no file exists.

[Configuration examples →](docs/tutorial.md#7-customize-and-reload)

## Desktop navigator

Tyler draws native AppKit desktop controls from Rust inside the macOS menu-bar
band on the main display. Choose **Left**, **Center**, or **Right** placement in
**Settings → Desktops**. Alignment uses unoccupied space around application menus,
status icons, and the camera housing; center placement moves beside a notch.
Click a desktop to switch; its number matches Tyler's shortcuts (Fn + 1…9,
Fn + 0 for desktop 10 by default). The current desktop is highlighted.
Crowded bars use a smaller horizontal scroll viewport with the current desktop
kept visible. If no safe space remains, or the system menu bar is hidden, the
controls hide; desktop shortcuts and reopening Tyler for Settings remain available.
Native fullscreen Spaces also hide the controls.

In **Settings → Desktops**, enable **automatic tiling** and **desktop navigator**
independently. Give each desktop a saved name or icon: an emoji or an SF Symbol
such as `sf:briefcase`, `sf:house`, or `sf:terminal`. Icons replace the visible name;
the name and actual configured shortcut remain in the tooltip. **Save Changes**
applies settings live. **Refresh desktops** updates the editor without discarding edits.
Reopen Tyler.app from Finder to access Settings even when its menu bar icon is crowded out.

Labels are saved against native Space IDs, following existing desktops when reordered
and across Tyler restarts. Removing/recreating desktops or macOS assigning new IDs
requires naming the new desktops again. No SketchyBar code or service is used;
the existing yabai backend still supplies native desktop information and switching.
Navigator-only mode releases tiling shortcuts and does not arrange windows.
The navigator reserves no extra height: windows use the normal macOS usable area.

For an offscreen native-control check without switching desktops or moving windows:

```sh
cargo run -- check-navigator /tmp/tyler-navigator.png
```

The optional PNG uses synthetic desktops and is a control preview, not a live screenshot.


## Good to know

- **Main display only.** Windows on other displays are left alone.
- **Native Mission Control desktops.** The existing yabai backend queries and switches desktops. Desktop switching leaves windows open; only explicit scratchpad actions minimize windows.
- **Quit from the menu bar.** This stops both features and restores explicit scratchpad windows. Closing the shortcut guide keeps Tyler running.
- **Some apps need more room.** Minimum window sizes and dialogs can limit tiling or resizing.
- **Built from source.** This is an ad-hoc signed app, rather than a notarized download. Accessibility access is required; System Integrity Protection stays enabled.

## Need a hand?

- [Setup, shortcuts, and troubleshooting](docs/tutorial.md)
- [Build from source and use the CLI](docs/development.md)
- [Technical design](docs/design.md)
- [Report a problem or suggest an improvement](https://github.com/ndelah/Tyler/issues)

Free and open source under the [MIT license](LICENSE).
