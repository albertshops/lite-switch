import AppKit

struct InstalledApplication: Identifiable, Hashable, Sendable {
    let url: URL
    let name: String
    let bundleIdentifier: String?

    var id: URL { url }

    var containingLocation: String {
        (url.deletingLastPathComponent().path as NSString).abbreviatingWithTildeInPath
    }

    @MainActor
    var icon: NSImage {
        NSWorkspace.shared.icon(forFile: url.path)
    }
}
