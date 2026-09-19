import SwiftUI
import WhisperCore

struct DesktopSettingsView: View {
    let application: WhisperApplication
    private var language: AppLanguage { application.state.settings.language }

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            section(language.text("Appearance", "外观")) {
                Picker(language.text("Theme", "主题"), selection: preference(\.theme)) {
                    Text(language.text("Light", "浅色")).tag(AppTheme.light)
                    Text(language.text("Dark", "深色")).tag(AppTheme.dark)
                    Text(language.text("System", "跟随系统")).tag(AppTheme.system)
                }
                .pickerStyle(.segmented).accessibilityIdentifier("desktop-theme")
                Toggle(language.text("Show menu bar icon", "显示菜单栏图标"), isOn: preference(\.showMenuBarIcon))
            }
            section(language.text("Sound Effects", "音效")) {
                Toggle(language.text("Dictation sounds", "听写提示音"), isOn: preference(\.audioCuesEnabled))
                Text(language.text("Play a sound when the microphone is ready and when recording stops.", "麦克风就绪和录音停止时播放提示音。"))
                    .font(.caption).foregroundStyle(.secondary)
                Toggle(language.text("Pause media during dictation", "听写时暂停媒体播放"), isOn: preference(\.pauseMediaOnDictation))
                Text(language.text("Resume media that Whisper paused when recording ends.", "录音结束时恢复 Whisper 暂停的媒体。"))
                    .font(.caption).foregroundStyle(.secondary)
            }
            section(language.text("Floating Icon", "悬浮图标")) {
                Toggle(language.text("Show Dictation Panel", "显示听写面板"), isOn: preference(\.pillVisible))
                Toggle(language.text("Auto-hide", "自动隐藏"), isOn: preference(\.floatingIconAutoHide))
                Text(language.text("Hide the panel when not dictating. Recording, errors and copy recovery stay visible.", "非听写状态时隐藏面板。录音、错误和复制恢复仍会显示。"))
                    .font(.caption).foregroundStyle(.secondary)
                Picker(language.text("Start position", "初始位置"), selection: preference(\.panelStartPosition)) {
                    Text(language.text("Bottom right", "右下角")).tag(PillPlacement.bottomRight)
                    Text(language.text("Center", "居中")).tag(PillPlacement.center)
                    Text(language.text("Bottom left", "左下角")).tag(PillPlacement.bottomLeft)
                }
            }
            section(language.text("Startup", "启动")) {
                Toggle(language.text("Launch at login", "登录时启动"), isOn: Binding(
                    get: { application.state.desktop.loginItemStatus.isRegistered },
                    set: { application.send(.setLaunchAtLogin($0)) }
                ))
                .disabled(application.state.desktop.loginItemChangePending)
                .accessibilityIdentifier("launch-at-login")
                if application.state.desktop.loginItemStatus == .requiresApproval {
                    Text(language.text("Enable Whisper in System Settings → General → Login Items.", "请在系统设置 → 通用 → 登录项中启用 Whisper。"))
                        .font(.caption).foregroundStyle(.secondary)
                    Button(language.text("Open Login Items", "打开登录项")) { application.send(.openLoginItemsSettings) }
                }
                if application.state.desktop.loginItemChangeFailed || application.state.desktop.loginItemStatus == .unavailable {
                    Text(language.text("Login item settings are unavailable. Try again from the packaged application.", "登录项设置暂不可用。请从打包后的应用重试。"))
                        .font(.caption).foregroundStyle(.red)
                }
                Toggle(language.text("Start minimized", "启动时最小化"), isOn: preference(\.startMinimized))
                Text(language.text("Keep Whisper in the background when it starts.", "Whisper 启动时保持在后台。"))
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private func preference<Value>(_ path: WritableKeyPath<DesktopPreferences, Value>) -> Binding<Value> {
        Binding(get: { application.state.settings.desktop[keyPath: path] }, set: { value in
            var preferences = application.state.settings.desktop
            preferences[keyPath: path] = value
            application.send(.saveDesktopPreferences(preferences))
        })
    }

    private func section<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title).font(.headline)
            VStack(alignment: .leading, spacing: 12, content: content)
                .frame(maxWidth: .infinity, alignment: .leading).padding(16)
                .background(.primary.opacity(0.025)).clipShape(RoundedRectangle(cornerRadius: 8))
                .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.primary.opacity(0.12)))
        }
    }
}
