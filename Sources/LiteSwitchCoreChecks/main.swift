import CoreGraphics
import Foundation
import LiteSwitchCore

func expect(_ condition: @autoclosure () -> Bool, _ message: String) {
    guard condition() else {
        FileHandle.standardError.write(Data("Check failed: \(message)\n".utf8))
        exit(1)
    }
}

let app = ProgramIdentity(bundleIdentifier: "com.example.editor", name: "Editor")
let otherName = ProgramIdentity(bundleIdentifier: "com.example.editor", name: "Renamed Editor")
expect(app.matches(otherName), "program identity should match by bundle identifier")

var book = AssignmentBook()
let key = ShortcutKey("e")!
book.assign(key, to: .program(app))
book.setLaunchIfNeeded(true, for: app)
expect(book.target(for: key) == .program(app), "assignment lookup")
expect(book.shouldLaunchIfNeeded(app), "launch-if-needed preference")
book.remove(key: key)
expect(!book.shouldLaunchIfNeeded(app), "removing an assignment should prune its preference")

let screen = CGRect(x: 0, y: 0, width: 1200, height: 800)
expect(
    WindowGeometry.frame(for: .moveLeft, currentFrame: screen, sourceVisibleFrame: screen)
        == CGRect(x: 0, y: 0, width: 600, height: 800),
    "left-half geometry"
)
expect(
    WindowGeometry.frame(for: .moveTop, currentFrame: screen, sourceVisibleFrame: screen)
        == CGRect(x: 0, y: 400, width: 1200, height: 400),
    "top-half geometry"
)

print("LiteSwitchCore checks passed")
