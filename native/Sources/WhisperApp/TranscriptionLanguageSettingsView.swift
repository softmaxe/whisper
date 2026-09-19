import SwiftUI
import WhisperCore

struct TranscriptionLanguageSettingsView: View {
    let application: WhisperApplication
    @State private var search = ""
    private var language: AppLanguage { application.state.settings.language }
    private var preferences: TranscriptionPreferences { application.state.settings.transcription }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            VStack(alignment: .leading, spacing: 5) {
                Text(language.text("Transcription language", "转录语言")).fontWeight(.medium)
                Text(language.text("Choose the language you speak for more accurate transcription", "选择你说话的语言，以提高转录准确率"))
                    .font(.caption).foregroundStyle(.secondary)
            }
            HStack {
                TextField(language.text("Search languages", "搜索语言"), text: $search)
                    .textFieldStyle(.roundedBorder).frame(maxWidth: 210)
                    .accessibilityIdentifier("transcription-language-search")
                Picker(language.text("Transcription language", "转录语言"), selection: Binding(
                    get: { preferences.preferredLanguage },
                    set: { application.send(.setTranscriptionLanguage($0)) }
                )) {
                    ForEach(TranscriptionLanguage.all.filter {
                        $0.code == preferences.preferredLanguage || search.isEmpty
                            || $0.title(in: language).localizedCaseInsensitiveContains(search)
                            || $0.code.localizedCaseInsensitiveContains(search)
                    }) { item in Text(item.title(in: language)).tag(item.code) }
                }
                .labelsHidden().frame(width: 190)
                .accessibilityIdentifier("transcription-language")
            }
            if preferences.preferredLanguage == "auto" {
                Divider()
                VStack(alignment: .leading, spacing: 8) {
                    Text(language.text("Chinese script (dictation)", "中文书写（听写）")).fontWeight(.medium)
                    Text(language.text(
                        "When Chinese is detected, convert dictated transcripts to Simplified or Traditional characters. Spoken Chinese is the same either way; Whisper only returns one script.",
                        "检测到中文时，将听写的转录结果转换为简体或繁体。口语听起来一样；Whisper 只会输出一种字形。"
                    ))
                    .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    Picker(language.text("Chinese script (dictation)", "中文书写（听写）"), selection: Binding(
                        get: { preferences.chineseScriptPreference },
                        set: { application.send(.setChineseScriptPreference($0)) }
                    )) {
                        ForEach(ChineseScriptPreference.allCases, id: \.self) { item in
                            Text(item.title(in: language)).tag(item)
                        }
                    }
                    .labelsHidden().frame(width: 210)
                    .accessibilityIdentifier("chinese-script-preference")
                }
            }
        }
        .padding(16).background(.primary.opacity(0.025))
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.primary.opacity(0.12)))
    }
}
