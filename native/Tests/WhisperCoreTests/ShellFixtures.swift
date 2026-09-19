import Foundation
import Testing
import WhisperCore

@MainActor final class ControlledPrivacySystem: PrivacySystem {
    var value = PermissionSnapshot(microphone: .notDetermined, accessibility: .denied)
    var reads = 0
    var requests: [PrivacyPermission] = []
    var settingsOpened: [PrivacyPermission] = []
    var holdRequest = false
    var pendingRequest: CheckedContinuation<Void, Never>?
    func snapshot() -> PermissionSnapshot { reads += 1; return value }
    func request(_ permission: PrivacyPermission) async {
        requests.append(permission)
        if holdRequest { await withCheckedContinuation { pendingRequest = $0 } }
        if permission == .microphone { value.microphone = .granted } else { value.accessibility = .granted }
    }
    func openSettings(_ permission: PrivacyPermission) { settingsOpened.append(permission) }
}

actor SyntheticNoConversion: UploadMediaConverter {
    func convert(_ source: URL, to destination: URL) async throws { throw UploadFailure.conversionFailed }
}

/// Reusable synthetic workspace with real persistence and explicitly controlled external effects.
@MainActor final class ShellFixture {
    let profile: ProfileFixture
    let microphones = ControlledMicrophones()
    let transport = ControlledHTTPTransport()
    let clock = ControlledClock()
    let desktop = ControlledDesktopEffects()
    let clipboard = ControlledClipboard()
    let paste = ControlledPasteSystem()
    let corrections = ControlledCorrectionSystem(field: ControlledCorrectionField(before: "", range: 0..<0))
    let audio = ControlledHistoryAudio()
    let privacy = ControlledPrivacySystem()
    let cleanup = ForbiddenUploadCleanup()
    let converter = SyntheticNoConversion()
    var app: WhisperApplication!

    init() throws {
        profile = try ProfileFixture(keychain: false)
        app = reopen()
        app.send(.saveASR(.init(serverURL: "http://localhost:8178/v1", model: "fixture"), credential: .unchanged))
        app.send(.saveCleanup(.init(enabled: false), credential: .unchanged))
    }
    func reopen() -> WhisperApplication {
        WhisperApplication(profile: profile.profile, credentials: profile.credentials,
            microphones: microphones, transcriber: SelfHostedTranscriber(transport: transport),
            clock: clock, clipboard: clipboard, pasteSystem: paste, cleanup: cleanup,
            correctionSystem: corrections, desktopEffects: desktop, audioSystem: audio,
            uploadConverter: converter, privacySystem: privacy)
    }
    @discardableResult func seedReference(language: AppLanguage = .english) async -> [HistoryEntry] {
        app.send(.setLanguage(language))
        let text = language == .english ? [
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
        var entries: [HistoryEntry] = []
        for index in text.indices {
            let date = clock.wallDate.addingTimeInterval(index == 3 ? -86_400 : -Double(index) * 600)
            let entry = HistoryEntry(id: fixtureID(index), text: text[index], rawText: index == 0 ? language.text("speak your thoughts um whisper turns your voice into clear text", "把想法说出来嗯 Whisper 会将语音转换成文字") : text[index],
                occurredAt: date, createdAt: date, audioDuration: 12)
            entries.append(entry)
            app.send(.saveHistory(entry))
        }
        app.send(.importDictionary("OpenWhispr, Metal, Unicode"))
        app.send(.changeDictionary(add: ["HyperNotes"], remove: [], source: .learned))
        app.send(.setSnippets([.init(trigger: "my signature", replacement: "Kind regards,\nExample"), .init(trigger: "项目地址", replacement: "https://example.com/project")]))
        await app.flushHistoryWrites()
        app.send(.loadHistory)
        await settle { !self.app.state.history.isLoading && self.app.state.history.entries.count == 4 }
        return entries
    }
    func seedPopulated() async {
        for index in 0..<1_000 {
            let date = clock.wallDate.addingTimeInterval(-Double(index) * 600)
            app.send(.saveHistory(.init(id: fixtureID(index), text: "Synthetic recording \(index): project notes and review.",
                occurredAt: date, createdAt: date, audioDuration: 10)))
        }
        app.send(.importDictionary((0..<100).map { "Term\($0)" }.joined(separator: ",")))
        app.send(.setSnippets((0..<100).map { .init(trigger: "snippet \($0)", replacement: "Replacement \($0)") }))
        await app.flushHistoryWrites()
        app.send(.loadHistory)
        await settle { !self.app.state.history.isLoading && self.app.state.history.totalCount == 1_000 }
    }
    func remove() { app.send(.cancelDictation); profile.remove() }
    private func fixtureID(_ index: Int) -> UUID {
        UUID(uuidString: String(format: "00000000-0000-4000-8000-%012d", index))!
    }
}
