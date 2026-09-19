import Foundation

public enum BatchUploadStatus: String, Equatable, Sendable {
    case queued, preparing, transcribing, saving, done, failed, cancelled
    public var isActive: Bool { self == .preparing || self == .transcribing || self == .saving }
    public var isSettled: Bool { self == .done || self == .failed || self == .cancelled }
}

public struct BatchUploadItem: Identifiable, Equatable, Sendable {
    public let id: UUID
    public let source: URL
    public var name: String { source.lastPathComponent }
    public var status: BatchUploadStatus = .queued
    public var sourceSize: Int64?
    public var model = ""
    public var text = ""
    public var failure: UploadFailure?
    public var historySaveFailed = false
    public var historyID: UUID?
    public var resultCopied = false
    public var copyFailed = false

    public init(source: URL) { id = UUID(); self.source = source }
}

public struct BatchUploadState: Equatable, Sendable {
    public var items: [BatchUploadItem] = []
    public var isProcessing = false
    public var runID: UUID?
    public var failure: UploadFailure?
    public var skippedNames: [String] = []
    public var completedCount: Int { items.filter { $0.status == .done }.count }
    public var failedCount: Int { items.filter { $0.status == .failed }.count }
    public var cancelledCount: Int { items.filter { $0.status == .cancelled }.count }
    public var settledCount: Int { completedCount + failedCount + cancelledCount }
    public var progress: Double { items.isEmpty ? 0 : Double(settledCount) / Double(items.count) }
    public var hasQueued: Bool { items.contains { $0.status == .queued } }
    public init() {}
}

extension WhisperApplication {
    func chooseUploadFiles(_ sources: [URL]) {
        guard !uploadShutdown, !state.upload.phase.isActive else { return }
        let accepted = sources.filter(UploadFormats.accepts)
        state.batchUpload.skippedNames = sources.filter { !UploadFormats.accepts($0) }.map(\.lastPathComponent)
        guard !accepted.isEmpty else { return }
        if accepted.count == 1, state.batchUpload.items.isEmpty, !state.batchUpload.isProcessing {
            selectUpload(accepted[0])
        } else { addUploadBatchFiles(sources) }
    }

    func addUploadBatchFiles(_ sources: [URL]) {
        guard !uploadShutdown, !state.upload.phase.isActive else { return }
        let accepted = sources.filter(UploadFormats.accepts)
        let skipped = sources.filter { !UploadFormats.accepts($0) }.map(\.lastPathComponent)
        state.batchUpload.skippedNames = skipped
        guard !accepted.isEmpty else { return }
        if state.batchUpload.items.isEmpty { state.upload = UploadState() }
        // Selecting the same file again creates another explicit queued operation, as in the legacy UI.
        state.batchUpload.items.append(contentsOf: accepted.map { BatchUploadItem(source: $0) })
        state.batchUpload.failure = nil
    }

    func removeUploadBatchItem(_ id: UUID) {
        state.batchUpload.items.removeAll { $0.id == id && $0.status == .queued }
    }

    func startUploadBatch() {
        guard !uploadShutdown, !state.upload.phase.isActive, !state.batchUpload.isProcessing, state.batchUpload.hasQueued else { return }
        let snapshot: UploadRunConfiguration
        do { snapshot = try uploadRunConfiguration() }
        catch {
            state.batchUpload.failure = error as? ConfigurationError == .credentialUnavailable ? .credentialUnavailable : .configuration
            return
        }
        let run = UUID()
        state.batchUpload.runID = run
        state.batchUpload.isProcessing = true
        state.batchUpload.failure = nil
        let converter = uploadConverter
        let transcriber = self.transcriber
        let task = Task { [weak self] in
            defer { self?.uploadTasks.removeValue(forKey: run) }
            while !Task.isCancelled, self?.isCurrentUploadBatch(run) == true,
                  let item = self?.state.batchUpload.items.first(where: { $0.status == .queued }) {
                self?.updateUploadBatchItem(item.id, run: run) { $0.status = .preparing; $0.model = snapshot.asr.model }
                do {
                    let text = try await UploadFileProcessing.transcribe(item.source, configuration: snapshot, converter: converter, transcriber: transcriber) { [weak self] size in
                        guard self?.isCurrentUploadBatch(run) == true else { return false }
                        self?.updateUploadBatchItem(item.id, run: run) { $0.status = .transcribing; $0.sourceSize = size }
                        return true
                    }
                    try Task.checkCancellation()
                    guard self?.isCurrentUploadBatch(run) == true else { return }
                    self?.updateUploadBatchItem(item.id, run: run) { $0.text = text; $0.status = .saving }
                    let completedAt = self?.clock.wallDate ?? Date()
                    let entry = HistoryEntry(id: item.id, text: text, rawText: text, occurredAt: completedAt,
                        createdAt: completedAt, source: .upload, model: snapshot.asr.model)
                    do {
                        let saved = try await self?.recordHistory(entry) ?? false
                        self?.updateUploadBatchItem(item.id, run: run) {
                            $0.status = .done
                            $0.historyID = saved ? item.id : nil
                        }
                    } catch {
                        self?.updateUploadBatchItem(item.id, run: run) { $0.status = .failed; $0.historySaveFailed = true }
                    }
                } catch is CancellationError {
                    guard !Task.isCancelled else { return }
                    self?.updateUploadBatchItem(item.id, run: run) { $0.status = .failed; $0.failure = .transcription(.network) }
                } catch {
                    let failure = error as? UploadFailure ?? .transcription(error as? DictationFailure ?? .network)
                    self?.updateUploadBatchItem(item.id, run: run) { $0.status = .failed; $0.failure = failure }
                }
            }
            if self?.isCurrentUploadBatch(run) == true { self?.state.batchUpload.isProcessing = false }
        }
        uploadTasks[run] = task
    }

    func isCurrentUploadBatch(_ run: UUID) -> Bool {
        state.batchUpload.isProcessing && state.batchUpload.runID == run
    }

    func updateUploadBatchItem(_ id: UUID, run: UUID, _ update: (inout BatchUploadItem) -> Void) {
        guard isCurrentUploadBatch(run), let index = state.batchUpload.items.firstIndex(where: { $0.id == id }) else { return }
        update(&state.batchUpload.items[index])
    }

    func cancelUploadBatch() {
        if let run = state.batchUpload.runID { uploadTasks[run]?.cancel() }
        state.batchUpload.runID = nil
        state.batchUpload.isProcessing = false
        state.batchUpload.items = state.batchUpload.items.map { item in
            var item = item
            if !item.status.isSettled { item.status = .cancelled }
            return item
        }
    }

    func clearUploadBatch() {
        cancelUploadBatch()
        state.batchUpload = BatchUploadState()
    }

    func copyUploadBatchItem(_ id: UUID) {
        guard let index = state.batchUpload.items.firstIndex(where: { $0.id == id }), !state.batchUpload.items[index].text.isEmpty else { return }
        let copied = clipboard.writeResult(state.batchUpload.items[index].text)
        state.batchUpload.items[index].resultCopied = copied
        state.batchUpload.items[index].copyFailed = !copied
    }
}
