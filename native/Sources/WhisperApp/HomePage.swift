import SwiftUI
import WhisperCore

struct HomePage: View {
    let application: WhisperApplication
    private var language: AppLanguage { application.state.settings.language }
    private var dictation: DictationState { application.state.dictation }
    private var showCurrentResult: Bool {
        !dictation.text.isEmpty && (!application.state.settings.history.enabled
            || application.state.history.failure != nil || application.state.history.lastSavedID != dictation.requestID)
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            if !application.state.corrections.learned.isEmpty || application.state.corrections.failure != nil {
                CorrectionLearningFeedback(application: application)
            }
            if dictation.phase.isActive || dictation.failure != nil || dictation.cleanupFailure != nil
                || dictation.chineseConversionFailed || showCurrentResult {
                VStack(alignment: .leading, spacing: 12) {
                    HStack {
                        RecordingStatus(application: application)
                        Spacer()
                        if dictation.phase == .recording || dictation.phase == .preparing {
                            Button(language.text("Stop", "停止")) { application.send(.stopDictation) }
                        }
                        if dictation.phase.isActive {
                            Button(language.text("Cancel", "取消")) { application.send(.cancelDictation) }
                        }
                    }
                    if let failure = dictation.failure { Text(failure.message(in: language)).foregroundStyle(.red) }
                    if let failure = dictation.cleanupFailure {
                        Text(failure.message(in: language) + " " + language.text("Using the original transcript.", "已使用原始转录。"))
                            .font(.caption).foregroundStyle(.orange)
                    }
                    if dictation.chineseConversionFailed { Text(language.text("Chinese conversion was unavailable. Your text has been preserved.", "中文转换暂不可用。已保留转录文字。")).font(.caption).foregroundStyle(.orange) }
                    if showCurrentResult {
                        Text(dictation.text).textSelection(.enabled).accessibilityIdentifier("latest-result")
                        if let message = dictation.delivery.message(in: language) { Text(message).font(.caption).foregroundStyle(.secondary) }
                        Button(language.text(dictation.resultCopied ? "Copied" : "Copy text", dictation.resultCopied ? "已复制" : "复制文字")) { application.send(.copyDictationResult) }
                        if dictation.rawText != dictation.text {
                            DisclosureGroup(language.text("Original transcript", "原始转录")) {
                                Text(dictation.rawText).textSelection(.enabled)
                                Button(language.text("Copy original", "复制原文")) { application.send(.copyRawDictationResult) }
                            }
                        }
                    }
                }.padding(16).background(.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 10))
            }
            if application.state.history.entries.isEmpty && !application.state.history.isLoading {
                HStack {
                    if application.state.settings.asr.serverURL.isEmpty {
                        Button(language.text("Configure Speech-to-Text", "配置语音转文字")) { application.send(.openSettings(.speechToText)) }
                    } else {
                        Text(language.text("Hold your shortcut to speak, or double-tap for Hands-free Dictation.", "按住快捷键说话，或双击开始免按键听写。"))
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button(language.text("Start recording", "开始录音")) { application.send(.startDictation) }
                        .disabled(dictation.phase.isActive)
                }
            }
            HistoryView(application: application)
        }
    }
}
