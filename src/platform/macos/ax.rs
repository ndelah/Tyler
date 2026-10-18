//! macOS window control through the Accessibility API (`AXUIElement`).
//!
//! Moving, resizing, focusing and closing go through public AX attributes.
//! Matching an AX element to a `CGWindowID` uses `_AXUIElementGetWindow`. That
//! symbol is not in the public headers, but it ships in ApplicationServices
//! and does not require disabling SIP; AeroSpace and yabai resolve it the
//! same way.

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;
use std::sync::Arc;

use accessibility_sys::{
    error_string, kAXCloseButtonAttribute, kAXErrorSuccess, kAXFocusedApplicationAttribute,
    kAXFocusedWindowAttribute, kAXFrontmostAttribute, kAXMinimizedAttribute, kAXPositionAttribute,
    kAXPressAction, kAXRaiseAction, kAXRoleAttribute, kAXSheetRole, kAXSizeAttribute,
    kAXStandardWindowSubrole, kAXSubroleAttribute, kAXValueTypeCGPoint, kAXValueTypeCGSize,
    kAXWindowRole, kAXWindowsAttribute, AXError, AXUIElementCopyAttributeValue,
    AXUIElementCreateApplication, AXUIElementCreateSystemWide, AXUIElementPerformAction,
    AXUIElementRef, AXUIElementSetAttributeValue, AXUIElementSetMessagingTimeout, AXValueCreate,
    AXValueGetTypeID, AXValueGetValue, AXValueRef,
};
use core_foundation::array::CFArray;
use core_foundation::base::{CFGetTypeID, CFRelease, CFRetain, CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::geometry::CGRect;
use core_graphics::window::{
    copy_window_info, kCGNullWindowID, kCGWindowAlpha, kCGWindowBounds, kCGWindowLayer,
    kCGWindowListOptionAll, kCGWindowNumber, kCGWindowOwnerName, kCGWindowOwnerPID,
};

use crate::geometry::Rect;
use crate::platform::filter::WindowCandidate;
use crate::WindowId;

/// An owned `AXUIElement`. Released on drop.
pub struct AxElement(AXUIElementRef);

impl AxElement {
    fn from_create(ptr: AXUIElementRef) -> Option<Self> {
        if ptr.is_null() {
            None
        } else {
            Some(Self(ptr))
        }
    }

    fn retain(ptr: AXUIElementRef) -> Option<Self> {
        if ptr.is_null() {
            None
        } else {
            unsafe { CFRetain(ptr as CFTypeRef) };
            Some(Self(ptr))
        }
    }
}

impl Drop for AxElement {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0 as CFTypeRef) };
    }
}

/// A real application window discovered on this scan.
pub struct ObservedWindow {
    pub id: WindowId,
    pub app: String,
    pub frame: Rect,
    pub prefers_floating: bool,
    pub on_screen: bool,
    pub minimized: bool,
    pub fullscreen: bool,
    pub element: AxElement,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    /// Private but stable. Maps an AX window element to its `CGWindowID`.
    fn _AXUIElementGetWindow(element: AXUIElementRef, wid: *mut u32) -> AXError;
}

const AX_TIMEOUT_SECS: f32 = 0.1;

/// An invalid element is definitive even while a native Space still lists its ID.
/// Timeouts and other AX errors are not evidence that a window was closed.
pub fn is_destroyed(element: &AxElement) -> bool {
    let role = CFString::new(kAXRoleAttribute);
    let mut value: CFTypeRef = ptr::null();
    let error =
        unsafe { AXUIElementCopyAttributeValue(element.0, role.as_concrete_TypeRef(), &mut value) };
    if !value.is_null() {
        unsafe { CFRelease(value) };
    }
    error == accessibility_sys::kAXErrorInvalidUIElement
}

/// Every manageable window currently in the window list.
///
/// Hung apps are bounded by [`AX_TIMEOUT_SECS`] so one stuck process cannot
/// stall the run loop for the AX default of several seconds.
pub fn observe_windows(known: &[(WindowId, &AxElement, &str)]) -> Vec<ObservedWindow> {
    let mut by_pid: HashMap<i32, (String, Vec<CgWindow>)> = HashMap::new();
    for window in cg_windows() {
        let candidate = WindowCandidate {
            layer: window.layer,
            owner: window.owner.clone(),
            alpha: window.alpha,
            minimized: false,
        };
        if candidate.should_manage() {
            by_pid
                .entry(window.pid)
                .or_insert_with(|| (window.owner.clone(), Vec::new()))
                .1
                .push(window);
        }
    }

    // Minimized windows may disappear from CGWindowList; their AX windows
    // must remain discoverable so a workspace can restore them.
    for (_, element, owner) in known {
        let mut pid = 0;
        if unsafe { accessibility_sys::AXUIElementGetPid(element.0, &mut pid) } == kAXErrorSuccess {
            by_pid
                .entry(pid)
                .or_insert_with(|| (owner.to_string(), Vec::new()));
        }
    }
    let mut observed = Vec::new();
    for (pid, (owner, cg)) in by_pid {
        let Some(app) = application(pid) else {
            continue;
        };
        // Observers enable AXManualAccessibility once when an app appears.
        for element in copy_elements(&app, kAXWindowsAttribute) {
            let _ = unsafe { AXUIElementSetMessagingTimeout(element.0, AX_TIMEOUT_SECS) };
            let Some(id) = cg_window_id(&element) else {
                continue;
            };
            if !cg.iter().any(|w| w.id == id)
                && !known.iter().any(|(known_id, _, _)| *known_id == id)
            {
                continue;
            }
            let minimized = copy_bool(&element, kAXMinimizedAttribute).unwrap_or(false);
            let candidate = WindowCandidate {
                layer: 0,
                owner: owner.clone(),
                alpha: 1.0,
                minimized: false,
            };
            if !candidate.should_manage() {
                continue;
            }
            let role = copy_string(&element, kAXRoleAttribute).unwrap_or_default();
            if role != kAXWindowRole && role != kAXSheetRole {
                continue;
            }
            let subrole = copy_string(&element, kAXSubroleAttribute).unwrap_or_default();
            let prefers_floating = role == kAXSheetRole
                || (!subrole.is_empty() && subrole != kAXStandardWindowSubrole)
                || size_is_fixed(&element);
            let frame =
                ax_frame(&element).or_else(|| cg.iter().find(|w| w.id == id).map(|w| w.bounds));
            let Some(frame) = frame else { continue };
            if frame.w < 20.0 || frame.h < 20.0 {
                continue;
            }
            observed.push(ObservedWindow {
                id,
                app: owner.clone(),
                frame,
                prefers_floating,
                minimized,
                fullscreen: copy_bool(&element, "AXFullScreen").unwrap_or(false),
                on_screen: cg.iter().find(|w| w.id == id).is_some_and(|w| w.on_screen),
                element,
            });
        }
    }
    observed
}

/// The `CGWindowID` of the window the user is currently typing into.
pub fn focused_window_id() -> Option<WindowId> {
    let system = AxElement::from_create(unsafe { AXUIElementCreateSystemWide() })?;
    let _ = unsafe { AXUIElementSetMessagingTimeout(system.0, AX_TIMEOUT_SECS) };
    let app = copy_element(&system, kAXFocusedApplicationAttribute)?;
    let window = copy_element(&app, kAXFocusedWindowAttribute)?;
    cg_window_id(&window)
}

/// AppKit/Electron can constrain the first resize using the old position.
/// Apply size, position, size with enhanced-UI animations temporarily disabled,
/// then read back the result rather than assuming AX accepted the request.
/// Distinguish a genuine app size constraint from a transport failure/timeout.
/// A timed-out write must never teach the layout that the old size is a minimum.
pub fn apply_frame(element: &AxElement, frame: Rect, area: Rect) -> (Option<Rect>, bool) {
    let _session = FrameSession::new(element);
    let before = ax_frame(element);
    let mut accepted = set_frame_step(element, before, frame).is_ok();
    // Retry size only when the old position actually constrained the resize.
    // Do not send every window the same resize twice unconditionally.
    if ax_frame(element)
        .is_some_and(|r| (r.w - frame.w).abs() >= 2.0 || (r.h - frame.h).abs() >= 2.0)
    {
        accepted &= set_size(element, frame.w, frame.h).is_ok();
    }
    let actual = ax_frame(element);
    if let Some(actual) = actual {
        let safe = actual.contained_in(area);
        if !safe.approx_eq(&actual) {
            accepted &= set_size(element, safe.w, safe.h).is_ok();
            accepted &= set_point(element, kAXPositionAttribute, safe.x, safe.y).is_ok();
        }
    }
    (ax_frame(element).or(actual), accepted)
}

pub fn frame(element: &AxElement) -> Option<Rect> {
    ax_frame(element)
}

/// Animation steps do only changed writes; validation happens once at the
/// destination after the application has had time to settle.
pub fn set_frame_step(
    element: &AxElement,
    previous: Option<Rect>,
    next: Rect,
) -> Result<(), AXError> {
    if previous.is_none_or(|r| (r.w - next.w).abs() >= 0.5 || (r.h - next.h).abs() >= 0.5) {
        set_size(element, next.w, next.h)?;
    }
    if previous.is_none_or(|r| (r.x - next.x).abs() >= 0.5 || (r.y - next.y).abs() >= 0.5) {
        set_point(element, kAXPositionAttribute, next.x, next.y)?;
    }
    Ok(())
}

/// Retained for the whole tween, then restored on completion or cancellation.
pub struct FrameSession {
    pid: Option<i32>,
    _main_thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

thread_local! {
    static FRAME_SESSIONS: std::cell::RefCell<HashMap<i32, (usize, AxElement)>> = std::cell::RefCell::new(HashMap::new());
}

impl FrameSession {
    pub fn new(element: &AxElement) -> Self {
        let mut pid = 0;
        let pid = (unsafe { accessibility_sys::AXUIElementGetPid(element.0, &mut pid) }
            == kAXErrorSuccess)
            .then_some(pid)
            .filter(|pid| {
                FRAME_SESSIONS.with(|sessions| {
                    let mut sessions = sessions.borrow_mut();
                    if let Some((count, _)) = sessions.get_mut(pid) {
                        *count += 1;
                        return true;
                    }
                    let Some(app) = application(*pid) else {
                        return false;
                    };
                    if copy_bool(&app, "AXEnhancedUserInterface") != Some(true)
                        || set_bool(&app, "AXEnhancedUserInterface", false).is_err()
                    {
                        return false;
                    }
                    sessions.insert(*pid, (1, app));
                    true
                })
            });
        Self {
            pid,
            _main_thread: std::marker::PhantomData,
        }
    }
}

impl Drop for FrameSession {
    fn drop(&mut self) {
        if let Some(pid) = self.pid {
            FRAME_SESSIONS.with(|sessions| {
                let mut sessions = sessions.borrow_mut();
                if let Some((count, _)) = sessions.get_mut(&pid) {
                    *count -= 1;
                    if *count == 0 {
                        let (_, app) = sessions.remove(&pid).unwrap();
                        let _ = set_bool(&app, "AXEnhancedUserInterface", true);
                    }
                }
            });
        }
    }
}

fn size_is_fixed(element: &AxElement) -> bool {
    let mut settable = 1;
    let attribute = CFString::new(kAXSizeAttribute);
    unsafe {
        accessibility_sys::AXUIElementIsAttributeSettable(
            element.0,
            attribute.as_concrete_TypeRef(),
            &mut settable,
        ) == kAXErrorSuccess
            && settable == 0
    }
}

struct AppObserver {
    observer: accessibility_sys::AXObserverRef,
    _app: AxElement,
    windows: HashMap<WindowId, AxElement>,
}

impl Drop for AppObserver {
    fn drop(&mut self) {
        unsafe {
            core_foundation::runloop::CFRunLoopRemoveSource(
                core_foundation::runloop::CFRunLoopGetCurrent(),
                accessibility_sys::AXObserverGetRunLoopSource(self.observer),
                core_foundation::runloop::kCFRunLoopCommonModes,
            );
            CFRelease(self.observer.cast());
        }
    }
}

pub struct Observers {
    apps: HashMap<i32, AppObserver>,
    wake: Arc<super::events::Wake>,
}

impl Observers {
    pub fn new(wake: Arc<super::events::Wake>) -> Self {
        Self {
            apps: HashMap::new(),
            wake,
        }
    }

    pub fn watch_apps(&mut self) {
        let running = objc2_app_kit::NSWorkspace::sharedWorkspace().runningApplications();
        let pids: std::collections::HashSet<_> = running
            .iter()
            .map(|app| app.processIdentifier())
            .filter(|pid| *pid > 0 && *pid as u32 != std::process::id())
            .collect();
        self.apps.retain(|pid, _| pids.contains(pid));
        for pid in pids {
            if self.apps.contains_key(&pid) {
                continue;
            }
            let Some(app) = application(pid) else {
                continue;
            };
            let _ = set_bool(&app, "AXManualAccessibility", true);
            let mut observer = ptr::null_mut();
            if unsafe { accessibility_sys::AXObserverCreate(pid, on_notification, &mut observer) }
                != kAXErrorSuccess
            {
                continue;
            }
            for notification in [
                "AXWindowCreated",
                "AXFocusedWindowChanged",
                "AXApplicationHidden",
                "AXApplicationShown",
            ] {
                self.subscribe(observer, &app, notification);
            }
            unsafe {
                core_foundation::runloop::CFRunLoopAddSource(
                    core_foundation::runloop::CFRunLoopGetCurrent(),
                    accessibility_sys::AXObserverGetRunLoopSource(observer),
                    core_foundation::runloop::kCFRunLoopCommonModes,
                );
            }
            self.apps.insert(
                pid,
                AppObserver {
                    observer,
                    _app: app,
                    windows: HashMap::new(),
                },
            );
        }
    }

    pub fn watch_windows(&mut self, windows: &[ObservedWindow]) {
        let live: std::collections::HashSet<_> = windows.iter().map(|w| w.id).collect();
        for observer in self.apps.values_mut() {
            observer.windows.retain(|id, element| {
                if live.contains(id) {
                    return true;
                }
                for name in WINDOW_NOTIFICATIONS {
                    let notification = CFString::new(name);
                    unsafe {
                        accessibility_sys::AXObserverRemoveNotification(
                            observer.observer,
                            element.0,
                            notification.as_concrete_TypeRef(),
                        );
                    }
                }
                false
            });
        }
        for window in windows {
            let mut pid = 0;
            if unsafe { accessibility_sys::AXUIElementGetPid(window.element.0, &mut pid) }
                != kAXErrorSuccess
            {
                continue;
            }
            let Some(observer) = self.apps.get(&pid) else {
                continue;
            };
            if observer.windows.contains_key(&window.id) {
                continue;
            }
            for name in WINDOW_NOTIFICATIONS {
                self.subscribe(observer.observer, &window.element, name);
            }
            self.apps
                .get_mut(&pid)
                .unwrap()
                .windows
                .insert(window.id, AxElement::retain(window.element.0).unwrap());
        }
    }

    fn subscribe(
        &self,
        observer: accessibility_sys::AXObserverRef,
        element: &AxElement,
        name: &str,
    ) {
        let notification = CFString::new(name);
        // Some apps omit individual notifications. The periodic reconciliation
        // remains as a fallback; unsupported events never prevent tiling.
        unsafe {
            accessibility_sys::AXObserverAddNotification(
                observer,
                element.0,
                notification.as_concrete_TypeRef(),
                Arc::as_ptr(&self.wake).cast_mut().cast(),
            );
        }
    }
}

const WINDOW_NOTIFICATIONS: [&str; 7] = [
    "AXUIElementDestroyed",
    "AXWindowMoved",
    "AXWindowResized",
    "AXMoved",
    "AXResized",
    "AXWindowMiniaturized",
    "AXWindowDeminiaturized",
];

unsafe extern "C" fn on_notification(
    _: accessibility_sys::AXObserverRef,
    _: AXUIElementRef,
    notification: CFStringRef,
    context: *mut c_void,
) {
    let name = CFString::wrap_under_get_rule(notification);
    let flags = match name.to_string().as_str() {
        "AXMoved" | "AXResized" | "AXWindowMoved" | "AXWindowResized" => super::events::GEOMETRY,
        "AXFocusedWindowChanged" => super::events::FOCUS,
        _ => super::events::WINDOWS,
    };
    (&*context.cast::<super::events::Wake>()).signal(flags);
}

pub fn set_minimized(element: &AxElement, minimized: bool) -> bool {
    set_bool(element, kAXMinimizedAttribute, minimized).is_ok()
        && copy_bool(element, kAXMinimizedAttribute) == Some(minimized)
}

/// Raises the window and brings its application forward.
pub fn focus(element: &AxElement) {
    let mut pid: i32 = 0;
    if unsafe { accessibility_sys::AXUIElementGetPid(element.0, &mut pid) } == kAXErrorSuccess {
        if let Some(app) = application(pid) {
            let _ = set_bool(&app, kAXFrontmostAttribute, true);
        }
    }
    // Activation can select another window in the same app. Explicitly make
    // our target main/key before raising it above the other normal windows.
    let _ = set_bool(element, "AXMain", true);
    let _ = set_bool(element, "AXFocused", true);
    let action = CFString::new(kAXRaiseAction);
    let error = unsafe { AXUIElementPerformAction(element.0, action.as_concrete_TypeRef()) };
    if error != kAXErrorSuccess {
        log::debug!("AXRaise: {}", error_string(error));
    }
}

/// Presses the window's AX close button. Equivalent to clicking the red button.
pub fn close(element: &AxElement) {
    let Some(button) = copy_element(element, kAXCloseButtonAttribute) else {
        log::debug!("window has no AXCloseButton");
        return;
    };
    let action = CFString::new(kAXPressAction);
    let error = unsafe { AXUIElementPerformAction(button.0, action.as_concrete_TypeRef()) };
    if error != kAXErrorSuccess {
        log::debug!("AXPress close: {}", error_string(error));
    }
}

struct CgWindow {
    id: WindowId,
    pid: i32,
    owner: String,
    layer: i64,
    alpha: f64,
    bounds: Rect,
    on_screen: bool,
}

fn cg_windows() -> Vec<CgWindow> {
    let Some(list) = copy_window_info(kCGWindowListOptionAll, kCGNullWindowID) else {
        return Vec::new();
    };
    let mut windows = Vec::new();
    for raw in list.get_all_values() {
        if raw.is_null() {
            continue;
        }
        let dict = unsafe {
            CFDictionary::<CFString, CFType>::wrap_under_get_rule(raw as CFDictionaryRef)
        };
        let Some(id) = dict_i64(&dict, unsafe { kCGWindowNumber }) else {
            continue;
        };
        let Some(pid) = dict_i64(&dict, unsafe { kCGWindowOwnerPID }) else {
            continue;
        };
        if pid as u32 == std::process::id() {
            continue;
        }
        let owner = dict_string(&dict, unsafe { kCGWindowOwnerName }).unwrap_or_default();
        let layer = dict_i64(&dict, unsafe { kCGWindowLayer }).unwrap_or(0);
        let alpha = dict_f64(&dict, unsafe { kCGWindowAlpha }).unwrap_or(1.0);
        let bounds =
            dict_rect(&dict, unsafe { kCGWindowBounds }).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
        windows.push(CgWindow {
            id: id as WindowId,
            pid: pid as i32,
            owner,
            layer,
            alpha,
            bounds,
            on_screen: dict_value(
                &dict,
                CFString::new("kCGWindowIsOnscreen").as_concrete_TypeRef(),
            )
            .and_then(|v| v.downcast::<CFBoolean>())
            .is_some_and(bool::from),
        });
    }
    windows
}

fn application(pid: i32) -> Option<AxElement> {
    let app = AxElement::from_create(unsafe { AXUIElementCreateApplication(pid) })?;
    let _ = unsafe { AXUIElementSetMessagingTimeout(app.0, AX_TIMEOUT_SECS) };
    Some(app)
}

fn cg_window_id(element: &AxElement) -> Option<WindowId> {
    let mut id = 0u32;
    let error = unsafe { _AXUIElementGetWindow(element.0, &mut id) };
    if error == kAXErrorSuccess && id != 0 {
        Some(WindowId::from(id))
    } else {
        None
    }
}

fn ax_frame(element: &AxElement) -> Option<Rect> {
    let origin = ax_value::<core_graphics::geometry::CGPoint>(
        element,
        kAXPositionAttribute,
        kAXValueTypeCGPoint,
    )?;
    let size =
        ax_value::<core_graphics::geometry::CGSize>(element, kAXSizeAttribute, kAXValueTypeCGSize)?;
    Some(Rect::new(origin.x, origin.y, size.width, size.height))
}

fn ax_value<T: Copy>(element: &AxElement, attribute: &str, kind: u32) -> Option<T> {
    let value = copy_attr(element, attribute)?;
    unsafe {
        if CFGetTypeID(value.as_CFTypeRef()) != AXValueGetTypeID() {
            return None;
        }
        let mut out = std::mem::MaybeUninit::<T>::uninit();
        let ok = AXValueGetValue(
            value.as_CFTypeRef() as AXValueRef,
            kind,
            out.as_mut_ptr().cast::<c_void>(),
        );
        if ok {
            Some(out.assume_init())
        } else {
            None
        }
    }
}

fn set_point(element: &AxElement, attribute: &str, x: f64, y: f64) -> Result<(), AXError> {
    let point = core_graphics::geometry::CGPoint::new(x, y);
    set_ax_value(element, attribute, kAXValueTypeCGPoint, &point)
}

fn set_size(element: &AxElement, width: f64, height: f64) -> Result<(), AXError> {
    let size = core_graphics::geometry::CGSize::new(width, height);
    set_ax_value(element, attribute_size(), kAXValueTypeCGSize, &size)
}

fn attribute_size() -> &'static str {
    kAXSizeAttribute
}

fn set_ax_value<T>(
    element: &AxElement,
    attribute: &str,
    kind: u32,
    value: &T,
) -> Result<(), AXError> {
    let created = unsafe { AXValueCreate(kind, (value as *const T).cast::<c_void>()) };
    if created.is_null() {
        return Err(accessibility_sys::kAXErrorFailure);
    }
    let attr = CFString::new(attribute);
    let error = unsafe {
        AXUIElementSetAttributeValue(element.0, attr.as_concrete_TypeRef(), created as CFTypeRef)
    };
    unsafe { CFRelease(created as CFTypeRef) };
    if error == kAXErrorSuccess {
        Ok(())
    } else {
        Err(error)
    }
}

fn set_bool(element: &AxElement, attribute: &str, value: bool) -> Result<(), AXError> {
    let attr = CFString::new(attribute);
    let boolean = CFBoolean::from(value);
    let error = unsafe {
        AXUIElementSetAttributeValue(
            element.0,
            attr.as_concrete_TypeRef(),
            boolean.as_CFTypeRef(),
        )
    };
    if error == kAXErrorSuccess {
        Ok(())
    } else {
        Err(error)
    }
}

fn copy_attr(element: &AxElement, attribute: &str) -> Option<CFType> {
    let attr = CFString::new(attribute);
    let mut value: CFTypeRef = ptr::null();
    let error =
        unsafe { AXUIElementCopyAttributeValue(element.0, attr.as_concrete_TypeRef(), &mut value) };
    if error != kAXErrorSuccess || value.is_null() {
        None
    } else {
        Some(unsafe { CFType::wrap_under_create_rule(value) })
    }
}

fn copy_string(element: &AxElement, attribute: &str) -> Option<String> {
    copy_attr(element, attribute)?
        .downcast::<CFString>()
        .map(|s| s.to_string())
}

fn copy_bool(element: &AxElement, attribute: &str) -> Option<bool> {
    copy_attr(element, attribute)?
        .downcast::<CFBoolean>()
        .map(bool::from)
}

fn copy_element(element: &AxElement, attribute: &str) -> Option<AxElement> {
    let value = copy_attr(element, attribute)?;
    AxElement::retain(value.as_CFTypeRef() as AXUIElementRef)
}

fn copy_elements(element: &AxElement, attribute: &str) -> Vec<AxElement> {
    let Some(value) = copy_attr(element, attribute) else {
        return Vec::new();
    };
    let Some(array) = value.downcast::<CFArray>() else {
        return Vec::new();
    };
    array
        .get_all_values()
        .into_iter()
        .filter_map(|ptr| AxElement::retain(ptr as AXUIElementRef))
        .collect()
}

fn dict_value<'a>(
    dict: &'a CFDictionary<CFString, CFType>,
    key: CFStringRef,
) -> Option<core_foundation::base::ItemRef<'a, CFType>> {
    let key = unsafe { CFString::wrap_under_get_rule(key) };
    dict.find(key)
}

fn dict_i64(dict: &CFDictionary<CFString, CFType>, key: CFStringRef) -> Option<i64> {
    dict_value(dict, key)?.downcast::<CFNumber>()?.to_i64()
}

fn dict_f64(dict: &CFDictionary<CFString, CFType>, key: CFStringRef) -> Option<f64> {
    dict_value(dict, key)?.downcast::<CFNumber>()?.to_f64()
}

fn dict_string(dict: &CFDictionary<CFString, CFType>, key: CFStringRef) -> Option<String> {
    dict_value(dict, key)?
        .downcast::<CFString>()
        .map(|s| s.to_string())
}

fn dict_rect(dict: &CFDictionary<CFString, CFType>, key: CFStringRef) -> Option<Rect> {
    let value = dict_value(dict, key)?;
    let untyped =
        unsafe { CFDictionary::wrap_under_get_rule(value.as_CFTypeRef() as CFDictionaryRef) };
    let rect = CGRect::from_dict_representation(&untyped)?;
    Some(Rect::new(
        rect.origin.x,
        rect.origin.y,
        rect.size.width,
        rect.size.height,
    ))
}
