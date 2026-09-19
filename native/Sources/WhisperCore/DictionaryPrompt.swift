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
        let uniqueText = Set(normalizedText.components(separatedBy: " "))
        let promptWords = Set(normalizedPrompt.components(separatedBy: " "))
        let matches = uniqueText.intersection(promptWords).count
        let composition = Double(matches) / Double(uniqueText.count)
        // Self-hosted ASR uses only exact/full-coverage echoes. Fragment retries belong to local engines.
        return composition >= 0.9 && Double(matches) / Double(promptWords.count) >= 0.7
    }

    private static func normalize(_ value: String) -> String {
        value.lowercased()
            .replacingOccurrences(of: "[^\\p{L}\\p{N}\\s]", with: "", options: .regularExpression)
            .replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
