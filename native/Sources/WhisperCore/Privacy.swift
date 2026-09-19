import Foundation

public enum PrivacyPermission: String, CaseIterable, Sendable { case microphone, accessibility }
public enum PrivacyAuthorization: String, Equatable, Sendable { case notDetermined, granted, denied, restricted, unavailable }
public struct PermissionSnapshot: Equatable, Sendable {
    public var microphone: PrivacyAuthorization
    public var accessibility: PrivacyAuthorization
    public init(microphone: PrivacyAuthorization = .unavailable, accessibility: PrivacyAuthorization = .unavailable) {
        self.microphone = microphone; self.accessibility = accessibility
    }
    public subscript(_ permission: PrivacyPermission) -> PrivacyAuthorization {
        permission == .microphone ? microphone : accessibility
    }
}
public struct AudioStorageUsage: Equatable, Sendable {
    public let files: Int
    public let bytes: Int64
    public init(files: Int = 0, bytes: Int64 = 0) { self.files = files; self.bytes = bytes }
}
public struct PrivacyState: Equatable, Sendable {
    public var permissions = PermissionSnapshot()
    public var requesting: PrivacyPermission?
    public var audioUsage: AudioStorageUsage?
    public var isLoadingStorage = false
    public var storageFailed = false
    public init() {}
}

@MainActor public protocol PrivacySystem {
    func snapshot() -> PermissionSnapshot
    func request(_ permission: PrivacyPermission) async
    func openSettings(_ permission: PrivacyPermission)
}
@MainActor public struct InertPrivacySystem: PrivacySystem {
    public init() {}
    public func snapshot() -> PermissionSnapshot { .init() }
    public func request(_ permission: PrivacyPermission) async {}
    public func openSettings(_ permission: PrivacyPermission) {}
}

extension WhisperApplication {
    func refreshPrivacy() {
        guard !state.isTerminating else { return }
        state.privacy.permissions = privacySystem.snapshot()
        privacyReadGeneration += 1
        let generation = privacyReadGeneration
        privacyReadTask?.cancel()
        state.privacy.isLoadingStorage = true
        state.privacy.storageFailed = false
        let writes = historyWriteTask
        privacyReadTask = workflowTasks.start { [weak self, historyStore] in
            _ = await writes?.value
            guard !Task.isCancelled else { return }
            do {
                let usage = try await historyStore.audioStorageUsage()
                guard !Task.isCancelled, self?.privacyReadGeneration == generation, self?.state.isTerminating == false else { return }
                self?.state.privacy.audioUsage = usage
                self?.state.privacy.isLoadingStorage = false
            } catch {
                guard !Task.isCancelled, self?.privacyReadGeneration == generation, self?.state.isTerminating == false else { return }
                self?.state.privacy.storageFailed = true
                self?.state.privacy.isLoadingStorage = false
            }
        }
    }

    func requestPrivacyPermission(_ permission: PrivacyPermission) {
        guard !state.isTerminating, state.privacy.requesting == nil else { return }
        state.privacy.requesting = permission
        let system = privacySystem
        Task { [weak self] in
            guard self?.state.isTerminating == false else { return }
            await system.request(permission)
            guard self?.state.isTerminating == false else { return }
            self?.state.privacy.permissions = system.snapshot()
            self?.state.privacy.requesting = nil
        }
    }
}
