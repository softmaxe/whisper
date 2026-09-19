import SwiftUI
import WhisperCore

struct GeneralSettingsView: View {
    let application: WhisperApplication
    private var language: AppLanguage { application.state.settings.language }
    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            DesktopSettingsView(application: application, groups: [.appearance, .sound])
            SettingsPanel(title: language.text("Clipboard", "剪贴板")) {
                Toggle(language.text("Automatic paste", "自动粘贴"), isOn: Binding(
                    get: { application.state.settings.autoPasteEnabled },
                    set: { application.send(.setClipboardPreferences(autoPaste: $0, keepResult: application.state.settings.keepTranscriptionInClipboard)) }
                )).accessibilityIdentifier("automatic-paste")
                Toggle(language.text("Keep transcription in clipboard", "在剪贴板中保留转录文字"), isOn: Binding(
                    get: { application.state.settings.keepTranscriptionInClipboard },
                    set: { application.send(.setClipboardPreferences(autoPaste: application.state.settings.autoPasteEnabled, keepResult: $0)) }
                )).accessibilityIdentifier("keep-transcription-clipboard")
            }
            DesktopSettingsView(application: application, groups: [.floating])
            SettingsPanel(title: language.text("Language", "语言")) {
                HStack {
                    VStack(alignment: .leading, spacing: 5) {
                        Text(language.text("Interface language", "界面语言"))
                        Text(language.text("Choose the language used throughout Whisper's interface", "选择 Whisper 界面使用的语言"))
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Picker(language.text("Interface language", "界面语言"), selection: Binding(
                        get: { language }, set: { application.send(.setLanguage($0)) }
                    )) {
                        ForEach(AppLanguage.allCases, id: \.self) { Text($0.displayName).tag($0) }
                    }.labelsHidden().frame(width: 145)
                }
                Divider()
                TranscriptionLanguageSettingsView(application: application, embedded: true)
            }
            DesktopSettingsView(application: application, groups: [.startup])
            MicrophoneSettingsView(application: application)
            SettingsPanel(title: language.text("Dictionary", "词典")) { CorrectionLearningSetting(application: application) }
            SettingsSaveFeedback(application: application)
        }
    }
}

struct SettingsSaveFeedback: View {
    let application: WhisperApplication
    var showSaved = true
    private var language: AppLanguage { application.state.settings.language }
    var body: some View {
        if let failure = application.state.configurationError {
            Label(failure.message(in: language), systemImage: "exclamationmark.triangle")
                .font(.caption).foregroundStyle(.red).accessibilityIdentifier("settings-error")
        } else if application.state.settingsSaved && showSaved {
            Label(language.text("Settings saved", "设置已保存"), systemImage: "checkmark.circle")
                .font(.caption).foregroundStyle(.secondary).accessibilityIdentifier("settings-saved")
        }
    }
}
