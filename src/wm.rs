//! Platform-independent window manager core.
//!
//! [`Wm`] owns workspaces, per-workspace dwindle trees, floating state and
//! focus. It consumes window events and [`Action`]s and returns [`Command`]s
//! that a platform backend (macOS Accessibility API) executes. Inactive
//! desktops are owned by macOS. Only the explicit scratchpad uses minimization.

use std::collections::{HashMap, HashSet};

use crate::action::{Action, FullscreenMode, WorkspaceTarget};
use crate::config::Config;
use crate::geometry::{find_neighbor, Direction, Rect};
use crate::layout::Tree;
use crate::WindowId;

/// Side effects requested by the core, executed by the platform layer.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    SetFrame(WindowId, Rect),
    /// Visibility is used only for the explicit scratchpad.
    SetVisible(WindowId, bool),
    FocusSpace(u64),
    MoveToSpace {
        window: WindowId,
        space: u64,
        follow: bool,
    },
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
    FocusMonitor {
        forward: bool,
    },
    ShowKeybindings,
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
    /// Full display bounds (true fullscreen).
    display: Rect,
    /// Usable area excluding menu bar and Dock.
    visible: Rect,
    workspaces: Vec<Workspace>,
    space_ids: Vec<u64>,
    native_order: Vec<usize>,
    native_occupied: HashSet<usize>,
    suspended: bool,
    scratchpad_home: HashMap<WindowId, usize>,
    scratchpad: Vec<WindowId>,
    scratchpad_visible: bool,
    active: usize,
    previous: usize,
    windows: HashMap<WindowId, Managed>,
    focused: Option<WindowId>,
    applied: HashMap<WindowId, Rect>,
    hidden: HashSet<WindowId>,
    minimums: HashMap<WindowId, (f64, f64)>,
}

impl Wm {
    pub fn new(config: Config, display: Rect, visible: Rect) -> Wm {
        Wm {
            config,
            display,
            visible,
            workspaces: vec![Workspace::default()],
            space_ids: vec![0],
            native_order: Vec::new(),
            native_occupied: HashSet::new(),
            suspended: true,
            scratchpad_home: HashMap::new(),
            scratchpad: Vec::new(),
            scratchpad_visible: false,
            active: 0,
            previous: 0,
            windows: HashMap::new(),
            focused: None,
            applied: HashMap::new(),
            hidden: HashSet::new(),
            minimums: HashMap::new(),
        }
    }

    // --- queries ------------------------------------------------------------

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// 1-based active workspace number.
    pub fn active_workspace(&self) -> usize {
        if self.suspended {
            return 0;
        }
        self.native_order
            .iter()
            .position(|&slot| slot == self.active)
            .map_or(0, |i| i + 1)
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
            Location::Workspace(i) => self
                .native_order
                .iter()
                .position(|&slot| slot == i)
                .map(|n| n + 1),
            Location::Scratchpad => None,
        }
    }

    pub fn is_floating(&self, id: WindowId) -> bool {
        self.windows.get(&id).is_some_and(|m| m.floating)
    }

    /// Workspaces (1-based) that currently hold windows.
    pub fn occupied_workspaces(&self) -> Vec<usize> {
        self.native_order
            .iter()
            .enumerate()
            .filter(|(_, slot)| {
                self.native_occupied.contains(slot) || !self.workspaces[**slot].windows().is_empty()
            })
            .map(|(i, _)| i + 1)
            .collect()
    }

    /// Frame the core last asked the platform to apply.
    pub fn frame_of(&self, id: WindowId) -> Option<Rect> {
        self.applied.get(&id).copied()
    }

    pub fn is_visible(&self, id: WindowId) -> bool {
        !self.suspended
            && self.windows.get(&id).is_some_and(|m| match m.location {
                Location::Workspace(i) => i == self.active,
                Location::Scratchpad => {
                    self.scratchpad_visible && self.scratchpad_home.get(&id) == Some(&self.active)
                }
            })
    }

    /// Reconcile observed native membership without requesting a desktop switch.
    /// Slots retain layouts by stable Space ID when Mission Control reorders desktops.
    pub fn sync_native_desktops(
        &mut self,
        desktops: &[(u64, Vec<WindowId>)],
        active: Option<u64>,
    ) -> Vec<Command> {
        self.native_order.clear();
        self.native_occupied.clear();
        for (id, members) in desktops {
            let slot = match self.space_ids.iter().position(|known| known == id) {
                Some(slot) => slot,
                None => {
                    self.space_ids.push(*id);
                    self.workspaces.push(Workspace::default());
                    self.workspaces.len() - 1
                }
            };
            self.native_order.push(slot);
            if !members.is_empty() {
                self.native_occupied.insert(slot);
            }
        }
        for (id, members) in desktops {
            let slot = self.space_ids.iter().position(|known| known == id).unwrap();
            for window in members {
                if let Some(Location::Workspace(old)) = self.windows.get(window).map(|m| m.location)
                {
                    if old != slot {
                        self.detach(*window, old);
                        self.attach(*window, slot);
                        self.workspaces[slot].focused = Some(*window);
                        self.applied.remove(window);
                    }
                }
            }
        }
        let slot = active
            .and_then(|id| self.space_ids.iter().position(|known| *known == id))
            .filter(|slot| self.native_order.contains(slot));
        self.suspended = slot.is_none();
        if let Some(slot) = slot {
            if self.active != slot {
                self.switch_to(slot);
                self.applied.clear();
            }
            if self.focused.is_none_or(|id| !self.is_visible(id)) {
                self.focused = self.workspaces[slot].focused;
            }
        } else {
            self.focused = None;
            self.scratchpad_visible = false;
        }
        self.relayout()
    }

    pub fn active_space_id(&self) -> Option<u64> {
        (!self.suspended).then_some(self.space_ids[self.active])
    }

    /// Explicitly moving a shown scratchpad window turns it into a regular tile.
    pub fn release_scratchpad(&mut self, id: WindowId) {
        if self
            .windows
            .get(&id)
            .is_some_and(|m| m.location == Location::Scratchpad)
        {
            self.scratchpad.retain(|w| *w != id);
            self.scratchpad_home.remove(&id);
            self.attach(id, self.active);
        }
    }

    // --- events from the platform -------------------------------------------

    pub fn add_window(&mut self, win: NewWindow) -> Vec<Command> {
        if self.suspended || self.windows.contains_key(&win.id) {
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
        self.hidden.remove(&id);
        self.minimums.remove(&id);
        self.scratchpad_home.remove(&id);
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
    /// Native Space changes are observed separately; stale focus never switches desktops.
    pub fn on_focus_changed(&mut self, id: WindowId) -> Vec<Command> {
        if !self.is_visible(id) || self.focused == Some(id) {
            return Vec::new();
        }
        self.focused = Some(id);
        if let Some(Location::Workspace(slot)) = self.windows.get(&id).map(|m| m.location) {
            self.workspaces[slot].focused = Some(id);
        }
        Vec::new()
    }

    /// The user moved/resized a floating window by hand.
    pub fn on_frame_changed(&mut self, id: WindowId, frame: Rect) {
        // A stretched floating window must retain its pre-fullscreen frame.
        if self
            .workspaces
            .iter()
            .any(|ws| ws.fullscreen.is_some_and(|(f, _)| f == id))
        {
            return;
        }
        let frame = frame.contained_in(self.visible);
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

    /// Restore only the windows hidden by Tyler, leaving manually minimized
    /// windows to the user. Frames remain inside the usable display.
    pub fn restore_all(&mut self) -> Vec<Command> {
        let mut ids: Vec<_> = self.hidden.drain().collect();
        ids.sort_unstable();
        let mut out = Vec::new();
        for id in ids {
            out.push(Command::SetVisible(id, true));
            if let Some(frame) = self.applied.get(&id) {
                out.push(Command::SetFrame(id, frame.contained_in(self.visible)));
            }
        }
        out
    }

    /// An app that cannot fit its tile remains usable as a floating window.
    pub fn float_uncooperative(&mut self, id: WindowId, actual: Rect) -> Vec<Command> {
        let Some(Location::Workspace(ws)) = self.windows.get(&id).map(|m| m.location) else {
            return Vec::new();
        };
        self.detach(id, ws);
        let frame = actual.contained_in(self.visible);
        let m = self.windows.get_mut(&id).unwrap();
        m.floating = true;
        m.float_frame = frame;
        self.attach(id, ws);
        if self.focused == Some(id) {
            self.workspaces[ws].focused = Some(id);
        }
        self.applied.remove(&id);
        self.relayout()
    }

    pub fn on_tile_rejected(&mut self, id: WindowId, desired: Rect, actual: Rect) -> Vec<Command> {
        self.on_tiles_rejected(&[(id, desired, actual)])
    }

    /// Learn all settled rejections before recomputing a layout. Sequentially
    /// repairing each window emits intermediate layouts that invalidate the
    /// next window's request and make the desktop visibly jump several times.
    pub fn on_tiles_rejected(&mut self, rejected: &[(WindowId, Rect, Rect)]) -> Vec<Command> {
        let rejected: Vec<_> = rejected
            .iter()
            .copied()
            .filter(|(id, desired, _)| {
                self.is_visible(*id)
                    && !self.is_floating(*id)
                    && self
                        .applied
                        .get(id)
                        .is_some_and(|frame| frame.approx_eq(desired))
            })
            .collect();
        if rejected.is_empty() {
            return Vec::new();
        }
        for (id, desired, actual) in &rejected {
            let min = self.minimums.entry(*id).or_insert((100.0, 100.0));
            if actual.w > desired.w + 2.0 {
                min.0 = min.0.max(actual.w);
            }
            if actual.h > desired.h + 2.0 {
                min.1 = min.1.max(actual.h);
            }
        }
        let area = self.tiling_area();
        let gaps = self.config.gaps();
        if self.workspaces[self.active]
            .tree
            .constrain(area, gaps, &self.minimums)
        {
            let commands = self.relayout();
            if !commands.is_empty() {
                return commands;
            }
        }
        // An impossible layout (or a window refusing even a valid position)
        // must remain usable. Clear accumulated intermediate frame commands;
        // emit only the final arrangement once all fallbacks are decided.
        let mut commands = Vec::new();
        for (id, _, actual) in rejected {
            commands.extend(self.float_uncooperative(id, actual));
            if self.workspaces[self.active]
                .tree
                .constrain(area, gaps, &self.minimums)
            {
                break;
            }
        }
        commands.retain(|c| !matches!(c, Command::SetFrame(..)));
        commands.extend(self.force_relayout());
        commands
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
            Action::ShowKeybindings => vec![Command::ShowKeybindings],
            Action::FocusMonitor { forward } => vec![Command::FocusMonitor { forward: *forward }],
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
                self.move_to_workspace(*workspace, *follow)
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
        if r.w <= 0.0 || r.h <= 0.0 {
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
            .filter(|(id, _)| self.is_visible(*id))
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
        if m.floating && !self.applied.contains_key(&id) {
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
        if !matches!(self.windows.get(&id).map(|m| m.location), Some(Location::Workspace(slot)) if slot == self.active)
        {
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
        let slot = match target {
            WorkspaceTarget::Index(n) => self.native_order.get(n.wrapping_sub(1)).copied(),
            WorkspaceTarget::Previous => self
                .native_order
                .contains(&self.previous)
                .then_some(self.previous),
            WorkspaceTarget::Relative(step) => {
                let existing: Vec<_> = self
                    .native_order
                    .iter()
                    .copied()
                    .filter(|&slot| {
                        slot == self.active
                            || self.native_occupied.contains(&slot)
                            || !self.workspaces[slot].windows().is_empty()
                    })
                    .collect();
                if existing.is_empty() {
                    return Vec::new();
                }
                let pos = existing
                    .iter()
                    .position(|&slot| slot == self.active)
                    .unwrap_or(0) as i32;
                Some(existing[(pos + step).rem_euclid(existing.len() as i32) as usize])
            }
        };
        match slot {
            Some(slot) if slot != self.active || self.suspended => {
                vec![Command::FocusSpace(self.space_ids[slot])]
            }
            _ => Vec::new(),
        }
    }

    fn move_to_workspace(&mut self, number: usize, follow: bool) -> Vec<Command> {
        let Some(id) = self.focused else {
            return Vec::new();
        };
        let Some(&slot) = self.native_order.get(number.wrapping_sub(1)) else {
            return Vec::new();
        };
        if slot == self.active {
            return Vec::new();
        }
        // Commit membership only after the native operation has succeeded and been observed.
        vec![Command::MoveToSpace {
            window: id,
            space: self.space_ids[slot],
            follow,
        }]
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
        self.scratchpad_home.insert(id, self.active);
        self.scratchpad.push(id);
        self.focused = None;
        match self.workspaces[self.active].focused {
            Some(n) => self.focus_window(n),
            None => self.relayout(),
        }
    }

    fn toggle_scratchpad(&mut self) -> Vec<Command> {
        self.scratchpad_visible = !self.scratchpad_visible
            && self
                .scratchpad
                .iter()
                .any(|id| self.scratchpad_home.get(id) == Some(&self.active));
        if self.scratchpad_visible {
            if let Some(&top) = self
                .scratchpad
                .iter()
                .rev()
                .find(|id| self.scratchpad_home.get(id) == Some(&self.active))
            {
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
        if self.suspended {
            return Vec::new();
        }
        let area = self.tiling_area();
        let gaps = self.config.gaps();
        let mut out = Vec::new();
        for (i, ws) in self.workspaces.iter().enumerate() {
            if i != self.active {
                continue;
            }
            let tiled = ws.tree.layout(area, gaps);
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
                    _ => self.windows[&id].float_frame.contained_in(area),
                };
                out.push((id, frame));
            }
        }
        if self.scratchpad_visible {
            for &id in &self.scratchpad {
                if self.scratchpad_home.get(&id) != Some(&self.active) {
                    continue;
                }
                out.push((id, self.windows[&id].float_frame.contained_in(area)));
            }
        }
        out
    }

    /// Emits `SetFrame` for every window whose desired frame changed.
    fn relayout(&mut self) -> Vec<Command> {
        let mut out = Vec::new();
        if !self.minimums.is_empty() {
            let area = self.tiling_area();
            let gaps = self.config.gaps();
            for ws in &mut self.workspaces {
                ws.tree.constrain(area, gaps, &self.minimums);
            }
        }
        let mut ids = self.managed_windows();
        ids.sort_unstable();
        // macOS handles desktop visibility. Only explicit scratchpad windows
        // may generate minimize/restore commands.
        for id in &ids {
            if self.windows[id].location == Location::Scratchpad
                && !self.is_visible(*id)
                && self.hidden.insert(*id)
            {
                out.push(Command::SetVisible(*id, false));
            }
        }
        for id in ids {
            if self.is_visible(id) && self.hidden.remove(&id) {
                out.push(Command::SetVisible(id, true));
                self.applied.remove(&id);
            }
        }
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
        let mut wm = Wm::new(
            Config::from_layers(crate::config::DEFAULT_CONFIG, Some(user)).unwrap(),
            DISPLAY,
            VISIBLE,
        );
        let desktops = (1..=10).map(|n| (n * 100, Vec::new())).collect::<Vec<_>>();
        wm.sync_native_desktops(&desktops, Some(100));
        wm
    }

    // Model successful OS confirmations separately from the requested command.
    fn perform(wm: &mut Wm, action: &Action) -> Vec<Command> {
        let commands = wm.handle(action);
        let mut desktops: Vec<_> = wm
            .native_order
            .iter()
            .map(|&slot| (wm.space_ids[slot], wm.workspaces[slot].windows()))
            .collect();
        let mut active = wm.active_space_id();
        for command in &commands {
            match command {
                Command::FocusSpace(space) => active = Some(*space),
                Command::MoveToSpace {
                    window,
                    space,
                    follow,
                } => {
                    for (_, members) in &mut desktops {
                        members.retain(|id| id != window);
                    }
                    desktops
                        .iter_mut()
                        .find(|(id, _)| id == space)
                        .unwrap()
                        .1
                        .push(*window);
                    if *follow {
                        active = Some(*space);
                    }
                }
                _ => {}
            }
        }
        wm.sync_native_desktops(&desktops, active);
        commands
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
    fn simultaneous_size_rejections_are_learned_before_emitting_a_layout() {
        let mut wm = wm();
        open(&mut wm, 1);
        open(&mut wm, 2);
        let rejected: Vec<_> = [1, 2]
            .into_iter()
            .map(|id| {
                let desired = frame(&wm, id);
                (
                    id,
                    desired,
                    Rect::new(desired.x, desired.y, desired.w + 100.0, desired.h),
                )
            })
            .collect();
        let commands = wm.on_tiles_rejected(&rejected);
        for (id, desired, _) in rejected {
            assert_eq!(wm.minimums[&id].0, desired.w + 100.0);
            assert!(!wm.is_floating(id));
            assert!(frame(&wm, id).w >= wm.minimums[&id].0);
            assert_eq!(
                commands
                    .iter()
                    .filter(|c| matches!(c, Command::SetFrame(w, _) if *w == id))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn old_rejections_cannot_change_constraints_after_a_new_layout() {
        let mut wm = wm();
        open(&mut wm, 1);
        let old = frame(&wm, 1);
        open(&mut wm, 2);
        assert!(wm
            .on_tiles_rejected(&[(1, old, Rect::new(old.x, old.y, old.w + 100.0, old.h))])
            .is_empty());
        assert!(wm.minimums.is_empty());
        assert!(!wm.is_floating(1));
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
    fn closed_tile_stays_removed_when_native_membership_lags_and_another_opens() {
        let mut wm = wm();
        for id in 1..=3 {
            open(&mut wm, id);
        }
        wm.remove_window(1);
        let covered_area = |wm: &Wm, ids: &[WindowId]| {
            ids.iter()
                .map(|id| {
                    let rect = frame(wm, *id);
                    rect.w * rect.h
                })
                .sum::<f64>()
        };
        assert_eq!(covered_area(&wm, &[2, 3]), VISIBLE.w * VISIBLE.h);
        // Native desktop queries can still include the destroyed window.
        wm.sync_native_desktops(&[(100, vec![1, 2, 3])], Some(100));
        open(&mut wm, 4);
        assert!(!wm.is_managed(1));
        assert_eq!(wm.workspaces[wm.active].tree.windows().len(), 3);
        assert_eq!(covered_area(&wm, &[2, 3, 4]), VISIBLE.w * VISIBLE.h);
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
    fn observed_native_desktops_select_their_own_layouts() {
        let mut wm = wm();
        open(&mut wm, 1);
        perform(&mut wm, &Action::Workspace(WorkspaceTarget::Index(2)));
        assert_eq!(wm.active_workspace(), 2);
        assert!(!wm.is_visible(1));
        assert_eq!(wm.focused(), None);
        open(&mut wm, 2);
        assert_eq!(wm.workspace_of(2), Some(2));
        perform(&mut wm, &Action::Workspace(WorkspaceTarget::Previous));
        assert_eq!(wm.active_workspace(), 1);
        assert!(frame(&wm, 1).approx_eq(&VISIBLE));
        assert!(!wm.is_visible(2));
        assert_eq!(wm.focused(), Some(1));
    }

    #[test]
    fn native_workspace_requests_never_minimize_or_commit_before_confirmation() {
        let mut wm = wm();
        open(&mut wm, 1);
        assert_eq!(
            wm.handle(&Action::MoveToWorkspace {
                workspace: 2,
                follow: true
            }),
            vec![Command::MoveToSpace {
                window: 1,
                space: 200,
                follow: true
            }]
        );
        assert_eq!(
            wm.workspace_of(1),
            Some(1),
            "a failed move must not change native membership"
        );
        let requested = wm.handle(&Action::Workspace(WorkspaceTarget::Index(2)));
        assert_eq!(requested, vec![Command::FocusSpace(200)]);
        assert_eq!(
            wm.active_workspace(),
            1,
            "failed native switch must leave model unchanged"
        );
        let observed = wm.sync_native_desktops(&[(100, vec![1]), (200, vec![])], Some(200));
        assert!(!observed
            .iter()
            .any(|c| matches!(c, Command::SetVisible(..))));
        assert_eq!(wm.active_workspace(), 2);
        assert!(
            wm.restore_all().is_empty(),
            "ordinary desktops require no restoration"
        );
        assert!(
            wm.handle(&Action::Workspace(WorkspaceTarget::Index(3)))
                .is_empty(),
            "no phantom desktops"
        );
    }

    #[test]
    fn deleted_native_desktops_are_not_selectable_and_scratchpads_stay_local() {
        let mut wm = wm();
        open(&mut wm, 1);
        let scratch = wm.handle(&Action::MoveToScratchpad);
        assert!(scratch.contains(&Command::SetVisible(1, false)));
        wm.sync_native_desktops(&[(100, vec![1]), (200, vec![])], Some(200));
        assert!(wm.handle(&Action::ToggleScratchpad).is_empty());
        assert!(!wm.is_visible(1));
        wm.sync_native_desktops(&[(100, vec![1])], Some(100));
        assert!(wm
            .handle(&Action::Workspace(WorkspaceTarget::Previous))
            .is_empty());
        assert!(wm
            .handle(&Action::ToggleScratchpad)
            .contains(&Command::SetVisible(1, true)));
    }

    #[test]
    fn cycling_includes_occupied_native_desktops_before_they_have_been_visited() {
        let mut wm = wm();
        open(&mut wm, 1);
        wm.sync_native_desktops(&[(100, vec![1]), (200, vec![42])], Some(100));
        assert!(!wm.is_managed(42));
        assert_eq!(
            wm.handle(&Action::Workspace(WorkspaceTarget::Relative(1))),
            vec![Command::FocusSpace(200)]
        );
        assert_eq!(wm.occupied_workspaces(), vec![1, 2]);
    }

    #[test]
    fn native_reordering_preserves_layout_and_external_moves_reconcile() {
        let mut wm = wm();
        open(&mut wm, 1);
        open(&mut wm, 2);
        let before = frame(&wm, 1);
        wm.sync_native_desktops(&[(200, vec![]), (100, vec![1, 2])], Some(100));
        assert_eq!(wm.active_workspace(), 2);
        assert_eq!(frame(&wm, 1), before);
        wm.sync_native_desktops(&[(200, vec![2]), (100, vec![1])], Some(100));
        assert_eq!(wm.workspace_of(2), Some(1));
        assert_eq!(frame(&wm, 1), VISIBLE);
        assert_eq!(wm.focused(), Some(1));
        let suspended = wm.sync_native_desktops(&[(200, vec![2]), (100, vec![1])], None);
        assert!(suspended.is_empty());
        assert!(wm.active_space_id().is_none());
        assert!(wm.handle(&Action::ToggleFloat).is_empty());
    }

    #[test]
    fn app_minimum_size_adapts_layout_then_floats_if_impossible() {
        let mut wm = wm();
        open(&mut wm, 1);
        open(&mut wm, 2);
        let desired = frame(&wm, 2);
        wm.on_tile_rejected(2, desired, Rect::new(800.0, 25.0, 1000.0, 975.0));
        assert!(!wm.is_floating(2));
        assert_eq!(frame(&wm, 2).w, 1000.0);
        let desired = frame(&wm, 2);
        wm.on_tile_rejected(2, desired, Rect::new(800.0, 25.0, 2000.0, 1200.0));
        assert!(wm.is_floating(2));
        assert_eq!(wm.workspaces[wm.active].focused, Some(2));
        assert_eq!(frame(&wm, 2), VISIBLE);
        assert_eq!(frame(&wm, 1), VISIBLE);
    }

    #[test]
    fn move_to_workspace_follow_and_silent() {
        let mut wm = wm();
        open(&mut wm, 1);
        open(&mut wm, 2);
        perform(
            &mut wm,
            &Action::MoveToWorkspace {
                workspace: 3,
                follow: false,
            },
        );
        assert_eq!(wm.active_workspace(), 1);
        assert_eq!(wm.workspace_of(2), Some(3));
        assert!(!wm.is_visible(2));
        assert!(frame(&wm, 1).approx_eq(&VISIBLE));
        assert_eq!(wm.focused(), Some(1));
        perform(
            &mut wm,
            &Action::MoveToWorkspace {
                workspace: 3,
                follow: true,
            },
        );
        assert_eq!(wm.active_workspace(), 3);
        assert_eq!(wm.focused(), Some(1));
        assert!(frame(&wm, 2).approx_eq(&Rect::new(0.0, 25.0, 800.0, 975.0)));
        assert_eq!(wm.occupied_workspaces(), vec![3]);
    }

    #[test]
    fn relative_workspace_cycles_through_occupied() {
        let mut wm = wm();
        open(&mut wm, 1);
        perform(
            &mut wm,
            &Action::MoveToWorkspace {
                workspace: 4,
                follow: false,
            },
        );
        open(&mut wm, 2);
        perform(
            &mut wm,
            &Action::MoveToWorkspace {
                workspace: 7,
                follow: false,
            },
        );
        // Active 1 (empty), occupied 4 and 7.
        perform(&mut wm, &Action::Workspace(WorkspaceTarget::Relative(1)));
        assert_eq!(wm.active_workspace(), 4);
        perform(&mut wm, &Action::Workspace(WorkspaceTarget::Relative(1)));
        assert_eq!(wm.active_workspace(), 7);
        perform(&mut wm, &Action::Workspace(WorkspaceTarget::Relative(1)));
        assert_eq!(wm.active_workspace(), 4);
        perform(&mut wm, &Action::Workspace(WorkspaceTarget::Relative(-1)));
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
    fn stretch_focuses_without_hiding_and_allows_switching_windows() {
        let mut wm = wm();
        open(&mut wm, 1);
        open(&mut wm, 2);
        let before = frame(&wm, 2);
        let action = Action::Fullscreen(FullscreenMode::Fullscreen);
        assert_eq!(
            wm.handle(&action),
            vec![Command::SetFrame(2, DISPLAY), Command::Focus(2)]
        );
        assert!(wm.is_visible(1) && wm.is_visible(2));
        // An external app/window switch must not raise the stretched window again.
        assert!(wm.on_focus_changed(1).is_empty());
        assert_eq!(wm.focused(), Some(1));
        assert_eq!(frame(&wm, 2), DISPLAY);
        assert!(wm.on_focus_changed(2).is_empty());
        assert_eq!(
            wm.handle(&action),
            vec![Command::SetFrame(2, before), Command::Focus(2)]
        );
    }

    #[test]
    fn floating_stretch_restores_original_frame_after_observation_and_switch() {
        let mut wm = wm();
        open(&mut wm, 1);
        open(&mut wm, 2);
        wm.handle(&Action::ToggleFloat);
        let original = Rect::new(100.0, 150.0, 600.0, 400.0);
        wm.on_frame_changed(2, original);
        let action = Action::Fullscreen(FullscreenMode::Fullscreen);
        wm.handle(&action);
        wm.on_frame_changed(2, DISPLAY);
        wm.on_focus_changed(1);
        wm.force_relayout();
        assert_eq!(frame(&wm, 2), DISPLAY);
        wm.on_focus_changed(2);
        wm.handle(&action);
        assert_eq!(frame(&wm, 2), original);
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
        assert!(!wm.is_visible(2));
        assert!(frame(&wm, 1).approx_eq(&VISIBLE));
        wm.handle(&Action::ToggleScratchpad);
        assert!(!!wm.is_visible(2));
        assert_eq!(wm.focused(), Some(2));
        perform(&mut wm, &Action::Workspace(WorkspaceTarget::Index(2)));
        assert!(!wm.is_visible(2), "scratchpad hides on workspace change");
    }

    #[test]
    fn stale_focus_cannot_switch_native_desktops() {
        let mut wm = wm();
        open(&mut wm, 1);
        perform(
            &mut wm,
            &Action::MoveToWorkspace {
                workspace: 5,
                follow: false,
            },
        );
        wm.on_focus_changed(1);
        assert_eq!(wm.active_workspace(), 1);
        assert!(!wm.is_visible(1));
        wm.sync_native_desktops(&[(100, vec![]), (500, vec![1])], Some(500));
        wm.on_focus_changed(1);
        assert_eq!(wm.active_space_id(), Some(500));
        assert!(wm.is_visible(1));
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
        perform(
            &mut wm,
            &Action::MoveToWorkspace {
                workspace: 2,
                follow: false,
            },
        );
        let cmds = wm.handle(&Action::Quit);
        assert!(!cmds.iter().any(|c| matches!(c, Command::SetVisible(..))));
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
