import AppKit
import ApplicationServices

// AppKit owns the menu bar and permission identity; the bundled Rust child
// owns tiling and the keyboard tap. Neither requires a terminal window.
final class TylerApp: NSObject, NSApplicationDelegate, NSMenuDelegate {
    private var statusItem: NSStatusItem!
    private let stateItem = NSMenuItem(title: "Starting…", action: nil, keyEquivalent: "")
    private var worker: Process?
    private var logHandle: FileHandle?
    private var shortcutsWindow: NSWindow?
    private var timer: Timer?
    private var quitting = false
    private var failed = false
    private var state = "Starting…"
    private let logURL = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Logs/Tyler.log")
    private var binary: URL {
        Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/tyler-tiler")
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let peers = NSRunningApplication.runningApplications(withBundleIdentifier: "com.ndelah.tyler")
        if peers.contains(where: { $0.processIdentifier != ProcessInfo.processInfo.processIdentifier }) {
            NSApp.terminate(nil)
            return
        }
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        let image = NSImage(named: "TylerMenuBar")
        image?.size = NSSize(width: 15.4, height: 18)
        image?.isTemplate = true
        image?.accessibilityDescription = "Tyler"
        statusItem.button?.image = image
        if image == nil { statusItem.button?.title = "T" }
        statusItem.button?.setAccessibilityLabel("Tyler window manager")
        let menu = NSMenu()
        menu.delegate = self
        menu.autoenablesItems = false
        stateItem.isEnabled = false
        menu.addItem(stateItem)
        menu.addItem(.separator())
        add("View Shortcuts…", #selector(showShortcuts), to: menu)
        add("Open Accessibility Settings…", #selector(openAccessibility), to: menu)
        add("Open Input Monitoring Settings…", #selector(openInputMonitoring), to: menu)
        add("Retry Starting Tyler", #selector(retry), to: menu)
        add("View Log…", #selector(showLog), to: menu)
        menu.addItem(.separator())
        add("Quit Tyler", #selector(quit), to: menu, key: "q")
        statusItem.menu = menu
        if CommandLine.arguments.contains("--self-test") {
            precondition(menu.items.contains { $0.title == "Quit Tyler" && $0.target === self })
            precondition(statusItem.button?.image != nil)
            showShortcuts()
            let scroll = shortcutsWindow!.contentView!.subviews[0] as! NSScrollView
            let text = scroll.documentView as! NSTextView
            precondition(text.string.contains("super=FN"), text.string)
            precondition(text.string.contains("FN + B") && text.string.contains("Open Safari"), text.string)
            shortcutsWindow?.close()
            precondition(!quitting)
            print("PASS: menu bar icon, shortcut window, resolved Fn bindings, and Quit target")
            NSApp.terminate(nil)
            return
        }
        setState("Accessibility permission needed")
        if !AXIsProcessTrusted() {
            let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true]
            _ = AXIsProcessTrustedWithOptions(options as CFDictionary)
        }
        reconcile()
        timer = Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in
            self?.reconcile()
        }
    }

    private func add(_ title: String, _ action: Selector, to menu: NSMenu, key: String = "") {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
        item.target = self
        menu.addItem(item)
    }

    private func setState(_ value: String) {
        state = value
        stateItem.title = value
        statusItem.button?.toolTip = "Tyler — \(value)"
    }

    private func reconcile() {
        guard !quitting, worker == nil, !failed else { return }
        guard AXIsProcessTrusted() else {
            setState("Accessibility permission needed")
            return
        }
        startWorker()
    }

    private func startWorker() {
        do {
            try FileManager.default.createDirectory(at: logURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            if !FileManager.default.fileExists(atPath: logURL.path) {
                FileManager.default.createFile(atPath: logURL.path, contents: nil)
            }
            let log = try FileHandle(forWritingTo: logURL)
            log.truncateFile(atOffset: 0)
            let process = Process()
            process.executableURL = binary
            process.standardOutput = log
            process.standardError = log
            process.standardInput = FileHandle.nullDevice
            process.terminationHandler = { [weak self] process in
                DispatchQueue.main.async {
                    guard let self = self else { return }
                    self.worker = nil
                    try? self.logHandle?.close()
                    self.logHandle = nil
                    if self.quitting {
                        NSApp.reply(toApplicationShouldTerminate: true)
                    } else if process.terminationStatus == 0 {
                        NSApp.terminate(nil)
                    } else {
                        self.failed = true
                        self.setState("Tiling could not start — View Log")
                    }
                }
            }
            try process.run()
            worker = process
            logHandle = log
            setState("Starting tiling…")
        } catch {
            failed = true
            setState("Could not start: \(error.localizedDescription)")
        }
    }

    func menuWillOpen(_ menu: NSMenu) {
        // A successful spawn alone is not proof the event tap was installed.
        if worker?.isRunning == true,
           let log = try? String(contentsOf: logURL, encoding: .utf8),
           log.contains("global hotkey tap active") {
            setState("Tiling active")
        }
    }

    @objc private func retry() {
        guard worker == nil else { return }
        failed = false
        reconcile()
    }

    @objc private func openAccessibility() {
        NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")!)
    }

    @objc private func openInputMonitoring() {
        NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")!)
    }

    @objc private func showLog() { NSWorkspace.shared.open(logURL) }
    @objc private func quit() { NSApp.terminate(nil) }

    private func output(_ command: String) -> String {
        let process = Process()
        let pipe = Pipe()
        process.executableURL = binary
        process.arguments = [command]
        process.standardOutput = pipe
        process.standardError = pipe
        do {
            try process.run()
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()
            return String(decoding: data, as: UTF8.self)
        } catch { return error.localizedDescription }
    }

    @objc private func showShortcuts() {
        if shortcutsWindow == nil {
            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 760, height: 600),
                                  styleMask: [.titled, .closable, .resizable, .miniaturizable],
                                  backing: .buffered, defer: false)
            window.title = "Tyler Shortcuts"
            window.isReleasedWhenClosed = false
            window.center()
            let scroll = NSScrollView(frame: window.contentView!.bounds)
            scroll.autoresizingMask = [.width, .height]
            scroll.hasVerticalScroller = true
            scroll.hasHorizontalScroller = true
            let text = NSTextView(frame: scroll.bounds)
            text.isEditable = false
            text.isSelectable = true
            text.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
            text.textContainerInset = NSSize(width: 20, height: 20)
            text.isHorizontallyResizable = true
            text.textContainer?.widthTracksTextView = false
            text.textContainer?.containerSize = NSSize(width: 1800, height: CGFloat.greatestFiniteMagnitude)
            text.autoresizingMask = [.width]
            scroll.documentView = text
            window.contentView?.addSubview(scroll)
            shortcutsWindow = window
        }
        let scroll = shortcutsWindow!.contentView!.subviews[0] as! NSScrollView
        let text = scroll.documentView as! NSTextView
        text.string = "SUPER = Fn / Globe by default.\nThe resolved modifier and current configuration are shown below.\n\n"
            + output("check-config") + "\n" + output("shortcuts")
            + "\nShortcuts reflect your saved configuration.\nQuit from the Tyler menu to stop tiling and restore parked windows."
        shortcutsWindow?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        timer?.invalidate()
        guard let worker = worker, worker.isRunning else { return .terminateNow }
        quitting = true
        setState("Restoring windows…")
        worker.interrupt() // SIGINT lets the Rust worker restore parked windows.
        return .terminateLater
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }
}

let app = NSApplication.shared
let delegate = TylerApp()
app.setActivationPolicy(.accessory)
app.delegate = delegate
app.run()
