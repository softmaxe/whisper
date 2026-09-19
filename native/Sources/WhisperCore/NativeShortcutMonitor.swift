import AppKit
import Carbon.HIToolbox

/// A session tap observes real key-up and side-specific physical state without claiming Command chords.
@MainActor public final class NativeShortcutMonitor {
    private weak var application: WhisperApplication?
    private var tap: CFMachPort?
    private var source: CFRunLoopSource?
    public init(application: WhisperApplication) { self.application = application }

    @discardableResult public func start(requestPermission: Bool = false) -> Bool {
        if tap != nil { return true }
        let options = ["AXTrustedCheckOptionPrompt": requestPermission] as CFDictionary
        guard AXIsProcessTrustedWithOptions(options) else {
            application?.send(.setShortcutAvailable(false)); return false
        }
        let mask = [CGEventType.keyDown, .keyUp, .flagsChanged].reduce(CGEventMask(0)) { $0 | (1 << $1.rawValue) }
        guard let port = CGEvent.tapCreate(tap: .cgSessionEventTap, place: .headInsertEventTap,
            options: .defaultTap, eventsOfInterest: mask, callback: { _, type, event, info in
                guard let info else { return Unmanaged.passUnretained(event) }
                let consume = MainActor.assumeIsolated {
                    let monitor = Unmanaged<NativeShortcutMonitor>.fromOpaque(info).takeUnretainedValue()
                    return monitor.receive(type, event: event)
                }
                return consume ? nil : Unmanaged.passUnretained(event)
            }, userInfo: Unmanaged.passUnretained(self).toOpaque()) else {
            application?.send(.setShortcutAvailable(false)); return false
        }
        tap = port
        application?.send(.resetShortcutInput(Self.physicalKeys()))
        source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, port, 0)
        CFRunLoopAddSource(CFRunLoopGetMain(), source, .commonModes)
        CGEvent.tapEnable(tap: port, enable: true)
        application?.send(.setShortcutAvailable(true))
        return true
    }

    public func stop() {
        if let tap { CGEvent.tapEnable(tap: tap, enable: false); CFMachPortInvalidate(tap) }
        if let source { CFRunLoopRemoveSource(CFRunLoopGetMain(), source, .commonModes) }
        tap = nil
        source = nil
        application?.send(.resetShortcutInput([]))
        application?.send(.setShortcutAvailable(false))
    }

    isolated deinit {
        if let tap { CFMachPortInvalidate(tap) }
        if let source { CFRunLoopRemoveSource(CFRunLoopGetMain(), source, .commonModes) }
    }

    private func receive(_ type: CGEventType, event: CGEvent) -> Bool {
        if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
            application?.send(.resetShortcutInput(Self.physicalKeys()))
            if let tap { CGEvent.tapEnable(tap: tap, enable: true) }
            return false
        }
        guard event.getIntegerValueField(.eventSourceUserData) != whisperPasteEventTag else { return false }
        let code = UInt16(event.getIntegerValueField(.keyboardEventKeycode))
        let down = type == .flagsChanged ? CGEventSource.keyState(.hidSystemState, key: code) : type == .keyDown
        let consume = code == ShortcutInput.escape && down && application?.state.dictation.phase.isActive == true
        let input = ShortcutInput(keyCode: code, isDown: down,
            isRepeat: event.getIntegerValueField(.keyboardEventAutorepeat) != 0,
            heldModifiers: Set(ShortcutInput.modifierKeyCodes.filter { CGEventSource.keyState(.hidSystemState, key: $0) }))
        if consume {
            // Cancellation only invalidates ownership; do it before an already queued server reply.
            application?.send(.shortcut(input))
            return true
        }
        // Keychain/device setup may block; never perform it inside the event-tap callback.
        DispatchQueue.main.async { [weak application] in application?.send(.shortcut(input)) }
        return consume
    }
    private static func physicalKeys() -> Set<UInt16> {
        Set((UInt16(0)..<128).filter { CGEventSource.keyState(.hidSystemState, key: $0) })
    }
}
