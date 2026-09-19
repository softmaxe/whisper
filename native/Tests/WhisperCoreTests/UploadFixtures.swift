import AVFoundation
import Foundation
import Testing
import WhisperCore

enum UploadFixtures {
    static var ffmpeg: URL {
        get throws {
            let repository = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            let candidates = [ProcessInfo.processInfo.environment["WHISPER_TEST_FFMPEG"].map { URL(fileURLWithPath: $0) },
                repository.appendingPathComponent("resources/bin/ffmpeg"), repository.appendingPathComponent("node_modules/ffmpeg-static/ffmpeg")].compactMap { $0 }
            return try #require(candidates.first { FileManager.default.isExecutableFile(atPath: $0.path) }, "Build native media tools or set WHISPER_TEST_FFMPEG before running Upload tests.")
        }
    }

    static let encoders: [String: [String]] = [
        "mp3": ["libmp3lame", "mp3"], "wav": ["pcm_s16le", "wav"], "m4a": ["aac", "mp4"],
        "webm": ["opus", "webm"], "ogg": ["flac", "ogg"], "oga": ["flac", "ogg"],
        "flac": ["flac", "flac"], "aac": ["aac", "adts"], "opus": ["opus", "opus"],
        "mpeg": ["mp2", "mpeg"], "mpg": ["mp2", "mpeg"], "mpga": ["libmp3lame", "mp3"],
        "mp2": ["mp2", "mp2"], "mp4": ["aac", "mp4"], "m4v": ["aac", "mp4"], "m4b": ["aac", "mp4"],
        "mov": ["aac", "mov"], "mkv": ["aac", "matroska"], "mka": ["aac", "matroska"],
        "3gp": ["aac", "3gp"], "avi": ["libmp3lame", "avi"], "wmv": ["wmav2", "asf"], "wma": ["wmav2", "asf"],
        "aiff": ["pcm_s16be", "aiff"], "aif": ["pcm_s16be", "aiff"], "aifc": ["pcm_s16le", "aiff"],
        "caf": ["pcm_s16le", "caf"], "ac3": ["ac3", "ac3"], "au": ["pcm_s16be", "au"],
        "snd": ["pcm_s16be", "au"], "wv": ["wavpack", "wv"]
    ]

    static func make(_ ext: String, in directory: URL, video: Bool = false) throws -> URL {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let file = directory.appendingPathComponent("synthetic." + ext)
        if ext == "amr" {
            // FFmpeg libavformat/amr.c describes 13-byte AMR-NB mode-0 frames.
            var bytes = Data("#!AMR\n".utf8)
            var frame = [UInt8](repeating: 0, count: 13); frame[0] = 4
            for _ in 0..<50 { bytes.append(contentsOf: frame) }
            try bytes.write(to: file)
        } else if ext == "ape" {
            try apeSilence().write(to: file)
            try run(["-err_detect", "crccheck+explode", "-i", file.path, "-f", "null", "-"])
        } else {
            let encoder = try #require(encoders[ext])
            var arguments: [String] = []
            if video { arguments += ["-f", "lavfi", "-i", "color=c=black:s=64x64:r=10:d=0.25"] }
            arguments += ["-f", "lavfi", "-i", "sine=frequency=440:duration=0.25", "-c:a", encoder[0]]
            if encoder[0] == "opus" { arguments += ["-strict", "-2", "-ar", "48000"] }
            if video { arguments += ["-c:v", "mpeg4", "-shortest"] }
            arguments += ["-f", encoder[1], "-y", file.path]
            try run(arguments)
        }
        return file
    }

    static func run(_ arguments: [String]) throws {
        let process = Process()
        process.executableURL = try ffmpeg
        process.arguments = ["-nostdin", "-hide_banner", "-loglevel", "error"] + arguments
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run(); process.waitUntilExit()
        #expect(process.terminationStatus == 0, "The synthetic media fixture must decode or encode successfully.")
    }

    // One second of 16-bit mono silence. The frame's CRC also passes FFmpeg crccheck+explode.
    // Layout and silence flag: FFmpeg n6.0 libavformat/ape.c and libavcodec/apedec.c.
    private static func apeSilence() -> Data {
        var data = Data(repeating: 0, count: 92)
        func put16(_ value: UInt16, at offset: Int) {
            for index in 0..<2 { data[offset + index] = UInt8(truncatingIfNeeded: value >> (index * 8)) }
        }
        func put32(_ value: UInt32, at offset: Int) {
            for index in 0..<4 { data[offset + index] = UInt8(truncatingIfNeeded: value >> (index * 8)) }
        }
        data.replaceSubrange(0..<4, with: Data("MAC ".utf8))
        put16(3990, at: 4); put32(52, at: 8); put32(24, at: 12); put32(4, at: 16)
        put32(12, at: 24); put16(2000, at: 52); put32(73728, at: 56); put32(16000, at: 60)
        put32(1, at: 64); put16(16, at: 68); put16(1, at: 70); put32(16000, at: 72); put32(80, at: 76)
        var crc: UInt32 = .max
        for _ in 0..<32000 { for _ in 0..<8 { crc = (crc >> 1) ^ (crc & 1 == 1 ? 0xedb88320 : 0) } }
        put32(((~crc) >> 1) | 0x80000000, at: 80)
        put32(1, at: 84)
        return data
    }
}

actor ControlledUploadTransport: FileHTTPTransport {
    private let forwarding: (any FileHTTPTransport)?
    struct Request: Sendable {
        let request: URLRequest
        let body: Data
        let audio: Data
        let bodyFile: URL
        let directoryMode: Int
        let fileMode: Int
    }
    var immediate = false
    private(set) var requests: [Request] = []
    private var replies: [Int: CheckedContinuation<HTTPResponse, any Error>] = [:]
    init(forwarding: (any FileHTTPTransport)? = nil) { self.forwarding = forwarding }
    func setImmediate() { immediate = true }
    func upload(_ request: URLRequest, file: URL) async throws -> HTTPResponse {
        let data = try Data(contentsOf: file)
        let mode = try FileManager.default.attributesOfItem(atPath: file.path)[.posixPermissions] as? NSNumber
        let directoryMode = try FileManager.default.attributesOfItem(atPath: file.deletingLastPathComponent().path)[.posixPermissions] as? NSNumber
        let index = requests.count
        requests.append(Request(request: request, body: data, audio: try multipartAudio(request: request, body: data), bodyFile: file,
            directoryMode: directoryMode?.intValue ?? 0, fileMode: mode?.intValue ?? 0))
        if let forwarding { return try await forwarding.upload(request, file: file) }
        if immediate { return HTTPResponse(status: 200, body: Data("{\"text\":\"Synthetic raw transcript\"}".utf8)) }
        return try await withCheckedThrowingContinuation { replies[index] = $0 }
    }
    func reply(_ index: Int = 0, status: Int = 200, text: String = "Synthetic raw transcript") throws {
        let body = try JSONSerialization.data(withJSONObject: ["text": text])
        replies.removeValue(forKey: index)?.resume(returning: HTTPResponse(status: status, body: body))
    }
}

actor ForbiddenUploadCleanup: CleanupService {
    private(set) var calls = 0
    func clean(text: String, configuration: CleanupConfiguration, credential: String?, context: CleanupContext) async throws -> String {
        calls += 1
        return "Upload must not use cleanup"
    }
}

actor RecordingUploadConverter: UploadMediaConverter {
    let converter: FFmpegUploadConverter
    private(set) var destinations: [URL] = []
    init(executable: URL) { converter = FFmpegUploadConverter(executable: executable) }
    func convert(_ source: URL, to destination: URL) async throws {
        destinations.append(destination)
        try await converter.convert(source, to: destination)
    }
}

@MainActor final class UploadFixture {
    let profile: ProfileFixture
    let microphones = ControlledMicrophones()
    let clipboard = ControlledClipboard()
    let paste = ControlledPasteSystem()
    let clock = ControlledClock()
    let transport = ControlledUploadTransport()
    let cleanup = ForbiddenUploadCleanup()
    let converter: RecordingUploadConverter
    let app: WhisperApplication
    var sourceDirectory: URL { profile.root.appendingPathComponent("Sources") }
    init(keychain: Bool = false, converter customConverter: (any UploadMediaConverter)? = nil, clipboard customClipboard: (any TextClipboard)? = nil) throws {
        profile = try ProfileFixture(keychain: keychain)
        converter = try RecordingUploadConverter(executable: UploadFixtures.ffmpeg)
        app = WhisperApplication(profile: profile.profile, credentials: profile.credentials, microphones: microphones,
            transcriber: SelfHostedTranscriber(transport: transport), clock: clock, clipboard: customClipboard ?? clipboard,
            pasteSystem: paste, cleanup: cleanup, uploadConverter: customConverter ?? converter)
        app.send(.saveASR(.init(serverURL: "http://localhost:8178/v1?upload=fixture", model: "upload-fixture"), credential: keychain ? .replace("upload-placeholder") : .unchanged))
    }
    func start(_ source: URL) {
        app.send(.selectUpload(source)); app.send(.startUpload)
    }
    func completed() async {
        await settle { self.app.state.upload.phase == .complete && !self.app.state.upload.isSavingHistory }
        await app.flushHistoryWrites()
        await settle { !self.app.state.history.isLoading && !self.app.state.history.isSearching }
    }
    func remove() { app.send(.cancelUpload); profile.remove() }
}
