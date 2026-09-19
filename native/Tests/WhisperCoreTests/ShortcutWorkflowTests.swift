import Foundation
import Testing
import WhisperCore

@MainActor final class ControlledPasteSystem: AutomaticPasteSystem {
    var frontmost = PasteTarget(processID: 101)
    var modifiersHeld = false
    var clipboard = ClipboardSnapshot(items: [["public.utf8-plain-text": Data("original".utf8), "public.rtf": Data("rich fixture".utf8)]])
    var revision = 0
    var allowWrite = true
    var allowActivation = true
    var allowPaste = true
    var allowProbe = true
    var suspendProbe = false
    var probeContinuation: CheckedContinuation<Bool, Never>?
    var captures: [PasteTarget] = []
    var activations: [PasteTarget] = []
    var pasted: [PasteTarget] = []
    var writes: [String] = []
    var probes = 0
    var onPaste: ((String) -> Void)?
    func captureTarget() -> PasteTarget? { captures.append(frontmost); return frontmost }
    func snapshotClipboard() -> ClipboardSnapshot { clipboard }
    func replaceClipboard(with text: String) -> Int? {
        guard allowWrite else { return nil }
        revision += 1
        writes.append(text)
        clipboard = ClipboardSnapshot(items: [["public.utf8-plain-text": Data(text.utf8)]])
        return revision
    }
    func restoreClipboard(_ snapshot: ClipboardSnapshot, ownedRevision: Int) {
        if revision == ownedRevision { clipboard = snapshot; revision += 1 }
    }
    func activate(_ target: PasteTarget) async -> Bool {
        activations.append(target)
        if allowActivation { frontmost = target }
        return allowActivation
    }
    func canPaste(_ target: PasteTarget) async -> Bool {
        probes += 1
        if suspendProbe { return await withCheckedContinuation { probeContinuation = $0 } }
        return allowProbe
    }
    func paste(_ target: PasteTarget) async -> Bool {
        guard allowPaste else { return false }
        onPaste?(writes.last ?? "")
        pasted.append(target)
        return true
    }
    func releaseProbe(_ result: Bool) { probeContinuation?.resume(returning: result); probeContinuation = nil }
}

@MainActor struct InertCorrectionMonitoringFixture: CorrectionMonitoringSystem {
    func capture(_ target: PasteTarget) async -> (any CorrectionField)? { nil }
}

@MainActor final class ShortcutFixture {
    let profile: ProfileFixture
    let microphones = ControlledMicrophones()
    let clock = ControlledClock()
    let transport = ControlledHTTPTransport()
    let paste = ControlledPasteSystem()
    let clipboard = ControlledClipboard()
    let desktop = ControlledDesktopEffects()
    var app: WhisperApplication
    init(transport customTransport: (any FileHTTPTransport)? = nil, correctionSystem: (any CorrectionMonitoringSystem)? = nil,
         cleanup: any CleanupService = SelfHostedCleanup(), pillDisplays: (any PillDisplaySystem)? = nil) throws {
        profile = try ProfileFixture()
        app = WhisperApplication(profile: profile.profile, credentials: profile.credentials,
            microphones: microphones, transcriber: SelfHostedTranscriber(transport: customTransport ?? transport),
            clock: clock, clipboard: clipboard, pasteSystem: paste, cleanup: cleanup,
            correctionSystem: correctionSystem ?? InertCorrectionMonitoringFixture(), desktopEffects: desktop, pillDisplays: pillDisplays)
        app.send(.saveASR(.init(serverURL: "http://localhost:8178/v1", model: "fixture"), credential: .unchanged))
    }
    func key(_ code: UInt16 = ShortcutInput.rightCommand, down: Bool = true, repeat repeated: Bool = false) {
        app.send(.shortcut(.init(keyCode: code, isDown: down, isRepeat: repeated)))
    }
    func hold() async -> ControlledCapture {
        key()
        let capture = microphones.sessions.last!
        capture.open()
        capture.deliver([Float](repeating: 0.25, count: 4800))
        await settle { self.app.state.dictation.timing["firstAudio"] != nil }
        clock.advance(WhisperApplication.holdThreshold)
        return capture
    }
    func submit(_ index: Int = 0) async {
        key(down: false)
        await settle { await self.transport.requests.count > index }
        await transport.reply(index)
    }
    func remove() {
        app.send(.cancelDictation)
        paste.releaseProbe(false)
        clock.advance(10)
        profile.remove()
    }
}

@Suite("Right Command Dictation") @MainActor
struct ShortcutWorkflowTests {
    @Test func holdCapturesOpeningAudioAndDeliversToStartupApp() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        let original = f.paste.clipboard
        f.key()
        let capture = try #require(f.microphones.sessions.first)
        #expect(f.app.state.dictation.gesture == .candidate)
        capture.open()
        capture.deliver([Float](repeating: 0.25, count: 4800))
        await settle { f.app.state.dictation.timing["firstAudio"] != nil }
        #expect(f.app.state.dictation.phase == .preparing)
        #expect(f.app.state.dictation.timing["readyFeedback"] == nil)
        f.paste.frontmost = PasteTarget(processID: 202)
        f.clock.advance(WhisperApplication.holdThreshold)
        #expect(f.app.state.dictation.phase == .recording)
        capture.deliver([Float](repeating: -0.25, count: 4800))
        await f.submit()
        await settle { f.app.state.dictation.phase == .result }
        #expect(f.app.state.dictation.delivery == .pasted)
        #expect(f.paste.captures == [PasteTarget(processID: 101)])
        #expect(f.paste.pasted == [PasteTarget(processID: 101)])
        #expect(!capture.physicallyOpen)
        let samples = await f.transport.requests[0].audio
        #expect(samples.prefix(4000).reduce(0, +) > 500)
        #expect(samples.suffix(4000).reduce(0, +) < -500)
        f.clock.advance(0.45)
        await settle { f.paste.clipboard == original }
    }

    @Test func validSilenceNeedsBothGestureAndInputReadiness() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.key()
        f.clock.advance(WhisperApplication.holdThreshold)
        #expect(f.app.state.dictation.phase == .preparing)
        #expect(f.app.state.dictation.gesture == .hold)
        let capture = try #require(f.microphones.sessions.first)
        capture.open(); capture.deliver([Float](repeating: 0, count: 4800))
        await settle { f.app.state.dictation.phase == .recording }
        #expect(f.app.state.dictation.level == 0)
        await f.submit()
        await settle { f.app.state.dictation.delivery == .pasted }
    }

    @Test(arguments: [false, true]) func shortTapAlwaysDiscards(deliverAudio: Bool) async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.key()
        let capture = try #require(f.microphones.sessions.first)
        if deliverAudio {
            capture.open(); capture.deliver([Float](repeating: 0.1, count: 4800))
            await settle { f.app.state.dictation.timing["firstAudio"] != nil }
        }
        f.clock.advance(0.05)
        f.key(down: false)
        f.clock.advance(WhisperApplication.doubleTapWindow)
        if !deliverAudio { capture.open() }
        #expect(f.app.state.dictation.phase == .idle)
        #expect(f.app.state.dictation.cancellation == .rejectedGesture)
        #expect(!capture.physicallyOpen)
        #expect(await f.transport.requests.isEmpty)
        #expect(f.paste.writes.isEmpty)
        f.clock.advance(1)
        #expect(f.app.state.dictation.phase == .idle)
        await settle {
            let temporary = f.profile.profile.directory.appendingPathComponent("Temporary")
            return ((try? FileManager.default.contentsOfDirectory(atPath: temporary.path)) ?? []).isEmpty
        }
        await f.app.flushHistoryWrites()
        f.app.send(.loadHistory)
        await settle { f.app.state.history.isLoaded && !f.app.state.history.isLoading }
        #expect(f.app.state.history.totalCount == 0)
    }

    @Test func leftAndRightOverlapAndCommandCombinationsDoNotDictate() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.key(55); f.key(); f.key(55, down: false); f.key(down: false)
        #expect(f.microphones.sessions.isEmpty)
        f.key(); f.key(55); f.key(down: false); f.key(55, down: false)
        #expect(f.app.state.dictation.cancellation == .rejectedGesture)
        f.key(); f.key(8); f.key(8, down: false); f.key(down: false)
        #expect(f.app.state.dictation.phase == .idle)
        #expect(await f.transport.requests.isEmpty)
        #expect(f.paste.pasted.isEmpty)
        f.microphones.sessions.forEach { $0.open() }
        #expect(f.microphones.sessions.allSatisfy { !$0.physicallyOpen })
    }

    @Test func repeatNeverRestartsCaptureOrBecomesAnotherGesture() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        _ = await f.hold()
        f.key(repeat: true); f.key(); f.clock.advance(3)
        #expect(f.microphones.sessions.count == 1)
        #expect(f.app.state.dictation.phase == .recording)
        await f.submit()
        await settle { f.app.state.dictation.phase == .result }
        #expect(f.paste.pasted.count == 1)
    }

    @Test func physicalSnapshotRejectsModifierAlreadyHeldBeforeListenerStarted() throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.app.send(.shortcut(.init(keyCode: 54, isDown: true, heldModifiers: [54, 55])))
        #expect(f.microphones.sessions.isEmpty)
        f.app.send(.shortcut(.init(keyCode: 54, isDown: false, heldModifiers: [55])))
        f.app.send(.resetShortcutInput([54]))
        f.key()
        #expect(f.microphones.sessions.isEmpty)
        f.key(down: false); f.key()
        #expect(f.microphones.sessions.count == 1)
        f.microphones.sessions[0].open()
    }

    @Test func rejectedCommandCandidateDoesNotExposeItsStartupFailure() throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.microphones.rejection = .inputUnavailable
        f.key(); f.key(8); f.key(8, down: false); f.key(down: false)
        #expect(f.app.state.dictation.phase == .idle)
        #expect(f.app.state.dictation.failure == nil)
        f.key()
        #expect(f.app.state.dictation.phase == .preparing)
        #expect(f.app.state.dictation.failure == nil)
        f.clock.advance(WhisperApplication.holdThreshold)
        #expect(f.app.state.dictation.phase == .failed)
        #expect(f.app.state.dictation.failure == .inputUnavailable)
    }

    @Test func escapeCancelsPendingAcquisitionAndOldCompletionCannotStopRetry() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.key()
        let old = f.microphones.sessions[0]
        f.key(ShortcutInput.escape); f.key(ShortcutInput.escape, down: false); f.key(down: false)
        let current = await f.hold()
        old.open(); old.deliver([0.8]); old.fail(.captureFailed)
        await Task.yield()
        #expect(!old.physicallyOpen)
        #expect(current.physicallyOpen)
        #expect(f.app.state.dictation.phase == .recording)
        await f.submit()
        await settle { f.app.state.dictation.delivery == .pasted }
    }

    @Test func ignoredProcessingPressAndLateASRStayWithTheirOwners() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        _ = await f.hold()
        f.key(down: false)
        await settle { await f.transport.requests.count == 1 }
        let requestID = f.app.state.dictation.requestID
        f.key(); f.key(down: false)
        #expect(f.app.state.dictation.phase == .processing)
        #expect(f.app.state.dictation.requestID == requestID)
        #expect(f.microphones.sessions.count == 1)
        f.key(ShortcutInput.escape); f.key(ShortcutInput.escape, down: false)
        _ = await f.hold()
        await f.transport.reply(0)
        await Task.yield()
        #expect(f.app.state.dictation.phase == .recording)
        #expect(f.paste.pasted.isEmpty)
        await f.submit(1)
        await settle { f.app.state.dictation.phase == .result }
        #expect(f.paste.pasted.count == 1)
    }

    @Test(arguments: [true, false]) func clipboardPreferencePersistsAndControlsDelivery(autoPaste: Bool) async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.app.send(.setClipboardPreferences(autoPaste: autoPaste, keepResult: true))
        let reopened = WhisperApplication(profile: f.profile.profile, credentials: f.profile.credentials)
        #expect(reopened.state.settings.autoPasteEnabled == autoPaste)
        #expect(reopened.state.settings.keepTranscriptionInClipboard)
        _ = await f.hold(); await f.submit()
        await settle { f.app.state.dictation.phase == .result }
        #expect(f.paste.pasted.count == (autoPaste ? 1 : 0))
        #expect(f.paste.writes == ["Fixture transcript"])
        f.clock.advance(1)
        #expect(f.paste.clipboard.items[0]["public.utf8-plain-text"] == Data("Fixture transcript".utf8))
    }

    @Test func noClipboardPreferenceLeavesOriginalUntouched() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.app.send(.setClipboardPreferences(autoPaste: false, keepResult: false))
        _ = await f.hold(); await f.submit()
        await settle { f.app.state.dictation.phase == .result }
        #expect(f.paste.writes.isEmpty)
        #expect(f.paste.pasted.isEmpty)
        #expect(f.app.state.dictation.text == "Fixture transcript")
    }

    @Test func newClipboardWriteWinsOverDelayedRestoration() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        _ = await f.hold(); await f.submit()
        await settle { f.app.state.dictation.phase == .result }
        _ = f.paste.replaceClipboard(with: "new user copy")
        f.clock.advance(0.45)
        await Task.yield()
        #expect(f.paste.clipboard.items[0]["public.utf8-plain-text"] == Data("new user copy".utf8))
    }

    @Test func nextPasteWaitsForPriorClipboardRestoration() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        let original = f.paste.clipboard
        _ = await f.hold(); await f.submit()
        await settle { f.app.state.dictation.phase == .result }
        _ = await f.hold(); await f.submit(1)
        await settle { f.app.state.dictation.text == "Fixture transcript" }
        #expect(f.paste.writes.count == 1)
        f.clock.advance(0.450)
        await settle { f.paste.pasted.count == 2 }
        f.clock.advance(0.450)
        await settle { f.paste.clipboard == original }
    }

    @Test func heldModifiersProduceCopyRecoveryWithoutInjection() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        _ = await f.hold()
        f.paste.modifiersHeld = true
        await f.submit()
        await settle { !f.paste.writes.isEmpty }
        f.clock.advance(0.51)
        await settle { f.app.state.dictation.phase == .result }
        #expect(f.paste.pasted.isEmpty)
        #expect(f.app.state.dictation.delivery == .recovery(copied: true))
    }

    @Test(arguments: ["probe", "activation", "keyboard", "clipboard"])
    func failuresKeepRecoverableText(boundary: String) async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.paste.allowProbe = boundary != "probe"
        f.paste.allowActivation = boundary != "activation"
        f.paste.allowPaste = boundary != "keyboard"
        f.paste.allowWrite = boundary != "clipboard"
        _ = await f.hold(); await f.submit()
        await settle { f.app.state.dictation.phase == .result }
        #expect(f.paste.pasted.isEmpty)
        #expect(f.app.state.dictation.delivery == .recovery(copied: boundary != "clipboard"))
        f.app.send(.copyDictationResult)
        #expect(f.clipboard.values == ["Fixture transcript"])
    }

    @Test func escapeDuringPasteProbeSuppressesLateInjectionAndRestoresOwnedClipboard() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        let original = f.paste.clipboard
        f.paste.suspendProbe = true
        _ = await f.hold(); await f.submit()
        await settle { f.paste.probes == 1 }
        f.key(ShortcutInput.escape)
        f.paste.releaseProbe(true)
        await settle { f.paste.clipboard == original }
        #expect(f.paste.pasted.isEmpty)
        #expect(f.app.state.dictation.phase == .idle)
        #expect(f.app.state.dictation.text.isEmpty)
    }

    @Test func chineseConversionPrecedesAutomaticPasteAndKeepsRawASR() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.app.send(.setTranscriptionLanguage("zh-TW"))
        f.app.send(.importDictionary("OpenWhispr"))
        _ = await f.hold()
        f.key(down: false)
        await settle { await f.transport.requests.count == 1 }
        let request = try #require(await f.transport.requests.first)
        #expect(String(decoding: request.body, as: UTF8.self).contains("以下是繁體中文。語言、學習、軟體、網路。 OpenWhispr"))
        f.app.send(.setTranscriptionLanguage("zh-CN"))
        await f.transport.reply(body: "{\"text\":\"这是中文软件\"}")
        for _ in 0..<5000 {
            if f.app.state.dictation.phase == .result { break }
            try await Task.sleep(for: .milliseconds(2))
        }
        #expect(f.app.state.dictation.phase == .result)
        #expect(f.app.state.dictation.delivery == .pasted)
        #expect(f.app.state.dictation.rawText == "这是中文软件")
        #expect(f.app.state.dictation.text == "這是中文軟體")
        #expect(f.paste.writes == ["這是中文軟體"])
        #expect(f.paste.pasted == [PasteTarget(processID: 101)])
    }
}
