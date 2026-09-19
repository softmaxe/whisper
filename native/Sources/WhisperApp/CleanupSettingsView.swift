import SwiftUI
import WhisperCore

struct CleanupSettingsView: View {
    let application: WhisperApplication
    @Bindable var secrets: SettingsSecretDrafts

    @State private var sample = "um can you send me the report by friday"
    @State private var promptTab = 0
    private var language: AppLanguage { application.state.settings.language }
    private var draft: CleanupConfiguration { application.state.settingsDraft?.cleanup ?? application.state.settings.cleanup }
    private var prompt: String { application.state.settingsDraft?.cleanupPrompt ?? CleanupPrompts.defaultText(in: language) }
    private var test: CleanupTestState { application.state.cleanupTest }

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text(language.text("Text cleanup", "文本整理")).font(.headline)
            Text(language.text("Clean up dictated text with your own server. If cleanup fails, Whisper keeps the original transcript.", "使用自有服务器整理听写文本。整理失败时，Whisper 会保留原始转录。"))
                .font(.caption).foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 12) {
                Toggle(language.text("Enable text cleanup", "启用文本整理"), isOn: draftValue(\.enabled))
                    .accessibilityIdentifier("cleanup-enabled")
                field(language.text("Server URL", "服务器地址")) { TextField("http://localhost:8080", text: draftValue(\.serverURL)).accessibilityIdentifier("cleanup-server-url") }
                field(language.text("Model", "模型")) { TextField("default", text: draftValue(\.model)).accessibilityIdentifier("cleanup-model") }
                field(language.text("API Key (Optional)", "API Key（可选）")) {
                    SecureField(application.state.cleanupCredentialConfigured ? language.text("Saved in Keychain", "已保存到钥匙串") : language.text("Enter API key", "输入 API Key"), text: $secrets.cleanupKey)
                        .accessibilityIdentifier("cleanup-api-key")
                }
                Text(language.text("Leave blank to keep the saved key. Uses /v1/chat/completions; public servers require HTTPS.", "留空可保留已保存的密钥。使用 /v1/chat/completions，公网服务器必须使用 HTTPS。"))
                    .font(.caption).foregroundStyle(.secondary)
                if application.state.cleanupCredentialConfigured {
                    Toggle(language.text("Remove saved API key", "移除已保存的 API Key"), isOn: $secrets.removeCleanupKey)
                }
                Toggle(language.text("Disable thinking", "关闭思考"), isOn: draftValue(\.disableThinking))
                HStack {
                    Text(language.text("Temperature", "温度"))
                    TextField("0", value: draftValue(\.temperature), format: .number).frame(width: 90)
                    Text(language.text("Token limit", "Token 上限"))
                    TextField("4096", value: draftValue(\.maxTokens), format: .number).frame(width: 110)
                }
            }.textFieldStyle(.roundedBorder).padding(16)
                .background(.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 8))
                .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.primary.opacity(0.12)))
            HStack {
                if let error = application.state.configurationError {
                    Text(error.message(in: language)).foregroundStyle(.red).font(.caption)
                } else if application.state.settingsSaved && draft == application.state.settings.cleanup {
                    Text(language.text("Settings saved", "设置已保存")).foregroundStyle(.secondary).font(.caption)
                }
                Spacer()
                Button(language.text("Save", "保存")) { save() }.buttonStyle(.borderedProminent)
                    .accessibilityIdentifier("save-cleanup")
            }
            Divider()
            Text(language.text("Prompt", "提示词")).font(.headline)
            Picker(language.text("Prompt editor", "提示词编辑器"), selection: $promptTab) {
                Text(language.text("Preview", "预览")).tag(0)
                Text(language.text("Customize", "自定义")).tag(1)
                Text(language.text("Test", "测试")).tag(2)
            }.pickerStyle(.segmented)
            if promptTab == 0 {
                Text(prompt).textSelection(.enabled).font(.caption).frame(maxWidth: .infinity, alignment: .leading)
            } else if promptTab == 1 {
                TextEditor(text: promptValue).frame(minHeight: 230).accessibilityIdentifier("cleanup-prompt-editor")
                HStack {
                    Button(language.text("Reset to default", "恢复默认")) {
                        application.send(.resetCleanupPrompt)
                        if application.state.settingsSaved {
                            var changed = draft; changed.customPrompt = nil; application.send(.editCleanupDraft(changed))
                            application.send(.editCleanupPromptDraft(CleanupPrompts.defaultText(in: language)))
                        }
                    }
                    Spacer()
                    Button(language.text("Save prompt", "保存提示词")) { savePrompt() }
                }
            } else {
                Text(language.text("Tests use the saved server and your current prompt draft.", "测试使用已保存的服务器和当前提示词草稿。"))
                    .font(.caption).foregroundStyle(.secondary)
                Text(application.state.settings.cleanup.serverURL + " · " + (application.state.settings.cleanup.model.isEmpty ? "default" : application.state.settings.cleanup.model))
                    .font(.caption).textSelection(.enabled)
                TextEditor(text: $sample).frame(minHeight: 95).accessibilityIdentifier("cleanup-test-input")
                HStack {
                    Button(language.text("Run test", "运行测试")) { application.send(.testCleanupPrompt(text: sample, prompt: prompt)) }
                        .disabled(test.isRunning || sample.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        .accessibilityIdentifier("test-cleanup-prompt")
                    if test.isRunning {
                        ProgressView().controlSize(.small)
                        Button(language.text("Cancel", "取消")) { application.send(.cancelCleanupPromptTest) }
                    }
                }
                if let failure = test.failure { Text(failure.message(in: language)).foregroundStyle(.red) }
                if !test.text.isEmpty { Text(test.text).textSelection(.enabled).accessibilityIdentifier("cleanup-test-result") }
            }
        }

    }
    private func field<Content: View>(_ label: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 6) { Text(label).fontWeight(.semibold); content().accessibilityLabel(label) }
    }
    private func draftValue<Value>(_ key: WritableKeyPath<CleanupConfiguration, Value>) -> Binding<Value> {
        Binding(get: { draft[keyPath: key] }, set: { value in
            var changed = draft; changed[keyPath: key] = value
            application.send(.editCleanupDraft(changed))
        })
    }
    private var promptValue: Binding<String> {
        Binding(get: { prompt }, set: { application.send(.editCleanupPromptDraft($0)) })
    }
    private func save() {
        application.send(.saveCleanupDraft(secrets.removeCleanupKey ? .remove : secrets.cleanupKey.isEmpty ? .unchanged : .replace(secrets.cleanupKey)))
        if application.state.settingsSaved { secrets.cleanupKey = ""; secrets.removeCleanupKey = false }
    }
    private func savePrompt() { application.send(.saveCleanupPromptDraft) }
}
