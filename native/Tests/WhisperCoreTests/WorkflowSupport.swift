import AVFoundation
import Foundation
import Testing
import WhisperCore

@MainActor final class ControlledMicrophones: MicrophoneProvider {
    var device = MicrophoneDevice(id: "built-in-fixture", name: "Test input")
    var rejection: DictationFailure?
    private(set) var sessions: [ControlledCapture] = []
    func resolveDevice() throws -> MicrophoneDevice {
        if let rejection { throw rejection }
        return device
    }
    func makeSession(device: MicrophoneDevice, receive: @escaping @Sendable (CaptureEvent) -> Void) -> any MicrophoneSession {
        let capture = ControlledCapture(device: device, receive: receive)
        sessions.append(capture)
        return capture
    }
}

final class ControlledCapture: MicrophoneSession, @unchecked Sendable {
    let device: MicrophoneDevice
    private let receive: @Sendable (CaptureEvent) -> Void
    private let lock = NSLock()
    private var ended = false
    private var opened = false
    private var callbacks: [@Sendable () -> Void] = []
    private(set) var physicallyOpen = false
    private(set) var releases = 0
    init(device: MicrophoneDevice, receive: @escaping @Sendable (CaptureEvent) -> Void) {
        self.device = device
        self.receive = receive
    }
    func start() {}
    func open() {
        let ended = lock.withLock {
            opened = true
            physicallyOpen = true
            return self.ended
        }
        if ended { release() } else { receive(.opened()) }
    }
    func deliver(_ samples: [Float], rate: Double = 48_000) { receive(.frame(AudioFrame(samples: samples, sampleRate: rate))) }
    func fail(_ failure: DictationFailure) { receive(.failed(failure)) }
    func stop(completion: @escaping @Sendable () -> Void) {
        let opened = lock.withLock {
            ended = true
            callbacks.append(completion)
            return self.opened
        }
        if opened { release() }
    }
    private func release() {
        let callbacks = lock.withLock {
            if physicallyOpen { physicallyOpen = false; releases += 1 }
            let callbacks = self.callbacks
            self.callbacks.removeAll()
            return callbacks
        }
        callbacks.forEach { $0() }
    }
}

@MainActor final class ControlledClock: WorkflowClock {
    var now: TimeInterval = 0
    var wallDate = Date(timeIntervalSince1970: 1_789_920_000)
    private var scheduled: [Action] = []
    func schedule(after seconds: TimeInterval, _ action: @escaping @MainActor @Sendable () -> Void) -> any ScheduledAction {
        let item = Action(deadline: now + seconds, action: action)
        scheduled.append(item)
        return item
    }
    func advance(_ seconds: TimeInterval) {
        now += seconds
        wallDate.addTimeInterval(seconds)
        let ready = scheduled.filter { $0.deadline <= now }
        scheduled.removeAll { $0.deadline <= now }
        ready.filter { !$0.cancelled }.forEach { $0.action() }
    }
    private final class Action: ScheduledAction {
        let deadline: TimeInterval
        let action: @MainActor @Sendable () -> Void
        var cancelled = false
        init(deadline: TimeInterval, action: @escaping @MainActor @Sendable () -> Void) { self.deadline = deadline; self.action = action }
        func cancel() { cancelled = true }
    }
}

actor ControlledHTTPTransport: FileHTTPTransport {
    struct Request: Sendable {
        let request: URLRequest
        let body: Data
        let audio: [Float]
    }
    private(set) var requests: [Request] = []
    private var replies: [Int: CheckedContinuation<HTTPResponse, any Error>] = [:]
    func upload(_ request: URLRequest, file: URL) async throws -> HTTPResponse {
        let audioURL = file.deletingLastPathComponent().appendingPathComponent("audio.m4a")
        let audioFile = try AVAudioFile(forReading: audioURL)
        let buffer = AVAudioPCMBuffer(pcmFormat: audioFile.processingFormat, frameCapacity: AVAudioFrameCount(audioFile.length))!
        try audioFile.read(into: buffer)
        let samples = Array(UnsafeBufferPointer(start: buffer.floatChannelData![0], count: Int(buffer.frameLength)))
        let index = requests.count
        requests.append(Request(request: request, body: try Data(contentsOf: file), audio: samples))
        return try await withCheckedThrowingContinuation { replies[index] = $0 }
    }
    func reply(_ index: Int = 0, status: Int = 200, body: String = "{\"text\":\"Fixture transcript\"}") {
        replies.removeValue(forKey: index)?.resume(returning: HTTPResponse(status: status, body: Data(body.utf8)))
    }
    func fail(_ index: Int = 0) { replies.removeValue(forKey: index)?.resume(throwing: DictationFailure.network) }
}

@MainActor final class ControlledClipboard: TextClipboard {
    private(set) var values: [String] = []
    func write(_ text: String) { values.append(text) }
}

@MainActor final class DictationFixture {
    let profile: ProfileFixture
    let microphones = ControlledMicrophones()
    let clock = ControlledClock()
    let transport = ControlledHTTPTransport()
    let clipboard = ControlledClipboard()
    let app: WhisperApplication
    init(server: String = "http://localhost:8178/v1?route=fixture", credential: String? = nil, transport customTransport: (any FileHTTPTransport)? = nil) throws {
        profile = try ProfileFixture(keychain: credential != nil)
        app = WhisperApplication(profile: profile.profile, credentials: profile.credentials, microphones: microphones,
            transcriber: SelfHostedTranscriber(transport: customTransport ?? transport), clock: clock, clipboard: clipboard)
        app.send(.saveASR(.init(serverURL: server, model: "whisper-fixture"), credential: credential.map(CredentialChange.replace) ?? .unchanged))
    }
    func begin() -> ControlledCapture {
        app.send(.startDictation)
        return microphones.sessions.last!
    }
    func record() async -> ControlledCapture {
        let capture = begin()
        capture.open()
        capture.deliver([Float](repeating: 0, count: 4800))
        await settle { self.app.state.dictation.phase == .recording }
        return capture
    }
    func remove() { app.send(.cancelDictation); profile.remove() }
}

@MainActor func settle(_ condition: () async -> Bool, sourceLocation: SourceLocation = #_sourceLocation) async {
    for _ in 0..<500 {
        if await condition() { return }
        try? await Task.sleep(for: .milliseconds(2))
    }
    Issue.record("Workflow did not reach the expected observable outcome.", sourceLocation: sourceLocation)
}
