import Foundation

extension WhisperApplication {
    /// Commit a validated profile before deleting the previous secret; roll back only a new uncommitted account.
    func saveServiceSettings(credential: CredentialChange, account: WritableKeyPath<AppSettings, String?>,
                             accountPrefix: String, update: (inout AppSettings) throws -> Void) {
        state.settingsSaved = false
        guard profileReadable else { state.configurationError = .incompatibleProfile; return }
        var createdAccount: String?
        do {
            var settings = state.settings
            try update(&settings)
            switch credential {
            case .unchanged: break
            case let .replace(value):
                if value.isEmpty { settings[keyPath: account] = nil }
                else {
                    let newAccount = accountPrefix + UUID().uuidString
                    try credentials.write(value, account: newAccount)
                    createdAccount = newAccount
                    settings[keyPath: account] = newAccount
                }
            case .remove: settings[keyPath: account] = nil
            }
            let previous = state.settings[keyPath: account]
            try profileStore.saveSettings(settings)
            state.settings = settings
            state.settingsSaved = true
            state.configurationError = nil
            if let previous, previous != settings[keyPath: account] { try credentials.delete(account: previous) }
        } catch {
            if !state.settingsSaved, let createdAccount { try? credentials.delete(account: createdAccount) }
            state.configurationError = error as? ConfigurationError ?? .persistenceFailed
        }
    }
}
