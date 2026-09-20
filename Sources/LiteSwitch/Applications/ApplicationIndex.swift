import AppKit
import CoreServices
import Foundation

/// Owns the searchable application snapshot and replaces it only after a complete
/// background discovery pass has finished.
@MainActor
final class ApplicationIndex: ObservableObject {
    @Published private(set) var applications: [InstalledApplication] = []
    @Published private(set) var isLoading = true

    private let buildIndex: () -> [InstalledApplication]
    private let workerQueue = DispatchQueue(label: "com.albertshops.liteswitch.application-index", qos: .userInitiated)
    private var isRefreshing = false
    private var refreshRequested = false
    private var changeObserver: ApplicationChangeObserver?

    // A deterministic event-to-refresh boundary used by tests and the FSEvents observer.
    func requestRefresh() {
        if isRefreshing {
            refreshRequested = true
            return
        }

        isRefreshing = true
        let buildIndex = buildIndex
        workerQueue.async { [weak self] in
            let applications = buildIndex()
            DispatchQueue.main.async {
                self?.finishRefresh(with: applications)
            }
        }
    }

    init(
        roots: [URL] = ApplicationDiscovery.defaultRoots,
        discovery: ApplicationDiscovery = ApplicationDiscovery(),
        observesChanges: Bool = true,
        startsImmediately: Bool = true
    ) {
        buildIndex = { discovery.discover(in: roots) }
        if observesChanges {
            changeObserver = ApplicationChangeObserver(roots: roots) { [weak self] in
                self?.requestRefresh()
            }
        }
        if startsImmediately {
            requestRefresh()
        }
    }

    init(
        observesChanges: Bool = false,
        startsImmediately: Bool = true,
        buildIndex: @escaping () -> [InstalledApplication]
    ) {
        self.buildIndex = buildIndex
        if startsImmediately {
            requestRefresh()
        }
    }

    private func finishRefresh(with discoveredApplications: [InstalledApplication]) {
        // Discovery already canonicalizes URLs. Normalizing again here keeps an injected
        // or future discovery provider from publishing duplicate intermediate entries.
        var uniqueApplications: [URL: InstalledApplication] = [:]
        for application in discoveredApplications {
            let canonicalURL = application.url.resolvingSymlinksInPath().standardizedFileURL
            uniqueApplications[canonicalURL] = InstalledApplication(
                url: canonicalURL,
                name: application.name,
                bundleIdentifier: application.bundleIdentifier
            )
        }

        applications = uniqueApplications.values.sorted {
            let comparison = $0.name.localizedCaseInsensitiveCompare($1.name)
            return comparison == .orderedSame ? $0.url.path < $1.url.path : comparison == .orderedAscending
        }
        isLoading = false
        isRefreshing = false

        if refreshRequested {
            refreshRequested = false
            requestRefresh()
        }
    }
}

/// Watches the application roots using macOS FSEvents. A burst of file operations
/// (such as copying an app bundle) is coalesced into one completed index rebuild.
private final class ApplicationChangeObserver {
    private let callback: () -> Void
    private var stream: FSEventStreamRef?
    private var pendingCallback: DispatchWorkItem?

    init(roots: [URL], callback: @escaping () -> Void) {
        self.callback = callback

        var context = FSEventStreamContext(
            version: 0,
            info: Unmanaged.passUnretained(self).toOpaque(),
            retain: nil,
            release: nil,
            copyDescription: nil
        )
        stream = FSEventStreamCreate(
            nil,
            { _, contextInfo, _, _, _, _ in
                guard let contextInfo else { return }
                Unmanaged<ApplicationChangeObserver>
                    .fromOpaque(contextInfo)
                    .takeUnretainedValue()
                    .applicationsDidChange()
            },
            &context,
            roots.map(\.path) as CFArray,
            FSEventStreamEventId(kFSEventStreamEventIdSinceNow),
            0.25,
            FSEventStreamCreateFlags(kFSEventStreamCreateFlagFileEvents | kFSEventStreamCreateFlagWatchRoot)
        )

        if let stream {
            FSEventStreamSetDispatchQueue(stream, DispatchQueue.global(qos: .utility))
            FSEventStreamStart(stream)
        }
    }

    deinit {
        pendingCallback?.cancel()
        if let stream {
            FSEventStreamStop(stream)
            FSEventStreamInvalidate(stream)
            FSEventStreamRelease(stream)
        }
    }

    private func applicationsDidChange() {
        let workItem = DispatchWorkItem { [weak self] in self?.callback() }
        DispatchQueue.main.async {
            self.pendingCallback?.cancel()
            self.pendingCallback = workItem
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.35, execute: workItem)
        }
    }
}
