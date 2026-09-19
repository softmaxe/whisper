import Foundation

/// Native port of OpenWhispr src/utils/snippets.ts at 6d56d75.
struct SnippetExpansion {
    private let regex: NSRegularExpression?
    private let replacements: [String: String]

    init(snippets: [Snippet]) {
        var replacements: [String: String] = [:]
        var keys: [String] = []
        func put(_ key: String, _ value: String) {
            if replacements[key] == nil { keys.append(key) }
            replacements[key] = value
        }
        for snippet in snippets {
            let folded = Self.foldCapitalIDot(snippet.trigger.trimmingCharacters(in: .whitespacesAndNewlines).precomposedStringWithCanonicalMapping)
            let key = folded.lowercased()
            guard !key.isEmpty else { continue }
            put(key, snippet.replacement)
            let dotless = folded.replacingOccurrences(of: "I", with: "ı").lowercased()
            if replacements[dotless] == nil { put(dotless, snippet.replacement) }
        }
        self.replacements = replacements
        let alternatives = keys.enumerated().sorted {
            let first = $0.element.utf16.count, second = $1.element.utf16.count
            return first == second ? $0.offset < $1.offset : first > second
        }.map {
            NSRegularExpression.escapedPattern(for: $0.element)
                .replacingOccurrences(of: "i", with: "[iİ]")
                .replacingOccurrences(of: "ı", with: "[ıI]")
        }
        // ECMAScript whitespace includes BOM but not ICU's U+0085 next-line character.
        let boundary = "[\\p{Z}\\u0009-\\u000D\\uFEFF\\p{P}\\p{S}]"
        self.regex = alternatives.isEmpty ? nil : try? NSRegularExpression(
            pattern: "(?<=^|" + boundary + ")(?:" + alternatives.joined(separator: "|") + ")(?=$|" + boundary + ")",
            options: [.caseInsensitive]
        )
    }

    func expand(_ text: String) -> String {
        guard !text.isEmpty, let regex else { return text }
        let normalized = text.precomposedStringWithCanonicalMapping
        let input = normalized as NSString
        let matches = regex.matches(in: normalized, range: NSRange(location: 0, length: input.length))
        var result = ""
        var previous = 0
        for match in matches {
            result += input.substring(with: NSRange(location: previous, length: match.range.location - previous))
            let value = input.substring(with: match.range)
            let folded = Self.foldCapitalIDot(value)
            // The regex can match a dotless-I alternative without a matching lowercase key.
            result += replacements[folded.lowercased()]
                ?? replacements[folded.replacingOccurrences(of: "I", with: "ı").lowercased()] ?? value
            previous = NSMaxRange(match.range)
        }
        result += input.substring(from: previous)
        return result
    }

    private static func foldCapitalIDot(_ value: String) -> String {
        value.replacingOccurrences(of: "İ", with: "i")
    }
}
