import Foundation
import Testing
import WhisperCore

@Suite(.serialized) @MainActor
struct HistoryWorkflowTests {
    @Test func completeFieldsPersistReopenSearchAndCopyWithoutLegacyImport() async throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let legacy = fixture.root.appendingPathComponent("whisper")
        try FileManager.default.createDirectory(at: legacy, withIntermediateDirectories: false)
        let sentinel = Data("legacy fixture remains intact".utf8)
        try sentinel.write(to: legacy.appendingPathComponent("transcriptions.db"))
        let clipboard = ControlledClipboard()
        let app = WhisperApplication(profile: fixture.profile, credentials: fixture.credentials, clipboard: clipboard)
        let occurredAt = Date(timeIntervalSince1970: 1_789_920_000)
        let entry = HistoryEntry(text: "Final CAFÉ\n中文\u{0} preserved", rawText: "raw fixture original", occurredAt: occurredAt,
            createdAt: occurredAt.addingTimeInterval(60), localDate: "2026-09-20", source: .dictation,
            provider: "self-hosted", model: "fixture-model", audioDuration: 12.5)
        app.send(.saveHistory(entry))
        await settled(app)
        #expect(app.state.history.entries == [entry])
        #expect(app.state.history.failure == nil)
        #expect(!app.state.history.entries[0].hasAudio)
        let reopened = WhisperApplication(profile: fixture.profile, credentials: fixture.credentials, clipboard: clipboard)
        reopened.send(.loadHistory)
        await settled(reopened)
        #expect(reopened.state.history.entries == [entry])
        reopened.send(.searchHistory("café"))
        await settle { !reopened.state.history.isSearching }
        #expect(reopened.state.history.searchResults == [entry])
        reopened.send(.searchHistory("RAW FIXTURE"))
        await settle { !reopened.state.history.isSearching }
        #expect(reopened.state.history.searchResults == [entry])
        reopened.send(.selectHistoryEntry(entry.id))
        #expect(reopened.state.history.selectedEntry == entry)
        reopened.send(.copyHistory(entry.id, .processed))
        reopened.send(.copyHistory(entry.id, .raw))
        #expect(clipboard.values == [entry.text, entry.rawText])
        #expect(reopened.state.history.copied?.version == .raw)
        #expect(try Data(contentsOf: legacy.appendingPathComponent("transcriptions.db")) == sentinel)
        let mode = try FileManager.default.attributesOfItem(atPath: fixture.profile.directory.appendingPathComponent("history.sqlite").path)[.posixPermissions] as? NSNumber
        #expect(mode?.intValue == 0o600)
    }

    @Test func dictationPersistsOriginalOccurrenceAndDisablingHistoryKeepsResultsCopyable() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        let startedAt = fixture.clock.wallDate
        _ = await fixture.record()
        fixture.clock.advance(60)
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        fixture.clock.advance(30)
        await fixture.transport.reply(body: "{\"text\":\"First fixture result\"}")
        await settle { fixture.app.state.dictation.phase == .result }
        await settled(fixture.app)
        let saved = try #require(fixture.app.state.history.entries.first)
        #expect(saved.id == fixture.app.state.dictation.requestID)
        #expect(saved.occurredAt == startedAt)
        #expect(saved.createdAt == fixture.clock.wallDate)
        #expect(saved.localDate == HistoryDateGroup.localDate(for: startedAt))
        #expect(saved.model == "whisper-fixture")
        #expect(saved.audioDuration == fixture.app.state.dictation.duration)
        fixture.app.send(.setHistoryEnabled(false))
        #expect(!fixture.app.state.settings.history.enabled)
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 2 }
        await fixture.transport.reply(1, body: "{\"text\":\"Unsaved current result\"}")
        await settle { fixture.app.state.dictation.phase == .result }
        fixture.app.send(.copyDictationResult)
        #expect(fixture.clipboard.values.last == "Unsaved current result")
        #expect(fixture.app.state.history.entries == [saved])
        let reopened = fixture.profile.open()
        #expect(!reopened.state.settings.history.enabled)
        reopened.send(.loadHistory)
        await settled(reopened)
        #expect(reopened.state.history.entries == [saved])
        #expect(reopened.state.dictation.text.isEmpty)
        reopened.send(.setHistoryEnabled(true))
        #expect(fixture.profile.open().state.settings.history.enabled)
    }

    @Test func retainedHistoryUsesStablePagesAndFiveCaseInsensitiveSearchMatches() async throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        let date = Date(timeIntervalSince1970: 1_789_920_000)
        for index in 0..<1_000 {
            app.send(.saveHistory(HistoryEntry(text: "Fixture \(index) CAFÉ", rawText: "Original \(index)",
                occurredAt: date.addingTimeInterval(Double(index)), createdAt: date, source: index == 0 ? .upload : .dictation)))
        }
        app.send(.setLanguage(.simplifiedChinese))
        #expect(app.state.settings.language == .simplifiedChinese)
        await settled(app)
        #expect(app.state.history.totalCount == 1_000)
        #expect(app.state.history.entries.count == 50)
        #expect(app.state.history.entries.first?.text == "Fixture 999 CAFÉ")
        #expect(app.state.history.hasMore)
        app.send(.loadMoreHistory)
        await settled(app)
        #expect(app.state.history.entries.count == 100)
        #expect(Set(app.state.history.entries.map(\.id)).count == 100)
        app.send(.searchHistory("CAFÉ"))
        await settle { !app.state.history.isSearching }
        #expect(app.state.history.searchResults.count == 5)
        app.send(.moveHistorySearchSelection(1))
        #expect(app.state.history.searchSelection == 1)
        app.send(.moveHistorySearchSelection(100))
        #expect(app.state.history.searchSelection == 4)
        app.send(.moveHistorySearchSelection(-100))
        #expect(app.state.history.searchSelection == 0)
        app.send(.searchHistory("Original 0"))
        await settle { !app.state.history.isSearching }
        #expect(app.state.history.searchResults.count == 1)
        #expect(app.state.history.searchResults.first?.source == .upload)
        app.send(.searchHistory("%"))
        await settle { !app.state.history.isSearching }
        #expect(app.state.history.searchResults.isEmpty)
    }

    @Test func failedEntriesStayHiddenUntilRequestedAndDatesUseLocalCalendar() async throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        let date = Date(timeIntervalSince1970: 1_789_920_000)
        let entries = [
            HistoryEntry(text: "Today", occurredAt: date),
            HistoryEntry(text: "Yesterday", occurredAt: date.addingTimeInterval(-86400)),
            HistoryEntry(text: "Older", occurredAt: date.addingTimeInterval(-172800)),
            HistoryEntry(text: "", occurredAt: date.addingTimeInterval(1), status: .failed, errorCode: "fixture-error", errorMessage: "Synthetic failure")
        ]
        entries.forEach { app.send(.saveHistory($0)) }
        await settled(app)
        #expect(app.state.history.totalCount == 4)
        #expect(app.state.history.entries.count == 3)
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(secondsFromGMT: -7 * 3600)!
        let groups = app.state.history.groups(calendar: calendar)
        #expect(groups.count == 3)
        #expect(groups[0].title(in: .english, now: date, calendar: calendar) == "Today")
        #expect(groups[1].title(in: .simplifiedChinese, now: date, calendar: calendar) == "昨天")
        #expect(groups[2].title(in: .english, now: date, calendar: calendar) != "Today")
        var buddhist = Calendar(identifier: .buddhist)
        buddhist.timeZone = calendar.timeZone
        #expect(HistoryDateGroup.localDate(for: date, calendar: buddhist) == HistoryDateGroup.localDate(for: date, calendar: calendar))
        app.send(.showDiscardedHistory(true))
        await settled(app)
        #expect(app.state.history.entries.count == 4)
        #expect(app.state.history.entries.first?.errorCode == "fixture-error")
    }

    @Test func individualDeletionPreservesClearCutoffAndClearAllSurvivesReopening() async throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let clock = ControlledClock()
        let app = WhisperApplication(profile: fixture.profile, credentials: fixture.credentials, clock: clock)
        let first = HistoryEntry(text: "First")
        let second = HistoryEntry(text: "Second")
        app.send(.saveHistory(first)); app.send(.saveHistory(second))
        await settled(app)
        app.send(.deleteHistory(first.id))
        await settled(app)
        #expect(app.state.history.entries == [second])
        #expect(app.state.history.clearedThrough == nil)
        app.send(.clearHistory)
        await settled(app)
        #expect(app.state.history.entries.isEmpty)
        #expect(app.state.history.clearedThrough == clock.wallDate)
        let reopened = fixture.open()
        reopened.send(.loadHistory)
        await settled(reopened)
        #expect(reopened.state.history.entries.isEmpty)
        #expect(reopened.state.history.clearedThrough == clock.wallDate)
        // Late History can remain useful; later Insights must compare occurrence against the cutoff.
        app.send(.saveHistory(HistoryEntry(text: "Late result", occurredAt: clock.wallDate.addingTimeInterval(-30))))
        await settled(app)
        #expect(app.state.history.entries.count == 1)
        #expect(app.state.history.clearedThrough == clock.wallDate)
    }

    @Test func upsertPreservesOccurrenceSourceAndIdentityForLaterRetry() async throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        let entry = HistoryEntry(text: "Original", occurredAt: Date(timeIntervalSince1970: 1000), createdAt: Date(timeIntervalSince1970: 1050), source: .upload, model: "first-model")
        #expect(try await app.recordHistory(entry))
        await settled(app)
        app.send(.selectHistoryEntry(entry.id))
        let replacement = HistoryEntry(id: entry.id, text: "Updated", rawText: "Raw update", occurredAt: Date(), model: "current-model")
        #expect(try await app.recordHistory(replacement))
        await settled(app)
        let saved = try #require(app.state.history.entries.first)
        #expect(saved.id == entry.id)
        #expect(saved.occurredAt == entry.occurredAt)
        #expect(saved.createdAt == entry.createdAt)
        #expect(saved.source == .upload)
        #expect(saved.text == "Updated" && saved.rawText == "Raw update")
        #expect(saved.model == "current-model")
        #expect(app.state.history.selectedEntry == saved)
        #expect(app.state.history.totalCount == 1)
        app.send(.setHistoryEnabled(false))
        #expect(try await !app.recordHistory(HistoryEntry(text: "Not saved")))
        #expect(app.state.history.totalCount == 1)
    }

    @Test func corruptHistoryPreservesFilesAndSuccessfulCurrentResult() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        let database = fixture.profile.profile.directory.appendingPathComponent("history.sqlite")
        let invalid = Data("not a SQLite database fixture".utf8)
        try invalid.write(to: database)
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        await fixture.transport.reply(body: "{\"text\":\"Recoverable current result\"}")
        await settle { fixture.app.state.dictation.phase == .result && fixture.app.state.history.pendingChanges == 0 }
        #expect(fixture.app.state.history.failure == .save)
        fixture.app.send(.copyDictationResult)
        #expect(fixture.clipboard.values == ["Recoverable current result"])
        #expect(try Data(contentsOf: database) == invalid)
        #expect(HistoryFailure.save.message(in: .simplifiedChinese).contains("仍可复制"))
    }

    @Test func oldFreshNativeSettingsDefaultToHistoryEnabled() throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        try FileManager.default.createDirectory(at: fixture.profile.directory, withIntermediateDirectories: false)
        try Data("{\"version\":1,\"settings\":{\"language\":\"en\",\"asr\":{\"serverURL\":\"\",\"model\":\"\"}}}".utf8)
            .write(to: fixture.profile.directory.appendingPathComponent("settings.json"))
        #expect(fixture.open().state.settings.history.enabled)
    }

    @Test func cancellationNeverSavesLateServerOutput() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        fixture.app.send(.cancelDictation)
        await fixture.transport.reply(body: "{\"text\":\"Cancelled synthetic result\"}")
        fixture.app.send(.loadHistory)
        await settled(fixture.app)
        #expect(fixture.app.state.history.entries.isEmpty)
        #expect(fixture.app.state.history.totalCount == 0)
        #expect(fixture.app.state.dictation.text.isEmpty)
    }

    @Test func unreadableSettingsBlockAllHistoryWritesWithoutReplacingFiles() throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        try FileManager.default.createDirectory(at: fixture.profile.directory, withIntermediateDirectories: false)
        let invalid = Data("{\"version\":1,\"settings\":{}}".utf8)
        let settings = fixture.profile.directory.appendingPathComponent("settings.json")
        try invalid.write(to: settings)
        let app = fixture.open()
        #expect(app.state.configurationError == .incompatibleProfile)
        app.send(.saveHistory(HistoryEntry(text: "Not saved")))
        app.send(.searchHistory("Not saved"))
        app.send(.clearHistory)
        #expect(app.state.history.pendingChanges == 0)
        #expect(!FileManager.default.fileExists(atPath: fixture.profile.directory.appendingPathComponent("history.sqlite").path))
        #expect(try Data(contentsOf: settings) == invalid)
    }

    private func settled(_ app: WhisperApplication) async {
        await app.flushHistoryWrites()
        await settle { app.state.history.pendingChanges == 0 && !app.state.history.isLoading && !app.state.history.isSearching }
    }
}
