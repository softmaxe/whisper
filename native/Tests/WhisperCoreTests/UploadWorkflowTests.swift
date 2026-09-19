import AVFoundation
import Foundation
import Testing
import WhisperCore

@Suite(.serialized) @MainActor
struct UploadWorkflowTests {
    @Test(arguments: [
        "mp3", "wav", "m4a", "webm", "ogg", "oga", "flac", "aac", "opus", "mpeg", "mpg",
        "mpga", "mp2", "mp4", "m4v", "m4b", "mov", "mkv", "mka", "3gp", "avi", "wmv", "wma",
        "aiff", "aif", "aifc", "caf", "amr", "ac3", "au", "snd", "wv", "ape"
    ])
    func everyAcceptedFormatReachesASRWithRealMediaAndRawHistory(ext: String) async throws {
        let f = try UploadFixture()
        defer { f.remove() }
        let source = try UploadFixtures.make(ext, in: f.sourceDirectory, video: ext == "mp4")
        let original = try Data(contentsOf: source)
        if ext == "aifc" { #expect(original.subdata(in: 8..<12) == Data("AIFC".utf8)) }
        await f.transport.setImmediate()
        f.start(source)
        #expect(f.app.state.upload.phase == .preparing)
        await f.completed()
        let request = try #require(await f.transport.requests.first)
        let body = String(decoding: request.body, as: UTF8.self)
        let direct = UploadFormats.directMIMETypes[ext]
        #expect(body.contains("Content-Type: \(direct ?? "audio/mpeg")\r\n"))
        #expect(body.contains("filename=\"audio.\(direct == nil ? "mp3" : ext)\""))
        #expect(!body.contains("name=\"prompt\""))
        #expect(!body.contains("name=\"language\""))
        #expect(request.request.url?.absoluteString == "http://localhost:8178/v1/audio/transcriptions?upload=fixture")
        #expect(request.directoryMode == 0o700 && request.fileMode == 0o600)
        if direct != nil { #expect(request.audio == original) }
        else {
            let posted = f.profile.root.appendingPathComponent("posted.mp3")
            try request.audio.write(to: posted)
            let decoded = try AVAudioFile(forReading: posted)
            #expect(decoded.processingFormat.sampleRate == 16000)
            #expect(decoded.processingFormat.channelCount == 1)
            #expect(decoded.length > 1000)
        }
        #expect(try Data(contentsOf: source) == original)
        #expect(f.app.state.upload.text == "Synthetic raw transcript")
        #expect(f.app.state.history.entries.count == 1)
        let entry = try #require(f.app.state.history.entries.first)
        #expect(entry.text == entry.rawText && entry.source == .upload)
        #expect(entry.audioFileName == nil && entry.audioDuration == nil)
        #expect(f.app.state.dictation.phase == .idle)
        #expect(f.microphones.sessions.isEmpty && f.paste.pasted.isEmpty)
        #expect(!FileManager.default.fileExists(atPath: request.bodyFile.deletingLastPathComponent().path))
        for destination in await f.converter.destinations {
            #expect(!FileManager.default.fileExists(atPath: destination.deletingLastPathComponent().path))
        }
    }

    @Test(arguments: [true, false])
    func rawResultBypassesAllLiveTransformsAndRemainsCopyable(historyEnabled: Bool) async throws {
        let f = try UploadFixture()
        defer { f.remove() }
        f.app.send(.setHistoryEnabled(historyEnabled))
        f.app.send(.setTranscriptionLanguage("zh-TW"))
        f.app.send(.setChineseScriptPreference(.traditional))
        f.app.send(.saveCleanup(.init(enabled: true, serverURL: "http://localhost:8080", model: "cleanup-fixture"), credential: .unchanged))
        f.app.send(.importDictionary("dictation-only-hint, 软件, signature, 原始结果"))
        f.app.send(.saveSnippet(trigger: "signature", replacement: "Expanded signature"))
        let source = try UploadFixtures.make("wav", in: f.sourceDirectory)
        f.start(source)
        await settle { await f.transport.requests.count == 1 }
        let text = "  软件 signature\n原始结果  "
        try await f.transport.reply(text: text)
        await f.completed()
        #expect(f.app.state.upload.text == text)
        f.app.send(.copyUploadResult)
        #expect(f.clipboard.values == [text])
        #expect(f.app.state.upload.resultCopied)
        #expect(await f.cleanup.calls == 0)
        #expect(f.paste.pasted.isEmpty && f.paste.writes.isEmpty)
        let request = try #require(await f.transport.requests.first)
        let body = String(decoding: request.body, as: UTF8.self)
        #expect(body.contains("name=\"language\"\r\n\r\nzh\r\n"))
        #expect(!body.contains("name=\"prompt\"") && !body.contains("dictation-only-hint"))
        #expect(f.app.state.history.entries.count == (historyEnabled ? 1 : 0))
        let reopened = f.profile.open()
        reopened.send(.loadHistory)
        await settle { !reopened.state.history.isLoading }
        #expect(reopened.state.history.entries.first?.text == (historyEnabled ? text : nil))
    }

    @Test func readonlySourceNeverReceivesTransportOrConversionFiles() async throws {
        let f = try UploadFixture()
        defer { f.remove() }
        let source = try UploadFixtures.make("wav", in: f.sourceDirectory)
        let original = try Data(contentsOf: source)
        try FileManager.default.setAttributes([.posixPermissions: 0o444], ofItemAtPath: source.path)
        try FileManager.default.setAttributes([.posixPermissions: 0o555], ofItemAtPath: f.sourceDirectory.path)
        defer { try? FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: f.sourceDirectory.path) }
        await f.transport.setImmediate()
        f.start(source)
        await f.completed()
        #expect(try FileManager.default.contentsOfDirectory(atPath: f.sourceDirectory.path) == [source.lastPathComponent])
        #expect(try Data(contentsOf: source) == original)
        #expect(f.app.state.upload.failure == nil)
    }

    @Test func configurationAndCredentialAreSnapshottedForTheActiveJob() async throws {
        let f = try UploadFixture(keychain: true)
        defer { f.remove() }
        f.app.send(.setTranscriptionLanguage("en-US"))
        let source = try UploadFixtures.make("m4a", in: f.sourceDirectory)
        f.start(source)
        f.app.send(.saveASR(.init(serverURL: "http://localhost:9999/new", model: "next-model"), credential: .replace("next-placeholder")))
        f.app.send(.setTranscriptionLanguage("fr"))
        await settle { await f.transport.requests.count == 1 }
        let request = try #require(await f.transport.requests.first)
        #expect(request.request.url?.port == 8178)
        #expect(request.request.value(forHTTPHeaderField: "Authorization") == "Bearer upload-placeholder")
        let body = String(decoding: request.body, as: UTF8.self)
        #expect(body.contains("\r\nupload-fixture\r\n") && body.contains("\r\nen\r\n"))
        f.clock.advance(30)
        try await f.transport.reply()
        await f.completed()
        #expect(f.app.state.history.entries.first?.model == "upload-fixture")
        #expect(f.app.state.history.entries.first?.occurredAt == f.clock.wallDate)
    }

    @Test func cancellingASRDiscardsLateOutputAndCannotReplaceANewerJob() async throws {
        let f = try UploadFixture()
        defer { f.remove() }
        let source = try UploadFixtures.make("wav", in: f.sourceDirectory)
        f.start(source)
        await settle { await f.transport.requests.count == 1 }
        let old = try #require(await f.transport.requests.first)
        f.app.send(.cancelUpload)
        #expect(f.app.state.upload.phase == .idle)
        f.start(source)
        await settle { await f.transport.requests.count == 2 }
        try await f.transport.reply(0, text: "Cancelled old result")
        await settle { !FileManager.default.fileExists(atPath: old.bodyFile.deletingLastPathComponent().path) }
        #expect(f.app.state.upload.phase == .transcribing)
        #expect(f.app.state.history.entries.isEmpty)
        for destination in await f.converter.destinations {
            #expect(!FileManager.default.fileExists(atPath: destination.deletingLastPathComponent().path))
        }
        try await f.transport.reply(1, text: "New result")
        await f.completed()
        #expect(f.app.state.upload.text == "New result")
        #expect(f.app.state.history.entries.map(\.text) == ["New result"])
    }

    @Test func failedConversionDoesNotReachASROrDeleteTheSource() async throws {
        let f = try UploadFixture()
        defer { f.remove() }
        try FileManager.default.createDirectory(at: f.sourceDirectory, withIntermediateDirectories: false)
        let source = f.sourceDirectory.appendingPathComponent("invalid.mov")
        let data = Data("Synthetic invalid movie".utf8)
        try data.write(to: source)
        f.start(source)
        await settle { f.app.state.upload.phase == .failed }
        #expect(f.app.state.upload.failure == .conversionFailed)
        #expect(await f.transport.requests.isEmpty)
        #expect(try Data(contentsOf: source) == data)
        #expect(f.app.state.history.entries.isEmpty)
        for destination in await f.converter.destinations {
            #expect(!FileManager.default.fileExists(atPath: destination.deletingLastPathComponent().path))
        }
    }

    @Test(arguments: [201, 401, 429, 503])
    func uploadRequiresHTTP200AndKeepsTheFileForRetry(status: Int) async throws {
        let f = try UploadFixture()
        defer { f.remove() }
        let source = try UploadFixtures.make("wav", in: f.sourceDirectory)
        f.start(source)
        await settle { await f.transport.requests.count == 1 }
        let request = try #require(await f.transport.requests.first)
        try await f.transport.reply(status: status)
        await settle { f.app.state.upload.phase == .failed }
        #expect(f.app.state.upload.failure == .transcription(.service(status)))
        #expect(f.app.state.upload.source == source)
        #expect(f.app.state.upload.text.isEmpty && f.app.state.history.entries.isEmpty)
        #expect(!FileManager.default.fileExists(atPath: request.bodyFile.deletingLastPathComponent().path))
    }

    @Test func historySaveFailurePreservesTheCopyableRawResult() async throws {
        let f = try UploadFixture()
        defer { f.remove() }
        try Data("Invalid History fixture".utf8).write(to: f.profile.profile.directory.appendingPathComponent("history.sqlite"))
        let source = try UploadFixtures.make("wav", in: f.sourceDirectory)
        await f.transport.setImmediate()
        f.start(source)
        await f.completed()
        #expect(f.app.state.upload.historySaveFailed)
        #expect(f.app.state.upload.historyID == nil)
        #expect(f.app.state.upload.failure == nil)
        f.app.send(.copyUploadResult)
        #expect(f.clipboard.values == ["Synthetic raw transcript"])
    }

    @Test func emptyResponseAndUnavailableCredentialProvideRecoverableUploadFeedback() async throws {
        let f = try UploadFixture(keychain: true)
        defer { f.remove() }
        let source = try UploadFixtures.make("wav", in: f.sourceDirectory)
        f.start(source)
        await settle { await f.transport.requests.count == 1 }
        try await f.transport.reply(text: "   ")
        await settle { f.app.state.upload.phase == .failed }
        #expect(f.app.state.upload.failure == .transcription(.emptyTranscript))
        #expect(f.app.state.upload.failure?.message(in: .english).contains("file") == true)
        let account = try #require(f.app.state.settings.asrCredentialAccount)
        try f.profile.credentials.delete(account: account)
        f.app.send(.startUpload)
        #expect(f.app.state.upload.failure == .credentialUnavailable)
        #expect(await f.transport.requests.count == 1)
        #expect(f.app.state.upload.source == source)
    }

    @Test(arguments: [true, false])
    func shutdownWaitsForRunningAndPreviouslyCancelledConversion(cancelFirst: Bool) async throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let helper = profile.root.appendingPathComponent("converter-fixture.sh")
        try Data("#!/bin/sh\nfor destination do :; done\nprintf ready > \"$destination\"\nexec /bin/sleep 30\n".utf8).write(to: helper)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: helper.path)
        let converter = RecordingUploadConverter(executable: helper)
        let f = try UploadFixture(converter: converter)
        defer { f.remove() }
        let source = try UploadFixtures.make("aiff", in: f.sourceDirectory)
        f.start(source)
        await settle {
            guard let destination = await converter.destinations.first else { return false }
            return FileManager.default.fileExists(atPath: destination.path)
        }
        let destination = try #require(await converter.destinations.first)
        if cancelFirst { f.app.send(.cancelUpload) }
        await f.app.cancelUploadsAndWait()
        #expect(f.app.state.upload.phase == .idle)
        #expect(!FileManager.default.fileExists(atPath: destination.deletingLastPathComponent().path))
        #expect(await f.transport.requests.isEmpty)
        #expect(FileManager.default.fileExists(atPath: source.path))
        f.start(source)
        #expect(f.app.state.upload.phase == .idle)
    }

    @Test func missingConverterAndUnsupportedOrUnreadableFilesRemainRecoverable() async throws {
        let converter = FFmpegUploadConverter(executable: URL(fileURLWithPath: "/missing-test-ffmpeg"))
        let f = try UploadFixture(converter: converter)
        defer { f.remove() }
        f.app.send(.selectUpload(f.sourceDirectory.appendingPathComponent("unsupported.txt")))
        #expect(f.app.state.upload.failure == .unsupportedFormat)
        f.start(f.sourceDirectory.appendingPathComponent("missing.wav"))
        await settle { f.app.state.upload.phase == .failed }
        #expect(f.app.state.upload.failure == .unreadableFile)
        let source = try UploadFixtures.make("aiff", in: f.sourceDirectory)
        f.start(source)
        await settle { f.app.state.upload.phase == .failed }
        #expect(f.app.state.upload.failure == .converterUnavailable)
        #expect(await f.transport.requests.isEmpty)
        #expect(UploadFailure.converterUnavailable.message(in: .simplifiedChinese).contains("转换器"))
    }

    @Test func historyDisabledWhileASRIsPendingPreventsPersistence() async throws {
        let f = try UploadFixture()
        defer { f.remove() }
        let source = try UploadFixtures.make("wav", in: f.sourceDirectory)
        f.start(source)
        await settle { await f.transport.requests.count == 1 }
        f.app.send(.setHistoryEnabled(false))
        try await f.transport.reply()
        await f.completed()
        #expect(f.app.state.upload.historyID == nil)
        #expect(f.app.state.history.entries.isEmpty)
        #expect(!f.app.state.upload.text.isEmpty)
    }

    @Test func realLoopbackUploadFromReadonlyFilePersistsRawOutput() async throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let server = try LocalHTTPServer(body: "{\"text\":\"Loopback raw signature 软件\"}")
        try await server.start()
        defer { server.stop() }
        let sourceDirectory = profile.root.appendingPathComponent("Source")
        let source = try UploadFixtures.make("wav", in: sourceDirectory)
        try FileManager.default.setAttributes([.posixPermissions: 0o555], ofItemAtPath: sourceDirectory.path)
        defer { try? FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: sourceDirectory.path) }
        let app = WhisperApplication(profile: profile.profile, credentials: profile.credentials,
            microphones: ControlledMicrophones(), clipboard: ControlledClipboard(), pasteSystem: ControlledPasteSystem())
        app.send(.saveASR(.init(serverURL: "http://127.0.0.1:\(server.port)/v1", model: "fixture"), credential: .unchanged))
        app.send(.selectUpload(source)); app.send(.startUpload)
        await settle { app.state.upload.phase == .complete && !app.state.upload.isSavingHistory }
        await app.flushHistoryWrites()
        await settle { !app.state.history.isLoading && !app.state.history.isSearching }
        #expect(server.requests.count == 1)
        #expect(app.state.upload.text == "Loopback raw signature 软件")
        #expect(app.state.history.entries.first?.source == .upload)
        #expect(app.state.history.entries.first?.text == "Loopback raw signature 软件")
    }

    @Test func failedClipboardWriteKeepsTheSelectableTranscript() async throws {
        let f = try UploadFixture(clipboard: FailedUploadClipboard())
        defer { f.remove() }
        let source = try UploadFixtures.make("wav", in: f.sourceDirectory)
        await f.transport.setImmediate()
        f.start(source)
        await f.completed()
        f.app.send(.copyUploadResult)
        #expect(!f.app.state.upload.resultCopied)
        #expect(f.app.state.upload.copyFailed)
        #expect(f.app.state.upload.text == "Synthetic raw transcript")
    }

    @Test func selfHostedUploadDoesNotImposeTheThirdPartyFileSizeCap() async throws {
        let f = try UploadFixture()
        defer { f.remove() }
        try FileManager.default.createDirectory(at: f.sourceDirectory, withIntermediateDirectories: false)
        let source = f.sourceDirectory.appendingPathComponent("large-synthetic.wav")
        try UploadFixtures.run(["-f", "lavfi", "-i", "anullsrc=r=16000:cl=mono", "-t", "850", "-c:a", "pcm_s16le", "-y", source.path])
        await f.transport.setImmediate()
        f.start(source)
        await f.completed()
        let request = try #require(await f.transport.requests.first)
        #expect(request.audio.count > 25 * 1024 * 1024)
        #expect(f.app.state.upload.phase == .complete)
        #expect(!FileManager.default.fileExists(atPath: request.bodyFile.deletingLastPathComponent().path))
    }

    @Test func teardownDoesNotRetainTheApplicationOrSaveLateUploadResults() async throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let transport = ControlledUploadTransport()
        let source = try UploadFixtures.make("wav", in: profile.root.appendingPathComponent("Source"))
        var app: WhisperApplication? = WhisperApplication(profile: profile.profile, credentials: profile.credentials,
            microphones: ControlledMicrophones(), transcriber: SelfHostedTranscriber(transport: transport),
            clipboard: ControlledClipboard(), pasteSystem: ControlledPasteSystem())
        app?.send(.saveASR(.init(serverURL: "http://localhost:8178/v1", model: "fixture"), credential: .unchanged))
        app?.send(.selectUpload(source)); app?.send(.startUpload)
        await settle { await transport.requests.count == 1 }
        let request = try #require(await transport.requests.first)
        let released = { [weak app] in app == nil }
        app = nil
        #expect(released())
        try await transport.reply(text: "Ended application response")
        await settle { !FileManager.default.fileExists(atPath: request.bodyFile.deletingLastPathComponent().path) }
        let reopened = profile.open()
        reopened.send(.loadHistory)
        await settle { !reopened.state.history.isLoading }
        #expect(reopened.state.history.entries.isEmpty)
    }

    @Test func shutdownCancelsActualURLSessionAndWaitsForMultipartRemovalAndHistoryWrites() async throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let server = try LocalHTTPServer(holdResponses: true)
        try await server.start()
        defer { server.stop() }
        let transport = ControlledUploadTransport(forwarding: URLSessionFileTransport())
        let source = try UploadFixtures.make("wav", in: profile.root.appendingPathComponent("Source"))
        let app = WhisperApplication(profile: profile.profile, credentials: profile.credentials,
            microphones: ControlledMicrophones(), transcriber: SelfHostedTranscriber(transport: transport),
            clipboard: ControlledClipboard(), pasteSystem: ControlledPasteSystem())
        app.send(.saveASR(.init(serverURL: "http://127.0.0.1:\(server.port)/v1", model: "fixture"), credential: .unchanged))
        app.send(.selectUpload(source)); app.send(.startUpload)
        await settle { server.requests.count == 1 }
        let request = try #require(await transport.requests.first)
        let acceptedHistory = HistoryEntry(text: "Already accepted result")
        app.send(.saveHistory(acceptedHistory))
        await app.cancelUploadsAndWait()
        #expect(app.state.upload.phase == .idle)
        #expect(app.state.history.pendingChanges == 0)
        #expect(!FileManager.default.fileExists(atPath: request.bodyFile.deletingLastPathComponent().path))
        let reopened = profile.open()
        reopened.send(.loadHistory)
        await settle { !reopened.state.history.isLoading }
        #expect(reopened.state.history.entries == [acceptedHistory])
    }
}

@MainActor private struct FailedUploadClipboard: TextClipboard {
    func write(_ text: String) {}
    func writeResult(_ text: String) -> Bool { false }
}
