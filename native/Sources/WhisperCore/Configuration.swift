import Foundation

public enum AppLanguage: String, Codable, CaseIterable, Sendable {
    case english = "en"
    case simplifiedChinese = "zh-CN"

    public var displayName: String {
        switch self {
        case .english: "English"
        case .simplifiedChinese: "简体中文"
        }
    }

    public func text(_ english: String, _ chinese: String) -> String {
        self == .simplifiedChinese ? chinese : english
    }
}

public struct ASRConfiguration: Codable, Equatable, Sendable {
    public var serverURL: String
    public var model: String

    public init(serverURL: String = "", model: String = "") {
        self.serverURL = serverURL
        self.model = model
    }

    public func validated() throws -> Self {
        var result = Self(
            serverURL: serverURL.trimmingCharacters(in: .whitespacesAndNewlines),
            model: model.trimmingCharacters(in: .whitespacesAndNewlines)
        )
        result.serverURL = try EndpointPolicy.normalizedURL(result.serverURL)
        guard !result.model.isEmpty else { throw ConfigurationError.modelRequired }
        return result
    }
}

public struct AppSettings: Codable, Equatable, Sendable {
    public var language: AppLanguage
    public var cleanup: CleanupConfiguration
    public var cleanupCredentialAccount: String?
    public var asr: ASRConfiguration
    // The opaque account is persisted; the credential itself exists only in Keychain.
    public var asrCredentialAccount: String?

    public init(
        language: AppLanguage = .english,
        asr: ASRConfiguration = .init(),
        asrCredentialAccount: String? = nil,
        cleanup: CleanupConfiguration = .init(),
        cleanupCredentialAccount: String? = nil
    ) {
        self.cleanup = cleanup
        self.cleanupCredentialAccount = cleanupCredentialAccount
        self.language = language
        self.asr = asr
        self.asrCredentialAccount = asrCredentialAccount
    }
    private enum CodingKeys: String, CodingKey { case language, asr, asrCredentialAccount, cleanup, cleanupCredentialAccount }
    public init(from decoder: any Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        language = try values.decodeIfPresent(AppLanguage.self, forKey: .language) ?? .english
        asr = try values.decodeIfPresent(ASRConfiguration.self, forKey: .asr) ?? .init()
        asrCredentialAccount = try values.decodeIfPresent(String.self, forKey: .asrCredentialAccount)
        cleanup = try values.decodeIfPresent(CleanupConfiguration.self, forKey: .cleanup) ?? .init()
        cleanupCredentialAccount = try values.decodeIfPresent(String.self, forKey: .cleanupCredentialAccount)
    }
}

public enum CredentialChange: Sendable {
    case unchanged
    case replace(String)
    case remove
}

public enum ConfigurationError: Error, Equatable, Sendable {
    case invalidCleanupOptions
    case invalidURL
    case insecureURL
    case embeddedCredential
    case modelRequired
    case credentialUnavailable
    case persistenceFailed
    case incompatibleProfile

    public func message(in language: AppLanguage) -> String {
        switch self {
        case .invalidCleanupOptions:
            language.text("Use a temperature between 0 and 2 and a positive token limit.", "温度应在 0 到 2 之间，token 上限必须为正整数。")
        case .invalidURL:
            language.text("Enter a valid HTTP or HTTPS server URL.", "请输入有效的 HTTP 或 HTTPS 服务器地址。")
        case .insecureURL:
            language.text("Public servers require HTTPS. HTTP is available for local and private hosts.", "公网服务器必须使用 HTTPS。本机和私有网络服务器可以使用 HTTP。")
        case .embeddedCredential:
            language.text("Remove credentials from the URL and use the API Key field.", "请移除地址中的凭据，并使用 API Key 字段。")
        case .modelRequired:
            language.text("Enter the transcription model name.", "请输入转录模型名称。")
        case .credentialUnavailable:
            language.text("The credential could not be accessed in Keychain. Unlock your login keychain and try again.", "无法访问钥匙串中的凭据。请解锁登录钥匙串后重试。")
        case .persistenceFailed:
            language.text("Settings could not be saved. Check that the native profile is writable and try again.", "无法保存设置。请检查原生配置目录是否可写，然后重试。")
        case .incompatibleProfile:
            language.text("The native profile could not be read. Your saved files have been preserved.", "无法读取原生配置。已保留您的所有文件。")
        }
    }
}

public enum EndpointPolicy {
    // Ported from OpenWhispr src/utils/urlUtils.ts at 6d56d75 and this fork.
    public static func validate(_ value: String) throws {
        _ = try normalizedURL(value)
    }

    static func normalizedURL(_ value: String) throws -> String {
        guard var parts = URLComponents(string: value),
              let scheme = parts.scheme?.lowercased(),
              ["http", "https"].contains(scheme),
              let host = parts.host?.lowercased(), !host.isEmpty,
              parts.url != nil,
              !host.contains(where: { $0.isWhitespace }),
              parts.port.map({ (1...65535).contains($0) }) ?? true
        else { throw ConfigurationError.invalidURL }
        guard parts.user == nil, parts.password == nil else {
            throw ConfigurationError.embeddedCredential
        }
        let address = try ipv4Address(host)
        let canonicalHost = address.map { address in
            [24, 16, 8, 0].map { String((address >> $0) & 255) }.joined(separator: ".")
        } ?? host
        guard scheme == "https" || isPrivateHost(canonicalHost) else {
            throw ConfigurationError.insecureURL
        }
        // Save the checked numeric destination so URLSession need not interpret legacy literal forms.
        if address != nil, canonicalHost != host {
            parts.host = canonicalHost
            guard let normalized = parts.string else { throw ConfigurationError.invalidURL }
            return normalized
        }
        return value
    }

    // WHATWG host parsing recognizes decimal, octal, hexadecimal and one-to-four-part IPv4.
    // A numeric final label must parse as IPv4; malformed literals cannot fall through as DNS.
    // https://url.spec.whatwg.org/#concept-ipv4-parser
    private static func ipv4Address(_ host: String) throws -> UInt64? {
        guard !host.contains(":") else { return nil }
        var parts = host.split(separator: ".", omittingEmptySubsequences: false)
        if parts.count > 1, parts.last?.isEmpty == true { parts.removeLast() }
        guard let last = parts.last else { return nil }
        let endsInDecimal = !last.isEmpty && last.utf8.allSatisfy { (48...57).contains($0) }
        let endsInHex = last.hasPrefix("0x") && last.dropFirst(2).utf8.allSatisfy {
            (48...57).contains($0) || (97...102).contains($0)
        }
        guard endsInDecimal || endsInHex else { return nil }
        guard parts.count <= 4 else { throw ConfigurationError.invalidURL }
        let numbers = parts.compactMap(ipv4Number)
        guard numbers.count == parts.count,
              numbers.dropLast().allSatisfy({ $0 <= 255 }),
              let final = numbers.last,
              final < UInt64(1) << (8 * (5 - numbers.count)) else {
            throw ConfigurationError.invalidURL
        }
        return numbers.dropLast().enumerated().reduce(final) { value, part in
            value + (part.element << (8 * (3 - part.offset)))
        }
    }

    private static func ipv4Number(_ part: Substring) -> UInt64? {
        guard !part.isEmpty else { return nil }
        var digits = part
        var radix = 10
        if digits.hasPrefix("0x") {
            digits = digits.dropFirst(2)
            radix = 16
        } else if digits.count > 1, digits.first == "0" {
            digits = digits.dropFirst()
            radix = 8
        }
        if digits.isEmpty { return 0 }
        guard digits.utf8.allSatisfy({ byte in
            let digit = (48...57).contains(byte) ? Int(byte - 48)
                : (97...102).contains(byte) ? Int(byte - 97) + 10 : radix
            return digit < radix
        }) else { return nil }
        return UInt64(digits, radix: radix)
    }

    private static func isPrivateHost(_ host: String) -> Bool {
        let host = host.trimmingCharacters(in: CharacterSet(charactersIn: "[]"))
        if ["localhost", "0.0.0.0", "::1"].contains(host) { return true }
        if host.hasSuffix(".local") || host.hasSuffix(".ts.net") { return true }
        let parts = host.split(separator: ".", omittingEmptySubsequences: false)
        let octets = parts.compactMap { part -> Int? in
            guard !part.isEmpty, part == "0" || !part.hasPrefix("0"),
                  part.allSatisfy({ $0.isASCII && $0.isNumber }),
                  let value = Int(part), value <= 255 else { return nil }
            return value
        }
        if parts.count == 4, octets.count == 4 {
            let (a, b) = (octets[0], octets[1])
            return a == 127 || a == 10 || (a == 192 && b == 168)
                || (a == 172 && (16...31).contains(b))
                || (a == 100 && (64...127).contains(b)) || (a == 169 && b == 254)
        }
        if host.contains(":"), let first = host.split(separator: ":").first,
           let prefix = UInt16(first, radix: 16) {
            return prefix & 0xffc0 == 0xfe80 || prefix & 0xfe00 == 0xfc00
        }
        return false
    }
}
