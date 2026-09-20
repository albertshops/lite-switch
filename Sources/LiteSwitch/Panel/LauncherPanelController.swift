import AppKit
import LiteSwitchCore
import SwiftUI

@MainActor
final class LauncherPanelController: NSObject, NSWindowDelegate {
    private let panel: LauncherPanel
    private let model: LauncherViewModel
    private var outsideClickMonitor: Any?
    private var assignmentKeyMonitor: Any?

    init(
        applicationIndex: ApplicationIndex,
        usageHistory: UsageHistoryStore,
        windowService: WindowService,
        assignmentStore: AssignmentStore,
        assignmentsDidChange: @escaping () -> Void
    ) {
        model = LauncherViewModel(
            applicationIndex: applicationIndex,
            usageHistory: usageHistory,
            windowService: windowService,
            assignmentStore: assignmentStore,
            assignmentsDidChange: assignmentsDidChange
        )
        panel = LauncherPanel(
            contentRect: NSRect(x: 0, y: 0, width: 680, height: 560),
            styleMask: [.borderless, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        super.init()

        panel.delegate = self
        panel.dismissAction = { [weak self] in self?.hide() }
        model.didCompleteAction = { [weak self] in self?.hide() }
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.level = .floating
        panel.isReleasedWhenClosed = false
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient, .ignoresCycle]
        panel.contentView = NSHostingView(rootView: LauncherView(model: model))
        installAssignmentKeyMonitor()
    }

    deinit {
        if let assignmentKeyMonitor { NSEvent.removeMonitor(assignmentKeyMonitor) }
    }

    func toggle() { panel.isVisible ? hide() : show() }

    func show() {
        model.prepareForPresentation()
        positionPanel(on: activeScreen())
        installOutsideClickMonitor()
        NSApp.activate(ignoringOtherApps: true)
        panel.makeKeyAndOrderFront(nil)
    }

    func hide() {
        guard panel.isVisible else { return }
        model.cancelAssigning()
        panel.orderOut(nil)
        removeOutsideClickMonitor()
    }

    func windowDidResignKey(_ notification: Notification) { hide() }

    private func installAssignmentKeyMonitor() {
        assignmentKeyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self, self.model.assignmentItemID != nil else { return event }
            if event.keyCode == 53 { // Escape
                self.model.cancelAssigning()
                return nil
            }
            guard event.modifierFlags.intersection([.command, .control, .option]).isEmpty else {
                NSSound.beep()
                return nil
            }
            let key: ShortcutKey?
            switch event.keyCode {
            case 123: key = .leftArrow
            case 124: key = .rightArrow
            case 125: key = .downArrow
            case 126: key = .upArrow
            default:
                key = event.charactersIgnoringModifiers?.first.flatMap(ShortcutKey.init)
            }
            if key == nil { NSSound.beep() }
            _ = self.model.handleAssignmentKey(key)
            return nil
        }
    }

    private func activeScreen() -> NSScreen {
        let mouseLocation = NSEvent.mouseLocation
        return NSScreen.screens.first(where: { NSMouseInRect(mouseLocation, $0.frame, false) })
            ?? NSScreen.main ?? NSScreen.screens[0]
    }

    private func positionPanel(on screen: NSScreen) {
        let frame = screen.visibleFrame
        let size = panel.frame.size
        panel.setFrameOrigin(NSPoint(
            x: frame.midX - size.width / 2,
            y: frame.maxY - (frame.height * 0.30) - size.height / 2
        ))
    }

    private func installOutsideClickMonitor() {
        removeOutsideClickMonitor()
        outsideClickMonitor = NSEvent.addGlobalMonitorForEvents(
            matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown]
        ) { [weak self] _ in self?.hide() }
    }

    private func removeOutsideClickMonitor() {
        if let outsideClickMonitor {
            NSEvent.removeMonitor(outsideClickMonitor)
            self.outsideClickMonitor = nil
        }
    }
}
