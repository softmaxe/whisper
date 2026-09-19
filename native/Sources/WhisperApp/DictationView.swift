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
                Text(language.text("Hold Right Command to speak, then release to transcribe and paste.", "按住右 Command 说话，松开后转录并粘贴。"))
                    .foregroundStyle(.secondary)
                if !application.state.shortcutAvailable {
                    Text(language.text("Enable Accessibility in General settings to use the global shortcut.", "请在通用设置中启用辅助功能，以使用全局快捷键。"))
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
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
                if !dictation.text.isEmpty {
                    Divider()
                    if let message = dictation.delivery.message(in: language) { Text(message).font(.caption).foregroundStyle(.secondary) }
                    Text(dictation.text).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                        .accessibilityIdentifier("dictation-result")
                    Button(language.text(dictation.resultCopied ? "Copied" : "Copy text", dictation.resultCopied ? "已复制" : "复制文字")) {
                        application.send(.copyDictationResult)
                    }
                    .accessibilityIdentifier("copy-dictation-result")
                }
            }
            .padding(20).background(.primary.opacity(0.025))
            .clipShape(RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(.primary.opacity(0.12)))
        }
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
        case .preparing: application.state.dictation.gesture == .candidate
                ? language.text("Hold to speak…", "继续按住以说话…")
                : language.text("Preparing microphone…", "正在准备麦克风…")
        case .recording: language.text("Listening", "正在聆听")
        case .processing: language.text("Transcribing…", "正在转录…")
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
        guard application.state.dictation.phase != .idle else { panel.orderOut(nil); return }
        if let screen = NSScreen.main {
            let recovery: Bool
            if case .recovery = application.state.dictation.delivery { recovery = true } else { recovery = false }
            let width: CGFloat = recovery ? 390 : 170
            panel.setFrame(NSRect(x: screen.visibleFrame.midX - width / 2, y: screen.visibleFrame.minY + 18,
                width: width, height: recovery ? 220 : 64), display: true)
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
        if case .recovery = dictation.delivery {
            VStack(alignment: .leading, spacing: 10) {
                Text(dictation.delivery.message(in: language) ?? "").font(.system(size: 12))
                ScrollView { Text(dictation.text).font(.system(size: 13)).frame(maxWidth: .infinity, alignment: .leading) }
                    .frame(maxHeight: 100)
                Button(language.text(dictation.resultCopied ? "Copied" : "Copy text", dictation.resultCopied ? "已复制" : "复制文字")) {
                    application.send(.copyDictationResult)
                }
                .accessibilityIdentifier("recovery-copy")
            }
            .padding(18).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 16)).padding(10)
        } else {
            compactPill
        }
    }

    private var compactPill: some View {
        HStack(spacing: 6) {
            Button {
                if dictation.phase == .recording { application.send(.stopDictation) }
                else if dictation.phase == .result { application.send(.copyDictationResult) }
                else if dictation.phase == .failed { application.send(.startDictation) }
            } label: {
                HStack(spacing: 6) {
                    if dictation.phase == .preparing || dictation.phase == .processing {
                        ProgressView().controlSize(.small).frame(width: 22, height: 22)
                    } else {
                        Image(systemName: dictation.phase == .result ? "checkmark" : dictation.phase == .failed ? "exclamationmark" : "waveform")
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
            .accessibilityLabel(language.text("Dictation control", "听写控制"))
            if dictation.phase.isActive {
                Button { application.send(.cancelDictation) } label: {
                    Image(systemName: "xmark").font(.system(size: 12, weight: .medium))
                        .frame(width: 30, height: 30).background(.regularMaterial, in: Circle())
                }
                .buttonStyle(.plain).accessibilityLabel(language.text("Cancel", "取消"))
            }
        }
        .padding(10)
        .help(dictation.failure?.message(in: language) ?? language.text("Stop recording or copy the completed text", "停止录音或复制已完成的文字"))
        .onChange(of: dictation.duration) {
            levels.removeFirst()
            levels.append(dictation.level)
        }
        .onChange(of: dictation.requestID) { levels = [Float](repeating: 0, count: 10) }
    }
}
