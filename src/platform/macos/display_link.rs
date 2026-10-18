//! Display-paced wakeups for the AX animation prototype. The real-time Core
//! Video callback never makes AX calls or waits for the main thread.
//! CVDisplayLink also supports Tyler's macOS 12 deployment target.

use super::events::{Wake, FRAME};
use std::{ffi::c_void, ptr, sync::Arc};

type Link = *mut c_void;
type Callback =
    unsafe extern "C" fn(Link, *const c_void, *const c_void, u64, *mut u64, *mut c_void) -> i32;

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    fn CVDisplayLinkCreateWithCGDisplay(display: u32, link: *mut Link) -> i32;
    fn CVDisplayLinkSetOutputCallback(link: Link, callback: Callback, context: *mut c_void) -> i32;
    fn CVDisplayLinkStart(link: Link) -> i32;
    fn CVDisplayLinkStop(link: Link) -> i32;
    fn CVDisplayLinkRelease(link: Link);
}

pub struct DisplayLink {
    link: Link,
    // Retains callback context until after the display link is stopped.
    _wake: Arc<Wake>,
    running: bool,
}

impl DisplayLink {
    pub fn new(wake: Arc<Wake>) -> Option<Self> {
        let mut link = ptr::null_mut();
        unsafe {
            if CVDisplayLinkCreateWithCGDisplay(
                core_graphics::display::CGDisplay::main().id,
                &mut link,
            ) != 0
            {
                return None;
            }
            if CVDisplayLinkSetOutputCallback(link, on_frame, Arc::as_ptr(&wake).cast_mut().cast())
                != 0
            {
                CVDisplayLinkRelease(link);
                return None;
            }
        }
        Some(Self {
            link,
            _wake: wake,
            running: false,
        })
    }

    pub fn start(&mut self) -> bool {
        if !self.running {
            self.running = unsafe { CVDisplayLinkStart(self.link) == 0 };
        }
        self.running
    }

    pub fn stop(&mut self) {
        if self.running {
            unsafe {
                CVDisplayLinkStop(self.link);
            }
            self.running = false;
        }
    }
}

impl Drop for DisplayLink {
    fn drop(&mut self) {
        self.stop();
        unsafe {
            CVDisplayLinkRelease(self.link);
        }
    }
}

unsafe extern "C" fn on_frame(
    _: Link,
    _: *const c_void,
    _: *const c_void,
    _: u64,
    _: *mut u64,
    context: *mut c_void,
) -> i32 {
    (&*context.cast::<Wake>()).signal(FRAME);
    0
}

/// A read-only probe of display-paced delivery to the main run loop. This
/// deliberately does not move windows or claim to measure presentation FPS.
pub fn check() -> Result<(), String> {
    use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop};
    use std::time::{Duration, Instant};

    struct Probe {
        wake: Arc<Wake>,
        ticks: usize,
        last: Option<Instant>,
        intervals: Vec<Duration>,
    }
    extern "C" fn sample(info: *const c_void) {
        let probe = unsafe { &mut *info.cast_mut().cast::<Probe>() };
        if probe.wake.take() & FRAME == 0 {
            return;
        }
        let now = Instant::now();
        if let Some(last) = probe.last {
            probe.intervals.push(now.duration_since(last));
        }
        probe.last = Some(now);
        probe.ticks += 1;
    }

    let wake = Wake::new();
    let mut link = DisplayLink::new(Arc::clone(&wake))
        .ok_or_else(|| "No display clock is available for the main screen".to_string())?;
    let mut probe = Box::new(Probe {
        wake: Arc::clone(&wake),
        ticks: 0,
        last: None,
        intervals: Vec::new(),
    });
    let source = unsafe { wake.install((&mut *probe as *mut Probe).cast(), sample) };
    if !link.start() {
        wake.uninstall(&source);
        return Err("Could not start the display clock".into());
    }
    unsafe {
        CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, Duration::from_millis(500), false);
    }
    link.stop();
    wake.uninstall(&source);
    if probe.intervals.is_empty() {
        return Err("No display ticks reached the main run loop".into());
    }
    probe.intervals.sort_unstable();
    let p95 = probe.intervals[(probe.intervals.len() - 1) * 95 / 100].as_secs_f64() * 1000.0;
    let average = probe
        .intervals
        .iter()
        .map(Duration::as_secs_f64)
        .sum::<f64>()
        / probe.intervals.len() as f64;
    println!(
        "Display clock: {} main-loop ticks in 500 ms, average interval {:.2} ms, p95 {:.2} ms",
        probe.ticks,
        average * 1000.0,
        p95
    );
    println!(
        "Reduce Motion: {}",
        objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
    );
    println!(
        "Clock delivery only; this does not measure AX throughput or visible animation smoothness."
    );
    Ok(())
}
