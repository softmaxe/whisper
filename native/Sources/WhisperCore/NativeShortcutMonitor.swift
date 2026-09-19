import AppKit
import Carbon.HIToolbox

/// A session tap observes real key-up and side-specific physical state without claiming Command chords.
@MainActor public final class NativeShortcutMonitor {
    private weak var application: WhisperApplication?
    private var tap: CFMachPort?
    private var source: CFRunLoopSource?
    private let globe: GlobePreferenceController
    private var suppression = ShortcutEventSuppression()
    private var physicalKeys = Set<UInt16>()
    private var quitObserver: NSObjectProtocol?
    private var signalSources: [DispatchSourceSignal] = []
    private var signalRestorers: [() -> Void] = []
    public init(application: WhisperApplication) {
        self.application = application
        globe = GlobePreferenceController(markerURL: application.profileStore.profile.directory.appendingPathComponent("globe-preference.json"),
            system: NativeGlobePreferenceSystem())
    }

    @discardableResult public func start(requestPermission: Bool = false) -> Bool {
        if tap != nil {
            if AXIsProcessTrusted() { updateConfiguration(); return true }
            stop()
        }
        application?.send(.setShortcutWarning(globe.setOwned(false) ? nil : .globeRestoreUnavailable))
        let options = ["AXTrustedCheckOptionPrompt": requestPermission] as CFDictionary
        guard AXIsProcessTrustedWithOptions(options) else {
            application?.send(.setShortcutAvailable(false)); return false
        }
        let mask = [CGEventType.keyDown, .keyUp, .flagsChanged, .otherMouseDown, .otherMouseUp]
            .reduce(CGEventMask(1 << 14)) { $0 | (1 << $1.rawValue) }
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
        physicalKeys = Self.readPhysicalKeys()
        application?.send(.resetShortcutInput(physicalKeys))
        source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, port, 0)
        CFRunLoopAddSource(CFRunLoopGetMain(), source, .commonModes)
        CGEvent.tapEnable(tap: port, enable: true)
        application?.send(.setShortcutAvailable(true))
        updateConfiguration()
        quitObserver = NotificationCenter.default.addObserver(forName: NSApplication.willTerminateNotification,
            object: nil, queue: .main) { [weak self] _ in MainActor.assumeIsolated { self?.stop() } }
        for number in [SIGTERM, SIGINT] {
            let previous = signal(number, SIG_IGN)
            signalRestorers.append { signal(number, previous) }
            let source = DispatchSource.makeSignalSource(signal: number, queue: .main)
            source.setEventHandler { [weak self] in
                MainActor.assumeIsolated {
                    self?.stop()
                    NSApplication.shared.terminate(nil)
                }
            }
            source.resume()
            signalSources.append(source)
        }
        return true
    }

    public func updateConfiguration() {
        guard tap != nil, let application else { return }
        let needsGlobe = application.state.shortcutCapture.isActive || application.state.settings.shortcuts.contains("GLOBE")
            || (application.state.dictation.phase.isActive && application.state.dictation.origin != .button && application.activeShortcut?.isGlobe == true)
        application.send(.setShortcutWarning(globe.setOwned(needsGlobe) ? nil : needsGlobe ? .globeUnavailable : .globeRestoreUnavailable))
    }

    public func stop() {
        if let tap { CGEvent.tapEnable(tap: tap, enable: false); CFMachPortInvalidate(tap) }
        if let source { CFRunLoopRemoveSource(CFRunLoopGetMain(), source, .commonModes) }
        tap = nil
        source = nil
        _ = globe.setOwned(false)
        if let quitObserver { NotificationCenter.default.removeObserver(quitObserver) }
        quitObserver = nil
        signalSources.forEach { $0.cancel() }
        signalSources = []
        signalRestorers.forEach { $0() }
        signalRestorers = []
        suppression.reset()
        application?.send(.resetShortcutInput([]))
        application?.send(.setShortcutAvailable(false))
    }

    isolated deinit {
        _ = globe.setOwned(false)
        if let quitObserver { NotificationCenter.default.removeObserver(quitObserver) }
        signalSources.forEach { $0.cancel() }
        signalRestorers.forEach { $0() }
        if let tap { CFMachPortInvalidate(tap) }
        if let source { CFRunLoopRemoveSource(CFRunLoopGetMain(), source, .commonModes) }
    }

    private func receive(_ type: CGEventType, event: CGEvent) -> Bool {
        if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
            guard AXIsProcessTrusted() else { stop(); return false }
            physicalKeys = Self.readPhysicalKeys()
            application?.send(.resetShortcutInput(physicalKeys))
            suppression.reset()
            if let tap { CGEvent.tapEnable(tap: tap, enable: true) }
            return false
        }
        guard event.getIntegerValueField(.eventSourceUserData) != whisperPasteEventTag else { return false }
        guard let nsEvent = NSEvent(cgEvent: event), let input = Self.input(for: nsEvent), let application else { return false }
        if let modifiers = input.heldModifiers {
            physicalKeys.subtract(ShortcutInput.modifierKeyCodes)
            physicalKeys.formUnion(modifiers)
        }
        if input.isDown { physicalKeys.insert(input.keyCode) } else { physicalKeys.remove(input.keyCode) }
        if application.state.shortcutCapture.isActive, !NSApplication.shared.isActive { return false }
        let capturing = application.state.shortcutCapture.isActive && NSApplication.shared.isActive
        var bindings = application.state.settings.shortcuts
        if application.state.dictation.phase.isActive, application.state.dictation.origin != .button,
           let active = application.activeShortcut { bindings.append(active.value) }
        let consume = suppression.consume(input, pressed: physicalKeys, bindings: bindings, capturing: capturing)
        let configuredEscape = application.activeShortcut?.key == "Esc"
            && application.activeShortcut?.matches(input, pressed: physicalKeys) == true
            && application.state.dictation.phase != .processing
        if input.keyCode == ShortcutInput.escape, input.isDown, !capturing,
           application.state.dictation.phase.isActive, !configuredEscape {
            // Cancellation only invalidates ownership; do it before an already queued server reply.
            application.send(.shortcut(input))
            return true
        }
        // Keychain/device setup may block; never perform it inside the event-tap callback.
        DispatchQueue.main.async { [weak application] in application?.send(.shortcut(input)) }
        return consume
    }
    private static func readPhysicalKeys() -> Set<UInt16> {
        Set((UInt16(0)..<128).filter { $0 != 57 && $0 != 71 && CGEventSource.keyState(.hidSystemState, key: $0) })
    }

    /// The settings responder uses the same event normalization when Accessibility is unavailable.
    public static func input(for event: NSEvent) -> ShortcutInput? {
        let code: UInt16
        let down: Bool
        var name: String?
        var repeated = false
        if event.type == .otherMouseDown || event.type == .otherMouseUp {
            guard event.buttonNumber == 3 || event.buttonNumber == 4 else { return nil }
            code = UInt16(0x1000 + event.buttonNumber)
            down = event.type == .otherMouseDown
        } else if event.type == .systemDefined {
            guard event.subtype.rawValue == 8 else { return nil }
            let type = (event.data1 >> 16) & 0xffff
            let names = [16: "MediaPlayPause", 17: "MediaNextTrack", 18: "MediaPreviousTrack", 19: "MediaNextTrack", 20: "MediaPreviousTrack"]
            guard let media = names[type] else { return nil }
            name = media
            code = UInt16(0x2000 + type)
            down = ((event.data1 >> 8) & 0xff) == 0x0a
            repeated = event.data1 & 1 != 0
        } else {
            guard event.type == .keyDown || event.type == .keyUp || event.type == .flagsChanged else { return nil }
            code = event.keyCode
            // Caps Lock's latched flags are not a held key and must not block every shortcut.
            if event.type == .flagsChanged, !ShortcutInput.modifierKeyCodes.contains(code) { return nil }
            down = event.type == .flagsChanged ? CGEventSource.keyState(.hidSystemState, key: code) : event.type == .keyDown
            if event.type != .flagsChanged {
                repeated = event.isARepeat
                if let scalar = event.charactersIgnoringModifiers?.unicodeScalars.first?.value {
                    if (0xf704...0xf71b).contains(scalar) { name = "F\(scalar - 0xf704 + 1)" }
                    else { name = [0xf72e: "PrintScreen", 0xf72f: "ScrollLock", 0xf730: "Pause", 0xf734: "MediaStop"][scalar] }
                }
            }
        }
        return ShortcutInput(keyCode: code, isDown: down, isRepeat: repeated,
            heldModifiers: Set(ShortcutInput.modifierKeyCodes.filter { CGEventSource.keyState(.hidSystemState, key: $0) }),
            keyName: name ?? ShortcutKeys.keyName(code))
    }
}
