# Native core verification

This records evidence for [#38](https://github.com/softmaxe/whisper/issues/38).
It is an intermediate milestone in [#35](https://github.com/softmaxe/whisper/issues/35),
not full feature, performance, or upgrade acceptance.

## Deterministic and package checks

The core implementation is `f8f48aa043e99e398fc263a6f8bfbc6fb9f6074b`.
Its native suite passed 21 test functions, including real AAC file decoding,
capture ownership, delayed acquisition, first-frame readiness, cancellation,
late success/failure, independent retry, and actual loopback HTTP requests.
Same-origin redirects preserve the multipart request and configured credentials;
cross-origin redirects are rejected. All fixtures use synthetic data.

The integrated revision `b96305711b3cd9ec61737844aca2732b2a403131` also includes
the Settings IPv4 compatibility correction. Its native suite passed 24 test
functions, and `npm run quality-check` passed 1,044 tests with no skips.
The core signed package, signing continuity test, and strict signature
verification passed using the original certificate.

## Packaged built-in microphone smoke

On September 20, 2026, the signed core bundle from `f8f48aa` was run on the
Apple M2 Pro target Mac with macOS 27. The bundle ran from its development
worktree with a disposable profile. The installed daily application and legacy
profile were left intact.

The test configured a loopback HTTP ASR fixture with no credential. The fixture
discarded each request body and returned a fixed synthetic transcript; it did
not transcribe, save, or forward captured speech.

- Two button-started recordings reached Listening, stopped, sent a multipart
  request, and displayed the fixture result in the native Home interface.
- One recording was cancelled after reaching Listening. It returned to idle
  and produced no additional ASR request.
- The native CoreAudio process-activity listener observed matching capture-start
  and capture-stop events for the test process during both a normal stop and
  a cancellation. No capture remained active between those checks.
- No microphone authorization prompt appeared during this run.
- After completion and cancellation, the disposable profile contained only its
  settings file, with no retained audio. The test app, fixture server and
  activity listener exited, and the profile and temporary logs were removed.

These observations establish basic built-in capture, packaged loopback HTTP,
result presentation and capture release. They do not establish production ASR
accuracy, preservation of spoken opening words, shortcut behavior, wireless
iPhone capture, long recording reliability, or permission retention after an
installation upgrade. The run was not a controlled latency or energy benchmark.

Continue with the [performance protocol](native-performance-baseline.md),
[acceptance graph](native-migration-status.md), and
[release smoke checks](../test/README.md#release-smoke-check).
