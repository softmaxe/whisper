import Foundation

public struct TranscriptionOptions: Sendable {
    public var language: String?
    public var prompt: String?
    public var diagnostics: RequestDiagnostics?
    public init(language: String? = nil, prompt: String? = nil) { self.language = language; self.prompt = prompt }
}

public protocol TranscriptionService: Sendable {
    func transcribe(file: URL, configuration: ASRConfiguration, credential: String?, options: TranscriptionOptions) async throws -> String
}

public struct HTTPResponse: Sendable {
    public let status: Int
    public let body: Data
    public init(status: Int, body: Data) { self.status = status; self.body = body }
}

public protocol FileHTTPTransport: Sendable {
    func upload(_ request: URLRequest, file: URL) async throws -> HTTPResponse
    func upload(_ request: URLRequest, file: URL, diagnostics: NetworkDiagnostics?) async throws -> HTTPResponse
}
public extension FileHTTPTransport {
    func upload(_ request: URLRequest, file: URL, diagnostics: NetworkDiagnostics?) async throws -> HTTPResponse {
        try await observeHTTP(diagnostics) { try await upload(request, file: file) }
    }
}

/// Redirects may normalize paths within the configured origin; audio and credentials cannot leave it.
public final class URLSessionFileTransport: NSObject, FileHTTPTransport, URLSessionTaskDelegate, @unchecked Sendable {
    public override init() { super.init() }
    public func upload(_ request: URLRequest, file: URL) async throws -> HTTPResponse {
        try await upload(request, file: file, diagnostics: nil)
    }
    public func upload(_ request: URLRequest, file: URL, diagnostics: NetworkDiagnostics?) async throws -> HTTPResponse {
        let configuration = URLSessionConfiguration.ephemeral
        // Match the self-hosted fetch contract: cancellation is explicit; long inference has no client deadline.
        configuration.timeoutIntervalForRequest = .greatestFiniteMagnitude
        configuration.timeoutIntervalForResource = .greatestFiniteMagnitude
        configuration.httpCookieStorage = nil
        configuration.urlCache = nil
        let session = URLSession(configuration: configuration, delegate: self, delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        return try await observeHTTP(diagnostics) {
            do {
                let (data, response) = try await session.upload(for: request, fromFile: file)
                guard let response = response as? HTTPURLResponse else { throw DictationFailure.invalidResponse }
                return HTTPResponse(status: response.statusCode, body: data)
            } catch is CancellationError { throw CancellationError() }
            catch let error as URLError where error.code == .cancelled { throw CancellationError() }
            catch let error as DictationFailure { throw error }
            catch { throw DictationFailure.network }
        }
    }
    public func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest, completionHandler: @escaping @Sendable (URLRequest?) -> Void) {
        guard let origin = task.originalRequest?.url, let destination = request.url,
              (try? EndpointPolicy.validate(destination.absoluteString)) != nil,
              Self.sameOrigin(origin, destination) else {
            completionHandler(nil)
            return
        }
        var redirected = request
        // URLSession can strip Authorization even for a same-origin 307. Restore it only after origin validation.
        redirected.setValue(task.originalRequest?.value(forHTTPHeaderField: "Authorization"), forHTTPHeaderField: "Authorization")
        completionHandler(redirected)
    }

    private static func sameOrigin(_ first: URL, _ second: URL) -> Bool {
        func port(_ url: URL) -> Int { url.port ?? (url.scheme?.lowercased() == "https" ? 443 : 80) }
        return first.scheme?.lowercased() == second.scheme?.lowercased()
            && first.host?.lowercased() == second.host?.lowercased() && port(first) == port(second)
    }
}

public struct SelfHostedTranscriber: TranscriptionService {
    private let transport: any FileHTTPTransport
    public init(transport: any FileHTTPTransport = URLSessionFileTransport()) { self.transport = transport }

    public func transcribe(file: URL, configuration: ASRConfiguration, credential: String?, options: TranscriptionOptions) async throws -> String {
        let configuration = try configuration.validated()
        let url = try Self.endpoint(configuration)
        let boundary = "Whisper-" + UUID().uuidString
        let body = file.deletingLastPathComponent().appendingPathComponent("multipart-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: body) }
        // File preparation runs on Swift's generic executor, never the main actor.
        try Self.writeMultipart(audio: file, body: body, boundary: boundary, configuration: configuration, options: options)
        try Task.checkCancellation()
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("multipart/form-data; boundary=" + boundary, forHTTPHeaderField: "Content-Type")
        if let credential, !credential.isEmpty { request.setValue("Bearer " + credential, forHTTPHeaderField: "Authorization") }
        let response = try await transport.upload(request, file: body, diagnostics: options.diagnostics?.network(.asr))
        try Task.checkCancellation()
        guard (200...299).contains(response.status) else { throw DictationFailure.service(response.status) }
        struct Result: Decodable { let text: String }
        guard let result = try? JSONDecoder().decode(Result.self, from: response.body) else { throw DictationFailure.invalidResponse }
        guard !result.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { throw DictationFailure.emptyTranscript }
        options.diagnostics?.mark(.asrResponseCompleted)
        return result.text
    }

    private static func endpoint(_ configuration: ASRConfiguration) throws -> URL {
        guard var components = URLComponents(string: configuration.serverURL) else { throw DictationFailure.configuration }
        var path = components.path
        while path.hasSuffix("/") { path.removeLast() }
        for suffix in ["/audio/transcriptions", "/audio/translations", "/chat/completions", "/completions", "/models"] {
            if path.hasSuffix(suffix) { path.removeLast(suffix.count); break }
        }
        components.path = path + "/audio/transcriptions"
        components.fragment = nil
        guard let url = components.url else { throw DictationFailure.configuration }
        return url
    }

    private static func writeMultipart(audio: URL, body: URL, boundary: String, configuration: ASRConfiguration, options: TranscriptionOptions) throws {
        guard FileManager.default.createFile(atPath: body.path, contents: nil, attributes: [.posixPermissions: 0o600]) else { throw DictationFailure.storageFailed }
        let output = try FileHandle(forWritingTo: body)
        defer { try? output.close() }
        func write(_ text: String) throws { try output.write(contentsOf: Data(text.utf8)) }
        func field(_ name: String, _ value: String) throws {
            try write("--\(boundary)\r\nContent-Disposition: form-data; name=\"\(name)\"\r\n\r\n\(value)\r\n")
        }
        try field("model", configuration.model)
        if let language = options.language, language != "auto", !language.isEmpty { try field("language", String(language.split(separator: "-")[0])) }
        if let prompt = options.prompt, !prompt.isEmpty { try field("prompt", prompt) }
        let ext = audio.pathExtension.lowercased()
        let mime = ["m4a": "audio/mp4", "wav": "audio/wav", "mp3": "audio/mpeg", "flac": "audio/flac", "ogg": "audio/ogg", "webm": "audio/webm", "mp4": "video/mp4"][ext] ?? "application/octet-stream"
        try write("--\(boundary)\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.\(ext)\"\r\nContent-Type: \(mime)\r\n\r\n")
        let input = try FileHandle(forReadingFrom: audio)
        defer { try? input.close() }
        while let chunk = try input.read(upToCount: 256 * 1024), !chunk.isEmpty {
            try Task.checkCancellation()
            try output.write(contentsOf: chunk)
        }
        try write("\r\n--\(boundary)--\r\n")
    }
}
