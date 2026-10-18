//! Shared presentation for the shortcut guide and editable settings.
use tyler_tiler::{
    action::{Action, FullscreenMode, WorkspaceTarget},
    config::Config,
};

pub fn title(action: &Action, config: &Config) -> String {
    match action {
        Action::Launch(app) => format!("Open {}", config.resolve_app(app)),
        Action::Close => "Close focused window".into(),
        Action::ToggleFloat => "Toggle floating / tiled".into(),
        Action::ToggleSplit => "Flip split direction".into(),
        Action::Fullscreen(FullscreenMode::Fullscreen) => "Fill display".into(),
        Action::Fullscreen(FullscreenMode::Maximized) => "Maximize in usable area".into(),
        Action::Focus(dir) | Action::Swap(dir) => {
            use tyler_tiler::geometry::Direction;
            let direction = match dir {
                Direction::Left => "left",
                Direction::Right => "right",
                Direction::Up => "above",
                Direction::Down => "below",
            };
            format!(
                "{} window {direction}",
                if matches!(action, Action::Focus(_)) {
                    "Focus"
                } else {
                    "Swap with"
                }
            )
        }
        Action::Workspace(WorkspaceTarget::Index(n)) => {
            format!("Switch to Mission Control desktop {n}")
        }
        Action::Workspace(WorkspaceTarget::Previous) => "Return to last native desktop".into(),
        Action::Workspace(WorkspaceTarget::Relative(n)) => format!(
            "{} occupied native desktop",
            if *n > 0 { "Next" } else { "Previous" }
        ),
        Action::MoveToWorkspace { workspace, follow } => format!(
            "Move to native desktop {workspace}{}",
            if *follow {
                " and follow"
            } else {
                " without following"
            }
        ),
        Action::Resize { dx, dy } if (*dx == 0.0) != (*dy == 0.0) => {
            let (axis, delta) = if *dy == 0.0 {
                ("width", *dx)
            } else {
                ("height", *dy)
            };
            let step = match delta.abs() {
                25.0 => " a little",
                300.0 => " a lot",
                _ => "",
            };
            format!(
                "{} window {axis}{step} ({} pt)",
                if delta > 0.0 { "Expand" } else { "Shrink" },
                delta.abs()
            )
        }
        Action::Resize { dx, dy } => format!("Resize: width {dx:+}, height {dy:+} points"),
        Action::CycleNext { forward } => {
            format!("{} window", if *forward { "Next" } else { "Previous" })
        }
        Action::FocusMonitor { forward } => {
            format!(
                "Focus {} monitor",
                if *forward { "next" } else { "previous" }
            )
        }
        Action::ShowKeybindings => "Show keybindings".into(),
        Action::ToggleScratchpad => "Show / hide scratchpad on this desktop".into(),
        Action::MoveToScratchpad => "Send window to scratchpad".into(),
        Action::Reload => "Reload configuration".into(),
        Action::Quit => "Quit Tyler and restore windows".into(),
        Action::Exec(cmd) => format!("Run {cmd}"),
        Action::WebApp(url) => format!("Open {url}"),
    }
}

pub fn group(action: &Action) -> &'static str {
    match action {
        Action::Focus(_) | Action::CycleNext { .. } | Action::FocusMonitor { .. } => {
            "Focus & navigation"
        }
        Action::Swap(_) | Action::ToggleSplit | Action::Resize { .. } => "Arrange & resize",
        Action::Close | Action::ToggleFloat | Action::Fullscreen(_) => "Window controls",
        Action::Workspace(_) | Action::MoveToWorkspace { .. } => "Workspaces",
        Action::ToggleScratchpad | Action::MoveToScratchpad => "Scratchpad",
        Action::Launch(_) | Action::WebApp(_) | Action::Exec(_) => "Launch apps & commands",
        Action::ShowKeybindings | Action::Reload | Action::Quit => "Tyler",
    }
}
