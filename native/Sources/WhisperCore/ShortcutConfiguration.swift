import Foundation

public enum ShortcutConfigurationError: Error, Equatable, Sendable {
    case empty, duplicate, reserved, tooManyKeys, modifierRequired, rightModifierRequired
    case modifierChordUnsupported, fnCombination, mouseCombination, mixedSides, unsupportedKey
    case globeUnavailable, globeRestoreUnavailable
    public func message(in language: AppLanguage) -> String {
        switch self {
        case .empty: language.text("Keep at least one shortcut.", "请至少保留一个快捷键。")
        case .duplicate: language.text("That shortcut is already in use.", "此快捷键已被使用。")
        case .reserved: language.text("This shortcut is reserved by the system.", "此快捷键由系统保留。")
        case .tooManyKeys: language.text("Shortcuts are limited to three keys.", "快捷键最多包含三个按键。")
        case .modifierRequired: language.text("Add a modifier to a letter or number shortcut.", "字母或数字快捷键需要搭配修饰键。")
        case .rightModifierRequired: language.text("Use a right-side modifier on its own, or add another key.", "单独使用修饰键时请选择右侧按键，或添加其他按键。")
        case .modifierChordUnsupported: language.text("Modifier-only combinations are not supported. Add a regular key.", "不支持仅由修饰键组成的组合。请添加普通按键。")
        case .fnCombination: language.text("Globe/Fn can only be used by itself.", "Globe/Fn 只能单独使用。")
        case .mouseCombination: language.text("Mouse buttons cannot be combined with other keys.", "鼠标按键不能与其他按键组合。")
        case .mixedSides: language.text("Do not mix left and right versions of the same modifier.", "请勿在同一快捷键中混用同一种修饰键的左右按键。")
        case .unsupportedKey: language.text("That shortcut is not supported.", "不支持此快捷键。")
        case .globeUnavailable: language.text("The macOS Globe action could not be suspended safely. Choose another shortcut.", "无法安全暂停 macOS Globe 操作。请选择其他快捷键。")
        case .globeRestoreUnavailable: language.text("The previous macOS Globe action could not be restored. Its recovery record has been kept.", "无法恢复之前的 macOS Globe 操作。已保留恢复记录。")
        }
    }
}

public struct ShortcutBinding: Equatable, Sendable {
    public let value: String
    let modifiers: [String]
    let key: String
    public var isGlobe: Bool { key == "GLOBE" }
    public var isMouse: Bool { key == "MouseButton4" || key == "MouseButton5" }
    public var isStandaloneModifier: Bool { modifiers.isEmpty && ShortcutKeys.modifierCodes[key] != nil }

    public init(_ value: String) throws {
        let parts = value.split(separator: "+", omittingEmptySubsequences: false).map { ShortcutKeys.normalize(String($0)) }
        guard !parts.isEmpty, parts.allSatisfy({ !$0.isEmpty }) else { throw ShortcutConfigurationError.empty }
        guard parts.count <= 3 else { throw ShortcutConfigurationError.tooManyKeys }
        if parts.contains("GLOBE"), parts.count > 1 { throw ShortcutConfigurationError.fnCombination }
        if parts.contains(where: { $0.hasPrefix("MouseButton") }), parts.count > 1 { throw ShortcutConfigurationError.mouseCombination }
        let modifiers = parts.filter { ShortcutKeys.modifierCodes[$0] != nil }
        for family in ["Control", "Command", "Alt", "Shift"] {
            if modifiers.contains("Left" + family), modifiers.contains("Right" + family) { throw ShortcutConfigurationError.mixedSides }
        }
        let keys = parts.filter { ShortcutKeys.modifierCodes[$0] == nil }
        if keys.isEmpty {
            guard modifiers.count == 1 else { throw ShortcutConfigurationError.modifierChordUnsupported }
            guard modifiers[0].hasPrefix("Right") else { throw ShortcutConfigurationError.rightModifierRequired }
            self.modifiers = []
            key = modifiers[0]
            self.value = key
            return
        }
        guard keys.count == 1, Set(modifiers).count == modifiers.count else { throw ShortcutConfigurationError.unsupportedKey }
        let key = keys[0]
        guard ShortcutKeys.codes[key] != nil || ShortcutKeys.specialKeys.contains(key) else { throw ShortcutConfigurationError.unsupportedKey }
        guard !modifiers.isEmpty || ShortcutKeys.specialKeys.contains(key) else { throw ShortcutConfigurationError.modifierRequired }
        guard key != "Esc" || !modifiers.isEmpty else { throw ShortcutConfigurationError.reserved }
        self.modifiers = modifiers.sorted { ShortcutKeys.modifierOrder($0) < ShortcutKeys.modifierOrder($1) }
        self.key = key
        self.value = (self.modifiers + [key]).joined(separator: "+")
        let unsided = (self.modifiers.map { ShortcutKeys.unsided($0) } + [key]).joined(separator: "+")
        guard !Self.reserved.contains(unsided) else { throw ShortcutConfigurationError.reserved }
    }

    public static func validate(_ values: [String]) throws -> [String] {
        guard !values.isEmpty else { throw ShortcutConfigurationError.empty }
        let normalized = try values.map { try ShortcutBinding($0).value }
        let bindings = try normalized.map(ShortcutBinding.init)
        for index in bindings.indices {
            for previous in bindings[..<index] where bindings[index].overlaps(previous) {
                throw ShortcutConfigurationError.duplicate
            }
        }
        return normalized
    }

    public func label(in language: AppLanguage) -> String {
        value.split(separator: "+").map { part in
            switch part {
            case "GLOBE": "Globe/Fn"
            case "MouseButton4": language.text("Mouse Button 4", "鼠标按键 4")
            case "MouseButton5": language.text("Mouse Button 5", "鼠标按键 5")
            case "RightCommand": language.text("Right Command", "右 Command")
            case "LeftCommand": language.text("Left Command", "左 Command")
            case "RightAlt": language.text("Right Option", "右 Option")
            case "LeftAlt": language.text("Left Option", "左 Option")
            case "RightControl": language.text("Right Control", "右 Control")
            case "LeftControl": language.text("Left Control", "左 Control")
            case "RightShift": language.text("Right Shift", "右 Shift")
            case "LeftShift": language.text("Left Shift", "左 Shift")
            case "Alt": "Option"
            default: String(part)
            }
        }.joined(separator: " + ")
    }

    func matches(_ input: ShortcutInput, pressed: Set<UInt16>) -> Bool {
        let keyMatches = ShortcutKeys.codes[key] == input.keyCode || input.keyName.map(ShortcutKeys.normalize) == key
        return keyMatches && isDown(pressed: pressed, baseCode: input.keyCode) && pressed.isSubset(of: allowedCodes.union([input.keyCode]))
    }
    func isDown(pressed: Set<UInt16>, baseCode: UInt16) -> Bool {
        pressed.contains(baseCode) && modifiers.allSatisfy { !(ShortcutKeys.modifierCodes[$0] ?? []).isDisjoint(with: pressed) }
    }
    var allowedCodes: Set<UInt16> { Set(modifiers.flatMap { ShortcutKeys.modifierCodes[$0] ?? [] }) }
    private func overlaps(_ other: Self) -> Bool {
        guard key == other.key, modifiers.count == other.modifiers.count else { return false }
        return modifiers.allSatisfy { modifier in
            other.modifiers.contains { otherModifier in
                ShortcutKeys.unsided(modifier) == ShortcutKeys.unsided(otherModifier)
                    && !(ShortcutKeys.modifierCodes[modifier] ?? []).isDisjoint(with: ShortcutKeys.modifierCodes[otherModifier] ?? [])
            }
        }
    }

    // The exact macOS reserved list from src/utils/hotkeyValidator.ts, normalized in modifier order.
    static let reserved = Set([
        "Command+C", "Command+V", "Command+X", "Command+Z", "Command+Shift+Z", "Command+A",
        "Command+Q", "Command+W", "Command+R", "Command+T", "Command+S", "Command+P", "Command+N",
        "Command+M", "Command+H", "Command+F", "Command+G", "Command+Shift+G", "Command+,",
        "Command+Left", "Command+Right", "Command+Up", "Command+Down", "Command+Shift+Left",
        "Command+Shift+Right", "Command+Shift+Up", "Command+Shift+Down", "Control+Command+F",
        "Command+Space", "Command+Alt+Space", "Command+Shift+3", "Command+Shift+4", "Command+Shift+5",
        "Command+Alt+Esc", "Command+Alt+D", "Command+Delete", "Command+Shift+Delete", "Command+Shift+Q",
        "Command+B", "Command+I", "Command+U", "Command+Shift+T", "Command+=", "Command+-",
        "Command+Alt+F", "Command+Shift+F", "GLOBE+F11", "GLOBE+F12"
    ])
}

enum ShortcutKeys {
    static let modifierCodes: [String: Set<UInt16>] = [
        "Command": [54, 55], "LeftCommand": [55], "RightCommand": [54],
        "Control": [59, 62], "LeftControl": [59], "RightControl": [62],
        "Alt": [58, 61], "LeftAlt": [58], "RightAlt": [61],
        "Shift": [56, 60], "LeftShift": [56], "RightShift": [60]
    ]
    static let codes: [String: UInt16] = [
        "A": 0, "S": 1, "D": 2, "F": 3, "H": 4, "G": 5, "Z": 6, "X": 7, "C": 8, "V": 9,
        "B": 11, "Q": 12, "W": 13, "E": 14, "R": 15, "Y": 16, "T": 17, "1": 18, "2": 19,
        "3": 20, "4": 21, "6": 22, "5": 23, "=": 24, "9": 25, "7": 26, "-": 27, "8": 28,
        "0": 29, "]": 30, "O": 31, "U": 32, "[": 33, "I": 34, "P": 35, "Enter": 36,
        "L": 37, "J": 38, "'": 39, "K": 40, ";": 41, "\\": 42, ",": 43, "/": 44, "N": 45,
        "M": 46, ".": 47, "Tab": 48, "Space": 49, "`": 50, "Backspace": 51, "Esc": 53,
        "RightCommand": 54, "RightShift": 60, "RightAlt": 61, "RightControl": 62, "GLOBE": 63,
        "F17": 64, "numdec": 65, "nummult": 67, "numadd": 69, "NumLock": 71, "numdiv": 75,
        "numsub": 78, "F18": 79, "F19": 80, "num0": 82, "num1": 83, "num2": 84, "num3": 85, "num4": 86, "num5": 87,
        "num6": 88, "num7": 89, "F20": 90, "num8": 91, "num9": 92, "F5": 96, "F6": 97,
        "F7": 98, "F3": 99, "F8": 100, "F9": 101, "F11": 103, "F13": 105, "F16": 106,
        "F14": 107, "F10": 109, "F12": 111, "F15": 113, "Insert": 114, "Home": 115,
        "PageUp": 116, "Delete": 117, "F4": 118, "End": 119, "F2": 120, "PageDown": 121,
        "F1": 122, "Left": 123, "Right": 124, "Down": 125, "Up": 126,
        "MouseButton4": 0x1003, "MouseButton5": 0x1004,
        "MediaPlayPause": 0x2010, "MediaNextTrack": 0x2011, "MediaPreviousTrack": 0x2012, "MediaStop": 0x2034
    ]
    static let specialKeys = Set(["GLOBE", "MouseButton4", "MouseButton5", "Esc", "Tab", "Space",
        "Backspace", "Insert", "Delete", "Home", "End", "PageUp", "PageDown", "Left", "Right", "Up", "Down",
        "PrintScreen", "Pause", "ScrollLock", "NumLock"] + (1...24).map { "F\($0)" })
    static func unsided(_ token: String) -> String { token.replacingOccurrences(of: "Left", with: "").replacingOccurrences(of: "Right", with: "") }
    static func modifierOrder(_ token: String) -> Int { ["Control", "Command", "Alt", "Shift"].firstIndex(of: unsided(token)) ?? 4 }
    static func normalize(_ token: String) -> String {
        let trimmed = token.trimmingCharacters(in: .whitespacesAndNewlines)
        let compact = (trimmed.count > 1 ? trimmed.replacingOccurrences(of: "-", with: "") : trimmed)
            .replacingOccurrences(of: " ", with: "").replacingOccurrences(of: "_", with: "").lowercased()
        if ["fn", "globe"].contains(compact) { return "GLOBE" }
        let sides = [("left", "Left"), ("right", "Right")]
        var base = compact
        var side = ""
        for (source, destination) in sides {
            if base.hasPrefix(source) { side = destination; base.removeFirst(source.count); break }
            if base.hasSuffix(source) { side = destination; base.removeLast(source.count); break }
        }
        let aliases = ["ctrl": "Control", "control": "Control", "cmd": "Command", "command": "Command",
            "commandorcontrol": "Command", "cmdorctrl": "Command", "meta": "Command", "super": "Command",
            "win": "Command", "option": "Alt", "alt": "Alt", "shift": "Shift"]
        if let modifier = aliases[base] { return side + modifier }
        let keyAliases = ["escape": "Esc", "return": "Enter", "arrowleft": "Left", "arrowright": "Right", "arrowup": "Up", "arrowdown": "Down"]
        if let key = keyAliases[compact] { return key }
        return (Array(codes.keys) + Array(specialKeys)).first { $0.lowercased() == compact } ?? trimmed.uppercased()
    }
    static func keyName(_ code: UInt16) -> String? { code == 76 ? "Enter" : codes.first { $0.value == code }?.key }
    static func modifierName(_ code: UInt16) -> String? {
        modifierCodes.first { ($0.key.hasPrefix("Left") || $0.key.hasPrefix("Right")) && $0.value == [code] }?.key
    }
}
