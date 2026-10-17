<p align="center">
  <a href="https://tyler.delahousse.xyz/">
    <img src="docs/assets/tyler-logo.png" alt="Tyler logo" width="160" height="160">
  </a>
</p>

<h1 align="center">Tyler</h1>
<p align="center"><strong>A place for every window.</strong></p>
<p align="center">Automatic window tiling for macOS. A little less arranging. A little more doing.</p>
<p align="center">
  <a href="https://tyler.delahousse.xyz/">Website & interactive demo</a> ·
  <a href="docs/tutorial.md">User guide</a> ·
  <a href="https://github.com/ndelah/Tyler/issues">Report a problem</a>
</p>

Tyler arranges windows as you open them. New windows split the focused tile; close one, and its neighbor takes the space. Use keyboard shortcuts to focus, swap, resize, or let a window float.

It lives in your menu bar, with a shortcut guide and a Quit button. No terminal needs to stay open.

## Get started

You need a Mac and **Xcode Command Line Tools**. If you haven't installed the tools, run `xcode-select --install` in Terminal and finish that installation first.

**1. Install Tyler.** Paste this into Terminal:

```sh
curl -fsSL https://raw.githubusercontent.com/ndelah/Tyler/main/scripts/install.sh | sh
```

The installer builds Tyler from source, installs Rust if needed, and places **Tyler.app** in your home Applications folder. You can [read the installer](scripts/install.sh) first.

**2. Give it permission.** Open **Tyler.app** from `~/Applications`. In its menu bar menu, choose **Open Accessibility Settings…** and enable **Tyler**. It starts tiling once access is granted.

**3. Open a few windows.** Start with two resizable windows on your main display. Use **View Shortcuts…** in Tyler's menu whenever you need a reminder.

To update, choose **Quit Tyler**, then run the install command again. For setup help, see the [user guide](docs/tutorial.md).

## A few shortcuts to start

**Fn** is the Globe key on Apple keyboards. Hold it while pressing the other keys.

| Shortcut | What it does |
| --- | --- |
| Fn + arrow | Focus a window in that direction |
| Fn + Shift + arrow | Swap with a neighboring window |
| Fn + T | Switch between tiled and floating |
| Fn + J | Change a split from side-by-side to stacked, or back |
| Fn + U | Fill the usable display; press again to restore |
| Fn + 1…0 | Switch to workspace 1…10 |
| Fn + Shift + 1…0 | Move a window to that workspace and follow it |
| Fn + B | Open your browser (Safari by default) |
| Fn + O | Open Finder |
| Fn + Return | Open Terminal |

[See all shortcuts →](docs/tutorial.md#5-complete-default-shortcut-reference)

## Make it yours

Choose your preferred apps, change shortcuts, or adjust the gaps between windows. Create a configuration file with:

```sh
~/.local/bin/tyler-tiler write-config
```

Edit `~/.config/tyler/tyler.toml`. Saving reloads your changes. Tyler works with its built-in defaults if you don't create a file.

[Configuration examples →](docs/tutorial.md#7-customize-and-reload)

## Good to know

- **Main display only.** Windows on other displays are left alone.
- **Workspaces are Tyler's own groups.** In this version, they aren't Mission Control desktops. Inactive windows are parked off to the side, and workspace groups reset when Tyler restarts.
- **Quit from the menu bar.** This stops tiling and brings parked windows back. Closing the shortcut guide keeps Tyler running.
- **Some apps need more room.** Minimum window sizes and dialogs can limit tiling or resizing.
- **Built from source.** This is an ad-hoc signed app, rather than a notarized download. Accessibility access is required; System Integrity Protection stays enabled.

## Need a hand?

- [Setup, shortcuts, and troubleshooting](docs/tutorial.md)
- [Build from source and use the CLI](docs/development.md)
- [Technical design](docs/design.md)
- [Report a problem or suggest an improvement](https://github.com/ndelah/Tyler/issues)

Free and open source under the [MIT license](LICENSE).
