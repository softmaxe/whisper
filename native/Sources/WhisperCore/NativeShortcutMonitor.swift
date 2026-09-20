import AppKit
import Carbon.HIToolbox

/// One independently serviced tap collects edges while the application prepares capture.
@MainActor public final class NativeShortcutMonitor {
    private weak var application: WhisperApplication?
    private var source: NativeShortcutEventSource?
    private let delivery: ShortcutEventDelivery
    private let globe: GlobePreferenceController
    private var quitObserver: NSObjectProtocol?
    private var signalSources: [DispatchSourceSignal] = []
    private var signalRestorers: [() -> Void] = []
    public init(application: WhisperApplication) {
        self.application = application
        delivery = ShortcutEventDelivery(application: application)
        globe = GlobePreferenceController(markerURL: application.profileStore.profile.directory.appendingPathComponent("globe-preference.json"),
            system: NativeGlobePreferenceSystem())
        delivery.didDrain = { [weak self] in self?.refreshDeliveryConfiguration() }
    }

    @discardableResult public func start(requestPermission: Bool = false) -> Bool {
        if source != nil {
            if AXIsProcessTrusted() { updateConfiguration(); return true }
            stop()
        }
        application?.send(.setShortcutWarning(globe.setOwned(false) ? nil : .globeRestoreUnavailable))
        let options = ["AXTrustedCheckOptionPrompt": requestPermission] as CFDictionary
        guard AXIsProcessTrustedWithOptions(options), let state = snapshot() else {
            application?.send(.setShortcutAvailable(false)); return false
        }
        let heldKeys = Self.readPhysicalKeys()
        let token = delivery.start(state: state, heldKeys: heldKeys, resetInput: false)
        let source = NativeShortcutEventSource(delivery: delivery, generation: token) { [weak self] in
            Task { @MainActor in
                guard let self, self.delivery.isCurrent(token) else { return }
                self.stop()
            }
        }
        guard source.start() else {
            delivery.stop(resetInput: false)
            application?.send(.setShortcutAvailable(false)); return false
        }
        self.source = source
        application?.send(.resetShortcutInput(heldKeys))
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

    private func snapshot() -> ShortcutMonitorState? {
        guard let application else { return nil }
        return ShortcutMonitorState(bindings: application.state.settings.shortcuts,
            activeShortcut: application.state.dictation.phase.isActive ? application.activeShortcut?.value : nil,
            requestActive: application.state.dictation.phase.isActive,
            processing: application.state.dictation.phase == .processing,
            capturing: application.state.shortcutCapture.isActive, captureAllowed: NSApplication.shared.isActive,
            dismissingRecovery: application.state.canDismissCopyRecovery)
    }

    public func updateConfiguration() {
        guard source != nil, let application, let state = snapshot() else { return }
        delivery.update(state)
        let needsGlobe = application.state.shortcutCapture.isActive || application.state.settings.shortcuts.contains("GLOBE")
            || (application.state.dictation.phase.isActive && application.state.dictation.origin != .button && application.activeShortcut?.isGlobe == true)
        application.send(.setShortcutWarning(globe.setOwned(needsGlobe) ? nil : needsGlobe ? .globeUnavailable : .globeRestoreUnavailable))
    }

    private func refreshDeliveryConfiguration() {
        if source != nil, let state = snapshot() { delivery.update(state) }
    }

    public func stop() {
        delivery.stop()
        source?.stop(); source = nil
        _ = globe.setOwned(false)
        if let quitObserver { NotificationCenter.default.removeObserver(quitObserver) }
        quitObserver = nil
        signalSources.forEach { $0.cancel() }
        signalSources = []
        signalRestorers.forEach { $0() }
        signalRestorers = []
        application?.send(.setShortcutAvailable(false))
    }

    isolated deinit {
        delivery.stop()
        source?.stop()
        _ = globe.setOwned(false)
        if let quitObserver { NotificationCenter.default.removeObserver(quitObserver) }
        signalSources.forEach { $0.cancel() }
        signalRestorers.forEach { $0() }
    }

    nonisolated static func readPhysicalKeys() -> Set<UInt16> {
        var keys = Set((UInt16(0)..<256).filter { $0 != 57 && $0 != 71 && CGEventSource.keyState(.hidSystemState, key: $0) })
        for button: UInt32 in [3, 4] where CGEventSource.buttonState(.hidSystemState, button: CGMouseButton(rawValue: button)!) {
            keys.insert(UInt16(0x1000 + button))
        }
        return keys
    }

    /// The settings responder uses the same event normalization when Accessibility is unavailable.
    nonisolated public static func input(for event: NSEvent, physicalKeys: Set<UInt16>? = nil) -> ShortcutInput? {
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
            down = event.type == .flagsChanged ? physicalKeys?.contains(code) ?? CGEventSource.keyState(.hidSystemState, key: code) : event.type == .keyDown
            if event.type != .flagsChanged {
                repeated = event.isARepeat
                if let scalar = event.charactersIgnoringModifiers?.unicodeScalars.first?.value {
                    if (0xf704...0xf71b).contains(scalar) { name = "F\(scalar - 0xf704 + 1)" }
                    else { name = [0xf72e: "PrintScreen", 0xf72f: "ScrollLock", 0xf730: "Pause", 0xf734: "MediaStop"][scalar] }
                }
            }
        }
        return ShortcutInput(keyCode: code, isDown: down, isRepeat: repeated,
            heldModifiers: physicalKeys?.intersection(ShortcutInput.modifierKeyCodes)
                ?? Set(ShortcutInput.modifierKeyCodes.filter { CGEventSource.keyState(.hidSystemState, key: $0) }),
            keyName: name ?? ShortcutKeys.keyName(code), occurredAt: event.timestamp)
    }
}
