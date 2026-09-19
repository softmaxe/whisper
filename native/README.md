# Native macOS application

This Swift application develops alongside the existing Electron application. It
targets Apple Silicon and macOS 27. It implements Settings, button-started
Dictation, text cleanup, History, Dictionary, correction learning, and Right
Command Hold-to-talk and Hands-free with Automatic paste. Remaining workflows
follow through specification #35.

Use Xcode 27 per command without changing the global Command Line Tools selection:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer npm run native:build
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer npm run native:test
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer npm run native:pack
```

Use Node.js 24 and `npm ci` for repository scripts. The Swift package has no external
dependencies. Node.js is only a build tool; the app contains Swift code, system
frameworks, app resources, and no Web runtime. `native:pack` and `pack:release` require the original
[signing identity](../docs/macos-signing.md). `pack` makes a separate, credential-free
ad-hoc development build. Packaging verifies identity, architecture, minimum OS,
runtime dependencies and extracted archive contents. Release output is
`dist/native-arm64/Whisper.app` plus a versioned ZIP and checksum in `release/`.
Development output is `dist/native-development-arm64/Whisper.app` with a
`-development.zip` archive. Neither mode installs or launches the app.

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

Home starts one selected-microphone request through `WhisperApplication.send`.
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
Audio retention and retry are described below; Insights accounting remains a separate workflow.
The application termination path awaits `flushHistoryWrites` before quitting.
`HistoryView` and `HistoryPrivacyView` are reusable content views; the temporary
flat Settings shell is not the final navigation design.

Workflow tests write and reopen actual isolated databases, exercise 1,000 entries,
page and search results, English/Chinese date labels, copy, deletion, disabled
History, corrupted storage and cancelled server responses. UI compilation is
verified separately from the remaining physical and English/Chinese visual review.

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
`native:test` runs this process-wide memory assertion in a separate test process
after all other tests, so concurrent large Upload fixtures cannot affect its RSS.
This is a regression check, not a hardware latency or energy benchmark.
Cancellation and source failures still release the affected request, and old
timers, acquisitions or ASR responses cannot stop a new one.

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

## Packaged resources and helpers

Production Swift resource bundles are copied from the release product directory;
declared test-target bundles are excluded using Swift package metadata.
`native/packaging.json` explicitly declares other resources, helper build scripts,
helper destinations and redistribution licenses. Paths are relative to the repo
and the app's Resources directory. Existing helpers must retain their `bin/` path,
framework identifiers and executable names because those determine their stable
signing identifiers. Every helper entry requires its license files. Third-party
FFmpeg or other native decoders can be build-time dependencies, but package only
the arm64 binary and required resources/licenses, never `node_modules` or Node.js.
All bundled Mach-O code is detected and signed, including helper code added by
later feature tickets. Native verification rejects build-machine library paths.

`npm run dev` packages and launches a development bundle with its usage descriptions
and an isolated `native/.development-profile`. This is an ad-hoc development
identity; use the original signed package for permission-retention acceptance.
Remove only that disposable development data with
`rm -r native/.development-profile` when it is no longer needed.

The codec build requires `pkgconf` (`brew install pkgconf`). Packaging builds a
pinned FFmpeg and LAME from SHA-256-verified official source archives. External
library autodetection, nonfree, GPL and version3 components are disabled; the
standalone arm64 decoder links LAME statically. The bundle includes complete
corresponding source archives, upstream license texts and its rebuild script in
`Contents/Resources/licenses/ffmpeg`. The npm `ffmpeg-static` binary is never
packaged because its current macOS build contains nonfree components.

Media control packages the existing MediaRemoteAdapter framework and Perl bridge
under their original `Contents/Resources/bin` paths with the upstream license.

## Desktop preferences and lifecycle

General settings persist appearance, readiness/stop sounds, media pause, idle pill
visibility, auto-hide, placement, menu bar visibility, and background startup.
Login-item registration remains a system setting through `SMAppService.mainApp`;
its current enabled/approval state is read when the app starts and becomes active.
A registration requiring approval links to System Settings instead of reporting a
successful enabled state without explanation.

`DesktopEffects` controls external sound, media and login-item operations. The
library defaults to `InertDesktopEffects`. Only the executable explicitly creates
`NativeDesktopEffects`; deterministic fixtures inject `ControlledDesktopEffects`.
Tests must never construct the production effects adapter or launch the executable
for these checks. No test should play a cue, control real media, register a login
item, open System Settings, or request permissions.

Readiness effects start only after both accepted gestures and the first valid
audio frame. Repeated frames and provisional taps cannot replay the cue. Normal
stop plays the stop cue; cancellation and failures release media ownership without
a stop cue. Media pause/resume operations are serialized so a late pause after
cancellation is balanced before a retry can acquire its own pause. Disabling media
pause during recording also returns the pause already owned by that request.

The production adapter reuses the existing `MediaRemoteAdapter.framework` and
`mediaremote-adapter.pl` from `Contents/Resources/bin`, plus the existing media-key
fallback behavior. Probe output is bounded and never logged. Helper deadlines
and inherited pipe closure cannot leave the media queue waiting indefinitely.
The app's About menu links to bundled third-party licenses.

An AppKit application delegate owns the main window, shortcut monitor and
nonactivating Recording pill independently. Closing the main window keeps
Dictation available and hides the Dock icon. Reopening through Finder or the menu
bar restores the main window. A login launch or Start minimized preference keeps
it in the background. Appearance changes apply only to Whisper, not system-wide.
The pill follows its configured bottom-left/center/bottom-right placement; active
Dictation, errors and copy recovery remain visible even when idle auto-hide is on.
Success/cancellation feedback lasts 500 ms before the idle visibility policy takes
over. Pill updates never activate the main window or make the pill key/main.

`prepareForTermination()` cancels capture, processing and prompt tests, stops
correction observation, awaits owned media cleanup, and flushes accepted History
writes before the delegate replies to AppKit's termination request.

Deterministic tests cover preference persistence, system-operation requests through
controlled adapters, delayed pause/cancel/retry ordering, preparation and provisional
capture, hold/hands-free feedback, error/copy recovery, and auto-hide timers. They
do not prove native focus, media compatibility, sounds, menu bar layout, login
registration, or physical permission behavior. Check those only in a coordinated
signed-app smoke test with the user awake.

Desktop integration keeps correction notifications visible even with idle pill
visibility disabled. The persistent lifecycle owns pill/shortcut updates after the
main window closes. `prepareForTermination()` cancels Dictation and prompt tests,
stops correction observation, waits for owned media release, and flushes accepted
History writes before AppKit replies that termination may proceed. Controlled
integration tests verify this sequence against real SQLite storage for both hold
and hands-free requests, without playing sound or changing system media/login state.

The Third-party licenses action opens a directory containing FFmpeg/LAME sources
and notices, MediaRemoteAdapter BSD 3-Clause, JetBrains Mono OFL, and OpenCC MIT/Apache-2.0
notices. Their original resource locations remain intact.

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

The AppKit lifecycle retains the shortcut monitor independently of Settings views
and updates its configuration when bindings, capture mode, or Dictation phase
change. Leaving the application ends local capture; termination never rearms the
monitor while waiting for cleanup. A request snapshots all candidate bindings, so
later saved chords cannot adopt its provisional modifier capture. Repeated native
configuration refresh also preserves a newer user Globe choice when the action
was already disabled before Whisper started using it.

## Retained audio and History retry

Privacy & Data keeps the existing retention defaults: History enabled, audio for
30 days, transcripts forever, and cancelled recordings off. Zero audio days
stops retaining new audio without deleting existing files; Delete saved audio
removes those files while preserving transcripts. Transcript expiry uses entry
creation time and removes associated audio. Audio expiry uses file modification
time and keeps the text. Saved preferences load before startup cleanup, followed
by a six-hour sweep and cleanup when retention preferences change.

The capture owner releases the physical input before finalizing AAC. Processing
and persistence share the finalized temporary file's lifetime, then History saves
audio in the native profile's private `Audio` directory. A failed audio copy keeps
successful transcript text. Failed Dictation can retain its audio for recovery.
Ordinary cancellation requires History, audio retention, the cancelled-recording
preference, and at least one second of valid audio. Rejected shortcut gestures
always leave neither audio nor a History row. An unrecoverable empty discarded
row is never saved after an audio-copy failure.

History exposes playback, Show in Finder, and retry only for available retained
Dictation audio. Retry snapshots the current ASR model, credential and language,
then optional cleanup and Chinese conversion. The legacy self-hosted retry request
sends no Dictionary or Chinese prompt bias; cleanup still receives the current
Dictionary and Snippet triggers as vocabulary. Retry never expands Snippets or
automatically pastes. It updates the same entry while preserving original
occurrence, creation time, source and audio. Failure and cancellation keep the
prior entry. The storage actor uses an existing-row update, never an upsert, so
late retry output cannot revive a deleted or expired recording.

Playback and Finder are external adapters in workflow tests. The tests use real
temporary AAC, retained files and SQLite profiles, including save/reopen/retry,
expiry, missing files, copy failure, cancellation and late responses. No test
plays audio or opens Finder. Actual playback and bilingual visual acceptance
remain separate checks with the user present.

## Local Insights

Insights retains the four existing metrics: raw words spoken, weighted words per
minute, Dictations and current streak with longest streak. Its activity grid
covers the current calendar month and previous five months. Stored occurrence
and local-date keys come from recording start, independently of server latency.
English uses whitespace-delimited words; CJK text uses ICU word-like segments,
matching the existing `Intl.Segmenter("und")` behavior. A small static C bridge
uses the system `libicucore` API without adding a helper or bundled runtime.
Apple's higher-level tokenizers produce different counts for mixed text, so the
workflow suite compares ICU output against retained JavaScript examples. See the
[ICU word-boundary API](https://unicode-org.github.io/icu-docs/apidoc/released/icu4c/ubrk_8h.html).

`insights_events` holds only IDs, timestamps, local day, word count and optional
spoken duration in the same SQLite store as History. Live accounting happens
independently before the History write; failure in either table leaves the other
outcome and current text usable. Reconciliation fills missing eligible completed
native History events without replacing an already-counted event. Upload, failed,
discarded, zero-word and new History-disabled Dictations are excluded. A recovered
existing Dictation keeps its original event identity and date.

Individual History deletion preserves counts. Clear History removes both tables
in one transaction and keeps its persisted occurrence cutoff, so late old results
cannot revive statistics. The retention integration deletes counter rows by the
same creation-time cutoff, including counters whose History rows were individually
deleted, and rejects late writes older than that persisted retention cutoff.

## Single-file Upload

Upload accepts the existing 33 audio/video extensions. MP3, WAV, M4A, WebM, Ogg,
OGA, FLAC, AAC and Opus are sent unchanged with their existing MIME types. Other
accepted containers pass through the packaged FFmpeg executable as mono 16 kHz,
64 kbit/s MP3 with video removed. The runtime resolves only
`Contents/Resources/bin/ffmpeg`; it does not launch Node.js or search Homebrew/PATH
for an alternate converter. Native packaging supplies the redistributable media
build and its license/source materials.

Selection, start, cancellation, result copy and retry use `WhisperApplication.send`
and `state.upload`. Progress describes preparation or transcription without
inventing a percentage. The job snapshots its ASR configuration, credential and
language at start. Upload sends language only, with no Dictionary or Chinese
prompt bias, and accepts HTTP 200 as the legacy file-transcription route does.
It returns raw text without cleanup, Chinese conversion, Snippets or Automatic
paste. Its History source is `upload`, with no retained audio or Dictation usage
record. Disabling History while a request is pending prevents its later save;
current text remains copyable when persistence or clipboard delivery fails.

Multipart bodies and conversion output live in separate, owned temporary
folders with owner-only access. The source file remains untouched, including
when its parent directory is read-only. Multipart copying uses bounded chunks
and has no third-party 25 MiB cap. File preparation explicitly leaves the UI
actor with `@concurrent`, following [Swift SE-0461](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0461-async-function-isolation.md).
Cancellation releases the visible job immediately, cancels the network operation
or conversion process, and rejects late work by request identity before History.
`cancelUploadsAndWait` closes Upload to new jobs, waits for active and previously
cancelled work to remove its temporary files, then waits for accepted History
writes. `prepareForTermination()` awaits this operation before exit, after blocking
new commands. Its final History/Insights flush also joins accepted retained-audio
operations. Upload results remain excluded from Dictation Insights and audio retry.

Upload tests run the real application, file preparation, multipart transport and
isolated History store with generated media. All 33 extensions have positive
fixtures, including a video track, genuine AIFC, synthetic AMR frames and a
CRC-checked APE silence frame. Their binary layouts follow FFmpeg's
[AMR demuxer](https://github.com/FFmpeg/FFmpeg/blob/n6.0/libavformat/amr.c),
[APE demuxer](https://github.com/FFmpeg/FFmpeg/blob/n6.0/libavformat/ape.c) and
[APE decoder](https://github.com/FFmpeg/FFmpeg/blob/n6.0/libavcodec/apedec.c).
No downloaded or private speech fixture is required. The tests also cover
read-only sources, raw output with all live transforms configured, server and
conversion failures, clipboard/History failures, large files, late cancellation,
and awaited process/network cleanup during termination.

For native conversion tests, build the native media tools first or set
`WHISPER_TEST_FFMPEG` to the generated FFmpeg executable. Test fixture generation
prefers that override, then `resources/bin/ffmpeg`; the legacy npm binary is a
local-test fallback only. Shipping conversion uses the packaged native helper.
English/Chinese visual and picker checks remain part of full interface acceptance.
