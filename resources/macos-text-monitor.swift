import Cocoa
import Foundation
import Darwin

let TIMEOUT_SECONDS: Double = 30.0
let MAX_OUTPUT_BYTES = 10240
let MAX_BASE64_SOURCE_CHARS = 7000

var monitoredElement: AXUIElement?
var observer: AXObserver?
var monitoredPid: pid_t = 0

func writeOutput(_ message: String) {
    FileHandle.standardOutput.write((message + "\n").data(using: .utf8)!)
    fflush(stdout)
}

func writeError(_ message: String) {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
}

func writeTextOutput(_ prefix: String, _ value: String) {
    let truncated = String(value.prefix(MAX_OUTPUT_BYTES))
    if truncated.contains("\n") || truncated.contains("\r") {
        let capped = String(truncated.prefix(MAX_BASE64_SOURCE_CHARS))
        let encoded = Data(capped.utf8).base64EncodedString()
        writeOutput("\(prefix)_B64:\(encoded)")
        return
    }

    writeOutput("\(prefix):\(truncated)")
}

func isEditableTextElement(_ element: AXUIElement, allowSelection: Bool = false) -> Bool {
    // A live selection means an editable verdict would let generated text
    // paste over the user's highlighted text (--editable-target mode probes
    // an element whose selection state the caller could not read itself).
    var selectedTextValue: AnyObject?
    if !allowSelection, AXUIElementCopyAttributeValue(
        element,
        kAXSelectedTextAttribute as CFString,
        &selectedTextValue
    ) == .success, let selectedText = selectedTextValue as? String, !selectedText.isEmpty {
        return false
    }

    var enabledValue: AnyObject?
    if AXUIElementCopyAttributeValue(
        element,
        kAXEnabledAttribute as CFString,
        &enabledValue
    ) == .success, let enabled = enabledValue as? Bool, !enabled {
        return false
    }

    var subroleValue: AnyObject?
    if AXUIElementCopyAttributeValue(
        element,
        kAXSubroleAttribute as CFString,
        &subroleValue
    ) == .success, subroleValue as? String == "AXSecureTextField" {
        return false
    }

    var roleValue: AnyObject?
    guard AXUIElementCopyAttributeValue(
        element,
        kAXRoleAttribute as CFString,
        &roleValue
    ) == .success, let role = roleValue as? String,
          ["AXTextField", "AXTextArea", "AXComboBox"].contains(role)
    else {
        return false
    }

    var settable = DarwinBoolean(false)
    if AXUIElementIsAttributeSettable(
        element,
        kAXValueAttribute as CFString,
        &settable
    ) == .success, settable.boolValue {
        return true
    }

    settable = DarwinBoolean(false)
    return AXUIElementIsAttributeSettable(
        element,
        kAXSelectedTextAttribute as CFString,
        &settable
    ) == .success && settable.boolValue
}

enum PasteTargetStatus: String {
    case pasteable = "PASTEABLE"
    case notPasteable = "NOT_PASTEABLE"
    case unknown = "UNKNOWN"
}

func pasteTargetAttribute(_ element: AXUIElement, _ attribute: String) -> AnyObject? {
    AXUIElementSetMessagingTimeout(element, 0.05)
    var value: AnyObject?
    guard AXUIElementCopyAttributeValue(element, attribute as CFString, &value) == .success else {
        return nil
    }
    return value
}

func pasteTargetElement(_ value: AnyObject?) -> AXUIElement? {
    guard let value = value, CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
    return (value as! AXUIElement)
}

// Some custom editors and terminals expose no writable AX text field. Use
// their plain Command-V menu only when the focused target permits this fallback.
func pasteMenuStatus(_ appElement: AXUIElement, deadline: TimeInterval) -> PasteTargetStatus {
    guard ProcessInfo.processInfo.systemUptime < deadline else { return .unknown }
    guard let menuBar = pasteTargetElement(pasteTargetAttribute(appElement, kAXMenuBarAttribute)) else {
        return .unknown
    }

    var pending: [(AXUIElement, Int)] = [(menuBar, 0)]
    var visited = 0
    while let (element, depth) = pending.popLast() {
        guard visited < 200, ProcessInfo.processInfo.systemUptime < deadline else { return .unknown }
        visited += 1

        if let command = pasteTargetAttribute(element, kAXMenuItemCmdCharAttribute) as? String,
           command.lowercased() == "v" {
            // Command is implicit. Shift, Option, Control and NoCommand are
            // explicit bits; none belong to the shortcut we actually send.
            if let modifiers = pasteTargetAttribute(element, kAXMenuItemCmdModifiersAttribute) as? Int,
               modifiers == 0,
               let enabled = pasteTargetAttribute(element, kAXEnabledAttribute) as? Bool {
                return enabled ? .pasteable : .notPasteable
            }
        }

        if depth < 5,
           let children = pasteTargetAttribute(element, kAXChildrenAttribute) as? [AXUIElement] {
            pending.append(contentsOf: children.prefix(200 - visited).reversed().map { ($0, depth + 1) })
        }
    }
    return .unknown
}

// Contenteditable editors can expose AXGroup rather than a standard text role.
// Require writable text, not merely a settable selection range: Chromium also
// allows selection-range changes in read-only inputs.
func isEditableBrowserPasteTarget(_ element: AXUIElement) -> Bool {
    guard let role = pasteTargetAttribute(element, kAXRoleAttribute) as? String,
          ["AXTextField", "AXTextArea", "AXComboBox", "AXGroup"].contains(role) else {
        return false
    }
    if pasteTargetAttribute(element, "AXEditable") as? Bool == true { return true }
    for attribute in [kAXValueAttribute, kAXSelectedTextAttribute] {
        var settable = DarwinBoolean(false)
        if AXUIElementIsAttributeSettable(element, attribute as CFString, &settable) == .success,
           settable.boolValue {
            return true
        }
    }
    return false
}

// Reading the application role enables Chromium's native AX support. On newer
// macOS versions, reading the web container's role also enables web AX support.
// Descend only the focused window, with a node/depth/time budget, and never
// enable AXEnhancedUserInterface or change the user's focused element.
func prepareBrowserAccessibility(_ appElement: AXUIElement, deadline: TimeInterval) {
    guard ProcessInfo.processInfo.systemUptime < deadline,
          let window = pasteTargetElement(pasteTargetAttribute(appElement, kAXFocusedWindowAttribute)) else {
        return
    }
    var pending: [(AXUIElement, Int)] = [(window, 0)]
    var index = 0
    while index < pending.count, index < 100, ProcessInfo.processInfo.systemUptime < deadline {
        let (element, depth) = pending[index]
        index += 1
        let role = pasteTargetAttribute(element, kAXRoleAttribute) as? String
        // Do not walk page content or menus; only the native window containers
        // are needed to activate Chromium's on-demand accessibility support.
        if role == "AXWebArea" { return }
        if depth < 8, ["AXWindow", "AXGroup", "AXSplitGroup", "AXScrollArea"].contains(role ?? ""),
           let children = pasteTargetAttribute(element, kAXChildrenAttribute) as? [AXUIElement] {
            pending.append(contentsOf: children.prefix(100 - pending.count).map { ($0, depth + 1) })
        }
    }
}

func focusedPasteTargetStatus(
    _ appElement: AXUIElement, requiresWritableTextField: Bool, isBrowser: Bool,
    bundleIdentifier: String
) -> PasteTargetStatus {
    let deadline = ProcessInfo.processInfo.systemUptime + 0.35
    if isBrowser { _ = pasteTargetAttribute(appElement, kAXRoleAttribute) }
    var element = pasteTargetElement(pasteTargetAttribute(appElement, kAXFocusedUIElementAttribute))
    // Browser AX trees may wake after the first read. Retry only missing or
    // container focus, without writing AX attributes or trusting the Paste menu.
    if isBrowser {
        for attempt in 0..<2 {
            let role = element.flatMap { pasteTargetAttribute($0, kAXRoleAttribute) as? String }
            if let role = role, !["AXApplication", "AXWindow", "AXWebArea"].contains(role) { break }
            guard ProcessInfo.processInfo.systemUptime < deadline else { break }
            if attempt == 0 {
                prepareBrowserAccessibility(appElement, deadline: deadline - 0.1)
            }
            Thread.sleep(forTimeInterval: 0.025)
            element = pasteTargetElement(pasteTargetAttribute(appElement, kAXFocusedUIElementAttribute))
        }
    }
    var explicitlyReadOnly = false
    if let element = element {
        if pasteTargetAttribute(element, kAXEnabledAttribute) as? Bool == false ||
            pasteTargetAttribute(element, kAXSubroleAttribute) as? String == "AXSecureTextField" {
            return .notPasteable
        }
        explicitlyReadOnly = pasteTargetAttribute(element, "AXEditable") as? Bool == false
        if !explicitlyReadOnly {
            if isEditableTextElement(element, allowSelection: true) { return .pasteable }
            if isBrowser, isEditableBrowserPasteTarget(element) { return .pasteable }
        }
    }

    // Browser menus can enable Paste while a page container has focus. Finder
    // can create a .textClipping file. Both need a confirmed writable text field.
    if requiresWritableTextField { return .notPasteable }

    if let element = element,
       let role = pasteTargetAttribute(element, kAXRoleAttribute) as? String,
       ["AXButton", "AXCheckBox", "AXRadioButton", "AXSlider", "AXStaticText", "AXImage", "AXLink", "AXWebArea"].contains(role) {
        return .notPasteable
    }

    let menuStatus = pasteMenuStatus(appElement, deadline: deadline)
    if menuStatus != .unknown { return menuStatus }
    if explicitlyReadOnly { return .notPasteable }

    // Ghostty's focused terminal accepts keyboard input, but its AX text
    // buffer is not settable and its menu may omit the Command-V shortcut.
    if ["com.mitchellh.ghostty", "com.mitchellh.ghostty.debug"].contains(bundleIdentifier),
       let element = element,
       pasteTargetAttribute(element, kAXRoleAttribute) as? String == "AXTextArea" {
        return .pasteable
    }
    return .unknown
}

func pasteTargetStatus(for targetPid: pid_t) -> PasteTargetStatus {
    guard let application = NSWorkspace.shared.frontmostApplication,
          application.processIdentifier == targetPid else {
        return .notPasteable
    }
    let bundleIdentifier = application.bundleIdentifier ?? ""
    // Include browser release channels such as Brave beta/nightly and Chrome
    // Canary. An unavailable AX tree must use manual copy instead of a menu guess.
    let isBrowser = [
        "com.apple.Safari", "com.apple.SafariTechnologyPreview", "com.brave.Browser",
        "com.google.Chrome", "org.chromium.Chromium", "com.microsoft.edgemac",
        "org.mozilla.firefox", "company.thebrowser.Browser"
    ].contains { bundleIdentifier == $0 || bundleIdentifier.hasPrefix($0 + ".") }
    let requiresWritableTextField = isBrowser || bundleIdentifier == "com.apple.finder"
    guard AXIsProcessTrusted() else {
        return requiresWritableTextField ? .notPasteable : .unknown
    }

    let status = focusedPasteTargetStatus(
        AXUIElementCreateApplication(targetPid), requiresWritableTextField: requiresWritableTextField,
        isBrowser: isBrowser, bundleIdentifier: bundleIdentifier
    )
    guard NSWorkspace.shared.frontmostApplication?.processIdentifier == targetPid else {
        return .notPasteable
    }
    return status
}

func readCurrentValue() -> String? {
    guard let element = monitoredElement else { return nil }
    var value: AnyObject?
    let result = AXUIElementCopyAttributeValue(element, kAXValueAttribute as CFString, &value)
    guard result == .success, let str = value as? String else { return nil }
    return str
}

func resolveFocusedElement(_ appElement: AXUIElement, pid: pid_t, maxRetries: Int = 5) -> AXUIElement? {
    for attempt in 1...maxRetries {
        var elementValue: AnyObject?
        let elementResult = AXUIElementCopyAttributeValue(
            appElement,
            kAXFocusedUIElementAttribute as CFString,
            &elementValue
        )

        if elementResult == .success, let element = elementValue {
            if attempt > 1 {
                writeError("Got focused element on attempt \(attempt)")
            }
            return (element as! AXUIElement)
        }
        writeError("Attempt \(attempt)/\(maxRetries): Cannot get focused element for PID \(pid) (error: \(elementResult.rawValue))")

        // Chromium and Electron apps (Claude, Codex) keep their AX tree dormant
        // until a client reads it, answering -25212 for the focused element
        // indefinitely. Wake it once with the same read-only walk the paste
        // probe uses; native apps resolve on the first attempt and skip this.
        if attempt == 1 {
            _ = pasteTargetAttribute(appElement, kAXRoleAttribute)
            prepareBrowserAccessibility(appElement, deadline: ProcessInfo.processInfo.systemUptime + 0.3)
            // The walk shortens the messaging timeout; restore the default.
            AXUIElementSetMessagingTimeout(appElement, 0)
        }

        if attempt < maxRetries {
            Thread.sleep(forTimeInterval: 0.3)
        }
    }
    return nil
}

func observerCallback(
    _ observer: AXObserver,
    _ element: AXUIElement,
    _ notification: CFString,
    _ refcon: UnsafeMutableRawPointer?
) {
    if let value = readCurrentValue() {
        writeTextOutput("CHANGED", value)
    }
}

// Usage: macos-text-monitor <pid>
//        macos-text-monitor --selected-text <pid>
//        macos-text-monitor --editable-target <pid>
//        macos-text-monitor --paste-target <pid>
//        macos-text-monitor --window-bounds <pid>
let selectionReadMode = CommandLine.arguments.count >= 3 &&
    CommandLine.arguments[1] == "--selected-text"
let editableTargetMode = CommandLine.arguments.count >= 3 &&
    CommandLine.arguments[1] == "--editable-target"
let pasteTargetMode = CommandLine.arguments.count >= 2 &&
    CommandLine.arguments[1] == "--paste-target"
let windowBoundsMode = CommandLine.arguments.count >= 3 &&
    CommandLine.arguments[1] == "--window-bounds"
let pidArgumentIndex = selectionReadMode || editableTargetMode || pasteTargetMode || windowBoundsMode ? 2 : 1

guard CommandLine.arguments.count > pidArgumentIndex,
      let targetPid = Int32(CommandLine.arguments[pidArgumentIndex]),
      targetPid > 0 else {
    writeError("Usage: macos-text-monitor [--selected-text|--editable-target|--paste-target|--window-bounds] <pid>")
    writeOutput(pasteTargetMode ? "UNKNOWN" : "NO_ELEMENT")
    exit(1)
}

monitoredPid = targetPid

// Dictation probing reads no stdin and does not use the monitor's retry ladder.
if pasteTargetMode {
    writeOutput(pasteTargetStatus(for: targetPid).rawValue)
    exit(0)
}

// Reports the target's window rect so the caller can tell which display the user
// is working on. Asks the window server rather than accessibility on purpose:
// AXFocusedWindow is unavailable in exactly the apps whose accessibility tree
// stays dormant (Chromium), and window bounds need no Screen Recording
// permission — only window *titles* do.
if windowBoundsMode {
    let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
    let windows = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] ?? []
    // Largest window rather than frontmost: layer 0 still includes the full-width
    // toolbar strips Arc-family browsers float above their content, and one of
    // those spanning another display would name the wrong screen.
    var best: (x: Int, y: Int, width: Int, height: Int, area: Double)?
    for window in windows {
        guard let ownerPid = window[kCGWindowOwnerPID as String] as? pid_t, ownerPid == targetPid,
              let layer = window[kCGWindowLayer as String] as? Int, layer == 0,
              let bounds = window[kCGWindowBounds as String] as? [String: Any],
              let x = bounds["X"] as? Double, let y = bounds["Y"] as? Double,
              let width = bounds["Width"] as? Double, let height = bounds["Height"] as? Double
        else { continue }
        let area = width * height
        if area > (best?.area ?? 0) {
            best = (Int(x), Int(y), Int(width), Int(height), area)
        }
    }

    guard let window = best else {
        writeOutput("NO_WINDOW")
        exit(1)
    }
    writeOutput("BOUNDS:\(window.x),\(window.y),\(window.width),\(window.height)")
    exit(0)
}

// Read original text from stdin (monitoring mode only). Selection reads are
// spawned via execFile, which keeps stdin open without writing — blocking on
// readLine there would hang until the caller's timeout kills the process.
var originalText = ""
if !selectionReadMode && !editableTargetMode, let line = readLine(strippingNewline: true) {
    originalText = line
}

// Target the specific application by PID (passed from the Electron host
// which captures it BEFORE the overlay steals focus).
let appElement = AXUIElementCreateApplication(monitoredPid)
guard let resolvedElement = resolveFocusedElement(appElement, pid: monitoredPid) else {
    writeOutput("NO_ELEMENT")
    exit(1)
}

if editableTargetMode {
    writeOutput(isEditableTextElement(resolvedElement) ? "EDITABLE" : "NOT_EDITABLE")
    exit(0)
}

// Selection capture is a short-lived, read-only mode used before an AI edit.
// It deliberately uses the same AX client as the monitor so a packaged app's
// trusted native helper does not need to rely on System Events automation.
if selectionReadMode {
    var selectionValue: AnyObject?
    let selectionResult = AXUIElementCopyAttributeValue(
        resolvedElement,
        kAXSelectedTextAttribute as CFString,
        &selectionValue
    )

    if selectionResult == .success, let selection = selectionValue as? String {
        if selection.isEmpty {
            writeOutput(isEditableTextElement(resolvedElement) ? "EDITABLE_NONE:" : "NONE:")
        } else {
            writeTextOutput("SELECTED", selection)
        }
        exit(0)
    }

    if selectionResult == .noValue || selectionResult == .attributeUnsupported {
        writeOutput("NONE:")
        exit(0)
    }

    writeError("Cannot read selected text for PID \(monitoredPid) (error: \(selectionResult.rawValue))")
    writeOutput("UNAVAILABLE:")
    exit(1)
}

monitoredElement = resolvedElement
writeError("Monitoring element in PID \(monitoredPid)")

// Read initial value
guard let initialValue = readCurrentValue() else {
    writeError("Focused element has no text value")
    writeOutput("NO_VALUE")
    exit(0)
}

writeTextOutput("INITIAL_VALUE", initialValue)

// Create AXObserver for the target application's PID
var createdObserver: AXObserver?
let observerResult = AXObserverCreate(monitoredPid, observerCallback, &createdObserver)

guard observerResult == .success, let obs = createdObserver else {
    writeError("Failed to create AXObserver (error: \(observerResult.rawValue))")
    exit(1)
}

observer = obs

// Watch for value changes
let addResult = AXObserverAddNotification(
    obs,
    monitoredElement!,
    kAXValueChangedNotification as CFString,
    nil
)

if addResult != .success {
    writeError("Failed to add notification (error: \(addResult.rawValue))")
    exit(1)
}

// Add observer to run loop
CFRunLoopAddSource(
    CFRunLoopGetCurrent(),
    AXObserverGetRunLoopSource(obs),
    .commonModes
)

// Schedule auto-exit after timeout
DispatchQueue.main.asyncAfter(deadline: .now() + TIMEOUT_SECONDS) {
    CFRunLoopStop(CFRunLoopGetCurrent())
}

// Handle SIGTERM for clean exit
let signalSource = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
signal(SIGTERM, SIG_IGN)
signalSource.setEventHandler {
    CFRunLoopStop(CFRunLoopGetCurrent())
}
signalSource.resume()

CFRunLoopRun()
