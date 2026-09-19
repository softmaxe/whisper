import Foundation
import Security
import Testing
import WhisperCore

@Suite(.serialized)
@MainActor
struct SettingsWorkflowTests {
    @Test func saveAndRelaunchWithRealProfileAndKeychain() throws {
        let fixture = try ProfileFixture()
        defer { fixture.remove() }
        let app = fixture.open()
        #expect(app.state.settings.asr.serverURL.isEmpty)
        #expect(!app.state.credentialConfigured)
        app.send(.saveASR(.init(serverURL: " http://localhost:8178/v1 ", model: " whisper-1 "), credential: .replace("test-api-key-placeholder")))
        #expect(app.state.settingsSaved)
        #expect(app.state.configurationError == nil)
        #expect(app.state.credentialConfigured)
        app.send(.setLanguage(.simplifiedChinese))

        let reopened = fixture.open()
        #expect(reopened.state.settings.asr == .init(serverURL: "http://localhost:8178/v1", model: "whisper-1"))
        #expect(reopened.state.settings.language == .simplifiedChinese)
        #expect(try reopened.asrCredential() == "test-api-key-placeholder")
        let stored = try String(contentsOf: fixture.profile.directory.appendingPathComponent("settings.json"), encoding: .utf8)
        #expect(!stored.contains("test-api-key-placeholder"))
        #expect(ConfigurationError.insecureURL.message(in: reopened.state.settings.language).contains("HTTPS"))
        #expect(ConfigurationError.modelRequired.message(in: .english) == "Enter the transcription model name.")
    }

    @Test func unchangedReplacedAndRemovedCredentialsSurviveRelaunch() throws {
        let fixture = try ProfileFixture()
        defer { fixture.remove() }
        let configuration = ASRConfiguration(serverURL: "https://asr.example.com", model: "whisper-1")
        let app = fixture.open()
        app.send(.saveASR(configuration, credential: .replace("first-placeholder")))
        let oldAccount = try #require(app.state.settings.asrCredentialAccount)
        app.send(.saveASR(configuration, credential: .unchanged))
        #expect(try fixture.open().asrCredential() == "first-placeholder")
        app.send(.saveASR(configuration, credential: .replace("second-placeholder")))
        #expect(try fixture.open().asrCredential() == "second-placeholder")
        #expect(try fixture.credentials.read(account: oldAccount) == nil)
        app.send(.saveASR(configuration, credential: .remove))
        #expect(!fixture.open().state.credentialConfigured)
        #expect(try fixture.open().asrCredential() == nil)
    }

    @Test(arguments: [
        "http://localhost:8178", "http://127.0.0.1", "http://0.0.0.0", "http://10.2.3.4",
        "http://192.168.2.4", "http://172.16.0.1", "http://172.31.255.255", "http://100.64.0.1",
        "http://100.127.255.255", "http://169.254.2.3", "http://[::1]:8080", "http://[fe80::1]",
        "http://[febf::1]", "http://[fc00::1]", "http://[fd12::1]", "http://server.local",
        "http://host.tailnet.ts.net", "https://asr.example.com/v1"
    ])
    func allowedEndpointsPersist(url: String) throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        app.send(.saveASR(.init(serverURL: url, model: "whisper-1"), credential: .unchanged))
        #expect(app.state.settingsSaved)
        #expect(fixture.open().state.settings.asr.serverURL == url)
    }

    @Test(arguments: [
        "http://example.com", "http://127.example.com", "http://10.example.com", "http://192.168.example.com",
        "http://172.32.0.1", "http://100.128.0.1", "http://[fec0::1]", "http://[2001:db8::1]",
        "ftp://localhost", "not a url", "https://",
        "http://localhost:99999", "https://user:placeholder@example.com"
    ])
    func invalidEndpointsDoNotReplaceSavedConfiguration(url: String) throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        let original = ASRConfiguration(serverURL: "https://asr.example.com", model: "whisper-1")
        app.send(.saveASR(original, credential: .unchanged))
        app.send(.saveASR(.init(serverURL: url, model: "new-model"), credential: .replace("unused-placeholder")))
        #expect(!app.state.settingsSaved)
        #expect(app.state.configurationError != nil)
        #expect(fixture.open().state.settings.asr == original)
    }

    @Test(arguments: [
        ("127.1", "127.0.0.1"), ("127.0.1", "127.0.0.1"),
        ("127.00.0.1", "127.0.0.1"), ("2130706433", "127.0.0.1"),
        ("0177.1", "127.0.0.1"), ("0x7f000001", "127.0.0.1"),
        ("0X7F.0.0.1", "127.0.0.1"), ("127.1.", "127.0.0.1"),
        ("0", "0.0.0.0"), ("0x", "0.0.0.0"), ("10.1", "10.0.0.1"),
        ("0300.0250.1", "192.168.0.1"), ("0xac100001", "172.16.0.1"),
        ("100.4194304", "100.64.0.0"), ("169.16646145", "169.254.0.1")
    ])
    func privateIPv4FormsPersistTheirCanonicalDestination(input: String, canonical: String) throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        let suffix = ":8178/custom%20path/v1?api-version=preview&name=a%2Fb"
        app.send(.saveASR(.init(serverURL: "http://" + input + suffix, model: "whisper-1"), credential: .unchanged))
        #expect(app.state.settingsSaved)
        #expect(app.state.configurationError == nil)
        #expect(fixture.open().state.settings.asr.serverURL == "http://" + canonical + suffix)
    }

    @Test(arguments: [
        ("134744072", "8.8.8.8"), ("0x08080808", "8.8.8.8"),
        ("010.010.010.010", "8.8.8.8"), ("4294967295", "255.255.255.255"),
        ("127.example.com", "127.example.com"), ("10.example.com", "10.example.com"),
        ("127.0.0.1.nip.io", "127.0.0.1.nip.io"),
        ("0x7f000001.example.com", "0x7f000001.example.com"), ("0xgg", "0xgg"),
        ("127.1..", "127.1..")
    ])
    func publicNumericAndLookalikeHostsStillRequireHTTPS(input: String, canonical: String) throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        app.send(.saveASR(.init(serverURL: "https://" + input + "/v1", model: "whisper-1"), credential: .unchanged))
        #expect(app.state.settingsSaved)
        let saved = ASRConfiguration(serverURL: "https://" + canonical + "/v1", model: "whisper-1")
        #expect(fixture.open().state.settings.asr == saved)
        app.send(.saveASR(.init(serverURL: "http://" + input + "/v1", model: "new-model"), credential: .unchanged))
        #expect(!app.state.settingsSaved)
        #expect(app.state.configurationError == .insecureURL)
        #expect(fixture.open().state.settings.asr == saved)
    }

    @Test(arguments: [
        "4294967296", "0x100000000", "999999999999999999999999999999999999",
        "0xffffffffffffffffffffffffffffffff", "127.16777216", "256.1", "1.2.3.256",
        "127..1", "1.2.3.4.5", "09.0.0.1", "example.1", "127.0xgg.1"
    ])
    func malformedIPv4CannotReplaceSavedConfiguration(host: String) throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        let original = ASRConfiguration(serverURL: "https://asr.example.com", model: "whisper-1")
        app.send(.saveASR(original, credential: .unchanged))
        for scheme in ["http", "https"] {
            app.send(.saveASR(.init(serverURL: scheme + "://" + host + "/v1", model: "new-model"), credential: .unchanged))
            #expect(!app.state.settingsSaved)
            #expect(app.state.configurationError == .invalidURL)
            #expect(fixture.open().state.settings.asr == original)
        }
    }

    @Test func legacySiblingIsNeverImportedOrModified() throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let legacy = fixture.root.appendingPathComponent("whisper", isDirectory: true)
        try FileManager.default.createDirectory(at: legacy, withIntermediateDirectories: true)
        let legacySettings = legacy.appendingPathComponent("settings.json")
        let marker = Data("legacy-profile-fixture".utf8)
        try marker.write(to: legacySettings)
        let app = fixture.open()
        #expect(app.state.settings == AppSettings())
        app.send(.saveASR(.init(serverURL: "https://asr.example.com", model: "whisper-1"), credential: .unchanged))
        #expect(try Data(contentsOf: legacySettings) == marker)
        #expect(try NativeProfile.applicationDefault().directory.lastPathComponent == "WhisperNative")
    }

    @Test func unreadableProfileIsPreservedAndCannotBeOverwritten() throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        try FileManager.default.createDirectory(at: fixture.profile.directory, withIntermediateDirectories: true)
        let file = fixture.profile.directory.appendingPathComponent("settings.json")
        let damaged = Data("corrupted-profile-fixture".utf8)
        try damaged.write(to: file)
        let app = fixture.open()
        #expect(app.state.configurationError == .incompatibleProfile)
        app.send(.saveASR(.init(serverURL: "https://asr.example.com", model: "whisper-1"), credential: .unchanged))
        app.send(.setLanguage(.simplifiedChinese))
        #expect(!app.state.settingsSaved)
        #expect(try Data(contentsOf: file) == damaged)
    }

    @Test func keychainFailureKeepsPriorConfigurationAndNeverFallsBackToDisk() throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        app.send(.saveASR(.init(serverURL: "https://asr.example.com", model: "whisper-1"), credential: .unchanged))
        app.send(.saveASR(.init(serverURL: "https://new.example.com", model: "new-model"), credential: .replace("unwritable-placeholder")))
        #expect(app.state.configurationError == .credentialUnavailable)
        #expect(!app.state.settingsSaved)
        #expect(fixture.open().state.settings.asr.model == "whisper-1")
        #expect(!fixture.open().state.credentialConfigured)
    }

    @Test func failedDiskWriteKeepsThePreviousCredentialUsable() throws {
        let fixture = try ProfileFixture()
        defer { fixture.remove() }
        let app = fixture.open()
        let original = ASRConfiguration(serverURL: "https://asr.example.com", model: "whisper-1")
        app.send(.saveASR(original, credential: .replace("original-placeholder")))
        let file = fixture.profile.directory.appendingPathComponent("settings.json")
        let backup = fixture.profile.directory.appendingPathComponent("before-failure.json")
        try FileManager.default.moveItem(at: file, to: backup)
        try FileManager.default.createDirectory(at: file, withIntermediateDirectories: false)
        app.send(.saveASR(.init(serverURL: "https://new.example.com", model: "new-model"), credential: .replace("failed-save-placeholder")))
        #expect(!app.state.settingsSaved)
        #expect(app.state.configurationError == .persistenceFailed)
        #expect(app.state.settings.asr == original)
        #expect(try app.asrCredential() == "original-placeholder")
        try FileManager.default.removeItem(at: file)
        try FileManager.default.moveItem(at: backup, to: file)
        #expect(try fixture.open().asrCredential() == "original-placeholder")
    }

    @Test func blankModelDoesNotSave() throws {
        let fixture = try ProfileFixture(keychain: false)
        defer { fixture.remove() }
        let app = fixture.open()
        app.send(.saveASR(.init(serverURL: "http://localhost", model: " "), credential: .unchanged))
        #expect(app.state.configurationError == .modelRequired)
        #expect(fixture.open().state.settings == AppSettings())
    }
}

@MainActor
private final class ProfileFixture {
    let root: URL
    let profile: NativeProfile
    let credentials: any CredentialStore
    private var keychain: SecKeychain?

    init(keychain createKeychain: Bool = true) throws {
        root = FileManager.default.temporaryDirectory.appendingPathComponent("whisper-native-test-" + UUID().uuidString)
        profile = NativeProfile(directory: root.appendingPathComponent("WhisperNative"))
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        if createKeychain {
            // A temporary file Keychain keeps fixture credentials out of the login Keychain.
            let password = UUID().uuidString
            let keychainPath = root.appendingPathComponent("fixture.keychain-db").path
            var createdKeychain: SecKeychain?
            let status = password.withCString { value in
                SecKeychainCreate(keychainPath, UInt32(password.utf8.count), value, false, nil, &createdKeychain)
            }
            guard status == errSecSuccess else {
                try? FileManager.default.removeItem(at: root)
                throw ConfigurationError.credentialUnavailable
            }
            keychain = createdKeychain
            credentials = KeychainCredentialStore(service: "local.whisper.desktop.native.tests", keychain: keychain)
        } else {
            credentials = UnavailableCredentials()
        }
    }

    func open() -> WhisperApplication { WhisperApplication(profile: profile, credentials: credentials) }
    func remove() {
        if let keychain { #expect(SecKeychainDelete(keychain) == errSecSuccess) }
        do {
            try FileManager.default.removeItem(at: root)
        } catch {
            Issue.record("Temporary native profile cleanup failed.")
        }
        #expect(!FileManager.default.fileExists(atPath: root.path))
    }
}

private struct UnavailableCredentials: CredentialStore {
    func read(account: String) throws -> String? { throw ConfigurationError.credentialUnavailable }
    func write(_ value: String, account: String) throws { throw ConfigurationError.credentialUnavailable }
    func delete(account: String) throws { throw ConfigurationError.credentialUnavailable }
}
