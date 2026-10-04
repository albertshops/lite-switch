// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "LiteSwitch",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "LiteSwitchCore", targets: ["LiteSwitchCore"]),
        .executable(name: "LiteSwitch", targets: ["LiteSwitch"]),
        .executable(name: "LiteSwitchCoreChecks", targets: ["LiteSwitchCoreChecks"]),
    ],
    targets: [
        .target(name: "LiteSwitchCore"),
        .executableTarget(
            name: "LiteSwitch",
            dependencies: ["LiteSwitchCore"],
            exclude: ["Resources/Info.plist"],
            linkerSettings: [
                .linkedFramework("AppKit"),
                .linkedFramework("ApplicationServices"),
                .linkedFramework("Carbon"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("ServiceManagement"),
                .linkedLibrary("sqlite3"),
            ]
        ),
        .executableTarget(
            name: "LiteSwitchCoreChecks",
            dependencies: ["LiteSwitchCore"]
        ),
    ],
    swiftLanguageModes: [.v5]
)
