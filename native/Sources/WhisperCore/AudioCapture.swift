import AVFoundation
import Foundation

public struct MicrophoneDevice: Equatable, Sendable {
    public let id: String
    public let name: String
    public let category: MicrophoneCategory
    public init(id: String, name: String, category: MicrophoneCategory = .external) {
        self.id = id
        self.name = name
        self.category = category
    }
}

/// One bounded, mono Float32 source frame. Zero amplitude is valid audio.
public struct AudioFrame: Sendable {
    public let samples: [Float]
    public let sampleRate: Double
    public let capturedAt: TimeInterval?
    public init(samples: [Float], sampleRate: Double = 48_000, capturedAt: TimeInterval? = nil) {
        self.samples = samples
        self.sampleRate = sampleRate
        self.capturedAt = capturedAt
    }
}

public enum CaptureEvent: Sendable {
    case opened(at: TimeInterval? = nil)
    case timing(CaptureTimingStage, at: TimeInterval)
    case frame(AudioFrame)
    case failed(DictationFailure)
}
public enum CaptureTimingStage: Sendable { case startRequested, startReturned }

public protocol MicrophoneSession: AnyObject, Sendable {
    func start()
    /// Mark the owner ended immediately; completion follows physical release, including late acquisition.
    func stop(completion: @escaping @Sendable () -> Void)
}

@MainActor public protocol MicrophoneProvider {
    /// Resolve a physical UID once per request, with no fallback after acquisition failure.
    func resolveDevice() throws -> MicrophoneDevice
    func inputSnapshot() throws -> MicrophoneSnapshot
    func makeSession(device: MicrophoneDevice, receive: @escaping @Sendable (CaptureEvent) -> Void) -> any MicrophoneSession
}

enum RecordingEvent: Sendable {
    case opened(at: TimeInterval? = nil)
    case audio(level: Float, duration: TimeInterval, capturedAt: TimeInterval?)
    case failed(DictationFailure)
}

/// Shared by processing and persistence so cancellation never removes a file still being consumed.
final class CapturedAudio: @unchecked Sendable {
    let url: URL
    let duration: TimeInterval
    init(url: URL, duration: TimeInterval) { self.url = url; self.duration = duration }
    deinit { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
}

/// The source callback writes incrementally, before notifying UI readiness. No duration-sized array is retained.
final class RecordingCapture: @unchecked Sendable {
    private let lock = NSLock()
    private let writerQueue = DispatchQueue(label: "local.whisper.writer." + UUID().uuidString)
    private var ended = false
    private var session: (any MicrophoneSession)?
    private var finishTask: Task<CapturedAudio, any Error>?
    private var recordedDuration: TimeInterval = 0
    // File state belongs exclusively to writerQueue. The terminal lock never covers disk I/O.
    private var file: AVAudioFile?
    private var sampleRate: Double = 0
    private var frames: Int64 = 0
    private var lastFeedback: TimeInterval = -.infinity
    private let directory: URL
    private let receiveEvent: @Sendable (RecordingEvent) -> Void
    private let diagnostics: RequestDiagnostics?

    init(directory: URL, diagnostics: RequestDiagnostics? = nil, receive: @escaping @Sendable (RecordingEvent) -> Void) {
        self.directory = directory
        self.diagnostics = diagnostics
        receiveEvent = receive
    }

    func attach(_ session: any MicrophoneSession) { lock.withLock { self.session = session } }
    private var isEnded: Bool { lock.withLock { ended } }
    var duration: TimeInterval { lock.withLock { recordedDuration } }

    func receive(_ event: CaptureEvent) {
        guard !isEnded else { return }
        switch event {
        case let .opened(time):
            diagnostics?.mark(.captureConfigured, at: time)
            receiveEvent(.opened(at: time))
        case let .timing(stage, time):
            diagnostics?.mark(stage == .startRequested ? .captureStartRequested : .captureStartReturned, at: time)
        case let .failed(failure): receiveEvent(.failed(failure))
        case let .frame(frame):
            // The serial source callback waits for this bounded write. Frames cannot accumulate unbounded tasks.
            writerQueue.sync { write(frame) }
        }
    }

    private func write(_ frame: AudioFrame) {
        guard !isEnded, !frame.samples.isEmpty, frame.samples.count <= 192_000,
              frame.sampleRate.isFinite, frame.sampleRate > 0,
              frame.samples.allSatisfy(\.isFinite) else { return }
        do {
            if frames == 0 { diagnostics?.mark(.firstAudio, at: frame.capturedAt) }
            if file == nil {
                try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
                sampleRate = frame.sampleRate
                file = try AVAudioFile(
                    forWriting: directory.appendingPathComponent("audio.m4a"),
                    settings: [AVFormatIDKey: kAudioFormatMPEG4AAC, AVSampleRateKey: sampleRate,
                               AVNumberOfChannelsKey: 1, AVEncoderBitRateKey: 128_000],
                    commonFormat: .pcmFormatFloat32, interleaved: false
                )
                try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: directory.appendingPathComponent("audio.m4a").path)
            }
            guard let file, sampleRate == frame.sampleRate,
                  let buffer = AVAudioPCMBuffer(pcmFormat: file.processingFormat, frameCapacity: AVAudioFrameCount(frame.samples.count)),
                  let channel = buffer.floatChannelData?[0] else { throw DictationFailure.captureFailed }
            buffer.frameLength = buffer.frameCapacity
            frame.samples.withUnsafeBufferPointer { channel.update(from: $0.baseAddress!, count: $0.count) }
            try file.write(from: buffer)
            frames += Int64(frame.samples.count)
            let duration = Double(frames) / sampleRate
            lock.withLock { recordedDuration = duration }
            let now = ProcessInfo.processInfo.systemUptime
            guard !isEnded, now - lastFeedback >= 0.08 else { return }
            lastFeedback = now
            let rms = sqrt(frame.samples.reduce(Float(0)) { $0 + $1 * $1 } / Float(frame.samples.count))
            receiveEvent(.audio(level: min(1, rms * 4), duration: duration, capturedAt: frame.capturedAt))
        } catch {
            if !isEnded { receiveEvent(.failed(error as? DictationFailure ?? .storageFailed)) }
        }
    }

    func finish() async throws -> CapturedAudio { try await finishOperation().value }

    /// Invalidates capture synchronously; all callers share one physical release and file finalization.
    func finishOperation() -> Task<CapturedAudio, any Error> {
        lock.withLock {
            if let finishTask { return finishTask }
            ended = true
            let session = self.session
            let stream = AsyncThrowingStream<CapturedAudio, any Error> { continuation in
                let complete: @Sendable () -> Void = { [self] in
                    diagnostics?.mark(.captureReleased)
                    writerQueue.async { [self] in
                        file?.close()
                        diagnostics?.mark(.recordingFinalized)
                        file = nil
                        lock.withLock { self.session = nil }
                        if frames > 0 {
                            continuation.yield(CapturedAudio(url: directory.appendingPathComponent("audio.m4a"), duration: Double(frames) / sampleRate))
                            continuation.finish()
                        } else {
                            try? FileManager.default.removeItem(at: directory)
                            continuation.finish(throwing: DictationFailure.noAudio)
                        }
                    }
                }
                if let session { session.stop(completion: complete) } else { complete() }
            }
            let task = Task {
                for try await audio in stream { return audio }
                throw DictationFailure.noAudio
            }
            finishTask = task
            return task
        }
    }

    func cancel() { _ = finishOperation() }
}
