# Tyler user guide

[Website & demo](https://tyler.delahousse.xyz/) · [Overview](../README.md) · [Report a problem](https://github.com/ndelah/Tyler/issues)

Tyler automatically arranges windows on your main display. It runs in the menu bar, so you can close Terminal after installing it.

[Install](#1-install-or-update) · [Permissions](#3-grant-macos-permissions) · [Shortcuts](#5-complete-default-shortcut-reference) · [Customize](#7-customize-and-reload) · [Troubleshooting](#8-limitations-and-recovery)

## 1. Install or update

You need macOS and **Xcode Command Line Tools**. If needed, run this in Terminal and finish the installation before continuing:

```sh
xcode-select --install
```

Then install Tyler:

```sh
curl -fsSL https://raw.githubusercontent.com/ndelah/Tyler/main/scripts/install.sh | sh
```

The installer builds the app, installs Rust if needed, and adds:

| Location | What it contains |
| --- | --- |
| `~/Applications/Tyler.app` | The menu bar app |
| `~/.local/bin/tyler-tiler` | The optional command-line tool |
| `~/.local/share/tyler` | The source checkout used for builds |

Open **Tyler.app** from your home Applications folder. You can [read the installer](../scripts/install.sh) before running it.

**Updating:** choose **Quit Tyler** in its menu bar menu, then run the same install command again. Your configuration is preserved. If you're upgrading from older shortcuts, see the next section.

## 2. Migrate an existing configuration

Skip this section for a fresh installation.

Older configuration files can override the current Fn shortcuts. To start with the current defaults, quit Tyler and back up `~/.config/tyler/tyler.toml` under a different name. Then run:

```sh
~/.local/bin/tyler-tiler write-config
```

Copy back only the settings you want to keep, such as apps and gaps. Leave `super = "fn"` to use the shortcuts below. If you use `TYLER_CONFIG` or `XDG_CONFIG_HOME`, back up your configuration at that location instead.

## 3. Grant macOS permissions

1. Open **Tyler.app**. Its icon appears in the menu bar.
2. Choose **Open Accessibility Settings…** from Tyler's menu.
3. Enable **Tyler** under **System Settings → Privacy & Security → Accessibility**.
4. Tyler starts automatically once access is granted. If necessary, quit and reopen it.

**Can't find the app?** It is in your personal Applications folder. In the Accessibility picker, click **+**, press **Command-Shift-G**, enter `~/Applications/Tyler.app`, and select it.

If the menu says **Tiling could not start**, choose **View Log…** and then **Retry Starting Tyler** after fixing the reported problem. If the keyboard event tap fails, check the permission guidance in the log; the app also offers **Open Input Monitoring Settings…**.

Replacing a locally signed build may require granting access again. Keep System Integrity Protection enabled and run Tyler as your normal user.

## 4. First session

Open two ordinary, resizable windows on the main display. The second window splits the first window's space. Each new window splits the **focused tile**, so click a window before opening another to choose where it appears. Closing a window gives its space to its neighbor.

Try these actions:

1. **Fn + arrow** focuses a neighboring window.
2. **Fn + Shift + arrow** swaps the two windows.
3. **Fn + T** lets a window float freely. Press again to tile it.
4. **Fn + J** changes the split between side-by-side and stacked.
5. **Fn + U** makes the focused window fill the usable display. Press again to restore it.

Choose **View Shortcuts…** from Tyler's menu to see your active bindings. Closing that window keeps Tyler running. Choose **Quit Tyler** to stop tiling and bring parked windows back.

Run only one instance: use either Tyler.app or the CLI.

### Workspaces and scratchpad

Workspaces let you keep separate groups of windows. **Fn + 2** switches to group 2; **Fn + 1** returns to group 1. **Fn + Shift + 2** moves the focused window to group 2 and follows it. The 0 key selects group 10.

In this version, these are **Tyler groups, not Mission Control desktops**. Inactive windows are parked at the main display's corner. They may still appear in Mission Control, the Dock, or Command-Tab. Groups reset when Tyler restarts.

The scratchpad is a temporary hiding place: **Fn + Shift + V** sends a window there, and **Fn + V** shows or hides it. Switching workspaces hides it again.

## 5. Complete default shortcut reference

**Fn** means the Globe key on Apple keyboards. The configuration calls this modifier `SUPER`. Custom settings can change these defaults.

### Windows

| Shortcut | Action |
| --- | --- |
| Fn + arrow | Focus in that direction |
| Fn + Shift + arrow | Swap in that direction |
| Fn + ] / [ | Focus the next / previous window |
| Fn + T | Toggle floating |
| Fn + J | Flip the split orientation |
| Fn + G | Fill the display; repeat to restore |
| Fn + U | Fill the usable area, respecting the Dock, menu bar, and gaps; repeat to restore |
| Fn + X | Close the focused window |

### Resize

| Shortcut | Action |
| --- | --- |
| Fn + minus / equals | Shrink / expand width by 100 points |
| Fn + Shift + minus / equals | Shrink / expand height by 100 points |
| Fn + comma / period | Shrink / expand width by 25 points |
| Fn + Shift + comma / period | Shrink / expand height by 25 points |

For tiled windows, resizing moves a shared split, so a suitable neighbor must exist. App minimum sizes and layout limits can prevent further resizing.

### Workspaces

| Shortcut | Action |
| --- | --- |
| Fn + 1…0 | Switch to workspace 1…10 |
| Fn + Shift + 1…0 | Move the focused window there and follow it |
| Fn + Tab | Next occupied workspace |
| Fn + Shift + Tab | Previous occupied workspace |
| Fn + grave accent | Return to the last workspace |
| Fn + V | Show / hide the scratchpad |
| Fn + Shift + V | Send the focused window to the scratchpad |

### Apps and Tyler

| Shortcut | Action |
| --- | --- |
| Fn + Return | Open or activate Terminal |
| Fn + B | Open or activate your browser (Safari by default) |
| Fn + O | Open or activate Finder |
| Fn + R | Reload configuration |
| Fn + Escape | Stop tiling and restore parked windows |

App shortcuts activate an existing app; they don't always create a new window. Window closing can show the app's save prompt.

Keys refer to **physical US keyboard positions**. On AZERTY and other layouts, the printed character may differ. In the config, `code:10`…`code:19` means the number row 1…0, and `code:20` / `code:21` means minus / equals. The [default configuration](../config/default.toml) lists the exact binding names.

## 6. Preserve Mac shortcuts and check personal conflicts

Your usual Command shortcuts, including Command-Q, Command-W, and Command-Tab, keep working. Fn + arrows takes over the usual Home/End/Page Up/Page Down behavior while Tyler runs.

If pressing Fn alone opens emoji or changes input sources, set **System Settings → Keyboard → Press Globe/Fn key to → Do Nothing**. Custom shortcuts in other utilities can still overlap Tyler's bindings.

Some external keyboards handle Fn inside the keyboard and don't send it to macOS. If yours does, choose another modifier, such as `super = "ctrl+alt+cmd"`, in the configuration. Avoid Command alone, which interferes with normal app shortcuts. If Fn + Escape doesn't reach Tyler, quit from the menu bar.

## 7. Customize and reload

Create your configuration:

```sh
~/.local/bin/tyler-tiler write-config
```

This creates `~/.config/tyler/tyler.toml` without overwriting an existing file. Edit it to change apps, shortcuts, and gaps. Save to reload, or press **Fn + R**. If a saved change is invalid, Tyler keeps the previous working settings; **View Log…** explains the error.

For example, edit the matching sections in your file:

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
"SUPER + X" = "unbind" # use Command-W to close windows instead
"SUPER + SHIFT + B" = "webapp https://chatgpt.com"
"SUPER + SHIFT + D" = "movetoworkspacesilent 2" # move without following
```

Don't add a second copy of an existing section. Settings override the built-in defaults; `unbind` removes a shortcut. To replace the entire binding table, set `default_bindings = false` in `[general]`.

Check your settings and list the active shortcuts:

```sh
~/.local/bin/tyler-tiler check-config
~/.local/bin/tyler-tiler bindings
```

Advanced options: use `TYLER_CONFIG` for a custom configuration path or `XDG_CONFIG_HOME` for a custom configuration directory. The [legacy Omarchy preset](../config/omarchy.toml) is optional; it replaces several standard Mac shortcuts and expects different apps.

## 8. Limitations and recovery

| Problem | What to try |
| --- | --- |
| Nothing tiles | Check Accessibility access for Tyler, then use **Retry Starting Tyler** or reopen the app. |
| Shortcuts differ from this guide | Open **View Shortcuts…**. An older configuration may override the defaults; see [migration](#2-migrate-an-existing-configuration). |
| Shortcuts stop in a password field or terminal | Leave the secure field. macOS Secure Input can block global shortcuts. |
| A window won't fit or resize | Try fewer windows or float it with **Fn + T**. Some apps enforce minimum sizes or reject window moves. |
| Windows remain parked after a force-quit | Activate them through the Dock or Command-Tab and drag them back. Use **Quit Tyler** for normal exits. |
| Windows appear on a neighboring display | Inactive workspace windows can be exposed there. Start with one display and avoid native fullscreen while using this version. |
| CLI mode has no permission | Enable Accessibility for the terminal app hosting it, then fully quit and reopen that terminal. |

Only the main display is tiled. Dialogs and apps configured to float keep their own placement. The app is built from source and ad-hoc signed; it isn't a notarized or App Store release.

If you still need help, [open an issue](https://github.com/ndelah/Tyler/issues). Include your macOS version, keyboard layout, the affected app, and the relevant **View Log…** output. Mention whether you use Tyler.app or the CLI, and whether the problem affects one app or all apps.

For source builds and debug logging, see the [developer guide](development.md).
