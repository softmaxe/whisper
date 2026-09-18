const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

test(
  "macOS paste targets use writable fields and plain Command-V availability",
  { skip: process.platform !== "darwin" },
  (t) => {
    const source = fs.readFileSync(
      path.join(__dirname, "../../resources/macos-text-monitor.swift"),
      "utf8"
    );
    const start = source.indexOf("func isEditableTextElement");
    const end = source.indexOf("func readCurrentValue", start);
    assert.ok(start >= 0 && end > start);
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), "openwhispr-paste-target-"));
    t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    const harness = path.join(directory, "main.swift");
    const binary = path.join(directory, "paste-target-test");
    // Compile the actual probe with AX responses supplied by a fake application.
    // These tests never query apps, change focus, or require Accessibility access.
    fs.writeFileSync(
      harness,
      `
import Cocoa
import Darwin

final class AXUIElement: NSObject {
    var attributes: [String: AnyObject] = [:]
    var writable: Set<String> = []
    var unavailable = false
}
struct MockApplication {
    let processIdentifier: pid_t
    var bundleIdentifier: String? = "com.example.editor"
}
final class MockWorkspace {
    var frontmostApplication: MockApplication? = MockApplication(processIdentifier: 42)
}
enum NSWorkspace { static let shared = MockWorkspace() }
final class MockClock {
    var time = 0.0
    var step = 0.0
    var systemUptime: TimeInterval {
        time += step
        return time
    }
}
enum ProcessInfo { static let processInfo = MockClock() }
enum Thread {
    static func sleep(forTimeInterval interval: TimeInterval) {
        ProcessInfo.processInfo.time += interval
    }
}
var focusResponses: [AXUIElement?] = []
var focusReads = 0
var onRoleRead: ((AXUIElement) -> Void)?
var application = AXUIElement()
var trusted = true
var reads = 0
var switchOnRead = false
func AXIsProcessTrusted() -> Bool { trusted }
func AXUIElementCreateApplication(_ pid: pid_t) -> AXUIElement { application }
func AXUIElementGetTypeID() -> CFTypeID { CFGetTypeID(AXUIElement()) }
@discardableResult
func AXUIElementSetMessagingTimeout(_ element: AXUIElement, _ timeout: Float) -> AXError {
    guard timeout > 0 && timeout <= 0.1 else { fatalError("Unbounded AX query") }
    return .success
}
func AXUIElementCopyAttributeValue(
    _ element: AXUIElement, _ attribute: CFString, _ value: UnsafeMutablePointer<AnyObject?>
) -> AXError {
    reads += 1
    if switchOnRead {
        NSWorkspace.shared.frontmostApplication = MockApplication(processIdentifier: 99)
    }
    if element.unavailable { return .cannotComplete }
    if attribute as String == kAXRoleAttribute { onRoleRead?(element) }
    if element === application, attribute as String == kAXFocusedUIElementAttribute {
        focusReads += 1
        if !focusResponses.isEmpty {
            value.pointee = focusResponses.removeFirst()
            return value.pointee == nil ? .noValue : .success
        }
    }
    value.pointee = element.attributes[attribute as String]
    return value.pointee == nil ? .attributeUnsupported : .success
}
func AXUIElementIsAttributeSettable(
    _ element: AXUIElement, _ attribute: CFString, _ settable: UnsafeMutablePointer<DarwinBoolean>
) -> AXError {
    settable.pointee = DarwinBoolean(element.writable.contains(attribute as String))
    return element.unavailable ? .cannotComplete : .success
}

${source.slice(start, end)}

func element(_ role: String, writable: Bool = false, selection: String = "") -> AXUIElement {
    let result = AXUIElement()
    result.attributes[kAXRoleAttribute] = role as NSString
    result.attributes[kAXEnabledAttribute] = true as NSNumber
    result.attributes[kAXSelectedTextAttribute] = selection as NSString
    if writable { result.writable.insert(kAXValueAttribute) }
    return result
}
func pasteMenu(enabled: Bool, modifiers: Int = 0, command: String = "V") -> AXUIElement {
    let item = element("AXMenuItem")
    item.attributes[kAXMenuItemCmdCharAttribute] = command as NSString
    item.attributes[kAXMenuItemCmdModifiersAttribute] = modifiers as NSNumber
    item.attributes[kAXEnabledAttribute] = enabled as NSNumber
    item.attributes[kAXTitleAttribute] = "Coller" as NSString
    let submenu = element("AXMenu")
    submenu.attributes[kAXChildrenAttribute] = [item] as NSArray
    let edit = element("AXMenuBarItem")
    edit.attributes[kAXChildrenAttribute] = [submenu] as NSArray
    let bar = element("AXMenuBar")
    bar.attributes[kAXChildrenAttribute] = [edit] as NSArray
    return bar
}
func configure(
    _ focused: AXUIElement?, menu: AXUIElement? = nil,
    bundleIdentifier: String = "com.example.editor"
) {
    application = AXUIElement()
    application.attributes[kAXFocusedUIElementAttribute] = focused
    application.attributes[kAXMenuBarAttribute] = menu
    trusted = true
    reads = 0
    focusReads = 0
    focusResponses = []
    onRoleRead = nil
    switchOnRead = false
    NSWorkspace.shared.frontmostApplication = MockApplication(
        processIdentifier: 42, bundleIdentifier: bundleIdentifier
    )
    ProcessInfo.processInfo.time = 0
    ProcessInfo.processInfo.step = 0
}
func expect(_ name: String, _ expected: PasteTargetStatus) {
    let actual = pasteTargetStatus(for: 42)
    guard actual == expected else {
        fatalError(name + ": expected " + expected.rawValue + ", got " + actual.rawValue)
    }
}

let text = element("AXTextField", writable: true)
configure(text)
expect("editable text", .pasteable)
guard isEditableTextElement(text) else { fatalError("Assistant must accept an empty cursor") }
text.attributes[kAXSelectedTextAttribute] = "replace this" as NSString
configure(text)
expect("dictation replaces a selection", .pasteable)
guard !isEditableTextElement(text) else { fatalError("Assistant must preserve selections") }

let combo = element("AXComboBox")
combo.writable.insert(kAXSelectedTextAttribute)
configure(combo)
expect("selected text is settable", .pasteable)

text.attributes[kAXEnabledAttribute] = false as NSNumber
configure(text, menu: pasteMenu(enabled: true))
expect("disabled field", .notPasteable)
guard !isEditableTextElement(text) else { fatalError("Assistant accepted disabled field") }
text.attributes[kAXEnabledAttribute] = true as NSNumber
text.attributes[kAXSubroleAttribute] = "AXSecureTextField" as NSString
configure(text, menu: pasteMenu(enabled: true))
expect("secure field", .notPasteable)
guard !isEditableTextElement(text) else { fatalError("Assistant accepted secure field") }

let readOnly = element("AXTextArea")
readOnly.attributes["AXEditable"] = false as NSNumber
configure(readOnly)
expect("explicitly read-only field", .notPasteable)
configure(readOnly, menu: pasteMenu(enabled: false))
expect("read-only field without Paste", .notPasteable)
configure(element("AXTextArea"), menu: pasteMenu(enabled: false))
expect("non-settable field without Paste", .notPasteable)
configure(element("AXStaticText"))
expect("static text", .notPasteable)
configure(element("AXButton"))
expect("focused button", .notPasteable)
for role in ["AXButton", "AXStaticText", "AXLink", "AXWebArea"] {
    configure(element(role), menu: pasteMenu(enabled: true))
    expect("non-editor focus overrides enabled Paste", .notPasteable)
}
configure(element("AXScrollArea"), menu: pasteMenu(enabled: false))
expect("desktop without text focus", .notPasteable)
configure(nil, menu: pasteMenu(enabled: false))
expect("no field and disabled Paste", .notPasteable)

for role in ["AXScrollArea", "AXList", "AXOutline", "AXBrowser", "AXWindow"] {
    configure(element(role), menu: pasteMenu(enabled: true), bundleIdentifier: "com.apple.finder")
    expect("Finder desktop or file view with enabled Paste", .notPasteable)
}
configure(nil, menu: pasteMenu(enabled: true), bundleIdentifier: "com.apple.finder")
expect("Finder without a focused element", .notPasteable)
configure(nil, bundleIdentifier: "com.apple.finder")
expect("Finder without AX focus or menus", .notPasteable)
application.unavailable = true
expect("Finder with unavailable AX", .notPasteable)
let finderUnavailableField = element("AXTextField", writable: true)
finderUnavailableField.unavailable = true
configure(finderUnavailableField, menu: pasteMenu(enabled: true), bundleIdentifier: "com.apple.finder")
expect("Finder with unavailable field attributes", .notPasteable)
configure(element("AXTextField"), menu: pasteMenu(enabled: true), bundleIdentifier: "com.apple.finder")
expect("Finder with a non-writable text field", .notPasteable)
configure(readOnly, menu: pasteMenu(enabled: true), bundleIdentifier: "com.apple.finder")
expect("Finder with a read-only field", .notPasteable)
let finderSearch = element("AXTextField", writable: true)
configure(finderSearch, bundleIdentifier: "com.apple.finder")
expect("Finder search field", .pasteable)
let finderRename = element("AXTextField", selection: "selected filename")
finderRename.writable.insert(kAXSelectedTextAttribute)
configure(finderRename, bundleIdentifier: "com.apple.finder")
expect("Finder rename with selected filename", .pasteable)
configure(finderSearch, menu: pasteMenu(enabled: true), bundleIdentifier: "com.apple.finder")
trusted = false
expect("Finder without Accessibility trust", .notPasteable)
guard reads == 0 else { fatalError("Untrusted Finder probe queried AX") }
configure(finderSearch, bundleIdentifier: "com.apple.finder")
switchOnRead = true
expect("Finder loses focus during the probe", .notPasteable)

for browser in [
    "com.brave.Browser", "com.brave.Browser.beta", "com.brave.Browser.nightly",
    "com.google.Chrome", "com.google.Chrome.canary", "org.chromium.Chromium",
    "com.apple.Safari", "com.apple.SafariTechnologyPreview", "com.microsoft.edgemac",
    "org.mozilla.firefox", "company.thebrowser.Browser"
] {
    for role in ["AXGroup", "AXWebArea", "AXWindow", "AXScrollArea", "AXLink", "AXButton"] {
        configure(element(role), menu: pasteMenu(enabled: true), bundleIdentifier: browser)
        expect(browser + " page without an input cursor", .notPasteable)
    }
    configure(nil, menu: pasteMenu(enabled: true), bundleIdentifier: browser)
    expect(browser + " dormant AX with enabled Paste", .notPasteable)
    configure(nil, bundleIdentifier: browser)
    expect(browser + " unavailable focus and menu", .notPasteable)
    application.unavailable = true
    expect(browser + " failed AX queries", .notPasteable)
    configure(readOnly, menu: pasteMenu(enabled: true), bundleIdentifier: browser)
    expect(browser + " read-only textarea", .notPasteable)
    configure(element("AXTextField"), menu: pasteMenu(enabled: true), bundleIdentifier: browser)
    expect(browser + " unconfirmed text field", .notPasteable)
    for role in ["AXTextField", "AXTextArea", "AXComboBox"] {
        let input = element(role, writable: true)
        configure(input, bundleIdentifier: browser)
        expect(browser + " focused input", .pasteable)
        input.attributes[kAXSelectedTextAttribute] = "replace this" as NSString
        expect(browser + " focused selection", .pasteable)
        input.attributes[kAXEnabledAttribute] = false as NSNumber
        expect(browser + " disabled input", .notPasteable)
    }
    for role in ["AXGroup", "AXTextArea", "AXTextField", "AXComboBox"] {
        for attribute in [kAXValueAttribute, kAXSelectedTextAttribute, "AXEditable"] {
            let editor = element(role, selection: "selected words")
            if attribute == "AXEditable" {
                editor.attributes[attribute] = true as NSNumber
            } else {
                editor.writable.insert(attribute)
            }
            configure(editor, bundleIdentifier: browser)
            expect(browser + " rich editor with " + attribute, .pasteable)
            editor.attributes["AXEditable"] = false as NSNumber
            expect(browser + " explicitly read-only rich editor", .notPasteable)
            editor.attributes.removeValue(forKey: "AXEditable")
            editor.attributes[kAXEnabledAttribute] = false as NSNumber
            expect(browser + " disabled rich editor", .notPasteable)
            editor.attributes[kAXEnabledAttribute] = true as NSNumber
            editor.attributes[kAXSubroleAttribute] = "AXSecureTextField" as NSString
            expect(browser + " secure rich editor", .notPasteable)
        }
    }
    for role in ["AXGroup", "AXTextField", "AXTextArea", "AXComboBox", "AXWebArea", "AXStaticText"] {
        let selectionOnly = element(role, selection: "read-only page selection")
        selectionOnly.writable.insert("AXSelectedTextMarkerRange")
        selectionOnly.writable.insert(kAXSelectedTextRangeAttribute)
        configure(selectionOnly, menu: pasteMenu(enabled: true), bundleIdentifier: browser)
        expect(browser + " selectable page is not editable", .notPasteable)
    }
    let grokEditor = element("AXGroup")
    grokEditor.writable.insert(kAXValueAttribute)
    for initial in [nil, element("AXWindow"), element("AXWebArea")] {
        configure(grokEditor, bundleIdentifier: browser)
        focusResponses = [initial, grokEditor]
        expect(browser + " lazy AX focus resolves to rich editor", .pasteable)
        guard focusReads == 2 else { fatalError("Browser focus was not retried") }
    }
    configure(nil, bundleIdentifier: browser)
    expect(browser + " unavailable focus has bounded retries", .notPasteable)
    guard focusReads == 3 else { fatalError("Browser focus retry budget changed") }
    configure(grokEditor, bundleIdentifier: browser)
    trusted = false
    expect(browser + " untrusted rich editor", .notPasteable)
    guard reads == 0 else { fatalError("Untrusted browser probe queried AX") }
    configure(grokEditor, bundleIdentifier: browser)
    switchOnRead = true
    expect(browser + " rich editor loses app focus", .notPasteable)
}

let coldEditor = element("AXTextArea", writable: true)
configure(nil, bundleIdentifier: "com.brave.Browser")
onRoleRead = { queried in
    if queried === application { application.attributes[kAXFocusedUIElementAttribute] = coldEditor }
}
expect("reading the browser application role wakes native AX", .pasteable)

configure(nil, bundleIdentifier: "com.brave.Browser")
let webContainer = element("AXScrollArea")
let windowGroup = element("AXGroup")
windowGroup.attributes[kAXChildrenAttribute] = [webContainer] as NSArray
let browserWindow = element("AXWindow")
browserWindow.attributes[kAXChildrenAttribute] = [windowGroup] as NSArray
application.attributes[kAXFocusedWindowAttribute] = browserWindow
onRoleRead = { queried in
    if queried === webContainer { application.attributes[kAXFocusedUIElementAttribute] = coldEditor }
}
expect("reading the web container role wakes web AX without writing attributes", .pasteable)

configure(nil, bundleIdentifier: "com.brave.Browser")
application.attributes[kAXFocusedWindowAttribute] = browserWindow
webContainer.attributes[kAXChildrenAttribute] = [coldEditor] as NSArray
expect("finding an unfocused editor during warmup must not permit paste", .notPasteable)

configure(nil, bundleIdentifier: "com.brave.Browser")
let cyclicWindow = element("AXWindow")
cyclicWindow.attributes[kAXChildrenAttribute] = [cyclicWindow] as NSArray
application.attributes[kAXFocusedWindowAttribute] = cyclicWindow
expect("browser warmup has a depth budget", .notPasteable)
guard reads < 45 else { fatalError("Browser warmup exceeded its depth budget") }

configure(nil, bundleIdentifier: "com.brave.Browser")
let hugeWindow = element("AXWindow")
hugeWindow.attributes[kAXChildrenAttribute] = (0..<1000).map { _ in element("AXGroup") } as NSArray
application.attributes[kAXFocusedWindowAttribute] = hugeWindow
expect("browser warmup has a node budget", .notPasteable)
guard reads < 215 else { fatalError("Browser warmup exceeded its node budget") }

configure(nil, bundleIdentifier: "com.brave.Browser")
application.attributes[kAXFocusedWindowAttribute] = hugeWindow
ProcessInfo.processInfo.step = 0.1
expect("browser warmup has a time budget", .notPasteable)
guard reads < 15 else { fatalError("Browser warmup exceeded its time budget") }

for bundle in ["com.apple.finder", "com.example.editor"] {
    let selectionOnly = element("AXTextField")
    selectionOnly.writable.insert(kAXSelectedTextRangeAttribute)
    configure(selectionOnly, bundleIdentifier: bundle)
    expect("range settability outside a browser is not editability",
           bundle == "com.apple.finder" ? .notPasteable : .unknown)
    guard !isEditableTextElement(selectionOnly) else {
        fatalError("Dictation-specific browser handling changed selection-edit probing")
    }
}

configure(readOnly, menu: pasteMenu(enabled: true))
expect("terminal with read-only AX buffer and enabled Paste", .pasteable)
configure(element("AXWindow"), menu: pasteMenu(enabled: true))
expect("custom app with dormant AX editor", .pasteable)
configure(element("AXGroup"), menu: pasteMenu(enabled: true, command: "v"))
expect("custom editor", .pasteable)
configure(nil, menu: pasteMenu(enabled: true))
expect("unavailable focused field with enabled Paste", .pasteable)
configure(nil)
expect("no AX focus or menu", .unknown)
configure(element("AXWindow"))
expect("dormant Chromium without a menu verdict", .unknown)
configure(element("AXTextField"))
expect("unsupported text attributes", .unknown)
configure(nil)
application.unavailable = true
expect("failed AX queries", .unknown)

for bundle in ["com.mitchellh.ghostty", "com.mitchellh.ghostty.debug"] {
    // Ghostty's terminal exposes a read-only AX buffer. Its Paste shortcut
    // can be absent from the menu even while the terminal accepts Command-V.
    let terminal = element("AXTextArea")
    terminal.attributes.removeValue(forKey: kAXEnabledAttribute)
    configure(terminal, bundleIdentifier: bundle)
    expect("Ghostty terminal without a Paste menu verdict", .pasteable)
    guard !isEditableTextElement(terminal) else { fatalError("Terminal buffer became AX-editable") }
    configure(terminal, menu: pasteMenu(enabled: true, modifiers: 1), bundleIdentifier: bundle)
    expect("Ghostty exposes only Paste Selection with Command-Shift-V", .pasteable)
    configure(terminal, menu: pasteMenu(enabled: false), bundleIdentifier: bundle)
    expect("Ghostty with explicitly disabled Command-V", .notPasteable)
    configure(terminal, bundleIdentifier: bundle)
    trusted = false
    expect("Ghostty without Accessibility trust", .unknown)
    guard reads == 0 else { fatalError("Untrusted Ghostty probe queried AX") }
    configure(terminal, bundleIdentifier: bundle)
    NSWorkspace.shared.frontmostApplication = MockApplication(processIdentifier: 99)
    expect("Ghostty is not the active app", .notPasteable)
    configure(terminal, bundleIdentifier: bundle)
    switchOnRead = true
    expect("Ghostty loses focus during the probe", .notPasteable)
    terminal.attributes[kAXEnabledAttribute] = false as NSNumber
    configure(terminal, bundleIdentifier: bundle)
    expect("disabled Ghostty terminal", .notPasteable)
    terminal.attributes[kAXEnabledAttribute] = true as NSNumber
    terminal.attributes[kAXSubroleAttribute] = "AXSecureTextField" as NSString
    expect("secure Ghostty field", .notPasteable)
    configure(readOnly, bundleIdentifier: bundle)
    expect("explicitly non-editable Ghostty field", .notPasteable)
    terminal.unavailable = true
    configure(terminal, bundleIdentifier: bundle)
    expect("Ghostty with unavailable focused attributes", .unknown)
    configure(nil, bundleIdentifier: bundle)
    expect("Ghostty without a focused terminal", .unknown)
    for role in ["AXWindow", "AXGroup", "AXTextField"] {
        configure(element(role), bundleIdentifier: bundle)
        expect("Ghostty non-terminal focus", .unknown)
    }
    configure(element("AXButton"), bundleIdentifier: bundle)
    expect("Ghostty button focus", .notPasteable)
}
for bundle in ["com.example.editor", "com.mitchellh.ghostty.other"] {
    configure(element("AXTextArea"), bundleIdentifier: bundle)
    expect("other apps still need a writable field or Paste menu", .unknown)
}

for modifiers in [1, 2, 4, 8] {
    configure(element("AXWindow"), menu: pasteMenu(enabled: true, modifiers: modifiers))
    expect("nonstandard Paste modifiers", .unknown)
}
configure(element("AXWindow"), menu: pasteMenu(enabled: true, command: "C"))
expect("copy is not Paste", .unknown)
let missingModifiers = pasteMenu(enabled: true)
let edit = (missingModifiers.attributes[kAXChildrenAttribute] as! [AXUIElement])[0]
let submenu = (edit.attributes[kAXChildrenAttribute] as! [AXUIElement])[0]
let item = (submenu.attributes[kAXChildrenAttribute] as! [AXUIElement])[0]
item.attributes.removeValue(forKey: kAXMenuItemCmdModifiersAttribute)
configure(element("AXWindow"), menu: missingModifiers)
expect("missing shortcut modifiers", .unknown)

configure(element("AXTextField", writable: true))
trusted = false
expect("missing Accessibility trust", .unknown)
guard reads == 0 else { fatalError("Untrusted helper queried AX") }
configure(element("AXTextField", writable: true))
NSWorkspace.shared.frontmostApplication = MockApplication(processIdentifier: 99)
expect("different active app", .notPasteable)
guard reads == 0 else { fatalError("Inactive target queried AX") }
configure(element("AXTextField", writable: true))
switchOnRead = true
expect("focus changed during AX probe", .notPasteable)

let hugeMenu = element("AXMenuBar")
hugeMenu.attributes[kAXChildrenAttribute] = (0..<1000).map { _ in element("AXMenuItem") } as NSArray
configure(nil, menu: hugeMenu)
expect("bounded menu traversal", .unknown)
guard reads < 450 else { fatalError("Menu node budget exceeded") }
configure(nil, menu: pasteMenu(enabled: true))
ProcessInfo.processInfo.step = 0.2
expect("menu deadline", .unknown)
guard reads < 10 else { fatalError("Menu deadline exceeded") }
print("paste target scenarios passed")
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
    assert.match(result.stdout, /paste target scenarios passed/);
  }
);
