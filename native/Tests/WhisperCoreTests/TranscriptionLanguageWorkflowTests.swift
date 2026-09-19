import Foundation
import Testing
import WhisperCore

private struct ChineseOracleCase: Decodable, Sendable, CustomTestStringConvertible {
    let name: String
    let input: String
    let language: String
    let preference: ChineseScriptPreference
    let expected: String
    var testDescription: String { name }
    static let cases: [Self] = {
        struct Oracle: Decodable { let cases: [ChineseOracleCase] }
        let url = Bundle.module.url(forResource: "ChineseOracle", withExtension: "json", subdirectory: "Resources")!
        return try! JSONDecoder().decode(Oracle.self, from: Data(contentsOf: url)).cases
    }()
}

@Suite(.serialized) @MainActor
struct TranscriptionLanguageWorkflowTests {
    @Test func transcriptionChoicesPersistSeparatelyFromInterfaceLanguage() throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let app = profile.open()
        #expect(app.state.settings.transcription == TranscriptionPreferences())
        app.send(.setLanguage(.simplifiedChinese))
        app.send(.setTranscriptionLanguage("zh-TW"))
        app.send(.setChineseScriptPreference(.simplified))
        #expect(app.state.settingsSaved)
        let reopened = profile.open()
        #expect(reopened.state.settings.language == .simplifiedChinese)
        #expect(reopened.state.settings.transcription == .init(preferredLanguage: "zh-TW", chineseScriptPreference: .simplified))
        reopened.send(.setTranscriptionLanguage("auto"))
        #expect(profile.open().state.settings.transcription == .init(chineseScriptPreference: .simplified))
    }

    @Test func earlierNativeProfileRetainsSettingsAndUsesLanguageDefaults() throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        try FileManager.default.createDirectory(at: profile.profile.directory, withIntermediateDirectories: true)
        let file = profile.profile.directory.appendingPathComponent("settings.json")
        try Data(#"{"version":1,"settings":{"language":"zh-CN","asr":{"serverURL":"http://localhost:8178","model":"whisper-fixture"}}}"#.utf8).write(to: file)
        let app = profile.open()
        #expect(app.state.configurationError == nil)
        #expect(app.state.settings.language == .simplifiedChinese)
        #expect(app.state.settings.asr.model == "whisper-fixture")
        #expect(app.state.settings.transcription == .init())
        app.send(.setChineseScriptPreference(.traditional))
        #expect(profile.open().state.settings.transcription.chineseScriptPreference == .traditional)
    }

    @Test(arguments: [
        ("auto", Optional<String>.none, Optional<String>.none),
        ("zh-CN", "zh", "以下是简体中文。语言、学习、软件、网络。"),
        ("zh-TW", "zh", "以下是繁體中文。語言、學習、軟體、網路。"),
        ("en-US", "en", Optional<String>.none), ("en-GB", "en", Optional<String>.none), ("ja", "ja", Optional<String>.none),
        ("ko", "ko", Optional<String>.none)
    ])
    func outgoingMultipartUsesLanguageAndOnlyExplicitChineseBias(language: String, expectedLanguage: String?, expectedPrompt: String?) async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        fixture.app.send(.setTranscriptionLanguage(language))
        fixture.app.send(.setChineseScriptPreference(.traditional))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        let request = try #require(await fixture.transport.requests.first)
        let multipart = String(decoding: request.body, as: UTF8.self)
        if let expectedLanguage { #expect(multipart.contains("name=\"language\"\r\n\r\n" + expectedLanguage + "\r\n")) }
        else { #expect(!multipart.contains("name=\"language\"")) }
        if let expectedPrompt { #expect(multipart.contains("name=\"prompt\"\r\n\r\n" + expectedPrompt + "\r\n")) }
        else { #expect(!multipart.contains("name=\"prompt\"")) }
        await fixture.transport.reply(body: "{\"text\":\"language fixture\"}")
        await settle { fixture.app.state.dictation.phase == .result }
    }

    @Test(arguments: ChineseOracleCase.cases)
    private func resultAndCopyMatchExistingOpenCCOracle(fixture oracle: ChineseOracleCase) async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        fixture.app.send(.setTranscriptionLanguage(oracle.language))
        fixture.app.send(.setChineseScriptPreference(oracle.preference))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        let json = String(decoding: try JSONEncoder().encode(["text": oracle.input]), as: UTF8.self)
        await fixture.transport.reply(body: json)
        await waitForResult(fixture.app)
        #expect(fixture.app.state.dictation.rawText == oracle.input)
        #expect(fixture.app.state.dictation.text.utf8.elementsEqual(oracle.expected.utf8))
        #expect(!fixture.app.state.dictation.chineseConversionFailed)
        fixture.app.send(.copyDictationResult)
        #expect(fixture.clipboard.values.last == oracle.expected)
    }

    @Test func preferenceChangeDuringServerRequestAppliesToTheNextDictation() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        fixture.app.send(.setTranscriptionLanguage("zh-TW"))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        fixture.app.send(.setTranscriptionLanguage("zh-CN"))
        await fixture.transport.reply(body: "{\"text\":\"这是中文软件\"}")
        await waitForResult(fixture.app)
        #expect(fixture.app.state.dictation.text == "這是中文軟體")
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 2 }
        await fixture.transport.reply(1, body: "{\"text\":\"這是中文軟體\"}")
        await waitForResult(fixture.app)
        #expect(fixture.app.state.dictation.text == "这是中文软件")
    }

    @Test func cancelledChineseResultCannotReplaceANewerRequest() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        fixture.app.send(.setTranscriptionLanguage("zh-TW"))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        fixture.app.send(.cancelDictation)
        fixture.app.send(.setTranscriptionLanguage("en-US"))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 2 }
        await fixture.transport.reply(1, body: "{\"text\":\"Current result\"}")
        await waitForResult(fixture.app)
        await fixture.transport.reply(0, body: "{\"text\":\"这是过期的中文软件\"}")
        await settle { fixture.app.state.dictation.text == "Current result" }
        fixture.app.send(.copyDictationResult)
        #expect(fixture.clipboard.values == ["Current result"])
    }

    @Test(arguments: ["干净的头发", "他很干练", "面条和方便面", "皇后在后面"])
    func repeatingTraditionalConversionPreservesTheCompletedResult(input: String) async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        fixture.app.send(.setTranscriptionLanguage("zh-TW"))
        var text = input
        for index in 0..<2 {
            _ = await fixture.record()
            fixture.app.send(.stopDictation)
            await settle { await fixture.transport.requests.count == index + 1 }
            await fixture.transport.reply(index, body: String(decoding: try JSONEncoder().encode(["text": text]), as: UTF8.self))
            await waitForResult(fixture.app)
            if index == 1 { #expect(fixture.app.state.dictation.text == text) }
            text = fixture.app.state.dictation.text
        }
    }

    @Test func cancellationDuringLargeConversionAllowsANewDictation() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        fixture.app.send(.setTranscriptionLanguage("zh-TW"))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        let text = String(repeating: "这是简体中文软件。", count: 100_000)
        await fixture.transport.reply(body: String(decoding: try JSONEncoder().encode(["text": text]), as: UTF8.self))
        await settle { fixture.app.state.dictation.timing["textConversion"] != nil }
        fixture.app.send(.cancelDictation)
        #expect(fixture.app.state.dictation.phase == .idle)
        #expect(fixture.app.state.dictation.text.isEmpty)
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 2 }
        await fixture.transport.reply(1, body: "{\"text\":\"这是新的软件\"}")
        await waitForResult(fixture.app)
        #expect(fixture.app.state.dictation.text == "這是新的軟體")
        #expect(fixture.app.state.dictation.rawText == "这是新的软件")
    }

    @Test func unknownScriptPreferenceDefaultsToUnconvertedText() throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        try FileManager.default.createDirectory(at: profile.profile.directory, withIntermediateDirectories: true)
        let document = #"{"version":1,"settings":{"language":"en","asr":{"serverURL":"http://localhost","model":"whisper-fixture"},"transcription":{"chineseScriptPreference":"unknown"}}}"#
        try Data(document.utf8).write(to: profile.profile.directory.appendingPathComponent("settings.json"))
        let reopened = profile.open()
        #expect(reopened.state.configurationError == nil)
        #expect(reopened.state.settings.transcription == .init())
    }

    @Test(arguments: [false, true])
    func chineseBiasAndCappedDictionaryKeepTheSentEchoGuard(longVocabulary: Bool) async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        fixture.app.send(.setTranscriptionLanguage("zh-CN"))
        let words = longVocabulary ? (0..<100).map { "Vocabulary\($0)" + String(repeating: "x", count: 32) } : ["OpenWhispr"]
        fixture.app.send(.importDictionary(words.joined(separator: ", ")))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        let request = try #require(await fixture.transport.requests.first)
        let multipart = String(decoding: request.body, as: UTF8.self)
        let prompt = try #require(multipart.components(separatedBy: "name=\"prompt\"\r\n\r\n").dropFirst().first?.components(separatedBy: "\r\n").first)
        let bias = "以下是简体中文。语言、学习、软件、网络。 "
        #expect(prompt.hasPrefix(bias))
        #expect(prompt.utf16.count <= 900)
        #expect(multipart.contains("name=\"language\"\r\n\r\nzh\r\n"))
        let sentVocabulary = String(prompt.dropFirst(bias.count))
        if longVocabulary {
            #expect(sentVocabulary != words.joined(separator: ", "))
            #expect(words.contains(String(sentVocabulary.components(separatedBy: ", ").last ?? "")))
        } else {
            #expect(sentVocabulary == "OpenWhispr")
        }
        fixture.app.send(.setTranscriptionLanguage("en-US"))
        fixture.app.send(.changeDictionary(add: [], remove: words))
        let response = String(decoding: try JSONEncoder().encode(["text": sentVocabulary]), as: UTF8.self)
        await fixture.transport.reply(body: response)
        await settle { fixture.app.state.dictation.phase == .failed }
        #expect(fixture.app.state.dictation.failure == .dictionaryEcho)
        #expect(fixture.app.state.dictation.text.isEmpty)
        #expect(fixture.clipboard.values.isEmpty)
    }

    private func waitForResult(_ app: WhisperApplication) async {
        for _ in 0..<5000 {
            if app.state.dictation.phase == .result || app.state.dictation.phase == .failed { return }
            try? await Task.sleep(for: .milliseconds(2))
        }
        Issue.record("Chinese processing did not publish a result.")
    }
}
