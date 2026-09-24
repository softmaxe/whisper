const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

function sourceSlice(source, from, to) {
  const start = source.indexOf(from);
  const end = source.indexOf(to, start);
  assert.ok(start >= 0 && end > start, `missing ${from}`);
  return source.slice(start, end);
}

test(
  "the edit monitor wakes a dormant Chromium accessibility tree to find the focused field",
  { skip: process.platform !== "darwin" },
  (t) => {
    const source = fs.readFileSync(
      path.join(__dirname, "../../resources/macos-text-monitor.swift"),
      "utf8"
    );
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), "whisper-monitor-wake-"));
    t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    const harness = path.join(directory, "main.swift");
    const binary = path.join(directory, "monitor-wake-test");
    // Compile the actual focus lookup and wake walk against a fake application.
    // Chromium apps (Claude, Codex) answer kAXErrorNoValue for the focused
    // element until a client reads their window containers.
    fs.writeFileSync(
      harness,
      `
import Cocoa
import Darwin

final class AXUIElement: NSObject {
    var attributes: [String: AnyObject] = [:]
}
final class MockClock {
    var time = 0.0
    var systemUptime: TimeInterval { time }
}
enum ProcessInfo { static let processInfo = MockClock() }
enum Thread {
    static func sleep(forTimeInterval interval: TimeInterval) {
        ProcessInfo.processInfo.time += interval
    }
}
var application = AXUIElement()
var dormant = true
var focusReads = 0
var windowReads = 0
var writes: [String] = []
func writeError(_ message: String) {}
func AXUIElementGetTypeID() -> CFTypeID { CFGetTypeID(AXUIElement()) }
@discardableResult
func AXUIElementSetMessagingTimeout(_ element: AXUIElement, _ timeout: Float) -> AXError { .success }
func AXUIElementIsAttributeSettable(
    _ element: AXUIElement, _ attribute: CFString, _ settable: UnsafeMutablePointer<DarwinBoolean>
) -> AXError { .success }
func AXUIElementSetAttributeValue(_ element: AXUIElement, _ attribute: CFString, _ value: AnyObject) -> AXError {
    writes.append(attribute as String)
    return .success
}
func AXUIElementCopyAttributeValue(
    _ element: AXUIElement, _ attribute: CFString, _ value: UnsafeMutablePointer<AnyObject?>
) -> AXError {
    let name = attribute as String
    if element === application && name == kAXFocusedUIElementAttribute {
        focusReads += 1
        if dormant { return .noValue }
    }
    if element === application && name == kAXFocusedWindowAttribute { windowReads += 1 }
    guard let result = element.attributes[name] else { return .noValue }
    if name == kAXRoleAttribute && result as? String == "AXWebArea" { dormant = false }
    value.pointee = result
    return .success
}

func element(_ role: String, _ children: [AXUIElement] = []) -> AXUIElement {
    let result = AXUIElement()
    result.attributes[kAXRoleAttribute] = role as NSString
    result.attributes[kAXChildrenAttribute] = children as NSArray
    return result
}
let field = element("AXTextArea")
func reset(dormantTree: Bool) {
    application = element("AXApplication")
    application.attributes[kAXFocusedWindowAttribute] =
        element("AXWindow", [element("AXGroup", [element("AXWebArea")])])
    application.attributes[kAXFocusedUIElementAttribute] = field
    dormant = dormantTree
    focusReads = 0
    windowReads = 0
    ProcessInfo.processInfo.time = 0
}
${sourceSlice(source, "enum PasteTargetStatus", "func focusedPasteTargetStatus")}
${sourceSlice(source, "func resolveFocusedElement", "func observerCallback")}

reset(dormantTree: true)
guard resolveFocusedElement(application, pid: 42) === field else {
    fatalError("Dormant Chromium tree was not woken")
}
guard focusReads == 2 else { fatalError("Wake took \\(focusReads) focus reads") }

reset(dormantTree: false)
guard resolveFocusedElement(application, pid: 42) === field, windowReads == 0 else {
    fatalError("A native app with a focused field was walked")
}

reset(dormantTree: true)
application.attributes[kAXFocusedWindowAttribute] = nil
guard resolveFocusedElement(application, pid: 42) == nil, focusReads == 5 else {
    fatalError("An app without focus must give up after the retry ladder")
}
guard writes.isEmpty else { fatalError("Monitor wrote AX attributes: \\(writes)") }
print("monitor wake scenarios passed")
`
    );
    const compiled = spawnSync(
      "xcrun",
      ["swiftc", harness, "-module-cache-path", path.join(directory, "cache"), "-o", binary],
      { encoding: "utf8", timeout: 120000 }
    );
    assert.equal(compiled.status, 0, compiled.stderr || compiled.error?.message);
    const result = spawnSync(binary, [], { encoding: "utf8", timeout: 10000 });
    assert.equal(result.status, 0, result.stderr || result.error?.message);
    assert.match(result.stdout, /monitor wake scenarios passed/);
  }
);
