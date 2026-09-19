import Foundation
import Observation

public enum AppCommand {
    case saveASR(ASRConfiguration, credential: CredentialChange)
    case setLanguage(AppLanguage)
    case startDictation, stopDictation, cancelDictation, copyDictationResult
    case shortcut(ShortcutInput)
    case setClipboardPreferences(autoPaste: Bool, keepResult: Bool)
    case setShortcutAvailable(Bool)
    case resetShortcutInput(Set<UInt16>)
    case saveShortcuts([String])
    case beginShortcutCapture(index: Int?), endShortcutCapture
    case setShortcutWarning(ShortcutConfigurationError?)
    case dismissMessage
}

public struct ApplicationState: Equatable, Sendable {
    public var dictation = DictationState()
    public var settings: AppSettings
    public var configurationError: ConfigurationError?
    public var settingsSaved = false
    public var shortcutAvailable = false
    public var shortcutError: ShortcutConfigurationError?
    public var shortcutWarning: ShortcutConfigurationError?
    public var shortcutCapture = ShortcutCaptureState()
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
    @ObservationIgnored private var profileReadable = true

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
    @ObservationIgnored let pasteSystem: any AutomaticPasteSystem
    @ObservationIgnored let automaticPaste: AutomaticPaste
    @ObservationIgnored var dictationTarget: PasteTarget?
    @ObservationIgnored var pressedKeys = Set<UInt16>()
    @ObservationIgnored var holdDeadline: (any ScheduledAction)?
    @ObservationIgnored var doubleTapDeadline: (any ScheduledAction)?
    @ObservationIgnored var gesturePressedAt: TimeInterval = 0
    @ObservationIgnored var firstTapReleasedAt: TimeInterval = 0
    @ObservationIgnored var provisionalFailure: DictationFailure?
    @ObservationIgnored var activeShortcut: ShortcutBinding?
    @ObservationIgnored var activeShortcutKey: UInt16?
    @ObservationIgnored var shortcutPressActive = false
    @ObservationIgnored var captureModifierPeak = Set<UInt16>()

    public init(
        profile: NativeProfile, credentials: any CredentialStore = KeychainCredentialStore(),
        microphones: (any MicrophoneProvider)? = nil,
        transcriber: any TranscriptionService = SelfHostedTranscriber(),
        clock: (any WorkflowClock)? = nil,
        clipboard: (any TextClipboard)? = nil,
        pasteSystem: (any AutomaticPasteSystem)? = nil
    ) {
        self.microphones = microphones ?? NativeMicrophoneProvider()
        self.transcriber = transcriber
        self.clock = clock ?? SystemWorkflowClock()
        self.clipboard = clipboard ?? SystemTextClipboard()
        let pasteSystem = pasteSystem ?? NativeAutomaticPasteSystem()
        self.pasteSystem = pasteSystem
        self.automaticPaste = AutomaticPaste(system: pasteSystem, clock: self.clock)
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
        dictationCapture?.cancel()
        processingTask?.cancel()
    }

    public func send(_ command: AppCommand) {
        switch command {
        case let .shortcut(input): receiveShortcut(input)
        case let .setShortcutAvailable(available): state.shortcutAvailable = available
        case let .resetShortcutInput(keys):
            cancelDictation()
            pressedKeys = keys
            shortcutPressActive = false
            activeShortcut = nil
        case let .saveShortcuts(values): saveShortcuts(values)
        case let .beginShortcutCapture(index):
            cancelDictation()
            state.shortcutCapture = ShortcutCaptureState(isActive: true, editingIndex: index)
            captureModifierPeak = []
            state.shortcutError = nil
        case .endShortcutCapture: state.shortcutCapture.isActive = false
        case let .setShortcutWarning(warning): state.shortcutWarning = warning
        case let .setClipboardPreferences(autoPaste, keepResult):
            var settings = state.settings
            settings.autoPasteEnabled = autoPaste
            settings.keepTranscriptionInClipboard = keepResult
            persist(settings)
        case .startDictation: startDictation()
        case .stopDictation: stopDictation()
        case .cancelDictation: cancelDictation()
        case .copyDictationResult:
            guard !state.dictation.text.isEmpty else { return }
            state.dictation.resultCopied = clipboard.writeResult(state.dictation.text)
            if !state.dictation.resultCopied { state.dictation.delivery = .recovery(copied: false) }
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
