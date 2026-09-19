import Foundation

public enum DictionarySource: String, Codable, Sendable {
    case manual, learned
}

public struct DictionaryEntry: Codable, Equatable, Identifiable, Sendable {
    public let id: UUID
    public var word: String
    public var source: DictionarySource
    public let createdAt: Date

    public init(word: String, source: DictionarySource = .manual) {
        self.id = UUID()
        self.word = word
        self.source = source
        self.createdAt = Date()
    }
}

public enum DictionaryFailure: Error, Equatable, Sendable {
    case emptyInput, duplicate, missingEntry, unreadable, saveFailed, exportFailed

    public func message(in language: AppLanguage) -> String {
        switch self {
        case .emptyInput: language.text("Enter a word or paste a comma-separated or newline-separated list.", "请输入词条，或粘贴用逗号或换行分隔的列表。")
        case .duplicate: language.text("This word is already in your dictionary.", "词典中已存在此词条。")
        case .missingEntry: language.text("This word has been removed. Refresh the dictionary and try again.", "此词条已被删除。请刷新词典后重试。")
        case .unreadable: language.text("The dictionary could not be read. Your saved file has been preserved.", "无法读取词典。已保留已保存的文件。")
        case .saveFailed: language.text("The dictionary could not be saved. Your changes have not been applied.", "无法保存词典。更改尚未应用。")
        case .exportFailed: language.text("The dictionary could not be exported. Check the destination and try again.", "无法导出词典。请检查目标位置后重试。")
        }
    }
}

public struct DictionaryState: Equatable, Sendable {
    public var entries: [DictionaryEntry] = []
    public var failure: DictionaryFailure?
    public var addedCount: Int?
    public var exportCompleted = false
    public var words: [String] { entries.map(\.word) }
    public var promptCharacters: Int { words.joined(separator: ", ").utf16.count }
    public init() {}
}

private struct DictionaryDocument: Codable {
    var version = 1
    var entries: [DictionaryEntry] = []
}

extension ProfileStore {
    func loadDictionary() throws -> [DictionaryEntry] {
        do {
            guard let document: DictionaryDocument = try read("dictionary.json") else { return [] }
            guard document.version == 1 else { throw DictionaryFailure.unreadable }
            var keys = Set<String>()
            guard document.entries.allSatisfy({ entry in
                !entry.word.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && keys.insert(entry.word.lowercased()).inserted
            }) else { throw DictionaryFailure.unreadable }
            return document.entries
        } catch { throw DictionaryFailure.unreadable }
    }

    func saveDictionary(_ entries: [DictionaryEntry]) throws {
        do { try write(DictionaryDocument(entries: entries), to: "dictionary.json") }
        catch { throw DictionaryFailure.saveFailed }
    }
}

extension WhisperApplication {
    func loadDictionary() {
        do {
            state.dictionary.entries = try profileStore.loadDictionary()
            state.dictionary.failure = nil
        } catch { state.dictionary.failure = .unreadable }
    }

    /// Delta writes preserve words learned after a page took its snapshot.
    func changeDictionary(add: [String], remove: [String], source: DictionarySource) {
        mutateDictionary { entries in
            let additions = Self.normalizedDictionaryWords(add)
            let additionKeys = Set(additions.map { $0.lowercased() })
            let removals = Set(Self.normalizedDictionaryWords(remove).map { $0.lowercased() }).subtracting(additionKeys)
            entries.removeAll { removals.contains($0.word.lowercased()) }
            var added = 0
            for word in additions {
                if let index = entries.firstIndex(where: { $0.word.lowercased() == word.lowercased() }) {
                    entries[index].word = word
                    if source == .manual { entries[index].source = .manual }
                } else {
                    entries.append(DictionaryEntry(word: word, source: source))
                    added += 1
                }
            }
            return added
        }
    }

    func importDictionary(_ text: String) {
        let words = text.components(separatedBy: CharacterSet(charactersIn: ",\n"))
        guard !Self.normalizedDictionaryWords(words).isEmpty else {
            state.dictionary.failure = .emptyInput
            state.dictionary.addedCount = nil
            return
        }
        changeDictionary(add: words, remove: [], source: .manual)
    }

    func editDictionaryWord(_ original: String, replacement: String) {
        mutateDictionary { entries in
            let word = replacement.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !word.isEmpty else { throw DictionaryFailure.emptyInput }
            guard let index = entries.firstIndex(where: { $0.word.lowercased() == original.lowercased() }) else {
                throw DictionaryFailure.missingEntry
            }
            guard !entries.enumerated().contains(where: { $0.offset != index && $0.element.word.lowercased() == word.lowercased() }) else {
                throw DictionaryFailure.duplicate
            }
            if entries[index].word.lowercased() == word.lowercased() {
                entries[index].word = word
                entries[index].source = .manual
            } else {
                entries.remove(at: index)
                entries.append(DictionaryEntry(word: word))
            }
            return 0
        }
    }

    private func mutateDictionary(_ change: (inout [DictionaryEntry]) throws -> Int) {
        state.dictionary.failure = nil
        state.dictionary.addedCount = nil
        state.dictionary.exportCompleted = false
        do {
            var entries = try profileStore.loadDictionary()
            let added = try change(&entries)
            try profileStore.saveDictionary(entries)
            state.dictionary.entries = entries
            state.dictionary.addedCount = added
        } catch {
            state.dictionary.failure = error as? DictionaryFailure ?? .saveFailed
        }
    }

    func exportDictionary(to destination: URL) {
        state.dictionary.exportCompleted = false
        state.dictionary.addedCount = nil
        do {
            let entries = try profileStore.loadDictionary()
            try entries.map(\.word).joined(separator: "\n").write(to: destination, atomically: true, encoding: .utf8)
            state.dictionary.exportCompleted = true
            state.dictionary.failure = nil
        } catch { state.dictionary.failure = .exportFailed }
    }

    /// Snippet triggers join the same vocabulary here when the Snippets workflow is installed.
    func dictionaryHintWords() -> [String] { state.dictionary.words }

    private static func normalizedDictionaryWords(_ words: [String]) -> [String] {
        var seen = Set<String>()
        return words.compactMap {
            let word = $0.trimmingCharacters(in: .whitespacesAndNewlines)
            return !word.isEmpty && seen.insert(word.lowercased()).inserted ? word : nil
        }
    }
}
