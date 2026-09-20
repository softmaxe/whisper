import Foundation

/// A value snapshot lets the tap decide suppression without waiting for the application thread.
public struct ShortcutMonitorState: Sendable {
    public var bindings: [String]
    public var activeShortcut: String?
    public var requestActive: Bool
    public var processing: Bool
    public var capturing: Bool
    public var captureAllowed: Bool
    public var dismissingRecovery: Bool
    public init(bindings: [String], activeShortcut: String? = nil, requestActive: Bool = false,
                processing: Bool = false, capturing: Bool = false, captureAllowed: Bool = true,
                dismissingRecovery: Bool = false) {
        self.bindings = bindings; self.activeShortcut = activeShortcut
        self.requestActive = requestActive; self.processing = processing
        self.capturing = capturing; self.captureAllowed = captureAllowed
        self.dismissingRecovery = dismissingRecovery
    }
}

/// One tap session collects ordered edges while Keychain/device preparation occupies MainActor.
public final class ShortcutEventDelivery: @unchecked Sendable {
    private enum Event { case input(ShortcutInput), reset(Set<UInt16>) }
    private let lock = NSLock()
    private var generation: UInt64 = 0
    private var enabled = false
    private var pending: [Event] = []
    private var scheduled = false
    private var suppression = ShortcutEventSuppression()
    private var keys = Set<UInt16>()
    private var configuration = ShortcutMonitorState(bindings: [])
    private var provisionalBinding: ShortcutBinding?
    @MainActor private weak var application: WhisperApplication?
    @MainActor private var draining = false
    @MainActor public var didDrain: (() -> Void)?

    @MainActor public init(application: WhisperApplication) { self.application = application }

    @MainActor @discardableResult public func start(state: ShortcutMonitorState, heldKeys: Set<UInt16> = [], resetInput: Bool = true) -> UInt64 {
        let token = lock.withLock {
            generation &+= 1
            enabled = true; pending = []; scheduled = false
            configuration = state; keys = heldKeys; suppression.reset(); provisionalBinding = nil
            return generation
        }
        application?.shortcutInputDrain = { [weak self] in self?.drain() }
        if resetInput { application?.send(.resetShortcutInput(heldKeys)) }
        return token
    }

    @MainActor public func stop(resetInput: Bool = true) {
        lock.withLock { enabled = false; generation &+= 1; pending = []; scheduled = false; suppression.reset(); provisionalBinding = nil }
        application?.shortcutInputDrain = nil
        if resetInput { application?.send(.resetShortcutInput([])) }
    }

    public func update(_ state: ShortcutMonitorState) {
        lock.withLock {
            configuration = state
            if pending.isEmpty, !scheduled { provisionalBinding = nil }
        }
    }
    public func isCurrent(_ token: UInt64) -> Bool { lock.withLock { enabled && generation == token } }

    /// Returns suppression immediately; applying the edge never blocks the native callback.
    @discardableResult public func receive(_ input: ShortcutInput, generation token: UInt64) -> Bool {
        lock.withLock {
            guard enabled, generation == token else { return false }
            if let held = input.heldModifiers { keys.subtract(ShortcutInput.modifierKeyCodes); keys.formUnion(held) }
            if input.isDown { keys.insert(input.keyCode) } else { keys.remove(input.keyCode) }
            if configuration.capturing, !configuration.captureAllowed { return false }
            var values = configuration.bindings
            if let active = configuration.activeShortcut { values.append(active) }
            let active = configuration.activeShortcut.flatMap { try? ShortcutBinding($0) } ?? provisionalBinding
            let configuredEscape = active?.key == "Esc" && active?.matches(input, pressed: keys) == true && !configuration.processing
            let cancel = input.keyCode == ShortcutInput.escape && input.isDown && !configuration.capturing
                && (configuration.requestActive || provisionalBinding != nil) && !configuredEscape
            let consume = suppression.consume(input, pressed: keys, bindings: values, capturing: configuration.capturing,
                dismissingRecovery: configuration.dismissingRecovery || cancel)
            if !configuration.capturing, provisionalBinding == nil, input.isDown, !input.isRepeat {
                provisionalBinding = values.compactMap { try? ShortcutBinding($0) }.first { $0.matches(input, pressed: keys) }
            }
            enqueue(.input(input), token: token)
            return consume
        }
    }

    public func reset(heldKeys: Set<UInt16>, generation token: UInt64) {
        lock.withLock {
            guard enabled, generation == token else { return }
            keys = heldKeys; suppression.reset(); provisionalBinding = nil
            enqueue(.reset(heldKeys), token: token)
        }
    }

    private func enqueue(_ event: Event, token: UInt64) {
        pending.append(event)
        guard !scheduled else { return }
        scheduled = true
        DispatchQueue.main.async { [weak self] in self?.drain(generation: token) }
    }

    /// Timers and request completion drain the collected edge watermark before publishing effects.
    @MainActor public func drain() { drain(generation: lock.withLock { generation }) }
    @MainActor private func drain(generation token: UInt64) {
        guard !draining else { return }
        draining = true
        defer { draining = false }
        while true {
            let batch: [Event] = lock.withLock {
                guard enabled, generation == token else { return [] }
                let batch = pending; pending = []
                if batch.isEmpty { scheduled = false }
                return batch
            }
            guard !batch.isEmpty else { break }
            for event in batch {
                guard lock.withLock({ enabled && generation == token }) else { return }
                switch event {
                case let .input(input): application?.send(.shortcut(input))
                case let .reset(keys): application?.send(.resetShortcutInput(keys))
                }
            }
        }
        if lock.withLock({ enabled && generation == token }) { didDrain?() }
    }
}
