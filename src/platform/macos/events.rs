//! Callbacks only signal a coalescing run-loop source. AX/IPC work happens
//! after the event tap and notification callbacks return.

use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::Arc;

use block2::RcBlock;
use core_foundation::base::TCFType;
use core_foundation::runloop::*;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSWorkspace, NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
    NSWorkspaceActiveSpaceDidChangeNotification, NSWorkspaceDidActivateApplicationNotification,
    NSWorkspaceDidHideApplicationNotification, NSWorkspaceDidLaunchApplicationNotification,
    NSWorkspaceDidTerminateApplicationNotification, NSWorkspaceDidUnhideApplicationNotification,
    NSWorkspaceDidWakeNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol};

pub const KEYS: usize = 1;
pub const WINDOWS: usize = 2;
pub const GEOMETRY: usize = 4;
pub const FOCUS: usize = 8;
pub const DESKTOP: usize = 16;
pub const FRAME: usize = 32;
pub const CONFIG: usize = 64;

pub struct Wake {
    flags: AtomicUsize,
    source: AtomicPtr<c_void>,
    run_loop: CFRunLoop,
}

impl Wake {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            flags: AtomicUsize::new(0),
            source: AtomicPtr::new(ptr::null_mut()),
            run_loop: CFRunLoop::get_current(),
        })
    }

    /// `info` must outlive the source, and must only be dereferenced by the
    /// main-thread perform callback. Tyler's State has process lifetime.
    pub unsafe fn install(
        &self,
        info: *mut c_void,
        perform: extern "C" fn(*const c_void),
    ) -> CFRunLoopSource {
        let mut context = CFRunLoopSourceContext {
            version: 0,
            info,
            retain: None,
            release: None,
            copyDescription: None,
            equal: None,
            hash: None,
            schedule: None,
            cancel: None,
            perform,
        };
        let raw = CFRunLoopSourceCreate(ptr::null(), 0, &mut context);
        assert!(!raw.is_null(), "cannot create Tyler event source");
        let source = CFRunLoopSource::wrap_under_create_rule(raw);
        self.run_loop.add_source(&source, kCFRunLoopCommonModes);
        self.source.store(raw.cast(), Ordering::Release);
        source
    }

    pub fn signal(&self, flags: usize) {
        self.flags.fetch_or(flags, Ordering::Release);
        let source = self.source.load(Ordering::Acquire);
        if !source.is_null() {
            unsafe {
                CFRunLoopSourceSignal(source.cast());
                CFRunLoopWakeUp(self.run_loop.as_concrete_TypeRef());
            }
        }
    }

    pub fn take(&self) -> usize {
        self.flags.swap(0, Ordering::AcqRel)
    }

    pub fn uninstall(&self, source: &CFRunLoopSource) {
        self.source.store(ptr::null_mut(), Ordering::Release);
        unsafe {
            self.run_loop.remove_source(source, kCFRunLoopCommonModes);
            CFRunLoopSourceInvalidate(source.as_concrete_TypeRef());
        }
    }
}

/// Public workspace notifications cover newly launched apps and desktop
/// switches, including trackpad gestures that never pass through Tyler.
pub struct WorkspaceEvents {
    center: Retained<NSNotificationCenter>,
    tokens: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

impl WorkspaceEvents {
    pub fn new(wake: Arc<Wake>) -> Self {
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        let mut tokens = Vec::new();
        unsafe {
            for (name, flags) in [
                (
                    NSWorkspaceActiveSpaceDidChangeNotification,
                    DESKTOP | WINDOWS,
                ),
                (NSWorkspaceDidLaunchApplicationNotification, WINDOWS),
                (NSWorkspaceDidTerminateApplicationNotification, WINDOWS),
                (
                    NSWorkspaceDidActivateApplicationNotification,
                    FOCUS | DESKTOP,
                ),
                (NSWorkspaceDidHideApplicationNotification, WINDOWS),
                (NSWorkspaceDidUnhideApplicationNotification, WINDOWS),
                (NSWorkspaceDidWakeNotification, WINDOWS | DESKTOP),
                (
                    NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
                    CONFIG,
                ),
            ] {
                let wake = Arc::clone(&wake);
                let callback = RcBlock::new(move |_: NonNull<NSNotification>| wake.signal(flags));
                tokens.push(center.addObserverForName_object_queue_usingBlock(
                    Some(name),
                    None,
                    None,
                    &callback,
                ));
            }
        }
        Self { center, tokens }
    }
}

impl Drop for WorkspaceEvents {
    fn drop(&mut self) {
        for token in &self.tokens {
            unsafe {
                self.center.removeObserver((**token).as_ref());
            }
        }
    }
}
