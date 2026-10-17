//! Global hotkeys via a `CGEventTap`.
//!
//! The tap swallows key-down and key-up for chords present in the config so
//! the focused application does not also see them (the same way Hyprland
//! consumes Super bindings). The callback only enqueues the chord. Running
//! Accessibility calls inside the tap deadlocks the event stream, so the run
//! loop timer drains the queue.
//!
//! Creating the tap requires Accessibility permission (and, on recent macOS,
//! Input Monitoring). Native callback tests construct events without posting
//! them; live event-tap installation still requires an interactive permission grant.

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
use crate::keys::{Chord, ConsumedKeys, Modifiers};

static TAP_PORT: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

#[derive(Default)]
struct KeyboardState {
    consumed: ConsumedKeys,
    fn_down: bool,
}

fn fn_keycode(key: u16) -> u16 {
    match key {
        0x73 => 0x7b,
        0x77 => 0x7c,
        0x74 => 0x7e,
        0x79 => 0x7d,
        0x4c => 0x24,
        0x75 => 0x33,
        other => other,
    }
}

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
    let consumed = Mutex::new(KeyboardState::default());
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
            on_event(event_type, event, &queue, &config, &consumed)
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
    consumed: &Mutex<KeyboardState>,
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
    let raw_keycode = event
        .get_integer_value_field(core_graphics::event::EventField::KEYBOARD_EVENT_KEYCODE)
        as u16;
    let mut keyboard = consumed.lock().unwrap_or_else(|p| p.into_inner());
    if matches!(event_type, CGEventType::FlagsChanged) && raw_keycode == 0x3f {
        keyboard.fn_down = event
            .get_flags()
            .contains(CGEventFlags::CGEventFlagSecondaryFn);
    }
    let kind = event_type as u32;
    if kind != CGEventType::KeyDown as u32 && kind != CGEventType::KeyUp as u32 {
        return CallbackResult::Keep;
    }
    // Fn+arrows/Return/Delete may arrive as Home/End/PageUp/PageDown/
    // keypad Enter/forward Delete. Normalize only while the physical Fn key
    // is held: dedicated navigation keys must keep their normal behavior.
    let keycode = if keyboard.fn_down {
        fn_keycode(raw_keycode)
    } else {
        raw_keycode
    };
    let mut mods = modifiers(event.get_flags());
    if keyboard.fn_down {
        mods |= Modifiers::FN;
    } else if matches!(raw_keycode, 0x73..=0x7e | 0x60..=0x71) {
        // macOS also sets SecondaryFn on standalone function/navigation keys.
        // It does not mean that the physical Fn modifier is held.
        mods = modifiers(event.get_flags() - CGEventFlags::CGEventFlagSecondaryFn);
    }
    let chord = Chord { keycode, mods };
    let decision = {
        let config = config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        config.action_for(chord).map(crate::action::Action::repeats)
    };
    let down = kind == CGEventType::KeyDown as u32;
    // Pair either representation if Fn is released before the navigation key.
    let swallow = keyboard
        .consumed
        .filter(fn_keycode(raw_keycode), down, decision.is_some());
    drop(keyboard);
    if !swallow {
        return CallbackResult::Keep;
    }
    if down && decision.is_some() {
        let repeats = decision.unwrap();
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
    // Pair releases by physical key, even when modifiers change first.
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
    if flags.contains(CGEventFlags::CGEventFlagSecondaryFn) {
        mods |= Modifiers::FN;
    }
    mods
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_graphics::event::EventField;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    // Construct events locally; never post them or install a global event tap.
    fn event(chord: Chord, down: bool) -> CGEvent {
        let source = CGEventSource::new(CGEventSourceStateID::Private).unwrap();
        let event = CGEvent::new_keyboard_event(source, chord.keycode, down).unwrap();
        let mut flags = CGEventFlags::empty();
        for (modifier, flag) in [
            (Modifiers::CTRL, CGEventFlags::CGEventFlagControl),
            (Modifiers::ALT, CGEventFlags::CGEventFlagAlternate),
            (Modifiers::CMD, CGEventFlags::CGEventFlagCommand),
            (Modifiers::SHIFT, CGEventFlags::CGEventFlagShift),
            (Modifiers::FN, CGEventFlags::CGEventFlagSecondaryFn),
        ] {
            if chord.mods.contains(modifier) {
                flags |= flag;
            }
        }
        event.set_flags(flags);
        event
    }

    #[test]
    fn every_default_dispatches_from_native_keyboard_events() {
        let builtin = Config::builtin();
        let config = Mutex::new(builtin.clone());
        let queue = Mutex::new(Vec::new());
        let consumed = Mutex::new(KeyboardState::default());
        for binding in builtin.bindings {
            let fn_event = event(
                Chord {
                    keycode: 0x3f,
                    mods: binding.chord.mods,
                },
                true,
            );
            on_event(
                CGEventType::FlagsChanged,
                &fn_event,
                &queue,
                &config,
                &consumed,
            );
            let down = event(binding.chord, true);
            assert!(matches!(
                on_event(CGEventType::KeyDown, &down, &queue, &config, &consumed),
                CallbackResult::Drop
            ));
            assert_eq!(*queue.lock().unwrap(), vec![binding.chord]);
            queue.lock().unwrap().clear();
            down.set_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT, 1);
            on_event(CGEventType::KeyDown, &down, &queue, &config, &consumed);
            assert_eq!(
                queue.lock().unwrap().len(),
                usize::from(binding.action.repeats())
            );
            queue.lock().unwrap().clear();
            let up = event(
                Chord {
                    mods: Modifiers::NONE,
                    ..binding.chord
                },
                false,
            );
            assert!(matches!(
                on_event(CGEventType::KeyUp, &up, &queue, &config, &consumed),
                CallbackResult::Drop
            ));
            assert!(queue.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn fn_navigation_is_normalized_without_capturing_plain_navigation() {
        let config = Mutex::new(Config::builtin());
        let queue = Mutex::new(Vec::new());
        let consumed = Mutex::new(KeyboardState::default());
        let fn_chord = Chord {
            keycode: 0x3f,
            mods: Modifiers::FN,
        };
        for (translated, original) in [
            (0x73, 0x7b),
            (0x77, 0x7c),
            (0x74, 0x7e),
            (0x79, 0x7d),
            (0x4c, 0x24),
        ] {
            on_event(
                CGEventType::FlagsChanged,
                &event(fn_chord, true),
                &queue,
                &config,
                &consumed,
            );
            let chord = Chord {
                keycode: translated,
                mods: Modifiers::FN,
            };
            assert!(matches!(
                on_event(
                    CGEventType::KeyDown,
                    &event(chord, true),
                    &queue,
                    &config,
                    &consumed
                ),
                CallbackResult::Drop
            ));
            assert_eq!(
                queue.lock().unwrap().pop(),
                Some(Chord {
                    keycode: original,
                    mods: Modifiers::FN
                })
            );
            let fn_up = Chord {
                mods: Modifiers::NONE,
                ..fn_chord
            };
            on_event(
                CGEventType::FlagsChanged,
                &event(fn_up, false),
                &queue,
                &config,
                &consumed,
            );
            let up = Chord {
                keycode: original,
                mods: Modifiers::NONE,
            };
            assert!(matches!(
                on_event(
                    CGEventType::KeyUp,
                    &event(up, false),
                    &queue,
                    &config,
                    &consumed
                ),
                CallbackResult::Drop
            ));
            assert!(matches!(
                on_event(
                    CGEventType::KeyDown,
                    &event(chord, true),
                    &queue,
                    &config,
                    &consumed
                ),
                CallbackResult::Keep
            ));
        }
    }

    #[test]
    fn standard_command_shortcuts_pass_through_native_callback() {
        let config = Mutex::new(Config::builtin());
        let queue = Mutex::new(Vec::new());
        let consumed = Mutex::new(KeyboardState::default());
        for key in [
            "Q", "W", "F", "T", "S", "TAB", "GRAVE", "SPACE", "1", "C", "V",
        ] {
            let chord = crate::keys::parse_chord(&format!("CMD + {key}"), Modifiers::NONE).unwrap();
            for (down, kind) in [(true, CGEventType::KeyDown), (false, CGEventType::KeyUp)] {
                assert!(matches!(
                    on_event(kind, &event(chord, down), &queue, &config, &consumed),
                    CallbackResult::Keep
                ));
            }
        }
        assert!(queue.lock().unwrap().is_empty());
    }
}
