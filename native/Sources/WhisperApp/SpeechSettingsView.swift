import SwiftUI
import WhisperCore

struct SpeechSettingsView: View {
    let application: WhisperApplication
    @Bindable var secrets: SettingsSecretDrafts
    private var language: AppLanguage { application.state.settings.language }
    private var draft: ASRConfiguration { application.state.settingsDraft?.asr ?? application.state.settings.asr }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(language.text("Dictation and file transcription share these settings.", "听写和文件转录共用这些设置。"))
                .font(.caption).foregroundStyle(.secondary)
            SettingsPanel(title: language.text("Self-Hosted", "自托管")) {
                field(language.text("Server URL", "服务器地址")) {
                    TextField("http://localhost:8178/v1", text: value(\.serverURL)).accessibilityIdentifier("asr-server-url")
                }
                field(language.text("Model", "模型")) {
                    TextField("Whisper-Large-v3-Turbo", text: value(\.model)).accessibilityIdentifier("asr-model")
                }
                field(language.text("API Key (Optional)", "API Key（可选）")) {
                    SecureField(application.state.credentialConfigured ? language.text("Saved in Keychain", "已保存到钥匙串") : language.text("Enter API key", "输入 API Key"), text: $secrets.asrKey)
                        .accessibilityIdentifier("asr-api-key")
                }
                Text(language.text("Leave blank to keep the saved key.", "留空可保留已保存的密钥。"))
                    .font(.caption).foregroundStyle(.secondary)
                if application.state.credentialConfigured {
                    Toggle(language.text("Remove saved API key", "移除已保存的 API Key"), isOn: $secrets.removeASRKey)
                }
            }
            Text(language.text("Public servers require HTTPS. Local and private-network hosts can use HTTP. Include /v1 if your server requires it.", "公网服务器必须使用 HTTPS。本机和私有网络可以使用 HTTP。如果服务器需要，请在地址中包含 /v1。"))
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            HStack {
                SettingsSaveFeedback(application: application, showSaved: draft == application.state.settings.asr)
                Spacer()
                Button(language.text("Save", "保存")) {
                    application.send(.saveASRDraft(secrets.removeASRKey ? .remove : secrets.asrKey.isEmpty ? .unchanged : .replace(secrets.asrKey)))
                    if application.state.settingsSaved { secrets.asrKey = ""; secrets.removeASRKey = false }
                }.buttonStyle(.borderedProminent).keyboardShortcut("s").accessibilityIdentifier("save-asr")
            }
        }.textFieldStyle(.roundedBorder)
    }
    private func value(_ key: WritableKeyPath<ASRConfiguration, String>) -> Binding<String> {
        Binding(get: { draft[keyPath: key] }, set: { text in
            var changed = draft; changed[keyPath: key] = text
            application.send(.editASRDraft(changed))
        })
    }
    private func field<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 6) { Text(title).fontWeight(.medium); content().accessibilityLabel(title) }
    }
}
