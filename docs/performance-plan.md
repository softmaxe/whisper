# Performance investigation and implementation plan

Implementation results and measurement limits are recorded in
[performance-results.md](performance-results.md). The investigation below
preserves the original findings and proposed sequence.

Investigated on 2026-11-07 against `811ecca`, with an independent Astra review.
This document proposes work; it does not claim that the application has been
optimized. No application launch, production-data inspection, permission changes,
or release packaging was performed.

## Main conclusion

Start by removing synchronous work from the UI thread at recording stop and
transcript delivery. Preserve ordering and audio quality initially. Then reduce
buffer duplication and repeated view computation. Startup work should be guided
by separate window-ready, shortcut-ready, and first-recording-ready measurements.

The application already shares its HTTP client and two-worker Tokio runtime,
enables SQLite WAL, lazily creates Insights and Dictionary views, and omits the
covered workspace when the Settings sheet is opaque. Keep these improvements.

## Evidence

### Measured audio conversion

Astra compiled an independent program with `rustc -O`, directly importing the
current `src/audio/wav.rs`. Synthetic mono input was constructed before timing;
each combination ran five times. The numbers below are medians for
`encode_wav_16k`, including resampling and WAV encoding.

| Input rate | 1-second recording | 10-second recording | 60-second recording |
| --- | ---: | ---: | ---: |
| 44.1 kHz | 4.73 ms | 36.31 ms | 209.58 ms |
| 48 kHz | 4.55 ms | 37.52 ms | 224.17 ms |

These are isolated function measurements on the investigation machine, not
end-to-end UI traces, percentile estimates, or measured optimization gains.
At 60 Hz the frame budget is 16.67 ms; at 120 Hz it is 8.33 ms. Even without
changing the algorithm, moving conversion off the UI thread removes a known
source of frame blocking. Kernel caching alone cannot eliminate the cost that
grows with recording length.

### Source-confirmed costs

Line references below describe the investigated revision.

| Path | Evidence | Proposed change |
| --- | --- | --- |
| Recording stop | `src/dictation.rs:463` encodes WAV synchronously; `src/audio/wav.rs:40` constructs a kernel and `:75` convolves output samples | Run conversion in a bounded background executor; preserve the algorithm first |
| Request preparation | `src/dictation.rs:440` reads priority from SQLite, clones dictionary state, and reads the cleanup key | Capture versioned settings/content snapshots; perform database and Keychain reads off the UI thread |
| Audio ownership | `src/dictation.rs:508` and `src/pipeline.rs:190` clone WAV bytes | Share immutable bytes for upload, storage, and retry; remove redundant owners |
| Delivery | `src/dictation.rs:596` saves before pasting; `src/history.rs:55` counts dictionary usage, writes analytics/history/audio, and rereads rows | Move persistence off the UI thread while preserving save-before-paste ordering in the first version |
| Dictionary usage | `src/db/dictionary.rs:294` holds the connection mutex during whole-dictionary text matching and repeatedly searches the count vector | Compute from a versioned dictionary snapshot outside the connection lock; update by stable ID and validate stale entries |
| Automatic learning | `src/app_state.rs:294` extracts corrections synchronously; `src/text/correction_learner.rs:245` uses an LCS matrix | Move segmentation/diff off the UI thread and bound actual allocation dimensions |
| History rendering | `src/ui/history_view.rs:1053` clones complete rows, groups dates, and builds all loaded rows on render | Cache row presentation by revision, locale and local day; share immutable text |
| Dictionary rendering | `src/ui/dictionary_view.rs:1589` clones collections, filters, normalizes, sorts and computes prompt statistics on render | Cache derived data by content/query/sort revision; evaluate virtualization for large collections |
| Insights refresh | `src/ui/insights_view.rs:391` subscribes to multiple events; `:411` queries synchronously | Merge invalidations, load off the UI thread, and refresh only when relevant data changes |
| File upload | `src/services/upload.rs` reads the complete upload into a byte vector | Stream multipart with known length and bounded buffering; preserve serial batch behavior |
| Settings opening | `src/ui/settings_modal.rs:630` synchronously obtains audio storage usage | Load usage asynchronously, coalesce scans, and show pending state |
| Audio lookup | `src/audio_storage.rs:116` enumerates and sorts files for each lookup | Maintain a recoverable ID-to-path index, or persist paths with compatibility for existing names |
| Settings writes | `src/app_state.rs:111` saves settings synchronously | Serialize background writes, coalesce rapid edits, retain atomic replace and flush on orderly exit |

SQLite uses one `Mutex<Connection>`. WAL does not prevent contention on this
in-process mutex. Moving only the retention sweep to a background task leaves
foreground callers exposed to that contention.

### Memory implications, not RSS measurements

- Capture stores mono device-rate `f32` samples and reserves 30 seconds at stream
  opening (`src/audio/capture.rs:161`). At 48 kHz that reservation represents
  5.76 MB of sample capacity; capacity is not equivalent to resident pages.
  A minute of samples has 11.52 MB of logical payload before vector overcapacity.
- A minute of 16 kHz PCM16 WAV contains approximately 1.92 MB of audio bytes.
  Deep clones add another payload of that size. Resampling also materializes an
  intermediate float output. These buffers have overlapping lifetimes, but their
  payload sizes must not be reported as measured process RSS or a guaranteed peak.
- The correction LCS already trims common prefixes/suffixes and caps `m*n` at
  4,000,000. A balanced case near that limit contains roughly 32 MB of `usize`
  cells on a 64-bit system, excluding row overhead and token objects. Bound
  `(m+1)*(n+1)` and total input units explicitly, including empty-sided edits.
- File uploads allocate a file-sized buffer. The existing batch loop is serial;
  this does not imply unbounded simultaneous file buffers or another full copy
  inside reqwest.
- History is capped at 50 rows. Cache/share its text first; prioritize Dictionary
  virtualization over a complex variable-height History list unless profiling
  shows that History layout dominates.

## Implementation sequence

### Batch A: measurements and responsive delivery

1. Add monotonic stage timings and stable operation IDs for shortcut receipt,
   first capture frame, stop, conversion, request start, ASR completion, cleanup
   completion, persistence completion, and paste completion. Record byte counts,
   queue delay, and lock wait/hold duration; never record content or credentials.
2. Run audio conversion on a bounded CPU executor. A plain Tokio async task still
   runs synchronous CPU work on a worker. Use a bounded blocking executor or a
   dedicated worker; do not increase runtime workers as the primary fix.
3. Move history/audio/statistics persistence off the UI thread. Await it before
   paste initially, preserving existing behavior. Keep user-visible processing
   feedback active while work runs. Serialize mutations that can race.
4. Move dictionary matching outside the connection lock. Check dictionary
   identity/version before applying counts; do not credit a newly edited word
   using a stale match. Use an ID-to-count mapping instead of repeated scans.
5. Move correction extraction off the UI thread, guard actual allocation, and
   discard stale results after edits, deletion, cancellation, or shutdown.
6. Share immutable WAV bytes across pipeline, persistence, and retry. Release
   device-rate samples and intermediate conversion buffers promptly. Do not
   replace one deep clone with another hidden conversion at the HTTP boundary.

Treat these as separate logical commits with focused regression checks.

### Batch B: rendering, uploads, and accumulated data

- Separate settings, recording phase, history, dictionary content, dictionary
  usage, and analytics revisions. Reduce broad entity observation where it
  causes unrelated views to recompute.
- Cache normalized Dictionary search keys, sorted visible IDs, prompt counts,
  and History date groups. Invalidate for locale changes and local midnight as
  well as content changes. Content indexes and usage ranking need different
  invalidation rules; priority freshness also changes with wall-clock time.
- Virtualize large Dictionary collections while preserving focus, keyboard
  navigation, inline editing, menus, and scroll restoration.
- Coalesce Insights loads and ignore out-of-date background results. SQL already
  groups analytics by day; add daily aggregate tables only if measured query
  time justifies migration and consistency complexity.
- Stream file uploads with a known multipart length. Own temporary conversion
  files until the body finishes or cancellation releases it. Reopen from offset
  zero for retries and preserve compatibility with self-hosted servers.
- Load Settings storage usage asynchronously and avoid repeated directory scans
  for audio lookup. Make any path index repairable after external file deletion.
- Profile error/recovery/toast presentation: `src/ui/pill.rs:2032` requests frames
  whenever a card is present. Replace unnecessary continuous redraw with
  deadline-based updates only after checking countdowns and pointer hit testing.
  The 30 ms pointer polling loop already stops when the pill hides.

### Batch C: startup and algorithm improvements

- Instrument the existing startup sequence: instance lock, settings, database
  open/migration, toolkit initialization, fonts, state hydration, native helpers,
  tray, first panel frame, and shortcut registration. Compare normal and minimized
  launch independently.
- Keep startup migrations correct. Split essential dictation readiness from
  History, Insights, storage totals, and retention maintenance. A shortcut must
  not silently run against an empty dictionary while hydration is pending.
- Schedule maintenance after essential readiness, with limited chunks and lower
  priority. A low-priority sweep must not monopolize the shared database mutex.
- Measure font registration before deferring it. Do not move an expensive font
  load onto the first shortcut or accept missing glyphs just to improve a launch
  stopwatch. Insights and Dictionary view creation are already lazy.
- Cache resampler kernels by input/output rate with a bounded cache. Then compare
  a 48-to-16 kHz polyphase implementation and direct PCM16 output. Preserve DC
  gain, passband, alias suppression, sample count, and edge behavior. Streaming
  conversion must retain phase and filter history between blocks.
- Consider a preallocated capture ring and streaming consumer only if callback
  allocation/lock traces or long-recording peaks justify it. Never silently drop
  audio when a consumer falls behind; specify backpressure/overflow behavior.
- Measure Keychain access before choosing credential caching. Keep Keychain as
  the source of truth and define invalidation on setting changes, deletion,
  external updates, and authentication failure. Do not persist a plaintext cache.

## Correctness gates

Background work must preserve generation checks and provide bounded queueing.
Cancellation prevents late paste and UI state changes; a started blocking task
also needs cooperative cancellation if it must stop computing. Quit behavior
must define which accepted writes are drained and which transient tasks cancel.

Validate rapid start/stop/cancel/retry sequences, deletion while work is pending,
retention changes, no duplicate analytics or dictionary counts, and no stale row
resurrection. Preserve retention-off behavior and cleanup-failure fallback.
Moving paste before persistence requires a separate decision about crash loss,
retry idempotency, and error reporting; it is not part of Batch A.

Keep microphone, helper, Accessibility, and Keychain identities unchanged.
This investigation does not propose signing or permission changes.

## Baseline and acceptance

Use synthetic fixture data and consistent release settings. Run at least 30
repetitions for latency distributions; report p50, p95, sample count, hardware,
power state, build revision, and warm/cold conditions. The five-run microbenchmark
above is initial evidence, not sufficient for p95 claims.

Cover 1/10/60-second audio at 16/44.1/48 kHz, empty and large dictionaries, long
correction inputs including one-sided edits, 50 long History rows, accumulated
audio directories, and large uploads to a controlled local server.

Record frame intervals at the display's actual refresh rate, stop-to-request and
response-to-paste stage latency, time to first panel/shortcut/capture, peak memory
and its post-operation recovery, idle CPU/wakeups, and executor queue/lock delays.
Measure hidden idle, visible panel, recording, processing, error card, and Settings
separately. Account for helper processes separately from the main app.

The desired outcome is fewer application-caused missed frames, bounded memory
under sustained operations, and improved startup/readiness distributions. Set
numerical improvement thresholds after the application baseline exists; do not
promise a percentage startup or RSS reduction from static inspection.

Run `scripts/check.sh` for implemented Rust changes and the relevant interaction
checks in `docs/testing.md`. Audio algorithm changes additionally require
signal-quality tests. Actual microphone/paste behavior requires a macOS smoke
check and must be reported separately from automated checks.

## API references

- [Tokio spawn_blocking](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html):
  CPU work needs concurrency limits; started blocking work cannot simply be
  aborted like a suspended async future.
- [reqwest multipart Part](https://docs.rs/reqwest/latest/reqwest/multipart/struct.Part.html):
  `stream_with_length` supports a stream whose byte length is known beforehand.
