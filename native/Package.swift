// swift-tools-version: 6.4
import PackageDescription

let package = Package(
    name: "WhisperNative",
    platforms: [.macOS("27.0")],
    products: [
        .library(name: "WhisperCore", targets: ["WhisperCore"]),
        .executable(name: "Whisper", targets: ["WhisperApp"])
    ],
    targets: [
        .target(name: "WhisperCore", resources: [.copy("Resources")]),
        .executableTarget(name: "WhisperApp", dependencies: ["WhisperCore"], resources: [.copy("Resources")]),
        .testTarget(name: "WhisperCoreTests", dependencies: ["WhisperCore"], resources: [.copy("Resources")])
    ]
)
