# Whisper tests

The regression suite follows the supported features in the root README. Keep inherited tests when they protect a feature used by this fork. Remove suites for retired features instead of hiding them with skips or a separate test command.

## Run

```sh
pnpm install --frozen-lockfile
pnpm test
pnpm run quality-check
```

Use the Node.js and pnpm versions pinned in [`mise.toml`](../mise.toml). `pnpm test` runs the suite inside Electron's Node runtime, matching the SQLite binding rebuilt for Electron by the install's `postinstall` script. Database tests must run; an unavailable native binding fails the command. `quality-check` adds lint, TypeScript, and translation checks and is the same command used by the CI and Build workflows.

On macOS, run `pnpm run test:signing` with the original [release signing credentials](../docs/macos-signing.md#build-locally) to check identity continuity. The test signs two app versions and native helpers with different code, compares their designated requirements, and verifies that each version satisfies the other's requirement. It also rejects a helper matching the main app's identity. Release runs this test automatically before packaging. It does not install the app or request permissions and cannot replace the upgrade smoke check below.

## Coverage

| Product behavior            | Regression coverage                                                                                                                                     |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Dictation                   | Recording start, stop and cancellation; microphone selection and recovery; MacBook lid changes; shortcuts; recording pill; clipboard and paste outcomes |
| Self-hosted processing      | ASR endpoint configuration, request authentication, file transport, cleanup prompts, request parameters, timeouts and failure recovery                  |
| Local History               | Transcript and audio retention, discarded recordings, retry, and database persistence                                                                   |
| Dictionary and Snippets     | Dictionary persistence and import, correction learning, text matching, and snippet expansion                                                            |
| File uploads                | Supported formats, batch state, cancellation, raw transcript persistence, and copyable results when History is disabled                                 |
| Local Insights and settings | Local event accounting, date grouping, microphone settings, menu bar visibility, permissions, and translations                                          |

Hosted provider APIs, OpenWhispr Cloud, accounts, sync, enterprise policies, meetings, Notes, Assistant, bundled model servers, Linux, and Windows are outside this suite. Shared test doubles may still name upstream interfaces imported by the retained implementation; those names do not add product requirements.

Keep tests that exercise supported behavior, failure recovery, or boundaries such as renderer-to-native window sizing. When a component test already covers a helper's cases, keep those cases in one place. Avoid tests that only pin CSS classes, decorative animation details, or React's attribute forwarding. Review those appearance changes in the running app.

The CI workflow runs checks for pull requests targeting `main` and pushes to `main`, without packaging or uploading the app. The Build workflow runs on manual dispatch or when called by Release for a `v*` tag on `main`. It runs the same checks, packages the macOS ARM64 app, and verifies its bundle name, version, architecture, signature, archive, and checksum. Manual builds use ad-hoc signing without release credentials and retain the package as an Actions artifact for seven days. Release builds require signatures matching `resources/mac/signing-certificate.pem`; a missing identity, different certificate, or ad-hoc signature must fail the build. Release publishes the verified package to GitHub and updates the Homebrew tap. Manual builds do neither.

## Recording startup measurements

For recording startup measurements, use the
[baseline procedure](../docs/recording-startup-baseline.md). Its renderer tests
drive the real Dictation Hook and AudioManager with controlled device delivery,
IPC, visual frames, and time. Hardware latency samples remain a separate check.
The [combined verification report](../docs/recording-startup-verification.md)
maps the integrated lifecycle coverage to #28 and records outstanding hardware
checks.

The [pill startup animation regression](components/pillStartupAnimation.test.js)
builds the production VoicePill and styles, then renders them in an isolated
offscreen Electron window. It blocks renderer JavaScript for 1.5 seconds and
checks that changing frames continue during the block, including first
appearance, restart during the finish fade, and a new appearance after idle.
It also checks the frozen finish, handoff to measured audio levels, and animation
cleanup. It opens no microphone, uses a temporary profile, and removes its build
and profile after the test. Run it separately with:

```sh
ELECTRON_RUN_AS_NODE=1 node_modules/.bin/electron --import tsx --test test/components/pillStartupAnimation.test.js
```

This checks animation continuity under renderer contention, not physical iPhone
connection latency or macOS permission behavior.

## Automatic paste

The [Automatic paste integration suite](helpers/automaticPasteIntegration.test.js)
submits requests through AutomaticPaste's entry interface with the real
ClipboardManager and TextEditMonitor. Its [shared scenarios](lib/automaticPasteScenarios.js)
cover target confirmation, spacing, permissions, native failure and retry,
clipboard restoration and queue timing, and correction monitoring. The
[fixture](lib/automaticPasteFixture.js) controls OS clipboard access, native
process responses, executable discovery, windows and time. It uses production
IPC dependency wiring while isolating unrelated startup services. It also holds
the real WindowManager, whose current-Dictation record binds the Target app at
the Dictation hotkey press, so paste requests that name a Dictation reach that
app even after later target captures. A dedicated
failure-isolation case injects a synchronous monitoring-start exception. Small
[IPC adapter tests](helpers/ipcPasteOutcome.test.js) cover argument forwarding,
sender context, serializable outcomes and error propagation.

These tests establish command completion under controlled OS responses. They do
not establish observed text insertion in a live macOS app. Existing native target,
keyboard-layout and renderer recovery tests remain separate regression coverage.

## Release smoke check

Automated checks do not establish microphone, Accessibility, or server compatibility on a user's Mac. Before calling a release ready, check the packaged app on macOS:

- Configure the intended ASR and cleanup servers, dictate with the global shortcut, and verify the pasted result and cleanup failure fallback.
- Switch microphones, exercise Auto selection with the laptop lid, and verify recording cancellation.
- Retry a History item, then check copy, delete, and retention settings.
- Upload a file and a batch with History both enabled and disabled. Check raw results remain copyable and do not increase dictation Insights.
- Verify dictionary and snippet behavior, the menu bar toggle, and installation through the Homebrew cask.

For changes to macOS signing, also test an upgrade between two release builds signed with the same saved certificate:

1. Build version A with `pnpm run pack:release`. Record its designated requirement with `codesign -d -r- /path/to/version-a/Whisper.app` and verify its signature with `codesign --verify --deep --strict /path/to/version-a/Whisper.app`.
2. Install version A at `/Applications/Whisper.app`. Grant microphone and Accessibility access, save a test server credential, and permit Keychain access when prompted. Dictate and verify automatic paste.
3. Build version B with a higher app version using the same certificate and private key. Repeat the signature checks and compare its designated requirement with version A. The requirement must remain the same even though the version and code hash differ. Check the helper app requirements as well.
4. Quit version A and upgrade to version B through the Homebrew cask, preserving the installation path. Launch it, dictate, verify automatic paste, and access the saved credential. Record whether microphone, Accessibility, or Keychain authorization appears again and the macOS version used.
5. Separately check the transition from an old ad-hoc release to the first release with the persistent certificate. One more authorization may be needed during that transition. It does not establish permission retention between two releases with the persistent identity.

Do not reset TCC permissions, change system trust, or delete Keychain items during the upgrade test. Those actions would invalidate the test. Automated signature checks alone do not prove permission retention, and a source change does not update an already installed app. See [macOS signing](../docs/macos-signing.md) for identity setup.
