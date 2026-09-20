import Foundation

struct ApplicationDiscovery {
    static var defaultRoots: [URL] {
        [
            URL(fileURLWithPath: "/Applications", isDirectory: true),
            URL(fileURLWithPath: "/System/Applications", isDirectory: true),
            FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Applications", isDirectory: true),
            URL(fileURLWithPath: "/System/Library/CoreServices/Finder.app", isDirectory: true)
        ]
    }

    private let fileManager: FileManager

    init(fileManager: FileManager = .default) {
        self.fileManager = fileManager
    }

    func discover(in roots: [URL] = Self.defaultRoots) -> [InstalledApplication] {
        var visitedDirectories = Set<URL>()
        var applicationsByURL: [URL: InstalledApplication] = [:]

        for root in roots {
            scan(
                directory: root,
                visitedDirectories: &visitedDirectories,
                applicationsByURL: &applicationsByURL
            )
        }

        return applicationsByURL.values.sorted {
            let comparison = $0.name.localizedCaseInsensitiveCompare($1.name)
            return comparison == .orderedSame ? $0.url.path < $1.url.path : comparison == .orderedAscending
        }
    }

    private func scan(
        directory: URL,
        visitedDirectories: inout Set<URL>,
        applicationsByURL: inout [URL: InstalledApplication]
    ) {
        let directory = resolvedURL(for: directory)
        guard visitedDirectories.insert(directory).inserted else { return }

        // Individual bundles may be roots too. This lets default discovery include
        // selected system applications without indexing every CoreServices helper.
        if directory.pathExtension.caseInsensitiveCompare("app") == .orderedSame {
            if let application = application(at: directory) {
                applicationsByURL[application.url] = application
            }
            return
        }

        guard let children = try? fileManager.contentsOfDirectory(
            at: directory,
            includingPropertiesForKeys: [.isDirectoryKey, .isAliasFileKey, .isSymbolicLinkKey],
            options: [.skipsHiddenFiles]
        ) else { return }

        for child in children {
            let resolved = resolvedURL(for: child)

            // An application is an opaque leaf, even when it is malformed. This keeps
            // helpers nested in Contents from becoming independent launcher results.
            if child.pathExtension.caseInsensitiveCompare("app") == .orderedSame ||
                resolved.pathExtension.caseInsensitiveCompare("app") == .orderedSame {
                if let application = application(at: resolved) {
                    applicationsByURL[application.url] = application
                }
                continue
            }

            if (try? resolved.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true {
                scan(
                    directory: resolved,
                    visitedDirectories: &visitedDirectories,
                    applicationsByURL: &applicationsByURL
                )
            }
        }
    }

    private func resolvedURL(for url: URL) -> URL {
        let values = try? url.resourceValues(forKeys: [.isAliasFileKey, .isSymbolicLinkKey])
        var resolved = url

        if values?.isAliasFile == true,
           let aliasTarget = try? URL(
               resolvingAliasFileAt: url,
               options: [.withoutUI, .withoutMounting]
           ) {
            resolved = aliasTarget
        }

        if values?.isSymbolicLink == true {
            resolved = resolved.resolvingSymlinksInPath()
        }

        return resolved.standardizedFileURL
    }

    private func application(at candidateURL: URL) -> InstalledApplication? {
        let canonicalURL = resolvedURL(for: candidateURL)
        guard canonicalURL.pathExtension.caseInsensitiveCompare("app") == .orderedSame,
              (try? canonicalURL.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true,
              let bundle = Bundle(url: canonicalURL),
              bundle.bundleURL.standardizedFileURL == canonicalURL,
              let executableURL = bundle.executableURL,
              fileManager.isExecutableFile(atPath: executableURL.path),
              (bundle.object(forInfoDictionaryKey: "LSBackgroundOnly") as? NSNumber)?.boolValue != true
        else { return nil }

        let name = (bundle.object(forInfoDictionaryKey: "CFBundleDisplayName") as? String)
            ?? (bundle.object(forInfoDictionaryKey: "CFBundleName") as? String)
            ?? fileManager.displayName(atPath: canonicalURL.path)

        guard !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return nil }
        return InstalledApplication(url: canonicalURL, name: name, bundleIdentifier: bundle.bundleIdentifier)
    }
}
