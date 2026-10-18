//! Actions a keybinding can trigger, parsed from Hyprland dispatcher syntax
//! (`"movefocus l"`, `"movetoworkspace 3"`, `"resizeactive -100 0"`, ...).

use std::fmt;

use crate::geometry::Direction;

pub const WORKSPACE_COUNT: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullscreenMode {
    /// Covers the whole display, no gaps (Hyprland `fullscreen` mode 0).
    Fullscreen,
    /// Fills the workspace area inside `gaps_out` (Hyprland mode 1, "maximized").
    Maximized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTarget {
    /// 1-based workspace number.
    Index(usize),
    /// `e+1` / `e-1`: next/previous non-empty workspace.
    Relative(i32),
    /// Last active workspace (`previous`).
    Previous,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Run a shell command via `/bin/sh -c`.
    Exec(String),
    /// Launch or focus a macOS app; the argument is an `[apps]` key or app name.
    Launch(String),
    /// Open a URL as an app window via the configured browser (Omarchy web apps).
    WebApp(String),
    Close,
    ToggleFloat,
    Fullscreen(FullscreenMode),
    ToggleSplit,
    Focus(Direction),
    Swap(Direction),
    Workspace(WorkspaceTarget),
    MoveToWorkspace {
        workspace: usize,
        follow: bool,
    },
    Resize {
        dx: f64,
        dy: f64,
    },
    CycleNext {
        forward: bool,
    },
    FocusMonitor {
        forward: bool,
    },
    ShowKeybindings,
    ToggleScratchpad,
    MoveToScratchpad,
    Reload,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionError(pub String);

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ActionError {}

fn err<T>(msg: impl Into<String>) -> Result<T, ActionError> {
    Err(ActionError(msg.into()))
}

fn parse_workspace_index(arg: &str) -> Result<usize, ActionError> {
    match arg.parse::<usize>() {
        Ok(n) if (1..=WORKSPACE_COUNT).contains(&n) => Ok(n),
        _ => err(format!(
            "workspace must be 1-{WORKSPACE_COUNT}, got `{arg}`"
        )),
    }
}

fn parse_direction(arg: &str) -> Result<Direction, ActionError> {
    Direction::parse(arg)
        .ok_or_else(|| ActionError(format!("invalid direction `{arg}` (use l/r/u/d)")))
}

impl Action {
    /// Whether holding the key should repeat the action (focus, swap, resize).
    pub fn repeats(&self) -> bool {
        matches!(
            self,
            Action::Focus(_)
                | Action::Swap(_)
                | Action::Resize { .. }
                | Action::CycleNext { .. }
                | Action::FocusMonitor { .. }
        )
    }

    pub fn parse(spec: &str) -> Result<Action, ActionError> {
        let spec = spec.trim();
        let (cmd, arg) = match spec.split_once(char::is_whitespace) {
            Some((c, a)) => (c, a.trim()),
            None => (spec, ""),
        };
        let action = match cmd.to_ascii_lowercase().as_str() {
            "exec" if !arg.is_empty() => Action::Exec(arg.to_string()),
            "launch" if !arg.is_empty() => Action::Launch(arg.to_string()),
            "webapp" if arg.starts_with("https://") => Action::WebApp(arg.to_string()),
            "webapp" => return err("webapp needs an https:// URL"),
            "killactive" | "close" => Action::Close,
            "togglefloating" | "float" => Action::ToggleFloat,
            "fullscreen" => match arg {
                "" | "0" | "fullscreen" => Action::Fullscreen(FullscreenMode::Fullscreen),
                "1" | "maximized" | "maximize" => Action::Fullscreen(FullscreenMode::Maximized),
                other => return err(format!("unknown fullscreen mode `{other}`")),
            },
            "togglesplit" => Action::ToggleSplit,
            "movefocus" => Action::Focus(parse_direction(arg)?),
            "swapwindow" | "movewindow" => Action::Swap(parse_direction(arg)?),
            "workspace" => Action::Workspace(match arg {
                "previous" => WorkspaceTarget::Previous,
                "e+1" | "+1" | "next" => WorkspaceTarget::Relative(1),
                "e-1" | "-1" | "prev" => WorkspaceTarget::Relative(-1),
                n => WorkspaceTarget::Index(parse_workspace_index(n)?),
            }),
            "movetoworkspace" | "movetoworkspacesilent" => {
                let follow = cmd.eq_ignore_ascii_case("movetoworkspace");
                if arg == "special:scratchpad" || arg == "special" {
                    Action::MoveToScratchpad
                } else {
                    Action::MoveToWorkspace {
                        workspace: parse_workspace_index(arg)?,
                        follow,
                    }
                }
            }
            "togglespecialworkspace" => Action::ToggleScratchpad,
            "resizeactive" => {
                let nums: Vec<&str> = arg.split_whitespace().collect();
                let [dx, dy] = nums.as_slice() else {
                    return err("resizeactive needs two numbers: dx dy");
                };
                let parse = |s: &str| {
                    s.parse::<f64>()
                        .map_err(|_| ActionError(format!("invalid number `{s}`")))
                };
                Action::Resize {
                    dx: parse(dx)?,
                    dy: parse(dy)?,
                }
            }
            "cyclenext" => Action::CycleNext {
                forward: !matches!(arg, "prev" | "b"),
            },
            "focusmonitor" => Action::FocusMonitor {
                forward: match arg {
                    "next" | "+1" => true,
                    "prev" | "-1" => false,
                    _ => return err("focusmonitor needs next or prev"),
                },
            },
            "keybindings" => Action::ShowKeybindings,
            "reload" => Action::Reload,
            "exit" | "quit" => Action::Quit,
            "exec" | "launch" => return err(format!("`{cmd}` needs an argument")),
            other => return err(format!("unknown dispatcher `{other}`")),
        };
        Ok(action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_core_dispatchers() {
        assert_eq!(Action::parse("killactive"), Ok(Action::Close));
        assert_eq!(
            Action::parse("movefocus l"),
            Ok(Action::Focus(Direction::Left))
        );
        assert_eq!(
            Action::parse("swapwindow d"),
            Ok(Action::Swap(Direction::Down))
        );
        assert_eq!(
            Action::parse("workspace 3"),
            Ok(Action::Workspace(WorkspaceTarget::Index(3)))
        );
        assert_eq!(
            Action::parse("workspace e+1"),
            Ok(Action::Workspace(WorkspaceTarget::Relative(1)))
        );
        assert_eq!(
            Action::parse("workspace previous"),
            Ok(Action::Workspace(WorkspaceTarget::Previous))
        );
        assert_eq!(
            Action::parse("movetoworkspacesilent 10"),
            Ok(Action::MoveToWorkspace {
                workspace: 10,
                follow: false
            })
        );
        assert_eq!(
            Action::parse("resizeactive -100 0"),
            Ok(Action::Resize {
                dx: -100.0,
                dy: 0.0
            })
        );
        assert_eq!(
            Action::parse("fullscreen maximized"),
            Ok(Action::Fullscreen(FullscreenMode::Maximized))
        );
        assert_eq!(
            Action::parse("movetoworkspacesilent special:scratchpad"),
            Ok(Action::MoveToScratchpad)
        );
        assert_eq!(
            Action::parse("exec open -a Ghostty"),
            Ok(Action::Exec("open -a Ghostty".into()))
        );
    }

    #[test]
    fn rejects_invalid() {
        assert!(Action::parse("focusmonitor").is_err());
        assert!(Action::parse("focusmonitor sideways").is_err());
        assert!(Action::parse("workspace 11").is_err());
        assert!(Action::parse("workspace 0").is_err());
        assert!(Action::parse("movefocus x").is_err());
        assert!(Action::parse("resizeactive 10").is_err());
        assert!(Action::parse("exec").is_err());
        assert!(Action::parse("pseudo").is_err());
    }
}
