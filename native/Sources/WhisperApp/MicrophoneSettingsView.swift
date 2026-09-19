import AppKit
import SwiftUI
import WhisperCore

struct MicrophoneSettingsView: View {
    let application: WhisperApplication
    private var language: AppLanguage { application.state.settings.language }
    private var preference: MicrophonePreference { application.state.settings.microphone }
    private var snapshot: MicrophoneSnapshot { application.state.microphoneInputs }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text(language.text("Microphone", "麦克风")).font(.headline)
                Spacer()
                Button { application.send(.refreshMicrophones) } label: {
                    Image(systemName: "arrow.clockwise")
                }
                .buttonStyle(.plain)
                .help(language.text("Refresh microphones", "刷新麦克风"))
                .accessibilityLabel(language.text("Refresh microphones", "刷新麦克风"))
            }
            VStack(alignment: .leading, spacing: 12) {
                Picker(language.text("Input selection", "输入选择"), selection: Binding(
                    get: { preference.mode },
                    set: { mode in
                        var changed = preference
                        changed.mode = mode
                        application.send(.setMicrophone(changed))
                    }
                )) {
                    ForEach(MicrophoneMode.allCases, id: \.self) { mode in Text(mode.title(in: language)).tag(mode) }
                }
                .accessibilityIdentifier("microphone-mode")
                if preference.mode == .specific {
                    Picker(language.text("Microphone", "麦克风"), selection: Binding(
                        get: { preference.deviceID ?? "" },
                        set: { id in
                            guard let device = snapshot.devices.first(where: { $0.id == id }) else { return }
                            application.send(.setMicrophone(.init(mode: .specific, deviceID: id, deviceName: device.name)))
                        }
                    )) {
                        Text(language.text("Choose a microphone", "选择麦克风")).tag("")
                        if let id = preference.deviceID, !snapshot.devices.contains(where: { $0.id == id }) {
                            Text((preference.deviceName ?? language.text("Saved microphone", "已保存的麦克风")) + language.text(" (unavailable)", "（不可用）")).tag(id)
                        }
                        ForEach(snapshot.devices, id: \.id) { device in Text(device.name).tag(device.id) }
                    }
                    .accessibilityIdentifier("specific-microphone")
                }
                Text(description).font(.caption).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                if let failure = application.state.microphoneFailure {
                    Label(failure.message(in: language), systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.red).font(.caption)
                }
                Text(language.text("Input changes apply to the next recording.", "输入变更将在下一次录音时生效。"))
                    .font(.caption).foregroundStyle(.secondary)
            }
            .padding(16).background(.primary.opacity(0.025))
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.primary.opacity(0.12)))
        }
        .onAppear { application.send(.refreshMicrophones) }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            application.send(.refreshMicrophones)
        }
    }

    private var description: String {
        switch preference.mode {
        case .auto:
            language.text("Use the built-in microphone while the lid is open. With the lid closed, prefer iPhone, then another external input. If unavailable, use the system default.", "开盖时使用内置麦克风。合盖时优先使用 iPhone，其次使用其他外接输入。没有相应设备时使用系统默认输入。")
        case .system:
            language.text("Use the physical input selected in macOS when recording starts.", "录音开始时使用 macOS 选择的物理输入设备。")
        case .builtIn:
            language.text("Use only the Mac's built-in microphone.", "仅使用 Mac 的内置麦克风。")
        case .specific:
            language.text("Use only this saved device. A disconnected microphone will not be replaced by another input.", "仅使用此已保存设备。麦克风断开后不会替换为其他输入。")
        }
    }
}
