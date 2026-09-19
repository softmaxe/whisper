import Foundation
import Testing
import WhisperCore

@Suite(.serialized) @MainActor
struct ProcessingIntegrationTests {
    @Test(arguments: [false, true], [false, true])
    func reopenedPipelineCleansThenConvertsThenExpandsBeforePaste(cleanupFails: Bool, historyEnabled: Bool) async throws {
        let fixture = try ProcessingFixture()
        defer { fixture.remove() }
        let replacement = "简体保留 $1"
        fixture.app.send(.setTranscriptionLanguage("zh-TW"))
        fixture.app.send(.setHistoryEnabled(historyEnabled))
        fixture.app.send(.importDictionary("OpenWhispr"))
        fixture.app.send(.saveSnippet(trigger: "這是中文軟體", replacement: replacement))
        fixture.app.send(.saveSnippet(trigger: "简体保留", replacement: "Must not cascade"))
        fixture.app = fixture.reopen()
        #expect(fixture.app.state.snippets.entries.count == 2)
        #expect(fixture.app.state.settings.transcription.preferredLanguage == "zh-TW")
        let raw = cleanupFails ? "这是中文软件" : "Um, preserve the original ASR response."
        let occurredAt = fixture.clock.wallDate
        await fixture.submit(raw, changeLanguageAfterSubmission: true)
        await settle { await fixture.cleanupHTTP.requests.count == 1 }
        let asr = try #require(await fixture.asr.requests.first)
        let multipart = String(decoding: asr.body, as: UTF8.self)
        #expect(multipart.contains("以下是繁體中文。語言、學習、軟體、網路。 OpenWhispr, 這是中文軟體, 简体保留"))
        #expect(!multipart.contains(replacement))
        let request = try #require(await fixture.cleanupHTTP.requests.first)
        let body = try #require(request.httpBody)
        let json = try #require(JSONSerialization.jsonObject(with: body) as? [String: Any])
        let messages = try #require(json["messages"] as? [[String: String]])
        #expect(messages[0]["content"]?.contains("Traditional Chinese") == true)
        #expect(messages[0]["content"]?.contains("OpenWhispr, 這是中文軟體, 简体保留") == true)
        #expect(messages[0]["content"]?.contains(replacement) == false)
        #expect(messages[1]["content"]?.contains(raw) == true)
        #expect(fixture.paste.writes.isEmpty)
        #expect(fixture.app.state.dictation.text.isEmpty)
        if cleanupFails { await fixture.cleanupHTTP.respond(0, status: 401, body: "rejected fixture") }
        else { await fixture.cleanupHTTP.reply(content: "这是中文软件") }
        await fixture.waitForResult()
        #expect(fixture.app.state.dictation.rawText == raw)
        #expect(fixture.app.state.dictation.text == replacement)
        #expect(fixture.app.state.dictation.cleanupFailure == (cleanupFails ? .service(401) : nil))
        #expect(fixture.app.state.dictation.delivery == .pasted)
        #expect(fixture.paste.writes == [replacement])
        #expect(fixture.paste.pasted == [PasteTarget(processID: 101)])
        fixture.app.send(.copyRawDictationResult)
        #expect(fixture.clipboard.values == [raw])
        await fixture.app.flushHistoryWrites()
        let reopened = fixture.reopen()
        reopened.send(.loadHistory)
        await settle { reopened.state.history.isLoaded && !reopened.state.history.isLoading }
        #expect(reopened.state.settings.history.enabled == historyEnabled)
        #expect(reopened.state.history.totalCount == (historyEnabled ? 1 : 0))
        if historyEnabled {
            let entry = try #require(reopened.state.history.entries.first)
            #expect(entry.id == fixture.app.state.dictation.requestID)
            #expect(entry.text == replacement && entry.rawText == raw)
            #expect(entry.occurredAt == occurredAt)
            #expect(entry.source == .dictation)
            #expect(entry.model == "asr-fixture")
        }
    }

    @Test func cancelledCleanupCannotExpandPasteOrReplaceTheNextResult() async throws {
        let fixture = try ProcessingFixture()
        defer { fixture.remove() }
        fixture.app.send(.setTranscriptionLanguage("zh-TW"))
        fixture.app.send(.saveSnippet(trigger: "這是中文軟體", replacement: "Cancelled replacement"))
        await fixture.submit("Old raw ASR")
        await settle { await fixture.cleanupHTTP.requests.count == 1 }
        fixture.app.send(.shortcut(.init(keyCode: ShortcutInput.escape, isDown: true)))
        fixture.app.send(.shortcut(.init(keyCode: ShortcutInput.escape, isDown: false)))
        #expect(fixture.app.state.dictation.phase == .idle)
        await fixture.submit("New raw ASR", index: 1)
        await settle { await fixture.cleanupHTTP.requests.count == 2 }
        await fixture.cleanupHTTP.reply(1, content: "新的中文软件")
        await fixture.waitForResult()
        await fixture.cleanupHTTP.reply(0, content: "这是中文软件")
        await fixture.cleanupHTTP.waitUntilReturned(2)
        #expect(fixture.app.state.dictation.rawText == "New raw ASR")
        #expect(fixture.app.state.dictation.text == "新的中文軟體")
        #expect(fixture.paste.writes == ["新的中文軟體"])
        #expect(fixture.paste.pasted.count == 1)
        await fixture.app.flushHistoryWrites()
        let reopened = fixture.reopen()
        reopened.send(.loadHistory)
        await settle { reopened.state.history.isLoaded && !reopened.state.history.isLoading }
        #expect(reopened.state.history.totalCount == 1)
        #expect(reopened.state.history.entries.first?.rawText == "New raw ASR")
        #expect(reopened.state.history.entries.first?.text == "新的中文軟體")
    }
}

@MainActor private final class ProcessingFixture {
    let profile: ProfileFixture
    let microphones = ControlledMicrophones()
    let clock = ControlledClock()
    let asr = ControlledHTTPTransport()
    let cleanupHTTP = ControlledCleanupHTTP()
    let clipboard = ControlledClipboard()
    let paste = ControlledPasteSystem()
    var app: WhisperApplication!

    init() throws {
        profile = try ProfileFixture(keychain: false)
        app = reopen()
        app.send(.saveASR(.init(serverURL: "http://localhost:8178", model: "asr-fixture"), credential: .unchanged))
        app.send(.saveCleanup(.init(serverURL: "http://localhost:8080", model: "cleanup-fixture"), credential: .unchanged))
    }

    func reopen() -> WhisperApplication {
        WhisperApplication(profile: profile.profile, credentials: profile.credentials, microphones: microphones,
            transcriber: SelfHostedTranscriber(transport: asr), clock: clock, clipboard: clipboard,
            pasteSystem: paste, cleanup: SelfHostedCleanup(transport: cleanupHTTP))
    }

    func submit(_ raw: String, index: Int = 0, changeLanguageAfterSubmission: Bool = false) async {
        app.send(.shortcut(.init(keyCode: ShortcutInput.rightCommand, isDown: true)))
        let capture = microphones.sessions.last!
        capture.open()
        capture.deliver([Float](repeating: 0.1, count: 4800))
        await settle { self.app.state.dictation.timing["firstAudio"] != nil }
        clock.advance(WhisperApplication.holdThreshold)
        app.send(.shortcut(.init(keyCode: ShortcutInput.rightCommand, isDown: false)))
        await settle { await self.asr.requests.count == index + 1 }
        if changeLanguageAfterSubmission { app.send(.setTranscriptionLanguage("en-US")) }
        let response = try! JSONEncoder().encode(["text": raw])
        await asr.reply(index, body: String(decoding: response, as: UTF8.self))
    }

    func waitForResult() async {
        for _ in 0..<5000 {
            if app.state.dictation.phase == .result { return }
            try? await Task.sleep(for: .milliseconds(2))
        }
        Issue.record("Combined processing did not publish a final result.")
    }

    func remove() {
        app.send(.cancelDictation)
        clock.advance(10)
        profile.remove()
    }
}
