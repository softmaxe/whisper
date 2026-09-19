import Foundation
import Network

/// A real loopback listener exercises URLSession file upload, including HTTP and redirects.
final class LocalHTTPServer: @unchecked Sendable {
    private let listener: NWListener
    private let queue = DispatchQueue(label: "local.whisper.tests.http")
    private let lock = NSLock()
    private var storedRequests: [Data] = []
    private var connections: [NWConnection] = []
    let response: String
    let redirectPath: String?
    let holdResponses: Bool
    var requests: [Data] { lock.withLock { storedRequests } }
    var port: UInt16 { listener.port!.rawValue }

    init(status: Int = 200, headers: String = "", redirectPath: String? = nil, body: String = "{\"text\":\"Loopback HTTP transcript\"}", holdResponses: Bool = false) throws {
        self.redirectPath = redirectPath
        self.holdResponses = holdResponses
        listener = try NWListener(using: .tcp, on: .any)
        response = "HTTP/1.1 \(status) Test\r\nContent-Type: application/json\r\nContent-Length: \(body.utf8.count)\r\n\(headers)Connection: close\r\n\r\n\(body)"
    }
    func start() async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
            listener.stateUpdateHandler = { state in
                switch state {
                case .ready: continuation.resume()
                case let .failed(error): continuation.resume(throwing: error)
                default: break
                }
            }
            listener.newConnectionHandler = { [weak self] connection in
                guard let self else { connection.cancel(); return }
                self.lock.withLock { self.connections.append(connection) }
                connection.start(queue: self.queue)
                self.read(connection, received: Data())
            }
            listener.start(queue: queue)
        }
        listener.stateUpdateHandler = nil
    }
    func stop() {
        listener.cancel()
        lock.withLock { connections }.forEach { $0.cancel() }
    }
    private func read(_ connection: NWConnection, received: Data) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 65536) { [weak self] chunk, _, done, error in
            guard let self else { return }
            var data = received
            if let chunk { data.append(chunk) }
            if let headerEnd = data.range(of: Data("\r\n\r\n".utf8)) {
                let header = String(decoding: data[..<headerEnd.lowerBound], as: UTF8.self)
                let length = header.components(separatedBy: "\r\n").first { $0.lowercased().hasPrefix("content-length:") }
                    .flatMap { Int($0.split(separator: ":", maxSplits: 1)[1].trimmingCharacters(in: .whitespaces)) } ?? 0
                if data.count - headerEnd.upperBound >= length {
                    let count = self.lock.withLock {
                        self.storedRequests.append(data)
                        return self.storedRequests.count
                    }
                    let response: String
                    if self.holdResponses { return }
                    if count == 1, let path = self.redirectPath {
                        response = "HTTP/1.1 307 Temporary Redirect\r\nLocation: \(path)\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    } else { response = self.response }
                    connection.send(content: Data(response.utf8), completion: .contentProcessed { _ in connection.cancel() })
                    return
                }
            }
            if !done, error == nil { self.read(connection, received: data) }
        }
    }
}
