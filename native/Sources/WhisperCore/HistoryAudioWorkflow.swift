import Foundation

extension WhisperApplication {
    func setHistoryRetention(_ preferences: HistoryPreferences) {
        var settings = state.settings
        settings.history = preferences
        persist(settings)
        if state.settingsSaved { runHistoryRetention() }
    }

    func startHistoryRetention() {
        guard profileReadable else { return }
        // Persisted preferences have already loaded; never sweep using provisional defaults.
        let database = profileStore.profile.directory.appendingPathComponent("history.sqlite")
        if FileManager.default.fileExists(atPath: database.path) { runHistoryRetention() }
        scheduleHistoryRetention()
    }

    private func scheduleHistoryRetention() {
        guard !state.isTerminating else { return }
        retentionTimer = clock.schedule(after: 6 * 3600) { [weak self] in
            self?.runHistoryRetention()
            self?.scheduleHistoryRetention()
        }
    }

    func runHistoryRetention() {
        guard !state.isTerminating else { return }
        guard profileReadable else { return }
        enqueueHistory(.expire(state.settings.history, clock.wallDate))
    }

    func preserveEndedRecording(_ status: HistoryStatus, failure: DictationFailure? = nil, rejected: Bool = false) {
        let preferences = state.settings.history
        guard !rejected, preferences.enabled, let capture = dictationCapture,
              capture.duration > 0, let id = state.dictation.requestID,
              let occurredAt = state.dictation.occurredAt else { return }
        if status == .discarded {
            guard preferences.saveDiscarded, preferences.audioRetentionDays > 0, capture.duration >= 1 else { return }
        }
        let entry = HistoryEntry(id: id, text: "", occurredAt: occurredAt, createdAt: clock.wallDate,
            localDate: state.dictation.localDate, status: status, model: dictationConfiguration?.model ?? "",
            audioDuration: capture.duration, errorCode: failure?.historyCode,
            errorMessage: failure?.message(in: state.settings.language))
        enqueueHistory(.recording(entry, capture.finishOperation(), preferences.audioRetentionDays > 0))
    }

    func stopHistoryPlayback() {
        historyAudioTask?.cancel()
        historyAudioTask = nil
        historyAudioGeneration += 1
        historyAudioEntryID = nil
        audioSystem.stop()
        state.history.playingID = nil
    }

    func useHistoryAudio(_ id: UUID, reveal: Bool) {
        stopHistoryPlayback()
        state.history.audioFailure = nil
        historyAudioEntryID = id
        let generation = historyAudioGeneration
        historyAudioTask = Task { [weak self, historyStore] in
            do {
                guard let url = try await historyStore.retainedAudioURL(for: id) else { throw HistoryAudioFailure.missing }
                guard !Task.isCancelled, let self, self.historyAudioGeneration == generation else { return }
                if reveal { self.audioSystem.reveal(url) }
                else if self.audioSystem.play(url, completion: { [weak self] in
                    guard self?.historyAudioGeneration == generation else { return }
                    self?.state.history.playingID = nil
                    self?.historyAudioEntryID = nil
                }) { self.state.history.playingID = id }
                else { self.state.history.audioFailure = .playback }
            } catch {
                guard !Task.isCancelled, self?.historyAudioGeneration == generation else { return }
                self?.state.history.audioFailure = error as? HistoryAudioFailure ?? .missing
                self?.refreshHistory()
            }
        }
    }

    func cancelHistoryRetry() {
        historyRetryOwnership?.cancel()
        historyRetryOwnership = nil
        historyRetryTask?.cancel()
        historyRetryTask = nil
        state.history.retry.isRunning = false
    }

    func retryHistory(_ id: UUID) {
        cancelHistoryRetry()
        let ownership = HistoryRetryOwnership()
        historyRetryOwnership = ownership
        state.history.retry = HistoryRetryState()
        state.history.retry.entryID = id
        state.history.retry.isRunning = true
        let configuration: ASRConfiguration
        let credential: String?
        do { configuration = try state.settings.asr.validated(); credential = try asrCredential() }
        catch { state.history.retry.isRunning = false; state.history.retry.failure = .retry; return }
        let preferences = state.settings.transcription
        // The retained self-hosted retry contract sends language but no Dictionary or Chinese prompt bias.
        let language = preferences.preferredLanguage == "auto" ? nil : preferences.preferredLanguage.split(separator: "-").first.map(String.init)
        let options = TranscriptionOptions(language: language, prompt: nil)
        let cleanupConfiguration = state.settings.cleanup
        let context = cleanupContext(language: preferences.preferredLanguage)
        let shouldClean = cleanupConfiguration.enabled && !cleanupConfiguration.serverURL.isEmpty
        let cleanupCredential: Result<String?, any Error> = shouldClean ? Result { try self.cleanupCredential() } : .success(nil)
        let transcriber = transcriber
        let cleanup = cleanup
        let clock = clock
        let current: @MainActor @Sendable () -> Bool = { [weak self] in
            self?.historyRetryOwnership === ownership && ownership.isActive
        }
        historyRetryTask = Task { [weak self, historyStore] in
            do {
                let input = try await historyStore.prepareRetry(id)
                defer { withExtendedLifetime(input.audio) {} }
                guard current(), let expectedAudio = input.entry.audioFileName else { return }
                let raw = try await transcriber.transcribe(file: input.audio.url, configuration: configuration,
                    credential: credential, options: options)
                try Task.checkCancellation()
                guard current() else { return }
                var text = raw
                if shouldClean {
                    do {
                        guard case let .success(secret) = cleanupCredential else { throw CleanupFailure.configuration }
                        text = try await Self.runCleanup(raw, configuration: cleanupConfiguration, credential: secret,
                                                       context: context, service: cleanup, clock: clock)
                    } catch is CancellationError { return }
                    catch {
                        guard current() else { return }
                        self?.state.history.retry.cleanupFailure = error as? CleanupFailure ?? .network
                    }
                }
                guard current(), !Task.isCancelled else { return }
                do { text = try await ChineseScriptConverter.shared.convert(text, preferences: preferences) }
                catch is CancellationError { return }
                catch {
                    guard current() else { return }
                    self?.state.history.retry.chineseConversionFailed = true
                }
                guard current(), !Task.isCancelled else { return }
                var entry = input.entry
                entry.rawText = raw; entry.text = text; entry.status = .completed
                entry.model = configuration.model; entry.provider = "self-hosted"
                entry.errorCode = nil; entry.errorMessage = nil
                // No Snippets or automatic delivery belong to retry. The actor only updates an existing row.
                let write = self?.enqueueHistory(.retry(entry, expectedAudio, ownership))
                let saved = try await write?.value.get() ?? false
                guard current() else { return }
                if saved { self?.finishHistoryRetry(entry) }
                else { throw HistoryAudioFailure.removed }
            } catch is CancellationError { return }
            catch {
                guard current() else { return }
                self?.state.history.retry.failure = error as? HistoryAudioFailure ?? .retry
                self?.state.history.retry.isRunning = false
                self?.historyRetryTask = nil
                self?.refreshHistory()
            }
        }
    }

    func finishHistoryRetry(_ entry: HistoryEntry) {
        // Persistence and recovered Insights have already been enqueued in the History write tail.
        state.history.retry.isRunning = false
        state.history.retry.failure = nil
        historyRetryTask = nil
    }
}

private extension DictationFailure {
    var historyCode: String {
        switch self {
        case .dictionaryEcho: "DICTIONARY_ECHO"
        case .noAudio: "NO_AUDIO"
        case let .service(status): "HTTP_\(status)"
        case .network: "NETWORK_ERROR"
        case .emptyTranscript: "EMPTY_TRANSCRIPT"
        case .invalidResponse: "INVALID_RESPONSE"
        case .inputUnavailable: "INPUT_UNAVAILABLE"
        case .captureFailed: "CAPTURE_FAILED"
        case .configuration: "CONFIGURATION_ERROR"
        case .permissionDenied: "PERMISSION_DENIED"
        case .storageFailed: "STORAGE_ERROR"
        }
    }
}
