import Foundation
import Observation

public enum AppCommand {
    case saveCleanup(CleanupConfiguration, credential: CredentialChange)
    case saveCleanupPrompt(String?)
    case testCleanupPrompt(text: String, prompt: String?), cancelCleanupPromptTest, resetCleanupPrompt
    case copyRawDictationResult
    case saveASR(ASRConfiguration, credential: CredentialChange)
    case setLanguage(AppLanguage)
    case startDictation, stopDictation, cancelDictation, copyDictationResult
    case dismissMessage
}

public struct ApplicationState: Equatable, Sendable {
    public var cleanupTest = CleanupTestState()
    public var cleanupCredentialConfigured: Bool { settings.cleanupCredentialAccount != nil }
    public var dictation = DictationState()
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

    @ObservationIgnored let cleanup: any CleanupService
    @ObservationIgnored var cleanupTestTask: Task<Void, Never>?
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
        clipboard: (any TextClipboard)? = nil,
        cleanup: any CleanupService = SelfHostedCleanup()
    ) {
        self.cleanup = cleanup
        self.microphones = microphones ?? NativeMicrophoneProvider()
        self.transcriber = transcriber
        self.clock = clock ?? SystemWorkflowClock()
        self.clipboard = clipboard ?? SystemTextClipboard()
        self.profileStore = ProfileStore(profile: profile)
        self.credentials = credentials
        do {
            self.state = ApplicationState(settings: try profileStore.loadSettings())
        } catch {
            self.state = ApplicationState(configurationError: .incompatibleProfile)
            self.profileReadable = false
        }
    }

    deinit {
        cleanupTestTask?.cancel()
        dictationCapture?.cancel()
        processingTask?.cancel()
    }

    public func send(_ command: AppCommand) {
        switch command {
        case let .saveCleanup(configuration, credential): saveCleanup(configuration, credential: credential)
        case let .testCleanupPrompt(text, prompt): testCleanupPrompt(text: text, prompt: prompt)
        case .cancelCleanupPromptTest: cancelCleanupTest()
        case let .saveCleanupPrompt(prompt): saveCleanupPrompt(prompt)
        case .resetCleanupPrompt: saveCleanupPrompt(nil)
        case .copyRawDictationResult:
            guard !state.dictation.rawText.isEmpty else { return }
            clipboard.write(state.dictation.rawText)
        case .startDictation: startDictation()
        case .stopDictation: stopDictation()
        case .cancelDictation: cancelDictation()
        case .copyDictationResult:
            guard !state.dictation.text.isEmpty else { return }
            clipboard.write(state.dictation.text)
            state.dictation.resultCopied = true
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
