import AppKit

/// The tap callback uses only its immutable session and thread-safe delivery snapshot.
final class NativeShortcutEventSource: @unchecked Sendable {
    private let lock = NSLock()
    private let delivery: ShortcutEventDelivery
    private let generation: UInt64
    private let permissionLost: @Sendable () -> Void
    private var tap: CFMachPort?
    private var runLoop: CFRunLoop?
    private var ended = false
    init(delivery: ShortcutEventDelivery, generation: UInt64, permissionLost: @escaping @Sendable () -> Void) {
        self.delivery = delivery; self.generation = generation; self.permissionLost = permissionLost
    }
    func start() -> Bool {
        let mask = [CGEventType.keyDown, .keyUp, .flagsChanged, .otherMouseDown, .otherMouseUp]
            .reduce(CGEventMask(1 << 14)) { $0 | (1 << $1.rawValue) }
        guard let port = CGEvent.tapCreate(tap: .cgSessionEventTap, place: .headInsertEventTap,
            options: .defaultTap, eventsOfInterest: mask, callback: { _, type, event, info in
                guard let info else { return Unmanaged.passUnretained(event) }
                let source = Unmanaged<NativeShortcutEventSource>.fromOpaque(info).takeUnretainedValue()
                return source.receive(type, event: event) ? nil : Unmanaged.passUnretained(event)
            }, userInfo: Unmanaged.passUnretained(self).toOpaque()) else { return false }
        lock.withLock { tap = port }
        let thread = Thread { [self] in run() }
        thread.name = "Whisper.Shortcuts"
        thread.qualityOfService = .userInteractive
        thread.start()
        return true
    }
    func stop() {
        let owned = lock.withLock { ended = true; return (tap, runLoop) }
        if let port = owned.0 { CGEvent.tapEnable(tap: port, enable: false); CFMachPortInvalidate(port) }
        if let loop = owned.1 { CFRunLoopStop(loop) }
    }
    private func run() {
        guard let port = lock.withLock({ ended ? nil : tap }) else { return }
        let loop = CFRunLoopGetCurrent()!
        guard let source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, port, 0) else {
            stop(); permissionLost(); return
        }
        CFRunLoopAddSource(loop, source, .defaultMode)
        lock.withLock { runLoop = loop }
        lock.withLock { if !ended { CGEvent.tapEnable(tap: port, enable: true) } }
        while !lock.withLock({ ended }) {
            _ = autoreleasepool { CFRunLoopRunInMode(.defaultMode, 0.25, true) }
        }
        CFRunLoopRemoveSource(loop, source, .defaultMode)
        lock.withLock { runLoop = nil; tap = nil }
    }
    private func receive(_ type: CGEventType, event: CGEvent) -> Bool {
        guard !lock.withLock({ ended }), delivery.isCurrent(generation) else { return false }
        if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
            delivery.reset(heldKeys: NativeShortcutMonitor.readPhysicalKeys(), generation: generation)
            guard AXIsProcessTrusted() else { stop(); permissionLost(); return false }
            lock.withLock { if !ended, let port = tap { CGEvent.tapEnable(tap: port, enable: true) } }
            return false
        }
        guard event.getIntegerValueField(.eventSourceUserData) != whisperPasteEventTag,
              let event = NSEvent(cgEvent: event), let input = NativeShortcutMonitor.input(for: event) else { return false }
        return delivery.receive(input, generation: generation)
    }
}
