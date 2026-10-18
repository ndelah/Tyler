//! macOS backend: observe Accessibility events, apply [`crate::wm::Command`]s,
//! and drain global hotkeys. Runs on the main thread's CFRunLoop.
//!
//! Mission Control owns desktop visibility; yabai handles native Space commands.
//! Only the explicit scratchpad is minimized by Tyler.
//!
//! Native event-callback tests are separate from live permission checks.

mod ax;
mod display_link;
mod events;
mod hotkeys;
mod navigator;
pub use navigator::check as check_navigator;
pub fn navigator_desktops(config: &Config) -> Result<Vec<crate::navigator::Desktop>, String> {
    Ok(crate::navigator::desktops(
        &spaces::Client::new()?.snapshot()?,
        config,
    ))
}
mod screens;
mod spaces;

pub use display_link::check as check_motion;
pub use spaces::check as check_spaces;

use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::process::Command as Process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use accessibility_sys::{
    kAXTrustedCheckOptionPrompt, AXIsProcessTrusted, AXIsProcessTrustedWithOptions,
};
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::runloop::{
    kCFRunLoopCommonModes, CFRunLoop, CFRunLoopTimer, CFRunLoopTimerContext, CFRunLoopTimerRef,
};
use core_foundation::string::CFString;

use crate::action::Action;
use crate::config::{self, Config};
use crate::geometry::Rect;
use crate::keys::Chord;
use crate::motion::Motion;
use crate::spaces::{Snapshot, Transition};
use crate::wm::{Command, NewWindow, Wm};
use crate::WindowId;

const TICK: f64 = 0.05;
const FALLBACK_SCAN: Duration = Duration::from_secs(1);
const SCAN_COALESCE: Duration = Duration::from_millis(50);
const DESKTOP_SETTLE: Duration = Duration::from_millis(150);
/// Ignore OS focus changes briefly after we handle a binding, so a slow AX
/// update cannot snap the workspace back to where it was.
const FOCUS_GRACE: Duration = Duration::from_millis(500);
/// Wait for a window to arrive at the frame we commanded before treating a
/// mismatch as the user dragging it.
const FRAME_GRACE: Duration = Duration::from_millis(120);

struct Tracked {
    element: ax::AxElement,
    app: String,
    hidden_by_tyler: bool,
    failures: u8,
    missing: u8,
    commanded: Rect,
    commanded_at: Instant,
    last_frame: Rect,
    rejected: Option<(Rect, Instant)>,
    session: Option<ax::FrameSession>,
    last_write_ok: bool,
}

struct State {
    navigator: navigator::Navigator,
    wm: Wm,
    native: spaces::Client,
    snapshot: Snapshot,
    native_ready: bool,
    native_error: Option<String>,
    pending: Option<(Transition, Instant)>,
    previous_desktop: Option<u64>,
    tracked: HashMap<WindowId, Tracked>,
    path: std::path::PathBuf,
    queue: Arc<Mutex<Vec<Chord>>>,
    config: Arc<Mutex<Config>>,
    reload: Arc<AtomicBool>,
    stop: &'static AtomicBool,
    recover: &'static AtomicBool,
    wake: Arc<events::Wake>,
    observers: ax::Observers,
    motion: Motion,
    display_link: Option<display_link::DisplayLink>,
    frame_targets: BTreeMap<WindowId, Rect>,
    last_space_sync: Instant,
    last_scan: Instant,
    scan_needed: bool,
    discovery_until: Instant,
    desktop_settle_until: Instant,
    suppress_animation_until: Instant,
    slow_frames: u8,
    animation_disabled: bool,
    animation_frames: usize,
    animation_worst_ms: f64,
    ignore_focus_until: Instant,
}

/// Entry point for the `tyler-tiler` binary on macOS.
pub fn run() -> ! {
    // Keep the file and its advisory lock alive until shutdown. A second tiler
    // would compete for both keyboard events and window positions.
    let _instance_lock = match instance_lock() {
        Ok(lock) => lock,
        Err(error) => {
            eprintln!("Tyler cannot start: {error}");
            std::process::exit(1);
        }
    };
    if !trusted(false) {
        // Shows the system prompt the first time. The process still has to be
        // relaunched after the user ticks the checkbox.
        let _ = trusted(true);
        eprintln!(
            "tyler-tiler needs Accessibility permission.\n\
             System Settings → Privacy & Security → Accessibility: enable your terminal app (Terminal, iTerm, etc.) or tyler-tiler if listed, then fully quit and reopen the terminal.\n\
             When using Tyler.app, add ~/Applications/Tyler.app with the + button instead. See docs/tutorial.md."
        );
        std::process::exit(1);
    }

    let path = config::default_path();
    let config = match Config::load(&path) {
        Ok(config) => config,
        Err(error) => {
            log::error!("config error, using built-in defaults:\n{error}");
            Config::builtin()
        }
    };
    log::info!(
        "{} bindings, super={}, config {}",
        config.bindings.len(),
        config.general.super_mods,
        path.display()
    );

    let (display, visible) = navigator::usable_screen(&config);
    remember_screen(display, visible);
    log::info!("main screen {display:?}, usable {visible:?}");
    let native = match spaces::Client::new() {
        Ok(native) => native,
        Err(error) => {
            eprintln!("Native Spaces unavailable: {error}");
            std::process::exit(78);
        }
    };
    let snapshot = match native.snapshot() {
        Ok(snapshot) if !config.tiling_enabled || !snapshot.layout_conflict() => snapshot,
        Ok(_) => {
            eprintln!("Native Spaces unavailable: yabai must use layout float; Tyler owns tiling. See docs/tutorial.md.");
            std::process::exit(78);
        }
        Err(error) if !config.tiling_enabled => {
            log::warn!("Native desktops unavailable: {error}");
            Snapshot {
                spaces: vec![],
                display: 1,
            }
        }
        Err(error) => {
            eprintln!("Native Spaces unavailable: {error}");
            std::process::exit(78);
        }
    };
    let native_ready = !snapshot.spaces.is_empty();
    let queue = Arc::new(Mutex::new(Vec::new()));
    let shared_config = Arc::new(Mutex::new(config.clone()));
    let reload = Arc::new(AtomicBool::new(false));
    let stop = install_signals();
    let wake = events::Wake::new();
    let now = Instant::now();

    let state = Box::leak(Box::new(State {
        navigator: navigator::Navigator::new(),
        wm: Wm::new(config, display, visible),
        native,
        snapshot,
        native_ready,
        native_error: None,
        pending: None,
        previous_desktop: None,
        tracked: HashMap::new(),
        path: path.clone(),
        queue: Arc::clone(&queue),
        config: Arc::clone(&shared_config),
        reload: Arc::clone(&reload),
        stop,
        recover: &RECOVER,
        wake: Arc::clone(&wake),
        observers: ax::Observers::new(Arc::clone(&wake)),
        motion: Motion::default(),
        display_link: None,
        frame_targets: BTreeMap::new(),
        last_space_sync: now,
        last_scan: now - FALLBACK_SCAN,
        scan_needed: true,
        discovery_until: now + Duration::from_millis(250),
        desktop_settle_until: now,
        suppress_animation_until: now + FALLBACK_SCAN,
        slow_frames: 0,
        animation_disabled: false,
        animation_frames: 0,
        animation_worst_ms: 0.0,
        ignore_focus_until: Instant::now(),
    }));

    state.reconcile_spaces();
    let _event_source = unsafe { wake.install((state as *mut State).cast(), on_event_source) };
    let _workspace_events = events::WorkspaceEvents::new(Arc::clone(&wake));

    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            log::warn!(
                "cannot create config directory {}: {error}",
                parent.display()
            );
        }
    }
    let flag = Arc::clone(&reload);
    let config_wake = Arc::clone(&wake);
    match config::watch(&path, move || {
        flag.store(true, Ordering::Relaxed);
        config_wake.signal(events::CONFIG);
    }) {
        Ok(watcher) => std::mem::forget(watcher),
        Err(error) => log::warn!("not watching {}: {error}", path.display()),
    }

    // CGEventTap invalidates its Mach port when dropped. Do not check a
    // temporary Result with is_err(): that immediately destroys a successful
    // tap. This owner must outlive the run loop below.
    let _hotkey_tap = match hotkeys::install(queue, shared_config, wake) {
        Ok(tap) => tap,
        Err(()) => {
            eprintln!("could not install the event tap. Check Accessibility for Tyler (or your terminal when using the CLI), then relaunch. See docs/tutorial.md.");
            std::process::exit(1);
        }
    };
    eprintln!("Tyler ready: global hotkey tap active");

    let mut context = CFRunLoopTimerContext {
        version: 0,
        info: (state as *mut State).cast::<c_void>(),
        retain: None,
        release: None,
        copyDescription: None,
    };
    let timer = CFRunLoopTimer::new(0.0, TICK, 0, 0, on_timer, &mut context);
    unsafe { CFRunLoop::get_current().add_timer(&timer, kCFRunLoopCommonModes) };
    navigator::run_application();
    std::process::exit(0);
}

fn instance_lock() -> std::io::Result<std::fs::File> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let path = std::env::temp_dir().join(format!("tyler-{}.lock", unsafe { libc::getuid() }));
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(std::io::Error::other(
            "another Tyler instance is already running; quit it first",
        ));
    }
    Ok(lock)
}

extern "C" fn on_timer(_timer: CFRunLoopTimerRef, info: *mut c_void) {
    let state = unsafe { &mut *info.cast::<State>() };
    state.tick();
}

extern "C" fn on_event_source(info: *const c_void) {
    unsafe {
        (&mut *info.cast_mut().cast::<State>()).tick();
    }
}

impl State {
    fn tick(&mut self) {
        if self.stop.load(Ordering::Relaxed) {
            self.shutdown();
        }
        if self.reload.swap(false, Ordering::Relaxed) {
            self.reload_config();
        }
        if !self.wm.config().tiling_enabled && !self.wm.config().navigator.enabled {
            self.navigator
                .update(&self.snapshot, self.wm.config(), self.native_ready);
            self.drain_keys();
            return;
        }
        let flags = self.wake.take();
        let now = Instant::now();
        if flags & events::WINDOWS != 0 {
            // App/window notifications can precede CGWindowList and yabai's
            // membership updates. Retry observations briefly, never frame writes.
            self.discovery_until = now + Duration::from_millis(250);
        }
        if flags & events::CONFIG != 0 && !self.animation_allowed() {
            self.frame_targets.extend(self.motion.finish());
        }
        if flags & (events::WINDOWS | events::GEOMETRY | events::DESKTOP) != 0 {
            self.scan_needed = true;
        }
        if flags & (events::DESKTOP | events::WINDOWS) != 0
            || self.pending.is_some()
            || self.last_space_sync.elapsed() >= FALLBACK_SCAN
            || (now < self.discovery_until && self.last_space_sync.elapsed() >= SCAN_COALESCE)
        {
            self.sync_spaces();
        }
        if let Some(space) = self.navigator.take_selection() {
            if self.wm.config().navigator.enabled && self.native_ready {
                if let Err(error) = self.native.focus(space) {
                    log::warn!("Desktop switch failed: {error}");
                }
                self.sync_spaces();
            }
        }
        self.navigator
            .update(&self.snapshot, self.wm.config(), self.native_ready);
        self.drain_keys();
        if !self.wm.config().tiling_enabled {
            self.recover.store(false, Ordering::Relaxed);
            return;
        }
        if self.recover.swap(false, Ordering::Relaxed)
            && self.native_ready
            && self.pending.is_none()
        {
            let commands = self.wm.force_relayout();
            self.apply(commands);
            eprintln!("Tyler: retiled current workspace");
        }
        if flags & events::FOCUS != 0 && self.native_ready && self.pending.is_none() {
            if let Some(id) = ax::focused_window_id() {
                let commands = self.wm.on_focus_changed(id);
                self.apply(commands);
            }
        }
        let fallback = self.last_scan.elapsed() >= FALLBACK_SCAN;
        if self.native_ready
            && self.pending.is_none()
            && now >= self.desktop_settle_until
            && self.motion.is_empty()
            && (fallback
                || ((self.scan_needed || now < self.discovery_until)
                    && self.last_scan.elapsed() >= SCAN_COALESCE))
        {
            self.scan_needed = false;
            self.observers.watch_apps();
            self.sync_screen();
            self.sync_windows();
            self.last_scan = Instant::now();
        }
        self.flush_frames();
        if flags & events::FRAME != 0 {
            self.animate();
        }
    }

    fn drain_keys(&mut self) {
        if self.pending.is_some() {
            return;
        }
        let chords: Vec<Chord> = self
            .queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .drain(..)
            .collect();
        if chords.is_empty() {
            return;
        }
        let config = self
            .config
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let mut chords = chords.into_iter();
        while let Some(chord) = chords.next() {
            let Some(action) = config.action_for(chord).cloned() else {
                continue;
            };
            if !config.permits(&action) {
                continue;
            }
            if !config.tiling_enabled {
                match action {
                    Action::Workspace(target) => {
                        if !self.sync_spaces() {
                            continue;
                        }
                        let desktops = self.snapshot.desktops();
                        let active = desktops.iter().position(|s| s.visible).unwrap_or(0);
                        let target_id = match target {
                            crate::action::WorkspaceTarget::Index(n) => {
                                desktops.get(n.saturating_sub(1)).map(|s| s.id)
                            }
                            crate::action::WorkspaceTarget::Relative(delta)
                                if !desktops.is_empty() =>
                            {
                                let index = (active as i32 + delta)
                                    .rem_euclid(desktops.len() as i32)
                                    as usize;
                                Some(desktops[index].id)
                            }
                            crate::action::WorkspaceTarget::Previous => self.previous_desktop,
                            _ => None,
                        };
                        if let Some(id) = target_id {
                            if let Err(error) = self.native.focus(id) {
                                log::warn!("Desktop switch failed: {error}");
                            }
                        }
                    }
                    Action::Quit => self.shutdown(),
                    Action::Reload => self.reload.store(true, Ordering::Relaxed),
                    Action::ShowKeybindings => self.apply(vec![Command::ShowKeybindings]),
                    _ => {}
                }
                continue;
            }
            // These actions do not target Tyler's main-display desktop, and
            // must also work when it is fullscreen or its Spaces sync fails.
            if matches!(
                action,
                Action::ShowKeybindings | Action::FocusMonitor { .. }
            ) {
                let commands = self.wm.handle(&action);
                self.apply(commands);
                continue;
            }
            // Always refresh before dispatch: Mission Control and trackpad switches
            // must not leave a shortcut targeting the previous desktop.
            if (!self.sync_spaces() && !matches!(action, Action::Quit | Action::Reload))
                || (self.native_ready
                    && self.wm.active_space_id().is_none()
                    && !matches!(
                        action,
                        Action::Workspace(_)
                            | Action::Launch(_)
                            | Action::Exec(_)
                            | Action::WebApp(_)
                            | Action::Quit
                            | Action::Reload
                    ))
            {
                continue;
            }
            let number = match &action {
                Action::Workspace(crate::action::WorkspaceTarget::Index(n)) => Some(*n),
                Action::MoveToWorkspace { workspace, .. } => Some(*workspace),
                _ => None,
            };
            if number.is_some_and(|n| n > self.snapshot.desktops().len()) {
                eprintln!(
                    "Tyler: desktop {} does not exist; create it in Mission Control first",
                    number.unwrap()
                );
                continue;
            }
            log::debug!("binding {chord} → {action:?}");
            // Target the actual active window even immediately after an app
            // switch, before the periodic scan (or focus grace) catches up.
            if let Some(id) = ax::focused_window_id() {
                let commands = self.wm.on_focus_changed(id);
                self.apply(commands);
            }
            self.ignore_focus_until = Instant::now() + FOCUS_GRACE;
            let commands = self.wm.handle(&action);
            self.apply(commands);
            if self.pending.is_some() {
                // Preserve shortcut order while macOS finishes the native transition.
                self.queue
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .splice(0..0, chords);
                break;
            }
        }
    }

    fn sync_spaces(&mut self) -> bool {
        self.last_space_sync = Instant::now();
        match self.native.snapshot().and_then(|snapshot| {
            if self.wm.config().tiling_enabled && snapshot.layout_conflict() {
                Err("yabai layout conflict: use layout float on the main display; Tyler owns tiling".into())
            } else { Ok(snapshot) }
        }) {
            Ok(snapshot) => {
                let desktop_changed = snapshot.active() != self.snapshot.active()
                    || snapshot.display != self.snapshot.display;
                if desktop_changed {
                    if let Some(id) = self.snapshot.active() { self.previous_desktop = Some(id); }
                    self.cancel_motion();
                    self.frame_targets.clear();
                    self.desktop_settle_until = Instant::now() + DESKTOP_SETTLE;
                    self.suppress_animation_until = self.desktop_settle_until + Duration::from_millis(350);
                    self.scan_needed = true;
                }
                let changed = !self.native_ready || snapshot.display != self.snapshot.display || snapshot.spaces != self.snapshot.spaces;
                self.native_ready = true;
                if self.native_error.take().is_some() { eprintln!("Tyler: native Spaces connection restored"); }
                self.snapshot = snapshot;
                let mut focus_after = None;
                let completed = if let Some((transition, deadline)) = self.pending {
                    if !transition.confirmed(&self.snapshot) && Instant::now() < deadline {
                        return true;
                    }
                    self.pending = None;
                    if transition.confirmed(&self.snapshot) {
                        if let Some(id) = transition.window {
                            self.wm.release_scratchpad(id);
                            if transition.follow { focus_after = Some(id); }
                        }
                    } else {
                        eprintln!("Tyler: native desktop transition was not confirmed; using observed macOS state");
                    }
                    true
                } else { false };
                if changed || completed { self.reconcile_spaces(); }
                if let Some(id) = focus_after {
                    if self.wm.is_visible(id) {
                        let commands = self.wm.on_focus_changed(id);
                        self.apply(commands);
                        if let Some(tracked) = self.tracked.get(&id) { ax::focus(&tracked.element); }
                    }
                }
                true
            }
            Err(error) => {
                self.cancel_motion();
                if self.native_error.as_ref() != Some(&error) {
                    eprintln!("Tyler: native Spaces paused: {error}");
                    self.native_error = Some(error);
                }
                self.native_ready = false;
                if self.pending.is_some_and(|(_, deadline)| Instant::now() >= deadline) { self.pending = None; }
                false
            }
        }
    }

    fn reconcile_spaces(&mut self) {
        if !self.wm.config().tiling_enabled {
            return;
        }
        // Fullscreen, sticky, and other-display windows are owned by macOS.
        let removed: Vec<_> = self
            .tracked
            .keys()
            .copied()
            .filter(|id| {
                self.snapshot.spaces.iter().any(|s| s.windows.contains(id))
                    && self.snapshot.desktop_for_window(*id).is_none()
            })
            .collect();
        let membership_removed = !removed.is_empty();
        for id in removed {
            self.tracked.remove(&id);
            self.wm.remove_window(id); // one final layout is applied after membership sync
        }
        let desktops: Vec<_> = self
            .snapshot
            .desktops()
            .iter()
            .map(|s| {
                (
                    s.id,
                    s.windows
                        .iter()
                        .copied()
                        .filter(|id| self.snapshot.desktop_for_window(*id) == Some(s.id))
                        .collect(),
                )
            })
            .collect();
        let mut commands = self
            .wm
            .sync_native_desktops(&desktops, self.snapshot.active());
        if membership_removed {
            commands.extend(self.wm.force_relayout());
        }
        self.apply(
            commands
                .into_iter()
                .filter(|c| !matches!(c, Command::Focus(_)))
                .collect(),
        );
    }

    fn sync_screen(&mut self) {
        let (display, visible) = navigator::usable_screen(self.wm.config());
        if self.screen_changed(display, visible) {
            self.cancel_motion();
            self.display_link = None;
            self.suppress_animation_until = Instant::now() + FALLBACK_SCAN;
            log::info!("screen changed, retiling");
            remember_screen(display, visible);
            let commands = self.wm.set_screen(display, visible);
            self.apply(commands);
        }
    }

    fn sync_windows(&mut self) {
        let known: Vec<_> = self
            .tracked
            .iter()
            .map(|(id, t)| (*id, &t.element, t.app.as_str()))
            .collect();
        let live = ax::observe_windows(&known);
        self.observers.watch_windows(&live);
        let os_focus = ax::focused_window_id();
        let mut seen = std::collections::HashSet::new();
        let mut appeared = Vec::new();
        let mut repairs = Vec::new();
        let mut failed_writes = Vec::new();
        let mut visibility_repairs = Vec::new();
        let mut removed = Vec::new();
        let full = self.screen_full();
        for window in live {
            let id = window.id;
            seen.insert(id);
            if !self.wm.is_managed(id) {
                // Never pull windows out of other native Spaces, native
                // fullscreen, or the user's minimized windows.
                if window.on_screen
                    && !window.minimized
                    && !window.fullscreen
                    && self
                        .snapshot
                        .desktop_for_window(id)
                        .is_some_and(|space| Some(space) == self.wm.active_space_id())
                    && window.frame.x < full.max_x()
                    && window.frame.max_x() > full.x
                    && window.frame.y < full.max_y()
                    && window.frame.max_y() > full.y
                {
                    appeared.push(window);
                }
                continue;
            }
            let Some(tracked) = self.tracked.get_mut(&id) else {
                continue;
            };
            tracked.missing = 0;
            tracked.element = window.element;
            if window.fullscreen || (window.minimized && !tracked.hidden_by_tyler) {
                removed.push(id);
                continue;
            }
            let visible = self.wm.is_visible(id);
            if tracked.hidden_by_tyler && !window.minimized && !visible {
                visibility_repairs.push(Command::SetVisible(id, visible));
            }
            if tracked.hidden_by_tyler || !visible {
                continue;
            }
            if !window.on_screen {
                continue;
            } // native Space/app hiding
            let frame = window.frame;
            tracked.last_frame = frame;
            if frame_matches(&tracked.app, &tracked.commanded, &frame) {
                tracked.failures = 0;
                tracked.rejected = None;
            } else if tracked.commanded_at.elapsed() > FRAME_GRACE {
                if self.wm.is_floating(id) {
                    self.wm.on_frame_changed(id, frame);
                    tracked.commanded = frame;
                } else if !tracked.last_write_ok {
                    tracked.failures = tracked.failures.saturating_add(1);
                    failed_writes.push((id, tracked.commanded, frame, tracked.failures));
                } else {
                    self.scan_needed = true;
                    if stable_rejection(&mut tracked.rejected, frame, Instant::now()) {
                        repairs.push((id, tracked.commanded, frame));
                        tracked.rejected = None;
                    }
                }
            } else {
                self.scan_needed = true;
            }
        }
        self.apply(visibility_repairs);
        // A native Space can retain a closed window's ID. A destroyed AX
        // handle overrides that stale membership, including for scratchpads.
        // Other AX failures still get the transient-missing grace period.
        let mut destroyed = std::collections::HashSet::new();
        for (id, tracked) in &mut self.tracked {
            if seen.contains(id) {
                continue;
            }
            if ax::is_destroyed(&tracked.element) {
                destroyed.insert(*id);
                removed.push(*id);
            } else if self.snapshot.desktop_for_window(*id).is_none() {
                tracked.missing = tracked.missing.saturating_add(1);
                self.scan_needed = true;
                if tracked.missing >= 2 {
                    removed.push(*id);
                }
            }
        }
        for id in removed {
            // Never forget a window we minimized while the app is briefly
            // unresponsive. The same handle can still be restored on exit.
            if !destroyed.contains(&id)
                && !seen.contains(&id)
                && self.tracked.get(&id).is_some_and(|t| t.hidden_by_tyler)
            {
                continue;
            }
            self.tracked.remove(&id);
            self.motion.remove(id);
            self.frame_targets.remove(&id);
            let commands = self.wm.remove_window(id);
            self.apply(
                commands
                    .into_iter()
                    .filter(|c| !matches!(c, Command::Focus(_)))
                    .collect(),
            );
        }
        // Stable startup order. Preserve the OS focus instead of raising each
        // discovered window, which used to split a random tile on every start.
        appeared.sort_by_key(|w| (Some(w.id) == os_focus, w.id));
        for window in appeared {
            let id = window.id;
            eprintln!("Tyler: managing {id} ({})", window.app);
            self.tracked.insert(
                id,
                Tracked {
                    element: window.element,
                    app: window.app.clone(),
                    commanded: window.frame,
                    commanded_at: Instant::now(),
                    last_frame: window.frame,
                    rejected: None,
                    session: None,
                    last_write_ok: true,
                    hidden_by_tyler: false,
                    failures: 0,
                    missing: 0,
                },
            );
            let commands = self.wm.add_window(NewWindow {
                id,
                app: window.app,
                frame: window.frame,
                prefers_floating: window.prefers_floating,
            });
            self.apply(
                commands
                    .into_iter()
                    .filter(|c| !matches!(c, Command::Focus(_)))
                    .collect(),
            );
        }
        if !repairs.is_empty() {
            log::info!(
                "learning settled size constraints for {} window(s)",
                repairs.len()
            );
            let commands = self.wm.on_tiles_rejected(&repairs);
            self.suppress_animation_until = Instant::now() + Duration::from_millis(200);
            self.apply(commands);
        }
        for (id, desired, actual, attempts) in failed_writes {
            if self
                .wm
                .frame_of(id)
                .is_none_or(|frame| !near(&frame, &desired))
            {
                continue;
            }
            self.suppress_animation_until = Instant::now() + Duration::from_millis(200);
            let commands = if attempts >= 3 {
                log::warn!("AX writes failed repeatedly for window {id}; leaving it floating");
                self.wm.float_uncooperative(id, actual)
            } else {
                vec![Command::SetFrame(id, desired)]
            };
            self.apply(commands);
        }
        if Instant::now() >= self.ignore_focus_until {
            if let Some(id) = os_focus {
                if self.wm.is_managed(id) && self.wm.is_visible(id) {
                    let commands = self.wm.on_focus_changed(id);
                    self.apply(commands);
                }
            }
        }
    }

    fn apply(&mut self, commands: Vec<Command>) {
        for command in commands {
            if !self.wm.config().tiling_enabled
                && matches!(
                    command,
                    Command::SetFrame(..)
                        | Command::Focus(..)
                        | Command::SetVisible(..)
                        | Command::Close(..)
                        | Command::MoveToSpace { .. }
                )
            {
                continue;
            }
            match command {
                Command::FocusSpace(space) => {
                    self.cancel_motion();
                    match self.native.focus(space) {
                        Ok(()) => {
                            self.pending = Some((
                                Transition {
                                    space,
                                    window: None,
                                    follow: true,
                                },
                                Instant::now() + Duration::from_secs(2),
                            ))
                        }
                        Err(error) => {
                            eprintln!("Tyler: could not switch native desktop: {error}");
                            let commands = self.wm.force_relayout();
                            self.apply(commands);
                        }
                    }
                    self.sync_spaces();
                }
                Command::MoveToSpace {
                    window,
                    space,
                    follow,
                } => {
                    self.cancel_motion();
                    match self.native.move_window(window, space) {
                        Ok(()) => {
                            let followed = if follow {
                                match self.native.focus(space) {
                                    Ok(()) => true,
                                    Err(error) => {
                                        eprintln!("Tyler: window moved, but could not follow desktop: {error}");
                                        false
                                    }
                                }
                            } else {
                                false
                            };
                            self.pending = Some((
                                Transition {
                                    space,
                                    window: Some(window),
                                    follow: followed,
                                },
                                Instant::now() + Duration::from_secs(2),
                            ));
                        }
                        Err(error) => {
                            eprintln!("Tyler: could not move window to native desktop: {error}");
                            let commands = self.wm.force_relayout();
                            self.apply(commands);
                        }
                    }
                    self.sync_spaces();
                }
                Command::SetFrame(id, frame) => {
                    // Discovery, constraint learning and rapid key repeats can
                    // all change the same tile in one turn. Send only its final
                    // destination after the event batch is complete.
                    self.frame_targets.insert(id, frame);
                }
                Command::SetVisible(id, visible) => {
                    self.motion.remove(id);
                    self.frame_targets.remove(&id);
                    if let Some(tracked) = self.tracked.get_mut(&id) {
                        tracked.session = None;
                        if visible && !tracked.hidden_by_tyler {
                            continue;
                        }
                        if ax::set_minimized(&tracked.element, !visible) {
                            tracked.hidden_by_tyler = !visible;
                            tracked.commanded_at = Instant::now();
                            tracked.failures = 0;
                        } else {
                            eprintln!(
                                "Tyler: could not {} window {id}",
                                if visible { "restore" } else { "minimize" }
                            );
                        }
                    }
                }
                Command::Focus(id) => {
                    if !self.native_ready || !self.wm.is_visible(id) {
                        continue;
                    }
                    if let Some(tracked) = self.tracked.get(&id) {
                        ax::focus(&tracked.element);
                    }
                }
                Command::Close(id) => {
                    self.motion.remove(id);
                    self.frame_targets.remove(&id);
                    if let Some(tracked) = self.tracked.get_mut(&id) {
                        tracked.session = None;
                        ax::close(&tracked.element);
                    }
                }
                Command::LaunchApp(name) => launch("open", &["-a", &name]),
                Command::ShowKeybindings => {
                    // Address the app owning this bundled worker explicitly.
                    let bundle = std::env::current_exe().ok().and_then(|path| {
                        path.parent()?
                            .parent()?
                            .parent()
                            .map(std::path::Path::to_path_buf)
                    });
                    if let Some(bundle) =
                        bundle.filter(|p| p.extension().is_some_and(|e| e == "app"))
                    {
                        launch(
                            "open",
                            &["-a", &bundle.to_string_lossy(), "tyler://keybindings"],
                        );
                    } else {
                        launch("open", &["-b", "com.ndelah.tyler", "tyler://keybindings"]);
                    }
                }
                Command::FocusMonitor { forward } => {
                    if let Err(error) = self.native.focus_display(forward) {
                        eprintln!("Tyler: could not focus monitor: {error}");
                    }
                }
                Command::OpenWebApp { browser, url } => open_webapp(&browser, &url),
                Command::Exec(shell) => launch("/bin/sh", &["-c", &shell]),
                Command::ReloadConfig => self.reload.store(true, Ordering::Relaxed),
                Command::Quit => self.shutdown(),
            }
        }
    }

    fn animation_allowed(&self) -> bool {
        self.wm.config().animations.enabled
            && !self.animation_disabled
            && !objc2_app_kit::NSWorkspace::sharedWorkspace()
                .accessibilityDisplayShouldReduceMotion()
    }

    fn can_frame(&self, id: WindowId) -> bool {
        self.native_ready
            && self.pending.is_none()
            && self.wm.is_visible(id)
            && self.snapshot.desktop_for_window(id) == self.wm.active_space_id()
    }

    fn frame_area(&self, frame: Rect) -> Rect {
        let cache = cache().lock().unwrap_or_else(|p| p.into_inner());
        if near(&frame, &cache.full) {
            cache.full
        } else {
            cache.visible
        }
    }

    fn flush_frames(&mut self) {
        if !self.native_ready
            || self.pending.is_some()
            || Instant::now() < self.desktop_settle_until
        {
            return;
        }
        let animate = self.animation_allowed() && Instant::now() >= self.suppress_animation_until;
        if animate && !self.frame_targets.is_empty() && self.display_link.is_none() {
            self.display_link = display_link::DisplayLink::new(Arc::clone(&self.wake));
            if self.display_link.is_none() {
                log::warn!("Display clock unavailable; using immediate tiling for this session");
                self.animation_disabled = true;
            }
        }
        let animate = animate
            && !self.frame_targets.is_empty()
            && self.display_link.as_mut().is_some_and(|link| link.start());
        let duration = Duration::from_millis(self.wm.config().animations.duration_ms);
        let now = Instant::now();
        let mut targets: Vec<_> = std::mem::take(&mut self.frame_targets)
            .into_iter()
            .collect();
        // Shrink before expanding to reduce transient overlaps during immediate
        // layouts. Ordering is stable for windows whose areas do not change.
        targets.sort_by_key(|(id, target)| {
            self.tracked
                .get(id)
                .is_some_and(|t| target.w * target.h > t.last_frame.w * t.last_frame.h)
        });
        for (id, target) in targets {
            if !self.can_frame(id) {
                continue;
            }
            let area = self.frame_area(target);
            let Some(tracked) = self.tracked.get_mut(&id) else {
                continue;
            };
            let was_animating = self.motion.contains(id);
            let from = if was_animating {
                tracked.last_frame
            } else {
                ax::frame(&tracked.element).unwrap_or(tracked.last_frame)
            };
            if !near(&tracked.commanded, &target) {
                tracked.failures = 0;
            }
            tracked.commanded = target;
            tracked.commanded_at = now;
            tracked.rejected = None;
            if animate && !from.approx_eq(&target) {
                tracked.last_write_ok = true;
                if tracked.session.is_none() {
                    tracked.session = Some(ax::FrameSession::new(&tracked.element));
                }
                tracked.last_frame = from;
                self.motion.retarget(id, from, target, now, duration);
            } else {
                self.motion.remove(id);
                let (actual, accepted) = ax::apply_frame(&tracked.element, target, area);
                tracked.last_frame = actual.unwrap_or(from);
                tracked.last_write_ok = accepted;
                tracked.session = None;
            }
            self.scan_needed = true;
        }
        self.stop_idle_display_link();
    }

    fn animate(&mut self) {
        if self.motion.is_empty() {
            return;
        }
        if !self.native_ready || self.pending.is_some() {
            self.cancel_motion();
            return;
        }
        let started = Instant::now();
        let frames = self.motion.samples(started);
        for (id, frame, done) in frames {
            if !self.can_frame(id) {
                self.motion.remove(id);
                if let Some(tracked) = self.tracked.get_mut(&id) {
                    tracked.session = None;
                }
                continue;
            }
            let area = self.frame_area(frame);
            let Some(tracked) = self.tracked.get_mut(&id) else {
                self.motion.remove(id);
                continue;
            };
            if done {
                let (actual, accepted) = ax::apply_frame(&tracked.element, frame, area);
                tracked.last_frame = actual.unwrap_or(tracked.last_frame);
                tracked.last_write_ok = accepted;
                tracked.session = None;
                tracked.commanded_at = Instant::now();
                self.scan_needed = true;
            } else if !tracked.last_frame.approx_eq(&frame) {
                if ax::set_frame_step(&tracked.element, Some(tracked.last_frame), frame).is_ok() {
                    tracked.last_frame = frame;
                } else {
                    self.motion.remove(id);
                    tracked.session = None;
                    tracked.last_write_ok = false;
                    self.scan_needed = true;
                }
            }
        }
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        self.animation_frames += 1;
        self.animation_worst_ms = self.animation_worst_ms.max(elapsed);
        if elapsed > 24.0 {
            self.slow_frames = self.slow_frames.saturating_add(1);
        }
        if self.slow_frames >= 3 {
            log::warn!("AX animation exceeded 24 ms for three frame batches; using immediate tiling for this session");
            self.animation_disabled = true;
            self.frame_targets.extend(self.motion.finish());
            self.flush_frames();
        }
        self.stop_idle_display_link();
    }

    fn stop_idle_display_link(&mut self) {
        if self.motion.is_empty() {
            if let Some(link) = &mut self.display_link {
                link.stop();
            }
            if self.animation_frames > 0 {
                log::info!("AX animation: {} submitted frame batches, slowest AX batch {:.1} ms (not presentation timing)", self.animation_frames, self.animation_worst_ms);
                self.animation_frames = 0;
                self.animation_worst_ms = 0.0;
            }
        }
    }

    fn cancel_motion(&mut self) {
        // Keep final destinations for windows that remain on the active
        // desktop (e.g. a silent move of another window or a failed switch).
        // A confirmed desktop change clears these before reconciling its layout.
        self.frame_targets.extend(self.motion.finish());
        for tracked in self.tracked.values_mut() {
            tracked.session = None;
        }
        self.stop_idle_display_link();
    }

    fn reload_config(&mut self) {
        match Config::load(&self.path) {
            Ok(config) => {
                log::info!("reloaded config ({} bindings)", config.bindings.len());
                *self.config.lock().unwrap_or_else(|p| p.into_inner()) = config.clone();
                if !config.tiling_enabled {
                    self.motion.finish();
                    self.frame_targets.clear();
                    self.stop_idle_display_link();
                    for tracked in self.tracked.values() {
                        if tracked.hidden_by_tyler {
                            ax::set_minimized(&tracked.element, false);
                        }
                    }
                    for id in self.wm.managed_windows() {
                        self.wm.remove_window(id);
                    }
                    self.tracked.clear();
                }
                self.scan_needed = true;
                self.last_scan = Instant::now() - FALLBACK_SCAN;
                self.animation_disabled = false;
                self.slow_frames = 0;
                if !config.animations.enabled {
                    self.frame_targets.extend(self.motion.finish());
                }
                let commands = self.wm.set_config(config);
                self.apply(commands);
                self.reconcile_spaces();
                self.sync_screen();
            }
            Err(error) => log::error!("config not applied, keeping the previous one:\n{error}"),
        }
    }

    fn shutdown(&mut self) {
        self.cancel_motion();
        eprintln!("Tyler: restoring scratchpad windows and exiting; native desktops unchanged");
        for tracked in self.tracked.values() {
            if tracked.hidden_by_tyler {
                ax::set_minimized(&tracked.element, false);
                // Leave native desktop membership and frames unchanged.
            }
        }
        std::process::exit(0);
    }
}

/// Screen geometry cached next to the WM so a display change can be detected
/// without poking at private WM fields.
struct ScreenCache {
    full: Rect,
    visible: Rect,
}

fn remember_screen(full: Rect, visible: Rect) {
    let mut cache = cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.full = full;
    cache.visible = visible;
}

fn cache() -> &'static Mutex<ScreenCache> {
    static CACHE: std::sync::OnceLock<Mutex<ScreenCache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(ScreenCache {
            full: Rect::new(0.0, 0.0, 0.0, 0.0),
            visible: Rect::new(0.0, 0.0, 0.0, 0.0),
        })
    })
}

impl State {
    fn screen_changed(&self, full: Rect, visible: Rect) -> bool {
        let cache = cache().lock().unwrap_or_else(|p| p.into_inner());
        !near(&cache.full, &full) || !near(&cache.visible, &visible)
    }

    fn screen_full(&self) -> Rect {
        cache().lock().unwrap_or_else(|p| p.into_inner()).full
    }
}

fn near(a: &Rect, b: &Rect) -> bool {
    (a.x - b.x).abs() < 2.0
        && (a.y - b.y).abs() < 2.0
        && (a.w - b.w).abs() < 2.0
        && (a.h - b.h).abs() < 2.0
}

fn frame_matches(app: &str, a: &Rect, b: &Rect) -> bool {
    // Terminal windows snap to character cells rather than arbitrary points.
    // Accept one cell of size rounding while still requiring exact placement.
    if ["Terminal", "iTerm", "iTerm2", "Ghostty"].contains(&app) {
        (a.x - b.x).abs() < 2.0
            && (a.y - b.y).abs() < 2.0
            && (a.w - b.w).abs() <= 16.0
            && (a.h - b.h).abs() <= 24.0
    } else {
        near(a, b)
    }
}

/// A delayed AX reply must not become a permanent minimum-size constraint.
/// Require two unchanged observations separated by a quiet interval.
fn stable_rejection(previous: &mut Option<(Rect, Instant)>, actual: Rect, now: Instant) -> bool {
    if let Some((sample, since)) = previous {
        if near(sample, &actual) {
            return now.saturating_duration_since(*since) >= Duration::from_millis(60);
        }
    }
    *previous = Some((actual, now));
    false
}

fn spawn(mut command: Process) {
    let program = command.get_program().to_string_lossy().into_owned();
    if let Err(error) = command.spawn() {
        log::error!("failed to spawn {program}: {error}");
    }
}

fn launch(program: &str, args: &[&str]) {
    let mut command = Process::new(program);
    command.args(args);
    spawn(command);
}

fn open_webapp(browser: &str, url: &str) {
    let lower = browser.to_ascii_lowercase();
    let chromium = ["chrome", "chromium", "brave", "edge", "arc", "vivaldi"]
        .iter()
        .any(|name| lower.contains(name));
    if chromium {
        let app_arg = format!("--app={url}");
        launch("open", &["-na", browser, "--args", &app_arg]);
    } else {
        launch("open", &["-a", browser, url]);
    }
}

fn trusted(prompt: bool) -> bool {
    if !prompt {
        return unsafe { AXIsProcessTrusted() };
    }
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let options = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) }
}

static RECOVER: AtomicBool = AtomicBool::new(false);

fn install_signals() -> &'static AtomicBool {
    static STOP: AtomicBool = AtomicBool::new(false);
    extern "C" fn handle(_signal: i32) {
        if _signal == libc::SIGUSR1 {
            RECOVER.store(true, Ordering::Relaxed);
        } else {
            STOP.store(true, Ordering::Relaxed);
        }
    }
    unsafe {
        // `sighandler_t` is an integer on this libc, so go through a pointer.
        let handler = handle as *const () as libc::sighandler_t;
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGUSR1, handler);
    }
    &STOP
}

#[cfg(test)]
mod frame_tests {
    use super::*;
    #[test]
    fn transient_readbacks_do_not_become_permanent_minimum_sizes() {
        let now = Instant::now();
        let first = Rect::new(0.0, 30.0, 900.0, 600.0);
        let settling = Rect::new(0.0, 30.0, 600.0, 600.0);
        let mut previous = None;
        assert!(!stable_rejection(&mut previous, first, now));
        assert!(!stable_rejection(
            &mut previous,
            settling,
            now + Duration::from_millis(80)
        ));
        assert!(!stable_rejection(
            &mut previous,
            settling,
            now + Duration::from_millis(100)
        ));
        assert!(stable_rejection(
            &mut previous,
            settling,
            now + Duration::from_millis(150)
        ));
    }
    #[test]
    fn terminal_cell_rounding_does_not_trigger_floating() {
        let requested = Rect::new(1344.0, 847.5, 438.0, 262.5);
        let rounded = Rect::new(1344.0, 848.0, 439.0, 258.0);
        assert!(frame_matches("Terminal", &requested, &rounded));
        assert!(!frame_matches("ChatGPT", &requested, &rounded));
        assert!(!frame_matches(
            "Terminal",
            &requested,
            &Rect::new(1500.0, 848.0, 439.0, 258.0)
        ));
    }
}
