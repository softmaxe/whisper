import Foundation
import Testing
import WhisperCore

@Suite(.serialized) @MainActor struct TerminationWorkflowTests {
    @Test(arguments: [false, true], [false, true])
    func quitWaitsForUnretainedCurrentAndPreviouslyCancelledCaptures(cancelFirst: Bool, historyEnabled: Bool) async throws {
        let f = try DictationFixture(); defer { f.remove() }
        f.app.send(.setHistoryEnabled(historyEnabled))
        let capture = await f.record()
        capture.holdRelease()
        let id = try #require(f.app.state.dictation.requestID)
        let directory = f.profile.profile.directory.appendingPathComponent("Temporary/" + id.uuidString)
        if cancelFirst { f.app.send(.cancelDictation) }
        var completed = false
        let termination = Task { await f.app.prepareForTermination(); completed = true }
        await settle { f.app.state.isTerminating }
        try await Task.sleep(for: .milliseconds(20))
        #expect(!completed)
        #expect(capture.physicallyOpen)
        capture.finishRelease()
        await termination.value
        #expect(!capture.physicallyOpen)
        #expect(!FileManager.default.fileExists(atPath: directory.path))
    }

    @Test func quitWaitsForRejectedProvisionalCaptureAndLateAcquisition() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.key()
        let old = try #require(f.microphones.sessions.first)
        f.key(8); f.key(8, down: false); f.key(down: false)
        #expect(f.app.state.dictation.phase == .idle)
        #expect(f.app.state.dictation.cancellation == .rejectedGesture)
        var completed = false
        let termination = Task { await f.app.prepareForTermination(); completed = true }
        await settle { f.app.state.isTerminating }
        try await Task.sleep(for: .milliseconds(20))
        #expect(!completed)
        old.open()
        await termination.value
        #expect(old.releases == 1 && !old.physicallyOpen)
        #expect(await f.transport.requests.isEmpty)
    }

    @Test func quitJoinsRealDictationURLSessionCancellationAndTemporaryAudioRemoval() async throws {
        let server = try LocalHTTPServer(holdResponses: true)
        try await server.start(); defer { server.stop() }
        let transport = ControlledUploadTransport(forwarding: URLSessionFileTransport())
        let f = try DictationFixture(server: "http://127.0.0.1:\(server.port)/v1", transport: transport)
        defer { f.remove() }
        f.app.send(.setHistoryEnabled(false))
        _ = await f.record()
        let id = try #require(f.app.state.dictation.requestID)
        let directory = f.profile.profile.directory.appendingPathComponent("Temporary/" + id.uuidString)
        f.app.send(.stopDictation)
        await settle { server.requests.count == 1 }
        let request = try #require(await transport.requests.first)
        await f.app.prepareForTermination()
        #expect(!FileManager.default.fileExists(atPath: request.bodyFile.deletingLastPathComponent().path))
        #expect(!FileManager.default.fileExists(atPath: directory.path))
    }

    @Test func quitJoinsCancelledASRAndNewRequestWithoutDelayingNormalRetry() async throws {
        let transport = ControlledUploadTransport()
        let f = try DictationFixture(transport: transport); defer { f.remove() }
        f.app.send(.setHistoryEnabled(false))
        for index in 0..<2 {
            _ = await f.record()
            f.app.send(.stopDictation)
            await settle { await transport.requests.count == index + 1 }
            if index == 0 { f.app.send(.cancelDictation) }
        }
        #expect(f.microphones.sessions.count == 2)
        var completed = false
        let termination = Task { await f.app.prepareForTermination(); completed = true }
        await settle { f.app.state.isTerminating }
        try await transport.reply(1)
        try await Task.sleep(for: .milliseconds(20))
        #expect(!completed)
        try await transport.reply(0)
        await termination.value
        for request in await transport.requests {
            #expect(!FileManager.default.fileExists(atPath: request.bodyFile.deletingLastPathComponent().path))
        }
        let temporary = f.profile.profile.directory.appendingPathComponent("Temporary")
        #expect(try FileManager.default.contentsOfDirectory(atPath: temporary.path).isEmpty)
        #expect(f.app.state.dictation.text.isEmpty && f.app.state.history.entries.isEmpty)
    }

    @Test func quitJoinsOldAndCurrentHistoryRetryAudioLeases() async throws {
        let f = try AudioHistoryFixture(); defer { f.remove() }
        let original = try await f.record("Keep original retained result")
        f.app.send(.retryHistory(original.id))
        await settle { await f.asr.requests.count == 2 }
        f.app.send(.retryHistory(original.id))
        await settle { await f.asr.requests.count == 3 }
        var completed = false
        let termination = Task { await f.app.prepareForTermination(); completed = true }
        await settle { f.app.state.isTerminating }
        await f.asr.reply(2)
        try await Task.sleep(for: .milliseconds(20))
        #expect(!completed)
        await f.asr.reply(1)
        await termination.value
        let temporary = f.profile.profile.directory.appendingPathComponent("Temporary")
        #expect(try FileManager.default.contentsOfDirectory(atPath: temporary.path).isEmpty)
        #expect(try await f.store.entry(original.id) == original)
        #expect(try await f.store.retainedAudioURL(for: original.id) != nil)
    }

    @Test func quitJoinsCleanupRequestsWhoseDeadlineWaitersAlreadyEnded() async throws {
        let profile = try ProfileFixture(keychain: false); defer { profile.remove() }
        let http = ControlledCleanupHTTP()
        let app = WhisperApplication(profile: profile.profile, credentials: profile.credentials,
            cleanup: SelfHostedCleanup(transport: http))
        app.send(.saveCleanup(.init(serverURL: "http://localhost:8080", model: "fixture"), credential: .unchanged))
        app.send(.testCleanupPrompt(text: "First draft", prompt: nil))
        await settle { await http.requests.count == 1 }
        app.send(.cancelCleanupPromptTest)
        app.send(.testCleanupPrompt(text: "Second draft", prompt: nil))
        await settle { await http.requests.count == 2 }
        var completed = false
        let termination = Task { await app.prepareForTermination(); completed = true }
        await settle { app.state.isTerminating && !app.state.cleanupTest.isRunning }
        await http.reply(1, content: "Cancelled second reply")
        try await Task.sleep(for: .milliseconds(20))
        #expect(!completed)
        await http.reply(0, content: "Cancelled first reply")
        await termination.value
        #expect(app.state.cleanupTest.text.isEmpty)
        #expect(app.state.cleanupTest.failure == nil)
    }

    @Test(arguments: [false, true])
    func quitWaitsForPasteProbeOrOwnedClipboardRestoration(pendingProbe: Bool) async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        let original = f.paste.clipboard
        f.paste.suspendProbe = pendingProbe
        _ = await f.hold(); await f.submit()
        if pendingProbe { await settle { f.paste.probes == 1 } }
        else { await settle { f.app.state.dictation.phase == .result } }
        var completed = false
        let termination = Task { await f.app.prepareForTermination(); completed = true }
        await settle { f.app.state.isTerminating }
        try await Task.sleep(for: .milliseconds(20))
        #expect(!completed)
        if pendingProbe { f.paste.releaseProbe(true) }
        else { f.clock.advance(0.45) }
        await termination.value
        #expect(f.paste.clipboard == original)
        #expect(f.paste.pasted.count == (pendingProbe ? 0 : 1))
    }
}
