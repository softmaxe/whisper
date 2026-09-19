# Native migration acceptance

This branch implements [specification #35](https://github.com/softmaxe/whisper/issues/35).
The behavior reference is Whisper 1.0.5 at
`4980cd2301ef3f790348a34b8317e52d47414d97`. The specification and its tickets
remain the acceptance contract. An implemented workflow does not establish
hardware performance, visual parity, or permission retention.

## Delivery graph

| Ticket | Workflow                                           | Dependencies                                | Evidence                                       |
| ------ | -------------------------------------------------- | ------------------------------------------- | ---------------------------------------------- |
| #36    | Packaged performance baseline and budgets          | None                                        | Tools ready; measurements pending              |
| #37    | Native launch and fresh protected ASR settings     | None                                        | Settings, persistence, UI and package verified |
| #38    | Button-started Dictation and capture ownership     | #37                                         | Core and built-in smoke verified               |
| #39    | Right Command hold and Automatic paste             | #38                                         | Implemented; manual checks pending             |
| #40    | Double-tap Hands-free Dictation                    | #39                                         | Implemented; manual checks pending             |
| #41    | Configurable keyboard and mouse shortcuts          | #40                                         | In progress                                    |
| #42    | Selected and Auto microphones                      | #38                                         | Implemented; manual checks pending             |
| #43    | Configurable text cleanup                          | #38                                         | Implemented; manual checks pending             |
| #44    | Language and Chinese conversion                    | #38                                         | Implemented; manual checks pending             |
| #45    | Searchable History, copy and delete                | #38                                         | Implemented; manual checks pending             |
| #46    | Retained audio and History retry                   | #45                                         | In progress                                    |
| #47    | Dictionary editing, import and persistence         | #38                                         | Implemented; manual checks pending             |
| #48    | Correction learning                                | #39, #47                                    | Implemented; manual checks pending             |
| #49    | Snippets                                           | #47                                         | Implemented; manual checks pending             |
| #50    | Single-file Upload                                 | #45                                         | In progress                                    |
| #51    | Batch Upload and cancellation                      | #50                                         | Pending                                        |
| #52    | Local Dictation Insights                           | #45                                         | In progress                                    |
| #53    | Recording pill and desktop preferences             | #40                                         | In progress                                    |
| #54    | Core hardware and performance acceptance           | #36, #40, #42, #43                          | Pending                                        |
| #55    | Full feature, interface and performance acceptance | #41, #44, #46, #48, #49, #51, #52, #53, #54 | Pending                                        |
| #56    | Signed native artifacts                            | #39                                         | In progress                                    |
| #57    | Signed upgrades and daily-use transition           | #55, #56                                    | Pending                                        |

## Acceptance gates

At `ab6976de9176eabdb883fd80cb2dedb59128c38e`, 137 integrated native tests
pass. The unchanged legacy suite has 1,047 passing checks, including the OpenCC
reference data and conversion oracle. Integration covers cleanup success and
fallback, Chinese conversion, literal Snippet expansion, Hold-to-talk and
Hands-free delivery, real History persistence/reopen, and correction learning
that affects a subsequent Dictation. See the
[core verification report](native-core-verification.md) for the separate built-in
microphone smoke.

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
