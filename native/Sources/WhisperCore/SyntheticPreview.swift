import Foundation

/// Explicit preview-only composition. A profile override alone must never select these effects.
@MainActor public enum SyntheticPreview {
    public static func make(profile: NativeProfile, language: AppLanguage = .english, pill: Bool = false,
                            recovery: Bool = false, pillDisplays: any PillDisplaySystem = InertPillDisplaySystem()) throws -> WhisperApplication {
        let directory = profile.directory.resolvingSymlinksInPath().standardizedFileURL
        let roots = [FileManager.default.temporaryDirectory, URL(fileURLWithPath: "/tmp", isDirectory: true)]
            .map { $0.resolvingSymlinksInPath().standardizedFileURL.pathComponents }
        guard roots.contains(where: { directory.pathComponents.starts(with: $0) && directory.pathComponents.count > $0.count }) else {
            throw ConfigurationError.incompatibleProfile
        }
        let marker = directory.appendingPathComponent(".whisper-synthetic-profile")
        if FileManager.default.fileExists(atPath: directory.path), !FileManager.default.fileExists(atPath: marker.path),
           !(try FileManager.default.contentsOfDirectory(atPath: directory.path)).isEmpty {
            throw ConfigurationError.incompatibleProfile
        }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        try Data("Whisper synthetic preview v1\n".utf8).write(to: marker, options: .atomic)
        let clipboard = PreviewClipboard()
        let app = WhisperApplication(profile: profile, credentials: PreviewCredentials(),
            microphones: PreviewMicrophones(), transcriber: PreviewTranscriber(language: language),
            clipboard: clipboard, pasteSystem: PreviewPaste(clipboard: clipboard, succeeds: pill && !recovery), cleanup: PreviewCleanup(),
            correctionSystem: PreviewCorrections(), desktopEffects: InertDesktopEffects(),
            audioSystem: PreviewHistoryAudio(), uploadConverter: PreviewUploadConverter(), pillDisplays: pillDisplays, privacySystem: PreviewPrivacy())
        var preferences = app.state.settings.desktop
        preferences.audioCuesEnabled = false; preferences.pauseMediaOnDictation = false
        preferences.showMenuBarIcon = false; preferences.pillVisible = pill; preferences.floatingIconAutoHide = !pill
        preferences.startMinimized = false
        app.send(.saveDesktopPreferences(preferences))
        app.send(.setAutoLearnCorrections(false))
        app.send(.setLanguage(language))
        return app
    }

    public static func seed(_ app: WhisperApplication, populated: Bool = false) async throws {
        let language = app.state.settings.language
        app.send(.saveASR(.init(serverURL: "http://localhost:8178/v1", model: "whisper-fixture"), credential: .unchanged))
        app.send(.saveCleanup(.init(enabled: false, serverURL: "http://localhost:8080", model: "cleanup-fixture"), credential: .unchanged))
        let samples = language == .english ? [
            "Speak your thoughts. Whisper turns your voice into clear text, adds punctuation, and pastes it into the app you are using.",
            "Today's plan: finish the interface design, review user feedback, and update the project documentation.",
            "For next week's demo, cover voice input, the custom dictionary, and audio or video file transcription. Prepare a few examples in advance.",
            "The goal is to make writing feel natural. Keep the original meaning, remove repetition, and let the text read clearly."
        ] : [
            "把想法说出来，Whisper 会将语音转换成文字，自动整理标点，再粘贴到当前应用。",
            "今天的工作计划：完成界面设计，审阅用户反馈，更新项目文档。",
            "下周的演示需要介绍三个部分：语音输入、自定义词典和音视频文件转写。请提前准备几个示例。",
            "这段录音的重点是让输入更自然，保留原意，删除重复表达，让文字清晰易读。"
        ]
        let anchor = Date()
        for index in 0..<(populated ? 1_000 : samples.count) {
            let date = anchor.addingTimeInterval(index < 3 ? -Double(index) * 600 : -86_400 - Double(index - 3) * 600)
            let text = populated ? "Synthetic recording \(index): project notes and review." : samples[index]
            let entry = HistoryEntry(id: UUID(uuidString: String(format: "11111111-1111-4111-8111-%012d", index))!,
                text: text, rawText: index == 0 ? language.text("speak your thoughts um whisper turns your voice into clear text", "把想法说出来嗯 Whisper 会将语音转换成文字") : text,
                occurredAt: date, createdAt: date, audioDuration: 12)
            app.send(.saveHistory(entry))
        }
        app.send(.importDictionary(populated ? (0..<100).map { "Term\($0)" }.joined(separator: ",") : "OpenWhispr, Metal, Unicode"))
        app.send(.setSnippets(populated ? (0..<100).map { .init(trigger: "snippet \($0)", replacement: "Replacement \($0)") }
            : [.init(trigger: "my signature", replacement: "Kind regards,\nExample"), .init(trigger: "项目地址", replacement: "https://example.com/project")]))
        let fixtures = app.profileStore.profile.directory.appendingPathComponent("Fixtures", isDirectory: true)
        try FileManager.default.createDirectory(at: fixtures, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        try silentWAV().write(to: fixtures.appendingPathComponent("sample.wav"))
        await app.flushHistoryWrites()
        app.send(.loadHistory)
        app.send(.loadInsights)
    }

    private static func silentWAV() -> Data {
        var data = Data()
        func text(_ value: String) { data.append(contentsOf: value.utf8) }
        func integer<T: FixedWidthInteger>(_ value: T) { var value = value.littleEndian; withUnsafeBytes(of: &value) { data.append(contentsOf: $0) } }
        text("RIFF"); integer(UInt32(36 + 32_000)); text("WAVEfmt "); integer(UInt32(16))
        integer(UInt16(1)); integer(UInt16(1)); integer(UInt32(16_000)); integer(UInt32(32_000))
        integer(UInt16(2)); integer(UInt16(16)); text("data"); integer(UInt32(32_000))
        data.append(Data(repeating: 0, count: 32_000))
        return data
    }
}

private final class PreviewCredentials: CredentialStore {
    private var values: [String: String] = [:]
    func read(account: String) throws -> String? { values[account] }
    func write(_ value: String, account: String) throws { values[account] = value }
    func delete(account: String) throws { values.removeValue(forKey: account) }
}
@MainActor private final class PreviewClipboard: TextClipboard {
    var text = ""
    func write(_ text: String) { self.text = text }
}
@MainActor private struct PreviewPaste: AutomaticPasteSystem {
    let clipboard: PreviewClipboard
    let succeeds: Bool
    var modifiersHeld: Bool { false }
    func captureTarget() -> PasteTarget? { succeeds ? .init(processID: 42) : nil }
    func snapshotClipboard() -> ClipboardSnapshot { .init(items: []) }
    func replaceClipboard(with text: String) -> Int? { clipboard.write(text); return 1 }
    func restoreClipboard(_ snapshot: ClipboardSnapshot, ownedRevision: Int) {}
    func activate(_ target: PasteTarget) async -> Bool { succeeds }
    func canPaste(_ target: PasteTarget) async -> Bool { succeeds }
    func paste(_ target: PasteTarget) async -> Bool { succeeds }
}
@MainActor private struct PreviewMicrophones: MicrophoneProvider {
    func resolveDevice() throws -> MicrophoneDevice { try inputSnapshot().devices[0] }
    func inputSnapshot() throws -> MicrophoneSnapshot {
        .init(devices: [.init(id: "fixture-built-in", name: "Built-in fixture", category: .builtIn),
                       .init(id: "fixture-wireless", name: "Wireless fixture", category: .continuity)], systemDefaultID: "fixture-built-in", lidClosed: false)
    }
    func makeSession(device: MicrophoneDevice, receive: @escaping @Sendable (CaptureEvent) -> Void) -> any MicrophoneSession { PreviewCapture(receive: receive) }
}
private final class PreviewCapture: MicrophoneSession, @unchecked Sendable {
    let receive: @Sendable (CaptureEvent) -> Void
    init(receive: @escaping @Sendable (CaptureEvent) -> Void) { self.receive = receive }
    func start() { receive(.opened()); receive(.frame(.init(samples: [Float](repeating: 0.06, count: 57_600)))) }
    func stop(completion: @escaping @Sendable () -> Void) { completion() }
}
private struct PreviewTranscriber: TranscriptionService {
    let language: AppLanguage
    func transcribe(file: URL, configuration: ASRConfiguration, credential: String?, options: TranscriptionOptions) async throws -> String {
        try await Task.sleep(for: .milliseconds(600))
        return language.text("Speak your thoughts. Whisper turns your voice into clear text.", "把想法说出来，Whisper 会将语音转换成文字。")
    }
}
private struct PreviewCleanup: CleanupService {
    func clean(text: String, configuration: CleanupConfiguration, credential: String?, context: CleanupContext) async throws -> String {
        try Task.checkCancellation(); return text
    }
}
@MainActor private struct PreviewCorrections: CorrectionMonitoringSystem {
    func capture(_ target: PasteTarget) async -> (any CorrectionField)? { nil }
}
@MainActor private final class PreviewHistoryAudio: HistoryAudioSystem {
    func play(_ url: URL, completion: @escaping @MainActor @Sendable () -> Void) -> Bool { true }
    func stop() {}
    func reveal(_ url: URL) {}
}
private struct PreviewUploadConverter: UploadMediaConverter {
    func convert(_ source: URL, to destination: URL) async throws { throw UploadFailure.conversionFailed }
}
@MainActor private final class PreviewPrivacy: PrivacySystem {
    var status = PermissionSnapshot(microphone: .notDetermined, accessibility: .denied)
    func snapshot() -> PermissionSnapshot { status }
    func request(_ permission: PrivacyPermission) async {
        if permission == .microphone { status.microphone = .granted } else { status.accessibility = .granted }
    }
    func openSettings(_ permission: PrivacyPermission) {}
}
