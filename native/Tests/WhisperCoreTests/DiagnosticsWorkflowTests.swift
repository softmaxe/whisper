import Foundation
import Testing
import WhisperCore

private actor TimedASRBoundary: FileHTTPTransport {
    let base = ControlledHTTPTransport()
    let time: ControlledTimestampSource
    var preparedBeforeDispatch = false
    init(time: ControlledTimestampSource) { self.time = time }
    func upload(_ request: URLRequest, file: URL) async throws -> HTTPResponse { try await base.upload(request, file: file) }
    func upload(_ request: URLRequest, file: URL, diagnostics: NetworkDiagnostics?) async throws -> HTTPResponse {
        preparedBeforeDispatch = (try? Data(contentsOf: file).isEmpty) == false
        time.advance(0.025)
        diagnostics?.dispatched()
        do {
            let result = try await base.upload(request, file: file)
            diagnostics?.received(status: result.status)
            return result
        } catch { diagnostics?.failed(cancelled: error is CancellationError); throw error }
    }
}

private actor TimedCleanupBoundary: JSONHTTPTransport {
    let base = ControlledCleanupHTTP()
    let time: ControlledTimestampSource
    init(time: ControlledTimestampSource) { self.time = time }
    func send(_ request: URLRequest) async throws -> HTTPResponse { try await base.send(request) }
    func send(_ request: URLRequest, diagnostics: NetworkDiagnostics?) async throws -> HTTPResponse {
        time.advance(0.035)
        diagnostics?.dispatched()
        do {
            let result = try await base.send(request)
            diagnostics?.received(status: result.status)
            return result
        } catch { diagnostics?.failed(cancelled: error is CancellationError); throw error }
    }
}

@MainActor private final class TimedPasteBoundary: AutomaticPasteSystem {
    let base = ControlledPasteSystem()
    let time: ControlledTimestampSource
    init(time: ControlledTimestampSource) { self.time = time }
    var modifiersHeld: Bool { base.modifiersHeld }
    func captureTarget() -> PasteTarget? { base.captureTarget() }
    func snapshotClipboard() -> ClipboardSnapshot { base.snapshotClipboard() }
    func replaceClipboard(with text: String) -> Int? { base.replaceClipboard(with: text) }
    func restoreClipboard(_ snapshot: ClipboardSnapshot, ownedRevision: Int) { base.restoreClipboard(snapshot, ownedRevision: ownedRevision) }
    func activate(_ target: PasteTarget) async -> Bool { time.advance(0.02); return await base.activate(target) }
    func canPaste(_ target: PasteTarget) async -> Bool { time.advance(0.03); return await base.canPaste(target) }
    func paste(_ target: PasteTarget) async -> Bool { await paste(target, diagnostics: nil) }
    func paste(_ target: PasteTarget, diagnostics: RequestDiagnostics?) async -> Bool {
        time.advance(0.04)
        diagnostics?.mark(.pasteDispatched)
        let result = await base.paste(target)
        time.advance(0.008)
        diagnostics?.mark(.pasteSettled)
        return result
    }
}

@MainActor private final class DiagnosticsFixture {
    let profile = try! ProfileFixture()
    let clock = ControlledClock()
    let microphones = ControlledMicrophones()
    let asr: TimedASRBoundary
    let cleanup: TimedCleanupBoundary
    let paste: TimedPasteBoundary
    let app: WhisperApplication
    var file: URL { profile.root.appendingPathComponent("timing.jsonl") }
    init(enabled: Bool = true, clean: Bool = false) {
        asr = TimedASRBoundary(time: clock.timeSource)
        cleanup = TimedCleanupBoundary(time: clock.timeSource)
        paste = TimedPasteBoundary(time: clock.timeSource)
        microphones.device = MicrophoneDevice(id: "private-device-id-fixture", name: "private-device-label-fixture", category: .builtIn)
        app = WhisperApplication(profile: profile.profile, credentials: profile.credentials,
            microphones: microphones, transcriber: SelfHostedTranscriber(transport: asr),
            clock: clock, clipboard: ControlledClipboard(), pasteSystem: paste,
            cleanup: SelfHostedCleanup(transport: cleanup), correctionSystem: InertCorrectionMonitoringFixture(), desktopEffects: ControlledDesktopEffects())
        app.send(.saveASR(.init(serverURL: "http://localhost/private-server-fixture", model: "private-model-fixture"), credential: .replace("asr-key-placeholder")))
        if clean {
            app.send(.saveCleanup(.init(serverURL: "http://localhost/private-cleanup-fixture", model: "cleanup-model-fixture", customPrompt: "private-prompt-fixture"), credential: .replace("cleanup-key-placeholder")))
        }
        app.send(.setClipboardPreferences(autoPaste: true, keepResult: true))
        if enabled { app.send(.setDiagnosticsOutput(file)) }
    }
    func key(_ code: UInt16 = 54, down: Bool = true) { app.send(.shortcut(.init(keyCode: code, isDown: down))) }
    func record() async -> ControlledCapture {
        key()
        let capture = microphones.sessions.last!
        capture.open()
        clock.advance(0.01); capture.timing(.startRequested, at: clock.now)
        clock.advance(0.02); capture.deliver([Float](repeating: 0, count: 4800))
        await settle { self.app.state.dictation.timing["firstAudio"] != nil }
        clock.advance(0.121)
        #expect(app.state.dictation.phase == .recording)
        clock.advance(0.06); capture.timing(.startReturned, at: clock.now)
        return capture
    }
    func submit(_ index: Int = 0) async {
        key(down: false)
        await settle { await self.asr.base.requests.count > index }
    }
    func reply(_ index: Int = 0) async { await asr.base.reply(index, body: "{\"text\":\"private-speech-fixture\"}") }
    func records() async throws -> [[String: Any]] {
        #expect(await app.flushDiagnostics())
        return try String(contentsOf: file, encoding: .utf8).split(separator: "\n").dropFirst().map {
            try #require(JSONSerialization.jsonObject(with: Data($0.utf8)) as? [String: Any])
        }
    }
    func remove() { app.send(.cancelDictation); app.send(.setDiagnosticsOutput(nil)); paste.base.releaseProbe(false); clock.advance(60); profile.remove() }
}

@Suite("Numeric Dictation diagnostics", .serialized) @MainActor
struct DiagnosticsWorkflowTests {
    @Test func completeTraceUsesActualBoundariesAndExportsNoPrivateContent() async throws {
        let f = DiagnosticsFixture(clean: true); defer { f.remove() }
        _ = await f.record()
        f.clock.advance(0.2); await f.submit()
        #expect(await f.asr.preparedBeforeDispatch)
        f.clock.advance(0.2); await f.reply()
        await settle { await f.cleanup.base.requests.count == 1 }
        f.clock.advance(0.15)
        await f.cleanup.base.reply(content: "private-final-text-fixture")
        await settle { f.app.state.dictation.phase == .result }
        let record = try #require(try await f.records().last)
        let stages = try #require(record["stages"] as? [String: Double])
        #expect(record["outcome"] as? String == "completed")
        #expect(record["startup"] as? String == "completed")
        #expect(record["delivery"] as? String == "pasted")
        #expect(record["cleanup"] as? String == "completed")
        #expect(stages["firstAudio"]! < stages["captureStartReturned"]!)
        #expect(stages["readyFeedback"]! < stages["captureStartReturned"]!)
        #expect(stages["asrRequestDispatched"]! > stages["asrPreparationStarted"]!)
        #expect(stages["recordingFinalized"]! <= stages["asrRequestDispatched"]!)
        #expect(abs(stages["asrResponseReceived"]! - stages["asrRequestDispatched"]! - 200) < 0.001)
        #expect(stages["pasteDispatched"]! > stages["processingComplete"]!)
        #expect(abs(stages["pasteSettled"]! - stages["pasteDispatched"]! - 8) < 0.001)
        #expect(f.app.state.dictation.timing["asrDispatch"] == nil)
        #expect(f.app.state.dictation.timing["asrPreparationStarted"] != nil)
        let output = try String(contentsOf: f.file, encoding: .utf8)
        for marker in ["private-device", "localhost", "private-server", "private-model", "key-placeholder", "private-speech", "private-prompt", "private-final", "original", f.profile.root.path] {
            #expect(!output.contains(marker))
        }
        let attempts = try #require(record["attempts"] as? [[String: Any]])
        #expect(attempts.count == 2)
        let wire = await f.asr.base.requests[0]
        #expect(!String(decoding: wire.body, as: UTF8.self).contains("diagnostics"))
        let cleanupWire = try #require(await f.cleanup.base.requests[0].httpBody)
        #expect(!String(decoding: cleanupWire, as: UTF8.self).contains("diagnostics"))
        await f.app.prepareForTermination()
    }

    @Test func disabledCollectionDoesNotCreateFilesOrChangeDelivery() async throws {
        let f = DiagnosticsFixture(enabled: false); defer { f.remove() }
        _ = await f.record(); await f.submit(); await f.reply()
        await settle { f.app.state.dictation.phase == .result }
        #expect(f.app.state.dictation.delivery == .pasted)
        #expect(!FileManager.default.fileExists(atPath: f.file.path))
        #expect(await f.app.flushDiagnostics())
        #expect(!f.app.state.diagnostics.enabled)
        await f.app.prepareForTermination()
    }

    @Test func pendingRejectedFailedAndCancelledRequestsStayDistinct() async throws {
        let f = DiagnosticsFixture(); defer { f.remove() }
        f.key()
        let pending = try #require(try await f.records().last)
        #expect(pending["outcome"] as? String == "pending")
        #expect((pending["stages"] as? [String: Double])?["firstAudio"] == nil)
        f.clock.advance(0.02); f.key(down: false); f.clock.advance(0.301)
        f.microphones.sessions[0].open()
        _ = await f.record()
        f.key(53); f.key(53, down: false); f.key(down: false)
        f.microphones.rejection = .inputUnavailable
        f.key(); f.clock.advance(0.15); f.key(down: false)
        let terminal = try await f.records().filter { $0["outcome"] as? String != "pending" }
        #expect(terminal.compactMap { $0["outcome"] as? String } == ["rejected", "cancelled", "failed"])
        #expect(Set(terminal.compactMap { $0["requestId"] as? String }).count == 3)
        #expect(await f.asr.base.requests.isEmpty)
        await f.app.prepareForTermination()
    }

    @Test func lateASRAndCleanupCannotMutateEndedOrReplacementTraces() async throws {
        let f = DiagnosticsFixture(clean: true); defer { f.remove() }
        _ = await f.record(); await f.submit()
        f.app.send(.cancelDictation)
        let before = try #require(try await f.records().last)
        let frozen = try JSONSerialization.data(withJSONObject: before, options: .sortedKeys)
        #expect((before["attempts"] as? [[String: Any]])?.first?["outcome"] as? String == "cancelled")
        _ = await f.record()
        await f.reply(0)
        await Task.yield()
        await f.submit(1); await f.reply(1)
        await settle { await f.cleanup.base.requests.count == 1 }
        f.app.send(.cancelDictation)
        await f.cleanup.base.reply(content: "late-private-text-fixture")
        await Task.yield()
        let records = try await f.records()
        let original = try #require(records.last { $0["requestId"] as? String == before["requestId"] as? String })
        #expect(try JSONSerialization.data(withJSONObject: original, options: .sortedKeys) == frozen)
        #expect(f.paste.base.pasted.isEmpty)
        #expect(f.app.state.dictation.phase == .idle)
        #expect(records.filter { $0["outcome"] as? String == "cancelled" }.count == 2)
        await f.app.prepareForTermination()
    }

    @Test func cleanupFallbackRetainsAttemptOutcomesAndSuccessfulRawDelivery() async throws {
        let f = DiagnosticsFixture(clean: true); defer { f.remove() }
        _ = await f.record(); await f.submit(); await f.reply()
        await settle { await f.cleanup.base.requests.count == 1 }
        await f.cleanup.base.respond(0, status: 400, body: "{\"error\":\"reasoning is unsupported\"}")
        await settle { await f.cleanup.base.requests.count == 2 }
        await f.cleanup.base.respond(1, status: 200, body: "{\"choices\":[{\"message\":{\"content\":\"\"}}]}")
        await settle { f.app.state.dictation.phase == .result }
        let record = try #require(try await f.records().last)
        #expect(record["cleanup"] as? String == "failed")
        #expect(record["outcome"] as? String == "completed")
        #expect(f.app.state.dictation.text == "private-speech-fixture")
        let attempts = try #require(record["attempts"] as? [[String: Any]])
        #expect(attempts.count == 3)
        #expect(attempts[1]["outcome"] as? String == "failed")
        #expect(attempts[2]["outcome"] as? String == "completed")
        await f.app.prepareForTermination()
    }

    @Test(arguments: [false, true])
    func cleanupTransientRetryAndTimeoutPreserveObservedAttemptBoundaries(timeout: Bool) async throws {
        let f = DiagnosticsFixture(clean: true); defer { f.remove() }
        _ = await f.record(); await f.submit(); await f.reply()
        await settle { await f.cleanup.base.requests.count == 1 }
        if timeout { f.clock.advance(30) }
        else {
            await f.cleanup.base.respond(0, status: 503, body: "{}")
            await settle { f.clock.scheduledDelays.contains { abs($0 - 1) < 0.000001 } }
            f.clock.advance(1)
            await settle { await f.cleanup.base.requests.count == 2 }
            await f.cleanup.base.reply(1, content: "retry-result-fixture")
        }
        await settle { f.app.state.dictation.phase == .result }
        let record = try #require(try await f.records().last)
        #expect(record["outcome"] as? String == "completed")
        #expect(record["cleanup"] as? String == (timeout ? "failed" : "completed"))
        let attempts = try #require(record["attempts"] as? [[String: Any]])
        #expect(attempts.count == (timeout ? 2 : 3))
        #expect(attempts[1]["outcome"] as? String == (timeout ? "pending" : "failed"))
        if timeout {
            let frozen = try JSONSerialization.data(withJSONObject: record, options: .sortedKeys)
            await f.cleanup.base.reply(0, content: "late-timeout-fixture")
            await Task.yield()
            let after = try #require(try await f.records().last)
            #expect(try JSONSerialization.data(withJSONObject: after, options: .sortedKeys) == frozen)
        } else {
            let stages = try #require(record["stages"] as? [String: Double])
            #expect(stages["cleanupCompleted"]! - stages["cleanupPreparationStarted"]! >= 1000)
        }
        await f.app.prepareForTermination()
    }

    @Test func explicitOutputAppendsAcrossOptInSessionsAndOptOutFreezesCollection() async throws {
        let f = DiagnosticsFixture(); defer { f.remove() }
        _ = await f.record()
        f.app.send(.setDiagnosticsOutput(nil))
        #expect(await f.app.flushDiagnostics())
        let first = try String(contentsOf: f.file, encoding: .utf8)
        await f.submit(); await f.reply()
        await settle { f.app.state.dictation.phase == .result }
        #expect(try String(contentsOf: f.file, encoding: .utf8) == first)
        f.app.send(.setDiagnosticsOutput(f.file))
        _ = await f.record(); await f.submit(1); await f.reply(1)
        await settle { f.app.state.dictation.phase == .result }
        let records = try await f.records()
        #expect(records.filter { $0["outcome"] as? String == "incomplete" }.count == 1)
        #expect(records.filter { $0["outcome"] as? String == "completed" }.count == 1)
        await f.app.prepareForTermination()
    }

    @Test func rapidReopenWaitsForPriorWriterAndAccountsForBoundedSnapshots() async throws {
        let f = DiagnosticsFixture(); defer { f.remove() }
        f.microphones.rejection = .inputUnavailable
        for _ in 0..<250 {
            f.key(); f.clock.advance(0.02); f.key(down: false); f.clock.advance(0.301)
        }
        #expect(await f.app.flushDiagnostics())
        await settle { f.app.state.diagnostics.writtenRecords + f.app.state.diagnostics.droppedRecords == 500 }
        let previous = try await f.records().count
        f.app.send(.setDiagnosticsOutput(nil))
        f.app.send(.setDiagnosticsOutput(f.file))
        f.key(); f.clock.advance(0.02); f.key(down: false); f.clock.advance(0.301)
        let records = try await f.records()
        #expect(records.count == previous + 2)
        #expect(Set(records.compactMap { $0["collectionId"] as? String }).count == 2)
        #expect(!f.app.state.diagnostics.writeFailed)
        #expect(records.last?["outcome"] as? String == "rejected")
        await f.app.prepareForTermination()
    }

    @Test func noFrameTimeoutAndPasteRecoveryNeverInventSuccessfulStages() async throws {
        let f = DiagnosticsFixture(); defer { f.remove() }
        f.key(); let source = f.microphones.sessions[0]; source.open()
        await settle { f.app.state.dictation.timing["captureConfigured"] != nil }
        f.clock.advance(10)
        await settle { f.app.state.dictation.phase == .failed }
        let failed = try #require(try await f.records().last)
        #expect(failed["startup"] as? String == "failed")
        #expect((failed["stages"] as? [String: Double])?["firstAudio"] == nil)
        f.key(down: false)
        f.paste.base.allowProbe = false
        _ = await f.record(); await f.submit(); await f.reply()
        await settle { f.app.state.dictation.phase == .result }
        let recovered = try #require(try await f.records().last)
        #expect(recovered["outcome"] as? String == "failed")
        #expect(recovered["startup"] as? String == "completed")
        #expect(recovered["delivery"] as? String == "recovery")
        #expect((recovered["stages"] as? [String: Double])?["pasteDispatched"] == nil)
        await f.app.prepareForTermination()
    }

    @Test func cancellationDuringDeliveryFreezesTraceBeforeLateProbeCompletion() async throws {
        let f = DiagnosticsFixture(); defer { f.remove() }
        f.paste.base.suspendProbe = true
        _ = await f.record(); await f.submit(); await f.reply()
        await settle { f.paste.base.probes == 1 }
        f.app.send(.cancelDictation)
        let cancelled = try #require(try await f.records().last)
        #expect(cancelled["outcome"] as? String == "cancelled")
        #expect((cancelled["stages"] as? [String: Double])?["deliveryStarted"] != nil)
        #expect((cancelled["stages"] as? [String: Double])?["pasteDispatched"] == nil)
        let count = try await f.records().count
        f.paste.base.releaseProbe(true)
        await Task.yield()
        #expect(try await f.records().count == count)
        #expect(f.paste.base.pasted.isEmpty)
        await f.app.prepareForTermination()
    }

    @Test(arguments: ["unrelated", "symlink", "public"])
    func unsafeOutputNeverOverwritesOtherFilesOrBlocksDictation(kind: String) async throws {
        let f = DiagnosticsFixture(enabled: false); defer { f.remove() }
        let original = Data("private-existing-file-fixture".utf8)
        let target = f.profile.root.appendingPathComponent("existing")
        try original.write(to: target)
        try FileManager.default.setAttributes([.posixPermissions: kind == "public" ? 0o644 : 0o600], ofItemAtPath: target.path)
        let destination: URL
        if kind == "symlink" {
            try FileManager.default.createSymbolicLink(at: f.file, withDestinationURL: target)
            destination = f.file
        } else { destination = target }
        f.app.send(.setDiagnosticsOutput(destination))
        await settle { f.app.state.diagnostics.writeFailed }
        _ = await f.record(); await f.submit(); await f.reply()
        await settle { f.app.state.dictation.phase == .result }
        #expect(f.app.state.dictation.delivery == .pasted)
        #expect(!(await f.app.flushDiagnostics()))
        #expect(try Data(contentsOf: target) == original)
        await f.app.prepareForTermination()
    }
}
