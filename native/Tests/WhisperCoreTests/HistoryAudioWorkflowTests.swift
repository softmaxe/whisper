import AVFoundation
import Foundation
import Testing
import WhisperCore

@Suite(.serialized) @MainActor
struct HistoryAudioWorkflowTests {
    @Test func terminationWaitsForAcceptedAudioAndRejectsNewCommands() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        f.app.send(.setHistoryRetention(.init(saveDiscarded: true)))
        await f.historySettled()
        let capture = await f.begin(seconds: 1.2)
        let id = try #require(f.app.state.dictation.requestID)
        capture.holdRelease()
        var terminated = false
        let shutdown = Task { await f.app.prepareForTermination(); terminated = true }
        await settle { f.app.state.isTerminating && f.app.state.history.pendingChanges == 1 }
        f.app.send(.startDictation)
        f.app.send(.clearHistory)
        #expect(f.microphones.sessions.count == 1)
        #expect(!terminated)
        do {
            _ = try await f.app.recordHistory(HistoryEntry(text: "Late producer"))
            Issue.record("A stopped producer was allowed to enqueue new persistence.")
        } catch is CancellationError {} catch { Issue.record("Unexpected termination error.") }
        capture.finishRelease()
        await shutdown.value
        #expect(terminated)
        #expect(!capture.physicallyOpen)
        let reopened = f.reopen()
        reopened.send(.loadHistory)
        reopened.send(.showDiscardedHistory(true))
        await reopened.flushHistoryWrites()
        await settle { !reopened.state.history.isLoading && reopened.state.history.entries.contains { $0.id == id } }
        let saved = try #require(reopened.state.history.entries.first { $0.id == id })
        #expect(saved.status == .discarded)
        #expect(saved.hasAudio)
        #expect(saved.audioDuration == 1.2)
    }

    @Test func recordingPersistsAudioAndRetryUsesCurrentProcessingWithoutLiveDelivery() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let original = try await f.record("Original transcript")
        let url = try #require(try await f.store.retainedAudioURL(for: original.id))
        #expect(try AVAudioFile(forReading: url).length > 48_000)
        #expect(original.audioDuration == 1.2)
        #expect(f.microphones.sessions.allSatisfy { !$0.physicallyOpen })
        f.app = f.reopen()
        f.app.send(.saveASR(.init(serverURL: "http://localhost:9234/custom", model: "current-model"), credential: .unchanged))
        f.app.send(.setTranscriptionLanguage("zh-TW"))
        f.app.send(.importDictionary("OpenWhispr"))
        f.app.send(.saveSnippet(trigger: "這是中文軟體", replacement: "Must not expand"))
        f.app.send(.saveCleanup(.init(serverURL: "http://localhost:8123", model: "current-cleanup"), credential: .unchanged))
        f.app.send(.retryHistory(original.id))
        await settle { await f.asr.requests.count == 2 }
        let request = await f.asr.requests[1]
        let body = String(decoding: request.body, as: UTF8.self)
        #expect(request.request.url?.absoluteString == "http://localhost:9234/custom/audio/transcriptions")
        #expect(body.contains("name=\"model\"\r\n\r\ncurrent-model"))
        #expect(body.contains("name=\"language\"\r\n\r\nzh"))
        #expect(!body.contains("name=\"prompt\""))
        #expect(!request.audio.isEmpty)
        await f.asr.reply(1, body: #"{"text":"Raw retry remains separate"}"#)
        await settle { await f.cleanup.requests.count == 1 }
        let cleanupJSON = String(decoding: try #require(await f.cleanup.requests[0].httpBody), as: UTF8.self)
        #expect(cleanupJSON.contains("OpenWhispr, 這是中文軟體"))
        #expect(!cleanupJSON.contains("Must not expand"))
        await f.cleanup.reply(content: "这是中文软件")
        await f.retrySettled()
        let saved = try #require(try await f.store.entry(original.id))
        #expect(saved.id == original.id && saved.occurredAt == original.occurredAt && saved.createdAt == original.createdAt)
        #expect(saved.source == .dictation && saved.model == "current-model")
        #expect(saved.text == "這是中文軟體" && saved.rawText == "Raw retry remains separate")
        #expect(saved.audioFileName == original.audioFileName)
        #expect(f.paste.writes.isEmpty && f.paste.pasted.isEmpty && f.clipboard.values.isEmpty)
        let reopened = f.reopen()
        reopened.send(.loadHistory)
        await settle { reopened.state.history.isLoaded && !reopened.state.history.isLoading }
        #expect(reopened.state.history.totalCount == 1)
        #expect(reopened.state.history.entries == [saved])
    }

    @Test(arguments: [false, true])
    func failedAndCancelledRetryKeepThePreviousEntry(cancel: Bool) async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let original = try await f.record("Keep the previous entry")
        f.app.send(.retryHistory(original.id))
        await settle { await f.asr.requests.count == 2 }
        if cancel { f.app.send(.cancelHistoryRetry) }
        await f.asr.reply(1, status: cancel ? 200 : 503, body: #"{"text":"Late replacement"}"#)
        await f.retrySettled()
        #expect(try await f.store.entry(original.id) == original)
        #expect(f.paste.writes.isEmpty)
        if !cancel { #expect(f.app.state.history.retry.failure == .retry) }
    }

    @Test(arguments: ["delete", "clear", "transcript-expiry", "audio-expiry"])
    func retryCannotResurrectDeletedOrExpiredData(operation: String) async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let original = try await f.record("Before deletion")
        f.app.send(.retryHistory(original.id))
        await settle { await f.asr.requests.count == 2 }
        switch operation {
        case "delete": f.app.send(.deleteHistory(original.id))
        case "clear": f.app.send(.clearHistory)
        default:
            f.clock.advance(2 * 86400)
            f.app.send(.setHistoryRetention(.init(audioRetentionDays: operation == "audio-expiry" ? 1 : 30,
                                                  transcriptRetentionDays: operation == "transcript-expiry" ? 1 : 0)))
        }
        await f.app.flushHistoryWrites()
        await f.asr.reply(1, body: #"{"text":"Must not resurrect"}"#)
        await f.retrySettled()
        let saved = try await f.store.entry(original.id)
        if operation == "audio-expiry" {
            #expect(saved?.text == original.text && saved?.hasAudio == false)
        } else { #expect(saved == nil) }
        #expect(f.paste.writes.isEmpty)
    }

    @Test func missingAudioDoesNotSendARequestOrReplaceText() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let original = try await f.record("Text survives external audio removal")
        try FileManager.default.removeItem(at: #require(try await f.store.retainedAudioURL(for: original.id)))
        f.app.send(.retryHistory(original.id))
        await f.retrySettled()
        #expect(f.app.state.history.retry.failure == .missing)
        #expect(await f.asr.requests.count == 1)
        #expect(try await f.store.entry(original.id)?.text == original.text)
    }

    @Test func newerRetryCannotBeOverwrittenByTheCancelledResponse() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let original = try await f.record("Original")
        f.app.send(.retryHistory(original.id))
        await settle { await f.asr.requests.count == 2 }
        f.app.send(.retryHistory(original.id))
        await settle { await f.asr.requests.count == 3 }
        await f.asr.reply(2, body: #"{"text":"Current retry result"}"#)
        await f.retrySettled()
        await f.asr.reply(1, body: #"{"text":"Old retry result"}"#)
        await f.historySettled()
        #expect(try await f.store.entry(original.id)?.text == "Current retry result")
        #expect(f.app.state.history.totalCount == 1)
    }

    @Test func cleanupFailureKeepsRawRetryAndChineseConversionStillRuns() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let original = try await f.record("Original")
        f.app.send(.setTranscriptionLanguage("zh-TW"))
        f.app.send(.saveCleanup(.init(serverURL: "http://localhost"), credential: .unchanged))
        f.app.send(.retryHistory(original.id))
        await settle { await f.asr.requests.count == 2 }
        await f.asr.reply(1, body: #"{"text":"这是中文软件"}"#)
        await settle { await f.cleanup.requests.count == 1 }
        await f.cleanup.respond(0, status: 401, body: "fixture rejection")
        await f.retrySettled()
        let saved = try #require(try await f.store.entry(original.id))
        #expect(saved.text == "這是中文軟體" && saved.rawText == "这是中文软件")
        #expect(f.app.state.history.retry.cleanupFailure == .service(401))
        #expect(f.paste.pasted.isEmpty)
    }

    @Test func cancellationDuringCleanupPreservesPriorRowAndIgnoresLateOutput() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let original = try await f.record("Previous row")
        f.app.send(.saveCleanup(.init(serverURL: "http://localhost"), credential: .unchanged))
        f.app.send(.retryHistory(original.id))
        await settle { await f.asr.requests.count == 2 }
        await f.asr.reply(1, body: #"{"text":"New raw response"}"#)
        await settle { await f.cleanup.requests.count == 1 }
        f.app.send(.cancelHistoryRetry)
        await f.cleanup.reply(content: "Late cleanup")
        await f.cleanup.waitUntilReturned(1)
        await f.retrySettled()
        #expect(try await f.store.entry(original.id) == original)
    }

    @Test(arguments: [(true, true, 30, 1.2, true), (true, true, 30, 1.0, true), (true, true, 30, 0.999, false), (true, true, 30, 0.5, false),
                      (false, true, 30, 1.2, false), (true, false, 30, 1.2, false), (true, true, 0, 1.2, false)])
    func ordinaryCancellationHonorsAllRetentionGates(discarded: Bool, history: Bool, days: Int, seconds: Double, saved: Bool) async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        f.app.send(.setHistoryRetention(.init(enabled: history, audioRetentionDays: days, saveDiscarded: discarded)))
        let capture = await f.begin(seconds: seconds)
        f.app.send(.cancelDictation)
        #expect(!capture.physicallyOpen)
        f.app.send(.showDiscardedHistory(true))
        await f.historySettled()
        #expect(f.app.state.history.totalCount == (saved ? 1 : 0))
        if saved {
            let entry = try #require(f.app.state.history.entries.first)
            #expect(entry.status == .discarded && entry.text.isEmpty && entry.hasAudio)
            #expect(try await f.store.retainedAudioURL(for: entry.id) != nil)
        }
        #expect(await f.asr.requests.isEmpty)
    }

    @Test(arguments: [false, true])
    func rejectedShortcutNeverRetainsEvenLongProvisionalAudio(commandCombination: Bool) async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        f.app.send(.setHistoryRetention(.init(saveDiscarded: true)))
        f.app.send(.shortcut(.init(keyCode: ShortcutInput.rightCommand, isDown: true)))
        let capture = try #require(f.microphones.sessions.last)
        capture.open(); capture.deliver([Float](repeating: 0.1, count: 60_000))
        await settle { f.app.state.dictation.timing["firstAudio"] != nil }
        if commandCombination {
            f.clock.advance(WhisperApplication.holdThreshold)
            f.app.send(.shortcut(.init(keyCode: 8, isDown: true)))
            f.app.send(.shortcut(.init(keyCode: 8, isDown: false)))
        } else { f.clock.advance(0.04) }
        f.app.send(.shortcut(.init(keyCode: ShortcutInput.rightCommand, isDown: false)))
        f.clock.advance(WhisperApplication.doubleTapWindow)
        await f.historySettled()
        #expect(f.app.state.dictation.cancellation == .rejectedGesture)
        #expect(f.app.state.history.totalCount == 0 && !capture.physicallyOpen)
        #expect(await f.asr.requests.isEmpty)
        await settle {
            let path = f.profile.profile.directory.appendingPathComponent("Temporary").path
            return ((try? FileManager.default.contentsOfDirectory(atPath: path)) ?? []).isEmpty
        }
    }

    @Test func ordinaryProcessingCancellationRetainsOneDiscardAndIgnoresLateASR() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        f.app.send(.setHistoryRetention(.init(saveDiscarded: true)))
        let capture = await f.begin(seconds: 1.2)
        f.app.send(.stopDictation)
        await settle { await f.asr.requests.count == 1 }
        f.app.send(.cancelDictation)
        #expect(!capture.physicallyOpen && f.app.state.dictation.phase == .idle)
        f.app.send(.showDiscardedHistory(true))
        await f.historySettled()
        let entry = try #require(f.app.state.history.entries.first)
        #expect(entry.status == .discarded && entry.hasAudio)
        await f.asr.reply(body: #"{"text":"Late cancelled result"}"#)
        await f.historySettled()
        #expect(f.app.state.history.entries == [entry])
        #expect(f.app.state.dictation.text.isEmpty)
    }

    @Test func retentionPreferencesPersistAndOlderHistorySettingsUseSafeDefaults() throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let preferences = HistoryPreferences(enabled: false, audioRetentionDays: 90, transcriptRetentionDays: 14, saveDiscarded: true)
        f.app.send(.setHistoryRetention(preferences))
        #expect(f.reopen().state.settings.history == preferences)
        f.app.send(.setHistoryEnabled(true))
        #expect(f.reopen().state.settings.history.audioRetentionDays == 90)
        let older = #"{"version":1,"settings":{"language":"en","asr":{"serverURL":"http://localhost","model":"fixture"},"history":{"enabled":false}}}"#
        try Data(older.utf8).write(to: f.profile.profile.directory.appendingPathComponent("settings.json"))
        #expect(f.reopen().state.settings.history == HistoryPreferences(enabled: false))
    }

    @Test func uploadRowsNeverAdvertiseAudioOrEnterRetry() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let original = try await f.record("Dictation audio stays separate")
        let upload = HistoryEntry(text: "Raw Upload", source: .upload, audioFileName: original.audioFileName)
        f.app.send(.saveHistory(upload))
        await f.historySettled()
        #expect(try await f.store.entry(upload.id)?.audioFileName == nil)
        f.app.send(.retryHistory(upload.id))
        await f.retrySettled()
        #expect(f.app.state.history.retry.failure == .missing)
        #expect(await f.asr.requests.count == 1)
        #expect(try await f.store.retainedAudioURL(for: original.id) != nil)
    }

    @Test func failedDictationRetainsRecoverableAudioAndRetryPreservesOccurrence() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let occurredAt = f.clock.wallDate
        _ = await f.begin(seconds: 1.2)
        f.app.send(.stopDictation)
        await settle { await f.asr.requests.count == 1 }
        await f.asr.reply(status: 503, body: "fixture failure")
        await settle { f.app.state.dictation.phase == .failed }
        f.app.send(.showDiscardedHistory(true))
        await f.historySettled()
        let failed = try #require(f.app.state.history.entries.first)
        #expect(failed.status == .failed && failed.hasAudio && failed.errorCode == "HTTP_503")
        f.app.send(.retryHistory(failed.id))
        await settle { await f.asr.requests.count == 2 }
        await f.asr.reply(1, body: #"{"text":"Recovered transcript"}"#)
        await f.retrySettled()
        let recovered = try #require(try await f.store.entry(failed.id))
        #expect(recovered.status == .completed && recovered.text == "Recovered transcript")
        #expect(recovered.occurredAt == occurredAt && recovered.createdAt == failed.createdAt && recovered.errorCode == nil)
    }

    @Test(arguments: [false, true])
    func audioWriteFailureKeepsSuccessfulTextButDoesNotLeaveAnEmptyDiscard(discard: Bool) async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        f.app.send(.setHistoryRetention(.init(saveDiscarded: true)))
        await f.app.flushHistoryWrites()
        try Data("audio-directory-blocker".utf8).write(to: f.profile.profile.directory.appendingPathComponent("Audio"))
        _ = await f.begin(seconds: 1.2)
        if discard { f.app.send(.cancelDictation) }
        else {
            f.app.send(.stopDictation)
            await settle { await f.asr.requests.count == 1 }
            await f.asr.reply(body: #"{"text":"Text remains usable"}"#)
            await settle { f.app.state.dictation.phase == .result }
        }
        f.app.send(.showDiscardedHistory(true))
        await f.historySettled()
        #expect(f.app.state.history.totalCount == (discard ? 0 : 1))
        #expect(f.app.state.history.audioFailure == .save)
        if !discard {
            #expect(f.app.state.history.entries.first?.hasAudio == false)
            f.app.send(.copyDictationResult)
            #expect(f.clipboard.values == ["Text remains usable"])
        }
        try FileManager.default.removeItem(at: f.profile.profile.directory.appendingPathComponent("Audio"))
        let next = try await f.record("Audio can be saved after recovery")
        #expect(next.hasAudio)
        #expect(f.app.state.history.audioFailure == nil)
    }

    @Test func savedRetentionLoadsBeforeStartupCleanupAndAudioExpiryKeepsText() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        f.app.send(.setHistoryRetention(.init(audioRetentionDays: 90)))
        let entry = try await f.record("Retain longer than defaults")
        let url = try #require(try await f.store.retainedAudioURL(for: entry.id))
        try FileManager.default.setAttributes([.modificationDate: f.clock.wallDate.addingTimeInterval(-60 * 86400)], ofItemAtPath: url.path)
        f.app = f.reopen()
        await f.historySettled()
        #expect(try await f.store.retainedAudioURL(for: entry.id) != nil)
        f.app.send(.setHistoryRetention(.init(audioRetentionDays: 30)))
        await f.historySettled()
        #expect(try await f.store.retainedAudioURL(for: entry.id) == nil)
        #expect(try await f.store.entry(entry.id)?.text == entry.text)
        #expect(f.app.state.history.entries.first?.hasAudio == false)
    }

    @Test func transcriptExpiryRemovesMatchingAudioWhileZeroMeansForever() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let entry = try await f.record("Old transcript")
        let url = try #require(try await f.store.retainedAudioURL(for: entry.id))
        f.clock.advance(8 * 86400)
        f.app.send(.runHistoryRetention)
        await f.historySettled()
        #expect(try await f.store.entry(entry.id) != nil)
        f.app.send(.setHistoryRetention(.init(transcriptRetentionDays: 7)))
        await f.historySettled()
        #expect(try await f.store.entry(entry.id) == nil)
        #expect(!FileManager.default.fileExists(atPath: url.path))
        #expect(f.app.state.history.retentionCutoff == f.clock.wallDate.addingTimeInterval(-7 * 86400))
    }

    @Test func zeroAudioDaysSkipsNewAudioWithoutErasingExistingFiles() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let original = try await f.record("Existing audio")
        f.app.send(.setHistoryRetention(.init(audioRetentionDays: 0)))
        let next = try await f.record("Text without audio")
        #expect(!next.hasAudio)
        #expect(try await f.store.retainedAudioURL(for: original.id) != nil)
        f.app.send(.clearHistoryAudio)
        await f.historySettled()
        #expect(f.app.state.history.totalCount == 2)
        #expect(f.app.state.history.entries.allSatisfy { !$0.hasAudio })
        #expect(try await f.store.retainedAudioURL(for: original.id) == nil)
    }

    @Test func playbackRevealAndDeleteUseOnlyControlledSystemEffects() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let entry = try await f.record("Playback fixture")
        let url = try #require(try await f.store.retainedAudioURL(for: entry.id))
        f.app.send(.playHistory(entry.id))
        await settle { f.app.state.history.playingID == entry.id }
        #expect(f.audio.played == [url])
        f.audio.finish()
        #expect(f.app.state.history.playingID == nil)
        f.app.send(.revealHistoryAudio(entry.id))
        await settle { f.audio.revealed == [url] }
        f.app.send(.deleteHistory(entry.id))
        await f.historySettled()
        #expect(!FileManager.default.fileExists(atPath: url.path))
        #expect(try await f.store.entry(entry.id) == nil)
    }
}

@MainActor private final class ControlledHistoryAudio: HistoryAudioSystem {
    var played: [URL] = []
    var revealed: [URL] = []
    var completion: (@MainActor @Sendable () -> Void)?
    func play(_ url: URL, completion: @escaping @MainActor @Sendable () -> Void) -> Bool {
        played.append(url); self.completion = completion; return true
    }
    func stop() { completion = nil }
    func reveal(_ url: URL) { revealed.append(url) }
    func finish() { let done = completion; completion = nil; done?() }
}

@MainActor private final class AudioHistoryFixture {
    let profile: ProfileFixture
    let microphones = ControlledMicrophones()
    let clock = ControlledClock()
    let asr = ControlledHTTPTransport()
    let cleanup = ControlledCleanupHTTP()
    let clipboard = ControlledClipboard()
    let paste = ControlledPasteSystem()
    let audio = ControlledHistoryAudio()
    let store: HistoryStore
    var app: WhisperApplication!
    init() throws {
        profile = try ProfileFixture(keychain: false)
        store = HistoryStore(profile: profile.profile)
        app = reopen()
        app.send(.saveASR(.init(serverURL: "http://localhost:8178", model: "original-model"), credential: .unchanged))
    }
    func reopen() -> WhisperApplication {
        WhisperApplication(profile: profile.profile, credentials: profile.credentials, microphones: microphones,
            transcriber: SelfHostedTranscriber(transport: asr), clock: clock, clipboard: clipboard,
            pasteSystem: paste, cleanup: SelfHostedCleanup(transport: cleanup), audioSystem: audio)
    }
    func begin(seconds: Double) async -> ControlledCapture {
        app.send(.startDictation)
        let capture = microphones.sessions.last!
        capture.open()
        capture.deliver([Float](repeating: 0.1, count: Int(seconds * 48_000)))
        await settle { self.app.state.dictation.phase == .recording }
        return capture
    }
    func record(_ text: String) async throws -> HistoryEntry {
        let index = await asr.requests.count
        _ = await begin(seconds: 1.2)
        app.send(.stopDictation)
        await settle { await self.asr.requests.count == index + 1 }
        await asr.reply(index, body: String(decoding: try JSONEncoder().encode(["text": text]), as: UTF8.self))
        await settle { self.app.state.dictation.phase == .result }
        await historySettled()
        return try #require(app.state.history.entries.first { $0.id == self.app.state.dictation.requestID })
    }
    func historySettled() async {
        await app.flushHistoryWrites()
        app.send(.loadHistory)
        await settle { self.app.state.history.isLoaded && !self.app.state.history.isLoading && self.app.state.history.pendingChanges == 0 }
    }
    func retrySettled() async {
        for _ in 0..<5000 {
            if !app.state.history.retry.isRunning { break }
            try? await Task.sleep(for: .milliseconds(2))
        }
        #expect(!app.state.history.retry.isRunning)
        await historySettled()
    }
    func remove() {
        app.send(.cancelDictation); app.send(.cancelHistoryRetry); app.send(.stopHistoryPlayback)
        clock.advance(10)
        profile.remove()
    }
}
