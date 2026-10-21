/**
 * macOS laptop lid watcher
 *
 * `--watch-lid-state` prints {"lidClosed":true|false|null} at once and then
 * every second, so the parent can expire a stale state.
 *
 * Compile: swiftc -O macos-mic-listener.swift -o macos-mic-listener -framework Foundation -framework IOKit
 */

import Foundation
import IOKit

func readLaptopLidState() -> Bool? {
    let service = IOServiceGetMatchingService(kIOMasterPortDefault, IOServiceMatching("IOPMrootDomain"))
    guard service != IO_OBJECT_NULL else { return nil }
    defer { IOObjectRelease(service) }

    guard let property = IORegistryEntryCreateCFProperty(
        service,
        "AppleClamshellState" as CFString,
        kCFAllocatorDefault,
        0
    )?.takeRetainedValue(), CFGetTypeID(property) == CFBooleanGetTypeID() else { return nil }
    return (property as? NSNumber)?.boolValue
}

func emit(_ message: String) {
    print(message)
    fflush(stdout)
}

func emitLaptopLidState() {
    let state = readLaptopLidState().map { $0 ? "true" : "false" } ?? "null"
    emit("{\"lidClosed\":\(state)}")
}

var signalSources: [DispatchSourceSignal] = []

func setupSignalHandlers() {
    for signalNumber in [SIGTERM, SIGINT] {
        signal(signalNumber, SIG_IGN)
        let source = DispatchSource.makeSignalSource(signal: signalNumber, queue: .main)
        source.setEventHandler { exit(0) }
        source.resume()
        signalSources.append(source)
    }
}

guard CommandLine.arguments.contains("--watch-lid-state") else {
    FileHandle.standardError.write("Usage: macos-mic-listener --watch-lid-state\n".data(using: .utf8)!)
    exit(2)
}

setupSignalHandlers()
emitLaptopLidState()
let lidTimer = DispatchSource.makeTimerSource(queue: .main)
lidTimer.schedule(deadline: .now() + 1, repeating: 1)
// Emit a heartbeat even without a change so the parent can expire stale state.
lidTimer.setEventHandler { emitLaptopLidState() }
lidTimer.resume()
withExtendedLifetime(lidTimer) { CFRunLoopRun() }
exit(0)
