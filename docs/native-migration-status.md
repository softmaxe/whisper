# Native migration acceptance

This branch implements [specification #35](https://github.com/softmaxe/whisper/issues/35).
The behavior reference is Whisper 1.0.5 at
`4980cd2301ef3f790348a34b8317e52d47414d97`. The specification and its tickets
remain the acceptance contract. An implemented workflow does not establish
hardware performance, visual parity, or permission retention.

## Delivery graph

| Ticket | Workflow                                           | Dependencies                                | Evidence                                               |
| ------ | -------------------------------------------------- | ------------------------------------------- | ------------------------------------------------------ |
| #36    | Packaged performance baseline and budgets          | None                                        | Tools ready; measurements pending                      |
| #37    | Native launch and fresh protected ASR settings     | None                                        | Settings, persistence, UI and package verified         |
| #38    | Button-started Dictation and capture ownership     | #37                                         | Core and built-in smoke verified                       |
| #39    | Right Command hold and Automatic paste             | #38                                         | Implemented; manual checks pending                     |
| #40    | Double-tap Hands-free Dictation                    | #39                                         | Implemented; manual checks pending                     |
| #41    | Configurable keyboard and mouse shortcuts          | #40                                         | Implemented; manual checks pending                     |
| #42    | Selected and Auto microphones                      | #38                                         | Implemented; manual checks pending                     |
| #43    | Configurable text cleanup                          | #38                                         | Implemented; manual checks pending                     |
| #44    | Language and Chinese conversion                    | #38                                         | Implemented; manual checks pending                     |
| #45    | Searchable History, copy and delete                | #38                                         | Implemented; manual checks pending                     |
| #46    | Retained audio and History retry                   | #45                                         | Implemented; manual checks pending                     |
| #47    | Dictionary editing, import and persistence         | #38                                         | Implemented; manual checks pending                     |
| #48    | Correction learning                                | #39, #47                                    | Implemented; manual checks pending                     |
| #49    | Snippets                                           | #47                                         | Implemented; manual checks pending                     |
| #50    | Single-file Upload                                 | #45                                         | Implemented; manual checks pending                     |
| #51    | Batch Upload and cancellation                      | #50                                         | Implemented; manual checks pending                     |
| #52    | Local Dictation Insights                           | #45                                         | Implemented; manual checks pending                     |
| #53    | Recording pill and desktop preferences             | #40                                         | Implemented; manual checks pending                     |
| #54    | Core hardware and performance acceptance           | #36, #40, #42, #43                          | Measurement support in progress; hardware pending      |
| #55    | Full feature, interface and performance acceptance | #41, #44, #46, #48, #49, #51, #52, #53, #54 | Final shell and parity fixes in progress               |
| #56    | Signed native artifacts                            | #39                                         | Signed packaging and CI verified; final repack pending |
| #57    | Signed upgrades and daily-use transition           | #55, #56                                    | Pending                                                |

## Acceptance gates

At `48ab39cfaf45a5239a3d06398da03388daa80662`, all 241 integrated native
workflow tests pass. The 1,052 legacy/tooling quality checks also pass. The native
command runs ordinary workflows and the long-recording memory case in separate
processes, so large Upload fixtures cannot contaminate the process-wide RSS
measurement. It still runs every case without relaxing the memory limit.

Integration covers the complete cleanup → Chinese conversion → Snippets →
History/Automatic paste workflow; correction learning; retained audio and retry;
transactional retention/Insights; all 33 Upload formats; and sequential batches
with cancellation, current results and Upload exclusions. The separate
[core verification report](native-core-verification.md) records the earlier
built-in microphone smoke and its limits.

[Remote CI](https://github.com/softmaxe/whisper/actions/runs/35470904060) passed for
`be835b4d5e49cdef376ff8f7d34c52ea821ea698`, including native tests and
credential-free packaging on macOS 27/Xcode 27 arm64. That earlier run and the
existing signed package do not validate the contents of later source revisions.
A final integrated package, exact-head CI result and source review remain required.

Record deterministic workflow tests, packaged measurements, English/Chinese
visual review, automated signature checks, and actual upgrade observations
separately. Use isolated native profiles and synthetic fixtures. Do not include
private speech, device labels, server configuration, credentials, or complete
application logs in evidence.

The installed legacy application and its profile remain available throughout
development. Daily use changes only after full acceptance and signed upgrade
checks pass. Remote release publication requires a separate request.

Once the migration is complete, remove the maintenance requirement to inspect
upstream OpenWhispr before every fix or feature. Subsequent development follows
the native macOS implementation and its supported workflows.

## Implementation still in progress

- Final Home/Insights/Upload/Dictionary shell, separate Settings modal, one
  application-wide search entry, persistent form drafts, Privacy controls and
  controlled synthetic UI fixtures.
- Recording pill start/paste parity, copy-recovery dismissal/countdown/hover and
  display selection/geometry.
- Content-free native capture/transport/paste diagnostics and strict numeric
  summary support. Physical samples and performance budgets remain uncollected.
- Existing self-hosted endpoint compatibility, including normalized endpoint
  suffixes and Azure deployment paths.
- A targeted termination audit for cancelled Dictation/retry tasks and capture
  release when History is disabled or a provisional gesture is rejected.

Continue these independent source tasks before waiting for human acceptance.
[Resume instructions](native-migration-resume.md) describe the following session.

## Implementation defaults awaiting acceptance

- Hold recognition currently uses 150 ms. A second press may begin within
  300 ms after the first short release; its own short release confirms
  Hands-free Dictation. These are provisional implementation defaults, not
  hardware-measured thresholds.
- Standalone Esc is reserved for cancellation. Allowing it as the recording
  control would conflict with cancellation during its own provisional capture.
  The settings UI must explain this restriction in both languages.
- Performance budgets remain unset until the packaged baseline is measured.
  The supported hardware acceptance set is built-in input and wireless iPhone.

Human-dependent voice, physical shortcut, permission and signed upgrade checks
remain open. Complete the remaining implementation and automated verification
before resuming those checks; keep the PR in draft while acceptance is incomplete.

See the [capture lifecycle decision](adr/0001-release-dictation-capture.md),
[baseline procedure](recording-startup-baseline.md),
[test requirements](../test/README.md), and
[signing requirements](macos-signing.md).
