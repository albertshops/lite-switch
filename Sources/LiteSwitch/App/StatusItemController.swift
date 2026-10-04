import AppKit

final class StatusItemController: NSObject, NSMenuDelegate {
    private let statusItem: NSStatusItem
    private let menu = NSMenu()
    private let audioOutputs = AudioOutputService()
    private let openLauncher: () -> Void
    private let openSettings: () -> Void
    private let openAccessibility: () -> Void
    private let clearAssignments: () -> Void
    private let restart: () -> Void
    private let quit: () -> Void

    init(
        openLauncher: @escaping () -> Void,
        openSettings: @escaping () -> Void,
        openAccessibility: @escaping () -> Void,
        clearAssignments: @escaping () -> Void,
        restart: @escaping () -> Void,
        quit: @escaping () -> Void
    ) {
        self.openLauncher = openLauncher
        self.openSettings = openSettings
        self.openAccessibility = openAccessibility
        self.clearAssignments = clearAssignments
        self.restart = restart
        self.quit = quit
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        super.init()

        statusItem.button?.image = NSImage(systemSymbolName: "rectangle.2.swap", accessibilityDescription: "Lite Switch")
        statusItem.button?.toolTip = "Lite Switch"
        menu.delegate = self
        statusItem.menu = menu
        rebuildMenu()
    }

    func menuWillOpen(_ menu: NSMenu) {
        rebuildMenu()
    }

    private func rebuildMenu() {
        menu.removeAllItems()

        addItem(title: "Open Lite Switch", action: #selector(openLauncherSelected))
        menu.addItem(.separator())

        let heading = NSMenuItem(title: "Sound Output", action: nil, keyEquivalent: "")
        heading.isEnabled = false
        menu.addItem(heading)

        let devices = audioOutputs.availableDevices()
        if devices.isEmpty {
            let unavailable = NSMenuItem(title: "No sound outputs found", action: nil, keyEquivalent: "")
            unavailable.isEnabled = false
            unavailable.indentationLevel = 1
            menu.addItem(unavailable)
        } else {
            for device in devices {
                let item = addItem(title: device.name, action: #selector(selectAudioOutput(_:)))
                item.indentationLevel = 1
                item.representedObject = NSNumber(value: device.id)
                item.state = device.isDefault ? .on : .off
            }
        }

        menu.addItem(.separator())
        addItem(title: "Settings…", action: #selector(openSettingsSelected), keyEquivalent: ",")
        addItem(title: "Open Accessibility Settings…", action: #selector(openAccessibilitySelected))
        addItem(title: "Clear Assigned Shortcuts", action: #selector(clearAssignmentsSelected))
        menu.addItem(.separator())
        addItem(title: "Restart Lite Switch", action: #selector(restartSelected))
        addItem(title: "Quit Lite Switch", action: #selector(quitSelected), keyEquivalent: "q")
    }

    @discardableResult
    private func addItem(title: String, action: Selector, keyEquivalent: String = "") -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: keyEquivalent)
        item.target = self
        menu.addItem(item)
        return item
    }

    @objc private func selectAudioOutput(_ sender: NSMenuItem) {
        guard let deviceID = (sender.representedObject as? NSNumber)?.uint32Value,
              audioOutputs.selectDevice(id: deviceID) else {
            NSSound.beep()
            return
        }
        rebuildMenu()
    }

    @objc private func openLauncherSelected() { openLauncher() }
    @objc private func openSettingsSelected() { openSettings() }
    @objc private func openAccessibilitySelected() { openAccessibility() }
    @objc private func clearAssignmentsSelected() { clearAssignments() }
    @objc private func restartSelected() { restart() }
    @objc private func quitSelected() { quit() }
}
