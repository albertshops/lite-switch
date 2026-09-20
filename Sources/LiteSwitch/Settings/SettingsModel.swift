import Foundation
import ServiceManagement

@MainActor
final class SettingsModel: ObservableObject {
    @Published private(set) var shortcut: KeyboardShortcut
    @Published private(set) var shortcutError: String?
    @Published private(set) var launchAtLoginEnabled = false
    @Published private(set) var launchAtLoginMessage: String?
    @Published private(set) var usageHistoryMessage: String?

    private let hotKeyController: GlobalHotKeyController
    private let loginItem: SMAppService
    private let usageHistory: UsageHistoryStore

    init(
        hotKeyController: GlobalHotKeyController,
        usageHistory: UsageHistoryStore,
        loginItem: SMAppService = .mainApp
    ) {
        self.hotKeyController = hotKeyController
        self.loginItem = loginItem
        self.usageHistory = usageHistory
        shortcut = hotKeyController.activeShortcut
        refreshLaunchAtLoginStatus()
    }

    func setShortcut(_ candidate: KeyboardShortcut) {
        switch hotKeyController.replaceShortcut(with: candidate) {
        case .success:
            shortcut = candidate
            shortcutError = nil
        case .failure(let error):
            shortcutError = error.localizedDescription
        }
    }

    func rejectShortcutWithoutModifiers() {
        shortcutError = "Include at least one modifier key, such as Option or Command. Your previous shortcut is still active."
    }

    func setLaunchAtLogin(_ enabled: Bool) {
        do {
            if enabled {
                if loginItem.status != .requiresApproval {
                    try loginItem.register()
                }
            } else {
                try loginItem.unregister()
            }
            refreshLaunchAtLoginStatus()
        } catch {
            refreshLaunchAtLoginStatus()
            launchAtLoginMessage = "Couldn’t \(enabled ? "enable" : "disable") Launch at Login: \(error.localizedDescription)"
        }
    }

    func resetUsageHistory() {
        usageHistory.reset()
        usageHistoryMessage = "Usage history was reset."
    }

    func refreshLaunchAtLoginStatus() {
        let status = loginItem.status
        launchAtLoginEnabled = status == .enabled
        switch status {
        case .requiresApproval:
            launchAtLoginMessage = "Allow Lite Switch in System Settings › General › Login Items to enable Launch at Login."
        case .notFound:
            launchAtLoginMessage = "Launch at Login is unavailable for this copy of Lite Switch."
        default:
            launchAtLoginMessage = nil
        }
    }
}
