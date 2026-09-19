import AVFoundation
import Darwin
import Foundation
import Testing
import WhisperCore

extension ShortcutFixture {
    func doubleTap(open: Bool = true) async -> ControlledCapture {
        key()
        let capture = microphones.sessions.last!
        if open {
            capture.open()
            capture.deliver([Float](repeating: 0.25, count: 4800))
            await settle { self.app.state.dictation.timing["firstAudio"] != nil }
        }
        clock.advance(0.04); key(down: false)
        clock.advance(0.06); key()
        clock.advance(0.04); key(down: false)
        return capture
    }
}

@Suite("Hands-free Dictation", .serialized) @MainActor
struct HandsFreeWorkflowTests {
    @Test func doubleTapKeepsOneCaptureAndSubmitsOnlyOnStandaloneRelease() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        let capture = await f.doubleTap()
        #expect(f.app.state.dictation.origin == .handsFree)
        #expect(f.app.state.dictation.phase == .recording)
        #expect(f.microphones.sessions.count == 1)
        #expect(capture.physicallyOpen)
        #expect(await f.transport.requests.isEmpty)
        f.paste.frontmost = PasteTarget(processID: 202)
        f.key()
        #expect(f.app.state.dictation.gesture == .stopCandidate)
        #expect(capture.physicallyOpen)
        #expect(await f.transport.requests.isEmpty)
        f.key(down: false)
        #expect(f.app.state.dictation.phase == .processing)
        f.paste.frontmost = PasteTarget(processID: 303)
        await settle { await f.transport.requests.count == 1 }
        await f.transport.reply()
        await settle { f.app.state.dictation.phase == .result }
        #expect(f.paste.pasted == [PasteTarget(processID: 202)])
        #expect(!capture.physicallyOpen)
    }

    @Test func commandCombinationsAndLeftRightOverlapDoNotEndHandsFree() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        _ = await f.doubleTap()
        for key: UInt16 in [8, 9, 48, 55] {
            f.key(); f.key(key); f.key(key, down: false); f.key(down: false)
            #expect(f.app.state.dictation.phase == .recording)
            #expect(f.app.state.dictation.origin == .handsFree)
        }
        f.key(55); f.key(); f.key(down: false); f.key(55, down: false)
        f.key(0); f.key(0, down: false)
        #expect(f.app.state.dictation.phase == .recording)
        #expect(await f.transport.requests.isEmpty)
        f.key(); f.key(down: false)
        await settle { await f.transport.requests.count == 1 }
        await f.transport.reply()
        await settle { f.app.state.dictation.delivery == .pasted }
    }

    @Test func secondHeldPressIsHoldRatherThanDoubleTap() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.key()
        let capture = f.microphones.sessions[0]
        capture.open(); capture.deliver([Float](repeating: 0, count: 4800))
        await settle { f.app.state.dictation.timing["firstAudio"] != nil }
        f.clock.advance(0.03); f.key(down: false)
        f.clock.advance(0.06); f.key()
        f.clock.advance(WhisperApplication.holdThreshold)
        #expect(f.app.state.dictation.origin == .hold)
        #expect(f.app.state.dictation.phase == .recording)
        f.key(down: false)
        await settle { await f.transport.requests.count == 1 }
        await f.transport.reply()
        await settle { f.app.state.dictation.phase == .result }
    }

    @Test func repeatCannotCompleteDoubleTapOrHandsFreeStop() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.key()
        f.key(repeat: true); f.key(repeat: true)
        f.clock.advance(0.04); f.key(down: false)
        #expect(f.app.state.dictation.gesture == .awaitingSecondTap)
        f.key(repeat: true)
        f.clock.advance(WhisperApplication.doubleTapWindow)
        #expect(f.app.state.dictation.phase == .idle)
        f.key(down: false)
        f.microphones.sessions[0].open()
        _ = await f.doubleTap()
        f.key(); f.key(repeat: true); f.key(repeat: true)
        #expect(f.app.state.dictation.phase == .recording)
        f.key(8); f.key(8, down: false); f.key(down: false)
        #expect(f.app.state.dictation.phase == .recording)
        f.app.send(.cancelDictation)
    }

    @Test func lateSecondTapStartsIndependentCandidateAfterOriginalWasDiscarded() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.key(); f.clock.advance(0.04); f.key(down: false)
        let old = f.microphones.sessions[0]
        f.clock.advance(WhisperApplication.doubleTapWindow + 0.01)
        #expect(f.app.state.dictation.cancellation == .rejectedGesture)
        f.key()
        #expect(f.microphones.sessions.count == 2)
        old.open(); old.deliver([0.9])
        #expect(!old.physicallyOpen)
        f.clock.advance(0.04); f.key(down: false)
        f.clock.advance(WhisperApplication.doubleTapWindow)
        f.microphones.sessions[1].open()
        #expect(f.app.state.dictation.phase == .idle)
        #expect(await f.transport.requests.isEmpty)
    }

    @Test func gestureResolutionDoesNotClaimReadinessWithoutAudio() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        let capture = await f.doubleTap(open: false)
        #expect(f.app.state.dictation.origin == .handsFree)
        #expect(f.app.state.dictation.phase == .preparing)
        #expect(f.app.state.dictation.timing["readyFeedback"] == nil)
        capture.open(); capture.deliver([Float](repeating: 0, count: 4800))
        await settle { f.app.state.dictation.phase == .recording }
        #expect(f.app.state.dictation.level == 0)
        f.key(ShortcutInput.escape); f.key(ShortcutInput.escape, down: false)
        #expect(f.app.state.dictation.phase == .idle)
        #expect(!capture.physicallyOpen)
    }

    @Test func escapeDuringTapWindowCannotBeRevivedByTimersOrOldAcquisition() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        f.key(); f.clock.advance(0.04); f.key(down: false)
        let old = f.microphones.sessions[0]
        f.key(ShortcutInput.escape); f.key(ShortcutInput.escape, down: false)
        #expect(f.app.state.dictation.cancellation == .rejectedGesture)
        let current = await f.doubleTap()
        f.clock.advance(1)
        old.open(); old.fail(.captureFailed)
        await Task.yield()
        #expect(f.app.state.dictation.origin == .handsFree)
        #expect(f.app.state.dictation.phase == .recording)
        #expect(current.physicallyOpen)
        #expect(!old.physicallyOpen)
    }

    @Test func cancelledHandsFreeASRDoesNotPasteIntoRetryTarget() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        _ = await f.doubleTap()
        f.key(); f.key(down: false)
        await settle { await f.transport.requests.count == 1 }
        f.key(ShortcutInput.escape); f.key(ShortcutInput.escape, down: false)
        _ = await f.doubleTap()
        await f.transport.reply(0)
        await Task.yield()
        #expect(f.app.state.dictation.phase == .recording)
        #expect(f.paste.pasted.isEmpty)
        f.paste.frontmost = PasteTarget(processID: 404)
        f.key(); f.key(down: false)
        await settle { await f.transport.requests.count == 2 }
        await f.transport.reply(1)
        await settle { f.app.state.dictation.phase == .result }
        #expect(f.paste.pasted == [PasteTarget(processID: 404)])
    }

    @Test func deviceFailureEndsHandsFreeWithoutReplacement() async throws {
        let f = try ShortcutFixture(); defer { f.remove() }
        let capture = await f.doubleTap()
        capture.fail(.inputUnavailable)
        await settle { f.app.state.dictation.phase == .failed }
        #expect(!capture.physicallyOpen)
        #expect(f.microphones.sessions.count == 1)
        f.clock.advance(3600)
        #expect(f.app.state.dictation.phase == .failed)
        #expect(await f.transport.requests.isEmpty)
    }

    @Test func longHandsFreeRecordingUsesBoundedMemoryAndAnIncrementalFile() async throws {
        let transport = StreamingAudioInspection()
        let f = try ShortcutFixture(transport: transport); defer { f.remove() }
        let capture = await f.doubleTap(open: false)
        capture.open()
        capture.deliver([Float](repeating: 0.25, count: 160_000))
        await settle { f.app.state.dictation.phase == .recording }
        f.clock.advance(7_200)
        #expect(f.app.state.dictation.phase == .recording)
        let before = maximumResidentBytes()
        let writing = Task.detached {
            for index in 0..<240 {
                // Distinct frame allocations expose any accidental session-sized retention.
                capture.deliver([Float](repeating: index == 239 ? -0.25 : 0.1, count: 160_000))
            }
        }
        f.key(); f.key(8); f.key(8, down: false); f.key(down: false)
        #expect(f.app.state.dictation.phase == .recording)
        await writing.value
        let growth = maximumResidentBytes() - before
        #expect(growth < 96 * 1_024 * 1_024)
        #expect(f.app.state.dictation.phase == .recording)
        f.key(); f.key(down: false)
        let deadline = ProcessInfo.processInfo.systemUptime + 40
        while f.app.state.dictation.phase == .processing, ProcessInfo.processInfo.systemUptime < deadline {
            try await Task.sleep(for: .milliseconds(10))
        }
        #expect(await transport.finished)
        #expect(f.app.state.dictation.phase == .result)
        #expect(await transport.frameCount >= 38_500_000)
        #expect(await transport.firstMean > 0.20)
        #expect(await transport.lastMean < -0.20)
        #expect(!capture.physicallyOpen)
    }
}

private func maximumResidentBytes() -> Int64 {
    var usage = rusage()
    _ = getrusage(RUSAGE_SELF, &usage)
    return Int64(usage.ru_maxrss)
}

/// Reads the real encoded recording with one small buffer instead of retaining the long fixture.
private actor StreamingAudioInspection: FileHTTPTransport {
    var finished = false
    var frameCount: Int64 = 0
    var firstMean: Float = 0
    var lastMean: Float = 0
    func upload(_ request: URLRequest, file: URL) async throws -> HTTPResponse {
        let audioURL = file.deletingLastPathComponent().appendingPathComponent("inspection.m4a")
        // Map the encoded body instead of loading the long recording into process memory.
        let body = try Data(contentsOf: file, options: .alwaysMapped)
        try multipartAudio(request: request, body: body).write(to: audioURL)
        defer { try? FileManager.default.removeItem(at: audioURL) }
        let audio = try AVAudioFile(forReading: audioURL)
        let buffer = AVAudioPCMBuffer(pcmFormat: audio.processingFormat, frameCapacity: 16_000)!
        while audio.framePosition < audio.length {
            let remaining = AVAudioFrameCount(min(Int64(buffer.frameCapacity), audio.length - audio.framePosition))
            try audio.read(into: buffer, frameCount: remaining)
            if buffer.frameLength == 0 { break }
            let samples = UnsafeBufferPointer(start: buffer.floatChannelData![0], count: Int(buffer.frameLength))
            let mean = samples.reduce(0, +) / Float(samples.count)
            if frameCount == 0 { firstMean = mean }
            lastMean = mean
            frameCount += Int64(buffer.frameLength)
        }
        finished = true
        return HTTPResponse(status: 200, body: Data("{\"text\":\"Long synthetic recording\"}".utf8))
    }
}
