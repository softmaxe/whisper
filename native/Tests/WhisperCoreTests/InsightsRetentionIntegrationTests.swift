import Foundation
import SQLite3
import Testing
import WhisperCore

@Suite(.serialized) @MainActor struct InsightsRetentionIntegrationTests {
    @Test(arguments: [false, true], [false, true])
    func retainedRecoveryCountsOriginalRawSpeechOnce(discarded: Bool, historyDisabled: Bool) async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        f.app.send(.setHistoryRetention(.init(saveDiscarded: true)))
        _ = await f.begin(seconds: 1.2)
        if discarded { f.app.send(.cancelDictation) }
        else {
            f.app.send(.stopDictation)
            await settle { await f.asr.requests.count == 1 }
            await f.asr.reply(status: 503, body: "fixture failure")
            await settle { f.app.state.dictation.phase == .failed }
        }
        f.app.send(.showDiscardedHistory(true))
        await f.historySettled()
        let original = try #require(f.app.state.history.entries.first)
        #expect(original.hasAudio)
        await load(f.app)
        #expect(f.app.state.insights.summary?.totalWords == 0)
        f.app.send(.setHistoryEnabled(!historyDisabled))
        f.app.send(.saveASR(.init(serverURL: "http://localhost:9234", model: "retry-model"), credential: .unchanged))
        f.app.send(.saveCleanup(.init(serverURL: "http://localhost:8123", model: "cleanup-model"), credential: .unchanged))
        f.app.send(.setTranscriptionLanguage("zh-TW"))
        f.app.send(.saveSnippet(trigger: "這是中文軟體", replacement: "Must not expand"))
        let index = await f.asr.requests.count
        f.clock.advance(2 * 86400)
        f.app.send(.retryHistory(original.id))
        await settle { await f.asr.requests.count == index + 1 }
        let request = await f.asr.requests[index]
        #expect(!request.audio.isEmpty)
        #expect(String(decoding: request.body, as: UTF8.self).contains("retry-model"))
        #expect(!String(decoding: request.body, as: UTF8.self).contains("name=\"prompt\""))
        await f.asr.reply(index, body: #"{"text":"one two three"}"#)
        await settle { await f.cleanup.requests.count == 1 }
        await f.cleanup.reply(content: "这是中文软件")
        await f.retrySettled()
        #expect(try persistedWords(f.profile.profile) == 3)
        let saved = try #require(try await f.store.entry(original.id))
        #expect(saved.rawText == "one two three" && saved.text == "這是中文軟體")
        #expect(saved.id == original.id && saved.occurredAt == original.occurredAt && saved.createdAt == original.createdAt)
        #expect(saved.audioDuration == 1.2)
        await load(f.app)
        #expect(f.app.state.insights.summary?.totalWords == 3)
        #expect(f.app.state.insights.summary?.averageWpm == 150)
        #expect(f.app.state.insights.summary?.daily.first?.date == original.localDate)
        f.app.send(.retryHistory(original.id))
        await settle { await f.asr.requests.count == index + 2 }
        await f.asr.reply(index + 1, body: #"{"text":"later retry has more words"}"#)
        await settle { await f.cleanup.requests.count == 2 }
        await f.cleanup.reply(1, content: "重试文本")
        await f.retrySettled()
        await f.app.prepareForTermination()
        #expect(try persistedWords(f.profile.profile) == 3)
        let reopened = f.reopen()
        await reopened.flushHistoryWrites()
        await load(reopened)
        #expect(reopened.state.insights.summary?.totalDictations == 1)
        #expect(reopened.state.insights.summary?.totalWords == 3)
        #expect(f.paste.pasted.isEmpty && f.clipboard.values.isEmpty)
    }

    @Test func transcriptExpiryUsesCreationAndRemovesCountersWithoutHistoryRows() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let deleted = try await f.record("old deleted words")
        let expired = try await f.record("old retained words")
        let oldAudio = try #require(try await f.store.retainedAudioURL(for: expired.id))
        await load(f.app)
        #expect(f.app.state.insights.summary?.totalWords == 6)
        f.app.send(.deleteHistory(deleted.id))
        await f.historySettled()
        await load(f.app)
        #expect(f.app.state.insights.summary?.totalWords == 6)
        f.clock.advance(8 * 86400)
        let recent = HistoryEntry(text: "Recent save", rawText: "recent old occurrence", occurredAt: deleted.occurredAt,
                                  createdAt: f.clock.wallDate, localDate: deleted.localDate)
        f.app.send(.saveHistory(recent))
        await f.historySettled()
        await load(f.app)
        #expect(f.app.state.insights.summary?.totalWords == 9)
        f.app.send(.setHistoryRetention(.init(transcriptRetentionDays: 7)))
        await f.historySettled()
        await load(f.app)
        #expect(f.app.state.insights.summary?.totalWords == 3)
        #expect(f.app.state.history.entries.map(\.id) == [recent.id])
        #expect(!FileManager.default.fileExists(atPath: oldAudio.path))
        f.app.recordRecoveredDictationInsights(deleted)
        await f.app.flushHistoryWrites()
        await load(f.app)
        #expect(f.app.state.insights.summary?.totalWords == 3)
        f.app.send(.clearHistory)
        await f.app.flushHistoryWrites()
        let reopened = f.reopen()
        await reopened.flushHistoryWrites()
        await load(reopened)
        #expect(reopened.state.insights.summary?.totalWords == 0)
    }

    @Test func audioOnlyExpiryAndZeroNewAudioPreserveSpokenCounts() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let first = try await f.record("first spoken words")
        f.clock.advance(2 * 86400)
        f.app.send(.setHistoryRetention(.init(audioRetentionDays: 1)))
        await f.historySettled()
        await load(f.app)
        #expect(try await f.store.retainedAudioURL(for: first.id) == nil)
        #expect(f.app.state.insights.summary?.totalWords == 3)
        let second = try await f.record("another phrase")
        f.app.send(.clearHistoryAudio)
        await f.historySettled()
        #expect(try await f.store.retainedAudioURL(for: second.id) == nil)
        f.app.send(.setHistoryRetention(.init(audioRetentionDays: 0)))
        let third = try await f.record("no retained audio")
        #expect(!third.hasAudio)
        await load(f.app)
        #expect(f.app.state.insights.summary?.totalWords == 8)
        #expect(f.app.state.insights.summary?.totalDictations == 3)
        #expect(f.app.state.insights.summary?.averageWpm == 133)
        #expect(f.app.state.insights.summary?.wpmCoveragePercent == 100)
    }

    @Test func disabledNewDictationAndUploadCannotEnterCountersOrAudioRetry() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let first = try await f.record("kept words")
        f.app.send(.setHistoryEnabled(false))
        _ = await f.begin(seconds: 1.2)
        f.app.send(.stopDictation)
        await settle { await f.asr.requests.count == 2 }
        await f.asr.reply(1, body: #"{"text":"not retained new speech"}"#)
        await settle { f.app.state.dictation.phase == .result }
        await f.app.flushHistoryWrites()
        await load(f.app)
        #expect(f.app.state.insights.summary?.totalWords == 2)
        #expect(f.app.state.history.totalCount == 1)
        f.app.send(.setHistoryEnabled(true))
        let upload = HistoryEntry(text: "Uncounted uploaded content", source: .upload, audioFileName: first.audioFileName)
        f.app.send(.saveHistory(upload))
        await f.historySettled()
        f.app.send(.retryHistory(upload.id))
        await f.retrySettled()
        #expect(f.app.state.history.retry.failure == .missing)
        #expect(await f.asr.requests.count == 2)
        await load(f.app)
        #expect(f.app.state.insights.summary?.totalWords == 2)
        #expect(f.app.state.insights.summary?.totalDictations == 1)
    }

    @Test(arguments: [false, true])
    func failedCounterDeletionRollsBackMatchingTranscriptDeletion(retention: Bool) async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let original = try await f.record("atomic fixture words")
        await load(f.app)
        try execute(f.profile.profile, "CREATE TRIGGER protect_fixture BEFORE DELETE ON insights_events BEGIN SELECT RAISE(FAIL, 'controlled failure'); END;")
        if retention {
            f.clock.advance(8 * 86400)
            f.app.send(.setHistoryRetention(.init(transcriptRetentionDays: 7)))
        } else { f.app.send(.clearHistory) }
        await f.app.flushHistoryWrites()
        #expect(f.app.state.history.failure == .delete)
        #expect(try await f.store.entry(original.id)?.rawText == original.rawText)
        await load(f.app)
        #expect(f.app.state.insights.summary?.totalWords == 3)
    }

    @Test func terminationFlushesSuccessfulAudioHistoryAndIndependentInsights() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        _ = await f.begin(seconds: 1.2)
        f.app.send(.stopDictation)
        await settle { await f.asr.requests.count == 1 }
        await f.asr.reply(body: #"{"text":"durable raw speech"}"#)
        await settle { f.app.state.dictation.phase == .result }
        let id = try #require(f.app.state.dictation.requestID)
        f.app.send(.loadInsights)
        await f.app.prepareForTermination()
        #expect(!f.app.state.insights.isLoading)
        #expect(try persistedWords(f.profile.profile) == 3)
        let saved = try #require(try await f.store.entry(id))
        #expect(saved.hasAudio && saved.rawText == "durable raw speech")
        let reopened = f.reopen()
        await reopened.flushHistoryWrites()
        await load(reopened)
        #expect(reopened.state.insights.summary?.totalWords == 3)
    }

    private func load(_ app: WhisperApplication) async {
        app.send(.loadInsights)
        await settle { !app.state.insights.isLoading }
        #expect(app.state.insights.failure == nil)
    }
    private func execute(_ profile: NativeProfile, _ sql: String) throws {
        var database: OpaquePointer?
        #expect(sqlite3_open(profile.directory.appendingPathComponent("history.sqlite").path, &database) == SQLITE_OK)
        defer { sqlite3_close(database) }
        #expect(sqlite3_exec(database, sql, nil, nil, nil) == SQLITE_OK)
    }
    private func persistedWords(_ profile: NativeProfile) throws -> Int {
        var database: OpaquePointer?, statement: OpaquePointer?
        #expect(sqlite3_open(profile.directory.appendingPathComponent("history.sqlite").path, &database) == SQLITE_OK)
        defer { sqlite3_finalize(statement); sqlite3_close(database) }
        #expect(sqlite3_prepare_v2(database, "SELECT COALESCE(SUM(word_count), 0) FROM insights_events", -1, &statement, nil) == SQLITE_OK)
        #expect(sqlite3_step(statement) == SQLITE_ROW)
        return Int(sqlite3_column_int64(statement, 0))
    }
}
