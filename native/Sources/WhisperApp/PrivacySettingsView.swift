import SwiftUI
import WhisperCore

struct PrivacySettingsView: View {
    let application: WhisperApplication
    private var language: AppLanguage { application.state.settings.language }
    private var privacy: PrivacyState { application.state.privacy }
    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            HistoryPrivacyView(application: application)
            SettingsPanel(title: language.text("Permissions", "权限")) {
                ForEach(PrivacyPermission.allCases, id: \.self) { permission in
                    let status = privacy.permissions[permission]
                    HStack(alignment: .top, spacing: 12) {
                        Image(systemName: permission == .microphone ? "mic" : "accessibility")
                            .frame(width: 20).padding(.top, 2)
                        VStack(alignment: .leading, spacing: 6) {
                            Text(permission == .microphone ? language.text("Microphone", "麦克风") : language.text("Accessibility", "辅助功能")).fontWeight(.medium)
                            Text(permission == .microphone
                                ? language.text("Allow microphone access to record dictation.", "允许麦克风访问，以录制听写。")
                                : language.text("Allow Accessibility to use global shortcuts and paste into other apps.", "允许辅助功能访问，以使用全局快捷键并粘贴到其他应用。"))
                                .font(.caption).foregroundStyle(.secondary)
                            Text(statusTitle(status)).font(.caption).foregroundStyle(status == .granted ? Color.secondary : Color.orange)
                        }
                        Spacer()
                        if status != .granted {
                            Button(language.text("Grant access", "授予访问权限")) { application.send(.requestPrivacyPermission(permission)) }
                                .disabled(privacy.requesting != nil || status == .restricted || status == .unavailable)
                                .accessibilityIdentifier("request-" + permission.rawValue)
                        } else { Image(systemName: "checkmark.circle").foregroundStyle(.green).accessibilityLabel(statusTitle(status)) }
                    }
                    if permission == .microphone { Divider() }
                }
                Divider()
                Button(language.text("Open Accessibility settings", "打开辅助功能设置")) { application.send(.openPrivacySettings(.accessibility)) }
                    .font(.caption)
                Text(language.text("If automatic paste is unavailable, check Whisper's permission in System Settings.", "如果自动粘贴不可用，请在系统设置中检查 Whisper 的权限。"))
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .onAppear { application.send(.refreshPrivacy) }
        .onChange(of: application.state.history.pendingChanges) { if application.state.history.pendingChanges == 0 { application.send(.refreshPrivacy) } }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in application.send(.refreshPrivacy) }
    }
    private func statusTitle(_ status: PrivacyAuthorization) -> String {
        switch status {
        case .granted: language.text("Access granted", "已授予访问权限")
        case .notDetermined: language.text("Not requested", "尚未请求")
        case .denied: language.text("Access not granted", "未授予访问权限")
        case .restricted: language.text("Restricted by system policy", "受系统策略限制")
        case .unavailable: language.text("Status unavailable", "状态不可用")
        }
    }
}
