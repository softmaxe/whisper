import Foundation
import Testing
import WhisperCore

@Suite(.serialized) @MainActor
struct ShellWorkflowTests {
    @Test(arguments: AppLanguage.allCases)
    func mainPagesAndSeparateSettingsPreserveTheirDestinations(language: AppLanguage) async throws {
        let fixture = try ShellFixture(); defer { fixture.remove() }
        fixture.app.send(.setLanguage(language))
        #expect(MainPage.allCases == [.home, .insights, .upload, .dictionary])
        #expect(SettingsSection.allCases == [.general, .hotkeys, .speechToText, .textCleanup, .privacy])
        for page in MainPage.allCases {
            fixture.app.send(.navigate(page))
            for section in SettingsSection.allCases {
                fixture.app.send(.openSettings(section))
                #expect(fixture.app.state.navigation.page == page)
                #expect(fixture.app.state.navigation.settingsPresented)
                #expect(fixture.app.state.navigation.settingsSection == section)
                #expect(!section.title(in: language).isEmpty)
                fixture.app.send(.closeSettings)
                #expect(!fixture.app.state.navigation.settingsPresented)
                #expect(fixture.app.state.navigation.page == page)
            }
        }
        #expect(fixture.privacy.requests.isEmpty)
        #expect(fixture.privacy.settingsOpened.isEmpty)
        #expect(fixture.microphones.sessions.isEmpty)
        #expect(fixture.desktop.registrationRequests.isEmpty)
        #expect(fixture.desktop.cues.isEmpty)
        #expect(fixture.paste.activations.isEmpty)
    }

    @Test func uploadSettingsAndHistoryLinksDoNotReplaceUploadState() throws {
        let fixture = try ShellFixture(); defer { fixture.remove() }
        fixture.app.send(.navigate(.upload))
        fixture.app.send(.openSettings(.speechToText))
        #expect(fixture.app.state.navigation.page == .upload)
        #expect(fixture.app.state.navigation.settingsSection == .speechToText)
        fixture.app.send(.closeSettings)
        #expect(fixture.app.state.navigation.page == .upload)
        fixture.app.send(.navigate(.home))
        #expect(fixture.app.state.navigation.page == .home)
        fixture.app.send(.toggleSidebar)
        #expect(fixture.app.state.navigation.sidebarCollapsed)
        fixture.app.send(.toggleSidebar)
        #expect(!fixture.app.state.navigation.sidebarCollapsed)
    }

    @Test func ASRAndCleanupDraftsSurviveNavigationAndOnlySaveExplicitly() throws {
        let fixture = try ShellFixture(); defer { fixture.remove() }
        let saved = fixture.app.state.settings.asr
        fixture.app.send(.openSettings(.speechToText))
        let asr = ASRConfiguration(serverURL: "http://127.1:9000/custom", model: "draft-model")
        fixture.app.send(.editASRDraft(asr))
        fixture.app.send(.openSettings(.textCleanup))
        let cleanup = CleanupConfiguration(serverURL: "http://localhost:9001", model: "cleanup-draft")
        fixture.app.send(.editCleanupDraft(cleanup))
        fixture.app.send(.editCleanupPromptDraft("Keep the original meaning. Remove filler."))
        fixture.app.send(.closeSettings)
        fixture.app.send(.navigate(.insights))
        fixture.app.send(.openSettings(.speechToText))
        #expect(fixture.app.state.settingsDraft?.asr == asr)
        #expect(fixture.app.state.settingsDraft?.cleanup == cleanup)
        #expect(fixture.reopen().state.settings.asr == saved)
        fixture.app.send(.saveASRDraft(.unchanged))
        #expect(fixture.app.state.settingsSaved)
        #expect(fixture.reopen().state.settings.asr == .init(serverURL: "http://127.0.0.1:9000/custom", model: "draft-model"))
        fixture.app.send(.saveCleanupDraft(.unchanged))
        fixture.app.send(.saveCleanupPromptDraft)
        #expect(fixture.reopen().state.settings.cleanup.customPrompt == "Keep the original meaning. Remove filler.")
        fixture.app.send(.editASRDraft(.init(serverURL: "http://public.example.com", model: "invalid")))
        fixture.app.send(.saveASRDraft(.unchanged))
        #expect(!fixture.app.state.settingsSaved)
        #expect(fixture.app.state.settingsDraft?.asr.model == "invalid")
        #expect(fixture.reopen().state.settings.asr.model == "draft-model")
    }

    @Test func languageChangesTranslateDefaultPromptButPreserveCustomDraft() throws {
        let fixture = try ShellFixture(); defer { fixture.remove() }
        fixture.app.send(.openSettings(.textCleanup))
        fixture.app.send(.setLanguage(.simplifiedChinese))
        #expect(fixture.app.state.settingsDraft?.cleanupPrompt == CleanupPrompts.defaultText(in: .simplifiedChinese))
        fixture.app.send(.editCleanupPromptDraft("Custom prompt draft"))
        fixture.app.send(.setLanguage(.english))
        #expect(fixture.app.state.settingsDraft?.cleanupPrompt == "Custom prompt draft")
    }

    @Test func leavingShortcutCaptureRestoresOrdinaryNavigationAndSearch() throws {
        let fixture = try ShellFixture(); defer { fixture.remove() }
        fixture.app.send(.openSettings(.hotkeys))
        fixture.app.send(.beginShortcutCapture(index: nil))
        fixture.app.send(.openHistorySearch)
        #expect(!fixture.app.state.navigation.searchPresented)
        fixture.app.send(.openSettings(.general))
        #expect(!fixture.app.state.shortcutCapture.isActive)
        fixture.app.send(.openSettings(.hotkeys))
        fixture.app.send(.beginShortcutCapture(index: nil))
        fixture.app.send(.closeSettings)
        #expect(!fixture.app.state.shortcutCapture.isActive)
        let before = fixture.app.state.settings.shortcuts
        fixture.app.send(.openHistorySearch)
        #expect(fixture.app.state.navigation.searchPresented)
        #expect(fixture.app.state.settings.shortcuts == before)
        #expect(fixture.microphones.sessions.isEmpty)
    }

    @Test(arguments: AppLanguage.allCases)
    func referenceFixtureSearchSelectionCopyAndReopenUseRealHistory(language: AppLanguage) async throws {
        let fixture = try ShellFixture(); defer { fixture.remove() }
        let entries = await fixture.seedReference(language: language)
        fixture.app.send(.navigate(.dictionary))
        #expect(fixture.app.state.dictionary.entries.count == 4)
        #expect(fixture.app.state.snippets.entries.count == 2)
        fixture.app.send(.openHistorySearch)
        fixture.app.send(.searchHistory(language == .english ? "interface design" : "界面设计"))
        await settle { !fixture.app.state.history.isSearching && fixture.app.state.history.searchResults.count == 1 }
        fixture.app.send(.openHistorySearchResult(entries[1].id))
        #expect(fixture.app.state.navigation.page == .home)
        #expect(fixture.app.state.history.selectedEntry?.id == entries[1].id)
        fixture.app.send(.copyHistory(entries[1].id, .processed))
        #expect(fixture.clipboard.values.last == entries[1].text)
        fixture.app.send(.closeHistorySearch)
        #expect(fixture.app.state.history.selectedEntry == nil)
        #expect(!fixture.app.state.navigation.searchPresented)
        let reopened = fixture.reopen()
        reopened.send(.loadHistory)
        await settle { reopened.state.history.isLoaded && !reopened.state.history.isLoading }
        #expect(reopened.state.history.totalCount == 4)
    }

    @Test func populatedFixtureHasStableCountsAndPagination() async throws {
        let fixture = try ShellFixture(); defer { fixture.remove() }
        await fixture.seedPopulated()
        #expect(fixture.app.state.history.totalCount == 1_000)
        #expect(fixture.app.state.history.entries.count == 50)
        #expect(fixture.app.state.history.hasMore)
        #expect(fixture.app.state.dictionary.entries.count == 100)
        #expect(fixture.app.state.snippets.entries.count == 100)
        fixture.app.send(.loadMoreHistory)
        await settle { !fixture.app.state.history.isLoading && fixture.app.state.history.entries.count == 100 }
        #expect(Set(fixture.app.state.history.entries.map(\.id)).count == 100)
    }

    @Test func privacyOnlyRequestsPermissionsOnExplicitCommands() async throws {
        let fixture = try ShellFixture(); defer { fixture.remove() }
        #expect(fixture.privacy.reads == 0)
        fixture.app.send(.openSettings(.privacy))
        await settle { !fixture.app.state.privacy.isLoadingStorage }
        #expect(fixture.app.state.privacy.permissions.microphone == .notDetermined)
        #expect(fixture.privacy.requests.isEmpty)
        fixture.app.send(.requestPrivacyPermission(.microphone))
        await settle { fixture.app.state.privacy.requesting == nil }
        #expect(fixture.privacy.requests == [.microphone])
        #expect(fixture.app.state.privacy.permissions.microphone == .granted)
        fixture.app.send(.openPrivacySettings(.accessibility))
        #expect(fixture.privacy.settingsOpened == [.accessibility])
        #expect(fixture.microphones.sessions.isEmpty)
    }

    @Test func privacyCountsActualRetainedAudioAndRefreshesAfterClear() async throws {
        let fixture = try ShellFixture(); defer { fixture.remove() }
        fixture.app.send(.startDictation)
        let capture = try #require(fixture.microphones.sessions.last)
        capture.open(); capture.deliver([Float](repeating: 0, count: 57_600))
        await settle { fixture.app.state.dictation.phase == .recording }
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        await fixture.transport.reply(body: "{\"text\":\"Synthetic storage fixture\"}")
        await settle { fixture.app.state.dictation.phase == .result }
        await fixture.app.flushHistoryWrites()
        fixture.app.send(.openSettings(.privacy))
        await settle { !fixture.app.state.privacy.isLoadingStorage }
        #expect(fixture.app.state.privacy.audioUsage?.files == 1)
        #expect((fixture.app.state.privacy.audioUsage?.bytes ?? 0) > 0)
        fixture.app.send(.clearHistoryAudio)
        await fixture.app.flushHistoryWrites()
        fixture.app.send(.refreshPrivacy)
        await settle { !fixture.app.state.privacy.isLoadingStorage }
        #expect(fixture.app.state.privacy.audioUsage == .init())
        fixture.app.send(.loadHistory)
        await settle { !fixture.app.state.history.isLoading }
        #expect(fixture.app.state.history.entries.first?.text == "Synthetic storage fixture")
    }

    @Test func delayedPermissionResponseCannotChangeTerminatedApplication() async throws {
        let fixture = try ShellFixture(); defer { fixture.remove() }
        fixture.privacy.holdRequest = true
        fixture.app.send(.requestPrivacyPermission(.microphone))
        await settle { fixture.privacy.pendingRequest != nil }
        await fixture.app.prepareForTermination()
        let terminated = fixture.app.state
        fixture.privacy.pendingRequest?.resume()
        fixture.privacy.pendingRequest = nil
        await settle { fixture.privacy.value.microphone == .granted }
        #expect(fixture.app.state == terminated)
    }
    @Test func syntheticPreviewUsesOnlyControlledEffectsAndRealFixturePersistence() async throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let app = try SyntheticPreview.make(profile: profile.profile, language: .simplifiedChinese)
        try await SyntheticPreview.seed(app)
        app.send(.refreshMicrophones)
        #expect(app.state.microphoneInputs.devices.allSatisfy { $0.name.contains("fixture") })
        app.send(.openSettings(.privacy))
        app.send(.requestPrivacyPermission(.microphone))
        await settle { app.state.privacy.requesting == nil && !app.state.privacy.isLoadingStorage }
        #expect(app.state.privacy.permissions.microphone == .granted)
        #expect(!app.state.settings.desktop.audioCuesEnabled)
        #expect(!app.state.settings.desktop.pauseMediaOnDictation)
        app.send(.closeSettings)
        app.send(.startDictation)
        await settle { app.state.dictation.phase == .recording }
        app.send(.stopDictation)
        await settle { app.state.dictation.phase == .result }
        #expect(!app.state.dictation.text.isEmpty)
        await app.prepareForTermination()
        let reopened = try SyntheticPreview.make(profile: profile.profile)
        reopened.send(.loadHistory)
        await settle { reopened.state.history.isLoaded && !reopened.state.history.isLoading }
        #expect(reopened.state.history.totalCount == 5)
        await reopened.prepareForTermination()
    }

    @Test func syntheticPreviewRejectsExistingUnmarkedDataWithoutChangingIt() throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        try FileManager.default.createDirectory(at: profile.profile.directory, withIntermediateDirectories: true)
        let sentinel = profile.profile.directory.appendingPathComponent("original.txt")
        try Data("untouched fixture".utf8).write(to: sentinel)
        #expect(throws: ConfigurationError.incompatibleProfile) { try SyntheticPreview.make(profile: profile.profile) }
        #expect(try String(contentsOf: sentinel, encoding: .utf8) == "untouched fixture")
        #expect(!FileManager.default.fileExists(atPath: profile.profile.directory.appendingPathComponent(".whisper-synthetic-profile").path))
    }

}
