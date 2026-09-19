import Foundation

/// Cancellation releases UI ownership immediately; termination joins every outstanding owner.
@MainActor final class WorkflowTasks {
    private var tasks: [UUID: Task<Void, Never>] = [:]

    @discardableResult func start(_ operation: @escaping @MainActor () async -> Void) -> Task<Void, Never> {
        let id = UUID()
        let task = Task { [weak self] in
            await operation()
            self?.tasks.removeValue(forKey: id)
        }
        tasks[id] = task
        return task
    }

    func cancelAll() { for task in tasks.values { task.cancel() } }

    func waitForAll() async {
        // A cancelled parent may still be settling a deadline's child operation.
        while !tasks.isEmpty {
            let pending = Array(tasks.values)
            for task in pending { await task.value }
        }
    }
}

extension WhisperApplication {
    func releaseDictationCapture() {
        guard let capture = dictationCapture else { return }
        let release = capture.finishOperation()
        workflowTasks.start { _ = try? await release.value }
        dictationCapture = nil
    }
}
