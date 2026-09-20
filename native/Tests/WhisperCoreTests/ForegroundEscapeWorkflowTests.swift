import Foundation
import Testing
import WhisperCore

@Suite(.serialized) @MainActor struct ForegroundEscapeWorkflowTests {
    @Test(arguments: ["preparing", "recording", "processing", "cleanup"], [false, true])
    func escapeCancelsDictationBeforeDismissingTheForegroundModal(stage: String, settings: Bool) async throws {
        let cleanup = ControlledCleanupHTTP()
        let f = try ShortcutFixture(cleanup: SelfHostedCleanup(transport: cleanup)); defer { f.remove() }
        #expect(!f.app.state.shortcutAvailable)
        if stage == "cleanup" {
            f.app.send(.saveCleanup(.init(serverURL: "http://localhost", model: "fixture"), credential: .unchanged))
        }
        f.app.send(settings ? .openSettings(.general) : .openHistorySearch)
        f.app.send(.startDictation)
        let capture = try #require(f.microphones.sessions.first)
        if stage != "preparing" {
            capture.open(); capture.deliver([Float](repeating: 0, count: 4800))
            await settle { f.app.state.dictation.phase == .recording }
        }
        if stage == "processing" || stage == "cleanup" {
            f.app.send(.stopDictation)
            await settle { await f.transport.requests.count == 1 }
            if stage == "cleanup" {
                await f.transport.reply()
                await settle { await cleanup.requests.count == 1 }
            }
        }
        f.app.send(.foregroundEscape)
        #expect(f.app.state.dictation.phase == .idle)
        #expect(settings ? f.app.state.navigation.settingsPresented : f.app.state.navigation.searchPresented)
        if stage == "preparing" { capture.open() }
        if stage == "processing" { await f.transport.reply() }
        if stage == "cleanup" { await cleanup.reply(content: "Late fixture") }
        f.app.send(.foregroundEscape)
        #expect(!f.app.state.navigation.settingsPresented && !f.app.state.navigation.searchPresented)
        await f.app.prepareForTermination()
        #expect(f.app.state.dictation.text.isEmpty)
        #expect(f.paste.pasted.isEmpty)
        #expect(!capture.physicallyOpen)
    }

    @Test func inactiveEscapeEndsShortcutCaptureBeforeClosingSettings() throws {
        let f = try ShellFixture(); defer { f.remove() }
        f.app.send(.openSettings(.hotkeys))
        f.app.send(.beginShortcutCapture(index: nil))
        f.app.send(.foregroundEscape)
        #expect(!f.app.state.shortcutCapture.isActive)
        #expect(f.app.state.navigation.settingsPresented)
        f.app.send(.foregroundEscape)
        #expect(!f.app.state.navigation.settingsPresented)
        #expect(f.privacy.requests.isEmpty)
        #expect(f.microphones.sessions.isEmpty)
    }
}
