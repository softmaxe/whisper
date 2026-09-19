@preconcurrency import AVFoundation
import CoreAudio
import Foundation

@MainActor public final class NativeMicrophoneProvider: MicrophoneProvider {
    public init() {}
    public func resolveDevice() throws -> MicrophoneDevice {
        try MicrophoneSelection.resolve(.init(mode: .builtIn), snapshot: inputSnapshot())
    }

    public func inputSnapshot() -> MicrophoneSnapshot { NativeMicrophoneInventory.snapshot() }

    public func makeSession(device: MicrophoneDevice, receive: @escaping @Sendable (CaptureEvent) -> Void) -> any MicrophoneSession {
        NativeMicrophoneSession(device: device, receive: receive)
    }
}

/// Each physical request has its own queue, so an abandoned blocking device open cannot delay a retry.
private final class NativeMicrophoneSession: NSObject, MicrophoneSession, AVCaptureAudioDataOutputSampleBufferDelegate, @unchecked Sendable {
    private let device: MicrophoneDevice
    private let receive: @Sendable (CaptureEvent) -> Void
    private let control = DispatchQueue(label: "local.whisper.capture." + UUID().uuidString)
    private let samples = DispatchQueue(label: "local.whisper.samples." + UUID().uuidString)
    private let lock = NSLock()
    private var ended = false
    private var session: AVCaptureSession?
    private var observers: [NSObjectProtocol] = []

    init(device: MicrophoneDevice, receive: @escaping @Sendable (CaptureEvent) -> Void) {
        self.device = device
        self.receive = receive
    }

    func start() {
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized: begin()
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .audio) { [self] allowed in
                if allowed { begin() } else { emit(.failed(.permissionDenied)) }
            }
        default: emit(.failed(.permissionDenied))
        }
    }

    private func begin() {
        control.async { [self] in
            guard !isEnded else { return }
            do {
                guard let inputDevice = AVCaptureDevice(uniqueID: device.id), inputDevice.isConnected, !inputDevice.isSuspended else {
                    throw DictationFailure.inputUnavailable
                }
                let input = try AVCaptureDeviceInput(device: inputDevice)
                guard !isEnded else { return }
                let session = AVCaptureSession()
                self.session = session
                let output = AVCaptureAudioDataOutput()
                output.audioSettings = [
                    AVFormatIDKey: kAudioFormatLinearPCM, AVSampleRateKey: 48_000,
                    AVNumberOfChannelsKey: 1, AVLinearPCMBitDepthKey: 32,
                    AVLinearPCMIsFloatKey: true, AVLinearPCMIsNonInterleaved: false
                ]
                output.setSampleBufferDelegate(self, queue: samples)
                session.beginConfiguration()
                guard session.canAddInput(input), session.canAddOutput(output) else {
                    session.commitConfiguration()
                    throw DictationFailure.captureFailed
                }
                session.addInput(input)
                session.addOutput(output)
                session.commitConfiguration()
                for name in [AVCaptureSession.runtimeErrorNotification, AVCaptureSession.wasInterruptedNotification, AVCaptureSession.didStopRunningNotification] {
                    observers.append(NotificationCenter.default.addObserver(forName: name, object: session, queue: nil) { [weak self] _ in
                        self?.emit(.failed(.captureFailed))
                    })
                }
                observers.append(NotificationCenter.default.addObserver(forName: AVCaptureDevice.wasDisconnectedNotification, object: inputDevice, queue: nil) { [weak self] _ in
                    self?.emit(.failed(.inputUnavailable))
                })
                guard !isEnded else { releaseCapture(); return }
                emit(.opened(at: ProcessInfo.processInfo.systemUptime))
                session.startRunning()
                guard !isEnded else { releaseCapture(); return }
                guard session.isRunning else { throw DictationFailure.captureFailed }
            } catch {
                releaseCapture()
                emit(.failed(error as? DictationFailure ?? .captureFailed))
            }
        }
    }

    private var isEnded: Bool { lock.withLock { ended } }
    private func emit(_ event: CaptureEvent) { if !isEnded { receive(event) } }

    func stop(completion: @escaping @Sendable () -> Void) {
        lock.withLock { ended = true }
        control.async { [self] in
            releaseCapture()
            samples.sync {}
            completion()
        }
    }

    private func releaseCapture() {
        for observer in observers { NotificationCenter.default.removeObserver(observer) }
        observers.removeAll()
        guard let session else { return }
        if session.isRunning { session.stopRunning() }
        for output in session.outputs {
            (output as? AVCaptureAudioDataOutput)?.setSampleBufferDelegate(nil, queue: nil)
            session.removeOutput(output)
        }
        for input in session.inputs { session.removeInput(input) }
        self.session = nil
    }

    func captureOutput(_ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer, from connection: AVCaptureConnection) {
        guard !isEnded, CMSampleBufferDataIsReady(sampleBuffer) else { return }
        let count = CMSampleBufferGetNumSamples(sampleBuffer)
        guard count > 0, count <= 192_000,
              let description = CMSampleBufferGetFormatDescription(sampleBuffer),
              let format = CMAudioFormatDescriptionGetStreamBasicDescription(description)?.pointee,
              format.mFormatID == kAudioFormatLinearPCM, format.mChannelsPerFrame == 1,
              format.mBitsPerChannel == 32, format.mFormatFlags & kAudioFormatFlagIsFloat != 0 else { return }
        var values = [Float](repeating: 0, count: count)
        let status = values.withUnsafeMutableBytes { bytes in
            var list = AudioBufferList(mNumberBuffers: 1, mBuffers: AudioBuffer(mNumberChannels: 1, mDataByteSize: UInt32(bytes.count), mData: bytes.baseAddress))
            return CMSampleBufferCopyPCMDataIntoAudioBufferList(sampleBuffer, at: 0, frameCount: Int32(count), into: &list)
        }
        guard status == noErr else { emit(.failed(.captureFailed)); return }
        emit(.frame(AudioFrame(samples: values, sampleRate: format.mSampleRate, capturedAt: ProcessInfo.processInfo.systemUptime)))
    }
}
