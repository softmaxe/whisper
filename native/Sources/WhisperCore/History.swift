import Foundation

public struct HistoryPreferences: Codable, Equatable, Sendable {
    public var enabled: Bool
    public var audioRetentionDays: Int
    public var transcriptRetentionDays: Int
    public var saveDiscarded: Bool
    public init(enabled: Bool = true, audioRetentionDays: Int = 30, transcriptRetentionDays: Int = 0, saveDiscarded: Bool = false) {
        self.enabled = enabled
        self.audioRetentionDays = audioRetentionDays >= 0 ? audioRetentionDays : 30
        self.transcriptRetentionDays = transcriptRetentionDays >= 0 ? transcriptRetentionDays : 0
        self.saveDiscarded = saveDiscarded
    }
    public init(from decoder: any Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        self.init(enabled: try values.decodeIfPresent(Bool.self, forKey: .enabled) ?? true,
                  audioRetentionDays: try values.decodeIfPresent(Int.self, forKey: .audioRetentionDays) ?? 30,
                  transcriptRetentionDays: try values.decodeIfPresent(Int.self, forKey: .transcriptRetentionDays) ?? 0,
                  saveDiscarded: try values.decodeIfPresent(Bool.self, forKey: .saveDiscarded) ?? false)
    }
}

public enum HistorySource: String, Codable, Sendable { case dictation, upload }
public enum HistoryStatus: String, Codable, Sendable { case completed, failed, discarded }
public enum HistoryTextVersion: String, Equatable, Sendable { case processed, raw }

public struct HistoryEntry: Codable, Equatable, Identifiable, Sendable {
    public let id: UUID
    public var text: String
    public var rawText: String
    public let occurredAt: Date
    public let createdAt: Date
    public let localDate: String
    public let source: HistorySource
    public var status: HistoryStatus
    public var provider: String
    public var model: String
    public var audioDuration: TimeInterval?
    public var audioFileName: String?
    public var errorCode: String?
    public var errorMessage: String?
    public var hasAudio: Bool { audioFileName != nil }

    public init(
        id: UUID = UUID(), text: String, rawText: String? = nil,
        occurredAt: Date = Date(), createdAt: Date = Date(), localDate: String? = nil,
        source: HistorySource = .dictation, status: HistoryStatus = .completed,
        provider: String = "self-hosted", model: String = "", audioDuration: TimeInterval? = nil,
        audioFileName: String? = nil, errorCode: String? = nil, errorMessage: String? = nil
    ) {
        self.id = id; self.text = text; self.rawText = rawText ?? text
        self.occurredAt = occurredAt; self.createdAt = createdAt
        self.localDate = localDate ?? HistoryDateGroup.localDate(for: occurredAt)
        self.source = source; self.status = status; self.provider = provider; self.model = model
        self.audioDuration = audioDuration; self.audioFileName = audioFileName
        self.errorCode = errorCode; self.errorMessage = errorMessage
    }

    public func text(_ version: HistoryTextVersion) -> String { version == .raw ? rawText : text }
}

public struct HistoryDateGroup: Equatable, Identifiable, Sendable {
    public var id: Date { day }
    public let day: Date
    public var entries: [HistoryEntry]

    public func title(in language: AppLanguage, now: Date = Date(), calendar: Calendar = .autoupdatingCurrent) -> String {
        if calendar.isDate(day, inSameDayAs: now) { return language.text("Today", "今天") }
        if let yesterday = calendar.date(byAdding: .day, value: -1, to: now), calendar.isDate(day, inSameDayAs: yesterday) {
            return language.text("Yesterday", "昨天")
        }
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: language.rawValue)
        formatter.calendar = calendar; formatter.timeZone = calendar.timeZone
        formatter.dateStyle = .medium
        return formatter.string(from: day)
    }

    public static func localDate(for date: Date, calendar: Calendar = .autoupdatingCurrent) -> String {
        var gregorian = Calendar(identifier: .gregorian)
        gregorian.timeZone = calendar.timeZone
        let components = gregorian.dateComponents([.year, .month, .day], from: date)
        return String(format: "%04d-%02d-%02d", components.year ?? 0, components.month ?? 0, components.day ?? 0)
    }
}

public enum HistoryFailure: Error, Equatable, Sendable {
    case load, save, delete
    public func message(in language: AppLanguage) -> String {
        switch self {
        case .load: language.text("History could not be loaded. Your saved files have been preserved.", "无法加载历史记录。已保留您的所有文件。")
        case .save: language.text("History could not be saved. Your current result is still available to copy.", "无法保存历史记录。仍可复制当前结果。")
        case .delete: language.text("History could not be deleted. Check that the native profile is writable and try again.", "无法删除历史记录。请检查原生配置目录是否可写，然后重试。")
        }
    }
}

public struct HistoryCopyFeedback: Equatable, Sendable {
    public let id: UUID
    public let version: HistoryTextVersion
}

public struct HistoryState: Equatable, Sendable {
    public var retry = HistoryRetryState()
    public var playingID: UUID?
    public var audioFailure: HistoryAudioFailure?
    public var retentionCutoff: Date?
    public var entries: [HistoryEntry] = []
    public var isLoading = false
    public var isLoaded = false
    public var pendingChanges = 0
    public var totalCount = 0
    public var hasMore = false
    public var includeDiscarded = false
    public var searchQuery = ""
    public var searchResults: [HistoryEntry] = []
    public var isSearching = false
    public var searchSelection = 0
    public var selectedEntry: HistoryEntry?
    public var copied: HistoryCopyFeedback?
    public var lastSavedID: UUID?
    public var clearedThrough: Date?
    public var failure: HistoryFailure?
    var cursor: HistoryCursor?
    public init() {}

    public func groups(calendar: Calendar = .autoupdatingCurrent) -> [HistoryDateGroup] {
        var result: [HistoryDateGroup] = []
        for entry in entries {
            let day = calendar.startOfDay(for: entry.occurredAt)
            if result.last?.day == day { result[result.count - 1].entries.append(entry) }
            else { result.append(HistoryDateGroup(day: day, entries: [entry])) }
        }
        return result
    }
}

enum HistoryChange: Sendable {
    case save(HistoryEntry), recording(HistoryEntry, Task<CapturedAudio, any Error>, Bool)
    case retry(HistoryEntry, String, HistoryRetryOwnership)
    case delete(UUID), clear(Date), clearAudio, expire(HistoryPreferences, Date)
    var failure: HistoryFailure {
        switch self { case .save, .recording, .retry: .save; default: .delete }
    }
}

extension WhisperApplication {
    func setHistoryEnabled(_ enabled: Bool) {
        var settings = state.settings
        settings.history.enabled = enabled
        persist(settings)
    }

    func refreshHistory(more: Bool = false) {
        guard profileReadable else { state.history.failure = .load; return }
        if more && (state.history.isLoading || !state.history.hasMore) { return }
        historyReadGeneration += 1
        let generation = historyReadGeneration
        let cursor = more ? state.history.cursor : nil
        let includeDiscarded = state.history.includeDiscarded
        historyReadTask?.cancel()
        state.history.isLoading = true
        historyReadTask = Task { [weak self, historyStore] in
            do {
                let page = try await historyStore.page(includeDiscarded: includeDiscarded, after: cursor)
                guard !Task.isCancelled, let self, self.historyReadGeneration == generation else { return }
                self.state.history.entries = more ? self.state.history.entries + page.entries : page.entries
                self.state.history.cursor = page.cursor
                self.state.history.hasMore = page.cursor != nil
                self.state.history.totalCount = page.totalCount
                self.state.history.clearedThrough = page.clearedThrough
                self.state.history.isLoaded = true
                self.state.history.isLoading = false
                if self.state.history.failure == .load { self.state.history.failure = nil }
            } catch {
                guard !Task.isCancelled, let self, self.historyReadGeneration == generation else { return }
                self.state.history.isLoading = false
                if self.state.history.failure == nil { self.state.history.failure = .load }
            }
        }
    }

    func searchHistory(_ query: String) {
        guard profileReadable else { state.history.failure = .load; return }
        state.history.searchQuery = query
        state.history.searchSelection = 0
        historySearchGeneration += 1
        let generation = historySearchGeneration
        let includeDiscarded = state.history.includeDiscarded
        historySearchTask?.cancel()
        state.history.isSearching = true
        historySearchTask = Task { [weak self, historyStore] in
            do {
                let page = try await historyStore.page(query: query, includeDiscarded: includeDiscarded, limit: 5)
                guard !Task.isCancelled, let self, self.historySearchGeneration == generation else { return }
                self.state.history.searchResults = page.entries
                self.state.history.isSearching = false
            } catch {
                guard !Task.isCancelled, let self, self.historySearchGeneration == generation else { return }
                self.state.history.isSearching = false
                if self.state.history.failure == nil { self.state.history.failure = .load }
            }
        }
    }

    func copyHistory(_ id: UUID, version: HistoryTextVersion) {
        guard let entry = historyEntry(id), !entry.text(version).isEmpty else { return }
        clipboard.write(entry.text(version))
        state.history.copied = HistoryCopyFeedback(id: id, version: version)
    }

    func historyEntry(_ id: UUID) -> HistoryEntry? {
        if let selected = state.history.selectedEntry, selected.id == id { return selected }
        return state.history.entries.first { $0.id == id } ?? state.history.searchResults.first { $0.id == id }
    }

    /// Upload and retry can await this same application operation without hiding persistence failure.
    @discardableResult public func recordHistory(_ entry: HistoryEntry) async throws -> Bool {
        try Task.checkCancellation()
        return try await enqueueHistory(.save(entry)).value.get()
    }

    /// The application lifecycle can await accepted writes before terminating the process.
    public func flushHistoryWrites() async {
        _ = await historyWriteTask?.value
    }

    @discardableResult func enqueueHistory(_ change: HistoryChange) -> Task<Result<Bool, HistoryFailure>, Never> {
        guard profileReadable else {
            state.history.failure = change.failure
            return Task { .failure(change.failure) }
        }
        switch change {
        case .save, .recording:
            if !state.settings.history.enabled { return Task { .success(false) } }
        default: break
        }
        let previous = historyWriteTask
        state.history.pendingChanges += 1
        state.history.failure = nil
        let task = Task { [weak self, historyStore] in
            _ = await previous?.value
            let result: Result<Bool, HistoryFailure>
            var savedEntry: HistoryEntry?
            var audioFailed = false
            var retention: HistoryRetentionResult?
            do {
                switch change {
                case let .save(entry): savedEntry = try await historyStore.save(entry)
                case let .recording(entry, operation, retain):
                    let audio = try await operation.value
                    let saved = try await historyStore.saveRecording(entry, audio: audio, retain: retain)
                    savedEntry = saved.entry
                    audioFailed = saved.audioFailed
                case let .retry(entry, expectedAudio, ownership):
                    savedEntry = try await historyStore.updateRetry(entry, expectedAudio: expectedAudio, ownership: ownership)
                case let .delete(id): try await historyStore.delete(id)
                case let .clear(date): try await historyStore.clear(through: date)
                case .clearAudio: try await historyStore.clearAudio()
                case let .expire(preferences, date): retention = try await historyStore.expire(preferences: preferences, now: date)
                }
                if case .retry = change { result = .success(savedEntry != nil) }
                else { result = .success(true) }
            } catch { result = .failure(change.failure) }
            guard let self else { return result }
            self.state.history.pendingChanges -= 1
            if case .recording = change { self.state.history.audioFailure = audioFailed ? .save : nil }
            if case .clearAudio = change, case .success = result { self.state.history.audioFailure = nil }
            if let retention {
                self.state.history.retentionCutoff = retention.transcriptCutoff
                if let playing = self.historyAudioEntryID,
                   retention.audioIDs.contains(playing) || retention.transcriptIDs.contains(playing) { self.stopHistoryPlayback() }
            }
            switch result {
            case .success:
                switch change {
                case .save, .recording, .retry:
                    if let savedEntry {
                        self.state.history.lastSavedID = savedEntry.id
                        if self.state.history.selectedEntry?.id == savedEntry.id { self.state.history.selectedEntry = savedEntry }
                    }
                case let .delete(id):
                    if self.state.history.selectedEntry?.id == id { self.state.history.selectedEntry = nil }
                case .clear:
                    self.state.history.selectedEntry = nil
                    self.state.history.lastSavedID = nil
                case .clearAudio, .expire: self.state.history.selectedEntry = nil
                }
            case let .failure(failure): self.state.history.failure = failure
            }
            if self.state.history.pendingChanges == 0 {
                self.refreshHistory()
                self.searchHistory(self.state.history.searchQuery)
            }
            return result
        }
        historyWriteTask = task
        return task
    }

    func saveCompletedDictationToHistory(rawText: String, text: String, requestID: UUID) {
        guard let occurredAt = state.dictation.occurredAt else { return }
        let entry = HistoryEntry(
            id: requestID, text: text, rawText: rawText, occurredAt: occurredAt,
            createdAt: clock.wallDate, localDate: state.dictation.localDate,
            model: dictationConfiguration?.model ?? "", audioDuration: state.dictation.duration
        )
        if let capture = dictationCapture {
            enqueueHistory(.recording(entry, capture.finishOperation(), state.settings.history.audioRetentionDays > 0))
        } else { enqueueHistory(.save(entry)) }
    }
}
