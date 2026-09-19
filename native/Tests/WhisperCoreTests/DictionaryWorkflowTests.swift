import Foundation
import Testing
import WhisperCore

@Suite(.serialized)
@MainActor
struct DictionaryWorkflowTests {
    @Test func editsImportsAndRelaunchPreserveMetadataAndConcurrentDeltas() throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        app.send(.importDictionary("  OpenWhispr, Sinead\r\n软体, ,OpenWhispr, sinead \n"))
        #expect(app.state.dictionary.words == ["OpenWhispr", "Sinead", "软体"])
        #expect(app.state.dictionary.addedCount == 3)
        let unchanged = try #require(app.state.dictionary.entries.first)

        let learner = fixture.open()
        learner.send(.changeDictionary(add: ["Alcahest", "Müller"], remove: [], source: .learned))
        let learned = try #require(learner.state.dictionary.entries.first { $0.word == "Alcahest" })
        app.send(.editDictionaryWord("软体", replacement: " 软件 "))
        #expect(app.state.dictionary.words == ["OpenWhispr", "Sinead", "Alcahest", "Müller", "软件"])
        #expect(app.state.dictionary.entries.first == unchanged)
        #expect(app.state.dictionary.entries.first { $0.word == "Alcahest" } == learned)

        app.send(.importDictionary("ALCAHEST, alcahest, OpenWhispr"))
        #expect(app.state.dictionary.addedCount == 0)
        let promoted = try #require(app.state.dictionary.entries.first { $0.word == "ALCAHEST" })
        #expect(promoted.id == learned.id)
        #expect(promoted.createdAt == learned.createdAt)
        #expect(promoted.source == .manual)
        app.send(.changeDictionary(add: ["sinead"], remove: ["Sinead", "软件"], source: .learned))
        let reopened = fixture.open()
        #expect(reopened.state.dictionary.words == ["OpenWhispr", "sinead", "ALCAHEST", "Müller"])
        #expect(reopened.state.dictionary.entries.first { $0.word == "sinead" }?.source == .manual)
        #expect(reopened.state.dictionary.entries.first { $0.word == "Müller" }?.source == .learned)

        // Clearing the displayed snapshot must not erase a subsequent learned word.
        let displayed = reopened.state.dictionary.words
        learner.send(.changeDictionary(add: ["Parakeet"], remove: [], source: .learned))
        reopened.send(.changeDictionary(add: [], remove: displayed))
        #expect(fixture.open().state.dictionary.words == ["Parakeet"])
    }

    @Test func invalidEditsAndEmptyImportsPreserveTheSavedWords() throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        app.send(.importDictionary("Sinead, OpenWhispr"))
        let original = app.state.dictionary.entries
        app.send(.editDictionaryWord("Sinead", replacement: " openwhispr "))
        #expect(app.state.dictionary.failure == .duplicate)
        #expect(fixture.open().state.dictionary.entries == original)
        app.send(.editDictionaryWord("Sinead", replacement: " \n "))
        #expect(app.state.dictionary.failure == .emptyInput)
        app.send(.editDictionaryWord("Already removed", replacement: "Parakeet"))
        #expect(app.state.dictionary.failure == .missingEntry)
        app.send(.importDictionary(",,\n, \r\n\t"))
        #expect(app.state.dictionary.failure == .emptyInput)
        #expect(fixture.open().state.dictionary.entries == original)
        app.send(.setLanguage(.simplifiedChinese))
        #expect(app.state.dictionary.failure?.message(in: app.state.settings.language).contains("词条") == true)
        #expect(app.state.dictionary.failure?.message(in: .english).contains("word") == true)
        app.send(.dismissDictionaryMessage)
        #expect(app.state.dictionary.failure == nil)
    }

    @Test func unreadableAndUnwritableProfilesDoNotDiscardWords() throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        app.send(.importDictionary("Sinead"))
        let path = fixture.profile.directory.appendingPathComponent("dictionary.json")
        try Data("invalid dictionary document".utf8).write(to: path)
        app.send(.importDictionary("OpenWhispr"))
        #expect(app.state.dictionary.failure == .unreadable)
        #expect(app.state.dictionary.words == ["Sinead"])
        #expect(try String(contentsOf: path, encoding: .utf8) == "invalid dictionary document")
        let reopened = fixture.open()
        #expect(reopened.state.dictionary.failure == .unreadable)
        reopened.send(.importDictionary("Müller"))
        #expect(try String(contentsOf: path, encoding: .utf8) == "invalid dictionary document")

        try FileManager.default.removeItem(at: path)
        app.send(.importDictionary("Sinead"))
        try FileManager.default.setAttributes([.posixPermissions: 0o500], ofItemAtPath: fixture.profile.directory.path)
        defer { try? FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: fixture.profile.directory.path) }
        app.send(.importDictionary("OpenWhispr"))
        #expect(app.state.dictionary.failure == .saveFailed)
        #expect(app.state.dictionary.words == ["Sinead"])
        #expect(fixture.open().state.dictionary.words == ["Sinead"])
    }

    @Test func exportUsesSavedVocabularyAndReportsDestinationFailure() throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        app.send(.importDictionary("Sinead, 软件"))
        let destination = fixture.profile.directory.appendingPathComponent("export.txt")
        app.send(.exportDictionary(destination))
        #expect(app.state.dictionary.exportCompleted)
        #expect(try String(contentsOf: destination, encoding: .utf8) == "Sinead\n软件")
        app.send(.exportDictionary(fixture.profile.directory))
        #expect(!app.state.dictionary.exportCompleted)
        #expect(app.state.dictionary.failure == .exportFailed)
    }

    @Test func persistedVocabularyReachesRealMultipartAndAffectsTheServerResult() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        fixture.app.send(.importDictionary("Sinead, Alcahest\nMüller, sinead"))
        fixture.app.send(.editDictionaryWord("Alcahest", replacement: "OpenWhispr"))
        let app = WhisperApplication(profile: fixture.profile.profile, credentials: fixture.profile.credentials,
            microphones: fixture.microphones, transcriber: SelfHostedTranscriber(transport: fixture.transport),
            clock: fixture.clock, clipboard: fixture.clipboard)
        await submit(app, microphones: fixture.microphones)
        await settle { await fixture.transport.requests.count == 1 }
        let request = try #require(await fixture.transport.requests.first)
        let multipart = String(decoding: request.body, as: UTF8.self)
        #expect(multipart.contains("name=\"prompt\"\r\n\r\nSinead, Müller, OpenWhispr\r\n"))
        #expect(!multipart.contains("Alcahest"))
        #expect(!request.audio.isEmpty)
        let response = multipart.contains("Sinead, Müller, OpenWhispr") ? "Ask Sinead about OpenWhispr." : "Ask Shunade about Open Whisper."
        await fixture.transport.reply(body: "{\"text\":\"\(response)\"}")
        await settle { app.state.dictation.phase == .result }
        #expect(app.state.dictation.rawText == "Ask Sinead about OpenWhispr.")
        #expect(app.state.dictation.text == app.state.dictation.rawText)
        app.send(.copyDictationResult)
        #expect(fixture.clipboard.values == ["Ask Sinead about OpenWhispr."])

        // Vocabulary biases the request; it does not replace words in returned prose.
        await submit(app, microphones: fixture.microphones)
        await settle { await fixture.transport.requests.count == 2 }
        await fixture.transport.reply(1, body: "{\"text\":\"Ask Shunade about Open Whisper.\"}")
        await settle { app.state.dictation.phase == .result }
        #expect(app.state.dictation.text == "Ask Shunade about Open Whisper.")
    }

    @Test(arguments: [
        ("https://asr.example.com", "custom-model", 900),
        ("https://api.groq.com/openai/v1", "gpt-4o-transcribe", 890),
        ("https://asr.example.com", "gpt-4o-mini-transcribe", 8000)
    ])
    func promptCapsAndEchoClassificationUseTheActualRequest(server: String, model: String, limit: Int) async throws {
        let fixture = try DictationFixture(server: server)
        defer { fixture.remove() }
        fixture.app.send(.saveASR(.init(serverURL: server, model: model), credential: .unchanged))
        let words = (0..<180).map { "Word\($0)" + String(repeating: "x", count: 45) }
        fixture.app.send(.importDictionary(words.joined(separator: ", ")))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        let request = try #require(await fixture.transport.requests.first)
        let multipart = String(decoding: request.body, as: UTF8.self)
        let prompt = try #require(multipart.components(separatedBy: "name=\"prompt\"\r\n\r\n").dropFirst().first?.components(separatedBy: "\r\n").first)
        #expect(prompt.utf16.count <= limit)
        #expect(prompt.utf16.count > limit - 60)
        #expect(words.contains(String(prompt.components(separatedBy: ", ").last ?? "")))
        #expect(prompt != words.joined(separator: ", "))
        // Changes made during processing cannot change the echo comparison's saved outgoing prompt.
        fixture.app.send(.changeDictionary(add: [], remove: words))
        await fixture.transport.reply(body: "{\"text\":\"\(prompt.lowercased()).\"}")
        await settle { fixture.app.state.dictation.phase == .failed }
        #expect(fixture.app.state.dictation.failure == .dictionaryEcho)
        #expect(fixture.app.state.dictation.text.isEmpty)
        #expect(fixture.clipboard.values.isEmpty)
    }

    @Test(arguments: [
        ("testing", false), ("testing, ", false), ("data, data, data, data,", false),
        ("OpenWhispr, OpenWhispr", false), ("OpenWhispr, OpenWhispr, OpenWhispr", false),
        ("Electron, renderer", false), ("On my way.", false), ("Let me know", false),
        ("TypeScript, Electron, testing, data, benchmark", false),
        ("benchmark, TypeScript, data, Electron, testing", false),
        ("yes, no, maybe, dunno,", false), ("OpenWhispr Parakeet Alcahest Chromium", false),
        ("OpenWhispr Parakeet Alcahest Chromium TypeScript Electron testing data benchmark inference transcription dictionary microphone renderer latency pipeline the", true)
    ])
    func selfHostedEchoRequiresBothVocabularyThresholds(text: String, isEcho: Bool) async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        fixture.app.send(.importDictionary("OpenWhispr, Parakeet, Alcahest, Chromium, TypeScript, Electron, testing, data, benchmark, inference, transcription, dictionary, microphone, renderer, latency, pipeline, on my way, let me know"))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        await fixture.transport.reply(body: "{\"text\":\"\(text)\"}")
        await settle { !fixture.app.state.dictation.phase.isActive }
        #expect(fixture.app.state.dictation.phase == (isEcho ? .failed : .result))
        #expect(fixture.app.state.dictation.failure == (isEcho ? .dictionaryEcho : nil))
        if !isEcho { #expect(fixture.app.state.dictation.text == text) }
    }

    @Test func emptyDictionaryDoesNotSendPromptOrRejectEchoShapedSpeech() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        let request = try #require(await fixture.transport.requests.first)
        #expect(!String(decoding: request.body, as: UTF8.self).contains("name=\"prompt\""))
        await fixture.transport.reply(body: "{\"text\":\"testing, testing, testing\"}")
        await settle { fixture.app.state.dictation.phase == .result }
        #expect(fixture.app.state.dictation.text == "testing, testing, testing")
    }

    private func submit(_ app: WhisperApplication, microphones: ControlledMicrophones) async {
        app.send(.startDictation)
        let capture = microphones.sessions.last!
        capture.open()
        capture.deliver([Float](repeating: 0, count: 4800))
        await settle { app.state.dictation.phase == .recording }
        app.send(.stopDictation)
    }
}
