# Dictation feedback verification

This report tracks implementation and validation of [#111](https://github.com/softmaxe/whisper/issues/111).

## Implementation scope

- [#112](https://github.com/softmaxe/whisper/issues/112): microphone connection, silent breathing, and speech waveform.
- [#113](https://github.com/softmaxe/whisper/issues/113): continuous pending feedback through Automatic paste and settled completion ownership.
- [#114](https://github.com/softmaxe/whisper/issues/114): actionable recovery motion, retry, and dismissal. This depends on #113.

The implementation preserves compact normal stages, existing recovery controls,
and microphone release between Dictations. It introduces no signing, packaging,
server, or settings changes.

## Upstream inspection

Reviewed [OpenWhispr VoicePill](https://github.com/OpenWhispr/openwhispr/blob/86e40760a6e229afc6488dac3ea981d7195e8458/src/components/dictation/VoicePill.tsx).
Its identity pulse and processing glow provide precedent for stage feedback.
The local implementation already has a connection sweep and processing wave;
these are the starting point for this change.

## Automated validation

Validated with Node.js 24 and dependencies installed by `npm ci`.
`npm run quality-check` passed on the integrated implementation: lint,
TypeScript, translation checks, and all 965 tests, with no skipped tests.
Lint retains three existing warnings.

- `recordingStartupLifecycle.test.js` drives the real Dictation Hook,
  AudioManager, and VoicePill through delayed acquisition, silent first frames,
  changing input levels, cancellation, and late device delivery. Reading the
  visual waveform leaves the measured audio level unchanged.
- `recordingCompletionLifecycle.test.js` retains the real completion callback
  and controls transcription, cleanup, and paste responses. It checks pending
  presentation until delivery settles, cleanup fallback, manual recovery,
  cancellation during finalization or processing, late results, and History
  audio ownership across sessions. A combined regression also mounts the real
  transcript panel hook: after manual-copy recovery, the next delayed device
  open must reveal connection feedback, including success, cancellation,
  startup failure, and late preview events.
- Existing clipboard, paste-outcome, microphone-release, native-window geometry,
  and error auto-hide suites remain part of the full quality check.

The production renderer build passed. Its existing large-chunk advisory remains.
Verification-document formatting and all five linked sources were checked.

## Code review

Standards review found no actionable issues. Spec review found one issue: manual
copy recovery still covered the next request until the microphone became ready.
The fix hands presentation to the next request when preparation begins and
invalidates stale panel callbacks. Both review axes passed the follow-up review.

## Visual inspection

The visual review uses the production components in an isolated macOS Electron
window with controlled input levels and a stubbed clipboard response. This
checks the appearance and component controls without opening a microphone or
writing to the system clipboard. It is not an end-to-end run of the installed
app or its native paste service.

The preview includes connection sweep, ready silence, simulated speech,
processing, microphone unavailability, error prompts, and manual-copy recovery.
Successive native-window captures showed the moving connection sweep, changing
silent bar heights, speech-driven bars, and processing wave. Recovery outlines
faded gently in place while the prompt text and controls stayed stationary.
This was visibly different from the moving processing wave.

The real CopyRecoveryPanel copy button changed the prompt to its copied state,
and Close removed the panel. Error-card Retry activated the preview's connection
state. Cancelling the preview state returned its pill to the idle sliver. These
checks establish component presentation and callbacks; the automated lifecycle
tests establish request ownership and retry handoffs. The preview used the same
`backgroundThrottling: false` setting as the production dictation window.

## Remaining device checks

The user chose to perform real-device voice validation later. The system listed
both a built-in microphone and an iPhone microphone, but neither was opened by
this task. No real-device latency, speech, or Target app insertion result is
claimed.

In the app build containing this change:

1. With the iPhone selected, start Dictation and observe the sweep throughout
   connection and the first-audio wait. Note whether the reported freeze is a
   subtle sweep, ready silence, or a renderer stall. This task has not reproduced
   an iPhone renderer stall.
2. Remain silent, speak, and pause. Check that ready silence breathes and speech
   follows actual input. Repeat with the built-in microphone.
3. Stop recording and observe finalization, transcription, enabled cleanup,
   and Automatic paste. The pill should remain pending until delivery settles.
   Repeat with cleanup disabled and a supported cleanup failure fallback.
4. Exercise unavailable microphone, failed transcription, failed paste, manual
   copy, retry, and dismissal. Prompts should remain explicit, recovery motion
   should differ from loading, and text and controls should stay usable.
5. Cancel during connection and processing, then start another Dictation. Check
   that late results do not change the new session, and confirm that capture is
   released between Dictations.

Automated IPC outcomes establish behavior under controlled responses. Verify
actual insertion in a live Target app separately.
