import Foundation
import Testing
import WhisperCore

@Suite(.serialized) @MainActor
struct MicrophoneWorkflowTests {
    private static let builtIn = MicrophoneDevice(id: "fixture-built-in", name: "USB-looking name", category: .builtIn)
    private static let phone = MicrophoneDevice(id: "fixture-phone", name: "Custom input", category: .continuity)
    private static let external = MicrophoneDevice(id: "fixture-external", name: "MacBook-looking name", category: .external)

    @Test(arguments: MicrophoneMode.allCases)
    func selectedModesPersistAndDriveRealRecordingToResult(mode: MicrophoneMode) async throws {
        let f = try SelectionFixture()
        defer { f.remove() }
        f.inputs.snapshot = MicrophoneSnapshot(devices: [Self.external, Self.phone, Self.builtIn], systemDefaultID: Self.external.id, lidClosed: false)
        let preference = MicrophonePreference(mode: mode, deviceID: Self.phone.id, deviceName: Self.phone.name)
        f.app.send(.setMicrophone(preference))
        #expect(f.app.state.configurationError == nil)
        let reopened = f.reopen()
        #expect(reopened.state.settings.microphone == preference)
        reopened.send(.startDictation)
        let capture = try #require(f.inputs.sessions.last)
        let expected = mode == .system ? Self.external : mode == .specific ? Self.phone : Self.builtIn
        #expect(capture.device == expected)
        capture.open()
        capture.deliver([Float](repeating: 0, count: 4800))
        await settle { reopened.state.dictation.phase == .recording }
        reopened.send(.stopDictation)
        await settle { await f.transport.requests.count == 1 }
        #expect(!capture.physicallyOpen)
        await f.transport.reply(body: "{\"text\":\"Selected input transcript\"}")
        await settle { reopened.state.dictation.phase == .result }
        reopened.send(.copyDictationResult)
        #expect(f.clipboard.values == ["Selected input transcript"])
        #expect(reopened.state.dictation.rawText == "Selected input transcript")
    }

    @Test(arguments: [false, true])
    func autoUsesNativeCategoryAndLidRatherThanDeviceLabels(closed: Bool) async throws {
        let f = try SelectionFixture()
        defer { f.remove() }
        f.inputs.snapshot = MicrophoneSnapshot(devices: [Self.external, Self.builtIn, Self.phone], systemDefaultID: Self.external.id, lidClosed: closed)
        f.app.send(.startDictation)
        let capture = try #require(f.inputs.sessions.last)
        #expect(capture.device.id == (closed ? Self.phone.id : Self.builtIn.id))
        capture.open()
        f.app.send(.cancelDictation)
        #expect(!capture.physicallyOpen)
    }

    @Test func autoFallbackOrderAndUnknownLidUseFreshRequestSnapshots() throws {
        let f = try SelectionFixture()
        defer { f.remove() }
        let cases: [(MicrophoneSnapshot, MicrophoneDevice)] = [
            (.init(devices: [Self.phone, Self.builtIn], systemDefaultID: Self.phone.id, lidClosed: nil), Self.builtIn),
            (.init(devices: [Self.builtIn, Self.external], systemDefaultID: Self.builtIn.id, lidClosed: true), Self.external),
            (.init(devices: [Self.builtIn], systemDefaultID: Self.builtIn.id, lidClosed: true), Self.builtIn),
            (.init(devices: [Self.external, Self.phone], systemDefaultID: Self.external.id, lidClosed: false), Self.external),
            (.init(devices: [Self.phone, Self.external], systemDefaultID: Self.external.id, lidClosed: nil), Self.external)
        ]
        for (snapshot, expected) in cases {
            f.inputs.snapshot = snapshot
            f.app.send(.startDictation)
            let capture = try #require(f.inputs.sessions.last)
            #expect(capture.device == expected)
            capture.open()
            f.app.send(.cancelDictation)
            #expect(!capture.physicallyOpen)
        }
    }

    @Test func changesDuringAcquisitionAndRecordingOnlyAffectNextRequest() async throws {
        let f = try SelectionFixture()
        defer { f.remove() }
        f.inputs.snapshot = .init(devices: [Self.builtIn, Self.phone, Self.external], systemDefaultID: Self.builtIn.id, lidClosed: false)
        f.app.send(.startDictation)
        let first = try #require(f.inputs.sessions.last)
        f.inputs.snapshot = .init(devices: [Self.external, Self.phone, Self.builtIn], systemDefaultID: Self.external.id, lidClosed: true)
        f.app.send(.refreshMicrophones)
        f.app.send(.setMicrophone(.init(mode: .specific, deviceID: Self.phone.id, deviceName: Self.phone.name)))
        #expect(f.inputs.sessions.count == 1)
        #expect(first.device.id == Self.builtIn.id)
        first.open()
        first.deliver([Float](repeating: 0, count: 4800))
        await settle { f.app.state.dictation.phase == .recording }
        f.app.send(.stopDictation)
        await settle { await f.transport.requests.count == 1 }
        await f.transport.reply()
        await settle { f.app.state.dictation.phase == .result }
        f.app.send(.startDictation)
        let second = try #require(f.inputs.sessions.last)
        #expect(second.device.id == Self.phone.id)
        second.open()
        second.deliver([Float](repeating: 0, count: 4800))
        await settle { f.app.state.dictation.phase == .recording }
        f.app.send(.setMicrophone(.init(mode: .system)))
        f.app.send(.refreshMicrophones)
        #expect(f.inputs.sessions.count == 2)
        #expect(second.physicallyOpen)
        f.app.send(.cancelDictation)
        f.app.send(.startDictation)
        let third = try #require(f.inputs.sessions.last)
        #expect(third.device.id == Self.external.id)
        third.open()
        f.app.send(.cancelDictation)
    }

    @Test func staleSpecificUIDDoesNotRemapSameNamedDeviceAndBuiltInDoesNotFallback() throws {
        let f = try SelectionFixture()
        defer { f.remove() }
        let replacement = MicrophoneDevice(id: "different-uid", name: Self.phone.name, category: .continuity)
        f.inputs.snapshot = .init(devices: [replacement, Self.external], systemDefaultID: Self.external.id)
        for preference in [
            MicrophonePreference(mode: .specific, deviceID: Self.phone.id, deviceName: Self.phone.name),
            .init(mode: .builtIn), .init(mode: .specific, deviceID: "default"), .init(mode: .specific)
        ] {
            f.app.send(.setMicrophone(preference))
            f.app.send(.startDictation)
            #expect(f.app.state.dictation.failure == .inputUnavailable)
            #expect(f.inputs.sessions.isEmpty)
        }
        #expect(f.reopen().state.settings.microphone.mode == .specific)
    }

    @Test func missingSystemUIDCannotSilentlySelectAnotherInput() throws {
        let f = try SelectionFixture()
        defer { f.remove() }
        f.inputs.snapshot = .init(devices: [Self.external, Self.phone], systemDefaultID: "stale-system-uid", lidClosed: nil)
        for mode in [MicrophoneMode.auto, .system] {
            f.app.send(.setMicrophone(.init(mode: mode)))
            f.app.send(.startDictation)
            #expect(f.app.state.dictation.failure == .inputUnavailable)
            #expect(f.inputs.sessions.isEmpty)
        }
    }

    @Test(arguments: [DictationFailure.permissionDenied, .inputUnavailable, .captureFailed])
    func failedChosenInputNeverOpensHealthyAlternative(failure: DictationFailure) async throws {
        let f = try SelectionFixture()
        defer { f.remove() }
        f.inputs.snapshot = .init(devices: [Self.builtIn, Self.phone], systemDefaultID: Self.builtIn.id, lidClosed: true)
        f.app.send(.startDictation)
        let selected = try #require(f.inputs.sessions.last)
        #expect(selected.device.id == Self.phone.id)
        selected.open()
        selected.fail(failure)
        await settle { f.app.state.dictation.phase == .failed }
        #expect(f.app.state.dictation.failure == failure)
        #expect(f.inputs.sessions.count == 1)
        #expect(!selected.physicallyOpen)
        #expect(await f.transport.requests.isEmpty)
        f.inputs.snapshot = .init(devices: [Self.builtIn], systemDefaultID: Self.builtIn.id, lidClosed: false)
        f.app.send(.startDictation)
        let retry = try #require(f.inputs.sessions.last)
        #expect(retry.device.id == Self.builtIn.id)
        retry.open()
        f.app.send(.cancelDictation)
    }

    @Test func externalDelayedFramesTimeoutDisconnectAndReconnectKeepOwnership() async throws {
        let f = try SelectionFixture()
        defer { f.remove() }
        f.inputs.snapshot = .init(devices: [Self.phone, Self.builtIn], systemDefaultID: Self.builtIn.id, lidClosed: true)
        f.app.send(.startDictation)
        let timedOut = try #require(f.inputs.sessions.last)
        timedOut.open()
        await settle { f.app.state.dictation.timing["acquisitionCompleted"] != nil }
        f.clock.advance(10)
        #expect(f.app.state.dictation.failure == .noAudio)
        #expect(!timedOut.physicallyOpen)
        #expect(f.inputs.sessions.count == 1)
        f.app.send(.startDictation)
        let disconnected = try #require(f.inputs.sessions.last)
        disconnected.open()
        disconnected.deliver([Float](repeating: 0, count: 4800))
        await settle { f.app.state.dictation.phase == .recording }
        f.inputs.snapshot = .init(devices: [Self.builtIn], systemDefaultID: Self.builtIn.id, lidClosed: true)
        disconnected.fail(.inputUnavailable)
        await settle { f.app.state.dictation.phase == .failed }
        #expect(!disconnected.physicallyOpen)
        f.inputs.snapshot = .init(devices: [Self.phone, Self.builtIn], systemDefaultID: Self.builtIn.id, lidClosed: true)
        f.app.send(.refreshMicrophones)
        disconnected.deliver([Float](repeating: 1, count: 4800))
        await Task.yield()
        #expect(f.app.state.dictation.phase == .failed)
        #expect(f.inputs.sessions.count == 2)
        f.app.send(.startDictation)
        let reconnected = try #require(f.inputs.sessions.last)
        #expect(reconnected.device.id == Self.phone.id)
        reconnected.open()
        reconnected.deliver([Float](repeating: 0, count: 4800))
        await settle { f.app.state.dictation.phase == .recording }
        timedOut.deliver([Float](repeating: 1, count: 4800))
        #expect(f.app.state.dictation.phase == .recording)
        f.app.send(.stopDictation)
        await settle { await f.transport.requests.count == 1 }
        await f.transport.reply()
        await settle { f.app.state.dictation.phase == .result }
        #expect(!reconnected.physicallyOpen)
    }

    @Test func oldFreshNativeSettingsDecodeWithoutMicrophoneField() throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        try FileManager.default.createDirectory(at: profile.profile.directory, withIntermediateDirectories: true)
        let json = "{\"version\":1,\"settings\":{\"language\":\"zh-CN\",\"asr\":{\"serverURL\":\"http://localhost:8178\",\"model\":\"fixture\"}}}"
        try Data(json.utf8).write(to: profile.profile.directory.appendingPathComponent("settings.json"))
        let app = profile.open()
        #expect(app.state.configurationError == nil)
        #expect(app.state.settings.microphone == .init())
        #expect(app.state.settings.language == .simplifiedChinese)
        #expect(MicrophoneMode.auto.title(in: .simplifiedChinese) == "自动")
        #expect(MicrophoneMode.specific.title(in: .english) == "Specific microphone")
    }
}

@MainActor private final class SelectionInputs: MicrophoneProvider {
    var snapshot = MicrophoneSnapshot()
    var sessions: [ControlledCapture] = []
    func resolveDevice() throws -> MicrophoneDevice {
        guard let first = snapshot.devices.first else { throw DictationFailure.inputUnavailable }
        return first
    }
    func inputSnapshot() -> MicrophoneSnapshot { snapshot }
    func makeSession(device: MicrophoneDevice, receive: @escaping @Sendable (CaptureEvent) -> Void) -> any MicrophoneSession {
        let capture = ControlledCapture(device: device, receive: receive)
        sessions.append(capture)
        return capture
    }
}

@MainActor private final class SelectionFixture {
    let profile: ProfileFixture
    let inputs = SelectionInputs()
    let clock = ControlledClock()
    let transport = ControlledHTTPTransport()
    let clipboard = ControlledClipboard()
    lazy var app = reopen()
    init() throws {
        profile = try ProfileFixture(keychain: false)
        app.send(.saveASR(.init(serverURL: "http://localhost:8178", model: "fixture"), credential: .unchanged))
    }
    func reopen() -> WhisperApplication {
        WhisperApplication(profile: profile.profile, credentials: profile.credentials, microphones: inputs,
            transcriber: SelfHostedTranscriber(transport: transport), clock: clock, clipboard: clipboard)
    }
    func remove() {
        app.send(.cancelDictation)
        profile.remove()
    }
}
