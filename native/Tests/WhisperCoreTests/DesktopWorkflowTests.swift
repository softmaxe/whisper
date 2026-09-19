import Carbon.HIToolbox
import Foundation
import Testing
import WhisperCore

@MainActor final class ControlledDesktopEffects: DesktopEffects {
    var cues: [DictationCue] = []
    var mediaEvents: [String] = []
    var mediaPlaying = true
    var suspendPause = false
    var pauseContinuation: CheckedContinuation<MediaPauseOwnership?, Never>?
    var loginStatus: LoginItemStatus = .disabled
    var registrationRequests: [Bool] = []
    var approvalRequired = false
    var registrationFails = false
    var openedLoginSettings = 0
    func playCue(_ cue: DictationCue) { cues.append(cue) }
    func pauseMedia() async -> MediaPauseOwnership? {
        mediaEvents.append("pause")
        if suspendPause { return await withCheckedContinuation { pauseContinuation = $0 } }
        return mediaPlaying ? .adapter : nil
    }
    func resumeMedia(_ ownership: MediaPauseOwnership) async { mediaEvents.append("resume") }
    func loginItemStatus() -> LoginItemStatus { loginStatus }
    func setLaunchAtLogin(_ enabled: Bool) async throws -> LoginItemStatus {
        registrationRequests.append(enabled)
        if registrationFails { throw CocoaError(.fileWriteUnknown) }
        loginStatus = enabled ? approvalRequired ? .requiresApproval : .enabled : .disabled
        return loginStatus
    }
    func openLoginItemsSettings() { openedLoginSettings += 1 }
    func resolvePause(_ token: MediaPauseOwnership? = .adapter) {
        pauseContinuation?.resume(returning: token)
        pauseContinuation = nil
    }
}

@Suite(.serialized) @MainActor
struct DesktopWorkflowTests {
    @Test(arguments: AppTheme.allCases, PillPlacement.allCases)
    func desktopPreferencesPersistAndDrivePresentation(theme: AppTheme, placement: PillPlacement) throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let app = profile.open()
        #expect(app.state.settings.desktop == DesktopPreferences())
        var preferences = DesktopPreferences()
        preferences.theme = theme
        preferences.audioCuesEnabled = false
        preferences.pauseMediaOnDictation = true
        preferences.pillVisible = false
        preferences.floatingIconAutoHide = true
        preferences.panelStartPosition = placement
        preferences.showMenuBarIcon = false
        preferences.startMinimized = true
        app.send(.saveDesktopPreferences(preferences))
        #expect(app.state.settingsSaved)
        let reopened = profile.open()
        reopened.send(.bootstrapDesktop(launchedAtLogin: false))
        #expect(reopened.state.settings.desktop == preferences)
        #expect(!reopened.state.desktop.mainWindowVisible)
        #expect(!reopened.state.recordingPill.visible)
        #expect(reopened.state.recordingPill.placement == placement)
        #expect(reopened.state.recordingPill.theme == theme)
        reopened.send(.showMainWindow)
        #expect(reopened.state.desktop.mainWindowVisible)
        reopened.send(.closeMainWindow)
        #expect(!reopened.state.desktop.mainWindowVisible)
    }

    @Test func loginLaunchHidesTheMainWindowWithoutReregistering() throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let effects = ControlledDesktopEffects()
        effects.loginStatus = .enabled
        let app = WhisperApplication(profile: profile.profile, credentials: profile.credentials, desktopEffects: effects)
        app.send(.bootstrapDesktop(launchedAtLogin: true))
        #expect(!app.state.desktop.mainWindowVisible)
        #expect(app.state.desktop.loginItemStatus == .enabled)
        #expect(effects.registrationRequests.isEmpty)
        app.send(.showMainWindow)
        app.send(.bootstrapDesktop(launchedAtLogin: true))
        #expect(app.state.desktop.mainWindowVisible)
    }

    @Test func loginRegistrationReportsApprovalAndFailureWithoutLosingPreferences() async throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let effects = ControlledDesktopEffects()
        let app = WhisperApplication(profile: profile.profile, credentials: profile.credentials, desktopEffects: effects)
        effects.approvalRequired = true
        app.send(.setLaunchAtLogin(true))
        await settle { !app.state.desktop.loginItemChangePending }
        #expect(app.state.desktop.loginItemStatus == .requiresApproval)
        app.send(.openLoginItemsSettings)
        #expect(effects.openedLoginSettings == 1)
        effects.loginStatus = .enabled
        app.send(.refreshLoginItemStatus)
        #expect(app.state.desktop.loginItemStatus == .enabled)
        effects.registrationFails = true
        app.send(.setLaunchAtLogin(false))
        await settle { !app.state.desktop.loginItemChangePending }
        #expect(app.state.desktop.loginItemChangeFailed)
        #expect(app.state.desktop.loginItemStatus == .enabled)
        effects.registrationFails = false
        app.send(.setLaunchAtLogin(false))
        await settle { !app.state.desktop.loginItemChangePending }
        #expect(app.state.desktop.loginItemStatus == .disabled)
        let reopened = WhisperApplication(profile: profile.profile, credentials: profile.credentials, desktopEffects: effects)
        reopened.send(.bootstrapDesktop(launchedAtLogin: false))
        #expect(reopened.state.desktop.loginItemStatus == .disabled)
        #expect(reopened.state.settings.desktop == DesktopPreferences())
    }

    @Test func cuesWaitForValidAudioAndMediaResumesBeforeServerCompletion() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        var preferences = DesktopPreferences()
        preferences.pauseMediaOnDictation = true
        fixture.app.send(.saveDesktopPreferences(preferences))
        let capture = fixture.begin()
        capture.open()
        await settle { fixture.app.state.dictation.timing["acquisitionCompleted"] != nil }
        #expect(fixture.desktop.cues.isEmpty)
        #expect(fixture.desktop.mediaEvents.isEmpty)
        capture.deliver([Float](repeating: 0, count: 4800))
        await settle { fixture.app.state.dictation.phase == .recording && fixture.desktop.mediaEvents.count == 1 }
        #expect(fixture.desktop.cues == [.ready])
        // Allow the real capture writer's feedback cadence to publish another frame.
        try await Task.sleep(for: .milliseconds(100))
        capture.deliver([Float](repeating: 0.2, count: 4800))
        await settle { fixture.app.state.dictation.duration > 0.1 }
        #expect(fixture.desktop.cues == [.ready])
        fixture.app.send(.stopDictation)
        await settle { fixture.desktop.mediaEvents == ["pause", "resume"] }
        #expect(fixture.desktop.cues == [.ready, .stopped])
        #expect(fixture.app.state.dictation.phase == .processing)
        await settle { await fixture.transport.requests.count == 1 }
        await fixture.transport.reply()
        await settle { fixture.app.state.dictation.phase == .result }
        #expect(fixture.desktop.mediaEvents == ["pause", "resume"])
    }

    @Test func shortTapAndPreparationCancellationNeverPlayCuesOrPauseMedia() async throws {
        let fixture = try ShortcutFixture()
        defer { fixture.remove() }
        var preferences = DesktopPreferences()
        preferences.pauseMediaOnDictation = true
        fixture.app.send(.saveDesktopPreferences(preferences))
        fixture.key()
        let capture = try #require(fixture.microphones.sessions.last)
        capture.open()
        capture.deliver([Float](repeating: 0, count: 4800))
        await settle { fixture.app.state.dictation.timing["firstAudio"] != nil }
        #expect(fixture.desktop.cues.isEmpty)
        fixture.key(down: false)
        fixture.clock.advance(WhisperApplication.doubleTapWindow)
        #expect(fixture.app.state.dictation.cancellation == .rejectedGesture)
        #expect(fixture.desktop.cues.isEmpty)
        #expect(fixture.desktop.mediaEvents.isEmpty)
        #expect(fixture.app.state.recordingPill.feedback == .idle)
    }

    @Test(arguments: [UInt16(8), UInt16(kVK_Command)])
    func ordinaryCommandCombinationRejectsProvisionalEffects(conflictingKey: UInt16) async throws {
        let fixture = try ShortcutFixture()
        defer { fixture.remove() }
        var preferences = DesktopPreferences()
        preferences.pauseMediaOnDictation = true
        fixture.app.send(.saveDesktopPreferences(preferences))
        fixture.key()
        let capture = try #require(fixture.microphones.sessions.last)
        capture.open()
        capture.deliver([Float](repeating: 0, count: 4800))
        await settle { fixture.app.state.dictation.timing["firstAudio"] != nil }
        fixture.key(conflictingKey)
        fixture.key(conflictingKey, down: false)
        fixture.key(down: false)
        fixture.clock.advance(1)
        await fixture.app.prepareForTermination()
        #expect(fixture.app.state.dictation.cancellation == .rejectedGesture)
        #expect(fixture.desktop.cues.isEmpty)
        #expect(fixture.desktop.mediaEvents.isEmpty)
        #expect(fixture.paste.activations.isEmpty)
        #expect(!capture.physicallyOpen)
    }

    @Test(arguments: [true, false])
    func holdEffectsRequireBothGestureAcceptanceAndSourceReadiness(audioArrivesFirst: Bool) async throws {
        let fixture = try ShortcutFixture()
        defer { fixture.remove() }
        var preferences = DesktopPreferences()
        preferences.pauseMediaOnDictation = true
        fixture.app.send(.saveDesktopPreferences(preferences))
        fixture.key()
        let capture = try #require(fixture.microphones.sessions.last)
        capture.open()
        if audioArrivesFirst {
            capture.deliver([Float](repeating: 0, count: 4800))
            await settle { fixture.app.state.dictation.timing["firstAudio"] != nil }
            #expect(fixture.desktop.cues.isEmpty)
            #expect(fixture.desktop.mediaEvents.isEmpty)
            fixture.clock.advance(WhisperApplication.holdThreshold)
        } else {
            fixture.clock.advance(WhisperApplication.holdThreshold)
            #expect(fixture.app.state.dictation.phase == .preparing)
            #expect(fixture.desktop.cues.isEmpty)
            #expect(fixture.desktop.mediaEvents.isEmpty)
            capture.deliver([Float](repeating: 0, count: 4800))
        }
        await settle { fixture.desktop.mediaEvents == ["pause"] }
        #expect(fixture.desktop.cues == [.ready])
        #expect(fixture.app.state.dictation.phase == .recording)
        fixture.app.send(.cancelDictation)
        await fixture.app.prepareForTermination()
        #expect(fixture.desktop.mediaEvents == ["pause", "resume"])
        #expect(fixture.desktop.cues == [.ready])
    }

    @Test func handsFreeFeedbackAndCueDoNotActivateTheTargetApp() async throws {
        let fixture = try ShortcutFixture()
        defer { fixture.remove() }
        let capture = await fixture.doubleTap()
        #expect(fixture.app.state.recordingPill.feedback == .handsFree)
        #expect(fixture.desktop.cues == [.ready])
        #expect(fixture.paste.activations.isEmpty)
        fixture.key()
        fixture.key(8)
        fixture.key(8, down: false)
        fixture.key(down: false)
        #expect(fixture.app.state.recordingPill.feedback == .handsFree)
        #expect(fixture.desktop.cues == [.ready])
        #expect(fixture.paste.activations.isEmpty)
        capture.fail(.inputUnavailable)
        await settle { fixture.app.state.recordingPill.feedback == .failed }
        #expect(fixture.paste.activations.isEmpty)
    }

    @Test func disabledCuesAndAlreadyPausedMediaHaveNoUnownedResume() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        var preferences = DesktopPreferences()
        preferences.audioCuesEnabled = false
        preferences.pauseMediaOnDictation = true
        fixture.desktop.mediaPlaying = false
        fixture.app.send(.saveDesktopPreferences(preferences))
        _ = await fixture.record()
        await settle { fixture.desktop.mediaEvents == ["pause"] }
        fixture.app.send(.cancelDictation)
        await fixture.app.prepareForTermination()
        #expect(fixture.desktop.cues.isEmpty)
        #expect(fixture.desktop.mediaEvents == ["pause"])
    }

    @Test func cancellationAndRetryBalanceALatePauseBeforeTheNewOwner() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        var preferences = DesktopPreferences()
        preferences.pauseMediaOnDictation = true
        fixture.app.send(.saveDesktopPreferences(preferences))
        fixture.desktop.suspendPause = true
        let old = await fixture.record()
        await settle { fixture.desktop.pauseContinuation != nil }
        fixture.app.send(.cancelDictation)
        _ = await fixture.record()
        fixture.desktop.suspendPause = false
        fixture.desktop.resolvePause()
        await settle { fixture.desktop.mediaEvents == ["pause", "resume", "pause"] }
        old.fail(.inputUnavailable)
        #expect(fixture.app.state.dictation.phase == .recording)
        fixture.app.send(.cancelDictation)
        await fixture.app.prepareForTermination()
        #expect(fixture.desktop.mediaEvents == ["pause", "resume", "pause", "resume"])
        #expect(fixture.desktop.cues == [.ready, .ready])
    }

    @Test func disablingPausePreferenceResumesItsExistingOwner() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        var preferences = DesktopPreferences()
        preferences.pauseMediaOnDictation = true
        fixture.app.send(.saveDesktopPreferences(preferences))
        _ = await fixture.record()
        await settle { fixture.desktop.mediaEvents == ["pause"] }
        preferences.pauseMediaOnDictation = false
        fixture.app.send(.saveDesktopPreferences(preferences))
        await settle { fixture.desktop.mediaEvents == ["pause", "resume"] }
        fixture.app.send(.cancelDictation)
        await fixture.app.prepareForTermination()
        #expect(fixture.desktop.mediaEvents == ["pause", "resume"])
    }

    @Test func teardownBalancesADelayedOwnedPause() async throws {
        let profile = try ProfileFixture(keychain: false)
        defer { profile.remove() }
        let microphones = ControlledMicrophones()
        let effects = ControlledDesktopEffects()
        effects.suspendPause = true
        var app: WhisperApplication? = WhisperApplication(profile: profile.profile, credentials: profile.credentials,
            microphones: microphones, desktopEffects: effects)
        app?.send(.saveASR(.init(serverURL: "http://localhost", model: "fixture"), credential: .unchanged))
        var preferences = DesktopPreferences()
        preferences.pauseMediaOnDictation = true
        app?.send(.saveDesktopPreferences(preferences))
        app?.send(.startDictation)
        let capture = try #require(microphones.sessions.last)
        capture.open()
        capture.deliver([Float](repeating: 0, count: 4800))
        await settle { effects.pauseContinuation != nil }
        weak let released = app
        app = nil
        #expect(released == nil)
        effects.resolvePause()
        await settle { effects.mediaEvents == ["pause", "resume"] }
        #expect(!capture.physicallyOpen)
    }

    @Test func autoHiddenPillShowsFeedbackWithoutOpeningTheMainWindow() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        var preferences = DesktopPreferences()
        preferences.floatingIconAutoHide = true
        preferences.pillVisible = false
        preferences.startMinimized = true
        fixture.app.send(.saveDesktopPreferences(preferences))
        fixture.app.send(.bootstrapDesktop(launchedAtLogin: false))
        #expect(!fixture.app.state.recordingPill.visible)
        let capture = fixture.begin()
        #expect(fixture.app.state.recordingPill.feedback == .preparing)
        #expect(fixture.app.state.recordingPill.visible)
        capture.open()
        capture.deliver([Float](repeating: 0, count: 4800))
        await settle { fixture.app.state.recordingPill.feedback == .hold }
        fixture.app.send(.cancelDictation)
        #expect(fixture.app.state.recordingPill.feedback == .cancelled)
        fixture.clock.advance(1)
        #expect(!fixture.app.state.recordingPill.visible)
        #expect(!fixture.app.state.desktop.mainWindowVisible)
    }

    @Test func errorAndCopyRecoveryRemainVisibleUntilDismissed() async throws {
        let fixture = try ShortcutFixture()
        defer { fixture.remove() }
        var preferences = DesktopPreferences()
        preferences.floatingIconAutoHide = true
        fixture.app.send(.saveDesktopPreferences(preferences))
        let capture = await fixture.hold()
        capture.fail(.inputUnavailable)
        await settle { fixture.app.state.dictation.phase == .failed }
        #expect(fixture.app.state.recordingPill.feedback == .failed)
        #expect(fixture.app.state.recordingPill.visible)
        fixture.app.send(.dismissPillFeedback)
        #expect(!fixture.app.state.recordingPill.visible)
        fixture.key(down: false)
        fixture.paste.allowWrite = false
        _ = await fixture.hold()
        await fixture.submit()
        await settle { fixture.app.state.dictation.phase == .result }
        fixture.clock.advance(5)
        #expect(fixture.app.state.recordingPill.feedback == .recovery)
        #expect(fixture.app.state.recordingPill.visible)
        fixture.app.send(.dismissPillFeedback)
        #expect(!fixture.app.state.recordingPill.visible)
        #expect(fixture.app.state.dictation.text == "Fixture transcript")
    }

    @Test func successfulAutoHideAndAnOldCancellationTimerCannotHideANewRecording() async throws {
        let fixture = try DictationFixture()
        defer { fixture.remove() }
        var preferences = DesktopPreferences()
        preferences.floatingIconAutoHide = true
        fixture.app.send(.saveDesktopPreferences(preferences))
        _ = await fixture.record()
        fixture.app.send(.stopDictation)
        await settle { await fixture.transport.requests.count == 1 }
        await fixture.transport.reply()
        await settle { fixture.app.state.dictation.phase == .result }
        #expect(fixture.app.state.recordingPill.feedback == .completed)
        fixture.clock.advance(1)
        #expect(!fixture.app.state.recordingPill.visible)
        _ = await fixture.record()
        fixture.app.send(.cancelDictation)
        _ = await fixture.record()
        fixture.clock.advance(1)
        #expect(fixture.app.state.recordingPill.visible)
        #expect(fixture.app.state.recordingPill.feedback == .hold)
    }
}
