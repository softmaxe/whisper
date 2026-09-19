import Foundation
import SQLite3
import Testing
import WhisperCore

@Suite(.serialized) @MainActor struct InsightsWorkflowTests {
    @Test(arguments: [
        ("  one two\nthree  ", 3), ("hello,world", 1),
        ("今天我们测试语音输入", 5), ("今日は良い天気です", 5), ("OpenWhisprで音声入力を試します", 7),
        ("今天测试 API v2。", 4), ("你好 👨‍💻 world 123", 3), ("한국어 음성 입력을 테스트합니다", 4),
        ("東京でOpenAIのAPIを使います", 8), ("今天 1️⃣ 是周日", 4), ("中文\nEnglish punctuation-only !!!", 4)
    ]) func wordAccountingUsesExistingEnglishAndCJKRules(raw: String, words: Int) async throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let app = profile.open()
        app.send(.saveHistory(HistoryEntry(text: "Unrelated cleaned replacement with more words", rawText: raw)))
        await app.flushHistoryWrites()
        await load(app)
        #expect(app.state.insights.summary?.totalWords == words)
        #expect(app.state.insights.summary?.totalDictations == 1)
        #expect(app.state.insights.summary?.averageWpm == nil)
        #expect(app.state.insights.summary?.wpmCoveragePercent == 0)
    }

    @Test func mixedOutcomesReopenWithWeightedSpeedStreaksAndSixCalendarMonths() async throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let clock = ControlledClock()
        clock.wallDate = localDate(2026, 8, 30)
        let app = WhisperApplication(profile: profile.profile, credentials: profile.credentials, clock: clock)
        let records = [
            entry(words: 100, day: "2026-08-28", duration: 60),
            entry(words: 50, day: "2026-08-29", duration: nil),
            entry(words: 70, day: "2026-08-30", duration: 20),
            entry(words: 30, day: "2026-08-30", duration: 10),
            HistoryEntry(text: "Upload should not count", rawText: "One two three", source: .upload),
            HistoryEntry(text: "", status: .failed), HistoryEntry(text: "", status: .discarded)
        ]
        for record in records { app.send(.saveHistory(record)) }
        await app.flushHistoryWrites()
        await load(app)
        let summary = try #require(app.state.insights.summary)
        #expect(summary.totalWords == 250)
        #expect(summary.totalDictations == 4)
        #expect(summary.totalSpokenDurationMs == 90_000)
        #expect(summary.averageWpm == 133)
        #expect(summary.wpmCoveragePercent == 80)
        #expect(summary.currentStreakDays == 3)
        #expect(summary.longestStreakDays == 3)
        #expect(summary.daily.map(\.date) == ["2026-08-28", "2026-08-29", "2026-08-30"])
        #expect(summary.activity.first?.date == "2026-03-01")
        #expect(summary.activity.last?.date == "2026-08-30")
        #expect(summary.activity.filter { $0.words > 0 }.count == 3)
        let reopened = WhisperApplication(profile: profile.profile, credentials: profile.credentials, clock: clock)
        await load(reopened)
        #expect(reopened.state.insights.summary == summary)
        clock.wallDate = localDate(2026, 8, 31)
        await load(reopened)
        #expect(reopened.state.insights.summary?.currentStreakDays == 3)
        clock.wallDate = localDate(2026, 9, 1)
        await load(reopened)
        #expect(reopened.state.insights.summary?.currentStreakDays == 0)
        #expect(reopened.state.insights.summary?.longestStreakDays == 3)
        #expect(reopened.state.insights.summary?.activity.first?.date == "2026-04-01")
    }

    @Test func liveDictationCountsRawWordsBeforeSnippetExpansionAndKeepsOccurrenceDate() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        fixture.app.send(.saveSnippet(trigger: "send greeting", replacement: "This is a long replacement that should never inflate spoken word totals."))
        fixture.clock.wallDate = localDate(2026, 9, 19).addingTimeInterval(12 * 3600 - 10)
        let start = fixture.clock.wallDate
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        fixture.clock.advance(30)
        await fixture.transport.reply(body: #"{"text":"please send greeting"}"#)
        await settle { fixture.app.state.dictation.phase == .result }
        await fixture.app.flushHistoryWrites()
        await load(fixture.app)
        #expect(fixture.app.state.dictation.text.contains("long replacement"))
        #expect(fixture.app.state.insights.summary?.totalWords == 3)
        #expect(fixture.app.state.insights.summary?.daily.first?.date == HistoryDateGroup.localDate(for: start))
        #expect(fixture.app.state.insights.summary?.daily.first?.date != HistoryDateGroup.localDate(for: fixture.clock.wallDate))
        #expect(fixture.app.state.insights.summary?.wpmCoveragePercent == 100)
        fixture.app.send(.setHistoryEnabled(false))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 2 }
        await fixture.transport.reply(1, body: #"{"text":"Do not retain this dictation"}"#)
        await settle { fixture.app.state.dictation.phase == .result }
        await fixture.app.flushHistoryWrites()
        await load(fixture.app)
        #expect(fixture.app.state.insights.summary?.totalWords == 3)
        fixture.app.send(.copyDictationResult)
        #expect(fixture.clipboard.values.last == "Do not retain this dictation")
    }

    @Test func cleanupAndPasteUseFinalTextWhileInsightsCountRawSpeech() async throws {
        let fixture = try ProcessingFixture()
        defer { fixture.remove() }
        await fixture.submit("One two three")
        await settle { await fixture.cleanupHTTP.requests.count == 1 }
        await fixture.cleanupHTTP.reply(content: "A cleaned reply with substantially more than three words")
        await fixture.waitForResult()
        await fixture.app.flushHistoryWrites()
        await load(fixture.app)
        #expect(fixture.paste.writes == ["A cleaned reply with substantially more than three words"])
        #expect(fixture.app.state.insights.summary?.totalWords == 3)
        #expect(fixture.app.state.insights.summary?.totalDictations == 1)
    }

    @Test func newestRefreshOwnsTheDisplayedCalendar() async throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let clock = ControlledClock()
        let app = WhisperApplication(profile: profile.profile, credentials: profile.credentials, clock: clock)
        app.send(.saveHistory(entry(words: 3, day: "2026-09-20", duration: 0)))
        await app.flushHistoryWrites()
        clock.wallDate = localDate(2026, 9, 20)
        app.send(.loadInsights)
        clock.wallDate = localDate(2026, 11, 30)
        app.send(.loadInsights)
        await settle { !app.state.insights.isLoading }
        #expect(app.state.insights.summary?.activity.first?.date == "2026-06-01")
        #expect(app.state.insights.summary?.activity.last?.date == "2026-11-30")
        #expect(app.state.insights.summary?.averageWpm == nil)
        #expect(app.state.insights.summary?.wpmCoveragePercent == 0)
    }

    @Test func recoveryAddsOnlyMissingEligibleEventsAndDuplicateIDsStayIdempotent() async throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let app = profile.open()
        let failed = HistoryEntry(text: "", occurredAt: localDate(2026, 9, 10), localDate: "2026-09-10", status: .failed)
        app.send(.saveHistory(failed))
        await app.flushHistoryWrites()
        await load(app)
        #expect(app.state.insights.summary?.totalDictations == 0)
        var recovered = failed
        recovered.text = "Cleaned text"
        recovered.rawText = "Recovered raw spoken words"
        recovered.status = .completed
        recovered.audioDuration = 2
        try await app.recordHistory(recovered)
        app.send(.setHistoryEnabled(false))
        app.recordRecoveredDictationInsights(recovered)
        app.recordRecoveredDictationInsights(recovered)
        await app.flushHistoryWrites()
        await load(app)
        #expect(app.state.insights.summary?.totalWords == 4)
        #expect(app.state.insights.summary?.totalDictations == 1)
        #expect(app.state.insights.summary?.daily.first?.date == "2026-09-10")
        app.send(.setHistoryEnabled(true))
        recovered.rawText = "A later retry changes the transcript but not the first successful count"
        try await app.recordHistory(recovered)
        app.recordRecoveredDictationInsights(recovered)
        let upload = HistoryEntry(text: "Upload retry", source: .upload)
        app.recordRecoveredDictationInsights(upload)
        await app.flushHistoryWrites()
        await load(app)
        #expect(app.state.insights.summary?.totalWords == 4)
        #expect(app.state.insights.summary?.totalDictations == 1)
    }

    @Test func individualHistoryDeletionKeepsCountsAndClearRejectsLateOldResults() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        fixture.clock.advance(1)
        fixture.app.send(.clearHistory)
        await fixture.app.flushHistoryWrites()
        await fixture.transport.reply(body: #"{"text":"Late old result"}"#)
        await settle { fixture.app.state.dictation.phase == .result }
        await fixture.app.flushHistoryWrites()
        await load(fixture.app)
        #expect(fixture.app.state.insights.summary?.totalWords == 0)
        #expect(fixture.app.state.history.entries.first?.text == "Late old result")
        fixture.clock.advance(1)
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 2 }
        await fixture.transport.reply(1, body: #"{"text":"New result counts"}"#)
        await settle { fixture.app.state.dictation.phase == .result }
        await fixture.app.flushHistoryWrites()
        await load(fixture.app)
        #expect(fixture.app.state.insights.summary?.totalWords == 3)
        let id = try #require(fixture.app.state.dictation.requestID)
        fixture.app.send(.deleteHistory(id))
        await fixture.app.flushHistoryWrites()
        await load(fixture.app)
        #expect(fixture.app.state.insights.summary?.totalWords == 3)
        fixture.app.send(.clearHistory)
        await fixture.app.flushHistoryWrites()
        let reopened = fixture.profile.open()
        await load(reopened)
        #expect(reopened.state.insights.summary?.totalWords == 0)
        reopened.recordRecoveredDictationInsights(HistoryEntry(id: id, text: "Late recovered result", occurredAt: fixture.clock.wallDate))
        await reopened.flushInsightsWrites()
        await load(reopened)
        #expect(reopened.state.insights.summary?.totalDictations == 0)
    }

    @Test(arguments: ["history", "insights_events"])
    func aFailedWriteDoesNotBlockTheOtherResultTable(table: String) async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        await load(fixture.app)
        try sql(fixture.profile.profile, "CREATE TRIGGER reject_fixture BEFORE INSERT ON \(table) BEGIN SELECT RAISE(FAIL, 'controlled write failure'); END;")
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        await fixture.transport.reply(body: #"{"text":"Three fixture words"}"#)
        await settle { fixture.app.state.dictation.phase == .result }
        await fixture.app.flushHistoryWrites()
        if table == "history" {
            #expect(fixture.app.state.history.failure == .save)
            await load(fixture.app)
            #expect(fixture.app.state.insights.summary?.totalWords == 3)
            for name in ["history.sqlite", "history.sqlite-wal", "history.sqlite-shm"] {
                let file = fixture.profile.profile.directory.appendingPathComponent(name)
                if FileManager.default.fileExists(atPath: file.path) {
                    #expect(try Data(contentsOf: file).range(of: Data("Three fixture words".utf8)) == nil)
                }
            }
        } else {
            #expect(fixture.app.state.history.failure == nil)
            await settle { fixture.app.state.history.entries.first?.rawText == "Three fixture words" }
            #expect(fixture.app.state.history.entries.first?.rawText == "Three fixture words")
            #expect(fixture.app.state.insights.failure != nil)
        }
        fixture.app.send(.copyDictationResult)
        #expect(fixture.clipboard.values.last == "Three fixture words")
    }

    @Test func cancelledAndUploadOutcomesAreExcluded() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        fixture.app.send(.startDictation)
        fixture.app.send(.cancelDictation)
        await load(fixture.app)
        #expect(fixture.app.state.insights.summary?.totalWords == 0)
        let upload = HistoryEntry(text: "Synthetic upload", source: .upload)
        fixture.app.send(.saveHistory(upload))
        await fixture.app.flushHistoryWrites()
        await load(fixture.app)
        #expect(fixture.app.state.insights.summary?.totalDictations == 0)
    }

    private func load(_ app: WhisperApplication) async {
        app.send(.loadInsights)
        await settle { !app.state.insights.isLoading }
        #expect(app.state.insights.failure == nil)
    }
    private func localDate(_ year: Int, _ month: Int, _ day: Int) -> Date {
        Calendar.current.date(from: DateComponents(year: year, month: month, day: day, hour: 12))!
    }
    private func entry(words: Int, day: String, duration: Double?) -> HistoryEntry {
        HistoryEntry(text: "Cleaned fixture", rawText: Array(repeating: "word", count: words).joined(separator: " "),
                     localDate: day, audioDuration: duration)
    }
    private func sql(_ profile: NativeProfile, _ sql: String) throws {
        var database: OpaquePointer?
        #expect(sqlite3_open(profile.directory.appendingPathComponent("history.sqlite").path, &database) == SQLITE_OK)
        defer { sqlite3_close(database) }
        #expect(sqlite3_exec(database, sql, nil, nil, nil) == SQLITE_OK)
    }
}
