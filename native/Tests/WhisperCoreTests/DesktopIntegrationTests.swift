import Foundation
import Testing
import WhisperCore

@Suite(.serialized) @MainActor
struct DesktopIntegrationTests {
    @Test(arguments: [false, true])
    func terminationReleasesGestureMediaAndCommitsAcceptedHistory(handsFree: Bool) async throws {
        let f = try ShortcutFixture()
        defer { f.remove() }
        var preferences = DesktopPreferences()
        preferences.pauseMediaOnDictation = true
        f.app.send(.saveDesktopPreferences(preferences))
        f.desktop.suspendPause = true
        let capture = handsFree ? await f.doubleTap() : await f.hold()
        await settle { f.desktop.pauseContinuation != nil }
        let entries = (0..<24).map { HistoryEntry(text: "Previously accepted result \($0)") }
        for entry in entries { f.app.send(.saveHistory(entry)) }
        var finished = false
        let termination = Task { await f.app.prepareForTermination(); finished = true }
        await settle { f.app.state.dictation.phase == .idle }
        #expect(!finished)
        #expect(!capture.physicallyOpen)
        #expect(f.desktop.cues == [.ready])
        f.desktop.resolvePause()
        await termination.value
        #expect(finished)
        #expect(f.desktop.mediaEvents == ["pause", "resume"])
        #expect(f.app.state.history.pendingChanges == 0)
        #expect(await f.transport.requests.isEmpty)
        let reopened = f.profile.open()
        reopened.send(.loadHistory)
        await settle { reopened.state.history.isLoaded && !reopened.state.history.isLoading }
        #expect(reopened.state.history.totalCount == entries.count)
        #expect(Set(reopened.state.history.entries.map(\.id)) == Set(entries.map(\.id)))
    }

    @Test func handsFreeCompletionPreservesHistoryAndDesktopEffectsThroughShutdown() async throws {
        let f = try ShortcutFixture()
        defer { f.remove() }
        var preferences = DesktopPreferences()
        preferences.pauseMediaOnDictation = true
        preferences.floatingIconAutoHide = true
        f.app.send(.saveDesktopPreferences(preferences))
        _ = await f.doubleTap()
        await settle { f.desktop.mediaEvents == ["pause"] }
        f.key(); f.key(down: false)
        await settle { await f.transport.requests.count == 1 }
        await f.transport.reply(body: "{\"text\":\"Desktop result fixture\"}")
        await settle { f.app.state.dictation.phase == .result }
        #expect(f.app.state.dictation.delivery == .pasted)
        #expect(f.desktop.cues == [.ready, .stopped])
        #expect(f.app.state.recordingPill.feedback == .completed)
        await f.app.prepareForTermination()
        #expect(f.desktop.mediaEvents == ["pause", "resume"])
        let reopened = f.profile.open()
        reopened.send(.loadHistory)
        await settle { reopened.state.history.isLoaded && !reopened.state.history.isLoading }
        #expect(reopened.state.history.entries.count == 1)
        #expect(reopened.state.history.entries.first?.text == "Desktop result fixture")
        #expect(reopened.state.history.entries.first?.id == f.app.state.dictation.requestID)
    }

    @Test func terminationCancelsPromptTestingAndSuppressesItsLateReply() async throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let http = ControlledCleanupHTTP()
        let app = WhisperApplication(profile: profile.profile, credentials: profile.credentials,
            cleanup: SelfHostedCleanup(transport: http))
        app.send(.saveCleanup(.init(serverURL: "http://localhost:8080", model: "fixture"), credential: .unchanged))
        app.send(.testCleanupPrompt(text: "Synthetic draft", prompt: nil))
        await settle { await http.requests.count == 1 }
        #expect(app.state.cleanupTest.isRunning)
        await app.prepareForTermination()
        #expect(!app.state.cleanupTest.isRunning)
        await http.reply(content: "Late result must not publish")
        try await Task.sleep(for: .milliseconds(10))
        #expect(app.state.cleanupTest.text.isEmpty)
        #expect(app.state.cleanupTest.failure == nil)
    }

    @Test func learnedCorrectionsKeepAutoHiddenPillVisibleAndTerminationStopsObservation() async throws {
        let field = ControlledCorrectionField(before: "", range: 0..<0)
        let system = ControlledCorrectionSystem(field: field)
        let f = try ShortcutFixture(correctionSystem: system)
        defer { f.remove() }
        f.paste.onPaste = { field.setRegion($0) }
        var preferences = DesktopPreferences()
        preferences.floatingIconAutoHide = true
        preferences.pillVisible = false
        f.app.send(.saveDesktopPreferences(preferences))
        f.app.send(.setClipboardPreferences(autoPaste: true, keepResult: true))
        _ = await f.hold()
        f.key(down: false)
        await settle { await f.transport.requests.count == 1 }
        await f.transport.reply(body: "{\"text\":\"Hey Shunade how are you\"}")
        await settle { f.app.state.dictation.phase == .result }
        f.clock.advance(0.5)
        await settle { field.reads > 0 }
        field.setRegion("Hey Sinead how are you")
        field.observation?.changed()
        await settle { f.clock.scheduledDelays.contains { abs($0 - 1.5) < 0.000001 } }
        f.clock.advance(1.5)
        await settle { f.app.state.corrections.learned.map(\.word) == ["Sinead"] }
        #expect(f.app.state.recordingPill.visible)
        #expect(f.app.state.recordingPill.feedback == .learned)
        #expect(!f.app.state.settings.desktop.pillVisible)
        f.app.send(.dismissLearnedCorrections)
        #expect(!f.app.state.recordingPill.visible)
        await f.app.prepareForTermination()
        #expect(field.observation?.cancelled == true)
        #expect(!f.app.state.recordingPill.visible)
        #expect(f.profile.open().state.dictionary.words == ["Sinead"])
    }
}
