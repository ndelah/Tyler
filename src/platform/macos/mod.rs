//! macOS backend: poll the Accessibility tree, apply [`crate::wm::Command`]s,
//! and drain global hotkeys. Runs on the main thread's CFRunLoop.
//!
//! Virtual workspaces hide windows by parking them at the bottom-right corner
//! of the main display (see [`crate::wm::Wm::hidden_origin`]). Quitting via the
//! `exit` binding, or SIGINT/SIGTERM, moves those windows back on screen first.
//!
//! This module typechecks on a cross compile and has never run on a Mac.

mod ax;
mod hotkeys;
mod screens;

use std::collections::HashMap;
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

use crate::config::{self, Config};
use crate::geometry::Rect;
use crate::keys::Chord;
use crate::wm::{Command, NewWindow, Wm};
use crate::WindowId;

const TICK: f64 = 0.05;
const SCAN_EVERY: u32 = 4;
/// Ignore OS focus changes briefly after we handle a binding, so a slow AX
/// update cannot snap the workspace back to where it was.
const FOCUS_GRACE: Duration = Duration::from_millis(500);
/// Wait for a window to arrive at the frame we commanded before treating a
/// mismatch as the user dragging it.
const FRAME_GRACE: Duration = Duration::from_millis(400);

struct Tracked {
    element: ax::AxElement,
    commanded: Rect,
    commanded_at: Instant,
}

struct State {
    wm: Wm,
    tracked: HashMap<WindowId, Tracked>,
    path: std::path::PathBuf,
    queue: Arc<Mutex<Vec<Chord>>>,
    config: Arc<Mutex<Config>>,
    reload: Arc<AtomicBool>,
    stop: &'static AtomicBool,
    ticks: u32,
    ignore_focus_until: Instant,
}

/// Entry point for the `tyler-tiler` binary on macOS.
pub fn run() -> ! {
    if !trusted(false) {
        // Shows the system prompt the first time. The process still has to be
        // relaunched after the user ticks the checkbox.
        let _ = trusted(true);
        eprintln!(
            "tyler-tiler needs Accessibility permission.\n\
             System Settings → Privacy & Security → Accessibility, enable tyler-tiler, then relaunch.\n\
             The hotkey tap also needs Input Monitoring on macOS 13+."
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

    let (display, visible) = screens::main_screen();
    remember_screen(display, visible);
    log::info!("main screen {display:?}, usable {visible:?}");
    let queue = Arc::new(Mutex::new(Vec::new()));
    let shared_config = Arc::new(Mutex::new(config.clone()));
    let reload = Arc::new(AtomicBool::new(false));
    let stop = install_signals();

    let state = Box::leak(Box::new(State {
        wm: Wm::new(config, display, visible),
        tracked: HashMap::new(),
        path: path.clone(),
        queue: Arc::clone(&queue),
        config: Arc::clone(&shared_config),
        reload: Arc::clone(&reload),
        stop,
        ticks: 0,
        ignore_focus_until: Instant::now(),
    }));

    let flag = Arc::clone(&reload);
    match config::watch(&path, move || flag.store(true, Ordering::Relaxed)) {
        Ok(watcher) => std::mem::forget(watcher),
        Err(error) => log::warn!("not watching {}: {error}", path.display()),
    }

    if hotkeys::install(queue, shared_config).is_err() {
        eprintln!("could not install the event tap. Grant Input Monitoring as well as Accessibility, then relaunch.");
        std::process::exit(1);
    }

    let mut context = CFRunLoopTimerContext {
        version: 0,
        info: (state as *mut State).cast::<c_void>(),
        retain: None,
        release: None,
        copyDescription: None,
    };
    let timer = CFRunLoopTimer::new(0.0, TICK, 0, 0, on_timer, &mut context);
    unsafe { CFRunLoop::get_current().add_timer(&timer, kCFRunLoopCommonModes) };
    CFRunLoop::run_current();
    std::process::exit(0);
}

extern "C" fn on_timer(_timer: CFRunLoopTimerRef, info: *mut c_void) {
    let state = unsafe { &mut *info.cast::<State>() };
    state.tick();
}

impl State {
    fn tick(&mut self) {
        if self.stop.load(Ordering::Relaxed) {
            self.shutdown();
        }
        if self.reload.swap(false, Ordering::Relaxed) {
            self.reload_config();
        }
        self.drain_keys();
        if self.ticks % SCAN_EVERY == 0 {
            self.sync_screen();
            self.sync_windows();
        }
        self.ticks = self.ticks.wrapping_add(1);
    }

    fn drain_keys(&mut self) {
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
        for chord in chords {
            let Some(action) = config.action_for(chord).cloned() else {
                continue;
            };
            log::debug!("binding {chord} → {action:?}");
            self.ignore_focus_until = Instant::now() + FOCUS_GRACE;
            let commands = self.wm.handle(&action);
            self.apply(commands);
        }
    }

    fn sync_screen(&mut self) {
        let (display, visible) = screens::main_screen();
        if self.screen_changed(display, visible) {
            log::info!("screen changed, retiling");
            remember_screen(display, visible);
            let commands = self.wm.set_screen(display, visible);
            self.apply(commands);
        }
    }

    fn sync_windows(&mut self) {
        let live = ax::observe_windows();
        let mut seen = std::collections::HashSet::new();
        let mut fresh: HashMap<WindowId, Tracked> = HashMap::new();
        let mut appeared = Vec::new();
        for window in live {
            let id = window.id;
            seen.insert(id);
            let frame = window.frame;
            if !self.wm.is_managed(id) {
                if self.on_main_screen(frame) {
                    appeared.push(window);
                }
                continue;
            }
            if self.wm.is_floating(id) {
                let moved = self.tracked.get(&id).is_some_and(|previous| {
                    previous.commanded_at.elapsed() > FRAME_GRACE
                        && !near(&previous.commanded, &frame)
                        && !self.wm.is_hidden_frame(&frame)
                });
                if moved {
                    self.wm.on_frame_changed(id, frame);
                }
            }
            let commanded = self
                .tracked
                .get(&id)
                .map(|t| (t.commanded, t.commanded_at))
                .unwrap_or((frame, Instant::now()));
            fresh.insert(
                id,
                Tracked {
                    element: window.element,
                    commanded: commanded.0,
                    commanded_at: commanded.1,
                },
            );
        }
        self.tracked = fresh;
        for window in appeared {
            log::info!("managing {} ({})", window.id, window.app);
            let id = window.id;
            self.tracked.insert(
                id,
                Tracked {
                    element: window.element,
                    commanded: window.frame,
                    commanded_at: Instant::now(),
                },
            );
            let commands = self.wm.add_window(NewWindow {
                id,
                app: window.app,
                frame: window.frame,
                prefers_floating: window.prefers_floating,
            });
            self.apply(commands);
        }
        let missing: Vec<WindowId> = self
            .wm
            .managed_windows()
            .into_iter()
            .filter(|id| !seen.contains(id))
            .collect();
        for id in missing {
            log::info!("window {id} gone");
            self.tracked.remove(&id);
            let commands = self.wm.remove_window(id);
            self.apply(commands);
        }
        if Instant::now() >= self.ignore_focus_until {
            if let Some(id) = ax::focused_window_id() {
                if self.wm.is_managed(id) {
                    let commands = self.wm.on_focus_changed(id);
                    self.apply(commands);
                }
            }
        }
    }

    fn apply(&mut self, commands: Vec<Command>) {
        for command in commands {
            match command {
                Command::SetFrame(id, frame) => {
                    if let Some(tracked) = self.tracked.get_mut(&id) {
                        ax::set_frame(&tracked.element, frame);
                        tracked.commanded = frame;
                        tracked.commanded_at = Instant::now();
                    }
                }
                Command::Focus(id) => {
                    if let Some(tracked) = self.tracked.get(&id) {
                        ax::focus(&tracked.element);
                    }
                }
                Command::Close(id) => {
                    if let Some(tracked) = self.tracked.get(&id) {
                        ax::close(&tracked.element);
                    }
                }
                Command::LaunchApp(name) => launch("open", &["-a", &name]),
                Command::OpenWebApp { browser, url } => open_webapp(&browser, &url),
                Command::Exec(shell) => launch("/bin/sh", &["-c", &shell]),
                Command::ReloadConfig => self.reload.store(true, Ordering::Relaxed),
                Command::Quit => self.shutdown(),
            }
        }
    }

    fn reload_config(&mut self) {
        match Config::load(&self.path) {
            Ok(config) => {
                log::info!("reloaded config ({} bindings)", config.bindings.len());
                *self.config.lock().unwrap_or_else(|p| p.into_inner()) = config.clone();
                let commands = self.wm.set_config(config);
                self.apply(commands);
            }
            Err(error) => log::error!("config not applied, keeping the previous one:\n{error}"),
        }
    }

    fn shutdown(&mut self) {
        log::info!("restoring hidden windows and exiting");
        let commands = self.wm.restore_all();
        for command in commands {
            if let Command::SetFrame(id, frame) = command {
                if let Some(tracked) = self.tracked.get(&id) {
                    ax::set_frame(&tracked.element, frame);
                }
            }
        }
        std::process::exit(0);
    }

    fn on_main_screen(&self, frame: Rect) -> bool {
        let (cx, cy) = frame.center();
        let screen = self.screen_full();
        cx >= screen.x && cx < screen.max_x() && cy >= screen.y && cy < screen.max_y()
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

fn install_signals() -> &'static AtomicBool {
    static STOP: AtomicBool = AtomicBool::new(false);
    extern "C" fn handle(_signal: i32) {
        STOP.store(true, Ordering::Relaxed);
    }
    unsafe {
        // `sighandler_t` is an integer on this libc, so go through a pointer.
        let handler = handle as *const () as libc::sighandler_t;
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
    &STOP
}
