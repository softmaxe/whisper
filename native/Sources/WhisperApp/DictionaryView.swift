import AppKit
import SwiftUI
import UniformTypeIdentifiers
import WhisperCore

struct DictionaryPage: View {
    let application: WhisperApplication
    @State private var tab = DictionaryTab.words
    private var language: AppLanguage { application.state.settings.language }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Picker(language.text("Dictionary sections", "词典栏目"), selection: $tab) {
                Text(language.text("Dictionary", "词典")).tag(DictionaryTab.words)
                Text(language.text("Snippets", "片段")).tag(DictionaryTab.snippets)
            }
            .pickerStyle(.segmented).labelsHidden().frame(width: 230)
            .accessibilityIdentifier("dictionary-tabs")
            switch tab {
            case .words: DictionaryWordsPage(application: application)
            case .snippets: SnippetsPage(application: application)
            }
        }
    }
}

private enum DictionaryTab { case words, snippets }

private struct DictionaryWordsPage: View {
    let application: WhisperApplication
    @State private var newWord = ""
    @State private var bulkText = ""
    @State private var showingImport = false
    @State private var editingWord: String?
    @State private var editValue = ""
    @State private var confirmingClear = false
    @State private var wordsToClear: [String] = []
    @FocusState private var addFocused: Bool
    private var language: AppLanguage { application.state.settings.language }
    private var dictionary: DictionaryState { application.state.dictionary }
    private var promptCharacters: Int {
        (dictionary.words + application.state.snippets.entries.map(\.trigger)).joined(separator: ", ").utf16.count
    }
    private var visibleEntries: [DictionaryEntry] {
        let search = newWord.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return dictionary.entries.filter { search.isEmpty || $0.word.lowercased().contains(search) }
    }
    private var pendingCount: Int {
        bulkText.components(separatedBy: CharacterSet(charactersIn: ",\n"))
            .filter { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }.count
    }

    var body: some View {
        let visible = visibleEntries
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 8) {
                TextField(language.text("Add words separated by commas", "用逗号分隔添加多个词"), text: $newWord)
                    .textFieldStyle(.roundedBorder).focused($addFocused)
                    .onSubmit(addWords).accessibilityIdentifier("dictionary-add-input")
                Button(language.text("Add", "添加"), action: addWords)
                    .disabled(newWord.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    .accessibilityIdentifier("dictionary-add")
                Button { showingImport.toggle() } label: { Image(systemName: "square.and.arrow.down") }
                    .help(language.text("Import words", "导入词条"))
                    .accessibilityLabel(language.text("Import words", "导入词条"))
            }
            CorrectionLearningFeedback(application: application)
            if showingImport { importForm }
            if let failure = dictionary.failure {
                Label(failure.message(in: language), systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.red).font(.caption).accessibilityIdentifier("dictionary-error")
            } else if let count = dictionary.addedCount {
                Text(count == 0 ? language.text("Dictionary saved.", "词典已保存。")
                    : language.text("Added \(count) words.", "已添加 \(count) 个词条。"))
                    .font(.caption).foregroundStyle(.secondary).accessibilityIdentifier("dictionary-saved")
            } else if dictionary.exportCompleted {
                Text(language.text("Dictionary exported.", "词典已导出。"))
                    .font(.caption).foregroundStyle(.secondary)
            }
            VStack(alignment: .leading, spacing: 10) {
                if dictionary.entries.isEmpty {
                    emptyState
                } else {
                    HStack {
                        Text(language.text("Your dictionary", "您的词典")).fontWeight(.semibold)
                        Text("\(dictionary.entries.count)").foregroundStyle(.secondary)
                        Spacer()
                        Button(language.text("Clear all", "清空全部")) {
                            wordsToClear = dictionary.words
                            confirmingClear = true
                        }.buttonStyle(.plain).foregroundStyle(.secondary)
                        Button(action: exportWords) { Image(systemName: "square.and.arrow.up") }
                            .buttonStyle(.plain).accessibilityLabel(language.text("Export dictionary", "导出词典"))
                    }.font(.caption)
                    Divider()
                    if visible.isEmpty {
                        Text(language.text("No matches. Press Enter to add \"\(newWord.trimmingCharacters(in: .whitespacesAndNewlines))\".", "无匹配项。按 Enter 添加“\(newWord.trimmingCharacters(in: .whitespacesAndNewlines))”。"))
                            .foregroundStyle(.secondary).frame(maxWidth: .infinity).padding(.vertical, 24)
                    }
                    LazyVStack(spacing: 8) {
                        ForEach(visible) { entry in
                            wordRow(entry)
                            if entry.id != visible.last?.id { Divider().opacity(0.5) }
                        }
                    }
                }
            }
            .padding(16).background(.primary.opacity(0.025))
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.primary.opacity(0.12)))
            if promptCharacters > 550 {
                Text(language.text(
                    "Your dictionary uses \(promptCharacters) prompt characters. Whisper models may only use part of a long list; other models can support larger dictionaries.",
                    "词典提示包含 \(promptCharacters) 个字符。Whisper 模型可能仅使用较长列表的一部分；其他模型可能支持更大的词典。"
                )).font(.caption).foregroundStyle(.secondary)
            }
        }
        .onAppear { application.send(.refreshDictionary) }
        .confirmationDialog(language.text("Clear your dictionary?", "清空词典？"), isPresented: $confirmingClear) {
            Button(language.text("Clear all", "清空全部"), role: .destructive) {
                application.send(.changeDictionary(add: [], remove: wordsToClear))
            }
            Button(language.text("Cancel", "取消"), role: .cancel) {}
        } message: {
            Text(language.text("This removes all words currently shown in your dictionary.", "此操作将删除词典中当前的全部词条。"))
        }
    }

    private var importForm: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(language.text("Paste words separated by commas or new lines", "粘贴用逗号或换行分隔的词条"))
                .font(.caption).foregroundStyle(.secondary)
            TextEditor(text: $bulkText).frame(height: 84)
                .accessibilityLabel(language.text("Words to import", "待导入词条"))
                .accessibilityIdentifier("dictionary-import-input")
            HStack {
                Text(language.text("\(pendingCount) words ready", "\(pendingCount) 个词条待导入"))
                    .font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button(language.text("Cancel", "取消")) { bulkText = ""; showingImport = false }
                Button(language.text("Import", "导入")) {
                    application.send(.importDictionary(bulkText))
                    if dictionary.failure == nil { bulkText = ""; showingImport = false }
                }.buttonStyle(.borderedProminent).disabled(pendingCount == 0)
                    .accessibilityIdentifier("dictionary-import")
            }
        }.padding(12).overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.tint.opacity(0.4)))
    }

    private var emptyState: some View {
        VStack(spacing: 10) {
            Image(systemName: "book").font(.title2).foregroundStyle(.tint)
            Text(language.text("Your dictionary is empty", "词典为空")).fontWeight(.semibold)
            Text(language.text("Add names, tools, and acronyms for Whisper to recognize.", "添加姓名、工具名称和缩写，帮助 Whisper 识别您的用词。"))
                .font(.caption).foregroundStyle(.secondary).multilineTextAlignment(.center)
            Button(language.text("Add your first word", "添加第一个词条")) { addFocused = true }
                .buttonStyle(.borderedProminent)
            Button(language.text("Import a list", "导入列表")) { showingImport = true }.buttonStyle(.plain)
        }.frame(maxWidth: .infinity).padding(.vertical, 22)
    }

    private func wordRow(_ entry: DictionaryEntry) -> some View {
        HStack {
            if editingWord == entry.word {
                TextField(language.text("Edit word", "编辑词条"), text: $editValue)
                    .textFieldStyle(.roundedBorder).onSubmit(commitEdit)
                    .onExitCommand { editingWord = nil }
                Button(language.text("Save", "保存"), action: commitEdit)
                Button(language.text("Cancel", "取消")) { editingWord = nil }
            } else {
                Text(entry.word).textSelection(.enabled)
                Spacer()
                if entry.source == .learned {
                    Text(language.text("Learned", "已学习")).font(.caption2).foregroundStyle(.secondary)
                }
                Button { editingWord = entry.word; editValue = entry.word } label: { Image(systemName: "pencil") }
                    .accessibilityLabel(language.text("Edit \(entry.word)", "编辑 \(entry.word)"))
                Button { application.send(.changeDictionary(add: [], remove: [entry.word])) } label: { Image(systemName: "xmark") }
                    .accessibilityLabel(language.text("Remove \(entry.word)", "删除 \(entry.word)"))
            }
        }.buttonStyle(.plain).frame(minHeight: 28)
    }

    private func addWords() {
        application.send(.importDictionary(newWord))
        if dictionary.failure == nil, (dictionary.addedCount ?? 0) > 0 { newWord = "" }
    }

    private func commitEdit() {
        guard let editingWord else { return }
        application.send(.editDictionaryWord(editingWord, replacement: editValue))
        if dictionary.failure == nil { self.editingWord = nil }
    }

    private func exportWords() {
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.plainText]
        panel.nameFieldStringValue = "whisper-dictionary.txt"
        panel.begin { response in
            if response == .OK, let destination = panel.url { application.send(.exportDictionary(destination)) }
        }
    }
}
