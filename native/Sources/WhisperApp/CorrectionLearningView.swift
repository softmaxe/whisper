import SwiftUI
import WhisperCore

struct CorrectionLearningSetting: View {
    let application: WhisperApplication
    private var language: AppLanguage { application.state.settings.language }
    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            Toggle(language.text("Learn from corrections", "从纠正中学习"), isOn: Binding(
                get: { application.state.settings.autoLearnCorrections },
                set: { application.send(.setAutoLearnCorrections($0)) }
            )).accessibilityIdentifier("auto-learn-corrections")
            Text(language.text("Add corrected words to your dictionary after automatic paste.", "自动粘贴后，将纠正过的词语加入词典。"))
                .font(.caption).foregroundStyle(.secondary)
        }
    }
}

struct CorrectionLearningFeedback: View {
    let application: WhisperApplication
    private var language: AppLanguage { application.state.settings.language }
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let failure = application.state.corrections.failure {
                Text(failure.message(in: language)).foregroundStyle(.red).font(.caption)
            }
            if !application.state.corrections.learned.isEmpty {
                Text(language.text("Learned words", "已学习词条")).fontWeight(.semibold)
                Text(application.state.corrections.learned.map(\.word).joined(separator: ", "))
                    .font(.caption).lineLimit(3).accessibilityIdentifier("learned-corrections")
                HStack {
                    Button(language.text("Undo", "撤销")) { application.send(.undoLearnedCorrections) }
                        .accessibilityIdentifier("undo-learned-corrections")
                    Spacer()
                    Button(language.text("Dismiss", "关闭")) { application.send(.dismissLearnedCorrections) }
                }
            }
        }
    }
}
