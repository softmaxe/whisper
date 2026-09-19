import AppKit
import CoreText
import SwiftUI
import WhisperCore

@main
struct WhisperMacApp: App {
    @State private var application: WhisperApplication?

    init() {
        for name in ["JetBrainsMono-Regular", "JetBrainsMono-SemiBold"] {
            if let url = Bundle.module.url(forResource: name, withExtension: "ttf", subdirectory: "Resources/Fonts") {
                CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
            }
        }
        do {
            let arguments = ProcessInfo.processInfo.arguments
            let profile: NativeProfile
            if let index = arguments.firstIndex(of: "--profile"), arguments.indices.contains(index + 1) {
                profile = NativeProfile(directory: URL(fileURLWithPath: arguments[index + 1], isDirectory: true))
            } else {
                profile = try NativeProfile.applicationDefault()
            }
            _application = State(initialValue: WhisperApplication(profile: profile))
        } catch {
            _application = State(initialValue: nil)
        }
    }

    var body: some Scene {
        Window("Whisper", id: "main") {
            if let application {
                SettingsRootView(application: application)
            } else {
                ContentUnavailableView(
                    "Whisper", systemImage: "exclamationmark.triangle",
                    description: Text("The native profile could not be opened. / 无法打开原生配置。")
                )
                .frame(minWidth: 600, minHeight: 400)
            }
        }
        .defaultSize(width: 960, height: 660)
        .windowStyle(.hiddenTitleBar)
        .commands {
            CommandGroup(replacing: .appSettings) {
                Button(application?.state.settings.language.text("Settings…", "设置…") ?? "Settings…") {
                    NSApplication.shared.activate()
                    NSApplication.shared.windows.first?.makeKeyAndOrderFront(nil)
                }
                .keyboardShortcut(",")
            }
        }
    }
}
