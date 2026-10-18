//! Native AppKit controls, owned by the Rust worker. No SketchyBar dependency.
//! Target/action callbacks enqueue a selection; IPC runs in the next worker tick.
use crate::{config::Config, geometry::Rect, navigator::Desktop, spaces::Snapshot};
use objc2::rc::Retained;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::*;
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use std::cell::Cell;

const CONTROL_HEIGHT: f64 = 22.0;

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Cell<Option<usize>>]
    struct Selection;
    unsafe impl NSObjectProtocol for Selection {}
    impl Selection {
        #[unsafe(method(selectDesktop:))]
        fn select(&self, sender: &NSSegmentedControl) {
            let index = sender.selectedSegment();
            if index >= 0 { self.ivars().set(Some(index as usize)); }
        }
    }
);

pub struct Navigator {
    panel: Retained<NSPanel>,
    control: Option<Retained<NSSegmentedControl>>,
    present: bool,
    target: Retained<Selection>,
    rows: Vec<Desktop>,
    frame: NSRect,
    shown: bool,
    ready: bool,
    occupied: Option<Vec<Rect>>,
    last_scan: std::time::Instant,
    front_pid: i32,
}

impl Navigator {
    pub fn new() -> Self {
        let mtm = MainThreadMarker::new().expect("navigator requires main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            NSRect::ZERO,
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        unsafe {
            panel.setReleasedWhenClosed(false);
        }
        panel.setLevel(25); // Inside the menu-bar band; dropdown menus remain above it.
        panel.setHasShadow(false);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        panel.setHidesOnDeactivate(false);
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setTitle(&NSString::from_str("Tyler desktops"));
        let target = unsafe {
            msg_send![
                super(Selection::alloc(mtm).set_ivars(Cell::new(None))),
                init
            ]
        };
        Self {
            panel,
            control: None,
            present: true,
            target,
            rows: vec![],
            frame: NSRect::ZERO,
            shown: false,
            ready: false,
            occupied: None,
            last_scan: std::time::Instant::now(),
            front_pid: 0,
        }
    }

    fn hide(&mut self) {
        self.panel.orderOut(None);
        self.shown = false;
    }

    pub fn take_selection(&mut self) -> Option<u64> {
        let selection = self
            .target
            .ivars()
            .take()
            .and_then(|i| self.rows.get(i))
            .and_then(|row| row.id.parse().ok());
        // A click changes AppKit's selection optimistically. Redraw from observed
        // macOS state even if the IPC request fails or the transition is delayed.
        if selection.is_some() {
            self.frame = NSRect::ZERO;
        }
        selection
    }

    pub fn update(&mut self, snapshot: &Snapshot, config: &Config, ready: bool) {
        let mtm = MainThreadMarker::new().unwrap();
        // Fullscreen Spaces are macOS-owned. The strip is absent there.
        if !config.navigator.enabled || (ready && snapshot.active().is_none()) {
            if self.shown {
                self.panel.orderOut(None);
                self.shown = false;
            }
            return;
        }
        let Some(screen) = NSScreen::screens(mtm).firstObject() else {
            return;
        };
        let full = screen.frame();
        let height = NSStatusBar::systemStatusBar()
            .thickness()
            .max(screen.safeAreaInsets().top);
        let bar = Rect::new(full.origin.x, 0.0, full.size.width, height);
        let pid = NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .map(|app| app.processIdentifier())
            .unwrap_or(0);
        if self.present && (pid != self.front_pid || self.last_scan.elapsed().as_millis() >= 250) {
            self.occupied = super::ax::menu_bar_obstacles(pid, bar);
            self.front_pid = pid;
            self.last_scan = std::time::Instant::now();
        }
        let mut occupied = if self.present {
            let Some(items) = self.occupied.clone() else {
                self.hide();
                return;
            };
            items
        } else {
            vec![]
        };
        // The area between these rectangles is the camera housing, not free space.
        if screen.safeAreaInsets().top > 0.0 {
            let left = screen.auxiliaryTopLeftArea();
            let right = screen.auxiliaryTopRightArea();
            occupied.push(Rect::new(
                left.origin.x + left.size.width,
                0.0,
                right.origin.x - left.origin.x - left.size.width,
                height,
            ));
        }
        // Do not leave a floating row behind when the system menu bar is hidden.
        if self.present
            && full.origin.y + full.size.height
                - screen.visibleFrame().origin.y
                - screen.visibleFrame().size.height
                < height - 1.0
        {
            self.hide();
            return;
        }
        let mut rows = crate::navigator::desktops(snapshot, config);
        if rows.is_empty() && !ready {
            rows.push(Desktop {
                id: "0".into(),
                number: 0,
                name: "Desktops unavailable".into(),
                icon: String::new(),
                shortcut: String::new(),
                selected: false,
            });
        }
        let desired = rows
            .iter()
            .map(|row| segment_width(&segment_label(row)))
            .sum::<f64>()
            + 4.0;
        let Some(area) =
            crate::navigator::menu_bar_frame(bar, &occupied, desired, config.navigator.placement)
        else {
            self.hide();
            return;
        };
        let frame = NSRect::new(
            NSPoint::new(area.x, full.origin.y + full.size.height - height),
            NSSize::new(area.w, height),
        );
        if self.shown && self.rows == rows && self.frame == frame && self.ready == ready {
            return;
        }
        self.rows = rows;
        self.frame = frame;
        self.ready = ready;
        self.panel.setFrame_display(frame, false);
        let background =
            NSView::initWithFrame(NSView::alloc(mtm), NSRect::new(NSPoint::ZERO, frame.size));
        let scroll = NSScrollView::initWithFrame(
            NSScrollView::alloc(mtm),
            NSRect::new(
                NSPoint::new(2.0, (height - CONTROL_HEIGHT) / 2.0),
                NSSize::new((frame.size.width - 4.0).max(1.0), CONTROL_HEIGHT),
            ),
        );
        scroll.setDrawsBackground(false);
        scroll.setHasHorizontalScroller(false);
        scroll.setHorizontalScrollElasticity(NSScrollElasticity::None);
        scroll.setAutohidesScrollers(true);
        let control = NSSegmentedControl::new(mtm);
        control.setSegmentCount(self.rows.len() as isize);
        control.setSegmentStyle(NSSegmentStyle::Automatic);
        control.setControlSize(NSControlSize::Small);
        control.setTrackingMode(NSSegmentSwitchTracking::SelectOne);
        control.setFont(Some(&NSFont::systemFontOfSize(12.0)));
        let mut width = 0.0;
        let mut selected_bounds = None;
        for (i, row) in self.rows.iter().enumerate() {
            let i = i as isize;
            let label = segment_label(row);
            control.setLabel_forSegment(&NSString::from_str(&label), i);
            if let Some(symbol) = row.icon.strip_prefix("sf:") {
                let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                    &NSString::from_str(symbol),
                    Some(&NSString::from_str(&row.name)),
                );
                control.setImage_forSegment(image.as_deref(), i);
            }
            let segment_width = segment_width(&label);
            control.setWidth_forSegment(segment_width, i);
            control.setSelected_forSegment(row.selected, i);
            control.setEnabled_forSegment(ready, i);
            let name = if row.name.is_empty() {
                format!("Desktop {}", row.number)
            } else {
                row.name.clone()
            };
            let hint = if ready {
                row.shortcut.as_str()
            } else {
                "Desktop connection unavailable; check yabai"
            };
            control
                .setToolTip_forSegment(Some(&NSString::from_str(&format!("{name} — {hint}"))), i);
            if row.selected {
                selected_bounds = Some(NSRect::new(
                    NSPoint::new(width, 0.0),
                    NSSize::new(segment_width, CONTROL_HEIGHT),
                ));
            }
            width += segment_width;
        }
        control.setFrame(NSRect::new(
            NSPoint::ZERO,
            NSSize::new(width.max(1.0), CONTROL_HEIGHT),
        ));
        unsafe {
            control.setTarget(Some(&self.target));
            control.setAction(Some(sel!(selectDesktop:)));
            let _: () = msg_send![&*control, setAccessibilityLabel: &*NSString::from_str("Virtual desktops")];
            background.addSubview(&scroll);
        }
        scroll.setDocumentView(Some(&control));
        self.panel.setContentView(Some(&background));
        if let Some(bounds) = selected_bounds {
            control.scrollRectToVisible(bounds);
        }
        self.control = Some(control);
        if self.present {
            self.panel.orderFrontRegardless();
        }
        self.shown = true;
    }
}

pub fn run_application() {
    let app = NSApplication::sharedApplication(MainThreadMarker::new().unwrap());
    app.run();
}

pub fn usable_screen(_config: &Config) -> (Rect, Rect) {
    super::screens::main_screen()
}

fn segment_label(row: &Desktop) -> String {
    let text = if row.icon.is_empty() {
        row.name.as_str()
    } else if row.icon.starts_with("sf:") {
        ""
    } else {
        row.icon.as_str()
    };
    if row.number == 0 {
        text.to_owned()
    } else {
        format!(
            "{}{}{}",
            row.number,
            if text.is_empty() { "" } else { "  " },
            text
        )
    }
}
fn segment_width(label: &str) -> f64 {
    (label.chars().count() as f64 * 7.0 + 24.0).clamp(36.0, 200.0)
}

/// Exercise actual AppKit controls offscreen without AX, yabai, or changing desktops.
/// An optional PNG is a synthetic layout preview, not a desktop screenshot.
pub fn check(preview: Option<&str>) -> Result<(), String> {
    let mut navigator = Navigator::new();
    navigator.present = false;
    let mut config = Config::builtin();
    config.navigator.labels.insert(
        "101".into(),
        crate::config::DesktopLabel {
            name: "Work".into(),
            icon: "sf:briefcase".into(),
        },
    );
    config.navigator.labels.insert(
        "102".into(),
        crate::config::DesktopLabel {
            name: "Personal".into(),
            icon: "".into(),
        },
    );
    config.navigator.labels.insert(
        "103".into(),
        crate::config::DesktopLabel {
            name: "Code".into(),
            icon: "💻".into(),
        },
    );
    let mut snapshot = Snapshot {
        display: 1,
        spaces: (0..3)
            .map(|i| crate::spaces::Space {
                id: 101 + i,
                index: i as usize + 1,
                display: 1,
                windows: vec![],
                visible: i == 1,
                fullscreen: false,
                layout: "float".into(),
            })
            .collect(),
    };
    navigator.update(&snapshot, &config, true);
    let control = navigator
        .control
        .as_ref()
        .ok_or("native control was not created")?
        .clone();
    assert_eq!(control.segmentCount(), 3);
    assert_eq!(control.selectedSegment(), 1);
    control.setSelectedSegment(0);
    unsafe {
        control.sendAction_to(Some(sel!(selectDesktop:)), Some(&navigator.target));
    }
    assert_eq!(navigator.take_selection(), Some(101));
    navigator.update(&snapshot, &config, true);
    assert_eq!(navigator.control.as_ref().unwrap().selectedSegment(), 1);
    if let Some(path) = preview {
        let view = navigator
            .panel
            .contentView()
            .ok_or("missing content view")?;
        let bounds = NSRect::new(
            NSPoint::ZERO,
            NSSize::new(
                640.0_f64.min(view.bounds().size.width),
                view.bounds().size.height,
            ),
        );
        let bitmap = view
            .bitmapImageRepForCachingDisplayInRect(bounds)
            .ok_or("cannot allocate preview")?;
        view.cacheDisplayInRect_toBitmapImageRep(bounds, &bitmap);
        let data = unsafe {
            bitmap.representationUsingType_properties(
                NSBitmapImageFileType::PNG,
                &objc2_foundation::NSDictionary::new(),
            )
        }
        .ok_or("cannot encode preview")?;
        if !data.writeToFile_atomically(&NSString::from_str(path), true) {
            return Err("cannot write preview".into());
        }
    }
    navigator.update(&snapshot, &config, false);
    assert!(!navigator.control.as_ref().unwrap().isEnabledForSegment(0));
    config.navigator.enabled = false;
    navigator.update(&snapshot, &config, true);
    assert!(!navigator.shown);
    config.navigator.enabled = true;
    snapshot.spaces[1].fullscreen = true;
    navigator.update(&snapshot, &config, true);
    assert!(!navigator.shown);
    println!("PASS: native segmented controls, target/action routing, observed selection restoration, failure state, module toggle, and fullscreen hiding (synthetic desktops; no live switching)");
    Ok(())
}
