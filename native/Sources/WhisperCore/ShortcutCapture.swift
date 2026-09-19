import Foundation

public struct ShortcutCaptureState: Equatable, Sendable {
    public var isActive: Bool
    public var editingIndex: Int?
    public var held = ""
    public var candidate = ""
    public init(isActive: Bool = false, editingIndex: Int? = nil) {
        self.isActive = isActive; self.editingIndex = editingIndex
    }
}

extension WhisperApplication {
    func saveShortcuts(_ values: [String]) {
        state.settingsSaved = false
        do {
            let normalized = try ShortcutBinding.validate(values)
            var settings = state.settings
            settings.shortcuts = normalized
            persist(settings)
            state.shortcutError = nil
        } catch {
            state.shortcutError = error as? ShortcutConfigurationError ?? .unsupportedKey
        }
    }

    func receiveShortcutCapture(_ input: ShortcutInput) {
        guard !input.isRepeat else { return }
        let modifiers = pressedKeys.intersection(ShortcutInput.modifierKeyCodes)
        state.shortcutCapture.held = modifiers.sorted().compactMap {
            $0 == 63 ? "GLOBE" : ShortcutKeys.modifierName($0)
        }.joined(separator: "+")
        if ShortcutInput.modifierKeyCodes.contains(input.keyCode) {
            if input.isDown { captureModifierPeak.formUnion(modifiers); return }
            guard !captureModifierPeak.isEmpty else { return }
            let candidate = captureModifierPeak.sorted().compactMap {
                $0 == 63 ? "GLOBE" : ShortcutKeys.modifierName($0)
            }.joined(separator: "+")
            captureModifierPeak = []
            finishShortcutCapture(candidate)
        } else if input.isDown {
            guard let key = input.keyName ?? ShortcutKeys.keyName(input.keyCode) else {
                state.shortcutError = .unsupportedKey; return
            }
            for pair: Set<UInt16> in [[54, 55], [56, 60], [58, 61], [59, 62]] where pair.isSubset(of: modifiers) {
                state.shortcutError = .mixedSides
                return
            }
            var tokens = Set(modifiers.compactMap { ShortcutKeys.modifierName($0).map(ShortcutKeys.unsided) })
            if modifiers.contains(63) { tokens.insert("GLOBE") }
            let ordered = tokens.sorted { ShortcutKeys.modifierOrder($0) < ShortcutKeys.modifierOrder($1) }
            finishShortcutCapture((ordered + [key]).joined(separator: "+"))
            captureModifierPeak = []
        }
    }

    private func finishShortcutCapture(_ value: String) {
        state.shortcutCapture.candidate = value
        var values = state.settings.shortcuts
        if let index = state.shortcutCapture.editingIndex, values.indices.contains(index) { values[index] = value }
        else { values.append(value) }
        saveShortcuts(values)
        if state.shortcutError == nil, state.configurationError == nil { state.shortcutCapture.isActive = false }
    }
}
