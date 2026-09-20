import Foundation
import Testing
import WhisperCore

@Suite(.serialized) @MainActor struct CompletionCancellationWorkflowTests {
    @Test(arguments: [false, true])
    func cancellationAcceptedBeforePublicationCannotReviveTheCompletedRequest(pill: Bool) async throws {
        var cancelledBeforePublication = 0
        // Sweep external delivery turn boundaries after the public conversion receipt. No
        // assertion depends on which private helper happens to perform a particular drain.
        func replay(arrival: Int?) async throws -> Int {
            let f = try ShortcutFixture(); defer { f.remove() }
            let delivery = ShortcutEventDelivery(application: f.app)
            let token = delivery.start(state: .init(bindings: ["RightCommand"]))
            defer { delivery.stop() }
            var checkpoint = 0
            var queued = false
            var acceptedBeforePublication = false
            delivery.didDrain = { [weak app = f.app, weak delivery] in
                guard let app, let delivery else { return }
                delivery.update(.init(bindings: ["RightCommand"], requestActive: app.state.dictation.phase.isActive,
                    processing: app.state.dictation.phase == .processing))
                if app.state.dictation.cancellation != nil, app.state.dictation.timing["result"] == nil {
                    acceptedBeforePublication = true
                }
                guard !queued, app.state.dictation.phase == .processing,
                      app.state.dictation.timing["textConversion"] != nil,
                      app.state.dictation.timing["result"] == nil else { return }
                if let arrival, checkpoint == arrival {
                    queued = true
                    delivery.receive(.init(keyCode: 53, isDown: true), generation: token)
                    delivery.receive(.init(keyCode: 53, isDown: false), generation: token)
                }
                checkpoint += 1
            }
            f.app.send(pill ? .recordingPillAction : .startDictation)
            let requestID = try #require(f.app.state.dictation.requestID)
            let capture = try #require(f.microphones.sessions.first)
            capture.open(); capture.deliver([Float](repeating: 0.1, count: 57_600))
            await settle { f.app.state.dictation.phase == .recording }
            f.app.send(.stopDictation)
            await settle { await f.transport.requests.count == 1 }
            await f.transport.reply(body: "{\"text\":\"Cancelled publication fixture\"}")
            await settle { !f.app.state.dictation.phase.isActive }
            delivery.drain()
            await f.app.flushHistoryWrites()
            await f.app.flushInsightsWrites()
            if arrival == nil {
                #expect(f.app.state.dictation.phase == .result)
                #expect(f.app.state.dictation.text == "Cancelled publication fixture")
                #expect(try await HistoryStore(profile: f.profile.profile).entry(requestID)?.status == .completed)
                #expect(f.paste.pasted.count == (pill ? 1 : 0))
            }
            if acceptedBeforePublication {
                cancelledBeforePublication += 1
                #expect(f.app.state.dictation.phase == .idle)
                #expect(f.app.state.dictation.text.isEmpty)
                #expect(f.app.state.dictation.rawText.isEmpty)
                #expect(f.paste.pasted.isEmpty)
                #expect(try await HistoryStore(profile: f.profile.profile).entry(requestID) == nil)
                f.app.send(.loadInsights)
                await settle { f.app.state.insights.summary != nil && !f.app.state.insights.isLoading }
                #expect(f.app.state.insights.summary?.totalDictations == 0)
                #expect(f.app.state.insights.summary?.totalWords == 0)
            }
            #expect(!capture.physicallyOpen)
            // Drain successful clipboard restoration before disposing this isolated application.
            f.clock.advance(0.5)
            await f.app.prepareForTermination()
            return checkpoint
        }
        let boundaries = try await replay(arrival: nil)
        for arrival in 0..<boundaries { _ = try await replay(arrival: arrival) }
        #expect(cancelledBeforePublication > 0)
    }

    @Test func anOldFirstFrameCannotWriteIntoTheRequestStartedByCollectedInput() async throws {
        let f = try ShortcutFixture(); defer { f.clock.onRead = nil; f.remove() }
        let delivery = ShortcutEventDelivery(application: f.app)
        let token = delivery.start(state: .init(bindings: ["RightCommand"]))
        defer { delivery.stop() }
        f.app.send(.startDictation)
        let oldID = try #require(f.app.state.dictation.requestID)
        let old = try #require(f.microphones.sessions.first)
        old.open()
        await settle { f.app.state.dictation.timing["captureConfigured"] != nil }
        var queued = false
        f.clock.onRead = {
            guard !queued else { return }
            queued = true
            delivery.receive(.init(keyCode: 53, isDown: true), generation: token)
            delivery.receive(.init(keyCode: 53, isDown: false), generation: token)
            delivery.receive(.init(keyCode: 54, isDown: true), generation: token)
        }
        old.deliver([Float](repeating: 0.8, count: 57_600))
        await settle { f.app.state.dictation.requestID != oldID }
        f.clock.onRead = nil
        #expect(f.app.state.dictation.phase == .preparing)
        #expect(f.app.state.dictation.level == 0)
        #expect(f.app.state.dictation.duration == 0)
        #expect(f.app.state.dictation.timing["firstAudio"] == nil)
        #expect(f.desktop.cues.isEmpty)
        #expect(await f.transport.requests.isEmpty)
        #expect(!old.physicallyOpen)
        f.microphones.sessions.last?.open()
        await f.app.prepareForTermination()
    }

    @Test func inputCollectedAtThePasteProbeReturnStillCancelsBeforePosting() async throws {
        let f = try ShortcutFixture(); defer { f.paste.onProbeReturn = nil; f.remove() }
        let delivery = ShortcutEventDelivery(application: f.app)
        let token = delivery.start(state: .init(bindings: ["RightCommand"]))
        defer { delivery.stop() }
        f.paste.onProbeReturn = {
            // Final text may already be in History. This boundary must still suppress injection.
            delivery.receive(.init(keyCode: 53, isDown: true), generation: token)
            delivery.receive(.init(keyCode: 53, isDown: false), generation: token)
        }
        _ = await f.hold(); await f.submit()
        await settle { f.app.state.dictation.phase == .idle }
        #expect(f.app.state.dictation.cancellation == .user)
        #expect(f.paste.probes == 1)
        #expect(f.paste.pasted.isEmpty)
        #expect(f.app.state.dictation.text.isEmpty)
        await f.app.prepareForTermination()
    }

    @Test func cancellationAtCleanupPreparationCannotRestoreItsCleaningFlag() async throws {
        let cleanup = ControlledCleanupHTTP()
        let f = try ShortcutFixture(cleanup: SelfHostedCleanup(transport: cleanup)); defer { f.remove() }
        f.app.send(.saveCleanup(.init(serverURL: "http://localhost", model: "fixture"), credential: .unchanged))
        let delivery = ShortcutEventDelivery(application: f.app)
        let token = delivery.start(state: .init(bindings: ["RightCommand"]))
        defer { delivery.stop() }
        f.app.send(.startDictation)
        let requestID = try #require(f.app.state.dictation.requestID)
        let capture = try #require(f.microphones.sessions.first)
        capture.open(); capture.deliver([Float](repeating: 0.1, count: 57_600))
        await settle { f.app.state.dictation.phase == .recording }
        f.app.send(.stopDictation)
        await settle { await f.transport.requests.count == 1 }
        var queued = false
        delivery.didDrain = { [weak app = f.app, weak delivery] in
            guard !queued, app?.state.dictation.phase == .processing, let delivery else { return }
            queued = true
            delivery.receive(.init(keyCode: 53, isDown: true), generation: token)
            delivery.receive(.init(keyCode: 53, isDown: false), generation: token)
        }
        await f.transport.reply()
        await settle { f.app.state.dictation.phase == .idle }
        #expect(!f.app.state.dictation.isCleaning)
        #expect(f.app.state.dictation.text.isEmpty)
        #expect(await cleanup.requests.isEmpty)
        #expect(f.paste.pasted.isEmpty)
        await f.app.flushHistoryWrites()
        #expect(try await HistoryStore(profile: f.profile.profile).entry(requestID) == nil)
        await f.app.prepareForTermination()
    }
}
