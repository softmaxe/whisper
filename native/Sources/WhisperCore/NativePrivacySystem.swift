import AppKit
import AVFoundation
import ApplicationServices

/// Used only by the executable; test/library defaults never query or request OS permissions.
@MainActor public struct NativePrivacySystem: PrivacySystem {
    public init() {}
    public func snapshot() -> PermissionSnapshot {
        let microphone: PrivacyAuthorization = switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized: .granted
        case .denied: .denied
        case .restricted: .restricted
        case .notDetermined: .notDetermined
        @unknown default: .unavailable
        }
        return PermissionSnapshot(microphone: microphone, accessibility: AXIsProcessTrusted() ? .granted : .denied)
    }
    public func request(_ permission: PrivacyPermission) async {
        switch permission {
        case .microphone:
            if AVCaptureDevice.authorizationStatus(for: .audio) == .notDetermined {
                _ = await AVCaptureDevice.requestAccess(for: .audio)
            } else { openSettings(permission) }
        case .accessibility:
            _ = AXIsProcessTrustedWithOptions(["AXTrustedCheckOptionPrompt": true] as CFDictionary)
            if !AXIsProcessTrusted() { openSettings(permission) }
        }
    }
    public func openSettings(_ permission: PrivacyPermission) {
        let pane = permission == .microphone ? "Privacy_Microphone" : "Privacy_Accessibility"
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?" + pane) {
            NSWorkspace.shared.open(url)
        }
    }
}
