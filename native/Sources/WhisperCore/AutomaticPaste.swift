import Foundation

public struct PasteTarget: Equatable, Sendable {
    public let processID: Int32
    public init(processID: Int32) { self.processID = processID }
}

public struct ClipboardSnapshot: Equatable, Sendable {
    public let items: [[String: Data]]
    public init(items: [[String: Data]]) { self.items = items }
}

/// Only operating-system effects are controlled by tests; delivery policy runs unchanged.
@MainActor public protocol AutomaticPasteSystem {
    func captureTarget() -> PasteTarget?
    var modifiersHeld: Bool { get }
    func snapshotClipboard() -> ClipboardSnapshot
    func replaceClipboard(with text: String) -> Int?
    func restoreClipboard(_ snapshot: ClipboardSnapshot, ownedRevision: Int)
    func activate(_ target: PasteTarget) async -> Bool
    func canPaste(_ target: PasteTarget) async -> Bool
    func paste(_ target: PasteTarget) async -> Bool
    func paste(_ target: PasteTarget, diagnostics: RequestDiagnostics?) async -> Bool
}
public extension AutomaticPasteSystem {
    func paste(_ target: PasteTarget, diagnostics: RequestDiagnostics?) async -> Bool {
        diagnostics?.mark(.pasteDispatched)
        let result = await paste(target)
        diagnostics?.mark(.pasteSettled)
        return result
    }
}

public enum DeliveryResult: Equatable, Sendable {
    case none, pasted, copied, recovery(copied: Bool)

    public func message(in language: AppLanguage) -> String? {
        switch self {
        case .none: nil
        case .pasted: language.text("Pasted into your app.", "已粘贴到目标应用。")
        case .copied: language.text("Copied to clipboard.", "已复制到剪贴板。")
        case .recovery(true): language.text("Automatic paste could not complete. Press Command-V to paste, or copy the text below.", "自动粘贴未完成。请按 Command-V 粘贴，或复制下方文字。")
        case .recovery(false): language.text("Automatic paste could not complete. Copy the text below to keep it.", "自动粘贴未完成。请复制下方文字。")
        }
    }
}

@MainActor final class AutomaticPaste {
    let system: any AutomaticPasteSystem
    let clock: any WorkflowClock
    private let tasks: WorkflowTasks
    private var tail: Task<Void, Never>?
    init(system: any AutomaticPasteSystem, clock: any WorkflowClock, tasks: WorkflowTasks) {
        self.system = system; self.clock = clock; self.tasks = tasks
    }

    func deliver(_ text: String, target: PasteTarget?, enabled: Bool, keepClipboard: Bool,
                 diagnostics: RequestDiagnostics? = nil,
                 willPaste: (@MainActor @Sendable (PasteTarget) async -> Void)? = nil,
                 isCurrent: @escaping @MainActor @Sendable () -> Bool) async -> DeliveryResult {
        let previous = tail
        diagnostics?.mark(.deliveryStarted)
        return await withCheckedContinuation { continuation in
            tail = tasks.start { [system, clock] in
                await previous?.value
                guard isCurrent() else { continuation.resume(returning: .none); return }
                guard enabled || keepClipboard else { continuation.resume(returning: .none); return }
                let original = system.snapshotClipboard()
                guard let revision = system.replaceClipboard(with: text) else {
                    continuation.resume(returning: .recovery(copied: false)); return
                }
                guard enabled else { continuation.resume(returning: .copied); return }
                // Never inject into physically held modifiers or release the user's keys.
                let deadline = clock.now + 0.5
                while system.modifiersHeld, clock.now < deadline, isCurrent() {
                    await clock.waitForDelivery(0.025)
                }
                guard isCurrent() else {
                    system.restoreClipboard(original, ownedRevision: revision)
                    continuation.resume(returning: .none); return
                }
                var pasted = false
                if !system.modifiersHeld, let target,
                   await system.activate(target), isCurrent(),
                   await system.canPaste(target), isCurrent(), !system.modifiersHeld {
                    await willPaste?(target)
                    if isCurrent(), !system.modifiersHeld { pasted = await system.paste(target, diagnostics: diagnostics) }
                }
                guard isCurrent() else {
                    if !pasted { system.restoreClipboard(original, ownedRevision: revision) }
                    continuation.resume(returning: .none)
                    if pasted, !keepClipboard {
                        await clock.waitForDelivery(0.450)
                        system.restoreClipboard(original, ownedRevision: revision)
                    }
                    return
                }
                guard pasted else {
                    continuation.resume(returning: .recovery(copied: true)); return
                }
                continuation.resume(returning: .pasted)
                if !keepClipboard {
                    await clock.waitForDelivery(0.450)
                    system.restoreClipboard(original, ownedRevision: revision)
                }
            }
        }
    }
}

extension WorkflowClock {
    /// The clipboard queue survives a completed Dictation until its own restoration settles.
    func waitForDelivery(_ seconds: TimeInterval) async {
        var pending: (any ScheduledAction)?
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            pending = schedule(after: seconds) { continuation.resume() }
        }
        withExtendedLifetime(pending) {}
    }
}
