import Foundation
import Observation

public enum AppCommand {
    case saveASR(ASRConfiguration, credential: CredentialChange)
    case setLanguage(AppLanguage)
    case startDictation, stopDictation, cancelDictation, copyDictationResult
    case setHistoryEnabled(Bool), loadHistory, loadMoreHistory, showDiscardedHistory(Bool)
    case saveHistory(HistoryEntry), deleteHistory(UUID), clearHistory
    case searchHistory(String), moveHistorySearchSelection(Int), selectHistoryEntry(UUID), dismissHistoryEntry
    case copyHistory(UUID, HistoryTextVersion)
    case dismissMessage
}

public struct ApplicationState: Equatable, Sendable {
    public var dictation = DictationState()
    public var history = HistoryState()
    public var settings: AppSettings
    public var configurationError: ConfigurationError?
    public var settingsSaved = false
    public var credentialConfigured: Bool { settings.asrCredentialAccount != nil }

    public init(settings: AppSettings = .init(), configurationError: ConfigurationError? = nil) {
        self.settings = settings
        self.configurationError = configurationError
    }
}

/// UI and native input adapters send commands here and observe the same state as workflow tests.
@MainActor @Observable
public final class WhisperApplication {
    public internal(set) var state: ApplicationState
    public let profileStore: ProfileStore
    @ObservationIgnored let credentials: any CredentialStore
    @ObservationIgnored var profileReadable = true
    @ObservationIgnored let historyStore: HistoryStore
    @ObservationIgnored var historyReadTask: Task<Void, Never>?
    @ObservationIgnored var historySearchTask: Task<Void, Never>?
    @ObservationIgnored var historyWriteTask: Task<Result<Bool, HistoryFailure>, Never>?
    @ObservationIgnored var historyReadGeneration = 0
    @ObservationIgnored var historySearchGeneration = 0

    @ObservationIgnored let microphones: any MicrophoneProvider
    @ObservationIgnored let transcriber: any TranscriptionService
    @ObservationIgnored let clock: any WorkflowClock
    @ObservationIgnored let clipboard: any TextClipboard
    @ObservationIgnored var dictationCapture: RecordingCapture?
    @ObservationIgnored var firstAudioDeadline: (any ScheduledAction)?
    @ObservationIgnored var processingTask: Task<Void, Never>?
    @ObservationIgnored var dictationStartedAt: TimeInterval = 0
    @ObservationIgnored var dictationConfiguration: ASRConfiguration?
    @ObservationIgnored var dictationCredential: String?

    public init(
        profile: NativeProfile, credentials: any CredentialStore = KeychainCredentialStore(),
        microphones: (any MicrophoneProvider)? = nil,
        transcriber: any TranscriptionService = SelfHostedTranscriber(),
        clock: (any WorkflowClock)? = nil,
        clipboard: (any TextClipboard)? = nil
    ) {
        self.microphones = microphones ?? NativeMicrophoneProvider()
        self.transcriber = transcriber
        self.clock = clock ?? SystemWorkflowClock()
        self.clipboard = clipboard ?? SystemTextClipboard()
        self.profileStore = ProfileStore(profile: profile)
        self.historyStore = HistoryStore(profile: profile)
        self.credentials = credentials
        do {
            self.state = ApplicationState(settings: try profileStore.loadSettings())
        } catch {
            self.state = ApplicationState(configurationError: .incompatibleProfile)
            self.profileReadable = false
        }
    }

    deinit {
        dictationCapture?.cancel()
        processingTask?.cancel()
        historyReadTask?.cancel()
        historySearchTask?.cancel()
    }

    public func send(_ command: AppCommand) {
        switch command {
        case .startDictation: startDictation()
        case .stopDictation: stopDictation()
        case .cancelDictation: cancelDictation()
        case .copyDictationResult:
            guard !state.dictation.text.isEmpty else { return }
            clipboard.write(state.dictation.text)
            state.dictation.resultCopied = true
        case let .setHistoryEnabled(enabled): setHistoryEnabled(enabled)
        case .loadHistory: refreshHistory()
        case .loadMoreHistory: refreshHistory(more: true)
        case let .showDiscardedHistory(included):
            state.history.includeDiscarded = included
            refreshHistory()
            searchHistory(state.history.searchQuery)
        case let .saveHistory(entry): enqueueHistory(.save(entry))
        case let .deleteHistory(id): enqueueHistory(.delete(id))
        case .clearHistory: enqueueHistory(.clear(clock.wallDate))
        case let .searchHistory(query): searchHistory(query)
        case let .moveHistorySearchSelection(offset):
            state.history.searchSelection = max(0, min(state.history.searchResults.count - 1, state.history.searchSelection + offset))
        case let .selectHistoryEntry(id): state.history.selectedEntry = historyEntry(id)
        case .dismissHistoryEntry: state.history.selectedEntry = nil
        case let .copyHistory(id, version): copyHistory(id, version: version)
        case let .saveASR(configuration, credential):
            saveASR(configuration, credential: credential)
        case let .setLanguage(language):
            var settings = state.settings
            settings.language = language
            persist(settings)
        case .dismissMessage:
            state.configurationError = nil
            state.settingsSaved = false
        }
    }

    /// Processing resolves the secret only when needed; observable state never contains it.
    public func asrCredential() throws -> String? {
        guard let account = state.settings.asrCredentialAccount else { return nil }
        guard let value = try credentials.read(account: account) else {
            throw ConfigurationError.credentialUnavailable
        }
        return value
    }

    private func saveASR(_ configuration: ASRConfiguration, credential: CredentialChange) {
        state.settingsSaved = false
        guard profileReadable else {
            state.configurationError = .incompatibleProfile
            return
        }
        var createdAccount: String?
        do {
            var settings = state.settings
            settings.asr = try configuration.validated()
            switch credential {
            case .unchanged: break
            case let .replace(value):
                if value.isEmpty {
                    settings.asrCredentialAccount = nil
                } else {
                    let account = "asr-" + UUID().uuidString
                    try credentials.write(value, account: account)
                    createdAccount = account
                    settings.asrCredentialAccount = account
                }
            case .remove:
                settings.asrCredentialAccount = nil
            }
            let previousAccount = state.settings.asrCredentialAccount
            try profileStore.saveSettings(settings)
            state.settings = settings
            state.settingsSaved = true
            state.configurationError = nil
            if let previousAccount, previousAccount != settings.asrCredentialAccount {
                // The new profile is committed before removing the old credential.
                try credentials.delete(account: previousAccount)
            }
        } catch {
            if !state.settingsSaved, let createdAccount { try? credentials.delete(account: createdAccount) }
            state.configurationError = error as? ConfigurationError ?? .persistenceFailed
        }
    }

    func persist(_ settings: AppSettings) {
        guard profileReadable else { state.configurationError = .incompatibleProfile; return }
        do {
            try profileStore.saveSettings(settings)
            state.settings = settings
            state.configurationError = nil
            state.settingsSaved = true
        } catch {
            state.configurationError = .persistenceFailed
            state.settingsSaved = false
        }
    }
}
