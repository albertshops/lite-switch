import AppKit

final class LauncherPanel: NSPanel {
    var dismissAction: (() -> Void)?

    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }

    override func cancelOperation(_ sender: Any?) {
        dismissAction?()
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        let shortcutModifiers = event.modifierFlags.intersection([.control, .option, .shift, .command])
        if shortcutModifiers == .control,
           event.charactersIgnoringModifiers?.lowercased() == "c" {
            dismissAction?()
            return true
        }
        return super.performKeyEquivalent(with: event)
    }
}
