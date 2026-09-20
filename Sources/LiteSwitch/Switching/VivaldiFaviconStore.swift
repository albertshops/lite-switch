import Foundation
import SQLite3

struct VivaldiFaviconStore {
    private let fileManager = FileManager.default

    func faviconData(for tabs: [ManagedVivaldiTab]) -> [String: Data] {
        var unresolved: [String: [String]] = [:]
        for tab in tabs {
            unresolved[tab.url, default: []].append(tab.id)
        }
        var results: [String: Data] = [:]

        for databaseURL in databaseURLs() where !unresolved.isEmpty {
            read(databaseURL: databaseURL, unresolved: &unresolved, results: &results)
        }
        return results
    }

    private func databaseURLs() -> [URL] {
        let root = fileManager.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/Vivaldi", isDirectory: true)
        guard let profiles = try? fileManager.contentsOfDirectory(
            at: root,
            includingPropertiesForKeys: [.isDirectoryKey],
            options: [.skipsHiddenFiles]
        ) else { return [] }

        return profiles.compactMap { profile in
            let database = profile.appendingPathComponent("Favicons")
            return fileManager.fileExists(atPath: database.path) ? database : nil
        }
    }

    private func read(
        databaseURL: URL,
        unresolved: inout [String: [String]],
        results: inout [String: Data]
    ) {
        var database: OpaquePointer?
        let uri = "file:\(databaseURL.path)?immutable=1"
        guard sqlite3_open_v2(uri, &database, SQLITE_OPEN_READONLY | SQLITE_OPEN_URI, nil) == SQLITE_OK,
              let database
        else {
            if let database { sqlite3_close(database) }
            return
        }
        defer { sqlite3_close(database) }

        let sql = """
        SELECT favicon_bitmaps.image_data
        FROM icon_mapping
        JOIN favicon_bitmaps ON favicon_bitmaps.icon_id = icon_mapping.icon_id
        WHERE icon_mapping.page_url = ?
        ORDER BY favicon_bitmaps.width DESC, favicon_bitmaps.last_updated DESC
        LIMIT 1
        """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(database, sql, -1, &statement, nil) == SQLITE_OK,
              let statement
        else { return }
        defer { sqlite3_finalize(statement) }

        var resolvedURLs: [String] = []
        for (pageURL, tabIDs) in unresolved {
            sqlite3_reset(statement)
            sqlite3_clear_bindings(statement)
            _ = pageURL.withCString { value in
                sqlite3_bind_text(statement, 1, value, -1, SQLITE_TRANSIENT)
            }
            guard sqlite3_step(statement) == SQLITE_ROW,
                  let bytes = sqlite3_column_blob(statement, 0)
            else { continue }
            let count = Int(sqlite3_column_bytes(statement, 0))
            guard count > 0 else { continue }
            let data = Data(bytes: bytes, count: count)
            for tabID in tabIDs {
                results[tabID] = data
            }
            resolvedURLs.append(pageURL)
        }
        for pageURL in resolvedURLs {
            unresolved.removeValue(forKey: pageURL)
        }
    }
}

private let SQLITE_TRANSIENT = unsafeBitCast(-1, to: sqlite3_destructor_type.self)
