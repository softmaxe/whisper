# Native macOS application

This Swift application develops alongside the existing Electron application. It
targets Apple Silicon and macOS 27. It implements Settings, button-started
Dictation, and Right Command Hold-to-talk with Automatic paste; the remaining
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

## Microphone selection

General Settings saves Auto, System default, Built-in microphone, or Specific
microphone. Auto prefers the built-in input with an open or unknown lid. With a
closed lid it prefers Continuity/iPhone, then another external input. If that
category is unavailable, Auto uses the current physical system default. Built-in
and Specific modes do not fall back. An unavailable saved UID remains saved and
visible as unavailable; an identically named replacement does not inherit it.

The selection policy receives a metadata snapshot through
`MicrophoneProvider.inputSnapshot()`. Native snapshots combine AVCapture device
UIDs with exact CoreAudio transport/default-device metadata and the IOKit lid
property. They never open capture. The application resolves one UID when accepting
a request. Later preference, default-input, device-list, and lid changes apply
only to new requests. The selected input's failure or disconnect ends its request;
reconnection cannot revive it. Refreshing the Settings list is metadata-only.
Device categories and selection never depend on a display-name match.

Workflow tests preserve and reopen real native settings, exercise all four modes,
Auto ordering and fallback, same-named stale IDs, preference changes during
acquisition and recording, external input failure, delayed frames, timeout,
disconnection, reconnection, and full recording-to-copyable-result processing.
These controlled cases do not replace built-in and wireless iPhone acceptance on
the signed application.

## Right Command Hold-to-talk

The first physical Right Command press starts provisional capture. A standalone
hold becomes Dictation; release submits the recording. A short tap or an
intervening Command combination always discards provisional audio without ASR.
The initial hold threshold is the legacy 150 ms value. It is provisional until
physical keyboard acceptance measures it. Double-tap Hands-free and configurable
bindings follow in #40 and #41.

The session event tap reads each modifier's physical key state rather than the
aggregate Command flag, so Left Command cannot masquerade as a Right Command
release. It ignores key repeat and the app's tagged synthetic paste events.
Ordinary Command combinations pass through. Global Esc cancels preparation,
recording, ASR and pending delivery. Late device, server and Accessibility probe
completions cannot revive the cancelled request. Enable Accessibility through
General settings if the global shortcut is unavailable.

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

## Transcription language and Chinese text

General settings keep transcription language separate from the interface language.
The native UI uses the existing language registry, with Simplified/Traditional
Chinese and auto-detection choices. Both settings persist in the native profile;
earlier native profiles default to auto-detection and keeping the transcript's
original script.

ASR receives the base language code, including `zh` for both Chinese choices, and
omits the language field for auto-detection. Explicit Chinese choices prepend the
existing script prompt bias; an auto-detection script preference never biases ASR.
The active request snapshots these choices when recording is submitted.

After ASR or optional cleanup, the native converter applies the exact
`opencc-js` 1.4.1 `twp -> cn` or `cn -> twp` pipeline before result publication.
It preserves raw ASR text. Auto-detection only converts text with the existing
Chinese-specific signals and no Kana or Hangul. Upload remains outside this
conversion path. Later Snippet expansion belongs after conversion.

The Swift converter loads and caches its tries on a separate actor when first
needed. It uses twelve pinned, unmodified OpenCC dictionary files, compatibility
normalization, fixed phrase segmentation, priority within dictionary groups, and
Unicode scalar matching. Identity phrases and ideographic-description sequences
retain the existing behavior. Cancellation checks suppress ended requests; a
resource failure preserves usable transcription with a visible warning.

Dictionary hashes, upstream revisions, and MIT/Apache-2.0 notices are bundled in
`Sources/WhisperCore/Resources/OpenCC`. The application has no JavaScript runtime.
`test/native/chineseConversionOracle.test.js` checks source-data and fixture parity
against the existing development dependency; the Swift workflow tests replay
those fixtures through actual ASR requests, publication, and Copy commands.

## Correction learning

The default-enabled **Learn from corrections** preference adds words only after
confirmed Automatic paste. The app captures the exact Accessibility element and
selection before insertion. After 500 ms it confirms the inserted text in that
range, then watches the same field for 30 seconds using AXObserver with a 500 ms
polling fallback. It waits 1500 ms after the latest edit before learning. A focus
change, selection outside the inserted region, surrounding-content change,
disabled preference, new Dictation, or teardown stops the observation. Copy
recovery and clipboard-only delivery do not start learning.

The matcher preserves the existing word-level LCS, common-word, minimum-length,
case, edit-distance and rewrite filters. It runs off the UI thread and bounds
quadratic comparisons to four million cells. Oversized comparisons skip learning;
recording and transcription remain unlimited. Only newly learned words and their
source metadata are persisted. Field snapshots remain in request memory and are
never logged or saved. Undo removes only the entries from that notification that
still have learned provenance, preserving words later promoted manually.

The adapter reuses `resources/macos-text-monitor.swift` behavior inside the main
signed application. It adds no helper executable or permission identity. The
workflow tests drive actual shortcut capture, AAC/ASR requests, automatic paste,
controlled field observations, Dictionary persistence and the next ASR prompt.
Actual Accessibility compatibility and the learned-word notification still need
coordinated physical and English/Chinese visual acceptance.
