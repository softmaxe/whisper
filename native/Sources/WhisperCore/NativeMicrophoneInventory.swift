@preconcurrency import AVFoundation
import CoreAudio
import Foundation
import IOKit

/// Metadata-only snapshot. Enumerating inputs never opens or warms a microphone.
@MainActor enum NativeMicrophoneInventory {
    static func snapshot() -> MicrophoneSnapshot {
        let transports = audioTransports()
        let discovered = AVCaptureDevice.DiscoverySession(deviceTypes: [.microphone], mediaType: .audio, position: .unspecified).devices
        let devices = discovered.filter(\.isConnected).map { device in
            let transport = transports[device.uniqueID] ?? UInt32(bitPattern: device.transportType)
            let category: MicrophoneCategory
            switch transport {
            case kAudioDeviceTransportTypeBuiltIn: category = .builtIn
            case kAudioDeviceTransportTypeContinuityCaptureWired, kAudioDeviceTransportTypeContinuityCaptureWireless: category = .continuity
            default: category = device.isContinuityCamera ? .continuity : .external
            }
            return MicrophoneDevice(id: device.uniqueID, name: device.localizedName, category: category)
        }
        return MicrophoneSnapshot(devices: devices, systemDefaultID: defaultInputUID(), lidClosed: lidClosed())
    }

    // Reused from macos-mic-listener.swift and upstream's CoreAudio UID lookup.
    private static func stringProperty(_ id: AudioObjectID, selector: AudioObjectPropertySelector) -> String? {
        var address = AudioObjectPropertyAddress(mSelector: selector, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
        var value: CFString?
        var size = UInt32(MemoryLayout<CFString?>.size)
        let status = withUnsafeMutablePointer(to: &value) {
            AudioObjectGetPropertyData(id, &address, 0, nil, &size, $0)
        }
        return status == noErr ? value as String? : nil
    }

    private static func defaultInputUID() -> String? {
        var address = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyDefaultInputDevice, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
        var device = AudioDeviceID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioDeviceID>.size)
        guard AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &device) == noErr,
              device != kAudioObjectUnknown else { return nil }
        return stringProperty(device, selector: kAudioDevicePropertyDeviceUID)
    }

    private static func audioTransports() -> [String: UInt32] {
        var address = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyDevices, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
        var size: UInt32 = 0
        let system = AudioObjectID(kAudioObjectSystemObject)
        guard AudioObjectGetPropertyDataSize(system, &address, 0, nil, &size) == noErr,
              size >= MemoryLayout<AudioDeviceID>.size else { return [:] }
        var devices = [AudioDeviceID](repeating: 0, count: Int(size) / MemoryLayout<AudioDeviceID>.size)
        let status = devices.withUnsafeMutableBytes { AudioObjectGetPropertyData(system, &address, 0, nil, &size, $0.baseAddress!) }
        guard status == noErr else { return [:] }
        var result: [String: UInt32] = [:]
        for device in devices {
            guard let uid = stringProperty(device, selector: kAudioDevicePropertyDeviceUID) else { continue }
            var property = AudioObjectPropertyAddress(mSelector: kAudioDevicePropertyTransportType, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
            var transport: UInt32 = 0
            var transportSize = UInt32(MemoryLayout<UInt32>.size)
            if AudioObjectGetPropertyData(device, &property, 0, nil, &transportSize, &transport) == noErr { result[uid] = transport }
        }
        return result
    }

    private static func lidClosed() -> Bool? {
        let service = IOServiceGetMatchingService(kIOMainPortDefault, IOServiceMatching("IOPMrootDomain"))
        guard service != IO_OBJECT_NULL else { return nil }
        defer { IOObjectRelease(service) }
        guard let property = IORegistryEntryCreateCFProperty(service, "AppleClamshellState" as CFString, kCFAllocatorDefault, 0)?.takeRetainedValue(),
              CFGetTypeID(property) == CFBooleanGetTypeID() else { return nil }
        return (property as? NSNumber)?.boolValue
    }
}
