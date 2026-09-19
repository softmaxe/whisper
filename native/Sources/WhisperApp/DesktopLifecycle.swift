import AppKit
import Carbon
import CoreText
import Observation
import SwiftUI
import WhisperCore

@MainActor final class WhisperAppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private var application: WhisperApplication?
    private var mainWindow: NSWindow?
    private var pill: RecordingPillController?
    private var shortcuts: NativeShortcutMonitor?
    private var statusItem: NSStatusItem?
    private var previousPreferences: DesktopPreferences?
    private var previousLanguage: AppLanguage?
    private var previousPill: RecordingPillPresentation?
    private var previousWindowVisible: Bool?
    private var terminating = false
    private var syntheticPreview = false
    private var previousShortcuts: [String]?
    private var previousCaptureMode: Bool?
    private var previousDictationPhase: DictationPhase?

    func applicationDidFinishLaunching(_ notification: Notification) {
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
            } else { profile = try NativeProfile.applicationDefault() }
            syntheticPreview = arguments.contains("--synthetic-preview")
            if syntheticPreview, !arguments.contains("--profile") { throw ConfigurationError.incompatibleProfile }
            let previewLanguage: AppLanguage = arguments.contains("--preview-chinese") ? .simplifiedChinese : .english
            let application = try syntheticPreview
                ? SyntheticPreview.make(profile: profile, language: previewLanguage)
                : WhisperApplication(profile: profile, desktopEffects: NativeDesktopEffects(), privacySystem: NativePrivacySystem())
            if syntheticPreview {
                Task { try? await SyntheticPreview.seed(application, populated: arguments.contains("--preview-populated")) }
            }
            self.application = application
            let event = NSAppleEventManager.shared().currentAppleEvent
            let launchedAtLogin = event?.eventID == kAEOpenApplication
                && event?.paramDescriptor(forKeyword: keyAEPropData)?.enumCodeValue == keyAELaunchedAsLogInItem
            application.send(.bootstrapDesktop(launchedAtLogin: launchedAtLogin || arguments.contains("--hidden")))
            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1200, height: 800),
                styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView], backing: .buffered, defer: false)
            window.title = syntheticPreview ? "Whisper · Sample data" : "Whisper"
            window.identifier = NSUserInterfaceItemIdentifier("main")
            window.titleVisibility = .hidden
            window.titlebarAppearsTransparent = true
            window.isReleasedWhenClosed = false
            window.minSize = NSSize(width: 780, height: 530)
            window.contentView = NSHostingView(rootView: SettingsRootView(application: application))
            window.delegate = self
            window.center()
            mainWindow = window
            if !syntheticPreview {
                pill = RecordingPillController(application: application)
                shortcuts = NativeShortcutMonitor(application: application)
                shortcuts?.start()
            }
            synchronize()
            observe()
            NotificationCenter.default.addObserver(self, selector: #selector(requestShortcutPermission), name: .init("WhisperRequestShortcutPermission"), object: nil)
            NotificationCenter.default.addObserver(self, selector: #selector(screenChanged), name: NSApplication.didChangeScreenParametersNotification, object: nil)
        } catch {
            let alert = NSAlert()
            alert.messageText = "Whisper"
            alert.informativeText = "The native profile could not be opened. / 无法打开原生配置。"
            alert.runModal()
            NSApplication.shared.terminate(nil)
        }
    }

    private func observe() {
        guard let application else { return }
        withObservationTracking {
            _ = application.state.settings.desktop
            _ = application.state.settings.language
            _ = application.state.desktop
            _ = application.state.recordingPill
            _ = application.state.settings.shortcuts
            _ = application.state.shortcutCapture.isActive
            _ = application.state.dictation.phase
        } onChange: { [weak self] in
            Task { @MainActor in self?.synchronize(); self?.observe() }
        }
    }

    private func synchronize() {
        guard let application else { return }
        let bindings = application.state.settings.shortcuts
        let captureMode = application.state.shortcutCapture.isActive
        let phase = application.state.dictation.phase
        if bindings != previousShortcuts || captureMode != previousCaptureMode || phase != previousDictationPhase {
            previousShortcuts = bindings
            previousCaptureMode = captureMode
            previousDictationPhase = phase
            shortcuts?.updateConfiguration()
        }
        let preferences = application.state.settings.desktop
        let language = application.state.settings.language
        if previousPreferences != preferences || previousLanguage != language {
            NSApplication.shared.appearance = switch preferences.theme {
            case .system: nil
            case .light: NSAppearance(named: .aqua)
            case .dark: NSAppearance(named: .darkAqua)
            }
            updateMenus(language: language, preferences: preferences)
            previousPreferences = preferences
            previousLanguage = language
        }
        let visible = application.state.desktop.mainWindowVisible
        if visible != previousWindowVisible {
            NSApplication.shared.setActivationPolicy(visible ? .regular : .accessory)
            if visible {
                mainWindow?.deminiaturize(nil)
                mainWindow?.makeKeyAndOrderFront(nil)
                NSApplication.shared.activate()
            } else { mainWindow?.orderOut(nil) }
            previousWindowVisible = visible
        }
        let presentation = application.state.recordingPill
        if presentation != previousPill {
            pill?.update()
            previousPill = presentation
        }
    }

    private func updateMenus(language: AppLanguage, preferences: DesktopPreferences) {
        let appMenu = NSMenu()
        appMenu.addItem(item(language.text("About Whisper", "关于 Whisper"), #selector(showAbout)))
        appMenu.addItem(item(language.text("Third-party licenses…", "第三方许可证…"), #selector(showLicenses)))
        appMenu.addItem(.separator())
        appMenu.addItem(item(language.text("Open Whisper", "打开 Whisper"), #selector(showMain)))
        appMenu.addItem(item(language.text("Settings…", "设置…"), #selector(showSettings), key: ","))
        appMenu.addItem(.separator())
        appMenu.addItem(item(language.text("Hide Whisper", "隐藏 Whisper"), #selector(hideMain), key: "h"))
        appMenu.addItem(item(language.text("Quit Whisper", "退出 Whisper"), #selector(quit), key: "q"))
        let menu = NSMenu()
        let applicationItem = NSMenuItem(title: "Whisper", action: nil, keyEquivalent: "")
        applicationItem.submenu = appMenu
        menu.addItem(applicationItem)
        let edit = NSMenu(title: language.text("Edit", "编辑"))
        for (title, action, key) in [
            (language.text("Undo", "撤销"), Selector(("undo:")), "z"),
            (language.text("Cut", "剪切"), #selector(NSText.cut(_:)), "x"),
            (language.text("Copy", "复制"), #selector(NSText.copy(_:)), "c"),
            (language.text("Paste", "粘贴"), #selector(NSText.paste(_:)), "v"),
            (language.text("Select All", "全选"), #selector(NSText.selectAll(_:)), "a")
        ] { edit.addItem(NSMenuItem(title: title, action: action, keyEquivalent: key)) }
        let redo = NSMenuItem(title: language.text("Redo", "重做"), action: Selector(("redo:")), keyEquivalent: "z")
        redo.keyEquivalentModifierMask = [.command, .shift]
        edit.insertItem(redo, at: 1)
        edit.addItem(.separator())
        edit.addItem(item(language.text("Search Transcripts…", "搜索转录…"), #selector(showSearch), key: "k"))
        let editItem = NSMenuItem(title: edit.title, action: nil, keyEquivalent: "")
        editItem.submenu = edit
        menu.addItem(editItem)
        let windowMenu = NSMenu(title: language.text("Window", "窗口"))
        windowMenu.addItem(NSMenuItem(title: language.text("Minimize", "最小化"), action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m"))
        windowMenu.addItem(NSMenuItem(title: language.text("Zoom", "缩放"), action: #selector(NSWindow.performZoom(_:)), keyEquivalent: ""))
        let fullScreen = NSMenuItem(title: language.text("Toggle Full Screen", "切换全屏"), action: #selector(NSWindow.toggleFullScreen(_:)), keyEquivalent: "f")
        fullScreen.keyEquivalentModifierMask = [.command, .control]
        windowMenu.addItem(fullScreen)
        let windowItem = NSMenuItem(title: windowMenu.title, action: nil, keyEquivalent: "")
        windowItem.submenu = windowMenu
        menu.addItem(windowItem)
        NSApplication.shared.windowsMenu = windowMenu
        let help = NSMenu(title: language.text("Help", "帮助"))
        help.addItem(item(language.text("Whisper Help", "Whisper 帮助"), #selector(showHelp)))
        help.addItem(item(language.text("Third-party licenses…", "第三方许可证…"), #selector(showLicenses)))
        let helpItem = NSMenuItem(title: help.title, action: nil, keyEquivalent: "")
        helpItem.submenu = help
        menu.addItem(helpItem)
        NSApplication.shared.helpMenu = help
        NSApplication.shared.mainMenu = menu

        if preferences.showMenuBarIcon {
            if statusItem == nil { statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength) }
            statusItem?.button?.image = NSImage(systemSymbolName: "waveform", accessibilityDescription: "Whisper")
            let tray = NSMenu()
            tray.addItem(item(language.text("Open Whisper", "打开 Whisper"), #selector(showMain)))
            tray.addItem(item(language.text(preferences.pillVisible ? "Hide Dictation Panel" : "Show Dictation Panel", preferences.pillVisible ? "隐藏听写面板" : "显示听写面板"), #selector(togglePill)))
            tray.addItem(.separator())
            tray.addItem(item(language.text("Quit Whisper", "退出 Whisper"), #selector(quit)))
            statusItem?.menu = tray
        } else if let statusItem {
            NSStatusBar.system.removeStatusItem(statusItem)
            self.statusItem = nil
        }
    }

    private func item(_ title: String, _ action: Selector, key: String = "") -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
        item.target = self
        return item
    }

    @objc private func screenChanged() { pill?.update() }
    @objc private func showMain() {
        previousWindowVisible = nil
        application?.send(.showMainWindow)
        synchronize()
    }
    @objc private func showSettings() {
        showMain()
        NotificationCenter.default.post(name: .init("WhisperOpenSettings"), object: nil)
    }
    @objc private func showSearch() { application?.send(.openHistorySearch) }
    @objc private func showHelp() {
        guard !syntheticPreview else { return }
        if let url = URL(string: "https://github.com/softmaxe/whisper#quick-start") { NSWorkspace.shared.open(url) }
    }
    @objc private func requestShortcutPermission() { shortcuts?.start(requestPermission: true) }
    @objc private func hideMain() { application?.send(.closeMainWindow) }
    @objc private func togglePill() {
        guard var preferences = application?.state.settings.desktop else { return }
        preferences.pillVisible.toggle()
        application?.send(.saveDesktopPreferences(preferences))
    }
    @objc private func quit() { NSApplication.shared.terminate(nil) }
    @objc private func showAbout() {
        let language = application?.state.settings.language ?? .english
        NSApplication.shared.orderFrontStandardAboutPanel(options: [
            .applicationName: "Whisper",
            .credits: NSAttributedString(string: language.text(
                "MediaRemoteAdapter: BSD 3-Clause. FFmpeg: LGPL 2.1 or later. LAME: LGPL 2.0 or later. See Third-party licenses for full notices.",
                "MediaRemoteAdapter：BSD 3-Clause。FFmpeg：LGPL 2.1 或更高版本。LAME：LGPL 2.0 或更高版本。完整声明请见第三方许可证。"
            ))
        ])
    }
    @objc private func showLicenses() {
        guard !syntheticPreview else { return }
        guard let resources = Bundle.main.resourceURL else { return }
        NSWorkspace.shared.open(resources.appendingPathComponent("licenses", isDirectory: true))
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool { application?.send(.closeMainWindow); return false }
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        showMain()
        return false
    }
    func applicationDidBecomeActive(_ notification: Notification) {
        guard !terminating else { return }
        application?.send(.refreshLoginItemStatus)
        shortcuts?.start()
    }
    func applicationDidResignActive(_ notification: Notification) {
        application?.send(.endShortcutCapture)
        shortcuts?.updateConfiguration()
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard !terminating else { return .terminateLater }
        terminating = true
        shortcuts?.stop()
        Task { @MainActor in
            await application?.prepareForTermination()
            // The barrier joins accepted audio, History, and the Insights events queued by those writes.
            sender.reply(toApplicationShouldTerminate: true)
        }
        return .terminateLater
    }
}
