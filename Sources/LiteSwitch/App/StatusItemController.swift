import AppKit

final class StatusItemController: NSObject {
    private let statusItem: NSStatusItem
    private let openLauncher: () -> Void
    private let openSettings: () -> Void
    private let openAccessibility: () -> Void
    private let clearAssignments: () -> Void
    private let quit: () -> Void

    init(
        openLauncher: @escaping () -> Void,
        openSettings: @escaping () -> Void,
        openAccessibility: @escaping () -> Void,
        clearAssignments: @escaping () -> Void,
        quit: @escaping () -> Void
    ) {
        self.openLauncher = openLauncher
        self.openSettings = openSettings
        self.openAccessibility = openAccessibility
        self.clearAssignments = clearAssignments
        self.quit = quit
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        super.init()

        statusItem.button?.image = NSImage(systemSymbolName: "rectangle.2.swap", accessibilityDescription: "Lite Switch")
        statusItem.button?.toolTip = "Lite Switch"
        let menu = NSMenu()
        menu.addItem(withTitle: "Open Lite Switch", action: #selector(openLauncherSelected), keyEquivalent: "")
        menu.addItem(withTitle: "Settings…", action: #selector(openSettingsSelected), keyEquivalent: ",")
        menu.addItem(.separator())
        menu.addItem(withTitle: "Open Accessibility Settings…", action: #selector(openAccessibilitySelected), keyEquivalent: "")
        menu.addItem(withTitle: "Clear Assigned Shortcuts", action: #selector(clearAssignmentsSelected), keyEquivalent: "")
        menu.addItem(.separator())
        menu.addItem(withTitle: "Quit Lite Switch", action: #selector(quitSelected), keyEquivalent: "q")
        menu.items.forEach { $0.target = self }
        statusItem.menu = menu
    }

    @objc private func openLauncherSelected() { openLauncher() }
    @objc private func openSettingsSelected() { openSettings() }
    @objc private func openAccessibilitySelected() { openAccessibility() }
    @objc private func clearAssignmentsSelected() { clearAssignments() }
    @objc private func quitSelected() { quit() }
}
