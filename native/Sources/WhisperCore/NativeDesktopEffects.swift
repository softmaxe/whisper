import AppKit
import Darwin
import ServiceManagement

/// Production-only effects. Constructed explicitly by the executable, never by test defaults.
@MainActor public final class NativeDesktopEffects: DesktopEffects {
    private var sound: NSSound?
    public init() {}

    public func playCue(_ cue: DictationCue) {
        sound?.stop()
        sound = NSSound(data: Self.cueData(cue))
        sound?.play()
    }

    public func loginItemStatus() -> LoginItemStatus {
        switch SMAppService.mainApp.status {
        case .enabled: .enabled
        case .requiresApproval: .requiresApproval
        case .notRegistered: .disabled
        case .notFound: .unavailable
        @unknown default: .unavailable
        }
    }

    public func setLaunchAtLogin(_ enabled: Bool) async throws -> LoginItemStatus {
        if enabled {
            if !loginItemStatus().isRegistered { try SMAppService.mainApp.register() }
        } else if loginItemStatus().isRegistered {
            try await SMAppService.mainApp.unregister()
        }
        return loginItemStatus()
    }

    public func openLoginItemsSettings() { SMAppService.openSystemSettingsLoginItems() }

    public func pauseMedia() async -> MediaPauseOwnership? {
        if let response = await runAdapter(["get", "--no-artwork"]), response.status == 0 {
            if response.output.trimmingCharacters(in: .whitespacesAndNewlines) == "null" { return nil }
            if let data = response.output.data(using: .utf8),
               let state = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let playing = state["playing"] as? Bool {
                if !playing { return nil }
                if await runAdapter(["send", "1"])?.status == 0 { return .adapter }
            }
        }
        // Preserve the existing media-key fallback when the state-aware adapter is unavailable.
        return sendMediaKey() ? .mediaKey : nil
    }

    public func resumeMedia(_ ownership: MediaPauseOwnership) async {
        if ownership == .adapter, await runAdapter(["send", "0"])?.status == 0 { return }
        _ = sendMediaKey()
    }

    private func runAdapter(_ arguments: [String]) async -> DesktopProcessResult? {
        guard let resources = Bundle.main.resourceURL else { return nil }
        let bin = resources.appendingPathComponent("bin")
        let script = bin.appendingPathComponent("mediaremote-adapter.pl")
        let framework = bin.appendingPathComponent("MediaRemoteAdapter.framework")
        guard FileManager.default.fileExists(atPath: script.path),
              FileManager.default.fileExists(atPath: framework.path) else { return nil }
        return await Task.detached {
            DesktopProcessResult.run(executable: "/usr/bin/perl", arguments: [script.path, framework.path] + arguments)
        }.value
    }

    // Adapted from resources/macos-media-remote.swift; never synthesize ordinary key codes.
    private func sendMediaKey() -> Bool {
        for down in [true, false] {
            let state: Int32 = down ? 0xA : 0xB
            guard let event = NSEvent.otherEvent(with: .systemDefined, location: .zero,
                modifierFlags: NSEvent.ModifierFlags(rawValue: down ? 0xA00 : 0xB00),
                timestamp: 0, windowNumber: 0, context: nil, subtype: 8,
                data1: Int((16 << 16) | (state << 8)), data2: -1)?.cgEvent else { return false }
            // The global shortcut listener must not interpret our own media fallback as user input.
            event.setIntegerValueField(.eventSourceUserData, value: whisperPasteEventTag)
            event.post(tap: .cghidEventTap)
        }
        return true
    }

    // Match the existing two sine notes, attack, exponential release, gain and gap.
    private static func cueData(_ cue: DictationCue) -> Data {
        let rate = 44_100.0
        let frequencies = cue == .ready ? [523.25, 659.25] : [587.33, 440.0]
        let duration = 0.09, gap = 0.025, attack = 0.015
        let count = Int((duration * 2 + gap + 0.02) * rate)
        var samples = [Int16](repeating: 0, count: count)
        for (note, frequency) in frequencies.enumerated() {
            let offset = Int(Double(note) * (duration + gap) * rate)
            for sample in 0..<Int(duration * rate) {
                let time = Double(sample) / rate
                let gain = time < attack ? 0.0001 + (0.2 - 0.0001) * time / attack
                    : 0.2 * pow(0.0001 / 0.2, (time - attack) / (duration - attack))
                samples[offset + sample] = Int16(sin(2 * .pi * frequency * time) * gain * 32767)
            }
        }
        var data = Data()
        func ascii(_ value: String) { data.append(contentsOf: value.utf8) }
        func integer<T: FixedWidthInteger>(_ value: T) {
            var value = value.littleEndian
            withUnsafeBytes(of: &value) { data.append(contentsOf: $0) }
        }
        ascii("RIFF"); integer(UInt32(36 + count * 2)); ascii("WAVEfmt ")
        integer(UInt32(16)); integer(UInt16(1)); integer(UInt16(1)); integer(UInt32(rate))
        integer(UInt32(rate * 2)); integer(UInt16(2)); integer(UInt16(16))
        ascii("data"); integer(UInt32(count * 2))
        for sample in samples { integer(sample) }
        return data
    }
}

private struct DesktopProcessResult: Sendable {
    let status: Int32
    let output: String
    static func run(executable: String, arguments: [String]) -> Self? {
        let process = Process()
        let pipe = Pipe()
        let collector = BoundedProcessOutput()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = arguments
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        let exited = DispatchSemaphore(value: 0)
        let outputClosed = DispatchSemaphore(value: 0)
        process.terminationHandler = { _ in exited.signal() }
        pipe.fileHandleForReading.readabilityHandler = { handle in
            let data = (try? handle.read(upToCount: 65_536)) ?? Data()
            if data.isEmpty { handle.readabilityHandler = nil; outputClosed.signal(); return }
            if !collector.append(data), process.isRunning { process.terminate() }
        }
        do { try process.run() } catch { pipe.fileHandleForReading.readabilityHandler = nil; return nil }
        let timedOut = exited.wait(timeout: .now() + 3) == .timedOut
        if timedOut {
            if process.isRunning { process.terminate() }
            if exited.wait(timeout: .now() + 0.1) == .timedOut, process.isRunning {
                kill(process.processIdentifier, SIGKILL)
            }
        }
        // An inherited pipe must not hold the serial media queue or application shutdown forever.
        _ = outputClosed.wait(timeout: .now() + 0.1)
        pipe.fileHandleForReading.readabilityHandler = nil
        try? pipe.fileHandleForReading.close()
        guard !timedOut, let output = collector.text else { return nil }
        return Self(status: process.terminationStatus, output: output)
    }
}

private final class BoundedProcessOutput: @unchecked Sendable {
    private let lock = NSLock()
    private var data = Data()
    private var overflowed = false
    func append(_ value: Data) -> Bool {
        lock.withLock {
            if overflowed || data.count + value.count > 65_536 { overflowed = true; return false }
            data.append(value)
            return true
        }
    }
    var text: String? { lock.withLock { overflowed ? nil : String(decoding: data, as: UTF8.self) } }
}
