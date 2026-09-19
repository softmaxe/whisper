import SwiftUI
import WhisperCore

struct HistoryView: View {
    let application: WhisperApplication
    @State private var searchPresented = false
    @State private var confirmClear = false
    private var language: AppLanguage { application.state.settings.language }
    private var history: HistoryState { application.state.history }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text(language.text("History", "历史记录")).font(.headline)
                Spacer()
                Button {
                    application.send(.dismissHistoryEntry)
                    application.send(.searchHistory(""))
                    searchPresented = true
                } label: { Label(language.text("Search", "搜索"), systemImage: "magnifyingglass") }
                .keyboardShortcut("k").accessibilityIdentifier("history-search")
                Menu {
                    Toggle(language.text("Show discarded recordings", "显示已丢弃的录音"), isOn: Binding(
                        get: { history.includeDiscarded }, set: { application.send(.showDiscardedHistory($0)) }
                    ))
                    Divider()
                    Button(language.text("Clear all", "清空全部"), role: .destructive) { confirmClear = true }
                        .disabled(history.totalCount == 0 || history.pendingChanges > 0)
                } label: { Image(systemName: "ellipsis") }
                .menuStyle(.borderlessButton).frame(width: 24)
                .accessibilityLabel(language.text("History actions", "历史记录操作"))
            }
            if !application.state.settings.history.enabled {
                Label(language.text("History is disabled. New results remain available to copy without being saved.", "历史记录已关闭。新结果仍可复制，但不会保存。"), systemImage: "archivebox")
                    .font(.caption).foregroundStyle(.orange).padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(.orange.opacity(0.06), in: RoundedRectangle(cornerRadius: 8))
            }
            if let failure = history.failure {
                Label(failure.message(in: language), systemImage: "exclamationmark.triangle")
                    .font(.caption).foregroundStyle(.red).accessibilityIdentifier("history-error")
            }
            if history.isLoading && history.entries.isEmpty {
                HStack { ProgressView().controlSize(.small); Text(language.text("Loading history…", "正在加载历史记录…")) }
                    .frame(maxWidth: .infinity).padding(32)
            } else if history.entries.isEmpty {
                VStack(spacing: 12) {
                    Image(systemName: "mic").font(.title2).foregroundStyle(.secondary)
                    Text(language.text("No recordings yet", "暂无录音")).font(.headline)
                    Text(language.text("Start dictation to see your history here.", "开始听写后，历史记录会显示在这里。"))
                        .foregroundStyle(.secondary).font(.caption)
                }
                .frame(maxWidth: .infinity).padding(36)
                .background(.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 16))
                .overlay(RoundedRectangle(cornerRadius: 16).strokeBorder(.primary.opacity(0.12)))
            } else {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(history.groups()) { group in
                        Text(group.title(in: language)).font(.caption).foregroundStyle(.secondary)
                            .padding(.top, 14).padding(.bottom, 9)
                        ForEach(group.entries) { entry in
                            HistoryEntryView(application: application, entry: entry)
                                .padding(16).background(.primary.opacity(0.025))
                                .overlay(Rectangle().strokeBorder(.primary.opacity(0.09)))
                        }
                    }
                }
                if history.hasMore {
                    Button(language.text(history.isLoading ? "Loading…" : "Load more", history.isLoading ? "正在加载…" : "加载更多")) {
                        application.send(.loadMoreHistory)
                    }
                    .disabled(history.isLoading).frame(maxWidth: .infinity)
                    .accessibilityIdentifier("history-load-more")
                }
            }
        }
        .onAppear { application.send(.loadHistory) }
        .onChange(of: history.selectedEntry?.id) {
            if history.selectedEntry != nil { searchPresented = true }
        }
        .alert(language.text("Clear all history?", "清空全部历史记录？"), isPresented: $confirmClear) {
            Button(language.text("Cancel", "取消"), role: .cancel) {}
            Button(language.text("Clear all", "清空全部"), role: .destructive) { application.send(.clearHistory) }
        } message: {
            Text(language.text("All history on this device will be permanently deleted. This cannot be undone.", "将永久删除此设备上的全部历史记录。此操作无法撤销。"))
        }
        .sheet(isPresented: $searchPresented, onDismiss: { application.send(.dismissHistoryEntry) }) {
            HistorySearchView(application: application)
        }
    }
}

private struct HistoryEntryView: View {
    let application: WhisperApplication
    let entry: HistoryEntry
    var detail = false
    @State private var expanded = false
    @State private var confirmDelete = false
    private var language: AppLanguage { application.state.settings.language }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text(entry.occurredAt.formatted(Date.FormatStyle(date: detail ? .abbreviated : .omitted, time: .shortened, locale: Locale(identifier: language.rawValue))))
                if entry.source == .upload { Label(language.text("Upload", "上传"), systemImage: "arrow.up.doc") }
                if entry.status != .completed {
                    Text(entry.status == .failed ? language.text("Failed", "失败") : language.text("Discarded", "已丢弃"))
                }
                Spacer()
            }
            .font(.caption).foregroundStyle(.secondary)
            if !entry.text.isEmpty {
                Text(entry.text).textSelection(.enabled).lineLimit(detail || expanded ? nil : 4)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            if let error = entry.errorMessage {
                Text(error).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
            }
            if !entry.rawText.isEmpty && entry.rawText != entry.text {
                DisclosureGroup(language.text("Original transcript", "原始转录")) {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(entry.rawText).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                        copyButton(.raw)
                    }.padding(.top, 8)
                }
                .font(.caption)
            }
            HStack(spacing: 12) {
                if !entry.text.isEmpty { copyButton(.processed) }
                if !detail {
                    Button(language.text("Open", "打开")) { application.send(.selectHistoryEntry(entry.id)) }
                }
                if !detail && entry.text.count > 180 {
                    Button(language.text(expanded ? "Show less" : "Show more", expanded ? "收起" : "展开")) { expanded.toggle() }
                }
                Spacer()
                Button(role: .destructive) { confirmDelete = true } label: { Image(systemName: "trash") }
                    .accessibilityLabel(language.text("Delete recording", "删除录音"))
                    .accessibilityIdentifier("history-delete-" + entry.id.uuidString)
            }
            .buttonStyle(.borderless).font(.caption)
        }
        .alert(language.text("Delete recording?", "删除录音？"), isPresented: $confirmDelete) {
            Button(language.text("Cancel", "取消"), role: .cancel) {}
            Button(language.text("Delete", "删除"), role: .destructive) { application.send(.deleteHistory(entry.id)) }
        } message: { Text(language.text("This history item will be permanently deleted.", "此历史记录将被永久删除。")) }
    }

    private func copyButton(_ version: HistoryTextVersion) -> some View {
        let copied = application.state.history.copied?.id == entry.id && application.state.history.copied?.version == version
        return Button {
            application.send(.copyHistory(entry.id, version))
        } label: {
            Label(copied ? language.text("Copied", "已复制") : version == .raw ? language.text("Copy original", "复制原文") : language.text("Copy", "复制"), systemImage: copied ? "checkmark" : "doc.on.doc")
        }
        .accessibilityIdentifier("history-copy-" + version.rawValue + "-" + entry.id.uuidString)
    }
}

private struct HistorySearchView: View {
    let application: WhisperApplication
    @Environment(\.dismiss) private var dismiss
    @FocusState private var focused: Bool
    private var language: AppLanguage { application.state.settings.language }
    private var history: HistoryState { application.state.history }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                if history.selectedEntry != nil {
                    Button { application.send(.dismissHistoryEntry) } label: { Image(systemName: "chevron.left") }
                        .accessibilityLabel(language.text("Back to search", "返回搜索"))
                }
                Text(language.text("Search history", "搜索历史记录")).font(.headline)
                Spacer()
                Button(language.text("Done", "完成")) { dismiss() }.keyboardShortcut(.escape, modifiers: [])
            }
            if let entry = history.selectedEntry {
                ScrollView { HistoryEntryView(application: application, entry: entry, detail: true) }
            } else {
                TextField(language.text("Search transcripts…", "搜索转录内容…"), text: Binding(
                    get: { history.searchQuery }, set: { application.send(.searchHistory($0)) }
                ))
                .textFieldStyle(.roundedBorder).focused($focused)
                .onSubmit(openSelection).accessibilityIdentifier("history-search-query")
                if history.isSearching { ProgressView().controlSize(.small) }
                else if history.searchResults.isEmpty {
                    Text(language.text("No matching recordings", "没有匹配的录音")).foregroundStyle(.secondary)
                }
                VStack(spacing: 4) {
                    ForEach(Array(history.searchResults.enumerated()), id: \.element.id) { index, entry in
                        Button { application.send(.selectHistoryEntry(entry.id)) } label: {
                            VStack(alignment: .leading, spacing: 5) {
                                Text(entry.text.isEmpty ? entry.rawText : entry.text).lineLimit(2)
                                Text(entry.occurredAt.formatted(Date.FormatStyle(date: .abbreviated, time: .shortened, locale: Locale(identifier: language.rawValue)))).font(.caption).foregroundStyle(.secondary)
                            }
                            .frame(maxWidth: .infinity, alignment: .leading).padding(12)
                            .background(index == history.searchSelection ? Color.accentColor.opacity(0.12) : .clear, in: RoundedRectangle(cornerRadius: 8))
                        }.buttonStyle(.plain).disabled(history.isSearching)
                    }
                }
                Spacer(minLength: 0)
                Text(language.text("↑ ↓ to navigate · Return to open", "↑ ↓ 选择 · 回车打开"))
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .padding(24).frame(width: 560, height: 440)
        .onAppear { focused = true }
        .onKeyPress(.downArrow) {
            guard history.selectedEntry == nil else { return .ignored }
            application.send(.moveHistorySearchSelection(1)); return .handled
        }
        .onKeyPress(.upArrow) {
            guard history.selectedEntry == nil else { return .ignored }
            application.send(.moveHistorySearchSelection(-1)); return .handled
        }
    }

    private func openSelection() {
        guard !history.isSearching, history.searchResults.indices.contains(history.searchSelection) else { return }
        application.send(.selectHistoryEntry(history.searchResults[history.searchSelection].id))
    }
}

struct HistoryPrivacyView: View {
    let application: WhisperApplication
    private var language: AppLanguage { application.state.settings.language }
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(language.text("Privacy & Data", "隐私与数据")).font(.headline)
            Toggle(language.text("Save history", "保存历史记录"), isOn: Binding(
                get: { application.state.settings.history.enabled },
                set: { application.send(.setHistoryEnabled($0)) }
            ))
            .accessibilityIdentifier("history-enabled")
            Text(language.text("Keep transcripts on this Mac. Turning this off leaves existing history intact and keeps new results available to copy.", "将转录保存在此 Mac 上。关闭后会保留已有历史记录，新结果仍可复制。"))
                .font(.caption).foregroundStyle(.secondary)
            if let failure = application.state.configurationError {
                Text(failure.message(in: language)).font(.caption).foregroundStyle(.red)
            }
        }
        .padding(18).background(.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(.primary.opacity(0.12)))
    }
}
