import AppKit

@MainActor public protocol TextClipboard {
    func write(_ text: String)
}

@MainActor public final class SystemTextClipboard: TextClipboard {
    public init() {}
    public func write(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }
}
