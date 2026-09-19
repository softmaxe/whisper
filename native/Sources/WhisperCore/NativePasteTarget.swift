import Cocoa
// Adapted from resources/macos-text-monitor.swift; keep target safety rules aligned.
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
