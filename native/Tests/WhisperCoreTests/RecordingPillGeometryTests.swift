import CoreGraphics
import Foundation
import Testing
import WhisperCore

@MainActor private final class ControlledPillDisplays: PillDisplaySystem {
    static let primary = PillDisplay(id: "primary", frame: CGRect(x: 0, y: 0, width: 1440, height: 900), workArea: CGRect(x: 0, y: 24, width: 1440, height: 852))
    static let left = PillDisplay(id: "left", frame: CGRect(x: -1920, y: 0, width: 1920, height: 1080), workArea: CGRect(x: -1920, y: 24, width: 1920, height: 1056))
    var value = PillDisplaySnapshot(displays: [primary, left], cursor: CGPoint(x: 720, y: 400))
    var windows: [CGRect] = []
    var suspend = false
    var requests: [PasteTarget] = []
    var returned = 0
    private var pending: [Int: CheckedContinuation<[CGRect], Never>] = [:]
    func snapshot() -> PillDisplaySnapshot { value }
    func windowBounds(for target: PasteTarget) async -> [CGRect] {
        let index = requests.count
        requests.append(target)
        let result = suspend ? await withCheckedContinuation { pending[index] = $0 } : windows
        returned += 1
        return result
    }
    func resolve(_ index: Int, windows: [CGRect]) { pending.removeValue(forKey: index)?.resume(returning: windows) }
}

@Suite(.serialized) @MainActor
struct RecordingPillGeometryTests {
    private let size = CGSize(width: 170, height: 64)
    private let leftWindow = CGRect(x: -1800, y: 80, width: 1700, height: 900)

    @Test func targetLargestWindowWinsOverCursorAndToolbarOnAnotherDisplay() async throws {
        let displays = ControlledPillDisplays()
        displays.windows = [CGRect(x: 0, y: 850, width: 1400, height: 30), leftWindow]
        let fixture = try ShortcutFixture(pillDisplays: displays); defer { fixture.remove() }
        fixture.app.send(.recordingPillAction)
        fixture.microphones.sessions.last?.open()
        fixture.app.send(.updatePillGeometry(size: size, currentFrame: nil))
        await settle { !fixture.app.state.desktop.pillGeometryPending }
        #expect(displays.requests == [PasteTarget(processID: 101)])
        #expect(fixture.app.state.desktop.pillDisplayID == "left")
        #expect(fixture.app.state.desktop.pillFrame == CGRect(x: -174, y: 28, width: 170, height: 64))
    }

    @Test func unavailableTargetFallsBackToCursorAndKeepsChosenDisplayAcrossResize() async throws {
        let displays = ControlledPillDisplays()
        displays.value = .init(displays: [ControlledPillDisplays.primary, ControlledPillDisplays.left], cursor: CGPoint(x: -1000, y: 300))
        let fixture = try ShortcutFixture(pillDisplays: displays); defer { fixture.remove() }
        fixture.app.send(.recordingPillAction)
        fixture.microphones.sessions.last?.open()
        fixture.app.send(.updatePillGeometry(size: size, currentFrame: nil))
        await settle { !fixture.app.state.desktop.pillGeometryPending }
        let original = try #require(fixture.app.state.desktop.pillFrame)
        displays.value = .init(displays: displays.value.displays, cursor: CGPoint(x: 1000, y: 300))
        fixture.app.send(.updatePillGeometry(size: CGSize(width: 390, height: 220), currentFrame: original))
        #expect(displays.requests.count == 1)
        #expect(fixture.app.state.desktop.pillDisplayID == "left")
        #expect(fixture.app.state.desktop.pillFrame?.maxX == original.maxX)
        #expect(fixture.app.state.desktop.pillFrame?.minY == original.minY)
    }

    @Test func staleTargetLookupCannotMoveANewRecordingAndCancellationPreventsItsMove() async throws {
        let displays = ControlledPillDisplays(); displays.suspend = true
        let fixture = try ShortcutFixture(pillDisplays: displays); defer { fixture.remove() }
        fixture.app.send(.updatePillGeometry(size: size, currentFrame: nil))
        let initial = fixture.app.state.desktop.pillFrame
        fixture.app.send(.recordingPillAction)
        fixture.microphones.sessions.last?.open()
        fixture.app.send(.updatePillGeometry(size: size, currentFrame: initial))
        await settle { displays.requests.count == 1 }
        fixture.app.send(.cancelDictation)
        displays.resolve(0, windows: [leftWindow])
        await settle { displays.returned == 1 }
        #expect(fixture.app.state.desktop.pillFrame == initial)
        fixture.paste.frontmost = PasteTarget(processID: 202)
        fixture.app.send(.recordingPillAction)
        fixture.microphones.sessions.last?.open()
        fixture.app.send(.updatePillGeometry(size: size, currentFrame: initial))
        await settle { displays.requests.count == 2 }
        fixture.app.send(.cancelDictation)
        fixture.paste.frontmost = PasteTarget(processID: 303)
        fixture.app.send(.recordingPillAction)
        fixture.microphones.sessions.last?.open()
        fixture.app.send(.updatePillGeometry(size: size, currentFrame: initial))
        await settle { displays.requests.count == 3 }
        displays.resolve(2, windows: [CGRect(x: 30, y: 50, width: 1200, height: 700)])
        await settle { !fixture.app.state.desktop.pillGeometryPending }
        let current = fixture.app.state.desktop.pillFrame
        displays.resolve(1, windows: [leftWindow])
        await settle { displays.returned == 3 }
        #expect(fixture.app.state.desktop.pillDisplayID == "primary")
        #expect(fixture.app.state.desktop.pillFrame == current)
    }

    @Test func handsFreeSubmissionTargetDoesNotRelocateTheExistingPill() async throws {
        let displays = ControlledPillDisplays(); displays.windows = [leftWindow]
        let fixture = try ShortcutFixture(pillDisplays: displays); defer { fixture.remove() }
        _ = await fixture.doubleTap()
        fixture.app.send(.updatePillGeometry(size: size, currentFrame: nil))
        await settle { !fixture.app.state.desktop.pillGeometryPending }
        let frame = fixture.app.state.desktop.pillFrame
        fixture.paste.frontmost = PasteTarget(processID: 202)
        fixture.key(); fixture.key(down: false)
        fixture.app.send(.updatePillGeometry(size: size, currentFrame: frame))
        #expect(displays.requests.count == 1)
        #expect(fixture.app.state.desktop.pillFrame == frame)
        await settle { await fixture.transport.requests.count == 1 }
        await fixture.transport.reply()
        await settle { fixture.app.state.dictation.phase == .result }
        #expect(fixture.paste.pasted == [PasteTarget(processID: 202)])
    }

    @Test func slowTargetLookupFallsBackAfterReferenceDeadlineAndIgnoresItsLateResult() async throws {
        let displays = ControlledPillDisplays(); displays.suspend = true
        let fixture = try ShortcutFixture(pillDisplays: displays); defer { fixture.remove() }
        fixture.app.send(.recordingPillAction); fixture.microphones.sessions.last?.open()
        fixture.app.send(.updatePillGeometry(size: size, currentFrame: nil))
        await settle { displays.requests.count == 1 }
        fixture.clock.advance(0.699)
        #expect(fixture.app.state.desktop.pillGeometryPending)
        displays.value = .init(displays: displays.value.displays, cursor: CGPoint(x: -1000, y: 200))
        fixture.clock.advance(0.001)
        #expect(!fixture.app.state.desktop.pillGeometryPending)
        #expect(fixture.app.state.desktop.pillDisplayID == "left")
        let fallback = fixture.app.state.desktop.pillFrame
        displays.resolve(0, windows: [CGRect(x: 30, y: 50, width: 1200, height: 700)])
        await settle { displays.returned == 1 }
        #expect(fixture.app.state.desktop.pillFrame == fallback)
    }

    @Test func displayRemovalAndChangedBoundsClampWithoutFollowingTheCursor() async throws {
        let displays = ControlledPillDisplays(); displays.windows = [leftWindow]
        let fixture = try ShortcutFixture(pillDisplays: displays); defer { fixture.remove() }
        fixture.app.send(.recordingPillAction); fixture.microphones.sessions.last?.open()
        fixture.app.send(.updatePillGeometry(size: size, currentFrame: nil))
        await settle { !fixture.app.state.desktop.pillGeometryPending }
        let original = fixture.app.state.desktop.pillFrame
        let moved = PillDisplay(id: "left", frame: CGRect(x: -451, y: -1440, width: 2560, height: 1440))
        displays.value = .init(displays: [ControlledPillDisplays.primary, moved], cursor: CGPoint(x: 700, y: 400))
        fixture.app.send(.updatePillGeometry(size: size, currentFrame: original))
        let adjusted = try #require(fixture.app.state.desktop.pillFrame)
        #expect(fixture.app.state.desktop.pillDisplayID == "left")
        #expect(moved.frame.contains(adjusted))
        displays.value = .init(displays: [ControlledPillDisplays.primary], cursor: CGPoint(x: -5000, y: -5000))
        fixture.app.send(.updatePillGeometry(size: size, currentFrame: adjusted))
        #expect(fixture.app.state.desktop.pillDisplayID == "primary")
        #expect(ControlledPillDisplays.primary.workArea!.contains(try #require(fixture.app.state.desktop.pillFrame)))
    }

    @Test(arguments: PillPlacement.allCases)
    func anchorsAndOversizedFramesRespectNegativeWorkAreaOrigins(placement: PillPlacement) {
        let display = PillDisplay(id: "negative", frame: CGRect(x: -451, y: -1440, width: 2560, height: 1440))
        let frame = RecordingPillGeometry.frame(size: size, on: display, placement: placement)
        #expect(display.frame.contains(frame))
        #expect(frame.minY == -1436)
        let huge = RecordingPillGeometry.frame(size: CGSize(width: 3000, height: 1800), on: display, placement: placement)
        #expect(huge.origin == display.frame.origin)
    }

    @Test func quartzConversionAndCenteredLayoutUseTheReferenceCoordinateRules() {
        let bounds = RecordingPillGeometry.appKitBounds(fromQuartz: CGRect(x: -500, y: -1000, width: 1000, height: 800), primaryTop: 900)
        #expect(bounds == CGRect(x: -500, y: 1100, width: 1000, height: 800))
        let display = PillDisplay(id: "center", frame: CGRect(x: -1000, y: -800, width: 999, height: 800))
        let frame = RecordingPillGeometry.frame(size: CGSize(width: 170, height: 64), on: display, placement: .center,
            previous: CGRect(x: -50, y: -300, width: 170, height: 64), preservePosition: true)
        #expect(frame.minX == -585 && frame.minY == -796)
    }
}
