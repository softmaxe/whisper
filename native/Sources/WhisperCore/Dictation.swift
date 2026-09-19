import Foundation

public enum DictationPhase: String, Equatable, Sendable {
    case idle, preparing, recording, processing, result, failed
    public var isActive: Bool { self == .preparing || self == .recording || self == .processing }
}

public enum DictationFailure: Error, Equatable, Sendable {
    case configuration, permissionDenied, inputUnavailable, captureFailed, noAudio, storageFailed
    case network, service(Int), invalidResponse, emptyTranscript

    public func message(in language: AppLanguage) -> String {
        switch self {
        case .configuration: language.text("Check your Speech-to-Text settings and saved API key.", "请检查语音转文字设置和已保存的 API key。")
        case .permissionDenied: language.text("Allow microphone access in System Settings, then try again.", "请在系统设置中允许麦克风访问，然后重试。")
        case .inputUnavailable: language.text("The selected microphone is unavailable. Reconnect it and try again.", "所选麦克风不可用。请重新连接后重试。")
        case .captureFailed: language.text("Microphone capture stopped. Check the selected input and try again.", "麦克风采集已停止。请检查所选输入，然后重试。")
        case .noAudio: language.text("No audio arrived from the microphone. Check the input and try again.", "未收到麦克风音频。请检查输入，然后重试。")
        case .storageFailed: language.text("The recording could not be saved. Check available disk space and try again.", "无法保存录音。请检查可用磁盘空间，然后重试。")
        case .network: language.text("The transcription server could not be reached. Check the connection and try again.", "无法连接转录服务器。请检查连接，然后重试。")
        case let .service(status): language.text("The transcription server returned HTTP \(status). Check its settings and try again.", "转录服务器返回 HTTP \(status)。请检查设置，然后重试。")
        case .invalidResponse: language.text("The server returned an invalid transcription response.", "服务器返回了无效的转录响应。")
        case .emptyTranscript: language.text("No text was transcribed. Try recording again.", "没有识别到文字。请重新录音。")
        }
    }
}

public struct DictationState: Equatable, Sendable {
    public var phase: DictationPhase = .idle
    public var requestID: UUID?
    public var rawText = ""
    public var text = ""
    public var resultCopied = false
    public var origin: DictationOrigin = .button
    public var gesture: DictationGesture = .none
    public var cancellation: DictationCancellation?
    public var delivery: DeliveryResult = .none
    public var failure: DictationFailure?
    public var level: Float = 0
    public var duration: TimeInterval = 0
    /// Monotonic stage offsets contain no device names, server URLs, or speech.
    public var timing: [String: TimeInterval] = [:]
    public init() {}
}

@MainActor public protocol ScheduledAction { func cancel() }
@MainActor public protocol WorkflowClock {
    var now: TimeInterval { get }
    func schedule(after seconds: TimeInterval, _ action: @escaping @MainActor @Sendable () -> Void) -> any ScheduledAction
}

@MainActor public final class SystemWorkflowClock: WorkflowClock {
    public init() {}
    public var now: TimeInterval { ProcessInfo.processInfo.systemUptime }
    public func schedule(after seconds: TimeInterval, _ action: @escaping @MainActor @Sendable () -> Void) -> any ScheduledAction {
        ClockAction(seconds: seconds, action: action)
    }
    private final class ClockAction: ScheduledAction {
        let task: Task<Void, Never>
        init(seconds: TimeInterval, action: @escaping @MainActor @Sendable () -> Void) {
            task = Task {
                do { try await Task.sleep(for: .seconds(seconds)); action() } catch {}
            }
        }
        func cancel() { task.cancel() }
        deinit { task.cancel() }
    }
}

extension WhisperApplication {
    func startDictation(origin: DictationOrigin = .button) {
        guard !state.dictation.phase.isActive else { return }
        let id = UUID()
        state.dictation = DictationState()
        state.dictation.requestID = id
        state.dictation.phase = .preparing
        state.dictation.origin = origin
        state.dictation.gesture = origin == .hold ? .candidate : .none
        provisionalFailure = nil
        dictationTarget = origin == .hold ? pasteSystem.captureTarget() : nil
        state.dictation.timing["accepted"] = 0
        dictationStartedAt = clock.now
        do {
            let configuration = try state.settings.asr.validated()
            let credential = try asrCredential()
            let device = try microphones.resolveDevice()
            let directory = profileStore.profile.directory.appendingPathComponent("Temporary/" + id.uuidString)
            let capture = RecordingCapture(directory: directory) { [weak self] event in
                Task { @MainActor in self?.receiveCapture(event, requestID: id) }
            }
            let session = microphones.makeSession(device: device) { [weak capture] event in capture?.receive(event) }
            capture.attach(session)
            dictationCapture = capture
            dictationConfiguration = configuration
            dictationCredential = credential
            session.start()
        } catch {
            failDictation(error as? DictationFailure ?? .configuration, requestID: id)
        }
    }

    func receiveCapture(_ event: RecordingEvent, requestID: UUID) {
        guard state.dictation.requestID == requestID, state.dictation.phase.isActive else { return }
        switch event {
        case let .opened(time):
            state.dictation.timing["acquisitionCompleted"] = (time ?? clock.now) - dictationStartedAt
            guard state.dictation.phase == .preparing, state.dictation.timing["firstAudio"] == nil, firstAudioDeadline == nil else { return }
            firstAudioDeadline = clock.schedule(after: max(0, 10 - (clock.now - (time ?? clock.now)))) { [weak self] in
                self?.failDictation(.noAudio, requestID: requestID)
            }
        case let .audio(level, duration, capturedAt):
            guard state.dictation.phase == .preparing || state.dictation.phase == .recording else { return }
            if state.dictation.timing["firstAudio"] == nil {
                firstAudioDeadline?.cancel()
                firstAudioDeadline = nil
                state.dictation.timing["firstAudio"] = (capturedAt ?? clock.now) - dictationStartedAt
                publishRecordingReadiness()
            }
            state.dictation.level = level
            state.dictation.duration = duration
        case let .failed(failure): failDictation(failure, requestID: requestID)
        }
    }

    func stopDictation() {
        guard let id = state.dictation.requestID,
              state.dictation.phase == .preparing || state.dictation.phase == .recording else { return }
        guard state.dictation.phase == .recording, let capture = dictationCapture else {
            cancelDictation()
            return
        }
        firstAudioDeadline?.cancel()
        firstAudioDeadline = nil
        state.dictation.phase = .processing
        state.dictation.level = 0
        state.dictation.timing["stop"] = clock.now - dictationStartedAt
        let configuration = dictationConfiguration
        let credential = dictationCredential
        let transcriber = self.transcriber
        let options = dictationTranscriptionOptions()
        processingTask = Task { [weak self] in
            do {
                let file = try await capture.finish()
                defer { capture.removeFiles() }
                try Task.checkCancellation()
                guard self?.isCurrentDictation(id) == true, let configuration else { return }
                self?.markDictationStage("asrDispatch", requestID: id)
                let text = try await transcriber.transcribe(
                    file: file, configuration: configuration, credential: credential,
                    options: options
                )
                try Task.checkCancellation()
                self?.completeDictation(rawText: text, text: text, requestID: id)
            } catch is CancellationError {
                capture.removeFiles()
            } catch {
                self?.failDictation(error as? DictationFailure ?? .network, requestID: id)
                capture.removeFiles()
            }
        }
    }

    func dictationTranscriptionOptions() -> TranscriptionOptions { TranscriptionOptions() }

    func markDictationStage(_ stage: String, requestID: UUID) {
        guard isCurrentDictation(requestID) else { return }
        state.dictation.timing[stage] = clock.now - dictationStartedAt
    }

    func completeDictation(rawText: String, text: String, requestID: UUID) {
        guard isCurrentDictation(requestID) else { return }
        markDictationStage("result", requestID: requestID)
        state.dictation.rawText = rawText
        state.dictation.text = text
        dictationCapture = nil
        dictationCredential = nil
        guard state.dictation.origin == .hold else {
            state.dictation.phase = .result
            processingTask = nil
            return
        }
        let target = dictationTarget
        let settings = state.settings
        let delivery = automaticPaste
        processingTask = Task { [weak self] in
            let result = await delivery.deliver(text, target: target, enabled: settings.autoPasteEnabled,
                keepClipboard: settings.keepTranscriptionInClipboard) { [weak self] in
                    self?.isCurrentDictation(requestID) == true
                }
            guard self?.isCurrentDictation(requestID) == true else { return }
            self?.state.dictation.delivery = result
            self?.state.dictation.phase = .result
            self?.processingTask = nil
        }
    }

    func isCurrentDictation(_ id: UUID) -> Bool {
        state.dictation.requestID == id && state.dictation.phase.isActive
    }

    func cancelDictation(kind: DictationCancellation = .user) {
        guard state.dictation.phase.isActive else { return }
        holdDeadline?.cancel()
        holdDeadline = nil
        firstAudioDeadline?.cancel()
        firstAudioDeadline = nil
        processingTask?.cancel()
        processingTask = nil
        dictationCapture?.cancel()
        dictationCapture = nil
        dictationCredential = nil
        state.dictation.phase = .idle
        state.dictation.cancellation = kind
        provisionalFailure = nil
        state.dictation.gesture = .none
        state.dictation.text = ""
        state.dictation.rawText = ""
        state.dictation.level = 0
        state.dictation.timing["cancelled"] = clock.now - dictationStartedAt
    }

    func failDictation(_ failure: DictationFailure, requestID: UUID) {
        guard state.dictation.requestID == requestID, state.dictation.phase.isActive else { return }
        if state.dictation.gesture == .candidate {
            // A normal Command combination must not leave a microphone/configuration error pill.
            firstAudioDeadline?.cancel()
            firstAudioDeadline = nil
            dictationCapture?.cancel()
            dictationCapture = nil
            dictationCredential = nil
            provisionalFailure = failure
            return
        }
        provisionalFailure = nil
        processingTask?.cancel()
        holdDeadline?.cancel()
        holdDeadline = nil
        processingTask = nil
        firstAudioDeadline?.cancel()
        firstAudioDeadline = nil
        dictationCapture?.cancel()
        dictationCapture = nil
        dictationCredential = nil
        state.dictation.phase = .failed
        state.dictation.failure = failure
        state.dictation.level = 0
        state.dictation.timing["failed"] = clock.now - dictationStartedAt
    }
}
