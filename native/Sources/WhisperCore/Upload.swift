import Foundation

public enum UploadPhase: String, Equatable, Sendable {
    case idle, selected, preparing, transcribing, complete, failed
    public var isActive: Bool { self == .preparing || self == .transcribing }
}

public enum UploadFailure: Error, Equatable, Sendable {
    case unsupportedFormat, unreadableFile, configuration, credentialUnavailable, temporaryStorage, converterUnavailable, conversionFailed
    case transcription(DictationFailure)
    public func message(in language: AppLanguage) -> String {
        switch self {
        case .unsupportedFormat: language.text("Choose a supported audio or video file.", "请选择支持的音频或视频文件。")
        case .unreadableFile: language.text("The file could not be read or is empty. Choose another file or check its permissions.", "无法读取文件或文件为空。请选择其他文件或检查访问权限。")
        case .configuration: language.text("Configure your Speech-to-Text server and model before uploading.", "请先配置语音转文字服务器和模型，然后上传。")
        case .credentialUnavailable: ConfigurationError.credentialUnavailable.message(in: language)
        case .temporaryStorage: language.text("The audio could not be prepared. Check available disk space and try again.", "无法准备音频。请检查可用磁盘空间，然后重试。")
        case .converterUnavailable: language.text("The audio converter is missing or could not start. Reinstall Whisper and try again.", "音频转换器缺失或无法启动。请重新安装 Whisper 后重试。")
        case .conversionFailed: language.text("This file could not be converted to audio. Check that it contains a supported audio track.", "无法将此文件转换为音频。请确认文件包含支持的音轨。")
        case .transcription(.emptyTranscript): language.text("No text was transcribed from this file. Try another file.", "没有从此文件识别到文字。请尝试其他文件。")
        case .transcription(.storageFailed): UploadFailure.temporaryStorage.message(in: language)
        case let .transcription(failure): failure.message(in: language)
        }
    }
}

public struct UploadState: Equatable, Sendable {
    public var phase: UploadPhase = .idle
    public var requestID: UUID?
    public var source: URL?
    public var fileName: String { source?.lastPathComponent ?? "" }
    public var sourceSize: Int64?
    public var model = ""
    public var text = ""
    public var failure: UploadFailure?
    public var historyID: UUID?
    public var historySaveFailed = false
    public var isSavingHistory = false
    public var resultCopied = false
    public var copyFailed = false
    public init() {}
}

struct UploadRunConfiguration: Sendable {
    let asr: ASRConfiguration
    let credential: String?
    let options: TranscriptionOptions
}

extension WhisperApplication {
    func uploadRunConfiguration() throws -> UploadRunConfiguration {
        let configuration = try state.settings.asr.validated()
        let credential = try asrCredential()
        let language = state.settings.transcription.preferredLanguage
        return UploadRunConfiguration(asr: configuration, credential: credential,
            options: TranscriptionOptions(language: language == "auto" ? nil : language.split(separator: "-").first.map(String.init), expectedStatus: 200))
    }

    func selectUpload(_ source: URL) {
        guard !uploadShutdown, !state.upload.phase.isActive, state.batchUpload.items.isEmpty else { return }
        state.upload = UploadState()
        state.upload.source = source
        guard UploadFormats.accepts(source) else {
            state.upload.phase = .failed
            state.upload.failure = .unsupportedFormat
            return
        }
        state.upload.phase = .selected
    }

    func startUpload() {
        guard !uploadShutdown, !state.upload.phase.isActive, !state.batchUpload.isProcessing, let source = state.upload.source else { return }
        let snapshot: UploadRunConfiguration
        do { snapshot = try uploadRunConfiguration() }
        catch {
            state.upload.phase = .failed
            state.upload.failure = error as? ConfigurationError == .credentialUnavailable ? .credentialUnavailable : .configuration
            return
        }
        let id = UUID()
        state.upload.requestID = id
        state.upload.model = snapshot.asr.model
        state.upload.phase = .preparing
        state.upload.text = ""
        state.upload.failure = nil
        state.upload.historyID = nil
        state.upload.historySaveFailed = false
        state.upload.isSavingHistory = false
        state.upload.resultCopied = false
        state.upload.copyFailed = false
        let converter = uploadConverter
        let transcriber = self.transcriber
        let task = Task { [weak self] in
            defer { self?.uploadTasks.removeValue(forKey: id) }
            do {
                let text = try await UploadFileProcessing.transcribe(source, configuration: snapshot, converter: converter, transcriber: transcriber) { [weak self] size in
                    guard self?.isCurrentUpload(id) == true else { return false }
                    self?.state.upload.sourceSize = size
                    self?.state.upload.phase = .transcribing
                    return true
                }
                try Task.checkCancellation()
                guard self?.isCurrentUpload(id) == true else { return }
                self?.state.upload.text = text
                self?.state.upload.phase = .complete
                self?.state.upload.isSavingHistory = true
                let completedAt = self?.clock.wallDate ?? Date()
                let entry = HistoryEntry(id: id, text: text, rawText: text, occurredAt: completedAt,
                    createdAt: completedAt, source: .upload, model: snapshot.asr.model)
                do {
                    let saved = try await self?.recordHistory(entry) ?? false
                    guard self?.state.upload.requestID == id else { return }
                    self?.state.upload.historyID = saved ? id : nil
                } catch {
                    guard self?.state.upload.requestID == id else { return }
                    self?.state.upload.historySaveFailed = true
                }
                guard self?.state.upload.requestID == id else { return }
                self?.state.upload.isSavingHistory = false
            } catch is CancellationError {
                if self?.isCurrentUpload(id) == true, !Task.isCancelled {
                    self?.state.upload.phase = .failed
                    self?.state.upload.failure = .transcription(.network)
                }
            } catch {
                guard self?.isCurrentUpload(id) == true else { return }
                self?.state.upload.phase = .failed
                self?.state.upload.failure = error as? UploadFailure ?? .transcription(error as? DictationFailure ?? .network)
            }
        }
        uploadTasks[id] = task
    }

    func isCurrentUpload(_ id: UUID) -> Bool { state.upload.requestID == id && state.upload.phase.isActive }

    func cancelUpload() {
        guard state.upload.phase.isActive else { return }
        if let id = state.upload.requestID { uploadTasks[id]?.cancel() }
        state.upload = UploadState()
    }

    func resetUpload() {
        cancelUpload()
        state.upload = UploadState()
    }

    /// Termination waits for active and previously cancelled jobs to release their private files.
    public func cancelUploadsAndWait() async {
        uploadShutdown = true
        let pending = Array(uploadTasks.values)
        cancelUpload()
        cancelUploadBatch()
        for task in pending { task.cancel() }
        for task in pending { await task.value }
        await flushHistoryWrites()
    }
}
