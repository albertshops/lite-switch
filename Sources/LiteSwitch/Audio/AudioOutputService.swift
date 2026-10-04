import CoreAudio
import Foundation

struct AudioOutputDevice {
    let id: UInt32
    let name: String
    let isDefault: Bool
}

final class AudioOutputService {
    private let systemObject = AudioObjectID(kAudioObjectSystemObject)

    func availableDevices() -> [AudioOutputDevice] {
        let currentDevice = defaultDeviceID()
        return deviceIDs()
            .filter(hasOutputStreams)
            .map { device in
                AudioOutputDevice(
                    id: device,
                    name: name(of: device),
                    isDefault: device == currentDevice
                )
            }
            .sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
    }

    func selectDevice(id: UInt32) -> Bool {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var selectedDevice = AudioDeviceID(id)
        let byteCount = UInt32(MemoryLayout<AudioDeviceID>.size)

        return AudioObjectSetPropertyData(
            systemObject,
            &address,
            0,
            nil,
            byteCount,
            &selectedDevice
        ) == noErr
    }

    private func deviceIDs() -> [AudioDeviceID] {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var byteCount: UInt32 = 0

        guard AudioObjectGetPropertyDataSize(systemObject, &address, 0, nil, &byteCount) == noErr else {
            return []
        }

        let count = Int(byteCount) / MemoryLayout<AudioDeviceID>.size
        var devices = Array(repeating: AudioDeviceID(0), count: count)
        guard AudioObjectGetPropertyData(systemObject, &address, 0, nil, &byteCount, &devices) == noErr else {
            return []
        }
        return devices
    }

    private func defaultDeviceID() -> AudioDeviceID? {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var device = AudioDeviceID(0)
        var byteCount = UInt32(MemoryLayout<AudioDeviceID>.size)

        guard AudioObjectGetPropertyData(systemObject, &address, 0, nil, &byteCount, &device) == noErr,
              device != kAudioObjectUnknown else {
            return nil
        }
        return device
    }

    private func name(of device: AudioDeviceID) -> String {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioObjectPropertyName,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var nameReference: Unmanaged<CFString>?
        var byteCount = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)

        guard AudioObjectGetPropertyData(device, &address, 0, nil, &byteCount, &nameReference) == noErr,
              let nameReference else {
            return "Unknown Output"
        }
        return nameReference.takeUnretainedValue() as String
    }

    private func hasOutputStreams(_ device: AudioDeviceID) -> Bool {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyStreams,
            mScope: kAudioDevicePropertyScopeOutput,
            mElement: kAudioObjectPropertyElementMain
        )
        var byteCount: UInt32 = 0

        return AudioObjectGetPropertyDataSize(device, &address, 0, nil, &byteCount) == noErr
            && byteCount > 0
    }
}
