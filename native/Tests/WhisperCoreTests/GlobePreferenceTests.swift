import Foundation
import Testing
import WhisperCore

@MainActor private final class ControlledGlobePreferences: GlobePreferenceSystem {
    var value: Int32? = 2
    var explicit = true
    var allowUpdate = true
    var writes: [Int32] = []
    var clears = 0
    func read() -> Int32? { value }
    func hasExplicitValue() -> Bool { explicit }
    func update(_ value: Int32) -> Bool {
        guard allowUpdate else { return false }
        writes.append(value)
        self.value = value
        explicit = true
        return true
    }
    func clearExplicitValue() { explicit = false; clears += 1 }
}

@Suite("Globe preference ownership") @MainActor
struct GlobePreferenceTests {
    @Test(arguments: [false, true]) func selectionRestoresTheOriginalExplicitOrDefaultPreference(explicit: Bool) throws {
        let fixture = try ProfileFixture(); defer { fixture.remove() }
        let marker = fixture.profile.directory.appendingPathComponent("globe-preference.json")
        let system = ControlledGlobePreferences()
        system.explicit = explicit
        let controller = GlobePreferenceController(markerURL: marker, system: system)
        #expect(controller.setOwned(false))
        #expect(system.writes.isEmpty)
        #expect(controller.setOwned(true))
        #expect(system.value == 0)
        #expect(FileManager.default.fileExists(atPath: marker.path))
        #expect(controller.setOwned(true))
        #expect(system.writes == [0])
        #expect(controller.setOwned(false))
        #expect(system.value == 2)
        #expect(system.explicit == explicit)
        #expect(system.clears == (explicit ? 0 : 1))
        #expect(!FileManager.default.fileExists(atPath: marker.path))
    }

    @Test func crashJournalRestoresOriginalInsteadOfSavingTheAppsOverride() throws {
        let fixture = try ProfileFixture(); defer { fixture.remove() }
        let marker = fixture.profile.directory.appendingPathComponent("globe-preference.json")
        let system = ControlledGlobePreferences()
        let first = GlobePreferenceController(markerURL: marker, system: system)
        #expect(first.setOwned(true))
        let reopened = GlobePreferenceController(markerURL: marker, system: system)
        #expect(reopened.setOwned(true))
        #expect(reopened.setOwned(false))
        #expect(system.value == 2)
        #expect(system.writes == [0, 2])
        #expect(!FileManager.default.fileExists(atPath: marker.path))
    }

    @Test(arguments: [false, true]) func aNewUserPreferenceAlwaysWins(afterCrash: Bool) throws {
        let fixture = try ProfileFixture(); defer { fixture.remove() }
        let marker = fixture.profile.directory.appendingPathComponent("globe-preference.json")
        let system = ControlledGlobePreferences()
        let first = GlobePreferenceController(markerURL: marker, system: system)
        #expect(first.setOwned(true))
        system.value = 3
        let controller = afterCrash ? GlobePreferenceController(markerURL: marker, system: system) : first
        #expect(controller.setOwned(false))
        #expect(system.value == 3)
        #expect(system.writes == [0])
        #expect(!FileManager.default.fileExists(atPath: marker.path))
    }

    @Test func journalOrSystemFailureNeverPretendsSuppressionSucceeded() throws {
        let fixture = try ProfileFixture(); defer { fixture.remove() }
        let blocked = fixture.profile.directory.appendingPathComponent("blocked")
        try FileManager.default.createDirectory(at: fixture.profile.directory, withIntermediateDirectories: true)
        try Data("fixture".utf8).write(to: blocked)
        let system = ControlledGlobePreferences()
        let unwritable = GlobePreferenceController(markerURL: blocked.appendingPathComponent("marker.json"), system: system)
        #expect(!unwritable.setOwned(true))
        #expect(system.writes.isEmpty)
        let marker = fixture.profile.directory.appendingPathComponent("globe-preference.json")
        let controller = GlobePreferenceController(markerURL: marker, system: system)
        system.allowUpdate = false
        #expect(!controller.setOwned(true))
        #expect(!controller.setOwned(true))
        #expect(system.value == 2)
        #expect(FileManager.default.fileExists(atPath: marker.path))
        #expect(controller.setOwned(false))
        #expect(!FileManager.default.fileExists(atPath: marker.path))
        system.allowUpdate = true
        #expect(controller.setOwned(true))
        system.allowUpdate = false
        #expect(!controller.setOwned(false))
        #expect(FileManager.default.fileExists(atPath: marker.path))
        system.allowUpdate = true
        #expect(controller.setOwned(false))
    }

    @Test func anExistingDoNothingPreferenceNeedsNoOwnershipOrRestoration() throws {
        let fixture = try ProfileFixture(); defer { fixture.remove() }
        let marker = fixture.profile.directory.appendingPathComponent("globe-preference.json")
        let system = ControlledGlobePreferences()
        system.value = 0
        let controller = GlobePreferenceController(markerURL: marker, system: system)
        #expect(controller.setOwned(true))
        #expect(controller.setOwned(false))
        #expect(system.writes.isEmpty)
        #expect(!FileManager.default.fileExists(atPath: marker.path))
    }
}
