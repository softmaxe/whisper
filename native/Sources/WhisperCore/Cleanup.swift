import Foundation

public struct CleanupConfiguration: Codable, Equatable, Sendable {
    public var enabled: Bool
    public var serverURL: String
    public var model: String
    public var customPrompt: String?
    public var disableThinking: Bool
    public var temperature: Double?
    public var maxTokens: Int?

    public init(enabled: Bool = true, serverURL: String = "", model: String = "", customPrompt: String? = nil,
                disableThinking: Bool = true, temperature: Double? = nil, maxTokens: Int? = nil) {
        self.enabled = enabled
        self.serverURL = serverURL
        self.model = model
        self.customPrompt = customPrompt
        self.disableThinking = disableThinking
        self.temperature = temperature
        self.maxTokens = maxTokens
    }

    public func validated() throws -> Self {
        var copy = self
        copy.serverURL = serverURL.trimmingCharacters(in: .whitespacesAndNewlines)
        if enabled || !copy.serverURL.isEmpty { copy.serverURL = try EndpointPolicy.normalizedURL(copy.serverURL) }
        copy.model = model.trimmingCharacters(in: .whitespacesAndNewlines)
        if customPrompt?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == true { copy.customPrompt = nil }
        guard temperature.map({ $0.isFinite && (0...2).contains($0) }) ?? true,
              maxTokens.map({ $0 > 0 }) ?? true else { throw ConfigurationError.invalidCleanupOptions }
        return copy
    }
}

public struct CleanupContext: Sendable {
    public var language: String
    public var vocabulary: [String]
    public var agentName: String
    public var systemPromptOverride: String?
    public var interfaceLanguage: AppLanguage
    public var diagnostics: RequestDiagnostics?
    public init(language: String = "auto", vocabulary: [String] = [], interfaceLanguage: AppLanguage = .english, systemPromptOverride: String? = nil, agentName: String = "OpenWhispr") {
        self.agentName = agentName
        self.systemPromptOverride = systemPromptOverride
        self.language = language
        self.vocabulary = vocabulary
        self.interfaceLanguage = interfaceLanguage
    }
}

public enum CleanupFailure: Error, Equatable, Sendable {
    case configuration, network, timeout, service(Int), invalidResponse, emptyResponse, truncated
    public func message(in language: AppLanguage) -> String {
        switch self {
        case .configuration: language.text("Check your text cleanup settings and API key.", "请检查文本整理设置和 API key。")
        case .network: language.text("The text cleanup server could not be reached.", "无法连接文本整理服务器。")
        case .timeout: language.text("Text cleanup timed out after 30 seconds.", "文本整理在 30 秒后超时。")
        case let .service(status): language.text("The text cleanup server returned HTTP \(status).", "文本整理服务器返回 HTTP \(status)。")
        case .invalidResponse: language.text("The text cleanup server returned an invalid response.", "文本整理服务器返回了无效响应。")
        case .emptyResponse: language.text("Text cleanup returned no text.", "文本整理没有返回文字。")
        case .truncated: language.text("Text cleanup was cut off before it finished.", "文本整理结果被截断。")
        }
    }
    var mayRetry: Bool {
        switch self {
        case .network: true
        case let .service(status): status == 408 || status == 429 || (500...599).contains(status)
        default: false
        }
    }
}

public protocol JSONHTTPTransport: Sendable {
    func send(_ request: URLRequest) async throws -> HTTPResponse
    func send(_ request: URLRequest, diagnostics: NetworkDiagnostics?) async throws -> HTTPResponse
}
public extension JSONHTTPTransport {
    func send(_ request: URLRequest, diagnostics: NetworkDiagnostics?) async throws -> HTTPResponse {
        try await observeHTTP(diagnostics) { try await send(request) }
    }
}

extension URLSessionFileTransport: JSONHTTPTransport {
    public func send(_ request: URLRequest) async throws -> HTTPResponse {
        try await send(request, diagnostics: nil)
    }
    public func send(_ request: URLRequest, diagnostics: NetworkDiagnostics?) async throws -> HTTPResponse {
        let configuration = URLSessionConfiguration.ephemeral
        // The application owns the cancellable 30-second deadline, including fallback attempts.
        configuration.timeoutIntervalForRequest = .greatestFiniteMagnitude
        configuration.timeoutIntervalForResource = .greatestFiniteMagnitude
        configuration.httpCookieStorage = nil
        configuration.urlCache = nil
        let session = URLSession(configuration: configuration, delegate: self, delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        return try await observeHTTP(diagnostics) {
            do {
                let (data, response) = try await session.data(for: request)
                guard let response = response as? HTTPURLResponse else { throw CleanupFailure.invalidResponse }
                return HTTPResponse(status: response.statusCode, body: data)
            } catch is CancellationError { throw CancellationError() }
            catch let error as URLError where error.code == .cancelled { throw CancellationError() }
            catch let error as CleanupFailure { throw error }
            catch { throw CleanupFailure.network }
        }
    }
}

public protocol CleanupService: Sendable {
    func clean(text: String, configuration: CleanupConfiguration, credential: String?, context: CleanupContext) async throws -> String
}

public struct SelfHostedCleanup: CleanupService {
    private let transport: any JSONHTTPTransport
    public init(transport: any JSONHTTPTransport = URLSessionFileTransport()) { self.transport = transport }

    public func clean(text: String, configuration input: CleanupConfiguration, credential: String?, context: CleanupContext) async throws -> String {
        let configuration: CleanupConfiguration
        do { configuration = try configurationForRequest(input) } catch { throw CleanupFailure.configuration }
        let endpoint = try Self.endpoint(configuration.serverURL)
        var body = CleanupRequest(configuration: configuration, context: context, text: text, endpoint: endpoint)
        var response = try await fetch(endpoint, body: body, credential: credential, diagnostics: context.diagnostics)
        if [400, 422].contains(response.status) {
            if body.reasoning != nil {
                body.reasoning = nil
                response = try await fetch(endpoint, body: body, credential: credential, diagnostics: context.diagnostics)
            }
            if [400, 422].contains(response.status), body.stripNamedParameters(String(decoding: response.body, as: UTF8.self)) {
                response = try await fetch(endpoint, body: body, credential: credential, diagnostics: context.diagnostics)
            }
        }
        guard (200...299).contains(response.status) else { throw CleanupFailure.service(response.status) }
        struct Reply: Decodable {
            struct Choice: Decodable {
                struct Message: Decodable { let content: String? }
                let message: Message?
                let finish_reason: String?
            }
            let choices: [Choice]
        }
        guard let reply = try? JSONDecoder().decode(Reply.self, from: response.body), let choice = reply.choices.first else {
            throw CleanupFailure.invalidResponse
        }
        guard !["length", "max_tokens"].contains(choice.finish_reason) else { throw CleanupFailure.truncated }
        let content = choice.message?.content?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let output = configuration.disableThinking ? Self.stripThinking(content) : content
        guard !output.isEmpty else { throw CleanupFailure.emptyResponse }
        return output
    }

    private func configurationForRequest(_ input: CleanupConfiguration) throws -> CleanupConfiguration {
        var copy = input
        copy.enabled = true
        return try copy.validated()
    }

    private func fetch(_ endpoint: URL, body: CleanupRequest, credential: String?, diagnostics: RequestDiagnostics?) async throws -> HTTPResponse {
        try Task.checkCancellation()
        var request = URLRequest(url: endpoint)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let credential = credential?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        if !credential.isEmpty { request.setValue("Bearer " + credential, forHTTPHeaderField: "Authorization") }
        request.httpBody = try JSONEncoder().encode(body)
        do {
            let response = try await transport.send(request, diagnostics: diagnostics?.network(.cleanup))
            try Task.checkCancellation()
            return response
        } catch is CancellationError { throw CancellationError() }
        catch let error as CleanupFailure { throw error }
        catch { throw CleanupFailure.network }
    }

    private static func endpoint(_ base: String) throws -> URL {
        guard var components = URLComponents(string: base) else { throw CleanupFailure.configuration }
        var path = normalizedServiceBasePath(components.percentEncodedPath)
        if !path.hasSuffix("/v1") { path += "/v1" }
        components.percentEncodedPath = path + "/chat/completions"
        components.fragment = nil
        guard let url = components.url else { throw CleanupFailure.configuration }
        return url
    }

    private static func stripThinking(_ text: String) -> String {
        var output = text
        let nested = #"(?i)<think>(?:(?!<think>)[\s\S])*?</think>"#
        while true {
            let next = output.replacingOccurrences(of: nested, with: "", options: .regularExpression)
            if next == output { break }
            output = next
        }
        return output.replacingOccurrences(of: #"(?i)<think>[\s\S]*$"#, with: "", options: .regularExpression)
            .replacingOccurrences(of: #"(?i)</think>"#, with: "", options: .regularExpression)
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

private struct CleanupRequest: Encodable {
    struct Message: Encodable { let role: String; let content: String }
    let model: String
    let messages: [Message]
    let max_tokens: Int
    var temperature: Double?
    var reasoning_effort: String?
    var reasoning: [String: String]?
    var thinking: [String: String]?
    var chat_template_kwargs: [String: Bool]?

    init(configuration: CleanupConfiguration, context: CleanupContext, text: String, endpoint: URL) {
        model = configuration.model.isEmpty ? "default" : configuration.model
        messages = [Message(role: "system", content: context.systemPromptOverride ?? CleanupPrompts.systemPrompt(configuration: configuration, context: context)),
                    Message(role: "user", content: context.systemPromptOverride == nil ? CleanupPrompts.wrap(text) : text)]
        // Current LAN cleanup has a 4096 floor above its 2048 calculated ceiling.
        max_tokens = configuration.maxTokens ?? 4096
        temperature = configuration.temperature ?? (context.systemPromptOverride == nil ? 0 : 0.3)
        let gptOSS = model.lowercased().contains("gpt-oss")
        if gptOSS { reasoning_effort = "low" }
        guard configuration.disableThinking, !Self.knownWithoutThinking.contains(model) else { return }
        let host = endpoint.host?.lowercased() ?? ""
        func matches(_ domain: String) -> Bool { host == domain || host.hasSuffix("." + domain) }
        if matches("mistral.ai") {
            if !model.lowercased().contains("magistral") { reasoning_effort = "none" }
        } else if matches("deepseek.com") {
            thinking = ["type": "disabled"]
        } else if matches("cerebras.ai") {
            if gptOSS { reasoning_effort = "low" }
            else if model.lowercased().contains("qwen") { reasoning_effort = "none" }
        } else {
            reasoning = ["effort": gptOSS ? "low" : "none"]
            chat_template_kwargs = ["enable_thinking": false]
        }
    }

    // Preserve existing registry exceptions for models that reject thinking controls.
    private static let knownWithoutThinking: Set<String> = [
        "gpt-6-astra",
        "gpt-5.6-sol",
        "gpt-5.6-terra",
        "gpt-5.6-luna",
        "gpt-5.5",
        "gpt-5.2",
        "gpt-5-mini",
        "gpt-5-nano",
        "gpt-4.1",
        "gpt-4.1-mini",
        "gpt-4.1-nano",
        "claude-fable-5-1",
        "claude-fable-5",
        "claude-sonnet-5",
        "claude-sonnet-4-6",
        "claude-haiku-4-5",
        "claude-opus-5",
        "claude-opus-4-8",
        "claude-opus-4-7",
        "claude-opus-4-6",
        "claude-sonnet-4-5",
        "claude-opus-4-5",
        "gemini-3.1-flash-lite",
        "gemini-2.5-flash-lite",
        "gemma-4-31b-it",
        "gemma-4-26b-a4b-it",
        "groq/compound",
        "groq/compound-mini",
        "corti-s1-instant",
        "corti-s1",
        "corti-s1-mini-instant",
        "corti-s1-mini",
        "us.anthropic.claude-fable-5",
        "us.anthropic.claude-haiku-4-5-20251001-v1:0",
        "us.anthropic.claude-sonnet-5",
        "us.anthropic.claude-opus-4-8",
        "openai.gpt-oss-120b-1:0",
        "deepseek.v3.2",
        "qwen.qwen3-next-80b-a3b",
        "gemini-2.5-flash",
        "gemini-2.5-pro",
        "qwen2.5-1.5b-instruct-q5_k_m",
        "qwen2.5-3b-instruct-q5_k_m",
        "qwen2.5-7b-instruct-q4_k_m",
        "qwen2.5-7b-instruct-q5_k_m",
        "mistral-nemo-12b-instruct-q4_k_m",
        "mistral-7b-instruct-v0.3-q4_k_m",
        "mistral-7b-instruct-v0.3-q5_k_m",
        "llama-3.2-1b-instruct-q4_k_m",
        "llama-3.2-3b-instruct-q4_k_m",
        "llama-3.1-8b-instruct-q4_k_m",
        "gemma-4-31b-it-q4_k_m",
        "gemma-4-31b-it-qat-q4_0",
        "gemma-4-26b-a4b-it-q4_k_m",
        "gemma-4-26b-a4b-it-qat-q4_0",
        "gemma-4-e4b-it-q4_k_m",
        "gemma-4-e4b-it-qat-q4_0",
        "gemma-4-e2b-it-q4_k_m",
        "gemma-4-e2b-it-qat-q4_0",
        "gemma-3-12b-it-q4_k_m",
        "gemma-3-4b-it-q4_k_m",
        "gemma-3-1b-it-q4_k_m",
        "lfm2.5-1.2b-instruct-q4_k_m",
        "lfm2.5-8b-a1b-q4_k_m",
        "lfm2-2.6b-q4_k_m",
        "lfm2.5-350m-q8_0",
        "lfm2.5-230m-q8_0"
    ]

    mutating func stripNamedParameters(_ error: String) -> Bool {
        var removed = false
        if reasoning_effort != nil, error.contains("reasoning_effort") { reasoning_effort = nil; removed = true }
        if chat_template_kwargs != nil, error.contains("chat_template_kwargs") { chat_template_kwargs = nil; removed = true }
        if thinking != nil, error.contains("thinking") { thinking = nil; removed = true }
        if temperature != nil, error.contains("temperature") { temperature = nil; removed = true }
        return removed
    }
}
