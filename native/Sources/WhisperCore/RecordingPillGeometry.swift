import Foundation
import CoreGraphics

/// All rectangles use AppKit global coordinates, with a bottom-left origin.
public struct PillDisplay: Equatable, Sendable {
    public let id: String
    public let frame: CGRect
    public let workArea: CGRect?
    public init(id: String, frame: CGRect, workArea: CGRect? = nil) {
        self.id = id; self.frame = frame; self.workArea = workArea
    }
}

public struct PillDisplaySnapshot: Sendable {
    public let displays: [PillDisplay]
    public let cursor: CGPoint
    public init(displays: [PillDisplay], cursor: CGPoint = .zero) { self.displays = displays; self.cursor = cursor }
}

@MainActor public protocol PillDisplaySystem {
    func snapshot() -> PillDisplaySnapshot
    func windowBounds(for target: PasteTarget) async -> [CGRect]
}

@MainActor public struct InertPillDisplaySystem: PillDisplaySystem {
    public init() {}
    public func snapshot() -> PillDisplaySnapshot { .init(displays: []) }
    public func windowBounds(for target: PasteTarget) async -> [CGRect] { [] }
}

public enum RecordingPillGeometry {
    public static func display(in snapshot: PillDisplaySnapshot, targetWindows: [CGRect] = [],
                               preferredID: String? = nil, currentFrame: CGRect? = nil) -> PillDisplay? {
        let displays = snapshot.displays.filter { valid($0.frame) }
        guard !displays.isEmpty else { return nil }
        if let preferredID, let existing = displays.first(where: { $0.id == preferredID }) { return existing }
        // Largest layer-zero target window matches the reference and avoids browser toolbar strips.
        if let target = targetWindows.filter(valid).max(by: { area($0) < area($1) }) {
            let matching = displays.max { area($0.frame.intersection(target)) < area($1.frame.intersection(target)) }
            if let matching, area(matching.frame.intersection(target)) > 0 { return matching }
            return nearest(CGPoint(x: target.midX, y: target.midY), displays: displays)
        }
        if let currentFrame, valid(currentFrame) {
            return nearest(CGPoint(x: currentFrame.midX, y: currentFrame.minY), displays: displays)
        }
        return nearest(snapshot.cursor, displays: displays)
    }

    public static func frame(size: CGSize, on display: PillDisplay, placement: PillPlacement,
                             previous: CGRect? = nil, preservePosition: Bool = false) -> CGRect {
        let area = display.workArea.flatMap { valid($0) ? $0 : nil } ?? display.frame
        let size = CGSize(width: size.width.isFinite && size.width > 0 ? size.width : 170,
                          height: size.height.isFinite && size.height > 0 ? size.height : 64)
        var origin = CGPoint(x: area.maxX - size.width - 4, y: area.minY + 4)
        if placement == .bottomLeft { origin.x = area.minX + 4 }
        if placement == .center { origin.x = (area.minX + (area.width - size.width) / 2 + 0.5).rounded(.down) }
        if preservePosition, placement != .center, let previous, valid(previous) {
            origin.x = previous.midX < area.midX ? previous.minX : previous.maxX - size.width
            origin.y = previous.minY
        }
        origin.x = max(area.minX, min(origin.x, area.maxX - size.width))
        origin.y = max(area.minY, min(origin.y, area.maxY - size.height))
        return CGRect(origin: origin, size: size)
    }

    public static func appKitBounds(fromQuartz bounds: CGRect, primaryTop: CGFloat) -> CGRect {
        CGRect(x: bounds.minX, y: primaryTop - bounds.maxY, width: bounds.width, height: bounds.height)
    }

    private static func valid(_ rect: CGRect) -> Bool {
        rect.origin.x.isFinite && rect.origin.y.isFinite && rect.width.isFinite && rect.height.isFinite
            && rect.width > 0 && rect.height > 0
    }
    private static func area(_ rect: CGRect) -> CGFloat { valid(rect) ? rect.width * rect.height : 0 }
    private static func nearest(_ point: CGPoint, displays: [PillDisplay]) -> PillDisplay? {
        guard point.x.isFinite, point.y.isFinite else { return displays.first }
        func distance(_ display: PillDisplay) -> CGFloat {
            let x = max(display.frame.minX, min(point.x, display.frame.maxX)) - point.x
            let y = max(display.frame.minY, min(point.y, display.frame.maxY)) - point.y
            return x * x + y * y
        }
        return displays.min { distance($0) < distance($1) }
    }
}

extension WhisperApplication {
    func beginPillTarget(_ target: PasteTarget?) {
        cancelPillTargetLookup()
        pillDisplayTarget = target
        pillHasResolvedRequest = false
    }

    func cancelPillTargetLookup() {
        pillGeometryGeneration += 1
        pillGeometryTask?.cancel()
        pillGeometryTask = nil
        pillGeometryDeadline?.cancel()
        pillGeometryDeadline = nil
        state.desktop.pillGeometryPending = false
        pillResolvedRequestID = state.dictation.requestID
        pillHasResolvedRequest = true
    }

    func updatePillGeometry(size: CGSize, currentFrame: CGRect?) {
        pillGeometryTask?.cancel()
        pillGeometryDeadline?.cancel()
        pillGeometryDeadline = nil
        pillGeometryGeneration += 1
        let generation = pillGeometryGeneration
        let requestID = state.dictation.requestID
        let newRequest = !pillHasResolvedRequest || pillResolvedRequestID != requestID
        let snapshot = pillDisplays.snapshot()
        let target = newRequest ? pillDisplayTarget : nil
        if let target {
            state.desktop.pillGeometryPending = true
            let system = pillDisplays
            // The reference target-window helper has a 700 ms deadline before cursor fallback.
            pillGeometryDeadline = clock.schedule(after: 0.7) { [weak self] in
                guard let self, self.pillGeometryGeneration == generation,
                      self.state.desktop.pillGeometryPending, self.state.dictation.requestID == requestID else { return }
                self.pillGeometryGeneration += 1
                self.pillGeometryTask?.cancel()
                self.applyPillGeometry(snapshot: self.pillDisplays.snapshot(), windows: [], size: size,
                                       currentFrame: currentFrame, requestID: requestID, newRequest: true)
            }
            pillGeometryTask = Task { [weak self] in
                let windows = await system.windowBounds(for: target)
                guard !Task.isCancelled, self?.pillGeometryGeneration == generation,
                      self?.state.dictation.requestID == requestID else { return }
                guard let self else { return }
                self.applyPillGeometry(snapshot: self.pillDisplays.snapshot(), windows: windows, size: size,
                                       currentFrame: currentFrame, requestID: requestID, newRequest: true)
            }
        } else {
            applyPillGeometry(snapshot: snapshot, windows: [], size: size, currentFrame: currentFrame,
                              requestID: requestID, newRequest: newRequest)
        }
    }

    private func applyPillGeometry(snapshot: PillDisplaySnapshot, windows: [CGRect], size: CGSize,
                                   currentFrame: CGRect?, requestID: UUID?, newRequest: Bool) {
        state.desktop.pillGeometryPending = false
        pillGeometryDeadline?.cancel()
        pillGeometryDeadline = nil
        guard let display = RecordingPillGeometry.display(in: snapshot, targetWindows: windows,
            preferredID: newRequest ? nil : state.desktop.pillDisplayID, currentFrame: newRequest ? nil : currentFrame) else {
            state.desktop.pillFrame = nil
            return
        }
        let placement = state.settings.desktop.panelStartPosition
        let preserve = state.desktop.pillDisplayID == display.id && pillGeometryPlacement == placement
        state.desktop.pillFrame = RecordingPillGeometry.frame(size: size, on: display, placement: placement,
                                                               previous: currentFrame, preservePosition: preserve)
        state.desktop.pillDisplayID = display.id
        pillGeometryPlacement = placement
        pillResolvedRequestID = requestID
        pillHasResolvedRequest = true
        pillGeometryTask = nil
    }
}
