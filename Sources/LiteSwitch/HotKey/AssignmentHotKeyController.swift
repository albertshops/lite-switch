import Carbon.HIToolbox
import Foundation
import LiteSwitchCore

private nonisolated(unsafe) let assignmentHotKeyHandler: EventHandlerUPP = { _, event, userData in
    guard let event, let userData else { return noErr }
    var hotKeyID = EventHotKeyID()
    let status = GetEventParameter(
        event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID),
        nil, MemoryLayout<EventHotKeyID>.size, nil, &hotKeyID
    )
    guard status == noErr else { return status }
    guard hotKeyID.signature == OSType(0x4C53_4153) else { return OSStatus(eventNotHandledErr) }
    let controller = Unmanaged<AssignmentHotKeyController>.fromOpaque(userData).takeUnretainedValue()
    MainActor.assumeIsolated { controller.handle(id: hotKeyID.id) }
    return noErr
}

@MainActor
final class AssignmentHotKeyController {
    private static let signature: OSType = 0x4C53_4153 // LSAS
    private struct Registration {
        let key: ShortcutKey
        let id: UInt32
        let reference: EventHotKeyRef
    }

    private var registrations: [UInt32: Registration] = [:]
    private var eventHandler: EventHandlerRef?
    private var activate: ((ShortcutKey) -> Void)?
    private var nextIdentifier = UInt32(100)

    init() {
        var eventType = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        InstallEventHandler(
            GetApplicationEventTarget(), assignmentHotKeyHandler, 1, &eventType,
            Unmanaged.passUnretained(self).toOpaque(), &eventHandler
        )
    }

    deinit {
        registrations.values.forEach { _ = UnregisterEventHotKey($0.reference) }
        if let eventHandler { RemoveEventHandler(eventHandler) }
    }

    func register(keys: [ShortcutKey], activate: @escaping (ShortcutKey) -> Void) {
        self.activate = activate

        let desired = Dictionary(uniqueKeysWithValues: keys.flatMap { key in
            Self.keyCodes[key, default: []].map { ($0, key) }
        })

        // Keep registrations whose physical key has not changed. Re-registering
        // every hot key after one assignment changes creates a brief gap where
        // Option-key presses reach the frontmost app as symbols. It can also
        // lose otherwise healthy registrations if Carbon reports a transient
        // failure while they are being recreated.
        for (keyCode, registration) in registrations
        where desired[keyCode] != registration.key {
            _ = UnregisterEventHotKey(registration.reference)
            registrations.removeValue(forKey: keyCode)
        }

        for (keyCode, key) in desired where registrations[keyCode] == nil {
            let id = nextIdentifier
            nextIdentifier &+= 1
            var reference: EventHotKeyRef?
            let hotKeyID = EventHotKeyID(signature: Self.signature, id: id)
            let status = RegisterEventHotKey(
                keyCode, UInt32(optionKey), hotKeyID, GetApplicationEventTarget(), 0, &reference
            )
            if status == noErr, let reference {
                registrations[keyCode] = Registration(key: key, id: id, reference: reference)
            }
        }
    }

    fileprivate func handle(id: UInt32) {
        guard let key = registrations.values.first(where: { $0.id == id })?.key else { return }
        activate?(key)
    }

    private static let keyCodes: [ShortcutKey: [UInt32]] = [
        ShortcutKey("a")!: [UInt32(kVK_ANSI_A)], ShortcutKey("b")!: [UInt32(kVK_ANSI_B)],
        ShortcutKey("c")!: [UInt32(kVK_ANSI_C)], ShortcutKey("d")!: [UInt32(kVK_ANSI_D)],
        ShortcutKey("e")!: [UInt32(kVK_ANSI_E)], ShortcutKey("f")!: [UInt32(kVK_ANSI_F)],
        ShortcutKey("g")!: [UInt32(kVK_ANSI_G)], ShortcutKey("h")!: [UInt32(kVK_ANSI_H)],
        ShortcutKey("i")!: [UInt32(kVK_ANSI_I)], ShortcutKey("j")!: [UInt32(kVK_ANSI_J)],
        ShortcutKey("k")!: [UInt32(kVK_ANSI_K)], ShortcutKey("l")!: [UInt32(kVK_ANSI_L)],
        ShortcutKey("m")!: [UInt32(kVK_ANSI_M)], ShortcutKey("n")!: [UInt32(kVK_ANSI_N)],
        ShortcutKey("o")!: [UInt32(kVK_ANSI_O)], ShortcutKey("p")!: [UInt32(kVK_ANSI_P)],
        ShortcutKey("q")!: [UInt32(kVK_ANSI_Q)], ShortcutKey("r")!: [UInt32(kVK_ANSI_R)],
        ShortcutKey("s")!: [UInt32(kVK_ANSI_S)], ShortcutKey("t")!: [UInt32(kVK_ANSI_T)],
        ShortcutKey("u")!: [UInt32(kVK_ANSI_U)], ShortcutKey("v")!: [UInt32(kVK_ANSI_V)],
        ShortcutKey("w")!: [UInt32(kVK_ANSI_W)], ShortcutKey("x")!: [UInt32(kVK_ANSI_X)],
        ShortcutKey("y")!: [UInt32(kVK_ANSI_Y)], ShortcutKey("z")!: [UInt32(kVK_ANSI_Z)],
        ShortcutKey("0")!: [UInt32(kVK_ANSI_0), UInt32(kVK_ANSI_Keypad0)],
        ShortcutKey("1")!: [UInt32(kVK_ANSI_1), UInt32(kVK_ANSI_Keypad1)],
        ShortcutKey("2")!: [UInt32(kVK_ANSI_2), UInt32(kVK_ANSI_Keypad2)],
        ShortcutKey("3")!: [UInt32(kVK_ANSI_3), UInt32(kVK_ANSI_Keypad3)],
        ShortcutKey("4")!: [UInt32(kVK_ANSI_4), UInt32(kVK_ANSI_Keypad4)],
        ShortcutKey("5")!: [UInt32(kVK_ANSI_5), UInt32(kVK_ANSI_Keypad5)],
        ShortcutKey("6")!: [UInt32(kVK_ANSI_6), UInt32(kVK_ANSI_Keypad6)],
        ShortcutKey("7")!: [UInt32(kVK_ANSI_7), UInt32(kVK_ANSI_Keypad7)],
        ShortcutKey("8")!: [UInt32(kVK_ANSI_8), UInt32(kVK_ANSI_Keypad8)],
        ShortcutKey("9")!: [UInt32(kVK_ANSI_9), UInt32(kVK_ANSI_Keypad9)],
        .leftArrow: [UInt32(kVK_LeftArrow)], .rightArrow: [UInt32(kVK_RightArrow)],
        .upArrow: [UInt32(kVK_UpArrow)], .downArrow: [UInt32(kVK_DownArrow)],
    ]
}
