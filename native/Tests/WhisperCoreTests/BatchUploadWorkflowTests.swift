import Foundation
import Testing
import WhisperCore

@Suite(.serialized) @MainActor
struct BatchUploadWorkflowTests {
    @Test func multipleSelectionDuplicatesRemovalAndClearPreserveTheirUserVisibleMeaning() async throws {
        let f = try UploadFixture()
        defer { f.remove() }
        let file = try UploadFixtures.make("wav", in: f.sourceDirectory)
        f.app.send(.chooseUploadFiles([file]))
        #expect(f.app.state.upload.phase == .selected)
        #expect(f.app.state.batchUpload.items.isEmpty)
        f.app.send(.chooseUploadFiles([file, file]))
        #expect(f.app.state.upload.phase == .idle)
        let firstIDs = f.app.state.batchUpload.items.map(\.id)
        #expect(firstIDs.count == 2 && Set(firstIDs).count == 2)
        f.app.send(.removeUploadBatchItem(firstIDs[0]))
        #expect(f.app.state.batchUpload.items.map(\.id) == [firstIDs[1]])
        f.app.send(.chooseUploadFiles([file, f.sourceDirectory.appendingPathComponent("unsupported.txt")]))
        #expect(f.app.state.batchUpload.items.count == 2)
        #expect(f.app.state.batchUpload.skippedNames == ["unsupported.txt"])
        f.app.send(.clearUploadBatch)
        #expect(f.app.state.batchUpload.items.isEmpty)
        #expect(f.app.state.batchUpload.skippedNames.isEmpty)
        #expect(FileManager.default.fileExists(atPath: file.path))
        #expect(await f.transport.requests.isEmpty)
    }

    @Test func sequentialRunIncludesAppendedFilesAndUsesOneConfigurationSnapshot() async throws {
        let f = try UploadFixture(keychain: true)
        defer { f.remove() }
        let first = try UploadFixtures.make("wav", in: f.sourceDirectory)
        let second = try UploadFixtures.make("aiff", in: f.sourceDirectory)
        f.app.send(.setTranscriptionLanguage("en-US"))
        f.app.send(.addUploadBatchFiles([first, second]))
        f.app.send(.startUploadBatch)
        let run = f.app.state.batchUpload.runID
        f.app.send(.startUploadBatch)
        #expect(f.app.state.batchUpload.runID == run)
        await settle { await f.transport.requests.count == 1 }
        #expect(f.app.state.batchUpload.items.map(\.status) == [.transcribing, .queued])
        let firstID = f.app.state.batchUpload.items[0].id
        f.app.send(.removeUploadBatchItem(firstID))
        #expect(f.app.state.batchUpload.items.count == 2)
        f.app.send(.saveASR(.init(serverURL: "http://localhost:9999/new", model: "next-model"), credential: .replace("next-key-placeholder")))
        f.app.send(.setTranscriptionLanguage("fr"))
        f.app.send(.chooseUploadFiles([first]))
        #expect(f.app.state.batchUpload.items.count == 3)
        try await f.transport.reply(0, text: "First raw result")
        await settle { await f.transport.requests.count == 2 }
        #expect(f.app.state.batchUpload.items.map(\.status) == [.done, .transcribing, .queued])
        try await f.transport.reply(1, text: "Converted raw result")
        await settle { await f.transport.requests.count == 3 }
        try await f.transport.reply(2, text: "Explicit duplicate result")
        await completed(f)
        #expect(f.app.state.batchUpload.items.allSatisfy { $0.status == .done })
        #expect(f.app.state.batchUpload.completedCount == 3)
        #expect(f.app.state.batchUpload.progress == 1)
        #expect(Set(f.app.state.history.entries.map(\.id)).count == 3)
        for request in await f.transport.requests {
            #expect(request.request.url?.port == 8178)
            #expect(request.request.value(forHTTPHeaderField: "Authorization") == "Bearer upload-placeholder")
            let body = String(decoding: request.body, as: UTF8.self)
            #expect(body.contains("\r\nupload-fixture\r\n") && body.contains("\r\nen\r\n"))
            #expect(!body.contains("name=\"prompt\""))
        }
        f.app.send(.addUploadBatchFiles([first]))
        f.app.send(.startUploadBatch)
        await settle { await f.transport.requests.count == 4 }
        let nextRequest = await f.transport.requests[3]
        #expect(nextRequest.request.url?.port == 9999)
        #expect(nextRequest.request.value(forHTTPHeaderField: "Authorization") == "Bearer next-key-placeholder")
        try await f.transport.reply(3, text: "Next run result")
        await completed(f)
        #expect(f.app.state.batchUpload.items.last?.model == "next-model")
        #expect(f.app.state.history.totalCount == 4)
        f.app.send(.clearUploadBatch)
        #expect(f.app.state.history.totalCount == 4)
    }

    @Test(arguments: [true, false])
    func eachItemKeepsRawTextAndIndependentHistoryAndCopyOutcomes(historyEnabled: Bool) async throws {
        let f = try UploadFixture()
        defer { f.remove() }
        f.app.send(.setHistoryEnabled(historyEnabled))
        f.app.send(.setChineseScriptPreference(.traditional))
        f.app.send(.setTranscriptionLanguage("zh-TW"))
        f.app.send(.saveCleanup(.init(enabled: true, serverURL: "http://localhost:8080", model: "cleanup"), credential: .unchanged))
        f.app.send(.importDictionary("软件, signature"))
        f.app.send(.saveSnippet(trigger: "signature", replacement: "Expanded signature"))
        let file = try UploadFixtures.make("wav", in: f.sourceDirectory)
        f.app.send(.addUploadBatchFiles([file, file]))
        f.app.send(.startUploadBatch)
        let values = ["  软件 signature  ", "\n软件 signature\n"]
        for index in 0..<2 {
            await settle { await f.transport.requests.count == index + 1 }
            try await f.transport.reply(index, text: values[index])
        }
        await completed(f)
        #expect(f.app.state.batchUpload.items.map(\.text) == values)
        #expect(f.app.state.batchUpload.items.allSatisfy { $0.status == .done })
        for item in f.app.state.batchUpload.items { f.app.send(.copyUploadBatchItem(item.id)) }
        #expect(f.clipboard.values == values)
        #expect(await f.cleanup.calls == 0)
        #expect(f.microphones.sessions.isEmpty && f.paste.pasted.isEmpty && f.paste.writes.isEmpty)
        #expect(f.app.state.dictation.phase == .idle)
        for entry in f.app.state.history.entries {
            #expect(entry.source == .upload && entry.text == entry.rawText)
            #expect(entry.audioFileName == nil && entry.audioDuration == nil)
        }
        let reopened = f.profile.open()
        reopened.send(.loadHistory)
        await settle { !reopened.state.history.isLoading }
        #expect(reopened.state.history.entries.count == (historyEnabled ? 2 : 0))
        #expect(f.app.state.batchUpload.items.allSatisfy { ($0.historyID != nil) == historyEnabled })
    }

    @Test func queuedRemovalSkipsOnlyThatOperationWhileOtherItemsContinue() async throws {
        let f = try UploadFixture()
        defer { f.remove() }
        let file = try UploadFixtures.make("wav", in: f.sourceDirectory)
        f.app.send(.addUploadBatchFiles([file, file, file]))
        let removed = f.app.state.batchUpload.items[1].id
        f.app.send(.startUploadBatch)
        await settle { await f.transport.requests.count == 1 }
        f.app.send(.removeUploadBatchItem(removed))
        try await f.transport.reply(0, text: "First")
        await settle { await f.transport.requests.count == 2 }
        try await f.transport.reply(1, text: "Third")
        await completed(f)
        #expect(f.app.state.batchUpload.items.map(\.text) == ["First", "Third"])
        #expect(f.app.state.history.entries.allSatisfy { $0.id != removed })
        #expect(await f.transport.requests.count == 2)
    }

    @Test func conversionServiceAndPersistenceFailuresDoNotStrandLaterFiles() async throws {
        let f = try UploadFixture()
        defer { f.remove() }
        let file = try UploadFixtures.make("wav", in: f.sourceDirectory)
        let badFile = f.sourceDirectory.appendingPathComponent("invalid.mov")
        try Data("Invalid synthetic container".utf8).write(to: badFile)
        let database = f.profile.profile.directory.appendingPathComponent("history.sqlite")
        let original = Data("Corrupted isolated database fixture".utf8)
        try original.write(to: database)
        f.app.send(.addUploadBatchFiles([badFile, file, file, file]))
        f.app.send(.startUploadBatch)
        await settle { await f.transport.requests.count == 1 }
        #expect(f.app.state.batchUpload.items[0].failure == .conversionFailed)
        try await f.transport.reply(0, status: 503)
        await settle { await f.transport.requests.count == 2 }
        try await f.transport.reply(1, text: "Copyable despite save failure")
        await settle { await f.transport.requests.count == 3 }
        #expect(f.app.state.batchUpload.items[2].historySaveFailed)
        f.app.send(.setHistoryEnabled(false))
        try await f.transport.reply(2, text: "Later result without History")
        await completed(f)
        #expect(f.app.state.batchUpload.items.map(\.status) == [.failed, .failed, .failed, .done])
        f.app.send(.copyUploadBatchItem(f.app.state.batchUpload.items[2].id))
        f.app.send(.copyUploadBatchItem(f.app.state.batchUpload.items[3].id))
        #expect(f.clipboard.values == ["Copyable despite save failure", "Later result without History"])
        #expect(try Data(contentsOf: database) == original)
        #expect(f.app.state.batchUpload.progress == 1)
    }

    @Test func cancelPreservesCompletedTextAndLateWorkCannotChangeTheNewRun() async throws {
        let f = try UploadFixture()
        defer { f.remove() }
        let direct = try UploadFixtures.make("wav", in: f.sourceDirectory)
        let converted = try UploadFixtures.make("mov", in: f.sourceDirectory)
        f.app.send(.addUploadBatchFiles([direct, converted, direct]))
        f.app.send(.startUploadBatch)
        await settle { await f.transport.requests.count == 1 }
        try await f.transport.reply(0, text: "Completed before cancel")
        await settle { await f.transport.requests.count == 2 }
        let lateBody = await f.transport.requests[1].bodyFile
        let lateAudio = try #require(await f.converter.destinations.first)
        f.app.send(.cancelUploadBatch)
        #expect(!f.app.state.batchUpload.isProcessing)
        #expect(f.app.state.batchUpload.items.map(\.status) == [.done, .cancelled, .cancelled])
        f.app.send(.copyUploadBatchItem(f.app.state.batchUpload.items[0].id))
        #expect(f.clipboard.values == ["Completed before cancel"])
        f.app.send(.addUploadBatchFiles([direct]))
        f.app.send(.startUploadBatch)
        let currentRun = f.app.state.batchUpload.runID
        await settle { await f.transport.requests.count == 3 }
        try await f.transport.reply(1, text: "Cancelled late result")
        await settle { !FileManager.default.fileExists(atPath: lateBody.deletingLastPathComponent().path) }
        #expect(!FileManager.default.fileExists(atPath: lateAudio.deletingLastPathComponent().path))
        #expect(f.app.state.batchUpload.runID == currentRun && f.app.state.batchUpload.isProcessing)
        try await f.transport.reply(2, text: "New run result")
        await completed(f)
        #expect(f.app.state.batchUpload.items.map(\.status) == [.done, .cancelled, .cancelled, .done])
        #expect(Set(f.app.state.history.entries.map(\.text)) == Set(["Completed before cancel", "New run result"]))
    }

    @Test func clearDuringProcessingCannotReviveOrDuplicateTheReplacementQueue() async throws {
        let f = try UploadFixture()
        defer { f.remove() }
        let file = try UploadFixtures.make("wav", in: f.sourceDirectory)
        f.app.send(.addUploadBatchFiles([file, file]))
        let oldIDs = Set(f.app.state.batchUpload.items.map(\.id))
        f.app.send(.startUploadBatch)
        await settle { await f.transport.requests.count == 1 }
        f.app.send(.clearUploadBatch)
        f.app.send(.addUploadBatchFiles([file]))
        f.app.send(.startUploadBatch)
        await settle { await f.transport.requests.count == 2 }
        try await f.transport.reply(1, text: "Current queue result")
        await completed(f)
        try await f.transport.reply(0, text: "Old queue result")
        await f.app.cancelUploadsAndWait()
        #expect(f.app.state.batchUpload.items.count == 1)
        #expect(f.app.state.batchUpload.items[0].text == "Current queue result")
        #expect(f.app.state.batchUpload.items[0].status == .done)
        #expect(!oldIDs.contains(f.app.state.batchUpload.items[0].id))
        #expect(f.app.state.history.entries.map(\.text) == ["Current queue result"])
    }

    @Test func batchesAndSingleUploadsCannotStartConcurrentActiveRuns() async throws {
        let f = try UploadFixture()
        defer { f.remove() }
        let file = try UploadFixtures.make("wav", in: f.sourceDirectory)
        f.start(file)
        await settle { await f.transport.requests.count == 1 }
        f.app.send(.addUploadBatchFiles([file, file]))
        #expect(f.app.state.batchUpload.items.isEmpty)
        try await f.transport.reply()
        await f.completed()
        f.app.send(.addUploadBatchFiles([file, file]))
        f.app.send(.startUploadBatch)
        await settle { await f.transport.requests.count == 2 }
        f.app.send(.selectUpload(file)); f.app.send(.startUpload)
        #expect(f.app.state.upload.phase == .idle)
        f.app.send(.cancelUploadBatch)
        try await f.transport.reply(1)
        await f.app.cancelUploadsAndWait()
        #expect(await f.transport.requests.count == 2)
    }

    @Test func shutdownWaitsForCancelledBatchConversionAndCancelsAllUnfinishedRows() async throws {
        let owner = try ProfileFixture(keychain: false)
        defer { owner.remove() }
        let helper = owner.root.appendingPathComponent("batch-converter-fixture.sh")
        try Data("#!/bin/sh\nfor destination do :; done\nprintf ready > \"$destination\"\nexec /bin/sleep 30\n".utf8).write(to: helper)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: helper.path)
        let converter = RecordingUploadConverter(executable: helper)
        let f = try UploadFixture(converter: converter)
        defer { f.remove() }
        let file = try UploadFixtures.make("aiff", in: f.sourceDirectory)
        f.app.send(.addUploadBatchFiles([file, file]))
        f.app.send(.startUploadBatch)
        await settle {
            guard let output = await converter.destinations.first else { return false }
            return FileManager.default.fileExists(atPath: output.path)
        }
        let output = try #require(await converter.destinations.first)
        f.app.send(.cancelUploadBatch)
        await f.app.cancelUploadsAndWait()
        #expect(!FileManager.default.fileExists(atPath: output.deletingLastPathComponent().path))
        #expect(f.app.state.batchUpload.items.map(\.status) == [.cancelled, .cancelled])
        #expect(await f.transport.requests.isEmpty)
        f.app.send(.addUploadBatchFiles([file])); f.app.send(.startUploadBatch)
        #expect(f.app.state.batchUpload.items.count == 2 && !f.app.state.batchUpload.isProcessing)
    }

    @Test func invalidConfigurationLeavesQueuedFilesAvailableForRecovery() async throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let app = profile.open()
        let file = try UploadFixtures.make("wav", in: profile.root.appendingPathComponent("Sources"))
        app.send(.addUploadBatchFiles([file, file]))
        app.send(.startUploadBatch)
        #expect(!app.state.batchUpload.isProcessing)
        #expect(app.state.batchUpload.failure == .configuration)
        #expect(app.state.batchUpload.items.allSatisfy { $0.status == .queued })
    }

    @Test func backendCancellationWithoutAUserCancelFailsOnlyItsItem() async throws {
        let f = try UploadFixture()
        defer { f.remove() }
        let file = try UploadFixtures.make("wav", in: f.sourceDirectory)
        f.app.send(.addUploadBatchFiles([file, file]))
        f.app.send(.startUploadBatch)
        await settle { await f.transport.requests.count == 1 }
        await f.transport.abort()
        await settle { await f.transport.requests.count == 2 }
        try await f.transport.reply(1, text: "Following result")
        await completed(f)
        #expect(f.app.state.batchUpload.items.map(\.status) == [.failed, .done])
        #expect(f.app.state.batchUpload.items[0].failure == .transcription(.network))
        #expect(f.app.state.history.entries.map(\.text) == ["Following result"])
    }

    @Test func perFileCopyFailureDoesNotDiscardACompletedResult() async throws {
        let f = try UploadFixture(clipboard: FailedBatchClipboard())
        defer { f.remove() }
        let file = try UploadFixtures.make("wav", in: f.sourceDirectory)
        await f.transport.setImmediate()
        f.app.send(.addUploadBatchFiles([file, file]))
        f.app.send(.startUploadBatch)
        await completed(f)
        let item = f.app.state.batchUpload.items[0]
        f.app.send(.copyUploadBatchItem(item.id))
        #expect(f.app.state.batchUpload.items[0].copyFailed)
        #expect(!f.app.state.batchUpload.items[0].resultCopied)
        #expect(f.app.state.batchUpload.items[0].text == item.text)
        #expect(f.app.state.batchUpload.items[0].status == .done)
    }

    @Test func droppingTheApplicationCancelsTheBatchWithoutLateHistory() async throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let transport = ControlledUploadTransport()
        let file = try UploadFixtures.make("wav", in: profile.root.appendingPathComponent("Sources"))
        var app: WhisperApplication? = WhisperApplication(profile: profile.profile, credentials: profile.credentials,
            microphones: ControlledMicrophones(), transcriber: SelfHostedTranscriber(transport: transport),
            clipboard: ControlledClipboard(), pasteSystem: ControlledPasteSystem())
        app?.send(.saveASR(.init(serverURL: "http://localhost:8178/v1", model: "fixture"), credential: .unchanged))
        app?.send(.addUploadBatchFiles([file, file])); app?.send(.startUploadBatch)
        await settle { await transport.requests.count == 1 }
        let request = try #require(await transport.requests.first)
        let released = { [weak app] in app == nil }
        app = nil
        #expect(released())
        try await transport.reply(text: "Ended batch result")
        await settle { !FileManager.default.fileExists(atPath: request.bodyFile.deletingLastPathComponent().path) }
        #expect(await transport.requests.count == 1)
        let reopened = profile.open()
        reopened.send(.loadHistory)
        await settle { !reopened.state.history.isLoading }
        #expect(reopened.state.history.entries.isEmpty)
    }

    private func completed(_ fixture: UploadFixture) async {
        await settle { !fixture.app.state.batchUpload.isProcessing }
        await fixture.app.flushHistoryWrites()
        await settle { !fixture.app.state.history.isLoading && !fixture.app.state.history.isSearching }
    }
}

@MainActor private struct FailedBatchClipboard: TextClipboard {
    func write(_ text: String) {}
    func writeResult(_ text: String) -> Bool { false }
}
