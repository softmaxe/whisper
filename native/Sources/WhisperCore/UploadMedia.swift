import Darwin
import Foundation

public enum UploadFormats {
    // Matches src/constants/uploadAudioFormats.json and OpenWhispr providerUploadAudio.js.
    public static let extensions = [
        "mp3", "wav", "m4a", "webm", "ogg", "oga", "flac", "aac", "opus", "mpeg", "mpg",
        "mpga", "mp2", "mp4", "m4v", "m4b", "mov", "mkv", "mka", "3gp", "avi", "wmv",
        "wma", "aiff", "aif", "aifc", "caf", "amr", "ac3", "au", "snd", "wv", "ape"
    ]
    public static let directMIMETypes = [
        "mp3": "audio/mpeg", "wav": "audio/wav", "m4a": "audio/mp4", "webm": "audio/webm",
        "ogg": "audio/ogg", "oga": "audio/ogg", "flac": "audio/flac", "aac": "audio/aac", "opus": "audio/ogg"
    ]
    public static func accepts(_ url: URL) -> Bool {
        url.isFileURL && extensions.contains(url.pathExtension.lowercased())
    }
}

struct PrivateUploadDirectory: Sendable {
    let url: URL
    init(prefix: String) throws {
        let candidate = FileManager.default.temporaryDirectory.appendingPathComponent(prefix + UUID().uuidString, isDirectory: true)
        do {
            try FileManager.default.createDirectory(at: candidate, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
            url = candidate
        } catch { throw UploadFailure.temporaryStorage }
    }
    func remove() { try? FileManager.default.removeItem(at: url) }
}

public protocol UploadMediaConverter: Sendable {
    func convert(_ source: URL, to destination: URL) async throws
}

public struct FFmpegUploadConverter: UploadMediaConverter {
    private let executable: URL
    public init(executable: URL? = nil) {
        self.executable = executable ?? Bundle.main.bundleURL.appendingPathComponent("Contents/Resources/bin/ffmpeg")
    }
    @concurrent public func convert(_ source: URL, to destination: URL) async throws {
        guard FileManager.default.isExecutableFile(atPath: executable.path) else { throw UploadFailure.converterUnavailable }
        let process = UploadConversionProcess()
        try await process.run(executable: executable, arguments: [
            "-nostdin", "-hide_banner", "-loglevel", "error", "-protocol_whitelist", "file,pipe",
            "-i", source.path, "-vn", "-map_metadata", "-1", "-c:a", "libmp3lame", "-b:a", "64k",
            "-ar", "16000", "-ac", "1", "-y", destination.path
        ])
        try Task.checkCancellation()
        guard let size = try? destination.resourceValues(forKeys: [.fileSizeKey]).fileSize, size > 0 else {
            throw UploadFailure.conversionFailed
        }
    }
}

private final class UploadConversionProcess: @unchecked Sendable {
    private let lock = NSLock()
    private let process = Process()
    private var cancelled = false

    func run(executable: URL, arguments: [String]) async throws {
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
                DispatchQueue.global(qos: .utility).async { [self] in
                    do {
                        try lock.withLock {
                            if cancelled { throw CancellationError() }
                            process.executableURL = executable
                            process.arguments = arguments
                            process.standardInput = FileHandle.nullDevice
                            process.standardOutput = FileHandle.nullDevice
                            // Decoder diagnostics can include private paths and embedded metadata.
                            process.standardError = FileHandle.nullDevice
                            do { try process.run() } catch { throw UploadFailure.converterUnavailable }
                        }
                        process.waitUntilExit()
                        try lock.withLock {
                            if cancelled { throw CancellationError() }
                            guard process.terminationReason == .exit && process.terminationStatus == 0 else { throw UploadFailure.conversionFailed }
                        }
                        continuation.resume()
                    } catch { continuation.resume(throwing: error) }
                }
            }
        } onCancel: { self.cancel() }
    }

    private func cancel() {
        lock.withLock {
            cancelled = true
            if process.isRunning { kill(process.processIdentifier, SIGKILL) }
        }
    }
}

struct PreparedUpload: Sendable {
    let file: URL
    let sourceSize: Int64
    @concurrent static func prepare(_ source: URL, directory: URL, converter: any UploadMediaConverter) async throws -> Self {
        try Task.checkCancellation()
        guard UploadFormats.accepts(source) else { throw UploadFailure.unsupportedFormat }
        let values: URLResourceValues
        do { values = try source.resourceValues(forKeys: [.isRegularFileKey, .isReadableKey, .fileSizeKey]) }
        catch { throw UploadFailure.unreadableFile }
        guard values.isRegularFile == true, values.isReadable == true, let size = values.fileSize, size > 0 else { throw UploadFailure.unreadableFile }
        if UploadFormats.directMIMETypes[source.pathExtension.lowercased()] != nil {
            return Self(file: source, sourceSize: Int64(size))
        }
        let destination = directory.appendingPathComponent("audio.mp3")
        try await converter.convert(source, to: destination)
        try Task.checkCancellation()
        return Self(file: destination, sourceSize: Int64(size))
    }
}
