import AppKit
import SwiftUI
import ApplicationServices
import UniformTypeIdentifiers
import Darwin

// AppKit owns the menu bar and permission identity; the bundled Rust child
// owns tiling and the keyboard tap. Neither requires a terminal window.
final class TylerApp: NSObject, NSApplicationDelegate, NSMenuDelegate {
    private var statusItem: NSStatusItem!
    private let stateItem = NSMenuItem(title: "Starting…", action: nil, keyEquivalent: "")
    private var worker: Process?
    private var logHandle: FileHandle?
    private var shortcutsWindow: NSWindow?
    private var settingsWindow: NSWindow?
    private var timer: Timer?
    private var borderTimer: Timer?
    private let border = ActiveWindowBorder()
    private var borderConfigSource: Data?
    private var loadedBorderConfig = false
    private var askedForAccessibility = false
    private var tilingEnabled = true
    private var navigatorEnabled = true
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
        if !CommandLine.arguments.contains("--self-test"),
           peers.contains(where: { $0.processIdentifier != ProcessInfo.processInfo.processIdentifier }) {
            NSApp.terminate(nil)
            return
        }
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        let image = NSImage(named: "TylerMenuBar")
        image?.size = NSSize(width: 18, height: 17.36)
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
        add("Settings", #selector(showSettings), to: menu, key: ",")
        menu.addItem(.separator())
        add("Quit Tyler", #selector(quit), to: menu, key: "q")
        statusItem.menu = menu
        if CommandLine.arguments.contains("--self-test") {
            precondition(menu.items.contains { $0.title == "Quit Tyler" && $0.target === self })
            precondition(menu.items.filter { $0.action != nil }.map(\.title) == ["Settings", "Quit Tyler"])
            precondition(!menu.items.contains { $0.title.contains("Input Monitoring") })
            precondition(statusItem.button?.image != nil)
            application(NSApp, open: [URL(string: "tyler://keybindings")!])
            precondition(shortcutsWindow!.contentView is NSHostingView<ShortcutGuide>)
            let entries = ShortcutEntry.parse(output("shortcuts-tsv"))
            precondition(!entries.isEmpty)
            precondition(entries.contains { $0.chord == "FN + B" && $0.title == "Open Safari" })
            precondition(entries.contains { $0.chord == "FN + K" && $0.title == "Show keybindings" })
            precondition(entries.contains { $0.chord == "FN + F" && $0.title == "Open Finder" })
            precondition(entries.contains { $0.chord == "FN + C" && $0.title == "Open Safari" })
            precondition(Set(entries.map(\.group)).count == 7)
            shortcutsWindow?.close()
            showSettings()
            precondition(settingsWindow!.contentView is NSHostingView<SettingsView>)
            let settings = SettingsModel(binary: binary)
            precondition(settings.settings?.bindings.count == entries.count)
            precondition(settings.settings?.apps["browser"] != nil)
            settingsWindow?.close()
            precondition(!quitting)
            print("PASS: menu bar icon, shortcut guide, Settings window and data, resolved Fn bindings, and Quit target")
            NSApp.terminate(nil)
            return
        }
        setState("Starting Tyler…")
        NotificationCenter.default.addObserver(self, selector: #selector(borderSettingsChanged(_:)),
                                               name: .tylerBorderSettingsChanged, object: nil)
        reconcile()
        let tracking = Timer(timeInterval: 1.0 / 30.0, repeats: true) { [weak self] _ in
            guard let self = self else { return }
            self.border.update(enabled: self.worker?.isRunning == true && self.tilingEnabled && !self.quitting)
        }
        RunLoop.main.add(tracking, forMode: .common)
        borderTimer = tracking
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

    @objc private func borderSettingsChanged(_ notification: Notification) {
        if let settings = notification.object as? EditableBorder { border.configure(settings) }
    }

    private func reconcile() {
        // Use the CLI's validated config and preserve the last good appearance on invalid edits.
        let source = try? Data(contentsOf: TylerApp.configURL)
        if !loadedBorderConfig || source != borderConfigSource {
            if let data = output("settings-json").data(using: .utf8),
               let settings = try? JSONDecoder().decode(EditableSettings.self, from: data) {
                border.configure(settings.border)
                tilingEnabled = settings.tiling_enabled
                navigatorEnabled = settings.navigator.enabled
            }
            borderConfigSource = source
            loadedBorderConfig = true
        }
        guard !quitting, worker == nil, !failed else { return }
        guard tilingEnabled || navigatorEnabled else {
            setState("Tiling and navigator off")
            return
        }
        guard AXIsProcessTrusted() else {
            if !askedForAccessibility {
                askedForAccessibility = true
                let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true]
                _ = AXIsProcessTrustedWithOptions(options as CFDictionary)
            }
            setState("Accessibility permission needed")
            return
        }
        startWorker()
    }

    private static var configURL: URL {
        let env = ProcessInfo.processInfo.environment
        if let path = env["TYLER_CONFIG"] { return URL(fileURLWithPath: path) }
        let base = env["XDG_CONFIG_HOME"].map { URL(fileURLWithPath: $0) }
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".config")
        return base.appendingPathComponent("tyler/tyler.toml")
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
                        self.setState(process.terminationStatus == 78 ? "Native desktops need setup" : "Tiling could not start")
                    }
                }
            }
            try process.run()
            worker = process
            logHandle = log
            setState("Starting Tyler…")
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
            let paused = log.range(of: "Tyler: native Spaces paused:", options: .backwards)
            let restored = log.range(of: "Tyler: native Spaces connection restored", options: .backwards)
            if let paused = paused, restored == nil || paused.lowerBound > restored!.lowerBound {
                setState("Native desktops unavailable")
            } else {
                setState(tilingEnabled ? (navigatorEnabled ? "Tiling and desktop navigator active" : "Tiling active") : (navigatorEnabled ? "Desktop navigator active" : "Tiling and navigator off"))
            }
        }
    }

    @objc private func checkNativeDesktops() {
        let alert = NSAlert()
        alert.messageText = "Native Mission Control desktops"
        alert.informativeText = output("check-spaces")
        alert.addButton(withTitle: "OK")
        alert.runModal()
    }

    @objc private func retile() {
        guard let worker = worker, worker.isRunning else { return }
        kill(worker.processIdentifier, SIGUSR1)
    }

    @objc private func retry() {
        guard worker == nil else { return }
        failed = false
        reconcile()
    }

    @objc private func openAccessibility() {
        NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")!)
    }

    @objc private func revealApp() {
        NSWorkspace.shared.activateFileViewerSelecting([Bundle.main.bundleURL])
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
        let raw = output("shortcuts-tsv")
        let entries = ShortcutEntry.parse(raw)
        let guide = ShortcutGuide(entries: entries, configuration: output("check-config"), error: entries.isEmpty ? raw : nil)
        if shortcutsWindow == nil {
            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 820, height: 740),
                                  styleMask: [.titled, .closable, .resizable, .miniaturizable],
                                  backing: .buffered, defer: false)
            window.title = "Tyler Shortcuts"
            window.titlebarAppearsTransparent = true
            window.isReleasedWhenClosed = false
            window.minSize = NSSize(width: 640, height: 480)
            window.center()
            shortcutsWindow = window
        }
        shortcutsWindow?.contentView = NSHostingView(rootView: guide)
        shortcutsWindow?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func application(_ application: NSApplication, open urls: [URL]) {
        if urls.contains(where: { $0.scheme == "tyler" && $0.host == "keybindings" }) {
            showShortcuts()
        }
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        showSettings()
        return true
    }

    @objc private func showSettings() {
        if let window = settingsWindow, window.isVisible {
            window.makeKeyAndOrderFront(nil)
        } else {
            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 820, height: 700),
                                  styleMask: [.titled, .closable, .resizable, .miniaturizable],
                                  backing: .buffered, defer: false)
            window.title = "Tyler Settings"
            window.isReleasedWhenClosed = false
            window.minSize = NSSize(width: 700, height: 500)
            window.contentView = NSHostingView(rootView: SettingsView(model: SettingsModel(binary: binary)))
            window.center()
            settingsWindow = window
            window.makeKeyAndOrderFront(nil)
        }
        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        timer?.invalidate()
        borderTimer?.invalidate()
        border.hide()
        guard let worker = worker, worker.isRunning else { return .terminateNow }
        quitting = true
        setState("Stopping tiling…")
        worker.interrupt() // SIGINT restores explicit scratchpad windows and leaves native desktops intact.
        return .terminateLater
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }
}

struct ShortcutEntry: Identifiable {
    let id: Int
    let group: String
    let chord: String
    let title: String

    static func parse(_ output: String) -> [ShortcutEntry] {
        output.split(separator: "\n").enumerated().compactMap { index, line in
            let fields = line.split(separator: "\t", maxSplits: 2, omittingEmptySubsequences: false)
            guard fields.count == 3 else { return nil }
            return ShortcutEntry(id: index, group: String(fields[0]), chord: String(fields[1]), title: String(fields[2]))
        }
    }
}

private let shortcutGroups: [(String, String)] = [
    ("Focus & navigation", "arrow.up.left.and.arrow.down.right"),
    ("Arrange & resize", "rectangle.split.2x2"),
    ("Window controls", "macwindow"),
    ("Workspaces", "square.grid.2x2"),
    ("Scratchpad", "square.on.square"),
    ("Launch apps & commands", "arrow.up.forward.app"),
    ("Tyler", "slider.horizontal.3")
]

struct ShortcutGuide: View {
    let entries: [ShortcutEntry]
    let configuration: String
    let error: String?
    @State private var search = ""

    private var filtered: [ShortcutEntry] {
        let query = search.trimmingCharacters(in: .whitespacesAndNewlines)
        return entries.filter { query.isEmpty || "\($0.title) \($0.chord) \($0.group)".localizedCaseInsensitiveContains(query) }
    }

    var body: some View {
        VStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 16) {
                HStack(alignment: .top, spacing: 14) {
                    Image(nsImage: NSImage(named: "TylerIcon") ?? NSApp.applicationIconImage)
                        .resizable()
                        .interpolation(.high)
                        .scaledToFit()
                        .frame(width: 52, height: 52)
                        .clipShape(RoundedRectangle(cornerRadius: 12))
                    VStack(alignment: .leading, spacing: 5) {
                        Text("Make room for your work.").font(.system(size: 25, weight: .semibold, design: .rounded))
                        Text("Your keyboard guide to Tyler").font(.subheadline).foregroundColor(.secondary)
                    }
                    Spacer()
                    Text("\(entries.count) shortcuts").font(.caption).foregroundColor(.secondary).padding(.top, 6)
                }
                HStack(spacing: 8) {
                    Image(systemName: "magnifyingglass").foregroundColor(.secondary)
                    TextField("Find an action, key, or section", text: $search).textFieldStyle(.plain)
                    if !search.isEmpty {
                        Button { search = "" } label: { Image(systemName: "xmark.circle.fill").foregroundColor(.secondary) }
                            .buttonStyle(.plain).accessibilityLabel("Clear search")
                    }
                }
                .padding(10)
                .background(Color(NSColor.controlBackgroundColor), in: RoundedRectangle(cornerRadius: 9))
                .overlay(RoundedRectangle(cornerRadius: 9).stroke(Color.primary.opacity(0.1)))
            }.padding(26)
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 22) {
                    if search.isEmpty {
                        VStack(alignment: .leading, spacing: 8) {
                            Label("A little tiling intuition", systemImage: "sparkles").font(.subheadline.weight(.semibold))
                            Text("New windows split the focused tile. Close a window and its neighbor expands. Focus a tile to choose what splits next.")
                                .font(.subheadline).foregroundColor(.secondary).fixedSize(horizontal: false, vertical: true)
                            Text("Fn means the Globe key. Workspace shortcuts use your Mission Control desktops. Create desktops there first; switching keeps windows open.")
                                .font(.caption).foregroundColor(.secondary).fixedSize(horizontal: false, vertical: true)
                        }.padding(16).frame(maxWidth: .infinity, alignment: .leading)
                            .background(Color.accentColor.opacity(0.06), in: RoundedRectangle(cornerRadius: 12))
                    }
                    if let error = error {
                        Text(error).font(.body).foregroundColor(.secondary).textSelection(.enabled)
                    } else if filtered.isEmpty {
                        VStack(spacing: 8) {
                            Image(systemName: "magnifyingglass").font(.title2)
                            Text("No matching shortcuts").font(.headline)
                            Text("Try a key, action, or workspace number.").font(.subheadline)
                        }.foregroundColor(.secondary).frame(maxWidth: .infinity).padding(32)
                    }
                    ForEach(shortcutGroups, id: \.0) { group, symbol in
                        let rows = filtered.filter { $0.group == group }
                        if !rows.isEmpty {
                            VStack(alignment: .leading, spacing: 10) {
                                Label(group, systemImage: symbol).font(.system(size: 13, weight: .semibold)).foregroundColor(.secondary)
                                VStack(spacing: 0) {
                                    ForEach(rows) { row in
                                        HStack(spacing: 20) {
                                            Text(row.title).font(.system(size: 13)).fixedSize(horizontal: false, vertical: true)
                                            Spacer(minLength: 8)
                                            ShortcutKeys(chord: row.chord)
                                        }.padding(.horizontal, 14).padding(.vertical, 10)
                                        if row.id != rows.last?.id { Divider().padding(.horizontal, 14) }
                                    }
                                }
                                .background(Color(NSColor.controlBackgroundColor).opacity(0.5), in: RoundedRectangle(cornerRadius: 12))
                                .overlay(RoundedRectangle(cornerRadius: 12).stroke(Color.primary.opacity(0.07)))
                            }
                        }
                    }
                    DisclosureGroup("Configuration details") {
                        Text(configuration).font(.system(size: 11, design: .monospaced))
                            .foregroundColor(.secondary).textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading).padding(.top, 8)
                    }.font(.caption).foregroundColor(.secondary)
                }.padding(26)
            }
            Divider()
            HStack(spacing: 6) {
                Image(systemName: "checkmark.circle")
                Text("Shortcuts reflect your saved configuration")
                Spacer()
            }.font(.caption).foregroundColor(.secondary).padding(.horizontal, 26).padding(.vertical, 12)
        }
        .background(Color(NSColor.windowBackgroundColor))
    }

}

struct ShortcutKeys: View {
    let chord: String

    var body: some View {
        Text(chord.components(separatedBy: " + ").map(keyLabel).joined(separator: " "))
            .font(.system(size: 12, weight: .medium, design: .monospaced))
            .padding(.horizontal, 10).padding(.vertical, 5)
            .background(Color.primary.opacity(0.06), in: Capsule())
            .fixedSize()
            .accessibilityLabel(chord)
    }

    private func keyLabel(_ key: String) -> String {
        ["FN": "fn", "SHIFT": "⇧", "CTRL": "⌃", "ALT": "⌥", "CMD": "⌘",
         "LEFT": "←", "RIGHT": "→", "UP": "↑", "DOWN": "↓", "RETURN": "↩",
         "TAB": "⇥", "ESCAPE": "esc", "SPACE": "space", "GRAVE": "`",
         "BRACKETLEFT": "[", "BRACKETRIGHT": "]", "APOSTROPHE": "'", "BACKSLASH": "\\", "COMMA": ",", "PERIOD": ".",
         "MINUS": "−", "EQUAL": "="][key] ?? key
    }
}

struct EditableShortcut: Codable, Identifiable {
    var id = UUID()
    var chord: String
    var action: String
    var title: String? = nil
    var group: String? = nil
    enum CodingKeys: String, CodingKey { case chord, action, title, group }

    func displayTitle(apps: [String: String]) -> String {
        let parts = action.split(maxSplits: 1, whereSeparator: { $0.isWhitespace })
        let target = parts.count > 1 ? String(parts[1]) : ""
        switch parts.first?.lowercased() {
        case "launch":
            let app = apps[target] ?? target
            let name = app.contains("/") ? URL(fileURLWithPath: app).deletingPathExtension().lastPathComponent : app
            return "Open \(name)"
        case "webapp": return "Open \(target)"
        case "exec": return "Run \(target)"
        default: return title ?? action
        }
    }

    var displayGroup: String {
        let command = action.split(whereSeparator: { $0.isWhitespace }).first?.lowercased()
        return ["launch", "webapp", "exec"].contains(command ?? "") ? "Launch apps & commands" : (group ?? "Tyler")
    }
}

struct EditableDesktop: Codable, Identifiable {
    var id: String
    var number: Int
    var name: String
    var icon: String
    var shortcut: String
    var selected: Bool
}
struct EditableDesktopLabel: Codable {
    var name = ""
    var icon = ""
}
struct EditableNavigator: Codable {
    var enabled: Bool
    var labels: [String: EditableDesktopLabel]
}
struct EditableSettings: Codable {
    var tiling_enabled: Bool
    var navigator: EditableNavigator
    var desktops: [EditableDesktop]
    var desktop_error: String?

    var source: String?
    var apps: [String: String]
    var bindings: [EditableShortcut]
    var layout: EditableLayout
    var animations: EditableAnimations
    var border: EditableBorder
}

struct EditableLayout: Codable {
    var gaps_in: Double
    var gaps_out: Double
    var force_split: Int
    var split_ratio: Double
}

extension Notification.Name {
    static let tylerBorderSettingsChanged = Notification.Name("TylerBorderSettingsChanged")
}

struct EditableBorder: Codable {
    var width: Double
    var color: String
}

extension NSColor {
    convenience init?(borderHex: String) {
        guard borderHex.count == 7, borderHex.first == "#",
              let value = UInt32(borderHex.dropFirst(), radix: 16) else { return nil }
        self.init(srgbRed: CGFloat((value >> 16) & 255) / 255,
                  green: CGFloat((value >> 8) & 255) / 255,
                  blue: CGFloat(value & 255) / 255, alpha: 1)
    }

    var borderHex: String {
        let rgb = usingColorSpace(.sRGB) ?? self
        return String(format: "#%02X%02X%02X", Int((rgb.redComponent * 255).rounded()),
                      Int((rgb.greenComponent * 255).rounded()), Int((rgb.blueComponent * 255).rounded()))
    }
}

// A transparent, non-activating overlay keeps all clicks and keyboard focus in the target app.
private final class BorderPanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

// Optional compositor metadata: resolving symbols at runtime keeps older macOS
// versions working. AX and public window-list APIs do not expose corner radii.
private final class WindowCornerRadius {
    private typealias Connection = @convention(c) () -> Int32
    private typealias Query = @convention(c) (Int32, CFArray, UInt32) -> Unmanaged<CFTypeRef>?
    private typealias Iterator = @convention(c) (CFTypeRef) -> Unmanaged<CFTypeRef>?
    private typealias Advance = @convention(c) (CFTypeRef) -> Bool
    private typealias Radii = @convention(c) (CFTypeRef) -> Unmanaged<CFArray>?
    private let library = dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight", RTLD_LAZY)
    private var lastWindow: CGWindowID?
    private var lastRead = Date.distantPast
    private var cached: CGFloat = 10

    private func function<T>(_ name: String, as type: T.Type) -> T? {
        guard let library = library, let pointer = dlsym(library, name) else { return nil }
        return unsafeBitCast(pointer, to: type)
    }

    func radius(for window: CGWindowID) -> CGFloat {
        guard lastWindow != window || Date().timeIntervalSince(lastRead) >= 0.5 else { return cached }
        lastWindow = window
        lastRead = Date()
        if #available(macOS 26.0, *) { cached = 26 } else { cached = 10 }
        guard #available(macOS 26.0, *),
              let connection = function("SLSMainConnectionID", as: Connection.self),
              let query = function("SLSWindowQueryWindows", as: Query.self),
              let iterator = function("SLSWindowQueryResultCopyWindows", as: Iterator.self),
              let advance = function("SLSWindowIteratorAdvance", as: Advance.self),
              let radii = function("SLSWindowIteratorGetCornerRadii", as: Radii.self),
              let result = query(connection(), [NSNumber(value: window)] as CFArray, 0)?.takeRetainedValue(),
              let cursor = iterator(result)?.takeRetainedValue(), advance(cursor),
              let values = radii(cursor)?.takeRetainedValue(),
              let value = (values as? [NSNumber])?.first else { return cached }
        let radius = CGFloat(value.doubleValue)
        if radius.isFinite && (0...200).contains(radius) { cached = radius }
        return cached
    }
}

private final class BorderOutline: NSView {
    var width: CGFloat = 4
    var color = NSColor(borderHex: "#586B4A")!
    var cornerRadius: CGFloat = 10 {
        didSet { if oldValue != cornerRadius { needsDisplay = true } }
    }
    override func draw(_ dirtyRect: NSRect) {
        NSGraphicsContext.current?.cgContext.clear(dirtyRect)
        color.setStroke()
        let path = NSBezierPath(roundedRect: bounds.insetBy(dx: width / 2, dy: width / 2),
                                xRadius: cornerRadius + width / 2, yRadius: cornerRadius + width / 2)
        path.lineWidth = width
        path.stroke()
    }
}

private final class ActiveWindowBorder {
    private let panel = BorderPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel],
                                    backing: .buffered, defer: false)
    private let outline = BorderOutline()
    private let corners = WindowCornerRadius()
    private var suspendedUntil = Date.distantPast
    private var sessionActive = true
    private var screensAwake = true
    private var observers: [NSObjectProtocol] = []

    init() {
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = false
        panel.ignoresMouseEvents = true
        panel.hidesOnDeactivate = false
        panel.level = .floating
        panel.collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle]
        panel.isExcludedFromWindowsMenu = true
        panel.isReleasedWhenClosed = false
        panel.contentView = outline
        panel.setAccessibilityElement(false)
        outline.setAccessibilityElement(false)
        let center = NSWorkspace.shared.notificationCenter
        for name in [NSWorkspace.activeSpaceDidChangeNotification, NSWorkspace.didActivateApplicationNotification] {
            observers.append(center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                self?.hide()
                self?.suspendedUntil = Date().addingTimeInterval(0.15)
            })
        }
        observers.append(center.addObserver(forName: NSWorkspace.screensDidSleepNotification,
                                             object: nil, queue: .main) { [weak self] _ in
            self?.screensAwake = false
            self?.hide()
        })
        observers.append(center.addObserver(forName: NSWorkspace.screensDidWakeNotification,
                                             object: nil, queue: .main) { [weak self] _ in self?.screensAwake = true })
        observers.append(center.addObserver(forName: NSWorkspace.sessionDidResignActiveNotification,
                                             object: nil, queue: .main) { [weak self] _ in
            self?.sessionActive = false
            self?.hide()
        })
        observers.append(center.addObserver(forName: NSWorkspace.sessionDidBecomeActiveNotification,
                                             object: nil, queue: .main) { [weak self] _ in self?.sessionActive = true })
    }

    deinit {
        for observer in observers { NSWorkspace.shared.notificationCenter.removeObserver(observer) }
    }

    func configure(_ settings: EditableBorder) {
        outline.width = CGFloat(settings.width)
        outline.color = NSColor(borderHex: settings.color) ?? outline.color
        outline.needsDisplay = true
        if settings.width == 0 { hide() }
    }

    func hide() { panel.orderOut(nil) }

    private func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else { return nil }
        return value
    }

    func update(enabled: Bool) {
        guard enabled, sessionActive, screensAwake, outline.width > 0, Date() >= suspendedUntil, AXIsProcessTrusted(),
              let app = NSWorkspace.shared.frontmostApplication, !app.isTerminated, !app.isHidden else {
            hide(); return
        }
        let element = AXUIElementCreateApplication(app.processIdentifier)
        AXUIElementSetMessagingTimeout(element, 0.05)
        guard let focused = attribute(element, kAXFocusedWindowAttribute),
              CFGetTypeID(focused) == AXUIElementGetTypeID() else { hide(); return }
        let window = focused as! AXUIElement
        guard (attribute(window, kAXMinimizedAttribute) as? Bool) != true,
              (attribute(window, "AXFullScreen") as? Bool) != true,
              let rawPosition = attribute(window, kAXPositionAttribute),
              let rawSize = attribute(window, kAXSizeAttribute),
              CFGetTypeID(rawPosition) == AXValueGetTypeID(), CFGetTypeID(rawSize) == AXValueGetTypeID() else {
            hide(); return
        }
        var position = CGPoint.zero
        var size = CGSize.zero
        guard AXValueGetValue(rawPosition as! AXValue, .cgPoint, &position),
              AXValueGetValue(rawSize as! AXValue, .cgSize, &size),
              position.x.isFinite, position.y.isFinite, size.width.isFinite, size.height.isFinite,
              size.width > 0, size.height > 0, let mainScreen = NSScreen.screens.first else { hide(); return }
        // AX uses a top-left origin on the primary display; AppKit uses bottom-left.
        let frame = NSRect(x: position.x, y: mainScreen.frame.maxY - position.y - size.height,
                           width: size.width, height: size.height)
        guard NSScreen.screens.contains(where: { $0.frame.intersects(frame) }) else { hide(); return }
        // AX can retain focus on a window from the previous desktop. Check public
        // WindowServer bounds before showing an outline; titles need no screen-recording access.
        guard let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
                as? [[String: Any]], let target = windows.first(where: { info in
            guard (info[kCGWindowOwnerPID as String] as? Int32) == app.processIdentifier,
                  (info[kCGWindowLayer as String] as? Int) == 0,
                  let bounds = info[kCGWindowBounds as String] as? NSDictionary,
                  let rect = CGRect(dictionaryRepresentation: bounds) else { return false }
            return abs(rect.minX - position.x) < 2 && abs(rect.minY - position.y) < 2
                && abs(rect.width - size.width) < 2 && abs(rect.height - size.height) < 2
        }), let windowID = target[kCGWindowNumber as String] as? UInt32 else { hide(); return }
        outline.cornerRadius = corners.radius(for: windowID)
        let expanded = frame.insetBy(dx: -outline.width, dy: -outline.width)
        if panel.frame != expanded { panel.setFrame(expanded, display: true) }
        if !panel.isVisible { panel.orderFrontRegardless() }
    }
}

struct BorderPalette: Identifiable {
    struct Swatch: Identifiable {
        let name: String
        let hex: String
        var id: String { hex }
    }
    let name: String
    let colors: [Swatch]
    var id: String { name }
    static let themes: [BorderPalette] = [
        BorderPalette(name: "Pastel", colors: [
            Swatch(name: "Mint", hex: "#A8D5BA"), Swatch(name: "Sky", hex: "#A9C7EA"),
            Swatch(name: "Peach", hex: "#F2B8A2"), Swatch(name: "Lilac", hex: "#C7B3E5")
        ]),
        BorderPalette(name: "Electric", colors: [
            Swatch(name: "Lime", hex: "#A3FF12"), Swatch(name: "Cyan", hex: "#00D9FF"),
            Swatch(name: "Magenta", hex: "#FF3DBB"), Swatch(name: "Amber", hex: "#FFB000")
        ]),
        // Official accent colors from each theme's published palette.
        BorderPalette(name: "Coding themes", colors: [
            Swatch(name: "Dracula", hex: "#FF79C6"), Swatch(name: "Nord", hex: "#88C0D0"),
            Swatch(name: "Catppuccin", hex: "#CBA6F7"), Swatch(name: "Tokyo Night", hex: "#7AA2F7")
        ])
    ]
}

struct EditableAnimations: Codable {
    var enabled: Bool
    var duration_ms: Int
}

final class SettingsModel: ObservableObject {
    @Published var settings: EditableSettings?
    @Published var message = ""
    @Published var dirty = false
    private let binary: URL

    init(binary: URL) {
        self.binary = binary
        reload()
    }

    private func run(_ command: String, input: Data? = nil) throws -> Data {
        let process = Process()
        let output = Pipe()
        let errors = Pipe()
        process.executableURL = binary
        process.arguments = [command]
        process.standardOutput = output
        process.standardError = errors
        let stdin = Pipe()
        process.standardInput = input == nil ? FileHandle.nullDevice : stdin
        try process.run()
        if let input = input {
            stdin.fileHandleForWriting.write(input)
            try stdin.fileHandleForWriting.close()
        }
        let data = output.fileHandleForReading.readDataToEndOfFile()
        let error = errors.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else {
            throw NSError(domain: "Tyler", code: Int(process.terminationStatus), userInfo: [
                NSLocalizedDescriptionKey: String(decoding: error, as: UTF8.self)
            ])
        }
        return data
    }

    func reload() {
        do {
            settings = try JSONDecoder().decode(EditableSettings.self, from: run("settings-json"))
            dirty = false
            message = ""
        } catch { message = error.localizedDescription }
    }

    func refreshDesktops() {
        do {
            let latest = try JSONDecoder().decode(EditableSettings.self, from: run("settings-json"))
            settings?.desktops = latest.desktops
            settings?.desktop_error = latest.desktop_error
        } catch { message = error.localizedDescription }
    }

    func save() {
        guard let settings = settings else { return }
        do {
            _ = try run("save-settings", input: JSONEncoder().encode(settings))
            reload()
            if let border = self.settings?.border {
                NotificationCenter.default.post(name: .tylerBorderSettingsChanged, object: border)
            }
            message = "Saved. Tyler reloads your configuration automatically."
        } catch { message = error.localizedDescription }
    }

    func update(_ row: EditableShortcut) {
        guard settings != nil else { return }
        if let index = settings!.bindings.firstIndex(where: { $0.id == row.id }) {
            settings!.bindings[index] = row
        } else {
            settings!.bindings.append(row)
        }
        dirty = true
        message = ""
    }
}

private func chooseApplication(_ selected: (String) -> Void) {
    let panel = NSOpenPanel()
    panel.title = "Choose Application"
    panel.directoryURL = URL(fileURLWithPath: "/Applications")
    panel.allowedContentTypes = [.applicationBundle]
    panel.allowsMultipleSelection = false
    panel.canChooseDirectories = false
    if panel.runModal() == .OK, let url = panel.url { selected(url.path) }
}

struct SettingsView: View {
    @ObservedObject var model: SettingsModel
    @State private var editing: EditableShortcut?
    @State private var search = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Tyler Settings").font(.title2.bold())
            if let settings = model.settings {
                TabView {
                    desktopControls.tabItem { Label("Desktops", systemImage: "rectangle.3.group") }
                    layoutControls.tabItem { Label("Layout & motion", systemImage: "rectangle.split.2x2") }
                    VStack(alignment: .leading, spacing: 16) {
                        Text("Edit shortcuts, choose your apps, or add an app or shell command.")
                            .foregroundColor(.secondary)
                        HStack {
                            Text("Default apps").font(.headline)
                            Spacer()
                        }
                        ForEach(settings.apps.keys.sorted(), id: \.self) { key in
                            HStack {
                                Text(key.replacingOccurrences(of: "_", with: " ").capitalized).frame(width: 120, alignment: .leading)
                                Text(settings.apps[key] ?? "").lineLimit(1).truncationMode(.middle)
                                Spacer()
                                Button("Choose…") {
                                    chooseApplication { path in
                                        model.settings?.apps[key] = path
                                        model.dirty = true
                                    }
                                }.accessibilityLabel("Choose \(key) application")
                            }
                        }
                        Divider()
                        HStack(spacing: 12) {
                            HStack(spacing: 8) {
                                Image(systemName: "magnifyingglass").foregroundColor(.secondary)
                                TextField("Find an action, key, or section", text: $search)
                                    .textFieldStyle(.plain)
                                if !search.isEmpty {
                                    Button { search = "" } label: {
                                        Image(systemName: "xmark.circle.fill").foregroundColor(.secondary)
                                    }.buttonStyle(.plain).accessibilityLabel("Clear search")
                                }
                            }
                            .padding(10)
                            .background(Color(NSColor.controlBackgroundColor), in: Capsule())
                            .overlay(Capsule().stroke(Color.primary.opacity(0.1)))
                            Button { editing = EditableShortcut(chord: "", action: "launch ") } label: {
                                Image(systemName: "plus.circle").font(.system(size: 18))
                            }
                            .buttonStyle(.plain).foregroundColor(.secondary)
                            .help("Add shortcut").accessibilityLabel("Add shortcut")
                        }
                        ScrollView {
                            LazyVStack(alignment: .leading, spacing: 22) {
                                let query = search.trimmingCharacters(in: .whitespacesAndNewlines)
                                let filtered = settings.bindings.filter {
                                    query.isEmpty || "\($0.displayTitle(apps: settings.apps)) \($0.chord) \($0.displayGroup) \($0.action)"
                                        .localizedCaseInsensitiveContains(query)
                                }
                                if filtered.isEmpty {
                                    Text("No matching shortcuts")
                                        .foregroundColor(.secondary)
                                        .frame(maxWidth: .infinity).padding(24)
                                }
                                ForEach(shortcutGroups, id: \.0) { group, symbol in
                                    let rows = filtered.filter { $0.displayGroup == group }.sorted {
                                        let order = $0.displayTitle(apps: settings.apps)
                                            .localizedStandardCompare($1.displayTitle(apps: settings.apps))
                                        return order == .orderedSame ? $0.chord < $1.chord : order == .orderedAscending
                                    }
                                    if !rows.isEmpty {
                                        VStack(alignment: .leading, spacing: 8) {
                                            Label(group, systemImage: symbol)
                                                .font(.system(size: 12, weight: .semibold)).foregroundColor(.secondary)
                                            VStack(spacing: 0) {
                                                ForEach(rows) { row in
                                                    HStack(spacing: 16) {
                                                        Text(row.displayTitle(apps: settings.apps))
                                                            .font(.system(size: 13))
                                                            .fixedSize(horizontal: false, vertical: true)
                                                        Spacer(minLength: 8)
                                                        Button { editing = row } label: {
                                                            ShortcutKeys(chord: row.chord)
                                                        }
                                                        .buttonStyle(.plain)
                                                        .help("Edit shortcut")
                                                        .accessibilityLabel("Edit \(row.displayTitle(apps: settings.apps)), \(row.chord)")
                                                        Button {
                                                            model.settings?.bindings.removeAll { $0.id == row.id }
                                                            model.dirty = true
                                                            model.message = ""
                                                        } label: {
                                                            Image(systemName: "minus.circle").font(.system(size: 16))
                                                        }
                                                        .buttonStyle(.plain).foregroundColor(.secondary)
                                                        .help("Remove shortcut")
                                                        .accessibilityLabel("Remove \(row.displayTitle(apps: settings.apps)), \(row.chord)")
                                                    }.padding(.vertical, 12)
                                                    if row.id != rows.last?.id { Divider() }
                                                }
                                            }
                                        }
                                    }
                                }
                            }.padding(.trailing, 4)
                        }

                    }.padding(16).tabItem { Label("Shortcuts & apps", systemImage: "keyboard") }
                }
            }
            HStack {
                Text(model.message.isEmpty ? (model.dirty ? "Unsaved changes" : "Changes apply when you save.") : model.message)
                    .font(.caption).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                Spacer()
                Button("Revert") { model.reload() }.disabled(!model.dirty)
                Button("Save Changes") { model.save() }.disabled(!model.dirty)
            }
        }.padding(24)
        .sheet(item: $editing) { row in
            ShortcutEditor(row: row, apps: model.settings?.apps ?? [:]) { model.update($0) }
        }
    }

    private var desktopControls: some View {
        VStack(alignment: .leading, spacing: 16) {
            Toggle("Enable automatic tiling", isOn: setting(\.tiling_enabled, fallback: true))
            Toggle("Show desktop navigator", isOn: setting(\.navigator.enabled, fallback: true))
            Text("Desktops appear below the macOS menu bar on the main display. Numbers follow Mission Control order and match your Tyler shortcuts.")
                .font(.callout).foregroundColor(.secondary)
            Divider()
            HStack {
                Text("Desktop names & icons").font(.headline)
                Spacer()
                Button("Refresh desktops") { model.refreshDesktops() }
            }
            if let error = model.settings?.desktop_error {
                Text(error).font(.callout).foregroundColor(.secondary).textSelection(.enabled)
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    ForEach(model.settings?.desktops ?? []) { desktop in
                        HStack(spacing: 12) {
                            Text("\(desktop.number)")
                                .font(.system(.body, design: .rounded).monospacedDigit())
                                .frame(width: 24)
                            TextField("Desktop name", text: desktopLabel(desktop.id, icon: false))
                            TextField("Emoji or sf:briefcase", text: desktopLabel(desktop.id, icon: true))
                                .frame(width: 180)
                            Text(desktop.shortcut.isEmpty ? "Unassigned" : desktop.shortcut)
                                .font(.caption).foregroundColor(.secondary).frame(width: 100)
                        }
                        .accessibilityElement(children: .contain)
                        .accessibilityLabel("Desktop \(desktop.number)")
                    }
                }.padding(.vertical, 4)
            }
            Text("An icon replaces the name in the strip; the name remains in its tooltip. Use an emoji or an Apple symbol such as sf:briefcase, sf:house, or sf:terminal. Names stay with existing desktops when reordered and across Tyler restarts. Deleted or recreated desktops have new identities.")
                .font(.caption).foregroundColor(.secondary)
        }.padding(16)
    }

    private func desktopLabel(_ id: String, icon: Bool) -> SwiftUI.Binding<String> {
        SwiftUI.Binding(
            get: {
                let label = model.settings?.navigator.labels[id] ?? EditableDesktopLabel()
                return icon ? label.icon : label.name
            },
            set: { value in
                var label = model.settings?.navigator.labels[id] ?? EditableDesktopLabel()
                if icon { label.icon = value } else { label.name = value }
                model.settings?.navigator.labels[id] = label
                model.dirty = true
            })
    }

    private func setting<Value>(_ path: WritableKeyPath<EditableSettings, Value>, fallback: Value) -> SwiftUI.Binding<Value> {
        SwiftUI.Binding(
            get: { model.settings?[keyPath: path] ?? fallback },
            set: { value in
                model.settings?[keyPath: path] = value
                model.dirty = true
                model.message = ""
            }
        )
    }

    private var borderPalettes: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(BorderPalette.themes) { palette in
                VStack(alignment: .leading, spacing: 6) {
                    Text(palette.name).font(.caption).foregroundColor(.secondary)
                    HStack(spacing: 8) {
                        ForEach(palette.colors) { swatch in
                            let selected = model.settings?.border.color.uppercased() == swatch.hex
                            Button {
                                setting(\.border.color, fallback: "#586B4A").wrappedValue = swatch.hex
                            } label: {
                                HStack(spacing: 7) {
                                    Circle().fill(Color(NSColor(borderHex: swatch.hex)!))
                                        .frame(width: 18, height: 18)
                                        .overlay(Circle().stroke(Color.primary.opacity(0.15), lineWidth: 0.5))
                                    Text(swatch.name).font(.system(size: 12)).lineLimit(1)
                                    Spacer(minLength: 0)
                                    if selected { Image(systemName: "checkmark").font(.system(size: 10, weight: .semibold)) }
                                }
                                .padding(8).frame(maxWidth: .infinity)
                                .background(Color.primary.opacity(selected ? 0.08 : 0.025), in: RoundedRectangle(cornerRadius: 8))
                                .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color.primary.opacity(selected ? 0.45 : 0.1), lineWidth: 1))
                                .contentShape(RoundedRectangle(cornerRadius: 8))
                            }
                            .buttonStyle(.plain)
                            .help("\(swatch.name) · \(swatch.hex)")
                            .accessibilityLabel("\(palette.name), \(swatch.name), \(swatch.hex)")
                            .accessibilityAddTraits(selected ? .isSelected : [])
                        }
                    }
                }
            }
        }
    }

    private var layoutControls: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                Text("Window spacing").font(.headline)
                Stepper(value: setting(\.layout.gaps_in, fallback: 5), in: 0...200, step: 1) {
                    Text("Inner gap per edge: \(model.settings?.layout.gaps_in ?? 5, specifier: "%.0f") pt")
                }
                Stepper(value: setting(\.layout.gaps_out, fallback: 10), in: 0...200, step: 1) {
                    Text("Screen edge gap: \(model.settings?.layout.gaps_out ?? 10, specifier: "%.0f") pt")
                }
                Text("Adjacent windows are separated by twice the inner gap.")
                    .font(.caption).foregroundColor(.secondary)
                Divider()
                Text("Active window border").font(.headline)
                borderPalettes
                ColorPicker("Custom color", selection: SwiftUI.Binding(
                    get: { Color(NSColor(borderHex: model.settings?.border.color ?? "#586B4A") ?? .systemGreen) },
                    set: { setting(\.border.color, fallback: "#586B4A").wrappedValue = NSColor($0).borderHex }
                ), supportsOpacity: false)
                Stepper(value: setting(\.border.width, fallback: 4), in: 0...20, step: 1) {
                    Text("Width: \(model.settings?.border.width ?? 4, specifier: "%.0f") pt")
                }
                Text("Stays visible on the active window. Set width to 0 to hide it.")
                    .font(.caption).foregroundColor(.secondary)
                Divider()
                Text("New window splits").font(.headline)
                Picker("New window position", selection: setting(\.layout.force_split, fallback: 2)) {
                    Text("Automatic (right / bottom)").tag(0)
                    Text("Left / top").tag(1)
                    Text("Right / bottom").tag(2)
                }
                HStack {
                    Text("Split ratio")
                    Slider(value: setting(\.layout.split_ratio, fallback: 0.5), in: 0.1...0.9, step: 0.05)
                        .accessibilityLabel("Split ratio")
                    Text("\((model.settings?.layout.split_ratio ?? 0.5) * 100, specifier: "%.0f")%")
                        .monospacedDigit().frame(width: 42, alignment: .trailing)
                }
                Text("Split settings apply when new windows are tiled.")
                    .font(.caption).foregroundColor(.secondary)
                Divider()
                Text("Window motion").font(.headline)
                Toggle("Animate window moves and resizes (experimental)", isOn: setting(\.animations.enabled, fallback: false))
                Stepper(value: setting(\.animations.duration_ms, fallback: 160), in: 80...400, step: 10) {
                    Text("Duration: \(model.settings?.animations.duration_ms ?? 160) ms")
                }.disabled(model.settings?.animations.enabled != true)
                Text("Mission Control desktop transitions follow macOS settings.")
                    .font(.caption).foregroundColor(.secondary)
            }.padding(20)
        }
    }

}

struct ShortcutEditor: View {
    @Environment(\.dismiss) private var dismiss
    @State var row: EditableShortcut
    let apps: [String: String]
    let onSave: (EditableShortcut) -> Void
    @State private var kind = ""
    @State private var target = ""

    private var editableAction: Bool {
        ["launch", "exec", "webapp"].contains(row.action.components(separatedBy: " ").first ?? "")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Edit shortcut").font(.title2.bold())
            TextField("Shortcut (e.g. FN + SHIFT + A)", text: $row.chord)
                .accessibilityLabel("Shortcut keys")
            Text("Use FN, CTRL, ALT, SHIFT or CMD plus a key, separated by +. Assigned shortcuts must be unique.")
                .font(.caption).foregroundColor(.secondary)
            if editableAction {
                Picker("Action", selection: $kind) {
                    Text("Launch app").tag("launch")
                    Text("Shell command").tag("exec")
                    Text("Website").tag("webapp")
                }
                HStack {
                    TextField(kind == "exec" ? "Shell command" : kind == "webapp" ? "https://…" : "App name, path, or default app alias", text: $target)
                        .accessibilityLabel("Shortcut target")
                    if kind == "launch" {
                        Button("Choose App…") { chooseApplication { target = $0 } }
                    }
                }
                if kind == "launch", let app = apps[target] {
                    Text("Default \(target): \(app)").font(.caption).foregroundColor(.secondary)
                }
                if kind == "exec" {
                    Text("Runs through /bin/sh when you press this shortcut.").font(.caption).foregroundColor(.secondary)
                }
            } else {
                Text(row.displayTitle(apps: apps)).foregroundColor(.secondary)
            }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Done") {
                    if editableAction { row.action = "\(kind) \(target.trimmingCharacters(in: .whitespacesAndNewlines))" }
                    onSave(row)
                    dismiss()
                }.disabled(row.chord.trimmingCharacters(in: .whitespaces).isEmpty || (editableAction && target.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty))
            }
        }.padding(24).frame(width: 550)
        .onAppear {
            let parts = row.action.split(separator: " ", maxSplits: 1)
            kind = String(parts.first ?? "launch")
            target = parts.count > 1 ? String(parts[1]) : ""
        }
    }
}

let app = NSApplication.shared
let delegate = TylerApp()
app.setActivationPolicy(.accessory)
app.delegate = delegate
app.run()
