import Foundation

public struct Snippet: Codable, Equatable, Identifiable, Sendable {
    public let id: UUID
    public var trigger: String
    public var replacement: String
    public static let maximumTriggerLength = 100

    public init(trigger: String, replacement: String) {
        self.id = UUID()
        self.trigger = trigger
        self.replacement = replacement
    }
}

public enum SnippetsFailure: Error, Equatable, Sendable {
    case emptyTrigger, emptyReplacement, triggerTooLong, duplicate, missingEntry, unreadable, saveFailed

    public func message(in language: AppLanguage) -> String {
        switch self {
        case .emptyTrigger: language.text("Enter the words you want to speak.", "请输入要说出的触发词。")
        case .emptyReplacement: language.text("Enter the text to insert when you say the trigger.", "请输入说出触发词时要插入的文本。")
        case .triggerTooLong: language.text("The trigger is too long. Use at most 100 characters.", "触发词过长，请使用不超过 100 个字符。")
        case .duplicate: language.text("A snippet with this trigger already exists.", "已存在使用此触发词的片段。")
        case .missingEntry: language.text("This snippet was removed. Refresh the page and try again.", "此片段已被删除。请刷新页面后重试。")
        case .unreadable: language.text("Snippets could not be read. Your saved file has been preserved.", "无法读取片段。已保留已保存的文件。")
        case .saveFailed: language.text("Snippets could not be saved. Your changes have not been applied.", "无法保存片段。更改尚未应用。")
        }
    }
}

public struct SnippetsState: Equatable, Sendable {
    public var entries: [Snippet] = []
    public var failure: SnippetsFailure?
    public var saved = false
    public init() {}
}

private struct SnippetsDocument: Codable {
    var version = 1
    var entries: [Snippet] = []
}

extension ProfileStore {
    func loadSnippets() throws -> [Snippet] {
        do {
            guard let document: SnippetsDocument = try read("snippets.json") else { return [] }
            var keys = Set<String>()
            var ids = Set<UUID>()
            guard document.version == 1, document.entries.allSatisfy({ entry in
                !entry.trigger.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                    && !entry.replacement.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                    && entry.trigger.utf16.count <= Snippet.maximumTriggerLength
                    && keys.insert(entry.trigger.lowercased()).inserted && ids.insert(entry.id).inserted
            }) else { throw SnippetsFailure.unreadable }
            return document.entries
        } catch { throw SnippetsFailure.unreadable }
    }

    func saveSnippets(_ entries: [Snippet]) throws {
        do { try write(SnippetsDocument(entries: entries), to: "snippets.json") }
        catch { throw SnippetsFailure.saveFailed }
    }
}

extension WhisperApplication {
    func loadSnippets() {
        do {
            state.snippets.entries = try profileStore.loadSnippets()
            snippetExpansion = SnippetExpansion(snippets: state.snippets.entries)
            state.snippets.failure = nil
        } catch { state.snippets.failure = .unreadable }
    }

    func saveSnippet(trigger: String, replacement: String, editingID: UUID?) {
        mutateSnippets { entries in
            let trigger = trigger.trimmingCharacters(in: .whitespacesAndNewlines)
            let replacement = replacement.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trigger.isEmpty else { throw SnippetsFailure.emptyTrigger }
            guard !replacement.isEmpty else { throw SnippetsFailure.emptyReplacement }
            guard trigger.utf16.count <= Snippet.maximumTriggerLength else { throw SnippetsFailure.triggerTooLong }
            guard !entries.contains(where: { $0.id != editingID && $0.trigger.lowercased() == trigger.lowercased() }) else {
                throw SnippetsFailure.duplicate
            }
            if let editingID {
                guard let index = entries.firstIndex(where: { $0.id == editingID }) else { throw SnippetsFailure.missingEntry }
                entries[index].trigger = trigger
                entries[index].replacement = replacement
            } else {
                entries.append(Snippet(trigger: trigger, replacement: replacement))
            }
        }
    }

    /// Whole-list persistence retains existing row identities and skips invalid or repeated input.
    func setSnippets(_ snippets: [Snippet]) {
        mutateSnippets { entries in
            var seen = Set<String>()
            let normalized = snippets.compactMap { snippet -> Snippet? in
                var result = snippet
                result.trigger = snippet.trigger.trimmingCharacters(in: .whitespacesAndNewlines)
                result.replacement = snippet.replacement.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !result.trigger.isEmpty, !result.replacement.isEmpty,
                      result.trigger.utf16.count <= Snippet.maximumTriggerLength,
                      seen.insert(result.trigger.lowercased()).inserted else { return nil }
                return result
            }
            let incoming = Dictionary(uniqueKeysWithValues: normalized.map { ($0.trigger.lowercased(), $0) })
            // Existing insertion order survives replacement, matching the retained database contract.
            entries = entries.compactMap { entry in
                guard let value = incoming[entry.trigger.lowercased()] else { return nil }
                var updated = entry
                updated.trigger = value.trigger
                updated.replacement = value.replacement
                return updated
            }
            let existing = Set(entries.map { $0.trigger.lowercased() })
            entries.append(contentsOf: normalized.filter { !existing.contains($0.trigger.lowercased()) }.map {
                Snippet(trigger: $0.trigger, replacement: $0.replacement)
            })
        }
    }

    func deleteSnippet(_ id: UUID) {
        mutateSnippets { $0.removeAll { $0.id == id } }
    }

    private func mutateSnippets(_ change: (inout [Snippet]) throws -> Void) {
        state.snippets.failure = nil
        state.snippets.saved = false
        do {
            var entries = try profileStore.loadSnippets()
            try change(&entries)
            try profileStore.saveSnippets(entries)
            state.snippets.entries = entries
            snippetExpansion = SnippetExpansion(snippets: entries)
            state.snippets.saved = true
        } catch { state.snippets.failure = error as? SnippetsFailure ?? .saveFailed }
    }

    /// Live Dictation enters here after cleanup and Chinese conversion. Upload and retry skip it.
    func completeDictationWithSnippets(rawText: String, text: String, requestID: UUID) {
        guard drainInputAndCheckDictation(requestID) else { return }
        completeDictation(rawText: rawText, text: snippetExpansion.expand(text), requestID: requestID)
    }
}
