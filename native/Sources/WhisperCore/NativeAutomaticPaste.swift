import AppKit
import Carbon.HIToolbox

// The input adapter ignores our synthetic shortcut, never the user's physical keys.
let whisperPasteEventTag: Int64 = 0x57535052

@MainActor public final class NativeAutomaticPasteSystem: AutomaticPasteSystem {
    public init() {}
    public func captureTarget() -> PasteTarget? {
        guard let app = NSWorkspace.shared.frontmostApplication,
              app.processIdentifier != ProcessInfo.processInfo.processIdentifier else { return nil }
        return PasteTarget(processID: app.processIdentifier)
    }
    public var modifiersHeld: Bool {
        [kVK_Command, kVK_RightCommand, kVK_Shift, kVK_RightShift,
         kVK_Option, kVK_RightOption, kVK_Control, kVK_RightControl, kVK_Function].contains {
            CGEventSource.keyState(.hidSystemState, key: CGKeyCode($0))
        }
    }
    public func snapshotClipboard() -> ClipboardSnapshot {
        ClipboardSnapshot(items: (NSPasteboard.general.pasteboardItems ?? []).map { item in
            item.types.reduce(into: [String: Data]()) { values, type in
                if let data = item.data(forType: type) { values[type.rawValue] = data }
            }
        })
    }
    public func replaceClipboard(with text: String) -> Int? {
        let board = NSPasteboard.general
        let original = snapshotClipboard()
        board.clearContents()
        guard board.setString(text, forType: .string) else {
            restoreClipboard(original, ownedRevision: board.changeCount)
            return nil
        }
        return board.changeCount
    }
    public func restoreClipboard(_ snapshot: ClipboardSnapshot, ownedRevision: Int) {
        let board = NSPasteboard.general
        guard board.changeCount == ownedRevision else { return }
        let items = snapshot.items.map { values in
            let item = NSPasteboardItem()
            for (type, data) in values { item.setData(data, forType: NSPasteboard.PasteboardType(type)) }
            return item
        }
        board.clearContents()
        if !items.isEmpty { board.writeObjects(items) }
    }
    public func activate(_ target: PasteTarget) async -> Bool {
        if NSWorkspace.shared.frontmostApplication?.processIdentifier == target.processID { return true }
        guard let app = NSRunningApplication(processIdentifier: target.processID), app.activate() else { return false }
        for _ in 0..<6 {
            try? await Task.sleep(for: .milliseconds(25))
            if NSWorkspace.shared.frontmostApplication?.processIdentifier == target.processID { return true }
        }
        return false
    }
    public func canPaste(_ target: PasteTarget) async -> Bool {
        guard AXIsProcessTrusted(),
              let app = NSWorkspace.shared.frontmostApplication, app.processIdentifier == target.processID else { return false }
        let bundle = app.bundleIdentifier ?? ""
        let isBrowser = ["com.apple.Safari", "com.apple.SafariTechnologyPreview", "com.brave.Browser",
                         "com.google.Chrome", "org.chromium.Chromium", "com.microsoft.edgemac",
                         "org.mozilla.firefox", "company.thebrowser.Browser"].contains {
            bundle == $0 || bundle.hasPrefix($0 + ".")
        }
        let verdict = await Task.detached {
            focusedPasteTargetStatus(AXUIElementCreateApplication(target.processID),
                requiresWritableTextField: isBrowser || bundle == "com.apple.finder",
                isBrowser: isBrowser, bundleIdentifier: bundle) == .pasteable
        }.value
        return verdict && NSWorkspace.shared.frontmostApplication?.processIdentifier == target.processID
    }
    public func paste(_ target: PasteTarget) async -> Bool {
        await paste(target, diagnostics: nil)
    }
    public func paste(_ target: PasteTarget, diagnostics: RequestDiagnostics?) async -> Bool {
        guard AXIsProcessTrusted(), !modifiersHeld,
              NSWorkspace.shared.frontmostApplication?.processIdentifier == target.processID,
              let key = Self.lookupVirtualKey(for: "v"),
              let down = CGEvent(keyboardEventSource: nil, virtualKey: key, keyDown: true),
              let up = CGEvent(keyboardEventSource: nil, virtualKey: key, keyDown: false) else { return false }
        for event in [down, up] {
            event.flags = .maskCommand
            event.setIntegerValueField(.eventSourceUserData, value: whisperPasteEventTag)
        }
        diagnostics?.mark(.pasteDispatched)
        down.post(tap: .cgSessionEventTap)
        // Always balance our own key, even if the task was cancelled after key-down.
        try? await Task.sleep(for: .milliseconds(8))
        up.post(tap: .cgSessionEventTap)
        diagnostics?.mark(.pasteSettled)
        return true
    }

    // Adapted from resources/macos-fast-paste.swift. Never use a fixed US-ANSI V key.
    private static func layoutData(_ source: TISInputSource?) -> Data? {
        guard let source, let pointer = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) else { return nil }
        return Unmanaged<CFData>.fromOpaque(pointer).takeUnretainedValue() as Data
    }
    private static func lookupVirtualKey(for character: String) -> CGKeyCode? {
        guard let data = layoutData(TISCopyCurrentKeyboardLayoutInputSource()?.takeRetainedValue())
            ?? layoutData(TISCopyCurrentASCIICapableKeyboardLayoutInputSource()?.takeRetainedValue()) else { return nil }
        return data.withUnsafeBytes { bytes in
            guard let address = bytes.baseAddress else { return nil }
            let layout = address.assumingMemoryBound(to: UCKeyboardLayout.self)
            for code in 0..<128 {
                var dead: UInt32 = 0
                var length = 0
                var characters = [UniChar](repeating: 0, count: 4)
                let status = UCKeyTranslate(layout, UInt16(code), UInt16(kUCKeyActionDisplay), UInt32(cmdKey) >> 8,
                    UInt32(LMGetKbdType()), OptionBits(kUCKeyTranslateNoDeadKeysBit), &dead,
                    characters.count, &length, &characters)
                if status == noErr, String(utf16CodeUnits: characters, count: length) == character { return CGKeyCode(code) }
            }
            return nil
        }
    }
}
