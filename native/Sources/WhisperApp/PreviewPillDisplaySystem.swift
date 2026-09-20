import AppKit
import WhisperCore

/// Synthetic review may use this app's screen geometry, never another application's windows or AX tree.
@MainActor struct PreviewPillDisplaySystem: PillDisplaySystem {
    func snapshot() -> PillDisplaySnapshot {
        let displays = NSScreen.screens.enumerated().map { index, screen in
            PillDisplay(id: "preview-screen-\(index)", frame: screen.frame, workArea: screen.visibleFrame)
        }
        let frame = NSScreen.main?.visibleFrame ?? .zero
        return .init(displays: displays, cursor: CGPoint(x: frame.midX, y: frame.midY))
    }
    func windowBounds(for target: PasteTarget) async -> [CGRect] { [] }
}
