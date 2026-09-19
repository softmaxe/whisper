import AppKit

/// Reuses macos-text-monitor's AXValue/AXObserver path inside the signed application.
/// No helper process, field contents on stdout, or secondary Accessibility identity is needed.
@MainActor public final class NativeCorrectionMonitoringSystem: CorrectionMonitoringSystem {
    public init() {}
    public func capture(_ target: PasteTarget) async -> (any CorrectionField)? {
        guard AXIsProcessTrusted(), NSWorkspace.shared.frontmostApplication?.processIdentifier == target.processID else { return nil }
        let box = await Task.detached { NativeCorrectionElement.capture(target.processID) }.value
        guard !Task.isCancelled, NSWorkspace.shared.frontmostApplication?.processIdentifier == target.processID, let box else { return nil }
        return NativeCorrectionField(element: box)
    }
}

@MainActor private final class NativeCorrectionField: CorrectionField {
    let element: NativeCorrectionElement
    var beforePaste: CorrectionSnapshot { element.before }
    init(element: NativeCorrectionElement) { self.element = element }
    func read() async -> CorrectionSnapshot? {
        guard NSWorkspace.shared.frontmostApplication?.processIdentifier == element.pid else { return nil }
        let element = element
        let snapshot = await Task.detached { element.read() }.value
        return NSWorkspace.shared.frontmostApplication?.processIdentifier == element.pid ? snapshot : nil
    }
    func observe(_ changed: @escaping @Sendable () -> Void) -> any CorrectionObservation {
        NativeCorrectionObservation(element: element, changed: changed)
    }
}

private final class NativeCorrectionElement: @unchecked Sendable {
    let pid: Int32
    let application: AXUIElement
    let field: AXUIElement
    let before: CorrectionSnapshot
    init(pid: Int32, application: AXUIElement, field: AXUIElement, before: CorrectionSnapshot) {
        self.pid = pid; self.application = application; self.field = field; self.before = before
    }
    static func capture(_ pid: Int32) -> NativeCorrectionElement? {
        let app = AXUIElementCreateApplication(pid)
        guard let field = pasteTargetElement(pasteTargetAttribute(app, kAXFocusedUIElementAttribute)),
              isEditableTextElement(field, allowSelection: true), let snapshot = snapshot(field) else { return nil }
        return NativeCorrectionElement(pid: pid, application: app, field: field, before: snapshot)
    }
    func read() -> CorrectionSnapshot? {
        guard let focused = pasteTargetElement(pasteTargetAttribute(application, kAXFocusedUIElementAttribute)),
              CFEqual(focused, field), isEditableTextElement(field, allowSelection: true) else { return nil }
        return Self.snapshot(field)
    }
    private static func snapshot(_ field: AXUIElement) -> CorrectionSnapshot? {
        guard let text = pasteTargetAttribute(field, kAXValueAttribute) as? String,
              let rangeValue = pasteTargetAttribute(field, kAXSelectedTextRangeAttribute),
              CFGetTypeID(rangeValue) == AXValueGetTypeID() else { return nil }
        let value = rangeValue as! AXValue
        var range = CFRange()
        guard AXValueGetType(value) == .cfRange, AXValueGetValue(value, .cfRange, &range),
              range.location >= 0, range.length >= 0, range.location <= text.utf16.count,
              range.length <= text.utf16.count - range.location else { return nil }
        return CorrectionSnapshot(text: text, selection: range.location..<(range.location + range.length))
    }
}

private final class NativeCorrectionObservation: CorrectionObservation, @unchecked Sendable {
    private let element: NativeCorrectionElement
    private let changed: @Sendable () -> Void
    private let lock = NSLock()
    private var ended = false
    private var runLoop: CFRunLoop?
    init(element: NativeCorrectionElement, changed: @escaping @Sendable () -> Void) {
        self.element = element; self.changed = changed
        Thread.detachNewThread { [self] in run() }
    }
    func cancel() {
        let loop = lock.withLock { ended = true; return runLoop }
        if let loop { CFRunLoopStop(loop) }
    }
    private func run() {
        guard !lock.withLock({ ended }) else { return }
        var created: AXObserver?
        let result = AXObserverCreate(element.pid, { _, _, _, context in
            guard let context else { return }
            let owner = Unmanaged<NativeCorrectionObservation>.fromOpaque(context).takeUnretainedValue()
            if !owner.lock.withLock({ owner.ended }) { owner.changed() }
        }, &created)
        guard result == .success, let observer = created else { return }
        let context = Unmanaged.passUnretained(self).toOpaque()
        guard AXObserverAddNotification(observer, element.field, kAXValueChangedNotification as CFString, context) == .success else { return }
        let loop = CFRunLoopGetCurrent()!
        let source = AXObserverGetRunLoopSource(observer)
        CFRunLoopAddSource(loop, source, .defaultMode)
        lock.withLock { runLoop = loop }
        // A bounded run-loop wait handles cancellation before CFRunLoopRun starts.
        while !lock.withLock({ ended }) { CFRunLoopRunInMode(.defaultMode, 0.25, true) }
        CFRunLoopRemoveSource(loop, source, .defaultMode)
        AXObserverRemoveNotification(observer, element.field, kAXValueChangedNotification as CFString)
        lock.withLock { runLoop = nil }
    }
}
