import AppKit

@MainActor public protocol TextClipboard {
    func write(_ text: String)
    func writeResult(_ text: String) -> Bool
}

extension TextClipboard {
    public func writeResult(_ text: String) -> Bool { write(text); return true }
}

@MainActor public final class SystemTextClipboard: TextClipboard {
    public init() {}
    public func write(_ text: String) {
        _ = writeResult(text)
    }
    public func writeResult(_ text: String) -> Bool {
        NSPasteboard.general.clearContents()
        return NSPasteboard.general.setString(text, forType: .string)
    }
}
