import Foundation
import Testing
import WhisperCore

@Suite(.serialized) @MainActor
struct PillParityWorkflowTests {
    @Test func footprintTracksTheActualWorkflowAndOnlyRecordingHasWaveform() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        #expect(f.app.state.recordingPill.compactSize == CGSize(width: 40, height: 40))
        #expect(!f.app.state.recordingPill.showsWaveform)
        #expect(f.app.state.recordingPill.panelSize == CGSize(width: 60, height: 60))
        f.app.send(.recordingPillAction)
        #expect(f.app.state.recordingPill.compactSize == CGSize(width: 40, height: 40))
        #expect(f.app.state.recordingPill.showsCancel)
        let capture = try #require(f.microphones.sessions.last)
        capture.open(); capture.deliver([Float](repeating: 0.1, count: 4800))
        await settle { f.app.state.dictation.phase == .recording }
        #expect(f.app.state.recordingPill.showsWaveform)
        #expect(f.app.state.recordingPill.compactSize == CGSize(width: 98, height: 36))
        #expect(f.app.state.recordingPill.panelSize == CGSize(width: 154, height: 56))
        f.app.send(.recordingPillAction)
        await settle { await f.transport.requests.count == 1 }
        #expect(!f.app.state.recordingPill.showsWaveform)
        #expect(f.app.state.recordingPill.compactSize == CGSize(width: 40, height: 40))
        await f.transport.reply()
        await settle { f.app.state.dictation.phase == .result }
        #expect(!f.app.state.recordingPill.showsCancel)
        #expect(f.app.state.recordingPill.panelSize == CGSize(width: 60, height: 60))
    }

    @Test func focusAcrossRecoveryChildrenHoldsTheTimerAfterHoverEnds() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        let revision = try await recovery(f)
        f.app.send(.copyRecoveryPresented(revision, true))
        f.app.send(.copyRecoveryHeld(revision, true))
        f.app.send(.copyRecoveryFocused(revision, true))
        f.app.send(.copyRecoveryHeld(revision, false))
        f.clock.advance(60)
        #expect(f.app.state.recordingPill.feedback == .recovery)
        #expect(f.app.state.desktop.copyRecovery.isHeld)
        f.app.send(.copyRecoveryFocused(revision, false))
        f.clock.advance(4.99)
        #expect(f.app.state.recordingPill.feedback == .recovery)
        f.clock.advance(0.01)
        #expect(!f.app.state.recordingPill.visible)
    }

    @Test func anOldRecoveryFocusEventCannotReleaseTheReplacementFocusHold() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        let old = try await recovery(f)
        f.app.send(.copyRecoveryFocused(old, true))
        f.app.send(.dismissPillFeedback)
        let current = try await recovery(f, index: 1)
        f.app.send(.copyRecoveryPresented(current, true))
        f.app.send(.copyRecoveryFocused(current, true))
        f.app.send(.copyRecoveryFocused(old, false))
        f.clock.advance(60)
        #expect(f.app.state.desktop.copyRecovery.isFocused)
        #expect(f.app.state.recordingPill.feedback == .recovery)
        f.app.send(.foregroundEscape)
        #expect(!f.app.state.recordingPill.visible)
    }

    @Test func clickingCompletedFeedbackImmediatelyStartsWithoutCopying() async throws {
        let fixture = try ShortcutFixture(); defer { fixture.remove() }
        fixture.app.send(.setClipboardPreferences(autoPaste: false, keepResult: false))
        _ = await fixture.hold(); await fixture.submit()
        await settle { fixture.app.state.dictation.phase == .result }
        #expect(fixture.app.state.recordingPill.feedback == .completed)
        fixture.app.send(.recordingPillAction)
        #expect(fixture.app.state.dictation.phase == .preparing)
        #expect(fixture.app.state.dictation.origin == .pill)
        #expect(fixture.clipboard.values.isEmpty)
        #expect(fixture.paste.writes.isEmpty)
        fixture.microphones.sessions.last?.open()
        fixture.app.send(.cancelDictation)
    }

    @Test func pillStartCapturesTheCurrentTargetAndAutomaticallyPastes() async throws {
        let fixture = try ShortcutFixture()
        defer { fixture.remove() }
        fixture.app.send(.recordingPillAction)
        let capture = try #require(fixture.microphones.sessions.last)
        capture.open(); capture.deliver([Float](repeating: 0.1, count: 4800))
        await settle { fixture.app.state.dictation.phase == .recording }
        fixture.paste.frontmost = PasteTarget(processID: 202)
        fixture.app.send(.recordingPillAction)
        await settle { await fixture.transport.requests.count == 1 }
        await fixture.transport.reply()
        await settle { fixture.app.state.dictation.phase == .result }
        #expect(fixture.app.state.dictation.delivery == .pasted)
        #expect(fixture.paste.pasted == [PasteTarget(processID: 101)])
    }

    @Test func escapeDismissesCopyRecoveryWithoutRemovingResultOrHistory() async throws {
        let fixture = try ShortcutFixture()
        defer { fixture.remove() }
        var preferences = fixture.app.state.settings.desktop
        preferences.floatingIconAutoHide = true
        fixture.app.send(.saveDesktopPreferences(preferences))
        fixture.paste.allowProbe = false
        _ = await fixture.hold()
        await fixture.submit()
        await settle { fixture.app.state.dictation.phase == .result }
        await fixture.app.flushHistoryWrites()
        let text = fixture.app.state.dictation.text
        let historyID = fixture.app.state.history.lastSavedID
        #expect(fixture.app.state.recordingPill.feedback == .recovery)
        fixture.key(ShortcutInput.escape)
        fixture.key(ShortcutInput.escape, down: false)
        #expect(!fixture.app.state.recordingPill.visible)
        #expect(fixture.app.state.dictation.text == text)
        #expect(fixture.app.state.history.lastSavedID == historyID)
    }

    @Test func recoveryWaitsForPresentationAndHoverReleaseRestartsFiveSeconds() async throws {
        let fixture = try ShortcutFixture(); defer { fixture.remove() }
        let revision = try await recovery(fixture)
        fixture.clock.advance(30)
        #expect(fixture.app.state.recordingPill.feedback == .recovery)
        fixture.app.send(.copyRecoveryPresented(revision, true))
        fixture.clock.advance(4)
        fixture.app.send(.copyRecoveryHeld(revision, true))
        fixture.clock.advance(60)
        #expect(fixture.app.state.recordingPill.feedback == .recovery)
        fixture.app.send(.copyRecoveryHeld(revision, false))
        fixture.clock.advance(4.99)
        #expect(fixture.app.state.recordingPill.feedback == .recovery)
        fixture.clock.advance(0.01)
        #expect(!fixture.app.state.recordingPill.visible)
        #expect(fixture.app.state.dictation.text == "Fixture transcript")
    }

    @Test func staleRecoveryEventsCannotDismissNewRecoveryOrNewRecording() async throws {
        let fixture = try ShortcutFixture(); defer { fixture.remove() }
        let old = try await recovery(fixture)
        fixture.app.send(.copyRecoveryPresented(old, true))
        fixture.clock.advance(4)
        fixture.app.send(.dismissPillFeedback)
        let current = try await recovery(fixture, index: 1)
        fixture.app.send(.copyRecoveryPresented(current, true))
        fixture.app.send(.copyRecoveryHeld(current, true))
        fixture.app.send(.copyRecoveryHeld(old, false))
        fixture.app.send(.copyRecoveryPresented(old, false))
        fixture.clock.advance(10)
        #expect(fixture.app.state.recordingPill.feedback == .recovery)
        fixture.app.send(.recordingPillAction)
        fixture.app.send(.dismissPillFeedback)
        fixture.app.send(.recordingPillAction)
        let capture = try #require(fixture.microphones.sessions.last)
        capture.open(); capture.deliver([Float](repeating: 0.1, count: 4800))
        await settle { fixture.app.state.dictation.phase == .recording }
        fixture.clock.advance(10)
        #expect(fixture.app.state.recordingPill.feedback == .hold)
        #expect(capture.physicallyOpen)
    }

    @Test func hiddenRecoveryPausesAndBothClipboardRecoveryModesCanExpire() async throws {
        for copied in [false, true] {
            let fixture = try ShortcutFixture(); defer { fixture.remove() }
            fixture.paste.allowWrite = copied
            let revision = try await recovery(fixture)
            #expect(fixture.app.state.dictation.delivery == .recovery(copied: copied))
            fixture.app.send(.copyRecoveryPresented(revision, true))
            fixture.clock.advance(4)
            fixture.app.send(.copyRecoveryPresented(revision, false))
            fixture.clock.advance(20)
            #expect(fixture.app.state.recordingPill.feedback == .recovery)
            fixture.app.send(.copyRecoveryPresented(revision, true))
            fixture.clock.advance(5)
            #expect(!fixture.app.state.recordingPill.visible)
            #expect(!fixture.app.state.dictation.text.isEmpty)
        }
    }

    @Test func pillRetryCapturesFreshTargetAndButtonEntryRemainsInert() async throws {
        let fixture = try ShortcutFixture(); defer { fixture.remove() }
        fixture.microphones.rejection = .inputUnavailable
        fixture.app.send(.recordingPillAction)
        #expect(fixture.app.state.dictation.phase == .failed)
        fixture.microphones.rejection = nil
        fixture.paste.frontmost = PasteTarget(processID: 303)
        fixture.app.send(.recordingPillAction)
        let capture = try #require(fixture.microphones.sessions.last)
        capture.open(); capture.deliver([Float](repeating: 0.1, count: 4800))
        await settle { fixture.app.state.dictation.phase == .recording }
        fixture.app.send(.recordingPillAction)
        await settle { await fixture.transport.requests.count == 1 }
        await fixture.transport.reply()
        await settle { fixture.app.state.dictation.phase == .result }
        #expect(fixture.paste.captures == [PasteTarget(processID: 101), PasteTarget(processID: 303)])
        #expect(fixture.paste.pasted == [PasteTarget(processID: 303)])
        fixture.app.send(.startDictation)
        #expect(fixture.app.state.dictation.origin == .button)
        #expect(fixture.paste.captures.count == 2)
        fixture.microphones.sessions.last?.open()
    }

    @Test(arguments: [false, true])
    func pillRunsTheFullProcessingHistoryAndDeliveryPipeline(recover: Bool) async throws {
        let cleanup = ControlledCleanupHTTP()
        let fixture = try ShortcutFixture(cleanup: SelfHostedCleanup(transport: cleanup))
        defer { fixture.remove() }
        fixture.app.send(.saveCleanup(.init(serverURL: "http://localhost:8123"), credential: .unchanged))
        fixture.app.send(.setTranscriptionLanguage("zh-TW"))
        fixture.app.send(.importDictionary("OpenWhispr"))
        fixture.app.send(.saveSnippet(trigger: "這是中文軟體", replacement: "Literal 简体"))
        fixture.paste.allowProbe = !recover
        fixture.app.send(.recordingPillAction)
        let capture = try #require(fixture.microphones.sessions.last)
        capture.open(); capture.deliver([Float](repeating: 0.1, count: 4800))
        await settle { fixture.app.state.dictation.phase == .recording }
        fixture.app.send(.recordingPillAction)
        await settle { await fixture.transport.requests.count == 1 }
        await fixture.transport.reply(body: #"{"text":"Raw pill transcript"}"#)
        await settle { await cleanup.requests.count == 1 }
        await cleanup.reply(content: "这是中文软件")
        for _ in 0..<5000 {
            if fixture.app.state.dictation.phase == .result { break }
            try await Task.sleep(for: .milliseconds(2))
        }
        await fixture.app.flushHistoryWrites()
        #expect(fixture.app.state.dictation.origin == .pill)
        #expect(fixture.app.state.dictation.text == "Literal 简体")
        #expect(fixture.paste.writes == ["Literal 简体"])
        #expect(fixture.app.state.dictation.delivery == (recover ? .recovery(copied: true) : .pasted))
        let requestID = try #require(fixture.app.state.dictation.requestID)
        let entry = try #require(try await HistoryStore(profile: fixture.profile.profile).entry(requestID))
        #expect(entry.text == "Literal 简体" && entry.rawText == "Raw pill transcript")
    }

    @Test func recoveryEscapeSuppressionBalancesReleaseAfterDismissal() {
        var suppression = ShortcutEventSuppression()
        let down = ShortcutInput(keyCode: ShortcutInput.escape, isDown: true)
        let up = ShortcutInput(keyCode: ShortcutInput.escape, isDown: false)
        let consumedDown = suppression.consume(down, pressed: [ShortcutInput.escape], bindings: ["RightCommand"], capturing: false, dismissingRecovery: true)
        let consumedUp = suppression.consume(up, pressed: [], bindings: ["RightCommand"], capturing: false)
        let ordinaryDown = suppression.consume(down, pressed: [ShortcutInput.escape], bindings: ["RightCommand"], capturing: false)
        #expect(consumedDown && consumedUp && !ordinaryDown)
    }

    @Test func alreadyQueuedOldCountdownCannotDismissReplacementRecovery() async throws {
        let profile = try ProfileFixture(keychain: false); defer { profile.remove() }
        let microphones = ControlledMicrophones()
        let transport = ControlledHTTPTransport()
        let clock = DeferredPillClock()
        let paste = ControlledPasteSystem(); paste.allowProbe = false
        let app = WhisperApplication(profile: profile.profile, credentials: profile.credentials,
            microphones: microphones, transcriber: SelfHostedTranscriber(transport: transport), clock: clock,
            clipboard: ControlledClipboard(), pasteSystem: paste, correctionSystem: InertCorrectionMonitoringFixture(),
            desktopEffects: ControlledDesktopEffects())
        app.send(.saveASR(.init(serverURL: "http://localhost", model: "fixture"), credential: .unchanged))
        var preferences = app.state.settings.desktop
        preferences.floatingIconAutoHide = true
        app.send(.saveDesktopPreferences(preferences))
        for index in 0..<2 {
            app.send(.recordingPillAction)
            let capture = try #require(microphones.sessions.last)
            capture.open(); capture.deliver([Float](repeating: 0.1, count: 4800))
            await settle { app.state.dictation.phase == .recording }
            app.send(.recordingPillAction)
            await settle { await transport.requests.count == index + 1 }
            await transport.reply(index)
            await settle { app.state.dictation.phase == .result }
            let revision = try #require(app.state.desktop.copyRecovery.revision)
            app.send(.copyRecoveryPresented(revision, true))
            if index == 0 {
                clock.now = 4
                app.send(.dismissPillFeedback)
            }
        }
        let current = app.state.desktop.copyRecovery.revision
        clock.now = 5
        clock.deliverDueIncludingCancelled()
        #expect(app.state.desktop.copyRecovery.revision == current)
        #expect(app.state.recordingPill.feedback == .recovery)
        clock.now = 9
        clock.deliverDueIncludingCancelled()
        #expect(!app.state.recordingPill.visible)
        await app.flushHistoryWrites()
        #expect(app.state.dictation.text == "Fixture transcript")
    }

    private func recovery(_ fixture: ShortcutFixture, index: Int = 0) async throws -> UUID {
        var preferences = fixture.app.state.settings.desktop
        preferences.floatingIconAutoHide = true
        fixture.app.send(.saveDesktopPreferences(preferences))
        fixture.paste.allowProbe = false
        _ = await fixture.hold()
        await fixture.submit(index)
        await settle { fixture.app.state.dictation.phase == .result }
        return try #require(fixture.app.state.desktop.copyRecovery.revision)
    }
}

/// Models timer callbacks already queued before cancellation; the app still owns their validity.
@MainActor private final class DeferredPillClock: WorkflowClock {
    var now: TimeInterval = 0
    private var pending: [(TimeInterval, @MainActor @Sendable () -> Void)] = []
    func schedule(after seconds: TimeInterval, _ action: @escaping @MainActor @Sendable () -> Void) -> any ScheduledAction {
        pending.append((now + seconds, action))
        return QueuedAction()
    }
    func deliverDueIncludingCancelled() {
        let due = pending.filter { $0.0 <= now }
        pending.removeAll { $0.0 <= now }
        due.forEach { $0.1() }
    }
    private final class QueuedAction: ScheduledAction { func cancel() {} }
}
