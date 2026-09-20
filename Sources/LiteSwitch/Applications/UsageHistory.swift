import Foundation

struct ApplicationUsageRecord: Codable, Equatable, Sendable {
    var launchCount: Int
    var latestLaunchDate: Date
}

enum ApplicationIdentity {
    static func identities(for applications: [InstalledApplication]) -> [URL: String] {
        let bundleIdentifierCounts = Dictionary(
            grouping: applications.compactMap(\.bundleIdentifier),
            by: { $0 }
        ).mapValues(\.count)

        var identities: [URL: String] = [:]
        for application in applications {
            let canonicalURL = application.url.resolvingSymlinksInPath().standardizedFileURL
            if let bundleIdentifier = application.bundleIdentifier,
               bundleIdentifierCounts[bundleIdentifier] == 1 {
                identities[canonicalURL] = "bundle:\(bundleIdentifier)"
            } else {
                identities[canonicalURL] = "path:\(canonicalURL.path)"
            }
        }
        return identities
    }

    static func identity(
        for application: InstalledApplication,
        among applications: [InstalledApplication]
    ) -> String {
        identities(for: applications)[application.url.resolvingSymlinksInPath().standardizedFileURL]
            ?? "path:\(application.url.standardizedFileURL.path)"
    }
}

/// Stores one aggregate record per application. No individual launch events are retained.
@MainActor
final class UsageHistoryStore: ObservableObject {
    @Published private(set) var records: [String: ApplicationUsageRecord]

    private let defaults: UserDefaults
    private let storageKey: String

    init(
        defaults: UserDefaults = .standard,
        storageKey: String = "applicationUsageHistory"
    ) {
        self.defaults = defaults
        self.storageKey = storageKey
        if let data = defaults.data(forKey: storageKey),
           let decoded = try? JSONDecoder().decode([String: ApplicationUsageRecord].self, from: data) {
            records = decoded
        } else {
            records = [:]
        }
    }

    func recordLaunch(
        of application: InstalledApplication,
        among applications: [InstalledApplication],
        at date: Date = Date()
    ) {
        let identity = ApplicationIdentity.identity(for: application, among: applications)
        let previousCount = records[identity]?.launchCount ?? 0
        records[identity] = ApplicationUsageRecord(
            launchCount: previousCount + 1,
            latestLaunchDate: date
        )
        persist()
    }

    func retainRecords(for applications: [InstalledApplication]) {
        let validIdentities = Set(ApplicationIdentity.identities(for: applications).values)
        let retained = records.filter { validIdentities.contains($0.key) }
        guard retained != records else { return }
        records = retained
        persist()
    }

    func reset() {
        guard !records.isEmpty else { return }
        records = [:]
        persist()
    }

    private func persist() {
        if records.isEmpty {
            defaults.removeObject(forKey: storageKey)
        } else if let data = try? JSONEncoder().encode(records) {
            defaults.set(data, forKey: storageKey)
        }
    }
}
