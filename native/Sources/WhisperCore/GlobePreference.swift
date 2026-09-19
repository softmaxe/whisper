import Foundation
import Darwin

@MainActor public protocol GlobePreferenceSystem {
    func read() -> Int32?
    func hasExplicitValue() -> Bool
    func update(_ value: Int32) -> Bool
    func clearExplicitValue()
}

/// The journal is durable before changing the system preference; a newer user choice always wins.
@MainActor public final class GlobePreferenceController {
    private struct Marker: Codable { let version: Int; let originalValue: Int32; let keyExisted: Bool }
    private let markerURL: URL
    private let system: any GlobePreferenceSystem
    private var owned: Marker?
    private var recovered = false
    private var enabledSession = false
    public init(markerURL: URL, system: any GlobePreferenceSystem) { self.markerURL = markerURL; self.system = system }

    @discardableResult public func setOwned(_ enabled: Bool) -> Bool {
        guard recover() else { return false }
        if enabled {
            if owned != nil || enabledSession { return system.read() == 0 }
            guard let original = system.read() else { return false }
            if original == 0 { enabledSession = true; return true }
            let marker = Marker(version: 1, originalValue: original, keyExisted: system.hasExplicitValue())
            do {
                try FileManager.default.createDirectory(at: markerURL.deletingLastPathComponent(), withIntermediateDirectories: true,
                    attributes: [.posixPermissions: 0o700])
                try JSONEncoder().encode(marker).write(to: markerURL, options: .atomic)
                try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: markerURL.path)
            } catch { return false }
            owned = marker
            enabledSession = true
            if system.update(0) { return true }
            // A failed confirmation can follow a delayed system update. Keep its recovery journal.
            return false
        }
        enabledSession = false
        guard let marker = owned else { return true }
        guard let current = system.read() else { return false }
        if current == 0 {
            guard system.update(marker.originalValue) else { return false }
            if !marker.keyExisted { system.clearExplicitValue() }
        }
        owned = nil
        try? FileManager.default.removeItem(at: markerURL)
        return true
    }

    private func recover() -> Bool {
        if recovered { return true }
        guard FileManager.default.fileExists(atPath: markerURL.path) else { recovered = true; return true }
        guard let data = try? Data(contentsOf: markerURL) else { return false }
        guard let marker = try? JSONDecoder().decode(Marker.self, from: data), marker.version == 1 else { return false }
        guard let current = system.read() else { return false }
        if current == 0 { owned = marker }
        else { try? FileManager.default.removeItem(at: markerURL) }
        recovered = true
        return true
    }
}

// Adapted from resources/macos-globe-listener.swift. These dynamically resolved TIS entry points
// are existing compatibility behavior, not public API guarantees. Failure leaves preferences intact.
@MainActor final class NativeGlobePreferenceSystem: GlobePreferenceSystem {
    private typealias Getter = @convention(c) () -> Int32
    private typealias Setter = @convention(c) (Int32) -> Void
    private let domain = "com.apple.HIToolbox" as CFString
    private let key = "AppleFnUsageType" as CFString
    private lazy var entryPoints: (Getter, Setter)? = {
        guard let carbon = dlopen("/System/Library/Frameworks/Carbon.framework/Carbon", RTLD_LAZY),
              let get = dlsym(carbon, "TISGetFnUsageType"), let update = dlsym(carbon, "TISUpdateFnUsageType") else { return nil }
        return (unsafeBitCast(get, to: Getter.self), unsafeBitCast(update, to: Setter.self))
    }()
    func read() -> Int32? { entryPoints?.0() }
    func hasExplicitValue() -> Bool {
        CFPreferencesSynchronize(domain, kCFPreferencesCurrentUser, kCFPreferencesAnyHost)
        return CFPreferencesCopyValue(key, domain, kCFPreferencesCurrentUser, kCFPreferencesAnyHost) != nil
    }
    func update(_ value: Int32) -> Bool {
        guard let entryPoints else { return false }
        entryPoints.1(value)
        return entryPoints.0() == value
    }
    func clearExplicitValue() {
        CFPreferencesSetValue(key, nil, domain, kCFPreferencesCurrentUser, kCFPreferencesAnyHost)
        CFPreferencesSynchronize(domain, kCFPreferencesCurrentUser, kCFPreferencesAnyHost)
    }
}
