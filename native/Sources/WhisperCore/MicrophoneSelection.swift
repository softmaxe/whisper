import Foundation

public enum MicrophoneCategory: String, Equatable, Sendable {
    case builtIn, continuity, external
}

public enum MicrophoneMode: String, Codable, CaseIterable, Sendable {
    case auto, system, builtIn, specific
    public func title(in language: AppLanguage) -> String {
        switch self {
        case .auto: language.text("Auto", "自动")
        case .system: language.text("System default", "系统默认")
        case .builtIn: language.text("Built-in microphone", "内置麦克风")
        case .specific: language.text("Specific microphone", "指定麦克风")
        }
    }
}

public struct MicrophonePreference: Codable, Equatable, Sendable {
    public var mode: MicrophoneMode
    public var deviceID: String?
    public var deviceName: String?
    public init(mode: MicrophoneMode = .auto, deviceID: String? = nil, deviceName: String? = nil) {
        self.mode = mode
        self.deviceID = deviceID
        self.deviceName = deviceName
    }
}

public struct MicrophoneSnapshot: Equatable, Sendable {
    public let devices: [MicrophoneDevice]
    public let systemDefaultID: String?
    public let lidClosed: Bool?
    public init(devices: [MicrophoneDevice] = [], systemDefaultID: String? = nil, lidClosed: Bool? = nil) {
        self.devices = devices
        self.systemDefaultID = systemDefaultID
        self.lidClosed = lidClosed
    }
}

public extension MicrophoneProvider {
    /// Compatibility for a provider that exposes one physical input. Production enumerates native metadata.
    func inputSnapshot() throws -> MicrophoneSnapshot {
        let device = try resolveDevice()
        return MicrophoneSnapshot(devices: [device], systemDefaultID: device.id)
    }
}

enum MicrophoneSelection {
    // Adapted from this fork's microphoneSelection.js; use native categories, never display-label heuristics.
    static func resolve(_ preference: MicrophonePreference, snapshot: MicrophoneSnapshot) throws -> MicrophoneDevice {
        let devices = snapshot.devices.filter { !$0.id.isEmpty && !["default", "communications"].contains($0.id) }
        let selected: MicrophoneDevice?
        switch preference.mode {
        case .specific:
            selected = devices.first { $0.id == preference.deviceID }
        case .builtIn:
            selected = devices.first { $0.category == .builtIn }
        case .system:
            selected = devices.first { $0.id == snapshot.systemDefaultID }
        case .auto:
            let preferred = snapshot.lidClosed == true
                ? devices.first { $0.category == .continuity } ?? devices.first { $0.category == .external }
                : devices.first { $0.category == .builtIn }
            selected = preferred ?? devices.first { $0.id == snapshot.systemDefaultID }
        }
        guard let selected else { throw DictationFailure.inputUnavailable }
        return selected
    }
}

extension WhisperApplication {
    func setMicrophone(_ preference: MicrophonePreference) {
        var settings = state.settings
        settings.microphone = preference
        persist(settings)
    }

    func refreshMicrophones() {
        do {
            state.microphoneInputs = try microphones.inputSnapshot()
            state.microphoneFailure = nil
        } catch {
            state.microphoneInputs = MicrophoneSnapshot()
            state.microphoneFailure = error as? DictationFailure ?? .inputUnavailable
        }
    }

    func selectedMicrophone() throws -> MicrophoneDevice {
        let snapshot = try microphones.inputSnapshot()
        state.microphoneInputs = snapshot
        state.microphoneFailure = nil
        return try MicrophoneSelection.resolve(state.settings.microphone, snapshot: snapshot)
    }
}
