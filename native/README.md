# Native macOS application

This Swift application develops alongside the existing Electron application. It
targets Apple Silicon and macOS 27. It implements Settings, button-started
Dictation, and Right Command Hold-to-talk and Hands-free with Automatic paste; the remaining
workflows and pages follow through specification #35.

Use Xcode 27 per command without changing the global Command Line Tools selection:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer npm run native:build
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer npm run native:test
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer npm run native:pack
```

Use Node.js 24 and `npm ci` for repository scripts. The Swift package has no external
dependencies. Node.js is only a build tool; the app contains Swift code, system
frameworks, app resources, and no Web runtime. Packaging requires the original
[signing identity](../docs/macos-signing.md) and verifies the pinned certificate,
bundle identifier, architecture, minimum OS, and runtime dependencies. It creates
`dist/native-arm64/Whisper.app` without installing or launching it.

The native app uses `~/Library/Application Support/WhisperNative` and a separate
Keychain service, `local.whisper.desktop.native`. It never reads the legacy profile
or imports its credentials. ASR secrets are stored only in Keychain, while the
profile holds an opaque account reference. A failed credential write does not
replace the saved settings, and an unreadable profile cannot be overwritten by a
save. The app supports `--profile /absolute/path/to/isolated-profile` for disposable
manual tests. This option does not change the default profile.

`WhisperApplication.send` is the shared command boundary for UI and native adapters.
Views observe its `state`. Tests exercise these same commands and reopen actual
temporary profiles. Credential persistence tests use real, temporary file
Keychains, deleted with their fixture directories. They do not use the login
Keychain or private user data. The deprecated file-Keychain creation API is confined
to test setup so each test can isolate its storage; application access uses SecItem.

The Settings structure, English/Chinese copy, and endpoint policy adapt the existing
implementation and OpenWhispr at `6d56d75e7e13ec47009e573e9ff4cded0d0ccc61`:
`src/components/SelfHostedPanel.tsx`, `src/components/SettingsPage.tsx`, and
`src/utils/urlUtils.ts`. The Keychain contract follows `src/helpers/secretCrypto.js`
without its Electron encryption dependency. Native text uses the existing design's
JetBrains Mono and the system Chinese fallback. The bundled TTF faces are from the
[same JetBrains Mono revision](https://github.com/JetBrains/JetBrainsMono/tree/19371302b95d218af43299bce79ddbddd0bc364d)
as the legacy webfonts, with their OFL-1.1 license retained in the resource bundle.

A successful package or deterministic test does not establish hardware microphone,
Accessibility, or Keychain permission retention after a Homebrew upgrade. Follow
[release smoke checks](../test/README.md#release-smoke-check) before making those
claims. Do not replace the daily-use app during this staged implementation.

## Button-started Dictation

Home starts one built-in microphone request through `WhisperApplication.send`.
The source adapter pins its physical UID and owns an `AVCaptureSession` with a
private control queue. A cancelled or failed request cannot revive when a late
open, frame, or server response arrives. An uninterruptible AVFoundation call may
finish before physical release; logical cancellation and a new request remain
independent. Stopping before the first usable frame discards the request.

The first nonempty Float32 source frame is written before Recording readiness,
including silent audio. The 10-second first-frame deadline starts after device
acquisition. Each frame is encoded incrementally into an AAC M4A file in the
native profile's temporary request directory. The file writer and multipart
preparation run off the main thread, and retained audio memory does not grow with
recording duration. Hardware is released before file finalization and ASR.

The ASR client uploads a disk-backed multipart body to the configured base plus
`/audio/transcriptions`, preserving explicit `/v1` and query parameters. It sends
only the separately entered native ASR credential. It has no client inference
deadline, matching the existing self-hosted fetch contract, and supports explicit
cancellation. Same-origin redirects support endpoint path normalization.
Cross-origin redirects are rejected so credentials and audio stay with the
configured server. Temporary capture and upload files are deleted
after success, failure, or cancellation. Button-started Dictation keeps its
copyable result. History follows in a later slice.

`MicrophoneProvider`, `MicrophoneSession`, `WorkflowClock`, `FileHTTPTransport`,
and `TextClipboard` are external adapter boundaries. Application tests use the
real owner, readiness, AAC writer, multipart builder, response parsing, and
isolated profile. `WorkflowSupport.swift` contains shared fixtures for subsequent
workflow tests. Real loopback HTTP tests exercise URLSession uploads and redirect
handling without opening hardware or using real speech. Native UI uses a
nonactivating AppKit Recording pill with the current 98 × 40 compact footprint.

## Right Command Hold-to-talk

The first physical Right Command press starts provisional capture. A standalone
hold becomes Dictation; release submits the recording. A short tap or an
intervening Command combination always discards provisional audio without ASR.
The initial hold threshold is the legacy 150 ms value. It is provisional until
physical keyboard acceptance measures it. Hotkeys settings applies the same
gestures to the supported keyboard and mouse choices.

The session event tap reads each modifier's physical key state rather than the
aggregate Command flag, so Left Command cannot masquerade as a Right Command
release. It ignores key repeat and the app's tagged synthetic paste events.
Ordinary Command combinations pass through. Global Esc cancels preparation,
recording, ASR and pending delivery. Late device, server and Accessibility probe
completions cannot revive the cancelled request. Enable Accessibility through
Hotkeys settings if the global shortcut is unavailable.

Hold-to-talk captures its Target app during startup and retains that PID through
processing, preserving the legacy startup target contract. A PID does not lock a
particular text field. Delivery confirms the app is frontmost and the target is
editable before posting a keyboard-layout-aware Command-V. It waits up to 500 ms
for physically held modifiers to clear, then uses copy recovery if unsafe. It
never releases the user's keys. The target probe and layout resolver adapt the
existing `resources/macos-text-monitor.swift` and `macos-fast-paste.swift`.

General settings persists Automatic paste and keep-in-clipboard independently.
After successful paste, prior clipboard items and all available formats restore
after 450 ms only if the app still owns that pasteboard revision. Subsequent
delivery waits for that restoration. Failure keeps text available in a
nonactivating copy-recovery panel; a failed clipboard write is not reported as a
successful copy. `AutomaticPasteSystem` controls only external focus, key and
pasteboard effects in tests; the real application owns the gesture and delivery
sequence.

Deterministic tests cover actual AAC and multipart output, startup Target app,
left/right overlap, repeat, rejected short taps, Command combinations, Esc and
late completion, clipboard preferences, recovery and restoration races. They do
not establish physical key delivery, Accessibility permission behavior, actual
insertion into target editors, or native English/Chinese visual acceptance.
Those checks remain pending against a signed disposable-profile build, without
replacing the installed daily-use app or resetting system permissions.

## Hands-free Dictation

Double-tap Right Command from idle to keep recording after release. A later
standalone press finishes only when released. Command combinations, including
app switching, preserve their ordinary action and keep Hands-free recording.
The Target app is captured at submission and kept fixed during processing.
Hold-to-talk continues to use its startup target.

The provisional double-tap window is 300 ms from the first short release to the
second press. Both taps must be shorter than the 150 ms hold threshold; a held
second press follows Hold-to-talk instead. The first tap keeps the same
provisional capture during that window to preserve opening audio. When the
window expires, all provisional audio is discarded without ASR or History.
Gesture recognition never substitutes for first-source-frame readiness. These
thresholds are implementation defaults, pending physical keyboard measurements.

There is no recording duration ceiling. A deterministic workflow test advances
the clock by two hours while recording, writes over thirteen minutes of actual
synthetic audio in bounded frames, then decodes the resulting file incrementally
to verify its opening/ending samples and complete frame count. A generous 96 MiB
process-memory growth envelope detects retaining the full 147 MiB PCM fixture.
This is a regression check, not a hardware latency or energy benchmark.
Cancellation and source failures still release the affected request, and old
timers, acquisitions or ASR responses cannot stop a new one.

## Configured shortcuts

Hotkeys settings supports multiple bindings, with Right Command as the fresh
default. Right-side modifiers, modifier-plus-key combinations, Globe/Fn,
supported special keys and Mouse Button 4/5 share the same hold, double-tap and
standalone-stop workflow. The triggering binding remains fixed until its
Dictation ends; changes apply to the next request. The native capture field
pauses Dictation dispatch, validates each captured choice, and persists a valid
list atomically. Failed choices keep the previous working configuration.

Validation ports the existing macOS reserved list and alias normalization. It
rejects bare letters/numbers, modifier-only chords, Fn combinations, keyboard
and mouse combinations, conflicting left/right modifiers, duplicate/overlapping
bindings and more than three keys. Standalone Esc is additionally reserved for
the new global cancellation contract. A nonreserved modifier-plus-Esc binding
remains supported; standalone Esc and Esc during another control's Dictation
still cancel. The settings error explains the reserved key in English/Chinese.

The event adapter consumes only configured base-key or side-mouse presses and
their matching releases. It passes ordinary modifier events and unconfigured
keys through, ignores repeat as a gesture, and reads physical left/right state.
It also recognizes extended function-key characters and supported macOS media
events. Actual availability still depends on the keyboard and system event
delivery; deterministic fixtures do not establish hardware compatibility.

Globe suppression adapts the existing helper's TIS compatibility entry points
and durable recovery marker in the isolated native profile. It changes the
system Globe action only while a binding, active request or capture field owns
it. Normal quit, SIGTERM/SIGINT, configuration changes and the next launch after
a crash restore the original explicit or default preference unless the user
has chosen a newer value. Failure to write the marker or resolve the TIS entry
points leaves the preference unchanged and shows feedback. Those dynamically
resolved entry points are not a public Apple API guarantee.

Tests reopen real temporary settings and run every input category through the
actual recording/ASR/delivery workflow, including short-tap cleanup. Additional
adapter tests cover event suppression and Globe ownership with controlled
system preferences and real temporary journal files. They never change the
user's Globe action, install an event tap, inject real keys, or request a
permission prompt. Physical controls and the capture UI still need signed-build
acceptance on the target Mac.
