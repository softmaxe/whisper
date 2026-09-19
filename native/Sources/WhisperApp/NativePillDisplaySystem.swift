import AppKit
import WhisperCore

@MainActor struct NativePillDisplaySystem: PillDisplaySystem {
    func snapshot() -> PillDisplaySnapshot {
        let displays = NSScreen.screens.enumerated().map { index, screen in
            let id = (screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.stringValue ?? "screen-\(index)"
            return PillDisplay(id: id, frame: screen.frame, workArea: screen.visibleFrame)
        }
        return PillDisplaySnapshot(displays: displays, cursor: NSEvent.mouseLocation)
    }

    func windowBounds(for target: PasteTarget) async -> [CGRect] {
        let primaryTop = NSScreen.screens.first?.frame.maxY ?? 0
        // Reuse the reference window-server query. Bounds need no Screen Recording permission or AX tree activation.
        return await Task.detached {
            let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
            return windows.compactMap { window -> CGRect? in
                guard let pid = window[kCGWindowOwnerPID as String] as? Int32, pid == target.processID,
                      let layer = window[kCGWindowLayer as String] as? Int, layer == 0,
                      let bounds = window[kCGWindowBounds as String] as? [String: Any],
                      let x = bounds["X"] as? Double, let y = bounds["Y"] as? Double,
                      let width = bounds["Width"] as? Double, let height = bounds["Height"] as? Double else { return nil }
                return RecordingPillGeometry.appKitBounds(fromQuartz: CGRect(x: x, y: y, width: width, height: height), primaryTop: primaryTop)
            }
        }.value
    }
}
