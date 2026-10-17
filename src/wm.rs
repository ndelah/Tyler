//! Platform-independent window manager core.
//!
//! [`Wm`] owns workspaces, per-workspace dwindle trees, floating state and
//! focus. It consumes window events and [`Action`]s and returns [`Command`]s
//! that a platform backend (macOS Accessibility API) executes. Virtual
//! workspaces are implemented AeroSpace-style: windows on inactive workspaces
//! are moved to the bottom-right corner of the display instead of using the
//! private Spaces API.

use std::collections::HashMap;

use crate::action::{Action, FullscreenMode, WorkspaceTarget, WORKSPACE_COUNT};
use crate::config::Config;
use crate::geometry::{find_neighbor, Direction, Rect};
use crate::layout::Tree;
use crate::WindowId;

/// Side effects requested by the core, executed by the platform layer.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    SetFrame(WindowId, Rect),
    /// Raise the window and activate its application.
    Focus(WindowId),
    Close(WindowId),
    /// Launch or activate an app by name (`open -a`).
    LaunchApp(String),
    OpenWebApp {
        browser: String,
        url: String,
    },
    Exec(String),
    ReloadConfig,
    Quit,
}

/// What the platform knows about a newly discovered window.
#[derive(Debug, Clone, PartialEq)]
pub struct NewWindow {
    pub id: WindowId,
    pub app: String,
    pub frame: Rect,
    /// Dialogs, sheets, fixed-size panels and the like.
    pub prefers_floating: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Location {
    Workspace(usize),
    Scratchpad,
}

#[derive(Debug, Clone)]
struct Managed {
    location: Location,
    floating: bool,
    float_frame: Rect,
}

#[derive(Debug, Clone, Default)]
struct Workspace {
    tree: Tree,
    /// Floating windows in stacking order (last = top).
    floating: Vec<WindowId>,
    fullscreen: Option<(WindowId, FullscreenMode)>,
    focused: Option<WindowId>,
}

impl Workspace {
    fn windows(&self) -> Vec<WindowId> {
        let mut all = self.tree.windows();
        all.extend(self.floating.iter().copied());
        all
    }
}

pub struct Wm {
    config: Config,
    /// Full display bounds (true fullscreen, hide corner).
    display: Rect,
    /// Usable area excluding menu bar and Dock.
    visible: Rect,
    workspaces: Vec<Workspace>,
    scratchpad: Vec<WindowId>,
    scratchpad_visible: bool,
    active: usize,
    previous: usize,
    windows: HashMap<WindowId, Managed>,
    focused: Option<WindowId>,
    applied: HashMap<WindowId, Rect>,
}

impl Wm {
    pub fn new(config: Config, display: Rect, visible: Rect) -> Wm {
        Wm {
            config,
            display,
            visible,
            workspaces: vec![Workspace::default(); WORKSPACE_COUNT],
            scratchpad: Vec::new(),
            scratchpad_visible: false,
            active: 0,
            previous: 0,
            windows: HashMap::new(),
            focused: None,
            applied: HashMap::new(),
        }
    }

    // --- queries ------------------------------------------------------------

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// 1-based active workspace number.
    pub fn active_workspace(&self) -> usize {
        self.active + 1
    }

    pub fn focused(&self) -> Option<WindowId> {
        self.focused
    }

    pub fn is_managed(&self, id: WindowId) -> bool {
        self.windows.contains_key(&id)
    }

    pub fn managed_windows(&self) -> Vec<WindowId> {
        self.windows.keys().copied().collect()
    }

    /// 1-based workspace of a window (`None` for unmanaged/scratchpad).
    pub fn workspace_of(&self, id: WindowId) -> Option<usize> {
        match self.windows.get(&id)?.location {
            Location::Workspace(i) => Some(i + 1),
            Location::Scratchpad => None,
        }
    }

    pub fn is_floating(&self, id: WindowId) -> bool {
        self.windows.get(&id).is_some_and(|m| m.floating)
    }

    /// Workspaces (1-based) that currently hold windows.
    pub fn occupied_workspaces(&self) -> Vec<usize> {
        (0..WORKSPACE_COUNT)
            .filter(|&i| !self.workspaces[i].windows().is_empty())
            .map(|i| i + 1)
            .collect()
    }

    /// Frame the core last asked the platform to apply.
    pub fn frame_of(&self, id: WindowId) -> Option<Rect> {
        self.applied.get(&id).copied()
    }

    /// Hidden windows park here: top-left at the display's bottom-right
    /// corner, one point inside so macOS does not reject the move.
    pub fn hidden_origin(&self) -> (f64, f64) {
        (self.display.max_x() - 1.0, self.display.max_y() - 1.0)
    }

    pub fn is_hidden_frame(&self, rect: &Rect) -> bool {
        let (hx, hy) = self.hidden_origin();
        (rect.x - hx).abs() < 1.0 && (rect.y - hy).abs() < 1.0
    }

    // --- events from the platform -------------------------------------------

    pub fn add_window(&mut self, win: NewWindow) -> Vec<Command> {
        if self.windows.contains_key(&win.id) {
            return Vec::new();
        }
        let floating = win.prefers_floating || self.config.should_float(&win.app);
        let float_frame = if floating {
            self.clamp_to_visible(win.frame)
        } else {
            self.default_float_frame()
        };
        self.windows.insert(
            win.id,
            Managed {
                location: Location::Workspace(self.active),
                floating,
                float_frame,
            },
        );
        self.attach(win.id, self.active);
        self.focus_window(win.id)
    }

    pub fn remove_window(&mut self, id: WindowId) -> Vec<Command> {
        let Some(managed) = self.windows.remove(&id) else {
            return Vec::new();
        };
        self.applied.remove(&id);
        let was_focused = self.focused == Some(id);
        match managed.location {
            Location::Workspace(i) => self.detach(id, i),
            Location::Scratchpad => self.scratchpad.retain(|w| *w != id),
        }
        if was_focused {
            self.focused = None;
            let next = self.workspaces[self.active]
                .focused
                .or_else(|| self.workspaces[self.active].windows().last().copied());
            if let Some(next) = next {
                return self.focus_window(next);
            }
        }
        self.relayout()
    }

    /// The OS reports that `id` became focused (click, Cmd-Tab, app activation).
    /// Mirrors Hyprland `focus_on_activate = true`: jump to its workspace.
    pub fn on_focus_changed(&mut self, id: WindowId) -> Vec<Command> {
        let Some(location) = self.windows.get(&id).map(|m| m.location) else {
            return Vec::new();
        };
        if self.focused == Some(id) {
            return Vec::new();
        }
        let mut out = Vec::new();
        self.focused = Some(id);
        if let Location::Workspace(i) = location {
            self.workspaces[i].focused = Some(id);
            if i != self.active {
                self.switch_to(i);
                out.extend(self.relayout());
            }
        }
        out
    }

    /// The user moved/resized a floating window by hand.
    pub fn on_frame_changed(&mut self, id: WindowId, frame: Rect) {
        if self.is_hidden_frame(&frame) {
            return;
        }
        if let Some(m) = self.windows.get_mut(&id) {
            if m.floating {
                m.float_frame = frame;
                self.applied.insert(id, frame);
            }
        }
    }

    pub fn set_screen(&mut self, display: Rect, visible: Rect) -> Vec<Command> {
        self.display = display;
        self.visible = visible;
        self.applied.clear();
        self.relayout()
    }

    pub fn set_config(&mut self, config: Config) -> Vec<Command> {
        self.config = config;
        self.relayout()
    }

    /// Forgets cached frames and re-emits every window's frame.
    pub fn force_relayout(&mut self) -> Vec<Command> {
        self.applied.clear();
        self.relayout()
    }

    /// Moves every hidden window back on screen, e.g. before exiting, so no
    /// window is left stranded in the corner.
    pub fn restore_all(&mut self) -> Vec<Command> {
        let mut out = Vec::new();
        let area = self.tiling_area();
        let mut ids: Vec<_> = self.windows.keys().copied().collect();
        ids.sort_unstable();
        for (n, id) in ids.into_iter().enumerate() {
            let offset = (n % 10) as f64 * 30.0;
            let frame = Rect::new(area.x + offset, area.y + offset, area.w * 0.6, area.h * 0.6);
            if self
                .applied
                .get(&id)
                .is_some_and(|r| self.is_hidden_frame(r))
            {
                out.push(Command::SetFrame(id, frame));
            }
        }
        out
    }

    // --- actions --------------------------------------------------------------

    pub fn handle(&mut self, action: &Action) -> Vec<Command> {
        match action {
            Action::Exec(cmd) => vec![Command::Exec(cmd.clone())],
            Action::Launch(key) => {
                vec![Command::LaunchApp(self.config.resolve_app(key).to_string())]
            }
            Action::WebApp(url) => vec![Command::OpenWebApp {
                browser: self.config.resolve_app("browser").to_string(),
                url: url.clone(),
            }],
            Action::Reload => vec![Command::ReloadConfig],
            Action::Quit => {
                let mut out = self.restore_all();
                out.push(Command::Quit);
                out
            }
            Action::Close => self.focused.map(Command::Close).into_iter().collect(),
            Action::ToggleFloat => self.toggle_float(),
            Action::Fullscreen(mode) => self.toggle_fullscreen(*mode),
            Action::ToggleSplit => {
                if let Some(id) = self.focused_tiled() {
                    self.workspaces[self.active].tree.toggle_split(id);
                }
                self.relayout()
            }
            Action::Focus(dir) => match self.neighbor_of_focused(*dir) {
                Some(next) => self.focus_window(next),
                None => Vec::new(),
            },
            Action::Swap(dir) => self.swap(*dir),
            Action::Workspace(target) => self.goto_workspace(*target),
            Action::MoveToWorkspace { workspace, follow } => {
                self.move_to_workspace(workspace - 1, *follow)
            }
            Action::Resize { dx, dy } => self.resize(*dx, *dy),
            Action::CycleNext { forward } => self.cycle(*forward),
            Action::ToggleScratchpad => self.toggle_scratchpad(),
            Action::MoveToScratchpad => self.move_to_scratchpad(),
        }
    }

    // --- internals -------------------------------------------------------------

    fn tiling_area(&self) -> Rect {
        self.visible
    }

    fn default_float_frame(&self) -> Rect {
        let a = self.tiling_area();
        let (w, h) = (a.w * 0.5, a.h * 0.6);
        Rect::new(a.x + (a.w - w) / 2.0, a.y + (a.h - h) / 2.0, w, h)
    }

    fn clamp_to_visible(&self, r: Rect) -> Rect {
        let a = self.tiling_area();
        if r.w <= 0.0 || r.h <= 0.0 || self.is_hidden_frame(&r) {
            return self.default_float_frame();
        }
        let w = r.w.min(a.w);
        let h = r.h.min(a.h);
        Rect::new(
            r.x.clamp(a.x, a.max_x() - w),
            r.y.clamp(a.y, a.max_y() - h),
            w,
            h,
        )
    }

    /// Adds a managed window to workspace `ws` (tree or floating list).
    fn attach(&mut self, id: WindowId, ws: usize) {
        let area = self.tiling_area();
        let (force, ratio) = (
            self.config.dwindle.force_split,
            self.config.dwindle.split_ratio,
        );
        let floating = self.windows[&id].floating;
        let w = &mut self.workspaces[ws];
        if floating {
            w.floating.push(id);
        } else {
            let target = w.focused.filter(|f| w.tree.contains(*f));
            w.tree.insert(id, target, area, force, ratio);
            w.fullscreen = None;
        }
        if let Some(m) = self.windows.get_mut(&id) {
            m.location = Location::Workspace(ws);
        }
    }

    fn detach(&mut self, id: WindowId, ws: usize) {
        let w = &mut self.workspaces[ws];
        w.tree.remove(id);
        w.floating.retain(|f| *f != id);
        if w.fullscreen.is_some_and(|(f, _)| f == id) {
            w.fullscreen = None;
        }
        if w.focused == Some(id) {
            w.focused = w
                .tree
                .windows()
                .last()
                .copied()
                .or_else(|| w.floating.last().copied());
        }
    }

    fn focus_window(&mut self, id: WindowId) -> Vec<Command> {
        self.focused = Some(id);
        match self.windows.get(&id).map(|m| m.location) {
            Some(Location::Workspace(i)) => {
                self.workspaces[i].focused = Some(id);
                let ws = &mut self.workspaces[i];
                if let Some(pos) = ws.floating.iter().position(|f| *f == id) {
                    let f = ws.floating.remove(pos);
                    ws.floating.push(f);
                }
            }
            Some(Location::Scratchpad) => {}
            None => {}
        }
        let mut out = self.relayout();
        out.push(Command::Focus(id));
        out
    }

    fn focused_tiled(&self) -> Option<WindowId> {
        self.focused
            .filter(|f| self.workspaces[self.active].tree.contains(*f))
    }

    fn visible_frames(&self) -> Vec<(WindowId, Rect)> {
        self.desired_frames()
            .into_iter()
            .filter(|(id, r)| {
                !self.is_hidden_frame(r)
                    && self.windows.get(id).is_some_and(|m| match m.location {
                        Location::Workspace(i) => i == self.active,
                        Location::Scratchpad => self.scratchpad_visible,
                    })
            })
            .collect()
    }

    fn neighbor_of_focused(&self, dir: Direction) -> Option<WindowId> {
        let focused = self.focused?;
        let ws = &self.workspaces[self.active];
        if ws.tree.contains(focused) && ws.fullscreen.is_none() {
            return ws.tree.neighbor(focused, dir, self.tiling_area());
        }
        let frames = self.visible_frames();
        let from = frames.iter().find(|(id, _)| *id == focused)?.1;
        let others: Vec<_> = frames
            .into_iter()
            .filter(|(id, _)| *id != focused)
            .collect();
        find_neighbor(&from, dir, &others)
    }

    fn swap(&mut self, dir: Direction) -> Vec<Command> {
        let Some(focused) = self.focused_tiled() else {
            return Vec::new();
        };
        let area = self.tiling_area();
        let ws = &mut self.workspaces[self.active];
        match ws.tree.neighbor(focused, dir, area) {
            Some(other) => {
                ws.tree.swap(focused, other);
                self.relayout()
            }
            None => Vec::new(),
        }
    }

    fn toggle_float(&mut self) -> Vec<Command> {
        let Some(id) = self.focused else {
            return Vec::new();
        };
        let Some(Location::Workspace(ws)) = self.windows.get(&id).map(|m| m.location) else {
            return Vec::new();
        };
        self.detach(id, ws);
        let default_frame = self.default_float_frame();
        let m = self
            .windows
            .get_mut(&id)
            .expect("focused window is managed");
        m.floating = !m.floating;
        if m.floating && self.applied.get(&id).is_none() {
            m.float_frame = default_frame;
        }
        self.attach(id, ws);
        self.workspaces[ws].focused = Some(id);
        self.focus_window(id)
    }

    fn toggle_fullscreen(&mut self, mode: FullscreenMode) -> Vec<Command> {
        let Some(id) = self.focused else {
            return Vec::new();
        };
        if self.workspace_of(id) != Some(self.active + 1) {
            return Vec::new();
        }
        let ws = &mut self.workspaces[self.active];
        ws.fullscreen = match ws.fullscreen {
            Some((f, m)) if f == id && m == mode => None,
            _ => Some((id, mode)),
        };
        let mut out = self.relayout();
        out.push(Command::Focus(id));
        out
    }

    fn resize(&mut self, dx: f64, dy: f64) -> Vec<Command> {
        let Some(id) = self.focused else {
            return Vec::new();
        };
        let area = self.tiling_area();
        if self.is_floating(id) {
            if let Some(m) = self.windows.get_mut(&id) {
                let f = m.float_frame;
                m.float_frame = Rect::new(f.x, f.y, (f.w + dx).max(100.0), (f.h + dy).max(100.0));
            }
        } else {
            self.workspaces[self.active].tree.resize(id, dx, dy, area);
        }
        self.relayout()
    }

    fn cycle(&mut self, forward: bool) -> Vec<Command> {
        let order: Vec<WindowId> = self
            .visible_frames()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        if order.is_empty() {
            return Vec::new();
        }
        let next = match self
            .focused
            .and_then(|f| order.iter().position(|w| *w == f))
        {
            Some(i) if forward => order[(i + 1) % order.len()],
            Some(i) => order[(i + order.len() - 1) % order.len()],
            None => order[0],
        };
        self.focus_window(next)
    }

    fn switch_to(&mut self, ws: usize) {
        if ws != self.active {
            self.previous = self.active;
            self.active = ws;
        }
        // Omarchy: binds:hide_special_on_workspace_change = true
        self.scratchpad_visible = false;
    }

    fn goto_workspace(&mut self, target: WorkspaceTarget) -> Vec<Command> {
        let ws = match target {
            WorkspaceTarget::Index(n) => n - 1,
            WorkspaceTarget::Previous => self.previous,
            WorkspaceTarget::Relative(step) => {
                let mut existing: Vec<usize> = (0..WORKSPACE_COUNT)
                    .filter(|&i| i == self.active || !self.workspaces[i].windows().is_empty())
                    .collect();
                existing.sort_unstable();
                let pos = existing.iter().position(|&i| i == self.active).unwrap_or(0) as i32;
                let len = existing.len() as i32;
                existing[(pos + step).rem_euclid(len) as usize]
            }
        };
        if ws == self.active {
            return Vec::new();
        }
        self.switch_to(ws);
        let target = self.workspaces[ws]
            .focused
            .or_else(|| self.workspaces[ws].windows().last().copied());
        match target {
            Some(id) => self.focus_window(id),
            None => {
                self.focused = None;
                self.relayout()
            }
        }
    }

    fn move_to_workspace(&mut self, target: usize, follow: bool) -> Vec<Command> {
        let Some(id) = self.focused else {
            return Vec::new();
        };
        let Some(m) = self.windows.get(&id) else {
            return Vec::new();
        };
        match m.location {
            Location::Workspace(cur) if cur == target => return Vec::new(),
            Location::Workspace(cur) => self.detach(id, cur),
            Location::Scratchpad => self.scratchpad.retain(|w| *w != id),
        }
        self.attach(id, target);
        self.workspaces[target].focused = Some(id);
        if follow {
            self.switch_to(target);
            return self.focus_window(id);
        }
        self.focused = None;
        let next = self.workspaces[self.active]
            .focused
            .or_else(|| self.workspaces[self.active].windows().last().copied());
        match next {
            Some(n) => self.focus_window(n),
            None => self.relayout(),
        }
    }

    fn move_to_scratchpad(&mut self) -> Vec<Command> {
        let Some(id) = self.focused else {
            return Vec::new();
        };
        let Some(Location::Workspace(cur)) = self.windows.get(&id).map(|m| m.location) else {
            return Vec::new();
        };
        self.detach(id, cur);
        let default_frame = self.default_float_frame();
        if let Some(m) = self.windows.get_mut(&id) {
            if !m.floating {
                m.float_frame = default_frame;
            }
            m.floating = true;
            m.location = Location::Scratchpad;
        }
        self.scratchpad.push(id);
        self.focused = None;
        match self.workspaces[self.active].focused {
            Some(n) => self.focus_window(n),
            None => self.relayout(),
        }
    }

    fn toggle_scratchpad(&mut self) -> Vec<Command> {
        self.scratchpad_visible = !self.scratchpad_visible && !self.scratchpad.is_empty();
        if self.scratchpad_visible {
            if let Some(&top) = self.scratchpad.last() {
                return self.focus_window(top);
            }
        }
        if self.focused.is_some_and(|f| self.scratchpad.contains(&f)) {
            self.focused = None;
            if let Some(n) = self.workspaces[self.active].focused {
                return self.focus_window(n);
            }
        }
        self.relayout()
    }

    fn desired_frames(&self) -> Vec<(WindowId, Rect)> {
        let (hx, hy) = self.hidden_origin();
        let area = self.tiling_area();
        let gaps = self.config.gaps();
        let mut out = Vec::new();
        let hidden = |id: WindowId, r: Option<Rect>| {
            let (w, h) = r
                .map(|r| (r.w, r.h))
                .unwrap_or((area.w * 0.5, area.h * 0.5));
            (id, Rect::new(hx, hy, w, h))
        };
        for (i, ws) in self.workspaces.iter().enumerate() {
            let tiled = ws.tree.layout(area, gaps);
            if i != self.active {
                for (id, r) in tiled {
                    out.push(hidden(id, Some(r)));
                }
                for &id in &ws.floating {
                    out.push(hidden(id, Some(self.windows[&id].float_frame)));
                }
                continue;
            }
            for (id, r) in tiled {
                let frame = match ws.fullscreen {
                    Some((f, FullscreenMode::Fullscreen)) if f == id => self.display,
                    Some((f, FullscreenMode::Maximized)) if f == id => area.inset(gaps.outer),
                    _ => r,
                };
                out.push((id, frame));
            }
            for &id in &ws.floating {
                let frame = match ws.fullscreen {
                    Some((f, FullscreenMode::Fullscreen)) if f == id => self.display,
                    Some((f, FullscreenMode::Maximized)) if f == id => area.inset(gaps.outer),
                    _ => self.windows[&id].float_frame,
                };
                out.push((id, frame));
            }
        }
        for &id in &self.scratchpad {
            let f = self.windows[&id].float_frame;
            out.push(if self.scratchpad_visible {
                (id, f)
            } else {
                hidden(id, Some(f))
            });
        }
        out
    }

    /// Emits `SetFrame` for every window whose desired frame changed.
    fn relayout(&mut self) -> Vec<Command> {
        let mut out = Vec::new();
        for (id, frame) in self.desired_frames() {
            let unchanged = self.applied.get(&id).is_some_and(|r| r.approx_eq(&frame));
            if !unchanged {
                self.applied.insert(id, frame);
                out.push(Command::SetFrame(id, frame));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISPLAY: Rect = Rect::new(0.0, 0.0, 1600.0, 1000.0);
    const VISIBLE: Rect = Rect::new(0.0, 25.0, 1600.0, 975.0);

    fn wm() -> Wm {
        let user = "[general]\ngaps_in = 0\ngaps_out = 0\n[rules]\nfloat = [\"Calculator\"]\n";
        Wm::new(
            Config::from_layers(crate::config::DEFAULT_CONFIG, Some(user)).unwrap(),
            DISPLAY,
            VISIBLE,
        )
    }

    fn open(wm: &mut Wm, id: WindowId) -> Vec<Command> {
        wm.add_window(NewWindow {
            id,
            app: "Ghostty".into(),
            frame: Rect::new(0.0, 0.0, 500.0, 400.0),
            prefers_floating: false,
        })
    }

    fn frame(wm: &Wm, id: WindowId) -> Rect {
        wm.frame_of(id).unwrap()
    }

    #[test]
    fn opening_windows_auto_tiles_and_focuses() {
        let mut wm = wm();
        let cmds = open(&mut wm, 1);
        assert_eq!(cmds, vec![Command::SetFrame(1, VISIBLE), Command::Focus(1)]);
        open(&mut wm, 2);
        assert!(frame(&wm, 1).approx_eq(&Rect::new(0.0, 25.0, 800.0, 975.0)));
        assert!(frame(&wm, 2).approx_eq(&Rect::new(800.0, 25.0, 800.0, 975.0)));
        assert_eq!(wm.focused(), Some(2));
    }

    #[test]
    fn relayout_only_emits_changes() {
        let mut wm = wm();
        open(&mut wm, 1);
        open(&mut wm, 2);
        let cmds = open(&mut wm, 3);
        // Window 1 keeps its frame; only 2 and 3 move.
        let moved: Vec<_> = cmds
            .iter()
            .filter_map(|c| {
                if let Command::SetFrame(id, _) = c {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(moved, vec![2, 3]);
    }

    #[test]
    fn closing_refocuses_and_retiles() {
        let mut wm = wm();
        open(&mut wm, 1);
        open(&mut wm, 2);
        assert_eq!(wm.handle(&Action::Close), vec![Command::Close(2)]);
        let cmds = wm.remove_window(2);
        assert!(cmds.contains(&Command::Focus(1)));
        assert!(frame(&wm, 1).approx_eq(&VISIBLE));
    }

    #[test]
    fn focus_and_swap_directionally() {
        let mut wm = wm();
        open(&mut wm, 1);
        open(&mut wm, 2);
        wm.handle(&Action::Focus(Direction::Left));
        assert_eq!(wm.focused(), Some(1));
        wm.handle(&Action::Swap(Direction::Right));
        assert_eq!(wm.focused(), Some(1));
        assert!(frame(&wm, 1).approx_eq(&Rect::new(800.0, 25.0, 800.0, 975.0)));
        assert!(wm.handle(&Action::Focus(Direction::Right)).is_empty());
    }

    #[test]
    fn workspaces_hide_and_show_windows() {
        let mut wm = wm();
        open(&mut wm, 1);
        wm.handle(&Action::Workspace(WorkspaceTarget::Index(2)));
        assert_eq!(wm.active_workspace(), 2);
        assert!(wm.is_hidden_frame(&frame(&wm, 1)));
        assert_eq!(wm.focused(), None);
        open(&mut wm, 2);
        assert_eq!(wm.workspace_of(2), Some(2));
        wm.handle(&Action::Workspace(WorkspaceTarget::Previous));
        assert_eq!(wm.active_workspace(), 1);
        assert!(frame(&wm, 1).approx_eq(&VISIBLE));
        assert!(wm.is_hidden_frame(&frame(&wm, 2)));
        assert_eq!(wm.focused(), Some(1));
    }

    #[test]
    fn move_to_workspace_follow_and_silent() {
        let mut wm = wm();
        open(&mut wm, 1);
        open(&mut wm, 2);
        wm.handle(&Action::MoveToWorkspace {
            workspace: 3,
            follow: false,
        });
        assert_eq!(wm.active_workspace(), 1);
        assert_eq!(wm.workspace_of(2), Some(3));
        assert!(wm.is_hidden_frame(&frame(&wm, 2)));
        assert!(frame(&wm, 1).approx_eq(&VISIBLE));
        assert_eq!(wm.focused(), Some(1));
        wm.handle(&Action::MoveToWorkspace {
            workspace: 3,
            follow: true,
        });
        assert_eq!(wm.active_workspace(), 3);
        assert_eq!(wm.focused(), Some(1));
        assert!(frame(&wm, 2).approx_eq(&Rect::new(0.0, 25.0, 800.0, 975.0)));
        assert_eq!(wm.occupied_workspaces(), vec![3]);
    }

    #[test]
    fn relative_workspace_cycles_through_occupied() {
        let mut wm = wm();
        open(&mut wm, 1);
        wm.handle(&Action::MoveToWorkspace {
            workspace: 4,
            follow: false,
        });
        open(&mut wm, 2);
        wm.handle(&Action::MoveToWorkspace {
            workspace: 7,
            follow: false,
        });
        // Active 1 (empty), occupied 4 and 7.
        wm.handle(&Action::Workspace(WorkspaceTarget::Relative(1)));
        assert_eq!(wm.active_workspace(), 4);
        wm.handle(&Action::Workspace(WorkspaceTarget::Relative(1)));
        assert_eq!(wm.active_workspace(), 7);
        wm.handle(&Action::Workspace(WorkspaceTarget::Relative(1)));
        assert_eq!(wm.active_workspace(), 4);
        wm.handle(&Action::Workspace(WorkspaceTarget::Relative(-1)));
        assert_eq!(wm.active_workspace(), 7);
    }

    #[test]
    fn toggle_float_removes_from_tree() {
        let mut wm = wm();
        open(&mut wm, 1);
        open(&mut wm, 2);
        wm.handle(&Action::ToggleFloat);
        assert!(wm.is_floating(2));
        assert!(frame(&wm, 1).approx_eq(&VISIBLE));
        wm.handle(&Action::ToggleFloat);
        assert!(!wm.is_floating(2));
        assert!(frame(&wm, 2).approx_eq(&Rect::new(800.0, 25.0, 800.0, 975.0)));
    }

    #[test]
    fn float_rules_and_dialogs_start_floating() {
        let mut wm = wm();
        open(&mut wm, 1);
        let r = Rect::new(100.0, 100.0, 300.0, 400.0);
        wm.add_window(NewWindow {
            id: 2,
            app: "Calculator".into(),
            frame: r,
            prefers_floating: false,
        });
        wm.add_window(NewWindow {
            id: 3,
            app: "Ghostty".into(),
            frame: r,
            prefers_floating: true,
        });
        assert!(wm.is_floating(2) && wm.is_floating(3));
        assert!(frame(&wm, 1).approx_eq(&VISIBLE));
        assert!(frame(&wm, 2).approx_eq(&r));
    }

    #[test]
    fn fullscreen_modes_toggle() {
        let mut wm = wm();
        open(&mut wm, 1);
        open(&mut wm, 2);
        wm.handle(&Action::Fullscreen(FullscreenMode::Fullscreen));
        assert!(frame(&wm, 2).approx_eq(&DISPLAY));
        wm.handle(&Action::Fullscreen(FullscreenMode::Maximized));
        assert!(frame(&wm, 2).approx_eq(&VISIBLE));
        wm.handle(&Action::Fullscreen(FullscreenMode::Maximized));
        assert!(frame(&wm, 2).approx_eq(&Rect::new(800.0, 25.0, 800.0, 975.0)));
    }

    #[test]
    fn resize_tiled_and_floating() {
        let mut wm = wm();
        open(&mut wm, 1);
        open(&mut wm, 2);
        wm.handle(&Action::Resize { dx: 160.0, dy: 0.0 });
        assert!((frame(&wm, 2).w - 960.0).abs() < 0.5);
        wm.handle(&Action::ToggleFloat);
        let before = frame(&wm, 2);
        wm.handle(&Action::Resize {
            dx: -25.0,
            dy: 25.0,
        });
        let after = frame(&wm, 2);
        assert!(
            (after.w - (before.w - 25.0)).abs() < 0.5 && (after.h - (before.h + 25.0)).abs() < 0.5
        );
    }

    #[test]
    fn scratchpad_toggle() {
        let mut wm = wm();
        open(&mut wm, 1);
        open(&mut wm, 2);
        wm.handle(&Action::MoveToScratchpad);
        assert!(wm.is_hidden_frame(&frame(&wm, 2)));
        assert!(frame(&wm, 1).approx_eq(&VISIBLE));
        wm.handle(&Action::ToggleScratchpad);
        assert!(!wm.is_hidden_frame(&frame(&wm, 2)));
        assert_eq!(wm.focused(), Some(2));
        wm.handle(&Action::Workspace(WorkspaceTarget::Index(2)));
        assert!(
            wm.is_hidden_frame(&frame(&wm, 2)),
            "scratchpad hides on workspace change"
        );
    }

    #[test]
    fn external_focus_switches_workspace() {
        let mut wm = wm();
        open(&mut wm, 1);
        wm.handle(&Action::MoveToWorkspace {
            workspace: 5,
            follow: false,
        });
        wm.on_focus_changed(1);
        assert_eq!(wm.active_workspace(), 5);
        assert!(!wm.is_hidden_frame(&frame(&wm, 1)));
    }

    #[test]
    fn cycle_next_wraps() {
        let mut wm = wm();
        open(&mut wm, 1);
        open(&mut wm, 2);
        open(&mut wm, 3);
        wm.handle(&Action::CycleNext { forward: true });
        assert_eq!(wm.focused(), Some(1));
        wm.handle(&Action::CycleNext { forward: false });
        assert_eq!(wm.focused(), Some(3));
    }

    #[test]
    fn launch_resolves_apps_and_quit_restores() {
        let mut wm = wm();
        assert_eq!(
            wm.handle(&Action::Launch("terminal".into())),
            vec![Command::LaunchApp("Terminal".into())]
        );
        assert_eq!(
            wm.handle(&Action::WebApp("https://x.com/".into())),
            vec![Command::OpenWebApp {
                browser: "Safari".into(),
                url: "https://x.com/".into()
            }]
        );
        open(&mut wm, 1);
        wm.handle(&Action::MoveToWorkspace {
            workspace: 2,
            follow: false,
        });
        let cmds = wm.handle(&Action::Quit);
        assert!(matches!(cmds[0], Command::SetFrame(1, r) if !wm.is_hidden_frame(&r)));
        assert_eq!(cmds.last(), Some(&Command::Quit));
    }

    #[test]
    fn hot_reload_applies_new_gaps() {
        let mut wm = wm();
        open(&mut wm, 1);
        let cfg = Config::from_layers(
            crate::config::DEFAULT_CONFIG,
            Some("[general]\ngaps_out = 20\n"),
        )
        .unwrap();
        let cmds = wm.set_config(cfg);
        assert_eq!(cmds, vec![Command::SetFrame(1, VISIBLE.inset(20.0))]);
    }
}
