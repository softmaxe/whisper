# Native migration performance baseline

This is the collection protocol and evidence ledger for
[ticket #36](https://github.com/softmaxe/whisper/issues/36) and
[specification #35](https://github.com/softmaxe/whisper/issues/35).
The ticket remains incomplete until controlled packaged measurements and numerical
budgets have been recorded. A successful build, a simulated microphone, and a
workflow test are not hardware performance evidence.

## Verified preparation

The legacy behavior reference is Whisper 1.0.5 at
`4980cd2301ef3f790348a34b8317e52d47414d97`. A baseline package was produced from
checkout `837eed2a0c8af7ddc97c4c360589243415f35d2a`, whose changes from that reference are documentation only,
using Node.js 24.21.0, Electron 41.10.5, and Xcode 27. The existing
`npm run test:signing`, `npm run pack:release`, and deep strict signature
verification passed. The original certificate was used. No installed app was
replaced, no permissions were reset, and no legacy profile was changed.

The baseline `Contents/Resources/app.asar` SHA-256 is
`91d769169d4e152eeb60e3dc39614c744300e65f566014c46b02f8abed4a4afb`.
It matches the installed 1.0.5 source archive. This identifies the application
archive; it does not establish identical compiled helper binaries or a measured
startup time. The prepared package is the normal build output at
`dist/mac-arm64/Whisper.app`.

The completion probes described below were added after this reference package.
Rebuild the instrumented baseline and record its exact revision and archive hash
before collecting completion intervals. Keep the reference package identity
separate; the additional diagnostics do not change its existing archive.

Read-only hardware inspection on September 20, 2026 confirmed an Apple M2 Pro,
32 GB memory, and macOS 27.0 build `26A428`. The Mac was on battery and concurrent
development work was running. No controlled timing or idle-resource samples were
collected under those conditions.

OpenWhispr was inspected at
`6d56d75e7e13ec47009e573e9ff4cded0d0ccc61`. Its `audioManager.js` reports
`Recording start timing` from inside AudioManager and `Pipeline timing` around
processing. Those intervals do not cover shortcut acceptance, full launch,
physical readiness feedback, or completed-processing-to-paste. This fork's
correlated [recording startup trace](recording-startup-baseline.md) supplies the
startup intervals below. The tools here reuse that trace without changing capture.

## Fix the conditions before measuring

Use one private manifest for each revision, input, power state, window state, and
profile fixture. Give devices ordinal aliases such as `input-1`. Publish only
the following manifest fields, without device labels, hostnames, file paths,
credentials, transcripts, or full diagnostic logs:

| Field    | Required value                                                                                                       |
| -------- | -------------------------------------------------------------------------------------------------------------------- |
| Build    | Full source SHA, package SHA-256, app version, signature identity, runtime version                                   |
| Machine  | Chip, memory, macOS build, iOS version for iPhone input                                                              |
| Input    | Alias, built-in/external/iPhone, USB/wireless connection, selected/Auto mode                                         |
| Power    | AC/battery, charge band, Low Power Mode, thermal state                                                               |
| Desktop  | Visible/hidden/occluded window, display refresh rate, other active microphone clients                                |
| Profile  | Fresh first-run or configured fixture, locale, History entry count, dictionary/snippet counts, retained audio bytes  |
| Settings | Right Command, activation mode, cue, media control, sample rate, cleanup and retention preferences                   |
| Service  | Loopback fixture or self-hosted category, protocol/model alias, controlled delay, request options, network condition |
| Run      | Launch method, 30-second idle interval, capture-release check, sample count, failures, observer resolution           |

Record first-run onboarding separately from a configured profile. Use synthetic
text and public generated audio fixtures in a fresh profile. Do not copy the
legacy database, settings, credentials, or audio into it. Compare empty and
populated fixtures separately. Start with 1,000 synthetic History entries,
100 dictionary entries, and 100 snippets for the populated fixture; preserve
these counts and the exact fixture generator revision across compared runs.

`OPENWHISPR_USER_DATA_DIR` is applied before the legacy app's environment file and
single-instance lock. It isolates storage and the profile's sidecar and Globe
recovery markers. It does not isolate system permissions, global shortcuts,
clipboard, focus, or login items. Coordinate a controlled session before starting
a second configured app. Leave the daily-use installation intact.

For the prepared baseline, launch the bundle executable directly with a fresh
temporary profile. Do not use `open -a Whisper`, which can select the daily app.
Run this only after background builds have stopped and the desktop session is
reserved for the test:

```sh
trial_root=$(mktemp -d "${TMPDIR:-/tmp}/whisper-baseline.XXXXXX")
OPENWHISPR_USER_DATA_DIR="$trial_root/profile" \
  dist/mac-arm64/Whisper.app/Contents/MacOS/Whisper --log-level=debug
```

Keep that shell variable until the run is complete. After the test app exits and
its process tree is gone, copy only reviewed numeric evidence to the report and
remove the temporary profile and logs with `rm -rf "$trial_root"`. Do not enable
login at startup in a temporary bundle. Keep newly entered credentials as test
placeholders. Keychain continuity is a separate signed upgrade test.

## Collect measurements

Collect at least 20 valid repeated trials per compared device and condition.
Record every attempt, including cancellations, missing observations and failures.
Use a 30-second idle interval after confirmed capture release. Collect at least
10 separate cold-process launches; label filesystem-cache state instead of
claiming a reboot or purged cache. Do not flush the system cache or restart the
daily app to manufacture a cold launch. Alternate revision order across runs.

| Metric                       | Start and end                                                                        | Evidence and separation                                                                                                                                               |
| ---------------------------- | ------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Cold launch                  | Process launch to usable initial UI                                                  | Timestamp the launched test PID and first usable UI observation. First-run and configured launches are separate. A process appearing in `ps` is insufficient.         |
| Window availability          | Open-window action to usable main window                                             | Test an already-running app, hidden and visible cases separately.                                                                                                     |
| Routine interaction          | Input to displayed result                                                            | Repeat navigation, History search with fixed query/fixture size, copy feedback, and settings navigation. Record each action separately.                               |
| Shortcut to first audio      | Accepted request to `firstAudio`                                                     | Use the correlated startup trace and at least 20 physical starts. Silence is valid audio delivery.                                                                    |
| Shortcut to ready feedback   | Accepted request to `readyFeedback`                                                  | This is committed UI state. Verify actual visual and optional audible feedback separately.                                                                            |
| Client scheduling            | Accepted request to `acquisitionRequested`; `firstAudio` to `readyFeedback`          | Neither interval is external microphone connection time.                                                                                                              |
| Input acquisition            | `acquisitionRequested` to `acquisitionCompleted`; then to `firstAudio`               | Includes driver/device work and application callback delivery. Do not attribute all of it to hardware.                                                                |
| Stop to request dispatch     | Accepted stop to outgoing ASR request dispatch                                       | Record both client events on the same monotonic clock. Server receipt alone includes transport.                                                                       |
| Processing complete to paste | Final text available after cleanup/fallback to paste dispatch and observed insertion | Record dispatch and visible insertion separately. A successful helper exit does not prove insertion in a text field.                                                  |
| Server round trip            | Request dispatch to response completion                                              | Report ASR and cleanup independently. Server processing duration needs a server-side observation; total round trip also includes transport.                           |
| Idle resources               | Stable idle process tree over 5 minutes                                              | Sample RSS and interval CPU at 1 second, after a 60-second settling period. Measure energy independently with a supported Instruments instrument and record its unit. |

The instrumented baseline records startup and completion through the same random
request identity. Its completion stages use the renderer's monotonic clock and
the startup trace's origin. The probes add no awaits, server requests, device
opens, or capture retention. They use the existing logger and debug-file logging
configuration. Record the instrumented revision, timing resolution, and observer
overhead with the measured build. Do not derive intervals by subtracting unrelated
wall-clock log messages or a whole processing duration.

For UI presentation, use a timestamped frame recording of the synthetic fixture
and a visible input marker, or an equivalent instrument with measured resolution.
Record frame rate and quantization. Review only the test windows. A stopwatch or
asynchronous screenshot cannot resolve sub-frame delays. Never publish a screen
recording containing other applications or personal content.

Use a controlled loopback ASR/cleanup service to establish client delay, then
repeat with the intended service and report its additional network/server work.
The fixture must return synthetic text and fixed delays without recording request
bodies or credentials. Missing server timing stays missing. Do not subtract two
independently computed medians to estimate per-request client work.

The required comparison inputs are the built-in microphone and wireless iPhone,
both confirmed available by the user. Keep their samples separate and use
anonymous aliases. In addition to timing, verify first spoken words, valid silence, real
readiness feedback, normal Command combinations, capture release, disconnection,
target changes, and long Hands-free Dictation. The new gesture thresholds require
physical Right Command holds, genuine double taps, and accidental taps. The legacy
build has separate activation modes, so compare its push-to-talk and toggle modes
with their corresponding native outcomes; it cannot validate the new gestures.

## Summarize numeric evidence

The summarizer reads a single condition at a time. It never exports request IDs,
full log messages, source paths, or unrecognized metadata. Keep the source log
private:

```sh
node scripts/measure-performance.js startup "$trial_root/profile/logs/debug-RUN.log"
node scripts/measure-performance.js completion "$trial_root/profile/logs/debug-RUN.log"
```

It uses the highest sequence per request and ignores late callbacks. It reports
terminal failures and cancellations separately. Main-only acceptance, missing
stages and negative intervals are missing observations, never zero-time success.
It does not merge capture attempts. Multiple acquisition attempts and held
capture are excluded. Confirm that prepared capture belongs to this request and
that the selected device did not change. The log does not prove either physical
condition; mark contaminated trials separately before comparison.

`Dictation completion` records describe self-hosted Dictation. They share the
startup `requestId`, but have their own `sequence`, frozen `stages`, and terminal
`outcome`. They contain no text, device labels, endpoints, models, or credentials.

| Stage                  | Observation                                                                                                                                                      |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `stopAccepted`         | The real Dictation Hook accepts stop after confirming an active recording. It includes subsequent recorder finalization in the stop-to-dispatch interval.        |
| `asrRequestDispatched` | Immediately before the renderer calls `fetch` for the ASR request. No server or network delay has completed yet.                                                 |
| `asrResponseCompleted` | The ASR response body has been received and parsed. The interval includes transport, server work, and client response parsing, so it is not pure inference time. |
| `processingComplete`   | The Hook has the result after cleanup/fallback, Chinese conversion and Snippets.                                                                                 |
| `pasteDispatched`      | Immediately before invoking the main-process paste IPC.                                                                                                          |
| `pasteSettled`         | The paste IPC resolves. Success requires its `pasted` result to be true; actual text-field insertion remains a separate physical observation.                    |

`processingCompleteToPasteDispatch` measures client work before the paste IPC;
`processingCompleteToPaste` measures through that IPC's completion. The latter is
not a screen-presented insertion measurement. Clipboard-only delivery has no
paste stages and therefore reports missing paste observations. Failed paste,
failed processing, cancellation, unfinished processing, and late responses do
not become successful zero-time samples. Cancellation freezes the affected
request even if a new Dictation has already started. The trace is passed alongside
the result inside the renderer, and is removed from options before paste IPC;
it is not sent to the ASR service or saved in History.

These probes cover the supported self-hosted Dictation path. A History retry,
Upload, a recording stopped outside the Hook, or an inherited unsupported provider
without the required boundaries must not be represented as a complete sample.

Other measurements use an array of fixed metric names, outcomes, and numbers:

```json
[
  { "metric": "coldLaunch", "outcome": "success", "value": 1234.5 },
  { "metric": "coldLaunch", "outcome": "missing" },
  { "metric": "routineInteraction", "outcome": "failed" }
]
```

These numbers illustrate the file format and are not measured results. Keep a
separate file per action and condition. Supported metrics are declared in
[`performance-baseline.js`](../scripts/lib/performance-baseline.js). Use
`success`, `failed`, `cancelled`, `missing`, or `excluded`; only `success` has a
finite nonnegative value. Unknown fields are rejected so arbitrary text cannot
silently enter a published summary.

```sh
node scripts/measure-performance.js summarize observations.json
node scripts/measure-performance.js sample-process TEST_APP_PID 300 1000
```

Process sampling is read-only. Select the test app's main PID, not a renderer.
It includes the current descendant process tree, sums RSS in MiB, and computes
CPU time differences as a percentage of one core. RSS can count shared pages
more than once and is not physical footprint. CPU has `ps` accounting resolution;
process churn makes that interval missing. The tool cannot determine whether the
app is idle, verify the PID's build, or measure energy. Record those conditions
separately. Permission or process failures abort sampling without printing paths
or private process arguments.

`idleEnergy` accepts joules only. If an instrument reports a relative energy-impact
score, preserve its native unit in separate evidence; do not relabel it as joules.

Reports use the arithmetic middle-pair median and nearest-rank p90, plus minimum,
maximum, successful count, and each unsuccessful outcome count. Empty groups
return null statistics. Preserve raw numeric observations privately for review.

## Numerical acceptance gate

No numerical performance budget has been approved or inferred from language
choice. Fill the worksheet after the baseline has valid samples. Freeze it before
evaluating the native candidate; do not relax a budget after seeing its result.

| Priority | Metric                                   | Baseline n / median / p90 / range | Native median limit | Native p90 limit | Status                                                        |
| -------- | ---------------------------------------- | --------------------------------- | ------------------- | ---------------- | ------------------------------------------------------------- |
| 1        | Shortcut to first audio, per input       | Pending                           | Pending             | Pending          | Blocked on hardware session                                   |
| 1        | Shortcut to readiness feedback           | Pending                           | Pending             | Pending          | Blocked on hardware session                                   |
| 1        | Routine interaction, per action          | Pending                           | Pending             | Pending          | Blocked on controlled UI observations                         |
| 1        | Stop to dispatch and processing to paste | Pending                           | Pending             | Pending          | Controlled processing and physical paste observations pending |
| 2        | Cold launch and window availability      | Pending                           | Pending             | Pending          | Controlled launch/UI observations pending                     |
| 3        | Idle RSS, CPU and energy                 | Pending                           | Pending             | Pending          | Quiescent session and energy instrument pending               |

Compare matched conditions only. Do not hide unsuccessful attempts in an average.
A performance pass also requires no device substitution, no loss of opening
speech, correct capture release, and no output from cancelled requests. Signature
checks and deterministic tests remain separate evidence from this gate.

To complete collection, reserve a quiet desktop session on the target Mac, keep
power and thermal conditions stable, connect the intended external/iPhone input,
provide physical Right Command gestures and a public test phrase, verify visible
microphone release and paste in a disposable text field, and grant only the test
app permissions needed for those observations. A native signed upgrade needs the
separate [release smoke check](../test/README.md#release-smoke-check). Do not change
system trust, delete Keychain items, reset TCC, or replace the daily app during
baseline collection.
