import Foundation

/// Ported from OpenWhispr dictionaryPromptCap.js and dictionaryEchoFilter.js at 6d56d75.
public enum DictionaryPrompt {
    public static func capped(_ prompt: String?, configuration: ASRConfiguration) -> String? {
        guard let prompt, !prompt.isEmpty else { return nil }
        let limit = configuration.serverURL.contains("api.groq.com") ? 890
            : configuration.model.lowercased().hasPrefix("gpt-4o") ? 8000 : 900
        guard prompt.utf16.count > limit else { return prompt }
        let head = String(decoding: prompt.utf16.prefix(limit), as: UTF16.self)
        if let comma = head.lastIndex(of: ","), comma != head.startIndex { return String(head[..<comma]) }
        return head
    }

    public static func isEcho(_ text: String, prompt: String?) -> Bool {
        guard let prompt else { return false }
        let normalizedText = normalize(text)
        let normalizedPrompt = normalize(prompt)
        guard !normalizedText.isEmpty, !normalizedPrompt.isEmpty else { return false }
        if normalizedText == normalizedPrompt { return true }
        let textWords = normalizedText.components(separatedBy: " ")
        let uniqueText = Set(textWords)
        let promptWords = Set(normalizedPrompt.components(separatedBy: " "))
        let matches = uniqueText.intersection(promptWords).count
        let composition = Double(matches) / Double(uniqueText.count)
        guard composition >= 0.9 else { return false }
        if Double(matches) / Double(promptWords.count) >= 0.7 { return true }

        var counts: [String: Int] = [:]
        for word in textWords {
            counts[word, default: 0] += 1
            if counts[word, default: 0] >= 3 { return true }
        }
        let delimiters = CharacterSet(charactersIn: ",、，")
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if let last = trimmed.unicodeScalars.last, delimiters.contains(last), normalizedText.utf16.count <= 30 { return true }
        guard text.rangeOfCharacter(from: delimiters) != nil else { return false }
        let terms = prompt.components(separatedBy: delimiters).map(normalize).filter { !$0.isEmpty }
        let sequence = terms.enumerated().flatMap { index, term in
            term.components(separatedBy: " ").map { (word: $0, term: index) }
        }
        guard textWords.count <= sequence.count else { return false }
        for start in 0...(sequence.count - textWords.count) {
            let end = start + textWords.count - 1
            if sequence[end].term - sequence[start].term + 1 >= 3,
               zip(sequence[start...end], textWords).allSatisfy({ $0.0.word == $0.1 }) { return true }
        }
        return false
    }

    private static func normalize(_ value: String) -> String {
        value.lowercased()
            .replacingOccurrences(of: "[^\\p{L}\\p{N}\\s]", with: "", options: .regularExpression)
            .replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
