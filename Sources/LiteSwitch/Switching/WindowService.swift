import AppKit
import ApplicationServices
import LiteSwitchCore

@_silgen_name("_AXUIElementGetWindow")
private func AXUIElementGetWindow(_ element: AXUIElement, _ windowID: UnsafeMutablePointer<CGWindowID>) -> AXError

struct ManagedWindow {
    let element: AXUIElement
    let application: NSRunningApplication
    let title: String
    let documentURL: String?
    let windowNumber: Int?
    let isMinimized: Bool

    var identity: WindowIdentity {
        WindowIdentity(
            bundleIdentifier: application.bundleIdentifier ?? "pid:\(application.processIdentifier)",
            title: title,
            documentURL: documentURL,
            windowNumber: windowNumber
        )
    }
}

struct ManagedProgram {
    let application: NSRunningApplication
    let windows: [ManagedWindow]

    var identity: ProgramIdentity {
        ProgramIdentity(
            bundleIdentifier: application.bundleIdentifier ?? "pid:\(application.processIdentifier)",
            name: application.localizedName ?? "Unknown Application"
        )
    }
}

struct ManagedVivaldiTab {
    let id: String
    let title: String
    let url: String
    let windowID: String?
    let windowTitle: String?
    let windowIndex: Int?

    var identity: VivaldiTabIdentity {
        VivaldiTabIdentity(id: id, title: title, url: url)
    }
}

final class WindowService {
    static let vivaldiBundleIdentifier = "com.vivaldi.Vivaldi"

    private var cycleOrders: [String: [AXUIElement]] = [:]

    func requestAccessibilityPermission() -> Bool {
        let options = ["AXTrustedCheckOptionPrompt": true] as CFDictionary
        return AXIsProcessTrustedWithOptions(options)
    }

    func isAccessibilityGranted() -> Bool {
        AXIsProcessTrusted()
    }

    func openAccessibilitySettings() {
        guard let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility") else { return }
        NSWorkspace.shared.open(url)
    }

    @discardableResult
    func perform(_ action: WindowAction, on targetWindow: ManagedWindow? = nil) -> Bool {
        guard let window = targetWindow?.element ?? activeWindow() else { return false }

        guard let accessibilityFrame = frame(of: window), !NSScreen.screens.isEmpty else { return false }
        let desktopTop = NSScreen.screens[0].frame.maxY
        let currentFrame = WindowGeometry.appKitFrame(
            fromAccessibilityFrame: accessibilityFrame,
            desktopTop: desktopTop
        )
        guard let sourceIndex = screenIndex(containing: currentFrame) else { return false }
        let source = NSScreen.screens[sourceIndex].visibleFrame
        let destination: CGRect?
        if action == .switchScreen {
            guard NSScreen.screens.count > 1 else { return false }
            destination = NSScreen.screens[(sourceIndex + 1) % NSScreen.screens.count].visibleFrame
        } else {
            destination = nil
        }
        guard let targetFrame = WindowGeometry.frame(
            for: action,
            currentFrame: currentFrame,
            sourceVisibleFrame: source,
            destinationVisibleFrame: destination
        ) else { return false }

        let target = WindowGeometry.accessibilityFrame(fromAppKitFrame: targetFrame, desktopTop: desktopTop)
        return setFrame(target, of: window)
    }

    func programs() -> [ManagedProgram] {
        guard isAccessibilityGranted() else { return [] }

        return NSWorkspace.shared.runningApplications
            .filter { $0.activationPolicy == .regular && !$0.isTerminated && $0.processIdentifier != ProcessInfo.processInfo.processIdentifier }
            .compactMap { application in
                let windows = windows(for: application)
                return windows.isEmpty ? nil : ManagedProgram(application: application, windows: windows)
            }
            .sorted { $0.identity.name.localizedCaseInsensitiveCompare($1.identity.name) == .orderedAscending }
    }

    func matchingProgram(for identity: ProgramIdentity) -> ManagedProgram? {
        let matches = programs().filter { identity.matches($0.identity) }
        return matches.count == 1 ? matches[0] : nil
    }

    func launch(_ program: ProgramIdentity) -> Bool {
        guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: program.bundleIdentifier) else {
            return false
        }
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = true
        NSWorkspace.shared.openApplication(at: url, configuration: configuration) { _, error in
            if let error {
                NSLog("Lite Switch: could not launch %@: %@", program.name, error.localizedDescription)
            }
        }
        return true
    }

    func matchingWindow(for identity: WindowIdentity) -> ManagedWindow? {
        let windows = programs().flatMap(\.windows)
        let exactMatches = windows.filter { identity.matches($0.identity) }
        if exactMatches.count == 1 { return exactMatches[0] }

        let persistedMatches = windows.filter { identity.matchesPersistedProperties($0.identity) }
        return persistedMatches.count == 1 ? persistedMatches[0] : nil
    }

    func vivaldiTabs() -> [ManagedVivaldiTab] {
        guard NSWorkspace.shared.runningApplications.contains(where: {
            $0.bundleIdentifier == Self.vivaldiBundleIdentifier && !$0.isTerminated
        }) else { return [] }

        let source = """
        tell application id "com.vivaldi.Vivaldi"
            set tabData to {}
            repeat with windowIndex from 1 to count of windows
                set browserWindow to window windowIndex
                repeat with browserTab in tabs of browserWindow
                    set end of tabData to {id of browserTab as text, title of browserTab, URL of browserTab, id of browserWindow as text, name of browserWindow, windowIndex}
                end repeat
            end repeat
            return tabData
        end tell
        """
        guard let result = executeAppleScript(source) else { return [] }
        guard result.numberOfItems > 0 else { return [] }

        return (1...result.numberOfItems).compactMap { index in
            result.atIndex(index).flatMap(managedVivaldiTab)
        }
    }

    func matchingVivaldiTab(for identity: VivaldiTabIdentity) -> ManagedVivaldiTab? {
        let tabs = vivaldiTabs()
        let exactMatches = tabs.filter { $0.id == identity.id }
        if exactMatches.count == 1 { return exactMatches[0] }

        guard !identity.url.isEmpty else { return nil }
        let urlMatches = tabs.filter { $0.url == identity.url }
        return urlMatches.count == 1 ? urlMatches[0] : nil
    }

    func focusVivaldiTab(for identity: VivaldiTabIdentity) -> ManagedVivaldiTab? {
        guard !identity.id.isEmpty, identity.id.allSatisfy(\.isNumber) else { return nil }
        let targetURL = appleScriptStringLiteral(identity.url)
        let source = """
        tell application id "com.vivaldi.Vivaldi"
            repeat with browserWindow in windows
                repeat with tabIndex from 1 to count of tabs of browserWindow
                    set browserTab to tab tabIndex of browserWindow
                    if (id of browserTab as text) is "\(identity.id)" then
                        set active tab index of browserWindow to tabIndex
                        set index of browserWindow to 1
                        activate
                        return {id of browserTab as text, title of browserTab, URL of browserTab}
                    end if
                end repeat
            end repeat

            set fallbackWindow to missing value
            set fallbackIndex to 0
            set fallbackCount to 0
            if "\(targetURL)" is not "" then
                repeat with browserWindow in windows
                    repeat with tabIndex from 1 to count of tabs of browserWindow
                        set browserTab to tab tabIndex of browserWindow
                        if (URL of browserTab) is "\(targetURL)" then
                            set fallbackWindow to browserWindow
                            set fallbackIndex to tabIndex
                            set fallbackCount to fallbackCount + 1
                        end if
                    end repeat
                end repeat
            end if

            if fallbackCount is 1 then
                set browserTab to tab fallbackIndex of fallbackWindow
                set active tab index of fallbackWindow to fallbackIndex
                set index of fallbackWindow to 1
                activate
                return {id of browserTab as text, title of browserTab, URL of browserTab}
            end if
            return {}
        end tell
        """
        return executeAppleScript(source).flatMap(managedVivaldiTab)
    }

    @discardableResult
    func cycleWindows(in program: ManagedProgram, excluding excluded: [WindowIdentity] = []) -> Bool {
        let candidates = program.windows.filter { window in
            !excluded.contains { $0.matches(window.identity) }
        }
        guard !candidates.isEmpty else { return false }
        let windows = windowsInCycleOrder(for: program, candidates: candidates)
        let appElement = AXUIElementCreateApplication(program.application.processIdentifier)
        let focusedWindow: AXUIElement? = attribute(kAXFocusedWindowAttribute, from: appElement)
        let focusedIndex = focusedWindow.flatMap { focused in
            windows.firstIndex { CFEqual($0.element, focused) }
        }
        let targetIndex: Int
        if program.application.isActive, let focusedIndex {
            targetIndex = (focusedIndex + 1) % windows.count
        } else {
            targetIndex = focusedIndex ?? 0
        }
        focus(windows[targetIndex])
        return true
    }

    private func windowsInCycleOrder(for program: ManagedProgram, candidates: [ManagedWindow]) -> [ManagedWindow] {
        let key = program.identity.bundleIdentifier
        var order = cycleOrders[key, default: []].filter { saved in
            candidates.contains { CFEqual($0.element, saved) }
        }
        for window in candidates where !order.contains(where: { CFEqual($0, window.element) }) {
            order.append(window.element)
        }
        cycleOrders[key] = order
        return order.compactMap { saved in
            candidates.first { CFEqual($0.element, saved) }
        }
    }

    func focus(_ window: ManagedWindow) {
        if window.isMinimized {
            AXUIElementSetAttributeValue(window.element, kAXMinimizedAttribute as CFString, kCFBooleanFalse)
        }
        window.application.activate()
        AXUIElementSetAttributeValue(
            AXUIElementCreateApplication(window.application.processIdentifier),
            kAXFocusedWindowAttribute as CFString,
            window.element
        )
        AXUIElementPerformAction(window.element, kAXRaiseAction as CFString)
    }

    private func windows(for application: NSRunningApplication) -> [ManagedWindow] {
        let appElement = AXUIElementCreateApplication(application.processIdentifier)
        guard let elements: [AXUIElement] = attribute(kAXWindowsAttribute, from: appElement) else { return [] }
        return elements.compactMap { managedWindow(for: $0, application: application) }
    }

    private func managedWindow(for element: AXUIElement, application: NSRunningApplication) -> ManagedWindow? {
        let role: String? = attribute(kAXRoleAttribute, from: element)
        let subrole: String? = attribute(kAXSubroleAttribute, from: element)
        guard role == kAXWindowRole,
              subrole == nil || subrole == kAXStandardWindowSubrole
        else { return nil }

        let rawTitle: String = attribute(kAXTitleAttribute, from: element) ?? ""
        let title = rawTitle.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "Untitled Window" : rawTitle
        let documentURL: String? = attribute(kAXDocumentAttribute, from: element)
        let windowNumber = windowNumber(for: element)
        let minimized: Bool = attribute(kAXMinimizedAttribute, from: element) ?? false
        return ManagedWindow(
            element: element,
            application: application,
            title: title,
            documentURL: documentURL,
            windowNumber: windowNumber,
            isMinimized: minimized
        )
    }

    func currentActiveWindow() -> ManagedWindow? {
        guard let application = NSWorkspace.shared.frontmostApplication,
              application.processIdentifier != ProcessInfo.processInfo.processIdentifier
        else { return nil }
        let appElement = AXUIElementCreateApplication(application.processIdentifier)
        guard let focused: AXUIElement = attribute(kAXFocusedWindowAttribute, from: appElement) else { return nil }
        return managedWindow(for: focused, application: application)
    }

    private func activeWindow() -> AXUIElement? {
        guard let application = NSWorkspace.shared.frontmostApplication,
              !application.isTerminated,
              application.processIdentifier != ProcessInfo.processInfo.processIdentifier
        else {
            NSLog("Lite Switch: no external frontmost application for window action")
            return nil
        }

        let applicationElement = AXUIElementCreateApplication(application.processIdentifier)
        let focused: (value: AXUIElement?, error: AXError) = attributeResult(
            kAXFocusedWindowAttribute,
            from: applicationElement
        )
        if let window = focused.value { return window }

        let main: (value: AXUIElement?, error: AXError) = attributeResult(
            kAXMainWindowAttribute,
            from: applicationElement
        )
        if let window = main.value { return window }

        NSLog(
            "Lite Switch: no active window for pid %d (focused AX error %d, main AX error %d)",
            application.processIdentifier,
            focused.error.rawValue,
            main.error.rawValue
        )
        return nil
    }

    private func frame(of element: AXUIElement) -> CGRect? {
        let positionResult: (value: AXValue?, error: AXError) = attributeResult(kAXPositionAttribute, from: element)
        let sizeResult: (value: AXValue?, error: AXError) = attributeResult(kAXSizeAttribute, from: element)
        guard let positionValue = positionResult.value, let sizeValue = sizeResult.value else {
            NSLog(
                "Lite Switch: could not read window frame (position AX error %d, size AX error %d)",
                positionResult.error.rawValue,
                sizeResult.error.rawValue
            )
            return nil
        }
        var position = CGPoint.zero
        var size = CGSize.zero
        guard AXValueGetValue(positionValue, .cgPoint, &position),
              AXValueGetValue(sizeValue, .cgSize, &size)
        else {
            NSLog("Lite Switch: window frame attributes had unexpected AX value types")
            return nil
        }
        return CGRect(origin: position, size: size)
    }

    private func setFrame(_ frame: CGRect, of element: AXUIElement) -> Bool {
        var position = frame.origin
        var size = frame.size
        guard let positionValue = AXValueCreate(.cgPoint, &position),
              let sizeValue = AXValueCreate(.cgSize, &size)
        else { return false }
        let positionStatus = AXUIElementSetAttributeValue(element, kAXPositionAttribute as CFString, positionValue)
        let sizeStatus = AXUIElementSetAttributeValue(element, kAXSizeAttribute as CFString, sizeValue)
        if sizeStatus == .success {
            _ = AXUIElementSetAttributeValue(element, kAXPositionAttribute as CFString, positionValue)
        }
        if positionStatus != .success || sizeStatus != .success {
            NSLog(
                "Lite Switch: could not set window frame (position AX error %d, size AX error %d)",
                positionStatus.rawValue,
                sizeStatus.rawValue
            )
        }
        return positionStatus == .success && sizeStatus == .success
    }

    private func screenIndex(containing windowFrame: CGRect) -> Int? {
        NSScreen.screens.indices.max { first, second in
            windowFrame.intersection(NSScreen.screens[first].frame).area
                < windowFrame.intersection(NSScreen.screens[second].frame).area
        }
    }

    private func attribute<T>(_ name: String, from element: AXUIElement) -> T? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else { return nil }
        return value as? T
    }

    private func executeAppleScript(_ source: String) -> NSAppleEventDescriptor? {
        var error: NSDictionary?
        let result = NSAppleScript(source: source)?.executeAndReturnError(&error)
        if let error {
            NSLog("Lite Switch: Vivaldi scripting failed: %@", error)
        }
        return result
    }

    private func managedVivaldiTab(_ descriptor: NSAppleEventDescriptor) -> ManagedVivaldiTab? {
        guard descriptor.numberOfItems == 3 || descriptor.numberOfItems == 6,
              let id = descriptor.atIndex(1)?.stringValue,
              let title = descriptor.atIndex(2)?.stringValue,
              let url = descriptor.atIndex(3)?.stringValue
        else { return nil }
        let windowID = descriptor.numberOfItems == 6 ? descriptor.atIndex(4)?.stringValue : nil
        let windowTitle = descriptor.numberOfItems == 6 ? descriptor.atIndex(5)?.stringValue : nil
        let windowIndex = descriptor.numberOfItems == 6 ? Int(descriptor.atIndex(6)?.int32Value ?? 0) : nil
        return ManagedVivaldiTab(
            id: id,
            title: title,
            url: url,
            windowID: windowID,
            windowTitle: windowTitle,
            windowIndex: windowIndex
        )
    }

    private func appleScriptStringLiteral(_ value: String) -> String {
        value.replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
    }

    private func attributeResult<T>(_ name: String, from element: AXUIElement) -> (value: T?, error: AXError) {
        var value: CFTypeRef?
        let error = AXUIElementCopyAttributeValue(element, name as CFString, &value)
        return (value as? T, error)
    }

    private func windowNumber(for element: AXUIElement) -> Int? {
        var windowID = CGWindowID(0)
        guard AXUIElementGetWindow(element, &windowID) == .success else { return nil }
        return Int(windowID)
    }
}

private extension CGRect {
    var area: CGFloat { isNull ? 0 : width * height }
}
