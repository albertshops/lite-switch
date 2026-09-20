import AppKit
import Combine
import LiteSwitchCore
import SwiftUI

@MainActor
final class LauncherViewModel: ObservableObject {
    struct Item: Identifiable {
        enum Kind {
            case application(InstalledApplication)
            case program(ManagedProgram)
            case window(ManagedWindow)
            case vivaldiTab(ManagedVivaldiTab)
            case action(WindowAction)
        }

        enum Section: String, CaseIterable {
            case applications = "Applications"
            case actions = "Window Actions"
        }

        let id: String
        let title: String
        let subtitle: String?
        let symbol: String?
        let kind: Kind
        let assignmentTarget: AssignmentTarget?
        let indentLevel: Int

        var section: Section {
            if case .action = kind { return .actions }
            return .applications
        }

    }

    @Published var query = "" { didSet { updateResults() } }
    @Published private(set) var results: [Item] = []
    @Published var selectedID: String?
    @Published var presentationID = UUID()
    @Published private(set) var isLoading = true
    @Published private(set) var errorMessage: String?
    @Published private(set) var assignmentItemID: String?
    @Published private(set) var expandedBundleIdentifiers: Set<String> = []
    @Published private(set) var expandedWindowIdentities: Set<WindowIdentity> = []
    @Published private(set) var faviconDataByTabID: [String: Data] = [:]

    var didCompleteAction: (() -> Void)?

    private var applications: [InstalledApplication] = []
    private var programs: [ManagedProgram] = []
    private var tabs: [ManagedVivaldiTab] = []
    private var actionWindow: ManagedWindow?
    private let applicationIndex: ApplicationIndex
    private let usageHistory: UsageHistoryStore
    private let windowService: WindowService
    private let assignmentStore: AssignmentStore
    private let assignmentsDidChange: () -> Void
    private var book: AssignmentBook
    private var subscriptions = Set<AnyCancellable>()
    private var switchingRefreshID = UUID()

    init(
        applicationIndex: ApplicationIndex,
        usageHistory: UsageHistoryStore,
        windowService: WindowService,
        assignmentStore: AssignmentStore,
        assignmentsDidChange: @escaping () -> Void
    ) {
        self.applicationIndex = applicationIndex
        self.usageHistory = usageHistory
        self.windowService = windowService
        self.assignmentStore = assignmentStore
        self.assignmentsDidChange = assignmentsDidChange
        book = assignmentStore.load()

        applicationIndex.$applications
            .sink { [weak self] in self?.replaceApplications(with: $0) }
            .store(in: &subscriptions)
        applicationIndex.$isLoading
            .sink { [weak self] loading in
                self?.isLoading = loading
                self?.updateResults()
            }
            .store(in: &subscriptions)
        usageHistory.$records
            .sink { [weak self] _ in self?.updateResults() }
            .store(in: &subscriptions)
    }

    func prepareForPresentation() {
        actionWindow = windowService.currentActiveWindow()
        query = ""
        errorMessage = nil
        assignmentItemID = nil
        expandedBundleIdentifiers.removeAll()
        expandedWindowIdentities.removeAll()
        book = assignmentStore.load()
        updateResults()
        refreshSwitchingTargets()
        presentationID = UUID()
    }

    func refreshSwitchingTargets() {
        let refreshID = UUID()
        switchingRefreshID = refreshID

        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let programs = WindowService().programs()
            DispatchQueue.main.async {
                guard let self, self.switchingRefreshID == refreshID else { return }
                self.programs = programs
                self.updateResults()
            }
        }

        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let tabs = WindowService().vivaldiTabs()
            DispatchQueue.main.async {
                guard let self, self.switchingRefreshID == refreshID else { return }
                self.tabs = tabs
                self.faviconDataByTabID = [:]
                self.updateResults()
            }

            let favicons = VivaldiFaviconStore().faviconData(for: tabs)
            DispatchQueue.main.async {
                guard let self, self.switchingRefreshID == refreshID else { return }
                self.faviconDataByTabID = favicons
            }
        }
    }

    func selectNext() { moveSelection(by: 1) }
    func selectPrevious() { moveSelection(by: -1) }

    func hasChildren(_ item: Item) -> Bool {
        switch item.kind {
        case let .program(program):
            program.windows.count > 1 || (
                program.identity.bundleIdentifier == WindowService.vivaldiBundleIdentifier
                    && program.windows.count == 1
                    && !tabs.isEmpty
            )
        case let .window(window):
            tabs(belongingTo: window, in: program(for: window)?.windows ?? []).count > 1
        case .application, .vivaldiTab, .action:
            false
        }
    }

    func isExpanded(_ item: Item) -> Bool {
        switch item.kind {
        case let .program(program):
            expandedBundleIdentifiers.contains(program.identity.bundleIdentifier)
        case let .window(window):
            expandedWindowIdentities.contains(window.identity)
        case .application, .vivaldiTab, .action:
            false
        }
    }

    func toggleChildren(of item: Item) {
        guard hasChildren(item) else { return }
        switch item.kind {
        case let .program(program):
            let identifier = program.identity.bundleIdentifier
            if expandedBundleIdentifiers.remove(identifier) == nil {
                expandedBundleIdentifiers.insert(identifier)
                if !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                   identifier == WindowService.vivaldiBundleIdentifier {
                    expandedWindowIdentities.formUnion(program.windows.map(\.identity))
                }
            } else {
                expandedWindowIdentities.subtract(program.windows.map(\.identity))
            }
        case let .window(window):
            if expandedWindowIdentities.remove(window.identity) == nil {
                expandedWindowIdentities.insert(window.identity)
            }
        case .application, .vivaldiTab, .action:
            return
        }
        selectedID = item.id
        updateResults()
    }

    func activateSelected() {
        guard let selectedID, let item = results.first(where: { $0.id == selectedID }) else { return }
        activate(item)
    }

    func activate(_ item: Item) {
        errorMessage = nil
        switch item.kind {
        case let .application(application):
            if let bundleIdentifier = application.bundleIdentifier,
               let program = programs.first(where: { $0.identity.bundleIdentifier == bundleIdentifier }) {
                let excluded = book.windowIdentities(for: program.identity)
                guard windowService.cycleWindows(in: program, excluding: excluded) else {
                    fail("No available windows for \(application.name).")
                    return
                }
                usageHistory.recordLaunch(of: application, among: applications)
                didCompleteAction?()
            } else {
                open(application)
            }
        case let .program(program):
            guard windowService.cycleWindows(in: program, excluding: book.windowIdentities(for: program.identity)) else {
                fail("No available windows for \(program.identity.name).")
                return
            }
            didCompleteAction?()
        case let .window(window):
            windowService.focus(window)
            didCompleteAction?()
        case let .vivaldiTab(tab):
            guard windowService.focusVivaldiTab(for: tab.identity) != nil else {
                fail("That Vivaldi tab is no longer available.")
                return
            }
            didCompleteAction?()
        case let .action(action):
            guard windowService.perform(action, on: actionWindow) else {
                fail("Lite Switch couldn’t change the previously active window.")
                return
            }
            didCompleteAction?()
        }
    }

    func beginAssigning(_ item: Item) {
        guard item.assignmentTarget != nil else { return }
        assignmentItemID = item.id
        selectedID = item.id
    }

    func cancelAssigning() { assignmentItemID = nil }

    func handleAssignmentKey(_ key: ShortcutKey?) -> Bool {
        guard let assignmentItemID else { return false }
        guard let item = results.first(where: { $0.id == assignmentItemID }),
              let target = item.assignmentTarget
        else {
            self.assignmentItemID = nil
            return true
        }
        guard let key else {
            self.assignmentItemID = nil
            return true
        }
        book.assign(key, to: target)
        if case let .program(program) = target {
            book.setLaunchIfNeeded(true, for: program)
        }
        assignmentStore.save(book)
        self.assignmentItemID = nil
        assignmentsDidChange()
        updateResults()
        return true
    }

    func removeAssignment(from item: Item) {
        guard let target = item.assignmentTarget else { return }
        book.remove(target: target)
        assignmentStore.save(book)
        assignmentsDidChange()
        updateResults()
    }

    func shortcut(for item: Item) -> ShortcutKey? {
        item.assignmentTarget.flatMap(book.key(for:))
    }

    func icon(for item: Item) -> NSImage? {
        switch item.kind {
        case let .application(application): application.icon
        case let .program(program): program.application.icon
        case let .window(window): window.application.icon
        case let .vivaldiTab(tab):
            if let data = faviconDataByTabID[tab.id], let favicon = NSImage(data: data) {
                favicon
            } else {
                NSWorkspace.shared.runningApplications.first {
                    $0.bundleIdentifier == WindowService.vivaldiBundleIdentifier
                }?.icon
            }
        case .action: nil
        }
    }

    private func replaceApplications(with applications: [InstalledApplication]) {
        self.applications = applications
        if !isLoading { usageHistory.retainRecords(for: applications) }
        updateResults()
    }

    private func updateResults() {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        let matchesQuery: (Item) -> Bool = { [self] item in
            trimmed.isEmpty || matches(trimmed, text: item.title + " " + (item.subtitle ?? ""))
        }
        let matchesNestedItem: (Item) -> Bool = { [self] item in
            guard !trimmed.isEmpty else { return true }
            switch item.kind {
            case let .window(window):
                return matches(trimmed, text: window.title)
            case let .vivaldiTab(tab):
                return matches(trimmed, text: tab.title + " " + tab.url)
            case .application, .program, .action:
                return matchesQuery(item)
            }
        }
        var applicationItems: [Item] = []
        var representedBundleIdentifiers = Set<String>()

        for program in programs {
            let parent = programItem(program)
            let parentMatches = matchesQuery(parent)
            var visibleDescendants: [Item] = []

            let flattensSingleVivaldiWindow =
                program.identity.bundleIdentifier == WindowService.vivaldiBundleIdentifier
                && program.windows.count == 1

            if flattensSingleVivaldiWindow, let window = program.windows.first {
                let tabRows = tabs(belongingTo: window, at: 0, in: program.windows).map {
                    tabItem($0, indentLevel: 1)
                }
                if trimmed.isEmpty {
                    if expandedBundleIdentifiers.contains(program.identity.bundleIdentifier) {
                        visibleDescendants.append(contentsOf: tabRows)
                    }
                } else if expandedBundleIdentifiers.contains(program.identity.bundleIdentifier) {
                    visibleDescendants.append(contentsOf: tabRows)
                } else {
                    visibleDescendants.append(contentsOf: tabRows.filter(matchesNestedItem))
                }
            } else {
                for (index, window) in program.windows.enumerated() {
                    if !trimmed.isEmpty, program.windows.count == 1, parentMatches {
                        continue
                    }
                    let windowRow = windowItem(window)
                    let windowTabs = program.identity.bundleIdentifier == WindowService.vivaldiBundleIdentifier
                        ? tabs(belongingTo: window, at: index, in: program.windows)
                        : []
                    let matchingTabs = windowTabs.map { tabItem($0) }.filter(matchesNestedItem)
                    let windowMatches = matchesNestedItem(windowRow)

                    if trimmed.isEmpty {
                        if expandedBundleIdentifiers.contains(program.identity.bundleIdentifier) {
                            visibleDescendants.append(windowRow)
                            if expandedWindowIdentities.contains(window.identity) {
                                visibleDescendants.append(contentsOf: windowTabs.map { tabItem($0) })
                            }
                        }
                    } else if expandedBundleIdentifiers.contains(program.identity.bundleIdentifier) {
                        visibleDescendants.append(windowRow)
                        if expandedWindowIdentities.contains(window.identity) {
                            visibleDescendants.append(contentsOf: windowTabs.map { tabItem($0) })
                        }
                    } else if windowMatches || !matchingTabs.isEmpty {
                        visibleDescendants.append(windowRow)
                        visibleDescendants.append(contentsOf: expandedWindowIdentities.contains(window.identity)
                            ? windowTabs.map { tabItem($0) }
                            : matchingTabs)
                    }
                }
            }

            if parentMatches || !visibleDescendants.isEmpty {
                applicationItems.append(parent)
                applicationItems.append(contentsOf: visibleDescendants)
            }
            representedBundleIdentifiers.insert(program.identity.bundleIdentifier)
        }

        let rankedApplications = ApplicationSearch.results(
            for: trimmed,
            in: applications,
            usageHistory: usageHistory.records
        ).filter { application in
            guard let bundleIdentifier = application.bundleIdentifier else { return true }
            return !representedBundleIdentifiers.contains(bundleIdentifier)
        }
        let applicationLimit = trimmed.isEmpty ? 8 : 20
        applicationItems.append(contentsOf: rankedApplications.prefix(applicationLimit).map(applicationItem))

        let actionItems = WindowAction.allCases.map(actionItem).filter(matchesQuery)
        results = applicationItems + actionItems
        if !results.contains(where: { $0.id == selectedID }) {
            selectedID = results.first?.id
        }
        errorMessage = nil
    }

    private func program(for window: ManagedWindow) -> ManagedProgram? {
        programs.first { program in
            program.windows.contains { $0.identity == window.identity }
        }
    }

    private func tabs(belongingTo window: ManagedWindow, in windows: [ManagedWindow]) -> [ManagedVivaldiTab] {
        guard window.identity.bundleIdentifier == WindowService.vivaldiBundleIdentifier,
              let index = windows.firstIndex(where: { $0.identity == window.identity })
        else { return [] }
        return tabs(belongingTo: window, at: index, in: windows)
    }

    private func tabs(
        belongingTo window: ManagedWindow,
        at index: Int,
        in windows: [ManagedWindow]
    ) -> [ManagedVivaldiTab] {
        let windowsWithTitle = windows.filter { $0.title == window.title }
        let tabWindowIDsWithTitle = Set(tabs.compactMap { tab -> String? in
            guard tab.windowTitle == window.title else { return nil }
            return tab.windowID
        })
        if windowsWithTitle.count == 1, tabWindowIDsWithTitle.count == 1 {
            return tabs.filter { $0.windowTitle == window.title }
        }
        return tabs.filter { $0.windowIndex == index + 1 }
    }

    private func applicationItem(_ application: InstalledApplication) -> Item {
        let target = application.bundleIdentifier.map {
            AssignmentTarget.program(ProgramIdentity(bundleIdentifier: $0, name: application.name))
        }
        return Item(
            id: "app:\(application.url.path)", title: application.name,
            subtitle: "Application", symbol: nil, kind: .application(application), assignmentTarget: target,
            indentLevel: 0
        )
    }

    private func programItem(_ program: ManagedProgram) -> Item {
        Item(
            id: "program:\(program.identity.bundleIdentifier)", title: program.identity.name,
            subtitle: "Running application · cycle windows", symbol: nil,
            kind: .program(program), assignmentTarget: .program(program.identity), indentLevel: 0
        )
    }

    private func windowItem(_ window: ManagedWindow) -> Item {
        Item(
            id: "window:\(window.identity.bundleIdentifier):\(window.identity.windowNumber ?? window.title.hashValue)",
            title: window.title,
            subtitle: "Window · \(window.application.localizedName ?? "Application")" + (window.isMinimized ? " · Minimized" : ""),
            symbol: nil, kind: .window(window), assignmentTarget: .window(window.identity), indentLevel: 1
        )
    }

    private func tabItem(_ tab: ManagedVivaldiTab, indentLevel: Int = 2) -> Item {
        Item(
            id: "tab:\(tab.id)", title: tab.title.isEmpty ? tab.url : tab.title,
            subtitle: "Vivaldi tab", symbol: nil, kind: .vivaldiTab(tab),
            assignmentTarget: .vivaldiTab(tab.identity), indentLevel: indentLevel
        )
    }

    private func actionItem(_ action: WindowAction) -> Item {
        Item(
            id: "action:\(action.rawValue)", title: action.title,
            subtitle: "Window action", symbol: action.symbolName,
            kind: .action(action), assignmentTarget: .action(action), indentLevel: 0
        )
    }

    private func open(_ application: InstalledApplication) {
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = true
        configuration.addsToRecentItems = true
        NSWorkspace.shared.openApplication(at: application.url, configuration: configuration) { [weak self] _, error in
            DispatchQueue.main.async {
                guard let self else { return }
                if let error {
                    self.fail("Couldn’t open \(application.name): \(error.localizedDescription)")
                } else {
                    self.usageHistory.recordLaunch(of: application, among: self.applications)
                    self.didCompleteAction?()
                }
            }
        }
    }

    private func fail(_ message: String) {
        errorMessage = message
        NSSound.beep()
    }

    private func moveSelection(by offset: Int) {
        guard !results.isEmpty else { return }
        guard let selectedID, let index = results.firstIndex(where: { $0.id == selectedID }) else {
            selectedID = results.first?.id
            return
        }
        self.selectedID = results[(index + offset + results.count) % results.count].id
    }

    private func matches(_ query: String, text: String) -> Bool {
        let query = query.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: .current)
        let text = text.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: .current)
        if text.contains(query) { return true }
        var index = text.startIndex
        for character in query {
            guard let found = text[index...].firstIndex(of: character) else { return false }
            index = text.index(after: found)
        }
        return true
    }
}

struct LauncherView: View {
    @ObservedObject var model: LauncherViewModel
    @FocusState private var searchIsFocused: Bool
    @State private var hoveredShortcutItemID: String?

    var body: some View {
        VStack(spacing: 10) {
            HStack(spacing: 12) {
                Image(systemName: "magnifyingglass").font(.system(size: 20, weight: .medium)).foregroundStyle(.secondary)
                TextField("Search apps, windows, tabs, and actions", text: $model.query)
                    .textFieldStyle(.plain).font(.system(size: 22)).focused($searchIsFocused)
                    .onSubmit { model.activateSelected() }
                Button { model.refreshSwitchingTargets() } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.plain).foregroundStyle(.secondary).help("Refresh windows and tabs")
            }
            .padding(.horizontal, 4).frame(height: 42)

            Divider()

            if let error = model.errorMessage {
                Label(error, systemImage: "exclamationmark.triangle").foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if model.isLoading && model.results.isEmpty {
                ProgressView("Finding applications…").frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if model.results.isEmpty {
                ContentUnavailableView.search(text: model.query)
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 2) {
                        ForEach(LauncherViewModel.Item.Section.allCases, id: \.self) { section in
                            let items = model.results.filter { $0.section == section }
                            if !items.isEmpty {
                                sectionHeader(section.rawValue)
                                ForEach(items) { item in resultRow(item).id(item.id) }
                            }
                        }
                    }
                }
            }

        }
        .padding(14).frame(width: 680, height: 560, alignment: .top)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        .overlay { RoundedRectangle(cornerRadius: 18).strokeBorder(.primary.opacity(0.14)) }
        .onKeyPress(.downArrow) { model.selectNext(); return .handled }
        .onKeyPress(.upArrow) { model.selectPrevious(); return .handled }
        .onKeyPress(.return) { model.activateSelected(); return .handled }
        .onChange(of: model.presentationID) { DispatchQueue.main.async { searchIsFocused = true } }
        .onAppear { searchIsFocused = true }
        .onExitCommand { NSApp.keyWindow?.cancelOperation(nil) }
    }

    private func sectionHeader(_ title: String) -> some View {
        Text(title.uppercased())
            .font(.caption2.weight(.semibold))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 9)
            .padding(.top, 8)
            .padding(.bottom, 3)
    }

    private func resultRow(_ item: LauncherViewModel.Item) -> some View {
        let selected = model.selectedID == item.id
        let shortcut = model.shortcut(for: item)
        let assigning = model.assignmentItemID == item.id
        return HStack(spacing: 11) {
            if item.indentLevel > 0 {
                Image(systemName: "arrow.turn.down.right")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
                    .frame(width: 12)
            }
            if let icon = model.icon(for: item) {
                Image(nsImage: icon).resizable().scaledToFit().frame(width: 32, height: 32)
            } else {
                Image(systemName: item.symbol ?? "bolt").frame(width: 32, height: 32).foregroundStyle(.secondary)
            }
            Text(item.title)
                .lineLimit(1)
            Spacer()
            if model.hasChildren(item) {
                Button { model.toggleChildren(of: item) } label: {
                    Image(systemName: model.isExpanded(item) ? "chevron.down" : "chevron.right")
                        .font(.caption.weight(.semibold))
                        .frame(width: 36, height: 40)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
                .help(model.isExpanded(item) ? "Hide windows and tabs" : "Show windows and tabs")
            }
            if item.assignmentTarget != nil {
                Button {
                    if shortcut != nil && !assigning { model.removeAssignment(from: item) }
                    else { model.beginAssigning(item) }
                } label: {
                    Text(assigning ? "…" : shortcut.map { "⌥\($0.displayValue)" } ?? "-")
                        .font(.system(.caption, design: .rounded).weight(.semibold))
                        .foregroundStyle(shortcut == nil && !assigning ? Color.secondary.opacity(0.4) : Color.primary)
                        .frame(minWidth: 36)
                        .padding(.horizontal, 6).padding(.vertical, 4)
                        .background(
                            hoveredShortcutItemID == item.id ? Color.primary.opacity(0.08) : .clear,
                            in: RoundedRectangle(cornerRadius: 6)
                        )
                }
                .buttonStyle(.plain)
                .onHover { hovering in
                    if hovering {
                        hoveredShortcutItemID = item.id
                    } else if hoveredShortcutItemID == item.id {
                        hoveredShortcutItemID = nil
                    }
                }
                .help(shortcut == nil ? "Assign an Option-key shortcut" : "Remove this shortcut")
            }
        }
        .padding(.leading, CGFloat(item.indentLevel) * 18)
        .padding(.horizontal, 9).frame(height: 48).contentShape(Rectangle())
        .background(selected ? Color.accentColor.opacity(0.18) : .clear, in: RoundedRectangle(cornerRadius: 8))
        .onTapGesture { model.selectedID = item.id; model.activate(item) }
        .onHover { if $0 { model.selectedID = item.id } }
    }
}

private extension WindowAction {
    var title: String {
        switch self {
        case .fillScreen: "Fill Screen"
        case .switchScreen: "Move to Next Screen"
        case .moveLeft: "Move to Left Half"
        case .moveRight: "Move to Right Half"
        case .moveTop: "Move to Top Half"
        case .moveBottom: "Move to Bottom Half"
        }
    }

    var symbolName: String {
        switch self {
        case .fillScreen: "rectangle.inset.filled"
        case .switchScreen: "rectangle.2.swap"
        case .moveLeft: "rectangle.lefthalf.inset.filled"
        case .moveRight: "rectangle.righthalf.inset.filled"
        case .moveTop: "rectangle.tophalf.inset.filled"
        case .moveBottom: "rectangle.bottomhalf.inset.filled"
        }
    }
}
