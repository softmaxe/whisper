# Testing

Tests follow the supported features in the root [README](../README.md). Remove tests for retired features instead of hiding them with `#[ignore]`.

## Run

```sh
scripts/check.sh
```

The script runs `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, and `cargo test --locked`. It is the same command the CI and Build workflows run. Use the Rust version pinned in [`rust-toolchain.toml`](../rust-toolchain.toml); rustup selects it automatically.

Unit tests live next to the code in `src/`. Tests marked `#[ignore]` need hardware or a live server; run them by name with `cargo test -- --ignored <name>`.

## Workflows

The CI workflow runs the checks for pull requests targeting `main` and pushes to `main`, without packaging the app, and lints the workflows with actionlint and the scripts with shellcheck. The Build workflow runs on manual dispatch or when called by Release for a `v*` tag on `main`. It runs the same checks, downloads the pinned ffmpeg, packages the macOS arm64 app, and verifies its bundle name, identifier, version, architecture, signature, archive, and checksum. Manual builds are signed ad hoc without release credentials and keep the package as an Actions artifact for seven days. Release builds require signatures matching `packaging/signing-certificate.pem`; a missing identity, a different certificate, or an ad-hoc signature fails the build. Release publishes the verified package to GitHub and updates the Homebrew tap. Manual builds do neither.

## Release smoke check

Automated checks do not establish microphone, Accessibility, or server compatibility on a user's Mac. Before calling a release ready, check the packaged app on macOS:

- Configure the intended ASR and cleanup servers, dictate with the global shortcut, and verify the pasted result and cleanup failure fallback.
- Switch microphones, exercise Auto selection with the laptop lid, and verify recording cancellation.
- Retry a History item, then check copy, delete, and retention settings.
- Upload a file and a batch with History both enabled and disabled. Check raw results remain copyable and do not increase dictation Insights.
- Verify dictionary and snippet behavior, the menu bar toggle, and installation through the Homebrew cask.

For changes to macOS signing, also test an upgrade between two release builds signed with the same saved certificate:

1. Build version A with `scripts/bundle-macos.sh --release`. Record its designated requirement with `codesign -d -r- dist/Whisper.app` and verify it with `scripts/sign-macos-release.sh verify dist/Whisper.app`.
2. Install version A at `/Applications/Whisper.app`. Grant microphone and Accessibility access, save a test server credential, and permit Keychain access when prompted. Dictate and verify automatic paste.
3. Build version B with a higher app version using the same certificate and private key. Repeat the signature checks and compare its designated requirement with version A. The requirement must stay the same even though the version and code hash differ. Check the helper requirements as well.
4. Quit version A and upgrade to version B through the Homebrew cask, preserving the installation path. Launch it, dictate, verify automatic paste, and access the saved credential. Record whether microphone, Accessibility, or Keychain authorization appears again and the macOS version used.

Do not reset TCC permissions, change system trust, or delete Keychain items during the upgrade test. Those actions would invalidate the test. Automated signature checks alone do not prove permission retention, and a source change does not update an already installed app. See [macOS signing](macos-signing.md) for identity setup.
