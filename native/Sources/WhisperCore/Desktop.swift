import Foundation

public enum AppTheme: String, Codable, CaseIterable, Sendable { case light, dark, system }
public enum PillPlacement: String, Codable, CaseIterable, Sendable {
    case bottomRight = "bottom-right", center, bottomLeft = "bottom-left"
}

public struct DesktopPreferences: Codable, Equatable, Sendable {
    public var theme: AppTheme = .system
    public var audioCuesEnabled = true
    public var pauseMediaOnDictation = false
    public var pillVisible = true
    public var floatingIconAutoHide = false
    public var panelStartPosition: PillPlacement = .bottomRight
    public var showMenuBarIcon = true
    public var startMinimized = false
    public init() {}

    public init(from decoder: any Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        theme = try values.decodeIfPresent(AppTheme.self, forKey: .theme) ?? .system
        audioCuesEnabled = try values.decodeIfPresent(Bool.self, forKey: .audioCuesEnabled) ?? true
        pauseMediaOnDictation = try values.decodeIfPresent(Bool.self, forKey: .pauseMediaOnDictation) ?? false
        pillVisible = try values.decodeIfPresent(Bool.self, forKey: .pillVisible) ?? true
        floatingIconAutoHide = try values.decodeIfPresent(Bool.self, forKey: .floatingIconAutoHide) ?? false
        panelStartPosition = try values.decodeIfPresent(PillPlacement.self, forKey: .panelStartPosition) ?? .bottomRight
        showMenuBarIcon = try values.decodeIfPresent(Bool.self, forKey: .showMenuBarIcon) ?? true
        startMinimized = try values.decodeIfPresent(Bool.self, forKey: .startMinimized) ?? false
    }
}

public enum LoginItemStatus: String, Equatable, Sendable {
    case disabled, enabled, requiresApproval, unavailable
    public var isRegistered: Bool { self == .enabled || self == .requiresApproval }
}
public enum DictationCue: Equatable, Sendable { case ready, stopped }
public enum MediaPauseOwnership: Equatable, Sendable { case adapter, mediaKey }
public enum PillFeedback: String, Equatable, Sendable {
    case idle, preparing, hold, handsFree, processing, cleaning, completed, failed, cancelled, recovery, learned
    public func title(in language: AppLanguage) -> String {
        switch self {
        case .learned: language.text("Dictionary updated", "词典已更新")
        case .idle: language.text("Ready to record", "等待录音")
        case .preparing: language.text("Preparing microphone…", "正在准备麦克风…")
        case .hold: language.text("Listening", "正在聆听")
        case .handsFree: language.text("Hands-free Dictation", "免按键听写")
        case .processing: language.text("Transcribing…", "正在转录…")
        case .cleaning: language.text("Cleaning up text…", "正在整理文字…")
        case .completed: language.text("Transcription complete", "转录完成")
        case .failed: language.text("Try again", "请重试")
        case .cancelled: language.text("Cancelled", "已取消")
        case .recovery: language.text("Text ready to copy", "文字已就绪，可复制")
        }
    }
}

public struct RecordingPillPresentation: Equatable, Sendable {
    public let visible: Bool
    public let feedback: PillFeedback
    public let placement: PillPlacement
    public let theme: AppTheme
}

public struct DesktopState: Equatable, Sendable {
    public var mainWindowVisible = true
    public var loginItemStatus: LoginItemStatus = .disabled
    public var loginItemChangePending = false
    public var loginItemChangeFailed = false
    public var transientFeedback: PillFeedback?
    public var dismissedRequestID: UUID?
    public init() {}
}

@MainActor public protocol DesktopEffects: AnyObject {
    func playCue(_ cue: DictationCue)
    func pauseMedia() async -> MediaPauseOwnership?
    func resumeMedia(_ ownership: MediaPauseOwnership) async
    func loginItemStatus() -> LoginItemStatus
    func setLaunchAtLogin(_ enabled: Bool) async throws -> LoginItemStatus
    func openLoginItemsSettings()
}

/// Library consumers and tests are inert unless the production desktop adapter is explicitly supplied.
@MainActor public final class InertDesktopEffects: DesktopEffects {
    public init() {}
    public func playCue(_ cue: DictationCue) {}
    public func pauseMedia() async -> MediaPauseOwnership? { nil }
    public func resumeMedia(_ ownership: MediaPauseOwnership) async {}
    public func loginItemStatus() -> LoginItemStatus { .unavailable }
    public func setLaunchAtLogin(_ enabled: Bool) async throws -> LoginItemStatus { .unavailable }
    public func openLoginItemsSettings() {}
}

extension ApplicationState {
    public var recordingPill: RecordingPillPresentation {
        let feedback: PillFeedback
        let recovery: Bool
        if case .recovery = dictation.delivery { recovery = true } else { recovery = false }
        let learning = !corrections.learned.isEmpty && !dictation.phase.isActive
        if learning { feedback = .learned }
        else if recovery { feedback = .recovery }
        else if let transient = desktop.transientFeedback, !dictation.phase.isActive { feedback = transient }
        else {
            feedback = switch dictation.phase {
            case .idle, .result: .idle
            case .preparing: .preparing
            case .recording: dictation.origin == .handsFree ? .handsFree : .hold
            case .processing: dictation.isCleaning ? .cleaning : .processing
            case .failed: desktop.dismissedRequestID == dictation.requestID ? .idle : .failed
            }
        }
        let needsFeedback = dictation.phase.isActive || recovery || learning || feedback == .failed
            || desktop.transientFeedback != nil
        return RecordingPillPresentation(
            visible: !isTerminating && (needsFeedback || (settings.desktop.pillVisible && !settings.desktop.floatingIconAutoHide)),
            feedback: feedback, placement: settings.desktop.panelStartPosition, theme: settings.desktop.theme
        )
    }
}

extension WhisperApplication {
    func bootstrapDesktop(launchedAtLogin: Bool) {
        guard !desktopBootstrapped else { return }
        desktopBootstrapped = true
        state.desktop.mainWindowVisible = !launchedAtLogin && !state.settings.desktop.startMinimized
        state.desktop.loginItemStatus = desktopEffects.loginItemStatus()
    }

    func saveDesktopPreferences(_ preferences: DesktopPreferences) {
        let wasPausing = state.settings.desktop.pauseMediaOnDictation
        var settings = state.settings
        settings.desktop = preferences
        persist(settings)
        guard state.settings.desktop == preferences else { return }
        if wasPausing && !preferences.pauseMediaOnDictation, let owner = readinessFeedbackOwner {
            mediaOwnership.release(owner: owner)
        } else if !wasPausing && preferences.pauseMediaOnDictation,
                  state.dictation.phase == .recording, let owner = readinessFeedbackOwner {
            mediaOwnership.acquire(owner: owner)
        }
    }

    func dictationBecameReady() {
        guard state.dictation.phase == .recording, let id = state.dictation.requestID,
              readinessFeedbackOwner != id else { return }
        readinessFeedbackOwner = id
        if state.settings.desktop.audioCuesEnabled { desktopEffects.playCue(.ready) }
        if state.settings.desktop.pauseMediaOnDictation { mediaOwnership.acquire(owner: id) }
    }

    func endRecordingFeedback(requestID: UUID, stopped: Bool) {
        mediaOwnership.release(owner: requestID)
        guard readinessFeedbackOwner == requestID else { return }
        readinessFeedbackOwner = nil
        if stopped && state.settings.desktop.audioCuesEnabled { desktopEffects.playCue(.stopped) }
    }

    func resetDesktopFeedback() {
        pillFeedbackDeadline?.cancel()
        pillFeedbackDeadline = nil
        state.desktop.transientFeedback = nil
        state.desktop.dismissedRequestID = nil
    }

    func showPillFeedback(_ feedback: PillFeedback, requestID: UUID) {
        pillFeedbackDeadline?.cancel()
        state.desktop.transientFeedback = feedback
        pillFeedbackDeadline = clock.schedule(after: 0.5) { [weak self] in
            guard self?.state.dictation.requestID == requestID else { return }
            self?.state.desktop.transientFeedback = nil
            self?.pillFeedbackDeadline = nil
        }
    }

    func setLaunchAtLogin(_ enabled: Bool) {
        guard !state.desktop.loginItemChangePending else { return }
        state.desktop.loginItemChangePending = true
        state.desktop.loginItemChangeFailed = false
        let effects = desktopEffects
        Task { [weak self] in
            do {
                let status = try await effects.setLaunchAtLogin(enabled)
                self?.state.desktop.loginItemStatus = status
                self?.state.desktop.loginItemChangeFailed = status == .unavailable
            } catch {
                let status = effects.loginItemStatus()
                self?.state.desktop.loginItemStatus = status
                self?.state.desktop.loginItemChangeFailed = enabled ? !status.isRegistered : status != .disabled
            }
            self?.state.desktop.loginItemChangePending = false
        }
    }

    public func prepareForTermination() async {
        state.isTerminating = true
        let insightsRead = insightsReadTask
        insightsRead?.cancel()
        insightsReadTask = nil
        insightsGeneration += 1
        state.insights.isLoading = false
        retentionTimer?.cancel()
        retentionTimer = nil
        cancelDictation()
        cancelHistoryRetry()
        stopHistoryPlayback()
        cancelCleanupTest()
        correctionLearning.stop()
        pillFeedbackDeadline?.cancel()
        workflowTasks.cancelAll()
        await cancelUploadsAndWait()
        await mediaOwnership.shutdown()
        await workflowTasks.waitForAll()
        await insightsRead?.value
        await flushHistoryWrites()
    }
}

/// Serializes ownership across slow media probes, cancellation and immediate retry.
@MainActor final class MediaOwnership {
    private let effects: any DesktopEffects
    private var desiredOwner: UUID?
    private var paused: (owner: UUID, token: MediaPauseOwnership)?
    private var tail: Task<Void, Never>?
    init(effects: any DesktopEffects) { self.effects = effects }

    func acquire(owner: UUID) {
        desiredOwner = owner
        enqueue { [self] in
            guard desiredOwner == owner else { return }
            if let paused, paused.owner == owner { return }
            if let paused { self.paused = nil; await effects.resumeMedia(paused.token) }
            guard desiredOwner == owner else { return }
            if let token = await effects.pauseMedia() {
                if desiredOwner == owner { paused = (owner, token) }
                else { await effects.resumeMedia(token) }
            }
        }
    }

    func release(owner: UUID) {
        if desiredOwner == owner { desiredOwner = nil }
        enqueue { [self] in
            guard let paused, paused.owner == owner else { return }
            self.paused = nil
            await effects.resumeMedia(paused.token)
        }
    }

    func releaseAll() {
        desiredOwner = nil
        enqueue { [self] in
            if let paused { self.paused = nil; await effects.resumeMedia(paused.token) }
        }
    }

    func shutdown() async {
        releaseAll()
        await tail?.value
    }

    private func enqueue(_ operation: @escaping @MainActor () async -> Void) {
        let previous = tail
        tail = Task { await previous?.value; await operation() }
    }
}
