import Foundation
import Testing
import WhisperCore

@Suite(.serialized) @MainActor struct CorrectionLearningWorkflowTests {
    @Test func confirmedPasteLearnsPersistsAndFeedsNextActualASRRequest() async throws {
        let fixture = try LearningFixture()
        defer { fixture.remove() }
        await fixture.paste("Hey Shunade how are you")
        #expect(fixture.system.targets == [PasteTarget(processID: 101)])
        #expect(fixture.field.reads == 0)
        fixture.f.clock.advance(0.499)
        #expect(fixture.field.reads == 0)
        fixture.f.clock.advance(0.001)
        await settle { fixture.field.reads == 1 }
        await fixture.edit("Hey Sinead how are you")
        fixture.f.clock.advance(1.499)
        #expect(fixture.f.app.state.dictionary.words.isEmpty)
        fixture.f.clock.advance(0.001)
        await settle { fixture.f.app.state.dictionary.words == ["Sinead"] }
        #expect(fixture.f.app.state.corrections.learned.map(\.word) == ["Sinead"])
        #expect(fixture.f.app.state.dictionary.entries.first?.source == .learned)
        let reopened = WhisperApplication(profile: fixture.f.profile.profile, credentials: fixture.f.profile.credentials)
        #expect(reopened.state.dictionary.entries == fixture.f.app.state.dictionary.entries)
        _ = await fixture.f.hold()
        fixture.f.key(down: false)
        await settle { await fixture.f.transport.requests.count == 2 }
        let request = await fixture.f.transport.requests[1]
        #expect(String(decoding: request.body, as: UTF8.self).contains("name=\"prompt\"\r\n\r\nSinead"))
        await fixture.f.transport.reply(1, body: #"{"text":"Hey Sinead how are you"}"#)
        await settle { fixture.f.app.state.dictation.phase == .result }
    }

    @Test func debounceUsesLatestEditAndUndoPreservesManuallyPromotedWords() async throws {
        let fixture = try LearningFixture(); defer { fixture.remove() }
        await fixture.pasteAndStart("Hey Shunade how are you")
        await fixture.edit("Hey Sinad how are you")
        fixture.f.clock.advance(1)
        await fixture.edit("Hey Sinead how are you")
        fixture.f.clock.advance(0.5)
        #expect(fixture.f.app.state.dictionary.words.isEmpty)
        fixture.f.clock.advance(1)
        await settle { fixture.f.app.state.dictionary.words == ["Sinead"] }
        fixture.f.app.send(.importDictionary("Sinead, ParallelManual"))
        fixture.f.app.send(.undoLearnedCorrections)
        #expect(fixture.f.app.state.dictionary.words == ["Sinead", "ParallelManual"])
        #expect(fixture.f.app.state.dictionary.entries.allSatisfy { $0.source == .manual })
        #expect(fixture.f.app.state.corrections.learned.isEmpty)
    }

    @Test func undoRemovesOnlyTheLearnedEntriesFromThatNotification() async throws {
        let fixture = try LearningFixture(); defer { fixture.remove() }
        await fixture.pasteAndStart("Hey Shunade how are you")
        await fixture.edit("Hey Sinead how are you")
        fixture.f.clock.advance(1.5)
        await settle { fixture.f.app.state.dictionary.words == ["Sinead"] }
        fixture.f.app.send(.changeDictionary(add: ["OtherLearned"], remove: [], source: .learned))
        fixture.f.app.send(.undoLearnedCorrections)
        #expect(fixture.f.app.state.dictionary.words == ["OtherLearned"])
        #expect(fixture.field.observation?.cancelled == true)
        let reopened = WhisperApplication(profile: fixture.f.profile.profile, credentials: fixture.f.profile.credentials)
        #expect(reopened.state.dictionary.words == ["OtherLearned"])
    }

    @Test(arguments: ["probe", "keyboard", "copy", "disabled"])
    func recoveryCopyAndDisabledPreferenceNeverMonitor(reason: String) async throws {
        let fixture = try LearningFixture(); defer { fixture.remove() }
        fixture.f.paste.allowProbe = reason != "probe"
        fixture.f.paste.allowPaste = reason != "keyboard"
        if reason == "copy" { fixture.f.app.send(.setClipboardPreferences(autoPaste: false, keepResult: true)) }
        if reason == "disabled" { fixture.f.app.send(.setAutoLearnCorrections(false)) }
        await fixture.paste("Hey Shunade how are you", requirePaste: reason == "disabled")
        fixture.f.clock.advance(5)
        await Task.yield()
        #expect(fixture.field.reads == 0)
        #expect(fixture.field.observation == nil)
        #expect(fixture.f.app.state.dictionary.words.isEmpty)
        let reopened = WhisperApplication(profile: fixture.f.profile.profile, credentials: fixture.f.profile.credentials)
        #expect(reopened.state.settings.autoLearnCorrections == (reason != "disabled"))
    }

    @Test func disablingDuringPasteProbeDoesNotCaptureAField() async throws {
        let fixture = try LearningFixture(); defer { fixture.remove() }
        fixture.f.paste.suspendProbe = true
        _ = await fixture.f.hold()
        fixture.f.key(down: false)
        await settle { await fixture.f.transport.requests.count == 1 }
        await fixture.f.transport.reply(body: #"{"text":"Hey Shunade how are you"}"#)
        await settle { fixture.f.paste.probes == 1 }
        fixture.f.app.send(.setAutoLearnCorrections(false))
        fixture.f.paste.releaseProbe(true)
        await settle { fixture.f.app.state.dictation.phase == .result }
        #expect(fixture.f.app.state.dictation.delivery == .pasted)
        #expect(fixture.system.targets.isEmpty)
        fixture.f.clock.advance(5)
        #expect(fixture.field.reads == 0)
    }

    @Test func disablingClearsPendingEditsAndLateObserverCallbacks() async throws {
        let fixture = try LearningFixture(); defer { fixture.remove() }
        await fixture.pasteAndStart("Hey Shunade how are you")
        await fixture.edit("Hey Sinead how are you")
        fixture.f.app.send(.setAutoLearnCorrections(false))
        fixture.f.clock.advance(2)
        fixture.field.observation?.changed()
        await Task.yield()
        #expect(fixture.field.observation?.cancelled == true)
        #expect(fixture.f.app.state.dictionary.words.isEmpty)
        fixture.f.app.send(.setAutoLearnCorrections(true))
        fixture.f.clock.advance(5)
        #expect(fixture.f.app.state.dictionary.words.isEmpty)
    }

    @Test(arguments: ["focus", "selection", "outside", "paste-mismatch"])
    func unrelatedFieldsOrRegionsCannotTeachWords(boundary: String) async throws {
        let fixture = try LearningFixture(before: "Prefix old Suffix", range: 7..<10)
        defer { fixture.remove() }
        await fixture.paste("Hey Shunade how are you")
        if boundary == "paste-mismatch" { fixture.field.current = .init(text: "Prefix unrelated Suffix", selection: 16..<16) }
        fixture.f.clock.advance(0.5)
        await settle { fixture.field.reads >= 1 }
        if boundary != "paste-mismatch" {
            await fixture.edit("Hey Sinead how are you")
            if boundary == "focus" { fixture.field.focused = false }
            if boundary == "selection" { fixture.field.current = .init(text: fixture.field.current.text, selection: 0..<0) }
            if boundary == "outside" { fixture.field.current = .init(text: "Other " + fixture.field.current.text, selection: 7..<7) }
            fixture.f.clock.advance(1.5)
            await settle { fixture.field.reads >= 3 }
        }
        fixture.f.clock.advance(31)
        await Task.yield()
        #expect(fixture.f.app.state.dictionary.words.isEmpty)
    }

    @Test func selectedReplacementAndSurroundingTextStayScoped() async throws {
        let fixture = try LearningFixture(before: "Prefix old Suffix", range: 7..<10)
        defer { fixture.remove() }
        await fixture.pasteAndStart("Hey Shunade how are you")
        await fixture.edit("Hey Sinead how are you")
        fixture.f.clock.advance(1.5)
        await settle { fixture.f.app.state.dictionary.words == ["Sinead"] }
        #expect(fixture.field.current.text == "Prefix Hey Sinead how are you Suffix")
    }

    @Test func monitoringExpiresAtThirtySecondsAndNextRecordingInvalidatesPendingEdits() async throws {
        let fixture = try LearningFixture(); defer { fixture.remove() }
        await fixture.pasteAndStart("Hey Shunade how are you")
        fixture.f.clock.advance(29)
        await fixture.edit("Hey Sinead how are you")
        fixture.f.clock.advance(1)
        #expect(fixture.field.observation?.cancelled == true)
        fixture.f.clock.advance(2)
        #expect(fixture.f.app.state.dictionary.words.isEmpty)
        await fixture.pasteAndStart("Hey Shunade how are you", index: 1)
        await fixture.edit("Hey Sinead how are you")
        fixture.f.app.send(.startDictation)
        fixture.f.clock.advance(2)
        await Task.yield()
        #expect(fixture.f.app.state.dictionary.words.isEmpty)
        #expect(fixture.field.observation?.cancelled == true)
    }

    @Test(arguments: [
        ("the cat sat on the mat", "a dog stood under a rug", [String]()),
        ("I went to see XX today", "I went to see Al today", []),
        ("I saw a cat yesterday", "I saw a elephant yesterday", []),
        ("This is why I'm speaking", "This is what I'm speaking", []),
        ("Hey Sinead how are you", "Hey SINEAD how are you", []),
        ("Shunade said hi to Shunade", "Sinead said hi to Sinead", ["Sinead"]),
        ("Hey Shunade how are you", "Hey Sinead how are you", ["Sinead"]),
        ("Hey Shunade how are you", "Hey Shunade how are you and Sinead is here", [])
    ]) func matchingRulesRunThroughPasteAndCorrection(original: String, edited: String, expected: [String]) async throws {
        let fixture = try LearningFixture(); defer { fixture.remove() }
        await fixture.pasteAndStart(original)
        await fixture.edit(edited)
        fixture.f.clock.advance(1.5)
        await settle { fixture.field.reads >= 3 }
        if !expected.isEmpty { await settle { fixture.f.app.state.dictionary.words == expected } }
        else { try await Task.sleep(for: .milliseconds(25)) }
        #expect(fixture.f.app.state.dictionary.words == expected)
    }

    @Test func pendingPasteCanSettleBeforeObservationAndLateReadCannotReviveDisabledLearning() async throws {
        let fixture = try LearningFixture(); defer { fixture.remove() }
        fixture.f.paste.onPaste = nil
        await fixture.paste("Hey Shunade how are you")
        fixture.f.clock.advance(0.5)
        await settle { fixture.field.reads == 1 }
        #expect(fixture.field.observation?.cancelled == false)
        fixture.field.setRegion("Hey Shunade how are you")
        fixture.f.clock.advance(0.3)
        await settle { fixture.field.reads == 2 }
        fixture.field.suspendRead = true
        fixture.field.observation?.changed()
        await settle { fixture.field.pendingRead != nil }
        fixture.f.app.send(.setAutoLearnCorrections(false))
        fixture.field.finishRead()
        fixture.f.clock.advance(5)
        await Task.yield()
        #expect(fixture.field.observation?.cancelled == true)
        #expect(fixture.f.app.state.dictionary.words.isEmpty)
    }

    @Test func changesDuringAPendingFieldReadAreObservedWithoutWaitingForThePoll() async throws {
        let fixture = try LearningFixture(); defer { fixture.remove() }
        let original = "Hey Shunade how are you"
        await fixture.pasteAndStart(original)
        fixture.field.suspendRead = true
        fixture.field.observation?.changed()
        await settle { fixture.field.pendingRead != nil }
        fixture.field.setRegion("Hey Sinead how are you")
        fixture.field.observation?.changed()
        for _ in 0..<10 { await Task.yield() }
        fixture.field.finishRead(snapshot: .init(text: original, selection: original.utf16.count..<original.utf16.count))
        await settle { fixture.f.clock.scheduledDelays.contains { abs($0 - 1.5) < 0.000001 } }
        fixture.f.clock.advance(1.5)
        await settle { fixture.f.app.state.dictionary.words == ["Sinead"] }
    }

    @Test func existingVocabularyIsNotLearnedOrReannounced() async throws {
        let fixture = try LearningFixture(); defer { fixture.remove() }
        fixture.f.app.send(.importDictionary("sinead"))
        await fixture.pasteAndStart("Hey Shunade how are you")
        await fixture.edit("Hey Sinead how are you")
        fixture.f.clock.advance(1.5)
        await settle { fixture.field.reads >= 4 }
        #expect(fixture.f.app.state.dictionary.words == ["sinead"])
        #expect(fixture.f.app.state.dictionary.entries.first?.source == .manual)
        #expect(fixture.f.app.state.corrections.learned.isEmpty)
    }

    @Test func failedDictionaryWriteDoesNotAnnounceLearnedWords() async throws {
        let fixture = try LearningFixture(); defer { fixture.remove() }
        await fixture.pasteAndStart("Hey Shunade how are you")
        let file = fixture.f.profile.profile.directory.appendingPathComponent("dictionary.json")
        try FileManager.default.createDirectory(at: file, withIntermediateDirectories: false)
        await fixture.edit("Hey Sinead how are you")
        fixture.f.clock.advance(1.5)
        await settle { fixture.f.app.state.corrections.failure != nil }
        #expect(fixture.f.app.state.corrections.learned.isEmpty)
        #expect(fixture.f.app.state.dictionary.words.isEmpty)
    }
}

@MainActor final class ControlledCorrectionSystem: CorrectionMonitoringSystem {
    let field: ControlledCorrectionField
    var targets: [PasteTarget] = []
    init(field: ControlledCorrectionField) { self.field = field }
    func capture(_ target: PasteTarget) async -> (any CorrectionField)? { targets.append(target); return field }
}

@MainActor final class ControlledCorrectionField: CorrectionField {
    var beforePaste: CorrectionSnapshot
    var current: CorrectionSnapshot
    var focused = true
    var reads = 0
    var suspendRead = false
    var pendingRead: CheckedContinuation<CorrectionSnapshot?, Never>?
    var observation: ControlledCorrectionObservation?
    init(before: String, range: Range<Int>) { beforePaste = .init(text: before, selection: range); current = beforePaste }
    func read() async -> CorrectionSnapshot? {
        reads += 1
        if suspendRead { return await withCheckedContinuation { pendingRead = $0 } }
        return focused ? current : nil
    }
    func finishRead(snapshot: CorrectionSnapshot? = nil) { suspendRead = false; pendingRead?.resume(returning: focused ? snapshot ?? current : nil); pendingRead = nil }
    func observe(_ changed: @escaping @Sendable () -> Void) -> any CorrectionObservation {
        let observation = ControlledCorrectionObservation(changed: changed)
        self.observation = observation
        return observation
    }
    func setRegion(_ text: String) {
        let before = beforePaste.text as NSString
        let prefix = before.substring(to: beforePaste.selection.lowerBound)
        let suffix = before.substring(from: beforePaste.selection.upperBound)
        let end = prefix.utf16.count + text.utf16.count
        current = .init(text: prefix + text + suffix, selection: end..<end)
    }
}
final class ControlledCorrectionObservation: CorrectionObservation, @unchecked Sendable {
    let changed: @Sendable () -> Void
    private let lock = NSLock()
    private var ended = false
    var cancelled: Bool { lock.withLock { ended } }
    init(changed: @escaping @Sendable () -> Void) { self.changed = changed }
    func cancel() { lock.withLock { ended = true } }
}

@MainActor private final class LearningFixture {
    let field: ControlledCorrectionField
    let system: ControlledCorrectionSystem
    let f: ShortcutFixture
    init(before: String = "", range: Range<Int> = 0..<0) throws {
        field = ControlledCorrectionField(before: before, range: range)
        system = ControlledCorrectionSystem(field: field)
        f = try ShortcutFixture(correctionSystem: system)
        f.paste.onPaste = { [field] in field.setRegion($0) }
        f.app.send(.setClipboardPreferences(autoPaste: true, keepResult: true))
    }
    func paste(_ text: String, index: Int = 0, requirePaste: Bool = true) async {
        _ = await f.hold()
        f.key(down: false)
        await settle { await self.f.transport.requests.count > index }
        let data = try! JSONSerialization.data(withJSONObject: ["text": text])
        await f.transport.reply(index, body: String(decoding: data, as: UTF8.self))
        await settle { self.f.app.state.dictation.phase == .result }
        if requirePaste { #expect(f.app.state.dictation.delivery == .pasted) }
    }
    func pasteAndStart(_ text: String, index: Int = 0) async {
        let reads = field.reads
        await paste(text, index: index)
        f.clock.advance(0.5)
        // A read starting does not prove its result reached the learning workflow yet.
        await settle {
            self.field.reads > reads && self.f.clock.scheduledDelays.contains { abs($0 - 0.5) < 0.000001 }
        }
    }
    func edit(_ text: String) async {
        field.setRegion(text)
        field.observation?.changed()
        await settle { self.f.clock.scheduledDelays.contains { abs($0 - 1.5) < 0.000001 } }
    }
    func remove() { f.app.send(.setAutoLearnCorrections(false)); f.remove() }
}
