import Cocoa
import Foundation
import Darwin
import Carbon

var monitoredPid: pid_t = 0

func writeOutput(_ message: String) {
    FileHandle.standardOutput.write((message + "\n").data(using: .utf8)!)
    fflush(stdout)
}

func writeError(_ message: String) {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
}

func isEditableTextElement(_ element: AXUIElement) -> Bool {
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
            if isEditableTextElement(element) { return .pasteable }
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

// The app that receives keystrokes. A floating launcher such as Raycast holds
// keyboard focus while the app behind it still owns the menu bar, so the
// frontmost application is only a fallback when AX cannot name the owner.
// Reads the system-wide element directly: a messaging timeout set on it would
// become this process's global AX timeout.
func keyboardFocusPid() -> pid_t? {
    let systemWide: AXUIElement = AXUIElementCreateSystemWide()
    var focused: AnyObject?
    var pid: pid_t = 0
    if AXIsProcessTrusted(),
       AXUIElementCopyAttributeValue(systemWide, kAXFocusedApplicationAttribute as CFString, &focused) == .success,
       let application = pasteTargetElement(focused),
       AXUIElementGetPid(application, &pid) == .success, pid > 0 {
        return pid
    }
    return NSWorkspace.shared.frontmostApplication?.processIdentifier
}

// Electron apps such as Claude desktop embed Chromium and keep its always
// enabled Paste menu, so a page container with focus looks pasteable.
func isElectronApp(_ application: NSRunningApplication?) -> Bool {
    guard let bundleURL = application?.bundleURL else { return false }
    let framework = bundleURL.appendingPathComponent("Contents/Frameworks/Electron Framework.framework")
    return FileManager.default.fileExists(atPath: framework.path)
}

func pasteTargetStatus(for targetPid: pid_t) -> PasteTargetStatus {
    guard keyboardFocusPid() == targetPid else { return .notPasteable }
    let application = NSRunningApplication(processIdentifier: targetPid)
    let bundleIdentifier = application?.bundleIdentifier ?? ""
    // Include browser release channels such as Brave beta/nightly and Chrome
    // Canary. An unavailable AX tree must use manual copy instead of a menu guess.
    let isBrowser = [
        "com.apple.Safari", "com.apple.SafariTechnologyPreview", "com.brave.Browser",
        "com.google.Chrome", "org.chromium.Chromium", "com.microsoft.edgemac",
        "org.mozilla.firefox", "company.thebrowser.Browser"
    ].contains { bundleIdentifier == $0 || bundleIdentifier.hasPrefix($0 + ".") } ||
        isElectronApp(application)
    let requiresWritableTextField = isBrowser || bundleIdentifier == "com.apple.finder"
    guard AXIsProcessTrusted() else {
        return requiresWritableTextField ? .notPasteable : .unknown
    }

    let status = focusedPasteTargetStatus(
        AXUIElementCreateApplication(targetPid), requiresWritableTextField: requiresWritableTextField,
        isBrowser: isBrowser, bundleIdentifier: bundleIdentifier
    )
    guard keyboardFocusPid() == targetPid else { return .notPasteable }
    return status
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

// A read-only, multi-field AX server. Each identity is scoped to this helper
// process and follows the same AX element across application/focus switches.
final class ObservedInput {
    let id: String
    let element: AXUIElement
    var active = false
    var healthOnly = false
    var hadContent = false
    var lastValue: String?
    var lastComposing = false
    var compositionHint = false
    var unreadable = false
    var lastSendGesture: TimeInterval = 0
    var lastFocusedAt: TimeInterval = 0
    var expires = ProcessInfo.processInfo.systemUptime + 300
    init(_ id: String, _ element: AXUIElement) { self.id = id; self.element = element }
}

var observedInputs: [String: ObservedInput] = [:]
var preparedInputs: [Int: (String, Bool, TimeInterval)] = [:]
var observationApp: AXUIElement?
var observationAX: AXObserver?
var unboundHealthUntil: TimeInterval = 0
let maxObservedUTF16 = 256 * 1024

func observationOutput(_ value: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]),
          let line = String(data: data, encoding: .utf8) else { return }
    writeOutput(line)
}

func observationValue(_ element: AXUIElement) -> String? {
    if let value = pasteTargetAttribute(element, kAXValueAttribute) as? String {
        return value.utf16.count <= maxObservedUTF16 ? value : nil
    }
    // Web contenteditable can omit AXValue while providing the standard
    // parameterized text interface. Read the whole bounded field, never a
    // prefix that silently omits the dictated passage near the end.
    if let count = pasteTargetAttribute(element, kAXNumberOfCharactersAttribute) as? Int,
       count >= 0, count <= maxObservedUTF16 {
        var range = CFRange(location: 0, length: count)
        if let parameter = AXValueCreate(.cfRange, &range) {
            var result: AnyObject?
            if AXUIElementCopyParameterizedAttributeValue(
                element, kAXStringForRangeParameterizedAttribute as CFString, parameter, &result
            ) == .success, let value = result as? String { return value }
        }
    }
    // Chromium contenteditable trees sometimes expose only static text
    // descendants. Restrict this walk to the bound editor, with hard budgets.
    var pending: [(AXUIElement, Int)] = [(element, 0)]
    var chunks: [String] = []
    var count = 0
    var visited = 0
    let deadline = ProcessInfo.processInfo.systemUptime + 0.05
    while let (node, depth) = pending.popLast() {
        visited += 1
        guard visited <= 2000, ProcessInfo.processInfo.systemUptime < deadline else { return nil }
        if pasteTargetAttribute(node, kAXRoleAttribute) as? String == "AXStaticText",
           let value = pasteTargetAttribute(node, kAXValueAttribute) as? String {
            count += value.utf16.count
            guard count <= maxObservedUTF16 else { return nil }
            chunks.append(value)
            continue
        }
        if depth < 16, let children = pasteTargetAttribute(node, kAXChildrenAttribute) as? [AXUIElement] {
            pending.append(contentsOf: children.reversed().map { ($0, depth + 1) })
        }
    }
    // Missing data must not masquerade as an empty, readable field.
    return chunks.isEmpty ? nil : chunks.joined(separator: "\n")
}

func observationSelection(_ element: AXUIElement) -> CFRange? {
    guard let value = pasteTargetAttribute(element, kAXSelectedTextRangeAttribute),
          CFGetTypeID(value) == AXValueGetTypeID() else { return nil }
    var range = CFRange()
    return AXValueGetValue(value as! AXValue, .cfRange, &range) ? range : nil
}

func observationComposing(_ element: AXUIElement) -> Bool {
    // Native input methods expose a marked range. Chromium versions that
    // expose AXMarkedText use that string instead. Neither attribute is set.
    if let value = pasteTargetAttribute(element, "AXMarkedTextRange"),
       CFGetTypeID(value) == AXValueGetTypeID() {
        var range = CFRange()
        if AXValueGetValue(value as! AXValue, .cfRange, &range), range.location >= 0, range.length > 0 { return true }
    }
    return !(pasteTargetAttribute(element, "AXMarkedText") as? String ?? "").isEmpty
}

func observationIsFocused(_ input: ObservedInput) -> Bool {
    guard keyboardFocusPid() == monitoredPid, let app = observationApp,
          let focused = pasteTargetElement(pasteTargetAttribute(app, kAXFocusedUIElementAttribute)) else { return false }
    return CFEqual(focused, input.element)
}

func observationIsSendButton(_ element: AXUIElement) -> Bool {
    guard pasteTargetAttribute(element, kAXRoleAttribute) as? String == "AXButton" else { return false }
    let label = [kAXTitleAttribute, kAXDescriptionAttribute, kAXHelpAttribute]
        .compactMap { pasteTargetAttribute(element, $0) as? String }.joined(separator: " ").lowercased()
    return ["send", "submit", "发送", "提交"].contains(where: { label.contains($0) })
}

func observationSample(_ input: ObservedInput) {
    guard input.active else { return }
    let now = ProcessInfo.processInfo.systemUptime
    let focused = observationIsFocused(input)
    if focused { input.lastFocusedAt = now }
    else {
        // Accessibility activation of a Send button need not emit a mouse
        // event. A just-focused Send button can confirm a subsequent clear.
        if now - input.lastFocusedAt < 0.7, keyboardFocusPid() == monitoredPid,
           let app = observationApp,
           let button = pasteTargetElement(pasteTargetAttribute(app, kAXFocusedUIElementAttribute)),
           observationIsSendButton(button) {
            input.lastSendGesture = now
        }
        if now - input.lastSendGesture >= 0.7 { return }
    }
    guard let value = observationValue(input.element) else {
        if !input.unreadable {
            observationOutput(["kind": "unreadable", "field": input.id, "reason": "ax_text_unavailable_or_exceeds_256k_utf16"])
        }
        input.unreadable = true
        return
    }
    let composing = observationComposing(input.element) || input.compositionHint
    let empty = value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    if empty, input.hadContent, !input.healthOnly,
       ProcessInfo.processInfo.systemUptime - input.lastSendGesture < 0.7 {
        observationOutput(["kind": "sent", "field": input.id])
        input.active = false
        input.hadContent = false
        input.lastValue = nil
        return
    }
    // A Send action may briefly move focus away. Only its clear is relevant;
    // normal edits in an unfocused field remain paused.
    guard focused else { return }
    guard input.unreadable || value != input.lastValue || composing != input.lastComposing else { return }
    input.unreadable = false
    observationOutput(["kind": "value", "field": input.id, "value": value, "composing": composing])
    input.lastValue = value
    if !empty { input.hadContent = true }
    input.lastComposing = composing
}

func observationCallback(_ observer: AXObserver, _ element: AXUIElement,
                         _ notification: CFString, _ refcon: UnsafeMutableRawPointer?) {
    if notification as String == kAXUIElementDestroyedNotification {
        for input in Array(observedInputs.values) where CFEqual(input.element, element) {
            observationOutput(["kind": "closed", "field": input.id])
            observedInputs.removeValue(forKey: input.id)
        }
        return
    }
    for input in observedInputs.values where CFEqual(input.element, element) { observationSample(input) }
}

func observationCommand(_ command: [String: Any]) {
    let name = command["command"] as? String ?? ""
    let ticket = command["ticket"] as? Int ?? 0
    switch name {
    case "prepare":
        guard let app = observationApp,
              keyboardFocusPid() == monitoredPid,
              let element = resolveFocusedElement(app, pid: monitoredPid, maxRetries: 1) else {
            observationOutput(["kind": "prepared", "ticket": ticket, "reason": "ax_focused_element_unavailable"])
            return
        }
        let input = observedInputs.values.first { CFEqual($0.element, element) } ??
            ObservedInput(UUID().uuidString, element)
        observedInputs[input.id] = input
        preparedInputs[ticket] = (input.id, input.active, input.expires)
        if let observer = observationAX {
            // Polling remains active if a web editor rejects notifications.
            _ = AXObserverAddNotification(observer, element, kAXValueChangedNotification as CFString, nil)
            _ = AXObserverAddNotification(observer, element, kAXSelectedTextChangedNotification as CFString, nil)
            _ = AXObserverAddNotification(observer, element, kAXUIElementDestroyedNotification as CFString, nil)
        }
        guard let value = observationValue(element),
              let selection = observationSelection(element) ?? (value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? CFRange(location: 0, length: 0) : nil) else {
            observationOutput(["kind": "prepared", "ticket": ticket, "field": input.id,
                               "reason": "ax_value_or_selection_unavailable"])
            return
        }
        observationSample(input)
        // Pause this field until commit so Command-V cannot be mistaken for
        // a correction to the preceding dictation while the ticket is open.
        input.active = false
        observationOutput(["kind": "prepared", "ticket": ticket, "field": input.id,
                           "value": value, "start": selection.location, "length": selection.length])
    case "commit", "cancel", "health":
        if let (id, wasActive, oldExpiry) = preparedInputs.removeValue(forKey: ticket), let input = observedInputs[id] {
            if name == "commit" {
                input.active = true
                input.healthOnly = false
                input.expires = ProcessInfo.processInfo.systemUptime + 300
            } else if name == "health" {
                input.active = true
                input.healthOnly = input.healthOnly || !wasActive
                input.expires = wasActive ? oldExpiry : ProcessInfo.processInfo.systemUptime + 300
            } else {
                input.active = wasActive
                input.expires = oldExpiry
                if !wasActive { observedInputs.removeValue(forKey: id) }
            }
            observationSample(input)
        } else if name == "health" {
            unboundHealthUntil = ProcessInfo.processInfo.systemUptime + 300
        }
    case "snapshot":
        for input in observedInputs.values { observationSample(input) }
        observationOutput(["kind": "synced", "ticket": ticket])
    case "forget":
        if let id = command["field"] as? String { observedInputs.removeValue(forKey: id) }
    default: break
    }
}

func runObservationServer(_ pid: pid_t) -> Never {
    monitoredPid = pid
    observationApp = AXUIElementCreateApplication(pid)
    var created: AXObserver?
    if AXObserverCreate(pid, observationCallback, &created) == .success, let created = created {
        observationAX = created
        CFRunLoopAddSource(CFRunLoopGetCurrent(), AXObserverGetRunLoopSource(created), .commonModes)
    }
    let events = NSEvent.addGlobalMonitorForEvents(matching: [.keyDown, .leftMouseDown]) { event in
        guard keyboardFocusPid() == pid else { return }
        let now = ProcessInfo.processInfo.systemUptime
        let inputSource = TISCopyCurrentKeyboardInputSource().takeRetainedValue()
        let asciiProperty = TISGetInputSourceProperty(inputSource, kTISPropertyInputSourceIsASCIICapable)
        let asciiCapable = asciiProperty.map { CFBooleanGetValue(Unmanaged<CFBoolean>.fromOpaque($0).takeUnretainedValue()) } ?? true
        var sendButtonClick = false
        if event.type == .leftMouseDown {
            var hit: AXUIElement?
            let point = event.cgEvent?.location ?? .zero
            if AXUIElementCopyElementAtPosition(AXUIElementCreateSystemWide(), Float(point.x), Float(point.y), &hit) == .success,
               let hit = hit {
                sendButtonClick = observationIsSendButton(hit)
            }
        }
        for input in observedInputs.values where input.active &&
            (observationIsFocused(input) || (sendButtonClick && now - input.lastFocusedAt < 0.3)) {
            let wasComposing = input.compositionHint || observationComposing(input.element)
            // When an input method omits AX marked-text attributes, retain a
            // conservative composing hint until its commit key or candidate
            // click. This avoids learning a paused Chinese preedit.
            if event.type == .leftMouseDown || [36, 76, 49, 53].contains(event.keyCode) {
                input.compositionHint = false
            } else if event.type == .keyDown && !asciiCapable &&
                        event.modifierFlags.intersection([.command, .control]).isEmpty &&
                        !(event.characters ?? "").isEmpty {
                input.compositionHint = true
            }
            // Return used to commit an input-method candidate is not Send.
            // A caret click is never a send hint, nor is Delete after a click.
            if event.type == .keyDown && [51, 117].contains(event.keyCode) {
                input.lastSendGesture = 0
            } else if event.type == .keyDown && [36, 76].contains(event.keyCode) && !wasComposing {
                input.lastSendGesture = now
            } else if sendButtonClick {
                input.lastSendGesture = now
            }
            observationSample(input)
        }
    }
    let timer = Timer(timeInterval: 0.1, repeats: true) { _ in
        let now = ProcessInfo.processInfo.systemUptime
        if now < unboundHealthUntil, keyboardFocusPid() == pid, let app = observationApp,
           let focused = pasteTargetElement(pasteTargetAttribute(app, kAXFocusedUIElementAttribute)),
           isEditableTextElement(focused) || isEditableBrowserPasteTarget(focused),
           let value = observationValue(focused), observationSelection(focused) != nil || value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            unboundHealthUntil = 0
            observationOutput(["kind": "health_recovered"])
        }
        for input in Array(observedInputs.values) {
            if now >= input.expires {
                observationOutput(["kind": "closed", "field": input.id])
                observedInputs.removeValue(forKey: input.id)
            } else { observationSample(input) }
        }
    }
    RunLoop.main.add(timer, forMode: .common)
    // Read complete newline-delimited commands off the AX run loop.
    DispatchQueue.global(qos: .utility).async {
        while let line = readLine() {
            guard let data = line.data(using: .utf8),
                  let command = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { continue }
            DispatchQueue.main.async { observationCommand(command) }
        }
        DispatchQueue.main.async { CFRunLoopStop(CFRunLoopGetCurrent()) }
    }
    CFRunLoopRun()
    if let events = events { NSEvent.removeMonitor(events) }
    exit(0)
}


// Usage: macos-text-monitor --observe <pid>
//        macos-text-monitor --paste-target <pid>
//        macos-text-monitor --focused-app
let pasteTargetMode = CommandLine.arguments.count >= 2 &&
    CommandLine.arguments[1] == "--paste-target"
// System-wide AX queries fail with kAXErrorCannotComplete until the process
// connects to the window server, which would silently reduce keyboardFocusPid()
// to the frontmost app. Connect before any mode can query keyboard focus.
_ = NSApplication.shared

// Names the Target app at hotkey press time. Takes no PID: it reports which
// app holds keyboard focus, so it runs before the PID argument is parsed.
if CommandLine.arguments.count >= 2 && CommandLine.arguments[1] == "--focused-app" {
    guard let pid = keyboardFocusPid() else {
        writeOutput("UNKNOWN")
        exit(1)
    }
    writeOutput("FOCUSED_PID:\(pid)")
    exit(0)
}

if CommandLine.arguments.count >= 3, CommandLine.arguments[1] == "--observe",
   let pid = Int32(CommandLine.arguments[2]), pid > 0 {
    runObservationServer(pid)
}

if pasteTargetMode, CommandLine.arguments.count >= 3,
   let pid = Int32(CommandLine.arguments[2]), pid > 0 {
    writeOutput(pasteTargetStatus(for: pid).rawValue)
    exit(0)
}
writeError("Usage: macos-text-monitor --observe <pid> | --paste-target <pid> | --focused-app")
writeOutput("UNKNOWN")
exit(1)
