import Foundation

public struct ShortcutKey: RawRepresentable, Codable, Equatable, Hashable, Sendable {
    public let rawValue: String

    public init?(rawValue: String) {
        let normalized = rawValue.lowercased()
        if normalized.count == 1,
           let character = normalized.first,
           character.isASCII,
           character.isLetter || character.isNumber {
            self.rawValue = normalized
            return
        }

        guard Self.arrowValues.contains(rawValue) else { return nil }
        self.rawValue = rawValue
    }

    public init?(_ character: Character) {
        self.init(rawValue: String(character))
    }

    public static let leftArrow = ShortcutKey(rawValue: "arrowLeft")!
    public static let rightArrow = ShortcutKey(rawValue: "arrowRight")!
    public static let upArrow = ShortcutKey(rawValue: "arrowUp")!
    public static let downArrow = ShortcutKey(rawValue: "arrowDown")!

    public var displayValue: String {
        switch self {
        case .leftArrow: "←"
        case .rightArrow: "→"
        case .upArrow: "↑"
        case .downArrow: "↓"
        default: rawValue.uppercased()
        }
    }

    private static let arrowValues = ["arrowLeft", "arrowRight", "arrowUp", "arrowDown"]
}

public struct ProgramIdentity: Codable, Equatable, Hashable, Sendable {
    public let bundleIdentifier: String
    public let name: String

    public init(bundleIdentifier: String, name: String) {
        self.bundleIdentifier = bundleIdentifier
        self.name = name
    }

    public func matches(_ candidate: ProgramIdentity) -> Bool {
        bundleIdentifier == candidate.bundleIdentifier
    }
}

public struct WindowIdentity: Codable, Equatable, Hashable, Sendable {
    public let bundleIdentifier: String
    public let title: String
    public let documentURL: String?
    public let windowNumber: Int?

    public init(bundleIdentifier: String, title: String, documentURL: String?, windowNumber: Int? = nil) {
        self.bundleIdentifier = bundleIdentifier
        self.title = title
        self.documentURL = documentURL
        self.windowNumber = windowNumber
    }

    public func matches(_ candidate: WindowIdentity) -> Bool {
        guard bundleIdentifier == candidate.bundleIdentifier else { return false }
        if let windowNumber, let candidateWindowNumber = candidate.windowNumber {
            return windowNumber == candidateWindowNumber
        }
        return matchesPersistedProperties(candidate)
    }

    public func matchesPersistedProperties(_ candidate: WindowIdentity) -> Bool {
        guard bundleIdentifier == candidate.bundleIdentifier else { return false }
        if let documentURL {
            return documentURL == candidate.documentURL
        }
        return title == candidate.title
    }
}

public struct VivaldiTabIdentity: Codable, Equatable, Hashable, Sendable {
    public let id: String
    public let title: String
    public let url: String

    public init(id: String, title: String, url: String) {
        self.id = id
        self.title = title
        self.url = url
    }

    public func matches(_ candidate: VivaldiTabIdentity) -> Bool {
        id == candidate.id
    }
}

public enum WindowAction: String, Codable, Equatable, Hashable, Sendable, CaseIterable {
    case fillScreen
    case switchScreen
    case moveLeft
    case moveRight
    case moveTop
    case moveBottom
}

public enum AssignmentTarget: Codable, Equatable, Hashable, Sendable {
    case program(ProgramIdentity)
    case window(WindowIdentity)
    case vivaldiTab(VivaldiTabIdentity)
    case action(WindowAction)

    private enum CodingKeys: String, CodingKey {
        case type
        case program
        case window
        case vivaldiTab
        case action
    }

    private enum TargetType: String, Codable {
        case program
        case window
        case vivaldiTab
        case action
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        switch try container.decode(TargetType.self, forKey: .type) {
        case .program:
            self = .program(try container.decode(ProgramIdentity.self, forKey: .program))
        case .window:
            self = .window(try container.decode(WindowIdentity.self, forKey: .window))
        case .vivaldiTab:
            self = .vivaldiTab(try container.decode(VivaldiTabIdentity.self, forKey: .vivaldiTab))
        case .action:
            self = .action(try container.decode(WindowAction.self, forKey: .action))
        }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case let .program(identity):
            try container.encode(TargetType.program, forKey: .type)
            try container.encode(identity, forKey: .program)
        case let .window(identity):
            try container.encode(TargetType.window, forKey: .type)
            try container.encode(identity, forKey: .window)
        case let .vivaldiTab(identity):
            try container.encode(TargetType.vivaldiTab, forKey: .type)
            try container.encode(identity, forKey: .vivaldiTab)
        case let .action(action):
            try container.encode(TargetType.action, forKey: .type)
            try container.encode(action, forKey: .action)
        }
    }

    public func matches(_ candidate: AssignmentTarget) -> Bool {
        switch (self, candidate) {
        case let (.program(identity), .program(candidateIdentity)):
            identity.matches(candidateIdentity)
        case let (.window(identity), .window(candidateIdentity)):
            identity.matches(candidateIdentity)
        case let (.vivaldiTab(identity), .vivaldiTab(candidateIdentity)):
            identity.matches(candidateIdentity)
        case let (.action(action), .action(candidateAction)):
            action == candidateAction
        default:
            false
        }
    }
}

public struct AssignmentBook: Codable, Equatable, Sendable {
    public private(set) var assignments: [String: AssignmentTarget]
    public private(set) var launchIfNeededBundleIdentifiers: Set<String>

    public init(
        assignments: [String: AssignmentTarget] = [:],
        launchIfNeededBundleIdentifiers: Set<String> = []
    ) {
        self.assignments = assignments
        self.launchIfNeededBundleIdentifiers = launchIfNeededBundleIdentifiers
        pruneLaunchPreferences()
    }

    public mutating func assign(_ key: ShortcutKey, to target: AssignmentTarget) {
        assignments = assignments.filter { !$0.value.matches(target) }
        assignments[key.rawValue] = target
        pruneLaunchPreferences()
    }

    public mutating func remove(key: ShortcutKey) {
        assignments.removeValue(forKey: key.rawValue)
        pruneLaunchPreferences()
    }

    public mutating func remove(target: AssignmentTarget) {
        assignments = assignments.filter { !$0.value.matches(target) }
        pruneLaunchPreferences()
    }

    public mutating func setLaunchIfNeeded(_ enabled: Bool, for program: ProgramIdentity) {
        guard assignments.values.contains(where: {
            if case let .program(identity) = $0 { return identity.matches(program) }
            return false
        }) else { return }

        if enabled {
            launchIfNeededBundleIdentifiers.insert(program.bundleIdentifier)
        } else {
            launchIfNeededBundleIdentifiers.remove(program.bundleIdentifier)
        }
    }

    public func shouldLaunchIfNeeded(_ program: ProgramIdentity) -> Bool {
        launchIfNeededBundleIdentifiers.contains(program.bundleIdentifier)
    }

    public func key(for target: AssignmentTarget) -> ShortcutKey? {
        assignments.first { $0.value.matches(target) }.flatMap { ShortcutKey(rawValue: $0.key) }
    }

    public func target(for key: ShortcutKey) -> AssignmentTarget? {
        assignments[key.rawValue]
    }

    public func windowIdentities(for program: ProgramIdentity) -> [WindowIdentity] {
        assignments.values.compactMap { target in
            guard case let .window(identity) = target,
                  identity.bundleIdentifier == program.bundleIdentifier
            else { return nil }
            return identity
        }
    }

    private enum CodingKeys: String, CodingKey {
        case assignments
        case launchIfNeededBundleIdentifiers
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        assignments = try container.decode([String: AssignmentTarget].self, forKey: .assignments)
        launchIfNeededBundleIdentifiers = try container.decodeIfPresent(
            Set<String>.self,
            forKey: .launchIfNeededBundleIdentifiers
        ) ?? []
        pruneLaunchPreferences()
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(assignments, forKey: .assignments)
        if !launchIfNeededBundleIdentifiers.isEmpty {
            try container.encode(launchIfNeededBundleIdentifiers, forKey: .launchIfNeededBundleIdentifiers)
        }
    }

    private mutating func pruneLaunchPreferences() {
        let assignedPrograms = Set<String>(assignments.values.compactMap { target in
            guard case let .program(identity) = target else { return nil }
            return identity.bundleIdentifier
        })
        launchIfNeededBundleIdentifiers.formIntersection(assignedPrograms)
    }
}

public final class AssignmentStore: @unchecked Sendable {
    private let defaults: UserDefaults
    private let key: String
    private let legacyKey: String
    private var sessionAssignments: [String: AssignmentTarget] = [:]

    public init(
        defaults: UserDefaults = .standard,
        key: String = "programAssignments",
        legacyKey: String = "windowAssignments"
    ) {
        self.defaults = defaults
        self.key = key
        self.legacyKey = legacyKey
    }

    public func load() -> AssignmentBook {
        if let data = defaults.data(forKey: key) {
            if let book = try? JSONDecoder().decode(AssignmentBook.self, from: data) {
                let persistentBook = persistentAssignments(from: book)
                if persistentBook != book {
                    savePersistent(persistentBook)
                }
                return mergingSessionAssignments(into: persistentBook)
            }
            if let legacy = try? JSONDecoder().decode(LegacyProgramBook.self, from: data) {
                let book = AssignmentBook(assignments: legacy.assignments.mapValues(AssignmentTarget.program))
                savePersistent(book)
                return mergingSessionAssignments(into: book)
            }
        }
        defaults.removeObject(forKey: legacyKey)
        return mergingSessionAssignments(into: AssignmentBook())
    }

    public func save(_ book: AssignmentBook) {
        sessionAssignments = book.assignments.filter { _, target in
            switch target {
            case .window, .vivaldiTab: true
            case .program, .action: false
            }
        }
        savePersistent(persistentAssignments(from: book))
    }

    private func persistentAssignments(from book: AssignmentBook) -> AssignmentBook {
        let assignments = book.assignments.filter { _, target in
            switch target {
            case .program, .action: true
            case .window, .vivaldiTab: false
            }
        }
        return AssignmentBook(
            assignments: assignments,
            launchIfNeededBundleIdentifiers: book.launchIfNeededBundleIdentifiers
        )
    }

    private func mergingSessionAssignments(into persistentBook: AssignmentBook) -> AssignmentBook {
        var book = persistentBook
        for (rawKey, target) in sessionAssignments.sorted(by: { $0.key < $1.key }) {
            guard let key = ShortcutKey(rawValue: rawKey) else { continue }
            book.assign(key, to: target)
        }
        return book
    }

    private func savePersistent(_ book: AssignmentBook) {
        guard let data = try? JSONEncoder().encode(book) else { return }
        defaults.set(data, forKey: key)
    }
}

private struct LegacyProgramBook: Decodable {
    let assignments: [String: ProgramIdentity]
}
