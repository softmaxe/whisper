import Foundation
import Testing
import WhisperCore

@Suite(.serialized) @MainActor struct UploadIntegrationTests {
    @Test(arguments: [false, true])
    func uploadedTextSurvivesAudioRetentionAndStaysOutsideInsights(batch: Bool) async throws {
        let f = try UploadFixture(); defer { f.remove() }
        let source = try UploadFixtures.make("aiff", in: f.sourceDirectory)
        let original = try Data(contentsOf: source)
        await f.transport.setImmediate()
        if batch {
            f.app.send(.chooseUploadFiles([source, source])); f.app.send(.startUploadBatch)
            await settle { !f.app.state.batchUpload.isProcessing }
            await f.app.flushHistoryWrites()
        } else { f.start(source); await f.completed() }
        let ids = batch ? f.app.state.batchUpload.items.compactMap(\.historyID) : [try #require(f.app.state.upload.historyID)]
        #expect(ids.count == (batch ? 2 : 1))
        f.app.send(.loadInsights)
        await settle { !f.app.state.insights.isLoading }
        #expect(f.app.state.insights.summary?.totalWords == 0)
        #expect(f.app.state.insights.summary?.totalDictations == 0)
        f.app.send(.clearHistoryAudio)
        await f.app.flushHistoryWrites()
        let store = HistoryStore(profile: f.profile.profile)
        for id in ids {
            let entry = try #require(try await store.entry(id))
            #expect(entry.source == .upload && entry.text == entry.rawText)
            #expect(!entry.hasAudio && entry.audioDuration == nil)
            #expect(try await store.retainedAudioURL(for: id) == nil)
            f.app.send(.retryHistory(id))
            await settle { !f.app.state.history.retry.isRunning }
            #expect(f.app.state.history.retry.failure != nil)
        }
        #expect(await f.transport.requests.count == ids.count)
        await f.app.prepareForTermination()
        let reopened = f.profile.open()
        reopened.send(.loadHistory); reopened.send(.loadInsights)
        await settle { !reopened.state.history.isLoading && !reopened.state.insights.isLoading }
        #expect(Set(reopened.state.history.entries.map(\.id)) == Set(ids))
        #expect(reopened.state.insights.summary?.totalDictations == 0)
        #expect(try Data(contentsOf: source) == original)
    }

    @Test(arguments: [false, true])
    func terminationJoinsUploadCleanupAndDiscardedCaptureRetention(batch: Bool) async throws {
        let f = try UploadFixture(); defer { f.remove() }
        f.app.send(.setHistoryRetention(.init(saveDiscarded: true)))
        f.app.send(.startDictation)
        let capture = try #require(f.microphones.sessions.first)
        capture.open()
        capture.deliver([Float](repeating: 0.1, count: 57_600))
        await settle { f.app.state.dictation.phase == .recording && f.app.state.dictation.duration >= 1.2 }
        let recordingID = try #require(f.app.state.dictation.requestID)
        capture.holdRelease()
        let source = try UploadFixtures.make("wav", in: f.sourceDirectory)
        if batch { f.app.send(.chooseUploadFiles([source, source])); f.app.send(.startUploadBatch) }
        else { f.start(source) }
        await settle { await f.transport.requests.count == 1 }
        let body = try #require(await f.transport.requests.first?.bodyFile)
        var finished = false
        let shutdown = Task { await f.app.prepareForTermination(); finished = true }
        await settle { f.app.state.isTerminating }
        f.app.send(.selectUpload(source)); f.app.send(.startUpload)
        f.app.send(.chooseUploadFiles([source, source])); f.app.send(.startUploadBatch)
        f.app.send(.startDictation)
        #expect(f.microphones.sessions.count == 1)
        try await f.transport.reply(text: "Cancelled Upload must not persist")
        await settle { !FileManager.default.fileExists(atPath: body.deletingLastPathComponent().path) }
        #expect(!finished && capture.physicallyOpen)
        capture.finishRelease()
        await shutdown.value
        #expect(finished && !capture.physicallyOpen)
        #expect(!f.app.state.batchUpload.isProcessing)
        #expect(f.app.state.batchUpload.items.count == (batch ? 2 : 0))
        #expect(f.app.state.batchUpload.items.allSatisfy { $0.status == .cancelled })
        #expect(f.app.state.history.pendingChanges == 0 && f.app.state.insights.pendingChanges == 0)
        let store = HistoryStore(profile: f.profile.profile)
        let saved = try #require(try await store.entry(recordingID))
        #expect(saved.status == .discarded && saved.hasAudio)
        #expect(try await store.retainedAudioURL(for: recordingID) != nil)
        let reopened = f.profile.open()
        reopened.send(.showDiscardedHistory(true)); reopened.send(.loadInsights)
        await settle { !reopened.state.history.isLoading && !reopened.state.insights.isLoading }
        #expect(reopened.state.history.entries.map(\.id) == [recordingID])
        #expect(reopened.state.insights.summary?.totalDictations == 0)
    }
}
