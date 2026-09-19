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
| #39    | Right Command hold and Automatic paste             | #38                                         | In progress                                    |
| #40    | Double-tap Hands-free Dictation                    | #39                                         | Pending                                        |
| #41    | Configurable keyboard and mouse shortcuts          | #40                                         | Pending                                        |
| #42    | Selected and Auto microphones                      | #38                                         | In progress                                    |
| #43    | Configurable text cleanup                          | #38                                         | In progress                                    |
| #44    | Language and Chinese conversion                    | #38                                         | In progress                                    |
| #45    | Searchable History, copy and delete                | #38                                         | In progress                                    |
| #46    | Retained audio and History retry                   | #45                                         | Pending                                        |
| #47    | Dictionary editing, import and persistence         | #38                                         | In progress                                    |
| #48    | Correction learning                                | #39, #47                                    | Pending                                        |
| #49    | Snippets                                           | #47                                         | Pending                                        |
| #50    | Single-file Upload                                 | #45                                         | Pending                                        |
| #51    | Batch Upload and cancellation                      | #50                                         | Pending                                        |
| #52    | Local Dictation Insights                           | #45                                         | Pending                                        |
| #53    | Recording pill and desktop preferences             | #40                                         | Pending                                        |
| #54    | Core hardware and performance acceptance           | #36, #40, #42, #43                          | Pending                                        |
| #55    | Full feature, interface and performance acceptance | #41, #44, #46, #48, #49, #51, #52, #53, #54 | Pending                                        |
| #56    | Signed native artifacts                            | #39                                         | Pending                                        |
| #57    | Signed upgrades and daily-use transition           | #55, #56                                    | Pending                                        |

## Acceptance gates

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

See the [capture lifecycle decision](adr/0001-release-dictation-capture.md),
[baseline procedure](recording-startup-baseline.md),
[test requirements](../test/README.md), and
[signing requirements](macos-signing.md).
