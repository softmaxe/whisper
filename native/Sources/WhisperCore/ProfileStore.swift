import Darwin
import Foundation

public struct NativeProfile: Sendable {
    public let directory: URL

    public init(directory: URL) {
        self.directory = directory.standardizedFileURL
    }

    public static func applicationDefault() throws -> Self {
        let support = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask,
            appropriateFor: nil, create: true
        )
        return Self(directory: support.appendingPathComponent("WhisperNative", isDirectory: true))
    }
}

public final class ProfileStore {
    public let profile: NativeProfile

    public init(profile: NativeProfile) {
        self.profile = profile
    }

    private struct SettingsDocument: Codable {
        var version = 1
        var settings: AppSettings
    }

    public func loadSettings() throws -> AppSettings {
        guard let document: SettingsDocument = try read("settings.json") else { return AppSettings() }
        guard document.version == 1 else { throw ConfigurationError.incompatibleProfile }
        return document.settings
    }

    public func saveSettings(_ settings: AppSettings) throws {
        try write(SettingsDocument(settings: settings), to: "settings.json")
    }

    public func read<Value: Decodable>(_ name: String) throws -> Value? {
        let url = try fileURL(name)
        guard FileManager.default.fileExists(atPath: url.path) else { return nil }
        return try JSONDecoder().decode(Value.self, from: Data(contentsOf: url))
    }

    public func write<Value: Encodable>(_ value: Value, to name: String) throws {
        let url = try fileURL(name)
        try FileManager.default.createDirectory(
            at: profile.directory, withIntermediateDirectories: true,
            attributes: [.posixPermissions: 0o700]
        )
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let temporary = profile.directory.appendingPathComponent(".write-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: temporary) }
        try encoder.encode(value).write(to: temporary, options: [.withoutOverwriting, .completeFileProtection])
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: temporary.path)
        // Commit only after encoding and permissions succeed, so a failed save keeps its old account.
        guard rename(temporary.path, url.path) == 0 else { throw ConfigurationError.persistenceFailed }
    }

    private func fileURL(_ name: String) throws -> URL {
        guard !name.isEmpty, name != ".", name != "..", !name.contains("/"), !name.contains("\\") else {
            throw ConfigurationError.persistenceFailed
        }
        return profile.directory.appendingPathComponent(name)
    }
}
