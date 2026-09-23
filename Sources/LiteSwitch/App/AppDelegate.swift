import AppKit
import LiteSwitchCore

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private static let openNotification = Notification.Name("com.albertshops.liteswitch.open-launcher")

    private let windowService = WindowService()
    private let assignmentStore = AssignmentStore()
    private let assignmentHotKeys = AssignmentHotKeyController()
    private var launcherHotKey: GlobalHotKeyController?
    private var applicationIndex: ApplicationIndex?
    private var usageHistory: UsageHistoryStore?
    private var panelController: LauncherPanelController?
    private var settingsController: SettingsWindowController?
    private var statusItemController: StatusItemController?
    private var isSecondaryInstance = false

    func applicationWillFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
        guard let bundleIdentifier = Bundle.main.bundleIdentifier else { return }
        if NSRunningApplication.runningApplications(withBundleIdentifier: bundleIdentifier)
            .contains(where: { $0.processIdentifier != ProcessInfo.processInfo.processIdentifier }) {
            isSecondaryInstance = true
            DistributedNotificationCenter.default().postNotificationName(
                Self.openNotification, object: bundleIdentifier, deliverImmediately: true
            )
            NSApp.terminate(nil)
        }
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        guard !isSecondaryInstance else { NSApp.terminate(nil); return }

        let applicationIndex = ApplicationIndex()
        let usageHistory = UsageHistoryStore()
        self.applicationIndex = applicationIndex
        self.usageHistory = usageHistory

        let launcherHotKey = GlobalHotKeyController { [weak self] in self?.panelController?.toggle() }
        self.launcherHotKey = launcherHotKey

        let panelController = LauncherPanelController(
            applicationIndex: applicationIndex,
            usageHistory: usageHistory,
            windowService: windowService,
            assignmentStore: assignmentStore,
            assignmentsDidChange: { [weak self] in self?.registerAssignmentHotKeys() }
        )
        self.panelController = panelController

        settingsController = SettingsWindowController(model: SettingsModel(
            hotKeyController: launcherHotKey,
            usageHistory: usageHistory
        ))
        statusItemController = StatusItemController(
            openLauncher: { [weak panelController] in panelController?.show() },
            openSettings: { [weak self] in self?.settingsController?.show() },
            openAccessibility: { [weak self] in self?.windowService.openAccessibilitySettings() },
            clearAssignments: { [weak self] in self?.clearAssignments() },
            restart: { [weak self] in self?.restart() },
            quit: { NSApp.terminate(nil) }
        )
        registerAssignmentHotKeys()

        DistributedNotificationCenter.default().addObserver(
            self, selector: #selector(openLauncherFromAnotherInstance),
            name: Self.openNotification, object: Bundle.main.bundleIdentifier
        )

        let defaults = UserDefaults.standard
        if !defaults.bool(forKey: "hasCompletedFirstLaunch") {
            defaults.set(true, forKey: "hasCompletedFirstLaunch")
            panelController.show()
        }
        if !windowService.isAccessibilityGranted() {
            _ = windowService.requestAccessibilityPermission()
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        DistributedNotificationCenter.default().removeObserver(self)
    }

    private func registerAssignmentHotKeys() {
        let keys = assignmentStore.load().assignments.keys.compactMap(ShortcutKey.init(rawValue:))
        assignmentHotKeys.register(keys: keys) { [weak self] in self?.activateAssignment(key: $0) }
    }

    private func activateAssignment(key: ShortcutKey) {
        var book = assignmentStore.load()
        guard let target = book.target(for: key) else { NSSound.beep(); return }
        switch target {
        case let .program(identity):
            guard let program = windowService.matchingProgram(for: identity) else {
                if book.shouldLaunchIfNeeded(identity), windowService.launch(identity) { return }
                NSSound.beep(); return
            }
            if !windowService.cycleWindows(in: program, excluding: book.windowIdentities(for: identity)) {
                NSSound.beep()
            }
        case let .window(identity):
            guard let window = windowService.matchingWindow(for: identity) else { NSSound.beep(); return }
            windowService.focus(window)
        case let .vivaldiTab(identity):
            guard let tab = windowService.focusVivaldiTab(for: identity) else { NSSound.beep(); return }
            if tab.identity != identity {
                book.assign(key, to: .vivaldiTab(tab.identity))
                assignmentStore.save(book)
            }
        case let .action(action):
            if !windowService.perform(action) { NSSound.beep() }
        }
    }

    private func clearAssignments() {
        assignmentStore.save(AssignmentBook())
        registerAssignmentHotKeys()
    }

    private func restart() {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/sh")
        process.arguments = [
            "-c",
            "while /bin/kill -0 \"$1\" 2>/dev/null; do /bin/sleep 0.1; done; /usr/bin/open \"$2\"",
            "lite-switch-restart",
            String(ProcessInfo.processInfo.processIdentifier),
            Bundle.main.bundleURL.path,
        ]

        do {
            try process.run()
            NSApp.terminate(nil)
        } catch {
            NSSound.beep()
        }
    }

    @objc private func openLauncherFromAnotherInstance(_ notification: Notification) {
        panelController?.show()
    }
}
