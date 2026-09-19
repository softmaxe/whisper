import SwiftUI
import WhisperCore

struct SettingsRootView: View {
    let application: WhisperApplication
    @State private var pill: RecordingPillController?
    @State private var section = SettingsSection.home
    @State private var serverURL = ""
    @State private var model = ""
    @State private var apiKey = ""
    @State private var removeCredential = false
    @Environment(\.colorScheme) private var colorScheme

    private var language: AppLanguage { application.state.settings.language }
    private var canvas: Color {
        colorScheme == .dark ? Color(red: 0.094, green: 0.102, blue: 0.106) : .white
    }
    private var sidebar: Color {
        colorScheme == .dark ? Color(red: 0.065, green: 0.073, blue: 0.078) : Color(white: 0.97)
    }
    private var accent: Color { Color(red: 0.27, green: 0.47, blue: 0.91) }

    var body: some View {
        HStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 5) {
                Text("Whisper").font(.custom("JetBrainsMono-SemiBold", size: 17))
                    .padding(.horizontal, 14).padding(.top, 28).padding(.bottom, 22)
                Text(language.text("Settings", "设置"))
                    .font(.caption).foregroundStyle(.secondary).padding(.horizontal, 14).padding(.bottom, 5)
                ForEach(SettingsSection.allCases) { item in
                    Button {
                        section = item
                    } label: {
                        Label(item.title(language), systemImage: item.icon)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.horizontal, 12).padding(.vertical, 9)
                            .background(section == item ? accent.opacity(0.13) : Color.clear)
                            .clipShape(RoundedRectangle(cornerRadius: 6))
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(section == item ? accent : .primary)
                    .accessibilityAddTraits(section == item ? .isSelected : [])
                }
                Spacer()
            }
            .padding(.horizontal, 10)
            .frame(width: 178)
            .background(sidebar)

            VStack(spacing: 0) {
                HStack(spacing: 10) {
                    Image(systemName: section.icon)
                    Text(section == .home ? "Whisper" : language.text("Settings", "设置")).fontWeight(.semibold)
                    Text("/").foregroundStyle(.tertiary)
                    Text(section.title(language))
                    Spacer()
                }
                .padding(.horizontal, 26).frame(height: 58)
                Divider()
                ScrollView {
                    VStack(alignment: .leading, spacing: 24) {
                        switch section {
                        case .home: DictationHomeView(application: application)
                        case .general: generalSettings
                        case .speechToText: speechSettings
                        case .privacyAndData: HistoryPrivacyView(application: application)
                        }
                    }
                    .frame(maxWidth: 720, alignment: .leading)
                    .padding(28)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .background(canvas)
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(.primary.opacity(0.12)))
            .padding([.trailing, .top, .bottom], 8)
        }
        .background(sidebar)
        .font(.custom("JetBrainsMono-Regular", size: 13))
        .tint(accent)
        .frame(minWidth: 780, minHeight: 530)
        .onAppear {
            restoreDraft()
            if pill == nil { pill = RecordingPillController(application: application) }
            pill?.update()
        }
        .onChange(of: application.state.dictation.phase) { pill?.update() }
    }

    private var generalSettings: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(language.text("Language", "语言")).font(.headline)
            HStack {
                VStack(alignment: .leading, spacing: 5) {
                    Text(language.text("Interface language", "界面语言"))
                    Text(language.text("Choose the language used in Whisper.", "选择 Whisper 的界面语言。"))
                        .font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Picker(language.text("Interface language", "界面语言"), selection: Binding(
                    get: { language },
                    set: { application.send(.setLanguage($0)) }
                )) {
                    ForEach(AppLanguage.allCases, id: \.self) { item in Text(item.displayName).tag(item) }
                }
                .labelsHidden().frame(width: 150)
            }
            .padding(16).background(.primary.opacity(0.025))
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.primary.opacity(0.12)))
            feedback
        }
    }

    private var speechSettings: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 5) {
                Text(language.text("Speech-to-Text", "语音转文字")).font(.headline)
                Text(language.text("Dictation and file transcription share these settings.", "听写和文件转录共用这些设置。"))
                    .font(.caption).foregroundStyle(.secondary)
            }
            VStack(alignment: .leading, spacing: 16) {
                field(language.text("Server URL", "服务器地址")) {
                    TextField("http://localhost:8178/v1", text: $serverURL)
                        .accessibilityIdentifier("asr-server-url")
                }
                field(language.text("Model", "模型")) {
                    TextField("Whisper-Large-v3-Turbo", text: $model)
                        .accessibilityIdentifier("asr-model")
                }
                field(language.text("API Key (Optional)", "API Key（可选）")) {
                    SecureField(
                        application.state.credentialConfigured
                            ? language.text("Saved in Keychain", "已保存到钥匙串")
                            : language.text("Enter API key", "输入 API Key"),
                        text: $apiKey
                    )
                    .accessibilityIdentifier("asr-api-key")
                }
                Text(language.text(
                    "Sent as a Bearer token. Leave blank to keep the saved key.",
                    "以 Bearer token 发送。留空可保留已保存的密钥。"
                ))
                .font(.caption).foregroundStyle(.secondary)
                if application.state.credentialConfigured {
                    Toggle(language.text("Remove saved API key", "移除已保存的 API Key"), isOn: $removeCredential)
                        .toggleStyle(.checkbox)
                }
            }
            .textFieldStyle(.roundedBorder)
            .padding(16).background(.primary.opacity(0.025))
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.primary.opacity(0.12)))

            Text(language.text(
                "Public servers require HTTPS. Local and private-network hosts can use HTTP. Include /v1 if your server requires it.",
                "公网服务器必须使用 HTTPS。本机和私有网络可以使用 HTTP。如果服务器需要，请在地址中包含 /v1。"
            ))
            .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            HStack {
                feedback
                Spacer()
                Button(language.text("Save", "保存"), action: save)
                    .buttonStyle(.borderedProminent).keyboardShortcut("s")
                    .accessibilityIdentifier("save-asr")
            }
        }
    }

    @ViewBuilder private var feedback: some View {
        if let error = application.state.configurationError {
            Label(error.message(in: language), systemImage: "exclamationmark.triangle")
                .foregroundStyle(.red).font(.caption).fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("settings-error")
        } else if application.state.settingsSaved {
            Label(language.text("Settings saved", "设置已保存"), systemImage: "checkmark.circle")
                .foregroundStyle(.secondary).font(.caption)
                .accessibilityIdentifier("settings-saved")
        }
    }

    private func field<Content: View>(_ label: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label).font(.custom("JetBrainsMono-SemiBold", size: 12))
            content().accessibilityLabel(label)
        }
    }

    private func save() {
        let change: CredentialChange = removeCredential ? .remove : apiKey.isEmpty ? .unchanged : .replace(apiKey)
        application.send(.saveASR(.init(serverURL: serverURL, model: model), credential: change))
        if application.state.settingsSaved {
            restoreDraft()
        }
    }

    private func restoreDraft() {
        serverURL = application.state.settings.asr.serverURL
        model = application.state.settings.asr.model
        apiKey = ""
        removeCredential = false
    }
}

private enum SettingsSection: String, CaseIterable, Identifiable {
    case home, general, speechToText, privacyAndData
    var id: Self { self }
    var icon: String { self == .home ? "house" : self == .general ? "slider.horizontal.3" : self == .privacyAndData ? "lock.shield" : "waveform" }
    func title(_ language: AppLanguage) -> String {
        self == .home ? language.text("Home", "首页") : self == .general ? language.text("General", "通用") : self == .privacyAndData ? language.text("Privacy & Data", "隐私与数据") : language.text("Speech-to-Text", "语音转文字")
    }
}
