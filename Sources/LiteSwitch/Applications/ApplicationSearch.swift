import Foundation

enum ApplicationSearch {
    static func results(
        for query: String,
        in applications: [InstalledApplication],
        usageHistory: [String: ApplicationUsageRecord] = [:],
        now: Date = Date()
    ) -> [InstalledApplication] {
        let identities = ApplicationIdentity.identities(for: applications)
        let query = normalized(query)
        guard !query.isEmpty else {
            return applications.sorted {
                usageOrder($0, $1, identities: identities, history: usageHistory, now: now)
            }
        }

        return applications.compactMap { application -> RankedMatch? in
            guard let match = match(query: query, candidate: normalized(application.name)) else { return nil }
            let identity = identities[application.url.resolvingSymlinksInPath().standardizedFileURL]
            return RankedMatch(
                application: application,
                match: match,
                usageScore: identity.flatMap { usageHistory[$0] }.map { usageScore(for: $0, now: now) } ?? 0
            )
        }
        .sorted {
            // Match tiers are deliberately absolute: history can never promote a
            // fuzzy result above an exact, prefix, or word-prefix result.
            if $0.match.tier != $1.match.tier { return $0.match.tier < $1.match.tier }
            if $0.match.gap != $1.match.gap { return $0.match.gap < $1.match.gap }
            if $0.usageScore != $1.usageScore { return $0.usageScore > $1.usageScore }
            if $0.match.length != $1.match.length { return $0.match.length < $1.match.length }
            return alphabeticalOrder($0.application, $1.application)
        }
        .map(\.application)
    }

    private struct RankedMatch {
        let application: InstalledApplication
        let match: Match
        let usageScore: Double
    }

    private struct Match {
        let tier: Int
        let gap: Int
        let length: Int
    }

    private static func match(query: String, candidate: String) -> Match? {
        if candidate == query {
            return Match(tier: 0, gap: 0, length: candidate.count)
        }
        if candidate.hasPrefix(query) {
            return Match(tier: 1, gap: 0, length: candidate.count)
        }
        if candidate.split(whereSeparator: { !$0.isLetter && !$0.isNumber })
            .contains(where: { $0.hasPrefix(query) }) {
            return Match(tier: 2, gap: 0, length: candidate.count)
        }

        var candidateIndex = candidate.startIndex
        var firstMatch: String.Index?
        var lastMatch: String.Index?
        for character in query {
            guard let matchIndex = candidate[candidateIndex...].firstIndex(of: character) else { return nil }
            firstMatch = firstMatch ?? matchIndex
            lastMatch = matchIndex
            candidateIndex = candidate.index(after: matchIndex)
        }

        let span = candidate.distance(from: firstMatch!, through: lastMatch!)
        return Match(tier: 3, gap: span - query.count, length: candidate.count)
    }

    private static func usageOrder(
        _ lhs: InstalledApplication,
        _ rhs: InstalledApplication,
        identities: [URL: String],
        history: [String: ApplicationUsageRecord],
        now: Date
    ) -> Bool {
        func score(_ application: InstalledApplication) -> Double {
            let url = application.url.resolvingSymlinksInPath().standardizedFileURL
            guard let identity = identities[url], let record = history[identity] else { return 0 }
            return usageScore(for: record, now: now)
        }

        let lhsScore = score(lhs)
        let rhsScore = score(rhs)
        return lhsScore == rhsScore ? alphabeticalOrder(lhs, rhs) : lhsScore > rhsScore
    }

    private static func usageScore(for record: ApplicationUsageRecord, now: Date) -> Double {
        let ageInDays = max(0, now.timeIntervalSince(record.latestLaunchDate)) / 86_400
        let frequency = log1p(Double(max(0, record.launchCount))) * 4
        let recency = exp(-ageInDays / 30) * 8
        return frequency + recency
    }

    private static func normalized(_ value: String) -> String {
        value.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: .current)
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static func alphabeticalOrder(_ lhs: InstalledApplication, _ rhs: InstalledApplication) -> Bool {
        let comparison = lhs.name.localizedCaseInsensitiveCompare(rhs.name)
        return comparison == .orderedSame ? lhs.url.path < rhs.url.path : comparison == .orderedAscending
    }
}

private extension String {
    func distance(from start: Index, through end: Index) -> Int {
        distance(from: start, to: index(after: end))
    }
}
