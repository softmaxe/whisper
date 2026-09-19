import AppKit
import CoreText
import SwiftUI
import WhisperCore

@main
enum WhisperMacApp {
    @MainActor static func main() {
        let app = NSApplication.shared
        let delegate = WhisperAppDelegate()
        app.delegate = delegate
        app.setActivationPolicy(.accessory)
        app.run()
        withExtendedLifetime(delegate) {}
    }
}
