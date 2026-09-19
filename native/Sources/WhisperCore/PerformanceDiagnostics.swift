import Darwin
import Foundation

public protocol MonotonicTimeSource: Sendable { func timestamp() -> TimeInterval }
public struct SystemMonotonicTimeSource: MonotonicTimeSource {
    public init() {}
    public func timestamp() -> TimeInterval { ProcessInfo.processInfo.systemUptime }
}

public enum DiagnosticStage: String, Codable, CaseIterable, Sendable {
    case requestAccepted, gestureResolved, deviceResolved, acquisitionRequested
    case captureConfigured, captureStartRequested, captureStartReturned, firstAudio, readyFeedback
    case stopAccepted, captureReleased, recordingFinalized, asrPreparationStarted
    case asrRequestDispatched, asrResponseReceived, asrResponseCompleted
    case cleanupPreparationStarted, cleanupRequestDispatched, cleanupResponseReceived, cleanupCompleted
    case processingComplete, deliveryStarted, pasteDispatched, pasteSettled, requestFinished
}
public enum DiagnosticOutcome: String, Codable, Sendable { case pending, completed, failed, cancelled, rejected, incomplete }
public enum DiagnosticService: String, Codable, Sendable { case asr, cleanup }
public enum DiagnosticDelivery: String, Codable, Sendable { case none, pasted, copied, recovery }
public enum DiagnosticCleanup: String, Codable, Sendable { case absent, pending, completed, failed }

public struct DiagnosticsState: Equatable, Sendable {
    public var enabled = false
    public var writtenRecords = 0
    public var droppedRecords = 0
    public var writeFailed = false
    public init() {}
}

private struct DiagnosticAttempt: Codable, Sendable {
    let service: DiagnosticService
    let index: Int
    var dispatchedMs: Double?
    var responseMs: Double?
    var outcome: DiagnosticOutcome = .pending
    var status: Int?
}

private struct DiagnosticRecord: Codable, Sendable {
    let collectionId: UUID
    let requestId: UUID
    var sequence = 0
    var origin: DictationOrigin
    var outcome: DiagnosticOutcome = .pending
    var startup: DiagnosticOutcome = .pending
    var delivery: DiagnosticDelivery = .none
    var cleanup: DiagnosticCleanup = .absent
    var stages: [String: Double] = [:]
    var attempts: [DiagnosticAttempt] = []
    var droppedRecords = 0
}

/// Only fixed vocabulary and numbers can enter a trace. No settings, text, errors or request objects.
public final class RequestDiagnostics: @unchecked Sendable {
    private let lock = NSLock()
    private let source: any MonotonicTimeSource
    private let acceptedAt: TimeInterval
    private let output: DiagnosticOutput
    private var record: DiagnosticRecord
    private var closed = false

    fileprivate init(id: UUID, origin: DictationOrigin, acceptedAt: TimeInterval, source: any MonotonicTimeSource, output: DiagnosticOutput) {
        self.source = source
        self.acceptedAt = acceptedAt
        self.output = output
        record = DiagnosticRecord(collectionId: output.id, requestId: id, origin: origin)
        record.stages[DiagnosticStage.requestAccepted.rawValue] = 0
        checkpoint()
    }

    public func mark(_ stage: DiagnosticStage, at timestamp: TimeInterval? = nil) {
        let elapsed = ((timestamp ?? source.timestamp()) - acceptedAt) * 1000
        guard elapsed.isFinite, elapsed >= 0 else { return }
        let checkpoint = lock.withLock {
            guard !closed, record.stages[stage.rawValue] == nil else { return false }
            record.stages[stage.rawValue] = elapsed
            if stage == .readyFeedback { record.startup = .completed }
            return stage == .readyFeedback
        }
        if checkpoint { self.checkpoint() }
    }
    public func setOrigin(_ origin: DictationOrigin) { lock.withLock { if !closed { record.origin = origin } } }
    public func setCleanup(_ outcome: DiagnosticCleanup) { lock.withLock { if !closed { record.cleanup = outcome } } }
    public func setDelivery(_ result: DeliveryResult) {
        lock.withLock {
            guard !closed else { return }
            record.delivery = switch result { case .none: .none; case .pasted: .pasted; case .copied: .copied; case .recovery: .recovery }
        }
    }

    public func network(_ service: DiagnosticService) -> NetworkDiagnostics? {
        lock.withLock {
            guard !closed, record.attempts.count < 64 else { return nil }
            let index = record.attempts.count
            record.attempts.append(DiagnosticAttempt(service: service, index: index))
            return NetworkDiagnostics(owner: self, index: index)
        }
    }
    fileprivate func network(_ index: Int, dispatched: Bool, status: Int? = nil, outcome: DiagnosticOutcome = .pending) {
        let elapsed = (source.timestamp() - acceptedAt) * 1000
        guard elapsed.isFinite, elapsed >= 0 else { return }
        lock.withLock {
            guard !closed, record.attempts.indices.contains(index) else { return }
            if dispatched {
                guard record.attempts[index].dispatchedMs == nil else { return }
                record.attempts[index].dispatchedMs = elapsed
                let stage: DiagnosticStage = record.attempts[index].service == .asr ? .asrRequestDispatched : .cleanupRequestDispatched
                if record.stages[stage.rawValue] == nil { record.stages[stage.rawValue] = elapsed }
            } else {
                guard record.attempts[index].outcome == .pending else { return }
                record.attempts[index].outcome = outcome
                if let status, (100...599).contains(status) {
                    record.attempts[index].status = status
                    record.attempts[index].responseMs = elapsed
                    let stage: DiagnosticStage = record.attempts[index].service == .asr ? .asrResponseReceived : .cleanupResponseReceived
                    record.stages[stage.rawValue] = elapsed
                }
            }
        }
    }
    public func finish(_ outcome: DiagnosticOutcome) {
        guard outcome != .pending else { return }
        let elapsed = (source.timestamp() - acceptedAt) * 1000
        let snapshot: DiagnosticRecord? = lock.withLock {
            guard !closed else { return nil }
            closed = true
            record.outcome = outcome
            if outcome == .cancelled {
                for index in record.attempts.indices where record.attempts[index].outcome == .pending {
                    record.attempts[index].outcome = .cancelled
                }
            }
            if record.startup == .pending { record.startup = outcome }
            if elapsed.isFinite, elapsed >= 0 { record.stages[DiagnosticStage.requestFinished.rawValue] = elapsed }
            record.sequence += 1
            return record
        }
        if let snapshot { output.append(snapshot) }
    }
    fileprivate func checkpoint() {
        let snapshot: DiagnosticRecord? = lock.withLock {
            guard !closed else { return nil }
            record.sequence += 1
            return record
        }
        if let snapshot { output.append(snapshot) }
    }
}

public final class NetworkDiagnostics: @unchecked Sendable {
    private let owner: RequestDiagnostics
    private let index: Int
    fileprivate init(owner: RequestDiagnostics, index: Int) {
        self.owner = owner; self.index = index
    }
    public func dispatched() { owner.network(index, dispatched: true) }
    public func received(status: Int) { owner.network(index, dispatched: false, status: status, outcome: (200...299).contains(status) ? .completed : .failed) }
    public func failed(cancelled: Bool) { owner.network(index, dispatched: false, outcome: cancelled ? .cancelled : .failed) }
}

/// Used exactly once by each real or controlled transport, immediately around its external operation.
func observeHTTP(_ diagnostics: NetworkDiagnostics?, operation: () async throws -> HTTPResponse) async throws -> HTTPResponse {
    diagnostics?.dispatched()
    do {
        let response = try await operation()
        diagnostics?.received(status: response.status)
        return response
    } catch {
        diagnostics?.failed(cancelled: error is CancellationError || (error as? URLError)?.code == .cancelled)
        throw error
    }
}

/// Disk work stays on a utility queue. Producers enqueue at most 200 fixed-size snapshots without waiting.
final class DiagnosticOutput: @unchecked Sendable {
    static let header = Data("{\"schema\":\"whisper-native-timing\",\"version\":1}\n".utf8)
    let id = UUID()
    private let queue = DispatchQueue(label: "local.whisper.diagnostics", qos: .utility)
    private let lock = NSLock()
    private let destination: URL
    private let report: @Sendable (UUID, DiagnosticsState) -> Void
    private var state = DiagnosticsState()
    private var pending = 0
    private var descriptor: Int32 = -1
    private var ended = false

    init(destination: URL, after previousClose: Task<Void, Never>?, report: @escaping @Sendable (UUID, DiagnosticsState) -> Void) {
        self.destination = destination
        self.report = report
        state.enabled = true
        // New writers wait for prior file ownership without blocking the application or dropping queued records.
        queue.suspend()
        queue.async { [self] in
            do { try openOutput() }
            catch {
                if descriptor >= 0 { Darwin.close(descriptor); descriptor = -1 }
                lock.withLock { state.writeFailed = true }
            }
            reportState()
        }
        Task { [queue] in await previousClose?.value; queue.resume() }
    }
    deinit { if descriptor >= 0 { Darwin.close(descriptor) } }
    func begin(id: UUID, origin: DictationOrigin, acceptedAt: TimeInterval, source: any MonotonicTimeSource) -> RequestDiagnostics {
        RequestDiagnostics(id: id, origin: origin, acceptedAt: acceptedAt, source: source, output: self)
    }
    fileprivate func append(_ snapshot: DiagnosticRecord) {
        lock.withLock {
            guard !ended, !state.writeFailed else { return }
            guard pending < 200 else { state.droppedRecords += 1; return }
            pending += 1
            // Enqueue while admission is locked so close cannot overtake an accepted snapshot.
            queue.async { [self] in
                defer { lock.withLock { pending -= 1 }; reportState() }
                do {
                    guard descriptor >= 0 else { return }
                    var snapshot = snapshot
                    snapshot.droppedRecords = lock.withLock { state.droppedRecords }
                    let encoder = JSONEncoder()
                    encoder.outputFormatting = [.sortedKeys]
                    var bytes = try encoder.encode(snapshot)
                    bytes.append(0x0a)
                    try write(bytes)
                    lock.withLock { state.writtenRecords += 1 }
                } catch {
                    if descriptor >= 0 { Darwin.close(descriptor); descriptor = -1 }
                    lock.withLock { state.writeFailed = true }
                }
            }
        }
    }
    func flush(close: Bool = false) async -> Bool {
        if close { lock.withLock { ended = true } }
        return await withCheckedContinuation { continuation in
            queue.async { [self] in
                if descriptor >= 0, fsync(descriptor) != 0 { lock.withLock { state.writeFailed = true } }
                if close, descriptor >= 0 { Darwin.close(descriptor); descriptor = -1 }
                reportState()
                continuation.resume(returning: lock.withLock { !state.writeFailed })
            }
        }
    }
    private func reportState() { report(id, lock.withLock { state }) }
    private func openOutput() throws {
        guard destination.isFileURL, destination.path.hasPrefix("/") else { throw CocoaError(.fileWriteInvalidFileName) }
        descriptor = Darwin.open(destination.path, O_RDWR | O_CREAT | O_APPEND | O_NOFOLLOW | O_CLOEXEC, 0o600)
        guard descriptor >= 0 else { throw CocoaError(.fileWriteUnknown) }
        var info = stat()
        guard fstat(descriptor, &info) == 0, info.st_mode & S_IFMT == S_IFREG,
              info.st_uid == getuid(), info.st_mode & 0o077 == 0,
              flock(descriptor, LOCK_EX | LOCK_NB) == 0 else { throw CocoaError(.fileWriteNoPermission) }
        if info.st_size == 0 { try write(Self.header) }
        else {
            var header = [UInt8](repeating: 0, count: Self.header.count)
            var last: UInt8 = 0
            guard pread(descriptor, &header, header.count, 0) == header.count,
                  Data(header) == Self.header, pread(descriptor, &last, 1, info.st_size - 1) == 1,
                  last == 0x0a else { throw CocoaError(.fileWriteInvalidFileName) }
        }
    }
    private func write(_ data: Data) throws {
        try data.withUnsafeBytes { bytes in
            var offset = 0
            while offset < bytes.count {
                let written = Darwin.write(descriptor, bytes.baseAddress!.advanced(by: offset), bytes.count - offset)
                if written < 0, errno == EINTR { continue }
                guard written > 0 else { throw CocoaError(.fileWriteUnknown) }
                offset += written
            }
        }
    }
}

extension WhisperApplication {
    func setDiagnosticsOutput(_ destination: URL?) {
        dictationDiagnostics?.finish(.incomplete)
        dictationDiagnostics = nil
        let previous = diagnosticsFlushTask
        let old = diagnosticOutput
        diagnosticsFlushTask = Task { await previous?.value; _ = await old?.flush(close: true) }
        state.diagnostics = DiagnosticsState()
        state.diagnostics.enabled = destination != nil
        diagnosticOutput = destination.map { destination in
            DiagnosticOutput(destination: destination, after: diagnosticsFlushTask) { [weak self] id, report in
                Task { @MainActor in
                    guard self?.diagnosticOutput?.id == id else { return }
                    self?.state.diagnostics = report
                }
            }
        }
    }
    @discardableResult public func flushDiagnostics() async -> Bool {
        dictationDiagnostics?.checkpoint()
        await diagnosticsFlushTask?.value
        return await diagnosticOutput?.flush() ?? true
    }
}
