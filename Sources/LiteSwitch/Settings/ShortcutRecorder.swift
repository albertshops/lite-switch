import AppKit
import Carbon.HIToolbox
import SwiftUI

struct ShortcutRecorder: NSViewRepresentable {
    let shortcut: KeyboardShortcut
    let onRecord: (KeyboardShortcut) -> Void
    let onInvalid: () -> Void

    func makeNSView(context: Context) -> ShortcutRecorderView {
        let view = ShortcutRecorderView()
        view.onRecord = onRecord
        view.onInvalid = onInvalid
        view.shortcut = shortcut
        return view
    }

    func updateNSView(_ view: ShortcutRecorderView, context: Context) {
        view.onRecord = onRecord
        view.onInvalid = onInvalid
        view.shortcut = shortcut
    }
}

final class ShortcutRecorderView: NSView {
    var onRecord: ((KeyboardShortcut) -> Void)?
    var onInvalid: (() -> Void)?
    var shortcut: KeyboardShortcut = .default {
        didSet { needsDisplay = true; updateAccessibility() }
    }

    private var isRecording = false {
        didSet { needsDisplay = true; updateAccessibility() }
    }

    override var acceptsFirstResponder: Bool { true }
    override var intrinsicContentSize: NSSize { NSSize(width: 150, height: 30) }
    override var focusRingMaskBounds: NSRect { bounds.insetBy(dx: 1, dy: 1) }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        let rect = bounds.insetBy(dx: 1, dy: 1)
        let path = NSBezierPath(roundedRect: rect, xRadius: 6, yRadius: 6)
        (isRecording ? NSColor.controlAccentColor.withAlphaComponent(0.14) : NSColor.controlBackgroundColor).setFill()
        path.fill()
        NSColor.separatorColor.setStroke()
        path.stroke()

        let text = isRecording ? "Type shortcut…" : shortcut.displayName
        let attributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedSystemFont(ofSize: NSFont.systemFontSize, weight: .regular),
            .foregroundColor: NSColor.labelColor
        ]
        let size = text.size(withAttributes: attributes)
        text.draw(
            at: NSPoint(x: (bounds.width - size.width) / 2, y: (bounds.height - size.height) / 2),
            withAttributes: attributes
        )
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        beginRecording()
    }

    override func keyDown(with event: NSEvent) {
        if !isRecording {
            let hasModifier = !event.modifierFlags.intersection([.control, .option, .shift, .command]).isEmpty
            if hasModifier {
                record(event)
            } else if Int(event.keyCode) == kVK_Space || Int(event.keyCode) == kVK_Return {
                beginRecording()
            } else {
                NSSound.beep()
            }
            return
        }

        if Int(event.keyCode) == kVK_Escape {
            isRecording = false
            return
        }
        record(event)
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard isRecording else { return false }
        keyDown(with: event)
        return true
    }

    override func accessibilityPerformPress() -> Bool {
        window?.makeFirstResponder(self)
        beginRecording()
        return true
    }

    private func beginRecording() {
        isRecording = true
    }

    private func record(_ event: NSEvent) {
        isRecording = false
        guard let shortcut = KeyboardShortcut(event: event) else {
            onInvalid?()
            return
        }
        onRecord?(shortcut)
    }

    private func updateAccessibility() {
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        setAccessibilityLabel("Global keyboard shortcut")
        setAccessibilityValue(isRecording ? "Recording. Type a shortcut." : shortcut.accessibilityName)
        setAccessibilityHelp("Press to record a new shortcut. Include at least one modifier key.")
    }
}
