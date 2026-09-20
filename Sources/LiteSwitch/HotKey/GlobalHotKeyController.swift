import Carbon.HIToolbox
import Foundation

@MainActor
final class GlobalHotKeyController {
    static let defaultsKey = "globalKeyboardShortcut"

    private var hotKey: EventHotKeyRef?
    private var eventHandler: EventHandlerRef?
    private var nextIdentifier: UInt32 = 1
    private let defaults: UserDefaults
    private let action: () -> Void

    private(set) var activeShortcut: KeyboardShortcut = .default

    init(defaults: UserDefaults = .standard, action: @escaping () -> Void) {
        self.defaults = defaults
        self.action = action
        installEventHandler()

        let persisted = defaults.data(forKey: Self.defaultsKey)
            .flatMap { try? JSONDecoder().decode(KeyboardShortcut.self, from: $0) }
        if !registerInitial(persisted ?? .default), persisted != nil {
            _ = registerInitial(.default)
            persist(.default)
        }
    }

    deinit {
        if let hotKey { UnregisterEventHotKey(hotKey) }
        if let eventHandler { RemoveEventHandler(eventHandler) }
    }

    /// Registers the candidate before removing the current hot key, so a failed
    /// replacement can never disable the user's working shortcut.
    func replaceShortcut(with candidate: KeyboardShortcut) -> Result<Void, HotKeyRegistrationError> {
        guard candidate != activeShortcut else { return .success(()) }

        var replacement: EventHotKeyRef?
        let status = register(candidate, reference: &replacement)
        guard status == noErr, let replacement else {
            return .failure(HotKeyRegistrationError(status: status))
        }

        if let hotKey { UnregisterEventHotKey(hotKey) }
        hotKey = replacement
        activeShortcut = candidate
        persist(candidate)
        return .success(())
    }

    private func registerInitial(_ shortcut: KeyboardShortcut) -> Bool {
        var reference: EventHotKeyRef?
        let status = register(shortcut, reference: &reference)
        guard status == noErr, let reference else { return false }
        hotKey = reference
        activeShortcut = shortcut
        return true
    }

    private func register(_ shortcut: KeyboardShortcut, reference: inout EventHotKeyRef?) -> OSStatus {
        defer { nextIdentifier &+= 1 }
        let identifier = EventHotKeyID(signature: OSType(0x504E5054), id: nextIdentifier) // "PNPT"
        return RegisterEventHotKey(
            shortcut.keyCode,
            shortcut.modifiers,
            identifier,
            GetApplicationEventTarget(),
            0,
            &reference
        )
    }

    private func persist(_ shortcut: KeyboardShortcut) {
        guard let data = try? JSONEncoder().encode(shortcut) else { return }
        defaults.set(data, forKey: Self.defaultsKey)
    }

    private func installEventHandler() {
        var eventType = EventTypeSpec(
            eventClass: OSType(kEventClassKeyboard),
            eventKind: UInt32(kEventHotKeyPressed)
        )
        let pointer = Unmanaged.passUnretained(self).toOpaque()
        InstallEventHandler(
            GetApplicationEventTarget(),
            { _, event, userData in
                guard let event, let userData else { return OSStatus(eventNotHandledErr) }
                var hotKeyID = EventHotKeyID()
                let status = GetEventParameter(
                    event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID),
                    nil, MemoryLayout<EventHotKeyID>.size, nil, &hotKeyID
                )
                guard status == noErr, hotKeyID.signature == OSType(0x504E5054) else {
                    return OSStatus(eventNotHandledErr)
                }
                let controller = Unmanaged<GlobalHotKeyController>
                    .fromOpaque(userData)
                    .takeUnretainedValue()
                DispatchQueue.main.async { controller.action() }
                return noErr
            },
            1,
            &eventType,
            pointer,
            &eventHandler
        )
    }
}

struct HotKeyRegistrationError: LocalizedError {
    let status: OSStatus

    var errorDescription: String? {
        if status == OSStatus(eventHotKeyExistsErr) {
            return "That shortcut is already in use. Your previous shortcut is still active."
        }
        return "Lite Switch couldn’t register that shortcut (error \(status)). Your previous shortcut is still active."
    }
}
