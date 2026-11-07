# Performance changes and measured results

Measured on 2026-11-07, comparing baseline `811eccadcae7fed03834b4e2e96fad9d93098998`
with the working-tree implementation. Hardware: Apple M2 Pro, 32 GiB RAM,
macOS 27.2 arm64, Rust 1.98.0. Endpoint addresses and credentials are omitted.

## Changes

- Recording stop schedules conversion on a bounded two-job blocking executor.
  Request preparation moves SQLite priority reads and Keychain access off the UI
  thread; credentials remain in Keychain and are not cached on disk.
- WAV payloads use shared immutable `Bytes` through processing, HTTP upload,
  persistence and failed-recording retry.
- Audio conversion caches a bounded set of filters and precomputes rational-rate
  phases. Independent float accumulators improve throughput; direct PCM16 output
  eliminates the intermediate resampled float vector. Filter quality is retained.
- A bounded serial storage queue handles accepted History writes, loading,
  deletion, retention and settings saves. Accepted jobs survive dropped UI
  receivers; orderly application shutdown drains the queue. Save-before-paste
  ordering is retained.
- History snapshots are shared with the view. Derived date groups and Dictionary
  filtering, sorting, counts, focus keys and correction indexes are cached.
  Unrelated state changes no longer force History to recompute.
- Dictionary usage matching runs outside the database mutex. Normalized string
  search preserves script/word boundaries; updates validate stable row identity.
- Correction extraction runs off the UI thread. A contiguous `u16` LCS matrix
  replaces nested `usize` vectors, with actual allocation and one-sided edit
  limits. Dictionary epochs are checked under the database lock before learning
  writes, preventing cleared words from returning from an old task.
- Insights queries and Settings storage scans run in the background, coalesce
  refresh requests and reject stale results. Storage scanning is deferred until
  the relevant Settings section opens; deletion finishes before success feedback.
- File uploads stream with a known multipart length and a 256 KiB buffer, keeping
  temporary files alive through transmission. Batch processing remains serial.
- Before automatic paste, the operation generation is checked again after
  waiting for the target application. Cancelled work cannot start a stale paste.

## Real-server pipeline

Both revisions used the same fixed 5.36-second synthetic English speech fixture,
the currently configured ASR and cleanup servers, and identical Cargo test
profiles: application `opt-level=1`, dependencies `opt-level=2`, debug information
enabled. These are not release-package measurements.

Five pairs of runs alternated revision order. Each revision/mode contributed ten
measured samples, with independent warmup requests excluded. No user recordings,
application database, microphone or clipboard were used; no Keychain read was
needed because both servers accepted these requests without authentication.

| Median | Before | After | Reduction |
| --- | ---: | ---: | ---: |
| Conversion + ASR pipeline | 224.848 ms | 192.800 ms | 14.25% |
| Conversion + ASR + cleanup pipeline | 514.471 ms | 461.598 ms | 10.28% |
| Conversion in ASR-only runs | 43.047 ms | 12.255 ms | 71.53% |
| Conversion in cleanup runs | 57.384 ms | 9.804 ms | 82.91% |

ASR-only pipeline time was 180.730 versus 181.287 ms; with cleanup it was 454.646
versus 449.854 ms. These network-plus-pipeline differences are small and do not
demonstrate server acceleration. The measured gain primarily comes from local
conversion. All 40 measured responses preserved the known report/Friday phrase;
all cleanup requests succeeded. Phrase checks establish fixture correctness,
not a general speech-accuracy score.

The probe includes conversion and the actual processing pipeline. It excludes
capture setup, GPUI scheduling/rendering, persistence and native paste. It must
not be described as a measurement of complete shortcut-to-paste latency.

## Audio conversion and temporary allocation

`scripts/bench-audio.sh 811ecca` compiles both converters into one `rustc -O`
process, with identical synthetic input. Before/after order alternates across 30
warm samples per case. Input construction is outside timing.

| Input | Before p50 | After p50 | Before p95 | After p95 |
| --- | ---: | ---: | ---: | ---: |
| 48 kHz, 10 seconds | 38.366 ms | 5.264 ms | 40.944 ms | 5.515 ms |
| 48 kHz, 60 seconds | 223.125 ms | 31.471 ms | 227.943 ms | 31.752 ms |
| 44.1 kHz, 60 seconds | 213.153 ms | 33.577 ms | 225.686 ms | 35.344 ms |

For 60-second warm conversions, peak additional allocator-tracked live bytes
fell from 5,760,044 to 1,920,044, a 66.7% reduction. This excludes the input and
previously cached filters and is not process RSS. At 44.1 kHz, the first
one-second conversion builds a phase cache: peak additional allocation increased
from 352,264 to 477,768 bytes. The cache is bounded and amortized by later calls.

Existing alias rejection, DC, duration and PCM tests pass. An independent
before/after probe tested nine rates and six input lengths including empty and
short edges; those tested samples and PCM16 results had zero difference. This
does not claim bitwise equality for every possible signal.

## File upload memory and throughput

A controlled local streaming receiver drained a 128 MiB fixture without buffering
the body. Each revision ran in three fresh processes, alternating order;
`/usr/bin/time -l` measured maximum RSS for the full test process.

| Median | Before | Final after | Reduction |
| --- | ---: | ---: | ---: |
| Peak RSS | 141.375 MiB | 14.797 MiB | 89.53% |
| Transport time | 44.708 ms | 30.040 ms | 32.81% |

Transport ranges were 40.542–74.133 ms before and 27.296–57.407 ms after. With only
three samples, the throughput percentage is preliminary; the large RSS change
is consistent across these fresh processes. This is upload-test-process peak
RSS, not the complete application's idle memory or a network-server benchmark.

The initial default-buffer streaming implementation reduced RSS but increased
transport time substantially. The final 256 KiB buffer fixed that regression;
only final measurements appear above. Actual AIFF-to-MP3-to-ASR uploads succeeded
against the current speech server for the baseline and final implementation.

## Text and view preparation microbenchmarks

- A 1,000-word all-hit usage fixture, 30 samples: p50 total 142.222 to 4.640 ms;
  database-lock hold p50 142.222 to 0.812 ms. This isolated module benchmark ran
  while other compilation occurred; absolute and tail timings are noisy. It
  supports the algorithm/lock improvement, not an application-wide speedup ratio.
- A 1,950 by 1,950 LCS fixture, 30 samples: p50 27.969 to 19.089 ms. Matrix cell
  payload fell from approximately 30.45 MB to 7.61 MB. A separate fresh-process
  microbenchmark had peak RSS 34,717,696 versus 10,059,776 bytes. This is not app
  RSS. A 14,641-pair short-sequence comparison preserved the original diff choices.
- Word sorting alone, 10,000 mixed-script entries, two warmups and 30 alternating
  samples: median 20.611 to 1.165 ms, p95 21.931 to 1.288 ms, with identical order.
  The extracted real sort functions used equivalent minimal row structures;
  unrelated timestamp fields were represented by integers. This is not a GPUI
  frame-rate benchmark.

## Startup and unmeasured behavior

A synthetic database containing 5,000 words and 50 history rows took warm median
1.402 ms before versus 1.281 ms after to open and hydrate, with 30 samples each.
That difference is small. This probe excludes GPUI, fonts, native helper startup,
first frame and shortcut readiness; it establishes no meaningful native launch
speedup. History loading and Settings scans now avoid blocking UI construction,
but actual first-frame and interaction latency still need native profiling.

The installed app was not replaced, packaged or re-signed. Real microphone,
Accessibility, permission retention, native paste, display frame intervals and
whole-application idle RSS were not measured. Existing fonts, helper identity,
signing configuration and permission behavior were not changed. Virtual lists,
capture-ring redesign and persistent audio-path indexing remain follow-up work
if native traces show they are worthwhile.

## Validation and reproduction

`scripts/check.sh` passed: formatting, all-target Clippy with warnings denied,
and 482 tests passed, 11 opt-in hardware/live/performance tests ignored. The live
pipeline and real AIFF upload checks described above were run separately.
The dependency `block 0.1.6` emits an existing Cargo future-compatibility notice;
it does not cause a current Clippy failure.

Meaningful new regressions cover multipart length/binary integrity, stale learned
results after clear, diff allocation limits, cached view derivation, retained
storage work after dropping receivers, storage-worker recovery after a failed
job, and bounded CPU concurrency. Automated checks do not replace native UI
interaction checks in `docs/testing.md`.

Reproduce audio measurements with `scripts/bench-audio.sh 811ecca`. The opt-in
`src/services/performance_tests.rs` probes accept `WHISPER_PERF_AUDIO`,
`WHISPER_PERF_SETTINGS_PATH`, `WHISPER_PERF_OUTPUT`, `WHISPER_PERF_LABEL`, and
optionally `WHISPER_PERF_REPETITIONS`. Use a synthetic fixture and a private local
settings path; do not commit server credentials. See each probe for its required
environment and fixture behavior.

Local workspace evidence is retained under ignored `.e2e/performance-review/`:
`audio-interleaved.csv`, `audio-benchmark-metadata.json`, `live-summary.json`,
`live-comparison.json`, `upload-summary.json`, `upload-comparison.json`,
`environment.json`, `hydration-before.json`, `hydration-after.json`,
`render-sort-results.json`, `data/`, and `check.log`. Baseline and final test
executables are saved there for repeat comparisons. Those local artifacts are
not source-controlled release assets.

## Follow-up optimization and independent review

Measured on 2026-11-07 after baseline `92e9f32`, on the same Apple M2 Pro,
macOS 27.2 arm64 and Rust 1.98.0. An Astra high discussion identified the
remaining candidates; separate independent Astra high reviews checked the
implementation and requested additional sparse-match and Unicode/cascade tests.

Changes in this follow-up:

- Retention collects deleted transcript IDs, scans the audio directory once and
  keeps an ephemeral ID-to-path index. Work becomes linear in directory entries
  plus requested IDs instead of rescanning and sorting for every ID. Exact ID
  matching, legacy names, case-insensitive WAV extensions and the first lexical
  match among duplicate files are preserved. Single lookups use a minimum rather
  than sorting. History's Finder lookup runs on the storage worker.
- Empty correction results skip dictionary transactions, full dictionary reloads
  and change notifications, while still recording learning-session statistics.
  Sessions with no edits also skip CPU-worker dispatch. Nonempty corrections
  still refresh even when no new word was learned. Priority sorting releases the
  connection lock after reading its owned rows.
- Within each correction rule, ordered protection intervals are traversed once
  and dense replacements build the result once. Rules still apply sequentially.
  A single-hit path uses the existing allocation: sparse long-text measurements
  exposed a regression in the initial always-rebuild implementation, which was
  corrected before the final measurements below.

Both opt-in probes retain the previous algorithms as test-only references and
alternate old/new execution order over 30 samples per case in one executable.
Fixtures are constructed outside timing; all retention files contain synthetic
four-byte payloads and use a temporary database/directory. The retention timer
includes database purging and actual file deletion, but excludes fixture setup.
Correction samples average ten calls for short/dense/miss fixtures and one call
for each long sparse fixture. The sparse fixture prefix repeats `普通文本 `
10,000 times. Build configuration is Cargo's test profile (application opt-level
1, dependency opt-level 2), not a release package. Final runs had no concurrent
Cargo compilation. These are warm local synthetic measurements, with noisy
scheduling tails, not end-to-end application latency or production workloads.

| Synthetic case | Before median (ms) | Final median (ms) | Before p95 (ms) | Final p95 (ms) |
| --- | ---: | ---: | ---: | ---: |
| Retention, 1,000 recordings | 3126.671000 | 73.375000 | 4659.112000 | 148.240000 |
| Retention, 250 recordings | 184.695000 | 13.149500 | 292.891000 | 26.773000 |
| Correction, 2,000 repeated phrases | 10.127389 | 0.771756 | 22.389187 | 3.182900 |
| Correction, no match | 0.286197 | 0.290695 | 0.403320 | 0.334416 |
| Correction, short text | 0.001879 | 0.001927 | 0.002320 | 0.002712 |
| Correction, long text / one equal-length match | 1.426979 | 1.425979 | 1.462375 | 1.551000 |
| Correction, long text / one expanding match | 1.429375 | 1.429583 | 1.466000 | 1.545042 |
| Correction, long text / three sparse rules | 4.265313 | 4.229730 | 4.343292 | 4.353041 |

The 1,000-recording retention median decreased by 97.65%; dense correction
rewriting decreased by 92.38% in this run. Short, no-match and sparse cases show
no material median improvement; sub-microsecond short-text differences and
scheduling-sensitive tails do not establish an application regression or gain.
The earlier always-rebuild sparse multi-rule regression is absent from the final
single-hit implementation. No app-wide speedup is inferred from these probes.

Validation: `scripts/check.sh` passed with 494 tests passed and 13 ignored;
the two new opt-in benchmark tests also passed when explicitly run. Differential
checks include 900 fragment/rule combinations, actual automatic Unicode
lowercase expansion, correct-spelling protection, and a positive ordered cascade.
Storage regressions cover duplicate IDs/files, exact versus padded IDs, missing
or externally removed files, uppercase extensions and unrelated files/directories.
The existing Cargo `block 0.1.6` future-compatibility notice remains.

Reproduce with:

```sh
scripts/check.sh
cargo test --locked --lib benchmark_retention_audio_deletion -- --ignored --nocapture --test-threads=1
cargo test --locked --lib benchmark_known_correction_rewrite -- --ignored --nocapture --test-threads=1
```

Local logs and the numeric summary are in ignored `.e2e/performance-followup/`:
`bench-final.log` (retention), `correction-final.log` (final correction code),
`check-final.log` and `summary.json`. Earlier correction rows in `bench-final.log`
predate the single-hit fix and are not the final correction measurements.

The independent reviews found no blocking correctness issue or additional
immediately justified optimization after the requested validation. Stop this
iteration at that evidence boundary. Absolute project-wide optimality cannot be
established: native frame timing, shortcut-to-paste latency, capture allocation
and whole-process RSS remain unmeasured. Virtual lists, persistent file indexes,
additional caches and capture redesign need traces demonstrating a bottleneck
before their complexity is justified. No installed app, signing, permissions,
real microphone, clipboard or live server was changed or tested in this follow-up.
