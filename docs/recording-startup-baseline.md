# Recording startup baseline

Issue [#23](https://github.com/softmaxe/whisper/issues/23) instruments existing
Dictation startup. Its baseline preserves the visual-frame wait, track-health
checks, device fallback, pre-roll, ready condition, and idle-hold preference.
Compare that baseline with later behavior changes described in
[#22](https://github.com/softmaxe/whisper/issues/22). Builds including
[#24](https://github.com/softmaxe/whisper/issues/24) release capture immediately
and no longer offer idle hold.

## Reference and current evidence

The behavior reference is `b10d49a6d628cc6b715e3ddc2af468cab0f5c4da`.
The upstream review used OpenWhispr
`6d56d75e7e13ec47009e573e9ff4cded0d0ccc61`; its prepared capture and recorder
handoff are already present in this fork. No additional upstream startup fix was
found. The instrumented baseline is `043d6bea0cb5a34f45cbfc38dee38760c0de1e9a`.
Record each measured checkout's exact SHA with `git rev-parse HEAD`. The uninstrumented reference's old
`Recording start timing` begins inside AudioManager and cannot supply a complete
shortcut-to-audio baseline.

Hardware baseline: **pending**. No built-in or iPhone latency samples have been
collected for this change. Controllable-event tests demonstrate timing and
identity behavior, not a hardware speedup. An Electron 41.10.5 check with a
generated silent audio track verified that the frame processor receives frames
and that cancelling its reader leaves the track live and MediaRecorder recording.
This is API validation, not microphone validation.

## Read a trace

`Recording startup` records use a random `requestId` shared by the main process,
preparation, and recording. `acceptedAt` is `performance.timeOrigin +
performance.now()`. Every value in `stages` is milliseconds since that acceptance,
rounded to 0.01 ms. These are timestamps on one timeline, not durations to add.
Use `sequence` to order renderer records if asynchronous logging delivers them
out of order. The main-process acceptance record has sequence 0.

`captureAttempt` identifies the capture currently observed within the request.
`captureSource` is `device` or `prepared`; the baseline also supports `held`.
If preparation expires and
recording acquires another stream, its capture stages start afresh under the
next attempt. Earlier attempts remain in earlier log records. Only the selected
attempt's frames can complete the trace. Reusing another request's prepared
stream records the reuse time and attaches an observer for the new request;
it does not claim another platform open. Do not combine capture stages from
different attempts. Late callbacks also identify `lateCaptureAttempt` when needed.

| Stage                                     | Observation                                                                                                                |
| ----------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `requestAccepted`                         | Main-process shortcut accepted before target capture and panel work, or a renderer recording request accepted by the Hook. |
| `rendererReceived`                        | The Hook receives the request. The difference from acceptance includes IPC delivery and renderer scheduling.               |
| `preparationEntered`                      | The Hook enters preparation, before the existing visual-frame wait.                                                        |
| `deviceResolved`                          | Settings/cache/device resolution supplies acquisition constraints.                                                         |
| `acquisitionRequested`                    | Acquisition begins, including reuse of an optional held stream.                                                            |
| `acquisitionCompleted`                    | The first successful platform open resolves, or a held clone is available.                                                 |
| `trackReady`                              | The effective capture track is observed live and unmuted after existing health checks.                                     |
| `firstAudio`                              | A nonempty `AudioData` frame from the effective recording track reaches the renderer observer. All-zero samples qualify.   |
| `recordingStarted`                        | AudioManager publishes its existing recording state.                                                                       |
| `readyFeedback`                           | React commits the Hook's recording state, which drives the recording pill.                                                 |
| `readyCueRequested` / `readyCueScheduled` | Optional sound cue requested / oscillator tones scheduled. Neither proves physical speaker output.                         |
| `firstAudioUnavailable`                   | The observer could not read a frame, for example an unsupported processor or an ended track. `firstAudio` remains absent.  |

`firstAudio` measures renderer observation, including dispatch delay; it is not a
hardware sample timestamp. Observation attaches after track-health resolution to
the same track passed to the recorder. It never opens another microphone, replaces
the recorder, modifies pre-roll, or reads sample values. Chromium's
[track processor](https://developer.chrome.com/docs/capabilities/web-apis/mediastreamtrack-insertable-media-processing)
is a sink on that track. After the first frame its reader is cancelled and the
frame is closed. The reusable `observeFirstAudio` helper also supports cancellation.

`readyFeedback` measures application state committed for display, not physical
screen presentation. In this reference behavior it can precede `firstAudio`.
The sound cue is optional and may finish scheduling after the startup trace
completes; that event retains the original identity as a `lateStage`.

The final `outcome` is `completed` only when start has settled successfully and
every required stage through first audio and ready feedback has been observed.
`totalMs` is the later of `firstAudio` and `readyFeedback`. For example, preparation
and target capture can overlap; do not add either to this total. To diagnose a
delay, subtract named stage timestamps, such as `acquisitionRequested` minus
`preparationEntered` or `firstAudio` minus `acquisitionCompleted`.

Cancellation is `cancelled`, unsuccessful start is `failed`, and an observation
ended by stop, teardown, supersession, or the 30-second observation limit is
`incomplete`. The limit only ends diagnostics; it does not cancel capture.
Missing stages stay absent and `totalMs` stays null. Late events carry
`lateStage` and `elapsedMs` under the original request and cannot fill its frozen
stages. A main-process acceptance with no renderer record is also incomplete
evidence, never a successful zero-duration startup.

Existing recovery can retry acquisition or select another device. The capture
timestamps describe the current attempt; track health can include internal
health-check retries. Exclude trials with recovery or cross-request reuse from a
same-device hardware comparison and report them separately.

## Collect comparable trials

1. Record the exact baseline and candidate SHAs, app version, Electron version,
   macOS and iOS versions, Mac model, and input category. Use anonymous device
   aliases such as `built-in-A` and `phone-A`, not personal device labels.
2. Use the same Mac, physical input, connection mode, power state, audio settings,
   shortcut mode, cue setting, app profile, ASR/cleanup configuration, and launch
   method for both revisions. Record USB versus wireless iPhone connection and
   any other application using the microphone. Do not mix development and
   packaged runs or reset permissions between revisions.
3. On the baseline, set **Keep Microphone Warm** to **Off** in microphone settings
   (`micWarmHoldSeconds = 0`) and verify it stays off after relaunch. Candidates
   including #24 remove this setting and always release capture when recording
   ends. Do not change History or audio-retention preferences to enable diagnostics.
4. Start the selected revision with the same debug logging configuration for
   each run. For an already installed matching build, quit first, then run
   `open -a Whisper --args --log-level=debug`. A source checkout does not update
   that installed app. Development runs can use
   `OPENWHISPR_LOG_LEVEL=debug npm run dev` after Node 24 and `npm ci` setup.
5. Collect at least 20 trials per revision, device, and condition. Keep visible
   and hidden/occluded window trials separate. Use a fixed 30-second idle interval
   after capture is released; record cold launches separately from repeated
   starts. Keep these conditions identical across revisions.
6. Start Dictation through the chosen shortcut, wait for the existing ready
   feedback, speak the same short test phrase, then stop. Also run silent starts,
   preparation cancellation, and a controlled device-disconnection trial.
   Verify capture is released before the next trial using the device's capture
   indication. If release cannot be confirmed, exclude and flag the trial.
7. Extract only `Recording startup` records from
   `~/Library/Application Support/whisper/logs/`. Keep full logs private: existing
   messages outside this trace can include labels, transcript data, or URLs.
   New timing records contain only identity, timing, and fixed lifecycle fields.
   Relaunch without debug flags after collection.
8. Group by request and retain outcome, missing stages, and late events. Report
   sample count, median, p90, and range for the stage timestamps and defined
   intervals. Report failures and cancellations separately rather than dropping
   them into a successful-start average. Check that speech following the ready
   signal is retained. Record any device fallback as a different-input trial.

Store comparison results with this metadata:

```text
Baseline SHA / candidate SHA:
App / Electron / macOS / iOS versions:
Mac model / anonymous input alias / connection mode:
Launch method / window visibility / power state:
Shortcut mode / cue setting / microphone mode / hold seconds: 0
Recording settings / retention settings unchanged:
Idle interval / capture-release check / sample count:
Stage medians, p90, ranges / failed, cancelled and incomplete counts:
Opening speech retained / device fallback or disconnect observations:
Hardware samples unavailable or pending:
```
