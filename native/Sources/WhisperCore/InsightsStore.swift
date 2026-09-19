import Foundation

extension HistoryStore {
    func ensureInsightsSchema(_ database: HistoryDatabase) throws {
        try database.execute("""
            CREATE TABLE IF NOT EXISTS insights_events (
                id TEXT PRIMARY KEY, occurred_at REAL NOT NULL, created_at REAL NOT NULL,
                local_date TEXT NOT NULL, word_count INTEGER NOT NULL, spoken_duration_ms REAL
            );
            CREATE INDEX IF NOT EXISTS insights_day ON insights_events(local_date);
            CREATE TABLE IF NOT EXISTS insights_retention_state (
                id INTEGER PRIMARY KEY CHECK(id = 1), expired_before REAL NOT NULL
            );
            """)
    }

    func recordInsights(entry: HistoryEntry, replace: Bool = false) throws {
        guard entry.source == .dictation, entry.status == .completed else { return }
        let database = try open()
        try ensureInsightsSchema(database)
        try insertInsights(InsightsEvent(entry: entry), replace: replace, database: database)
    }

    private func insertInsights(_ event: InsightsEvent, replace: Bool, database: HistoryDatabase) throws {
        guard event.wordCount > 0 else { return }
        guard event.occurredAt.timeIntervalSinceReferenceDate.isFinite,
              event.createdAt.timeIntervalSinceReferenceDate.isFinite,
              InsightsCalendar.date(event.localDate) != nil else { throw HistoryDatabaseError.invalidEntry }
        if let cutoff = try clearedThrough(), event.occurredAt <= cutoff { return }
        let retention = try database.statement("SELECT expired_before FROM insights_retention_state WHERE id = 1")
        if try retention.next(), event.createdAt.timeIntervalSinceReferenceDate < retention.number(0) { return }
        let conflict = replace ? """
            DO UPDATE SET occurred_at = excluded.occurred_at, local_date = excluded.local_date,
                word_count = excluded.word_count,
                spoken_duration_ms = COALESCE(excluded.spoken_duration_ms, insights_events.spoken_duration_ms)
            """ : "DO NOTHING"
        let statement = try database.statement("""
            INSERT INTO insights_events (id, occurred_at, created_at, local_date, word_count, spoken_duration_ms)
            VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(id) \(conflict)
            """, [.text(event.id.uuidString), .number(event.occurredAt.timeIntervalSinceReferenceDate),
                  .number(event.createdAt.timeIntervalSinceReferenceDate), .text(event.localDate),
                  .number(Double(event.wordCount)), event.durationMs.map(HistorySQLValue.number) ?? .null])
        try statement.finish()
    }

    func insightsSummary(now: Date) throws -> InsightsSummary {
        let database = try open()
        try ensureInsightsSchema(database)
        try reconcileInsights(database)
        try Task.checkCancellation()
        let query = try database.statement("""
            SELECT local_date, SUM(word_count), COUNT(*),
                SUM(CASE WHEN spoken_duration_ms > 0 THEN spoken_duration_ms ELSE 0 END),
                SUM(CASE WHEN spoken_duration_ms > 0 THEN word_count ELSE 0 END)
            FROM insights_events GROUP BY local_date ORDER BY local_date
            """)
        var days: [InsightsDay] = []
        while try query.next() {
            try Task.checkCancellation()
            days.append(InsightsDay(date: query.text(0) ?? "", words: Int(query.number(1)), dictations: Int(query.number(2)),
                                    spokenDurationMs: query.number(3), coveredWords: Int(query.number(4))))
        }
        return InsightsSummary(days: days, now: now)
    }

    /// Earlier native History and newly recovered rows can fill missing events without replacing live counts.
    private func reconcileInsights(_ database: HistoryDatabase) throws {
        var cursor: Double = 0
        while true {
            try Task.checkCancellation()
            let rows = try database.statement("""
                SELECT rowid, id, text, raw_text, occurred_at, created_at, local_date, audio_duration
                FROM history WHERE rowid > ? AND status = 'completed' AND source = 'dictation'
                    AND NOT EXISTS (SELECT 1 FROM insights_events WHERE insights_events.id = history.id)
                ORDER BY rowid LIMIT 250
                """, [.number(cursor)])
            var entries: [HistoryEntry] = []
            var scanned = 0
            while try rows.next() {
                scanned += 1
                cursor = rows.number(0)
                guard let id = UUID(uuidString: rows.text(1) ?? ""), let localDate = rows.text(6),
                      InsightsCalendar.date(localDate) != nil else { continue }
                entries.append(HistoryEntry(id: id, text: rows.text(2) ?? "", rawText: rows.text(3) ?? "",
                    occurredAt: Date(timeIntervalSinceReferenceDate: rows.number(4)),
                    createdAt: Date(timeIntervalSinceReferenceDate: rows.number(5)), localDate: localDate,
                    audioDuration: rows.isNull(7) ? nil : rows.number(7)))
            }
            if scanned == 0 { return }
            try database.execute("BEGIN IMMEDIATE")
            do {
                for entry in entries {
                    try Task.checkCancellation()
                    try insertInsights(InsightsEvent(entry: entry), replace: false, database: database)
                }
                try database.execute("COMMIT")
            } catch {
                try? database.execute("ROLLBACK")
                throw error
            }
        }
    }

    /// Called inside transcript retention's transaction, including when its History ID list is empty.
    func deleteExpiredInsights(before cutoff: Date, database: HistoryDatabase) throws {
        try ensureInsightsSchema(database)
        let delete = try database.statement("DELETE FROM insights_events WHERE created_at < ?", [.number(cutoff.timeIntervalSinceReferenceDate)])
        try delete.finish()
        let state = try database.statement("""
            INSERT INTO insights_retention_state (id, expired_before) VALUES (1, ?)
            ON CONFLICT(id) DO UPDATE SET expired_before = MAX(expired_before, excluded.expired_before)
            """, [.number(cutoff.timeIntervalSinceReferenceDate)])
        try state.finish()
    }
}
