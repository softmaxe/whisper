import Foundation

public struct CleanupTestState: Equatable, Sendable {
    public var requestID: UUID?
    public var isRunning = false
    public var text = ""
    public var failure: CleanupFailure?
    public init() {}
}

extension WhisperApplication {
    public func cleanupCredential() throws -> String? {
        guard let account = state.settings.cleanupCredentialAccount else { return nil }
        guard let value = try credentials.read(account: account) else { throw ConfigurationError.credentialUnavailable }
        return value
    }

    func saveCleanup(_ configuration: CleanupConfiguration, credential: CredentialChange) {
        state.settingsSaved = false
        guard profileReadable else { state.configurationError = .incompatibleProfile; return }
        var createdAccount: String?
        do {
            var settings = state.settings
            settings.cleanup = try configuration.validated()
            switch credential {
            case .unchanged: break
            case let .replace(value):
                let value = value.trimmingCharacters(in: .whitespacesAndNewlines)
                if value.isEmpty { settings.cleanupCredentialAccount = nil }
                else {
                    let account = "cleanup-" + UUID().uuidString
                    try credentials.write(value, account: account)
                    createdAccount = account
                    settings.cleanupCredentialAccount = account
                }
            case .remove: settings.cleanupCredentialAccount = nil
            }
            let previous = state.settings.cleanupCredentialAccount
            try profileStore.saveSettings(settings)
            state.settings = settings
            state.settingsSaved = true
            state.configurationError = nil
            if let previous, previous != settings.cleanupCredentialAccount { try credentials.delete(account: previous) }
        } catch {
            if !state.settingsSaved, let createdAccount { try? credentials.delete(account: createdAccount) }
            state.configurationError = error as? ConfigurationError ?? .persistenceFailed
        }
    }

    func saveCleanupPrompt(_ prompt: String?) {
        var settings = state.settings
        settings.cleanup.customPrompt = prompt.flatMap { $0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : $0 }
        persist(settings)
    }

    func cleanupContext(language: String? = nil) -> CleanupContext {
        CleanupContext(language: language ?? state.settings.transcription.preferredLanguage,
                       vocabulary: dictionaryHintWords(), interfaceLanguage: state.settings.language)
    }

    /// History retry shares cleanup, but owns its result, conversion, and persistence.
    func processCleanup(_ text: String, context: CleanupContext, configuration override: CleanupConfiguration? = nil) async throws -> String {
        let configuration = override ?? state.settings.cleanup
        let credential: String?
        do { credential = try cleanupCredential() } catch { throw CleanupFailure.configuration }
        return try await Self.runCleanup(text, configuration: configuration, credential: credential,
                                         context: context, service: cleanup, clock: clock)
    }

    static func runCleanup(_ text: String, configuration: CleanupConfiguration, credential: String?,
                                   context: CleanupContext, service: any CleanupService, clock: any WorkflowClock) async throws -> String {
        for attempt in 0...3 {
            do {
                let deadline = CleanupDeadline(clock: clock, seconds: 30)
                return try await deadline.run {
                    try await service.clean(text: text, configuration: configuration, credential: credential, context: context)
                }
            } catch let failure as CleanupFailure where failure.mayRetry && attempt < 3 {
                try await waitForRetry(pow(2, Double(attempt)), clock: clock)
            }
        }
        throw CleanupFailure.network
    }

    private static func waitForRetry(_ seconds: TimeInterval, clock: any WorkflowClock) async throws {
        let wait = CleanupDeadline(clock: clock, seconds: seconds, deadlineResult: .success(""))
        _ = try await wait.run(operation: nil)
    }

    // Build the operation synchronously so no application instance spans the server await.
    func cleanDictation(rawText: String, requestID: UUID, context suppliedContext: CleanupContext? = nil,
                       preferences suppliedPreferences: TranscriptionPreferences? = nil) -> @MainActor () async -> Void {
        guard isCurrentDictation(requestID) else { return {} }
        let configuration = state.settings.cleanup
        let context = suppliedContext ?? cleanupContext()
        let preferences = suppliedPreferences ?? state.settings.transcription
        let service = cleanup
        let clock = clock
        let shouldClean = configuration.enabled && !configuration.serverURL.isEmpty
        let credential: Result<String?, any Error> = shouldClean ? Result { try cleanupCredential() } : .success(nil)
        if shouldClean {
            markDictationStage("cleanupDispatch", requestID: requestID)
            state.dictation.isCleaning = true
        }
        return { [weak self] in
            guard self?.isCurrentDictation(requestID) == true else { return }
            var text = rawText
            if shouldClean {
                do {
                    guard case let .success(secret) = credential else { throw CleanupFailure.configuration }
                    text = try await Self.runCleanup(rawText, configuration: configuration, credential: secret,
                                                     context: context, service: service, clock: clock)
                } catch is CancellationError { return }
                catch {
                    guard self?.isCurrentDictation(requestID) == true else { return }
                    self?.state.dictation.cleanupFailure = error as? CleanupFailure ?? .network
                }
            }
            guard self?.isCurrentDictation(requestID) == true, !Task.isCancelled else { return }
            self?.state.dictation.isCleaning = false
            let finish = self?.finishDictationText(rawText: rawText, text: text, requestID: requestID, preferences: preferences)
            await finish?()
        }
    }

    func testCleanupPrompt(text: String, prompt: String?) {
        cancelCleanupTest()
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        let id = UUID()
        state.cleanupTest = CleanupTestState()
        state.cleanupTest.requestID = id
        state.cleanupTest.isRunning = true
        var configuration = state.settings.cleanup
        guard configuration.enabled else {
            state.cleanupTest.failure = .configuration
            state.cleanupTest.isRunning = false
            return
        }
        configuration.customPrompt = prompt
        var context = cleanupContext()
        context.systemPromptOverride = CleanupPrompts.systemPrompt(configuration: configuration, context: context)
        configuration.temperature = configuration.temperature ?? 0
        let testContext = context
        let testConfiguration = configuration
        let service = cleanup
        let clock = clock
        let credential = Result { try cleanupCredential() }
        cleanupTestTask = Task { [weak self] in
            do {
                guard case let .success(secret) = credential else { throw CleanupFailure.configuration }
                let result = try await Self.runCleanup(CleanupPrompts.wrap(text), configuration: testConfiguration,
                                                       credential: secret, context: testContext, service: service, clock: clock)
                guard !Task.isCancelled, self?.state.cleanupTest.requestID == id else { return }
                self?.state.cleanupTest.text = result
            } catch is CancellationError { return }
            catch {
                guard self?.state.cleanupTest.requestID == id else { return }
                self?.state.cleanupTest.failure = error as? CleanupFailure ?? .network
            }
            self?.state.cleanupTest.isRunning = false
            self?.cleanupTestTask = nil
        }
    }

    func cancelCleanupTest() {
        cleanupTestTask?.cancel()
        cleanupTestTask = nil
        state.cleanupTest.requestID = nil
        state.cleanupTest.isRunning = false
    }
}

/// A cancelled or timed-out request releases its waiter even if a server adapter replies late.
@MainActor private final class CleanupDeadline {
    let clock: any WorkflowClock
    let seconds: TimeInterval
    let deadlineResult: Result<String, any Error>
    var continuation: CheckedContinuation<String, any Error>?
    var task: Task<Void, Never>?
    var timer: (any ScheduledAction)?
    init(clock: any WorkflowClock, seconds: TimeInterval, deadlineResult: Result<String, any Error> = .failure(CleanupFailure.timeout)) {
        self.clock = clock
        self.seconds = seconds
        self.deadlineResult = deadlineResult
    }
    func run(operation: (@Sendable () async throws -> String)?) async throws -> String {
        try Task.checkCancellation()
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                self.continuation = continuation
                timer = clock.schedule(after: seconds) { [weak self] in
                    guard let self else { return }
                    finish(deadlineResult)
                }
                if let operation {
                    task = Task { [weak self] in
                        do { let value = try await operation(); self?.finish(.success(value)) }
                        catch { self?.finish(.failure(error)) }
                    }
                }
            }
        } onCancel: { Task { @MainActor in self.finish(.failure(CancellationError())) } }
    }
    func finish(_ result: Result<String, any Error>) {
        guard let continuation else { return }
        self.continuation = nil
        timer?.cancel()
        timer = nil
        task?.cancel()
        task = nil
        continuation.resume(with: result)
    }
}
