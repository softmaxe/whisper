# Native migration acceptance

This branch implements [specification #35](https://github.com/softmaxe/whisper/issues/35).
The behavior reference is Whisper 1.0.5 at
`4980cd2301ef3f790348a34b8317e52d47414d97`. The specification and its tickets
remain the acceptance contract. An implemented workflow does not establish
hardware performance, visual parity, or permission retention.

## Delivery graph

| Ticket | Workflow                                           | Dependencies                                | Evidence                                             |
| ------ | -------------------------------------------------- | ------------------------------------------- | ---------------------------------------------------- |
| #36    | Packaged performance baseline and budgets          | None                                        | Tools ready; packaged measurements pending           |
| #37    | Native launch and fresh protected ASR settings     | None                                        | Settings, persistence, UI and package verified       |
| #38    | Button-started Dictation and capture ownership     | #37                                         | Core and built-in smoke verified                     |
| #39    | Right Command hold and Automatic paste             | #38                                         | Implemented; manual checks pending                   |
| #40    | Double-tap Hands-free Dictation                    | #39                                         | Implemented; manual checks pending                   |
| #41    | Configurable keyboard and mouse shortcuts          | #40                                         | Implemented; manual checks pending                   |
| #42    | Selected and Auto microphones                      | #38                                         | Implemented; manual checks pending                   |
| #43    | Configurable text cleanup                          | #38                                         | Implemented; manual checks pending                   |
| #44    | Language and Chinese conversion                    | #38                                         | Implemented; manual checks pending                   |
| #45    | Searchable History, copy and delete                | #38                                         | Implemented; manual checks pending                   |
| #46    | Retained audio and History retry                   | #45                                         | Implemented; manual checks pending                   |
| #47    | Dictionary editing, import and persistence         | #38                                         | Implemented; manual checks pending                   |
| #48    | Correction learning                                | #39, #47                                    | Implemented; manual checks pending                   |
| #49    | Snippets                                           | #47                                         | Implemented; manual checks pending                   |
| #50    | Single-file Upload                                 | #45                                         | Implemented; manual checks pending                   |
| #51    | Batch Upload and cancellation                      | #50                                         | Implemented; manual checks pending                   |
| #52    | Local Dictation Insights                           | #45                                         | Implemented; manual checks pending                   |
| #53    | Recording pill and desktop preferences             | #40                                         | Implemented; manual checks pending                   |
| #54    | Core hardware and performance acceptance           | #36, #40, #42, #43                          | Diagnostic tooling integrated; hardware pending      |
| #55    | Full feature, interface and performance acceptance | #41, #44, #46, #48, #49, #51, #52, #53, #54 | Source complete; signed synthetic recheck passed     |
| #56    | Signed native artifacts                            | #39                                         | Signed package verified at the final source revision |
| #57    | Signed upgrades and daily-use transition           | #55, #56                                    | Pending                                              |

## Acceptance gates

At `8770993a460eba252dce91a77274327ee436799f`, all 325 integrated native workflow
tests pass in 33 suites, plus the isolated long-recording memory case. The native
command runs ordinary workflows and the long-recording memory case in separate
processes, so large Upload fixtures cannot contaminate the process-wide RSS
measurement. It still runs every case without relaxing the memory limit. The 1,057
legacy/tooling quality checks pass on the pushed checkpoint that carries this
documentation.

Integration covers the complete cleanup → Chinese conversion → Snippets →
History/Automatic paste workflow; correction learning; retained audio and retry;
transactional retention/Insights; all 33 Upload formats; and sequential batches
with cancellation, current results and Upload exclusions. The separate
[core verification report](native-core-verification.md) records the earlier
built-in microphone smoke and its limits.

[Remote CI](https://github.com/softmaxe/whisper/actions/runs/35492185907) passed for
`7c7f3f0f4127a785895c345958cd15f770f9c3be`, including native tests and
credential-free packaging on macOS 27/Xcode 27 arm64. Its predecessor
[run 35470904060](https://github.com/softmaxe/whisper/actions/runs/35470904060)
covered the earlier revision `be835b4d5e49cdef376ff8f7d34c52ea821ea698`; review
rounds changed the source in between, so the newer run is the exact-head record.
`npm run test:signing` at the same revision confirms that two changed app versions
and native helpers keep the same certificate-bound identity and that temporary
signing material disappears after success and after a failed import. The reviewed
source rounds and the signed synthetic checks are recorded separately from physical
acceptance.

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

## Implementation status

Source work is complete at `8770993a460eba252dce91a77274327ee436799f`:

- The Home/Insights/Upload/Dictionary shell, separate Settings modal, one
  application-wide search entry, persistent form drafts, Privacy controls and the
  controlled synthetic UI fixtures are integrated.
- Recording pill start and cancel ownership, copy-recovery dismissal, countdown,
  hover hold and display geometry are integrated. The signed bundle was rechecked
  on September 20, 2026 in English and Chinese; the complete recovery card,
  including its footer, is visible.
- Content-free capture, transport and paste diagnostics with strict numeric
  summaries are implemented. Packaged samples and the baseline-derived budgets
  remain uncollected.
- Self-hosted endpoint compatibility, including normalized suffixes, encoded paths
  and Azure deployment routes, is covered by the merged protocol suite.
- Termination awaits cancelled Dictation, retry, Upload, cleanup and paste work,
  including captures that never write History.

What remains needs the physical session, the desktop, packaged measurements and the
signed upgrade. [Resume instructions](native-migration-resume.md) list that order.
An implemented workflow is not hardware, performance or permission-retention
evidence.

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
remain open. Keep the PR in draft while acceptance is incomplete.

See the [capture lifecycle decision](adr/0001-release-dictation-capture.md),
[baseline procedure](recording-startup-baseline.md),
[test requirements](../test/README.md), and
[signing requirements](macos-signing.md).
