import AppKit
import SwiftUI
import WhisperCore

struct DictationHomeView: View {
    let application: WhisperApplication
    private var language: AppLanguage { application.state.settings.language }
    private var dictation: DictationState { application.state.dictation }

    var body: some View {
        VStack(alignment: .leading, spacing: 22) {
            VStack(alignment: .leading, spacing: 8) {
                Text(language.text("Dictation", "听写")).font(.title2).fontWeight(.semibold)
                Text(language.text("Hold \(shortcutLabel) to speak, or double-tap for Hands-free Dictation.", "按住\(shortcutLabel)说话，或双击开始免按键听写。"))
                    .foregroundStyle(.secondary)
                if !application.state.shortcutAvailable {
                    Text(language.text("Enable Accessibility in Hotkeys settings to use global shortcuts.", "请在快捷键设置中启用辅助功能，以使用全局快捷键。"))
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
            CorrectionLearningFeedback(application: application)
            VStack(alignment: .leading, spacing: 18) {
                HStack {
                    RecordingStatus(application: application)
                    Spacer()
                    if dictation.phase == .preparing || dictation.phase == .recording {
                        Button(language.text("Stop", "停止")) { application.send(.stopDictation) }
                            .accessibilityIdentifier("stop-dictation")
                    } else if dictation.phase != .processing {
                        Button(language.text("Start recording", "开始录音")) { application.send(.startDictation) }
                            .buttonStyle(.borderedProminent).accessibilityIdentifier("start-dictation")
                    }
                    if dictation.phase.isActive {
                        Button(language.text("Cancel", "取消")) { application.send(.cancelDictation) }
                            .keyboardShortcut(.escape, modifiers: []).accessibilityIdentifier("cancel-dictation")
                    }
                }
                if let failure = dictation.failure {
                    Label(failure.message(in: language), systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.red).fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("dictation-error")
                }
                if dictation.chineseConversionFailed {
                    Label(language.text(
                        "Chinese conversion was unavailable. Your transcription is still ready to copy.",
                        "中文转换暂不可用。仍可复制转录结果。"
                    ), systemImage: "exclamationmark.triangle")
                    .font(.caption).foregroundStyle(.secondary)
                }
                if let failure = dictation.cleanupFailure {
                    Text(failure.message(in: language) + " " + language.text("Using the original transcript.", "已使用原始转录。"))
                        .foregroundStyle(.orange).font(.caption).accessibilityIdentifier("cleanup-fallback")
                }
                if !dictation.text.isEmpty {
                    Divider()
                    if let message = dictation.delivery.message(in: language) { Text(message).font(.caption).foregroundStyle(.secondary) }
                    Text(dictation.text).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                        .accessibilityIdentifier("dictation-result")
                    Button(language.text(dictation.resultCopied ? "Copied" : "Copy text", dictation.resultCopied ? "已复制" : "复制文字")) {
                        application.send(.copyDictationResult)
                    }
                    .accessibilityIdentifier("copy-dictation-result")
                    if dictation.rawText != dictation.text {
                        DisclosureGroup(language.text("Original transcript", "原始转录")) {
                            Text(dictation.rawText).textSelection(.enabled)
                            Button(language.text("Copy original", "复制原文")) { application.send(.copyRawDictationResult) }
                        }
                    }
                }
            }
            .padding(20).background(.primary.opacity(0.025))
            .clipShape(RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(.primary.opacity(0.12)))
            HistoryView(application: application)
        }
    }
    private var shortcutLabel: String {
        (try? ShortcutBinding(application.state.settings.shortcuts.first ?? "RightCommand").label(in: language)) ?? "Right Command"
    }
}

struct RecordingStatus: View {
    let application: WhisperApplication
    private var language: AppLanguage { application.state.settings.language }
    private var phase: DictationPhase { application.state.dictation.phase }
    var body: some View {
        HStack(spacing: 10) {
            if phase == .preparing || phase == .processing { ProgressView().controlSize(.small) }
            else { Image(systemName: phase == .recording ? "mic.fill" : phase == .failed ? "exclamationmark.triangle" : "waveform") }
            Text(title).accessibilityIdentifier("dictation-status")
        }
    }
    private var title: String {
        switch phase {
        case .idle: language.text("Ready to record", "等待录音")
        case .preparing:
            if application.state.dictation.gesture == .awaitingSecondTap {
                language.text("Tap again for Hands-free Dictation…", "再轻按一次以开始免按键听写…")
            } else if application.state.dictation.gesture.isProvisional {
                language.text("Hold to speak…", "继续按住以说话…")
            } else {
                language.text("Preparing microphone…", "正在准备麦克风…")
            }
        case .recording: application.state.dictation.origin == .handsFree
                ? language.text("Hands-free · Tap your shortcut to finish", "免按键听写 · 轻按快捷键结束")
                : language.text("Listening", "正在聆听")
        case .processing: application.state.dictation.isCleaning ? language.text("Cleaning up…", "正在整理…") : language.text("Transcribing…", "正在转录…")
        case .result: language.text("Transcription complete", "转录完成")
        case .failed: language.text("Try again", "请重试")
        }
    }
}

@MainActor final class RecordingPillController {
    private let application: WhisperApplication
    private let panel: NonactivatingRecordingPanel
    init(application: WhisperApplication) {
        self.application = application
        panel = NonactivatingRecordingPanel(contentRect: NSRect(x: 0, y: 0, width: 170, height: 64), styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = false
        panel.level = .floating
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.contentView = NSHostingView(rootView: RecordingPill(application: application))
    }
    func update() {
        let presentation = application.state.recordingPill
        guard presentation.visible else { panel.orderOut(nil); return }
        if let screen = NSScreen.main {
            let recovery: Bool
            if case .recovery = application.state.dictation.delivery { recovery = true } else { recovery = false }
            let failure = presentation.feedback == .failed
            let learning = presentation.feedback == .learned
            let width: CGFloat = recovery || failure || learning ? 390 : 170
            let frame = screen.visibleFrame
            let x: CGFloat = switch presentation.placement {
            case .bottomLeft: frame.minX + 4
            case .center: frame.midX - width / 2
            case .bottomRight: frame.maxX - width - 4
            }
            panel.setFrame(NSRect(x: max(frame.minX, min(x, frame.maxX - width)), y: frame.minY + 4,
                width: width, height: recovery ? 220 : failure ? 150 : learning ? 160 : 64), display: true)
        }
        panel.orderFrontRegardless()
    }
}

private final class NonactivatingRecordingPanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

private struct RecordingPill: View {
    let application: WhisperApplication
    @State private var levels = [Float](repeating: 0, count: 10)
    private var dictation: DictationState { application.state.dictation }
    private var language: AppLanguage { application.state.settings.language }
    var body: some View {
        if !application.state.corrections.learned.isEmpty && !dictation.phase.isActive {
            CorrectionLearningFeedback(application: application)
                .padding(18).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 16)).padding(10)
        } else if case .recovery = dictation.delivery {
            VStack(alignment: .leading, spacing: 10) {
                Text(dictation.delivery.message(in: language) ?? "").font(.system(size: 12))
                ScrollView { Text(dictation.text).font(.system(size: 13)).frame(maxWidth: .infinity, alignment: .leading) }
                    .frame(maxHeight: 100)
                Button(language.text(dictation.resultCopied ? "Copied" : "Copy text", dictation.resultCopied ? "已复制" : "复制文字")) {
                    application.send(.copyDictationResult)
                }
                .accessibilityIdentifier("recovery-copy")
                Button(language.text("Dismiss", "关闭")) { application.send(.dismissPillFeedback) }
            }
            .padding(18).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 16)).padding(10)
        } else if application.state.recordingPill.feedback == .failed {
            VStack(alignment: .leading, spacing: 12) {
                Label(dictation.failure?.message(in: language) ?? language.text("Try again", "请重试"), systemImage: "exclamationmark.triangle")
                    .font(.system(size: 12)).fixedSize(horizontal: false, vertical: true)
                HStack {
                    Button(language.text("Retry", "重试")) { application.send(.startDictation) }
                    Button(language.text("Dismiss", "关闭")) { application.send(.dismissPillFeedback) }
                }
            }
            .padding(18).frame(maxWidth: .infinity, alignment: .leading)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 16)).padding(10)
        } else {
            compactPill
        }
    }

    private var symbolName: String {
        switch application.state.recordingPill.feedback {
        case .cancelled: "xmark"
        case .completed: "checkmark"
        case .failed: "exclamationmark"
        case .handsFree: "lock.fill"
        default: "waveform"
        }
    }

    private var compactPill: some View {
        HStack(spacing: 6) {
            Button {
                if application.state.recordingPill.feedback == .idle { application.send(.startDictation) }
                else if dictation.phase == .recording { application.send(.stopDictation) }
                else if dictation.phase == .result { application.send(.copyDictationResult) }
                else if dictation.phase == .failed || dictation.phase == .idle { application.send(.startDictation) }
            } label: {
                HStack(spacing: 6) {
                    if dictation.phase == .preparing || dictation.phase == .processing {
                        ProgressView().controlSize(.small).frame(width: 22, height: 22)
                    } else {
                        Image(systemName: symbolName)
                            .font(.system(size: 20)).frame(width: 22, height: 22)
                    }
                    HStack(spacing: 3) {
                        ForEach(0..<10) { index in
                            Capsule().frame(width: 2, height: CGFloat(3 + levels[index] * 21))
                        }
                    }.frame(width: 52, height: 24)
                }
                .frame(width: 98, height: 40)
                .background(.regularMaterial, in: Capsule())
                .overlay(Capsule().strokeBorder(.primary.opacity(0.18)))
            }
            .buttonStyle(.plain)
            .accessibilityLabel(application.state.recordingPill.feedback.title(in: language))
            if dictation.phase.isActive || application.state.recordingPill.feedback == .failed {
                Button { application.send(dictation.phase.isActive ? .cancelDictation : .dismissPillFeedback) } label: {
                    Image(systemName: "xmark").font(.system(size: 12, weight: .medium))
                        .frame(width: 30, height: 30).background(.regularMaterial, in: Circle())
                }
                .buttonStyle(.plain).accessibilityLabel(language.text("Cancel", "取消"))
            }
        }
        .padding(10)
        .help(dictation.failure?.message(in: language) ?? application.state.recordingPill.feedback.title(in: language))
        .onChange(of: dictation.duration) {
            levels.removeFirst()
            levels.append(dictation.level)
        }
        .onChange(of: dictation.requestID) { levels = [Float](repeating: 0, count: 10) }
    }
}
