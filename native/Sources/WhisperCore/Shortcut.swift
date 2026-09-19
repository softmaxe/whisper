import Foundation

/// Physical key edges, shared by the native event adapter and workflow fixtures.
public struct ShortcutInput: Equatable, Sendable {
    public var keyCode: UInt16
    public var isDown: Bool
    public var isRepeat: Bool
    public var heldModifiers: Set<UInt16>?
    public var keyName: String?
    public init(keyCode: UInt16, isDown: Bool, isRepeat: Bool = false, heldModifiers: Set<UInt16>? = nil, keyName: String? = nil) {
        self.keyCode = keyCode
        self.isDown = isDown
        self.isRepeat = isRepeat
        self.heldModifiers = heldModifiers
        self.keyName = keyName
    }
    public static let rightCommand: UInt16 = 54
    public static let escape: UInt16 = 53
    public static let modifierKeyCodes: Set<UInt16> = [54, 55, 56, 58, 59, 60, 61, 62, 63]
}

public enum DictationOrigin: String, Sendable { case button, hold, handsFree }
public enum DictationGesture: String, Sendable {
    case none, candidate, awaitingSecondTap, secondTap, hold, handsFree, stopCandidate
    public var isProvisional: Bool { self == .candidate || self == .awaitingSecondTap || self == .secondTap }
}
public enum DictationCancellation: String, Sendable { case user, rejectedGesture }

extension WhisperApplication {
    /// Provisional threshold inherited from the legacy hold path; hardware acceptance is separate.
    public static let holdThreshold: TimeInterval = 0.150
    /// A second press must follow the first short release within this provisional window.
    public static let doubleTapWindow: TimeInterval = 0.300

    func receiveShortcut(_ input: ShortcutInput) {
        let wasDown = pressedKeys.contains(input.keyCode)
        if let held = input.heldModifiers {
            pressedKeys.subtract(ShortcutInput.modifierKeyCodes)
            pressedKeys.formUnion(held)
        }
        if input.isDown { pressedKeys.insert(input.keyCode) }
        else { pressedKeys.remove(input.keyCode) }
        if state.shortcutCapture.isActive {
            receiveShortcutCapture(input)
            return
        }
        if input.keyCode == ShortcutInput.escape, input.isDown, !input.isRepeat,
           state.dictation.phase.isActive,
           !(activeShortcut?.key == "Esc" && activeShortcut?.matches(input, pressed: pressedKeys) == true && state.dictation.phase != .processing) {
            cancelDictation()
            return
        }
        if input.isDown, input.isRepeat || wasDown { return }
        guard state.dictation.phase != .processing else { return }
        guard let trigger = resolveShortcutTrigger(input) else { return }

        if state.dictation.origin == .handsFree,
           state.dictation.phase == .preparing || state.dictation.phase == .recording {
            receiveHandsFreeShortcut(trigger)
            return
        }

        if trigger != .other {
            if trigger == .down {
                if state.dictation.gesture == .awaitingSecondTap,
                   state.dictation.phase == .preparing {
                    doubleTapDeadline?.cancel()
                    doubleTapDeadline = nil
                    if clock.now - firstTapReleasedAt <= Self.doubleTapWindow {
                        state.dictation.gesture = .secondTap
                        scheduleHoldRecognition()
                        return
                    }
                    cancelDictation(kind: .rejectedGesture)
                }
                guard !state.dictation.phase.isActive else { return }
                startDictation(origin: .hold)
                scheduleHoldRecognition()
            } else {
                guard state.dictation.origin == .hold,
                      state.dictation.phase == .preparing || state.dictation.phase == .recording else { return }
                holdDeadline?.cancel()
                holdDeadline = nil
                // A busy run loop can deliver release before the scheduled threshold callback.
                if (state.dictation.gesture == .candidate || state.dictation.gesture == .secondTap),
                   clock.now - gesturePressedAt >= Self.holdThreshold {
                    recognizeHold()
                }
                if state.dictation.gesture == .candidate {
                    guard let id = state.dictation.requestID else { return }
                    state.dictation.gesture = .awaitingSecondTap
                    firstTapReleasedAt = clock.now
                    doubleTapDeadline = clock.schedule(after: Self.doubleTapWindow) { [weak self] in
                        guard let self, self.isCurrentDictation(id), self.state.dictation.gesture == .awaitingSecondTap else { return }
                        self.doubleTapDeadline = nil
                        self.cancelDictation(kind: .rejectedGesture)
                    }
                } else if state.dictation.gesture == .secondTap {
                    state.dictation.origin = .handsFree
                    state.dictation.gesture = .handsFree
                    dictationTarget = nil
                    if let failure = provisionalFailure, let id = state.dictation.requestID {
                        failDictation(failure, requestID: id)
                    } else { publishRecordingReadiness() }
                } else if state.dictation.gesture == .hold {
                    stopDictation()
                }
            }
        } else if state.dictation.origin == .hold,
                  state.dictation.phase == .preparing || state.dictation.phase == .recording {
            // Pass the ordinary combination through; provisional speech never becomes History.
            cancelDictation(kind: .rejectedGesture)
        }
    }

    private func scheduleHoldRecognition() {
        guard state.dictation.phase == .preparing, let id = state.dictation.requestID else { return }
        gesturePressedAt = clock.now
        holdDeadline = clock.schedule(after: Self.holdThreshold) { [weak self] in
            guard let self, self.isCurrentDictation(id),
                  self.state.dictation.gesture == .candidate || self.state.dictation.gesture == .secondTap else { return }
            self.holdDeadline = nil
            self.recognizeHold()
        }
    }

    private func recognizeHold() {
        state.dictation.gesture = .hold
        if let failure = provisionalFailure, let id = state.dictation.requestID {
            failDictation(failure, requestID: id)
        } else { publishRecordingReadiness() }
    }

    private func receiveHandsFreeShortcut(_ trigger: ShortcutTrigger) {
        if trigger != .other {
            if trigger == .down {
                state.dictation.gesture = .stopCandidate
            } else if state.dictation.gesture == .stopCandidate {
                stopDictation()
            } else {
                state.dictation.gesture = .handsFree
            }
        } else {
            // Command shortcuts and typing keep Hands-free Dictation running.
            state.dictation.gesture = .handsFree
        }
    }

    private enum ShortcutTrigger { case down, up, other }
    private func resolveShortcutTrigger(_ input: ShortcutInput) -> ShortcutTrigger? {
        if !state.dictation.phase.isActive {
            guard input.isDown else { return nil }
            let bindings = state.settings.shortcuts.compactMap { try? ShortcutBinding($0) }
            guard let binding = bindings.first(where: { $0.matches(input, pressed: pressedKeys) }) else { return nil }
            activeShortcutCandidates = bindings
            activeShortcut = binding
            activeShortcutKey = input.keyCode
            shortcutPressActive = true
            return .down
        }
        guard state.dictation.origin != .button, let binding = activeShortcut else { return nil }
        if input.isDown, state.dictation.gesture.isProvisional,
           let longer = activeShortcutCandidates.first(where: { $0 != binding && $0.matches(input, pressed: pressedKeys) }) {
            activeShortcut = longer
            activeShortcutKey = input.keyCode
            shortcutPressActive = true
            holdDeadline?.cancel()
            doubleTapDeadline?.cancel()
            doubleTapDeadline = nil
            state.dictation.gesture = .candidate
            scheduleHoldRecognition()
            return nil
        }
        if input.isDown, binding.matches(input, pressed: pressedKeys) {
            guard !shortcutPressActive else { return nil }
            activeShortcutKey = input.keyCode
            shortcutPressActive = true
            return .down
        }
        if !input.isDown, shortcutPressActive, let base = activeShortcutKey,
           !binding.isDown(pressed: pressedKeys, baseCode: base) {
            shortcutPressActive = false
            return .up
        }
        if input.isDown, !binding.allowedCodes.contains(input.keyCode) { return .other }
        return nil
    }

    func publishRecordingReadiness() {
        guard state.dictation.phase == .preparing,
              state.dictation.timing["firstAudio"] != nil,
              !state.dictation.gesture.isProvisional else { return }
        state.dictation.phase = .recording
        state.dictation.timing["readyFeedback"] = clock.now - dictationStartedAt
        dictationBecameReady()
    }
}
