import Foundation
import Darwin
import SQLite3

struct HistoryCursor: Equatable, Sendable {
    let occurredAt: Date
    let createdAt: Date
    let id: UUID
}

struct HistoryPage: Sendable {
    var entries: [HistoryEntry]
    let cursor: HistoryCursor?
    let totalCount: Int
    let clearedThrough: Date?
}

/// Database work runs on this actor, never on the UI actor. Queries return bounded pages.
public actor HistoryStore {
    private let profile: NativeProfile
    private var database: HistoryDatabase?
    private var unreadable = false
    public init(profile: NativeProfile) { self.profile = profile }

    func page(query: String = "", includeDiscarded: Bool = false, after cursor: HistoryCursor? = nil, limit: Int = 50) throws -> HistoryPage {
        try Task.checkCancellation()
        let db = try open()
        let limit = max(1, min(limit, 100))
        var clauses = ["(? = 1 OR status = 'completed')", "(? = '' OR instr(search_text, ?) > 0)"]
        let query = query.trimmingCharacters(in: .whitespacesAndNewlines).precomposedStringWithCanonicalMapping.lowercased()
        var values: [HistorySQLValue] = [.number(includeDiscarded ? 1 : 0), .text(query), .text(query)]
        if let cursor {
            clauses.append("(occurred_at, created_at, id) < (?, ?, ?)")
            values += [.number(cursor.occurredAt.timeIntervalSinceReferenceDate), .number(cursor.createdAt.timeIntervalSinceReferenceDate), .text(cursor.id.uuidString)]
        }
        values.append(.number(Double(limit + 1)))
        let statement = try db.statement("SELECT \(Self.columns) FROM history WHERE \(clauses.joined(separator: " AND ")) ORDER BY occurred_at DESC, created_at DESC, id DESC LIMIT ?", values)
        var entries: [HistoryEntry] = []
        while try statement.next() {
            try Task.checkCancellation()
            entries.append(try read(statement))
        }
        let hasMore = entries.count > limit
        if hasMore { entries.removeLast() }
        let last = entries.last
        let next = hasMore ? last.map { HistoryCursor(occurredAt: $0.occurredAt, createdAt: $0.createdAt, id: $0.id) } : nil
        let count = try db.statement("SELECT count(*) FROM history")
        guard try count.next() else { throw HistoryDatabaseError.unavailable }
        return HistoryPage(entries: entries, cursor: next, totalCount: Int(count.number(0)), clearedThrough: try clearedThrough())
    }

    @discardableResult public func save(_ input: HistoryEntry) throws -> HistoryEntry {
        try Task.checkCancellation()
        var entry = input
        if entry.source == .upload { entry.audioFileName = nil }
        guard entry.occurredAt.timeIntervalSinceReferenceDate.isFinite, entry.createdAt.timeIntervalSinceReferenceDate.isFinite,
              entry.status != .completed || !entry.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              entry.audioDuration.map({ $0.isFinite && $0 >= 0 }) ?? true,
              entry.audioFileName.map({ !$0.isEmpty && $0 != "." && $0 != ".." && !$0.contains("/") && !$0.contains("\\") }) ?? true
        else { throw HistoryDatabaseError.invalidEntry }
        let db = try open()
        // A retry updates content and model while retaining source identity and original occurrence.
        let statement = try db.statement("""
            INSERT INTO history (\(Self.columns), search_text) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET text = excluded.text, raw_text = excluded.raw_text,
                status = excluded.status, provider = excluded.provider, model = excluded.model,
                audio_duration = excluded.audio_duration, audio_file = excluded.audio_file,
                error_code = excluded.error_code, error_message = excluded.error_message,
                search_text = excluded.search_text
            """, [
                .text(entry.id.uuidString), .text(entry.text), .text(entry.rawText),
                .number(entry.occurredAt.timeIntervalSinceReferenceDate), .number(entry.createdAt.timeIntervalSinceReferenceDate),
                .text(entry.localDate), .text(entry.source.rawValue), .text(entry.status.rawValue),
                .text(entry.provider), .text(entry.model), entry.audioDuration.map(HistorySQLValue.number) ?? .null,
                entry.audioFileName.map(HistorySQLValue.text) ?? .null,
                entry.errorCode.map(HistorySQLValue.text) ?? .null, entry.errorMessage.map(HistorySQLValue.text) ?? .null,
                .text((entry.text + "\n" + entry.rawText).precomposedStringWithCanonicalMapping.lowercased())
            ])
        try statement.finish()
        guard let saved = try self.entry(entry.id) else { throw HistoryDatabaseError.unavailable }
        return saved
    }

    public func entry(_ id: UUID) throws -> HistoryEntry? {
        let statement = try open().statement("SELECT \(Self.columns) FROM history WHERE id = ?", [.text(id.uuidString)])
        return try statement.next() ? read(statement) : nil
    }

    public func delete(_ id: UUID) throws {
        if let entry = try entry(id) { try removeAudio(entry.audioFileName) }
        let statement = try open().statement("DELETE FROM history WHERE id = ?", [.text(id.uuidString)])
        try statement.finish()
        // Individual deletion deliberately does not advance the device-wide Insights clear cutoff.
    }

    public func clear(through date: Date) throws {
        guard date.timeIntervalSinceReferenceDate.isFinite else { throw HistoryDatabaseError.invalidEntry }
        let db = try open()
        try removeAllAudio()
        try db.execute("BEGIN IMMEDIATE")
        do {
            try db.execute("DELETE FROM history")
            let cutoff = try db.statement("""
                INSERT INTO history_clear_state (id, cleared_through) VALUES (1, ?)
                ON CONFLICT(id) DO UPDATE SET cleared_through = MAX(cleared_through, excluded.cleared_through)
                """, [.number(date.timeIntervalSinceReferenceDate)])
            try cutoff.finish()
            try db.execute("COMMIT")
        } catch {
            try? db.execute("ROLLBACK")
            throw error
        }
    }

    /// Insights can reject late events at or before this cutoff without coupling individual deletion to counts.
    public func clearedThrough() throws -> Date? {
        let statement = try open().statement("SELECT cleared_through FROM history_clear_state WHERE id = 1")
        return try statement.next() ? Date(timeIntervalSinceReferenceDate: statement.number(0)) : nil
    }

    func saveRecording(_ input: HistoryEntry, audio: CapturedAudio, retain: Bool) throws -> HistoryRecordingSave {
        var entry = input
        entry.audioDuration = audio.duration
        var audioFailed = false
        if retain {
            do {
                try ensureAudioDirectory()
                let name = "Whisper-" + entry.localDate + "-" + entry.id.uuidString + ".m4a"
                let destination = audioDirectory.appendingPathComponent(name)
                let staging = audioDirectory.appendingPathComponent(".partial-" + UUID().uuidString)
                defer { try? FileManager.default.removeItem(at: staging) }
                try FileManager.default.copyItem(at: audio.url, to: staging)
                try FileManager.default.setAttributes([.posixPermissions: 0o600, .modificationDate: entry.createdAt], ofItemAtPath: staging.path)
                guard rename(staging.path, destination.path) == 0 else { throw HistoryAudioFailure.save }
                entry.audioFileName = name
            } catch { audioFailed = true }
        }
        // A discarded row has no useful text and must not outlive an unsuccessful audio write.
        if entry.status == .discarded && entry.audioFileName == nil {
            return HistoryRecordingSave(entry: nil, audioFailed: audioFailed)
        }
        do { return try HistoryRecordingSave(entry: save(entry), audioFailed: audioFailed) }
        catch { try? removeAudio(entry.audioFileName); throw error }
    }

    public func retainedAudioURL(for id: UUID) throws -> URL? {
        guard let entry = try entry(id), entry.source == .dictation else { return nil }
        return availableAudio(entry.audioFileName)
    }

    func prepareRetry(_ id: UUID) throws -> HistoryRetryInput {
        try Task.checkCancellation()
        guard let entry = try entry(id), entry.source == .dictation,
              let file = availableAudio(entry.audioFileName) else { throw HistoryAudioFailure.missing }
        let temporary = profile.directory.appendingPathComponent("Temporary", isDirectory: true)
        if !FileManager.default.fileExists(atPath: temporary.path) {
            try FileManager.default.createDirectory(at: temporary, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        }
        let directory = temporary.appendingPathComponent("retry-" + UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        do {
            let destination = directory.appendingPathComponent("audio.m4a")
            try FileManager.default.copyItem(at: file, to: destination)
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: destination.path)
            return HistoryRetryInput(entry: entry, audio: CapturedAudio(url: destination, duration: entry.audioDuration ?? 0))
        } catch { try? FileManager.default.removeItem(at: directory); throw error }
    }

    /// Update only an existing, still-retained recording. Late retries cannot resurrect deleted or expired rows.
    func updateRetry(_ entry: HistoryEntry, expectedAudio: String, ownership: HistoryRetryOwnership) throws -> HistoryEntry? {
        guard ownership.isActive, !Task.isCancelled,
              let existing = try self.entry(entry.id), existing.audioFileName == expectedAudio,
              availableAudio(expectedAudio) != nil else { return nil }
        let db = try open()
        try db.execute("BEGIN IMMEDIATE")
        do {
            let statement = try db.statement("""
                UPDATE history SET text = ?, raw_text = ?, status = 'completed', provider = ?, model = ?,
                    error_code = NULL, error_message = NULL, search_text = ? WHERE id = ? AND audio_file = ?
                """, [.text(entry.text), .text(entry.rawText), .text(entry.provider), .text(entry.model),
                       .text((entry.text + "\n" + entry.rawText).precomposedStringWithCanonicalMapping.lowercased()),
                       .text(entry.id.uuidString), .text(expectedAudio)])
            try statement.finish()
            guard ownership.isActive, !Task.isCancelled else {
                try db.execute("ROLLBACK")
                return nil
            }
            try db.execute("COMMIT")
            return try self.entry(entry.id)
        } catch { try? db.execute("ROLLBACK"); throw error }
    }

    public func expireTranscripts(before cutoff: Date) throws -> [UUID] {
        let db = try open()
        let query = try db.statement("SELECT id, audio_file FROM history WHERE created_at < ?", [.number(cutoff.timeIntervalSinceReferenceDate)])
        var ids: [UUID] = []
        while try query.next() {
            if let value = query.text(0).flatMap(UUID.init(uuidString:)) { ids.append(value) }
            try removeAudio(query.text(1))
        }
        try db.execute("BEGIN IMMEDIATE")
        do {
            let remove = try db.statement("DELETE FROM history WHERE created_at < ?", [.number(cutoff.timeIntervalSinceReferenceDate)])
            try remove.finish()
            // Integrate Insights expiry at the same cutoff inside this transaction.
            try db.execute("COMMIT")
        } catch { try? db.execute("ROLLBACK"); throw error }
        return ids
    }

    public func expire(preferences: HistoryPreferences, now: Date) throws -> HistoryRetentionResult {
        let cutoff = preferences.transcriptRetentionDays > 0 ? now.addingTimeInterval(-Double(preferences.transcriptRetentionDays) * 86400) : nil
        let ids = try cutoff.map { try expireTranscripts(before: $0) } ?? []
        var audioIDs: [UUID] = []
        let db = try open()
        let query = try db.statement("SELECT id, audio_file FROM history WHERE audio_file IS NOT NULL")
        while try query.next() {
            let name = query.text(1)
            let file = availableAudio(name)
            let modified = file.flatMap { try? $0.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate }
            let expired = preferences.audioRetentionDays > 0 && modified.map { $0 < now.addingTimeInterval(-Double(preferences.audioRetentionDays) * 86400) } == true
            if file == nil || expired {
                try removeAudio(name)
                let update = try db.statement("UPDATE history SET audio_file = NULL WHERE id = ?", [.text(query.text(0) ?? "")])
                try update.finish()
                if let id = query.text(0).flatMap(UUID.init(uuidString:)) { audioIDs.append(id) }
            }
        }
        return HistoryRetentionResult(transcriptIDs: ids, transcriptCutoff: cutoff, audioIDs: audioIDs)
    }

    public func clearAudio() throws {
        try removeAllAudio()
        try open().execute("UPDATE history SET audio_file = NULL")
    }

    private var audioDirectory: URL { profile.directory.appendingPathComponent("Audio", isDirectory: true) }
    private func ensureAudioDirectory() throws {
        _ = try open()
        if !FileManager.default.fileExists(atPath: audioDirectory.path) {
            try FileManager.default.createDirectory(at: audioDirectory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        }
    }
    private func availableAudio(_ name: String?) -> URL? {
        guard let name, !name.isEmpty, name != ".", name != "..", !name.contains("/"), !name.contains("\\") else { return nil }
        let url = audioDirectory.appendingPathComponent(name)
        guard let values = try? url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey]),
              values.isRegularFile == true, values.isSymbolicLink != true else { return nil }
        return url
    }
    private func removeAudio(_ name: String?) throws {
        if let file = availableAudio(name) { try FileManager.default.removeItem(at: file) }
    }
    private func removeAllAudio() throws {
        guard FileManager.default.fileExists(atPath: audioDirectory.path) else { return }
        for file in try FileManager.default.contentsOfDirectory(at: audioDirectory, includingPropertiesForKeys: nil) {
            try FileManager.default.removeItem(at: file)
        }
    }

    private static let columns = "id, text, raw_text, occurred_at, created_at, local_date, source, status, provider, model, audio_duration, audio_file, error_code, error_message"

    private func read(_ statement: HistoryStatement) throws -> HistoryEntry {
        guard let id = UUID(uuidString: statement.text(0) ?? ""),
              let source = HistorySource(rawValue: statement.text(6) ?? ""),
              let status = HistoryStatus(rawValue: statement.text(7) ?? "") else { throw HistoryDatabaseError.unavailable }
        var entry = HistoryEntry(
            id: id, text: statement.text(1) ?? "", rawText: statement.text(2) ?? "",
            occurredAt: Date(timeIntervalSinceReferenceDate: statement.number(3)), createdAt: Date(timeIntervalSinceReferenceDate: statement.number(4)),
            localDate: statement.text(5), source: source, status: status,
            provider: statement.text(8) ?? "", model: statement.text(9) ?? "",
            audioDuration: statement.isNull(10) ? nil : statement.number(10), audioFileName: statement.text(11),
            errorCode: statement.text(12), errorMessage: statement.text(13)
        )
        if entry.source != .dictation || availableAudio(entry.audioFileName) == nil { entry.audioFileName = nil }
        return entry
    }

    private func open() throws -> HistoryDatabase {
        if let database { return database }
        guard !unreadable else { throw HistoryDatabaseError.unavailable }
        do {
            if !FileManager.default.fileExists(atPath: profile.directory.path) {
                // Do not recreate a removed parent profile or a deleted test fixture asynchronously.
                try FileManager.default.createDirectory(at: profile.directory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
            }
            let url = profile.directory.appendingPathComponent("history.sqlite")
            let db = try HistoryDatabase(url: url)
            let version = try db.statement("PRAGMA user_version")
            guard try version.next() else { throw HistoryDatabaseError.unavailable }
            let schemaVersion = Int(version.number(0))
            try version.finish()
            switch schemaVersion {
            case 0:
                let tables = try db.statement("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
                guard try tables.next(), tables.number(0) == 0 else { throw HistoryDatabaseError.unavailable }
                try tables.finish()
                try db.execute("""
                    BEGIN IMMEDIATE;
                    CREATE TABLE history (
                        id TEXT PRIMARY KEY, text TEXT NOT NULL, raw_text TEXT NOT NULL,
                        occurred_at REAL NOT NULL, created_at REAL NOT NULL, local_date TEXT NOT NULL,
                        source TEXT NOT NULL, status TEXT NOT NULL, provider TEXT NOT NULL, model TEXT NOT NULL,
                        audio_duration REAL, audio_file TEXT, error_code TEXT, error_message TEXT, search_text TEXT NOT NULL
                    );
                    CREATE INDEX history_order ON history (occurred_at DESC, created_at DESC, id DESC);
                    CREATE TABLE history_clear_state (id INTEGER PRIMARY KEY CHECK(id = 1), cleared_through REAL NOT NULL);
                    PRAGMA user_version = 1;
                    COMMIT;
                    """)
            case 1: break
            default: throw HistoryDatabaseError.unavailable
            }
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
            try db.execute("PRAGMA journal_mode = WAL")
            self.database = db
            return db
        } catch {
            unreadable = true
            throw HistoryDatabaseError.unavailable
        }
    }
}

private enum HistoryDatabaseError: Error { case unavailable, invalidEntry }
private enum HistorySQLValue { case text(String), number(Double), null }

// The actor is the only caller. This wrapper allows connection cleanup from deinit.
private final class HistoryDatabase: @unchecked Sendable {
    let handle: OpaquePointer
    init(url: URL) throws {
        var handle: OpaquePointer?
        guard sqlite3_open_v2(url.path, &handle, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX, nil) == SQLITE_OK, let handle else {
            if let handle { sqlite3_close(handle) }
            throw HistoryDatabaseError.unavailable
        }
        self.handle = handle
        sqlite3_busy_timeout(handle, 3000)
    }
    deinit { sqlite3_close(handle) }
    func execute(_ sql: String) throws {
        guard sqlite3_exec(handle, sql, nil, nil, nil) == SQLITE_OK else { throw HistoryDatabaseError.unavailable }
    }
    func statement(_ sql: String, _ values: [HistorySQLValue] = []) throws -> HistoryStatement {
        try HistoryStatement(database: self, sql: sql, values: values)
    }
}

private final class HistoryStatement {
    private let database: HistoryDatabase
    let handle: OpaquePointer
    init(database: HistoryDatabase, sql: String, values: [HistorySQLValue]) throws {
        self.database = database
        var handle: OpaquePointer?
        guard sqlite3_prepare_v2(database.handle, sql, -1, &handle, nil) == SQLITE_OK, let handle else {
            if let handle { sqlite3_finalize(handle) }
            throw HistoryDatabaseError.unavailable
        }
        self.handle = handle
        for (offset, value) in values.enumerated() {
            let index = Int32(offset + 1)
            let status: Int32
            switch value {
            case let .text(value):
                guard let length = Int32(exactly: value.utf8.count) else { throw HistoryDatabaseError.invalidEntry }
                status = value.withCString { sqlite3_bind_text(handle, index, $0, length, unsafeBitCast(-1, to: sqlite3_destructor_type.self)) }
            case let .number(value): status = sqlite3_bind_double(handle, index, value)
            case .null: status = sqlite3_bind_null(handle, index)
            }
            guard status == SQLITE_OK else { throw HistoryDatabaseError.unavailable }
        }
    }
    deinit { sqlite3_finalize(handle) }
    func next() throws -> Bool {
        let result = sqlite3_step(handle)
        guard result == SQLITE_ROW || result == SQLITE_DONE else { throw HistoryDatabaseError.unavailable }
        return result == SQLITE_ROW
    }
    func finish() throws { guard try !next() else { throw HistoryDatabaseError.unavailable } }
    func text(_ column: Int32) -> String? {
        sqlite3_column_text(handle, column).map {
            String(decoding: UnsafeBufferPointer(start: $0, count: Int(sqlite3_column_bytes(handle, column))), as: UTF8.self)
        }
    }
    func number(_ column: Int32) -> Double { sqlite3_column_double(handle, column) }
    func isNull(_ column: Int32) -> Bool { sqlite3_column_type(handle, column) == SQLITE_NULL }
}
