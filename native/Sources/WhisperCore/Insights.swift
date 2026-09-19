import Foundation
import WhisperICU

public struct InsightsDay: Equatable, Sendable, Identifiable {
    public var id: String { date }
    public let date: String
    public let words: Int
    public let dictations: Int
    public let spokenDurationMs: Double
    public let coveredWords: Int
}

public struct InsightsActivityDay: Equatable, Sendable, Identifiable {
    public var id: String { date }
    public let date: String
    public let words: Int
}

public struct InsightsSummary: Equatable, Sendable {
    public let totalWords: Int
    public let totalDictations: Int
    public let totalSpokenDurationMs: Double
    public let averageWpm: Int?
    public let wpmCoveragePercent: Int
    public let currentStreakDays: Int
    public let longestStreakDays: Int
    public let daily: [InsightsDay]
    public let activity: [InsightsActivityDay]

    init(days: [InsightsDay], now: Date) {
        let days = days.sorted { $0.date < $1.date }
        totalWords = days.reduce(0) { $0 + $1.words }
        totalDictations = days.reduce(0) { $0 + $1.dictations }
        totalSpokenDurationMs = days.reduce(0) { $0 + $1.spokenDurationMs }
        let covered = days.reduce(0) { $0 + $1.coveredWords }
        averageWpm = totalSpokenDurationMs > 0 ? Int((Double(covered) * 60_000 / totalSpokenDurationMs).rounded()) : nil
        wpmCoveragePercent = totalWords > 0 ? Int((Double(covered) * 100 / Double(totalWords)).rounded()) : 0
        var run = 0, longest = 0, previous: Date?
        for day in days {
            guard let date = InsightsCalendar.date(day.date) else { continue }
            run = previous.map { Int(date.timeIntervalSince($0).rounded()) == 86400 ? run + 1 : 1 } ?? 1
            longest = max(longest, run)
            previous = date
        }
        let todayKey = HistoryDateGroup.localDate(for: now)
        let today = InsightsCalendar.date(todayKey)!
        currentStreakDays = previous.map { today.timeIntervalSince($0) > 86400 ? 0 : run } ?? 0
        longestStreakDays = longest
        daily = Array(days.suffix(366))
        var calendar = InsightsCalendar.calendar
        calendar.timeZone = .autoupdatingCurrent
        var start = calendar.dateComponents([.year, .month], from: now)
        start.day = 1; start.hour = 12
        let first = calendar.date(byAdding: .month, value: -5, to: calendar.date(from: start)!)!
        let end = calendar.startOfDay(for: now)
        let counts = Dictionary(uniqueKeysWithValues: days.map { ($0.date, $0.words) })
        var cells: [InsightsActivityDay] = []
        var date = calendar.startOfDay(for: first)
        while date <= end {
            let key = HistoryDateGroup.localDate(for: date, calendar: calendar)
            cells.append(InsightsActivityDay(date: key, words: counts[key] ?? 0))
            guard let next = calendar.date(byAdding: .day, value: 1, to: date) else { break }
            date = next
        }
        activity = cells
    }
}

public enum InsightsFailure: Error, Equatable, Sendable {
    case load, save
    public func message(in language: AppLanguage) -> String {
        switch self {
        case .load: language.text("Insights couldn't be read from this device.", "无法从本设备读取 Insights。")
        case .save: language.text("Usage could not be saved. Your transcription is still available.", "无法保存使用统计。转录结果仍然可用。")
        }
    }
}

public struct InsightsState: Equatable, Sendable {
    public var summary: InsightsSummary?
    public var isLoading = false
    public var pendingChanges = 0
    public var failure: InsightsFailure?
    public init() {}
}

struct InsightsEvent: Sendable {
    let id: UUID
    let occurredAt: Date
    let createdAt: Date
    let localDate: String
    let wordCount: Int
    let durationMs: Double?
    init(entry: HistoryEntry) {
        id = entry.id; occurredAt = entry.occurredAt; createdAt = entry.createdAt; localDate = entry.localDate
        let text = entry.rawText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? entry.text : entry.rawText
        wordCount = SpokenWords.count(text)
        durationMs = entry.audioDuration.flatMap { let ms = $0 * 1000; return ms.isFinite && ms > 0 ? ms : nil }
    }
}

enum SpokenWords {
    static func count(_ text: String) -> Int {
        let fallback = text.split(whereSeparator: \.isWhitespace).count
        guard fallback > 0, text.range(of: #"[\p{Han}\p{Hiragana}\p{Katakana}\p{Hangul}]"#, options: .regularExpression) != nil else { return fallback }
        let units = Array(text.utf16)
        guard let length = Int32(exactly: units.count) else { return fallback }
        let count = units.withUnsafeBufferPointer { whisper_icu_word_count($0.baseAddress, length) }
        return count > 0 ? Int(count) : fallback
    }
}

enum InsightsCalendar {
    static var calendar: Calendar { var value = Calendar(identifier: .gregorian); value.timeZone = TimeZone(secondsFromGMT: 0)!; return value }
    static func date(_ key: String) -> Date? {
        let pieces = key.split(separator: "-").compactMap { Int($0) }
        guard pieces.count == 3 else { return nil }
        let value = calendar.date(from: DateComponents(year: pieces[0], month: pieces[1], day: pieces[2]))
        guard let value, HistoryDateGroup.localDate(for: value, calendar: calendar) == key else { return nil }
        return value
    }
}

extension WhisperApplication {
    func refreshInsights() {
        guard profileReadable else { state.insights.failure = .load; return }
        insightsReadTask?.cancel()
        insightsGeneration += 1
        let generation = insightsGeneration
        let now = clock.wallDate
        let writes = insightsWriteTask
        state.insights.isLoading = true
        insightsReadTask = Task { [weak self, historyStore] in
            _ = await writes?.value
            do {
                let summary = try await historyStore.insightsSummary(now: now)
                guard !Task.isCancelled, self?.insightsGeneration == generation else { return }
                self?.state.insights.summary = summary
                self?.state.insights.isLoading = false
                self?.state.insights.failure = nil
            } catch {
                guard !Task.isCancelled, self?.insightsGeneration == generation else { return }
                self?.state.insights.isLoading = false
                self?.state.insights.failure = .load
            }
        }
    }

    func recordLiveDictationInsights(rawText: String, requestID: UUID) {
        guard state.settings.history.enabled, let occurred = state.dictation.occurredAt else { return }
        enqueueInsights(HistoryEntry(id: requestID, text: rawText, rawText: rawText, occurredAt: occurred,
            createdAt: clock.wallDate, localDate: state.dictation.localDate, audioDuration: state.dictation.duration), replace: true)
    }

    /// Retry calls this only after updating an existing row; Upload never qualifies.
    public func recordRecoveredDictationInsights(_ entry: HistoryEntry) {
        guard entry.source == .dictation, entry.status == .completed else { return }
        enqueueInsights(entry, replace: false)
    }

    public func flushInsightsWrites() async { _ = await insightsWriteTask?.value }

    private func enqueueInsights(_ entry: HistoryEntry, replace: Bool) {
        guard profileReadable else { state.insights.failure = .save; return }
        let previous = insightsWriteTask
        state.insights.pendingChanges += 1
        insightsWriteTask = Task { [weak self, historyStore] in
            _ = await previous?.value
            do {
                // Counting and database access both run on the store actor.
                try await historyStore.recordInsights(entry: entry, replace: replace)
            } catch { self?.state.insights.failure = .save }
            guard let self else { return }
            self.state.insights.pendingChanges -= 1
            if self.state.insights.pendingChanges == 0, self.state.insights.summary != nil { self.refreshInsights() }
        }
    }
}
