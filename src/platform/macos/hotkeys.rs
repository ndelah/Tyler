//! Global hotkeys via a `CGEventTap`.
//!
//! The tap swallows key-down and key-up for chords present in the config so
//! the focused application does not also see them (the same way Hyprland
//! consumes Super bindings). The callback only enqueues the chord. Running
//! Accessibility calls inside the tap deadlocks the event stream, so the run
//! loop timer drains the queue.
//!
//! Creating the tap requires Accessibility permission (and, on recent macOS,
//! Input Monitoring). Untested: this file is typechecked, never executed.

use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};

use core_foundation::base::TCFType;
use core_foundation::mach_port::CFMachPortRef;
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType, CallbackResult,
};

use crate::config::Config;
use crate::keys::{Chord, Modifiers};

static TAP_PORT: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
}

/// Installs the session event tap on the current thread's run loop.
///
/// The returned tap must be kept alive for as long as the run loop runs.
pub fn install(
    queue: Arc<Mutex<Vec<Chord>>>,
    config: Arc<Mutex<Config>>,
) -> Result<CGEventTap<'static>, ()> {
    let tap = CGEventTap::new(
        CGEventTapLocation::Session,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![
            CGEventType::KeyDown,
            CGEventType::KeyUp,
            CGEventType::FlagsChanged,
        ],
        move |_proxy: CGEventTapProxy, event_type, event: &CGEvent| {
            on_event(event_type, event, &queue, &config)
        },
    )?;
    let source = tap.mach_port().create_runloop_source(0).map_err(|_| ())?;
    unsafe {
        CFRunLoop::get_current().add_source(&source, kCFRunLoopCommonModes);
    }
    // The run loop retains the source. Forgetting our reference keeps it
    // alive even if this function's locals were reordered.
    std::mem::forget(source);
    TAP_PORT.store(
        tap.mach_port().as_concrete_TypeRef() as *mut c_void,
        Ordering::Relaxed,
    );
    tap.enable();
    Ok(tap)
}

fn on_event(
    event_type: CGEventType,
    event: &CGEvent,
    queue: &Mutex<Vec<Chord>>,
    config: &Mutex<Config>,
) -> CallbackResult {
    if matches!(
        event_type,
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
    ) {
        let port = TAP_PORT.load(Ordering::Relaxed) as CFMachPortRef;
        if !port.is_null() {
            unsafe { CGEventTapEnable(port, true) };
            log::warn!("event tap was disabled; re-enabled it");
        }
        return CallbackResult::Keep;
    }
    let kind = event_type as u32;
    if kind != CGEventType::KeyDown as u32 && kind != CGEventType::KeyUp as u32 {
        return CallbackResult::Keep;
    }
    let keycode = event
        .get_integer_value_field(core_graphics::event::EventField::KEYBOARD_EVENT_KEYCODE)
        as u16;
    let chord = Chord {
        keycode,
        mods: modifiers(event.get_flags()),
    };
    let decision = {
        let config = config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        config.action_for(chord).map(crate::action::Action::repeats)
    };
    let Some(repeats) = decision else {
        return CallbackResult::Keep;
    };
    if kind == CGEventType::KeyDown as u32 {
        let autorepeat = event
            .get_integer_value_field(core_graphics::event::EventField::KEYBOARD_EVENT_AUTOREPEAT)
            != 0;
        if !autorepeat || repeats {
            queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(chord);
        }
    }
    // Swallow the matching key-up as well, so the focused app does not see a
    // press without a release (or the reverse).
    CallbackResult::Drop
}

fn modifiers(flags: CGEventFlags) -> Modifiers {
    let mut mods = Modifiers::NONE;
    if flags.contains(CGEventFlags::CGEventFlagShift) {
        mods |= Modifiers::SHIFT;
    }
    if flags.contains(CGEventFlags::CGEventFlagControl) {
        mods |= Modifiers::CTRL;
    }
    if flags.contains(CGEventFlags::CGEventFlagAlternate) {
        mods |= Modifiers::ALT;
    }
    if flags.contains(CGEventFlags::CGEventFlagCommand) {
        mods |= Modifiers::CMD;
    }
    mods
}
