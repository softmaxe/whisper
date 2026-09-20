import SwiftUI
import Observation
import WhisperCore

@MainActor @Observable final class SettingsSecretDrafts {
    var asrKey = ""
    var removeASRKey = false
    var cleanupKey = ""
    var removeCleanupKey = false
}

/// The main app and Settings share workflow state, while secure input drafts remain view-owned.
struct SettingsRootView: View {
    let application: WhisperApplication
    @State private var secrets = SettingsSecretDrafts()
    @Environment(\.colorScheme) private var colorScheme
    private var language: AppLanguage { application.state.settings.language }
    private var navigation: NavigationState { application.state.navigation }
    private var palette: WhisperPalette { WhisperPalette(scheme: colorScheme) }

    var body: some View {
        GeometryReader { geometry in
            shell(availableSize: geometry.size)
        }
        .frame(minWidth: 780, minHeight: 530)
        .ignoresSafeArea(.container, edges: .top)
    }

    private func shell(availableSize: CGSize) -> some View {
        HStack(spacing: 0) {
            if !navigation.sidebarCollapsed { sidebar }
            VStack(spacing: 0) {
                topBar
                Divider()
                ScrollView {
                    Group {
                        switch navigation.page {
                        case .home: HomePage(application: application)
                        case .insights: InsightsView(application: application)
                        case .upload: UploadView(application: application,
                            onOpenSettings: { application.send(.openSettings(.speechToText)) },
                            onOpenHistory: { application.send(.navigate(.home)) })
                        case .dictionary: DictionaryPage(application: application)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading).padding(24)
                }
            }
            .background(palette.canvas)
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(palette.border))
            .padding([.top, .trailing, .bottom], 8)
            .padding(.leading, navigation.sidebarCollapsed ? 8 : 0)
        }
        .background(palette.window)
        .font(.custom("JetBrainsMono-Regular", size: 13))
        .tint(palette.accent)
        .accessibilityIdentifier("main-shell")
        .sheet(item: Binding(get: {
            navigation.settingsPresented ? ShellModal.settings : navigation.searchPresented ? .search : nil
        }, set: { item in
            if item == nil {
                if navigation.settingsPresented { application.send(.closeSettings) }
                else { application.send(.closeHistorySearch) }
            }
        })) { modal in
            switch modal {
            case .settings: SettingsModalView(application: application, secrets: secrets, availableSize: availableSize)
            case .search: HistorySearchView(application: application)
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: .init("WhisperOpenSettings"))) { _ in application.send(.openSettings(.general)) }
        .onReceive(NotificationCenter.default.publisher(for: .init("WhisperOpenHistorySearch"))) { _ in application.send(.openHistorySearch) }
    }

    private var sidebar: some View {
        VStack(spacing: 2) {
            Color.clear.frame(height: 40).accessibilityHidden(true)
            ForEach(MainPage.allCases, id: \.self) { page in
                Button { application.send(.navigate(page)) } label: {
                    Label(page.title(in: language), systemImage: page.icon)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 10).frame(height: 32)
                        .foregroundStyle(page == navigation.page ? palette.accent : .primary)
                        .background(page == navigation.page ? palette.accent.opacity(0.1) : .clear, in: RoundedRectangle(cornerRadius: 6))
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain).accessibilityIdentifier("navigation-" + page.rawValue)
                .accessibilityAddTraits(page == navigation.page ? .isSelected : [])
            }
            Spacer()
            Button { application.send(.openSettings(.general)) } label: {
                Label(language.text("Settings", "设置"), systemImage: "gearshape")
                    .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 10).frame(height: 32)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain).accessibilityIdentifier("open-settings")
        }
        .padding(.horizontal, 8).padding(.bottom, 8).frame(width: 192)
    }

    private var topBar: some View {
        HStack(spacing: 16) {
            HStack(spacing: 10) {
                Button { application.send(.toggleSidebar) } label: { Image(systemName: "sidebar.left") }
                    .buttonStyle(.plain).frame(width: 28, height: 28)
                    .accessibilityLabel(language.text(navigation.sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar", navigation.sidebarCollapsed ? "展开侧边栏" : "收起侧边栏"))
                Text(navigation.page.title(in: language)).fontWeight(.medium)
                Spacer(minLength: 0)
            }.frame(maxWidth: .infinity)
            Button { application.send(.openHistorySearch) } label: {
                HStack(spacing: 10) {
                    Image(systemName: "magnifyingglass")
                    Text(language.text("Transcripts", "转录")).lineLimit(1)
                    Spacer()
                    Text("⌘ K").font(.system(size: 10)).padding(.horizontal, 7).padding(.vertical, 2)
                        .background(.primary.opacity(0.06), in: Capsule())
                }.foregroundStyle(.secondary).padding(.horizontal, 14).frame(height: 32)
                    .background(.primary.opacity(0.04), in: Capsule())
                    .overlay(Capsule().strokeBorder(palette.border))
            }
            .buttonStyle(.plain).frame(maxWidth: 340)
            .accessibilityIdentifier("open-history-search")
            Spacer(minLength: 0).frame(maxWidth: .infinity)
        }
        .padding(.horizontal, 14).padding(.leading, navigation.sidebarCollapsed ? 64 : 0).frame(height: 48)
    }
}

struct SettingsModalView: View {
    let application: WhisperApplication
    let secrets: SettingsSecretDrafts
    let availableSize: CGSize
    @Environment(\.colorScheme) private var colorScheme
    private var language: AppLanguage { application.state.settings.language }
    private var section: SettingsSection { application.state.navigation.settingsSection }
    private var width: CGFloat { min(896, availableSize.width * 0.9) }
    private var height: CGFloat { availableSize.height * 0.85 }
    private var compact: Bool { width < 800 }

    var body: some View {
        HStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 4) {
                if !compact {
                    Text(language.text("Settings", "设置")).font(.headline).padding(.horizontal, 12).padding(.vertical, 20)
                }
                ForEach(SettingsSection.allCases, id: \.self) { item in
                    if !compact && (item == .general || item == .speechToText || item == .privacy) {
                        Text(item == .general ? language.text("App", "应用") : item == .speechToText ? language.text("AI Models", "AI 模型") : language.text("System", "系统"))
                            .font(.system(size: 10)).foregroundStyle(.secondary).padding(.horizontal, 12).padding(.top, 14)
                    }
                    Button { application.send(.openSettings(item)) } label: {
                        HStack(spacing: 8) {
                            Image(systemName: item.icon).frame(width: 16)
                            if !compact { Text(item.title(in: language)) }
                        }
                            .frame(maxWidth: .infinity, alignment: compact ? .center : .leading)
                            .padding(.horizontal, compact ? 0 : 12).padding(.vertical, 10)
                            .background(item == section ? Color.accentColor.opacity(0.12) : .clear, in: RoundedRectangle(cornerRadius: 7))
                            .contentShape(Rectangle())
                    }.buttonStyle(.plain).accessibilityIdentifier("settings-" + item.rawValue)
                        .accessibilityLabel(item.title(in: language)).help(item.title(in: language))
                        .accessibilityAddTraits(item == section ? .isSelected : [])
                }
                Spacer()
            }
            .padding(.horizontal, compact ? 6 : 10).padding(.top, compact ? 16 : 0).frame(width: compact ? 48 : 190)
            .background(WhisperPalette(scheme: colorScheme).window)
            Divider()
            VStack(spacing: 0) {
                HStack {
                    Text(section.title(in: language)).fontWeight(.semibold)
                    Spacer()
                    Button { application.send(.closeSettings) } label: { Image(systemName: "xmark") }
                        .buttonStyle(.plain).frame(width: 28, height: 28)
                        .accessibilityLabel(language.text("Close Settings", "关闭设置"))
                }.padding(16)
                Divider()
                ScrollView {
                    Group {
                        switch section {
                        case .general: GeneralSettingsView(application: application)
                        case .hotkeys: ShortcutSettingsView(application: application) {
                            NotificationCenter.default.post(name: .init("WhisperRequestShortcutPermission"), object: nil)
                        }
                        case .speechToText: SpeechSettingsView(application: application, secrets: secrets)
                        case .textCleanup: CleanupSettingsView(application: application, secrets: secrets)
                        case .privacy: PrivacySettingsView(application: application)
                        }
                    }.frame(maxWidth: .infinity, alignment: .leading).padding(compact ? 16 : 24)
                }
            }.background(WhisperPalette(scheme: colorScheme).canvas)
        }
        .frame(width: width, height: height)
        .font(.custom("JetBrainsMono-Regular", size: 12))
        .tint(WhisperPalette(scheme: colorScheme).accent)
        .onExitCommand {
            if application.state.dictation.phase.isActive { application.send(.cancelDictation) }
            else if application.state.shortcutCapture.isActive { application.send(.endShortcutCapture) }
            else { application.send(.closeSettings) }
        }
        .onDisappear { application.send(.endShortcutCapture) }
        .accessibilityIdentifier("settings-modal")
    }
}

private enum ShellModal: String, Identifiable {
    case settings, search
    var id: String { rawValue }
}
