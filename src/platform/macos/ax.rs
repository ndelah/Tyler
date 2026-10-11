//! macOS window control through the Accessibility API (`AXUIElement`).
//!
//! Moving, resizing, focusing and closing go through public AX attributes.
//! Matching an AX element to a `CGWindowID` uses `_AXUIElementGetWindow`. That
//! symbol is not in the public headers, but it ships in ApplicationServices
//! and does not require disabling SIP; AeroSpace and yabai resolve it the
//! same way. Nothing in this module has been executed — it only typechecks
//! under `cargo check --target aarch64-apple-darwin`.

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;

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
    pub element: AxElement,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    /// Private but stable. Maps an AX window element to its `CGWindowID`.
    fn _AXUIElementGetWindow(element: AXUIElementRef, wid: *mut u32) -> AXError;
}

const AX_TIMEOUT_SECS: f32 = 0.4;

/// Every manageable window currently in the window list.
///
/// Hung apps are bounded by [`AX_TIMEOUT_SECS`] so one stuck process cannot
/// stall the run loop for the AX default of several seconds.
pub fn observe_windows() -> Vec<ObservedWindow> {
    let mut by_pid: HashMap<i32, Vec<CgWindow>> = HashMap::new();
    for window in cg_windows() {
        let candidate = WindowCandidate {
            layer: window.layer,
            owner: window.owner.clone(),
            alpha: window.alpha,
            minimized: false,
        };
        if candidate.should_manage() {
            by_pid.entry(window.pid).or_default().push(window);
        }
    }

    let mut observed = Vec::new();
    for (pid, cg) in by_pid {
        let Some(app) = application(pid) else {
            continue;
        };
        // Electron/Chrome hide their AX tree until this is set. Failures are
        // expected for AppKit apps and ignored.
        let _ = set_bool(&app, "AXManualAccessibility", true);
        let _ = set_bool(&app, "AXEnhancedUserInterface", true);
        let owner = cg.first().map(|w| w.owner.clone()).unwrap_or_default();
        for element in copy_elements(&app, kAXWindowsAttribute) {
            let Some(id) = cg_window_id(&element) else {
                continue;
            };
            if !cg.iter().any(|w| w.id == id) {
                continue;
            }
            let minimized = copy_bool(&element, kAXMinimizedAttribute).unwrap_or(false);
            let candidate = WindowCandidate {
                layer: 0,
                owner: owner.clone(),
                alpha: 1.0,
                minimized,
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
                || (!subrole.is_empty() && subrole != kAXStandardWindowSubrole);
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

/// Moves and resizes `element`. Size is applied first: several AppKit apps
/// reject a position that, combined with the old size, would land off screen.
pub fn set_frame(element: &AxElement, frame: Rect) {
    if let Err(error) = set_size(element, frame.w, frame.h) {
        log::debug!("AXSize: {}", error_string(error));
    }
    if let Err(error) = set_point(element, kAXPositionAttribute, frame.x, frame.y) {
        log::debug!("AXPosition: {}", error_string(error));
    }
}

/// Raises the window and brings its application forward.
pub fn focus(element: &AxElement) {
    let action = CFString::new(kAXRaiseAction);
    let error = unsafe { AXUIElementPerformAction(element.0, action.as_concrete_TypeRef()) };
    if error != kAXErrorSuccess {
        log::debug!("AXRaise: {}", error_string(error));
    }
    let mut pid: i32 = 0;
    if unsafe { accessibility_sys::AXUIElementGetPid(element.0, &mut pid) } == kAXErrorSuccess {
        if let Some(app) = application(pid) {
            let _ = set_bool(&app, kAXFrontmostAttribute, true);
        }
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
