import Foundation

public struct CorrectionSnapshot: Sendable, Equatable {
    public let text: String
    /// Accessibility selection coordinates use UTF-16, including a caret's empty range.
    public let selection: Range<Int>
    public init(text: String, selection: Range<Int>) { self.text = text; self.selection = selection }
}

public protocol CorrectionObservation: Sendable { func cancel() }

@MainActor public protocol CorrectionField: AnyObject {
    var beforePaste: CorrectionSnapshot { get }
    /// Nil means the captured field is no longer focused, readable or editable.
    func read() async -> CorrectionSnapshot?
    func observe(_ changed: @escaping @Sendable () -> Void) -> any CorrectionObservation
}

@MainActor public protocol CorrectionMonitoringSystem {
    func capture(_ target: PasteTarget) async -> (any CorrectionField)?
}

public struct CorrectionLearningState: Equatable, Sendable {
    public var learned: [DictionaryEntry] = []
    public var failure: DictionaryFailure?
    public init() {}
}

/// Coordinates one confirmed paste against one captured field and insertion range.
@MainActor final class CorrectionLearning {
    let system: any CorrectionMonitoringSystem
    let clock: any WorkflowClock
    let learned: @MainActor ([String]) -> Void
    var generation = UUID()
    var field: (any CorrectionField)?
    var observation: (any CorrectionObservation)?
    var timer: (any ScheduledAction)?
    var expiry: (any ScheduledAction)?
    var debounce: (any ScheduledAction)?
    var readTask: Task<Void, Never>?
    var commitTask: Task<Void, Never>?
    var original = ""
    var prefix = ""
    var suffix = ""
    var lastRegion: String?
    var baselineConfirmed = false
    var baselineAttempts = 0
    var queryAgain = false

    init(system: any CorrectionMonitoringSystem, clock: any WorkflowClock, learned: @escaping @MainActor ([String]) -> Void) {
        self.system = system; self.clock = clock; self.learned = learned
    }
    deinit { observation?.cancel(); readTask?.cancel(); commitTask?.cancel() }

    func prepare(_ target: PasteTarget, text: String) async {
        stop()
        let id = generation
        let field = await system.capture(target)
        guard generation == id, !Task.isCancelled, let field else { return }
        let before = field.beforePaste
        let value = before.text as NSString
        guard before.selection.lowerBound >= 0, before.selection.upperBound <= value.length else { return }
        self.field = field
        original = text
        prefix = value.substring(to: before.selection.lowerBound)
        suffix = value.substring(from: before.selection.upperBound)
    }

    func confirmedPaste() {
        guard field != nil else { return }
        let id = generation
        timer = clock.schedule(after: 0.5) { [weak self] in
            guard let self, generation == id else { return }
            timer = nil
            expiry = clock.schedule(after: 30) { [weak self] in self?.stop() }
            observation = field?.observe { [weak self] in Task { @MainActor in self?.query(id) } }
            query(id)
        }
    }

    func stop() {
        generation = UUID()
        observation?.cancel(); observation = nil
        timer?.cancel(); timer = nil
        expiry?.cancel(); expiry = nil
        debounce?.cancel(); debounce = nil
        readTask?.cancel(); readTask = nil
        commitTask?.cancel(); commitTask = nil
        field = nil
        original = ""; prefix = ""; suffix = ""; lastRegion = nil
        baselineConfirmed = false
        baselineAttempts = 0
        queryAgain = false
    }

    private func region(_ snapshot: CorrectionSnapshot) -> String? {
        guard snapshot.text.hasPrefix(prefix), snapshot.text.hasSuffix(suffix) else { return nil }
        let value = snapshot.text as NSString
        let start = prefix.utf16.count
        let end = value.length - suffix.utf16.count
        guard end >= start, snapshot.selection.lowerBound >= start, snapshot.selection.upperBound <= end else { return nil }
        return value.substring(with: NSRange(location: start, length: end - start))
    }

    private func query(_ id: UUID) {
        guard generation == id, let field else { return }
        guard readTask == nil else { queryAgain = true; return }
        readTask = Task { [weak self] in
            let snapshot = await field.read()
            guard let self, generation == id, !Task.isCancelled else { return }
            defer {
                readTask = nil
                if generation == id, queryAgain {
                    queryAgain = false
                    query(id)
                }
            }
            guard let snapshot, let edited = region(snapshot) else { stop(); return }
            if !baselineConfirmed {
                // Seeing the exact inserted text proves the paste landed in this captured scope.
                if edited != original {
                    baselineAttempts += 1
                    guard baselineAttempts < 5, snapshot == field.beforePaste else { stop(); return }
                    timer?.cancel()
                    timer = clock.schedule(after: 0.3) { [weak self] in self?.query(id) }
                    return
                }
                baselineConfirmed = true
                lastRegion = edited
            } else if edited != lastRegion {
                lastRegion = edited
                debounce?.cancel()
                commitTask?.cancel(); commitTask = nil
                debounce = clock.schedule(after: 1.5) { [weak self] in self?.commit(id, edited: edited) }
            }
            timer?.cancel()
            timer = clock.schedule(after: 0.5) { [weak self] in self?.query(id) }
        }
    }

    private func commit(_ id: UUID, edited: String) {
        guard generation == id, let field else { return }
        debounce = nil
        let original = original
        commitTask = Task { [weak self] in
            // Recheck field, focus and selection after the debounce, before storing any word.
            let snapshot = await field.read()
            guard self?.generation == id, !Task.isCancelled,
                  let snapshot, self?.region(snapshot) == edited, self?.lastRegion == edited else { return }
            let matcher = Task.detached { CorrectionMatcher.extract(original: original, field: edited) }
            let words = await withTaskCancellationHandler { await matcher.value } onCancel: { matcher.cancel() }
            let current = await field.read()
            guard self?.generation == id, !Task.isCancelled, let current, self?.region(current) == edited else { return }
            self?.learned(words)
            self?.commitTask = nil
        }
    }
}

extension WhisperApplication {
    func setAutoLearnCorrections(_ enabled: Bool) {
        var settings = state.settings
        settings.autoLearnCorrections = enabled
        persist(settings)
        if !state.settings.autoLearnCorrections { correctionLearning.stop() }
    }

    func saveLearnedCorrections(_ words: [String]) {
        guard state.settings.autoLearnCorrections, !words.isEmpty else { return }
        var added: [DictionaryEntry] = []
        mutateDictionary { entries in
            var keys = Set(entries.map { $0.word.lowercased() })
            for word in words where keys.insert(word.lowercased()).inserted {
                let entry = DictionaryEntry(word: word, source: .learned)
                entries.append(entry); added.append(entry)
            }
            return added.count
        }
        if let failure = state.dictionary.failure { state.corrections.failure = failure }
        else if !added.isEmpty { state.corrections = CorrectionLearningState(); state.corrections.learned = added }
    }

    func undoLearnedCorrections() {
        correctionLearning.stop()
        let ids = Set(state.corrections.learned.map(\.id))
        guard !ids.isEmpty else { return }
        mutateDictionary { entries in
            entries.removeAll { ids.contains($0.id) && $0.source == .learned }
            return 0
        }
        if let failure = state.dictionary.failure { state.corrections.failure = failure }
        else { state.corrections = CorrectionLearningState() }
    }
}
