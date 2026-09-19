import Foundation
import Testing
import WhisperCore

@Suite(.serialized) @MainActor struct CleanupWorkflowTests {
    @Test func savedCleanupConfigurationAndSeparateCredentialsDriveDictationAndCopy() async throws {
        let fixture = try CleanupFixture(keychain: true)
        defer { fixture.remove() }
        fixture.app.send(.saveASR(.init(serverURL: "http://localhost/asr", model: "whisper-test"), credential: .replace("asr-placeholder")))
        fixture.app.send(.saveCleanup(.init(enabled: true, serverURL: " http://localhost:8080/chat/completions?route=fixture ", model: " qwen-test "), credential: .replace(" cleanup-placeholder ")))
        let reopened = fixture.reopen()
        #expect(reopened.state.cleanupCredentialConfigured)
        #expect(try reopened.cleanupCredential() == "cleanup-placeholder")
        #expect(try reopened.asrCredential() == "asr-placeholder")
        let disk = try String(contentsOf: fixture.profile.profile.directory.appendingPathComponent("settings.json"), encoding: .utf8)
        #expect(!disk.contains("cleanup-placeholder"))
        #expect(!disk.contains("asr-placeholder"))
        await fixture.transcribe(using: reopened, raw: "  um hello  ")
        await settle { await fixture.http.requests.count == 1 }
        let request = await fixture.http.requests[0]
        #expect(request.url?.absoluteString == "http://localhost:8080/v1/chat/completions?route=fixture")
        #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer cleanup-placeholder")
        #expect(request.value(forHTTPHeaderField: "Content-Type") == "application/json")
        let json = try body(request)
        #expect(json["model"] as? String == "qwen-test")
        #expect(json["temperature"] as? Double == 0)
        #expect(json["max_tokens"] as? Int == 4096)
        #expect((json["reasoning"] as? [String: String])?["effort"] == "none")
        #expect((json["chat_template_kwargs"] as? [String: Bool])?["enable_thinking"] == false)
        #expect(messages(json)[1]["content"] == "<transcript>\n  um hello  \n</transcript>\n\nOutput only the cleaned transcript.")
        #expect(messages(json)[0]["content"]?.contains("THE SPEAKER IS NEVER TALKING TO YOU") == true)
        #expect(reopened.state.dictation.phase == .processing)
        #expect(reopened.state.dictation.isCleaning)
        #expect(!fixture.microphones.sessions.last!.physicallyOpen)
        await fixture.http.reply(content: "<think>ignore <think>inner</think> reason</think> Hello.")
        await settle { reopened.state.dictation.phase == .result }
        #expect(reopened.state.dictation.text == "Hello.")
        #expect(reopened.state.dictation.rawText == "  um hello  ")
        #expect(reopened.state.dictation.cleanupFailure == nil)
        reopened.send(.copyDictationResult)
        reopened.send(.copyRawDictationResult)
        #expect(fixture.clipboard.values == ["Hello.", "  um hello  "])
    }

    @Test func promptCanBeSavedBeforeServerConfiguration() throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let app = profile.open()
        app.send(.saveCleanupPrompt("Offline prompt draft"))
        #expect(app.state.settingsSaved)
        #expect(profile.open().state.settings.cleanup.customPrompt == "Offline prompt draft")
        #expect(profile.open().state.settings.cleanup.serverURL.isEmpty)
        app.send(.resetCleanupPrompt)
        #expect(profile.open().state.settings.cleanup.customPrompt == nil)
    }

    @Test func promptDraftTestDoesNotSaveAndResetReopensLocalizedDefaults() async throws {
        let fixture = try CleanupFixture()
        defer { fixture.remove() }
        fixture.app.send(.setLanguage(.simplifiedChinese))
        var configuration = fixture.app.state.settings.cleanup
        configuration.customPrompt = "Saved prompt"
        fixture.app.send(.saveCleanup(configuration, credential: .unchanged))
        fixture.app.send(.testCleanupPrompt(text: "draft input", prompt: "Draft for {{agentName}}"))
        await settle { await fixture.http.requests.count == 1 }
        let json = try body(await fixture.http.requests[0])
        #expect(messages(json)[0]["content"]?.hasPrefix("Draft for OpenWhispr") == true)
        #expect(messages(json)[0]["content"]?.contains("auto-detect") == true)
        #expect(messages(json)[1]["content"] == "<transcript>\ndraft input\n</transcript>\n\nOutput only the cleaned transcript.")
        await fixture.http.reply(content: "草稿结果")
        await settle { !fixture.app.state.cleanupTest.isRunning }
        #expect(fixture.app.state.cleanupTest.text == "草稿结果")
        #expect(fixture.reopen().state.settings.cleanup.customPrompt == "Saved prompt")
        fixture.app.send(.resetCleanupPrompt)
        #expect(fixture.reopen().state.settings.cleanup.customPrompt == nil)
        fixture.app.send(.testCleanupPrompt(text: "测试", prompt: nil))
        await settle { await fixture.http.requests.count == 2 }
        #expect(messages(try body(await fixture.http.requests[1]))[0]["content"]?.hasPrefix("你是一个") == true)
        await fixture.http.reply(1, content: "测试。")
        await settle { !fixture.app.state.cleanupTest.isRunning }
    }

    @Test(arguments: [
        ("{}", CleanupFailure.invalidResponse),
        ("malformed-json", .invalidResponse),
        (#"{"choices":[{"message":{"content":"  "}}]}"#, .emptyResponse),
        (#"{"choices":[{"message":{"content":"<think>unfinished"}}]}"#, .emptyResponse),
        (#"{"choices":[{"message":{"content":"Partial"},"finish_reason":"length"}]}"#, .truncated),
        (#"{"choices":[{"message":{"content":"Partial"},"finish_reason":"max_tokens"}]}"#, .truncated)
    ]) func malformedEmptyAndTruncatedResponsesKeepRaw(body: String, failure: CleanupFailure) async throws {
        let fixture = try CleanupFixture()
        defer { fixture.remove() }
        await fixture.transcribe(raw: "Raw remains available")
        await settle { await fixture.http.requests.count == 1 }
        await fixture.http.respond(0, status: 200, body: body)
        await settle { fixture.app.state.dictation.phase == .result }
        #expect(fixture.app.state.dictation.text == "Raw remains available")
        #expect(fixture.app.state.dictation.rawText == "Raw remains available")
        #expect(fixture.app.state.dictation.cleanupFailure == failure)
        fixture.app.send(.copyDictationResult)
        #expect(fixture.clipboard.values.last == "Raw remains available")
    }

    @Test func deadlineDoesNotRetryAndLateOutputCannotReplaceRawFallback() async throws {
        let fixture = try CleanupFixture()
        defer { fixture.remove() }
        await fixture.transcribe(raw: "Raw after timeout")
        await settle { await fixture.http.requests.count == 1 }
        fixture.clock.advance(29.999)
        #expect(fixture.app.state.dictation.phase == .processing)
        fixture.clock.advance(0.001)
        await settle { fixture.app.state.dictation.phase == .result }
        #expect(fixture.app.state.dictation.cleanupFailure == .timeout)
        #expect(fixture.app.state.dictation.text == "Raw after timeout")
        await fixture.http.reply(content: "Too late")
        await fixture.http.waitUntilReturned(1)
        fixture.clock.advance(120)
        #expect(await fixture.http.requests.count == 1)
        #expect(fixture.app.state.dictation.text == "Raw after timeout")
    }

    @Test func parameterFallbackSharesTheOriginalDeadline() async throws {
        let fixture = try CleanupFixture()
        defer { fixture.remove() }
        await fixture.transcribe(raw: "Original deadline")
        await settle { await fixture.http.requests.count == 1 }
        fixture.clock.advance(20)
        await fixture.http.respond(0, status: 400, body: "strict proxy")
        await settle { await fixture.http.requests.count == 2 }
        fixture.clock.advance(10)
        await settle { fixture.app.state.dictation.phase == .result }
        #expect(fixture.app.state.dictation.cleanupFailure == .timeout)
        #expect(fixture.app.state.dictation.text == "Original deadline")
        await fixture.http.reply(1, content: "Late fallback")
        await fixture.http.waitUntilReturned(2)
        #expect(await fixture.http.requests.count == 2)
    }

    @Test func missingSavedCleanupCredentialFallsBackWithoutSendingASRKey() async throws {
        let fixture = try CleanupFixture(keychain: true)
        defer { fixture.remove() }
        fixture.app.send(.saveASR(.init(serverURL: "http://localhost", model: "asr"), credential: .replace("asr-placeholder")))
        fixture.app.send(.saveCleanup(.init(enabled: true, serverURL: "http://localhost"), credential: .replace("cleanup-placeholder")))
        try fixture.profile.credentials.delete(account: #require(fixture.app.state.settings.cleanupCredentialAccount))
        await fixture.transcribe(raw: "Credential fallback")
        await settle { fixture.app.state.dictation.phase == .result }
        #expect(fixture.app.state.dictation.cleanupFailure == .configuration)
        #expect(fixture.app.state.dictation.text == "Credential fallback")
        #expect(await fixture.http.requests.isEmpty)
    }

    @Test func cancellationReleasesCleanupAndOldReplyCannotOverwriteNewRecording() async throws {
        let fixture = try CleanupFixture()
        defer { fixture.remove() }
        await fixture.transcribe(raw: "Cancelled")
        await settle { await fixture.http.requests.count == 1 }
        fixture.app.send(.startDictation)
        #expect(fixture.microphones.sessions.count == 1)
        fixture.app.send(.cancelDictation)
        #expect(fixture.app.state.dictation.phase == .idle)
        fixture.app.send(.startDictation)
        let id = fixture.app.state.dictation.requestID
        await fixture.http.reply(content: "Late old output")
        await fixture.http.waitUntilReturned(1)
        #expect(fixture.app.state.dictation.requestID == id)
        #expect(fixture.app.state.dictation.phase == .preparing)
        #expect(fixture.app.state.dictation.text.isEmpty)
        #expect(fixture.clipboard.values.isEmpty)
    }

    @Test func fallbackStripsOnlyRejectedParametersAndKeepsCredentialAndPrompt() async throws {
        let fixture = try CleanupFixture()
        defer { fixture.remove() }
        await fixture.transcribe(raw: "Test fallback")
        await settle { await fixture.http.requests.count == 1 }
        await fixture.http.respond(0, status: 400, body: "strict proxy")
        await settle { await fixture.http.requests.count == 2 }
        let second = try body(await fixture.http.requests[1])
        #expect(second["reasoning"] == nil)
        #expect(second["chat_template_kwargs"] != nil)
        await fixture.http.respond(1, status: 422, body: "unsupported chat_template_kwargs temperature model max_tokens messages")
        await settle { await fixture.http.requests.count == 3 }
        let third = try body(await fixture.http.requests[2])
        #expect(third["chat_template_kwargs"] == nil)
        #expect(third["temperature"] == nil)
        #expect(third["model"] as? String == "qwen-fixture")
        #expect(third["max_tokens"] as? Int == 4096)
        #expect(messages(third).count == 2)
        await fixture.http.reply(2, content: "Fallback worked")
        await settle { fixture.app.state.dictation.phase == .result }
        #expect(fixture.app.state.dictation.text == "Fallback worked")
    }

    @Test func transientServerFailuresRetryWithBackoffAndDeterministicRejectionsDoNot() async throws {
        let fixture = try CleanupFixture()
        defer { fixture.remove() }
        await fixture.transcribe(raw: "Raw service fallback")
        for index in 0...3 {
            await settle { await fixture.http.requests.count == index + 1 }
            await fixture.http.respond(index, status: 503, body: "busy")
            await fixture.http.waitUntilReturned(index + 1)
            if index < 3 {
                await settle { fixture.clock.nextDelay == pow(2, Double(index)) }
                fixture.clock.advance(pow(2, Double(index)))
            }
        }
        await settle { fixture.app.state.dictation.phase == .result }
        #expect(fixture.app.state.dictation.cleanupFailure == .service(503))
        await fixture.transcribe(raw: "Unauthorized fallback")
        await settle { await fixture.http.requests.count == 5 }
        await fixture.http.respond(4, status: 401, body: "unauthorized")
        await settle { fixture.app.state.dictation.phase == .result }
        #expect(fixture.app.state.dictation.cleanupFailure == .service(401))
        #expect(await fixture.http.requests.count == 5)
    }

    @Test(arguments: [
        ("http://localhost", "gpt-oss-120b", "low", true, false),
        ("https://api.deepseek.com", "deepseek-reasoner", "", false, true),
        ("https://api.mistral.ai", "magistral-small", "", false, false),
        ("https://api.mistral.ai", "test-model", "none", false, false),
        ("https://api.cerebras.ai", "qwen-3", "none", false, false),
        ("https://api.cerebras.ai", "test-model", "", false, false),
        ("http://localhost", "gpt-4.1", "", false, false),
        ("http://localhost", "gemini-2.5-flash", "", false, false)
    ]) func modelAndEndpointConstraintsAppearInActualRequests(url: String, model: String, effort: String, template: Bool, thinking: Bool) async throws {
        let fixture = try CleanupFixture()
        defer { fixture.remove() }
        fixture.app.send(.saveCleanup(.init(enabled: true, serverURL: url, model: model, temperature: 0.4, maxTokens: 8000), credential: .unchanged))
        fixture.app.send(.testCleanupPrompt(text: "Sample", prompt: nil))
        await settle { await fixture.http.requests.count == 1 }
        let json = try body(await fixture.http.requests[0])
        #expect(json["max_tokens"] as? Int == 8000)
        #expect(json["temperature"] as? Double == 0.4)
        #expect(json["reasoning_effort"] as? String ?? "" == effort)
        #expect((json["chat_template_kwargs"] != nil) == template)
        #expect((json["thinking"] != nil) == thinking)
        if model.contains("gpt-oss") { #expect((json["reasoning"] as? [String: String])?["effort"] == "low") }
        await fixture.http.reply(content: "Done")
        await settle { !fixture.app.state.cleanupTest.isRunning }
    }

    @Test func disabledCleanupDoesNotRequestAndEnabledThinkingIsPreserved() async throws {
        let fixture = try CleanupFixture()
        defer { fixture.remove() }
        fixture.app.send(.saveCleanup(.init(enabled: false), credential: .unchanged))
        await fixture.transcribe(raw: "Untouched")
        await settle { fixture.app.state.dictation.phase == .result }
        #expect(await fixture.http.requests.isEmpty)
        fixture.app.send(.saveCleanup(.init(enabled: true, serverURL: "http://localhost", model: " ", disableThinking: false), credential: .unchanged))
        await fixture.transcribe(raw: "Thinking enabled")
        await settle { await fixture.http.requests.count == 1 }
        let json = try body(await fixture.http.requests[0])
        #expect(json["model"] as? String == "default")
        #expect(json["reasoning"] == nil)
        #expect(json["chat_template_kwargs"] == nil)
        await fixture.http.reply(content: "<think>Visible</think> Done")
        await settle { fixture.app.state.dictation.phase == .result }
        #expect(fixture.app.state.dictation.text == "<think>Visible</think> Done")
    }

    @Test func invalidConfigurationCredentialFailuresAndRemovalPreserveSeparateASRSlot() throws {
        let fixture = try CleanupFixture(keychain: true)
        defer { fixture.remove() }
        fixture.app.send(.saveASR(.init(serverURL: "http://localhost", model: "asr"), credential: .replace("asr-placeholder")))
        fixture.app.send(.saveCleanup(.init(enabled: true, serverURL: "http://localhost", model: "cleanup"), credential: .replace("cleanup-placeholder")))
        let saved = fixture.app.state.settings.cleanup
        for url in ["http://public.example.com", "ftp://localhost", "https://name:secret@example.com", ""] {
            fixture.app.send(.saveCleanup(.init(enabled: true, serverURL: url), credential: .replace("rejected-placeholder")))
            #expect(!fixture.app.state.settingsSaved)
            #expect(fixture.reopen().state.settings.cleanup == saved)
            #expect(try fixture.reopen().cleanupCredential() == "cleanup-placeholder")
        }
        fixture.app.send(.saveCleanup(.init(enabled: true, serverURL: "http://localhost", maxTokens: 0), credential: .unchanged))
        #expect(fixture.app.state.configurationError == .invalidCleanupOptions)
        fixture.app.send(.saveCleanup(saved, credential: .remove))
        #expect(!fixture.reopen().state.cleanupCredentialConfigured)
        #expect(try fixture.reopen().asrCredential() == "asr-placeholder")
    }

    @Test func unavailableKeychainAndUnreadableProfilesNeverSaveCleanupSecrets() throws {
        let fixture = try CleanupFixture()
        defer { fixture.remove() }
        let original = fixture.app.state.settings.cleanup
        fixture.app.send(.saveCleanup(original, credential: .replace("unwritable-placeholder")))
        #expect(fixture.app.state.configurationError == .credentialUnavailable)
        #expect(fixture.reopen().state.settings.cleanup == original)
        let file = fixture.profile.profile.directory.appendingPathComponent("settings.json")
        let damaged = Data("damaged native profile".utf8)
        try damaged.write(to: file)
        let reopened = fixture.reopen()
        reopened.send(.saveCleanup(original, credential: .unchanged))
        #expect(reopened.state.configurationError == .incompatibleProfile)
        #expect(try Data(contentsOf: file) == damaged)
    }

    @Test func teardownDuringCleanupDoesNotKeepApplicationAlive() async throws {
        let fixture = try CleanupFixture()
        defer { fixture.remove() }
        await fixture.transcribe(raw: "Cancelled at teardown")
        await settle { await fixture.http.requests.count == 1 }
        weak let app = fixture.app
        fixture.app = nil
        #expect(app == nil)
        await fixture.http.reply(content: "After teardown")
        await fixture.http.waitUntilReturned(1)
        #expect(fixture.clipboard.values.isEmpty)
    }

    @Test func oldNativeSettingsReopenWithCleanupDefaultsAndFailedSaveKeepsCredential() throws {
        let fixture = try CleanupFixture(keychain: true)
        defer { fixture.remove() }
        let file = fixture.profile.profile.directory.appendingPathComponent("settings.json")
        try Data(#"{"version":1,"settings":{"language":"en","asr":{"serverURL":"http://localhost","model":"asr"}}}"#.utf8).write(to: file)
        let old = fixture.reopen()
        #expect(old.state.configurationError == nil)
        #expect(old.state.settings.cleanup == CleanupConfiguration())
        old.send(.saveCleanup(.init(enabled: true, serverURL: "http://localhost"), credential: .replace("original-placeholder")))
        let backup = fixture.profile.root.appendingPathComponent("settings-backup")
        try FileManager.default.moveItem(at: file, to: backup)
        try FileManager.default.createDirectory(at: file, withIntermediateDirectories: false)
        old.send(.saveCleanup(.init(enabled: true, serverURL: "http://localhost:9999"), credential: .replace("failed-placeholder")))
        #expect(old.state.configurationError == .persistenceFailed)
        #expect(try old.cleanupCredential() == "original-placeholder")
        try FileManager.default.removeItem(at: file)
        try FileManager.default.moveItem(at: backup, to: file)
        #expect(try fixture.reopen().cleanupCredential() == "original-placeholder")
    }

    @Test func cancellationDuringRetryBackoffPreventsAnotherRequest() async throws {
        let fixture = try CleanupFixture()
        defer { fixture.remove() }
        await fixture.transcribe(raw: "Cancel backoff")
        await settle { await fixture.http.requests.count == 1 }
        await fixture.http.respond(0, status: 429, body: "rate limit")
        await settle { fixture.clock.nextDelay == 1 }
        fixture.app.send(.cancelDictation)
        fixture.clock.advance(60)
        for _ in 0..<20 { await Task.yield() }
        #expect(await fixture.http.requests.count == 1)
        #expect(fixture.app.state.dictation.phase == .idle)
    }

    @Test func promptCancellationAndTimeoutKeepSavedDraftAndSuppressLateTestResults() async throws {
        let fixture = try CleanupFixture()
        defer { fixture.remove() }
        fixture.app.send(.testCleanupPrompt(text: "Old", prompt: "Draft"))
        await settle { await fixture.http.requests.count == 1 }
        fixture.app.send(.cancelCleanupPromptTest)
        fixture.app.send(.testCleanupPrompt(text: "New", prompt: "New draft"))
        await settle { await fixture.http.requests.count == 2 }
        await fixture.http.reply(0, content: "Old reply")
        await fixture.http.waitUntilReturned(1)
        #expect(fixture.app.state.cleanupTest.isRunning)
        #expect(fixture.app.state.cleanupTest.text.isEmpty)
        fixture.clock.advance(30)
        await settle { !fixture.app.state.cleanupTest.isRunning }
        #expect(fixture.app.state.cleanupTest.failure == .timeout)
        await fixture.http.reply(1, content: "Late new reply")
        await fixture.http.waitUntilReturned(2)
        #expect(fixture.app.state.cleanupTest.text.isEmpty)
        #expect(fixture.reopen().state.settings.cleanup.customPrompt == nil)
    }

    @Test func realHTTPJSONTransportPreservesSameOriginRedirectAndRejectsCrossOrigin() async throws {
        let target = try LocalHTTPServer(body: #"{"choices":[{"message":{"content":"Loopback cleanup"}}]}"#)
        try await target.start()
        defer { target.stop() }
        let redirect = try LocalHTTPServer(redirectPath: "/normalized", body: #"{"choices":[{"message":{"content":"Normalized cleanup"}}]}"#)
        try await redirect.start()
        defer { redirect.stop() }
        let cross = try LocalHTTPServer(status: 307, headers: "Location: http://127.0.0.1:\(target.port)/other\r\n")
        try await cross.start()
        defer { cross.stop() }
        let fixture = try CleanupFixture(keychain: true, realTransport: true)
        defer { fixture.remove() }
        fixture.app.send(.saveCleanup(.init(enabled: true, serverURL: "http://127.0.0.1:\(redirect.port)"), credential: .replace("cleanup-placeholder")))
        fixture.app.send(.testCleanupPrompt(text: "Wire fixture", prompt: nil))
        await settle { !fixture.app.state.cleanupTest.isRunning }
        #expect(fixture.app.state.cleanupTest.text == "Normalized cleanup")
        #expect(redirect.requests.count == 2)
        for request in redirect.requests {
            let wire = String(decoding: request, as: UTF8.self)
            #expect(wire.contains("Bearer cleanup-placeholder"))
            #expect(wire.contains("Wire fixture"))
        }
        fixture.app.send(.saveCleanup(.init(enabled: true, serverURL: "http://127.0.0.1:\(cross.port)"), credential: .unchanged))
        fixture.app.send(.testCleanupPrompt(text: "Private synthetic text", prompt: nil))
        await settle { !fixture.app.state.cleanupTest.isRunning }
        #expect(fixture.app.state.cleanupTest.failure == .service(307))
        #expect(target.requests.isEmpty)
    }

    private func body(_ request: URLRequest) throws -> [String: Any] {
        let data = try #require(request.httpBody)
        return try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
    }
    private func messages(_ body: [String: Any]) -> [[String: String]] { body["messages"] as? [[String: String]] ?? [] }
}

private actor ControlledCleanupHTTP: JSONHTTPTransport {
    private(set) var requests: [URLRequest] = []
    private var replies: [Int: CheckedContinuation<HTTPResponse, any Error>] = [:]
    private(set) var returned = 0
    func send(_ request: URLRequest) async throws -> HTTPResponse {
        let index = requests.count
        requests.append(request)
        let response = try await withCheckedThrowingContinuation { replies[index] = $0 }
        returned += 1
        return response
    }
    func respond(_ index: Int, status: Int, body: String) {
        replies.removeValue(forKey: index)?.resume(returning: HTTPResponse(status: status, body: Data(body.utf8)))
    }
    func reply(_ index: Int = 0, content: String) {
        let data = try! JSONSerialization.data(withJSONObject: ["choices": [["message": ["content": content], "finish_reason": "stop"]]])
        replies.removeValue(forKey: index)?.resume(returning: HTTPResponse(status: 200, body: data))
    }
    @MainActor func waitUntilReturned(_ count: Int) async { await settle { await self.returned >= count } }
}

@MainActor private final class CleanupFixture {
    let profile: ProfileFixture
    let microphones = ControlledMicrophones()
    let clock = ControlledClock()
    let asr = ControlledHTTPTransport()
    let http = ControlledCleanupHTTP()
    let clipboard = ControlledClipboard()
    let realTransport: Bool
    var app: WhisperApplication!
    init(keychain: Bool = false, realTransport: Bool = false) throws {
        profile = try ProfileFixture(keychain: keychain)
        self.realTransport = realTransport
        app = reopen()
        app.send(.saveASR(.init(serverURL: "http://localhost", model: "asr-fixture"), credential: .unchanged))
        app.send(.saveCleanup(.init(enabled: true, serverURL: "http://localhost", model: "qwen-fixture"), credential: .unchanged))
    }
    func reopen() -> WhisperApplication {
        WhisperApplication(profile: profile.profile, credentials: profile.credentials, microphones: microphones,
            transcriber: SelfHostedTranscriber(transport: asr), clock: clock, clipboard: clipboard,
            cleanup: realTransport ? SelfHostedCleanup() : SelfHostedCleanup(transport: http))
    }
    func transcribe(using custom: WhisperApplication? = nil, raw: String) async {
        let app = custom ?? self.app!
        let index = await asr.requests.count
        app.send(.startDictation)
        let capture = microphones.sessions.last!
        capture.open()
        capture.deliver([Float](repeating: 0, count: 4800))
        await settle { app.state.dictation.phase == .recording }
        app.send(.stopDictation)
        await settle { await self.asr.requests.count == index + 1 }
        let data = try! JSONSerialization.data(withJSONObject: ["text": raw])
        await asr.reply(index, body: String(decoding: data, as: UTF8.self))
    }
    func remove() {
        app?.send(.cancelDictation)
        app?.send(.cancelCleanupPromptTest)
        profile.remove()
    }
}
