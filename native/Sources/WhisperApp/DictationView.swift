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

@MainActor final class RecordingPillController: NSObject, NSWindowDelegate {
    private let application: WhisperApplication
    private let panel: NonactivatingRecordingPanel
    private struct LayoutRequest: Equatable {
        let size: CGSize
        let placement: PillPlacement
        let requestID: UUID?
    }
    private var lastLayout: LayoutRequest?
    private var presentedRecovery: UUID?
    private var focusedRecovery: UUID?
    init(application: WhisperApplication) {
        self.application = application
        panel = NonactivatingRecordingPanel(contentRect: NSRect(x: 0, y: 0, width: 170, height: 64), styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        super.init()
        panel.delegate = self
        panel.becomesKeyOnlyIfNeeded = false
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = false
        panel.level = .floating
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.contentView = NSHostingView(rootView: RecordingPill(application: application))
    }
    func update(refreshGeometry: Bool = false) {
        let presentation = application.state.recordingPill
        guard presentation.visible else {
            panel.orderOut(nil)
            if let revision = presentedRecovery { application.send(.copyRecoveryPresented(revision, false)) }
            presentedRecovery = nil
            lastLayout = nil
            return
        }
        let recovery = presentation.feedback == .recovery
        let size = presentation.panelSize
        let layout = LayoutRequest(size: size, placement: presentation.placement, requestID: presentation.requestID)
        if refreshGeometry || lastLayout != layout {
            lastLayout = layout
            application.send(.updatePillGeometry(size: size, currentFrame: panel.isVisible ? panel.frame : application.state.desktop.pillFrame))
        }
        let resolved = application.state.recordingPill
        if let frame = resolved.frame { panel.setFrame(frame, display: true) }
        else { panel.setContentSize(size) }
        let readyRecovery = recovery && !resolved.geometryPending ? resolved.recoveryRevision : nil
        if presentedRecovery != readyRecovery, panel.isKeyWindow { panel.orderOut(nil) }
        panel.permitsRecoveryFocus = readyRecovery != nil
        panel.orderFrontRegardless()
        if presentedRecovery != readyRecovery {
            if let revision = presentedRecovery { application.send(.copyRecoveryPresented(revision, false)) }
            presentedRecovery = readyRecovery
            if let revision = readyRecovery {
                // Start the countdown only after native bounds and visibility are applied.
                application.send(.copyRecoveryPresented(revision, true))
            }
        }
    }
    func windowDidBecomeKey(_ notification: Notification) {
        guard let revision = presentedRecovery else { return }
        focusedRecovery = revision
        application.send(.copyRecoveryFocused(revision, true))
    }
    func windowDidResignKey(_ notification: Notification) {
        if let revision = focusedRecovery { application.send(.copyRecoveryFocused(revision, false)) }
        focusedRecovery = nil
    }
}

private final class NonactivatingRecordingPanel: NSPanel {
    var permitsRecoveryFocus = false
    override var canBecomeKey: Bool { permitsRecoveryFocus }
    override var canBecomeMain: Bool { false }
}

private struct RecordingPill: View {
    let application: WhisperApplication
    @State private var levels = [Float](repeating: 0, count: 10)
    @State private var recoveryHovered = false
    private var dictation: DictationState { application.state.dictation }
    private var language: AppLanguage { application.state.settings.language }
    var body: some View {
        if !application.state.corrections.learned.isEmpty && !dictation.phase.isActive {
            CorrectionLearningFeedback(application: application)
                .padding(18).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 16)).padding(10)
        } else if case .recovery = dictation.delivery {
            let recoveryRevision = application.state.desktop.copyRecovery.revision
            VStack(alignment: .leading, spacing: 10) {
                Text(dictation.delivery.message(in: language) ?? "").font(.system(size: 12))
                ScrollView { Text(dictation.text).textSelection(.enabled).font(.system(size: 13)).frame(maxWidth: .infinity, alignment: .leading) }
                    .frame(maxHeight: 100)
                    .focusable().accessibilityIdentifier("recovery-text")
                Button(language.text(dictation.resultCopied ? "Copied" : "Copy text", dictation.resultCopied ? "已复制" : "复制文字")) {
                    application.send(.copyDictationResult)
                }
                .accessibilityIdentifier("recovery-copy")
                Button(language.text("Dismiss", "关闭")) { application.send(.dismissPillFeedback) }
                    .accessibilityIdentifier("recovery-dismiss")
            }
            .padding(18).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 16)).padding(10)
            .onHover { held in
                recoveryHovered = held
                if let recoveryRevision { application.send(.copyRecoveryHeld(recoveryRevision, held)) }
            }
            .onChange(of: recoveryRevision) {
                if let recoveryRevision { application.send(.copyRecoveryHeld(recoveryRevision, recoveryHovered)) }
            }
            .onDisappear {
                recoveryHovered = false
                if let recoveryRevision { application.send(.copyRecoveryHeld(recoveryRevision, false)) }
            }
            .onExitCommand { application.send(.foregroundEscape) }
        } else if application.state.recordingPill.feedback == .failed {
            VStack(alignment: .leading, spacing: 12) {
                Label(dictation.failure?.message(in: language) ?? language.text("Try again", "请重试"), systemImage: "exclamationmark.triangle")
                    .font(.system(size: 12)).fixedSize(horizontal: false, vertical: true)
                HStack {
                    Button(language.text("Retry", "重试")) { application.send(.recordingPillAction) }
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
        let presentation = application.state.recordingPill
        return HStack(spacing: 8) {
            if presentation.showsCancel && presentation.placement == .bottomRight { cancelButton }
            Button {
                application.send(.recordingPillAction)
            } label: {
                HStack(spacing: 6) {
                    if dictation.phase == .preparing || dictation.phase == .processing {
                        ProgressView().controlSize(.small).frame(width: 22, height: 22)
                    } else {
                        Image(systemName: symbolName)
                            .font(.system(size: 20)).frame(width: 22, height: 22)
                    }
                    if presentation.showsWaveform {
                        HStack(spacing: 3) {
                            ForEach(0..<10) { index in
                                Capsule().frame(width: 2, height: CGFloat(3 + levels[index] * 21))
                            }
                        }.frame(width: 52, height: 24).accessibilityHidden(true)
                    }
                }
                .padding(.leading, presentation.showsWaveform ? 6 : 0)
                .padding(.trailing, presentation.showsWaveform ? 12 : 0)
                .frame(width: presentation.compactSize.width, height: presentation.compactSize.height)
                .background(.regularMaterial, in: Capsule())
                .overlay(Capsule().strokeBorder(.primary.opacity(0.18)))
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("recording-pill-primary")
            .accessibilityLabel(application.state.recordingPill.feedback.title(in: language))
            if presentation.showsCancel && presentation.placement != .bottomRight { cancelButton }
        }
        .padding(10)
        .help(dictation.failure?.message(in: language) ?? application.state.recordingPill.feedback.title(in: language))
        .onChange(of: dictation.duration) {
            levels.removeFirst()
            levels.append(dictation.level)
        }
        .onChange(of: dictation.requestID) { levels = [Float](repeating: 0, count: 10) }
    }
    private var cancelButton: some View {
        Button { application.send(.cancelDictation) } label: {
            Image(systemName: "xmark").font(.system(size: 12, weight: .medium))
                .frame(width: 28, height: 28).background(.regularMaterial, in: Circle())
        }
        .buttonStyle(.plain).accessibilityLabel(language.text("Cancel", "取消"))
        .accessibilityIdentifier("recording-pill-cancel")
    }
}
