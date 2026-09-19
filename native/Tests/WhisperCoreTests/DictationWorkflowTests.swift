import Foundation
import Testing
import WhisperCore

@Suite(.serialized) @MainActor
struct DictationWorkflowTests {
    @Test func firstSilentFrameIsReadyAndOpeningAudioReachesASRInOrder() async throws {
        let f = try DictationFixture(credential: "native-asr-placeholder")
        defer { f.remove() }
        let capture = f.begin()
        #expect(f.app.state.dictation.phase == .preparing)
        capture.open()
        await settle { f.app.state.dictation.timing["captureConfigured"] != nil }
        f.clock.advance(0.2)
        capture.deliver([])
        #expect(f.app.state.dictation.phase == .preparing)
        capture.deliver([Float](repeating: 0, count: 4800))
        await settle { f.app.state.dictation.phase == .recording }
        #expect(f.app.state.dictation.level == 0)
        let opening = (0..<4800).map { sin(Float($0) * 0.04) * 0.5 }
        capture.deliver(opening)
        capture.deliver([Float](repeating: 0, count: 4800))
        f.app.send(.stopDictation)
        #expect(f.app.state.dictation.phase == .processing)
        await settle { await f.transport.requests.count == 1 }
        #expect(!capture.physicallyOpen)
        #expect(capture.releases == 1)
        let sent = try #require(await f.transport.requests.first)
        #expect(sent.request.url?.absoluteString == "http://localhost:8178/v1/audio/transcriptions?route=fixture")
        #expect(sent.request.value(forHTTPHeaderField: "Authorization") == "Bearer native-asr-placeholder")
        let body = String(decoding: sent.body, as: UTF8.self)
        #expect(body.contains("name=\"model\"\r\n\r\nwhisper-fixture"))
        #expect(body.contains("filename=\"audio.m4a\"\r\nContent-Type: audio/mp4"))
        #expect(!body.contains("name=\"language\""))
        #expect(sent.audio.count >= 14_000)
        #expect(sent.audio.prefix(3500).allSatisfy { abs($0) < 0.01 })
        #expect(sent.audio[6000..<8000].contains { abs($0) > 0.2 })
        #expect(sent.audio.suffix(3000).allSatisfy { abs($0) < 0.01 })
        await f.transport.reply(body: "{\"text\":\"  Opening words preserved.  \"}")
        await settle { f.app.state.dictation.phase == .result }
        #expect(f.app.state.dictation.rawText == "  Opening words preserved.  ")
        f.app.send(.copyDictationResult)
        #expect(f.clipboard.values == ["  Opening words preserved.  "])
        #expect(f.app.state.dictation.resultCopied)
        #expect(f.app.state.dictation.timing["firstAudio"] == 0.2)
        await settle { (try? FileManager.default.contentsOfDirectory(atPath: f.profile.profile.directory.appendingPathComponent("Temporary").path).isEmpty) == true }
    }

    @Test func noFrameDeadlineBeginsAfterAcquisitionAndSilenceCancelsIt() async throws {
        let f = try DictationFixture()
        defer { f.remove() }
        let first = f.begin()
        f.clock.advance(60)
        #expect(f.app.state.dictation.phase == .preparing)
        first.open()
        await settle { f.app.state.dictation.timing["captureConfigured"] != nil }
        f.clock.advance(9.9)
        #expect(f.app.state.dictation.phase == .preparing)
        f.clock.advance(0.1)
        #expect(f.app.state.dictation.failure == .noAudio)
        #expect(!first.physicallyOpen)
        #expect(await f.transport.requests.isEmpty)
        let next = await f.record()
        f.clock.advance(600)
        #expect(f.app.state.dictation.phase == .recording)
        f.app.send(.cancelDictation)
        #expect(!next.physicallyOpen)
    }

    @Test func stopBeforeReadyAndLateAcquisitionDoNotBlockIndependentRetry() async throws {
        let f = try DictationFixture()
        defer { f.remove() }
        let old = f.begin()
        f.app.send(.stopDictation)
        #expect(f.app.state.dictation.phase == .idle)
        let current = await f.record()
        let id = f.app.state.dictation.requestID
        old.open()
        old.deliver([Float](repeating: 1, count: 4800))
        old.fail(.inputUnavailable)
        await Task.yield()
        #expect(!old.physicallyOpen)
        #expect(old.releases == 1)
        #expect(current.physicallyOpen)
        #expect(f.app.state.dictation.requestID == id)
        #expect(f.app.state.dictation.phase == .recording)
        #expect(await f.transport.requests.isEmpty)
    }

    @Test(arguments: [true, false]) func lateASRSuccessAndFailureCannotReviveCancellation(success: Bool) async throws {
        let f = try DictationFixture()
        defer { f.remove() }
        _ = await f.record()
        f.app.send(.stopDictation)
        await settle { await f.transport.requests.count == 1 }
        f.app.send(.cancelDictation)
        if success { await f.transport.reply() } else { await f.transport.fail() }
        try await Task.sleep(for: .milliseconds(20))
        #expect(f.app.state.dictation.phase == .idle)
        #expect(f.app.state.dictation.rawText.isEmpty)
        #expect(f.app.state.dictation.failure == nil)
    }

    @Test(arguments: [true, false]) func oldASRCompletionCannotOverwriteNewRecording(success: Bool) async throws {
        let f = try DictationFixture()
        defer { f.remove() }
        _ = await f.record()
        f.app.send(.stopDictation)
        await settle { await f.transport.requests.count == 1 }
        f.app.send(.cancelDictation)
        let current = await f.record()
        let id = f.app.state.dictation.requestID
        if success { await f.transport.reply() } else { await f.transport.fail() }
        try await Task.sleep(for: .milliseconds(20))
        #expect(f.app.state.dictation.phase == .recording)
        #expect(f.app.state.dictation.requestID == id)
        #expect(current.physicallyOpen)
        #expect(f.app.state.dictation.rawText.isEmpty)
    }

    @Test(arguments: [(503, "{}", DictationFailure.service(503)), (200, "{}", .invalidResponse), (200, "{\"text\":\"  \"}", .emptyTranscript)])
    func recoverableFailuresReleaseCapture(status: Int, body: String, failure: DictationFailure) async throws {
        let f = try DictationFixture()
        defer { f.remove() }
        let capture = await f.record()
        f.app.send(.stopDictation)
        await settle { await f.transport.requests.count == 1 }
        await f.transport.reply(status: status, body: body)
        await settle { f.app.state.dictation.phase == .failed }
        #expect(f.app.state.dictation.failure == failure)
        #expect(!capture.physicallyOpen)
        #expect(!failure.message(in: .english).isEmpty)
        #expect(!failure.message(in: .simplifiedChinese).isEmpty)
        let retry = await f.record()
        #expect(retry.physicallyOpen)
    }

    @Test func unavailableDeviceAndStorageFailureRemainRecoverable() async throws {
        let f = try DictationFixture()
        defer { f.remove() }
        f.microphones.rejection = .inputUnavailable
        f.app.send(.startDictation)
        #expect(f.app.state.dictation.failure == .inputUnavailable)
        #expect(f.microphones.sessions.isEmpty)
        f.microphones.rejection = nil
        let temporary = f.profile.profile.directory.appendingPathComponent("Temporary")
        try Data("blocked-directory-fixture".utf8).write(to: temporary)
        let capture = f.begin()
        capture.open()
        capture.deliver([Float](repeating: 0, count: 4800))
        await settle { f.app.state.dictation.phase == .failed }
        #expect(f.app.state.dictation.failure == .storageFailed)
        #expect(!capture.physicallyOpen)
        #expect(await f.transport.requests.isEmpty)
        try FileManager.default.removeItem(at: temporary)
        _ = await f.record()
        #expect(f.app.state.dictation.phase == .recording)
    }

    @Test func applicationTeardownReleasesActiveAndLateCapture() async throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let microphones = ControlledMicrophones()
        var app: WhisperApplication? = WhisperApplication(profile: profile.profile, credentials: profile.credentials, microphones: microphones)
        app?.send(.saveASR(.init(serverURL: "http://localhost:8178", model: "fixture"), credential: .unchanged))
        app?.send(.startDictation)
        let capture = try #require(microphones.sessions.first)
        weak var weakApp = app
        app = nil
        #expect(weakApp == nil)
        capture.open()
        #expect(!capture.physicallyOpen)
        #expect(capture.releases == 1)
    }

    @Test func deviceFailureDoesNotSubstituteAndProcessingStartIsIgnored() async throws {
        let f = try DictationFixture()
        defer { f.remove() }
        let first = await f.record()
        first.fail(.inputUnavailable)
        await settle { f.app.state.dictation.phase == .failed }
        #expect(f.microphones.sessions.count == 1)
        #expect(!first.physicallyOpen)
        let next = await f.record()
        f.app.send(.stopDictation)
        let id = f.app.state.dictation.requestID
        f.app.send(.startDictation)
        #expect(f.microphones.sessions.count == 2)
        #expect(f.app.state.dictation.requestID == id)
        await settle { await f.transport.requests.count == 1 }
        let sent = try #require(await f.transport.requests.first)
        #expect(sent.request.value(forHTTPHeaderField: "Authorization") == nil)
        await f.transport.reply()
        await settle { f.app.state.dictation.phase == .result }
        #expect(!next.physicallyOpen)
    }
}

@Suite(.serialized) @MainActor
struct DictationHTTPTests {
    @Test(arguments: ["127.0.0.1", "localhost"]) func actualURLSessionUploadsToLocalHTTP(host: String) async throws {
        let server = try LocalHTTPServer()
        try await server.start()
        defer { server.stop() }
        let f = try DictationFixture(server: "http://\(host):\(server.port)/custom?fixture=1", transport: URLSessionFileTransport())
        defer { f.remove() }
        let capture = await f.record()
        f.app.send(.stopDictation)
        await settle { f.app.state.dictation.phase == .result || f.app.state.dictation.phase == .failed }
        #expect(f.app.state.dictation.failure == nil)
        #expect(f.app.state.dictation.rawText == "Loopback HTTP transcript")
        #expect(!capture.physicallyOpen)
        let request = String(decoding: try #require(server.requests.first), as: UTF8.self)
        #expect(request.hasPrefix("POST /custom/audio/transcriptions?fixture=1 HTTP/1.1"))
        #expect(request.contains("name=\"model\"\r\n\r\nwhisper-fixture"))
        #expect(request.contains("Content-Type: audio/mp4"))
    }

    @Test func sameOriginRedirectPreservesMultipartAndCredential() async throws {
        let server = try LocalHTTPServer(redirectPath: "/normalized/audio/transcriptions/")
        try await server.start()
        defer { server.stop() }
        let f = try DictationFixture(server: "http://127.0.0.1:\(server.port)", credential: "redirect-placeholder", transport: URLSessionFileTransport())
        defer { f.remove() }
        _ = await f.record()
        f.app.send(.stopDictation)
        await settle { f.app.state.dictation.phase == .result || f.app.state.dictation.phase == .failed }
        #expect(f.app.state.dictation.failure == nil)
        #expect(f.app.state.dictation.text == "Loopback HTTP transcript")
        #expect(server.requests.count == 2)
        let request = String(decoding: try #require(server.requests.last), as: UTF8.self)
        #expect(request.hasPrefix("POST /normalized/audio/transcriptions/ HTTP/1.1"))
        #expect(request.lowercased().contains("authorization: bearer redirect-placeholder"))
        #expect(request.contains("Content-Type: audio/mp4"))
    }

    @Test func crossOriginRedirectsNeverForwardAudioOrCredentials() async throws {
        let destination = try LocalHTTPServer()
        try await destination.start()
        defer { destination.stop() }
        let redirect = try LocalHTTPServer(status: 307, headers: "Location: http://127.0.0.1:\(destination.port)/stolen\r\n", body: "")
        try await redirect.start()
        defer { redirect.stop() }
        let f = try DictationFixture(server: "http://127.0.0.1:\(redirect.port)", credential: "scoped-placeholder", transport: URLSessionFileTransport())
        defer { f.remove() }
        _ = await f.record()
        f.app.send(.stopDictation)
        await settle { f.app.state.dictation.phase == .failed }
        #expect(f.app.state.dictation.failure == .service(307))
        #expect(redirect.requests.count == 1)
        #expect(destination.requests.isEmpty)
    }
}
