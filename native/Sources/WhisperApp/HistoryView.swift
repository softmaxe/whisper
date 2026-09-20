import SwiftUI
import WhisperCore

struct HistoryView: View {
    let application: WhisperApplication
    @State private var confirmClear = false
    private var language: AppLanguage { application.state.settings.language }
    private var history: HistoryState { application.state.history }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
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
            if let failure = history.audioFailure ?? history.retry.failure {
                Text(failure.message(in: language)).font(.caption).foregroundStyle(.red)
                    .accessibilityIdentifier("history-audio-error")
            }
            if let failure = history.retry.cleanupFailure {
                Text(failure.message(in: language) + " " + language.text("The retry used the original transcript.", "重试已使用原始转录。"))
                    .font(.caption).foregroundStyle(.orange)
            }
            if history.retry.chineseConversionFailed {
                Text(language.text("Chinese conversion was unavailable. The retry text has been preserved.", "中文转换暂不可用。已保留重试文本。"))
                    .font(.caption).foregroundStyle(.orange)
            }
            if history.isLoading && history.entries.isEmpty {
                HStack { ProgressView().controlSize(.small); Text(language.text("Loading history…", "正在加载历史记录…")) }
                    .frame(maxWidth: .infinity).padding(32)
            } else if history.entries.isEmpty {
                HStack {
                    Text(language.text("History", "历史记录")).font(.custom("JetBrainsMono-Regular", size: 14)).foregroundStyle(.secondary)
                    Spacer()
                    historyActions
                }
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
                    ForEach(Array(history.groups().enumerated()), id: \.element.id) { index, group in
                        HStack {
                            Text(group.title(in: language)).font(.custom("JetBrainsMono-Regular", size: 14)).foregroundStyle(.secondary)
                            Spacer()
                            if index == 0 { historyActions }
                        }.padding(.top, index == 0 ? 8 : 24).padding(.bottom, 10)
                        VStack(spacing: 0) {
                            ForEach(group.entries) { entry in
                                HistoryEntryView(application: application, entry: entry).padding(.horizontal, 16).padding(.vertical, 12)
                                if entry.id != group.entries.last?.id { Divider() }
                            }
                        }
                        .background(.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 16))
                        .overlay(RoundedRectangle(cornerRadius: 16).strokeBorder(.primary.opacity(0.12)))
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
        .alert(language.text("Clear all history?", "清空全部历史记录？"), isPresented: $confirmClear) {
            Button(language.text("Cancel", "取消"), role: .cancel) {}
            Button(language.text("Clear all", "清空全部"), role: .destructive) { application.send(.clearHistory) }
        } message: {
            Text(language.text("All history on this device will be permanently deleted. This cannot be undone.", "将永久删除此设备上的全部历史记录。此操作无法撤销。"))
        }
    }

    private var historyActions: some View {
        HStack(spacing: 8) {
            Button { application.send(.showDiscardedHistory(!history.includeDiscarded)) } label: {
                Label(history.includeDiscarded ? language.text("Hide discarded", "隐藏已丢弃") : language.text("Show discarded", "显示已丢弃"), systemImage: "archivebox")
            }
            .accessibilityValue(history.includeDiscarded ? language.text("Shown", "已显示") : language.text("Hidden", "已隐藏"))
            .accessibilityIdentifier("history-show-discarded")
            if !history.entries.isEmpty {
                Button(role: .destructive) { confirmClear = true } label: {
                    Label(language.text("Clear all", "清空全部"), systemImage: "trash")
                }.disabled(history.totalCount == 0 || history.pendingChanges > 0)
                    .accessibilityIdentifier("history-clear-all")
            }
        }.buttonStyle(.borderless).font(.custom("JetBrainsMono-Regular", size: 11)).foregroundStyle(.secondary)
    }
}

private struct HistoryEntryView: View {
    let application: WhisperApplication
    let entry: HistoryEntry
    var detail = false
    @State private var rawExpanded = false
    @State private var confirmDelete = false
    private var language: AppLanguage { application.state.settings.language }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(entry.occurredAt.formatted(Date.FormatStyle(date: detail ? .abbreviated : .omitted, time: .shortened, locale: Locale(identifier: language.rawValue))))
                if entry.source == .upload { Label(language.text("Upload", "上传"), systemImage: "arrow.up.doc") }
                if entry.status != .completed {
                    Text(entry.status == .failed ? language.text("Failed", "失败") : language.text("Discarded", "已丢弃"))
                }
                Spacer()
                if !entry.text.isEmpty {
                    copyButton(.processed).labelStyle(.iconOnly).frame(width: 28, height: 28)
                        .help(language.text("Copy text", "复制文字"))
                }
                actions
            }
            .buttonStyle(.borderless).font(.custom("JetBrainsMono-Regular", size: 12)).foregroundStyle(.secondary)
            if !entry.text.isEmpty {
                Text(entry.text).textSelection(.enabled).font(.custom("JetBrainsMono-Regular", size: 16)).lineSpacing(4)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            if let error = entry.errorMessage {
                Text(error).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
            }
            if rawExpanded && !entry.rawText.isEmpty {
                Divider().padding(.top, 2)
                HStack {
                    Text(language.text("ORIGINAL TRANSCRIPT", "原始转录")).font(.custom("JetBrainsMono-SemiBold", size: 10))
                    Spacer()
                    copyButton(.raw).labelStyle(.iconOnly).buttonStyle(.borderless).frame(width: 20, height: 20)
                        .help(language.text("Copy original", "复制原文"))
                }
                .foregroundStyle(.secondary)
                Text(entry.rawText).textSelection(.enabled).font(.custom("JetBrainsMono-Regular", size: 12)).lineSpacing(3)
                    .foregroundStyle(.secondary).frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .alert(language.text("Delete recording?", "删除录音？"), isPresented: $confirmDelete) {
            Button(language.text("Cancel", "取消"), role: .cancel) {}
            Button(language.text("Delete", "删除"), role: .destructive) { application.send(.deleteHistory(entry.id)) }
        } message: { Text(language.text("This history item will be permanently deleted.", "此历史记录将被永久删除。")) }
    }

    private var actions: some View {
        Menu {
            if entry.hasAudio {
                Button(application.state.history.playingID == entry.id ? language.text("Stop playback", "停止播放") : language.text("Play audio", "播放音频")) {
                    application.send(application.state.history.playingID == entry.id ? .stopHistoryPlayback : .playHistory(entry.id))
                }.accessibilityIdentifier("history-play-" + entry.id.uuidString)
                if application.state.history.retry.isRunning && application.state.history.retry.entryID == entry.id {
                    Button(language.text("Cancel retry", "取消重试")) { application.send(.cancelHistoryRetry) }
                } else {
                    Button(entry.status == .completed ? language.text("Retry", "重试") : language.text("Recover", "恢复")) { application.send(.retryHistory(entry.id)) }
                        .accessibilityIdentifier("history-retry-" + entry.id.uuidString)
                }
            }
            if entry.status == .completed && !entry.rawText.isEmpty {
                Button(rawExpanded ? language.text("Hide original transcript", "隐藏原始转录") : language.text("View original transcript", "查看原始转录")) { rawExpanded.toggle() }
                    .accessibilityIdentifier("history-original-" + entry.id.uuidString)
            }
            if entry.hasAudio {
                Button(language.text("Show audio in Finder", "在 Finder 中显示音频")) { application.send(.revealHistoryAudio(entry.id)) }
            }
            if !detail {
                Button(language.text("Open", "打开")) { application.send(.openHistorySearchResult(entry.id)) }
            }
            Divider()
            Button(language.text("Delete recording", "删除录音"), role: .destructive) { confirmDelete = true }
                .accessibilityIdentifier("history-delete-" + entry.id.uuidString)
        } label: {
            Image(systemName: "ellipsis").rotationEffect(.degrees(90)).frame(width: 28, height: 28)
        }.menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
            .accessibilityLabel(language.text("More actions", "更多操作"))
            .accessibilityIdentifier("history-actions-" + entry.id.uuidString)
            .help(language.text("More actions", "更多操作"))
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

struct HistorySearchView: View {
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
                Button(language.text("Done", "完成")) { application.send(.closeHistorySearch); dismiss() }
            }
            if let entry = history.selectedEntry {
                ScrollView { HistoryEntryView(application: application, entry: entry, detail: true) }
            } else {
                TextField(language.text("Search transcripts…", "搜索转录内容…"), text: Binding(
                    get: { history.searchQuery }, set: { application.send(.searchHistory($0)) }
                ))
                .textFieldStyle(.roundedBorder).focused($focused)
                .onAppear { focused = true }
                .onSubmit(openSelection).accessibilityIdentifier("history-search-query")
                if history.isSearching { ProgressView().controlSize(.small) }
                else if history.searchResults.isEmpty {
                    Text(language.text("No matching recordings", "没有匹配的录音")).foregroundStyle(.secondary)
                }
                VStack(spacing: 4) {
                    ForEach(Array(history.searchResults.enumerated()), id: \.element.id) { index, entry in
                        if index == history.searchSelection {
                            resultButton(entry, selected: true).keyboardShortcut(.defaultAction)
                        } else { resultButton(entry, selected: false) }
                    }
                }
                Spacer(minLength: 0)
                Text(language.text("↑ ↓ to navigate · Return to open", "↑ ↓ 选择 · 回车打开"))
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .padding(24).frame(width: 560, height: 440)
        .onExitCommand { application.send(.foregroundEscape) }
        .onKeyPress(.downArrow) {
            guard history.selectedEntry == nil else { return .ignored }
            application.send(.moveHistorySearchSelection(1)); return .handled
        }
        .onKeyPress(.upArrow) {
            guard history.selectedEntry == nil else { return .ignored }
            application.send(.moveHistorySearchSelection(-1)); return .handled
        }
    }

    private func resultButton(_ entry: HistoryEntry, selected: Bool) -> some View {
        Button { application.send(.openHistorySearchResult(entry.id)) } label: {
            VStack(alignment: .leading, spacing: 5) {
                Text(entry.text.isEmpty ? entry.rawText : entry.text).lineLimit(2)
                Text(entry.occurredAt.formatted(Date.FormatStyle(date: .abbreviated, time: .shortened, locale: Locale(identifier: language.rawValue)))).font(.caption).foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading).padding(12)
            .background(selected ? Color.accentColor.opacity(0.12) : .clear, in: RoundedRectangle(cornerRadius: 8))
        }.buttonStyle(.plain).disabled(history.isSearching)
            .accessibilityAddTraits(selected ? .isSelected : [])
            .accessibilityIdentifier("history-search-result-" + entry.id.uuidString)
    }

    private func openSelection() {
        guard !history.isSearching, history.searchResults.indices.contains(history.searchSelection) else { return }
        application.send(.openHistorySearchResult(history.searchResults[history.searchSelection].id))
    }
}

struct HistoryPrivacyView: View {
    let application: WhisperApplication
    @State private var confirmClearAudio = false
    private var language: AppLanguage { application.state.settings.language }
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(language.text("Local History and audio", "本地历史记录与音频")).font(.headline)
            Toggle(language.text("Save history", "保存历史记录"), isOn: Binding(
                get: { application.state.settings.history.enabled },
                set: { application.send(.setHistoryEnabled($0)) }
            ))
            .accessibilityIdentifier("history-enabled")
            Text(language.text("Keep transcripts on this Mac. Turning this off leaves existing history intact and keeps new results available to copy.", "将转录保存在此 Mac 上。关闭后会保留已有历史记录，新结果仍可复制。"))
                .font(.caption).foregroundStyle(.secondary)
            retentionPicker(audio: false)
            retentionPicker(audio: true)
            Toggle(language.text("Save cancelled recordings", "保存已取消的录音"), isOn: Binding(
                get: { application.state.settings.history.saveDiscarded },
                set: { value in
                    var preferences = application.state.settings.history
                    preferences.saveDiscarded = value
                    application.send(.setHistoryRetention(preferences))
                }
            ))
            .disabled(!application.state.settings.history.enabled || application.state.settings.history.audioRetentionDays == 0)
            Text(language.text("Cancelled recordings need at least one second of audio and audio retention enabled. Accidental shortcut taps are never saved.", "已取消的录音至少需要一秒音频，并启用音频保留。误触快捷键产生的录音不会保存。"))
                .font(.caption).foregroundStyle(.secondary)
            Divider()
            HStack {
                VStack(alignment: .leading, spacing: 5) {
                    Text(language.text("Storage Usage", "存储用量")).fontWeight(.medium)
                    if application.state.privacy.isLoadingStorage {
                        Text(language.text("Calculating…", "正在计算…")).font(.caption).foregroundStyle(.secondary)
                    } else if let usage = application.state.privacy.audioUsage {
                        Text(language.text("\(usage.files) files, \(storageSize(usage.bytes))", "\(usage.files) 个文件，\(storageSize(usage.bytes))"))
                            .font(.caption).foregroundStyle(.secondary).accessibilityIdentifier("audio-storage-usage")
                    }
                }
                Spacer()
                Button(language.text("Delete saved audio", "删除保存的音频"), role: .destructive) { confirmClearAudio = true }
                    .disabled(application.state.privacy.audioUsage?.files == nil || application.state.privacy.audioUsage?.files == 0 || application.state.history.pendingChanges > 0)
            }
            if application.state.privacy.storageFailed {
                Text(language.text("Storage usage could not be read. Saved files have been preserved.", "无法读取存储用量。已保留保存的文件。"))
                    .font(.caption).foregroundStyle(.red)
                Button(language.text("Retry", "重试")) { application.send(.refreshPrivacy) }
            }
            if let failure = application.state.configurationError {
                Text(failure.message(in: language)).font(.caption).foregroundStyle(.red)
            }
        }
        .padding(18).background(.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(.primary.opacity(0.12)))
        .alert(language.text("Delete all saved audio?", "删除全部保存的音频？"), isPresented: $confirmClearAudio) {
            Button(language.text("Cancel", "取消"), role: .cancel) {}
            Button(language.text("Delete audio", "删除音频"), role: .destructive) { application.send(.clearHistoryAudio) }
        } message: { Text(language.text("Transcripts stay in History. Deleted audio cannot be played or retried.", "转录文本会保留在历史记录中。删除音频后将无法播放或重试。")) }
    }

    private func storageSize(_ bytes: Int64) -> String {
        let formatter = ByteCountFormatter()
        formatter.countStyle = .file
        formatter.allowsNonnumericFormatting = false
        return formatter.string(fromByteCount: bytes)
    }

    private func retentionPicker(audio: Bool) -> some View {
        let preferences = application.state.settings.history
        let days = audio ? preferences.audioRetentionDays : preferences.transcriptRetentionDays
        let choices = [0, 1, 7, 14, 30, 60, 90]
        return Picker(audio ? language.text("Keep audio", "保留音频") : language.text("Keep transcripts", "保留转录"), selection: Binding(
            get: { days },
            set: { value in
                var changed = application.state.settings.history
                if audio { changed.audioRetentionDays = value } else { changed.transcriptRetentionDays = value }
                application.send(.setHistoryRetention(changed))
            }
        )) {
            ForEach(choices.contains(days) ? choices : choices + [days], id: \.self) { value in
                Text(value == 0 ? (audio ? language.text("Do not retain new audio", "不保留新音频") : language.text("Forever", "永久"))
                    : language.text("\(value) days", "\(value) 天")).tag(value)
            }
        }.disabled(!audio && !application.state.settings.history.enabled)
        .accessibilityIdentifier(audio ? "audio-retention" : "transcript-retention")
    }
}
