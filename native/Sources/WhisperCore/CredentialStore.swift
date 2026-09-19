import Foundation
import Security

public protocol CredentialStore {
    func read(account: String) throws -> String?
    func write(_ value: String, account: String) throws
    func delete(account: String) throws
}

public final class KeychainCredentialStore: CredentialStore {
    private let service: String
    private let keychain: SecKeychain?

    public init(service: String = "local.whisper.desktop.native", keychain: SecKeychain? = nil) {
        self.service = service
        self.keychain = keychain
    }

    private func query(account: String) -> [String: Any] {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account
        ]
        if let keychain { query[kSecMatchSearchList as String] = [keychain] }
        return query
    }

    public func read(account: String) throws -> String? {
        var query = query(account: account)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = item as? Data,
              let value = String(data: data, encoding: .utf8) else {
            throw ConfigurationError.credentialUnavailable
        }
        return value
    }

    public func write(_ value: String, account: String) throws {
        let attributes = [kSecValueData as String: Data(value.utf8)]
        let status = SecItemUpdate(query(account: account) as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound {
            var item = query(account: account)
            item.removeValue(forKey: kSecMatchSearchList as String)
            if let keychain { item[kSecUseKeychain as String] = keychain }
            item[kSecValueData as String] = Data(value.utf8)
            guard SecItemAdd(item as CFDictionary, nil) == errSecSuccess else {
                throw ConfigurationError.credentialUnavailable
            }
        } else if status != errSecSuccess {
            throw ConfigurationError.credentialUnavailable
        }
    }

    public func delete(account: String) throws {
        let status = SecItemDelete(query(account: account) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw ConfigurationError.credentialUnavailable
        }
    }
}
