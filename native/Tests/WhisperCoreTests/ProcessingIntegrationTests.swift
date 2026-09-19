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

    @Test(arguments: [false, true], [false, true])
    func correctionLearningUsesFinalPastedTextAndKeepsHistory(handsFree: Bool, pasteSucceeds: Bool) async throws {
        let field = ControlledCorrectionField(before: "", range: 0..<0)
        let monitor = ControlledCorrectionSystem(field: field)
        let fixture = try ProcessingFixture(correctionSystem: monitor)
        defer { fixture.remove() }
        fixture.app.send(.setTranscriptionLanguage("zh-TW"))
        fixture.app.send(.saveSnippet(trigger: "這是中文軟體", replacement: "Hey Shunade how are you"))
        fixture.paste.allowPaste = pasteSucceeds
        fixture.paste.onPaste = { field.setRegion($0) }
        let raw = "Original ASR learning fixture"
        await fixture.submit(raw, handsFree: handsFree)
        await settle { await fixture.cleanupHTTP.requests.count == 1 }
        await fixture.cleanupHTTP.reply(content: "这是中文软件")
        await fixture.waitForResult()
        let firstID = try #require(fixture.app.state.dictation.requestID)
        #expect(fixture.app.state.dictation.text == "Hey Shunade how are you")
        #expect(fixture.app.state.dictation.rawText == raw)
        #expect(fixture.app.state.dictation.origin == (handsFree ? .handsFree : .hold))
        #expect(monitor.targets == [PasteTarget(processID: handsFree ? 202 : 101)])
        fixture.clock.advance(0.5)
        if pasteSucceeds {
            await settle { field.reads == 1 }
            field.setRegion("Hey Sinead how are you")
            field.observation?.changed()
            await settle { fixture.clock.scheduledDelays.contains { abs($0 - 1.5) < 0.000001 } }
            fixture.clock.advance(1.5)
            await settle { fixture.app.state.dictionary.words == ["Sinead"] }
            #expect(fixture.app.state.corrections.learned.first?.source == .learned)
            await fixture.submit("Second ASR learning fixture", index: 1, handsFree: handsFree)
            await settle { await fixture.cleanupHTTP.requests.count == 2 }
            let secondASR = try #require(await fixture.asr.requests.last)
            #expect(String(decoding: secondASR.body, as: UTF8.self).contains("Sinead"))
            let secondCleanup = try #require(await fixture.cleanupHTTP.requests.last?.httpBody)
            let json = try #require(JSONSerialization.jsonObject(with: secondCleanup) as? [String: Any])
            let messages = try #require(json["messages"] as? [[String: String]])
            #expect(messages.first?["content"]?.contains("Sinead") == true)
            await fixture.cleanupHTTP.reply(1, content: "Hey Sinead how are you")
            await fixture.waitForResult()
        } else {
            field.setRegion("Hey Sinead how are you")
            fixture.clock.advance(2)
            #expect(field.reads == 0)
            #expect(field.observation == nil)
            #expect(fixture.app.state.corrections.learned.isEmpty)
            #expect(fixture.app.state.dictionary.words.isEmpty)
            #expect(fixture.app.state.dictation.delivery == .recovery(copied: true))
        }
        await fixture.app.flushHistoryWrites()
        let reopened = fixture.reopen()
        reopened.send(.loadHistory)
        await settle { reopened.state.history.isLoaded && !reopened.state.history.isLoading }
        #expect(reopened.state.history.totalCount == (pasteSucceeds ? 2 : 1))
        let first = try #require(reopened.state.history.entries.first { $0.id == firstID })
        #expect(first.rawText == raw)
        #expect(first.text == "Hey Shunade how are you")
        #expect(reopened.state.dictionary.words == (pasteSucceeds ? ["Sinead"] : []))
    }

    @Test func handsFreeFinalPipelineUsesSubmissionTargetAndSavesOneHistoryEntry() async throws {
        let fixture = try ProcessingFixture()
        defer { fixture.remove() }
        fixture.app.send(.setTranscriptionLanguage("zh-TW"))
        fixture.app.send(.saveSnippet(trigger: "這是中文軟體", replacement: "Hands-free literal 简体"))
        let occurredAt = fixture.clock.wallDate
        await fixture.submit("Hands-free raw fixture", handsFree: true)
        await settle { await fixture.cleanupHTTP.requests.count == 1 }
        await fixture.cleanupHTTP.reply(content: "这是中文软件")
        await fixture.waitForResult()
        #expect(fixture.app.state.dictation.origin == .handsFree)
        #expect(fixture.paste.pasted == [PasteTarget(processID: 202)])
        #expect(fixture.paste.writes == ["Hands-free literal 简体"])
        await fixture.app.flushHistoryWrites()
        let reopened = fixture.reopen()
        reopened.send(.loadHistory)
        await settle { reopened.state.history.isLoaded && !reopened.state.history.isLoading }
        #expect(reopened.state.history.totalCount == 1)
        let entry = try #require(reopened.state.history.entries.first)
        #expect(entry.rawText == "Hands-free raw fixture")
        #expect(entry.text == "Hands-free literal 简体")
        #expect(entry.occurredAt == occurredAt)
        #expect(entry.source == .dictation)
    }
}

@MainActor final class ProcessingFixture {
    let profile: ProfileFixture
    let microphones = ControlledMicrophones()
    let clock = ControlledClock()
    let asr = ControlledHTTPTransport()
    let cleanupHTTP = ControlledCleanupHTTP()
    let clipboard = ControlledClipboard()
    let paste = ControlledPasteSystem()
    var app: WhisperApplication!
    let correctionSystem: (any CorrectionMonitoringSystem)?

    init(correctionSystem: (any CorrectionMonitoringSystem)? = nil) throws {
        self.correctionSystem = correctionSystem
        profile = try ProfileFixture(keychain: false)
        app = reopen()
        app.send(.saveASR(.init(serverURL: "http://localhost:8178", model: "asr-fixture"), credential: .unchanged))
        app.send(.saveCleanup(.init(serverURL: "http://localhost:8080", model: "cleanup-fixture"), credential: .unchanged))
    }

    func reopen() -> WhisperApplication {
        WhisperApplication(profile: profile.profile, credentials: profile.credentials, microphones: microphones,
            transcriber: SelfHostedTranscriber(transport: asr), clock: clock, clipboard: clipboard,
            pasteSystem: paste, cleanup: SelfHostedCleanup(transport: cleanupHTTP), correctionSystem: correctionSystem)
    }

    func submit(_ raw: String, index: Int = 0, changeLanguageAfterSubmission: Bool = false, handsFree: Bool = false) async {
        app.send(.shortcut(.init(keyCode: ShortcutInput.rightCommand, isDown: true)))
        let capture = microphones.sessions.last!
        capture.open()
        capture.deliver([Float](repeating: 0.1, count: 4800))
        await settle { self.app.state.dictation.timing["firstAudio"] != nil }
        if handsFree {
            clock.advance(0.04)
            app.send(.shortcut(.init(keyCode: ShortcutInput.rightCommand, isDown: false)))
            clock.advance(0.06)
            app.send(.shortcut(.init(keyCode: ShortcutInput.rightCommand, isDown: true)))
            clock.advance(0.04)
            app.send(.shortcut(.init(keyCode: ShortcutInput.rightCommand, isDown: false)))
            #expect(app.state.dictation.origin == .handsFree)
            paste.frontmost = PasteTarget(processID: 202)
            app.send(.shortcut(.init(keyCode: ShortcutInput.rightCommand, isDown: true)))
        } else { clock.advance(WhisperApplication.holdThreshold) }
        app.send(.shortcut(.init(keyCode: ShortcutInput.rightCommand, isDown: false)))
        if handsFree { paste.frontmost = PasteTarget(processID: 303) }
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
        app.send(.setAutoLearnCorrections(false))
        app.send(.cancelDictation)
        clock.advance(10)
        profile.remove()
    }
}
