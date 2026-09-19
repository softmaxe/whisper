import Foundation

/// The event tap balances a consumed press even if configuration changes before release.
public struct ShortcutEventSuppression: Sendable {
    private var swallowed = Set<UInt16>()
    public init() {}
    public mutating func reset() { swallowed = [] }
    public mutating func consume(_ input: ShortcutInput, pressed: Set<UInt16>, bindings values: [String], capturing: Bool, dismissingRecovery: Bool = false) -> Bool {
        if capturing {
            let keyboard = input.keyCode != 0x1003 && input.keyCode != 0x1004
            if input.isDown, keyboard { swallowed.insert(input.keyCode) }
            if !input.isDown { swallowed.remove(input.keyCode) }
            return keyboard
        }
        if dismissingRecovery, input.keyCode == ShortcutInput.escape, input.isDown { swallowed.insert(input.keyCode) }
        let bindings = values.compactMap { try? ShortcutBinding($0) }
        if input.isDown, bindings.contains(where: {
            ($0.isMouse && ShortcutKeys.codes[$0.key] == input.keyCode)
                || (!$0.isGlobe && !$0.isStandaloneModifier && $0.matches(input, pressed: pressed))
        }) { swallowed.insert(input.keyCode) }
        let consume = swallowed.contains(input.keyCode)
        if !input.isDown { swallowed.remove(input.keyCode) }
        return consume
    }
}
