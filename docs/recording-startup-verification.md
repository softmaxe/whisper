# External microphone startup verification

Verification for [#28](https://github.com/softmaxe/whisper/issues/28) against the
[parent specification](https://github.com/softmaxe/whisper/issues/22).
Automated evidence and hardware observations are reported separately. Hardware
validation is pending; this report does not establish an iPhone speedup or full
acceptance of #28.

## Revisions and environment

- Behavior reference: `b10d49a6d628cc6b715e3ddc2af468cab0f5c4da`.
- Instrumented baseline: `043d6bea0cb5a34f45cbfc38dee38760c0de1e9a`.
- Candidate application code: `279bbb04dfdb8816aa06ac18c43d5665b7830664`,
  containing #24, #25, #26, and #27. This verification adds test assertions and
  documentation only. Use the commit containing this report for those assertions.
- Upstream checked on 2026-09-19: OpenWhispr
  `6d56d75e7e13ec47009e573e9ff4cded0d0ccc61`, unchanged from the implementation
  review. The existing prepared-capture handoff and renderer harness are reused.
- Host: MacBook Pro `Mac14,10`, Apple M2 Pro, 32 GB, arm64,
  macOS 27.0 build `26A428`.
- Toolchain: Node.js 24.21.0, Electron 41.10.5, Whisper 1.0.4,
  dependencies installed with `npm ci` using the committed lockfile.
- Available external input reported by the operator: wireless iPhone, iOS 27.
  Exact iOS build and hardware trial settings have not yet been recorded.

## Automated evidence

The primary seam is the real `useAudioRecording` Hook and AudioManager together.
`test/lib/rendererTestHarness.js` replaces media devices, audio delivery, system
IPC, and time. It does not replace device selection, preparation, readiness, or
capture termination. Recorder payloads are synthetic, so preserving their order
does not prove acoustic speech preservation or transcription accuracy.

| Requirement                                                                              | Reproducible coverage                                                                                                                |
| ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Acquisition proceeds while visual frames and Target app capture are pending              | `recordingStartupLifecycle.test.js`: `Dictation acquisition overlaps pending visual frames and target capture`                       |
| Delayed first audio, silence, preparation reuse, and one acquisition                     | Same suite: silent input, preparation/start sharing, and delayed selected input cases                                                |
| No first audio fails and releases capture                                                | Same suite: `no delivered audio fails within ten seconds and releases capture without readiness`                                     |
| Cue on/off, readiness once, late visual frames                                           | Same suite: both `readiness is visible and emitted once with cues` cases hold visual callbacks until after first audio and readiness |
| Cancel/stop/teardown, late results, independent rapid retry                              | Both lifecycle suites: acquisition cancellation, stop/retry trace, teardown, and first-audio waiting cases                           |
| Selected-device rejection, permission denial, stale ID, muted/ended input, disappearance | Startup suite: selected input failure matrix and stale identifier cases; no healthy alternative is opened                            |
| Active disconnect and no replacement                                                     | Startup suite: selected microphone disconnect and active input failure cases                                                         |
| Stable source during lid, default, device-list, and preference changes                   | Startup suite: selection-mode matrix and next-request selection cases                                                                |
| Release on every exit, including legacy nonzero hold                                     | `useAudioRecordingCaptureLifecycle.test.js`: normal stop, failure, cancellation, teardown, and retired preference cases              |
| Both shortcuts and panel start, built-in/external input, Target app and Automatic paste  | Capture suite: six input/start-mode combinations retain opening audio and the refreshed Target app with visual frames pending        |
| Pre-roll and speech after readiness preserved                                            | Capture suite: `prepared audio and speech immediately after readiness reach transcription in order`                                  |
| Retention unchanged and failure cannot submit audio                                      | Capture suite: discarded History and confirmed input failure cases                                                                   |
| Correlated diagnostics, absent stages, late identities, no sensitive timing fields       | Startup suite: rejected request, cancellation/retry, expired preparation, and overlapping request cases                              |

Reproduce from the verification commit with Node.js 24 on PATH:

```sh
npm ci
npm run typecheck
ELECTRON_RUN_AS_NODE=1 REQUIRE_DB_TESTS=1 node_modules/.bin/electron \
  --import tsx --test test/helpers/recordingStartupLifecycle.test.js \
  test/helpers/useAudioRecordingCaptureLifecycle.test.js
npm run quality-check
```

The initial two-suite run passed 60/60 tests with zero skipped. After strengthening
the cue cases, the startup suite passed 34/34 with zero skipped. TypeScript passed.
`npm run quality-check` passed: lint, TypeScript, translations, and all 1033 tests,
with zero failed, cancelled, or skipped tests. SQLite tests ran through Electron
with `REQUIRE_DB_TESTS=1`; no database tests were bypassed. ESLint reported three
existing warnings in unchanged component files: one Hook dependency warning and
two Fast Refresh export warnings, with zero errors.

Both revisions also passed `npm run pack:release`, including verification against
the pinned certificate, and the candidate passed `npm run test:signing`. The latter
checks changed app versions and native helpers for certificate-bound identity
continuity. Neither test bundle was launched or installed. These are automated
signature checks, not microphone, Accessibility, Keychain, or upgrade behavior
results. No signing code or release infrastructure changed.

## What the controlled timing shows

Use the stage definitions in the [baseline procedure](recording-startup-baseline.md#read-a-trace).
The existing #26 comparison uses `cf08c19` to isolate the scheduling change;
it is not a replacement for the #23 instrumented hardware baseline above.

| Stage, ms after acceptance | Before #26, controlled evidence           | Combined candidate, controlled evidence |
| -------------------------- | ----------------------------------------- | --------------------------------------- |
| `preparationEntered`       | 20                                        | 20                                      |
| `acquisitionRequested`     | 60, after visual callbacks                | 20, visual callbacks still pending      |
| `acquisitionCompleted`     | 160                                       | 160                                     |
| `firstAudio`               | Not supplied by the historical comparison | 190                                     |
| `readyFeedback`            | 190                                       | 190, after first audio                  |

The candidate assertions reproduce the 20/160/190 ms timestamps. The before
column is the previously recorded #26 evidence, not a fresh hardware run. Removing
the controlled 40 ms visual wait does not imply a 40 ms end-to-end improvement.
The fixture holds device completion and target completion fixed; candidate device
acquisition still takes 140 ms in this synthetic schedule. No measured hardware
device-startup cost is available yet.

## Hardware procedure and results

The operator chose to defer hardware collection after confirming wireless iPhone
availability. All physical checks and all before/after distributions below remain
pending. #28 must remain open for that work.

Use the [comparable trial procedure](recording-startup-baseline.md#collect-comparable-trials)
without changing History or audio-retention settings. Do not overwrite the
installed application, reset permissions, change signing identity, or publish a
release. Build both revisions using the original signing credentials and the
existing `npm run pack:release` command. Run one revision at a time. Record the
actual bundle path, signature verification, and launch method for each. Different
bundle paths are a disclosed environment difference; permission prompts must be
recorded separately and those trials excluded from steady-state latency.

Before baseline collection, turn its optional microphone hold off and confirm
release between trials. Use the same profile, selected physical microphone,
wireless connection, power state, cue preference, shortcut mode, server settings,
and launch method for both builds. Never infer the tested revision from the app
version alone: both are 1.0.4. Quit other Whisper instances before opening the
explicit test bundle with `--log-level=debug`.

Collect at least 20 valid starts in each of these eight cells, waiting 30 seconds
after confirmed release between trials. Record cold launches separately.

| Revision  | Input           | Main window        | Collected / minimum |
| --------- | --------------- | ------------------ | ------------------- |
| Baseline  | Built-in        | Visible            | 0 / 20              |
| Baseline  | Built-in        | Hidden or occluded | 0 / 20              |
| Baseline  | Wireless iPhone | Visible            | 0 / 20              |
| Baseline  | Wireless iPhone | Hidden or occluded | 0 / 20              |
| Candidate | Built-in        | Visible            | 0 / 20              |
| Candidate | Built-in        | Hidden or occluded | 0 / 20              |
| Candidate | Wireless iPhone | Visible            | 0 / 20              |
| Candidate | Wireless iPhone | Hidden or occluded | 0 / 20              |

Keep hidden and occluded conditions distinct if both are used. For every cell,
record total attempts and successful, failed, cancelled, and incomplete counts.
For successful same-device trials report count, median, nearest-rank p90, minimum,
and maximum for every stage and for these intervals:

- Application scheduling: `acquisitionRequested - preparationEntered`.
- Device open: `acquisitionCompleted - acquisitionRequested`.
- First audio after open: `firstAudio - acquisitionCompleted`.
- Feedback after first audio: `readyFeedback - firstAudio`, which can be negative
  in the baseline because readiness did not wait for observed audio.
- Total: `max(firstAudio, readyFeedback)` relative to request acceptance.

Group records by `requestId`, order renderer records by `sequence`, and preserve
capture-attempt boundaries and late events. Missing stages are absent, never zero.
Report baseline device substitutions separately; any candidate substitution is a
failure. Do not publish full debug logs. Extract only `Recording startup` records
and keep anonymous input aliases, stage values, outcomes, and fixed lifecycle
fields. Exclude credentials, private URLs, device labels, transcripts, and audio.

The following operator checks are all pending for both physical inputs:

- Quiet input reaches readiness; enabled audible cue follows readiness; disabled
  cue still gives visual readiness. Compare physical observations with trace
  events: cue scheduling and React state do not prove audible or visible output.
- Speak a fixed phrase immediately after the signal and verify its opening words.
- Normal stop, stop/cancel while connecting, rapid retry, and active disconnection
  release application-owned capture with no automatic microphone replacement.
- Push-to-talk, toggle, and supported panel start preserve the intended Target app
  and Automatic paste on a successful Dictation.
- No idle capture after every trial; built-in behavior remains normal; existing
  retention settings and signing/permission identities remain unchanged.

No latency distributions or physical permission-retention conclusions are
reported until these trials are performed. The available hardware and operator
participation do not count as completed measurements.
