import Foundation

public enum MainPage: String, CaseIterable, Sendable {
    case home, insights, upload, dictionary
    public func title(in language: AppLanguage) -> String {
        switch self {
        case .home: language.text("Home", "首页")
        case .insights: language.text("Insights", "统计")
        case .upload: language.text("Upload", "上传")
        case .dictionary: language.text("Dictionary", "词典")
        }
    }
    public var icon: String {
        switch self { case .home: "house"; case .insights: "chart.bar"; case .upload: "square.and.arrow.up"; case .dictionary: "book" }
    }
}

public enum SettingsSection: String, CaseIterable, Sendable {
    case general, hotkeys, speechToText, textCleanup, privacy
    public func title(in language: AppLanguage) -> String {
        switch self {
        case .general: language.text("General", "通用")
        case .hotkeys: language.text("Hotkeys", "快捷键")
        case .speechToText: language.text("Speech-to-Text", "语音转文字")
        case .textCleanup: language.text("Text cleanup", "文本整理")
        case .privacy: language.text("Privacy & Data", "隐私与数据")
        }
    }
    public var icon: String {
        switch self { case .general: "slider.horizontal.3"; case .hotkeys: "keyboard"; case .speechToText: "mic"; case .textCleanup: "sparkles"; case .privacy: "lock.shield" }
    }
}

public struct NavigationState: Equatable, Sendable {
    public var page: MainPage = .home
    public var settingsPresented = false
    public var settingsSection: SettingsSection = .general
    public var searchPresented = false
    public var sidebarCollapsed = false
    public init() {}
}

/// Nonsecret drafts survive page and modal changes. Credential text stays in protected UI fields.
public struct SettingsDraftState: Equatable, Sendable {
    public var asr: ASRConfiguration
    public var cleanup: CleanupConfiguration
    public var cleanupPrompt: String
    public init(settings: AppSettings) {
        asr = settings.asr
        cleanup = settings.cleanup
        cleanupPrompt = settings.cleanup.customPrompt ?? CleanupPrompts.defaultText(in: settings.language)
    }
}

extension WhisperApplication {
    func navigate(to page: MainPage) {
        closeHistorySearch()
        state.navigation.page = page
        state.navigation.settingsPresented = false
        state.shortcutCapture.isActive = false
        switch page {
        case .home: refreshHistory()
        case .insights: refreshInsights()
        case .dictionary: loadDictionary(); loadSnippets()
        case .upload: break
        }
    }

    func openSettings(_ section: SettingsSection) {
        closeHistorySearch()
        state.navigation.settingsPresented = true
        state.navigation.settingsSection = section
        if section != .hotkeys { state.shortcutCapture.isActive = false }
        if state.settingsDraft == nil { state.settingsDraft = SettingsDraftState(settings: state.settings) }
        if section == .privacy { refreshPrivacy() }
    }

    func closeSettings() {
        state.navigation.settingsPresented = false
        state.shortcutCapture.isActive = false
    }

    func openHistorySearch() {
        guard !state.navigation.settingsPresented, !state.shortcutCapture.isActive else { return }
        state.navigation.searchPresented = true
        state.history.selectedEntry = nil
        searchHistory("")
    }

    func closeHistorySearch() {
        state.navigation.searchPresented = false
        state.history.selectedEntry = nil
        historySearchGeneration += 1
        historySearchTask?.cancel()
        state.history.isSearching = false
    }

    func saveASRDraft(_ credential: CredentialChange) {
        guard let draft = state.settingsDraft else { return }
        send(.saveASR(draft.asr, credential: credential))
        if state.settingsSaved { let saved = state.settings.asr; state.settingsDraft?.asr = saved }
    }

    func saveCleanupDraft(_ credential: CredentialChange) {
        guard let draft = state.settingsDraft else { return }
        send(.saveCleanup(draft.cleanup, credential: credential))
        if state.settingsSaved { let saved = state.settings.cleanup; state.settingsDraft?.cleanup = saved }
    }

    func saveCleanupPromptDraft() {
        guard let draft = state.settingsDraft else { return }
        let prompt = draft.cleanupPrompt == CleanupPrompts.defaultText(in: state.settings.language) ? nil : draft.cleanupPrompt
        send(.saveCleanupPrompt(prompt))
        if state.settingsSaved { let saved = state.settings.cleanup.customPrompt; state.settingsDraft?.cleanup.customPrompt = saved }
    }
}
