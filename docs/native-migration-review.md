# Native migration source review

This records two independent review axes for [specification #35](https://github.com/softmaxe/whisper/issues/35). The initial frozen source is `e001c6014a7158b43cc21cc0f861c957022da9f3`. Both reviewers used:

`git diff 4980cd2301ef3f790348a34b8317e52d47414d97...e001c6014a7158b43cc21cc0f861c957022da9f3`

The source review did not run hardware, UI, packaging or performance checks. The
final shell and Recording pill changes were still being integrated and require
a separate review of the later changes. The [acceptance checkpoint](native-migration-status.md)
tracks those gates.

## Standards

### Documented standards

No confirmed hard violation found against `AGENTS.md`, `README.md`, `test/README.md`, `docs/agents/domain.md`, `docs/macos-signing.md`, `native/README.md`, `CONTEXT.md`, and the supplied native ADR. The shared application command boundary, real persistence/HTTP-body workflow tests, original signing identity, separate native profile, and distinction between automated evidence and pending human acceptance remain explicit. This read-only review did not run tests, packaging, UI, or hardware checks. It does not certify the pending shell/pill delta or release acceptance.

### Heuristics requiring judgment

1. **Duplicated Code, P3.** `native/Sources/WhisperCore/Transcription.swift:131-138` and `native/Sources/WhisperCore/Cleanup.swift:178-185` repeat the endpoint-suffix list, removal loop, and `if path.lowercased().hasSuffix("/v1") { path.removeLast(3); path += "/v1" }`. Each future gateway-normalization correction must be applied twice; missing one produces different ASR and Cleanup destinations for the same configured gateway form. Extract only the common path normalization, leaving Azure routing and Cleanup's `/v1` insertion in their respective callers.

2. **Duplicated Code, P3.** `native/Sources/WhisperCore/CleanupWorkflow.swift:18-47` and `native/Sources/WhisperCore/WhisperApplication.swift:351-387` duplicate credential replacement, profile commit, old-account deletion, and rollback: `if !state.settingsSaved, let createdAccount { try? credentials.delete(account: createdAccount) }`. This security-sensitive ordering now has two implementations. A future rollback or failed-deletion repair can leave one service with orphaned secrets or a profile pointing at a deleted credential. Share the credential/profile transaction while retaining separate account prefixes, configuration validation, and input normalization.

Both findings are maintenance recommendations, not demonstrated behavior defects or documented-standard breaches.

## Spec

1. **[P2] Measure a hold from its press, before synchronous preparation.**
   Spec story 10: “I want holding my configured shortcut to record and releasing
   it to submit”. In `native/Sources/WhisperCore/Shortcut.swift:77`,
   `startDictation` runs before `scheduleHoldRecognition`; line 120 then sets
   `gesturePressedAt = clock.now`. The intervening work synchronously reads
   Keychain and enumerates inputs (`Dictation.swift:104`). If preparation takes
   200 ms and the user releases after a physical 300 ms hold, recognition sees
   only 100 ms, treats it as a short tap and discards the recording. The existing
   release-time fallback uses that same late timestamp. Start gesture timing at
   accepted press and schedule only the remaining threshold duration; cover a
   delayed synchronous external boundary through the public shortcut command.

2. **[P2] Do not permanently abandon learning after an early correction.**
   Spec story 48: “I want correction learning preserved”.
   `native/Sources/WhisperCore/CorrectionLearning.swift:124` stops observation when
   its first snapshot differs from both the original transcript and the pre-paste
   field. The first read is delayed 500 ms (line 73): a user who corrects a pasted
   word during that interval therefore loses the entire remaining monitoring
   window, including later corrections. The retained monitor accepts a nonempty
   initial value and continues observing (`src/helpers/textEditMonitor.js:746`),
   comparing subsequent edits against the supplied original transcript. Preserve
   safe field/range ownership without requiring the initial unedited text still
   to be present; add an early-edit-then-later-edit workflow case.

Missing/partial acceptance remains the explicitly tracked final shell/pill delta
and #36/#54/#55/#57 visual, physical, performance and upgrade gates; these are not
new defects or proof of completion. No additional scope creep found. OpenCC's
provenance and conversion interfaces were inspected without reviewing immutable
dictionary rows individually. Existing passing checks do not establish human
acceptance.

## Resolution status

All four findings are assigned to one implementation follow-up. The two P2
behavior defects require public-workflow reproduction and regression tests. The
two P3 recommendations will use small shared operations with existing endpoint
and real Keychain transaction tests. Fixes and the final UI/pill changes still
need integration and review; this document does not mark them resolved.

Initial totals: Standards has zero confirmed hard violations and two P3
heuristics; Spec has two P2 behavior findings, in addition to the explicitly
pending acceptance gates.
