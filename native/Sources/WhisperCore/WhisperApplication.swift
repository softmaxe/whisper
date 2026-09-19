import Foundation
import Observation

public enum AppCommand {
    case saveCleanup(CleanupConfiguration, credential: CredentialChange)
    case saveCleanupPrompt(String?)
    case testCleanupPrompt(text: String, prompt: String?), cancelCleanupPromptTest, resetCleanupPrompt
    case copyRawDictationResult
    case setAutoLearnCorrections(Bool), undoLearnedCorrections, dismissLearnedCorrections
    case saveASR(ASRConfiguration, credential: CredentialChange)
    case setLanguage(AppLanguage)
    case setTranscriptionLanguage(String)
    case setChineseScriptPreference(ChineseScriptPreference)
    case startDictation, stopDictation, cancelDictation, copyDictationResult
    case setMicrophone(MicrophonePreference), refreshMicrophones
    case importDictionary(String)
    case changeDictionary(add: [String], remove: [String], source: DictionarySource = .manual)
    case editDictionaryWord(String, replacement: String)
    case refreshDictionary, dismissDictionaryMessage
    case exportDictionary(URL)
    case shortcut(ShortcutInput)
    case setClipboardPreferences(autoPaste: Bool, keepResult: Bool)
    case setShortcutAvailable(Bool)
    case resetShortcutInput(Set<UInt16>)
    case saveSnippet(trigger: String, replacement: String, editingID: UUID? = nil)
    case setSnippets([Snippet]), deleteSnippet(UUID), refreshSnippets, dismissSnippetsMessage
    case setHistoryEnabled(Bool), loadHistory, loadMoreHistory, showDiscardedHistory(Bool)
    case saveHistory(HistoryEntry), deleteHistory(UUID), clearHistory
    case searchHistory(String), moveHistorySearchSelection(Int), selectHistoryEntry(UUID), dismissHistoryEntry
    case copyHistory(UUID, HistoryTextVersion)
    case saveDesktopPreferences(DesktopPreferences)
    case bootstrapDesktop(launchedAtLogin: Bool)
    case showMainWindow, closeMainWindow, dismissPillFeedback
    case recordingPillAction
    case copyRecoveryPresented(UUID, Bool), copyRecoveryHeld(UUID, Bool)
    case updatePillGeometry(size: CGSize, currentFrame: CGRect?)
    case setLaunchAtLogin(Bool), refreshLoginItemStatus, openLoginItemsSettings
    case saveShortcuts([String])
    case beginShortcutCapture(index: Int?), endShortcutCapture
    case setShortcutWarning(ShortcutConfigurationError?)
    case setHistoryRetention(HistoryPreferences), runHistoryRetention, clearHistoryAudio
    case retryHistory(UUID), cancelHistoryRetry, playHistory(UUID), stopHistoryPlayback, revealHistoryAudio(UUID)
    case dismissMessage
}

public struct ApplicationState: Equatable, Sendable {
    public var isTerminating = false
    public var cleanupTest = CleanupTestState()
    public var cleanupCredentialConfigured: Bool { settings.cleanupCredentialAccount != nil }
    public var dictation = DictationState()
    public var microphoneInputs = MicrophoneSnapshot()
    public var microphoneFailure: DictationFailure?
    public var corrections = CorrectionLearningState()
    public var dictionary = DictionaryState()
    public var snippets = SnippetsState()
    public var history = HistoryState()
    public var desktop = DesktopState()
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
    @ObservationIgnored var profileReadable = true
    @ObservationIgnored let historyStore: HistoryStore
    @ObservationIgnored var historyReadTask: Task<Void, Never>?
    @ObservationIgnored var historySearchTask: Task<Void, Never>?
    @ObservationIgnored var historyWriteTask: Task<Result<Bool, HistoryFailure>, Never>?
    @ObservationIgnored var historyReadGeneration = 0
    @ObservationIgnored var historySearchGeneration = 0
    @ObservationIgnored let audioSystem: any HistoryAudioSystem
    @ObservationIgnored var historyAudioTask: Task<Void, Never>?
    @ObservationIgnored var historyAudioGeneration = 0
    @ObservationIgnored var historyAudioEntryID: UUID?
    @ObservationIgnored var historyRetryTask: Task<Void, Never>?
    @ObservationIgnored var historyRetryOwnership: HistoryRetryOwnership?
    @ObservationIgnored var retentionTimer: (any ScheduledAction)?

    @ObservationIgnored let cleanup: any CleanupService
    @ObservationIgnored var cleanupTestTask: Task<Void, Never>?
    @ObservationIgnored let desktopEffects: any DesktopEffects
    @ObservationIgnored let mediaOwnership: MediaOwnership
    @ObservationIgnored var desktopBootstrapped = false
    @ObservationIgnored var readinessFeedbackOwner: UUID?
    @ObservationIgnored var pillFeedbackDeadline: (any ScheduledAction)?
    @ObservationIgnored var copyRecoveryDeadline: (any ScheduledAction)?
    @ObservationIgnored let pillDisplays: any PillDisplaySystem
    @ObservationIgnored var pillGeometryTask: Task<Void, Never>?
    @ObservationIgnored var pillGeometryDeadline: (any ScheduledAction)?
    @ObservationIgnored var pillGeometryGeneration = 0
    @ObservationIgnored var pillDisplayTarget: PasteTarget?
    @ObservationIgnored var pillResolvedRequestID: UUID?
    @ObservationIgnored var pillHasResolvedRequest = false
    @ObservationIgnored var pillGeometryPlacement: PillPlacement?

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
    @ObservationIgnored var correctionLearning: CorrectionLearning!
    @ObservationIgnored let pasteSystem: any AutomaticPasteSystem
    @ObservationIgnored let automaticPaste: AutomaticPaste
    @ObservationIgnored var dictationTarget: PasteTarget?
    @ObservationIgnored var pressedKeys = Set<UInt16>()
    @ObservationIgnored var holdDeadline: (any ScheduledAction)?
    @ObservationIgnored var doubleTapDeadline: (any ScheduledAction)?
    @ObservationIgnored var gesturePressedAt: TimeInterval = 0
    @ObservationIgnored var firstTapReleasedAt: TimeInterval = 0
    @ObservationIgnored var provisionalFailure: DictationFailure?
    @ObservationIgnored var snippetExpansion = SnippetExpansion(snippets: [])
    @ObservationIgnored var activeShortcut: ShortcutBinding?
    @ObservationIgnored var activeShortcutCandidates: [ShortcutBinding] = []
    @ObservationIgnored var activeShortcutKey: UInt16?
    @ObservationIgnored var shortcutPressActive = false
    @ObservationIgnored var captureModifierPeak = Set<UInt16>()

    public init(
        profile: NativeProfile, credentials: any CredentialStore = KeychainCredentialStore(),
        microphones: (any MicrophoneProvider)? = nil,
        transcriber: any TranscriptionService = SelfHostedTranscriber(),
        clock: (any WorkflowClock)? = nil,
        clipboard: (any TextClipboard)? = nil,
        pasteSystem: (any AutomaticPasteSystem)? = nil,
        cleanup: any CleanupService = SelfHostedCleanup(),
        correctionSystem: (any CorrectionMonitoringSystem)? = nil,
        desktopEffects: (any DesktopEffects)? = nil,
        audioSystem: (any HistoryAudioSystem)? = nil,
        pillDisplays: (any PillDisplaySystem)? = nil
    ) {
        self.cleanup = cleanup
        self.pillDisplays = pillDisplays ?? InertPillDisplaySystem()
        self.audioSystem = audioSystem ?? NativeHistoryAudioSystem()
        let effects = desktopEffects ?? InertDesktopEffects()
        self.desktopEffects = effects
        self.mediaOwnership = MediaOwnership(effects: effects)
        self.microphones = microphones ?? NativeMicrophoneProvider()
        self.transcriber = transcriber
        self.clock = clock ?? SystemWorkflowClock()
        self.clipboard = clipboard ?? SystemTextClipboard()
        let pasteSystem = pasteSystem ?? NativeAutomaticPasteSystem()
        self.pasteSystem = pasteSystem
        self.automaticPaste = AutomaticPaste(system: pasteSystem, clock: self.clock)
        self.profileStore = ProfileStore(profile: profile)
        self.historyStore = HistoryStore(profile: profile)
        self.credentials = credentials
        do {
            self.state = ApplicationState(settings: try profileStore.loadSettings())
        } catch {
            self.state = ApplicationState(configurationError: .incompatibleProfile)
            self.profileReadable = false
        }
        loadDictionary()
        loadSnippets()
        correctionLearning = CorrectionLearning(system: correctionSystem ?? NativeCorrectionMonitoringSystem(), clock: self.clock) { [weak self] words in
            self?.saveLearnedCorrections(words)
        }
        startHistoryRetention()
    }

    isolated deinit {
        pillGeometryTask?.cancel()
        pillGeometryDeadline?.cancel()
        copyRecoveryDeadline?.cancel()
        historyRetryOwnership?.cancel()
        historyRetryTask?.cancel()
        historyAudioTask?.cancel()
        retentionTimer?.cancel()
        audioSystem.stop()
        cleanupTestTask?.cancel()
        mediaOwnership.releaseAll()
        pillFeedbackDeadline?.cancel()
        dictationCapture?.cancel()
        processingTask?.cancel()
        historyReadTask?.cancel()
        historySearchTask?.cancel()
    }

    public func send(_ command: AppCommand) {
        guard !state.isTerminating else { return }
        switch command {
        case let .setAutoLearnCorrections(enabled): setAutoLearnCorrections(enabled)
        case .undoLearnedCorrections: undoLearnedCorrections()
        case .dismissLearnedCorrections: state.corrections = CorrectionLearningState()
        case let .setMicrophone(preference): setMicrophone(preference)
        case .refreshMicrophones: refreshMicrophones()
        case let .saveSnippet(trigger, replacement, editingID): saveSnippet(trigger: trigger, replacement: replacement, editingID: editingID)
        case let .setSnippets(snippets): setSnippets(snippets)
        case let .deleteSnippet(id): deleteSnippet(id)
        case .refreshSnippets: loadSnippets()
        case .dismissSnippetsMessage:
            state.snippets.failure = nil
            state.snippets.saved = false
        case let .importDictionary(text): importDictionary(text)
        case let .changeDictionary(add, remove, source): changeDictionary(add: add, remove: remove, source: source)
        case let .editDictionaryWord(original, replacement): editDictionaryWord(original, replacement: replacement)
        case .refreshDictionary: loadDictionary()
        case let .exportDictionary(destination): exportDictionary(to: destination)
        case .dismissDictionaryMessage:
            state.dictionary.failure = nil
            state.dictionary.addedCount = nil
            state.dictionary.exportCompleted = false
        case let .saveDesktopPreferences(preferences): saveDesktopPreferences(preferences)
        case let .bootstrapDesktop(launchedAtLogin): bootstrapDesktop(launchedAtLogin: launchedAtLogin)
        case .showMainWindow: state.desktop.mainWindowVisible = true
        case .closeMainWindow:
            state.shortcutCapture.isActive = false
            state.desktop.mainWindowVisible = false
        case .dismissPillFeedback:
            resetDesktopFeedback()
            state.desktop.dismissedRequestID = state.dictation.requestID
            if case .recovery = state.dictation.delivery { state.dictation.delivery = .none }
        case .recordingPillAction: recordingPillAction()
        case let .copyRecoveryPresented(revision, presented): setCopyRecoveryPresented(revision, presented)
        case let .copyRecoveryHeld(revision, held): setCopyRecoveryHeld(revision, held)
        case let .updatePillGeometry(size, frame): updatePillGeometry(size: size, currentFrame: frame)
        case let .setLaunchAtLogin(enabled): setLaunchAtLogin(enabled)
        case .refreshLoginItemStatus: state.desktop.loginItemStatus = desktopEffects.loginItemStatus()
        case .openLoginItemsSettings: desktopEffects.openLoginItemsSettings()
        case let .shortcut(input): receiveShortcut(input)
        case let .setShortcutAvailable(available): state.shortcutAvailable = available
        case let .resetShortcutInput(keys):
            cancelDictation()
            pressedKeys = keys
            shortcutPressActive = false
            activeShortcut = nil
            activeShortcutCandidates = []
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
        case let .setTranscriptionLanguage(code): setTranscriptionLanguage(code)
        case let .setChineseScriptPreference(preference): setChineseScriptPreference(preference)
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
            state.dictation.resultCopied = clipboard.writeResult(state.dictation.text)
            if !state.dictation.resultCopied {
                state.dictation.delivery = .recovery(copied: false)
                if let requestID = state.dictation.requestID { beginCopyRecovery(requestID: requestID) }
            } else if case .recovery = state.dictation.delivery {
                state.dictation.delivery = .recovery(copied: true)
            }
        case let .setHistoryEnabled(enabled): setHistoryEnabled(enabled)
        case .loadHistory: refreshHistory()
        case .loadMoreHistory: refreshHistory(more: true)
        case let .showDiscardedHistory(included):
            state.history.includeDiscarded = included
            refreshHistory()
            searchHistory(state.history.searchQuery)
        case let .saveHistory(entry): enqueueHistory(.save(entry))
        case let .deleteHistory(id):
            if state.history.retry.entryID == id { cancelHistoryRetry() }
            if historyAudioEntryID == id { stopHistoryPlayback() }
            enqueueHistory(.delete(id))
        case .clearHistory:
            cancelHistoryRetry(); stopHistoryPlayback()
            enqueueHistory(.clear(clock.wallDate))
        case let .searchHistory(query): searchHistory(query)
        case let .moveHistorySearchSelection(offset):
            state.history.searchSelection = max(0, min(state.history.searchResults.count - 1, state.history.searchSelection + offset))
        case let .selectHistoryEntry(id): state.history.selectedEntry = historyEntry(id)
        case .dismissHistoryEntry: state.history.selectedEntry = nil
        case let .copyHistory(id, version): copyHistory(id, version: version)
        case let .setHistoryRetention(preferences): setHistoryRetention(preferences)
        case .runHistoryRetention: runHistoryRetention()
        case .clearHistoryAudio:
            cancelHistoryRetry(); stopHistoryPlayback()
            enqueueHistory(.clearAudio)
        case let .retryHistory(id): retryHistory(id)
        case .cancelHistoryRetry: cancelHistoryRetry()
        case let .playHistory(id): useHistoryAudio(id, reveal: false)
        case .stopHistoryPlayback: stopHistoryPlayback()
        case let .revealHistoryAudio(id): useHistoryAudio(id, reveal: true)
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
