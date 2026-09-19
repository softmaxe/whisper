import Foundation
import Testing
import WhisperCore

struct BindingFixture: Sendable {
    let value: String
    let code: UInt16
    var modifiers: [UInt16] = []
    var keyName: String?
    @MainActor func press(_ f: ShortcutFixture) {
        modifiers.forEach { f.key($0) }
        f.app.send(.shortcut(.init(keyCode: code, isDown: true, keyName: keyName)))
    }
    @MainActor func release(_ f: ShortcutFixture) {
        f.app.send(.shortcut(.init(keyCode: code, isDown: false, keyName: keyName)))
        modifiers.reversed().forEach { f.key($0, down: false) }
    }
}

private let bindingFixtures = [
    BindingFixture(value: "RightCommand", code: 54),
    BindingFixture(value: "RightOption", code: 61),
    BindingFixture(value: "RightControl", code: 62),
    BindingFixture(value: "RightShift", code: 60),
    BindingFixture(value: "Control+Shift+K", code: 40, modifiers: [59, 56]),
    BindingFixture(value: "RightControl+K", code: 40, modifiers: [62]),
    BindingFixture(value: "Alt+F7", code: 98, modifiers: [58]),
    BindingFixture(value: "Control+,", code: 43, modifiers: [59]),
    BindingFixture(value: "Control+Esc", code: 53, modifiers: [59]),
    BindingFixture(value: "GLOBE", code: 63),
    BindingFixture(value: "F8", code: 100),
    BindingFixture(value: "F24", code: 200, keyName: "F24"),
    BindingFixture(value: "Space", code: 49),
    BindingFixture(value: "MouseButton4", code: 0x1003),
    BindingFixture(value: "MouseButton5", code: 0x1004),
    BindingFixture(value: "Control+MediaPlayPause", code: 0x2010, modifiers: [59])
]

@Suite("Configured shortcuts", .serialized) @MainActor
struct ConfiguredShortcutTests {
    @Test(arguments: bindingFixtures, [false, true])
    func everyCategoryReopensAndDrivesTheSameDictation(binding: BindingFixture, handsFree: Bool) async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.app.send(.saveShortcuts([binding.value]))
        var desktop = DesktopPreferences()
        desktop.pauseMediaOnDictation = true
        f.app.send(.saveDesktopPreferences(desktop))
        #expect(f.app.state.shortcutError == nil)
        f.app = WhisperApplication(profile: f.profile.profile, credentials: f.profile.credentials,
            microphones: f.microphones, transcriber: SelfHostedTranscriber(transport: f.transport),
            clock: f.clock, clipboard: f.clipboard, pasteSystem: f.paste,
            correctionSystem: InertCorrectionMonitoringFixture(), desktopEffects: f.desktop)
        #expect(f.app.state.settings.shortcuts == [try ShortcutBinding(binding.value).value])
        binding.press(f)
        let capture = try #require(f.microphones.sessions.first)
        capture.open(); capture.deliver([Float](repeating: 0.25, count: 4800))
        await settle { f.app.state.dictation.timing["firstAudio"] != nil }
        if handsFree {
            f.clock.advance(0.04); binding.release(f)
            f.clock.advance(0.05); binding.press(f)
            f.clock.advance(0.04); binding.release(f)
            #expect(f.app.state.dictation.origin == .handsFree)
            binding.press(f)
            #expect(f.app.state.dictation.phase == .recording)
            #expect(await f.transport.requests.isEmpty)
        } else {
            f.clock.advance(WhisperApplication.holdThreshold)
            #expect(f.app.state.dictation.origin == .hold)
        }
        await settle { f.desktop.mediaEvents == ["pause"] }
        binding.release(f)
        await settle { await f.transport.requests.count == 1 }
        await f.transport.reply()
        await settle { f.app.state.dictation.phase == .result }
        #expect(f.app.state.dictation.delivery == .pasted)
        #expect(f.microphones.sessions.count == 1)
        #expect(!capture.physicallyOpen)
        #expect(f.desktop.cues == [.ready, .stopped])
        f.clock.advance(0.45)
        await f.app.prepareForTermination()
        #expect(f.desktop.mediaEvents == ["pause", "resume"])
        let reopened = f.profile.open()
        reopened.send(.loadHistory)
        await settle { reopened.state.history.isLoaded && !reopened.state.history.isLoading }
        #expect(reopened.state.history.entries.count == 1)
        #expect(reopened.state.history.entries.first?.text == f.app.state.dictation.text)
        #expect(reopened.state.history.entries.first?.id == f.app.state.dictation.requestID)
    }

    @Test(arguments: ["K", "Control", "LeftCommand", "Control+Alt", "Fn+A", "Shift+Fn", "Control+MouseButton4",
                      "LeftCtrl+RightCtrl+K", "Command+C", "Cmd+Space", "Ctrl+Alt+Shift+K", "Esc", "F25"])
    func rejectedChoicesKeepTheWorkingConfiguration(value: String) throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.app.send(.saveShortcuts([value]))
        #expect(f.app.state.shortcutError != nil)
        #expect(f.app.state.settings.shortcuts == ["RightCommand"])
        #expect(!f.app.state.settingsSaved)
        let reopened = f.profile.open()
        #expect(reopened.state.settings.shortcuts == ["RightCommand"])
        #expect(f.microphones.sessions.isEmpty)
    }

    @Test(arguments: bindingFixtures)
    func everyCategoryRejectsShortTapsWithoutRequestsOrRetainedAudio(binding: BindingFixture) async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.app.send(.saveShortcuts([binding.value]))
        binding.press(f)
        let capture = try #require(f.microphones.sessions.first)
        capture.open(); capture.deliver([Float](repeating: 0, count: 4800))
        await settle { f.app.state.dictation.timing["firstAudio"] != nil }
        f.clock.advance(0.03); binding.release(f)
        f.clock.advance(WhisperApplication.doubleTapWindow)
        #expect(f.app.state.dictation.phase == .idle)
        #expect(f.app.state.dictation.cancellation == .rejectedGesture)
        #expect(await f.transport.requests.isEmpty)
        #expect(!capture.physicallyOpen)
        await settle {
            ((try? FileManager.default.contentsOfDirectory(atPath: f.profile.profile.directory.appendingPathComponent("Temporary").path)) ?? []).isEmpty
        }
        #expect(f.desktop.cues.isEmpty)
        await f.app.prepareForTermination()
        let reopened = f.profile.open()
        reopened.send(.loadHistory)
        await settle { reopened.state.history.isLoaded && !reopened.state.history.isLoading }
        #expect(reopened.state.history.entries.isEmpty)
    }

    @Test func duplicatesAndEmptyListsDoNotReplaceMultipleChoices() throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.app.send(.saveShortcuts(["RightOption", "Ctrl+Shift+K", "MouseButton5"]))
        let saved = f.app.state.settings.shortcuts
        #expect(saved == ["RightAlt", "Control+Shift+K", "MouseButton5"])
        for choices in [["RightAlt", "RightOption"], ["Ctrl+K", "Control+K"], ["Control+K", "RightControl+K"], []] {
            f.app.send(.saveShortcuts(choices))
            #expect(f.app.state.shortcutError != nil)
            #expect(f.app.state.settings.shortcuts == saved)
        }
        #expect(f.profile.open().state.settings.shortcuts == saved)
    }

    @Test func anEarlierNativeProfileGetsTheRightCommandDefault() throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        let url = f.profile.profile.directory.appendingPathComponent("settings.json")
        var document = try #require(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
        var settings = try #require(document["settings"] as? [String: Any])
        settings.removeValue(forKey: "shortcuts")
        document["settings"] = settings
        try JSONSerialization.data(withJSONObject: document).write(to: url)
        #expect(f.profile.open().state.settings.shortcuts == ["RightCommand"])
    }

    @Test func captureFieldSavesInputsWithoutStartingDictationAndKeepsInvalidErrors() throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.app.send(.beginShortcutCapture(index: nil))
        f.key(55); f.key(8)
        #expect(f.app.state.shortcutError == .reserved)
        #expect(f.app.state.shortcutCapture.isActive)
        f.key(8, down: false); f.key(55, down: false)
        f.key(61); f.key(61, down: false)
        #expect(!f.app.state.shortcutCapture.isActive)
        #expect(f.app.state.settings.shortcuts == ["RightCommand", "RightAlt"])
        f.app.send(.beginShortcutCapture(index: 1))
        f.key(63); f.key(63, down: false)
        #expect(f.app.state.settings.shortcuts == ["RightCommand", "GLOBE"])
        f.app.send(.beginShortcutCapture(index: 1))
        f.key(0x1004); f.key(0x1004, down: false)
        #expect(f.app.state.settings.shortcuts == ["RightCommand", "MouseButton5"])
        #expect(f.microphones.sessions.isEmpty)
        #expect(f.app.state.dictation.phase == .idle)
        f.app.send(.beginShortcutCapture(index: nil))
        f.app.send(.closeMainWindow)
        #expect(!f.app.state.shortcutCapture.isActive)
    }

    @Test func escapeIsReservedAloneButModifierEscapeCoexistsWithCancellation() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.app.send(.saveShortcuts(["Esc"]))
        #expect(f.app.state.shortcutError == .reserved)
        _ = await f.hold()
        f.key(53)
        #expect(f.app.state.dictation.phase == .idle)
        f.key(53, down: false); f.key(down: false)
        f.app.send(.saveShortcuts(["Control+Esc"]))
        let chord = BindingFixture(value: "Control+Esc", code: 53, modifiers: [59])
        chord.press(f)
        let capture = try #require(f.microphones.sessions.last)
        capture.open(); capture.deliver([Float](repeating: 0, count: 4800))
        await settle { f.app.state.dictation.timing["firstAudio"] != nil }
        f.clock.advance(0.04); chord.release(f)
        f.clock.advance(0.04); chord.press(f)
        f.clock.advance(0.04); chord.release(f)
        #expect(f.app.state.dictation.origin == .handsFree)
        f.key(53)
        #expect(f.app.state.dictation.phase == .idle)
        #expect(!capture.physicallyOpen)
    }

    @Test func modifierEscapeDuringProcessingCancelsRatherThanStartingAnotherControl() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        let binding = BindingFixture(value: "Control+Esc", code: 53, modifiers: [59])
        f.app.send(.saveShortcuts([binding.value]))
        binding.press(f)
        let capture = try #require(f.microphones.sessions.first)
        capture.open(); capture.deliver([Float](repeating: 0, count: 4800))
        await settle { f.app.state.dictation.timing["firstAudio"] != nil }
        f.clock.advance(WhisperApplication.holdThreshold)
        binding.release(f)
        await settle { await f.transport.requests.count == 1 }
        binding.press(f)
        #expect(f.app.state.dictation.phase == .idle)
        await f.transport.reply(body: "{\"text\":\"Ended control result\"}")
        try await Task.sleep(for: .milliseconds(10))
        #expect(f.paste.pasted.isEmpty)
        #expect(f.app.state.dictation.text.isEmpty)
        await f.app.prepareForTermination()
        let reopened = f.profile.open()
        reopened.send(.loadHistory)
        await settle { reopened.state.history.isLoaded && !reopened.state.history.isLoading }
        #expect(reopened.state.history.entries.isEmpty)
    }

    @Test func aNewlyConfiguredChordCannotTakeOverAnExistingCandidate() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.key()
        let capture = try #require(f.microphones.sessions.first)
        capture.open(); capture.deliver([Float](repeating: 0, count: 4800))
        await settle { f.app.state.dictation.timing["firstAudio"] != nil }
        f.app.send(.saveShortcuts(["Command+K"]))
        f.key(40)
        #expect(f.app.state.dictation.cancellation == .rejectedGesture)
        #expect(!capture.physicallyOpen)
        #expect(f.desktop.cues.isEmpty)
        #expect(await f.transport.requests.isEmpty)
        f.key(40, down: false); f.key(down: false)
        f.key(55); f.key(40)
        #expect(f.microphones.sessions.count == 2)
        #expect(f.app.state.dictation.phase == .preparing)
        f.microphones.sessions.last?.open()
        f.app.send(.cancelDictation)
    }

    @Test func configuredChordCanRefineItsProvisionalModifierBinding() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.app.send(.saveShortcuts(["RightCommand", "Command+K"]))
        f.key()
        let capture = f.microphones.sessions[0]
        capture.open(); capture.deliver([Float](repeating: 0.2, count: 4800))
        await settle { f.app.state.dictation.timing["firstAudio"] != nil }
        f.clock.advance(0.05); f.key(40)
        f.clock.advance(WhisperApplication.holdThreshold)
        f.key(40, down: false); f.key(down: false)
        await settle { await f.transport.requests.count == 1 }
        await f.transport.reply()
        await settle { f.app.state.dictation.phase == .result }
        #expect(f.microphones.sessions.count == 1)
        #expect(f.paste.pasted.count == 1)
    }

    @Test func preferenceChangeAppliesToNextRequestAndWrongSideDoesNotStart() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        _ = await f.hold()
        f.app.send(.saveShortcuts(["RightControl+K"]))
        await f.submit()
        await settle { f.app.state.dictation.phase == .result }
        f.key(59); f.key(40); f.key(40, down: false); f.key(59, down: false)
        #expect(f.microphones.sessions.count == 1)
        f.key(62); f.key(40)
        #expect(f.microphones.sessions.count == 2)
        f.app.send(.cancelDictation)
        f.microphones.sessions[1].open()
    }

    @Test func suppressionClaimsOnlyConfiguredKeysAndBalancesReleaseAfterChange() {
        var policy = ShortcutEventSuppression()
        let modifier = policy.consume(.init(keyCode: 54, isDown: true), pressed: [54], bindings: ["RightCommand"], capturing: false)
        let ordinary = policy.consume(.init(keyCode: 8, isDown: true), pressed: [54, 8], bindings: ["RightCommand"], capturing: false)
        let bound = policy.consume(.init(keyCode: 40, isDown: true), pressed: [59, 40], bindings: ["Control+K"], capturing: false)
        let repeated = policy.consume(.init(keyCode: 40, isDown: true, isRepeat: true), pressed: [59, 40], bindings: [], capturing: false)
        let released = policy.consume(.init(keyCode: 40, isDown: false), pressed: [59], bindings: [], capturing: false)
        let unboundMouse = policy.consume(.init(keyCode: 0x1003, isDown: true), pressed: [0x1003], bindings: ["MouseButton5"], capturing: false)
        let boundMouse = policy.consume(.init(keyCode: 0x1004, isDown: true), pressed: [0x1004], bindings: ["MouseButton5"], capturing: false)
        let captureMouse = policy.consume(.init(keyCode: 0x1004, isDown: false), pressed: [], bindings: ["MouseButton5"], capturing: true)
        let captureKeyboard = policy.consume(.init(keyCode: 8, isDown: true), pressed: [55, 8], bindings: [], capturing: true)
        #expect(!modifier && !ordinary && bound && repeated && released)
        #expect(!unboundMouse && boundMouse && !captureMouse && captureKeyboard)
    }
}
