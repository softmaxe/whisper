import AVFoundation
import AppKit
import Foundation

public enum HistoryAudioFailure: Error, Equatable, Sendable {
    case missing, save, playback, retry, removed
    public func message(in language: AppLanguage) -> String {
        switch self {
        case .missing: language.text("The saved audio is no longer available.", "保存的音频已不可用。")
        case .save: language.text("Audio could not be retained. Your transcript is still available.", "无法保留音频。转录文本仍然可用。")
        case .playback: language.text("The saved audio could not be played.", "无法播放保存的音频。")
        case .retry: language.text("Retry failed. The previous History entry has been preserved.", "重试失败。已保留之前的历史记录。")
        case .removed: language.text("This recording was deleted or expired before retry completed.", "重试完成前，此录音已被删除或过期。")
        }
    }
}

public struct HistoryRetryState: Equatable, Sendable {
    public var entryID: UUID?
    public var isRunning = false
    public var failure: HistoryAudioFailure?
    public var cleanupFailure: CleanupFailure?
    public var chineseConversionFailed = false
    public init() {}
}

/// Playback and Finder are external effects; tests substitute them without opening system UI.
@MainActor public protocol HistoryAudioSystem: AnyObject {
    func play(_ url: URL, completion: @escaping @MainActor @Sendable () -> Void) -> Bool
    func stop()
    func reveal(_ url: URL)
}

@MainActor public final class NativeHistoryAudioSystem: NSObject, HistoryAudioSystem, AVAudioPlayerDelegate {
    private var player: AVAudioPlayer?
    private var completion: (@MainActor @Sendable () -> Void)?
    public override init() { super.init() }
    public func play(_ url: URL, completion: @escaping @MainActor @Sendable () -> Void) -> Bool {
        stop()
        do {
            let player = try AVAudioPlayer(contentsOf: url)
            player.delegate = self
            guard player.play() else { return false }
            self.player = player
            self.completion = completion
            return true
        } catch { return false }
    }
    public func stop() { player?.stop(); player = nil; completion = nil }
    public func reveal(_ url: URL) { NSWorkspace.shared.activateFileViewerSelecting([url]) }
    nonisolated public func audioPlayerDidFinishPlaying(_ player: AVAudioPlayer, successfully flag: Bool) {
        finishPlayback(ObjectIdentifier(player))
    }
    nonisolated public func audioPlayerDecodeErrorDidOccur(_ player: AVAudioPlayer, error: (any Error)?) {
        finishPlayback(ObjectIdentifier(player))
    }
    private nonisolated func finishPlayback(_ finished: ObjectIdentifier) {
        Task { @MainActor [weak self] in
            guard self?.player.map(ObjectIdentifier.init) == finished else { return }
            let complete = self?.completion
            self?.stop()
            complete?()
        }
    }
}

/// Cancellation is visible to the storage actor even while MainActor starts another request.
final class HistoryRetryOwnership: @unchecked Sendable {
    private let lock = NSLock()
    private var active = true
    var isActive: Bool { lock.withLock { active } }
    func cancel() { lock.withLock { active = false } }
}

struct HistoryRetryInput: Sendable {
    let entry: HistoryEntry
    let audio: CapturedAudio
}

struct HistoryRecordingSave: Sendable {
    let entry: HistoryEntry?
    let audioFailed: Bool
}

public struct HistoryRetentionResult: Sendable {
    public let transcriptIDs: [UUID]
    public let transcriptCutoff: Date?
    public let audioIDs: [UUID]
}
