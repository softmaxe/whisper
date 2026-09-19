# Native macOS application

This Swift application develops alongside the existing Electron application. It
targets Apple Silicon and macOS 27. It implements Settings, button-started
Dictation, text cleanup, and Right Command Hold-to-talk with Automatic paste;
the remaining workflows and pages follow through specification #35.

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
after success, failure, or cancellation. Successful final Dictation results are
saved through History. Button-started Dictation keeps its copyable result.

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

## Text cleanup

Settings includes a separate Text cleanup server, model, optional Keychain API key,
thinking toggle, temperature and token limit. The default prompt preserves the
existing English and Chinese text; the prompt page previews, edits, resets, and
tests a draft without saving it. Prompt tests use the saved server configuration.
Neither credentials nor server replies are logged.

Dictation runs cleanup after ASR and preserves the raw transcript separately from
the final copyable text. Invalid, empty, truncated, failed, or timed-out cleanup
keeps the raw result and displays a localized explanation. Cancellation and app
teardown cancel processing immediately and suppress late responses. The cleanup
request uses a 30-second deadline across parameter fallback attempts. Transient
network, 408, 429, and 5xx failures may retry three times with 1, 2, and 4-second
backoff; a deadline expiry never retries. Same-origin redirects preserve the
explicit cleanup credential; cross-origin redirects are rejected.

Cleanup workflow tests use the public application commands, isolated native
profiles and temporary file Keychains, real serialized HTTP bodies, and controlled
server/clock boundaries. Loopback tests also exercise the actual URLSession JSON
transport and redirects. These checks do not establish production-server behavior,
physical Dictation or paste behavior, or native visual acceptance.

## History

Home displays date-grouped History with processed and original text, separate copy
commands, individual deletion and clear-all confirmation. Command-K searches at
most five matches, supports arrow-key selection and Return, and opens the full
entry. History and search query real SQLite storage in the isolated native
profile; they return bounded pages rather than decoding the entire archive on
the UI thread. Search includes original and final text, uses Unicode lowercase
normalization, and treats wildcard characters literally.

The History setting defaults to enabled and lives in Privacy & Data. Turning it
off does not erase existing entries or prevent copying the current Dictation.
Saving failure leaves the successful current result usable and reports a separate
History error. The single final Dictation completion point schedules persistence
after all text transforms. It captures the recording's original wall-clock time,
local date, stable request identity, input duration and ASR model snapshot. Raw
ASR text remains separate from the final pipeline text. Cancelled requests do not
save late output.

`HistoryStore` owns a system SQLite connection on an actor. It stores one row per
request, updates content idempotently, and preserves original occurrence and
source on update. Dates use Foundation reference intervals to preserve `Date`
precision through SQLite. The profile directory and database use owner-only
permissions. Unknown database versions, malformed databases and unreadable
settings fail without replacing their saved files. The native app does not read
or migrate the legacy History database.

Later Upload and retention workflows can use the shared `recordHistory` operation
and store methods. Individual deletion does not advance the device clear cutoff;
clear-all persists a cutoff for later Insights to reject old in-flight events.
This slice does not implement audio retention, retry or Insights accounting.
The application termination path awaits `flushHistoryWrites` before quitting.
`HistoryView` and `HistoryPrivacyView` are reusable content views; the temporary
flat Settings shell is not the final navigation design.

Workflow tests write and reopen actual isolated databases, exercise 1,000 entries,
page and search results, English/Chinese date labels, copy, deletion, disabled
History, corrupted storage and cancelled server responses. UI compilation is
verified separately from the remaining physical and English/Chinese visual review.
