import Foundation
import Testing
import WhisperCore

@Suite(.serialized) @MainActor
struct EndpointCompatibilityTests {
    @Test(arguments: [
        ("http://localhost:8178/v1/responses?fixture=1", "http://localhost:8178/v1/audio/transcriptions?fixture=1"),
        ("https://asr.example.com/prefix/CHAT/COMPLETIONS?gateway=a%2Fb", "https://asr.example.com/prefix/audio/transcriptions?gateway=a%2Fb"),
        ("https://asr.example.com/v1/MODELS", "https://asr.example.com/v1/audio/transcriptions"),
        ("https://asr.example.com/AUDIO/TRANSLATIONS/", "https://asr.example.com/audio/transcriptions"),
        ("https://asr.example.com/prefix%2Fnamespace/v1/RESPONSES?api-version=fixture", "https://asr.example.com/prefix%2Fnamespace/v1/audio/transcriptions?api-version=fixture"),
        ("https://resource.openai.azure.com.evil.example/v1/responses", "https://resource.openai.azure.com.evil.example/v1/audio/transcriptions"),
        ("https://openai.azure.com/v1", "https://openai.azure.com/v1/audio/transcriptions"),
        ("https://notopenai.azure.com/v1", "https://notopenai.azure.com/v1/audio/transcriptions"),
        ("https://asr.example.com?next=resource.openai.azure.com", "https://asr.example.com/audio/transcriptions?next=resource.openai.azure.com")
    ])
    func ordinaryAndLookalikeHostsPreserveTheirConfiguredOriginAndQuery(base: String, endpoint: String) async throws {
        let fixture = try DictationFixture(server: base, credential: "asr-placeholder")
        defer { fixture.remove() }
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        let request = try #require(await fixture.transport.requests.first)
        #expect(request.request.url?.absoluteString == endpoint)
        #expect(request.request.value(forHTTPHeaderField: "Authorization") == "Bearer asr-placeholder")
        #expect(request.request.value(forHTTPHeaderField: "api-key") == nil)
        #expect(!request.audio.isEmpty)
        #expect(String(decoding: request.body, as: UTF8.self).contains("name=\"model\"\r\n\r\nwhisper-fixture"))
        await fixture.transport.reply(body: #"{"text":"Endpoint fixture result"}"#)
        await settle { fixture.app.state.dictation.phase == .result }
        await fixture.app.flushHistoryWrites()
        #expect(fixture.app.state.dictation.text == "Endpoint fixture result")
    }

    @Test(arguments: ["resource.openai.azure.com", "resource.cognitiveservices.azure.com", "resource.services.ai.azure.com"])
    func azureResourceHostsUseDeploymentRoutesAndOnlyTheExplicitAPIKey(host: String) async throws {
        let fixture = try DictationFixture(server: "https://" + host.uppercased(), credential: "azure-placeholder")
        defer { fixture.remove() }
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        let request = try #require(await fixture.transport.requests.first)
        let url = try #require(request.request.url)
        let components = try #require(URLComponents(url: url, resolvingAgainstBaseURL: false))
        #expect(components.host?.lowercased() == host)
        #expect(components.percentEncodedPath == "/openai/deployments/whisper-fixture/audio/transcriptions")
        #expect(components.percentEncodedQuery == "api-version=2025-03-01-preview")
        #expect(request.request.value(forHTTPHeaderField: "api-key") == "azure-placeholder")
        #expect(request.request.value(forHTTPHeaderField: "Authorization") == nil)
        #expect(!request.audio.isEmpty)
        await fixture.transport.reply()
        await settle { fixture.app.state.dictation.phase == .result }
        await fixture.app.flushHistoryWrites()
    }

    @Test(arguments: [
        ("https://resource.openai.azure.com:8443/ignored/v1?api-version=preview%2Fv2+canary&api-version=ignored&gateway=drop", "https://resource.openai.azure.com:8443/openai/deployments/%E9%83%A8%E7%BD%B2%2Fa%20%2B%20b%3F/audio/transcriptions?api-version=preview%2Fv2%20canary"),
        ("https://resource.openai.azure.com?api-version=", "https://resource.openai.azure.com/openai/deployments/%E9%83%A8%E7%BD%B2%2Fa%20%2B%20b%3F/audio/transcriptions?api-version=2025-03-01-preview"),
        ("https://resource.openai.azure.com/custom/deployments/pinned%2Fname/Audio/Transcriptions/?api-version=custom&gateway=a%2Fb#ignored", "https://resource.openai.azure.com/custom/deployments/pinned%2Fname/Audio/Transcriptions/?api-version=custom&gateway=a%2Fb"),
        ("https://resource.services.ai.azure.com/Audio/Translations?api-version=first&api-version=second", "https://resource.services.ai.azure.com/Audio/Translations?api-version=first&api-version=second")
    ])
    func azureEncodingAndPinnedAudioEndpointsMatchTheRetainedContract(base: String, endpoint: String) async throws {
        let fixture = try DictationFixture(server: base)
        defer { fixture.remove() }
        fixture.app.send(.saveASR(.init(serverURL: base, model: " 部署/a + b? "), credential: .unchanged))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        let request = try #require(await fixture.transport.requests.first)
        #expect(request.request.url?.absoluteString == endpoint)
        #expect(request.request.value(forHTTPHeaderField: "api-key") == nil)
        #expect(request.request.value(forHTTPHeaderField: "Authorization") == nil)
        #expect(String(decoding: request.body, as: UTF8.self).contains("name=\"model\"\r\n\r\n部署/a + b?\r\n"))
        await fixture.transport.reply()
        await settle { fixture.app.state.dictation.phase == .result }
        await fixture.app.flushHistoryWrites()
    }

    @Test(arguments: ["https://resource.openai.azure.com", "http://localhost:8178"])
    func anAbsentASRKeyNeverBorrowsTheSeparateCleanupCredential(base: String) async throws {
        let profile = try ProfileFixture()
        defer { profile.remove() }
        let microphones = ControlledMicrophones(), transport = ControlledHTTPTransport()
        let app = WhisperApplication(profile: profile.profile, credentials: profile.credentials, microphones: microphones,
            transcriber: SelfHostedTranscriber(transport: transport), clipboard: ControlledClipboard())
        app.send(.saveCleanup(.init(enabled: false, serverURL: "http://localhost:8080"), credential: .replace("cleanup-only-placeholder")))
        app.send(.saveASR(.init(serverURL: base, model: "fixture"), credential: .unchanged))
        #expect(app.state.cleanupCredentialConfigured && !app.state.credentialConfigured)
        app.send(.startDictation)
        let capture = try #require(microphones.sessions.last)
        capture.open(); capture.deliver([Float](repeating: 0, count: 4800))
        await settle { app.state.dictation.phase == .recording }
        app.send(.stopDictation)
        await settle { await transport.requests.count == 1 }
        let request = try #require(await transport.requests.first)
        #expect(request.request.value(forHTTPHeaderField: "Authorization") == nil)
        #expect(request.request.value(forHTTPHeaderField: "api-key") == nil)
        #expect(!String(decoding: request.body, as: UTF8.self).contains("cleanup-only-placeholder"))
        await transport.reply()
        await settle { app.state.dictation.phase == .result }
        await app.flushHistoryWrites()
    }

    @Test func uploadAndBatchUseTheSameAzureRouteWithRawResults() async throws {
        let fixture = try UploadFixture(keychain: true)
        defer { fixture.remove() }
        fixture.app.send(.saveASR(.init(serverURL: "https://resource.cognitiveservices.azure.com?api-version=pinned", model: "deployment fixture"), credential: .replace("upload-api-placeholder")))
        let file = try UploadFixtures.make("wav", in: fixture.sourceDirectory)
        await fixture.transport.setImmediate()
        fixture.start(file)
        await fixture.completed()
        fixture.app.send(.addUploadBatchFiles([file, file]))
        fixture.app.send(.startUploadBatch)
        await settle { !fixture.app.state.batchUpload.isProcessing }
        await fixture.app.flushHistoryWrites()
        let requests = await fixture.transport.requests
        #expect(requests.count == 3)
        for request in requests {
            #expect(request.request.url?.absoluteString == "https://resource.cognitiveservices.azure.com/openai/deployments/deployment%20fixture/audio/transcriptions?api-version=pinned")
            #expect(request.request.value(forHTTPHeaderField: "api-key") == "upload-api-placeholder")
            #expect(request.request.value(forHTTPHeaderField: "Authorization") == nil)
            #expect(request.audio == (try Data(contentsOf: file)))
        }
        #expect(fixture.app.state.batchUpload.items.allSatisfy { $0.status == .done })
    }

    @Test(arguments: [
        ("http://localhost:8080/custom/RESPONSES?gateway=fixture", "http://localhost:8080/custom/v1/chat/completions?gateway=fixture"),
        ("https://cleanup.example.com/v1/CHAT/COMPLETIONS?api-version=pinned", "https://cleanup.example.com/v1/chat/completions?api-version=pinned"),
        ("https://resource.openai.azure.com/v1/MODELS", "https://resource.openai.azure.com/v1/chat/completions")
    ])
    func cleanupRetainsItsExistingCaseInsensitiveRouteAndIndependentBearerKey(base: String, endpoint: String) async throws {
        let profile = try ProfileFixture()
        defer { profile.remove() }
        let http = ControlledCleanupHTTP()
        let app = WhisperApplication(profile: profile.profile, credentials: profile.credentials, cleanup: SelfHostedCleanup(transport: http))
        app.send(.saveCleanup(.init(enabled: true, serverURL: base, model: "fixture"), credential: .replace("cleanup-placeholder")))
        app.send(.testCleanupPrompt(text: "Synthetic cleanup input", prompt: nil))
        await settle { await http.requests.count == 1 }
        let request = try #require(await http.requests.first)
        #expect(request.url?.absoluteString == endpoint)
        #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer cleanup-placeholder")
        #expect(request.value(forHTTPHeaderField: "api-key") == nil)
        #expect(String(decoding: try #require(request.httpBody), as: UTF8.self).contains("Synthetic cleanup input"))
        await http.reply(content: "Cleaned fixture")
        await settle { !app.state.cleanupTest.isRunning }
        #expect(app.state.cleanupTest.text == "Cleaned fixture")
    }

    @Test(arguments: ["https://resource.openai.azure.com", "https://asr.example.com"], [false, true])
    func redirectPolicyPreservesOnlySameOriginExplicitCredentials(base: String, crossOrigin: Bool) async throws {
        let destination = URL(string: crossOrigin ? "https://other.example.com/stolen" : base + "/normalized/audio/transcriptions/")!
        let transport = ControlledProtocolRedirect(destination: destination)
        let fixture = try DictationFixture(server: base, credential: "redirect-key-placeholder", transport: transport)
        defer { fixture.remove() }
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { fixture.app.state.dictation.phase == .result || fixture.app.state.dictation.phase == .failed }
        await fixture.app.flushHistoryWrites()
        #expect(await transport.audioBytes > 0)
        let redirected = await transport.redirected
        if crossOrigin {
            #expect(redirected == nil)
            #expect(fixture.app.state.dictation.failure == .service(307))
        } else {
            let request = try #require(redirected)
            let azure = base.contains(".openai.azure.com")
            #expect(request.value(forHTTPHeaderField: "api-key") == (azure ? "redirect-key-placeholder" : nil))
            #expect(request.value(forHTTPHeaderField: "Authorization") == (azure ? nil : "Bearer redirect-key-placeholder"))
            #expect(fixture.app.state.dictation.text == "Redirect fixture result")
        }
    }
}

// The application builds real AAC and multipart bytes. This controlled HTTP boundary invokes
// the production redirect delegate without making requests to public Azure infrastructure.
// DictationHTTPTests separately exercise the same delegate through actual loopback URLSession.
private actor ControlledProtocolRedirect: FileHTTPTransport {
    let destination: URL
    private(set) var redirected: URLRequest?
    private(set) var audioBytes = 0
    init(destination: URL) { self.destination = destination }
    func upload(_ request: URLRequest, file: URL) async throws -> HTTPResponse {
        audioBytes = try multipartAudio(request: request, body: Data(contentsOf: file)).count
        let session = URLSession(configuration: .ephemeral)
        defer { session.invalidateAndCancel() }
        let task = session.uploadTask(with: request, fromFile: file)
        var candidate = URLRequest(url: destination)
        candidate.setValue("unexpected-forwarded-credential", forHTTPHeaderField: "Authorization")
        candidate.setValue("unexpected-forwarded-credential", forHTTPHeaderField: "api-key")
        let response = HTTPURLResponse(url: request.url!, statusCode: 307, httpVersion: "HTTP/1.1", headerFields: ["Location": destination.absoluteString])!
        redirected = await withCheckedContinuation { continuation in
            URLSessionFileTransport().urlSession(session, task: task, willPerformHTTPRedirection: response, newRequest: candidate) {
                continuation.resume(returning: $0)
            }
        }
        return HTTPResponse(status: redirected == nil ? 307 : 200, body: Data(#"{"text":"Redirect fixture result"}"#.utf8))
    }
}
