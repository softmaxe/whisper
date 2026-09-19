import Foundation
import Testing
import WhisperCore

@Suite(.serialized)
@MainActor
struct SnippetsWorkflowTests {
    @Test func createEditRemoveAndReopenUseRealPersistence() throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        app.send(.saveSnippet(trigger: "  my cal  ", replacement: "  https://calendar.example.com/me  "))
        #expect(app.state.snippets.saved)
        let initial = try #require(app.state.snippets.entries.first)
        #expect(initial.trigger == "my cal")
        #expect(initial.replacement == "https://calendar.example.com/me")
        let reopened = fixture.open()
        #expect(reopened.state.snippets.entries == [initial])
        reopened.send(.saveSnippet(trigger: "MY CAL", replacement: "https://calendar.example.com/new", editingID: initial.id))
        #expect(reopened.state.snippets.entries.first?.id == initial.id)
        #expect(reopened.state.snippets.entries.first?.replacement == "https://calendar.example.com/new")
        let saved = reopened.state.snippets.entries
        reopened.send(.saveSnippet(trigger: "MY CAL", replacement: "https://calendar.example.com/new", editingID: initial.id))
        #expect(reopened.state.snippets.entries == saved)
        reopened.send(.saveSnippet(trigger: "sign off", replacement: "Best regards\nAlex"))
        // A second UI owner can edit one entry without erasing an entry saved after its snapshot.
        app.send(.saveSnippet(trigger: "calendar link", replacement: "New calendar", editingID: initial.id))
        #expect(app.state.snippets.entries.map(\.trigger) == ["calendar link", "sign off"])
        app.send(.deleteSnippet(initial.id))
        #expect(fixture.open().state.snippets.entries.map(\.trigger) == ["sign off"])
    }

    @Test func normalizationSkipsInvalidAndDuplicateRowsAndPreservesNoops() throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        let unicodeLimit = String(repeating: "😀", count: 50)
        app.send(.setSnippets([
            .init(trigger: "  signoff  ", replacement: "  Regards  "),
            .init(trigger: "SIGNOFF", replacement: "Ignored"),
            .init(trigger: " ", replacement: "Empty trigger"),
            .init(trigger: "empty", replacement: " \n "),
            .init(trigger: String(repeating: "x", count: 101), replacement: "Too long"),
            .init(trigger: unicodeLimit, replacement: "Exactly 100 UTF-16 units"),
            .init(trigger: unicodeLimit + "a", replacement: "Too long in UTF-16")
        ]))
        #expect(app.state.snippets.entries.map(\.trigger) == ["signoff", unicodeLimit])
        #expect(app.state.snippets.entries.map(\.replacement) == ["Regards", "Exactly 100 UTF-16 units"])
        let initial = app.state.snippets.entries
        app.send(.setSnippets([.init(trigger: "signoff", replacement: "Regards"), .init(trigger: unicodeLimit, replacement: "Exactly 100 UTF-16 units")]))
        #expect(fixture.open().state.snippets.entries == initial)
        app.send(.setSnippets([.init(trigger: unicodeLimit, replacement: "Updated"), .init(trigger: "signoff", replacement: "Best regards")]))
        #expect(app.state.snippets.entries.map(\.id) == initial.map(\.id))
        #expect(app.state.snippets.entries.map(\.trigger) == ["signoff", unicodeLimit])
        #expect(app.state.snippets.entries.map(\.replacement) == ["Best regards", "Updated"])
        app.send(.setSnippets([]))
        #expect(fixture.open().state.snippets.entries.isEmpty)
    }

    @Test func invalidEditsAndDuplicateCreatesKeepSavedValuesWithBilingualFeedback() throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        app.send(.saveSnippet(trigger: "my cal", replacement: "Calendar"))
        app.send(.saveSnippet(trigger: "sign off", replacement: "Regards"))
        let initial = app.state.snippets.entries
        let id = try #require(initial.first?.id)
        app.send(.saveSnippet(trigger: "MY CAL", replacement: "Duplicate"))
        #expect(app.state.snippets.failure == .duplicate)
        app.send(.saveSnippet(trigger: "SIGN OFF", replacement: "Duplicate edit", editingID: id))
        #expect(app.state.snippets.failure == .duplicate)
        app.send(.saveSnippet(trigger: " ", replacement: "Invalid", editingID: id))
        #expect(app.state.snippets.failure == .emptyTrigger)
        app.send(.saveSnippet(trigger: "my cal", replacement: " \n ", editingID: id))
        #expect(app.state.snippets.failure == .emptyReplacement)
        app.send(.saveSnippet(trigger: String(repeating: "😀", count: 51), replacement: "Invalid", editingID: id))
        #expect(app.state.snippets.failure == .triggerTooLong)
        #expect(!app.state.snippets.saved)
        #expect(fixture.open().state.snippets.entries == initial)
        app.send(.setLanguage(.simplifiedChinese))
        #expect(app.state.snippets.failure?.message(in: app.state.settings.language).contains("触发词") == true)
        #expect(app.state.snippets.failure?.message(in: .english).contains("trigger") == true)
        app.send(.dismissSnippetsMessage)
        #expect(app.state.snippets.failure == nil)
    }

    @Test func malformedAndUnwritableProfilesPreserveSnippets() throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        app.send(.saveSnippet(trigger: "my cal", replacement: "Calendar"))
        let initial = app.state.snippets.entries
        let path = fixture.profile.directory.appendingPathComponent("snippets.json")
        try Data("malformed saved document".utf8).write(to: path)
        let reopened = fixture.open()
        #expect(reopened.state.snippets.failure == .unreadable)
        reopened.send(.setSnippets([]))
        #expect(try String(contentsOf: path, encoding: .utf8) == "malformed saved document")
        app.send(.saveSnippet(trigger: "signature", replacement: "Regards"))
        #expect(app.state.snippets.entries == initial)
        #expect(app.state.snippets.failure == .unreadable)

        try FileManager.default.removeItem(at: path)
        app.send(.setSnippets(initial))
        let restored = app.state.snippets.entries
        try FileManager.default.setAttributes([.posixPermissions: 0o500], ofItemAtPath: fixture.profile.directory.path)
        defer { try? FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: fixture.profile.directory.path) }
        app.send(.deleteSnippet(restored[0].id))
        #expect(app.state.snippets.failure == .saveFailed)
        #expect(app.state.snippets.entries == restored)
        #expect(fixture.open().state.snippets.entries == restored)
    }

    @Test func savedTriggersReachASRAndExpandedResultKeepsRawTextAfterRelaunch() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        fixture.app.send(.importDictionary("OpenWhispr, my cal"))
        fixture.app.send(.saveSnippet(trigger: "my cal", replacement: "https://calendar.example.com/me"))
        fixture.app.send(.saveSnippet(trigger: "sign off", replacement: "Regards,\nAlex"))
        let app = WhisperApplication(profile: fixture.profile.profile, credentials: fixture.profile.credentials,
            microphones: fixture.microphones, transcriber: SelfHostedTranscriber(transport: fixture.transport),
            clock: fixture.clock, clipboard: fixture.clipboard)
        await submit(app, microphones: fixture.microphones)
        await settle { await fixture.transport.requests.count == 1 }
        let request = try #require(await fixture.transport.requests.first)
        let multipart = String(decoding: request.body, as: UTF8.self)
        #expect(multipart.contains("name=\"prompt\"\r\n\r\nOpenWhispr, my cal, my cal, sign off\r\n"))
        #expect(!multipart.contains("https://calendar.example.com/me"))
        #expect(!multipart.contains("Regards"))
        let raw = "Please use my cal. Then sign off."
        await fixture.transport.reply(body: "{\"text\":\"\(raw)\"}")
        await settle { app.state.dictation.phase == .result }
        #expect(app.state.dictation.rawText == raw)
        #expect(app.state.dictation.text == "Please use https://calendar.example.com/me. Then Regards,\nAlex.")
        app.send(.copyDictationResult)
        #expect(fixture.clipboard.values == ["Please use https://calendar.example.com/me. Then Regards,\nAlex."])
    }

    @Test(arguments: [
        ("İmza", "imza"), ("İmza", "İmza"), ("İmza", "İMZA"), ("İmza", "Imza"),
        ("imza", "İmza"), ("imza", "İMZA"), ("İmza", "I\u{0307}mza"),
        ("ışık", "ışık"), ("ışık", "Işık"), ("ışık", "IŞIK"),
        ("Işık", "ışık"), ("Işık", "Işık"), ("Işık", "IŞIK"), ("Işık", "işık"),
        ("IBAN", "iban"), ("IBAN", "IBAN"), ("IBAN", "İBAN"),
        ("Signoff", "SIGNOFF"), ("Signoff", "signoff"), ("café", "cafe\u{0301}")
    ])
    func supportedCaseAndNFCFormsExpandThroughDictation(trigger: String, spoken: String) async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        fixture.app.send(.saveSnippet(trigger: trigger, replacement: "EXPANDED"))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        await respond(fixture, text: "Please \(spoken) now.")
        #expect(fixture.app.state.dictation.text == "Please EXPANDED now.")
        #expect(fixture.app.state.dictation.rawText == "Please \(spoken) now.")
    }

    @Test(arguments: [
        ("ımza", "imza", "imza"),
        ("İmza", "imzalar", "imzalar"),
        ("cat", "bobcat catapult", "bobcat catapult"),
        ("cat", "(CAT), cat!", "(EXPANDED), EXPANDED!"),
        ("cat", "🐈cat🐈", "🐈EXPANDED🐈"),
        ("cat", "a_cat_b", "a_EXPANDED_b"),
        ("cat", "\u{FEFF}cat\u{FEFF}", "\u{FEFF}EXPANDED\u{FEFF}"),
        ("cat", "\u{0085}cat\u{0085}", "\u{0085}cat\u{0085}"),
        ("cat", "cat\u{0301}", "cat\u{0301}"),
        ("签名", "签名。", "EXPANDED。"),
        ("签名", "我的签名", "我的签名"),
        ("a+b?", "a+b?", "EXPANDED"),
        ("[link]", "[link]", "EXPANDED"),
        ("$1", "$1", "EXPANDED")
    ])
    func boundariesAndLiteralPatternsMatchTheReference(trigger: String, spoken: String, expected: String) async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        fixture.app.send(.saveSnippet(trigger: trigger, replacement: "EXPANDED"))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        await respond(fixture, text: "Before \(spoken) after.")
        #expect(fixture.app.state.dictation.text == "Before \(expected.precomposedStringWithCanonicalMapping) after.")
    }

    @Test func longestTriggerWinsAndReplacementIsLiteralWithoutCascading() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        fixture.app.send(.setSnippets([
            .init(trigger: "ask", replacement: "SHORT"),
            .init(trigger: "investor ask", replacement: "my cal $& $1 \\n"),
            .init(trigger: "my cal", replacement: "https://calendar.example.com")
        ]))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        await respond(fixture, text: "Please investor ask, then my cal and ask.")
        #expect(fixture.app.state.dictation.text == "Please my cal $& $1 \\n, then https://calendar.example.com and SHORT.")
    }

    @Test(arguments: [false, true])
    func explicitTurkishTriggerOverridesAnUppercaseIVariant(reverse: Bool) async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        let snippets: [Snippet] = [
            .init(trigger: "Işık", replacement: "LATIN"),
            .init(trigger: "ışık", replacement: "TURKISH")
        ]
        fixture.app.send(.setSnippets(reverse ? Array(snippets.reversed()) : snippets))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        await respond(fixture, text: "Please işık and ışık now.")
        #expect(fixture.app.state.dictation.text == "Please LATIN and TURKISH now.")
    }

    @Test(arguments: [false, true])
    func normalizationRunsOnlyWhenSnippetsExistAndNeverChangesRawText(enabled: Bool) async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        if enabled { fixture.app.send(.saveSnippet(trigger: "signature", replacement: "Regards")) }
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        let raw = "Please visit the cafe\u{0301} today."
        await respond(fixture, text: raw)
        #expect(Array(fixture.app.state.dictation.rawText.utf8) == Array(raw.utf8))
        let expected = enabled ? raw.precomposedStringWithCanonicalMapping : raw
        #expect(Array(fixture.app.state.dictation.text.utf8) == Array(expected.utf8))
    }

    @Test func cancelledResponseCannotExpandOrPublishAndNextRequestUsesEditedSnippet() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        fixture.app.send(.saveSnippet(trigger: "my cal", replacement: "Old calendar"))
        let id = try #require(fixture.app.state.snippets.entries.first?.id)
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        fixture.app.send(.cancelDictation)
        await fixture.transport.reply(body: "{\"text\":\"Please my cal now.\"}")
        await Task.yield()
        #expect(fixture.app.state.dictation.phase == .idle)
        #expect(fixture.app.state.dictation.text.isEmpty)
        fixture.app.send(.saveSnippet(trigger: "my cal", replacement: "New calendar", editingID: id))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 2 }
        await respond(fixture, text: "Please my cal now.", index: 1)
        #expect(fixture.app.state.dictation.text == "Please New calendar now.")
    }

    @Test func exactSingleTriggerPromptEchoKeepsTheInheritedFailure() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        fixture.app.send(.saveSnippet(trigger: "my cal", replacement: "Calendar"))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        await fixture.transport.reply(body: "{\"text\":\"my cal\"}")
        await settle { fixture.app.state.dictation.phase == .failed }
        #expect(fixture.app.state.dictation.failure == .dictionaryEcho)
        #expect(fixture.app.state.dictation.text.isEmpty)
    }

    private func submit(_ app: WhisperApplication, microphones: ControlledMicrophones) async {
        app.send(.startDictation)
        let capture = microphones.sessions.last!
        capture.open()
        capture.deliver([Float](repeating: 0, count: 4800))
        await settle { app.state.dictation.phase == .recording }
        app.send(.stopDictation)
    }

    private func respond(_ fixture: DictationFixture, text: String, index: Int = 0) async {
        let body = try! JSONSerialization.data(withJSONObject: ["text": text])
        await fixture.transport.reply(index, body: String(decoding: body, as: UTF8.self))
        await settle { !fixture.app.state.dictation.phase.isActive }
        #expect(fixture.app.state.dictation.phase == .result)
    }
}
