import Foundation
import Testing
import WhisperCore

private final class ObservedCredentialStore: CredentialStore {
    let base: any CredentialStore
    var written: [String] = []
    var deleted: [String] = []
    var rejectedDeletion: String?
    init(_ base: any CredentialStore) { self.base = base }
    func read(account: String) throws -> String? { try base.read(account: account) }
    func write(_ value: String, account: String) throws {
        try base.write(value, account: account)
        written.append(account)
    }
    func delete(account: String) throws {
        if account == rejectedDeletion { throw ConfigurationError.credentialUnavailable }
        try base.delete(account: account)
        deleted.append(account)
    }
}

@Suite(.serialized) @MainActor struct ServiceSettingsWorkflowTests {
    private func save(_ app: WhisperApplication, cleanup: Bool, credential: CredentialChange, model: String = "model-fixture") {
        if cleanup { app.send(.saveCleanup(.init(serverURL: "https://cleanup.example.com", model: model), credential: credential)) }
        else { app.send(.saveASR(.init(serverURL: "https://asr.example.com", model: model), credential: credential)) }
    }
    private func account(_ app: WhisperApplication, cleanup: Bool) -> String? {
        cleanup ? app.state.settings.cleanupCredentialAccount : app.state.settings.asrCredentialAccount
    }
    private func secret(_ app: WhisperApplication, cleanup: Bool) throws -> String? {
        try cleanup ? app.cleanupCredential() : app.asrCredential()
    }

    @Test(arguments: [false, true])
    func replacementRollbackAndNormalizationPreserveServiceContracts(cleanup: Bool) throws {
        let profile = try ProfileFixture(); defer { profile.remove() }
        let credentials = ObservedCredentialStore(profile.credentials)
        let app = WhisperApplication(profile: profile.profile, credentials: credentials)
        save(app, cleanup: cleanup, credential: .replace(" original-placeholder "))
        let prior = try #require(account(app, cleanup: cleanup))
        #expect(prior.hasPrefix(cleanup ? "cleanup-" : "asr-"))
        let expected = cleanup ? "original-placeholder" : " original-placeholder "
        #expect(try secret(app, cleanup: cleanup) == expected)
        let settings = app.state.settings
        let file = profile.profile.directory.appendingPathComponent("settings.json")
        let backup = profile.profile.directory.appendingPathComponent("backup.json")
        try FileManager.default.moveItem(at: file, to: backup)
        try FileManager.default.createDirectory(at: file, withIntermediateDirectories: false)
        save(app, cleanup: cleanup, credential: .replace(" uncommitted-placeholder "), model: "new-model")
        #expect(!app.state.settingsSaved)
        #expect(app.state.configurationError == .persistenceFailed)
        #expect(app.state.settings == settings)
        #expect(credentials.written.count == 2)
        #expect(credentials.deleted == [credentials.written[1]])
        #expect(try credentials.read(account: credentials.written[1]) == nil)
        #expect(try secret(app, cleanup: cleanup) == expected)
        try FileManager.default.removeItem(at: file)
        try FileManager.default.moveItem(at: backup, to: file)
        let reopened = profile.open()
        #expect(reopened.state.settings == settings)
        #expect(try secret(reopened, cleanup: cleanup) == expected)
    }

    @Test(arguments: [false, true])
    func oldCredentialDeletionFailureKeepsTheCommittedReplacementUsable(cleanup: Bool) throws {
        let profile = try ProfileFixture(); defer { profile.remove() }
        let credentials = ObservedCredentialStore(profile.credentials)
        let app = WhisperApplication(profile: profile.profile, credentials: credentials)
        save(app, cleanup: cleanup, credential: .replace("old-placeholder"))
        credentials.rejectedDeletion = try #require(account(app, cleanup: cleanup))
        save(app, cleanup: cleanup, credential: .replace("new-placeholder"), model: "new-model")
        #expect(app.state.settingsSaved)
        #expect(app.state.configurationError == .credentialUnavailable)
        #expect(account(app, cleanup: cleanup) != credentials.rejectedDeletion)
        #expect(try secret(app, cleanup: cleanup) == "new-placeholder")
        #expect(try secret(profile.open(), cleanup: cleanup) == "new-placeholder")
        #expect(credentials.deleted.isEmpty)
        let disk = try String(contentsOf: profile.profile.directory.appendingPathComponent("settings.json"), encoding: .utf8)
        #expect(!disk.contains("new-placeholder"))
        #expect(!disk.contains("old-placeholder"))
    }
}
