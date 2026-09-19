import AppKit
import SwiftUI
import WhisperCore

struct ShortcutSettingsView: View {
    let application: WhisperApplication
    let requestPermission: () -> Void
    private var language: AppLanguage { application.state.settings.language }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(language.text("Dictation shortcuts", "听写快捷键")).font(.headline)
            Text(language.text("Hold and release to submit. Double-tap for Hands-free Dictation, then tap to finish. Esc always cancels.", "按住说话，松开提交。双击开始免按键听写，再轻按结束。Esc 用于取消。"))
                .font(.caption).foregroundStyle(.secondary)
            ForEach(Array(application.state.settings.shortcuts.enumerated()), id: \.offset) { index, value in
                HStack {
                    Text((try? ShortcutBinding(value).label(in: language)) ?? value)
                    Spacer()
                    Button(language.text("Edit", "编辑")) { application.send(.beginShortcutCapture(index: index)) }
                    if application.state.settings.shortcuts.count > 1 {
                        Button(role: .destructive) {
                            var values = application.state.settings.shortcuts
                            values.remove(at: index)
                            application.send(.saveShortcuts(values))
                        } label: { Image(systemName: "trash") }
                        .accessibilityLabel(language.text("Remove shortcut", "移除快捷键"))
                    }
                }
                .padding(14).background(.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 8))
            }
            if application.state.shortcutCapture.isActive {
                ZStack {
                    ShortcutCaptureSurface(application: application)
                    Text(application.state.shortcutCapture.held.isEmpty
                         ? language.text("Press a shortcut…", "请按下快捷键…")
                         : application.state.shortcutCapture.held)
                        .allowsHitTesting(false)
                }
                .frame(height: 58)
                .background(.tint.opacity(0.06), in: RoundedRectangle(cornerRadius: 8))
                .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.tint))
                Button(language.text("Cancel capture", "取消设置")) { application.send(.endShortcutCapture) }
            } else {
                HStack {
                    Button(language.text("Add another shortcut", "添加快捷键")) { application.send(.beginShortcutCapture(index: nil)) }
                    Spacer()
                    Button(language.text("Reset to Right Command", "重置为右 Command")) { application.send(.saveShortcuts(["RightCommand"])) }
                }
            }
            if let error = application.state.shortcutError ?? application.state.shortcutWarning {
                Text(error.message(in: language)).font(.caption).foregroundStyle(.red)
                    .accessibilityIdentifier("shortcut-error")
            }
            if let error = application.state.configurationError {
                Text(error.message(in: language)).font(.caption).foregroundStyle(.red)
            }
            Text(language.text("Use a right-side modifier, a keyboard combination, Globe/Fn, a function key, or Mouse Button 4/5. Plain Esc is reserved for cancellation.", "可使用右侧修饰键、键盘组合、Globe/Fn、功能键或鼠标按键 4/5。单独 Esc 保留用于取消。"))
                .font(.caption).foregroundStyle(.secondary)
            if !application.state.shortcutAvailable {
                Button(language.text("Enable Accessibility for global shortcuts", "启用辅助功能以使用全局快捷键"), action: requestPermission)
                    .accessibilityIdentifier("enable-shortcut-access")
            }
        }
        .onDisappear { application.send(.endShortcutCapture) }
    }
}

private struct ShortcutCaptureSurface: NSViewRepresentable {
    let application: WhisperApplication
    func makeNSView(context: Context) -> CaptureResponder {
        let view = CaptureResponder()
        view.application = application
        view.setAccessibilityIdentifier("shortcut-capture")
        return view
    }
    func updateNSView(_ view: CaptureResponder, context: Context) {
        view.application = application
        DispatchQueue.main.async { [weak view] in
            guard let view, view.application?.state.shortcutCapture.isActive == true else { return }
            view.window?.makeFirstResponder(view)
        }
    }
}

private final class CaptureResponder: NSView {
    weak var application: WhisperApplication?
    override var acceptsFirstResponder: Bool { true }
    private func capture(_ event: NSEvent) {
        guard let application, application.state.shortcutCapture.isActive,
              !application.state.shortcutAvailable,
              let input = NativeShortcutMonitor.input(for: event) else { return }
        application.send(.shortcut(input))
    }
    override func keyDown(with event: NSEvent) { capture(event) }
    override func keyUp(with event: NSEvent) { capture(event) }
    override func flagsChanged(with event: NSEvent) { capture(event) }
    override func otherMouseDown(with event: NSEvent) { capture(event) }
    override func otherMouseUp(with event: NSEvent) { capture(event) }
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard application?.state.shortcutCapture.isActive == true else { return false }
        capture(event)
        return true
    }
}
