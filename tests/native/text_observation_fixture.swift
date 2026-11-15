// Standalone AppKit fixture for the read-only AX observation smoke check.
import Cocoa
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let window = NSWindow(contentRect: NSRect(x: 300, y: 300, width: 500, height: 250), styleMask: [.titled, .closable], backing: .buffered, defer: false)
window.title = "Whisper observation verification fixture"
let editor = NSTextView(frame: NSRect(x: 10, y: 50, width: 480, height: 180))
final class SendButton: NSButton {
    override var acceptsFirstResponder: Bool { true }
}
final class Actions: NSObject {
    @objc func send() {
        editor.setSelectedRange(NSRange(location: 0, length: editor.string.utf16.count))
        editor.insertText("", replacementRange: editor.selectedRange())
    }
}
let actions = Actions()
let send = SendButton(title: "Send", target: actions, action: #selector(Actions.send))
send.frame = NSRect(x: 10, y: 10, width: 100, height: 30)
window.contentView?.addSubview(editor)
window.contentView?.addSubview(send)
window.makeKeyAndOrderFront(nil)
window.makeFirstResponder(editor)
app.activate(ignoringOtherApps: true)
print(ProcessInfo.processInfo.processIdentifier)
fflush(stdout)
DispatchQueue.global().async {
    while let line = readLine() {
        DispatchQueue.main.async {
            if line == "quit" { app.terminate(nil) }
            else if line == "correct" { editor.setSelectedRange(NSRange(location: 6, length: 5)); editor.insertText("World", replacementRange: editor.selectedRange()) }
            else if line == "paste" { window.makeFirstResponder(editor); editor.insertText("hello world", replacementRange: editor.selectedRange()) }
            else if line == "clear" { editor.setSelectedRange(NSRange(location: 0, length: editor.string.utf16.count)); editor.insertText("", replacementRange: editor.selectedRange()) }
            else if line == "send" { window.makeFirstResponder(send); send.performClick(nil) }
            else if line == "long" { window.makeFirstResponder(editor); editor.string = String(repeating: "existing ", count: 2000) + "克劳德"; editor.setSelectedRange(NSRange(location: editor.string.utf16.count, length: 0)) }
            else if line == "long-correct" { editor.setSelectedRange(NSRange(location: editor.string.utf16.count - 3, length: 3)); editor.insertText("Claude", replacementRange: editor.selectedRange()) }
            print("DONE:" + line)
            fflush(stdout)
        }
    }
}
app.run()
