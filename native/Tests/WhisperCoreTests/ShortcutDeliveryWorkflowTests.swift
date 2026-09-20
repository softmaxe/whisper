import AppKit
import Testing
import WhisperCore

@MainActor private func monitorState(_ app: WhisperApplication) -> ShortcutMonitorState {
    ShortcutMonitorState(bindings: app.state.settings.shortcuts,
        requestActive: app.state.dictation.phase.isActive, processing: app.state.dictation.phase == .processing,
        capturing: app.state.shortcutCapture.isActive, dismissingRecovery: app.state.canDismissCopyRecovery)
}

@Suite(.serialized) @MainActor struct ShortcutDeliveryWorkflowTests {
    @Test func nativeEventValuesPreserveTimestampRepeatAndIndependentPhysicalSides() throws {
        let key = try #require(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: .control,
            timestamp: 12.25, windowNumber: 0, context: nil, characters: "k", charactersIgnoringModifiers: "k", isARepeat: true, keyCode: 40))
        let input = try #require(NativeShortcutMonitor.input(for: key, physicalKeys: [59]))
        #expect(input.occurredAt == 12.25)
        #expect(input.isRepeat && input.isDown)
        #expect(input.heldModifiers == [59])
        let flags = try #require(NSEvent.keyEvent(with: .flagsChanged, location: .zero, modifierFlags: .command,
            timestamp: 12.35, windowNumber: 0, context: nil, characters: "", charactersIgnoringModifiers: "", isARepeat: false, keyCode: 54))
        let release = try #require(NativeShortcutMonitor.input(for: flags, physicalKeys: [55]))
        #expect(!release.isDown)
        #expect(release.heldModifiers == [55])
        #expect(release.occurredAt == 12.35)
    }

    @Test(arguments: ["short", "long", "double", "combination", "escape"])
    func edgesCollectedDuringSynchronousPreparationKeepTheirOrderAndDurations(gesture: String) async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        let delivery = ShortcutEventDelivery(application: f.app)
        let token = delivery.start(state: monitorState(f.app))
        defer { delivery.stop() }
        delivery.didDrain = { [weak f, weak delivery] in if let f { delivery?.update(monitorState(f.app)) } }
        var desktop = f.app.state.settings.desktop
        desktop.pauseMediaOnDictation = true
        f.app.send(.saveDesktopPreferences(desktop))
        f.microphones.beforeResolve = {
            // These are already collected by the tap while MainActor is busy preparing the first press.
            DispatchQueue.global().sync {
                if gesture == "combination" {
                    #expect(!delivery.receive(.init(keyCode: 8, isDown: true, occurredAt: 0.08), generation: token))
                    #expect(!delivery.receive(.init(keyCode: 8, isDown: false, occurredAt: 0.09), generation: token))
                }
                if gesture == "escape" {
                    #expect(delivery.receive(.init(keyCode: 53, isDown: true, occurredAt: 0.08), generation: token))
                    #expect(delivery.receive(.init(keyCode: 53, isDown: false, occurredAt: 0.09), generation: token))
                }
                if gesture != "long" {
                    delivery.receive(.init(keyCode: 54, isDown: false, occurredAt: 0.1), generation: token)
                }
                if gesture == "double" {
                    delivery.receive(.init(keyCode: 54, isDown: true, occurredAt: 0.21), generation: token)
                    delivery.receive(.init(keyCode: 54, isDown: true, isRepeat: true, occurredAt: 0.22), generation: token)
                    delivery.receive(.init(keyCode: 54, isDown: false, occurredAt: 0.25), generation: token)
                }
            }
            f.clock.now = gesture == "double" ? 0.36 : 0.2
        }
        #expect(!delivery.receive(.init(keyCode: 54, isDown: true, occurredAt: 0), generation: token))
        delivery.drain()
        #expect(f.desktop.cues.isEmpty)
        #expect(f.desktop.mediaEvents.isEmpty)
        let capture = try #require(f.microphones.sessions.first)
        capture.open(); capture.deliver([Float](repeating: 0, count: 4800))
        if ["long", "double"].contains(gesture) {
            await settle { f.app.state.dictation.timing["firstAudio"] != nil }
            f.clock.advance(0)
            try #require(f.app.state.dictation.phase == .recording)
            #expect(f.app.state.dictation.origin == (gesture == "double" ? .handsFree : .hold))
            f.clock.now = 0.6
            if gesture == "double" { delivery.receive(.init(keyCode: 54, isDown: true, occurredAt: 0.5), generation: token) }
            delivery.receive(.init(keyCode: 54, isDown: false, occurredAt: 0.55), generation: token)
            delivery.drain()
            await settle { await f.transport.requests.count == 1 }
            await f.transport.reply()
            await settle { f.app.state.dictation.delivery == .pasted }
            #expect(f.microphones.sessions.count == 1)
        } else {
            f.clock.advance(0.301)
            #expect(f.app.state.dictation.phase == .idle)
            #expect(f.desktop.cues.isEmpty)
            #expect(f.desktop.mediaEvents.isEmpty)
            #expect(await f.transport.requests.isEmpty)
            await f.app.flushHistoryWrites(); f.app.send(.loadHistory)
            await settle { f.app.state.history.isLoaded && !f.app.state.history.isLoading }
            #expect(f.app.state.history.totalCount == 0)
        }
        #expect(!capture.physicallyOpen)
    }

    @Test func overdueTimerDrainsCollectedCombinationBeforeReadiness() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        let delivery = ShortcutEventDelivery(application: f.app)
        let token = delivery.start(state: monitorState(f.app)); defer { delivery.stop() }
        f.key(at: 0)
        let capture = try #require(f.microphones.sessions.first)
        capture.open(); capture.deliver([Float](repeating: 0, count: 4800))
        await settle { f.app.state.dictation.timing["firstAudio"] != nil }
        #expect(!delivery.receive(.init(keyCode: 8, isDown: true, occurredAt: 0.08), generation: token))
        #expect(!delivery.receive(.init(keyCode: 8, isDown: false, occurredAt: 0.09), generation: token))
        // The combination is no longer physically down. Its collected edges still reject this hold.
        f.clock.advance(0.2)
        #expect(f.app.state.dictation.phase == .idle)
        #expect(f.desktop.cues.isEmpty)
        #expect(await f.transport.requests.isEmpty)
        #expect(!capture.physicallyOpen)
    }

    @Test func anOverdueFirstPressTimerCannotRecognizeTheNewSecondPress() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        let delivery = ShortcutEventDelivery(application: f.app)
        let token = delivery.start(state: monitorState(f.app)); defer { delivery.stop() }
        f.key(at: 0)
        let capture = try #require(f.microphones.sessions.first)
        capture.open(); capture.deliver([Float](repeating: 0, count: 4800))
        await settle { f.app.state.dictation.timing["firstAudio"] != nil }
        delivery.receive(.init(keyCode: 54, isDown: false, occurredAt: 0.05), generation: token)
        delivery.receive(.init(keyCode: 54, isDown: true, occurredAt: 0.12), generation: token)
        f.clock.advance(0.16)
        try #require(f.app.state.dictation.gesture == .secondTap)
        #expect(f.app.state.dictation.phase == .preparing)
        #expect(f.desktop.cues.isEmpty)
        delivery.receive(.init(keyCode: 54, isDown: false, occurredAt: 0.16), generation: token)
        delivery.drain()
        #expect(f.app.state.dictation.origin == .handsFree)
        #expect(f.app.state.dictation.phase == .recording)
        f.app.send(.cancelDictation)
        #expect(!capture.physicallyOpen)
    }

    @Test func configuredEscapeStillRecordsAndProcessingEscapeIsConsumedAndCancels() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.app.send(.saveShortcuts(["Control+Esc"]))
        let delivery = ShortcutEventDelivery(application: f.app)
        let token = delivery.start(state: monitorState(f.app)); defer { delivery.stop() }
        #expect(!delivery.receive(.init(keyCode: 59, isDown: true, occurredAt: 0), generation: token))
        #expect(delivery.receive(.init(keyCode: 53, isDown: true, occurredAt: 0.01), generation: token))
        f.clock.now = 0.01; delivery.drain()
        let capture = try #require(f.microphones.sessions.first)
        capture.open(); capture.deliver([Float](repeating: 0, count: 4800))
        await settle { f.app.state.dictation.timing["firstAudio"] != nil }
        f.clock.advance(0.151)
        #expect(f.app.state.dictation.phase == .recording)
        var state = monitorState(f.app); state.activeShortcut = "Control+Esc"; delivery.update(state)
        #expect(delivery.receive(.init(keyCode: 53, isDown: false, occurredAt: 0.161), generation: token))
        delivery.drain()
        await settle { await f.transport.requests.count == 1 }
        state = monitorState(f.app); state.activeShortcut = "Control+Esc"; delivery.update(state)
        #expect(delivery.receive(.init(keyCode: 53, isDown: true, occurredAt: 0.17), generation: token))
        #expect(delivery.receive(.init(keyCode: 53, isDown: false, occurredAt: 0.18), generation: token))
        delivery.drain()
        await f.transport.reply()
        #expect(f.app.state.dictation.phase == .idle)
        #expect(f.paste.pasted.isEmpty)
    }

    @Test func stopRestartDiscardsOldSessionCallbacksAndPendingEscapeWinsOverASR() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        let delivery = ShortcutEventDelivery(application: f.app)
        let old = delivery.start(state: monitorState(f.app))
        delivery.receive(.init(keyCode: 54, isDown: true, occurredAt: 0), generation: old)
        delivery.stop()
        let current = delivery.start(state: monitorState(f.app)); defer { delivery.stop() }
        #expect(!delivery.receive(.init(keyCode: 54, isDown: true, occurredAt: 0.1), generation: old))
        delivery.drain()
        #expect(f.microphones.sessions.isEmpty)
        _ = await f.hold(); f.key(down: false)
        await settle { await f.transport.requests.count == 1 }
        delivery.update(monitorState(f.app))
        #expect(delivery.receive(.init(keyCode: 53, isDown: true, occurredAt: f.clock.now), generation: current))
        #expect(delivery.receive(.init(keyCode: 53, isDown: false, occurredAt: f.clock.now), generation: current))
        await f.transport.reply()
        await settle { f.app.state.dictation.phase == .idle }
        #expect(f.paste.pasted.isEmpty)
        delivery.stop()
        #expect(!delivery.receive(.init(keyCode: 54, isDown: true), generation: current))
        delivery.drain()
        #expect(f.microphones.sessions.count == 1)
    }

    @Test func unsuccessfulTapSetupDoesNotCancelAnExistingDictation() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        let capture = await f.hold()
        let delivery = ShortcutEventDelivery(application: f.app)
        let token = delivery.start(state: monitorState(f.app), heldKeys: [54], resetInput: false)
        // The native owner could not create its port, so it never commits an input reset.
        delivery.stop(resetInput: false)
        #expect(f.app.state.dictation.phase == .recording)
        #expect(capture.physicallyOpen)
        #expect(!delivery.receive(.init(keyCode: 53, isDown: true), generation: token))
        await f.submit()
        await settle { f.app.state.dictation.delivery == .pasted }
    }
}
