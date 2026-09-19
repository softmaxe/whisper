import Foundation

/// Physical key edges, shared by the native event adapter and workflow fixtures.
public struct ShortcutInput: Equatable, Sendable {
    public var keyCode: UInt16
    public var isDown: Bool
    public var isRepeat: Bool
    public var heldModifiers: Set<UInt16>?
    public init(keyCode: UInt16, isDown: Bool, isRepeat: Bool = false, heldModifiers: Set<UInt16>? = nil) {
        self.keyCode = keyCode
        self.isDown = isDown
        self.isRepeat = isRepeat
        self.heldModifiers = heldModifiers
    }
    public static let rightCommand: UInt16 = 54
    public static let escape: UInt16 = 53
    public static let modifierKeyCodes: Set<UInt16> = [54, 55, 56, 58, 59, 60, 61, 62, 63]
}

public enum DictationOrigin: String, Sendable { case button, hold }
public enum DictationGesture: String, Sendable { case none, candidate, hold }
public enum DictationCancellation: String, Sendable { case user, rejectedGesture }

extension WhisperApplication {
    /// Provisional threshold inherited from the legacy hold path; hardware acceptance is separate.
    public static let holdThreshold: TimeInterval = 0.150

    func receiveShortcut(_ input: ShortcutInput) {
        let wasDown = pressedKeys.contains(input.keyCode)
        if let held = input.heldModifiers {
            pressedKeys.subtract(ShortcutInput.modifierKeyCodes)
            pressedKeys.formUnion(held)
        }
        if input.isDown { pressedKeys.insert(input.keyCode) }
        else { pressedKeys.remove(input.keyCode) }
        if input.keyCode == ShortcutInput.escape, input.isDown, !input.isRepeat {
            cancelDictation()
            return
        }
        if input.isDown, input.isRepeat || wasDown { return }

        if input.keyCode == ShortcutInput.rightCommand {
            if input.isDown {
                guard !state.dictation.phase.isActive, pressedKeys == [ShortcutInput.rightCommand] else { return }
                startDictation(origin: .hold)
                guard state.dictation.phase == .preparing, let id = state.dictation.requestID else { return }
                holdDeadline = clock.schedule(after: Self.holdThreshold) { [weak self] in
                    guard let self, self.isCurrentDictation(id), self.state.dictation.gesture == .candidate else { return }
                    self.holdDeadline = nil
                    self.state.dictation.gesture = .hold
                    if let failure = self.provisionalFailure {
                        self.failDictation(failure, requestID: id)
                        return
                    }
                    self.publishRecordingReadiness()
                }
            } else {
                guard state.dictation.origin == .hold,
                      state.dictation.phase == .preparing || state.dictation.phase == .recording else { return }
                holdDeadline?.cancel()
                holdDeadline = nil
                // A busy run loop can deliver release before the scheduled threshold callback.
                if state.dictation.gesture == .candidate, clock.now - dictationStartedAt >= Self.holdThreshold {
                    state.dictation.gesture = .hold
                    if let failure = provisionalFailure, let id = state.dictation.requestID {
                        failDictation(failure, requestID: id)
                        return
                    }
                    publishRecordingReadiness()
                }
                if state.dictation.gesture == .candidate {
                    cancelDictation(kind: .rejectedGesture)
                } else if state.dictation.gesture == .hold {
                    stopDictation()
                }
            }
        } else if input.isDown, state.dictation.origin == .hold,
                  state.dictation.phase == .preparing || state.dictation.phase == .recording {
            // Pass the ordinary combination through; provisional speech never becomes History.
            cancelDictation(kind: .rejectedGesture)
        }
    }

    func publishRecordingReadiness() {
        guard state.dictation.phase == .preparing,
              state.dictation.timing["firstAudio"] != nil,
              state.dictation.gesture != .candidate else { return }
        state.dictation.phase = .recording
        state.dictation.timing["readyFeedback"] = clock.now - dictationStartedAt
    }
}
