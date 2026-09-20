import Foundation

public struct CopyRecoveryState: Equatable, Sendable {
    public var revision: UUID?
    public var requestID: UUID?
    public var isPresented = false
    public var isHovered = false
    public var isFocused = false
    public var isHeld: Bool { isHovered || isFocused }
    public init() {}
}

extension ApplicationState {
    public var canDismissCopyRecovery: Bool {
        !dictation.phase.isActive && recordingPill.feedback == .recovery && recordingPill.visible
    }
}

extension WhisperApplication {
    func recordingPillAction() {
        if state.recordingPill.feedback == .idle { startDictation(origin: .pill) }
        else if state.dictation.phase == .recording { stopDictation() }
        else if state.dictation.phase == .result { startDictation(origin: .pill) }
        else if state.dictation.phase == .failed || state.dictation.phase == .idle { startDictation(origin: .pill) }
    }

    func beginCopyRecovery(requestID: UUID) {
        guard state.dictation.requestID == requestID, case .recovery = state.dictation.delivery else { return }
        if state.desktop.copyRecovery.requestID == requestID, state.desktop.copyRecovery.revision != nil { return }
        resetCopyRecovery()
        state.desktop.copyRecovery.requestID = requestID
        state.desktop.copyRecovery.revision = UUID()
    }

    func setCopyRecoveryPresented(_ revision: UUID, _ presented: Bool) {
        guard state.desktop.copyRecovery.revision == revision,
              state.desktop.copyRecovery.isPresented != presented else { return }
        state.desktop.copyRecovery.isPresented = presented
        scheduleCopyRecoveryDismissal()
    }

    func setCopyRecoveryHeld(_ revision: UUID, _ held: Bool) {
        guard state.desktop.copyRecovery.revision == revision,
              state.desktop.copyRecovery.isHovered != held else { return }
        state.desktop.copyRecovery.isHovered = held
        scheduleCopyRecoveryDismissal()
    }

    func setCopyRecoveryFocused(_ revision: UUID, _ focused: Bool) {
        guard state.desktop.copyRecovery.revision == revision,
              state.desktop.copyRecovery.isFocused != focused else { return }
        state.desktop.copyRecovery.isFocused = focused
        scheduleCopyRecoveryDismissal()
    }

    func resetCopyRecovery() {
        copyRecoveryDeadline?.cancel()
        copyRecoveryDeadline = nil
        state.desktop.copyRecovery = CopyRecoveryState()
    }

    private func scheduleCopyRecoveryDismissal() {
        copyRecoveryDeadline?.cancel()
        copyRecoveryDeadline = nil
        guard let revision = state.desktop.copyRecovery.revision,
              state.desktop.copyRecovery.isPresented, !state.desktop.copyRecovery.isHeld else { return }
        // Match the reference: every release of hover/focus/copy hold starts a fresh five seconds.
        copyRecoveryDeadline = clock.schedule(after: 5) { [weak self] in
            guard let self, self.state.desktop.copyRecovery.revision == revision,
                  self.state.desktop.copyRecovery.isPresented, !self.state.desktop.copyRecovery.isHeld else { return }
            self.send(.dismissPillFeedback)
        }
    }
}
